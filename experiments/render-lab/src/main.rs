mod embedded;
mod fixtures;
mod geometry;
mod glass;
use fixtures::*;
use std::{error::Error, fs, path::Path, sync::Arc, time::Instant};
use vello::{
    kurbo::{Affine, BezPath, PathEl, Rect, Shape},
    peniko::{Color, Fill, Gradient},
    wgpu,
};
type Result<T> = std::result::Result<T, Box<dyn Error>>;
fn color(c: [u8; 4]) -> Color {
    Color::from_rgba8(c[0], c[1], c[2], c[3])
}
fn vscene(marks: &[Mark], scale: f64) -> Result<vello::Scene> {
    let mut scene = vello::Scene::new();
    for m in marks {
        let p = BezPath::from_svg(&m.path)?;
        if let Some(r) = m.clip {
            scene.push_clip_layer(
                Fill::NonZero,
                Affine::scale(scale),
                &Rect::new(r[0], r[1], r[2], r[3]),
            );
        }
        if let Some(w) = m.stroke {
            scene.stroke(
                &vello::kurbo::Stroke::new(w),
                Affine::scale(scale),
                color(m.color),
                None,
                &p,
            );
        } else if let Some(end) = m.gradient {
            let r = p.bounding_box();
            let endpoint = if m.vertical_gradient {
                (r.x0, r.y1)
            } else {
                (r.x1, r.y0)
            };
            let g = Gradient::new_linear((r.x0, r.y0), endpoint)
                .with_interpolation_cs(if m.oklab {
                    vello::peniko::color::ColorSpaceTag::Oklab
                } else {
                    vello::peniko::color::ColorSpaceTag::Srgb
                })
                .with_stops([color(m.color), color(end)]);
            scene.fill(Fill::NonZero, Affine::scale(scale), &g, None, &p);
        } else {
            scene.fill(
                Fill::NonZero,
                Affine::scale(scale),
                color(m.color),
                None,
                &p,
            );
        }
        if m.clip.is_some() {
            scene.pop_layer();
        }
    }
    Ok(scene)
}
fn gscene(marks: &[Mark], scale: f32, tight: bool) -> Result<gpui::Scene> {
    use gpui::{point, px, size};
    let mut scene = gpui::Scene::default();
    for m in marks {
        let p = BezPath::from_svg(&m.path)?;
        let c = |v: [u8; 4]| gpui::Rgba {
            r: v[0] as f32 / 255.,
            g: v[1] as f32 / 255.,
            b: v[2] as f32 / 255.,
            a: v[3] as f32 / 255.,
        };
        let bg = if let Some(end) = m.gradient {
            gpui::linear_gradient(
                if m.vertical_gradient { 180. } else { 90. },
                gpui::linear_color_stop(c(m.color), 0.),
                gpui::linear_color_stop(c(end), 1.),
            )
            .color_space(if m.oklab {
                gpui::ColorSpace::Oklab
            } else {
                gpui::ColorSpace::Srgb
            })
        } else {
            c(m.color).into()
        };
        let r = m.clip.unwrap_or([0., 0., WIDTH as f64, HEIGHT as f64]);
        let mask = gpui::ContentMask {
            bounds: gpui::Bounds {
                origin: point(px(r[0] as f32), px(r[1] as f32)),
                size: size(px((r[2] - r[0]) as f32), px((r[3] - r[1]) as f32)),
            },
        };
        if let Some(mut r) = m.native_rect {
            let stroke = m.stroke.unwrap_or(0.) as f32;
            let half = f64::from(stroke) / 2.;
            r = [r[0] - half, r[1] - half, r[2] + half, r[3] + half];
            scene.insert_primitive(gpui::Quad {
                bounds: gpui::Bounds {
                    origin: point(px(r[0] as f32), px(r[1] as f32)),
                    size: size(px((r[2] - r[0]) as f32), px((r[3] - r[1]) as f32)),
                }
                .scale(scale),
                content_mask: mask.scale(scale),
                background: if m.stroke.is_some() {
                    gpui::transparent_black().into()
                } else {
                    bg
                },
                corner_radii: gpui::Corners::all(px(m.radius as f32 + stroke / 2.)).scale(scale),
                border_widths: gpui::Edges::all(px(stroke)).scale(scale),
                border_color: c(m.color).into(),
                ..Default::default()
            });
            continue;
        }
        let tolerance = if tight { 0.01 / scale } else { 0.1 };
        let mut builder = if let Some(w) = m.stroke {
            gpui::PathBuilder::stroke(px(w as f32)).with_style(gpui::PathStyle::Stroke(
                gpui::StrokeOptions::default()
                    .with_line_width(w as f32)
                    .with_tolerance(tolerance),
            ))
        } else {
            gpui::PathBuilder::fill().with_style(gpui::PathStyle::Fill(
                gpui::FillOptions::default()
                    .with_fill_rule(gpui::FillRule::NonZero)
                    .with_tolerance(tolerance),
            ))
        };
        let pt = |p: vello::kurbo::Point| point(px(p.x as f32), px(p.y as f32));
        for e in p.elements() {
            match *e {
                PathEl::MoveTo(a) => builder.move_to(pt(a)),
                PathEl::LineTo(a) => builder.line_to(pt(a)),
                PathEl::QuadTo(a, b) => builder.curve_to(pt(b), pt(a)),
                PathEl::CurveTo(a, b, c) => builder.cubic_bezier_to(pt(c), pt(a), pt(b)),
                PathEl::ClosePath => builder.close(),
            }
        }
        let mut path = builder.build()?;
        path.color = bg;
        path.content_mask = mask;
        scene.insert_primitive(path.scale(scale));
    }
    scene.finish();
    Ok(scene)
}
fn png(path: &Path, w: u32, h: u32, data: &[u8]) -> Result<()> {
    let mut enc = png::Encoder::new(fs::File::create(path)?, w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()?.write_image_data(data)?;
    Ok(())
}
fn readback(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    t: &wgpu::Texture,
    w: u32,
    h: u32,
) -> Result<Vec<u8>> {
    let pitch = (w * 4).div_ceil(256) * 256;
    let b = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: (pitch * h) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut e = device.create_command_encoder(&Default::default());
    e.copy_texture_to_buffer(
        t.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &b,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(pitch),
                rows_per_image: Some(h),
            },
        },
        wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
    );
    queue.submit([e.finish()]);
    let (tx, rx) = std::sync::mpsc::channel();
    b.slice(..)
        .map_async(wgpu::MapMode::Read, move |r| tx.send(r).unwrap());
    device.poll(wgpu::PollType::wait_indefinitely())?;
    rx.recv()??;
    let mapped = b.slice(..).get_mapped_range();
    let data = mapped
        .chunks(pitch as usize)
        .flat_map(|r| r[..(w * 4) as usize].iter().copied())
        .collect();
    drop(mapped);
    b.unmap();
    Ok(data)
}
fn stats(v: &mut [f64]) -> serde_json::Value {
    v.sort_by(f64::total_cmp);
    serde_json::json!({"samples":v.len(),"median_ms":v[v.len()/2],"p95_ms":v[((v.len()-1)as f64*0.95).ceil()as usize],"min_ms":v[0],"max_ms":v[v.len()-1]})
}
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    let mode = args.get(1).map(String::as_str).unwrap_or("vello");
    if mode == "glass" {
        return glass::run(
            Path::new(args.get(2).ok_or("input PNG required")?),
            Path::new(args.get(3).ok_or("output PNG required")?),
        );
    }
    let scale: f64 = args.get(2).map(String::as_str).unwrap_or("1").parse()?;
    let samples: usize = args.get(3).map(String::as_str).unwrap_or("30").parse()?;
    if !(0.5..=4.).contains(&scale) || samples == 0 || samples > 10000 {
        return Err("invalid scale/samples".into());
    }
    let out = std::path::PathBuf::from(args.get(4).map(String::as_str).unwrap_or("output"));
    fs::create_dir_all(&out)?;
    if mode == "embedding" {
        return embedded::run(&out);
    }
    if mode == "geometry" {
        return geometry::run(&out);
    }
    let side: usize = args.get(5).map(String::as_str).unwrap_or("1").parse()?;
    let component_fixture = std::env::var_os("MUI_LAB_COMPONENTS").is_some();
    let marks = tiled(
        &if component_fixture {
            components()?
        } else {
            fixture()?
        },
        side,
    )?;
    fs::write(out.join("fixture.json"), serde_json::to_vec_pretty(&marks)?)?;
    let (w, h) = (
        (WIDTH as f64 * scale) as u32,
        (HEIGHT as f64 * scale) as u32,
    );
    let mut encoding = Vec::new();
    for _ in 0..samples {
        let t = Instant::now();
        if mode.starts_with("gpui") {
            std::hint::black_box(gscene(&marks, scale as f32, mode == "gpui-tight")?);
        } else {
            std::hint::black_box(vscene(&marks, scale)?);
        }
        encoding.push(t.elapsed().as_secs_f64() * 1000.);
    }
    let start = Instant::now();
    let mut times = Vec::new();
    let adapter;
    let first;
    let init;
    if mode.starts_with("gpui") {
        #[allow(deprecated)]
        let event_loop = winit::event_loop::EventLoop::new()?;
        #[allow(deprecated)]
        let window = Arc::new(
            event_loop.create_window(
                winit::window::Window::default_attributes()
                    .with_title("MUI GPUI renderer lab")
                    .with_inner_size(winit::dpi::PhysicalSize::new(w, h)),
            )?,
        );
        let cx: gpui_wgpu::GpuContext = Default::default();
        let mut renderer = gpui_wgpu::WgpuRenderer::new(
            cx.clone(),
            &window,
            gpui_wgpu::WgpuSurfaceConfig {
                size: gpui::size(gpui::DevicePixels(w as i32), gpui::DevicePixels(h as i32)),
                transparent: false,
                preferred_present_mode: Some(wgpu::PresentMode::Immediate),
            },
            None,
        )?;
        let context = cx.borrow();
        let context = context.as_ref().unwrap();
        adapter = format!("{:?}", context.adapter.get_info());
        let scene = gscene(&marks, scale as f32, mode == "gpui-tight")?;
        init = start.elapsed().as_secs_f64() * 1000.;
        for i in 0..samples + 6 {
            let t = Instant::now();
            if !renderer.draw(&scene) {
                return Err("GPUI draw did not present".into());
            }
            context.device.poll(wgpu::PollType::wait_indefinitely())?;
            let ms = t.elapsed().as_secs_f64() * 1000.;
            if i == 0 || i >= 6 {
                times.push(ms)
            }
        }
        first = times.remove(0);
        let raw = out.join(format!("{mode}-{scale}.rgba"));
        std::env::set_var("MUI_LAB_CAPTURE", &raw);
        if !renderer.draw(&scene) {
            return Err("capture failed".into());
        }
        std::env::remove_var("MUI_LAB_CAPTURE");
        let mut data = fs::read(&raw)?;
        let format = fs::read_to_string(format!("{}.format", raw.display()))?;
        if format.starts_with("Bgra") {
            for p in data.chunks_mut(4) {
                p.swap(0, 2)
            }
        }
        png(&out.join(format!("{mode}-{scale}.png")), w, h, &data)?;
    } else {
        let events = if mode.ends_with("-present") {
            Some(winit::event_loop::EventLoop::new()?)
        } else {
            None
        };
        #[allow(deprecated)]
        let window = events
            .as_ref()
            .map(|e| {
                e.create_window(
                    winit::window::Window::default_attributes()
                        .with_inner_size(winit::dpi::PhysicalSize::new(w, h)),
                )
            })
            .transpose()?
            .map(Arc::new);
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::VULKAN,
            flags: Default::default(),
            backend_options: Default::default(),
            memory_budget_thresholds: Default::default(),
            display: window.clone().map(|w| Box::new(w) as _),
        });
        let surface = window.map(|w| instance.create_surface(w)).transpose()?;
        let a = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: surface.as_ref(),
            ..Default::default()
        }))?;
        adapter = format!("{:?}", a.get_info());
        let (device, queue) = pollster::block_on(a.request_device(&wgpu::DeviceDescriptor {
            required_limits: a.limits(),
            ..Default::default()
        }))?;
        let blitter = if let Some(surface) = &surface {
            let caps = surface.get_capabilities(&a);
            let format = caps
                .formats
                .iter()
                .copied()
                .find(|f| *f == wgpu::TextureFormat::Bgra8Unorm)
                .ok_or("Bgra8Unorm presentation unavailable")?;
            surface.configure(
                &device,
                &wgpu::SurfaceConfiguration {
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    format,
                    width: w,
                    height: h,
                    present_mode: if caps.present_modes.contains(&wgpu::PresentMode::Immediate) {
                        wgpu::PresentMode::Immediate
                    } else {
                        wgpu::PresentMode::Fifo
                    },
                    desired_maximum_frame_latency: 2,
                    alpha_mode: wgpu::CompositeAlphaMode::Opaque,
                    view_formats: vec![],
                },
            );
            Some(wgpu::util::TextureBlitter::new(&device, format))
        } else {
            None
        };
        let mut renderer = vello::Renderer::new(&device, vello::RendererOptions::default())?;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("MUI Vello output"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::STORAGE_BINDING
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let scene = vscene(&marks, scale)?;
        let aa = match mode {
            "vello-area" | "vello-area-present" => vello::AaConfig::Area,
            "vello-msaa8" => vello::AaConfig::Msaa8,
            _ => vello::AaConfig::Msaa16,
        };
        let params = vello::RenderParams {
            base_color: Color::TRANSPARENT,
            width: w,
            height: h,
            antialiasing_method: aa,
        };
        init = start.elapsed().as_secs_f64() * 1000.;
        for i in 0..samples + 6 {
            let t = Instant::now();
            let frame = if let Some(surface) = &surface {
                match surface.get_current_texture() {
                    wgpu::CurrentSurfaceTexture::Success(f) => Some(f),
                    _ => return Err("Vello presentation surface unavailable".into()),
                }
            } else {
                None
            };
            renderer.render_to_texture(&device, &queue, &scene, &view, &params)?;
            if let Some(frame) = frame {
                let mut encoder = device.create_command_encoder(&Default::default());
                blitter.as_ref().unwrap().copy(
                    &device,
                    &mut encoder,
                    &view,
                    &frame.texture.create_view(&Default::default()),
                );
                queue.submit([encoder.finish()]);
                frame.present();
            }
            device.poll(wgpu::PollType::wait_indefinitely())?;
            let ms = t.elapsed().as_secs_f64() * 1000.;
            if i == 0 || i >= 6 {
                times.push(ms)
            }
        }
        first = times.remove(0);
        let data = readback(&device, &queue, &texture, w, h)?;
        png(&out.join(format!("{mode}-{scale}.png")), w, h, &data)?;
    }
    let report = serde_json::json!({"backend":mode,"scale":scale,"width":w,"height":h,"adapter":adapter,"initialization_ms":init,"first_draw_ms":first,"scene_encoding":stats(&mut encoding),"steady_draw_wait":stats(&mut times),"timing_scope":if mode.starts_with("gpui"){"surface acquire + draw + present + device wait"}else if mode.ends_with("-present"){"surface acquire + offscreen draw + blit + present + device wait"}else{"offscreen draw + device wait"},"capture_excluded":true,"shared_geometry":true,"marks":marks.len(),"tile_side":side,"warmup_frames":5});
    fs::write(
        out.join(format!("{mode}-{scale}.json")),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    Ok(())
}
