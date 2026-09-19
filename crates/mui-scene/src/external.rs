//! Renderer-independent external material nodes. The renderer owns GPU handles;
//! the scene owns placement, authored identity, and the validated material spec.
use crate::{ResolvedScene, SceneError};
use mui_geometry::{Bounds, Point, RoundedRect};
use mui_weld::{analytic::AnalyticWeld, Point as WeldPoint};
use std::sync::Arc;

/// Execution is selected by the host, not inferred from animation state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WeldBackend {
    /// Explicit bounded software reference, suitable for snapshots.
    #[default]
    Reference,
    /// One to three analytic plates; unsupported materials return an error.
    AnalyticGpu,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExternalWeld {
    /// Local source origin in scene coordinates. Not part of uploaded uniforms.
    pub origin: Point,
    pub material: AnalyticWeld,
}
impl ExternalWeld {
    pub fn bounds(&self) -> Bounds {
        let b = self.material.domain();
        Bounds::new(
            b.x0 + self.origin.x,
            b.y0 + self.origin.y,
            b.x1 + self.origin.x,
            b.y1 + self.origin.y,
        )
    }
    pub fn rect(&self) -> Result<RoundedRect, SceneError> {
        Ok(RoundedRect::new(self.bounds(), 0.)?)
    }
    pub fn contains(&self, p: Point) -> bool {
        self.material
            .contains(WeldPoint::new(p.x - self.origin.x, p.y - self.origin.y))
    }
}
impl ResolvedScene {
    pub fn external_weld(&self, key: &str) -> Option<&ExternalWeld> {
        self.external_welds.get(key)
    }
    pub fn external_welds(&self) -> impl Iterator<Item = (&str, &ExternalWeld)> + Clone {
        self.external_welds.iter().map(|(k, v)| (k.as_ref(), v))
    }
    pub fn set_weld_solid_material(
        &mut self,
        key: &str,
        index: usize,
        fill: Option<crate::Color>,
        border: Option<crate::Color>,
        width: f64,
    ) -> Result<bool, SceneError> {
        self.external_welds
            .get_mut(key)
            .ok_or(SceneError::UnsupportedWeld(
                "key is not a GPU material weld",
            ))?
            .material
            .set_solid_material(
                index,
                fill.map(crate::material_weld::color),
                border.map(crate::material_weld::color),
                width,
            )
            .map_err(SceneError::MaterialWeld)
    }
    /// Update only one plate's material, preserving its geometry and domain.
    pub fn set_weld_source_material(
        &mut self,
        key: &str,
        index: usize,
        source: mui_weld::analytic::AnalyticSource,
    ) -> Result<bool, SceneError> {
        self.external_welds
            .get_mut(key)
            .ok_or(SceneError::UnsupportedWeld(
                "key is not a GPU material weld",
            ))?
            .material
            .set_source_material(index, source)
            .map_err(SceneError::MaterialWeld)
    }
    /// Morph-only update: no layout, text measurement, path conversion, contour
    /// extraction, or CPU image baking. Keep the application model in sync: a
    /// later tree resolve intentionally replaces this value with its declaration.
    pub fn set_weld_morph(&mut self, key: &str, progress: f64) -> Result<bool, SceneError> {
        self.external_welds
            .get_mut(key)
            .ok_or(SceneError::UnsupportedWeld(
                "key is not a GPU material weld",
            ))?
            .material
            .set_morph(progress)
            .map_err(SceneError::MaterialWeld)
    }
    pub fn set_weld_material_blend(&mut self, key: &str, blend: f64) -> Result<bool, SceneError> {
        self.external_welds
            .get_mut(key)
            .ok_or(SceneError::UnsupportedWeld(
                "key is not a GPU material weld",
            ))?
            .material
            .set_material_blend(blend)
            .map_err(SceneError::MaterialWeld)
    }
}

/// GPU source lowering must not pretend the rectangular texture domain is the
/// welded shape. Input uses the analytic predicate against `ExternalWeld`, while
/// the rectangle exists only for sampling placement and conservative rejection.
pub(crate) fn finish_gpu(
    sources: &[mui_weld::Source],
    weld: mui_weld::Weld,
    quality: mui_weld::Quality,
    origin: Point,
    members: std::collections::HashSet<Arc<str>>,
    cache: &mut mui_weld::WeldCache,
) -> Result<crate::material_weld::MaterialWeld, SceneError> {
    let material = cache.get_analytic(sources, weld, quality.scale)?;
    let [w, h] = material.pixels();
    let pixels = u64::from(w) * u64::from(h);
    let work = pixels
        .checked_mul(material.boundary_segments().max(sources.len()) as u64)
        .ok_or(SceneError::UnsupportedWeld("GPU weld work budget overflow"))?;
    if pixels > quality.max_pixels as u64 || work > quality.max_work as u64 {
        return Err(SceneError::UnsupportedWeld(
            "GPU weld exceeds explicit node pixel/work budget",
        ));
    }
    let external = ExternalWeld { origin, material };
    let image_rect = external.rect()?;
    Ok(crate::material_weld::MaterialWeld {
        outline: image_rect.path(),
        image_fill: crate::Fill::None,
        image_rect,
        members,
        external: Some(external),
    })
}
