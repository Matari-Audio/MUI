//! A material weld's plates, baked or handed to the GPU.

use mui_geometry::{CornerStyle, Point};
use mui_layout::Frame;

use super::{SceneError, Walk, snap};
use crate::{Color, Content, El, Fill, Paint};

impl Walk<'_> {
    /// Bake the plates in group-local coordinates, before clips and the parent
    /// hit surface are emitted. Descendant content and identities are untouched.
    pub(super) fn material_weld(
        &mut self,
        n: &El,
        frame: Frame,
        path: &str,
        under: Color,
    ) -> Result<Option<crate::material_weld::MaterialWeld>, SceneError> {
        let Some(weld) = n.payload().extras().welding else {
            return Ok(None);
        };
        let gpu = self.spec.weld_backend == crate::WeldBackend::AnalyticGpu;
        if gpu && n.is_clip() {
            return Err(SceneError::UnsupportedWeld(
                "GPU weld cannot itself clip children to its changing union; put a normal clipping viewport above it",
            ));
        }
        crate::material_weld::check_plate(n, false)?;
        if n.payload().extras().outline.is_some() || n.payload().style.union.unwrap_or_default() {
            return Err(SceneError::UnsupportedWeld(
                "custom or union outline on the group; put it on a source child",
            ));
        }
        let mut quality = weld.quality.unwrap_or_default();
        if let Some(scale) = self.spec.device_scale {
            quality.scale = scale;
        }
        if !quality.scale.is_finite() || !(0.125..=8.0).contains(&quality.scale) {
            return Err(mui_weld::Error::Invalid("weld raster scale").into());
        }
        // A snapped local origin keeps the baked image on the device grid even
        // when layout has a fractional origin. Layout itself remains untouched.
        let origin = Point::new(
            snap(frame.x, Some(quality.scale)),
            snap(frame.y, Some(quality.scale)),
        );
        let th = self.spec.theme;
        let parent = &n.payload().style;
        let mut sources = Vec::new();
        let mut members = std::collections::HashSet::new();
        let mut at = self.i;
        for (j, c) in n.children().iter().enumerate() {
            let first = at;
            let f = self.tree.frames[first];
            at += self.tree.sizes[first];
            let e = c.payload();
            if c.is_float()
                || c.is_sticky()
                || e.carve.is_some()
                || e.has(crate::Element::WELD_EXCLUDED)
                || matches!(&e.content, Content::Text(_))
                || f.size.width <= 0.0
                || f.size.height <= 0.0
            {
                continue;
            }
            // Empty spacers affect layout, never material. A group fill can
            // explicitly give otherwise unpainted child outlines material.
            if parent.fill.as_ref().is_none_or(Fill::is_none)
                && parent.stroke.is_none()
                && e.style.fill.as_ref().is_none_or(Fill::is_none)
                && e.style.stroke.is_none()
            {
                continue;
            }
            crate::material_weld::check_plate(c, true)?;
            if gpu
                && (sources.len() >= mui_weld::analytic::ANALYTIC_SOURCES
                    || e.extras().outline.is_some()
                    || e.style.union.unwrap_or_default()
                    || e.style.corners.unwrap_or_default() != CornerStyle::Round
                    || c.children()
                        .iter()
                        .any(|child| child.payload().carve.is_some()))
            {
                return Err(SceneError::UnsupportedWeld(
                    "analytic GPU weld requires at most three ordinary rounded-rectangle plates; custom/carved/nested outlines need an explicit reference backend",
                ));
            }
            // Plates are placed by their frames: scene space.
            let contour = self.outline(c, f, first + 1)?;
            let (outline, rect) = (contour.world(), contour.world_rect());
            if gpu && rect.is_none() {
                return Err(SceneError::UnsupportedWeld(
                    "GPU participant is not an analytic rounded rectangle",
                ));
            }
            let fill = match &parent.fill {
                Some(f) if !f.is_none() => f,
                _ => e.style.fill.as_ref().unwrap_or(&Fill::None),
            };
            let fill_paint = fill.paint(&th.palette, under);
            let ground = fill_paint.as_ref().map_or(under, Paint::solid);
            let stroke = parent.stroke.as_ref().or(e.style.stroke.as_ref());
            let border = stroke.and_then(|s| s.fill.paint(&th.palette, ground));
            let width = stroke.map_or(0.0, |s| s.width.unwrap_or(th.stroke_width));
            sources.push(crate::material_weld::source(
                crate::material_weld::Plate {
                    outline,
                    rect,
                    frame: f,
                    fill: fill_paint,
                    border,
                    width,
                },
                origin,
                0.25 / quality.scale,
            )?);
            members.insert(super::child_key(c, path, j));
        }
        if sources.is_empty() {
            return Err(SceneError::UnsupportedWeld(
                "no painted, non-excluded plate children",
            ));
        }
        if gpu {
            return Ok(Some(crate::external::finish_gpu(
                &sources,
                weld,
                quality,
                origin,
                members,
                self.caches.welds,
            )?));
        }
        let request = mui_weld::Request {
            sources,
            weld,
            quality,
        };
        let baked = self.caches.welds.get(&request)?;
        Ok(Some(crate::material_weld::finish(&baked, origin, members)?))
    }
}
