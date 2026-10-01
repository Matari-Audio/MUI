//! Milliseconds per pass of the hybrid frame at 1080p: the stage's raster
//! frame, one traced sample per pixel (with the filter), and the glass
//! composited over it. 64 printed glass slabs under a sky, as a glassified
//! plugin UI is; the camera moves every frame, so no frame is helped by
//! the ones before it.
//!
//!     cargo run --release -p mui-stage-rt --example bench [-- FRAMES]
use mui_stage::{Camera, Light, LightKind, Material, Plane, Post, Shot, Sky};
use std::time::Instant;

fn main() {
    let frames: usize = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(120);
    let (w, h) = (1920, 1080);
    let (mut stage, mut rt) = mui_stage_rt::open(w, h).expect("a ray-query adapter");
    // Glyph-like ink: thin light strokes on clear, over a faint fill.
    let size = [1024u32, 1024];
    let tex = stage
        .layer_target("ui", size, wgpu::TextureFormat::Rgba8Unorm, 1)
        .unwrap();
    let data: Vec<u8> = (0..size[1])
        .flat_map(|y| (0..size[0]).map(move |x| (x, y)))
        .flat_map(|(x, y)| {
            if (x / 3) % 5 == 0 && (y / 24) % 3 == 0 {
                [235, 160, 60, 255]
            } else {
                [40, 50, 70, 40]
            }
        })
        .collect();
    rt_write(&rt, &tex, &data, size);
    stage.layer_done("ui").unwrap();
    rt.layer("ui", &tex);

    let glass = Material {
        transmission: 1.,
        ior: 1.5,
        roughness: 0.04,
        dispersion: 0.3,
        print: 1.,
        bevel: 6.,
        ..Material::SLAB
    };
    let planes: Vec<Plane> = (0..64)
        .map(|i| {
            let (x, y) = ((i % 8) as f32 - 3.5, (i / 8) as f32 - 3.5);
            Plane::new("ui", 210., 110.)
                .at(x * 240., y * 130., (i % 3) as f32 * -60.)
                .depth(12.)
                .material(glass)
        })
        .collect();
    let mut sun = Light::new(LightKind::Directional);
    sun.direction = [-0.4, -0.7, -0.6];
    sun.color = [3., 2.9, 2.7];
    let sky = Sky {
        sun: [0.4, 0.7, 0.6],
        zenith: [0.1, 0.25, 0.6],
        horizon: [0.6, 0.7, 0.85],
        sun_color: [2.6, 2.5, 2.3],
        cover: 0.5,
        drift: [0., 0.],
    };
    let target = stage_target(&rt, w, h);
    let view = target.create_view(&wgpu::TextureViewDescriptor::default());
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut ms = [Vec::new(), Vec::new(), Vec::new(), Vec::new()];
    for f in 0..frames + 10 {
        let yaw = (f as f32 * 0.2).sin() * 12.;
        let shot = Shot {
            planes: planes.clone(),
            lights: vec![sun],
            sky: Some(sky),
            post: Post::NONE,
            ..Shot::new(Camera::front(1100., 40.).orbit(yaw, 8.))
        };
        let t0 = Instant::now();
        stage.draw(&shot, 0., &view, format).unwrap();
        rt.finish().unwrap();
        let t1 = Instant::now();
        rt.trace(&shot, 1).unwrap();
        rt.finish().unwrap();
        let t2 = Instant::now();
        rt.composite(&shot, &view, format);
        rt.finish().unwrap();
        let t3 = Instant::now();
        if f >= 10 {
            for (k, d) in [t1 - t0, t2 - t1, t3 - t2, t3 - t0].into_iter().enumerate() {
                ms[k].push(d.as_secs_f64() * 1e3);
            }
        }
    }
    for (name, v) in ["raster", "trace 1 spp + filter", "composite", "frame"]
        .iter()
        .zip(&mut ms)
    {
        v.sort_by(f64::total_cmp);
        println!(
            "{name:>22}: p50 {:6.2} ms  p95 {:6.2} ms",
            v[v.len() / 2],
            v[v.len() * 95 / 100]
        );
    }
    let p50 = ms[3][ms[3].len() / 2];
    println!(
        "{:>22}: {:.1} fps at p50, {w}x{h}, {frames} frames",
        "hybrid",
        1e3 / p50
    );
}

fn rt_write(rt: &mui_stage_rt::Rt, tex: &wgpu::Texture, data: &[u8], size: [u32; 2]) {
    rt.queue().write_texture(
        tex.as_image_copy(),
        data,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(size[0] * 4),
            rows_per_image: None,
        },
        tex.size(),
    );
}

fn stage_target(rt: &mui_stage_rt::Rt, w: u32, h: u32) -> wgpu::Texture {
    rt.device().create_texture(&wgpu::TextureDescriptor {
        label: Some("bench frame"),
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    })
}
