//! Payload-bearing authoring trees using the shared Taffy layout engine.
pub use crate::{
    Align, Error, Frame, Insets, Justify, Layout, Limits, Size, Spacing, SpacingScale, SpacingToken,
};
use std::collections::BTreeMap;
/// A length on one axis. `Auto` is measured, `Px` is fixed, `Pct` is a share
/// of the parent's inner extent -- and counts as `Auto` while the parent is
/// still hugging, since there is nothing to take a share of yet.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Len {
    #[default]
    Auto,
    Px(f64),
    Pct(f64),
}
impl From<f64> for Len {
    fn from(v: f64) -> Self {
        Self::Px(v)
    }
}
impl Len {
    fn valid(self, limit: f64) -> bool {
        match self {
            Self::Auto => true,
            Self::Px(v) => v.is_finite() && (0.0..=limit).contains(&v),
            Self::Pct(p) => p.is_finite() && (0.0..=100.0).contains(&p),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Kind<P> {
    /// Opaque content of a declared size.
    Leaf,
    /// Content the solver cannot size itself -- text, mostly. Measured by the
    /// callback handed to [`resolve_with`].
    Content,
    Branch {
        vertical: bool,
        children: Vec<Node<P>>,
    },
    Overlay(Vec<Node<P>>),
    Grid {
        cols: usize,
        children: Vec<Node<P>>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Node<P = ()> {
    id: Option<String>,
    kind: Kind<P>,
    payload: P,
    gap: Spacing,
    /// Pixel insets, unless `pad` names a token for all four sides.
    padding: Insets,
    pad: Option<SpacingToken>,
    minimum: Size,
    maximum: Option<Size>,
    width: Len,
    height: Len,
    aspect: Option<f64>,
    grow: f64,
    basis: Option<f64>,
    shrink: f64,
    align: Align,
    align_self: Option<Align>,
    justify: Justify,
    anchor: Option<(Align, Align)>,
    offset: [f64; 2],
}

impl<P: Default> Node<P> {
    fn new(kind: Kind<P>) -> Self {
        Self {
            id: None,
            kind,
            payload: P::default(),
            gap: Spacing::Px(0.0),
            padding: Insets::ZERO,
            pad: None,
            minimum: Size::ZERO,
            maximum: None,
            width: Len::Auto,
            height: Len::Auto,
            aspect: None,
            grow: 0.0,
            basis: None,
            shrink: 1.0,
            align: Align::Stretch,
            align_self: None,
            justify: Justify::Start,
            anchor: None,
            offset: [0.0; 2],
        }
    }
    /// Content whose size is already known: an icon cell, a spacer.
    pub fn leaf(width: f64, height: f64) -> Self {
        Self::new(Kind::Leaf).size(width, height)
    }
    /// Content measured by the callback given to [`resolve_with`].
    pub fn content() -> Self {
        Self::new(Kind::Content)
    }
    /// Children laid out left to right.
    pub fn row(children: impl IntoIterator<Item = Self>) -> Self {
        Self::new(Kind::Branch {
            vertical: false,
            children: children.into_iter().collect(),
        })
    }
    /// Children laid out top to bottom.
    pub fn column(children: impl IntoIterator<Item = Self>) -> Self {
        Self::new(Kind::Branch {
            vertical: true,
            children: children.into_iter().collect(),
        })
    }
    /// Children sharing one rect, back to front. Each may `anchor` itself.
    pub fn overlay(children: impl IntoIterator<Item = Self>) -> Self {
        Self::new(Kind::Overlay(children.into_iter().collect()))
    }
    /// Children flowed into `cols` equal columns, row by row. Rows take the
    /// height of their tallest child and share any surplus equally.
    pub fn grid(cols: usize, children: impl IntoIterator<Item = Self>) -> Self {
        Self::new(Kind::Grid {
            cols,
            children: children.into_iter().collect(),
        })
    }
}

impl<P> Node<P> {
    /// Name this node, so `Layout::frame` can find it. Structural nodes need no
    /// name and cost nothing unnamed.
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }
    pub fn key(&self) -> Option<&str> {
        self.id.as_deref()
    }
    pub fn with(mut self, payload: P) -> Self {
        self.payload = payload;
        self
    }
    pub fn payload(&self) -> &P {
        &self.payload
    }
    pub fn payload_mut(&mut self) -> &mut P {
        &mut self.payload
    }
    pub fn children(&self) -> &[Self] {
        match &self.kind {
            Kind::Branch { children, .. }
            | Kind::Overlay(children)
            | Kind::Grid { children, .. } => children,
            _ => &[],
        }
    }
    pub fn children_mut(&mut self) -> &mut [Self] {
        match &mut self.kind {
            Kind::Branch { children, .. }
            | Kind::Overlay(children)
            | Kind::Grid { children, .. } => children,
            _ => &mut [],
        }
    }
    /// Containers stretch; content centres. This is what `Align::Stretch`
    /// consults.
    pub fn is_container(&self) -> bool {
        !matches!(self.kind, Kind::Leaf | Kind::Content)
    }
    pub fn gap(mut self, gap: impl Into<Spacing>) -> Self {
        self.gap = gap.into();
        self
    }
    /// The same on all four sides: `.pad(12.0)` or `.pad(M)`.
    pub fn pad(mut self, padding: impl Into<Spacing>) -> Self {
        match padding.into() {
            Spacing::Px(v) => self.padding = Insets::all(v),
            Spacing::Token(t) => self.pad = Some(t),
        }
        self
    }
    pub fn pad_xy(mut self, horizontal: f64, vertical: f64) -> Self {
        self.padding = Insets::symmetric(horizontal, vertical);
        self.pad = None;
        self
    }
    pub fn insets(mut self, insets: Insets) -> Self {
        self.padding = insets;
        self.pad = None;
        self
    }
    /// What the padding comes to under `scale`.
    pub fn padding(&self, scale: SpacingScale) -> Insets {
        self.pad.map_or(self.padding, |t| Insets::all(scale.get(t)))
    }
    /// Declared outer size on both axes. `Auto` measures, `Px` fixes, `Pct`
    /// takes a share of the parent: `.size(Len::Pct(50.0), 24.0)`.
    pub fn size(mut self, width: impl Into<Len>, height: impl Into<Len>) -> Self {
        self.width = width.into();
        self.height = height.into();
        self
    }
    pub fn width(mut self, width: impl Into<Len>) -> Self {
        self.width = width.into();
        self
    }
    pub fn height(mut self, height: impl Into<Len>) -> Self {
        self.height = height.into();
        self
    }
    /// `width / height`. Fills in whichever axis was left `Auto` -- at measure
    /// from a fixed sibling axis, at arrange from the allocated one.
    pub fn aspect(mut self, ratio: f64) -> Self {
        self.aspect = Some(ratio);
        self
    }
    pub fn min_width(mut self, width: f64) -> Self {
        self.minimum.width = width;
        self
    }
    pub fn min_height(mut self, height: f64) -> Self {
        self.minimum.height = height;
        self
    }
    pub fn min_size(mut self, size: Size) -> Self {
        self.minimum = size;
        self
    }
    pub fn max_size(mut self, size: Size) -> Self {
        self.maximum = Some(size);
        self
    }
    pub fn grow(mut self, weight: f64) -> Self {
        self.grow = weight;
        self
    }
    /// `grow(1.0)`: take a share of the surplus.
    pub fn expand(self) -> Self {
        self.grow(1.0)
    }
    /// Taffy uses browser flexbox intrinsic sizing when hugging.
    /// Start from this main-axis size instead of the measured one, before any
    /// growth or shrink. `basis(0.0)` is the only way to get equal *shares* of
    /// an axis rather than equal shares of the surplus: CSS `flex-basis: 0`,
    /// what `1fr` means.
    pub fn basis(mut self, basis: f64) -> Self {
        self.basis = Some(basis);
        self
    }
    /// Weight for absorbing a deficit, scaled by basis the way flexbox scales
    /// it. Defaults to 1; `shrink(0.0)` opts out, and `minimum` is the floor
    /// either way.
    pub fn shrink(mut self, weight: f64) -> Self {
        self.shrink = weight;
        self
    }
    /// `grow(weight).basis(0.0)`: an equal share of the axis per unit of
    /// weight, regardless of what the child measured. CSS `flex: <weight>`.
    pub fn flex(self, weight: f64) -> Self {
        self.grow(weight).basis(0.0)
    }
    /// Override the parent's `align` for this child alone. CSS `align-self`.
    pub fn align_self(mut self, align: Align) -> Self {
        self.align_self = Some(align);
        self
    }
    pub fn align(mut self, align: Align) -> Self {
        self.align = align;
        self
    }
    pub fn justify(mut self, justify: Justify) -> Self {
        self.justify = justify;
        self
    }
    /// Where this child sits inside an overlay or grid cell, per axis. Defaults
    /// to the parent's `align` on both axes, or `justify` for y once that is
    /// set.
    pub fn anchor(mut self, x: Align, y: Align) -> Self {
        self.anchor = Some((x, y));
        self
    }
    /// A nudge from the anchored position. The only coordinates in the system,
    /// and relative ones at that.
    pub fn offset(mut self, dx: f64, dy: f64) -> Self {
        self.offset = [dx, dy];
        self
    }
}

pub fn leaf(width: f64, height: f64) -> Node {
    Node::leaf(width, height)
}
pub fn row(children: impl IntoIterator<Item = Node>) -> Node {
    Node::row(children)
}
pub fn column(children: impl IntoIterator<Item = Node>) -> Node {
    Node::column(children)
}
pub fn overlay(children: impl IntoIterator<Item = Node>) -> Node {
    Node::overlay(children)
}
pub fn grid(cols: usize, children: impl IntoIterator<Item = Node>) -> Node {
    Node::grid(cols, children)
}

fn validate_node<P>(node: &Node<P>, l: Limits) -> Result<(), Error> {
    let finite = |v: f64| v.is_finite() && (0.0..=l.extent).contains(&v);
    if node.id.as_deref().is_some_and(str::is_empty)
        || !node.minimum.valid(l.extent)
        || node.maximum.is_some_and(|s| !s.valid(l.extent))
        || !node.padding.valid(l.extent)
        || ![node.grow, node.shrink].iter().all(|v| finite(*v))
        || node.basis.is_some_and(|b| !finite(b))
        || !node.width.valid(l.extent)
        || !node.height.valid(l.extent)
        || node.aspect.is_some_and(|a| !(a.is_finite() && a > 0.0))
        || !node
            .offset
            .iter()
            .all(|v| v.is_finite() && v.abs() <= l.extent)
        || matches!(node.kind, Kind::Grid { cols: 0, .. })
    {
        return Err(Error::InvalidValue);
    }
    if let Some(max) = node.maximum {
        if max.width + 1e-9 < node.minimum.width || max.height + 1e-9 < node.minimum.height {
            return Err(Error::InvalidValue);
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

fn horizontal(a: Align) -> crate::Horizontal {
    match a {
        Align::Start | Align::Baseline => crate::Horizontal::Left,
        Align::End => crate::Horizontal::Right,
        Align::Stretch => crate::Horizontal::Stretch,
        _ => crate::Horizontal::Center,
    }
}
fn vertical(a: Align) -> crate::Vertical {
    match a {
        Align::Start | Align::Baseline => crate::Vertical::Top,
        Align::End => crate::Vertical::Bottom,
        Align::Stretch => crate::Vertical::Stretch,
        _ => crate::Vertical::Middle,
    }
}
fn sizing(l: Len) -> Option<crate::Sizing> {
    match l {
        Len::Auto => None,
        Len::Px(v) => Some(crate::Sizing::Fixed(v)),
        Len::Pct(v) => Some(crate::Sizing::Percent(v)),
    }
}
impl<P> Node<P> {
    fn lower(
        &self,
        scale: SpacingScale,
        parent_align: Align,
        parent_vertical: bool,
        cell: Option<(Align, Align)>,
    ) -> crate::Node {
        let defaults = (
            self.align,
            match self.justify {
                Justify::Start => self.align,
                Justify::End => Align::End,
                _ => Align::Center,
            },
        );
        let children = self
            .children()
            .iter()
            .map(|c| {
                c.lower(
                    scale,
                    self.align,
                    matches!(self.kind, Kind::Branch { vertical: true, .. }),
                    matches!(self.kind, Kind::Overlay(_) | Kind::Grid { .. }).then_some(defaults),
                )
            })
            .collect::<Vec<_>>();
        let key = self.id.as_deref().unwrap_or("");
        let mut n = match self.kind {
            Kind::Leaf => crate::Node::leaf(key, Size::ZERO),
            Kind::Content => crate::Node::measured(key),
            Kind::Branch { vertical: true, .. } => crate::Node::column(key, children),
            Kind::Branch { .. } => crate::Node::row(key, children),
            Kind::Overlay(_) => crate::Node::overlay(key, children),
            Kind::Grid { cols, .. } => {
                crate::Node::row(key, children).layout(crate::Flow::Grid(cols))
            }
        };
        n.width = sizing(self.width);
        n.height = sizing(self.height);
        n.minimum = self.minimum;
        n.maximum = self.maximum;
        n.gap = self.gap;
        n = n.insets(self.padding(scale));
        if let Some(token) = self.pad {
            n.padding = [Spacing::Token(token); 4];
        }
        n.grow = self.grow;
        n.shrink = Some(self.shrink);
        n.align = self.align;
        n.justify = self.justify;
        let content_align = |a| {
            if a == Align::Stretch
                && !self.is_container()
                && (self.aspect.is_none() || !parent_vertical)
            {
                Align::Center
            } else {
                a
            }
        };
        n.align_self = Some(content_align(self.align_self.unwrap_or(parent_align)));
        if let Some(default) = cell {
            let (x, y) = self.anchor.unwrap_or(default);
            n.place = Some((horizontal(content_align(x)), vertical(content_align(y))));
        }
        if matches!(self.kind, Kind::Overlay(_) | Kind::Grid { .. }) {
            n.physical = Some((horizontal(defaults.0), vertical(defaults.1)));
        }
        n.content_floor = true;
        n.aspect = self.aspect;
        n.basis = self.basis;
        n.offset = self.offset;
        n
    }
}
impl From<Node> for crate::Node {
    fn from(node: Node) -> Self {
        node.lower(SpacingScale::DEFAULT, Align::Stretch, false, None)
    }
}

pub fn resolve<P>(root: &Node<P>, offered: Option<Size>, limits: Limits) -> Result<Layout, Error> {
    resolve_with(root, offered, limits, SpacingScale::DEFAULT, |_| Size::ZERO)
}
pub fn resolve_with<P>(
    root: &Node<P>,
    offered: Option<Size>,
    limits: Limits,
    scale: SpacingScale,
    mut measurer: impl FnMut(&P) -> Size,
) -> Result<Layout, Error> {
    fn visit<'a, P>(
        node: &'a Node<P>,
        path: String,
        depth: usize,
        limits: Limits,
        entries: &mut Vec<(String, &'a Node<P>)>,
    ) -> Result<(), Error> {
        if depth > limits.depth || entries.len() >= limits.nodes {
            return Err(Error::BudgetExceeded);
        }
        validate_node(node, limits)?;
        entries.push((node.id.clone().unwrap_or_else(|| format!("@{path}")), node));
        for (i, c) in node.children().iter().enumerate() {
            visit(c, format!("{path}.{i}"), depth + 1, limits, entries)?;
        }
        Ok(())
    }
    let mut entries = Vec::new();
    visit(root, "0".into(), 0, limits, &mut entries)?;
    let mut sizes = BTreeMap::new();
    for (key, node) in &entries {
        if matches!(node.kind, Kind::Content) {
            sizes.insert(key.as_str(), measurer(&node.payload));
        }
    }
    let mut lowered = root.lower(scale, Align::Stretch, false, None);
    if let Some(size) = offered {
        lowered.width = Some(crate::Sizing::Fixed(size.width));
        lowered.height = Some(crate::Sizing::Fixed(size.height));
    }
    let mut result = crate::resolve_measured(
        &lowered,
        crate::Constraints {
            width: offered.map(|s| s.width),
            height: offered.map(|s| s.height),
        },
        limits,
        &scale,
        |key, _| {
            sizes
                .get(key)
                .copied()
                .ok_or_else(|| Error::MissingMeasurement(key.into()))
        },
    )?;
    result.frames.retain(|key, _| !key.starts_with('@'));
    Ok(result)
}

#[derive(Debug, Default)]
pub struct LayoutState {
    revision: u64,
    current: Option<Layout>,
}
impl LayoutState {
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn current(&self) -> Option<&Layout> {
        self.current.as_ref()
    }
    pub fn commit<P>(
        &mut self,
        root: &Node<P>,
        offered: Option<Size>,
        limits: Limits,
    ) -> Result<(), Error> {
        let next = resolve(root, offered, limits)?;
        let revision = self
            .revision
            .checked_add(1)
            .ok_or(Error::RevisionExhausted)?;
        self.current = Some(next);
        self.revision = revision;
        Ok(())
    }
}
