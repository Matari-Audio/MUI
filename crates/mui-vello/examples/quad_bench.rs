//! The quad pass against classic Vello's `VelloRenderer` on the same paint
//! lists, ones the quad pass draws entirely, at 1280x800. The headline: the
//! synth-editor scene the other benches share (`bench_scene`: 40 knobs, a
//! welded body clipping everything, labels, curves), cold, static, with
//! one knob turning and scrolling. Then a dense case (a 1,000-point
//! polyline redrawn every frame over 300 static cards), 96 cards of
//! rounded rects, strokes, drop and inset shadows, gradients and clips
//! (about 1250 entries), and the same cards with five labels each (576
//! glyph runs).
//!
//! It also writes the static editor frame from both renderers to
//! `quad-bench-editor-{quad,vello}.png` in the temp directory, and the same
//! frame without the menu float that covers it to
//! `quad-bench-editor-under-menu-{quad,vello}.png`. Last, a modal fading in
//! over the editor through a backdrop blur, every frame a new opacity, with
//! its first frame as `quad-bench-editor-modal-{quad,vello}.png`.
//!
//!     cargo run -p mui-vello --profile perf --features vello-classic --example quad_bench
//!
//! Per frame, medians of `N` frames after `WARM` warm-ups:
//! - `cpu`: the `render` call (build, upload, encode, submit).
//! - `span`: GPU time between timestamps written in encoders submitted just
//!   before and just after `render`. It includes any time the GPU sat idle
//!   while `render` ran on the CPU, so it is an upper bound on GPU work.
//!   `VelloRenderer::render` waits on the device itself (Vello reads back its
//!   allocation counts), so its `cpu` column includes GPU time too.
//! - `pass`: the quad render pass's own begin/end timestamps: its true GPU
//!   time, upload copies excluded. Zero when `render` drew no pass.
//! - `wait`: wall time from the `render` return until the device is idle.
//!
//! The renderers alternate frame by frame within each scenario, each on its
//! own target, so machine load hits both alike. `VelloRenderer` skips a frame
//! whose list, transform and target are unchanged. On the editor a third
//! renderer runs too: `GpuRenderer`, the quad pass as hosts get it (behind
//! its retained frame and present pass), to show the wiring costs nothing.

use std::time::Instant;

use mui_scene::prelude::*;
use mui_scene::{Layer, Paint, ResolvedScene};

mod bench_scene;
#[expect(dead_code, reason = "shared by several examples; each calls a subset")]
mod gpu_support;
use mui_vello::effects::{Budget, GpuRenderer, GpuTimer, VelloRenderer};
use mui_vello::kurbo::Affine;
use mui_vello::quad::{QuadRenderer, classify};

const W: u32 = 1280;
const H: u32 = 800;
const WARM: usize = 10;
const N: usize = 80;

// One font for every frame, as a host keeps it: a fresh `Font` is a new face
// to every glyph cache.
static HACK: std::sync::LazyLock<mui_scene::Font> = std::sync::LazyLock::new(|| {
    mui_scene::Font::new(epaint_default_fonts::HACK_REGULAR).expect("font")
});

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

/// The device and the query set the quad pass writes its timestamps to.
struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    queries: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    read: wgpu::Buffer,
}

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

impl Gpu {
    fn target(&self) -> wgpu::TextureView {
        self.device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("quad bench"),
                size: wgpu::Extent3d {
                    width: W,
                    height: H,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            })
            .create_view(&wgpu::TextureViewDescriptor::default())
    }

    fn quad(&self) -> QuadRenderer {
        let mut quad = QuadRenderer::new(&self.device, FORMAT);
        quad.timestamps = Some(self.queries.clone());
        quad
    }

    fn wired(&self) -> GpuRenderer {
        pollster::block_on(GpuRenderer::new(
            &self.device,
            &self.queue,
            FORMAT,
            [W, H],
            Budget::default(),
        ))
        .expect("GpuRenderer")
    }

    fn vello(&self) -> VelloRenderer {
        pollster::block_on(VelloRenderer::new(
            &self.device,
            &self.queue,
            FORMAT,
            [W, H],
            Budget::default(),
        ))
        .expect("VelloRenderer")
    }

    /// The quad pass's last begin-to-end time, in ms; 0 when it drew none.
    fn pass_ms(&self) -> f64 {
        let slice = self.read.slice(..);
        slice.map_async(wgpu::MapMode::Read, |r| r.expect("map timestamps"));
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("poll");
        let ticks = {
            let bytes = slice.get_mapped_range().expect("mapped");
            let t = |i: usize| u64::from_le_bytes(bytes[i..i + 8].try_into().expect("8 bytes"));
            t(8).saturating_sub(t(0))
        };
        self.read.unmap();
        ticks as f64 * f64::from(self.queue.get_timestamp_period()) / 1e6
    }

    /// The quad pass's `render`; whether it drew a pass.
    fn quad_frame(
        &self,
        quad: &mut QuadRenderer,
        s: &ResolvedScene,
        xf: Affine,
        view: &wgpu::TextureView,
    ) -> bool {
        quad.render(&self.device, &self.queue, &s.paint, xf, view, [W, H])
            .expect("quad render");
        quad.frame().drawn
    }
}

fn vello_frame(
    r: &mut VelloRenderer,
    s: &ResolvedScene,
    xf: Affine,
    view: &wgpu::TextureView,
) -> bool {
    r.render(s, xf, view).expect("vello render");
    true
}

fn wired_frame(
    r: &mut GpuRenderer,
    s: &ResolvedScene,
    xf: Affine,
    view: &wgpu::TextureView,
) -> bool {
    r.render(s, xf, view).expect("wired render");
    r.quad_frame().drawn
}

/// A renderer's frame `i` of a scenario; whether it drew a pass. `quad`
/// marks the quad pass, whose own timestamps are read back.
struct Job<'a> {
    name: &'static str,
    quad: bool,
    run: Box<dyn FnMut(usize) -> bool + 'a>,
}

type Row = (&'static str, &'static str, [f64; 4]);

/// Medians of (cpu, span, pass, wait) ms per job over `n` frames after
/// `warm`, the jobs alternating within each frame, as rows named `frame`.
fn rounds(
    gpu: &Gpu,
    timer: &mut GpuTimer,
    frame: &'static str,
    mut jobs: Vec<Job<'_>>,
    warm: usize,
    n: usize,
    rows: &mut Vec<Row>,
) {
    let mut samples: Vec<[Vec<f64>; 4]> = jobs.iter().map(|_| Default::default()).collect();
    let mut spans = Vec::new();
    let encoder = || {
        gpu.device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default())
    };
    for i in 0..warm + n {
        for (job, out) in jobs.iter_mut().zip(&mut samples) {
            let mut before = encoder();
            let ticket = timer.begin(&mut before).expect("a free timestamp slot");
            gpu.queue.submit([before.finish()]);
            let start = Instant::now();
            let drew = (job.run)(i);
            let cpu = ms(start);
            let mut after = encoder();
            timer.finish(&mut after, ticket);
            if job.quad {
                if drew {
                    after.resolve_query_set(&gpu.queries, 0..2, &gpu.resolve, 0);
                } else {
                    after.clear_buffer(&gpu.resolve, 0, None);
                }
                after.copy_buffer_to_buffer(&gpu.resolve, 0, &gpu.read, 0, 16);
            }
            gpu.queue.submit([after.finish()]);
            timer.submitted(ticket);
            let t = Instant::now();
            gpu.device
                .poll(wgpu::PollType::wait_indefinitely())
                .expect("poll");
            let wait = ms(t);
            spans.clear();
            timer.collect(&gpu.device, &mut spans);
            let pass = if job.quad { gpu.pass_ms() } else { f64::NAN };
            if i >= warm {
                let span = spans.first().expect("a GPU timing").milliseconds;
                for (v, x) in out.iter_mut().zip([cpu, span, pass, wait]) {
                    v.push(x);
                }
            }
        }
    }
    for (job, s) in jobs.iter().zip(&mut samples) {
        rows.push((job.name, frame, s.each_mut().map(|v| median(v))));
    }
}

/// The quad pass and `VelloRenderer`, alternating.
fn both<'a>(
    quad: impl FnMut(usize) -> bool + 'a,
    vello: impl FnMut(usize) -> bool + 'a,
) -> Vec<Job<'a>> {
    vec![
        Job {
            name: "QuadRenderer",
            quad: true,
            run: Box::new(quad),
        },
        Job {
            name: "VelloRenderer",
            quad: false,
            run: Box::new(vello),
        },
    ]
}

/// [`both`] with the wired `GpuRenderer` between them.
fn three<'a>(
    quad: impl FnMut(usize) -> bool + 'a,
    wired: impl FnMut(usize) -> bool + 'a,
    vello: impl FnMut(usize) -> bool + 'a,
) -> Vec<Job<'a>> {
    let mut jobs = both(quad, vello);
    jobs.insert(
        1,
        Job {
            name: "GpuRenderer",
            quad: false,
            run: Box::new(wired),
        },
    );
    jobs
}

fn table(title: &str, rows: &[Row]) {
    let load = std::fs::read_to_string("/proc/loadavg").unwrap_or_default();
    println!("\n{title}; load average: {}", load.trim());
    println!(
        "{:<14} {:<13} {:>8} {:>8} {:>8} {:>8}",
        "renderer", "frame", "cpu ms", "span ms", "pass ms", "wait ms"
    );
    let mut rows = rows.to_vec();
    rows.sort_by_key(|r| {
        ["QuadRenderer", "GpuRenderer", "VelloRenderer"]
            .iter()
            .position(|n| *n == r.0)
    });
    for (name, frame, [cpu, span, pass, wait]) in rows {
        println!("{name:<14} {frame:<13} {cpu:8.3} {span:8.3} {pass:8.3} {wait:8.3}");
    }
}

fn main() {
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
    let buffer = |usage| {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("quad pass timestamps"),
            size: 16,
            usage,
            mapped_at_creation: false,
        })
    };
    let mut timer = GpuTimer::new(&device, &queue).expect("timestamp queries");
    let gpu = Gpu {
        queries: device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("quad pass"),
            ty: wgpu::QueryType::Timestamp,
            count: 2,
        }),
        resolve: buffer(
            wgpu::BufferUsages::QUERY_RESOLVE
                | wgpu::BufferUsages::COPY_SRC
                | wgpu::BufferUsages::COPY_DST,
        ),
        read: buffer(wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST),
        device,
        queue,
    };
    editor(&gpu, &mut timer);
    dense(&gpu, &mut timer);
    shapes(&gpu, &mut timer);
    labelled(&gpu, &mut timer);
    modal(&gpu, &mut timer);
}

/// The editor scene: `frames` frames of a `Ui` in which knob 5 turns (none
/// with `turn` off), after enough frames for every tween to settle.
fn editor_frames(frames: usize, turn: bool) -> Vec<ResolvedScene> {
    let mut ui = mui::Ui::default().font(HACK.clone());
    let mut app = bench_scene::App::new();
    let frame = |ui: &mut mui::Ui, app: &mut bench_scene::App| {
        let root = bench_scene::editor(ui, app);
        ui.frame(
            root,
            Some(Size::new(f64::from(W), f64::from(H))),
            mui::prelude::PointerInput::default(),
            0.016,
        )
        .expect("scene resolves")
        .scene
        .clone()
    };
    for _ in 0..120 {
        frame(&mut ui, &mut app);
    }
    (0..frames)
        .map(|i| {
            if turn {
                app.knobs[5] = (0.2 + 0.013 * i as f64) % 1.;
            }
            frame(&mut ui, &mut app)
        })
        .collect()
}

/// The editor frame without the menu float, which lays out over the whole
/// window and hides the editor in every renderer.
fn under_menu(still: &ResolvedScene) -> ResolvedScene {
    let mut under = still.clone();
    under
        .paint
        .retain(|p| !(&*p.key == "/3" || p.key.starts_with("/3/")));
    under
}

/// What the quad pass's frame drew and holds, after the timing rows.
fn note(name: &str, f: mui_vello::quad::Frame) {
    println!(
        "  quad {name}: {} instances, {} draw calls in {} passes, {} B offscreen, \
         {} path records built, {} B paths uploaded, cache {} B (glyph atlas {} + paths {})",
        f.instances,
        f.draws,
        f.passes,
        f.offscreen_bytes,
        f.paths_built,
        f.path_uploaded,
        f.atlas_bytes + f.path_bytes,
        f.atlas_bytes,
        f.path_bytes
    );
}

fn editor(gpu: &Gpu, timer: &mut GpuTimer) {
    let still = editor_frames(1, false).remove(0);
    let coverage = classify(&still.paint);
    assert!(coverage.complete(), "editor: {coverage:?}");
    let turning = editor_frames(WARM + N, true);
    let id = Affine::IDENTITY;
    let scroll = |i: usize| Affine::translate((0.37 * i as f64, 0.37 * i as f64));
    let (qv, vv) = (gpu.target(), gpu.target());
    let mut rows = Vec::new();
    let mut fresh_quads: Vec<QuadRenderer> = (0..7).map(|_| gpu.quad()).collect();
    let mut fresh_vellos: Vec<VelloRenderer> = (0..7).map(|_| gpu.vello()).collect();
    let jobs = both(
        |i| gpu.quad_frame(&mut fresh_quads[i], &still, id, &qv),
        |i| vello_frame(&mut fresh_vellos[i], &still, id, &vv),
    );
    rounds(gpu, timer, "cold", jobs, 0, 7, &mut rows);
    let cold = fresh_quads[0].frame();
    drop((fresh_quads, fresh_vellos));
    let (mut quad, mut vello) = (gpu.quad(), gpu.vello());
    let (mut wired, wv) = (gpu.wired(), gpu.target());
    let jobs = three(
        |_| gpu.quad_frame(&mut quad, &still, id, &qv),
        |_| wired_frame(&mut wired, &still, id, &wv),
        |_| vello_frame(&mut vello, &still, id, &vv),
    );
    rounds(gpu, timer, "static", jobs, WARM, N, &mut rows);
    let jobs = three(
        |i| gpu.quad_frame(&mut quad, &turning[i], id, &qv),
        |i| wired_frame(&mut wired, &turning[i], id, &wv),
        |i| vello_frame(&mut vello, &turning[i], id, &vv),
    );
    rounds(gpu, timer, "knob turning", jobs, WARM, N, &mut rows);
    let turned = quad.frame();
    let jobs = three(
        |i| gpu.quad_frame(&mut quad, &still, scroll(i), &qv),
        |i| wired_frame(&mut wired, &still, scroll(i), &wv),
        |i| vello_frame(&mut vello, &still, scroll(i), &vv),
    );
    rounds(gpu, timer, "scroll", jobs, WARM, N, &mut rows);
    let scrolled = quad.frame();
    table(
        &format!(
            "editor: {} entries, all drawn by the quad pass",
            still.paint.len()
        ),
        &rows,
    );
    note("cold", cold);
    note("knob turning", turned);
    note("scroll", scrolled);
    pngs(gpu, &still, "", &mut gpu.quad(), &mut gpu.vello());
    pngs(
        gpu,
        &under_menu(&still),
        "-under-menu",
        &mut gpu.quad(),
        &mut gpu.vello(),
    );
}

/// A 1,000-point polyline that changes every frame, over 300 static cards.
fn dense_scene(t: f64) -> ResolvedScene {
    let wave = canvas(move |s: Size| {
        let pts = (0..1000).map(|i| {
            let x = s.width * f64::from(i) / 999.;
            let k = f64::from(i);
            let y = s.height * (0.5 + 0.4 * ((k * 0.05 + t).sin() * (k * 0.37 + 2. * t).cos()));
            Point::new(x, y)
        });
        vec![Draw::stroke(
            Path::polyline(pts, false),
            Color::srgb(0.4, 1., 0.6),
            1.5,
        )]
    })
    .size(1200., 200.)
    .at(40., 560.);
    let mut kids: Vec<El> = (0..300)
        .map(|i| {
            block(56., 24.)
                .radius(6.)
                .fill(Color::srgb(0.2 + 0.002 * i as f32, 0.22, 0.26))
                .at(8. + 63. * (i % 20) as f64, 8. + 34. * (i / 20) as f64)
        })
        .collect();
    kids.push(wave);
    let root = stack(kids)
        .size(f64::from(W), f64::from(H))
        .radius(0.)
        .fill(Color::srgb(0.06, 0.07, 0.09));
    let spec = SceneSpec::new(root).offered(Size::new(f64::from(W), f64::from(H)));
    mui_scene::resolve(&spec).expect("scene resolves")
}

fn dense(gpu: &Gpu, timer: &mut GpuTimer) {
    let frames: Vec<ResolvedScene> = (0..WARM + N)
        .map(|i| dense_scene(0.05 * i as f64))
        .collect();
    assert!(
        classify(&frames[0].paint).complete(),
        "dense scene must be all supported"
    );
    let id = Affine::IDENTITY;
    let (qv, vv) = (gpu.target(), gpu.target());
    let (mut quad, mut vello) = (gpu.quad(), gpu.vello());
    let mut rows = Vec::new();
    let jobs = both(
        |i| gpu.quad_frame(&mut quad, &frames[i], id, &qv),
        |i| vello_frame(&mut vello, &frames[i], id, &vv),
    );
    rounds(gpu, timer, "wave changing", jobs, WARM, N, &mut rows);
    table(
        &format!(
            "dense: {} entries, a 1,000-point polyline changing every frame",
            frames[0].paint.len()
        ),
        &rows,
    );
    note("wave changing", quad.frame());
}

/// `scene` from both renderers, as `quad-bench-editor{tag}-{quad,vello}.png`
/// in the temp directory.
fn pngs(
    gpu: &Gpu,
    scene: &ResolvedScene,
    tag: &str,
    quad: &mut QuadRenderer,
    vello: &mut VelloRenderer,
) {
    let (q, v) = (
        gpu_support::target(&gpu.device, [W, H]),
        gpu_support::target(&gpu.device, [W, H]),
    );
    let view = |t: &wgpu::Texture| t.create_view(&wgpu::TextureViewDescriptor::default());
    gpu.quad_frame(quad, scene, Affine::IDENTITY, &view(&q));
    vello_frame(vello, scene, Affine::IDENTITY, &view(&v));
    for (name, t) in [("quad", &q), ("vello", &v)] {
        let path = std::env::temp_dir().join(format!("quad-bench-editor{tag}-{name}.png"));
        let rgba = gpu_support::readback(&gpu.device, &gpu.queue, t, [W, H]).expect("readback");
        gpu_support::save(&path, [W, H], &rgba).expect("write png");
        println!("wrote {}", path.display());
    }
}

fn shapes(gpu: &Gpu, timer: &mut GpuTimer) {
    let base = cards(None);
    let coverage = classify(&base.paint);
    assert!(
        coverage.complete(),
        "bench scene must be all supported: {coverage:?}"
    );
    let count = |l: fn(&Layer) -> bool| base.paint.iter().filter(|p| l(&p.layer)).count();
    println!(
        "shapes: {} entries ({} fills, {} strokes, {} shadows, {} clips, {} gradients)",
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
    let frames: Vec<ResolvedScene> = (0..WARM + N)
        .map(|i| {
            let mut s = base.clone();
            s.paint[moving].paint = Paint::Solid(Color::srgb((i % 50) as f32 / 50., 0.5, 0.5));
            s
        })
        .collect();
    let mut empty = base.clone();
    empty.paint.clear();
    let (qv, vv) = (gpu.target(), gpu.target());
    let (mut quad, mut vello) = (gpu.quad(), gpu.vello());
    let id = Affine::IDENTITY;
    let mut rows = Vec::new();
    // The floor: a clear and a submit, nothing drawn.
    let jobs = vec![Job {
        name: "QuadRenderer",
        quad: true,
        run: Box::new(|_| gpu.quad_frame(&mut quad, &empty, id, &qv)),
    }];
    rounds(gpu, timer, "empty list", jobs, WARM, N, &mut rows);
    let jobs = both(
        |_| gpu.quad_frame(&mut quad, &base, id, &qv),
        |_| vello_frame(&mut vello, &base, id, &vv),
    );
    rounds(gpu, timer, "static", jobs, WARM, N, &mut rows);
    let jobs = both(
        |i| gpu.quad_frame(&mut quad, &frames[i], id, &qv),
        |i| vello_frame(&mut vello, &frames[i], id, &vv),
    );
    rounds(gpu, timer, "one change", jobs, WARM, N, &mut rows);
    table(&format!("shapes: {} entries", base.paint.len()), &rows);
}

fn labelled(gpu: &Gpu, timer: &mut GpuTimer) {
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
    let id = Affine::IDENTITY;
    let (qv, vv) = (gpu.target(), gpu.target());
    let mut rows = Vec::new();
    // Cold: each renderer's first frame of this list, its glyph atlas empty
    // (the quad pass has drawn the unlabelled cards once).
    let unlabelled = cards(None);
    let mut fresh_quads: Vec<QuadRenderer> = (0..7)
        .map(|_| {
            let mut r = gpu.quad();
            gpu.quad_frame(&mut r, &unlabelled, id, &qv);
            r
        })
        .collect();
    let mut fresh_vellos: Vec<VelloRenderer> = (0..7).map(|_| gpu.vello()).collect();
    let jobs = both(
        |i| gpu.quad_frame(&mut fresh_quads[i], &base, id, &qv),
        |i| vello_frame(&mut fresh_vellos[i], &base, id, &vv),
    );
    rounds(gpu, timer, "cold", jobs, 0, 7, &mut rows);
    let cold = fresh_quads[0].frame();
    drop(fresh_vellos);
    let (mut quad, mut vello) = (gpu.quad(), gpu.vello());
    let jobs = both(
        |_| gpu.quad_frame(&mut quad, &base, id, &qv),
        |_| vello_frame(&mut vello, &base, id, &vv),
    );
    rounds(gpu, timer, "static", jobs, WARM, N, &mut rows);
    let steady = quad.frame();
    let jobs = both(
        |i| gpu.quad_frame(&mut quad, &labels[i], id, &qv),
        |i| vello_frame(&mut vello, &labels[i], id, &vv),
    );
    rounds(gpu, timer, "label change", jobs, WARM, N, &mut rows);
    let jobs = both(
        |i| gpu.quad_frame(&mut quad, &base, scroll(i), &qv),
        |i| vello_frame(&mut vello, &base, scroll(i), &vv),
    );
    rounds(gpu, timer, "scroll", jobs, WARM, N, &mut rows);
    println!(
        "\nquad cold frame: {} cells rasterised, {} bytes uploaded; steady: {steady:?}",
        cold.rasterised, cold.uploaded
    );
    table(
        &format!(
            "labelled cards: {} entries, {runs} glyph runs, {glyphs} glyphs",
            base.paint.len()
        ),
        &rows,
    );
}

/// A dialog fading in over the editor: a frosted panel blurring what is
/// under it, a title and two buttons, at a new opacity every frame. The
/// quad pass keeps the panel's blur and its group and draws the window
/// again with them; `VelloRenderer` replays the editor under the blur every
/// frame.
fn modal(gpu: &Gpu, timer: &mut GpuTimer) {
    let ink = Color::srgb(0.95, 0.95, 0.97);
    let dialog = |opacity: f32| {
        let button = |label: &str, x: f64| {
            stack![text(label).text_size(15.).fill(ink).at(22., 8.)]
                .size(110., 34.)
                .radius(8.)
                .fill(Color::srgba(1., 1., 1., 0.14))
                .at(x, 150.)
        };
        stack![
            text("Discard changes to this patch?")
                .text_size(20.)
                .fill(ink)
                .at(28., 40.),
            button("Cancel", 160.),
            button("Discard", 290.),
        ]
        .size(420., 210.)
        .radius(16.)
        .fill(Color::srgba(0.08, 0.09, 0.12, 0.55))
        .stroke(Color::srgba(1., 1., 1., 0.12))
        .shadow(Shadow::soft(24.))
        .backdrop_blur(18.)
        .opacity(opacity)
        .at(430., 295.)
    };
    let under = under_menu(&editor_frames(1, false)[0]);
    let size = Size::new(f64::from(W), f64::from(H));
    let frames: Vec<ResolvedScene> = (0..WARM + N)
        .map(|i| {
            let over =
                stack![dialog(0.3 + 0.7 * (i % 30) as f32 / 29.)].size(size.width, size.height);
            let mut spec = SceneSpec::new(over).offered(size);
            spec.font = Some(HACK.clone());
            let over = mui_scene::resolve(&spec).expect("dialog");
            let mut scene = under.clone();
            scene.paint.extend(over.paint.iter().map(|p| {
                let mut p = p.clone();
                p.key = format!("/modal{}", p.key).into();
                p
            }));
            scene
        })
        .collect();
    let coverage = classify(&frames[0].paint);
    assert!(coverage.complete(), "modal: {coverage:?}");
    let id = Affine::IDENTITY;
    let (qv, vv) = (gpu.target(), gpu.target());
    let (mut quad, mut vello) = (gpu.quad(), gpu.vello());
    pngs(gpu, &frames[20], "-modal", &mut quad, &mut vello);
    let mut rows = Vec::new();
    let mut passes = std::collections::BTreeMap::<usize, usize>::new();
    let jobs = both(
        |i| {
            let drew = gpu.quad_frame(&mut quad, &frames[i], id, &qv);
            *passes.entry(quad.frame().passes).or_default() += 1;
            drew
        },
        |i| vello_frame(&mut vello, &frames[i], id, &vv),
    );
    rounds(gpu, timer, "modal fading", jobs, WARM, N, &mut rows);
    table(
        &format!(
            "modal over editor: {} entries, a blurred dialog fading in",
            frames[0].paint.len()
        ),
        &rows,
    );
    note("modal fading", quad.frame());
    println!("  quad modal fading, frames by passes drawn: {passes:?}");
}
