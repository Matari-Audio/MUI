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

use crate::plugin::{CACHE, Capture, PluginAt, poses};
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
    /// A plugin layer's and its parts' quads: the affine `[a, b, c, d, e,
    /// f]` from the UI's pixels to project pixels, so the editor can aim
    /// the pointer through a moved part and move a part in its parent's.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ui: Option<[f64; 6]>,
}

/// The decoded images, by the path the project names them with, and the
/// per-layer MUI trees every backend paints.
#[derive(Clone, Default)]
pub struct Assets {
    images: HashMap<String, Arc<Image>>,
    svgs: HashMap<String, Arc<Vec<vector::Piece>>>,
    lotties: HashMap<String, Arc<velato::Composition>>,
    models: HashMap<String, Arc<crate::three::Mesh>>,
    envs: HashMap<String, Arc<mui_stage::EnvImage>>,
    captures: HashMap<String, Arc<Capture>>,
    fonts: HashMap<String, Font>,
}

/// One frame's layers, ready to paint: each resolved tree with where it goes
/// in project pixels, plus every layer's quad (drawn or not), and every
/// plugin part's quad, `layer#part`.
pub struct Layers {
    pub scenes: Vec<(ResolvedScene, Affine)>,
    pub quads: Vec<Quad>,
    pub parts: Vec<Quad>,
}

/// A plugin part's highlight: graphite, flat.
const HIGHLIGHT: Rgba = Rgba([230, 230, 230, 255]);

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
        Ok((rgba, layers.all_quads()))
    }
}

impl Assets {
    /// A file a layer names, by its extension: `.png` for image layers,
    /// `.svg` for SVG layers, `.json` for Lottie layers; a `.json` under
    /// [`CACHE`] is a plugin capture (its images are PNGs under it too).
    pub fn add_asset(&mut self, path: &str, bytes: &[u8]) -> Result<(), String> {
        let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
        match ext.as_str() {
            "json" if path.starts_with(CACHE) => {
                let c: Capture =
                    serde_json::from_slice(bytes).map_err(|e| format!("{path}: {e}"))?;
                self.captures.insert(path.to_owned(), Arc::new(c));
                Ok(())
            }
            "svg" => {
                let pieces =
                    vector::svg(bytes, ttf_inter::REGULAR).map_err(|e| format!("{path}: {e}"))?;
                self.svgs.insert(path.to_owned(), Arc::new(pieces));
                Ok(())
            }
            "ttf" | "otf" => {
                let font = Font::new(bytes.to_vec()).map_err(|e| format!("{path}: {e:?}"))?;
                self.fonts.insert(path.to_owned(), font);
                Ok(())
            }
            "hdr" | "exr" => {
                let img = mui_stage::EnvImage::decode(path, bytes)?;
                self.envs.insert(path.to_owned(), Arc::new(img));
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
            // A font not loaded (yet) draws in Inter.
            Kind::Text { text, align, font } => {
                let face = self.fonts.get(font).unwrap_or(&INTER);
                vector::text(l, text, *align, face)?
            }
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
            | Kind::Model { .. }
            | Kind::Plugin { .. } => Vec::new(),
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

    /// Layer `l`'s content alone, placement ignored, at `k` pixels per
    /// project pixel on a clear ground: straight RGBA and its pixel size,
    /// and the [`Assets::element`] box it fills.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn paint(
        &self,
        l: &Drawn,
        k: f64,
    ) -> Result<(Vec<u8>, [u16; 2], Size, KPoint), String> {
        let (scene, size, corner) = self.element(l)?;
        let side = |v: f64| (v * k).ceil().clamp(1., 8192.) as u16;
        let (w, h) = (side(size.width), side(size.height));
        let mut ctx = RenderContext::new(w, h);
        let mut res = Resources::default();
        mui_vello::paint(
            &mut mui_vello::Cpu {
                ctx: &mut ctx,
                resources: &mut res,
                cache: &mut mui_vello::Cache::default(),
            },
            &scene,
            Affine::scale(k),
        )
        .map_err(|e| format!("paint: {e:?}"))?;
        ctx.flush();
        let mut pix = Pixmap::new(w, h);
        ctx.render(&mut pix, &mut res);
        let rgba = pix
            .take_unpremultiplied()
            .iter()
            .flat_map(|p| [p.r, p.g, p.b, p.a])
            .collect();
        Ok((rgba, [w, h], size, corner))
    }

    /// A model layer's mesh, once its file is loaded.
    pub(crate) fn model(&self, path: &str) -> Option<&Arc<crate::three::Mesh>> {
        self.models.get(path)
    }
    pub(crate) fn env(&self, path: &str) -> Option<&Arc<mui_stage::EnvImage>> {
        self.envs.get(path)
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
            parts: Vec::new(),
        };
        for l in &frame.layers {
            if let Some(p) = &l.plugin {
                self.plugin(l, p, &mut out)?;
                continue;
            }
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
                ui: None,
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

impl Layers {
    /// Layer quads, then part quads: what the editor hit-tests (backwards,
    /// so a part wins over its plugin).
    pub fn all_quads(&self) -> Vec<Quad> {
        self.quads.iter().chain(&self.parts).cloned().collect()
    }
}

fn quad(id: String, place: Affine, w: f64, h: f64) -> Quad {
    Quad {
        id,
        ui: None,
        pts: [(0., 0.), (w, 0.), (w, h), (0., h)].map(|(x, y)| {
            let p = place * KPoint::new(x, y);
            [p.x, p.y]
        }),
    }
}

impl Assets {
    /// A capture, if it has been added.
    pub fn capture(&self, state: &str) -> Option<&Capture> {
        self.captures
            .get(&format!("{CACHE}/{state}.json"))
            .map(|c| &**c)
    }

    /// A plugin layer: its captured fragments, each an image block where
    /// the UI put it, parts moved by `explode` and their own tracks, with a
    /// flat outline when highlighted. Before its capture is there, a faint
    /// `width` x `height` box in its fill.
    /// A 3D scene's plugin layer, as plain layers mui-stage sets on their
    /// own slabs: the backdrop (the layer's id, so its quad), then each
    /// part (`layer#part`) where the 2D drawing puts it, `explode` times
    /// [`EXPLODE_DEPTH`](crate::plugin::EXPLODE_DEPTH) towards the viewer
    /// plus its own `z`, all turned with the layer. A highlight is a flat
    /// plate just behind its part (with the part's id too). Other layers come back as they are.
    pub(crate) fn slabs(&self, l: &Drawn) -> Vec<Drawn> {
        use mui_stage::Mat4;
        let Some(p) = &l.plugin else {
            return vec![l.clone()];
        };
        let Some(cap) = self.capture(&p.state) else {
            // The 2D placeholder: a faint card of the layer's size.
            let fill = Rgba([l.fill.0[0], l.fill.0[1], l.fill.0[2], l.fill.0[3] / 8]);
            return vec![Drawn {
                kind: Kind::Rect,
                radius: 8.,
                fill,
                plugin: None,
                ..l.clone()
            }];
        };
        let (w, h) = (cap.width, cap.height);
        let s = &l.space;
        // The layer's turn, in mui-stage's y-up, z-towards-the-viewer
        // world, as gpu3d poses a slab.
        let turn = Mat4::rotate_y(s.ry.to_radians() as f32)
            * Mat4::rotate_x(-s.rx.to_radians() as f32)
            * Mat4::rotate_z(-l.rotation.to_radians() as f32);
        // A point in the UI's pixels, `depth` towards the viewer from its
        // face, to the project's x, y and z (larger is farther).
        let place = |c: [f64; 2], depth: f64| {
            let k = l.scale;
            let local = [
                ((c[0] - w / 2.) * k) as f32,
                (-(c[1] - h / 2.) * k) as f32,
                ((s.anchor_z + depth) * k) as f32,
            ];
            let [x, y, z] = turn.project(local).map(f64::from);
            (l.x + x, l.y - y, s.z - z)
        };
        let mut out = Vec::new();
        let mut slab = |id: String,
                        kind: Kind,
                        c: [f64; 2],
                        depth,
                        size: [f64; 2],
                        look: (f64, f64, f64, Rgba)| {
            let (scale, rotation, opacity, fill) = look;
            let (x, y, z) = place(c, depth);
            out.push(Drawn {
                id,
                kind,
                x,
                y,
                scale: l.scale * scale,
                rotation: l.rotation + rotation,
                opacity: l.opacity * opacity,
                width: size[0],
                height: size[1],
                radius: 0.,
                fill,
                plugin: None,
                space: crate::three::Space {
                    z,
                    anchor_z: 0.,
                    ..s.clone()
                },
                ..l.clone()
            });
        };
        let posed = poses(cap, p);
        let mut plated = std::collections::HashSet::new();
        for f in &cap.fragments {
            if !p.draws(&f.group) {
                continue;
            }
            if f.group == "background" {
                let [rx, ry, rw, rh] = f.rect;
                let image = Kind::Image {
                    path: format!("{CACHE}/{}", f.src),
                };
                let c = [rx + rw / 2., ry + rh / 2.];
                slab(
                    l.id.clone(),
                    image,
                    c,
                    0.,
                    [rw, rh],
                    (1., 0., p.backdrop, l.fill),
                );
                continue;
            }
            let Some((info, pose)) = posed.iter().find(|(i, _)| i.path == f.group) else {
                continue;
            };
            let look = |fill| (pose.scale, pose.rotation, pose.opacity, fill);
            let centre = |r: [f64; 4]| {
                let c = pose.at * KPoint::new(r[0] + r[2] / 2., r[1] + r[3] / 2.);
                [c.x, c.y]
            };
            if pose.highlight > 0. && plated.insert(&info.path) {
                let b = 2. / l.scale.abs().max(0.05) / pose.scale.abs().max(0.05);
                let [r0, g0, b0, _] = HIGHLIGHT.0;
                let plate = Rgba([r0, g0, b0, (pose.highlight * 255.).round() as u8]);
                let [_, _, fw, fh] = info.frame;
                slab(
                    format!("{}#{}", l.id, f.group),
                    Kind::Rect,
                    centre(info.frame),
                    pose.depth - 0.5,
                    [fw + 2. * b, fh + 2. * b],
                    look(plate),
                );
            }
            let (src, rect) = f.image(pose.moved);
            slab(
                format!("{}#{}", l.id, f.group),
                Kind::Image {
                    path: format!("{CACHE}/{src}"),
                },
                centre(rect),
                pose.depth,
                [rect[2], rect[3]],
                look(l.fill),
            );
        }
        out
    }

    fn plugin(&self, l: &Drawn, p: &PluginAt, out: &mut Layers) -> Result<(), String> {
        let cap = self.capture(&p.state);
        let (w, h) = cap.map_or((l.width, l.height), |c| (c.width, c.height));
        let place = Affine::translate((l.x, l.y))
            * Affine::rotate(l.rotation.to_radians())
            * Affine::scale(l.scale)
            * Affine::translate((-w / 2., -h / 2.));
        // A component layer's outline is its parts' captured rects.
        let own = cap
            .filter(|_| !p.show.is_empty())
            .into_iter()
            .flat_map(|c| &c.fragments)
            .filter(|f| p.draws(&f.group))
            .map(|f| {
                let [x, y, w, h] = f.rect;
                Rect::new(x, y, x + w, y + h)
            })
            .reduce(|a, b| a.union(b));
        let outline = match own {
            Some(r) => quad(
                l.id.clone(),
                place * Affine::translate((r.x0, r.y0)),
                r.width(),
                r.height(),
            ),
            None => quad(l.id.clone(), place, w, h),
        };
        out.quads.push(Quad {
            ui: Some(place.as_coeffs()),
            ..outline
        });
        let drawn = l.opacity > 0. && l.scale != 0.;
        let mut push = |el: El, at: Affine| -> Result<(), String> {
            let scene =
                resolve(&SceneSpec::new(el)).map_err(|e| format!("layer `{}`: {e}", l.id))?;
            out.scenes.push((scene, at));
            Ok(())
        };
        let Some(cap) = cap else {
            if drawn {
                let faint = Rgba([l.fill.0[0], l.fill.0[1], l.fill.0[2], l.fill.0[3] / 8]);
                push(
                    block(w, h)
                        .radius(8.)
                        .fill(color(faint))
                        .opacity(l.opacity as f32),
                    place,
                )?;
            }
            return Ok(());
        };
        let posed = poses(cap, p);
        for f in cap.fragments.iter().filter(|f| p.draws(&f.group)) {
            let (src, [rx, ry, rw, rh], at, opacity) = if f.group == "background" {
                (&*f.src, f.rect, Affine::IDENTITY, p.backdrop)
            } else {
                let Some((_, pose)) = posed.iter().find(|(i, _)| i.path == f.group) else {
                    continue;
                };
                let (src, rect) = f.image(pose.moved);
                (src, rect, pose.at, pose.opacity)
            };
            let Some(img) = self.images.get(&format!("{CACHE}/{src}")) else {
                continue;
            };
            if !drawn || opacity <= 0. {
                continue;
            }
            let fill = Fill::Image(img.clone(), Fit::Fill);
            push(
                // Square: a capture's pixels are its corners.
                block(rw, rh)
                    .radius(0.)
                    .fill(fill)
                    .opacity((l.opacity * opacity) as f32),
                place * at * Affine::translate((rx, ry)),
            )?;
        }
        // Each part's quad (parents first, so a child wins a hit) and its
        // highlight: a flat outline round its frame, over everything.
        for (info, pose) in posed.iter().filter(|(i, _)| p.draws(&i.path)) {
            let [fx, fy, fw, fh] = info.frame;
            let at = place * pose.at * Affine::translate((fx, fy));
            out.parts.push(Quad {
                ui: Some((place * pose.at).as_coeffs()),
                ..quad(format!("{}#{}", l.id, info.path), at, fw, fh)
            });
            if drawn && pose.highlight > 0. && pose.opacity > 0. {
                let a = (pose.highlight * l.opacity * 255.).round() as u8;
                let [r, g, b, _] = HIGHLIGHT.0;
                let line = block(fw, fh)
                    .radius(0.)
                    .no_fill()
                    .stroke(color(Rgba([r, g, b, a])))
                    .stroke_width(2. / (l.scale * pose.scale).abs().max(0.05));
                push(line, at)?;
            }
        }
        Ok(())
    }
}
