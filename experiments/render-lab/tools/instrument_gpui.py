"""Instrument pinned upstream for readback only; never change drawing/shader code."""
from pathlib import Path
p = Path(__file__).resolve().parents[2] / 'upstream/crates/gpui_wgpu/src/wgpu_renderer.rs'
s = p.read_text()
if '// MUI LAB READBACK' in s:
    raise SystemExit('already instrumented')
old = 'usage: wgpu::TextureUsages::RENDER_ATTACHMENT,\n            format: surface_format,'
assert s.count(old) == 1
s = s.replace(old, 'usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,\n            format: surface_format,')
old = '        frame.present();\n        true'
assert s.count(old) == 1
s = s.replace(old, '''        // MUI LAB READBACK: diagnostic capture outside benchmark samples.
        if let Ok(path) = std::env::var("MUI_LAB_CAPTURE") {
            let r = self.resources();
            let width = self.surface_config.width;
            let height = self.surface_config.height;
            let pitch = (width * 4).div_ceil(256) * 256;
            let buffer = r.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("MUI diagnostic readback"), size: u64::from(pitch * height),
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            let mut encoder = r.device.create_command_encoder(&Default::default());
            encoder.copy_texture_to_buffer(
                frame.texture.as_image_copy(),
                wgpu::TexelCopyBufferInfo { buffer: &buffer, layout: wgpu::TexelCopyBufferLayout {
                    offset: 0, bytes_per_row: Some(pitch), rows_per_image: Some(height),
                } }, wgpu::Extent3d { width, height, depth_or_array_layers: 1 });
            r.queue.submit([encoder.finish()]);
            let (tx, rx) = std::sync::mpsc::channel();
            buffer.slice(..).map_async(wgpu::MapMode::Read, move |result| { tx.send(result).unwrap(); });
            r.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
            rx.recv().unwrap().unwrap();
            let view = buffer.slice(..).get_mapped_range();
            let mut pixels = Vec::new();
            for row in view.chunks(pitch as usize) { pixels.extend_from_slice(&row[..(width * 4) as usize]); }
            std::fs::write(&path, pixels).unwrap();
            std::fs::write(format!("{path}.format"), format!("{:?}", self.surface_config.format)).unwrap();
            drop(view); buffer.unmap();
        }
        frame.present();
        true''')
p.write_text(s)
print('Instrumented pinned GPUI: COPY_SRC surface usage + optional readback; shaders unchanged.')
