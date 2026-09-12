//! Renderer-independent intrinsic layout with a compact MUI-owned API.
//!
//! Flex and overlay layout is delegated to Taffy. MUI owns tokens, keys,
//! validation, adaptive direction, measurement, and transactional publication.
#![forbid(unsafe_code)]

use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Size {
    pub width: f64,
    pub height: f64,
}
impl Size {
    pub const fn new(width: f64, height: f64) -> Self {
        Self { width, height }
    }
    fn valid(self, limit: f64) -> bool {
        [self.width, self.height]
            .iter()
            .all(|n| n.is_finite() && *n >= 0.0 && *n <= limit)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Insets {
    pub left: f64,
    pub right: f64,
    pub top: f64,
    pub bottom: f64,
}
impl Insets {
    pub const ZERO: Self = Self {
        left: 0.0,
        right: 0.0,
        top: 0.0,
        bottom: 0.0,
    };
    pub const fn all(v: f64) -> Self {
        Self {
            left: v,
            right: v,
            top: v,
            bottom: v,
        }
    }
    pub const fn symmetric(horizontal: f64, vertical: f64) -> Self {
        Self {
            left: horizontal,
            right: horizontal,
            top: vertical,
            bottom: vertical,
        }
    }
    fn horizontal(self) -> f64 {
        self.left + self.right
    }
    fn vertical(self) -> f64 {
        self.top + self.bottom
    }
    fn valid(self, limit: f64) -> bool {
        [self.left, self.right, self.top, self.bottom]
            .iter()
            .all(|v| v.is_finite() && *v >= 0.0 && *v <= limit)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Align {
    Start,
    #[default]
    Center,
    End,
    Stretch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Justify {
    #[default]
    Start,
    Center,
    End,
    SpaceBetween,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Axis {
    #[default]
    Row,
    Column,
    Auto,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Sizing {
    Hug,
    Fill,
    Fixed(f64),
}
pub use Sizing::{Fill, Hug};
impl From<f64> for Sizing {
    fn from(v: f64) -> Self {
        Self::Fixed(v)
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Kind {
    Leaf(Size),
    Measured,
    Stack(Vec<Node>),
    Overlay(Vec<Node>),
}
#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    key: String,
    scope: Option<String>,
    kind: Kind,
    axis: Axis,
    gap: Spacing,
    padding: [Spacing; 4], // left, right, top, bottom
    minimum: Size,
    maximum: Option<Size>,
    grow: f64,
    shrink: f64,
    align: Align,
    justify: Justify,
    width: Option<Sizing>,
    height: Option<Sizing>,
    wrap: bool,
}
impl Node {
    fn new(key: impl Into<String>, kind: Kind) -> Self {
        Self {
            key: key.into(),
            scope: None,
            kind,
            axis: Axis::Row,
            gap: 0.0.into(),
            padding: [Spacing::Px(0.0); 4],
            minimum: Size::default(),
            maximum: None,
            grow: 0.0,
            shrink: 0.0,
            align: Align::Center,
            justify: Justify::Start,
            width: None,
            height: None,
            wrap: false,
        }
    }
    pub fn leaf(key: impl Into<String>, size: Size) -> Self {
        Self::new(key, Kind::Leaf(size))
    }
    /// A leaf measured through `resolve_measured`; returning cached font metrics is encouraged.
    pub fn measured(key: impl Into<String>) -> Self {
        Self::new(key, Kind::Measured).shrink(1.0)
    }
    pub fn row(key: impl Into<String>, children: impl IntoIterator<Item = Node>) -> Self {
        Self::new(key, Kind::Stack(children.into_iter().collect()))
    }
    pub fn column(key: impl Into<String>, children: impl IntoIterator<Item = Node>) -> Self {
        Self::row(key, children).axis(Axis::Column)
    }
    pub fn overlay(key: impl Into<String>, children: impl IntoIterator<Item = Node>) -> Self {
        Self::new(key, Kind::Overlay(children.into_iter().collect()))
    }
    /// Anonymous container, with a deterministic structural key. Explicitly key dynamic lists.
    pub fn flow(children: impl IntoIterator<Item = Node>) -> Self {
        Self::row("", children)
    }
    pub fn id(mut self, key: impl Into<String>) -> Self {
        self.key = key.into();
        self
    }
    /// Namespaces this component and all descendants, so reusable local keys cannot collide.
    pub fn scope(mut self, key: impl Into<String>) -> Self {
        self.scope = Some(key.into());
        self
    }
    pub fn axis(mut self, axis: Axis) -> Self {
        self.axis = axis;
        self
    }
    pub fn gap(mut self, value: impl Into<Spacing>) -> Self {
        self.gap = value.into();
        self
    }
    pub fn padding(mut self, value: impl Into<Spacing>) -> Self {
        self.padding = [value.into(); 4];
        self
    }
    pub fn pad(self, value: impl Into<Spacing>) -> Self {
        self.padding(value)
    }
    pub fn padding_xy(mut self, x: f64, y: f64) -> Self {
        self.padding = [x.into(), x.into(), y.into(), y.into()];
        self
    }
    pub fn insets(mut self, v: Insets) -> Self {
        self.padding = [v.left.into(), v.right.into(), v.top.into(), v.bottom.into()];
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
    pub fn shrink(mut self, weight: f64) -> Self {
        self.shrink = weight;
        self
    }
    /// Grow on the parent's main axis. For per-axis sizing use width/height(Fill).
    pub fn fill(self) -> Self {
        self.grow(1.0)
    }
    pub fn align(mut self, value: Align) -> Self {
        self.align = value;
        self
    }
    pub fn justify(mut self, value: Justify) -> Self {
        self.justify = value;
        self
    }
    pub fn width(mut self, value: impl Into<Sizing>) -> Self {
        self.width = Some(value.into());
        self
    }
    pub fn height(mut self, value: impl Into<Sizing>) -> Self {
        self.height = Some(value.into());
        self
    }
    pub fn wrap(mut self) -> Self {
        self.wrap = true;
        self
    }
    pub fn key(&self) -> &str {
        &self.key
    }
    fn children(&self) -> &[Node] {
        match &self.kind {
            Kind::Stack(v) | Kind::Overlay(v) => v,
            _ => &[],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub x: f64,
    pub y: f64,
    pub size: Size,
}
impl Frame {
    pub fn right(self) -> f64 {
        self.x + self.size.width
    }
    pub fn bottom(self) -> f64 {
        self.y + self.size.height
    }
    pub fn inset(self, i: Insets) -> Option<Self> {
        let w = self.size.width - i.horizontal();
        let h = self.size.height - i.vertical();
        (w >= 0.0 && h >= 0.0).then_some(Self {
            x: self.x + i.left,
            y: self.y + i.top,
            size: Size::new(w, h),
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    pub size: Size,
    frames: BTreeMap<String, Frame>,
}
impl Layout {
    pub fn frame(&self, key: &str) -> Option<Frame> {
        self.frames.get(key).copied()
    }
    pub fn frames(&self) -> impl Iterator<Item = (&str, Frame)> {
        self.frames.iter().map(|(k, v)| (k.as_str(), *v))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    InvalidValue,
    DuplicateKey(String),
    BudgetExceeded,
    InsufficientSpace(String),
    RevisionExhausted,
    MissingMeasurement(String),
    Backend(String),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "layout: {self:?}")
    }
}
impl std::error::Error for Error {}

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub nodes: usize,
    pub depth: usize,
    pub extent: f64,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            nodes: 4096,
            depth: 64,
            extent: 1e6,
        }
    }
}

mod solver;
mod spacing;
pub use solver::{
    resolve, resolve_measured, resolve_with_spacing, Available, Constraints, MeasureInput,
};
pub use spacing::{Gap, Pad, Spacing, SpacingScale, SpacingToken};

/// UI-thread transactional commit. This is not a CPU atomic and not an audio-thread data structure.
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
    pub fn commit(
        &mut self,
        root: &Node,
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
