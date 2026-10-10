use anyhow::Result;
use classic::{
    kurbo::{Affine, Stroke},
    peniko::Fill,
};

pub struct Renderer {
    gpu: crate::gpu::Gpu,
    renderer: classic::Renderer,
    width: u32,
    height: u32,
}
impl Renderer {
    pub async fn new(width: u32, height: u32) -> Result<Self> {
        let gpu = crate::gpu::Gpu::new(width, height).await?;
        let renderer = classic::Renderer::new(
            &gpu.device,
            classic::RendererOptions {
                antialiasing_support: classic::AaSupport::area_only(),
                ..Default::default()
            },
        )?;
        Ok(Self {
            gpu,
            renderer,
            width,
            height,
        })
    }
    pub fn draw(&mut self, shapes: &[crate::Shape]) -> Result<Vec<u8>> {
        let mut scene = classic::Scene::new();
        for shape in shapes {
            let [r, g, b, a] = shape.color();
            let color = classic::peniko::Color::from_rgba8(r, g, b, a);
            match shape {
                crate::Shape::Rect { x, y, w, h, .. } => scene.fill(
                    Fill::NonZero,
                    Affine::IDENTITY,
                    color,
                    None,
                    &classic::kurbo::Rect::new(*x, *y, x + w, y + h),
                ),
                crate::Shape::Path { points, width, .. } => scene.stroke(
                    &Stroke::new(*width),
                    Affine::IDENTITY,
                    color,
                    None,
                    &crate::path(points),
                ),
            }
        }
        self.renderer.render_to_texture(
            &self.gpu.device,
            &self.gpu.queue,
            &scene,
            &self.gpu.view,
            &classic::RenderParams {
                base_color: classic::peniko::Color::BLACK,
                width: self.width,
                height: self.height,
                antialiasing_method: classic::AaConfig::Area,
            },
        )?;
        self.gpu.readback()
    }
}
