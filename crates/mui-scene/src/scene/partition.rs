//! `.inside(..)`: a parent's interior partitioned into its children's regions.
use std::ops::Range;
use std::sync::Arc;

use mui_geometry::{BooleanOp, Path, Point, Rect, RoundedRect};
use mui_layout::{Frame, Insets, Size};

use super::{SceneError, Walk, bounds, find, fit, missing};
use crate::regions::Operation;
use crate::{El, Element, Radius};

impl Walk<'_> {
    /// Carve `n`'s interior -- its outline less its border and padding --
    /// into one region per in-flow child, and lay each child out again
    /// inside its region's bounds. A no-op for a node without `.inside(..)`.
    pub(super) fn partition(
        &mut self,
        n: &El,
        outline: &Path,
        frame: Frame,
        at: usize,
        (key, parent): (&crate::Id, &str),
    ) -> Result<(), SceneError> {
        let e = n.payload();
        let Some(padding) = e.extras().inside else {
            if e.bend != 0. {
                return Err(mui_geometry::Error::InvalidOptions("bend requires inside").into());
            }
            return Ok(());
        };
        let padding = padding.resolve(self.spec.theme.spacing);
        if !padding.is_finite() || padding < 0. || !e.bend.is_finite() || e.bend.abs() > 0.45 {
            return Err(mui_geometry::Error::InvalidOptions("shape padding/bend").into());
        }
        if e.extras().welding.is_some() {
            return Err(SceneError::UnsupportedWeld(
                "inside takes a vector union, not a material weld",
            ));
        }
        let interior = self.interior(n, outline, frame, (key, at), padding)?;
        let end = at + self.tree.sizes[at];
        let Some(b) = self.flat_bounds(&interior)? else {
            self.collapse(at + 1..end);
            return Ok(());
        };
        // The interior is already inside the padding.
        self.relayout(n, Insets::ZERO, b, at + 1..end, 1)?;
        let mut next = at + 1;
        let children: Vec<_> = n
            .children()
            .iter()
            .enumerate()
            .filter_map(|(j, c)| {
                let i = next;
                next += self.tree.sizes[i];
                (!c.is_float() && c.payload().carve.is_none()).then_some((i, c, j))
            })
            .collect();
        let masks = self.masks(e, key, b, &children)?;
        for ((i, child, j), mask) in children.into_iter().zip(masks) {
            // The same identity `node` gives this child when it walks it.
            let id = super::child_key(child, parent, j);
            self.region((i, &id), child, &interior, mask)?;
        }
        Ok(())
    }

    /// `n`'s outline less its border -- a ramp's band or the stroke's
    /// inward width -- and then less `padding`.
    fn interior(
        &mut self,
        n: &El,
        outline: &Path,
        frame: Frame,
        (key, at): (&crate::Id, usize),
        padding: f64,
    ) -> Result<Path, SceneError> {
        let e = n.payload();
        let mut interior = outline.clone();
        if let Some(ramp) = &e.extras().border_ramp {
            ramp.validate()?;
            let anchor = match &ramp.anchor {
                None => frame,
                Some(id) => {
                    self.tree.frames[find(n, id.as_str(), at, &self.tree.sizes)
                        .ok_or_else(|| missing("border ramp anchor", id))?]
                }
            };
            if anchor.size.width <= 0. {
                return Err(
                    mui_geometry::Error::InvalidOptions("border ramp anchor has no width").into(),
                );
            }
            let anchor = *self.plan.ramp_anchors.get_or_insert(at, anchor);
            for id in ramp.tabs.iter().chain(&ramp.dividers) {
                let index = find(n, id.as_str(), at, &self.tree.sizes)
                    .ok_or_else(|| missing("border ramp tab", id))?;
                self.plan
                    .ramp_frames
                    .insert((at, id.clone()), self.tree.frames[index]);
            }
            let mut inward = ramp.clone();
            inward.from.1 *= ramp.align.inward();
            inward.to.1 *= ramp.align.inward();
            if inward.from.1.max(inward.to.1) > 0. {
                let mut band = self.caches.borders.band(
                    &format!("inside/{key}"),
                    outline,
                    &inward,
                    anchor,
                    0.1 / self.spec.device_scale.unwrap_or(1.),
                )?;
                let shoulder = match e.style.radius.unwrap_or_default() {
                    Radius::Pair(_, r) => r,
                    Radius::Scale(k) => self.spec.theme.corners.concave * k,
                    _ => self.spec.theme.corners.concave,
                };
                crate::border_ramp::decorate(&mut band, ramp, anchor, shoulder, |id| {
                    Ok(self.plan.ramp_frames[&(at, id.clone())])
                })?;
                let band = self.cached_region((key.clone(), 0), Operation::Sweep(band))?;
                interior = self.cached_region(
                    (key.clone(), 1),
                    Operation::Combine(interior, band, BooleanOp::Difference),
                )?;
            }
        } else if let Some(stroke) = &e.style.stroke {
            let width = stroke.width.unwrap_or(self.spec.theme.stroke_width);
            if !width.is_finite() || width < 0. {
                return Err(SceneError::InvalidRadius);
            }
            interior = self.cached_region(
                (key.clone(), 2),
                Operation::Inset(interior, width * e.border_align.inward()),
            )?;
        }
        self.cached_region((key.clone(), 3), Operation::Inset(interior, padding))
    }

    /// Each child's share of the interior's bounds `b`: its own frame, or
    /// the two halves of a bent split.
    fn masks(
        &mut self,
        e: &Element,
        key: &crate::Id,
        b: Rect,
        children: &[(usize, &El, usize)],
    ) -> Result<Vec<Path>, SceneError> {
        let mut masks: Vec<Path> = children
            .iter()
            .map(|(i, ..)| {
                if self.tree.frames[*i].size.width <= 0. || self.tree.frames[*i].size.height <= 0. {
                    Ok(Path::default())
                } else {
                    RoundedRect::new(bounds(self.tree.frames[*i], None), 0.)
                        .map(mui_geometry::RoundedRect::path)
                }
            })
            .collect::<Result<_, _>>()?;
        if e.bend == 0. {
            return Ok(masks);
        }
        if children.len() != 2 {
            return Err(mui_geometry::Error::InvalidOptions("bend requires two siblings").into());
        }
        let size = Size::new(b.x1 - b.x0, b.y1 - b.y0);
        let a = self.tree.frames[children[0].0];
        let z = self.tree.frames[children[1].0];
        let horizontal = z.x >= a.right() - 1e-6;
        if !horizontal && z.y < a.bottom() - 1e-6 {
            return Err(
                mui_geometry::Error::InvalidOptions("bend requires a row or column").into(),
            );
        }
        let (axis, cut, gap) = if horizontal {
            (
                mui_geometry::SplitAxis::X,
                ((a.right() + z.x) * 0.5 - b.x0) / size.width,
                z.x - a.right(),
            )
        } else {
            (
                mui_geometry::SplitAxis::Y,
                ((a.bottom() + z.y) * 0.5 - b.y0) / size.height,
                z.y - a.bottom(),
            )
        };
        let split = mui_geometry::ShapeSplit::new(axis, cut)
            .gap(gap.max(0.))
            .bend(e.bend);
        for (side, mask) in masks.iter_mut().enumerate() {
            *mask = self.cached_region(
                (key.clone(), 4 + side as u8),
                Operation::SplitMask(b, split, side == 1),
            )?;
        }
        Ok(masks)
    }

    /// Child `i`'s region: the interior within its mask, less any border it
    /// draws outward. The child is laid out again inside the region.
    fn region(
        &mut self,
        (i, id): (usize, &crate::Id),
        child: &El,
        interior: &Path,
        mask: Path,
    ) -> Result<(), SceneError> {
        let path = self.cached_region(
            (id.clone(), 6),
            Operation::Combine(interior.clone(), mask, BooleanOp::Intersection),
        )?;
        // A child's outward border belongs inside its allocation too. Reserve
        // it before fitting the child, and retain the allocation as a paint cap.
        let path = Arc::new(path);
        self.plan.region_envelopes.insert(i, path.clone());
        let mut path = path;
        if let Some(ramp) = &child.payload().extras().border_ramp {
            ramp.validate()?;
            let outward = 1. - ramp.align.inward();
            if outward > 0. && ramp.from.1.max(ramp.to.1) > 0. {
                if ramp
                    .anchor
                    .as_ref()
                    .is_some_and(|id| child.key() != Some(id.as_str()))
                {
                    return Err(mui_geometry::Error::InvalidOptions(
                        "outward region border anchor must be the child",
                    )
                    .into());
                }
                let anchor = self.tree.frames[i];
                self.plan.ramp_anchors.insert(i, anchor);
                let mut sweep = ramp.clone();
                sweep.from.1 *= outward;
                sweep.to.1 *= outward;
                let band = self.caches.borders.band(
                    &format!("outside/{id}"),
                    &path,
                    &sweep,
                    anchor,
                    0.1 / self.spec.device_scale.unwrap_or(1.),
                )?;
                let band = self.cached_region((id.clone(), 8), Operation::Sweep(band))?;
                path = Arc::new(self.cached_region(
                    (id.clone(), 9),
                    Operation::Combine(Arc::unwrap_or_clone(path), band, BooleanOp::Difference),
                )?);
            }
        } else if let Some(stroke) = &child.payload().style.stroke {
            let width = stroke.width.unwrap_or(self.spec.theme.stroke_width);
            if !width.is_finite() || width < 0. {
                return Err(SceneError::InvalidRadius);
            }
            let outward = width * (1. - child.payload().border_align.inward());
            if outward > 0. {
                path = Arc::new(self.cached_region(
                    (id.clone(), 9),
                    Operation::Inset(Arc::unwrap_or_clone(path), outward),
                )?);
            }
        }
        let end = i + self.tree.sizes[i];
        match self.flat_bounds(&path)? {
            Some(b) => {
                let padding = child.padding(self.spec.theme.spacing);
                self.relayout(child, padding, b, i..end, 0)?;
            }
            None => self.collapse(i..end),
        }
        self.plan.regions.insert(i, (path, Point::ZERO));
        Ok(())
    }

    /// Lay `root` out again as exactly `b`, padded by `padding`, and move
    /// the frames in `range` to where it put its nodes past the first `skip`.
    fn relayout(
        &mut self,
        root: &El,
        padding: Insets,
        b: Rect,
        range: Range<usize>,
        skip: usize,
    ) -> Result<(), SceneError> {
        let size = Size::new(b.x1 - b.x0, b.y1 - b.y0);
        let th = self.spec.theme;
        let layout = mui_layout::resolve_boxed_with(
            root,
            size,
            padding,
            self.spec.limits,
            th.spacing,
            |e, room| fit(&mut self.runs, th, e, room),
        )?;
        for (dest, f) in self.tree.frames.to_mut()[range]
            .iter_mut()
            .zip(&layout.all()[skip..])
        {
            *dest = Frame {
                x: f.x + b.x0,
                y: f.y + b.y0,
                size: f.size,
            };
        }
        Ok(())
    }

    /// A subtree with no room: every frame in `range` paints nothing.
    fn collapse(&mut self, range: Range<usize>) {
        for f in &mut self.tree.frames.to_mut()[range] {
            f.size = Size::ZERO;
        }
    }

    fn flat_bounds(&self, path: &Path) -> Result<Option<Rect>, SceneError> {
        let o = self.spec.offsets;
        Ok(mui_geometry::bounds(
            path.flatten(o.flatten_tolerance, o.max_points)?.concat(),
        ))
    }

    /// Region geometry for step `key`, reused while its inputs match.
    pub(super) fn cached_region(
        &mut self,
        key: (crate::Id, u8),
        op: Operation,
    ) -> Result<Path, SceneError> {
        self.caches
            .regions
            .resolve(key, op, self.spec.offsets, self.spec.geometry)
    }
}

#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::prelude::*;

    #[test]
    fn inside_regions_past_the_old_flush_limit_stay_cached() {
        // Three region entries per row: past the 256 the cache used to
        // clear at, which refilled and flushed it every frame.
        let tree = col((0..100).map(|_| {
            row([block(10., 10.).grow(1.), block(10., 10.).grow(1.)])
                .inside(2.)
                .w(40.)
                .h(20.)
        }));
        let spec = SceneSpec::new(tree).offered(Size::new(40., 2000.));
        let mut text = TextState::default();
        text.resolve(&spec).unwrap();
        let held = text.region_cache.len();
        assert!(held > 256, "{held}");
        text.resolve(&spec).unwrap();
        assert_eq!(text.region_cache.len(), held);
    }
}
