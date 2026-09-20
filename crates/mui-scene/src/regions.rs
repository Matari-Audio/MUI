//! Shape-aware layout: ordinary rows, columns and stacks inherit an inset contour.
use crate::{BorderAlign, El, SceneError, Spacing};
use mui_geometry::{
    boolean_paths, inset_path, union_contours, BooleanOp, Bounds, GeometryOptions, OffsetOptions,
    Path, ShapeSplit,
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
}
#[derive(Debug, Default)]
pub(crate) struct RegionCache(
    std::collections::HashMap<(usize, u8), (Operation, OffsetOptions, GeometryOptions, Path)>,
);
impl RegionCache {
    pub(crate) fn resolve(
        &mut self,
        key: (usize, u8),
        op: Operation,
        o: OffsetOptions,
        g: GeometryOptions,
    ) -> Result<Path, SceneError> {
        if let Some((old, offsets, geometry, path)) = self.0.get(&key) {
            if old == &op && *offsets == o && *geometry == g {
                return Ok(path.clone());
            }
        }
        let path = match &op {
            Operation::Inset(p, d) => inset_path(p, *d, o)?.path,
            Operation::Combine(a, b, op) => boolean_paths(a, b, *op, o, g)?,
            Operation::Sweep(p) => union_contours(p, o, g)?,
            Operation::SplitMask(bounds, split, second) => split.mask(*bounds, *second, o)?,
        };
        if self.0.len() >= 256 {
            self.0.clear();
        }
        self.0.insert(key, (op, o, g, path.clone()));
        Ok(path)
    }
}
