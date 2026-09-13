use std::collections::BTreeMap;

use mui_geometry::{
    fillet, inset_path, outset_path, union, Bounds, CornerStyle, GeometryOptions, OffsetOptions,
    Path, PlacedShape, Polygon, RoundedRect, Topology,
};
use mui_layout::{Layout, Limits, Node, Size};

use crate::{CornerProfile, Spacing, Theme};

#[derive(Debug, Clone, PartialEq)]
pub enum FrameRadius {
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
    /// An invalid fluent operation is reported when resolving, never ignored.
    InvalidModifier(&'static str),
    /// A layout frame. Boolean basis is the sharp rectangle; `path` uses the
    /// resolved convex radius. This enforces Boolean-first / fillet-second when
    /// the frame later participates in a merge.
    Frame {
        layout_key: String,
        radius: FrameRadius,
        extension: Option<Extension>,
    },
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
    pub fn frame(id: impl Into<String>, layout_key: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            source: SurfaceSource::Frame {
                layout_key: layout_key.into(),
                radius: FrameRadius::Global,
                extension: None,
            },
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
    pub fn radius(mut self, radius: FrameRadius) -> Self {
        if let SurfaceSource::Frame { radius: r, .. } = &mut self.source {
            *r = radius;
        } else {
            self.source = SurfaceSource::InvalidModifier("radius requires a frame");
        }
        self
    }
    pub fn corners(mut self, corners: CornerRule) -> Self {
        if let SurfaceSource::Merge { corners: c, .. } = &mut self.source {
            *c = corners;
        } else {
            self.source = SurfaceSource::InvalidModifier("corners requires a merge");
        }
        self
    }
}

/// Which edge of the source frame may grow. The opposite edge stays fixed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Auto,
    Top,
    Right,
    Bottom,
    Left,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Extension {
    pub edge: Edge,
    /// A layout key, not a surface ID: attachment never feeds back into layout.
    pub target: String,
}

impl SurfaceSpec {
    /// Extend painted geometry through padding to the target layout frame.
    /// Does not move content, widen a vertical tab, or merge anything implicitly.
    pub fn extend_to(mut self, edge: Edge, target: impl Into<String>) -> Self {
        if let SurfaceSource::Frame { extension, .. } = &mut self.source {
            *extension = Some(Extension {
                edge,
                target: target.into(),
            });
        } else {
            self.source = SurfaceSource::InvalidModifier("extend_to requires a frame");
        }
        self
    }
}

#[derive(Debug, Clone)]
pub struct SceneSpec {
    pub theme: Theme,
    pub root: Node,
    pub offered: Option<Size>,
    pub available: mui_layout::Constraints,
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
            available: Default::default(),
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
    pub fn available_width(mut self, width: f64) -> Self {
        self.available.width = Some(width);
        self.offered = None;
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
    InvalidModifier(&'static str),
    InvalidAttachment { source: String, target: String },
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
            Self::Layout(e) => Some(e),
            Self::Geometry(e) => Some(e),
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

fn sharp_rect(frame: mui_layout::Frame, options: GeometryOptions) -> Result<Topology, SceneError> {
    if frame.size.width == 0.0 || frame.size.height == 0.0 {
        // Still validate options even when there is no material to paint.
        return Ok(union(&[], options)?);
    }
    let p = Polygon::rectangle(frame.x, frame.y, frame.size.width, frame.size.height)?;
    Ok(union(&[PlacedShape::from(p)], options)?)
}

fn extend_frame(
    mut source: mui_layout::Frame,
    target: mui_layout::Frame,
    edge: Edge,
    epsilon: f64,
) -> Option<mui_layout::Frame> {
    let overlap_x = source.right().min(target.right()) - source.x.max(target.x);
    let overlap_y = source.bottom().min(target.bottom()) - source.y.max(target.y);
    let edge = if edge == Edge::Auto {
        if overlap_x > epsilon && overlap_y > epsilon {
            return Some(source);
        }
        if overlap_x > epsilon {
            if target.y >= source.bottom() - epsilon {
                Edge::Bottom
            } else if target.bottom() <= source.y + epsilon {
                Edge::Top
            } else {
                return None;
            }
        } else if overlap_y > epsilon {
            if target.x >= source.right() - epsilon {
                Edge::Right
            } else if target.right() <= source.x + epsilon {
                Edge::Left
            } else {
                return None;
            }
        } else {
            return None;
        }
    } else {
        edge
    };
    match edge {
        Edge::Bottom if overlap_x > epsilon && target.bottom() > source.y => {
            source.size.height = source.bottom().max(target.y) - source.y;
        }
        Edge::Top if overlap_x > epsilon && target.y < source.bottom() => {
            let bottom = source.bottom();
            source.y = source.y.min(target.bottom());
            source.size.height = bottom - source.y;
        }
        Edge::Right if overlap_y > epsilon && target.right() > source.x => {
            source.size.width = source.right().max(target.x) - source.x;
        }
        Edge::Left if overlap_y > epsilon && target.x < source.right() => {
            let right = source.right();
            source.x = source.x.min(target.right());
            source.size.width = right - source.x;
        }
        _ => return None,
    }
    Some(source)
}

fn empty_surface(id: &str) -> ResolvedSurface {
    ResolvedSurface {
        id: id.into(),
        basis: Topology::default(),
        path: Path::default(),
        bounds: None,
        analytic_rect: None,
        convex_radius_hint: None,
        topology_changed: false,
    }
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

fn dependencies(source: &SurfaceSource) -> Vec<&str> {
    match source {
        SurfaceSource::Frame {
            radius: FrameRadius::ParentNormalized { parent, .. },
            ..
        }
        | SurfaceSource::Inset { parent, .. }
        | SurfaceSource::Outset { parent, .. } => vec![parent],
        SurfaceSource::Merge { inputs, .. } => inputs.iter().map(String::as_str).collect(),
        _ => Vec::new(),
    }
}

struct Resolver<'a> {
    spec: &'a SceneSpec,
    layout: &'a Layout,
    specs: BTreeMap<&'a str, &'a SurfaceSpec>,
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
            out: BTreeMap::new(),
        })
    }
    fn resolve(mut self) -> Result<BTreeMap<String, ResolvedSurface>, SceneError> {
        // Compile dependencies once. Kahn's algorithm avoids recursive stack
        // depth, repeated geometry clones, and order-dependent graph validity.
        let mut incoming = BTreeMap::new();
        let mut consumers = BTreeMap::<&str, Vec<&str>>::new();
        let mut depths = BTreeMap::<&str, usize>::new();
        let mut ready = std::collections::VecDeque::new();
        let mut edges = 0usize;
        for (&id, spec) in &self.specs {
            let deps = dependencies(&spec.source);
            edges += deps.len();
            if edges > 16384 {
                return Err(SceneError::DependencyDepth);
            }
            incoming.insert(id, deps.len());
            depths.insert(id, 0);
            if deps.is_empty() {
                ready.push_back(id);
            }
            for dep in deps {
                if !self.specs.contains_key(dep) {
                    return Err(SceneError::MissingSurface(dep.into()));
                }
                consumers.entry(dep).or_default().push(id);
            }
        }
        let mut order = Vec::with_capacity(self.specs.len());
        while let Some(id) = ready.pop_front() {
            order.push(id);
            if let Some(children) = consumers.get(id) {
                for child in children {
                    let depth = depths[child].max(depths[id] + 1);
                    if depth > 128 {
                        return Err(SceneError::DependencyDepth);
                    }
                    depths.insert(child, depth);
                    let count = incoming.get_mut(child).expect("validated graph node");
                    *count -= 1;
                    if *count == 0 {
                        ready.push_back(child);
                    }
                }
            }
        }
        if order.len() != self.specs.len() {
            let id = incoming
                .iter()
                .find(|(_, count)| **count > 0)
                .expect("cycle remains")
                .0;
            return Err(SceneError::DependencyCycle((*id).into()));
        }
        for id in order {
            let surface = self.one(id)?;
            self.out.insert(id.into(), surface);
        }
        Ok(self.out)
    }
    fn parent(&self, id: &str) -> Result<&ResolvedSurface, SceneError> {
        self.out
            .get(id)
            .ok_or_else(|| SceneError::MissingSurface(id.into()))
    }
    fn one(&self, id: &str) -> Result<ResolvedSurface, SceneError> {
        let spec = self.specs[id];
        let resolved = match &spec.source {
            SurfaceSource::InvalidModifier(message) => {
                return Err(SceneError::InvalidModifier(message))
            }
            SurfaceSource::Frame {
                layout_key,
                radius,
                extension,
            } => {
                let mut frame = self
                    .layout
                    .frame(layout_key)
                    .ok_or_else(|| SceneError::MissingLayoutFrame(layout_key.clone()))?;
                if let Some(extension) = extension {
                    let target = self
                        .layout
                        .frame(&extension.target)
                        .ok_or_else(|| SceneError::MissingLayoutFrame(extension.target.clone()))?;
                    if target.size.width == 0.0
                        || target.size.height == 0.0
                        || frame.size.width == 0.0
                        || frame.size.height == 0.0
                    {
                        return Err(SceneError::InvalidAttachment {
                            source: layout_key.clone(),
                            target: extension.target.clone(),
                        });
                    }
                    frame = extend_frame(
                        frame,
                        target,
                        extension.edge,
                        self.spec.geometry_options.epsilon,
                    )
                    .ok_or_else(|| SceneError::InvalidAttachment {
                        source: layout_key.clone(),
                        target: extension.target.clone(),
                    })?;
                }
                let basis = sharp_rect(frame, self.spec.geometry_options)?;
                let r = match radius {
                    FrameRadius::Global => self.spec.theme.corners.convex,
                    FrameRadius::Absolute(r) => *r,
                    FrameRadius::ParentNormalized { parent, scale } => {
                        if !scale.is_finite() || *scale < 0.0 {
                            return Err(SceneError::InvalidRadius);
                        }
                        let p = self.parent(parent)?;
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
                if !r.is_finite() || r < 0.0 {
                    return Err(SceneError::InvalidRadius);
                }
                if frame.size.width == 0.0 || frame.size.height == 0.0 {
                    return Ok(empty_surface(id));
                }
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
                    let r = self.parent(input)?;
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
                let p = self.parent(parent)?;
                let d = distance
                    .resolve(&self.spec.theme.spacing)
                    .ok_or(SceneError::InvalidRadius)?;
                if let Some(rr) = p.analytic_rect {
                    let i = rr.inset(d)?;
                    if let Some(child) = i.shape {
                        let basis = mui_geometry::offset_path(
                            &child.path(),
                            0.0,
                            self.spec.offset_options,
                        )?
                        .topology;
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
                let p = self.parent(parent)?;
                let d = distance
                    .resolve(&self.spec.theme.spacing)
                    .ok_or(SceneError::InvalidRadius)?;
                if let Some(rr) = p.analytic_rect {
                    let child = rr.outset(d)?;
                    let basis =
                        mui_geometry::offset_path(&child.path(), 0.0, self.spec.offset_options)?
                            .topology;
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
        if let Some(bounds) = resolved.bounds {
            if [bounds.min.x, bounds.min.y, bounds.max.x, bounds.max.y]
                .iter()
                .any(|v| !v.is_finite() || v.abs() > self.spec.geometry_options.coordinate_limit)
            {
                return Err(mui_geometry::Error::CoordinateLimit.into());
            }
        }
        if resolved.basis.vertex_count() > self.spec.geometry_options.max_vertices {
            return Err(mui_geometry::Error::TooManyVertices.into());
        }
        Ok(resolved)
    }
}

pub fn resolve_scene(spec: &SceneSpec) -> Result<ResolvedScene, SceneError> {
    resolve_scene_measured(spec, |key, _| {
        Err(mui_layout::Error::MissingMeasurement(key.into()))
    })
}

pub fn resolve_scene_measured(
    spec: &SceneSpec,
    mut measure: impl FnMut(&str, mui_layout::MeasureInput) -> Result<Size, mui_layout::Error>,
) -> Result<ResolvedScene, SceneError> {
    resolve_scene_measured_with_baseline(spec, |id, input| measure(id, input).map(Into::into))
}
pub fn resolve_scene_measured_with_baseline(
    spec: &SceneSpec,
    measure: impl FnMut(
        &str,
        mui_layout::MeasureInput,
    ) -> Result<mui_layout::Measurement, mui_layout::Error>,
) -> Result<ResolvedScene, SceneError> {
    let _ = union(&[], spec.geometry_options)?;
    let constraints = spec
        .offered
        .map_or(spec.available, |size| mui_layout::Constraints {
            width: Some(size.width),
            height: Some(size.height),
        });
    let layout = mui_layout::resolve_measured_with_baseline(
        &spec.root,
        constraints,
        spec.layout_limits,
        &spec.theme.spacing,
        measure,
    )?;
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
        self.commit_measured(spec, |key, _| {
            Err(mui_layout::Error::MissingMeasurement(key.into()))
        })
    }
    pub fn commit_measured(
        &mut self,
        spec: &SceneSpec,
        measure: impl FnMut(&str, mui_layout::MeasureInput) -> Result<Size, mui_layout::Error>,
    ) -> Result<(), SceneError> {
        let next = resolve_scene_measured(spec, measure)?;
        let rev = self
            .revision
            .checked_add(1)
            .ok_or(SceneError::RevisionExhausted)?;
        self.current = Some(next);
        self.revision = rev;
        Ok(())
    }
}
