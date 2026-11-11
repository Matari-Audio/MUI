//! The quad pass against the classic-Vello `GpuRenderer` on the same paint
//! list, one the quad pass draws entirely: about 1250 entries of rounded
//! rects, strokes, drop and inset shadows, gradients and clips at 1280x800.
//!
//!     cargo run -p mui-vello --profile perf --features gpu-quad,gpu-effects --example quad_bench
//!
//! Per frame: `cpu` is the `render` call (build, upload, encode, submit);
//! `gpu` is GPU time from timestamps written in encoders submitted just
//! before and just after it; `wait` is wall time from the `render` return
//! until the device is idle, which on this machine is dominated by a fixed
//! submit-to-idle latency (see the empty-list row). The GPU span includes
//! any time the GPU sat idle while `render` ran on the CPU, so it is an upper
//! bound; `GpuRenderer::render` itself waits on the device (Vello reads back
//! its allocation counts), so its `cpu` column includes GPU time. Medians of 60
//! frames after 10 warm-ups, for a static frame and for one where a single
//! fill changes colour every frame. `GpuRenderer` retains its output, so a
//! static frame costs it no render at all; the quad pass redraws every frame.
//!
//! The second table is the same cards with five labels each (480 glyph
//! runs, bench-editor density), for: a cold frame (a renderer's first, its
//! glyph atlas empty; median of 7 fresh renderers), a static frame, a frame
//! where one label's text changes, and one where everything scrolls by a
//! fractional offset (0.37 px a frame on both axes).

use std::time::Instant;

use mui_scene::prelude::*;
use mui_scene::{Layer, Paint, ResolvedScene};
use mui_vello::effects::{Budget, GpuRenderer, GpuTimer};
use mui_vello::kurbo::Affine;
use mui_vello::quad::{QuadRenderer, classify};

const W: u32 = 1280;
const H: u32 = 800;
const WARM: usize = 10;
const N: usize = 60;

fn scene() -> ResolvedScene {
    cards(None)
}

/// The cards; with `Some(n)`, five labels on each, card 0's first reading `n`.
fn cards(labels: Option<usize>) -> ResolvedScene {
    let (a, b) = (Color::srgb(0.25, 0.45, 0.9), Color::srgb(0.85, 0.3, 0.5));
    let cards: Vec<El> = (0..96)
        .map(|i| {
            let fill: Fill = match i % 4 {
                0 => Gradient::linear(90., [(0., a), (1., b)]).into(),
                1 => Gradient::radial((0.5, 0.3), 0.8, [(0., b), (1., a)]).into(),
                _ => Color::srgb(0.2, 0.22, 0.26).into(),
            };
            let chip = |c: Color| block(18., 10.).radius(5.).fill(c);
            let mut kids = vec![
                row![chip(a), chip(b), chip(a)].gap(4.),
                block(100., 8.)
                    .radius(4.)
                    .fill(Color::srgb(0.35, 0.37, 0.4)),
                stack![block(160., 12.).radius(6.).fill(b).at(-20., 0.)]
                    .size(80., 12.)
                    .radius(0.)
                    .clip(),
            ];
            if let Some(n) = labels {
                let ink = Color::srgb(0.9, 0.92, 0.95);
                let first = if i == 0 {
                    format!("Cutoff {n} Hz")
                } else {
                    format!("Cutoff {i} Hz")
                };
                kids.push(text(first).text_size(11.).fill(ink));
                kids.push(text(format!("Osc {i} saw")).text_size(9.).fill(ink));
                kids.push(text("Resonance 0.42").text_size(10.).fill(ink));
                kids.push(
                    text(format!("{:.1} dB", i as f32 * 0.7))
                        .text_size(12.)
                        .fill(ink),
                );
                kids.push(text("Env").text_size(14.).fill(ink));
            }
            col(kids)
                .gap(6.)
                .pad(8.)
                .radius(10.)
                .fill(fill)
                .stroke(Color::srgba(1., 1., 1., 0.3))
                .stroke_width(1.)
                .shadow(Shadow::soft(6.))
                .shadow(Shadow::inset(3.))
                .id(format!("card{i}"))
        })
        .collect();
    let root = grid(12, cards)
        .gap(8.)
        .pad(8.)
        .size(f64::from(W), f64::from(H))
        .radius(0.)
        .fill(Color::srgb(0.06, 0.07, 0.09));
    let mut spec = SceneSpec::new(root).offered(Size::new(f64::from(W), f64::from(H)));
    // One font for every frame, as a host keeps it: a fresh `Font` is a new
    // face to every glyph cache.
    static HACK: std::sync::LazyLock<mui_scene::Font> = std::sync::LazyLock::new(|| {
        mui_scene::Font::new(epaint_default_fonts::HACK_REGULAR).expect("font")
    });
    spec.font = Some(HACK.clone());
    mui_scene::resolve(&spec).expect("scene resolves")
}

fn median(v: &mut [f64]) -> f64 {
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1e3
}

/// Medians of (cpu, gpu, wait) ms over `N` frames of `frame(i)`.
fn time(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    timer: &mut GpuTimer,
    frame: impl FnMut(usize),
) -> [f64; 3] {
    time_n(device, queue, timer, WARM, N, frame)
}

fn time_n(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    timer: &mut GpuTimer,
    warm: usize,
    n: usize,
    mut frame: impl FnMut(usize),
) -> [f64; 3] {
    let (mut cpu, mut gpu, mut wait) = (vec![], vec![], vec![]);
    let mut samples = Vec::new();
    for i in 0..warm + n {
        let mut before = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        let ticket = timer.begin(&mut before).expect("a free timestamp slot");
        queue.submit([before.finish()]);
        let start = Instant::now();
        frame(i);
        let c = ms(start);
        let mut after = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        timer.finish(&mut after, ticket);
        queue.submit([after.finish()]);
        timer.submitted(ticket);
        let t = Instant::now();
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("poll");
        let w = ms(t);
        samples.clear();
        timer.collect(device, &mut samples);
        if i >= warm {
            cpu.push(c);
            wait.push(w);
            gpu.push(samples.first().expect("a GPU timing").milliseconds);
        }
    }
    [median(&mut cpu), median(&mut gpu), median(&mut wait)]
}

fn main() {
    let base = scene();
    let coverage = classify(&base.paint);
    assert!(
        coverage.complete(),
        "bench scene must be all supported: {coverage:?}"
    );
    let count = |l: fn(&Layer) -> bool| base.paint.iter().filter(|p| l(&p.layer)).count();
    println!(
        "scene: {} entries ({} fills, {} strokes, {} shadows, {} clips, {} gradients)",
        base.paint.len(),
        count(|l| matches!(l, Layer::Fill)),
        count(|l| matches!(l, Layer::Stroke)),
        count(|l| matches!(l, Layer::Shadow(_))),
        count(|l| matches!(l, Layer::Clip)),
        base.paint
            .iter()
            .filter(|p| matches!(p.paint, Paint::Gradient { .. }))
            .count(),
    );
    // The one entry that changes: the last solid card fill.
    let moving = base
        .paint
        .iter()
        .rposition(|p| p.layer == Layer::Fill && matches!(p.paint, Paint::Solid(_)))
        .expect("a solid fill");
    let changed = |i: usize| {
        let mut s = base.clone();
        s.paint[moving].paint = Paint::Solid(Color::srgb((i % 50) as f32 / 50., 0.5, 0.5));
        s
    };
    let frames: Vec<ResolvedScene> = (0..WARM + N).map(changed).collect();

    let (device, queue, info) = pollster::block_on(async {
        let adapter =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env())
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::HighPerformance,
                    ..Default::default()
                })
                .await
                .expect("adapter");
        let (d, q) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                required_features: GpuTimer::required_features(),
                required_limits: adapter.limits(),
                ..Default::default()
            })
            .await
            .expect("device");
        (d, q, adapter.get_info())
    });
    println!("adapter: {} ({:?})", info.name, info.backend);
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("quad bench"),
        size: wgpu::Extent3d {
            width: W,
            height: H,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = target.create_view(&wgpu::TextureViewDescriptor::default());

    let mut timer = GpuTimer::new(&device, &queue).expect("timestamp queries");
    let mut quad = QuadRenderer::new(&device, format);
    let mut q = |s: &ResolvedScene| {
        quad.render(&device, &queue, &s.paint, Affine::IDENTITY, &view, [W, H])
            .expect("quad render");
    };
    // The floor: a clear and a submit, nothing drawn.
    let mut empty = base.clone();
    empty.paint.clear();
    let quad_empty = time(&device, &queue, &mut timer, |_| q(&empty));
    let quad_static = time(&device, &queue, &mut timer, |_| q(&base));
    let quad_change = time(&device, &queue, &mut timer, |i| q(&frames[i]));

    let mut vello = pollster::block_on(GpuRenderer::new(
        &device,
        &queue,
        format,
        [W, H],
        Budget::default(),
    ))
    .expect("GpuRenderer");
    let (mut renders_static, mut renders_change) = (0, 0);
    let vello_static = time(&device, &queue, &mut timer, |_| {
        renders_static += vello
            .render(&base, Affine::IDENTITY, &view)
            .expect("vello render")
            .renders;
    });
    let vello_change = time(&device, &queue, &mut timer, |i| {
        renders_change += vello
            .render(&frames[i], Affine::IDENTITY, &view)
            .expect("vello render")
            .renders;
    });

    let load = std::fs::read_to_string("/proc/loadavg").unwrap_or_default();
    println!("load average: {}", load.trim());
    println!(
        "GpuRenderer renders over {} frames: static {renders_static}, one change {renders_change}",
        WARM + N
    );
    println!(
        "\n{:<14} {:<12} {:>9} {:>9} {:>9}",
        "renderer", "frame", "cpu ms", "gpu ms", "wait ms"
    );
    for (name, frame, t) in [
        ("QuadRenderer", "empty list", quad_empty),
        ("QuadRenderer", "static", quad_static),
        ("QuadRenderer", "one change", quad_change),
        ("GpuRenderer", "static", vello_static),
        ("GpuRenderer", "one change", vello_change),
    ] {
        let [cpu, gpu, wait] = t;
        println!("{name:<14} {frame:<12} {cpu:9.3} {gpu:9.3} {wait:9.3}");
    }
    labelled(&device, &queue, &mut timer, &view, format, &mut vello);
}

/// The labelled cards through both renderers.
fn labelled(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    timer: &mut GpuTimer,
    view: &wgpu::TextureView,
    format: wgpu::TextureFormat,
    vello: &mut GpuRenderer,
) {
    let base = cards(Some(0));
    let runs = base.paint.iter().filter(|p| p.text.is_some()).count();
    let glyphs: usize = base
        .paint
        .iter()
        .filter_map(|p| p.text.as_ref())
        .map(|t| t.glyphs.len())
        .sum();
    assert!(
        classify(&base.paint).complete(),
        "labelled cards must be all supported"
    );
    let labels: Vec<ResolvedScene> = (0..WARM + N).map(|i| cards(Some(1000 + i * 37))).collect();
    let scroll = |i: usize| Affine::translate((0.37 * i as f64, 0.37 * i as f64));
    let mut quad = QuadRenderer::new(device, format);
    let q = |quad: &mut QuadRenderer, s: &ResolvedScene, xf: Affine| {
        quad.render(device, queue, &s.paint, xf, view, [W, H])
            .expect("quad render");
    };
    let mut fresh: Vec<QuadRenderer> = (0..7)
        .map(|_| {
            let mut r = QuadRenderer::new(device, format);
            q(&mut r, &cards(None), Affine::IDENTITY);
            r
        })
        .collect();
    let quad_cold = time_n(device, queue, timer, 0, 7, |i| {
        q(&mut fresh[i], &base, Affine::IDENTITY);
    });
    let cold = fresh[0].frame();
    let quad_static = time(device, queue, timer, |_| {
        q(&mut quad, &base, Affine::IDENTITY)
    });
    let steady = quad.frame();
    let quad_label = time(device, queue, timer, |i| {
        q(&mut quad, &labels[i], Affine::IDENTITY)
    });
    let quad_scroll = time(device, queue, timer, |i| q(&mut quad, &base, scroll(i)));

    let v = |r: &mut GpuRenderer, s: &ResolvedScene, xf: Affine| {
        r.render(s, xf, view).expect("vello render");
    };
    let mut fresh: Vec<GpuRenderer> = (0..7)
        .map(|_| {
            pollster::block_on(GpuRenderer::new(
                device,
                queue,
                format,
                [W, H],
                Budget::default(),
            ))
            .expect("GpuRenderer")
        })
        .collect();
    let vello_cold = time_n(device, queue, timer, 0, 7, |i| {
        v(&mut fresh[i], &base, Affine::IDENTITY)
    });
    let vello_static = time(device, queue, timer, |_| v(vello, &base, Affine::IDENTITY));
    let vello_label = time(device, queue, timer, |i| {
        v(vello, &labels[i], Affine::IDENTITY)
    });
    let vello_scroll = time(device, queue, timer, |i| v(vello, &base, scroll(i)));

    let load = std::fs::read_to_string("/proc/loadavg").unwrap_or_default();
    println!(
        "\nlabelled cards: {} entries, {runs} glyph runs, {glyphs} glyphs; load average: {}",
        base.paint.len(),
        load.trim()
    );
    println!(
        "quad cold frame: {} cells rasterised, {} bytes uploaded; steady: {:?}",
        cold.rasterised, cold.uploaded, steady
    );
    println!(
        "{:<14} {:<12} {:>9} {:>9} {:>9}",
        "renderer", "frame", "cpu ms", "gpu ms", "wait ms"
    );
    for (name, frame, t) in [
        ("QuadRenderer", "cold", quad_cold),
        ("QuadRenderer", "static", quad_static),
        ("QuadRenderer", "label change", quad_label),
        ("QuadRenderer", "scroll", quad_scroll),
        ("GpuRenderer", "cold", vello_cold),
        ("GpuRenderer", "static", vello_static),
        ("GpuRenderer", "label change", vello_label),
        ("GpuRenderer", "scroll", vello_scroll),
    ] {
        let [cpu, gpu, wait] = t;
        println!("{name:<14} {frame:<12} {cpu:9.3} {gpu:9.3} {wait:9.3}");
    }
}
