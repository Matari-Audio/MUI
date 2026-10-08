use anyhow::{Result, ensure};
use skia_safe::{
    AlphaType, Color4f, ColorType, ImageInfo, Paint, PathBuilder, Rect, Surface, paint::Style,
};

pub struct Renderer {
    surface: Surface,
    info: ImageInfo,
    width: usize,
    height: usize,
}

impl Renderer {
    pub fn new(width: u32, height: u32) -> Result<Self> {
        let info = ImageInfo::new(
            (width as i32, height as i32),
            ColorType::RGBA8888,
            AlphaType::Premul,
            None,
        );
        let surface = skia_safe::surfaces::raster(&info, None, None)
            .ok_or_else(|| anyhow::anyhow!("Skia surface allocation"))?;
        Ok(Self {
            surface,
            info,
            width: width as usize,
            height: height as usize,
        })
    }
    pub fn draw(&mut self, shapes: &[crate::Shape]) -> Result<Vec<u8>> {
        let canvas = self.surface.canvas();
        canvas.clear(skia_safe::Color::BLACK);
        let mut paint = Paint::default();
        paint.set_anti_alias(true);
        for shape in shapes {
            let color = shape.color();
            paint.set_color4f(
                Color4f::new(
                    color[0] as f32 / 255.,
                    color[1] as f32 / 255.,
                    color[2] as f32 / 255.,
                    color[3] as f32 / 255.,
                ),
                None,
            );
            match shape {
                crate::Shape::Rect { x, y, w, h, .. } => {
                    paint.set_style(Style::Fill);
                    canvas.draw_rect(
                        Rect::from_xywh(*x as f32, *y as f32, *w as f32, *h as f32),
                        &paint,
                    );
                }
                crate::Shape::Path { points, width, .. } => {
                    let mut path = PathBuilder::new();
                    path.move_to((points[0][0] as f32, points[0][1] as f32));
                    for p in &points[1..] {
                        path.line_to((p[0] as f32, p[1] as f32));
                    }
                    paint
                        .set_style(Style::Stroke)
                        .set_stroke_width(*width as f32);
                    canvas.draw_path(&path.detach(), &paint);
                }
            }
        }
        let mut pixels = vec![0; self.width * self.height * 4];
        ensure!(
            self.surface
                .read_pixels(&self.info, &mut pixels, self.width * 4, (0, 0)),
            "Skia readback failed"
        );
        Ok(pixels)
    }
}
