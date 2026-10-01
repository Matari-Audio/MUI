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
    const ROWS: [&str; 6] = [
        "raster (bracketed)",
        "TLAS build",
        "trace 1 spp",
        "filter",
        "composite",
        "frame (CPU wall)",
    ];
    let frames: usize = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(120);
    let (w, h) = (1920, 1080);
    let adapter = mui_stage_rt::probe().expect("a ray-query adapter");
    // GPU timestamps between the passes, so a busy CPU does not count.
    let stamps = wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS;
    let mut desc = mui_stage_rt::device_descriptor(&adapter);
    desc.required_features |= stamps;
    let (device, queue) = pollster::block_on(adapter.request_device(&desc)).unwrap();
    let mut rt = mui_stage_rt::Rt::new(&device, &queue, w, h).unwrap();
    let mut stage = mui_stage::Stage::with_device(device.clone(), queue.clone(), w, h).unwrap();
    let set = device.create_query_set(&wgpu::QuerySetDescriptor {
        label: None,
        ty: wgpu::QueryType::Timestamp,
        count: 2,
    });
    let resolve = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 16,
        usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let read = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 16,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let period = f64::from(queue.get_timestamp_period()) * 1e-6;
    let stamp = |i: u32| {
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        enc.write_timestamp(&set, i);
        if i == 1 {
            enc.resolve_query_set(&set, 0..2, &resolve, 0);
            enc.copy_buffer_to_buffer(&resolve, 0, &read, 0, 16);
        }
        queue.submit([enc.finish()]);
    };
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
    let mut ms: [Vec<f64>; 6] = Default::default();
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
        stamp(0);
        stage.draw(&shot, 0., &view, format).unwrap();
        stamp(1);
        rt.trace(&shot, 0., 1).unwrap();
        rt.composite(&shot, &view, format);
        let g = rt.gpu_times().expect("timestamp queries");
        let wall = t0.elapsed().as_secs_f64() * 1e3;
        read.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        rt.finish().unwrap();
        let t: Vec<u64> =
            bytemuck::cast_slice(&read.slice(..).get_mapped_range().unwrap()).to_vec();
        read.unmap();
        if f >= 10 {
            let row = [
                (t[1] - t[0]) as f64 * period,
                g.tlas,
                g.trace,
                g.filter,
                g.composite,
                wall,
            ];
            for (v, x) in ms.iter_mut().zip(row) {
                v.push(x);
            }
        }
    }
    let mut p50 = [0.; 6];
    for (k, v) in ms.iter_mut().enumerate() {
        v.sort_by(f64::total_cmp);
        p50[k] = v[v.len() / 2];
        println!(
            "{:>20}: p50 {:6.2} ms  p95 {:6.2} ms",
            ROWS[k],
            p50[k],
            v[v.len() * 95 / 100]
        );
    }
    let gpu: f64 = p50[..5].iter().sum();
    println!(
        "{w}x{h}, {frames} frames: {gpu:.1} ms GPU a frame at p50, {:.0} fps",
        1e3 / gpu
    );
    println!(
        "(the raster row is timestamps either side of Stage::draw's submissions: CPU gaps between them count)"
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
