use anyhow::Result;
use vello_cpu::{
    Pixmap, RenderContext, Resources,
    kurbo::{Rect, Stroke},
};

pub struct Renderer {
    context: RenderContext,
    resources: Resources,
    target: Pixmap,
}
impl Renderer {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            context: RenderContext::new(width as u16, height as u16),
            resources: Resources::new(),
            target: Pixmap::new(width as u16, height as u16),
        }
    }
    pub fn draw(&mut self, shapes: &[crate::Shape]) -> Result<Vec<u8>> {
        self.context.reset();
        self.context
            .set_paint(vello_cpu::color::palette::css::BLACK);
        self.context.fill_rect(&Rect::new(
            0.,
            0.,
            f64::from(self.target.width()),
            f64::from(self.target.height()),
        ));
        for shape in shapes {
            let [r, g, b, a] = shape.color();
            self.context
                .set_paint(vello_cpu::peniko::Color::from_rgba8(r, g, b, a));
            match shape {
                crate::Shape::Rect { x, y, w, h, .. } => {
                    self.context.fill_rect(&Rect::new(*x, *y, x + w, y + h))
                }
                crate::Shape::Path { points, width, .. } => {
                    self.context.set_stroke(Stroke::new(*width));
                    self.context.stroke_path(&crate::path(points));
                }
            }
        }
        self.context.flush();
        self.context.render(&mut self.target, &mut self.resources);
        Ok(self.target.data_as_u8_slice().to_vec())
    }
}
