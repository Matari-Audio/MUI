use crate::{png, readback, stats, Result};
use std::{fs, path::Path, time::Instant};
use vello::wgpu;
/// Shared postprocess on captured frames, NOT a native GPUI element hook.
pub fn run(input: &Path, output: &Path) -> Result<()> {
    let decoder = png::Decoder::new(std::io::BufReader::new(fs::File::open(input)?));
    let mut reader = decoder.read_info()?;
    let mut pixels = vec![0; reader.output_buffer_size().ok_or("PNG buffer size")?];
    let info = reader.next_frame(&mut pixels)?;
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return Err("expected RGBA8 PNG".into());
    }
    let (w, h) = (info.width, info.height);
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::VULKAN,
        flags: Default::default(),
        backend_options: Default::default(),
        memory_budget_thresholds: Default::default(),
        display: None,
    });
    let a = pollster::block_on(instance.request_adapter(&Default::default()))?;
    let (device, queue) = pollster::block_on(a.request_device(&Default::default()))?;
    let texture = |usage| {
        device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage,
            view_formats: &[],
        })
    };
    let source = texture(wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST);
    let target = texture(wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC);
    queue.write_texture(
        source.as_image_copy(),
        &pixels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(w * 4),
            rows_per_image: Some(h),
        },
        wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
    );
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("shared glass prototype"),
        source: wgpu::ShaderSource::Wgsl(include_str!("glass.wgsl").into()),
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: None,
        module: &module,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    });
    let sv = source.create_view(&Default::default());
    let tv = target.create_view(&Default::default());
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&sv),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&tv),
            },
        ],
    });
    let mut times = Vec::new();
    for i in 0..13 {
        let t = Instant::now();
        let mut e = device.create_command_encoder(&Default::default());
        {
            let mut pass = e.begin_compute_pass(&Default::default());
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.dispatch_workgroups(w.div_ceil(8), h.div_ceil(8), 1);
        }
        queue.submit([e.finish()]);
        device.poll(wgpu::PollType::wait_indefinitely())?;
        if i >= 3 {
            times.push(t.elapsed().as_secs_f64() * 1000.)
        }
    }
    png(output, w, h, &readback(&device, &queue, &target, w, h)?)?;
    let report = serde_json::json!({"adapter":format!("{:?}",a.get_info()),"input":input,"width":w,"height":h,"glass_compute_and_wait":stats(&mut times),"scope":"same WGSL on captured backend image; excludes upload/readback and does not prove GPUI runtime shader integration"});
    fs::write(
        output.with_extension("json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    Ok(())
}
