//! Item authoring compiles to the same validated layout/geometry pipeline.
use crate::*;
use mui_layout::{Flow, Horizontal, MeasureInput, Sizing, Track, Vertical};
use std::collections::BTreeMap;

/// Explicit semantic colors. Resolve from the current theme once per UI update.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Color {
    Canvas,
    Panel,
    Raised,
    Text,
    Muted,
    Outline,
    Primary(usize),
    PrimarySoft(usize),
    Status(usize),
    Custom(Rgb),
}
impl Color {
    pub fn resolve(self, colors: &Colors) -> Result<Rgb, SceneError> {
        Ok(match self {
            Self::Canvas => colors.canvas,
            Self::Panel => colors.panel,
            Self::Raised => colors.raised,
            Self::Text => colors.text,
            Self::Muted => colors.muted,
            Self::Outline => colors.outline,
            Self::Primary(i) => colors.primary.get(i).ok_or(SceneError::InvalidTheme)?.fill,
            Self::PrimarySoft(i) => colors.primary.get(i).ok_or(SceneError::InvalidTheme)?.soft,
            Self::Status(i) => colors.status.get(i).ok_or(SceneError::InvalidTheme)?.fill,
            Self::Custom(rgb) => rgb,
        })
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Rounding {
    Theme,
    Both(Spacing),
    Separate { outer: Spacing, inner: Spacing },
}
impl From<SpacingToken> for Rounding {
    fn from(v: SpacingToken) -> Self {
        Self::Both(v.into())
    }
}
impl From<f64> for Rounding {
    fn from(v: f64) -> Self {
        Self::Both(v.into())
    }
}
impl Rounding {
    pub fn separate(outer: impl Into<Spacing>, inner: impl Into<Spacing>) -> Self {
        Self::Separate {
            outer: outer.into(),
            inner: inner.into(),
        }
    }
    fn profile(self, theme: &Theme) -> Result<CornerProfile, SceneError> {
        match self {
            Self::Theme => Ok(theme.corners),
            Self::Both(v) => {
                let r = v.resolve(&theme.spacing).ok_or(SceneError::InvalidRadius)?;
                Ok(CornerProfile::new(r, r))
            }
            Self::Separate { outer, inner } => Ok(CornerProfile::new(
                outer
                    .resolve(&theme.spacing)
                    .ok_or(SceneError::InvalidRadius)?,
                inner
                    .resolve(&theme.spacing)
                    .ok_or(SceneError::InvalidRadius)?,
            )),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Up,
    Right,
    Down,
    Left,
}
impl From<Direction> for Edge {
    fn from(v: Direction) -> Self {
        match v {
            Direction::Up => Edge::Top,
            Direction::Right => Edge::Right,
            Direction::Down => Edge::Bottom,
            Direction::Left => Edge::Left,
        }
    }
}
#[derive(Clone, Debug)]
pub struct Item {
    node: Node,
    children: Vec<Item>,
    scope: Option<String>,
    text: Option<String>,
    tap: Option<String>,
    hoverable: bool,
    hover_color: Option<Color>,
    extension: Option<(Edge, String)>,
    rounding: Rounding,
    color: Option<Color>,
    stroke: Option<(Color, f64)>,
    merges: Vec<Vec<String>>,
}
pub fn item(id: impl Into<String>) -> Item {
    Item {
        node: Node::container(id),
        children: Vec::new(),
        scope: None,
        text: None,
        tap: None,
        hoverable: false,
        hover_color: None,
        extension: None,
        rounding: Rounding::Theme,
        color: None,
        stroke: None,
        merges: Vec::new(),
    }
}
/// Anonymous structural container; the default flow is a row.
pub fn container(children: impl IntoIterator<Item = Item>) -> Item {
    item("").children(children)
}
impl Item {
    pub fn children(mut self, children: impl IntoIterator<Item = Item>) -> Self {
        self.children = children.into_iter().collect();
        self
    }
    pub fn layout(mut self, value: Flow) -> Self {
        self.node = self.node.layout(value);
        self
    }
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.node = self.node.id(id);
        self
    }
    pub fn scope(mut self, id: impl Into<String>) -> Self {
        let id = id.into();
        self.scope = Some(id.clone());
        self.node = self.node.scope(id);
        self
    }
    pub fn text(mut self, value: impl Into<String>) -> Self {
        self.text = Some(value.into());
        self
    }
    /// Host action ID. The host recognizes a tap and dispatches this ID.
    pub fn on_tap(mut self, action: impl Into<String>) -> Self {
        self.tap = Some(action.into());
        self
    }
    /// Enable hover feedback without a click action. Clickable items imply this.
    pub fn hoverable(mut self) -> Self {
        self.hoverable = true;
        self
    }
    pub fn hover_color(mut self, color: Color) -> Self {
        self.hoverable = true;
        self.hover_color = Some(color);
        self
    }
    pub fn extend_to(mut self, target: impl Into<String>) -> Self {
        self.extension = Some((Edge::Auto, target.into()));
        self
    }
    pub fn extend_toward(mut self, direction: Direction, target: impl Into<String>) -> Self {
        self.extension = Some((direction.into(), target.into()));
        self
    }
    pub fn merge(mut self, items: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.merges
            .push(items.into_iter().map(Into::into).collect());
        self
    }
    pub fn round(mut self, value: impl Into<Rounding>) -> Self {
        self.rounding = value.into();
        self
    }
    pub fn color(mut self, value: Color) -> Self {
        self.color = Some(value);
        self
    }
    pub fn stroke(mut self, color: Color, width: f64) -> Self {
        self.stroke = Some((color, width));
        self
    }
    pub fn gap(mut self, value: impl Into<Spacing>) -> Self {
        self.node = self.node.gap(value);
        self
    }
    pub fn pad(mut self, value: impl Into<Spacing>) -> Self {
        self.node = self.node.pad(value);
        self
    }
    pub fn insets(mut self, value: Insets) -> Self {
        self.node = self.node.insets(value);
        self
    }
    pub fn width(mut self, value: impl Into<Sizing>) -> Self {
        self.node = self.node.width(value);
        self
    }
    pub fn height(mut self, value: impl Into<Sizing>) -> Self {
        self.node = self.node.height(value);
        self
    }
    pub fn min(mut self, width: f64, height: f64) -> Self {
        self.node = self.node.min_size(Size::new(width, height));
        self
    }
    pub fn max(mut self, width: f64, height: f64) -> Self {
        self.node = self.node.max_size(Size::new(width, height));
        self
    }
    pub fn grow(mut self, value: f64) -> Self {
        self.node = self.node.grow(value);
        self
    }
    pub fn shrink(mut self, value: f64) -> Self {
        self.node = self.node.shrink(value);
        self
    }
    pub fn wrap(mut self) -> Self {
        self.node = self.node.wrap();
        self
    }
    pub fn pack(mut self, value: Justify) -> Self {
        self.node = self.node.pack(value);
        self
    }
    pub fn position(mut self, x: Horizontal, y: Vertical) -> Self {
        self.node = self.node.position(x, y);
        self
    }
    pub fn center(self) -> Self {
        self.position(Horizontal::Center, Vertical::Middle)
    }
    pub fn align(mut self, value: Align) -> Self {
        self.node = self.node.align(value);
        self
    }
    pub fn align_self(mut self, value: Align) -> Self {
        self.node = self.node.align_self(value);
        self
    }
    pub fn columns(mut self, value: impl IntoIterator<Item = Track>) -> Self {
        self.node = self.node.columns(value);
        self
    }
    pub fn rows(mut self, value: impl IntoIterator<Item = Track>) -> Self {
        self.node = self.node.rows(value);
        self
    }
    pub fn cell(mut self, column: usize, row: usize) -> Self {
        self.node = self.node.cell(column, row);
        self
    }
    pub fn span(mut self, columns: usize, rows: usize) -> Self {
        self.node = self.node.span(columns, rows);
        self
    }
    pub fn place(mut self, x: Horizontal, y: Vertical) -> Self {
        self.node = self.node.place(x, y);
        self
    }
    pub fn build(self) -> Result<Ui, SceneError> {
        self.build_with(Theme::default())
    }
    pub fn build_with(self, theme: Theme) -> Result<Ui, SceneError> {
        if !theme.valid() {
            return Err(SceneError::InvalidTheme);
        }
        let mut compiler = Compiler {
            theme,
            surfaces: Vec::new(),
            roundings: BTreeMap::new(),
            items: BTreeMap::new(),
            order: Vec::new(),
            groups: Vec::new(),
            count: 0,
        };
        let root = compiler.visit(self, "", "0", 0, None)?;
        for group in &compiler.groups {
            if group.members.is_empty() {
                return Err(SceneError::InvalidModifier("merge needs at least one item"));
            }
            for member in &group.members {
                let mut ancestor = Some(member.as_str());
                while ancestor.is_some_and(|id| id != group.owner) {
                    ancestor = ancestor
                        .and_then(|id| compiler.items.get(id))
                        .and_then(|i| i.parent.as_deref());
                }
                if ancestor.is_none() {
                    return Err(SceneError::InvalidModifier(
                        "merge must be declared on a common ancestor of its members",
                    ));
                }
                let m = compiler
                    .items
                    .get_mut(member)
                    .ok_or_else(|| SceneError::MissingLayoutFrame(member.clone()))?;
                if m.merged {
                    return Err(SceneError::InvalidModifier(
                        "an item can belong to only one merge group",
                    ));
                }
                m.merged = true;
            }
        }
        let mut spec = SceneSpec::new(root).theme(theme);
        spec.surfaces = compiler.surfaces;
        Ok(Ui {
            spec,
            items: compiler.items,
            order: compiler.order,
            groups: compiler.groups,
            roundings: compiler.roundings,
            colors: theme.colors().map_err(|_| SceneError::InvalidTheme)?,
        })
    }
}
#[derive(Clone, Debug)]
pub struct ItemInfo {
    pub text: Option<String>,
    pub tap: Option<String>,
    pub hoverable: bool,
    pub hover_color: Option<Color>,
    pub(crate) parent: Option<String>,
    pub color: Option<Color>,
    pub stroke: Option<(Color, f64)>,
    merged: bool,
}
#[derive(Clone, Debug)]
pub(crate) struct Group {
    pub(crate) id: String,
    pub(crate) owner: String,
    pub(crate) members: Vec<String>,
}
#[derive(Clone, Debug)]
pub struct Ui {
    spec: SceneSpec,
    roundings: BTreeMap<String, Rounding>,
    colors: Colors,
    pub(crate) items: BTreeMap<String, ItemInfo>,
    pub(crate) order: Vec<String>,
    pub(crate) groups: Vec<Group>,
}
impl Ui {
    pub fn colors(&self) -> &Colors {
        &self.colors
    }
    /// Apply colors, spacing and rounding together; a failed update changes nothing.
    pub fn set_theme(&mut self, theme: Theme) -> Result<ResolvedScene, crate::StyleError> {
        self.set_theme_with(theme, |id, _, _| {
            Err(mui_layout::Error::MissingMeasurement(id.into()))
        })
    }
    pub fn set_theme_with(
        &mut self,
        theme: Theme,
        measure: impl FnMut(&str, &str, MeasureInput) -> Result<Size, mui_layout::Error>,
    ) -> Result<ResolvedScene, crate::StyleError> {
        if !theme.valid() {
            return Err(SceneError::InvalidTheme.into());
        }
        let mut next = self.clone();
        next.spec.theme = theme;
        next.colors = theme.colors()?;
        for surface in &mut next.spec.surfaces {
            let profile = next.roundings[&surface.id].profile(&theme)?;
            match &mut surface.source {
                SurfaceSource::Frame { radius, .. } => {
                    *radius = FrameRadius::Absolute(profile.convex)
                }
                SurfaceSource::Merge { corners, .. } => *corners = CornerRule::Absolute(profile),
                _ => {}
            }
        }
        let scene = next.resolve_with(measure)?;
        next.resolved_styles(None)?;
        // Validate every hover state before publishing a stronger contrast policy.
        for (id, info) in &next.items {
            if info.hoverable {
                next.resolved_styles(Some(id))?;
            }
        }
        *self = next;
        Ok(scene)
    }
    /// Read-only low-level form for integrations. Use set_theme for live updates.
    pub fn scene_spec(&self) -> &SceneSpec {
        &self.spec
    }
    pub fn theme(&self) -> &Theme {
        &self.spec.theme
    }
    pub fn available_width(mut self, width: f64) -> Self {
        self.spec = self.spec.available_width(width);
        self
    }
    pub fn offered(mut self, width: f64, height: f64) -> Self {
        self.spec = self.spec.offered(Size::new(width, height));
        self
    }
    pub fn info(&self, id: &str) -> Option<&ItemInfo> {
        self.items.get(id)
    }
    pub fn resolve(&self) -> Result<ResolvedScene, SceneError> {
        resolve_scene(&self.spec)
    }
    /// Measure text with actual host font metrics. Padding is handled by layout.
    pub fn resolve_with(
        &self,
        mut measure: impl FnMut(&str, &str, MeasureInput) -> Result<Size, mui_layout::Error>,
    ) -> Result<ResolvedScene, SceneError> {
        resolve_scene_measured(&self.spec, |id, input| {
            let text = self
                .items
                .get(id)
                .and_then(|i| i.text.as_deref())
                .ok_or_else(|| mui_layout::Error::MissingMeasurement(id.into()))?;
            measure(id, text, input)
        })
    }
    /// Find the deepest, last-authored action under a point in the original layout.
    /// Call after the host recognizes a completed tap; decorative bridges are not targets.
    pub fn tap_at<'a>(&'a self, scene: &ResolvedScene, x: f64, y: f64) -> Option<&'a str> {
        self.hover_at(scene, x, y)
            .and_then(|id| self.items[id].tap.as_deref())
    }
    /// Shared hover/tap target resolution using original content bounds.
    pub fn hover_at<'a>(&'a self, scene: &ResolvedScene, x: f64, y: f64) -> Option<&'a str> {
        self.order.iter().rev().find_map(|id| {
            let f = scene.layout.frame(id)?;
            (self.items[id].hoverable && x >= f.x && x < f.right() && y >= f.y && y < f.bottom())
                .then_some(id.as_str())
        })
    }
    /// Draw fills/strokes in order; draw text separately using original layout frames.
    /// A merged outline replaces its members' individual outlines.
    pub fn outlines<'a>(
        &'a self,
        scene: &'a ResolvedScene,
    ) -> Vec<(&'a ResolvedSurface, &'a ItemInfo)> {
        let mut result = Vec::new();
        for id in &self.order {
            let info = &self.items[id];
            if !info.merged {
                if let Some(s) = scene.surface(id) {
                    result.push((s, info));
                }
            }
            for group in self.groups.iter().filter(|g| &g.owner == id) {
                if let Some(s) = scene.surface(&group.id) {
                    result.push((s, &self.items[&group.members[0]]));
                }
            }
        }
        result
    }
}
struct Compiler {
    theme: Theme,
    surfaces: Vec<SurfaceSpec>,
    roundings: BTreeMap<String, Rounding>,
    items: BTreeMap<String, ItemInfo>,
    order: Vec<String>,
    groups: Vec<Group>,
    count: usize,
}
fn reference(scope: &str, key: &str) -> String {
    key.strip_prefix('/')
        .map_or_else(|| format!("{scope}{key}"), str::to_owned)
}
impl Compiler {
    fn visit(
        &mut self,
        item: Item,
        parent_scope: &str,
        path: &str,
        depth: usize,
        parent: Option<String>,
    ) -> Result<Node, SceneError> {
        self.count += 1;
        if self.count > 2048 || depth > 64 {
            return Err(SceneError::DependencyDepth);
        }
        let scope = item.scope.as_ref().map_or_else(
            || parent_scope.to_owned(),
            |s| format!("{parent_scope}{s}/"),
        );
        let key = format!(
            "{scope}{}",
            if item.node.key().is_empty() {
                format!("@{path}")
            } else {
                item.node.key().to_owned()
            }
        );
        if self.items.contains_key(&key) {
            return Err(SceneError::DuplicateSurface(key));
        }
        if item.text.is_some() && !item.children.is_empty() {
            return Err(SceneError::InvalidModifier(
                "put text in its own child when an item also has children",
            ));
        }
        if item.tap.as_ref().is_some_and(String::is_empty) {
            return Err(SceneError::InvalidModifier("tap action must not be empty"));
        }
        if item.stroke.is_some_and(|(_, w)| !w.is_finite() || w < 0.) {
            return Err(SceneError::InvalidModifier(
                "stroke width must be finite and nonnegative",
            ));
        }
        for color in [item.color, item.hover_color, item.stroke.map(|s| s.0)]
            .into_iter()
            .flatten()
        {
            if matches!(color,Color::Primary(i)|Color::PrimarySoft(i) if i>=3)
                || matches!(color,Color::Status(i) if i>=4)
            {
                return Err(SceneError::InvalidTheme);
            }
        }
        let profile = item.rounding.profile(&self.theme)?;
        let mut surface =
            SurfaceSpec::frame(&key, &key).radius(FrameRadius::Absolute(profile.convex));
        if let Some((edge, target)) = item.extension {
            surface = surface.extend_to(edge, reference(&scope, &target));
        }
        self.roundings.insert(key.clone(), item.rounding);
        self.surfaces.push(surface);
        self.order.push(key.clone());
        self.items.insert(
            key.clone(),
            ItemInfo {
                text: item.text.clone(),
                hoverable: item.hoverable || item.tap.is_some(),
                hover_color: item.hover_color,
                parent,
                tap: item.tap,
                color: item.color,
                stroke: item.stroke,
                merged: false,
            },
        );
        for (i, members) in item.merges.into_iter().enumerate() {
            let members: Vec<_> = members.iter().map(|m| reference(&scope, m)).collect();
            let id = format!("@merge:{key}:{i}");
            self.roundings.insert(id.clone(), item.rounding);
            self.surfaces.push(
                SurfaceSpec::merge(&id, members.clone()).corners(CornerRule::Absolute(profile)),
            );
            self.groups.push(Group {
                id,
                owner: key.clone(),
                members,
            });
        }
        let children = item
            .children
            .into_iter()
            .enumerate()
            .map(|(i, c)| {
                self.visit(
                    c,
                    &scope,
                    &format!("{path}.{i}"),
                    depth + 1,
                    Some(key.clone()),
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(if item.text.is_some() {
            item.node.measured_content()
        } else {
            item.node.with_children(children)
        })
    }
}
