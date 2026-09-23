//! `.inside(..)`: a parent's interior partitioned into its children's regions.
use mui_geometry::{BooleanOp, Bounds, Path, RoundedRect};
use mui_layout::{Frame, Size};

use super::{bounds, find, fit, SceneError, Walk};
use crate::regions::Operation;
use crate::{El, Radius};

impl Walk<'_> {
    pub(super) fn partition(
        &mut self,
        n: &El,
        outline: &Path,
        frame: Frame,
        at: usize,
    ) -> Result<(), SceneError> {
        let e = n.payload();
        let Some(padding) = e.inside else {
            if e.bend != 0. {
                return Err(mui_geometry::Error::InvalidOptions("bend requires inside").into());
            }
            return Ok(());
        };
        let padding = padding.resolve(self.spec.theme.spacing);
        if !padding.is_finite() || padding < 0. || !e.bend.is_finite() || e.bend.abs() > 0.45 {
            return Err(mui_geometry::Error::InvalidOptions("shape padding/bend").into());
        }
        if e.welding.is_some() {
            return Err(SceneError::UnsupportedWeld(
                "inside requires vector welding",
            ));
        }
        let mut interior = outline.clone();
        if let Some(ramp) = &e.border_ramp {
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
            let decoration = ramp.clone();
            let mut ramp = ramp.clone();
            ramp.from.1 *= ramp.align.inward();
            ramp.to.1 *= ramp.align.inward();
            if ramp.from.1.max(ramp.to.1) > 0. {
                let mut band = self.borders.band(
                    &format!("inside/{at}"),
                    outline,
                    &ramp,
                    anchor,
                    0.1 / self.spec.device_scale.unwrap_or(1.),
                )?;
                let shoulder = match e.style.radius {
                    Radius::Pair(_, r) => r,
                    Radius::Scale(k) => self.spec.theme.corners.concave * k,
                    _ => self.spec.theme.corners.concave,
                };
                crate::border_ramp::decorate(&mut band, &decoration, anchor, shoulder, |id| {
                    Ok(self.ramp_frames[&(at, id.clone())])
                })?;
                let band = self.region_cache.resolve(
                    (at, 0),
                    Operation::Sweep(band),
                    self.spec.offsets,
                    self.spec.geometry,
                )?;
                interior = self.region_cache.resolve(
                    (at, 1),
                    Operation::Combine(interior, band, BooleanOp::Difference),
                    self.spec.offsets,
                    self.spec.geometry,
                )?;
            }
        } else if let Some(stroke) = &e.style.stroke {
            let width = stroke.width.unwrap_or(self.spec.theme.stroke_width);
            if !width.is_finite() || width < 0. {
                return Err(SceneError::InvalidRadius);
            }
            interior = self.region_cache.resolve(
                (at, 2),
                Operation::Inset(interior, width * e.border_align.inward()),
                self.spec.offsets,
                self.spec.geometry,
            )?;
        }
        interior = self.region_cache.resolve(
            (at, 3),
            Operation::Inset(interior, padding),
            self.spec.offsets,
            self.spec.geometry,
        )?;
        let Some(b) = Bounds::from_points(
            interior
                .flatten(
                    self.spec.offsets.flatten_tolerance,
                    self.spec.offsets.max_points,
                )?
                .concat(),
        ) else {
            let end = at + self.sizes[at];
            for f in &mut self.frames.to_mut()[at + 1..end] {
                f.size = Size::ZERO;
            }
            return Ok(());
        };
        let size = Size::new(b.max.x - b.min.x, b.max.y - b.min.y);
        let root = n.clone().size(size.width, size.height).pad(0.);
        let th = self.spec.theme;
        let layout = mui_layout::resolve_with(
            &root,
            Some(size),
            self.spec.limits,
            th.spacing,
            |e, room| fit(&mut self.runs, th, e, room),
        )?;
        let end = at + self.sizes[at];
        for (dest, f) in self.frames.to_mut()[at + 1..end]
            .iter_mut()
            .zip(&layout.all()[1..])
        {
            *dest = Frame {
                x: f.x + b.min.x,
                y: f.y + b.min.y,
                size: f.size,
            };
        }
        let mut next = at + 1;
        let children: Vec<_> = n
            .children()
            .iter()
            .filter_map(|c| {
                let i = next;
                next += self.sizes[i];
                (!c.is_float() && c.payload().carve.is_none()).then_some((i, c))
            })
            .collect();
        let mut masks: Vec<Path> = children
            .iter()
            .map(|(i, _)| {
                if self.frames[*i].size.width <= 0. || self.frames[*i].size.height <= 0. {
                    Ok(Path::default())
                } else {
                    RoundedRect::new(bounds(self.frames[*i], None), 0.).map(|r| r.path())
                }
            })
            .collect::<Result<_, _>>()?;
        if e.bend != 0. {
            if children.len() != 2 {
                return Err(
                    mui_geometry::Error::InvalidOptions("bend requires two siblings").into(),
                );
            }
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
                *mask = self.region_cache.resolve(
                    (at, 4 + side as u8),
                    Operation::SplitMask(b, split, side == 1),
                    self.spec.offsets,
                    self.spec.geometry,
                )?;
            }
        }

        for ((i, child), mask) in children.into_iter().zip(masks) {
            let path = self.region_cache.resolve(
                (i, 6),
                Operation::Combine(interior.clone(), mask, BooleanOp::Intersection),
                self.spec.offsets,
                self.spec.geometry,
            )?;
            // A child's outward border belongs inside its allocation too. Reserve
            // it before fitting the child, and retain the allocation as a paint cap.
            self.region_envelopes.insert(i, path.clone());
            let mut path = path;
            if let Some(ramp) = &child.payload().border_ramp {
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
                        &format!("outside/{i}"),
                        &path,
                        &sweep,
                        anchor,
                        0.1 / self.spec.device_scale.unwrap_or(1.),
                    )?;
                    let band = self.region_cache.resolve(
                        (i, 8),
                        Operation::Sweep(band),
                        self.spec.offsets,
                        self.spec.geometry,
                    )?;
                    path = self.region_cache.resolve(
                        (i, 9),
                        Operation::Combine(path, band, BooleanOp::Difference),
                        self.spec.offsets,
                        self.spec.geometry,
                    )?;
                }
            } else if let Some(stroke) = &child.payload().style.stroke {
                let width = stroke.width.unwrap_or(self.spec.theme.stroke_width);
                if !width.is_finite() || width < 0. {
                    return Err(SceneError::InvalidRadius);
                }
                let outward = width * (1. - child.payload().border_align.inward());
                if outward > 0. {
                    path = self.region_cache.resolve(
                        (i, 9),
                        Operation::Inset(path, outward),
                        self.spec.offsets,
                        self.spec.geometry,
                    )?;
                }
            }
            if let Some(b) = Bounds::from_points(
                path.flatten(
                    self.spec.offsets.flatten_tolerance,
                    self.spec.offsets.max_points,
                )?
                .concat(),
            ) {
                let size = Size::new(b.max.x - b.min.x, b.max.y - b.min.y);
                let root = child.clone().size(size.width, size.height);
                let layout = mui_layout::resolve_with(
                    &root,
                    Some(size),
                    self.spec.limits,
                    th.spacing,
                    |e, room| fit(&mut self.runs, th, e, room),
                )?;
                let end = i + self.sizes[i];
                for (dest, f) in self.frames.to_mut()[i..end].iter_mut().zip(layout.all()) {
                    *dest = Frame {
                        x: f.x + b.min.x,
                        y: f.y + b.min.y,
                        size: f.size,
                    };
                }
            } else {
                let end = i + self.sizes[i];
                for f in &mut self.frames.to_mut()[i..end] {
                    f.size = Size::ZERO;
                }
            }
            self.regions.insert(i, path);
        }
        Ok(())
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
