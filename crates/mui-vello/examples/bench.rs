//! Why `vello_hybrid` and not classic `vello`, measured on a Kurv-sized editor.
//!
//!     cargo run -p mui-vello --release --features cpu --example bench
//!     cargo run -p mui-vello --release --features cpu,bench-classic --example bench
//!
//! One scene, three backends, three cases. Each frame is split into resolve
//! (`Ui::frame`: styling, layout, text shaping, the paint list), encode (the
//! paint-list walk onto a `Canvas`) and render (rasterise and wait for it).
//! Reported as the median of 50 frames after 5 warm-ups, so a stray scheduler
//! hiccup cannot move a number.

use std::time::Instant;

use std::sync::Arc;

use mui::prelude::*;
use mui::Ui;
use mui_scene::{Layer, ResolvedScene};
use mui_vello::kurbo::Affine;
use mui_vello::{Cpu, Gpu, PathCache};
use vello_common::pixmap::Pixmap;

const W: u16 = 1280;
const H: u16 = 800;
const WARM: usize = 5;
const N: usize = 50;

// ---------------------------------------------------------------- the scene

/// The knobs, sliders and labels an editor actually carries. One `f64` per
/// control is the whole application state; the tree is rebuilt from it.
struct App {
    knobs: Vec<f64>,
    sliders: Vec<f64>,
}

impl App {
    fn new() -> Self {
        Self {
            knobs: (0..40).map(|i| f64::from(i) / 40.0).collect(),
            sliders: (0..8).map(|i| f64::from(i) / 8.0).collect(),
        }
    }
}

/// A four-pixel texture, standing in for whatever a host decodes. One buffer,
/// shared by every pill: `PathCache` and the pixmap intern both key on the
/// `Arc`, so four pills cost one upload.
fn swatch() -> Arc<Image> {
    let px: Vec<u8> = [
        [220, 60, 60, 255],
        [60, 200, 120, 255],
        [70, 110, 240, 255],
        [240, 210, 80, 255],
    ]
    .concat();
    Arc::new(Image::rgba(2, 2, px).expect("2x2 rgba"))
}

/// A response curve: 50 cubics through the node's own frame.
fn curve(seed: f64) -> El {
    canvas(move |size: Size| {
        let step = size.width / 50.0;
        let mut p = Path::default().move_to(Point::new(0.0, size.height * 0.5));
        for i in 0..50 {
            let (x0, x1) = (step * f64::from(i), step * f64::from(i + 1));
            let y = size.height * (0.5 + 0.4 * (f64::from(i) * 0.6 + seed).sin());
            p = p.cubic_to(
                Point::new(x0 + step / 3.0, y),
                Point::new(x1 - step / 3.0, y),
                Point::new(x1, size.height * 0.5),
            );
        }
        vec![Draw::stroke(p, Role::Primary, 2.0)]
    })
    .height(90.0)
}

const BLURB: &str = "the filter tracks the key and the envelope follows it, \
which is what the second knob is for when the resonance is high";

/// 40 knobs, 8 sliders, 200 labels, a clipped 60-row list, two curves, three
/// floats, 20 wrapped paragraphs, 4 image-filled pills and 6 cards whose fill
/// is spring-driven -- roughly what a synth editor puts on screen at once.
/// `images` is false for classic vello (see `run`); the four pills are then
/// plain fills.
fn editor(ui: &mut Ui, app: &mut App, images: bool) -> El {
    let knobs: Vec<El> = (0..40)
        .map(|i| {
            knob(ui, &format!("k{i}"), "cut", &mut app.knobs[i], 0.0..=1.0)
                .size(S)
                .el()
        })
        .collect();
    let sliders: Vec<El> = (0..8)
        .map(|i| {
            slider(
                ui,
                &format!("s{i}"),
                "amount",
                &mut app.sliders[i],
                0.0..=1.0,
            )
            .el()
        })
        .collect();
    let labels: Vec<El> = (0..200).map(|i| caption(format!("p{i:03}"))).collect();
    let list: Vec<El> = (0..60)
        .map(|i| {
            row![label(format!("step {i:02}")), spacer(), caption("0.00")]
                .pad(Xs)
                .id(format!("row{i}"))
        })
        .collect();

    // 20 wrapped paragraphs: each is narrower than its text, so the scene
    // resolver breaks lines and solves the layout a second time.
    let blurbs: Vec<El> = (0..20)
        .map(|i| label(format!("{i}. {BLURB}")).w(150.0).lines(4))
        .collect();
    // 4 image-filled pills, all sharing one 2x2 buffer.
    let img = swatch();
    let pills: Vec<El> = [Fit::Cover, Fit::Contain, Fit::Fill, Fit::Cover]
        .into_iter()
        .map(|fit| {
            let l = leaf(120.0, 40.0).pill();
            if images {
                l.fill(Fill::Image(img.clone(), fit))
            } else {
                l.fill(Role::Raised)
            }
        })
        .collect();
    // 6 cards carrying a transition, so every frame walks their spring
    // channels whether or not the colour moved.
    let cards: Vec<El> = (0..6)
        .map(|i| {
            col![title(format!("card {i}")), caption("ready")]
                .gap(Xs)
                .pad(M)
                .shell(8.0, Role::Raised)
                .animate()
                .id(format!("card{i}"))
        })
        .collect();

    let body = col![
        row![title("Kurv"), spacer(), caption("48 kHz")].pad(S),
        grid(8, knobs).gap(M).pad(M).shell(10.0, Role::Raised),
        column(sliders).gap(S).pad(M),
        grid(20, labels).gap(Xs).pad(S),
        column(list).gap(2.0).scroll().height(240.0).pad(Xs),
        row![curve(0.0), curve(1.3)].gap(M).pad(M),
        grid(5, blurbs).gap(S).pad(S),
        row(pills).gap(S).pad(S),
        grid(6, cards).gap(S).pad(S),
    ]
    .weld(Role::Surface);

    // Three floats: a tooltip, a readout and a menu, painted over everything.
    stack![
        body,
        caption("tip")
            .pad(S)
            .fill(Role::Raised)
            .float()
            .offset(200.0, 120.0),
        caption("-6.0 dB")
            .pad(S)
            .fill(Role::Raised)
            .float()
            .offset(900.0, 60.0),
        col![caption("open"), caption("save")]
            .pad(S)
            .fill(Role::Raised)
            .float()
            .offset(600.0, 400.0),
    ]
}

// ---------------------------------------------------------------- the clock

struct Row {
    backend: &'static str,
    case: &'static str,
    resolve: f64,
    encode: f64,
    render: f64,
}

impl Row {
    fn total(&self) -> f64 {
        self.resolve + self.encode + self.render
    }
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
}

impl Case {
    fn name(self) -> &'static str {
        match self {
            Self::Cold => "cold",
            Self::Static => "static",
            Self::Knob => "one knob turning",
        }
    }
}

/// Drive `WARM + N` frames of `case` and hand each resolved scene to `draw`,
/// which returns (encode ms, render ms).
/// Classic vello wants a `peniko::Image` this bench does not build, so it
/// alone gets the same scene with four solid fills instead.
fn run(
    backend: &'static str,
    case: Case,
    font: &[u8],
    images: bool,
    mut draw: impl FnMut(&ResolvedScene) -> (f64, f64),
) -> Row {
    let mut ui = Ui::new(Theme::DEFAULT).font(font.to_vec());
    let mut app = App::new();
    let (mut r, mut e, mut d) = (vec![], vec![], vec![]);
    for i in 0..WARM + N {
        if case == Case::Cold {
            ui = Ui::new(Theme::DEFAULT).font(font.to_vec());
        }
        if case == Case::Knob {
            app.knobs[7] = f64::from(i as u32 % 100) / 100.0;
        }
        let root = editor(&mut ui, &mut app, images);
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
            r.push(resolve);
            e.push(encode);
            d.push(render);
        }
    }
    Row {
        backend,
        case: case.name(),
        resolve: median(&mut r),
        encode: median(&mut e),
        render: median(&mut d),
    }
}

const CASES: [Case; 3] = [Case::Cold, Case::Static, Case::Knob];

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
        let mut ui = Ui::new(Theme::DEFAULT).font(font.to_vec());
        let mut app = App::new();
        let root = editor(&mut ui, &mut app, true);
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
        // the backend's strip building: the same walk, converting only.
        let paths = || f.scene.paint.iter().filter(|p| p.layer != Layer::Unclip);
        let mut cache = PathCache::new();
        let (mut cold, mut warm) = (vec![], vec![]);
        for i in 0..WARM + N {
            let t = Instant::now();
            for p in paths() {
                std::hint::black_box(mui_vello::bez_path(&p.path, mui_vello::ARC_TOLERANCE))
                    .expect("converts");
            }
            let a = since(t);
            let t = Instant::now();
            for p in paths() {
                std::hint::black_box(cache.bez(p)).expect("converts");
            }
            let b = since(t);
            if i >= WARM {
                cold.push(a);
                warm.push(b);
            }
        }
        println!(
            "bez conversion: {:.3} ms uncached, {:.3} ms from a warm PathCache ({} entries)",
            median(&mut cold),
            median(&mut warm),
            cache.len()
        );
    }

    // Resolve with nothing painted at all: MUI's own floor.
    rows.push(run("mui (resolve only)", Case::Static, font, true, |_| {
        (0.0, 0.0)
    }));

    // --- CPU (vello_cpu, the same sparse-strip pipeline as hybrid)
    {
        let mut ctx = vello_cpu::RenderContext::new(W, H);
        let mut res = vello_cpu::Resources::default();
        let mut pix = Pixmap::new(W, H);
        let mut draw = |scene: &ResolvedScene| {
            ctx.reset();
            let t = Instant::now();
            mui_vello::paint(
                &mut Cpu {
                    ctx: &mut ctx,
                    resources: &mut res,
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
            rows.push(run("vello_cpu", c, font, true, &mut draw));
        }
    }

    // --- CPU again, with the path conversion remembered between frames.
    {
        let mut ctx = vello_cpu::RenderContext::new(W, H);
        let mut res = vello_cpu::Resources::default();
        let mut pix = Pixmap::new(W, H);
        let mut cache = PathCache::new();
        let mut draw = |scene: &ResolvedScene| {
            ctx.reset();
            let t = Instant::now();
            mui_vello::paint_cached(
                &mut Cpu {
                    ctx: &mut ctx,
                    resources: &mut res,
                },
                scene,
                Affine::IDENTITY,
                &mut cache,
            )
            .expect("paints");
            ctx.flush();
            let encode = since(t);
            let t = Instant::now();
            ctx.render(&mut pix, &mut res);
            (encode, since(t))
        };
        for c in CASES {
            rows.push(run("vello_cpu cached", c, font, true, &mut draw));
        }
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
        "\n{:<20} {:<17} {:>9} {:>9} {:>9} {:>9} {:>8}",
        "backend", "case", "resolve", "encode", "render", "total", "fps"
    );
    for r in &rows {
        println!(
            "{:<20} {:<17} {:9.3} {:9.3} {:9.3} {:9.3} {:8.0}",
            r.backend,
            r.case,
            r.resolve,
            r.encode,
            r.render,
            r.total(),
            1000.0 / r.total()
        );
    }
    println!("\npeak RSS {:.1} MiB", peak_rss());
}

/// Every GPU backend, on one device. `None` when there is no adapter.
async fn gpu() -> Option<(wgpu::AdapterInfo, Vec<Row>)> {
    let font = epaint_default_fonts::HACK_REGULAR;
    let instance = wgpu::Instance::default();
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions::default())
        .await
        .ok()?;
    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            // Classic vello's compute pipeline needs more than the downlevel
            // defaults; hybrid would be happy with them.
            required_limits: adapter.limits(),
            ..Default::default()
        })
        .await
        .ok()?;
    let mut rows = Vec::new();

    // --- hybrid: a render pass over sparse strips, no compute.
    {
        let texture = target(&device, wgpu::TextureUsages::RENDER_ATTACHMENT);
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let (mut renderer, mut resources) = vello_hybrid::Renderer::new(
            &device,
            &vello_hybrid::RenderTargetConfig {
                format: texture.format(),
                width: W.into(),
                height: H.into(),
            },
        );
        let mut scene = vello_hybrid::Scene::new(W, H);
        let mut ids = mui_vello::ImageIds::default();
        let mut draw = |resolved: &ResolvedScene| {
            scene.reset();
            let t = Instant::now();
            mui_vello::paint(
                &mut Gpu {
                    scene: &mut scene,
                    resources: &mut resources,
                    atlas: Some(mui_vello::Atlas {
                        renderer: &mut renderer,
                        device: &device,
                        queue: &queue,
                        ids: &mut ids,
                    }),
                },
                resolved,
                Affine::IDENTITY,
            )
            .expect("paints");
            let encode = since(t);
            let t = Instant::now();
            let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            renderer
                .render(
                    &scene,
                    &mut resources,
                    &device,
                    &queue,
                    &mut enc,
                    &vello_hybrid::RenderSize {
                        width: W.into(),
                        height: H.into(),
                    },
                    &view,
                    &vello_hybrid::TextureBindings::new(),
                )
                .expect("render");
            queue.submit([enc.finish()]);
            device
                .poll(wgpu::PollType::wait_indefinitely())
                .expect("poll");
            (encode, since(t))
        };
        for c in CASES {
            rows.push(run("vello_hybrid", c, font, true, &mut draw));
        }
    }

    // --- hybrid, with the path conversion cached.
    {
        let texture = target(&device, wgpu::TextureUsages::RENDER_ATTACHMENT);
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let (mut renderer, mut resources) = vello_hybrid::Renderer::new(
            &device,
            &vello_hybrid::RenderTargetConfig {
                format: texture.format(),
                width: W.into(),
                height: H.into(),
            },
        );
        let mut scene = vello_hybrid::Scene::new(W, H);
        let mut cache = PathCache::new();
        let mut ids = mui_vello::ImageIds::default();
        let mut draw = |resolved: &ResolvedScene| {
            scene.reset();
            let t = Instant::now();
            mui_vello::paint_cached(
                &mut Gpu {
                    scene: &mut scene,
                    resources: &mut resources,
                    atlas: Some(mui_vello::Atlas {
                        renderer: &mut renderer,
                        device: &device,
                        queue: &queue,
                        ids: &mut ids,
                    }),
                },
                resolved,
                Affine::IDENTITY,
                &mut cache,
            )
            .expect("paints");
            let encode = since(t);
            let t = Instant::now();
            let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            renderer
                .render(
                    &scene,
                    &mut resources,
                    &device,
                    &queue,
                    &mut enc,
                    &vello_hybrid::RenderSize {
                        width: W.into(),
                        height: H.into(),
                    },
                    &view,
                    &vello_hybrid::TextureBindings::new(),
                )
                .expect("render");
            queue.submit([enc.finish()]);
            device
                .poll(wgpu::PollType::wait_indefinitely())
                .expect("poll");
            (encode, since(t))
        };
        for c in CASES {
            rows.push(run("vello_hybrid cached", c, font, true, &mut draw));
        }
    }

    #[cfg(feature = "bench-classic")]
    rows.extend(classic::rows(&device, &queue, font));

    Some((adapter.get_info(), rows))
}

fn target(device: &wgpu::Device, extra: wgpu::TextureUsages) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
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
        usage: extra,
        view_formats: &[],
    })
}

// ------------------------------------------------------------ classic vello

#[cfg(feature = "bench-classic")]
mod classic {
    use super::{run, since, target, Row, CASES, H, W};
    use mui_scene::ResolvedScene;
    use mui_vello::kurbo::{Affine, BezPath, Rect, Stroke};
    use mui_vello::{Canvas, PaintType};
    use std::sync::Arc;
    use std::time::Instant;
    use vello_common::peniko::color::{AlphaColor, Srgb};
    use vello_common::peniko::{Blob, Brush, Fill, FontData};

    /// The same paint walk, encoded into a classic `vello::Scene`. Classic
    /// takes the transform and brush per call where the sparse-strip scenes
    /// hold them as state, so this wrapper is that state.
    struct Classic<'a> {
        scene: &'a mut vello::Scene,
        transform: Affine,
        brush: Brush,
        stroke: Stroke,
        font: Option<FontData>,
    }

    impl Classic<'_> {
        fn color(&self) -> AlphaColor<Srgb> {
            match self.brush {
                Brush::Solid(c) => c,
                _ => AlphaColor::TRANSPARENT,
            }
        }
    }

    impl Canvas for Classic<'_> {
        fn set_transform(&mut self, t: Affine) {
            self.transform = t;
        }
        fn set_paint(&mut self, p: PaintType) {
            self.brush = match p {
                PaintType::Solid(c) => Brush::Solid(c),
                PaintType::Gradient(g) => Brush::Gradient(g),
                // Classic runs with `images: false`, so this never fires.
                PaintType::Image(_) => Brush::Solid(AlphaColor::TRANSPARENT),
            };
        }
        fn set_stroke(&mut self, s: Stroke) {
            self.stroke = s;
        }
        fn fill_path(&mut self, p: &BezPath) {
            self.scene
                .fill(Fill::NonZero, self.transform, &self.brush, None, p);
        }
        fn stroke_path(&mut self, p: &BezPath) {
            self.scene
                .stroke(&self.stroke, self.transform, &self.brush, None, p);
        }
        fn fill_blurred_rounded_rect(&mut self, r: &Rect, radius: f32, std_dev: f32, _: bool) {
            let c = self.color();
            self.scene.draw_blurred_rounded_rect(
                self.transform,
                *r,
                c,
                radius.into(),
                std_dev.into(),
            );
        }
        fn push_clip(&mut self, p: &BezPath) {
            self.scene.push_clip_layer(Fill::NonZero, self.transform, p);
        }
        fn pop_clip(&mut self) {
            self.scene.pop_layer();
        }
        fn push_layer(&mut self, blend: mui_vello::peniko::BlendMode, opacity: f32) {
            // Classic has no unbounded layer, so the clip is the whole canvas.
            self.scene.push_layer(
                Fill::NonZero,
                blend,
                opacity,
                self.transform,
                &Rect::new(-1e6, -1e6, 1e6, 1e6),
            );
        }
        fn pop_layer(&mut self) {
            self.scene.pop_layer();
        }
        // ponytail: one font per process, because `Blob::new` mints a fresh id
        // per call and classic's glyph cache keys on it. The library keeps a
        // real map; this bench only ever draws one font.
        fn glyphs(&mut self, text: &mui_scene::Text) {
            let (size, glyphs) = (text.size, &text.glyphs);
            let f = self
                .font
                .get_or_insert_with(|| FontData::new(Blob::new(Arc::new(text.font.clone())), 0))
                .clone();
            let (ox, oy) = (text.origin.x as f32, text.origin.y as f32);
            self.scene
                .draw_glyphs(&f)
                .font_size(size)
                .hint(true)
                .transform(self.transform)
                .brush(&self.brush)
                .draw(
                    Fill::NonZero,
                    glyphs.iter().map(|&(id, x)| vello::Glyph {
                        id,
                        x: ox + x,
                        y: oy,
                    }),
                );
        }
    }

    pub fn rows(device: &wgpu::Device, queue: &wgpu::Queue, font: &[u8]) -> Vec<Row> {
        let texture = target(
            device,
            wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut renderer = vello::Renderer::new(device, vello::RendererOptions::default())
            .expect("classic renderer");
        let mut scene = vello::Scene::new();
        let mut peak = 0u32;
        let mut draw = |resolved: &ResolvedScene| {
            scene.reset();
            let t = Instant::now();
            mui_vello::paint(
                &mut Classic {
                    scene: &mut scene,
                    transform: Affine::IDENTITY,
                    brush: Brush::Solid(AlphaColor::TRANSPARENT),
                    stroke: Stroke::new(1.0),
                    font: None,
                },
                resolved,
                Affine::IDENTITY,
            )
            .expect("paints");
            let encode = since(t);
            peak = peak.max(scene.bump_estimate(None).total);
            let t = Instant::now();
            renderer
                .render_to_texture(
                    device,
                    queue,
                    &scene,
                    &view,
                    &vello::RenderParams {
                        base_color: vello_common::peniko::color::palette::css::BLACK,
                        width: W.into(),
                        height: H.into(),
                        antialiasing_method: vello::AaConfig::Area,
                    },
                )
                .expect("render");
            device
                .poll(wgpu::PollType::wait_indefinitely())
                .expect("poll");
            (encode, since(t))
        };
        let rows = CASES.map(|c| run("vello (classic)", c, font, false, &mut draw));
        println!(
            "classic GPU buffer estimate: {:.1} MiB peak",
            f64::from(peak) / (1024.0 * 1024.0)
        );
        rows.into()
    }
}
