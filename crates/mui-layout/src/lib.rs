//! Small, renderer-independent intrinsic layout for plugin UIs.
//!
//! This is intentionally not CSS. Children are measured from intrinsic sizes,
//! containers hug content by default, parents may offer an exact size, and
//! growth/align/justify determine how surplus is distributed.
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
    fn main(self, vertical: bool) -> f64 {
        if vertical {
            self.height
        } else {
            self.width
        }
    }
    fn cross(self, vertical: bool) -> f64 {
        if vertical {
            self.width
        } else {
            self.height
        }
    }
    fn axes(main: f64, cross: f64, vertical: bool) -> Self {
        if vertical {
            Self::new(cross, main)
        } else {
            Self::new(main, cross)
        }
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
    fn main_start(self, vertical: bool) -> f64 {
        if vertical {
            self.top
        } else {
            self.left
        }
    }
    fn cross_start(self, vertical: bool) -> f64 {
        if vertical {
            self.left
        } else {
            self.top
        }
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

#[derive(Clone, Debug, PartialEq)]
enum Kind {
    Leaf(Size),
    Stack { vertical: bool, children: Vec<Node> },
    Overlay { children: Vec<Node> },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    key: String,
    kind: Kind,
    gap: f64,
    padding: Insets,
    minimum: Size,
    maximum: Option<Size>,
    grow: f64,
    basis: Option<f64>,
    shrink: f64,
    align: Align,
    align_self: Option<Align>,
    justify: Justify,
}
impl Node {
    fn new(key: impl Into<String>, kind: Kind) -> Self {
        Self {
            key: key.into(),
            kind,
            gap: 0.0,
            padding: Insets::ZERO,
            minimum: Size::default(),
            maximum: None,
            grow: 0.0,
            basis: None,
            shrink: 1.0,
            align: Align::Center,
            align_self: None,
            justify: Justify::Start,
        }
    }
    pub fn leaf(key: impl Into<String>, size: Size) -> Self {
        Self::new(key, Kind::Leaf(size))
    }
    pub fn row(key: impl Into<String>, children: impl IntoIterator<Item = Node>) -> Self {
        Self::new(
            key,
            Kind::Stack {
                vertical: false,
                children: children.into_iter().collect(),
            },
        )
    }
    pub fn column(key: impl Into<String>, children: impl IntoIterator<Item = Node>) -> Self {
        Self::new(
            key,
            Kind::Stack {
                vertical: true,
                children: children.into_iter().collect(),
            },
        )
    }
    pub fn overlay(key: impl Into<String>, children: impl IntoIterator<Item = Node>) -> Self {
        Self::new(
            key,
            Kind::Overlay {
                children: children.into_iter().collect(),
            },
        )
    }
    pub fn gap(mut self, gap: f64) -> Self {
        self.gap = gap;
        self
    }
    pub fn padding(mut self, padding: f64) -> Self {
        self.padding = Insets::all(padding);
        self
    }
    pub fn padding_xy(mut self, horizontal: f64, vertical: f64) -> Self {
        self.padding = Insets::symmetric(horizontal, vertical);
        self
    }
    pub fn insets(mut self, insets: Insets) -> Self {
        self.padding = insets;
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
    pub fn fill(mut self) -> Self {
        self.grow = 1.0;
        self
    }
    /// Start from this main-axis size instead of the measured one, before any
    /// growth or shrink. `basis(0.0)` is the only way to get equal *shares* of
    /// an axis rather than equal shares of the surplus, which is what a
    /// left/centre/right bar with differently sized ends needs: CSS
    /// `flex-basis: 0`, and what `1fr` means.
    pub fn basis(mut self, basis: f64) -> Self {
        self.basis = Some(basis);
        self
    }
    /// Weight for absorbing a deficit, scaled by basis the way flexbox scales
    /// it. Defaults to 1, so a child compresses rather than overflowing;
    /// `shrink(0.0)` opts out, and `minimum` is the floor either way.
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
    pub fn key(&self) -> &str {
        &self.key
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

struct Measured<'a> {
    node: &'a Node,
    size: Size,
    /// The smallest this subtree may be squeezed to. A node's own `minimum` is
    /// only the start of it: a row cannot go below the sum of what its children
    /// refuse to go below, or it would be shrunk to a width its own contents
    /// then overflow.
    floor: Size,
    children: Vec<Measured<'a>>,
}

impl Measured<'_> {
    /// Where this child starts before growth or shrink: its declared `basis`,
    /// or what it measured.
    fn base(&self, vertical: bool) -> f64 {
        self.node.basis.unwrap_or(self.size.main(vertical))
    }
}

fn validate_node(node: &Node, l: Limits) -> Result<(), Error> {
    if node.key.is_empty()
        || !node.minimum.valid(l.extent)
        || node.maximum.is_some_and(|s| !s.valid(l.extent))
        || !node.padding.valid(l.extent)
        || ![node.gap, node.grow, node.shrink]
            .iter()
            .all(|v| v.is_finite() && *v >= 0.0 && *v <= l.extent)
        || node
            .basis
            .is_some_and(|b| !(b.is_finite() && (0.0..=l.extent).contains(&b)))
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

fn measure<'a>(
    node: &'a Node,
    depth: usize,
    left: &mut usize,
    l: Limits,
    keys: &mut BTreeMap<&'a str, ()>,
) -> Result<Measured<'a>, Error> {
    if depth > l.depth || *left == 0 {
        return Err(Error::BudgetExceeded);
    }
    *left -= 1;
    validate_node(node, l)?;
    if keys.insert(&node.key, ()).is_some() {
        return Err(Error::DuplicateKey(node.key.clone()));
    }
    let mut children = Vec::new();
    let (content, sunk) = match &node.kind {
        Kind::Leaf(s) => {
            if !s.valid(l.extent) {
                return Err(Error::InvalidValue);
            }
            // A leaf is opaque -- it *is* the content measurement, so there is
            // no smaller version of it to discover. Declare `min_size` on one
            // that must not be squeezed.
            (*s, Size::default())
        }
        Kind::Stack {
            vertical,
            children: source,
        } => {
            for child in source {
                children.push(measure(child, depth + 1, left, l, keys)?);
            }
            let gaps = source.len().saturating_sub(1) as f64 * node.gap;
            // Intrinsic main is not the sum of the children: a child with a
            // `basis` contributes that instead, and then the row has to be wide
            // enough that its *share* of the surplus still clears its content.
            // This is the flex fraction. Without it a hugging row collapses to
            // its non-flexible children and squashes the rest.
            let base = children.iter().map(|c| c.base(*vertical)).sum::<f64>();
            let total_grow = children.iter().map(|c| c.node.grow).sum::<f64>();
            let surplus = children
                .iter()
                .filter(|c| c.node.grow > 0.0)
                .map(|c| (c.size.main(*vertical) - c.base(*vertical)) * total_grow / c.node.grow)
                .fold(0.0, f64::max);
            let cross = |f: fn(&Measured<'_>) -> Size| {
                children
                    .iter()
                    .map(|c| f(c).cross(*vertical))
                    .fold(0.0, f64::max)
            };
            let floor_main = children
                .iter()
                .map(|c| c.floor.main(*vertical))
                .sum::<f64>()
                + gaps;
            (
                Size::axes(base + surplus + gaps, cross(|c| c.size), *vertical),
                Size::axes(floor_main, cross(|c| c.floor), *vertical),
            )
        }
        Kind::Overlay { children: source } => {
            for child in source {
                children.push(measure(child, depth + 1, left, l, keys)?);
            }
            let envelope = |f: fn(&Measured<'_>) -> Size| {
                Size::new(
                    children.iter().map(|c| f(c).width).fold(0.0, f64::max),
                    children.iter().map(|c| f(c).height).fold(0.0, f64::max),
                )
            };
            (envelope(|c| c.size), envelope(|c| c.floor))
        }
    };
    let pad = |s: Size| {
        Size::new(
            (s.width + node.padding.horizontal()).max(node.minimum.width),
            (s.height + node.padding.vertical()).max(node.minimum.height),
        )
    };
    let (size, floor) = (pad(content), pad(sunk));
    if !size.valid(l.extent) {
        return Err(Error::BudgetExceeded);
    }
    if let Some(max) = node.maximum {
        if size.width > max.width + 1e-9 || size.height > max.height + 1e-9 {
            return Err(Error::InsufficientSpace(node.key.clone()));
        }
    }
    Ok(Measured {
        node,
        size,
        floor,
        children,
    })
}

/// Hand every child its main-axis size. Surplus goes out by `grow`; a deficit
/// comes back by `shrink` scaled by basis, which is how flexbox weights it, and
/// never takes a child below its declared `minimum`. Either way a child that
/// can absorb no more is dropped from the pool and the remainder redistributes
/// over the rest, which is what the outer loop is for.
///
/// Only one direction runs. A row that overflows never grew.
fn distribute(m: &Measured<'_>, vertical: bool, inner_main: f64, base_gap: f64) -> Vec<f64> {
    let base: Vec<f64> = m.children.iter().map(|c| c.base(vertical)).collect();
    let mut allocated = base.clone();
    let gaps = base_gap * m.children.len().saturating_sub(1) as f64;
    let mut free = inner_main - base.iter().sum::<f64>() - gaps;
    let growing = free > 0.0;
    let room = |i: usize, allocated: &[f64]| {
        let c = &m.children[i];
        let edge = if growing {
            c.node.maximum.map_or(1e6, |s| s.main(vertical)) - allocated[i]
        } else {
            allocated[i] - m.children[i].floor.main(vertical)
        };
        edge.max(0.0)
    };
    let weight = |i: usize| {
        let c = &m.children[i];
        if growing {
            c.node.grow
        } else {
            c.node.shrink * base[i]
        }
    };
    for _ in 0..=m.children.len() {
        let active: Vec<usize> = (0..m.children.len())
            .filter(|i| weight(*i) > 0.0 && room(*i, &allocated) > 1e-8)
            .collect();
        let total = active.iter().map(|i| weight(*i)).sum::<f64>();
        if free.abs() < 1e-8 || total <= 0.0 {
            break;
        }
        let budget = free;
        for i in active {
            let limit = room(i, &allocated);
            let delta = (budget * weight(i) / total).clamp(-limit, limit);
            allocated[i] += delta;
            free -= delta;
        }
    }
    allocated
}

fn arrange(
    m: &Measured<'_>,
    origin: [f64; 2],
    size: Size,
    out: &mut BTreeMap<String, Frame>,
) -> Result<(), Error> {
    let n = m.node;
    // Content is squeezable -- that is the whole point of shrink -- but the
    // floor is not: it is every minimum in this subtree, summed along the axis
    // they sit on.
    if size.width + 1e-8 < m.floor.width || size.height + 1e-8 < m.floor.height {
        return Err(Error::InsufficientSpace(n.key.clone()));
    }
    out.insert(
        n.key.clone(),
        Frame {
            x: origin[0],
            y: origin[1],
            size,
        },
    );
    match &n.kind {
        Kind::Leaf(_) => Ok(()),
        Kind::Overlay { .. } => {
            let inner = Size::new(
                (size.width - n.padding.horizontal()).max(0.0),
                (size.height - n.padding.vertical()).max(0.0),
            );
            for c in &m.children {
                let max = c.node.maximum.unwrap_or(Size::new(1e6, 1e6));
                let align = c.node.align_self.unwrap_or(n.align);
                let w = if align == Align::Stretch {
                    inner.width.min(max.width)
                } else {
                    c.size.width
                };
                let h = if align == Align::Stretch {
                    inner.height.min(max.height)
                } else {
                    c.size.height
                };
                let x = n.padding.left
                    + match align {
                        Align::Center => (inner.width - w) * 0.5,
                        Align::End => inner.width - w,
                        _ => 0.0,
                    };
                let y = n.padding.top
                    + match n.justify {
                        Justify::Center => (inner.height - h) * 0.5,
                        Justify::End => inner.height - h,
                        _ => 0.0,
                    };
                arrange(c, [origin[0] + x, origin[1] + y], Size::new(w, h), out)?;
            }
            Ok(())
        }
        Kind::Stack { vertical, .. } => {
            let inner = Size::new(
                (size.width - n.padding.horizontal()).max(0.0),
                (size.height - n.padding.vertical()).max(0.0),
            );
            let allocated = distribute(m, *vertical, inner.main(*vertical), n.gap);
            let children_main = allocated.iter().sum::<f64>();
            let count = m.children.len();
            let nominal_gap = n.gap * count.saturating_sub(1) as f64;
            let residual = (inner.main(*vertical) - children_main - nominal_gap).max(0.0);
            let (mut cursor, extra_gap) = match n.justify {
                Justify::Start => (n.padding.main_start(*vertical), 0.0),
                Justify::Center => (n.padding.main_start(*vertical) + residual * 0.5, 0.0),
                Justify::End => (n.padding.main_start(*vertical) + residual, 0.0),
                Justify::SpaceBetween if count > 1 => (
                    n.padding.main_start(*vertical),
                    residual / (count - 1) as f64,
                ),
                Justify::SpaceBetween => (n.padding.main_start(*vertical) + residual * 0.5, 0.0),
            };
            for (i, c) in m.children.iter().enumerate() {
                let available = inner.cross(*vertical);
                let natural = c.size.cross(*vertical);
                let align = c.node.align_self.unwrap_or(n.align);
                let cross = if align == Align::Stretch {
                    available.min(c.node.maximum.map_or(available, |s| s.cross(*vertical)))
                } else {
                    natural
                };
                let slack = (available - cross).max(0.0);
                let cross_pos = n.padding.cross_start(*vertical)
                    + match align {
                        Align::Center => slack * 0.5,
                        Align::End => slack,
                        _ => 0.0,
                    };
                let pos = if *vertical {
                    [origin[0] + cross_pos, origin[1] + cursor]
                } else {
                    [origin[0] + cursor, origin[1] + cross_pos]
                };
                arrange(c, pos, Size::axes(allocated[i], cross, *vertical), out)?;
                cursor += allocated[i] + n.gap + extra_gap;
            }
            Ok(())
        }
    }
}

/// `None` means hug intrinsic content. `Some` is an exact offered parent size.
/// Text shaping/wrapping is deliberately an external leaf-measurement concern.
pub fn resolve(root: &Node, offered: Option<Size>, limits: Limits) -> Result<Layout, Error> {
    if !limits.extent.is_finite() || limits.extent <= 0.0 || limits.nodes == 0 || limits.depth > 256
    {
        return Err(Error::InvalidValue);
    }
    let mut left = limits.nodes;
    let m = measure(root, 0, &mut left, limits, &mut BTreeMap::new())?;
    let size = offered.unwrap_or(m.size);
    if !size.valid(limits.extent) {
        return Err(Error::InvalidValue);
    }
    if root
        .maximum
        .is_some_and(|max| size.width > max.width || size.height > max.height)
    {
        return Err(Error::InsufficientSpace(root.key.clone()));
    }
    let mut frames = BTreeMap::new();
    arrange(&m, [0.0, 0.0], size, &mut frames)?;
    Ok(Layout { size, frames })
}

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

#[cfg(test)]
mod tests {
    use super::*;
    fn leaf(k: &str, w: f64, h: f64) -> Node {
        Node::leaf(k, Size::new(w, h))
    }
    #[test]
    fn intrinsic_chain() {
        let controls = Node::column(
            "controls",
            [
                leaf("add", 28., 28.),
                leaf("a", 28., 28.),
                leaf("b", 28., 28.),
            ],
        )
        .gap(10.);
        let tree =
            Node::column("tab", [Node::column("pill", [controls]).padding(10.)]).padding(12.);
        let l = resolve(&tree, None, Default::default()).unwrap();
        assert_eq!(l.size, Size::new(72., 148.));
        assert_eq!(l.frame("pill").unwrap().size, Size::new(48., 124.));
    }
    #[test]
    fn asymmetric_padding() {
        let t = Node::column("p", [leaf("c", 10., 20.)]).insets(Insets {
            left: 1.,
            right: 2.,
            top: 3.,
            bottom: 4.,
        });
        let l = resolve(&t, None, Default::default()).unwrap();
        assert_eq!(l.size, Size::new(13., 27.));
        assert_eq!(l.frame("c").unwrap().x, 1.0);
        assert_eq!(l.frame("c").unwrap().y, 3.);
    }
    #[test]
    fn space_between() {
        let t = Node::row("r", [leaf("a", 10., 10.), leaf("b", 10., 10.)])
            .justify(Justify::SpaceBetween);
        let l = resolve(&t, Some(Size::new(100., 10.)), Default::default()).unwrap();
        assert_eq!(l.frame("a").unwrap().x, 0.);
        assert_eq!(l.frame("b").unwrap().x, 90.);
    }
    #[test]
    fn overlay_centers() {
        let t = Node::overlay("o", [leaf("a", 10., 10.)])
            .padding(5.)
            .align(Align::Center)
            .justify(Justify::Center);
        let l = resolve(&t, Some(Size::new(40., 50.)), Default::default()).unwrap();
        let f = l.frame("a").unwrap();
        assert_eq!(f.x, 15.);
        assert_eq!(f.y, 20.);
    }
    #[test]
    fn growth_caps() {
        let t = Node::row(
            "r",
            [
                leaf("a", 10., 10.).grow(1.).max_size(Size::new(20., 20.)),
                leaf("b", 10., 10.).grow(1.),
            ],
        );
        let l = resolve(&t, Some(Size::new(100., 10.)), Default::default()).unwrap();
        assert_eq!(l.frame("a").unwrap().size.width, 20.);
        assert_eq!(l.frame("b").unwrap().size.width, 80.);
    }
    #[test]
    fn failed_commit_preserves() {
        let mut s = LayoutState::default();
        s.commit(&leaf("x", 1., 1.), None, Default::default())
            .unwrap();
        let old = s.current.clone();
        assert!(s
            .commit(&leaf("bad", f64::NAN, 1.), None, Default::default())
            .is_err());
        assert_eq!(s.current, old);
        assert_eq!(s.revision, 1);
    }
    #[test]
    fn duplicate_ids() {
        assert!(matches!(
            resolve(
                &Node::row("r", [leaf("x", 1., 1.), leaf("x", 1., 1.)]),
                None,
                Default::default()
            ),
            Err(Error::DuplicateKey(_))
        ));
    }
    #[test]
    fn a_declared_minimum_is_the_floor_and_content_alone_is_not() {
        // Content is squeezable; that is what shrink means.
        let l = resolve(
            &leaf("x", 100., 100.),
            Some(Size::new(50., 50.)),
            Default::default(),
        )
        .unwrap();
        assert_eq!(l.frame("x").unwrap().size, Size::new(50., 50.));
        // A minimum the author asked for is not.
        assert!(matches!(
            resolve(
                &leaf("x", 100., 100.).min_size(Size::new(100., 100.)),
                Some(Size::new(50., 50.)),
                Default::default()
            ),
            Err(Error::InsufficientSpace(_))
        ));
    }

    /// The left/centre/right bar. Ends of different widths -- 50 and 20 -- must
    /// still leave the middle child centred on the *container*, which neither
    /// `SpaceBetween` nor `grow` can do, because both hand out the surplus left
    /// after intrinsic sizing and so inherit the ends' asymmetry.
    #[test]
    fn basis_zero_shares_the_axis_rather_than_the_surplus() {
        let bar = |ends: Node| {
            let l = resolve(&ends, Some(Size::new(400., 20.)), Default::default()).unwrap();
            let m = l.frame("m").unwrap();
            m.x + m.size.width / 2.
        };
        let slots = |shape: fn(Node) -> Node| {
            Node::row(
                "bar",
                [
                    shape(Node::row("l", [leaf("li", 50., 20.)])),
                    leaf("m", 30., 20.),
                    shape(Node::row("r", [leaf("ri", 20., 20.)]).justify(Justify::End)),
                ],
            )
        };
        assert_eq!(bar(slots(|n| n).justify(Justify::SpaceBetween)), 215.);
        assert_eq!(bar(slots(|n| n.grow(1.))), 215.);
        assert_eq!(bar(slots(|n| n.flex(1.))), 200.);

        // ...and the ends still sit against the edges they belong to.
        let l = resolve(
            &slots(|n| n.flex(1.)),
            Some(Size::new(400., 20.)),
            Default::default(),
        )
        .unwrap();
        assert_eq!(l.frame("li").unwrap().x, 0.);
        assert_eq!(l.frame("ri").unwrap().right(), 400.);

        // Hugging, the row is sized by the flex fraction: wide enough that the
        // hungriest flexible child's *share* still clears its content. The left
        // slot needs 50, so at one unit of grow each both slots are 50 and the
        // row is 130 -- not the 100 that summing the children would give, which
        // would have squashed that slot to 35.
        let hug = resolve(&slots(|n| n.flex(1.)), None, Default::default()).unwrap();
        assert_eq!(hug.size.width, 130.);
        assert_eq!(hug.frame("li").unwrap().size.width, 50.);
        assert_eq!(hug.frame("ri").unwrap().size.width, 20.);
        // ...so the middle child is centred at its hugging size too.
        let m = hug.frame("m").unwrap();
        assert_eq!(m.x + m.size.width / 2., 65.);
    }

    #[test]
    fn a_deficit_comes_back_by_shrink_and_stops_at_each_minimum() {
        let row = |a: Node, b: Node| {
            let l = resolve(
                &Node::row("r", [a, b]),
                Some(Size::new(150., 20.)),
                Default::default(),
            )
            .unwrap();
            (
                l.frame("a").unwrap().size.width,
                l.frame("b").unwrap().size.width,
            )
        };
        // Equal basis, equal shrink: the 50 px deficit splits evenly.
        assert_eq!(row(leaf("a", 100., 20.), leaf("b", 100., 20.)), (75., 75.));
        // `a` freezes at its minimum after giving up 10, and `b` absorbs the rest.
        assert_eq!(
            row(
                leaf("a", 100., 20.).min_size(Size::new(90., 0.)),
                leaf("b", 100., 20.)
            ),
            (90., 60.)
        );
        // `shrink(0.0)` opts out entirely.
        assert_eq!(
            row(leaf("a", 100., 20.).shrink(0.), leaf("b", 100., 20.)),
            (100., 50.)
        );
    }

    /// A node's floor is not just its own `min_size`. Without the children's
    /// minimums summing upward, a parent is shrunk to a width its own contents
    /// then overflow, and nothing anywhere reports it.
    #[test]
    fn a_parent_cannot_be_squeezed_past_what_its_children_refuse() {
        let root = Node::row(
            "out",
            [Node::row(
                "in",
                [
                    leaf("a", 100., 20.).min_size(Size::new(40., 0.)),
                    leaf("b", 100., 20.).min_size(Size::new(30., 0.)),
                ],
            )],
        );
        // 70 is exactly the two floors, and both children land on theirs.
        let l = resolve(&root, Some(Size::new(70., 20.)), Default::default()).unwrap();
        assert_eq!(l.frame("a").unwrap().size.width, 40.);
        assert_eq!(l.frame("b").unwrap().size.width, 30.);
        // A pixel under, and it is refused rather than silently overflowing.
        assert!(matches!(
            resolve(&root, Some(Size::new(69., 20.)), Default::default()),
            Err(Error::InsufficientSpace(_))
        ));

        // The floor also has to bind while the deficit is being shared out, not
        // only as a check afterwards. Given a squeezable sibling, `in` freezes
        // at 70 and the rest of the deficit goes to `c` -- an even split would
        // have put `in` at 50, under a floor it never declared itself.
        let pair = Node::row("pair", [root, leaf("c", 200., 20.)]);
        let l = resolve(&pair, Some(Size::new(100., 20.)), Default::default()).unwrap();
        assert_eq!(l.frame("out").unwrap().size.width, 70.);
        assert_eq!(l.frame("c").unwrap().size.width, 30.);
    }

    #[test]
    fn align_self_overrides_the_parent_for_one_child() {
        let l = resolve(
            &Node::column(
                "c",
                [
                    leaf("a", 20., 10.),
                    leaf("b", 20., 10.).align_self(Align::End),
                ],
            )
            .align(Align::Start),
            Some(Size::new(100., 20.)),
            Default::default(),
        )
        .unwrap();
        assert_eq!(l.frame("a").unwrap().x, 0.);
        assert_eq!(l.frame("b").unwrap().x, 80.);
    }
}
