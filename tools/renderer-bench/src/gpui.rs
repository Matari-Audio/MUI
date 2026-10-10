use gpui::{
    Bounds, ContentMask, DevicePixels, PathBuilder, PlatformHeadlessRenderer, Quad, ScaledPixels,
    Scene, Size, point, px, rgba, size,
};
use gpui_wgpu::WgpuHeadlessRenderer;

pub struct Renderer {
    renderer: WgpuHeadlessRenderer,
    size: Size<DevicePixels>,
    mask: ContentMask<ScaledPixels>,
}

impl Renderer {
    pub fn new(width: u32, height: u32) -> anyhow::Result<Self> {
        anyhow::ensure!(
            width > 0 && height > 0,
            "render dimensions must be positive"
        );
        Ok(Self {
            renderer: WgpuHeadlessRenderer::new()?,
            size: size(
                DevicePixels(width.try_into()?),
                DevicePixels(height.try_into()?),
            ),
            mask: ContentMask {
                bounds: Bounds {
                    origin: point(ScaledPixels(0.0), ScaledPixels(0.0)),
                    size: size(ScaledPixels(width as f32), ScaledPixels(height as f32)),
                },
            },
        })
    }

    pub fn draw(&mut self, shapes: &[crate::Shape]) -> anyhow::Result<Vec<u8>> {
        let mut scene = Scene::default();
        for shape in shapes {
            match shape {
                crate::Shape::Rect { x, y, w, h, color } => {
                    scene.insert_primitive(Quad {
                        bounds: Bounds {
                            origin: point(ScaledPixels(*x as f32), ScaledPixels(*y as f32)),
                            size: size(ScaledPixels(*w as f32), ScaledPixels(*h as f32)),
                        },
                        content_mask: self.mask,
                        background: rgba(u32::from_be_bytes(*color)).into(),
                        ..Quad::default()
                    });
                }
                crate::Shape::Path {
                    points,
                    width,
                    color,
                } => {
                    if points.len() < 2 {
                        continue;
                    }
                    let mut builder = PathBuilder::stroke(px(*width as f32));
                    builder.move_to(point(px(points[0][0] as f32), px(points[0][1] as f32)));
                    for [x, y] in &points[1..] {
                        builder.line_to(point(px(*x as f32), px(*y as f32)));
                    }
                    let mut path = builder.build()?.scale(1.0);
                    path.color = rgba(u32::from_be_bytes(*color)).into();
                    path.content_mask = self.mask;
                    scene.insert_primitive(path);
                }
            }
        }
        scene.finish();
        // GPUI clears its headless target to opaque black and waits for GPU readback.
        Ok(self
            .renderer
            .render_scene_to_image(&scene, self.size)?
            .into_raw())
    }
}
