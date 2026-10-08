use anyhow::{Context, Result, ensure};
use vello_gpu::color::{AlphaColor, Srgb};
use vello_gpu::kurbo::{BezPath, Rect, Stroke};

pub struct Renderer {
    renderer: vello_gpu::Renderer,
    resources: vello_gpu::Resources,
    gpu: crate::gpu::Gpu,
    size: vello_gpu::RenderSize,
}

impl Renderer {
    pub async fn new(width: u32, height: u32) -> Result<Self> {
        ensure!(
            width > 0 && height > 0,
            "render dimensions must be positive"
        );
        let size = vello_gpu::RenderSize {
            width: width.try_into().context("Vello GPU width exceeds u16")?,
            height: height.try_into().context("Vello GPU height exceeds u16")?,
        };
        let gpu = crate::gpu::Gpu::new(width, height).await?;
        let (renderer, resources) = vello_gpu::Renderer::new(
            &gpu.device,
            &vello_gpu::RenderTargetConfig {
                format: wgpu::TextureFormat::Rgba8Unorm,
                width: size.width,
                height: size.height,
            },
        );
        Ok(Self {
            renderer,
            resources,
            gpu,
            size,
        })
    }

    pub fn draw(&mut self, shapes: &[crate::Shape]) -> Result<Vec<u8>> {
        let mut scene = vello_gpu::Scene::new(self.size.width, self.size.height);
        for shape in shapes {
            match shape {
                crate::Shape::Rect { x, y, w, h, color } => {
                    scene.set_paint(rgba(*color));
                    scene.fill_rect(&Rect::new(*x, *y, *x + *w, *y + *h));
                }
                crate::Shape::Path {
                    points,
                    width,
                    color,
                } => {
                    if let Some(first) = points.first() {
                        let mut path = BezPath::new();
                        path.move_to((first[0], first[1]));
                        for point in &points[1..] {
                            path.line_to((point[0], point[1]));
                        }
                        scene.set_paint(rgba(*color));
                        scene.set_stroke(Stroke::new(*width));
                        scene.stroke_path(&path);
                    }
                }
            }
        }

        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        self.renderer.render(
            &scene,
            &mut self.resources,
            &self.gpu.device,
            &self.gpu.queue,
            &mut encoder,
            &self.size,
            &self.gpu.view,
            None,
            &vello_gpu::TextureBindings::new(),
            vello_gpu::TargetInit::Clear(vello_gpu::ClearSettings::Viewport {
                color: rgba([0, 0, 0, 255]),
            }),
        )?;
        self.gpu.queue.submit([encoder.finish()]);
        self.gpu.readback()
    }
}

fn rgba([r, g, b, a]: [u8; 4]) -> AlphaColor<Srgb> {
    AlphaColor::from_rgba8(r, g, b, a)
}
