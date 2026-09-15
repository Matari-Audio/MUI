use std::collections::BTreeMap;

use mui_geometry::{
    fillet, inset_path, outset_path, union, Bounds, CornerStyle, GeometryOptions, OffsetOptions,
    Path, PlacedShape, Polygon, RoundedRect, Topology,
};
#[cfg(test)]
use mui_layout::{column, leaf, overlay};
use mui_layout::{resolve, Layout, Limits, Node, Size};

use crate::{CornerProfile, Spacing, Theme};

#[derive(Debug, Clone, PartialEq)]
pub enum Radius {
    /// Use the global convex radius from the theme.
    Global,
    Absolute(f64),
    /// Styling relationship only. For an exact constant-width shell use
    /// `SurfaceSource::Inset` instead.
    ParentNormalized {
        parent: String,
        scale: f64,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum CornerRule {
    Global,
    Absolute(CornerProfile),
    GlobalScaled(f64),
}

#[derive(Debug, Clone, PartialEq)]
pub enum SurfaceSource {
    /// A layout frame. Boolean basis is the sharp rectangle; `path` uses the
    /// resolved convex radius. This enforces Boolean-first / fillet-second when
    /// the frame later participates in a merge.
    Frame { layout_id: String, radius: Radius },
    /// Set union of already-resolved surface bases followed by a new corner pass.
    Merge {
        inputs: Vec<String>,
        corners: CornerRule,
    },
    /// True parallel erosion of the FINAL rendered parent outline.
    Inset { parent: String, distance: Spacing },
    /// True parallel dilation of the FINAL rendered parent outline.
    Outset { parent: String, distance: Spacing },
}

#[derive(Debug, Clone, PartialEq)]
pub struct SurfaceSpec {
    pub id: String,
    pub source: SurfaceSource,
}
impl SurfaceSpec {
    /// A surface taken straight from a named layout node. The node's id is the
    /// surface id -- one name per box, so there is nothing to keep in step.
    pub fn frame(id: impl Into<String>) -> Self {
        let id = id.into();
        Self {
            source: SurfaceSource::Frame {
                layout_id: id.clone(),
                radius: Radius::Global,
            },
            id,
        }
    }
    pub fn merge(
        id: impl Into<String>,
        inputs: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            id: id.into(),
            source: SurfaceSource::Merge {
                inputs: inputs.into_iter().map(Into::into).collect(),
                corners: CornerRule::Global,
            },
        }
    }
    pub fn inset(id: impl Into<String>, parent: impl Into<String>, distance: Spacing) -> Self {
        Self {
            id: id.into(),
            source: SurfaceSource::Inset {
                parent: parent.into(),
                distance,
            },
        }
    }
    pub fn outset(id: impl Into<String>, parent: impl Into<String>, distance: Spacing) -> Self {
        Self {
            id: id.into(),
            source: SurfaceSource::Outset {
                parent: parent.into(),
                distance,
            },
        }
    }
    /// Set the radius used when this surface is taken from a layout frame.
    ///
    /// # Panics
    ///
    /// Panics if this surface is not a [`SurfaceSource::Frame`].
    pub fn radius(mut self, radius: Radius) -> Self {
        match &mut self.source {
            SurfaceSource::Frame { radius: r, .. } => *r = radius,
            _ => panic!("SurfaceSpec::radius requires a frame surface"),
        }
        self
    }
    /// Set the corner rule applied after merging input surfaces.
    ///
    /// # Panics
    ///
    /// Panics if this surface is not a [`SurfaceSource::Merge`].
    pub fn corners(mut self, corners: CornerRule) -> Self {
        match &mut self.source {
            SurfaceSource::Merge { corners: c, .. } => *c = corners,
            _ => panic!("SurfaceSpec::corners requires a merge surface"),
        }
        self
    }
}

#[derive(Debug, Clone)]
pub struct SceneSpec {
    pub theme: Theme,
    pub root: Node,
    pub offered: Option<Size>,
    pub layout_limits: Limits,
    pub geometry_options: GeometryOptions,
    pub offset_options: OffsetOptions,
    pub surfaces: Vec<SurfaceSpec>,
}
impl SceneSpec {
    pub fn new(root: Node) -> Self {
        Self {
            theme: Theme::default(),
            root,
            offered: None,
            layout_limits: Limits::default(),
            geometry_options: GeometryOptions::default(),
            offset_options: OffsetOptions::default(),
            surfaces: Vec::new(),
        }
    }
    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }
    pub fn offered(mut self, size: Size) -> Self {
        self.offered = Some(size);
        self
    }
    pub fn surface(mut self, surface: SurfaceSpec) -> Self {
        self.surfaces.push(surface);
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedSurface {
    pub id: String,
    /// Raw set used by downstream Boolean merges.
    pub basis: Topology,
    /// Final rendered outline after corner/offset semantics.
    pub path: Path,
    pub bounds: Option<Bounds>,
    /// Exact analytic rounded rectangle when the relationship permits it.
    pub analytic_rect: Option<RoundedRect>,
    /// Styling hint only; never used to claim constant thickness.
    pub convex_radius_hint: Option<f64>,
    pub topology_changed: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedScene {
    pub layout: Layout,
    surfaces: BTreeMap<String, ResolvedSurface>,
}
impl ResolvedScene {
    pub fn surface(&self, id: &str) -> Option<&ResolvedSurface> {
        self.surfaces.get(id)
    }
    pub fn surfaces(&self) -> impl Iterator<Item = (&str, &ResolvedSurface)> {
        self.surfaces.iter().map(|(k, v)| (k.as_str(), v))
    }
}

#[derive(Debug)]
pub enum SceneError {
    InvalidTheme,
    DuplicateSurface(String),
    MissingSurface(String),
    MissingLayoutFrame(String),
    EmptyMerge(String),
    DependencyCycle(String),
    DependencyDepth,
    InvalidRadius,
    Layout(mui_layout::Error),
    Geometry(mui_geometry::Error),
    RevisionExhausted,
}
impl std::fmt::Display for SceneError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "scene: {self:?}")
    }
}
impl std::error::Error for SceneError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Layout(error) => Some(error),
            Self::Geometry(error) => Some(error),
            _ => None,
        }
    }
}
impl From<mui_layout::Error> for SceneError {
    fn from(v: mui_layout::Error) -> Self {
        Self::Layout(v)
    }
}
impl From<mui_geometry::Error> for SceneError {
    fn from(v: mui_geometry::Error) -> Self {
        Self::Geometry(v)
    }
}

fn profile(rule: &CornerRule, theme: &Theme) -> Result<CornerProfile, SceneError> {
    let p = match rule {
        CornerRule::Global => theme.corners,
        CornerRule::Absolute(p) => *p,
        CornerRule::GlobalScaled(s) => theme.corners.scaled(*s).ok_or(SceneError::InvalidRadius)?,
    };
    p.valid().then_some(p).ok_or(SceneError::InvalidRadius)
}

fn sharp_rect(frame: mui_layout::Frame) -> Result<(Topology, Polygon), SceneError> {
    let p = Polygon::rectangle(frame.x, frame.y, frame.size.width, frame.size.height)?;
    let t = union(&[PlacedShape::from(p.clone())], GeometryOptions::default())?;
    Ok((t, p))
}

fn rounded_frame(frame: mui_layout::Frame, radius: f64) -> Result<RoundedRect, SceneError> {
    if !radius.is_finite() || radius < 0.0 {
        return Err(SceneError::InvalidRadius);
    }
    RoundedRect::new(
        Bounds {
            min: mui_geometry::Point::new(frame.x, frame.y),
            max: mui_geometry::Point::new(frame.right(), frame.bottom()),
        },
        radius,
    )
    .map_err(Into::into)
}

struct Resolver<'a> {
    spec: &'a SceneSpec,
    layout: &'a Layout,
    specs: BTreeMap<&'a str, &'a SurfaceSpec>,
    state: BTreeMap<String, u8>,
    out: BTreeMap<String, ResolvedSurface>,
}
impl<'a> Resolver<'a> {
    fn new(spec: &'a SceneSpec, layout: &'a Layout) -> Result<Self, SceneError> {
        if !spec.theme.valid() {
            return Err(SceneError::InvalidTheme);
        }
        if spec.surfaces.len() > 2048 {
            return Err(SceneError::DependencyDepth);
        }
        let mut specs = BTreeMap::new();
        for s in &spec.surfaces {
            if s.id.is_empty() {
                return Err(SceneError::MissingSurface("empty surface id".into()));
            }
            if specs.insert(s.id.as_str(), s).is_some() {
                return Err(SceneError::DuplicateSurface(s.id.clone()));
            }
        }
        Ok(Self {
            spec,
            layout,
            specs,
            state: BTreeMap::new(),
            out: BTreeMap::new(),
        })
    }
    fn resolve(mut self) -> Result<BTreeMap<String, ResolvedSurface>, SceneError> {
        let ids: Vec<String> = self.spec.surfaces.iter().map(|s| s.id.clone()).collect();
        for id in ids {
            self.one(&id, 0)?;
        }
        Ok(self.out)
    }
    fn one(&mut self, id: &str, depth: usize) -> Result<ResolvedSurface, SceneError> {
        if let Some(v) = self.out.get(id) {
            return Ok(v.clone());
        }
        if depth > 128 {
            return Err(SceneError::DependencyDepth);
        }
        match self.state.get(id).copied().unwrap_or(0) {
            1 => return Err(SceneError::DependencyCycle(id.into())),
            2 => {
                return self
                    .out
                    .get(id)
                    .cloned()
                    .ok_or_else(|| SceneError::MissingSurface(id.into()))
            }
            _ => {}
        }
        let spec = *self
            .specs
            .get(id)
            .ok_or_else(|| SceneError::MissingSurface(id.into()))?;
        self.state.insert(id.into(), 1);
        let resolved = match &spec.source {
            SurfaceSource::Frame { layout_id, radius } => {
                let frame = self
                    .layout
                    .frame(layout_id)
                    .ok_or_else(|| SceneError::MissingLayoutFrame(layout_id.clone()))?;
                let (basis, _) = sharp_rect(frame)?;
                let r = match radius {
                    Radius::Global => self.spec.theme.corners.convex,
                    Radius::Absolute(r) => *r,
                    Radius::ParentNormalized { parent, scale } => {
                        if !scale.is_finite() || *scale < 0.0 {
                            return Err(SceneError::InvalidRadius);
                        }
                        let p = self.one(parent, depth + 1)?;
                        let pb = p
                            .bounds
                            .ok_or_else(|| SceneError::MissingSurface(parent.clone()))?;
                        let hint = p.convex_radius_hint.ok_or(SceneError::InvalidRadius)?;
                        let pshort = pb.width().min(pb.height());
                        let cshort = frame.size.width.min(frame.size.height);
                        if pshort <= 0.0 {
                            return Err(SceneError::InvalidRadius);
                        }
                        hint / pshort * cshort * scale
                    }
                };
                let rr = rounded_frame(frame, r)?;
                ResolvedSurface {
                    id: id.into(),
                    basis,
                    path: rr.path(),
                    bounds: Some(rr.bounds()),
                    analytic_rect: Some(rr),
                    convex_radius_hint: Some(rr.radius()),
                    topology_changed: false,
                }
            }
            SurfaceSource::Merge { inputs, corners } => {
                if inputs.is_empty() {
                    return Err(SceneError::EmptyMerge(id.into()));
                }
                let mut shapes: Vec<PlacedShape> = Vec::new();
                for input in inputs {
                    let r = self.one(input, depth + 1)?;
                    shapes.extend(r.basis.placed_shapes());
                }
                let basis = union(&shapes, self.spec.geometry_options)?;
                let p = profile(corners, &self.spec.theme)?;
                let rounded = fillet(
                    &basis,
                    CornerStyle {
                        convex_radius: p.convex,
                        concave_radius: p.concave,
                        ..CornerStyle::default()
                    },
                )?;
                let bounds = basis.bounds();
                ResolvedSurface {
                    id: id.into(),
                    basis,
                    path: rounded.path,
                    bounds,
                    analytic_rect: None,
                    convex_radius_hint: Some(p.convex),
                    topology_changed: false,
                }
            }
            SurfaceSource::Inset { parent, distance } => {
                let p = self.one(parent, depth + 1)?;
                let d = distance
                    .resolve(&self.spec.theme)
                    .ok_or(SceneError::InvalidRadius)?;
                if let Some(rr) = p.analytic_rect {
                    let i = rr.inset(d)?;
                    if let Some(child) = i.shape {
                        let poly = Polygon::rectangle(
                            child.bounds().min.x,
                            child.bounds().min.y,
                            child.bounds().width(),
                            child.bounds().height(),
                        )?;
                        let basis = union(&[PlacedShape::from(poly)], self.spec.geometry_options)?;
                        ResolvedSurface {
                            id: id.into(),
                            basis,
                            path: child.path(),
                            bounds: Some(child.bounds()),
                            analytic_rect: Some(child),
                            convex_radius_hint: Some(child.radius()),
                            topology_changed: i.corner_collapsed,
                        }
                    } else {
                        ResolvedSurface {
                            id: id.into(),
                            basis: Topology::default(),
                            path: Path::default(),
                            bounds: None,
                            analytic_rect: None,
                            convex_radius_hint: None,
                            topology_changed: true,
                        }
                    }
                } else {
                    let i = inset_path(&p.path, d, self.spec.offset_options)?;
                    let bounds = i.topology.bounds();
                    ResolvedSurface {
                        id: id.into(),
                        basis: i.topology,
                        path: i.path,
                        bounds,
                        analytic_rect: None,
                        convex_radius_hint: None,
                        topology_changed: i.counts_changed,
                    }
                }
            }
            SurfaceSource::Outset { parent, distance } => {
                let p = self.one(parent, depth + 1)?;
                let d = distance
                    .resolve(&self.spec.theme)
                    .ok_or(SceneError::InvalidRadius)?;
                if let Some(rr) = p.analytic_rect {
                    let child = rr.outset(d)?;
                    let poly = Polygon::rectangle(
                        child.bounds().min.x,
                        child.bounds().min.y,
                        child.bounds().width(),
                        child.bounds().height(),
                    )?;
                    let basis = union(&[PlacedShape::from(poly)], self.spec.geometry_options)?;
                    ResolvedSurface {
                        id: id.into(),
                        basis,
                        path: child.path(),
                        bounds: Some(child.bounds()),
                        analytic_rect: Some(child),
                        convex_radius_hint: Some(child.radius()),
                        topology_changed: false,
                    }
                } else {
                    let o = outset_path(&p.path, d, self.spec.offset_options)?;
                    let bounds = o.topology.bounds();
                    ResolvedSurface {
                        id: id.into(),
                        basis: o.topology,
                        path: o.path,
                        bounds,
                        analytic_rect: None,
                        convex_radius_hint: None,
                        topology_changed: o.counts_changed,
                    }
                }
            }
        };
        self.state.insert(id.into(), 2);
        self.out.insert(id.into(), resolved.clone());
        Ok(resolved)
    }
}

pub fn resolve_scene(spec: &SceneSpec) -> Result<ResolvedScene, SceneError> {
    let layout = resolve(&spec.root, spec.offered, spec.layout_limits)?;
    let surfaces = Resolver::new(spec, &layout)?.resolve()?;
    Ok(ResolvedScene { layout, surfaces })
}

/// Transactional UI-thread scene snapshot. A failure leaves the published scene untouched.
#[derive(Debug, Default)]
pub struct SceneState {
    revision: u64,
    current: Option<ResolvedScene>,
}
impl SceneState {
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn current(&self) -> Option<&ResolvedScene> {
        self.current.as_ref()
    }
    pub fn commit(&mut self, spec: &SceneSpec) -> Result<(), SceneError> {
        let next = resolve_scene(spec)?;
        let rev = self
            .revision
            .checked_add(1)
            .ok_or(SceneError::RevisionExhausted)?;
        self.current = Some(next);
        self.revision = rev;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mui_layout::{Align, Justify};
    fn spec() -> SceneSpec {
        let controls = column([
            leaf(28., 28.).id("plus"),
            leaf(28., 28.).id("pie-a"),
            leaf(28., 28.).id("pie-b"),
        ])
        .id("controls")
        .gap(10.)
        .align(Align::Center)
        .justify(Justify::Center);
        let pill = column([controls]).id("pill-frame").padding(10.);
        let tab = column([pill])
            .id("tab")
            .padding(12.)
            .min_size(Size::new(92., 0.));
        let root = overlay([leaf(520., 230.).id("panel"), tab])
            .id("root")
            .align(Align::Start);
        SceneSpec::new(root)
            .theme(Theme {
                corners: CornerProfile::new(28., 32.),
                ..Theme::default()
            })
            .surface(SurfaceSpec::frame("panel"))
            .surface(SurfaceSpec::frame("tab"))
            .surface(SurfaceSpec::merge("outer", ["panel", "tab"]))
            .surface(SurfaceSpec::inset("pill-shell", "tab", Spacing::px(12.)))
    }
    #[test]
    fn analytic_inset_preserves_parallel_radius() {
        let s = resolve_scene(&spec()).unwrap();
        let tab = s.surface("tab").unwrap().analytic_rect.unwrap();
        let child = s.surface("pill-shell").unwrap().analytic_rect.unwrap();
        assert!((tab.radius() - child.radius() - 12.).abs() < 1e-9);
        assert!((child.bounds().min.x - tab.bounds().min.x - 12.).abs() < 1e-9);
    }
    #[test]
    fn surface_modifiers_apply_to_matching_variants() {
        let framed = SurfaceSpec::frame("frame").radius(Radius::Absolute(4.));
        assert!(matches!(
            framed.source,
            SurfaceSource::Frame {
                radius: Radius::Absolute(r),
                ..
            } if (r - 4.).abs() < f64::EPSILON
        ));

        let merged = SurfaceSpec::merge("merge", ["frame"]).corners(CornerRule::GlobalScaled(0.8));
        assert!(matches!(
            merged.source,
            SurfaceSource::Merge {
                corners: CornerRule::GlobalScaled(scale),
                ..
            } if (scale - 0.8).abs() < f64::EPSILON
        ));
    }
    #[test]
    #[should_panic(expected = "SurfaceSpec::radius requires a frame surface")]
    fn radius_on_non_frame_surface_fails_fast() {
        SurfaceSpec::inset("inner", "root", Spacing::px(1.)).radius(Radius::Global);
    }
    #[test]
    #[should_panic(expected = "SurfaceSpec::corners requires a merge surface")]
    fn corners_on_non_merge_surface_fails_fast() {
        SurfaceSpec::frame("frame").corners(CornerRule::Global);
    }
    #[test]
    fn parent_normalized_is_distinct_from_parallel() {
        let mut s = spec();
        s.surfaces.push(
            SurfaceSpec::frame("pill-frame").radius(Radius::ParentNormalized {
                parent: "tab".into(),
                scale: 1.0,
            }),
        );
        let r = resolve_scene(&s).unwrap();
        let tab = r.surface("tab").unwrap();
        let styled = r.surface("pill-frame").unwrap();
        let p = tab.bounds.unwrap();
        let c = styled.bounds.unwrap();
        let expected =
            tab.convex_radius_hint.unwrap() / p.width().min(p.height()) * c.width().min(c.height());
        assert!((styled.convex_radius_hint.unwrap() - expected).abs() < 1e-9);
        assert_ne!(
            styled.convex_radius_hint,
            r.surface("pill-shell").unwrap().convex_radius_hint
        );
    }
    #[test]
    fn cycle_is_rejected() {
        let root = leaf(10., 10.).id("x");
        let s = SceneSpec::new(root)
            .surface(SurfaceSpec::inset("a", "b", Spacing::px(1.)))
            .surface(SurfaceSpec::inset("b", "a", Spacing::px(1.)));
        assert!(matches!(
            resolve_scene(&s),
            Err(SceneError::DependencyCycle(_))
        ));
    }
    #[test]
    fn merged_parent_inset_follows_final_concave_outline() {
        let mut s = spec();
        s.surfaces
            .push(SurfaceSpec::inset("outer-inner", "outer", Spacing::px(6.0)));
        let r = resolve_scene(&s).unwrap();
        let outer = r.surface("outer").unwrap();
        let inner = r.surface("outer-inner").unwrap();
        assert!(inner.analytic_rect.is_none()); // merged contour uses the general offset path
        let outer_contours = outer.path.flatten(0.1, 20_000).unwrap();
        let inner_contours = inner.path.flatten(0.1, 20_000).unwrap();
        let mut min = f64::INFINITY;
        for p in inner_contours.iter().flatten().step_by(7) {
            min = min.min(mui_geometry::boundary_distance(*p, &outer_contours));
        }
        assert!((min - 6.0).abs() < 0.35, "measured inset={min}");
    }

    #[test]
    fn failed_commit_is_transactional() {
        let mut state = SceneState::default();
        state.commit(&spec()).unwrap();
        let rev = state.revision();
        let bad = SceneSpec::new(leaf(f64::NAN, 1.).id("x"));
        assert!(state.commit(&bad).is_err());
        assert_eq!(state.revision(), rev);
        assert!(state.current().is_some());
    }
    #[test]
    fn layout_error_is_exposed_as_scene_error_source() {
        let bad = SceneSpec::new(leaf(f64::NAN, 1.).id("x"));
        let error = resolve_scene(&bad).unwrap_err();
        let source = std::error::Error::source(&error).expect("layout source");
        assert!(source.is::<mui_layout::Error>());
    }
    #[test]
    fn geometry_error_is_exposed_as_scene_error_source() {
        let mut bad = spec();
        bad.geometry_options.epsilon = f64::NAN;
        let error = resolve_scene(&bad).unwrap_err();
        let source = std::error::Error::source(&error).expect("geometry source");
        assert!(source.is::<mui_geometry::Error>());
    }
}
