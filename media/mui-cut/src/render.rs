//! A [`Frame`] to pixels through MUI and Vello. Each layer is its own
//! small MUI tree (a block, a canvas ellipse, a text run, an image block),
//! resolved and painted under the layer's affine: MUI has no rotation in the
//! tree, and a per-layer paint transform is exactly what one needs.
use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

use mui_scene::ResolvedScene;
use mui_scene::prelude::*;
use mui_vello::kurbo::{Affine, Point as KPoint, Rect, Shape as _};
use serde::Serialize;
use vello_cpu::{Pixmap, RenderContext, Resources};

use crate::{Drawn, Frame, Kind, Rgba, vector};

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
#[derive(Clone, Default)]
pub struct Assets {
    images: HashMap<String, Arc<Image>>,
    svgs: HashMap<String, Arc<Vec<vector::Piece>>>,
    lotties: HashMap<String, Arc<velato::Composition>>,
    models: HashMap<String, Arc<crate::three::Mesh>>,
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
    /// Rasterises on the calling thread.
    pub fn new(w: u16, h: u16) -> Self {
        Self::with_threads(w, h, 0)
    }
    /// [`Renderer::new`] with `threads` extra rasteriser threads (native
    /// only; ignored on the web). SIMD is picked at run time either way.
    pub fn with_threads(w: u16, h: u16, threads: u16) -> Self {
        let settings = vello_cpu::RenderSettings {
            num_threads: threads,
            ..vello_cpu::RenderSettings::default()
        };
        Self {
            w: w.max(1),
            h: h.max(1),
            ctx: RenderContext::new_with(w.max(1), h.max(1), settings),
            res: Resources::default(),
            cache: mui_vello::Cache::default(),
            assets: Assets::default(),
        }
    }
    pub fn size(&self) -> (u16, u16) {
        (self.w, self.h)
    }
    pub fn add_asset(&mut self, path: &str, bytes: &[u8]) -> Result<(), String> {
        self.assets.add_asset(path, bytes)
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
    /// A file a layer names, by its extension: `.png` for image layers,
    /// `.svg` for SVG layers, `.json` for Lottie layers.
    pub fn add_asset(&mut self, path: &str, bytes: &[u8]) -> Result<(), String> {
        let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
        match ext.as_str() {
            "svg" => {
                let pieces =
                    vector::svg(bytes, ttf_inter::REGULAR).map_err(|e| format!("{path}: {e}"))?;
                self.svgs.insert(path.to_owned(), Arc::new(pieces));
                Ok(())
            }
            "glb" => {
                let mesh = crate::three::glb(bytes).map_err(|e| format!("{path}: {e}"))?;
                self.models.insert(path.to_owned(), Arc::new(mesh));
                Ok(())
            }
            "json" => {
                let comp =
                    velato::Composition::from_slice(bytes).map_err(|e| format!("{path}: {e}"))?;
                self.lotties.insert(path.to_owned(), Arc::new(comp));
                Ok(())
            }
            _ => self.add_png(path, bytes),
        }
    }

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

    /// Every decoded image.
    pub fn images(&self) -> impl Iterator<Item = &Image> {
        self.images.values().map(|i| &**i)
    }

    /// A vector layer's pieces: built, trimmed and deformed.
    fn pieces(&self, l: &Drawn) -> Result<Vec<vector::Piece>, String> {
        let mut pieces = match &l.kind {
            Kind::Text { text, align } => vector::text(l, text, *align, &INTER)?,
            Kind::Path { d } => vector::path(l, d),
            Kind::Duplicator {
                shape,
                d,
                layout,
                along,
                orient,
            } => vector::duplicator(l, *shape, d, *layout, along, *orient),
            // A missing file draws nothing (the CLI warns when it loads).
            Kind::Svg { path } => self.svgs.get(path).map(|p| p.to_vec()).unwrap_or_default(),
            Kind::Lottie { path, looped, .. } => self
                .lotties
                .get(path)
                .map(|c| vector::lottie(c, l.time, *looped))
                .unwrap_or_default(),
            Kind::Rect
            | Kind::Ellipse
            | Kind::Image { .. }
            | Kind::Camera { .. }
            | Kind::Light { .. }
            | Kind::Model { .. } => Vec::new(),
        };
        vector::trim(&mut pieces, l.trim);
        vector::deform(&mut pieces, &l.deformers);
        Ok(pieces)
    }

    /// The layer as a MUI tree, the size it lays out to, and where its top
    /// left sits relative to the layer's origin (its pivot).
    pub(crate) fn element(&self, l: &Drawn) -> Result<(ResolvedScene, Size, KPoint), String> {
        let fill = color(l.fill);
        let centred = |size: Size| KPoint::new(-size.width / 2., -size.height / 2.);
        let el = match &l.kind {
            Kind::Rect => block(l.width, l.height).radius(l.radius).fill(fill),
            Kind::Ellipse => canvas(move |size| vec![Draw::fill(ellipse(size), fill)])
                .w(l.width)
                .h(l.height),
            Kind::Image { path } => {
                let paint: Fill = match self.images.get(path) {
                    Some(i) => Fill::Image(i.clone(), Fit::Cover),
                    // A missing file draws as its fill: visible, not fatal.
                    None => fill.into(),
                };
                block(l.width, l.height).radius(l.radius).fill(paint)
            }
            _ => {
                let (draws, corner, (w, h)) = vector::draws(&self.pieces(l)?);
                let el = canvas(move |_| draws.clone()).w(w).h(h);
                let scene = resolve(&SceneSpec::new(el.opacity(l.opacity as f32)))
                    .map_err(|e| format!("layer `{}`: {e}", l.id))?;
                let size = scene.layout.size;
                return Ok((scene, size, corner));
            }
        };
        let mut spec = SceneSpec::new(el.opacity(l.opacity as f32));
        spec.font = Some(INTER.clone());
        let scene = resolve(&spec).map_err(|e| format!("layer `{}`: {e}", l.id))?;
        let size = scene.layout.size;
        Ok((scene, size, centred(size)))
    }

    /// A model layer's mesh, once its file is loaded.
    pub(crate) fn model(&self, path: &str) -> Option<&Arc<crate::three::Mesh>> {
        self.models.get(path)
    }

    /// The outline an extruded layer's walls follow, in its own y-down
    /// pixels from the top left of its [`Assets::element`] box; `None` is
    /// that box.
    pub(crate) fn outline(&self, l: &Drawn, size: Size, corner: KPoint) -> Option<Path> {
        let rounded = |r: f64| {
            let p =
                mui_vello::kurbo::RoundedRect::new(0., 0., size.width, size.height, r).to_path(0.1);
            vector::outline(
                &[vector::Piece {
                    path: p,
                    fill: Some((Rgba([0; 4]), 1.)),
                    stroke: None,
                }],
                KPoint::ZERO,
            )
        };
        match &l.kind {
            Kind::Rect | Kind::Image { .. } if l.radius > 0. => Some(rounded(l.radius)),
            Kind::Rect | Kind::Image { .. } => None,
            Kind::Ellipse => Some(ellipse(size)),
            _ => Some(vector::outline(&self.pieces(l).ok()?, corner)),
        }
    }

    /// Every layer of `frame` resolved and placed, bottom first. A layer
    /// with no opacity or no scale keeps its quad but is not drawn.
    pub fn layers(&self, frame: &Frame) -> Result<Layers, String> {
        let mut out = Layers {
            scenes: Vec::with_capacity(frame.layers.len()),
            quads: Vec::with_capacity(frame.layers.len()),
        };
        for l in &frame.layers {
            let (scene, size, corner) = self.element(l)?;
            let place = Affine::translate((l.x, l.y))
                * Affine::rotate(l.rotation.to_radians())
                * Affine::scale(l.scale)
                * Affine::translate(corner.to_vec2());
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
