//! Transactional damage detection. No pixels are assumed to survive in a
//! swapchain image. TiledEffects owns persistent tiles and recomposes all paint
//! overlapping each dirty tile, not just the node that changed.
use crate::kurbo::{Affine, BezPath, Rect, Shape as _};
use mui_scene::{ExternalWeld, Layer, Painted, ResolvedScene};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tile {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}
impl Tile {
    pub fn rect(self) -> Rect {
        Rect::new(
            self.x as f64,
            self.y as f64,
            (self.x + self.width) as f64,
            (self.y + self.height) as f64,
        )
    }
}
/// Reused frame to frame: planning an idle frame allocates nothing.
#[derive(Clone, Debug, Default)]
pub struct DamagePlan {
    pub dirty: Vec<usize>,
    pub total_tiles: usize,
    pub full: bool,
    pub dirty_pixels: u64,
    flags: Vec<bool>,
}
#[derive(Default)]
pub struct DamageTracker {
    paint: Vec<Painted>,
    external: BTreeMap<String, ExternalWeld>,
    xf: Option<Affine>,
    valid: bool,
}
impl DamageTracker {
    pub fn invalidate(&mut self) {
        self.valid = false;
    }
    /// Inspection does NOT commit: rendering can fail or be cancelled after it.
    pub fn plan(&self, scene: &ResolvedScene, xf: Affine, tiles: &[Tile], out: &mut DamagePlan) {
        let flags = &mut out.flags;
        flags.clear();
        flags.resize(tiles.len(), false);
        let mut full = !self.valid || self.xf != Some(xf);
        if !full {
            for (a, b) in self.paint.iter().zip(&scene.paint) {
                if a == b {
                    continue;
                }
                match (paint_bounds(a, xf), paint_bounds(b, xf)) {
                    (Some(a), Some(b)) => mark(flags, tiles, a.union(b)),
                    _ => {
                        full = true;
                        break;
                    }
                }
            }
            let common = self.paint.len().min(scene.paint.len());
            for paint in self.paint[common..]
                .iter()
                .chain(scene.paint[common..].iter())
            {
                if let Some(bounds) = paint_bounds(paint, xf) {
                    mark(flags, tiles, bounds);
                } else {
                    full = true;
                    break;
                }
            }
            // Live external uniforms can change while the paint list is equal.
            for (k, b) in scene.external_welds() {
                match self.external.get(k) {
                    Some(a) if a == b => {}
                    Some(a) => mark(
                        flags,
                        tiles,
                        external_bounds(a, xf).union(external_bounds(b, xf)),
                    ),
                    None => mark(flags, tiles, external_bounds(b, xf)),
                }
            }
            for (k, a) in &self.external {
                if scene.external_weld(k).is_none() {
                    mark(flags, tiles, external_bounds(a, xf));
                }
            }
        }
        if full {
            flags.fill(true);
        }
        out.dirty.clear();
        out.dirty
            .extend(flags.iter().enumerate().filter_map(|(i, f)| f.then_some(i)));
        out.dirty_pixels = out
            .dirty
            .iter()
            .map(|i| u64::from(tiles[*i].width) * u64::from(tiles[*i].height))
            .sum();
        out.total_tiles = tiles.len();
        out.full = full;
    }
    /// Call only after ALL dirty tiles and final presentation commands submit.
    /// Only entries that changed are copied, so committing an idle frame
    /// allocates nothing.
    pub fn commit(&mut self, scene: &ResolvedScene, xf: Affine) {
        copy_changed(&mut self.paint, &scene.paint);
        self.external
            .retain(|k, _| scene.external_weld(k).is_some());
        for (k, v) in scene.external_welds() {
            match self.external.get_mut(k) {
                Some(old) if old == v => {}
                Some(old) => old.clone_from(v),
                None => {
                    self.external.insert(k.to_owned(), v.clone());
                }
            }
        }
        self.xf = Some(xf);
        self.valid = true;
    }
}
/// Make `dst` equal `src`, cloning only the entries that differ: a paint list
/// kept across frames reallocates nothing for what stayed the same.
pub(crate) fn copy_changed(dst: &mut Vec<Painted>, src: &[Painted]) {
    dst.truncate(src.len());
    let kept = dst.len();
    for (old, new) in dst.iter_mut().zip(src) {
        if old != new {
            old.clone_from(new);
        }
    }
    dst.extend_from_slice(&src[kept..]);
}
pub fn tiles(size: [u32; 2], side: u32) -> Vec<Tile> {
    if size.contains(&0) || side == 0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut y = 0;
    while y < size[1] {
        let mut x = 0;
        while x < size[0] {
            out.push(Tile {
                x,
                y,
                width: side.min(size[0] - x),
                height: side.min(size[1] - y),
            });
            x = x.saturating_add(side);
        }
        y = y.saturating_add(side);
    }
    out
}
pub(crate) fn intersects(a: Rect, b: Rect) -> bool {
    a.x1 > b.x0 && a.y1 > b.y0 && a.x0 < b.x1 && a.y0 < b.y1
}
fn mark(flags: &mut [bool], tiles: &[Tile], rect: Rect) {
    for (f, t) in flags.iter_mut().zip(tiles) {
        *f |= intersects(t.rect(), rect);
    }
}
pub(crate) fn external_bounds(e: &ExternalWeld, xf: Affine) -> Rect {
    let b = e.bounds();
    xf.transform_rect_bbox(Rect::new(b.min.x, b.min.y, b.max.x, b.max.y))
        .inflate(2., 2.)
}
/// None means UNKNOWN extent or compositing state: retain it when replaying and
/// invalidate the whole target when it changes. Glyph overhang/shadow filters
/// must not be incorrectly cropped to their layout boxes.
pub(crate) fn paint_bounds(p: &Painted, xf: Affine) -> Option<Rect> {
    bounds(p, &crate::bez_path(&p.path, crate::ARC_TOLERANCE).ok()?, xf)
}
/// [`paint_bounds`] of an entry whose path is already converted to `bez`.
pub(crate) fn bounds(p: &Painted, bez: &BezPath, xf: Affine) -> Option<Rect> {
    if p.layer == Layer::Text {
        let text = p.text.as_ref()?;
        let em = f64::from(text.size);
        if !(em.is_finite() && em > 0.0 && text.origin.finite()) {
            return None;
        }
        // Glyph outlines intentionally stay out of `Painted`. Four em around
        // every shaped origin is conservative for italic overhangs, marks and
        // fallback faces: a neighbour may redraw, but changed ink cannot crop.
        let around =
            |x: f64, y: f64| Rect::new(x - 4.0 * em, y - 4.0 * em, x + 4.0 * em, y + 4.0 * em);
        let mut local = around(text.origin.x, text.origin.y);
        for glyph in text.glyphs.iter() {
            local = local.union(around(
                text.origin.x + f64::from(glyph.x),
                text.origin.y + f64::from(glyph.y),
            ));
        }
        let b = xf.transform_rect_bbox(local).inflate(2., 2.);
        return [b.x0, b.y0, b.x1, b.y1]
            .iter()
            .all(|v| v.is_finite())
            .then_some(b);
    }
    if matches!(
        p.layer,
        Layer::Clip
            | Layer::Unclip
            | Layer::Blend { .. }
            | Layer::Unblend
            | Layer::Mask
            | Layer::Shadow(_)
    ) {
        return None;
    }
    if bez.elements().is_empty() {
        return None;
    }
    let local = bez
        .bounding_box()
        .inflate(p.width.max(0.0), p.width.max(0.0));
    let b = xf.transform_rect_bbox(local).inflate(2., 2.);
    [b.x0, b.y0, b.x1, b.y1]
        .iter()
        .all(|v| v.is_finite())
        .then_some(b)
}
#[cfg(test)]
mod tests {
    use super::*;
    use mui_geometry::{Path, Point};
    use mui_scene::prelude::*;
    use mui_scene::{Paint, Text, TextGlyph};
    use std::sync::Arc;
    fn plan(t: &DamageTracker, s: &ResolvedScene, xf: Affine, tiles: &[Tile]) -> DamagePlan {
        let mut out = DamagePlan::default();
        t.plan(s, xf, tiles, &mut out);
        out
    }
    fn scene(colour: Role, x: f64) -> ResolvedScene {
        resolve_scene(&SceneSpec::new(
            leaf(40., 40.).fill(colour).id("x").offset(x, 0.),
        ))
        .unwrap()
    }
    #[test]
    fn first_frame_invalidates_all() {
        let t = tiles([512, 512], 128);
        let d = plan(
            &DamageTracker::default(),
            &scene(Primary, 0.),
            Affine::IDENTITY,
            &t,
        );
        assert_eq!(d.dirty.len(), 16);
    }
    #[test]
    fn no_change_means_no_tile_raster() {
        let s = scene(Primary, 0.);
        let mut c = DamageTracker::default();
        c.commit(&s, Affine::IDENTITY);
        assert!(plan(&c, &s, Affine::IDENTITY, &tiles([512, 512], 128))
            .dirty
            .is_empty());
    }
    #[test]
    fn colour_change_dirties_only_intersected_tiles() {
        let mut c = DamageTracker::default();
        // The default palette paints Primary and Secondary the same grey.
        c.commit(&scene(Primary, 0.), Affine::IDENTITY);
        let d = plan(
            &c,
            &scene(Ink, 0.),
            Affine::IDENTITY,
            &tiles([512, 512], 128),
        );
        assert_eq!(d.dirty, vec![0]);
    }
    #[test]
    fn appended_bounded_paint_does_not_invalidate_the_window() {
        let before = scene(Primary, 0.);
        let mut after = before.clone();
        let mut extra = after.paint[0].clone();
        extra.key = "added".into();
        extra.path = extra
            .path
            .rigid_transform(Point::new(300., 0.), 0.)
            .unwrap();
        after.paint.push(extra);
        let mut tracker = DamageTracker::default();
        tracker.commit(&before, Affine::IDENTITY);
        let damage = plan(&tracker, &after, Affine::IDENTITY, &tiles([512, 128], 128));
        assert!(!damage.full);
        assert_eq!(damage.dirty, vec![2]);
    }
    /// `commit` copies only what changed, so it must still leave the tracker
    /// equal to the scene: growing, shrinking and recolouring alike.
    #[test]
    fn an_incremental_commit_matches_the_scene() {
        let before = scene(Primary, 0.);
        let mut after = scene(Ink, 0.);
        let mut extra = after.paint[0].clone();
        extra.key = "added".into();
        after.paint.push(extra);
        let t = tiles([512, 512], 128);
        let mut c = DamageTracker::default();
        for s in [&before, &after, &before] {
            c.commit(s, Affine::IDENTITY);
            assert_eq!(c.paint, s.paint);
            assert!(plan(&c, s, Affine::IDENTITY, &t).dirty.is_empty());
        }
    }
    #[test]
    fn an_unchanged_entry_keeps_its_allocation() {
        let s = scene(Primary, 0.);
        let mut kept = s.paint.clone();
        let at = kept[0].path.commands.as_ptr();
        copy_changed(&mut kept, &s.paint);
        assert_eq!(kept[0].path.commands.as_ptr(), at);
        assert_eq!(kept, s.paint);
    }
    #[test]
    fn resize_grid_covers_the_target_without_overlap() {
        let t = tiles([777, 333], 256);
        assert_eq!(t.iter().map(|t| t.width * t.height).sum::<u32>(), 777 * 333);
    }
    #[test]
    fn cancelled_plan_is_not_committed() {
        let s = scene(Primary, 0.);
        let c = DamageTracker::default();
        plan(&c, &s, Affine::IDENTITY, &tiles([512, 512], 128));
        assert!(plan(&c, &s, Affine::IDENTITY, &tiles([512, 512], 128)).full);
    }
    #[test]
    fn transform_change_invalidates_all() {
        let s = scene(Primary, 0.);
        let mut c = DamageTracker::default();
        c.commit(&s, Affine::IDENTITY);
        assert!(plan(&c, &s, Affine::scale(2.), &tiles([512, 512], 128)).full);
    }
    #[test]
    fn text_has_bounded_damage() {
        let paint = Painted {
            key: "readout".into(),
            layer: Layer::Text,
            path: Path::default(),
            paint: Paint::Solid(Color::oklch(0.8, 0., 0.)),
            rect: None,
            width: 0.,
            blur: 0.,
            text: Some(Text {
                fonts: Arc::from([
                    mui_scene::Font::new(epaint_default_fonts::HACK_REGULAR).unwrap()
                ]),
                size: 12.,
                origin: Point::new(100., 80.),
                glyphs: Arc::from(
                    &[TextGlyph {
                        id: 1,
                        x: 24.,
                        y: -2.,
                        font: 0,
                    }][..],
                ),
                axes: Default::default(),
                hint: true,
                coords: Arc::from(&[][..]),
                font_coords: Arc::from(&[][..]),
            }),
        };
        let bounds = paint_bounds(&paint, Affine::IDENTITY).expect("text bounds");
        assert!(bounds.contains((100., 80.)));
        assert!(bounds.contains((124., 78.)));
        assert!(bounds.width() < 256. && bounds.height() < 256.);
    }
}
