//! What a frame costs on a synth-sized editor, on the CPU rasteriser and on
//! the retained GPU renderer.
//!
//!     cargo run -p mui-vello --profile perf --features cpu,gpu-effects --example bench
//!
//! One scene, both backends, four cases. Each frame is split into resolve
//! (`Ui::frame`: styling, layout, text shaping, the paint list), encode (the
//! paint-list walk onto a `Canvas`) and render (rasterise and wait for it).
//! Reported as the median of 50 frames after 5 warm-ups, so a stray scheduler
//! hiccup cannot move a number.
//! GPU initialization is reported separately; `WGPU_BACKEND` selects backends
//! for startup comparisons, as it does in the native hosts.

use std::time::Instant;

use mui::Ui;
use mui::prelude::*;
use mui_scene::{Layer, ResolvedScene};
use mui_vello::kurbo::Affine;
use mui_vello::{Cache, Cpu};
use vello_common::pixmap::Pixmap;

const W: u16 = 1280;
const H: u16 = 800;
const WARM: usize = 5;
const N: usize = 50;

mod bench_scene;
use bench_scene::{App, editor, swatch, vectors};

// ---------------------------------------------------------------- the clock

struct Row {
    backend: &'static str,
    case: &'static str,
    build: f64,
    total: f64,
    p95: f64,
    resolve: f64,
    encode: f64,
    render: f64,
}

fn median(v: &mut [f64]) -> f64 {
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

fn since(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1e3
}

#[derive(Clone, Copy, PartialEq)]
enum Case {
    /// A fresh `Ui` every frame: nothing shaped, nothing cached.
    Cold,
    /// The same tree again, unchanged.
    Static,
    /// One knob moves; everything else is the same tree.
    Knob,
    /// The same tree, but the host hands over a freshly decoded image every
    /// frame: a video or meter texture. Every retained path has to redraw.
    Image,
}

impl Case {
    fn name(self) -> &'static str {
        match self {
            Self::Cold => "cold",
            Self::Static => "static",
            Self::Knob if vectors() => "all curves moving",
            Self::Knob => "one knob turning",
            Self::Image => "fresh image",
        }
    }
}

/// Drive `WARM + N` frames of `case` and hand each resolved scene to `draw`,
/// which returns (encode ms, render ms).
fn run(
    backend: &'static str,
    case: Case,
    font: &[u8],
    mut draw: impl FnMut(&ResolvedScene) -> (f64, f64),
) -> Row {
    let mut ui = Ui::default().font(Font::new(font).unwrap());
    let mut app = App::new();
    let (mut r, mut e, mut d) = (vec![], vec![], vec![]);
    let (mut b, mut totals) = (vec![], vec![]);
    for i in 0..WARM + N {
        if case == Case::Cold {
            ui = Ui::default().font(Font::new(font).unwrap());
        }
        if case == Case::Knob {
            app.knobs[7] = f64::from(i as u32 % 100) / 100.0;
        }
        if case == Case::Image {
            app.swatch = swatch();
        }
        let start = Instant::now();
        let root = editor(&mut ui, &mut app);
        let build = since(start);
        let t = Instant::now();
        let frame = ui
            .frame(
                root,
                Some(Size::new(W.into(), H.into())),
                PointerInput::default(),
                0.016,
            )
            .expect("scene resolves");
        let resolve = since(t);
        let (encode, render) = draw(frame.scene);
        if i >= WARM {
            totals.push(since(start));
            b.push(build);
            r.push(resolve);
            e.push(encode);
            d.push(render);
        }
    }
    let total = median(&mut totals);
    Row {
        build: median(&mut b),
        total,
        p95: totals[(totals.len() * 95).div_ceil(100) - 1],
        backend,
        case: case.name(),
        resolve: median(&mut r),
        encode: median(&mut e),
        render: median(&mut d),
    }
}

const CASES: [Case; 4] = [Case::Cold, Case::Static, Case::Knob, Case::Image];

/// Peak resident set of this process, in MiB -- the only memory number the
/// sparse-strip renderers expose at all.
fn peak_rss() -> f64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find_map(|l| l.strip_prefix("VmHWM:"))
                .and_then(|v| v.split_whitespace().next()?.parse::<f64>().ok())
        })
        .unwrap_or(0.0)
        / 1024.0
}

// ---------------------------------------------------------------- backends

fn main() {
    let font = epaint_default_fonts::HACK_REGULAR;
    let mut rows = Vec::new();
    {
        let mut ui = Ui::default().font(Font::new(font).unwrap());
        let mut app = App::new();
        let root = editor(&mut ui, &mut app);
        let f = ui
            .frame(
                root,
                Some(Size::new(W.into(), H.into())),
                PointerInput::default(),
                0.016,
            )
            .expect("scene resolves");
        println!(
            "scene: {} surfaces, {} paint ops, {} glyph runs",
            f.scene.surfaces().count(),
            f.scene.paint.len(),
            f.scene.paint.iter().filter(|p| p.text.is_some()).count()
        );

        // What of `encode` is MUI's own arc-to-cubic conversion rather than
        // the backend's strip building: the same walk, converting only. It is
        // also the most a cache of the conversion could save per frame.
        let mut times = vec![];
        for i in 0..WARM + N {
            let t = Instant::now();
            for p in f.scene.paint.iter().filter(|p| p.layer != Layer::Unclip) {
                std::hint::black_box(mui_vello::bez_path(&p.path, mui_vello::ARC_TOLERANCE))
                    .expect("converts");
            }
            if i >= WARM {
                times.push(since(t));
            }
        }
        println!("bez conversion: {:.3} ms per frame", median(&mut times));
    }

    // Resolve with nothing painted at all: MUI's own floor.
    rows.push(run("mui (resolve only)", Case::Static, font, |_| {
        (0.0, 0.0)
    }));

    // --- CPU (vello_cpu, sparse strips)
    {
        let mut ctx = vello_cpu::RenderContext::new(W, H);
        let mut res = vello_cpu::Resources::default();
        let mut cache = Cache::default();
        let mut pix = Pixmap::new(W, H);
        let mut draw = |scene: &ResolvedScene| {
            ctx.reset();
            let t = Instant::now();
            mui_vello::paint(
                &mut Cpu {
                    ctx: &mut ctx,
                    resources: &mut res,
                    cache: &mut cache,
                },
                scene,
                Affine::IDENTITY,
            )
            .expect("paints");
            ctx.flush();
            let encode = since(t);
            let t = Instant::now();
            ctx.render(&mut pix, &mut res);
            (encode, since(t))
        };
        for c in CASES {
            rows.push(run("vello_cpu", c, font, &mut draw));
        }
    }

    // The fallback retains pixels. Its render column includes both preparation
    // and rasterization; unlike the raw CPU rows, unchanged pixels need neither.
    let mut retained = mui_vello::software::Renderer::new((W.into(), H.into())).unwrap();
    for case in CASES {
        let mut draws = 0;
        rows.push(run("CPU retained", case, font, |scene| {
            let start = Instant::now();
            draws += usize::from(
                retained
                    .render(scene, Affine::IDENTITY)
                    .expect("CPU render"),
            );
            (0.0, since(start))
        }));
        println!(
            "CPU retained {}: {draws} rasterizations (render includes preparation)",
            case.name()
        );
    }

    match pollster::block_on(gpu()) {
        Some((info, mut gpu_rows)) => {
            println!(
                "adapter: {} ({:?}, {:?})",
                info.name, info.backend, info.device_type
            );
            rows.append(&mut gpu_rows);
        }
        None => println!("adapter: none -- GPU rows skipped, CPU numbers only"),
    }

    println!(
        "\n{:<20} {:<17} {:>9} {:>9} {:>9} {:>9} {:>9} {:>9}",
        "backend", "case", "build", "resolve", "encode", "render", "total", "p95"
    );
    for r in &rows {
        println!(
            "{:<20} {:<17} {:9.3} {:9.3} {:9.3} {:9.3} {:9.3} {:9.3}",
            r.backend, r.case, r.build, r.resolve, r.encode, r.render, r.total, r.p95
        );
    }
    println!("\npeak RSS {:.1} MiB", peak_rss());
}

/// The retained GPU renderer. `None` when there is no adapter.
async fn gpu() -> Option<(wgpu::AdapterInfo, Vec<Row>)> {
    let font = epaint_default_fonts::HACK_REGULAR;
    // The cold frame case resets UI state, not the device or its pipelines.
    // Report startup separately so editor-open stalls are not hidden by warmup.
    let startup = Instant::now();
    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let instance_ms = since(startup);
    let start = Instant::now();
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions::default())
        .await
        .ok()?;
    let adapter_ms = since(start);
    let start = Instant::now();
    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            // Vello's compute pipeline needs more than the downlevel defaults.
            required_limits: adapter.limits(),
            ..Default::default()
        })
        .await
        .ok()?;
    let device_ms = since(start);
    let start = Instant::now();
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("bench target"),
        size: wgpu::Extent3d {
            width: W.into(),
            height: H.into(),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let mut renderer = mui_vello::effects::GpuRenderer::new(
        &device,
        &queue,
        texture.format(),
        [W.into(), H.into()],
        mui_vello::effects::Budget::default(),
    )
    .await
    .expect("GPU renderer");
    println!(
        "GPU startup (ms): instance {instance_ms:.3}, adapter {adapter_ms:.3}, device {device_ms:.3}, renderer+target {:.3}, total {:.3}",
        since(start),
        since(startup),
    );
    // An unchanged paint list skips the encode and the render entirely.
    // `encode` is everything `render` spends on the CPU; `render` is the
    // wait for the GPU.
    let mut rows = Vec::new();
    for case in CASES {
        let (mut encodes, mut renders) = (0, 0);
        rows.push(run("GpuRenderer", case, font, |scene| {
            let start = Instant::now();
            let stats = renderer
                .render(scene, Affine::IDENTITY, &view)
                .expect("render");
            encodes += stats.encoded_scenes;
            renders += stats.renders;
            let encode = since(start);
            let start = Instant::now();
            device
                .poll(wgpu::PollType::wait_indefinitely())
                .expect("poll");
            (encode, since(start))
        }));
        println!(
            "GpuRenderer {}: {encodes} scene encodes, {renders} renders",
            case.name()
        );
    }
    Some((adapter.get_info(), rows))
}
