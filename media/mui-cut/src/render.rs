//! A [`Frame`] to pixels through MUI and Vello. Each layer is its own
//! small MUI tree (a block, a canvas ellipse, a text run, an image block),
//! resolved and painted under the layer's affine: MUI has no rotation in the
//! tree, and a per-layer paint transform is exactly what one needs.
use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

use mui_scene::ResolvedScene;
use mui_scene::prelude::*;
use mui_vello::kurbo::{Affine, Point as KPoint, Rect};
use serde::Serialize;
use vello_cpu::{Pixmap, RenderContext, Resources};

use crate::{Drawn, Frame, Kind, Rgba};

/// Inter, variable in weight, is every text layer's face.
static INTER: LazyLock<Font> =
    LazyLock::new(|| Font::new(ttf_inter::REGULAR).expect("the bundled Inter parses"));

/// A layer's four corners in project pixels, clockwise from its top left:
/// what the editor outlines and hit-tests.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Quad {
    pub id: String,
    pub pts: [[f64; 2]; 4],
}

/// The decoded images, by the path the project names them with, and the
/// per-layer MUI trees every backend paints.
#[derive(Default)]
pub struct Assets {
    images: HashMap<String, Arc<Image>>,
}

/// One frame's layers, ready to paint: each resolved tree with where it goes
/// in project pixels, plus every layer's quad (drawn or not).
pub struct Layers {
    pub scenes: Vec<(ResolvedScene, Affine)>,
    pub quads: Vec<Quad>,
}

/// The CPU rasteriser, reused across frames.
pub struct Renderer {
    w: u16,
    h: u16,
    ctx: RenderContext,
    res: Resources,
    cache: mui_vello::Cache,
    pub assets: Assets,
}

pub(crate) fn color(c: Rgba) -> Color {
    let [r, g, b, a] = c.0.map(|v| f32::from(v) / 255.);
    Color::srgba(r, g, b, a)
}

/// Four cubics, the usual kappa: exact enough to be indistinguishable.
fn ellipse(size: Size) -> Path {
    let (rx, ry) = (size.width / 2., size.height / 2.);
    let k = 0.5522847498;
    let p = |x: f64, y: f64| Point::new(rx + x * rx, ry + y * ry);
    Path::default()
        .move_to(p(1., 0.))
        .cubic_to(p(1., k), p(k, 1.), p(0., 1.))
        .cubic_to(p(-k, 1.), p(-1., k), p(-1., 0.))
        .cubic_to(p(-1., -k), p(-k, -1.), p(0., -1.))
        .cubic_to(p(k, -1.), p(1., -k), p(1., 0.))
        .close()
}

impl Renderer {
    /// Output pixels; frames are scaled to fit whatever their project size.
    pub fn new(w: u16, h: u16) -> Self {
        Self {
            w: w.max(1),
            h: h.max(1),
            ctx: RenderContext::new(w.max(1), h.max(1)),
            res: Resources::default(),
            cache: mui_vello::Cache::default(),
            assets: Assets::default(),
        }
    }
    pub fn size(&self) -> (u16, u16) {
        (self.w, self.h)
    }
    pub fn add_png(&mut self, path: &str, bytes: &[u8]) -> Result<(), String> {
        self.assets.add_png(path, bytes)
    }

    /// Straight RGBA at the renderer's size, and each layer's quad in project
    /// pixels.
    pub fn draw(&mut self, frame: &Frame) -> Result<(Vec<u8>, Vec<Quad>), String> {
        let [fw, fh] = frame.size.map(f64::from);
        let view = Affine::scale_non_uniform(f64::from(self.w) / fw, f64::from(self.h) / fh);
        self.ctx.reset();
        self.ctx.set_transform(view);
        let [r, g, b, a] = frame.background.0;
        self.ctx
            .set_paint(mui_vello::peniko::Color::from_rgba8(r, g, b, a));
        self.ctx.fill_rect(&Rect::new(0., 0., fw, fh));
        let layers = self.assets.layers(frame)?;
        for (scene, place) in &layers.scenes {
            mui_vello::paint(
                &mut mui_vello::Cpu {
                    ctx: &mut self.ctx,
                    resources: &mut self.res,
                    cache: &mut self.cache,
                },
                scene,
                view * *place,
            )
            .map_err(|e| format!("paint: {e:?}"))?;
        }
        self.ctx.flush();
        let mut pix = Pixmap::new(self.w, self.h);
        self.ctx.render(&mut pix, &mut self.res);
        let rgba = pix
            .take_unpremultiplied()
            .iter()
            .flat_map(|p| [p.r, p.g, p.b, p.a])
            .collect();
        Ok((rgba, layers.quads))
    }
}

impl Assets {
    /// Decode a PNG for image layers naming `path`.
    pub fn add_png(&mut self, path: &str, bytes: &[u8]) -> Result<(), String> {
        let err = |e: png::DecodingError| format!("{path}: {e}");
        let mut dec = png::Decoder::new(std::io::Cursor::new(bytes));
        dec.set_transformations(png::Transformations::normalize_to_color8());
        let mut reader = dec.read_info().map_err(err)?;
        let mut buf = vec![0; reader.output_buffer_size().ok_or("png too large")?];
        let info = reader.next_frame(&mut buf).map_err(err)?;
        let px = &buf[..info.buffer_size()];
        let rgba: Vec<u8> = match info.color_type {
            png::ColorType::Rgba => px.to_vec(),
            png::ColorType::Rgb => px.chunks(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
            png::ColorType::GrayscaleAlpha => px
                .chunks(2)
                .flat_map(|p| [p[0], p[0], p[0], p[1]])
                .collect(),
            png::ColorType::Grayscale => px.iter().flat_map(|&g| [g, g, g, 255]).collect(),
            png::ColorType::Indexed => return Err(format!("{path}: indexed png not expanded")),
        };
        let image = Image::rgba(info.width, info.height, rgba).ok_or("empty png")?;
        self.images.insert(path.to_owned(), Arc::new(image));
        Ok(())
    }

    /// The layer as a MUI tree, and the size it lays out to.
    fn element(&self, l: &Drawn) -> Result<(ResolvedScene, Size), String> {
        let fill = color(l.fill);
        let el = match &l.kind {
            Kind::Rect => block(l.width, l.height).radius(l.radius).fill(fill),
            Kind::Ellipse => canvas(move |size| vec![Draw::fill(ellipse(size), fill)])
                .w(l.width)
                .h(l.height),
            Kind::Text { text: s } => text(s.as_str())
                .text_size(l.font_size)
                .text_axis("wght", l.weight as f32)
                .fill(fill),
            Kind::Image { path } => {
                let paint: Fill = match self.images.get(path) {
                    Some(i) => Fill::Image(i.clone(), Fit::Cover),
                    // A missing file draws as its fill: visible, not fatal.
                    None => fill.into(),
                };
                block(l.width, l.height).radius(l.radius).fill(paint)
            }
        };
        let mut spec = SceneSpec::new(el.opacity(l.opacity as f32));
        spec.font = Some(INTER.clone());
        let scene = resolve(&spec).map_err(|e| format!("layer `{}`: {e}", l.id))?;
        let size = scene.layout.size;
        Ok((scene, size))
    }

    /// Every layer of `frame` resolved and placed, bottom first. A layer
    /// with no opacity or no scale keeps its quad but is not drawn.
    pub fn layers(&self, frame: &Frame) -> Result<Layers, String> {
        let mut out = Layers {
            scenes: Vec::with_capacity(frame.layers.len()),
            quads: Vec::with_capacity(frame.layers.len()),
        };
        for l in &frame.layers {
            let (scene, size) = self.element(l)?;
            let place = Affine::translate((l.x, l.y))
                * Affine::rotate(l.rotation.to_radians())
                * Affine::scale(l.scale)
                * Affine::translate((-size.width / 2., -size.height / 2.));
            let corners = [
                (0., 0.),
                (size.width, 0.),
                (size.width, size.height),
                (0., size.height),
            ];
            out.quads.push(Quad {
                id: l.id.clone(),
                pts: corners.map(|(x, y)| {
                    let p = place * KPoint::new(x, y);
                    [p.x, p.y]
                }),
            });
            if l.opacity > 0. && l.scale != 0. {
                out.scenes.push((scene, place));
            }
        }
        Ok(out)
    }
}
