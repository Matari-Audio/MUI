//! Glyph runs as atlas instances. glifo turns a run into hinted, varied
//! outlines at a quarter-pixel horizontal bin and packs each into a cell of
//! an atlas page; `vello_cpu` rasterises the cell's coverage once; the pass
//! draws every glyph as a whole-pixel box that reads its cell 1:1 and
//! multiplies the entry's paint by it. A steady frame finds every glyph in
//! the atlas, so it rasterises and uploads nothing.

use super::build::{Instance, SHAPE_GLYPH, code};
use crate::Cache;
use crate::kurbo::{Affine, BezPath, Rect, Shape as _};
use glifo::atlas::{AtlasCommand, AtlasConfig, AtlasSlot, GlyphAtlas, GlyphCacheConfig, ImageCache};
use glifo::{AtlasCacher, AtlasPaint, Glyph, GlyphPrepCache, GlyphRun, GlyphRunBackend};
use vello_common::paint::{Image, ImageId, ImageSource, PaintType, Tint, TintMode};
use vello_common::peniko::color::{AlphaColor, Srgb, palette::css::BLACK};

/// Why a run could not be drawn from the atlas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    /// A bitmap or colour glyph: not a coverage mask.
    Colour,
    /// The atlas is out of room.
    Full,
}

/// One region of an atlas page to write: `bytes[at..]`, `w` bytes a row.
#[derive(Clone, Copy, Debug)]
pub struct Upload {
    pub page: u32,
    pub rect: [u32; 4],
    pub at: usize,
}

/// The CPU side of the glyph atlas: what is where, and what changed.
pub struct Atlas {
    glyphs: GlyphAtlas,
    cells: ImageCache,
    prep: GlyphPrepCache,
    config: AtlasConfig,
    /// Rasterises one glyph's cell at a time.
    raster: vello_cpu::RenderContext,
    resources: vello_cpu::Resources,
    /// What [`Atlas::rasterise`] left for the GPU, cleared each frame.
    pub uploads: Vec<Upload>,
    pub bytes: Vec<u8>,
    /// The atlas was emptied: every page must start transparent.
    pub flushed: bool,
    /// Glyph cells rasterised, ever: what a steady frame must not add to.
    pub rasterised: u64,
}

impl Atlas {
    /// `pages` pages of `side` square, single-channel.
    pub fn new(side: u16, pages: usize) -> Self {
        let config = AtlasConfig {
            initial_atlas_count: 0,
            max_atlases: pages,
            atlas_size: (side, side),
            auto_grow: true,
            ..AtlasConfig::default()
        };
        Self {
            glyphs: atlas(),
            cells: ImageCache::new_with_config(config),
            prep: GlyphPrepCache::default(),
            config,
            raster: vello_cpu::RenderContext::new(1, 1),
            resources: vello_cpu::Resources::default(),
            uploads: Vec::new(),
            bytes: Vec::new(),
            flushed: false,
            rasterised: 0,
        }
    }

    pub fn side(&self) -> u16 {
        self.config.atlas_size.0
    }

    /// Pages allocated so far.
    pub fn pages(&self) -> usize {
        self.cells.atlas_count()
    }

    /// Start a frame: glyphs unused for glifo's age limit go.
    pub fn begin(&mut self) {
        self.glyphs.maintain(&mut self.cells);
        self.prep.maintain();
    }

    /// Empty the atlas: what a frame does when it ran out of room, before it
    /// tries again with every page free.
    pub fn flush(&mut self) {
        self.glyphs = atlas();
        self.cells = ImageCache::new_with_config(self.config);
        self.flushed = true;
    }

    /// Append `text`'s glyphs to `out` as instances made from `template`;
    /// how many.
    pub fn draw(
        &mut self,
        cache: &mut Cache,
        text: &mui_scene::Text,
        xf: Affine,
        template: &Instance,
        out: &mut Vec<u8>,
    ) -> Result<usize, Failure> {
        let mut sink = Sink {
            template,
            out,
            count: 0,
            transform: Affine::IDENTITY,
            paint_transform: Affine::IDENTITY,
            page: None,
            tint: false,
            failed: None,
            side: self.side(),
        };
        // Split per face, as the `Cpu` canvas does: a fallback font is its
        // own run.
        let mut start = 0;
        while start < text.glyphs.len() {
            let face = text.glyphs[start].font;
            let end = text.glyphs[start..]
                .iter()
                .position(|g| g.font != face)
                .map_or(text.glyphs.len(), |n| start + n);
            if let Some(font) = text.fonts.get(face) {
                let (font, _) = cache.font(font);
                let coords = text.font_coords.get(face).map_or(&[][..], |c| &c[..]);
                let backend = Backend {
                    sink: &mut sink,
                    glyphs: &mut self.glyphs,
                    cells: &mut self.cells,
                    prep: &mut self.prep,
                };
                // A glyph the font has no outline for draws nothing, as on
                // the `Cpu` canvas; glifo still reports it.
                let _ = glifo::GlyphRunBuilder::new(font, xf, Affine::IDENTITY, backend)
                    .font_size(text.size)
                    .hint(text.hint)
                    .normalized_coords(coords)
                    .fill_glyphs(crate::run(text.origin, &text.glyphs[start..end]));
            }
            start = end;
        }
        match sink.failed {
            Some(f) => Err(f),
            None => Ok(sink.count),
        }
    }

    /// Rasterise every cell glifo allocated this frame and zero every cell
    /// it evicted, into [`Atlas::uploads`].
    pub fn rasterise(&mut self) {
        self.uploads.clear();
        self.bytes.clear();
        for clear in self.glyphs.drain_pending_clear_rects() {
            let rect = [clear.x, clear.y, clear.width, clear.height].map(u32::from);
            self.uploads.push(Upload {
                page: clear.page_index,
                rect,
                at: self.bytes.len(),
            });
            self.bytes.resize(self.bytes.len() + (rect[2] * rect[3]) as usize, 0);
        }
        let side = f64::from(self.side());
        let (raster, resources) = (&mut self.raster, &mut self.resources);
        let (uploads, bytes, count) = (&mut self.uploads, &mut self.bytes, &mut self.rasterised);
        let _ = self.glyphs.replay_pending_atlas_commands(|recorder| {
            let mut transform = Affine::IDENTITY;
            for command in &recorder.commands {
                match command {
                    AtlasCommand::SetTransform(t) => transform = *t,
                    AtlasCommand::FillPath(path) => {
                        let ink = transform.transform_rect_bbox(path.bounding_box());
                        let cell = Rect::new(
                            ink.x0.floor().max(0.),
                            ink.y0.floor().max(0.),
                            ink.x1.ceil().min(side),
                            ink.y1.ceil().min(side),
                        );
                        if cell.width() <= 0. || cell.height() <= 0. {
                            continue;
                        }
                        let at = bytes.len();
                        mask(raster, resources, path, transform, cell, bytes);
                        *count += 1;
                        uploads.push(Upload {
                            page: recorder.page_index,
                            rect: [cell.x0, cell.y0, cell.width(), cell.height()].map(|v| v as u32),
                            at,
                        });
                    }
                    // Colour glyphs record more; their entries are refused,
                    // so nothing reads their cells.
                    _ => {}
                }
            }
            Ok::<(), std::convert::Infallible>(())
        });
    }
}

fn atlas() -> GlyphAtlas {
    GlyphAtlas::with_config(GlyphCacheConfig {
        max_cached_font_size: super::build::MAX_GLYPH_PX,
        ..GlyphCacheConfig::default()
    })
}

/// `path` under `transform`, covered black, as the alpha of `cell`: one byte
/// a pixel, appended to `out`.
fn mask(
    raster: &mut vello_cpu::RenderContext,
    resources: &mut vello_cpu::Resources,
    path: &BezPath,
    transform: Affine,
    cell: Rect,
    out: &mut Vec<u8>,
) {
    let (w, h) = (cell.width() as u16, cell.height() as u16);
    raster.reset_and_resize(w, h);
    raster.set_transform(Affine::translate((-cell.x0, -cell.y0)) * transform);
    raster.set_paint(BLACK);
    raster.fill_path(path);
    raster.flush();
    let mut pixmap = vello_cpu::Pixmap::new(w, h);
    raster.render(&mut pixmap, resources);
    out.extend(pixmap.data().iter().map(|p| p.a));
}

/// What glifo draws a run through: it records each glyph the atlas holds
/// as an instance, and fails on anything else.
struct Sink<'a> {
    template: &'a Instance,
    out: &'a mut Vec<u8>,
    count: usize,
    transform: Affine,
    paint_transform: Affine,
    /// The atlas page of the current image paint.
    page: Option<u32>,
    /// Whether the image is a coverage mask to tint.
    tint: bool,
    failed: Option<Failure>,
    side: u16,
}

impl Sink<'_> {
    fn fail(&mut self, f: Failure) {
        self.failed.get_or_insert(f);
    }
}

/// glifo only asks a solid paint's colour, to tint the mask; the entry's
/// own paint, solid or gradient, is in the template.
const SOLID: PaintType = PaintType::Solid(BLACK);

impl glifo::DrawSink for Sink<'_> {
    fn set_transform(&mut self, t: Affine) {
        self.transform = t;
    }
    fn set_paint(&mut self, _: AtlasPaint) {}
    fn set_paint_transform(&mut self, t: Affine) {
        self.paint_transform = t;
    }
    /// glifo fills an outline itself only when the atlas could not take it.
    fn fill_path(&mut self, _: &BezPath) {
        self.fail(Failure::Full);
    }
    fn fill_rect(&mut self, rect: &Rect) {
        let (Some(page), true) = (self.page, self.tint) else {
            return self.fail(Failure::Colour);
        };
        // A cell is placed at a whole-pixel offset, read at its own origin.
        let r = self.transform.transform_rect_bbox(*rect);
        let cell = self.paint_transform.translation();
        let mut one = *self.template;
        one[2] = [r.x0, r.y0, r.x1, r.y1].map(|v| v as f32);
        let paint = one[3][0] - code(SHAPE_GLYPH, 0.);
        one[3] = [
            code(SHAPE_GLYPH, paint) + 64. * page as f32,
            one[3][1],
            -cell.x as f32,
            -cell.y as f32,
        ];
        for v in one.as_flattened() {
            self.out.extend_from_slice(&v.to_ne_bytes());
        }
        self.count += 1;
    }
    fn push_clip_layer(&mut self, _: &BezPath) {
        self.fail(Failure::Colour);
    }
    fn push_blend_layer(&mut self, _: vello_common::peniko::BlendMode) {
        self.fail(Failure::Colour);
    }
    fn pop_layer(&mut self) {}
    fn width(&self) -> u16 {
        self.side
    }
    fn height(&self) -> u16 {
        self.side
    }
}

impl glifo::GlyphRenderer for Sink<'_> {
    type SavedState = (Affine, Affine);
    fn save_state(&mut self) -> Self::SavedState {
        (self.transform, self.paint_transform)
    }
    fn restore_state(&mut self, (t, p): Self::SavedState) {
        (self.transform, self.paint_transform) = (t, p);
    }
    fn stroke_path(&mut self, _: &BezPath) {
        self.fail(Failure::Colour);
    }
    fn set_paint_image(&mut self, image: Image) {
        self.page = match image.image {
            ImageSource::OpaqueId { id, .. } => Some(id.as_u32()),
            _ => None,
        };
    }
    fn set_tint(&mut self, tint: Option<Tint>) {
        self.tint = tint.is_some_and(|t| t.mode == TintMode::AlphaMask);
    }
    fn get_context_color(&self) -> AlphaColor<Srgb> {
        BLACK
    }
    fn current_paint(&self) -> &PaintType {
        &SOLID
    }
    fn atlas_image_source(&self, slot: &AtlasSlot) -> ImageSource {
        ImageSource::opaque_id(ImageId::new(slot.page_index))
    }
    fn atlas_paint_transform(&self, slot: &AtlasSlot) -> Affine {
        Affine::translate((-f64::from(slot.x), -f64::from(slot.y)))
    }
}

struct Backend<'a, 's> {
    sink: &'a mut Sink<'s>,
    glyphs: &'a mut GlyphAtlas,
    cells: &'a mut ImageCache,
    prep: &'a mut GlyphPrepCache,
}

impl<'a> GlyphRunBackend<'a> for Backend<'a, '_> {
    fn atlas_cache(self, _: bool) -> Self {
        self
    }
    fn fill_glyphs<G>(self, run: GlyphRun<'a>, glyphs: G) -> Result<(), glifo::GlyphRenderError>
    where
        G: Iterator<Item = Glyph> + Clone,
    {
        run.build(
            glyphs,
            self.prep.as_mut(),
            AtlasCacher::Enabled(self.glyphs, self.cells),
        )
        .fill_glyphs(self.sink)
    }
    fn stroke_glyphs<G>(self, _: GlyphRun<'a>, _: G) -> Result<(), glifo::GlyphRenderError>
    where
        G: Iterator<Item = Glyph> + Clone,
    {
        self.sink.fail(Failure::Colour);
        Ok(())
    }
    fn render_decoration<G>(
        self,
        _: GlyphRun<'a>,
        _: G,
        _: std::ops::RangeInclusive<f32>,
        _: f32,
        _: f32,
        _: f32,
        _: f32,
    ) where
        G: Iterator<Item = Glyph> + Clone,
    {
        self.sink.fail(Failure::Colour);
    }
}
