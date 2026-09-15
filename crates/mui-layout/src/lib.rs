//! Small, renderer-independent intrinsic layout for plugin UIs.
//!
//! Not CSS, but it reads like the good half of it. Containers hug content by
//! default, a parent may offer an exact size, and surplus goes out by `grow`
//! and comes back by `shrink`. Alignment is automatic: containers stretch to
//! fill their cross axis, content centres in it, and nothing is ever placed by
//! coordinate -- an `offset` on an overlay child is the only nudge there is.
//!
//! A node carries a payload `P` so a styling layer can ride the same tree
//! instead of mirroring it by id.
#![forbid(unsafe_code)]

use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Size {
    pub width: f64,
    pub height: f64,
}
impl Size {
    pub const ZERO: Self = Self::new(0.0, 0.0);
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
    pub const ZERO: Self = Self::all(0.0);
    pub const fn all(v: f64) -> Self {
        Self::symmetric(v, v)
    }
    pub const fn symmetric(horizontal: f64, vertical: f64) -> Self {
        Self {
            left: horizontal,
            right: horizontal,
            top: vertical,
            bottom: vertical,
        }
    }
    pub fn horizontal(self) -> f64 {
        self.left + self.right
    }
    pub fn vertical(self) -> f64 {
        self.top + self.bottom
    }
    fn valid(self, limit: f64) -> bool {
        [self.left, self.right, self.top, self.bottom]
            .iter()
            .all(|v| v.is_finite() && *v >= 0.0 && *v <= limit)
    }
}

/// A step on the theme's spacing scale. `.gap(M)` reads like the CSS it
/// replaces and re-tunes with the theme instead of with a search-and-replace.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpacingToken {
    Xs,
    S,
    M,
    L,
    Xl,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpacingScale {
    pub xs: f64,
    pub s: f64,
    pub m: f64,
    pub l: f64,
    pub xl: f64,
}
impl SpacingScale {
    pub const DEFAULT: Self = Self {
        xs: 4.0,
        s: 8.0,
        m: 12.0,
        l: 18.0,
        xl: 28.0,
    };
    pub fn get(self, t: SpacingToken) -> f64 {
        match t {
            SpacingToken::Xs => self.xs,
            SpacingToken::S => self.s,
            SpacingToken::M => self.m,
            SpacingToken::L => self.l,
            SpacingToken::Xl => self.xl,
        }
    }
    pub fn is_valid(self) -> bool {
        [self.xs, self.s, self.m, self.l, self.xl]
            .iter()
            .all(|v| v.is_finite() && *v >= 0.0)
    }
}
impl Default for SpacingScale {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// A gap or padding: pixels, or a token resolved against the scale handed to
/// [`resolve_with`]. Plain `f64` converts, so `.gap(10.0)` still works.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Spacing {
    Px(f64),
    Token(SpacingToken),
}
impl Spacing {
    pub const fn px(v: f64) -> Self {
        Self::Px(v)
    }
    pub fn resolve(self, scale: SpacingScale) -> f64 {
        match self {
            Self::Px(v) => v,
            Self::Token(t) => scale.get(t),
        }
    }
}
impl From<f64> for Spacing {
    fn from(v: f64) -> Self {
        Self::Px(v)
    }
}
impl From<SpacingToken> for Spacing {
    fn from(t: SpacingToken) -> Self {
        Self::Token(t)
    }
}

/// Cross-axis placement. `Stretch` is the default and means "fill if you are a
/// container, centre if you are content": a column of rows fills its width,
/// a column of labels lines them up down the middle, and neither needs saying.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Align {
    Start,
    Center,
    End,
    #[default]
    Stretch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Justify {
    #[default]
    Start,
    Center,
    End,
    SpaceBetween,
    SpaceAround,
    SpaceEvenly,
}
impl Justify {
    fn as_align(self) -> Align {
        match self {
            Self::Start => Align::Start,
            Self::End => Align::End,
            _ => Align::Center,
        }
    }
}

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
    fn px(self) -> Option<f64> {
        match self {
            Self::Px(v) => Some(v),
            _ => None,
        }
    }
    fn fixed(self, parent: f64) -> Option<f64> {
        match self {
            Self::Auto => None,
            Self::Px(v) => Some(v),
            Self::Pct(p) => Some(parent * p / 100.0),
        }
    }
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
    /// Children may overflow the main axis; the frame clips them and
    /// `scrolled` slides them. Implies `clip`.
    scroll: bool,
    clip: bool,
    scrolled: [f64; 2],
    /// Out of flow: takes no space in its parent and sits like an overlay
    /// child, anchored and offset within the parent's padding box. Tooltips,
    /// popups, drag ghosts.
    float: bool,
    /// Break the flow into lines when the children no longer fit the main
    /// axis. Branches only.
    wrap: bool,
    /// Grid cells only: how many columns this cell occupies.
    span: usize,
    /// Placement order among siblings; ties keep declaration order. Frames
    /// stay in declaration order regardless.
    order: i32,
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
            scroll: false,
            clip: false,
            scrolled: [0.0; 2],
            float: false,
            wrap: false,
            span: 1,
            order: 0,
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
    /// Let the children overflow the main axis behind a clip. The node's
    /// floor on that axis drops to its padding, so it can be squeezed.
    pub fn scroll(mut self) -> Self {
        self.scroll = true;
        self.clip = true;
        self
    }
    /// Clip the children to this node's outline.
    pub fn clip(mut self) -> Self {
        self.clip = true;
        self
    }
    /// How far the children are slid, in pixels; the runtime sets this.
    pub fn scrolled(mut self, x: f64, y: f64) -> Self {
        self.scrolled = [x, y];
        self
    }
    /// Take this node out of flow: see the `float` field.
    pub fn float(mut self) -> Self {
        self.float = true;
        self
    }
    /// Let a row or column break into lines when its children overflow the
    /// main axis. Lines stack on the cross axis with the same `gap`.
    pub fn wrap(mut self) -> Self {
        self.wrap = true;
        self
    }
    /// How many grid columns this cell takes, clamped to the column count.
    pub fn span(mut self, cols: usize) -> Self {
        self.span = cols.max(1);
        self
    }
    /// Place this child as if it were declared at `order`; its frame keeps
    /// its declaration slot, so a tree walk still lines up.
    pub fn order(mut self, order: i32) -> Self {
        self.order = order;
        self
    }
    /// Append a child to a container. A leaf or content node has no children
    /// and is returned unchanged.
    pub fn push(mut self, child: Self) -> Self {
        match &mut self.kind {
            Kind::Branch { children, .. }
            | Kind::Overlay(children)
            | Kind::Grid { children, .. } => children.push(child),
            Kind::Leaf | Kind::Content => {}
        }
        self
    }
    pub fn is_scroll(&self) -> bool {
        self.scroll
    }
    pub fn is_clip(&self) -> bool {
        self.clip
    }
    pub fn is_float(&self) -> bool {
        self.float
    }
    pub fn scroll_offset(&self) -> [f64; 2] {
        self.scrolled
    }
    /// The main axis of a branch, if any: `true` for a column.
    pub fn vertical(&self) -> Option<bool> {
        match self.kind {
            Kind::Branch { vertical, .. } => Some(vertical),
            _ => None,
        }
    }
    fn len(&self, vertical: bool) -> Len {
        if vertical {
            self.height
        } else {
            self.width
        }
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
    pub fn center(self) -> (f64, f64) {
        (
            self.x + self.size.width / 2.0,
            self.y + self.size.height / 2.0,
        )
    }
    pub fn contains(self, x: f64, y: f64) -> bool {
        x >= self.x && y >= self.y && x <= self.right() && y <= self.bottom()
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
    /// Every node's frame, in tree order (parent first, then children in
    /// declaration order). A walk of the same tree indexes straight into it,
    /// so nothing needs a name to be found.
    order: Vec<Frame>,
}
impl Layout {
    pub fn frame(&self, key: &str) -> Option<Frame> {
        self.frames.get(key).copied()
    }
    pub fn all(&self) -> &[Frame] {
        &self.order
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
        match self {
            Self::InvalidValue => {
                f.write_str("a size, spacing or limit is negative, not finite or out of range")
            }
            Self::DuplicateKey(k) => write!(f, "two nodes share the id {k}"),
            Self::BudgetExceeded => f.write_str("the tree exceeds its node or depth limit"),
            Self::InsufficientSpace(k) => write!(f, "node {k} does not fit in the space offered"),
            Self::RevisionExhausted => f.write_str("the layout revision counter overflowed"),
        }
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

struct Measured<'a, P> {
    node: &'a Node<P>,
    /// Slot among the parent's children, so a reordered placement can be
    /// written back to the declaration order the frames keep.
    index: usize,
    gap: f64,
    padding: Insets,
    size: Size,
    /// The smallest this subtree may be squeezed to: every minimum in it,
    /// summed along the axis they sit on.
    floor: Size,
    /// The children's extent inside the padding, before any definite size
    /// overrides it: what a scroll node lays its children into.
    content: Size,
    children: Vec<Measured<'a, P>>,
}

/// The in-flow children -- everything but the floats -- in placement order.
/// The sort is stable, so `order` only moves what asked to be moved.
fn flow_of<'a, 'm, P>(children: &'m [Measured<'a, P>]) -> Vec<&'m Measured<'a, P>> {
    let mut flow: Vec<_> = children.iter().filter(|c| !c.node.float).collect();
    flow.sort_by_key(|c| c.node.order);
    flow
}

/// Greedy line breaking: ranges into `flow`, each as long as it fits `avail`.
/// A child wider than the line gets a line of its own.
fn wrap_lines<P>(
    flow: &[&Measured<'_, P>],
    gap: f64,
    vertical: bool,
    avail: f64,
) -> Vec<(usize, usize)> {
    let mut lines = Vec::new();
    let (mut start, mut used) = (0, 0.0);
    for (i, c) in flow.iter().enumerate() {
        let w = c.base(vertical, None);
        if i > start && used + gap + w > avail + 1e-9 {
            lines.push((start, i));
            (start, used) = (i, w);
        } else {
            used += if i > start { gap + w } else { w };
        }
    }
    lines.push((start, flow.len()));
    lines
}

impl<'a, P> Measured<'a, P> {
    fn flow(&self) -> Vec<&Measured<'a, P>> {
        flow_of(&self.children)
    }
    /// Where this child starts before growth or shrink: a percentage of the
    /// parent, a height derived from its aspect, its declared `basis`, or what
    /// it measured. `inner` is `None` while the parent is still hugging.
    fn base(&self, vertical: bool, inner: Option<Size>) -> f64 {
        let fallback = || self.node.basis.unwrap_or(self.size.main(vertical));
        let Some(inner) = inner else {
            return fallback();
        };
        match (self.node.len(vertical), self.aspect_width(inner)) {
            (Len::Pct(p), _) => inner.main(vertical) * p / 100.0,
            (_, Some((w, a))) if vertical => w / a,
            _ => fallback(),
        }
    }
    /// Aspect is width-first, like CSS: when the height is `Auto`, the width
    /// is whatever the node would have -- fixed, a share, or the parent's inner
    /// width -- and the height follows.
    fn aspect_width(&self, inner: Size) -> Option<(f64, f64)> {
        let n = self.node;
        let a = n.aspect.filter(|_| matches!(n.height, Len::Auto))?;
        let cap = |v: f64| n.maximum.map_or(v, |m| v.min(m.width));
        Some((cap(n.width.fixed(inner.width).unwrap_or(inner.width)), a))
    }
    /// Size on one axis inside `avail`, given how it is aligned there.
    fn extent(&self, vertical: bool, avail: f64, align: Align) -> f64 {
        let n = self.node;
        // A share of the parent is still never less than this subtree's floor:
        // `height: 50%` on a padded node is a squeeze, not an error.
        let cap = |v: f64| {
            n.maximum
                .map_or(v, |m| v.min(m.cross(!vertical)))
                .max(self.floor.main(vertical))
        };
        match n.len(vertical).fixed(avail) {
            Some(v) => cap(v),
            None if align == Align::Stretch && n.is_container() => cap(avail),
            // Content never keeps a cross extent wider than the room it was
            // given: it would be centred half outside its own parent.
            None => cap(self.size.main(vertical).min(avail)),
        }
    }
}

/// In a cell, `align` governs both axes until `justify` is actually set.
fn cell_default<P>(n: &Node<P>) -> (Align, Align) {
    let y = if n.justify == Justify::Start {
        n.align
    } else {
        n.justify.as_align()
    };
    (n.align, y)
}

fn place(avail: f64, extent: f64, align: Align) -> f64 {
    match align {
        Align::Start => 0.0,
        Align::End => avail - extent,
        _ => (avail - extent) * 0.5,
    }
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

/// What to call a node in an error. Unnamed nodes are structural, so the
/// nearest named ancestor is the useful thing to point at.
fn label<P>(node: &Node<P>, ancestor: &str) -> String {
    node.id
        .clone()
        .unwrap_or_else(|| format!("{ancestor} > unnamed"))
}

/// Rows of a grid, splitting where the spans fill the column count. A cell
/// wider than the grid takes a row alone.
fn grid_rows<'a, 'm, P>(
    children: &'m [&'m Measured<'a, P>],
    cols: usize,
) -> Vec<&'m [&'m Measured<'a, P>]> {
    let mut rows = Vec::new();
    let (mut start, mut used) = (0, 0);
    for (i, c) in children.iter().enumerate() {
        let span = c.node.span.clamp(1, cols.max(1));
        if i > start && used + span > cols.max(1) {
            rows.push(&children[start..i]);
            (start, used) = (i, span);
        } else {
            used += span;
        }
    }
    if start < children.len() {
        rows.push(&children[start..]);
    }
    rows
}

/// What a parent can promise a child about its outer size before layout runs:
/// a fixed length, a share of a definite inner extent, or -- for a container
/// (or an aspect-ratio node) that will be stretched -- that extent itself.
/// `None` is "measure yourself".
fn offer<P>(c: &Node<P>, vertical: bool, inner: Option<f64>, stretch: bool) -> Option<f64> {
    match c.len(vertical) {
        Len::Px(v) => Some(v),
        Len::Pct(p) => inner.map(|i| i * p / 100.0),
        Len::Auto => inner.filter(|_| stretch && (c.is_container() || c.aspect.is_some())),
    }
}

/// One measure pass: the budget, the id set and the content measurer.
struct Pass<'a, 'f, P> {
    left: usize,
    limits: Limits,
    scale: SpacingScale,
    keys: BTreeMap<&'a str, ()>,
    measurer: &'f mut dyn FnMut(&P, Option<f64>) -> Size,
}

/// `room` is the narrowest definite inner width above this node (a grid
/// column counts; a scroll node stops it), handed to content so a paragraph
/// can wrap in the first and only pass. It is the room, not the flex share:
/// a row still squeezes its content after measuring.
fn measure<'a, P>(
    node: &'a Node<P>,
    ancestor: &str,
    definite: [Option<f64>; 2],
    room: Option<f64>,
    depth: usize,
    pass: &mut Pass<'a, '_, P>,
) -> Result<Measured<'a, P>, Error> {
    let l = pass.limits;
    if depth > l.depth || pass.left == 0 {
        return Err(Error::BudgetExceeded);
    }
    pass.left -= 1;
    validate_node(node, l)?;
    let (gap, padding) = (node.gap.resolve(pass.scale), node.padding(pass.scale));
    if !(gap.is_finite() && (0.0..=l.extent).contains(&gap) && padding.valid(l.extent)) {
        return Err(Error::InvalidValue);
    }
    if let Some(id) = node.id.as_deref() {
        if pass.keys.insert(id, ()).is_some() {
            return Err(Error::DuplicateKey(id.to_string()));
        }
    }
    let here = node.id.as_deref().unwrap_or(ancestor);
    // Aspect is width-first, like CSS: a definite width settles the height,
    // and only a definite height with no width settles the width.
    let mut definite = [
        node.width.px().or(definite[0]),
        node.height.px().or(definite[1]),
    ];
    if let Some(a) = node.aspect {
        match (definite, node.height, node.width) {
            ([Some(w), _], Len::Auto, _) => definite[1] = Some(w / a),
            ([None, Some(h)], _, Len::Auto) => definite[0] = Some(h * a),
            _ => {}
        }
    }
    let inner = [
        definite[0].map(|w| (w - padding.horizontal()).max(0.0)),
        definite[1].map(|h| (h - padding.vertical()).max(0.0)),
    ];
    let room = [room.map(|r| (r - padding.horizontal()).max(0.0)), inner[0]]
        .into_iter()
        .flatten()
        .reduce(f64::min);
    let mut children = Vec::with_capacity(node.children().len());
    for (index, c) in node.children().iter().enumerate() {
        let align = c.align_self.unwrap_or(node.align);
        let mut child_room = room.filter(|_| !node.scroll);
        let promise = match &node.kind {
            _ if c.float => {
                let (ax, ay) = c.anchor.unwrap_or(cell_default(node));
                [
                    offer(c, false, inner[0], ax == Align::Stretch),
                    offer(c, true, inner[1], ay == Align::Stretch),
                ]
            }
            Kind::Branch { vertical, .. } => {
                let v = *vertical;
                let cross = offer(c, !v, inner[!v as usize], align == Align::Stretch);
                let main = offer(c, v, inner[v as usize], false);
                if v {
                    [cross, main]
                } else {
                    [main, cross]
                }
            }
            Kind::Overlay(_) => {
                let (ax, ay) = c.anchor.unwrap_or(cell_default(node));
                [
                    offer(c, false, inner[0], ax == Align::Stretch),
                    offer(c, true, inner[1], ay == Align::Stretch),
                ]
            }
            Kind::Grid { cols, .. } => {
                let (ax, _) = c.anchor.unwrap_or(cell_default(node));
                let span = c.span.clamp(1, *cols) as f64;
                let col = inner[0].map(|w| {
                    ((w - gap * (*cols - 1) as f64).max(0.0) / *cols as f64) * span
                        + gap * (span - 1.0)
                });
                child_room = [child_room, col].into_iter().flatten().reduce(f64::min);
                [offer(c, false, col, ax == Align::Stretch), None]
            }
            _ => [None; 2],
        };
        let mut m = measure(c, here, promise, child_room, depth + 1, pass)?;
        m.index = index;
        children.push(m);
    }
    let flow = flow_of(&children);
    let max_of =
        |g: &dyn Fn(&Measured<'_, P>) -> f64| flow.iter().map(|c| g(c)).fold(0.0, f64::max);
    let (content, sunk) = match &node.kind {
        Kind::Leaf => (Size::ZERO, Size::ZERO),
        Kind::Content => {
            let s = (pass.measurer)(&node.payload, room);
            if !s.valid(l.extent) {
                return Err(Error::InvalidValue);
            }
            (s, Size::ZERO)
        }
        Kind::Branch { vertical: v, .. } => {
            let v = *v;
            let gaps = flow.len().saturating_sub(1) as f64 * gap;
            // Intrinsic main is not the sum of the children: a child with a
            // `basis` contributes that instead, and then the row has to be
            // wide enough that its *share* of the surplus still clears its
            // content. This is the flex fraction. Without it a hugging row
            // collapses to its non-flexible children and squashes the rest.
            let base = flow.iter().map(|c| c.base(v, None)).sum::<f64>();
            let total_grow = flow.iter().map(|c| c.node.grow).sum::<f64>();
            let surplus = flow
                .iter()
                .filter(|c| c.node.grow > 0.0)
                .map(|c| (c.size.main(v) - c.base(v, None)) * total_grow / c.node.grow)
                .fold(0.0, f64::max);
            let floor_main = flow.iter().map(|c| c.floor.main(v)).sum::<f64>() + gaps;
            // A wrapping row measured under an offered main axis is as tall as
            // its lines. With nothing offered there is nothing to break
            // against, so it stays one line.
            // ponytail: the cross floor stays the single-line one, so a squeeze
            // past the measured width overflows instead of erroring.
            if let Some(avail) = node.wrap.then(|| inner[v as usize]).flatten() {
                let lines = wrap_lines(&flow, gap, v, avail);
                let cross = lines
                    .iter()
                    .map(|(a, b)| {
                        flow[*a..*b]
                            .iter()
                            .map(|c| c.size.cross(v))
                            .fold(0.0, f64::max)
                    })
                    .sum::<f64>()
                    + gap * lines.len().saturating_sub(1) as f64;
                let sunk_main = if node.scroll {
                    0.0
                } else {
                    max_of(&|c| c.floor.main(v))
                };
                (
                    Size::axes(avail, cross, v),
                    Size::axes(sunk_main, max_of(&|c| c.floor.cross(v)), v),
                )
            } else {
                (
                    Size::axes(base + surplus + gaps, max_of(&|c| c.size.cross(v)), v),
                    // A scroll node can always be squeezed on its main axis.
                    Size::axes(
                        if node.scroll { 0.0 } else { floor_main },
                        max_of(&|c| c.floor.cross(v)),
                        v,
                    ),
                )
            }
        }
        Kind::Overlay(_) => (
            Size::new(max_of(&|c| c.size.width), max_of(&|c| c.size.height)),
            Size::new(max_of(&|c| c.floor.width), max_of(&|c| c.floor.height)),
        ),
        Kind::Grid { cols, .. } => {
            let rows = grid_rows(&flow, *cols);
            let gaps = |n: f64| (n - 1.0).max(0.0) * gap;
            let hug = |g: fn(&Measured<'_, P>) -> Size| {
                // A spanning cell pays for its span, so its share of one
                // column is what sets the column width.
                let widest = flow
                    .iter()
                    .map(|c| g(c).width / c.node.span.clamp(1, *cols) as f64)
                    .fold(0.0, f64::max);
                let tall: f64 = rows
                    .iter()
                    .map(|r| r.iter().map(|c| g(c).height).fold(0.0, f64::max))
                    .sum();
                Size::new(
                    widest * *cols as f64 + gaps(*cols as f64),
                    tall + gaps(rows.len() as f64),
                )
            };
            (hug(|c| c.size), hug(|c| c.floor))
        }
    };
    let pad = |s: Size| {
        Size::new(
            (s.width + padding.horizontal()).max(node.minimum.width),
            (s.height + padding.vertical()).max(node.minimum.height),
        )
    };
    let hug = pad(content);
    let size = Size::new(
        definite[0].unwrap_or(hug.width),
        definite[1].unwrap_or(hug.height),
    );
    let floor = match node.vertical() {
        Some(v) if node.scroll => Size::axes(
            if v {
                padding.vertical()
            } else {
                padding.horizontal()
            }
            .max(node.minimum.main(v)),
            pad(sunk).cross(v),
            v,
        ),
        _ => pad(sunk),
    };
    // A size the node declares itself is also its floor: `.size(10., 10.).pad(6.)`
    // is a 10x10 box with no room inside, not a layout error.
    let cap = |f: f64, l: Len| l.px().map_or(f, |v| f.min(v));
    let floor = Size::new(cap(floor.width, node.width), cap(floor.height, node.height));
    if !size.valid(l.extent) {
        return Err(Error::BudgetExceeded);
    }
    if let Some(max) = node.maximum {
        if size.width > max.width + 1e-9 || size.height > max.height + 1e-9 {
            return Err(Error::InsufficientSpace(label(node, ancestor)));
        }
    }
    Ok(Measured {
        node,
        index: 0,
        gap,
        padding,
        size,
        floor,
        content,
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
fn distribute<P>(children: &[&Measured<'_, P>], gap: f64, vertical: bool, inner: Size) -> Vec<f64> {
    let inner_main = inner.main(vertical);
    let base: Vec<f64> = children
        .iter()
        .map(|c| c.base(vertical, Some(inner)))
        .collect();
    let mut allocated = base.clone();
    let gaps = gap * children.len().saturating_sub(1) as f64;
    let mut free = inner_main - base.iter().sum::<f64>() - gaps;
    let growing = free > 0.0;
    let room = |i: usize, allocated: &[f64]| {
        let c = children[i];
        let edge = if growing {
            c.node.maximum.map_or(f64::INFINITY, |s| s.main(vertical)) - allocated[i]
        } else {
            allocated[i] - c.floor.main(vertical)
        };
        edge.max(0.0)
    };
    let weight = |i: usize| {
        let c = children[i];
        if growing {
            c.node.grow
        } else {
            c.node.shrink * base[i]
        }
    };
    for _ in 0..=children.len() {
        let active: Vec<usize> = (0..children.len())
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

/// A child in a rect of its own: an overlay layer or a grid cell. Returns the
/// child's origin within the cell and its size.
fn cell<P>(c: &Measured<'_, P>, cell: Size, default: (Align, Align)) -> ([f64; 2], Size) {
    let n = c.node;
    let (ax, ay) = n.anchor.unwrap_or(default);
    let (w, h) = match c.aspect_width(cell) {
        Some((w, a)) => (w, w / a),
        None => (
            c.extent(false, cell.width, ax),
            c.extent(true, cell.height, ay),
        ),
    };
    (
        [
            place(cell.width, w, ax) + n.offset[0],
            place(cell.height, h, ay) + n.offset[1],
        ],
        Size::new(w, h),
    )
}

fn arrange<P>(
    m: &Measured<'_, P>,
    ancestor: &str,
    origin: [f64; 2],
    size: Size,
    out: &mut (BTreeMap<String, Frame>, Vec<Frame>),
) -> Result<(), Error> {
    let n = m.node;
    // Content is squeezable -- that is the whole point of shrink -- but the
    // floor is not.
    if size.width + 1e-8 < m.floor.width || size.height + 1e-8 < m.floor.height {
        return Err(Error::InsufficientSpace(label(n, ancestor)));
    }
    let here = n.id.as_deref().unwrap_or(ancestor);
    let frame = Frame {
        x: origin[0],
        y: origin[1],
        size,
    };
    out.1.push(frame);
    if let Some(id) = n.id.clone() {
        out.0.insert(id, frame);
    }
    let inner = Size::new(
        (size.width - m.padding.horizontal()).max(0.0),
        (size.height - m.padding.vertical()).max(0.0),
    );
    let at = |x: f64, y: f64| {
        [
            origin[0] + m.padding.left + x - n.scrolled[0],
            origin[1] + m.padding.top + y - n.scrolled[1],
        ]
    };
    let default = cell_default(n);
    let flow = m.flow();
    // Where each in-flow child goes, then every child in declaration order
    // so frames stay in tree order; a float sits in the padding box like an
    // overlay child, unscrolled.
    let mut placed: Vec<Option<([f64; 2], Size)>> = vec![None; m.children.len()];
    match &n.kind {
        Kind::Leaf | Kind::Content => {}
        Kind::Overlay(_) => {
            for c in &flow {
                let (p, s) = cell(c, inner, default);
                placed[c.index] = Some((at(p[0], p[1]), s));
            }
        }
        Kind::Grid { cols, .. } => {
            let cols = *cols;
            let grid = grid_rows(&flow, cols);
            let col_w =
                (inner.width - m.gap * cols.saturating_sub(1) as f64).max(0.0) / cols as f64;
            let heights: Vec<f64> = grid
                .iter()
                .map(|r| r.iter().map(|c| c.size.height).fold(0.0, f64::max))
                .collect();
            let surplus = (inner.height
                - heights.iter().sum::<f64>()
                - m.gap * grid.len().saturating_sub(1) as f64)
                .max(0.0)
                / grid.len().max(1) as f64;
            let mut y = 0.0;
            for (row, h) in grid.iter().zip(heights) {
                let mut col = 0;
                for c in row.iter() {
                    let span = c.node.span.clamp(1, cols.max(1));
                    let cell_size =
                        Size::new(col_w * span as f64 + m.gap * (span - 1) as f64, h + surplus);
                    let (p, s) = cell(c, cell_size, default);
                    // A cell wider than its column overhangs the far edge,
                    // like CSS safe alignment: the start stays visible.
                    let safe = |v: f64, o: f64| (v - o).max(0.0) + o;
                    placed[c.index] = Some((
                        at(
                            col as f64 * (col_w + m.gap) + safe(p[0], c.node.offset[0]),
                            y + safe(p[1], c.node.offset[1]),
                        ),
                        s,
                    ));
                    col += span;
                }
                y += h + surplus + m.gap;
            }
        }
        Kind::Branch { vertical, .. } => {
            let v = *vertical;
            // A scroll node lays its children into their own extent when
            // that is larger than the frame: nothing shrinks, it overflows.
            let inner = if n.scroll {
                Size::axes(inner.main(v).max(m.content.main(v)), inner.cross(v), v)
            } else {
                inner
            };
            let lines = if n.wrap {
                wrap_lines(&flow, m.gap, v, inner.main(v))
            } else {
                vec![(0, flow.len())]
            };
            // One line keeps the whole cross axis, so an unwrapped row is
            // exactly what it was; several share it by their own heights.
            let single = lines.len() == 1;
            let mut line_start = 0.0;
            for (a, b) in lines {
                let line = &flow[a..b];
                let line_cross = if single {
                    inner.cross(v)
                } else {
                    line.iter().map(|c| c.size.cross(v)).fold(0.0, f64::max)
                };
                let line_inner = Size::axes(inner.main(v), line_cross, v);
                let allocated = distribute(line, m.gap, v, line_inner);
                let count = line.len() as f64;
                let residual = (inner.main(v)
                    - allocated.iter().sum::<f64>()
                    - m.gap * (count - 1.0).max(0.0))
                .max(0.0);
                let (mut cursor, extra) = match n.justify {
                    Justify::Start => (0.0, 0.0),
                    Justify::Center => (residual * 0.5, 0.0),
                    Justify::End => (residual, 0.0),
                    Justify::SpaceBetween if count > 1.0 => (0.0, residual / (count - 1.0)),
                    Justify::SpaceBetween => (residual * 0.5, 0.0),
                    Justify::SpaceAround => (residual / count * 0.5, residual / count),
                    Justify::SpaceEvenly => (residual / (count + 1.0), residual / (count + 1.0)),
                };
                for (c, main) in line.iter().zip(allocated) {
                    let align = c.node.align_self.unwrap_or(n.align);
                    let cross = match c.aspect_width(line_inner) {
                        Some((w, _)) if v => w,
                        Some((_, a)) => main / a,
                        None => c.extent(!v, line_cross, align),
                    };
                    let cross_pos = line_start + place(line_cross, cross, align);
                    let pos = if v {
                        at(cross_pos, cursor)
                    } else {
                        at(cursor, cross_pos)
                    };
                    placed[c.index] = Some((pos, Size::axes(main, cross, v)));
                    cursor += main + m.gap + extra;
                }
                line_start += line_cross + m.gap;
            }
        }
    }
    for (i, c) in m.children.iter().enumerate() {
        let (pos, s) = if c.node.float {
            let (p, s) = cell(c, inner, default);
            (
                [
                    origin[0] + m.padding.left + p[0],
                    origin[1] + m.padding.top + p[1],
                ],
                s,
            )
        } else {
            placed[i].take().ok_or(Error::BudgetExceeded)?
        };
        arrange(c, here, pos, s, out)?;
    }
    Ok(())
}

/// `None` means hug intrinsic content. `Some` is an exact offered parent size.
/// Content leaves measure as empty; use [`resolve_with`] to size them.
pub fn resolve<P>(root: &Node<P>, offered: Option<Size>, limits: Limits) -> Result<Layout, Error> {
    resolve_with(root, offered, limits, SpacingScale::DEFAULT, |_, _| {
        Size::ZERO
    })
}

/// [`resolve`] with a spacing scale for tokens and a measurer for
/// `Node::content` leaves, called once per leaf with its payload and the
/// room it has: the narrowest definite inner width above it, `None` under a
/// hugging parent or inside a scroll. Text shaping lives outside this crate
/// on purpose.
pub fn resolve_with<P>(
    root: &Node<P>,
    offered: Option<Size>,
    limits: Limits,
    scale: SpacingScale,
    mut measurer: impl FnMut(&P, Option<f64>) -> Size,
) -> Result<Layout, Error> {
    if !limits.extent.is_finite()
        || limits.extent <= 0.0
        || limits.nodes == 0
        || limits.depth > 256
        || !scale.is_valid()
    {
        return Err(Error::InvalidValue);
    }
    let definite = offered.map_or([None; 2], |s| [Some(s.width), Some(s.height)]);
    let mut pass = Pass {
        left: limits.nodes,
        limits,
        scale,
        keys: BTreeMap::new(),
        measurer: &mut measurer,
    };
    let m = measure(root, "root", definite, None, 0, &mut pass)?;
    let size = offered.unwrap_or(m.size);
    if !size.valid(limits.extent) {
        return Err(Error::InvalidValue);
    }
    if root
        .maximum
        .is_some_and(|max| size.width > max.width || size.height > max.height)
    {
        return Err(Error::InsufficientSpace(label(root, "root")));
    }
    let mut out = (
        BTreeMap::new(),
        Vec::with_capacity(limits.nodes - pass.left),
    );
    arrange(&m, "root", [0.0, 0.0], size, &mut out)?;
    Ok(Layout {
        size,
        frames: out.0,
        order: out.1,
    })
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

#[cfg(test)]
mod tests;
