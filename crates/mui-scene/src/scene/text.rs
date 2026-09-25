//! Shaping, line breaking and measuring, cached across resolves.
use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;

use mui_layout::Size;
use mui_text::{Axes, Font, TextRun};

use super::outline::OutlineCache;
use super::{SceneSpec, TextGlyph};
use crate::{Content, Element, Theme};

/// One shaped string, kept in [`TextCache`] while the font stays the same.
#[derive(Debug, Clone)]
pub(super) struct CachedRun {
    pub(super) advance: f64,
    pub(super) ascent: f64,
    pub(super) descent: f64,
    pub(super) line_height: f64,
    pub(super) glyphs: Arc<[TextGlyph]>,
    /// Per-char advances, shaped the first time this string wraps, so every
    /// later width breaks without shaping.
    pub(super) advances: Option<Arc<[(f64, bool)]>>,
}

impl CachedRun {
    pub(super) fn from_run(run: TextRun) -> Self {
        Self {
            advance: run.advance,
            ascent: run.ascent,
            descent: run.descent,
            line_height: run.line_height,
            glyphs: run
                .glyphs
                .into_iter()
                .map(|g| TextGlyph {
                    id: g.id,
                    x: g.x as f32,
                    y: g.y as f32,
                    font: g.font,
                })
                .collect(),
            advances: None,
        }
    }
}

/// Per-face normalized coordinates, primary face first. What a run is keyed
/// on: two axis settings that round to the same coordinates share a run.
pub(super) type Coords = Arc<[Arc<[i16]>]>;
/// (size bits, primary face id, coordinates).
pub(super) type RunKey = (u64, u64, Coords);
/// A run's key plus the width it broke at and its line cap.
pub(super) type BreakKey = (RunKey, u64, Option<usize>);
/// A paragraph's line byte ranges, and whether the cap cut lines off.
pub(super) type Breaks = (Vec<std::ops::Range<usize>>, bool);
/// Per string, per variant: the value and the resolve that last used it.
pub(super) type PerText<K, V> = HashMap<String, HashMap<K, (V, u64)>>;
/// Normalized coordinates per axes, then per (size bits, primary face id).
pub(super) type CoordsCache = HashMap<Axes, HashMap<(u64, Option<u64>), (Coords, u64)>>;

/// Everything shaped, broken and bent across frames. Own one in your runtime
/// and pass it to [`resolve_scene_with`](crate::resolve_scene_with). Nothing is flushed wholesale: an
/// entry the last resolve did not use is dropped at its end, so memory tracks
/// the live tree and a steady frame reshapes nothing.
#[derive(Debug, Default)]
pub struct TextCache {
    pub(super) layout: mui_layout::LayoutCache,
    /// The ids of the spec's faces the cache was filled with.
    pub(super) fonts: Vec<u64>,
    pub(super) generation: u64,
    pub(super) runs: PerText<RunKey, CachedRun>,
    /// Wrapped paragraphs' line breaks, keyed like `runs`.
    pub(super) breaks: PerText<BreakKey, Breaks>,
    /// Axis settings already normalized, so a frame does not re-derive the
    /// same coordinates per measure call. Keyed by axes first so a lookup
    /// borrows them.
    pub(super) coords: CoordsCache,
    /// The coordinates each text key drew with last frame, for `Text::hint`.
    pub(super) last_coords: HashMap<Arc<str>, (Coords, u64)>,
    /// Node keys by hash, reused while the node stays in the tree.
    pub(super) keys: HashMap<u64, (Arc<str>, u64)>,
    pub(super) outlines: OutlineCache,
    pub(super) borders: crate::border_ramp::BorderCache,
    pub(super) region_cache: crate::regions::RegionCache,
    pub(super) surface_cache: crate::surfaces::Cache,
}
impl TextCache {
    pub fn layout_stats(&self) -> mui_layout::LayoutStats {
        self.layout.stats()
    }
    /// Shaped runs held: one per (string, size, face, axes) variant.
    pub fn len(&self) -> usize {
        self.runs.values().map(HashMap::len).sum()
    }
    pub fn is_empty(&self) -> bool {
        // An inner map is never left empty, so no strings means no runs.
        self.runs.is_empty()
    }
    /// Start of a resolve: drop every shaped run when the faces they were
    /// shaped with changed.
    pub(super) fn retain_for(&mut self, spec: &SceneSpec) {
        let font_ids = spec.font.iter().chain(&spec.fallback_fonts).map(Font::id);
        if !self.fonts.iter().copied().eq(font_ids.clone()) {
            self.runs.clear();
            self.breaks.clear();
            self.coords.clear();
            self.last_coords.clear();
            self.fonts = font_ids.collect();
            self.layout.clear();
        }
    }
    /// End of a resolve: keep exactly what it used.
    pub(super) fn sweep(&mut self) {
        fn keep<K, V>(m: &mut HashMap<K, (V, u64)>, g: u64) -> bool {
            m.retain(|_, (_, seen)| *seen == g);
            !m.is_empty()
        }
        let g = self.generation;
        self.runs.retain(|_, m| keep(m, g));
        self.breaks.retain(|_, m| keep(m, g));
        self.coords.retain(|_, m| keep(m, g));
        keep(&mut self.last_coords, g);
        keep(&mut self.keys, g);
        self.generation = g.wrapping_add(1);
        self.outlines.sweep();
        self.borders.sweep();
        self.region_cache.sweep();
    }
}

/// What one text node is measured and shaped with.
#[derive(Clone, Copy)]
pub(super) struct Face<'a> {
    pub(super) size: f64,
    pub(super) axes: &'a Axes,
    /// The node's own face, if it set one; tried before the scene's fonts.
    pub(super) font: Option<&'a Font>,
}
impl<'a> Face<'a> {
    pub(super) fn of(e: &'a crate::Element, th: Theme) -> Self {
        Self {
            size: e.text_size.unwrap_or(th.text),
            axes: &e.axes,
            font: e.font.as_ref(),
        }
    }
}

pub(super) struct Runs<'a> {
    /// The scene's faces, shared into every `Text` that has no face of its own.
    pub(super) fonts: Arc<[Font]>,
    /// The last node face's chain, so a column of icons builds it once.
    pub(super) own_fonts: Option<(u64, Arc<[Font]>)>,
    /// [`SceneSpec::device_scale`]: a line measures the pitch the walk
    /// paints it at, not the face's raw line height.
    pub(super) scale: Option<f64>,
    pub(super) generation: u64,
    pub(super) cache: &'a mut PerText<RunKey, CachedRun>,
    pub(super) breaks: &'a mut PerText<BreakKey, Breaks>,
    pub(super) coords: &'a mut CoordsCache,
    pub(super) last_coords: &'a mut HashMap<Arc<str>, (Coords, u64)>,
}
impl<'a> Runs<'a> {
    /// The faces a node shapes with: its own first, then the scene's.
    pub(super) fn fonts_for(&mut self, face: Face<'_>) -> Arc<[Font]> {
        let Some(own) = face.font else {
            return self.fonts.clone();
        };
        if let Some((id, fonts)) = &self.own_fonts {
            if *id == own.id() {
                return fonts.clone();
            }
        }
        let fonts: Arc<[Font]> = std::iter::once(own).chain(&*self.fonts).cloned().collect();
        self.own_fonts = Some((own.id(), fonts.clone()));
        fonts
    }
    /// Per-face coordinates for `face`, memoised per (size, font, axes).
    pub(super) fn coords(&mut self, fonts: &[Font], face: Face<'_>) -> Coords {
        let key = (face.size.to_bits(), fonts.first().map(Font::id));
        let generation = self.generation;
        if let Some((c, seen)) = self.coords.get_mut(face.axes).and_then(|m| m.get_mut(&key)) {
            *seen = generation;
            return c.clone();
        }
        let settings = face.axes.to_vec();
        let coords: Coords = fonts
            .iter()
            .map(|f| {
                mui_text::normalized_coords(f, face.size, &settings)
                    .map_or_else(|_| Arc::from(&[][..]), Arc::from)
            })
            .collect::<Vec<_>>()
            .into();
        self.coords
            .entry(face.axes.clone())
            .or_default()
            .insert(key, (coords.clone(), generation));
        coords
    }
    /// Whether `key` drew at these coordinates last frame too. False only
    /// for the frame after an axis moved, which is what turns hinting off
    /// mid-tween; text seen for the first time counts as settled.
    pub(super) fn settled(&mut self, key: &Arc<str>, coords: &Coords) -> bool {
        match self
            .last_coords
            .insert(key.clone(), (coords.clone(), self.generation))
        {
            Some((last, _)) => last == *coords,
            None => true,
        }
    }
    pub(super) fn run(
        &mut self,
        text: &str,
        face: Face<'_>,
    ) -> Result<Option<&CachedRun>, mui_text::Error> {
        let fonts = self.fonts_for(face);
        let Some(font) = fonts.first() else {
            return Ok(None);
        };
        let coords = self.coords(&fonts, face);
        // Nested so a hit borrows `text` instead of allocating a key for it:
        // `run` is called several times per line, per frame.
        let key: RunKey = (face.size.to_bits(), font.id(), coords);
        let generation = self.generation;
        if self.cache.get(text).is_none_or(|m| !m.contains_key(&key)) {
            let settings = face.axes.to_vec();
            let run = CachedRun::from_run(mui_text::shape_run(&fonts, text, face.size, &settings)?);
            self.cache
                .entry(text.to_owned())
                .or_default()
                .insert(key.clone(), (run, generation));
        }
        Ok(self
            .cache
            .get_mut(text)
            .and_then(|m| m.get_mut(&key))
            .map(|(run, seen)| {
                *seen = generation;
                &*run
            }))
    }
    /// The lines `text` breaks into at `max` width, capped at `cap` of them
    /// with an ellipsis on the last. One line when it fits, or when there is
    /// no font to break against.
    ///
    /// Every line is a slice of `text`, so the overwhelmingly common case --
    /// a label that fits -- allocates the `Vec` and nothing else. Only an
    /// ellipsised last line owns its bytes. The breaks are cached like runs:
    /// a steady paragraph is broken once, not once per frame.
    pub(super) fn lines<'t>(
        &mut self,
        text: &'t str,
        face: Face<'_>,
        max: f64,
        cap: Option<usize>,
    ) -> Vec<Cow<'t, str>> {
        let one = || vec![Cow::Borrowed(text)];
        if max.is_nan() || max <= 0.0 || self.measure(text, face).width <= max + 0.5 {
            return one();
        }
        let fonts = self.fonts_for(face);
        let Some(font) = fonts.first() else {
            return one();
        };
        let key: BreakKey = (
            (face.size.to_bits(), font.id(), self.coords(&fonts, face)),
            max.to_bits(),
            cap,
        );
        let generation = self.generation;
        if self.breaks.get(text).is_none_or(|m| !m.contains_key(&key)) {
            // `measure` above cached the run this string wraps from.
            let Some((run, _)) = self.cache.get_mut(text).and_then(|m| m.get_mut(&key.0)) else {
                return one();
            };
            if run.advances.is_none() {
                let settings = face.axes.to_vec();
                let Ok(a) = mui_text::char_advances(&fonts, text, face.size, &settings) else {
                    return one();
                };
                run.advances = Some(a.into());
            }
            let advances = run.advances.as_deref().unwrap_or_default();
            let lines = mui_text::break_lines_from_advances(text, advances, max);
            let n = cap.unwrap_or(usize::MAX).max(1);
            let ranges = lines.iter().take(n).map(|l| l.text_range.clone()).collect();
            self.breaks
                .entry(text.to_owned())
                .or_default()
                .insert(key.clone(), ((ranges, lines.len() > n), generation));
        }
        let Some(((ranges, cut), seen)) = self.breaks.get_mut(text).and_then(|m| m.get_mut(&key))
        else {
            return one();
        };
        *seen = generation;
        let mut out: Vec<Cow<'t, str>> = ranges
            .iter()
            .map(|r| Cow::Borrowed(text[r.clone()].trim_end()))
            .collect();
        if *cut {
            // ponytail: the ellipsis is appended, not measured -- a capped
            // line can overhang by one glyph. Re-break the last line against
            // `max - advance('…')` if that shows.
            if let Some(last) = out.last_mut() {
                last.to_mut().push('\u{2026}');
            }
        }
        if out.is_empty() {
            out.push(Cow::Borrowed(""));
        }
        out
    }
    /// A wrapped label's height. Every line has the one pitch its face sets,
    /// whatever its text, so no line is shaped to find it.
    pub(super) fn wrapped(
        &mut self,
        text: &str,
        face: Face<'_>,
        max: f64,
        cap: Option<usize>,
    ) -> f64 {
        let lines = self.lines(text, face, max, cap).len();
        lines as f64 * self.measure(text, face).height
    }
    pub(super) fn measure(&mut self, text: &str, face: Face<'_>) -> Size {
        let size = face.size;
        match self.run(text, face) {
            // ponytail: no font → a monospace guess, so layout tests stay
            // font-free. Wrong widths are visible the moment a font is set.
            Ok(None) | Err(_) => Size::new(text.chars().count() as f64 * size * 0.6, size * 1.25),
            // The snapped pitch the walk stacks lines at, as egui rounds each
            // row to a device pixel: a raw 15.6 pt Barlow line at 1.5x would
            // otherwise measure 0.27 pt taller than it paints, per line.
            Ok(Some(r)) => Size::new(r.advance, super::snap(r.line_height, self.scale)),
        }
    }
}

/// Exactly the fields [`fit`] consumes, as bytes the layout cache compares
/// in full. Strings carry their length, so no two payloads share an
/// encoding, and nothing is formatted.
pub(super) fn layout_key(e: &Element, th: Theme, scale: Option<f64>, out: &mut Vec<u8>) {
    let Content::Text(t) = &e.content else {
        return;
    };
    let mut bytes = |b: &[u8]| {
        out.extend_from_slice(&b.len().to_le_bytes());
        out.extend_from_slice(b);
    };
    bytes(t.as_bytes());
    match &e.reserve {
        Some(r) => bytes(r.as_bytes()),
        None => out.extend_from_slice(&u64::MAX.to_le_bytes()),
    }
    // Four-byte tags, so the count is all the framing they need.
    out.extend_from_slice(&e.axes.iter().count().to_le_bytes());
    for (tag, value) in e.axes.iter() {
        out.extend_from_slice(tag.as_bytes());
        out.extend_from_slice(&value.to_bits().to_le_bytes());
    }
    out.extend_from_slice(&e.text_size.unwrap_or(th.text).to_bits().to_le_bytes());
    // 0 is "no face of its own"; ids shift up one past it.
    out.extend_from_slice(&e.font.as_ref().map_or(0, |f| f.id() + 1).to_le_bytes());
    // usize::MAX is "no cap".
    out.extend_from_slice(&e.lines.unwrap_or(usize::MAX).to_le_bytes());
    // Line heights snap to the device scale, so a scale change remeasures.
    out.extend_from_slice(&scale.map_or(u64::MAX, f64::to_bits).to_le_bytes());
}

/// A content leaf's size: a paragraph wrapped to its room when it needs it.
pub(super) fn fit(runs: &mut Runs, th: Theme, e: &crate::Element, room: Option<f64>) -> Size {
    let Content::Text(t) = &e.content else {
        return Size::ZERO;
    };
    let (t, face) = (t.as_str(), Face::of(e, th));
    let mut fit = match room {
        // The room it wrapped into, not its longest line: a paragraph that
        // reported the ragged width would then be centred inside its own
        // column, aligned with nothing above it.
        Some(room) if room > 0.0 && runs.measure(t, face).width > room + 0.5 => {
            Size::new(room, runs.wrapped(t, face, room, e.lines))
        }
        _ => runs.measure(t, face),
    };
    // The reserved string widens the box and nothing else: its own height is
    // the same line at the same size, and a longer value still measures long.
    if let Some(r) = &e.reserve {
        fit.width = fit.width.max(runs.measure(r, face).width);
    }
    fit
}
#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::prelude::*;

    #[test]
    fn caches_keep_only_what_the_last_resolve_used() {
        let tree = |n: usize, tag: &str| {
            column((0..n).map(|i| {
                row([
                    leaf(12., 12.),
                    text(format!("{tag}{i}")).id(format!("{tag}{i}")),
                ])
                .union(Role::Surface)
            }))
        };
        let spec = |n, tag| {
            SceneSpec::new(tree(n, tag))
                .offered(Size::new(200., 8192.))
                .font(Font::new(epaint_default_fonts::HACK_REGULAR).unwrap())
        };
        let mut text = TextCache::default();
        resolve_scene_with(&spec(300, "a"), &mut text).unwrap();
        assert_eq!(
            text.outlines.entries.len(),
            300,
            "no cap flushes a big tree"
        );
        resolve_scene_with(&spec(1, "b"), &mut text).unwrap();
        assert_eq!(text.outlines.entries.len(), 1);
        assert_eq!(text.len(), 1, "only b0's run is left");
        assert_eq!(text.last_coords.len(), 1, "only b0's hinting state is left");
    }

    #[test]
    fn a_steady_paragraph_reuses_its_line_breaks() {
        let para = "one two three four five six seven eight nine ten";
        let spec = SceneSpec::new(col![text(para).id("p")].w(80.))
            .font(Font::new(epaint_default_fonts::HACK_REGULAR).unwrap());
        let mut text = TextCache::default();
        let lines = |s: &ResolvedScene| s.paint.iter().filter(|p| p.layer == Layer::Text).count();
        assert!(lines(&resolve_scene_with(&spec, &mut text).unwrap()) > 1);
        // Doctor the cached breaks: a resolve that re-broke would not see it.
        for ((ranges, _), _) in text.breaks.values_mut().flat_map(|m| m.values_mut()) {
            *ranges = std::iter::once(0..para.len()).collect();
        }
        assert_eq!(lines(&resolve_scene_with(&spec, &mut text).unwrap()), 1);
    }

    #[test]
    fn a_wrapped_paragraph_fills_its_column_instead_of_its_longest_line() {
        let long = "wrap ".repeat(40);
        let root = row([
            column([text("About").text_size(18.).id("h"), text(long).id("p")])
                .gap(6.)
                .flex(1.)
                .id("col"),
            leaf(90., 40.).shrink(0.),
        ])
        .gap(10.);
        let mut sp = SceneSpec::new(root).offered(Size::new(320., 200.));
        sp.font = Some(font());
        let s = resolve_scene(&sp).unwrap();
        let (col, p) = (s.layout.frame("col").unwrap(), s.layout.frame("p").unwrap());
        assert_eq!(
            (p.x, p.size.width),
            (col.x, col.size.width),
            "{p:?} {col:?}"
        );
    }

    #[test]
    fn the_same_paragraph_wraps_to_each_width_it_is_given_and_a_row_share_too() {
        let long = "wrap ".repeat(40);
        // Two copies of one string in two widths: each wraps to its own,
        // so the wider one is shorter. A third beside a sibling in a
        // definite row gets its flex share, narrower than the row.
        let root = column([
            column([text(long.clone()).id("a")]).w(120),
            column([text(long.clone()).id("b")]).w(240),
            row([
                text(long.clone()).id("c").shrink(1.0),
                text(long).id("d").shrink(1.0),
            ])
            .w(300),
        ]);
        let mut sp = SceneSpec::new(root);
        sp.font = Some(font());
        let s = resolve_scene(&sp).unwrap();
        let f = |k| s.layout.frame(k).unwrap().size;
        assert!(
            f("a").width <= 120.1 && f("b").width <= 240.1,
            "{:?} {:?}",
            f("a"),
            f("b")
        );
        assert!(
            f("a").height > f("b").height * 1.5,
            "{:?} {:?}",
            f("a"),
            f("b")
        );
        assert!(
            f("c").width <= 150.1 && f("c").height > f("b").height,
            "{:?}",
            f("c")
        );
        let lines = |k| {
            s.paint
                .iter()
                .filter(|p| &*p.key == k && p.layer == Layer::Text)
                .count()
        };
        assert!(
            lines("a") > lines("b") && lines("c") > lines("b"),
            "{} {} {}",
            lines("a"),
            lines("b"),
            lines("c")
        );
    }

    #[test]
    fn a_narrow_column_wraps_a_paragraph_and_grows_taller() {
        let long = "wrap ".repeat(40);
        let one = {
            let mut sp = SceneSpec::new(column([text(long.clone()).id("t")]));
            sp.font = Some(font());
            resolve_scene(&sp).unwrap().layout.frame("t").unwrap().size
        };
        let mut sp = SceneSpec::new(column([text(long.clone()).id("t")]).w(120));
        sp.font = Some(font());
        let s = resolve_scene(&sp).unwrap();
        let lines = s
            .paint
            .iter()
            .filter(|p| &*p.key == "t" && p.layer == Layer::Text)
            .count();
        assert!(lines > 3, "wrapped into {lines} lines");
        let f = s.layout.frame("t").unwrap().size;
        assert!(f.width <= 120.1 && f.height > one.height * 3., "{f:?}");
        let mut capped = sp.clone();
        capped.root = column([text(long).id("t").lines(2)]).w(120);
        let c = resolve_scene(&capped).unwrap();
        assert_eq!(
            c.paint
                .iter()
                .filter(|p| &*p.key == "t" && p.layer == Layer::Text)
                .count(),
            2,
            "capped at two lines"
        );
    }

    #[test]
    fn text_cache_keys_on_size_as_well_as_string() {
        let mut cache = TextCache::default();
        let mut sp = SceneSpec::new(row([
            text("hi").text_size(12.).id("a"),
            text("hi").text_size(24.).id("b"),
        ]));
        sp.font = Some(font());
        resolve_scene_with(&sp, &mut cache).unwrap();
        assert_eq!(cache.len(), 2, "one string, two sizes");
    }

    #[test]
    fn weight_reaches_the_run_and_keys_the_cache() {
        let mut cache = TextCache::default();
        let mut sp = SceneSpec::new(row![
            text("hi").id("a"),
            text("hi").text_weight(Weight::BOLD).id("b")
        ]);
        sp.font = Some(Font::new(ttf_inter::REGULAR).unwrap());
        resolve_scene_with(&sp, &mut cache).unwrap();
        // Same string, two weights of a variable face: two shaped runs, not
        // one reused at the wrong instance.
        assert_eq!(cache.len(), 2);
        // A static face shapes the same at any weight, so it shares one.
        let mut hack = TextCache::default();
        sp.font = Some(Font::new(epaint_default_fonts::HACK_REGULAR).unwrap());
        resolve_scene_with(&sp, &mut hack).unwrap();
        assert_eq!(hack.len(), 1);
        sp.font = Some(Font::new(ttf_inter::REGULAR).unwrap());
        let s = resolve_scene_with(&sp, &mut cache).unwrap();
        let w: Vec<Option<f32>> = s
            .paint
            .iter()
            .filter(|p| p.layer == Layer::Text)
            .filter_map(|p| p.text.as_ref().map(|t| t.axes.get("wght")))
            .collect();
        assert_eq!(w, [None, Some(700.)]);
    }

    #[test]
    fn hinting_pauses_for_the_frame_after_an_axis_moves() {
        let inter = Font::new(ttf_inter::REGULAR).unwrap();
        let hint_at = |w: f32, cache: &mut TextCache| {
            let mut sp = SceneSpec::new(row([text("hi").text_axis("wght", w).id("t")]));
            sp.font = Some(inter.clone());
            let s = resolve_scene_with(&sp, cache).unwrap();
            s.paint.iter().find_map(|p| p.text.as_ref()).unwrap().hint
        };
        let mut cache = TextCache::default();
        assert!(hint_at(400., &mut cache), "first frame is settled");
        assert!(hint_at(400., &mut cache));
        assert!(!hint_at(500., &mut cache), "the frame it moved on");
        assert!(hint_at(500., &mut cache), "settled again");
        // Two settings that round to the same coordinates are one instance.
        assert!(hint_at(500.001, &mut cache));
    }

    #[test]
    fn text_cache_survives_frames_and_carries_glyphs() {
        let mut cache = TextCache::default();
        let mut sp = SceneSpec::new(row([text("hi").id("t")]));
        sp.font = Some(Font::new(epaint_default_fonts::HACK_REGULAR).unwrap());
        let s = resolve_scene_with(&sp, &mut cache).unwrap();
        assert_eq!(cache.len(), 1);
        let t = s
            .paint
            .iter()
            .find(|p| p.layer == Layer::Text)
            .unwrap()
            .text
            .as_ref()
            .unwrap();
        assert_eq!(t.glyphs.len(), 2);
        assert!(t.glyphs[1].x > 0.);
        let next = resolve_scene_with(&sp, &mut cache).unwrap();
        let next_text = next.paint.iter().find_map(|p| p.text.as_ref()).unwrap();
        assert!(Arc::ptr_eq(&t.glyphs, &next_text.glyphs));
        assert_eq!(cache.len(), 1);
        assert!(
            s.paint == next.paint,
            "unchanged text must retain its paint"
        );
        let mut changed_face = next_text.clone();
        changed_face.fonts = next_text
            .fonts
            .iter()
            .map(|font| Font::new(font.as_ref().to_vec()).unwrap())
            .collect();
        assert!(t != &changed_face, "a different Font invalidates paint");
    }
}
