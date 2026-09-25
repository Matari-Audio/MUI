//! Shape-aware layout: ordinary rows, columns and stacks inherit an inset contour.
use crate::{BorderAlign, El, SceneError, Spacing};
use mui_geometry::{
    boolean_paths, inset_path, union_contours, BooleanOp, Bounds, GeometryOptions, OffsetOptions,
    Path, PathCommand, Point, ShapeSplit,
};

pub trait ShapeLayout: Sized {
    /// Children fill shares of the final contour. Padding and total sibling gap
    /// default to the same distance; a subsequent `.gap(...)` overrides the gap.
    /// Nest freely; `.grow(weight)` retains normal layout semantics.
    ///
    /// ```
    /// use mui_scene::prelude::*;
    /// let panel = row![
    ///     stack![].flex(1.).fill(Primary),
    ///     col![stack![].flex(1.), stack![].flex(1.)]
    ///         .inside(2.).flex(1.).fill(Secondary),
    /// ].inside(2.).radius(20.).w(200.).h(100.);
    /// resolve_scene(&SceneSpec::new(panel)).unwrap();
    /// ```
    fn inside(self, padding: impl Into<Spacing>) -> Self;
    /// Bow a two-way split. Signed fraction of the cross-divider extent;
    /// zero is straight. Limited to +/-0.45; narrow shares can disappear.
    fn bend(self, amount: f64) -> Self;
    fn border_align(self, alignment: BorderAlign) -> Self;
}
impl ShapeLayout for El {
    fn inside(mut self, padding: impl Into<Spacing>) -> Self {
        let padding = padding.into();
        self.payload_mut().inside = Some(padding);
        self.gap(padding)
    }
    fn bend(mut self, amount: f64) -> Self {
        self.payload_mut().bend = amount;
        self
    }
    fn border_align(mut self, alignment: BorderAlign) -> Self {
        self.payload_mut().border_align = alignment;
        self
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Operation {
    Inset(Path, f64),
    Combine(Path, Path, BooleanOp),
    /// A nonzero sweep consists of overlapping solid pieces, not even-odd holes.
    Sweep(Path),
    SplitMask(Bounds, ShapeSplit, bool),
    /// The band of a uniform stroke on a path outline.
    Border(Path, f64, BorderAlign),
}
impl Operation {
    /// Where the operation's local space starts: its first point. Every
    /// operation commutes with translation, so a panel that moves or
    /// scrolls keeps its cached geometry.
    fn origin(&self) -> Point {
        let first = |p: &Path| match p.commands.first() {
            Some(PathCommand::MoveTo(p)) => *p,
            _ => Point::ZERO,
        };
        match self {
            Self::Inset(p, _) | Self::Combine(p, ..) | Self::Sweep(p) | Self::Border(p, ..) => {
                first(p)
            }
            Self::SplitMask(b, ..) => b.min,
        }
    }
    /// Equal but for the last bits a translation rounds away: one input
    /// that moved and came back is the same input.
    fn near(&self, other: &Self) -> bool {
        const TOL: f64 = 1e-9;
        match (self, other) {
            (Self::Inset(a, d), Self::Inset(b, e)) => d == e && a.near(b, TOL),
            (Self::Combine(a, b, op), Self::Combine(c, d, oq)) => {
                op == oq && a.near(c, TOL) && b.near(d, TOL)
            }
            (Self::Sweep(a), Self::Sweep(b)) => a.near(b, TOL),
            (Self::Border(a, w, x), Self::Border(b, v, y)) => w == v && x == y && a.near(b, TOL),
            (a, b) => a == b,
        }
    }
    fn translate(&mut self, d: Point) {
        match self {
            Self::Inset(p, _) | Self::Sweep(p) | Self::Border(p, ..) => p.translate(d),
            Self::Combine(a, b, _) => {
                a.translate(d);
                b.translate(d);
            }
            Self::SplitMask(b, ..) => {
                b.min = b.min + d;
                b.max = b.max + d;
            }
        }
    }
}
/// Region-cache slots a node's own stroke and ramp borders share.
pub(crate) const STROKE_BAND: u8 = 10;
pub(crate) const RAMP_BAND: u8 = 11;
/// An outside ramp's band less the outline it wraps.
pub(crate) const RAMP_OUTSIDE: u8 = 12;
/// The first of a node's shells; shell `i` is `SHELL + i`.
pub(crate) const SHELL: u8 = 32;
/// Local-space input and output, whether an inset changed ring counts, and
/// the resolve that last used it.
type Entry = (Operation, OffsetOptions, GeometryOptions, Path, bool, u64);
/// Region geometry per (node identity, step). Keyed by identity, not
/// pre-order index: a tooltip or menu wrapping the root shifts every index
/// but leaves keys and geometry untouched. Stored in the operation's local
/// space (see [`Operation::origin`]), so moving it is a hit too. An entry is
/// reused only when its whole input compares equal, and dropped by the first
/// resolve that does not use it.
#[derive(Debug, Default)]
pub(crate) struct RegionCache {
    entries: rustc_hash::FxHashMap<(std::sync::Arc<str>, u8), Entry>,
    generation: u64,
}
impl RegionCache {
    pub(crate) fn resolve(
        &mut self,
        key: (std::sync::Arc<str>, u8),
        op: Operation,
        o: OffsetOptions,
        g: GeometryOptions,
    ) -> Result<Path, SceneError> {
        Ok(self.resolve_counted(key, op, o, g)?.0)
    }
    /// [`RegionCache::resolve`], and whether an inset changed ring counts.
    pub(crate) fn resolve_counted(
        &mut self,
        key: (std::sync::Arc<str>, u8),
        mut op: Operation,
        o: OffsetOptions,
        g: GeometryOptions,
    ) -> Result<(Path, bool), SceneError> {
        let origin = op.origin();
        op.translate(-origin);
        let world = |mut p: Path| {
            p.translate(origin);
            p
        };
        if let Some((old, offsets, geometry, path, changed, seen)) = self.entries.get_mut(&key) {
            if old.near(&op) && *offsets == o && *geometry == g {
                *seen = self.generation;
                return Ok((world(path.clone()), *changed));
            }
        }
        let (path, changed) = match &op {
            Operation::Inset(p, d) => {
                let inset = inset_path(p, *d, o)?;
                (inset.path, inset.counts_changed)
            }
            Operation::Combine(a, b, op) => (boolean_paths(a, b, *op, o, g)?, false),
            Operation::Sweep(p) => (union_contours(p, o, g)?, false),
            Operation::SplitMask(bounds, split, second) => {
                (split.mask(*bounds, *second, o)?, false)
            }
            Operation::Border(p, width, align) => (
                mui_geometry::border_band(
                    p,
                    mui_geometry::WidthProfile::uniform(*width),
                    *align,
                    o,
                    g,
                )?,
                false,
            ),
        };
        self.entries
            .insert(key, (op, o, g, path.clone(), changed, self.generation));
        Ok((world(path), changed))
    }
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }
    pub(crate) fn sweep(&mut self) {
        let generation = self.generation;
        self.entries.retain(|_, e| e.5 == generation);
        self.generation = generation.wrapping_add(1);
    }
}
