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
    align: Align,
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
            align: Align::Center,
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
    children: Vec<Measured<'a>>,
}

fn validate_node(node: &Node, l: Limits) -> Result<(), Error> {
    if node.key.is_empty()
        || !node.minimum.valid(l.extent)
        || node.maximum.is_some_and(|s| !s.valid(l.extent))
        || !node.padding.valid(l.extent)
        || ![node.gap, node.grow]
            .iter()
            .all(|v| v.is_finite() && *v >= 0.0 && *v <= l.extent)
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
    let content = match &node.kind {
        Kind::Leaf(s) => {
            if !s.valid(l.extent) {
                return Err(Error::InvalidValue);
            }
            *s
        }
        Kind::Stack {
            vertical,
            children: source,
        } => {
            for child in source {
                children.push(measure(child, depth + 1, left, l, keys)?);
            }
            let main = children.iter().map(|c| c.size.main(*vertical)).sum::<f64>()
                + source.len().saturating_sub(1) as f64 * node.gap;
            let cross = children
                .iter()
                .map(|c| c.size.cross(*vertical))
                .fold(0.0, f64::max);
            Size::axes(main, cross, *vertical)
        }
        Kind::Overlay { children: source } => {
            for child in source {
                children.push(measure(child, depth + 1, left, l, keys)?);
            }
            Size::new(
                children.iter().map(|c| c.size.width).fold(0.0, f64::max),
                children.iter().map(|c| c.size.height).fold(0.0, f64::max),
            )
        }
    };
    let size = Size::new(
        (content.width + node.padding.horizontal()).max(node.minimum.width),
        (content.height + node.padding.vertical()).max(node.minimum.height),
    );
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
        children,
    })
}

fn distribute_growth(m: &Measured<'_>, vertical: bool, inner_main: f64, base_gap: f64) -> Vec<f64> {
    let mut allocated: Vec<f64> = m.children.iter().map(|c| c.size.main(vertical)).collect();
    let used = allocated.iter().sum::<f64>() + base_gap * m.children.len().saturating_sub(1) as f64;
    let mut free = (inner_main - used).max(0.0);
    for _ in 0..=m.children.len() {
        let active: Vec<usize> = m
            .children
            .iter()
            .enumerate()
            .filter(|(i, c)| {
                c.node.grow > 0.0
                    && allocated[*i] + 1e-8 < c.node.maximum.map_or(1e6, |s| s.main(vertical))
            })
            .map(|(i, _)| i)
            .collect();
        let total = active.iter().map(|i| m.children[*i].node.grow).sum::<f64>();
        if free < 1e-8 || total == 0.0 {
            break;
        }
        let budget = free;
        for i in active {
            let c = &m.children[i];
            let cap = c.node.maximum.map_or(1e6, |s| s.main(vertical));
            let extra = (budget * c.node.grow / total).min(cap - allocated[i]);
            allocated[i] += extra;
            free -= extra;
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
    if size.width + 1e-8 < m.size.width || size.height + 1e-8 < m.size.height {
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
                size.width - n.padding.horizontal(),
                size.height - n.padding.vertical(),
            );
            for c in &m.children {
                let max = c.node.maximum.unwrap_or(Size::new(1e6, 1e6));
                let w = if n.align == Align::Stretch {
                    inner.width.min(max.width)
                } else {
                    c.size.width
                };
                let h = if n.align == Align::Stretch {
                    inner.height.min(max.height)
                } else {
                    c.size.height
                };
                let x = n.padding.left
                    + match n.align {
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
                size.width - n.padding.horizontal(),
                size.height - n.padding.vertical(),
            );
            let allocated = distribute_growth(m, *vertical, inner.main(*vertical), n.gap);
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
                let cross = if n.align == Align::Stretch {
                    available.min(c.node.maximum.map_or(available, |s| s.cross(*vertical)))
                } else {
                    natural
                };
                let slack = (available - cross).max(0.0);
                let cross_pos = n.padding.cross_start(*vertical)
                    + match n.align {
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
    fn insufficient_space() {
        assert!(matches!(
            resolve(
                &leaf("x", 100., 100.),
                Some(Size::new(50., 50.)),
                Default::default()
            ),
            Err(Error::InsufficientSpace(_))
        ));
    }
}
