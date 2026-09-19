//! ISOLATED effect/composition benchmark, not a KURV/DAW or presented-FPS test.
//! The same WGSL, plates, crop, background, clip, opacity and foreground are used
//! for both backends. Classic includes its registered-texture atlas copy.
//! GPU timestamps bracket queue commands; classic's multi-submit interval also
//! includes intervening queue idle time. CPU timing excludes the in-flight gate.
use mui_vello::{
    effects::{Budget, GpuTimer, GpuTiming, OutputEncoding, WeldTextures},
    kurbo::{Affine, Rect, RoundedRect, Shape as _},
    Canvas as _, Gpu,
};
use mui_weld::{analytic::AnalyticWeld, Brush, Color, Geometry, Rect as WRect, Source, Weld};
use std::{
    io::Write,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use vello_common::{
    geometry::RectU16,
    peniko::{
        color::{AlphaColor, Srgb},
        BlendMode, Fill, ImageData, ImageQuality,
    },
};
#[allow(dead_code)]
mod gpu_support;
const WARM: u64 = 32;
fn spec(delta: f64, scale: f64) -> AnalyticWeld {
    let source = |x: f64, color: Color, width: f64| Source {
        shape: Geometry::RoundedRect {
            bounds: WRect::new(x, 0., x + 110., 80.),
            radius: 18.,
        },
        fill: Some(Brush::Solid(color)),
        border: Some(Brush::Solid(Color::srgb(1., 1., 1., 0.7))),
        width,
    };
    AnalyticWeld::from_sources(
        &[
            source(0., Color::srgb(0.9, 0.15, 0.08, 0.8), 2.),
            source(130. + delta, Color::srgb(0.18, 0.35, 0.95, 0.7), 9.),
        ],
        Weld::all().reach(40.).blend(85.),
        scale,
    )
    .expect("fixed benchmark material")
}
fn image_transform(m: &AnalyticWeld, scale: f64) -> Affine {
    let d = m.domain();
    Affine::scale(scale) * Affine::translate((55. + d.x0, 70. + d.y0)) * Affine::scale(1. / scale)
}
// Keep clipping source construction simple and shared; apply the transform when
// encoding rather than relying on backend-specific rectangle convenience APIs.
fn clip_path() -> mui_vello::kurbo::BezPath {
    RoundedRect::from_rect(Rect::new(18., 24., 320., 215.), 18.).to_path(0.01)
}
fn opaque(v: f32) -> AlphaColor<Srgb> {
    AlphaColor::new([v, v, v, 1.])
}
fn encode_hybrid(
    scene: &mut vello_hybrid::Scene,
    resources: &mut vello_hybrid::Resources,
    pool: &WeldTextures,
    m: &AnalyticWeld,
    size: [u32; 2],
    scale: f64,
) {
    scene.reset_and_resize(size[0] as u16, size[1] as u16);
    let mut c = Gpu {
        scene,
        resources,
        atlas: None,
    };
    c.set_transform(Affine::IDENTITY);
    c.set_paint(opaque(0.).into());
    c.fill_path(&Rect::new(0., 0., f64::from(size[0]), f64::from(size[1])).to_path(0.01));
    c.set_transform(Affine::scale(scale));
    c.push_layer(BlendMode::default(), 0.8);
    c.push_clip(&clip_path());
    c.set_transform(Affine::IDENTITY);
    let [w, h] = m.pixels();
    c.scene.draw_texture_rects(
        pool.texture_id("join").expect("encoded texture"),
        ImageQuality::Medium,
        [vello_hybrid::SampleRect {
            source_region: RectU16::new(0, 0, w as u16, h as u16),
            transform: image_transform(m, scale),
        }],
    );
    c.pop_clip();
    c.pop_layer();
    c.set_transform(Affine::scale(scale));
    c.set_paint(opaque(1.).into());
    c.fill_path(&Rect::new(130., 95., 155., 120.).to_path(0.01));
}
fn encode_classic(
    scene: &mut vello::Scene,
    image: &ImageData,
    m: &AnalyticWeld,
    size: [u32; 2],
    scale: f64,
) {
    scene.reset();
    let whole = Rect::new(0., 0., f64::from(size[0]), f64::from(size[1]));
    scene.fill(Fill::NonZero, Affine::IDENTITY, opaque(0.), None, &whole);
    scene.push_layer(
        Fill::NonZero,
        BlendMode::default(),
        0.8,
        Affine::IDENTITY,
        &whole,
    );
    scene.push_clip_layer(Fill::NonZero, Affine::scale(scale), &clip_path());
    // The registered texture has capacity dimensions; transparent bucket padding
    // is clipped to the same USED region that Hybrid's SampleRect selects.
    let [w, h] = m.pixels();
    let transform = image_transform(m, scale);
    scene.push_clip_layer(
        Fill::NonZero,
        transform,
        &Rect::new(0., 0., f64::from(w), f64::from(h)),
    );
    scene.draw_image(image, transform);
    scene.pop_layer();
    scene.pop_layer();
    scene.pop_layer();
    scene.fill(
        Fill::NonZero,
        Affine::scale(scale),
        opaque(1.),
        None,
        &Rect::new(130., 95., 155., 120.),
    );
}
fn value(args: &[String], key: &str, default: &str) -> gpu_support::Result<String> {
    match args.iter().position(|a| a == key) {
        Some(i) => args
            .get(i + 1)
            .cloned()
            .ok_or_else(|| format!("missing value for {key}").into()),
        None => Ok(default.into()),
    }
}
fn main() -> gpu_support::Result<()> {
    pollster::block_on(run())
}
async fn run() -> gpu_support::Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let backend = value(&args, "--backend", "hybrid")?;
    let case = value(&args, "--case", "morph")?;
    if !["hybrid", "classic"].contains(&backend.as_str())
        || !["static", "morph", "geometry", "resize"].contains(&case.as_str())
    {
        return Err("invalid backend/case".into());
    }
    let frames: u64 = value(&args, "--frames", "600")?.parse()?;
    let scale: f64 = value(&args, "--scale", "1")?.parse()?;
    if !(1..=100000).contains(&frames) || !scale.is_finite() || !(0.125..=8.).contains(&scale) {
        return Err("invalid frame count or scale".into());
    }
    let out = std::path::PathBuf::from(value(&args, "--out", "gpu-matrix")?);
    std::fs::create_dir_all(&out)?;
    let (info, device, queue) = gpu_support::device().await?;
    if info.device_type == wgpu::DeviceType::Cpu && !args.iter().any(|a| a == "--allow-software") {
        return Err("software adapter refused for performance ranking; pass --allow-software for diagnostic execution".into());
    }
    let tag = format!("{backend}-{case}-{scale}");
    std::fs::write(out.join(format!("{tag}-adapter.txt")),format!("{info:?}\nheadless effect/composition fixture; not presented FPS or whole-editor performance\n"))?;
    let encoding = if backend == "hybrid" {
        OutputEncoding::HybridPremultipliedSrgb
    } else {
        OutputEncoding::ClassicStraightSrgb
    };
    let mut pool = WeldTextures::new(&device, &queue, Budget::default(), encoding).await?;
    let mut size = [(384. * scale) as u32, (256. * scale) as u32];
    let mut texture = gpu_support::target(&device, size);
    let mut view = texture.create_view(&Default::default());
    let mut hybrid = if backend == "hybrid" {
        Some(vello_hybrid::Renderer::new(
            &device,
            &vello_hybrid::RenderTargetConfig {
                format: wgpu::TextureFormat::Rgba8Unorm,
                width: size[0],
                height: size[1],
            },
        ))
    } else {
        None
    };
    let mut hscene = if backend == "hybrid" {
        Some(vello_hybrid::Scene::new(size[0] as u16, size[1] as u16))
    } else {
        None
    };
    let mut classic = if backend == "classic" {
        Some(vello::Renderer::new(
            &device,
            vello::RendererOptions::default(),
        )?)
    } else {
        None
    };
    let mut cscene = if backend == "classic" {
        Some(vello::Scene::new())
    } else {
        None
    };
    let mut registered: Option<ImageData> = None;
    let mut mapping = u64::MAX;
    let mut m = spec(0., scale);
    let mut timer = GpuTimer::new(&device, &queue).ok();
    let mut gpu_times = Vec::<GpuTiming>::with_capacity(frames as usize + WARM as usize);
    let done = Arc::new(AtomicU64::new(0));
    let mut issued = 0u64;
    let mut cpu = Vec::with_capacity(frames as usize);
    #[allow(clippy::explicit_counter_loop)]
    for i in 0..frames + WARM {
        if let Some(t) = &mut timer {
            t.collect(&device, &mut gpu_times);
        }
        let gate = Instant::now();
        while issued.saturating_sub(done.load(Ordering::Acquire)) >= 3 {
            device.poll(wgpu::PollType::Poll)?;
            if gate.elapsed() > Duration::from_secs(30) {
                return Err("GPU in-flight gate timed out".into());
            }
            std::thread::sleep(Duration::from_micros(100));
        }
        let begin = Instant::now();
        let mut target_alloc = 0;
        if case == "morph" {
            m.set_morph(0.5 + 0.5 * (i as f64 * 0.11).sin())?;
        }
        if case == "geometry" {
            m = spec((i as f64 * 0.09).sin() * 18., scale);
        }
        if case == "resize" {
            size = [
                ((384. + (i % 64) as f64) * scale) as u32,
                (256. * scale) as u32,
            ];
            texture = gpu_support::target(&device, size);
            view = texture.create_view(&Default::default());
            target_alloc = 1;
        }
        let mut stats = pool.begin(std::iter::once(("join", &m)))?;
        let mut encoder = device.create_command_encoder(&Default::default());
        let ticket = timer.as_mut().and_then(|t| t.begin(&mut encoder));
        pool.encode("join", &m, &mut encoder, &mut stats)?;
        let changed = mapping != pool.mapping_revision() || case == "resize";
        if let Some(renderer) = &mut classic {
            let cscene = cscene.as_mut().expect("classic scene");
            if mapping != pool.mapping_revision() {
                if let Some(old) = registered.take() {
                    renderer.unregister_texture(old);
                }
                registered =
                    Some(renderer.register_texture(pool.texture("join").expect("texture").clone()));
            }
            let image = registered.as_ref().expect("registered");
            if stats.effect_draws > 0 {
                renderer.mark_override_image_dirty(image);
            }
            if changed {
                encode_classic(cscene, image, &m, size, scale);
                stats.encoded_scenes += 1;
            }
            queue.submit([encoder.finish()]);
            renderer.render_to_texture(
                &device,
                &queue,
                cscene,
                &view,
                &vello::RenderParams {
                    base_color: opaque(0.),
                    width: size[0],
                    height: size[1],
                    antialiasing_method: vello::AaConfig::Area,
                },
            )?;
            if let (Some(t), Some(ticket)) = (&mut timer, ticket) {
                let mut end = device.create_command_encoder(&Default::default());
                t.finish(&mut end, ticket);
                queue.submit([end.finish()]);
                t.submitted(ticket);
            }
        } else {
            let (hybrid, resources) = hybrid.as_mut().expect("Hybrid resources");
            let hscene = hscene.as_mut().expect("Hybrid scene");
            if changed {
                encode_hybrid(hscene, resources, &pool, &m, size, scale);
                stats.encoded_scenes += 1;
            }
            hybrid.render(
                hscene,
                resources,
                &device,
                &queue,
                &mut encoder,
                &vello_hybrid::RenderSize {
                    width: size[0],
                    height: size[1],
                },
                &view,
                pool.bindings(),
            )?;
            if let (Some(t), Some(ticket)) = (&mut timer, ticket) {
                t.finish(&mut encoder, ticket);
            }
            queue.submit([encoder.finish()]);
            if let (Some(t), Some(ticket)) = (&mut timer, ticket) {
                t.submitted(ticket);
            }
        }
        pool.commit_submitted(&mut stats);
        mapping = pool.mapping_revision();
        let elapsed = begin.elapsed().as_secs_f64() * 1000.;
        issued += 1;
        let completed = done.clone();
        let serial = issued;
        queue.on_submitted_work_done(move || {
            completed.fetch_max(serial, Ordering::Release);
        });
        if i >= WARM {
            cpu.push((i, elapsed, stats, target_alloc));
        }
    }
    // Explicit final drain is outside every timed CPU sample.
    device.poll(wgpu::PollType::wait_indefinitely())?;
    if let Some(t) = &mut timer {
        t.collect(&device, &mut gpu_times);
    }
    let mut file = std::io::BufWriter::new(std::fs::File::create(out.join(format!("{tag}.csv")))?);
    writeln!(file,"frame,cpu_submit_ms,gpu_queue_interval_ms,uniform_bytes,effect_draws,effect_pixels,texture_allocations,scene_encodes,target_allocations,resident_effect_bytes")?;
    for (i, elapsed, s, target_alloc) in cpu {
        let gpu = gpu_times
            .iter()
            .find(|g| g.frame == i)
            .map(|g| format!("{:.9}", g.milliseconds))
            .unwrap_or_default();
        writeln!(
            file,
            "{i},{elapsed:.9},{gpu},{},{},{},{},{},{target_alloc},{}",
            s.uniform_upload_bytes,
            s.effect_draws,
            s.effect_pixels,
            s.texture_allocations,
            s.encoded_scenes,
            s.resident_texture_bytes
        )?;
    }
    if let Some(t) = timer {
        println!(
            "timestamp dropped={} failed={}",
            t.dropped_samples, t.failed_samples
        );
    } else {
        println!("timestamp features unavailable; CPU samples only");
    }
    let pixels = gpu_support::readback(&device, &queue, &texture, size)?;
    gpu_support::save(&out.join(format!("{tag}.png")), size, &pixels)?;
    if let (Some(renderer), Some(image)) = (&mut classic, registered) {
        renderer.unregister_texture(image);
    }
    println!("WROTE {}", out.join(format!("{tag}.csv")).display());
    Ok(())
}
