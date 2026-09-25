//! `.inside(..)`: a parent's interior partitioned into its children's regions.
use std::ops::Range;
use std::sync::Arc;

use mui_geometry::{BooleanOp, Bounds, Path, RoundedRect};
use mui_layout::{Frame, Insets, Size};

use super::{bounds, find, fit, SceneError, Walk};
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
        (key, parent): (&Arc<str>, &str),
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
        let end = at + self.sizes[at];
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
                next += self.sizes[i];
                (!c.is_float() && c.payload().carve.is_none()).then_some((i, c, j))
            })
            .collect();
        let masks = self.masks(e, key, b, &children)?;
        for ((i, child, j), mask) in children.into_iter().zip(masks) {
            // The same identity `node` gives this child when it walks it.
            let id: Arc<str> = child
                .key()
                .map_or_else(|| Arc::from(format!("{parent}/{j}")), Arc::from);
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
        (key, at): (&Arc<str>, usize),
        padding: f64,
    ) -> Result<Path, SceneError> {
        let e = n.payload();
        let mut interior = outline.clone();
        if let Some(ramp) = &e.extras().border_ramp {
            ramp.validate()?;
            let anchor = match &ramp.anchor {
                None => frame,
                Some(id) => {
                    self.frames[find(n, id.as_str(), at, &self.sizes).ok_or(
                        mui_geometry::Error::InvalidOptions("border ramp descendant missing"),
                    )?]
                }
            };
            if anchor.size.width <= 0. {
                return Err(
                    mui_geometry::Error::InvalidOptions("border ramp anchor has no width").into(),
                );
            }
            let anchor = *self.ramp_anchors.entry(at).or_insert(anchor);
            for id in ramp.tabs.iter().chain(&ramp.dividers) {
                let index = find(n, id.as_str(), at, &self.sizes).ok_or(
                    mui_geometry::Error::InvalidOptions("border ramp descendant missing"),
                )?;
                self.ramp_frames
                    .insert((at, id.clone()), self.frames[index]);
            }
            let mut inward = ramp.clone();
            inward.from.1 *= ramp.align.inward();
            inward.to.1 *= ramp.align.inward();
            if inward.from.1.max(inward.to.1) > 0. {
                let mut band = self.borders.band(
                    &format!("inside/{key}"),
                    outline,
                    &inward,
                    anchor,
                    0.1 / self.spec.device_scale.unwrap_or(1.),
                )?;
                let shoulder = match e.style.radius {
                    Radius::Pair(_, r) => r,
                    Radius::Scale(k) => self.spec.theme.corners.concave * k,
                    _ => self.spec.theme.corners.concave,
                };
                crate::border_ramp::decorate(&mut band, ramp, anchor, shoulder, |id| {
                    Ok(self.ramp_frames[&(at, id.clone())])
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
        key: &Arc<str>,
        b: Bounds,
        children: &[(usize, &El, usize)],
    ) -> Result<Vec<Path>, SceneError> {
        let mut masks: Vec<Path> = children
            .iter()
            .map(|(i, ..)| {
                if self.frames[*i].size.width <= 0. || self.frames[*i].size.height <= 0. {
                    Ok(Path::default())
                } else {
                    RoundedRect::new(bounds(self.frames[*i], None), 0.).map(|r| r.path())
                }
            })
            .collect::<Result<_, _>>()?;
        if e.bend == 0. {
            return Ok(masks);
        }
        if children.len() != 2 {
            return Err(mui_geometry::Error::InvalidOptions("bend requires two siblings").into());
        }
        let size = Size::new(b.max.x - b.min.x, b.max.y - b.min.y);
        let a = self.frames[children[0].0];
        let z = self.frames[children[1].0];
        let horizontal = z.x >= a.right() - 1e-6;
        if !horizontal && z.y < a.bottom() - 1e-6 {
            return Err(
                mui_geometry::Error::InvalidOptions("bend requires a row or column").into(),
            );
        }
        let (axis, cut, gap) = if horizontal {
            (
                mui_geometry::SplitAxis::X,
                ((a.right() + z.x) * 0.5 - b.min.x) / size.width,
                z.x - a.right(),
            )
        } else {
            (
                mui_geometry::SplitAxis::Y,
                ((a.bottom() + z.y) * 0.5 - b.min.y) / size.height,
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
        (i, id): (usize, &Arc<str>),
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
        self.region_envelopes.insert(i, path.clone());
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
                let anchor = self.frames[i];
                self.ramp_anchors.insert(i, anchor);
                let mut sweep = ramp.clone();
                sweep.from.1 *= outward;
                sweep.to.1 *= outward;
                let band = self.borders.band(
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
        let end = i + self.sizes[i];
        match self.flat_bounds(&path)? {
            Some(b) => {
                let padding = child.padding(self.spec.theme.spacing);
                self.relayout(child, padding, b, i..end, 0)?;
            }
            None => self.collapse(i..end),
        }
        self.regions.insert(i, path);
        Ok(())
    }

    /// Lay `root` out again as exactly `b`, padded by `padding`, and move
    /// the frames in `range` to where it put its nodes past the first `skip`.
    fn relayout(
        &mut self,
        root: &El,
        padding: Insets,
        b: Bounds,
        range: Range<usize>,
        skip: usize,
    ) -> Result<(), SceneError> {
        let size = Size::new(b.max.x - b.min.x, b.max.y - b.min.y);
        let th = self.spec.theme;
        let layout = mui_layout::resolve_boxed_with(
            root,
            size,
            padding,
            self.spec.limits,
            th.spacing,
            |e, room| fit(&mut self.runs, th, e, room),
        )?;
        for (dest, f) in self.frames.to_mut()[range]
            .iter_mut()
            .zip(&layout.all()[skip..])
        {
            *dest = Frame {
                x: f.x + b.min.x,
                y: f.y + b.min.y,
                size: f.size,
            };
        }
        Ok(())
    }

    /// A subtree with no room: every frame in `range` paints nothing.
    fn collapse(&mut self, range: Range<usize>) {
        for f in &mut self.frames.to_mut()[range] {
            f.size = Size::ZERO;
        }
    }

    fn flat_bounds(&self, path: &Path) -> Result<Option<Bounds>, SceneError> {
        let o = self.spec.offsets;
        Ok(Bounds::from_points(
            path.flatten(o.flatten_tolerance, o.max_points)?.concat(),
        ))
    }

    /// Region geometry for step `key`, reused while its inputs match.
    pub(super) fn cached_region(
        &mut self,
        key: (Arc<str>, u8),
        op: Operation,
    ) -> Result<Path, SceneError> {
        self.region_cache
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
        let tree = column((0..100).map(|_| {
            row([leaf(10., 10.).grow(1.), leaf(10., 10.).grow(1.)])
                .inside(2.)
                .w(40.)
                .h(20.)
        }));
        let spec = SceneSpec::new(tree).offered(Size::new(40., 2000.));
        let mut text = TextCache::default();
        resolve_scene_with(&spec, &mut text).unwrap();
        let held = text.region_cache.len();
        assert!(held > 256, "{held}");
        resolve_scene_with(&spec, &mut text).unwrap();
        assert_eq!(text.region_cache.len(), held);
    }
}
