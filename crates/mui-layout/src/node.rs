//! The tree: [`Node`], its builders and the payload it carries.
//!
//! Everything here is declaration. No measuring, no arranging -- a `Node`
//! is what the caller wrote down, and the solver reads it without
//! changing it.

use super::*;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Kind<P> {
    /// Opaque content of a declared size.
    Leaf,
    /// Content the solver cannot size itself -- text, mostly. Measured by the
    /// callback handed to [`resolve_with`]. Children pushed onto it sit over
    /// it, as on a stack: a carve, a badge.
    Content(Vec<Node<P>>),
    Branch {
        vertical: bool,
        children: Vec<Node<P>>,
    },
    Overlay(Vec<Node<P>>),
    Grid {
        cols: usize,
        children: Vec<Node<P>>,
    },
    /// Candidates in order of preference. See [`Node::fits`].
    Fits(Vec<Node<P>>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Node<P = ()> {
    pub(crate) id: Option<Id>,
    pub(crate) kind: Kind<P>,
    pub(crate) payload: P,
    pub(crate) gap: Spacing,
    /// Pixel insets, unless `pad` names a token for all four sides.
    pub(crate) padding: Insets,
    pub(crate) pad: Option<Spacing>,
    pub(crate) minimum: Size,
    pub(crate) width: Len,
    pub(crate) height: Len,
    pub(crate) grow: f64,
    pub(crate) basis: Option<f64>,
    pub(crate) shrink: f64,
    pub(crate) align: Align,
    pub(crate) align_self: Option<Align>,
    pub(crate) justify: Justify,
    pub(crate) anchor: Option<(Align, Align)>,
    pub(crate) offset: [f64; 2],
    /// Children may overflow the main axis; the frame clips them and
    /// `scrolled` slides them. Implies `clip`.
    pub(crate) scroll: bool,
    pub(crate) clip: bool,
    pub(crate) scrolled: [f64; 2],
    /// Pinned to the enclosing scroll viewport's leading edge; see
    /// [`Node::sticky`]. Stays in flow, unlike a float.
    pub(crate) sticky: bool,
    /// Out of flow: takes no space in its parent and sits like a stack
    /// child, anchored and offset within the parent's padding box. Tooltips,
    /// popups, drag ghosts.
    pub(crate) float: bool,
    /// Break the flow into lines when the children no longer fit the main
    /// axis. Branches only.
    pub(crate) wrap: bool,
    /// Grid cells only: how many columns this cell occupies.
    pub(crate) span: usize,
    /// Placement order among siblings; ties keep declaration order. Frames
    /// stay in declaration order regardless.
    pub(crate) order: i32,
    /// What most nodes never set, boxed on first write so a builder call
    /// moves a small node. Read it through [`Node::rare`].
    pub(crate) rare: Option<Box<Rare>>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Rare {
    /// The gap between wrapped lines and grid rows; `None` is `gap`. See
    /// [`Node::line_gap`].
    pub(crate) line_gap: Option<Spacing>,
    pub(crate) maximum: Option<Size>,
    pub(crate) aspect: Option<f64>,
    /// Anchored placement for a float; see [`Node::pin`].
    pub(crate) pin: Option<Pin>,
    /// Grids only: the narrowest a column may get before the grid drops one.
    pub(crate) min_col: Option<f64>,
}
impl Rare {
    const NONE: Self = Self {
        line_gap: None,
        maximum: None,
        aspect: None,
        pin: None,
        min_col: None,
    };
}

impl<P: Default> Node<P> {
    pub(crate) fn new(kind: Kind<P>) -> Self {
        Self {
            id: None,
            kind,
            payload: P::default(),
            gap: Spacing::Px(0.0),
            padding: Insets::ZERO,
            pad: None,
            minimum: Size::ZERO,
            width: Len::Auto,
            height: Len::Auto,
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
            sticky: false,
            float: false,
            wrap: false,
            span: 1,
            order: 0,
            rare: None,
        }
    }
    /// Content whose size is already known: an icon cell, a spacer.
    pub fn block(width: impl Into<Len>, height: impl Into<Len>) -> Self {
        Self::new(Kind::Leaf).size(width, height)
    }
    /// Content measured by the callback given to [`resolve_with`].
    pub fn content() -> Self {
        Self::new(Kind::Content(Vec::new()))
    }
    /// Children laid out left to right.
    pub fn row(children: impl IntoIterator<Item = Self>) -> Self {
        Self::new(Kind::Branch {
            vertical: false,
            children: children.into_iter().collect(),
        })
    }
    /// Children laid out top to bottom.
    pub fn col(children: impl IntoIterator<Item = Self>) -> Self {
        Self::new(Kind::Branch {
            vertical: true,
            children: children.into_iter().collect(),
        })
    }
    /// Children sharing one rect, back to front. Each may `anchor` itself.
    pub fn stack(children: impl IntoIterator<Item = Self>) -> Self {
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
    /// Candidates in order of preference, largest first: the first one whose
    /// measured size fits the space this node is offered is the one laid out,
    /// and the rest are given empty frames. SwiftUI's `ViewThatFits`.
    ///
    /// The candidates are already built, so nothing is measured twice and
    /// there is no second build pass -- which is the whole reason to prefer
    /// it to a width branch in the caller. An axis this node has no definite
    /// size on never rejects a candidate; the last one is the fallback.
    ///
    /// ```
    /// use mui_layout::{block, resolve, Node, Size};
    /// let bar = Node::fits([
    ///     block(300., 20.).id("wide"),
    ///     block(120., 20.).id("mid"),
    ///     block(40., 20.).id("thin"),
    /// ]);
    /// let shown = |w: f64| {
    ///     let l = resolve(&bar, Some(Size::new(w, 20.)), Default::default()).unwrap();
    ///     ["wide", "mid", "thin"]
    ///         .iter()
    ///         .find(|k| l.frame(k).unwrap().size.width > 0.)
    ///         .copied()
    ///         .unwrap()
    /// };
    /// assert_eq!((shown(400.), shown(200.), shown(60.)), ("wide", "mid", "thin"));
    /// ```
    pub fn fits(candidates: impl IntoIterator<Item = Self>) -> Self {
        Self::new(Kind::Fits(candidates.into_iter().collect()))
    }
}

impl<P> Node<P> {
    pub(crate) fn rare(&self) -> &Rare {
        static NONE: Rare = Rare::NONE;
        self.rare.as_deref().unwrap_or(&NONE)
    }
    fn rare_mut(&mut self) -> &mut Rare {
        self.rare.get_or_insert_with(|| Box::new(Rare::NONE))
    }
    /// Name this node, so `Layout::frame` can find it. Structural nodes need no
    /// name and cost nothing unnamed.
    /// ```
    /// use mui_layout::{block, Id};
    /// assert_eq!(block(1., 1.).id(Id::of("osc").slot(3)).key(), Some("osc/3"));
    /// ```
    pub fn id(mut self, id: impl Into<Id>) -> Self {
        self.id = Some(id.into());
        self
    }
    pub fn key(&self) -> Option<&str> {
        self.id.as_deref()
    }
    /// The node's [`Id`], when it has one; [`Node::key`] as a `&str`.
    pub fn ident(&self) -> Option<&Id> {
        self.id.as_ref()
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
            | Kind::Content(children)
            | Kind::Grid { children, .. }
            | Kind::Fits(children) => children,
            Kind::Leaf => &[],
        }
    }
    pub fn children_mut(&mut self) -> &mut [Self] {
        match &mut self.kind {
            Kind::Branch { children, .. }
            | Kind::Overlay(children)
            | Kind::Content(children)
            | Kind::Grid { children, .. }
            | Kind::Fits(children) => children,
            Kind::Leaf => &mut [],
        }
    }
    /// Containers stretch; content centres. This is what `Align::Stretch`
    /// consults.
    pub fn is_container(&self) -> bool {
        !matches!(self.kind, Kind::Leaf | Kind::Content(_))
    }
    /// Mutable spacing for retained animation of a coupled shape gap.
    pub fn gap_mut(&mut self) -> &mut Spacing {
        &mut self.gap
    }
    pub fn gap(mut self, gap: impl Into<Spacing>) -> Self {
        self.gap = gap.into();
        self
    }
    /// The same on all four sides: `.pad(12.0)`, `.pad(M)` or
    /// `.pad(Spacing::step(3.))`. One slot per concept: the last call wins,
    /// whichever spelling it used.
    ///
    /// `.pad((16, 8))` is 16 px left and right, 8 top and bottom;
    /// `.pad(Insets { .. })` sets each side.
    pub fn pad(mut self, padding: impl Into<Pad>) -> Self {
        match padding.into() {
            Pad::All(Spacing::Px(v)) => {
                self.padding = Insets::all(v);
                self.pad = None;
            }
            Pad::All(scaled) => self.pad = Some(scaled),
            Pad::Insets(i) => {
                self.padding = i;
                self.pad = None;
            }
        }
        self
    }
    /// What the padding comes to under `scale`.
    pub fn padding(&self, scale: SpacingScale) -> Insets {
        self.pad
            .map_or(self.padding, |s| Insets::all(s.resolve(scale)))
    }
    /// Declared outer size on both axes. `Auto` measures, `Px` fixes, `Pct`
    /// takes a share of the parent: `.size(Len::Pct(50.0), 24.0)`.
    pub fn size(mut self, width: impl Into<Len>, height: impl Into<Len>) -> Self {
        self.width = width.into();
        self.height = height.into();
        self
    }
    /// `width / height`. Fills in whichever axis was left `Auto` -- at measure
    /// from a fixed sibling axis, at arrange from the allocated one.
    pub fn aspect(mut self, ratio: impl Px) -> Self {
        self.rare_mut().aspect = Some(ratio.px());
        self
    }
    /// The floor on the width.
    pub fn min_w(mut self, width: impl Px) -> Self {
        self.minimum.width = width.px();
        self
    }
    /// The floor on the height.
    pub fn min_h(mut self, height: impl Px) -> Self {
        self.minimum.height = height.px();
        self
    }
    pub fn min_size(mut self, size: impl Into<Size>) -> Self {
        self.minimum = size.into();
        self
    }
    pub fn max_size(mut self, size: impl Into<Size>) -> Self {
        self.rare_mut().maximum = Some(size.into());
        self
    }
    /// Width: `.w(120)`, `.w(Len::Pct(50.))`.
    pub fn w(mut self, len: impl Into<Len>) -> Self {
        self.width = len.into();
        self
    }
    /// Height: `.h(24)`.
    pub fn h(mut self, len: impl Into<Len>) -> Self {
        self.height = len.into();
        self
    }
    /// Both axes: `.square(32)`.
    pub fn square(self, len: impl Into<Len>) -> Self {
        let l = len.into();
        self.size(l, l)
    }
    /// Centred on both axes.
    pub fn center(self) -> Self {
        self.align(Align::Center).justify(Justify::Center)
    }
    /// Packed at the start of both axes.
    pub fn start(self) -> Self {
        self.align(Align::Start).justify(Justify::Start)
    }
    /// Packed at the end of both axes.
    pub fn end(self) -> Self {
        self.align(Align::End).justify(Justify::End)
    }
    /// Children pushed to the two ends, cross-axis centred.
    pub fn between(self) -> Self {
        self.align(Align::Center).justify(Justify::SpaceBetween)
    }
    /// All of the parent, both axes: `.w(Len::Pct(100.)).h(Len::Pct(100.))`.
    pub fn full(self) -> Self {
        self.size(Len::Pct(100.), Len::Pct(100.))
    }
    /// Take a share of the surplus, by weight: `.grow(1)`.
    pub fn grow(mut self, weight: impl Px) -> Self {
        self.grow = weight.px();
        self
    }
    /// Start from this main-axis size instead of the measured one, before any
    /// growth or shrink. `basis(0.0)` is the only way to get equal *shares* of
    /// an axis rather than equal shares of the surplus: CSS `flex-basis: 0`,
    /// what `1fr` means.
    pub fn basis(mut self, basis: impl Px) -> Self {
        self.basis = Some(basis.px());
        self
    }
    /// Weight for absorbing a deficit, scaled by basis the way flexbox scales
    /// it. Defaults to 1; `shrink(0.0)` opts out, and `minimum` is the floor
    /// either way.
    pub fn shrink(mut self, weight: impl Px) -> Self {
        self.shrink = weight.px();
        self
    }
    /// `grow(weight).basis(0.0)`: an equal share of the axis per unit of
    /// weight, regardless of what the child measured. CSS `flex: <weight>`.
    pub fn flex(self, weight: impl Px) -> Self {
        self.grow(weight).basis(0)
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
    /// Where this child sits inside a stack or grid cell, per axis. Defaults
    /// to the parent's `align` on both axes, or `justify` for y once that is
    /// set.
    pub fn anchor(mut self, x: Align, y: Align) -> Self {
        self.anchor = Some((x, y));
        self
    }
    /// A nudge from the anchored position. The only coordinates in the system,
    /// and relative ones at that.
    pub fn offset(mut self, dx: impl Px, dy: impl Px) -> Self {
        self.offset = [dx.px(), dy.px()];
        self
    }
    /// Placed `(dx, dy)` from the top-left of the parent's box: `anchor` at
    /// the start of both axes, then `offset`.
    ///
    /// ```
    /// use mui_layout::{block, resolve, stack, Size};
    /// let dot = block(4., 4.).at(10., 20.).id("dot");
    /// let l = resolve(&stack([dot]), Some(Size::new(100., 100.)), Default::default()).unwrap();
    /// assert_eq!((l.frame("dot").unwrap().x, l.frame("dot").unwrap().y), (10., 20.));
    /// ```
    pub fn at(mut self, dx: impl Px, dy: impl Px) -> Self {
        self.anchor = Some((Align::Start, Align::Start));
        self.offset(dx, dy)
    }
    /// Centred in the parent's box, then nudged by `(dx, dy)`.
    pub fn centered_at(mut self, dx: impl Px, dy: impl Px) -> Self {
        self.anchor = Some((Align::Center, Align::Center));
        self.offset(dx, dy)
    }
    /// Centred in the parent's box (a stack or grid cell).
    ///
    /// ```
    /// use mui_layout::{block, resolve, stack, Size};
    /// let dot = block(4, 4).centered().id("dot");
    /// let l = resolve(&stack([dot]), Some(Size::new(100., 100.)), Default::default()).unwrap();
    /// assert_eq!(l.frame("dot").unwrap().x, 48.);
    /// ```
    pub fn centered(mut self) -> Self {
        self.anchor = Some((Align::Center, Align::Center));
        self.offset = [0.0, 0.0];
        self
    }
    /// Let the children overflow the main axis behind a clip. The node's
    /// floor on that axis drops to its padding, so it can be squeezed. A
    /// stack has no main axis and overflows on both.
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
    pub fn scrolled(mut self, x: impl Px, y: impl Px) -> Self {
        self.scrolled = [x.px(), y.px()];
        self
    }
    /// Place this node against another node by name rather than inside its
    /// own parent: see [`Pin`]. Implies [`float`](Node::float), and the
    /// position is absolute, so the parent's padding and alignment no longer
    /// apply. Keep [`offset`](Node::offset) for a nudge no region can name.
    ///
    /// ```
    /// use mui_layout::{block, stack, resolve, Area, Pin, Size};
    /// let menu = block(10., 20.).pin(Pin::to("field").area(Area::Bottom).match_width()).id("m");
    /// let l = resolve(&stack([block(90., 24.).id("field"), menu]),
    ///                 Some(Size::new(200., 200.)), Default::default()).unwrap();
    /// assert_eq!(l.frame("m").unwrap().size.width, 90.);
    /// ```
    pub fn pin(mut self, pin: Pin) -> Self {
        self.rare_mut().pin = Some(pin);
        self.float()
    }
    /// Pin this node to the leading edge of the enclosing
    /// [`scroll`](Node::scroll) viewport -- the top of a scrolling column, the
    /// start of a scrolling row -- while its section is still in view. The
    /// section is this node's own parent, so a header in a section column
    /// rides at the edge until its section ends and then is pushed off by the
    /// next one, CSS `position: sticky`. It keeps its slot in the flow, unlike
    /// a float, and with no scrolling ancestor it does nothing.
    ///
    /// ```
    /// use mui_layout::{col, block, resolve, Size};
    /// let section = |k: &str| col([block(80., 20.).id(k).sticky(), block(80., 200.)]);
    /// let header_y = |dy: f64| {
    ///     let list = col([section("a"), section("b")]).scroll().scrolled(0., dy);
    ///     let l = resolve(&list, Some(Size::new(80., 100.)), Default::default()).unwrap();
    ///     l.frame("a").unwrap().y
    /// };
    /// // Held at the top while its section runs, then pushed off by its end.
    /// assert_eq!((header_y(0.), header_y(50.), header_y(210.)), (0., 0., -10.));
    /// ```
    pub fn sticky(mut self) -> Self {
        self.sticky = true;
        self
    }
    /// Take this node out of flow: see the `float` field.
    pub fn float(mut self) -> Self {
        self.float = true;
        self
    }
    /// Let a row or column break into lines when its children overflow the
    /// main axis. Lines stack on the cross axis with the same `gap`, unless
    /// [`Node::line_gap`] says otherwise.
    pub fn wrap(mut self) -> Self {
        self.wrap = true;
        self
    }
    /// The gap between the lines of a [`Node::wrap`]ped row or column, and
    /// between a grid's rows: CSS `row-gap` beside `gap`'s `column-gap`, so
    /// wrapped chips can sit 8 apart and their lines 6.
    ///
    /// ```
    /// use mui_layout::{resolve, Node, Size};
    /// let chips = Node::<()>::row((0..4).map(|i| Node::block(40., 20.).id(format!("c{i}"))))
    ///     .gap(8.)
    ///     .line_gap(2.)
    ///     .wrap();
    /// let l = resolve(&chips, Some(Size::new(100., 100.)), Default::default()).unwrap();
    /// assert_eq!(l.frame("c2").unwrap().y, 22.);
    /// ```
    pub fn line_gap(mut self, gap: impl Into<Spacing>) -> Self {
        self.rare_mut().line_gap = Some(gap.into());
        self
    }
    /// How many grid columns this cell takes, clamped to the column count.
    pub fn span(mut self, cols: usize) -> Self {
        self.span = cols.max(1);
        self
    }
    /// Grids only: CSS `repeat(auto-fit, minmax(px, 1fr))`. The declared
    /// column count becomes a ceiling, and the grid drops columns until each
    /// one is at least `px` wide. One primitive covers most reflow: the same
    /// tree is three columns in a wide window and one in a thin one. A
    /// hugging grid -- a modal, a popover, anything offered no width -- has
    /// nothing to drop columns against, so it keeps its count and widens
    /// itself to the minimum instead of squeezing a column under it.
    ///
    /// ```
    /// use mui_layout::{grid, block, resolve, Size};
    /// let cells = (0..6).map(|i| block(20., 20.).id(format!("c{i}")));
    /// let g = grid(3, cells).gap(10.).min_col(120.).id("g");
    /// let cols = |w: f64| {
    ///     let l = resolve(&g, Some(Size::new(w, 300.)), Default::default()).unwrap();
    ///     (0..6).filter(|i| l.frame(&format!("c{i}")).unwrap().y == l.frame("c0").unwrap().y).count()
    /// };
    /// assert_eq!((cols(800.), cols(260.), cols(240.)), (3, 2, 1));
    /// ```
    pub fn min_col(mut self, px: impl Px) -> Self {
        self.rare_mut().min_col = Some(px.px());
        self
    }
    /// Place this child as if it were declared at `order`; its frame keeps
    /// its declaration slot, so a tree walk still lines up.
    pub fn order(mut self, order: i32) -> Self {
        self.order = order;
        self
    }
    /// Append a child to a container. A block becomes a stack of its own
    /// size holding the child; on a content node the child sits over the
    /// content, as it would on a stack, and the node is at least as big as
    /// its in-flow children.
    pub fn push(mut self, child: Self) -> Self {
        match &mut self.kind {
            Kind::Branch { children, .. }
            | Kind::Overlay(children)
            | Kind::Content(children)
            | Kind::Grid { children, .. }
            | Kind::Fits(children) => children.push(child),
            Kind::Leaf => self.kind = Kind::Overlay(vec![child]),
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
    /// Whether [`Node::sticky`] was called: the scene walk asks, because a
    /// sticky child paints after the siblings that scroll under it.
    ///
    /// ```
    /// use mui_layout::block;
    /// assert!(block(10., 10.).sticky().is_sticky());
    /// assert!(!block(10., 10.).is_sticky());
    /// ```
    pub fn is_sticky(&self) -> bool {
        self.sticky
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
    pub(crate) fn len(&self, vertical: bool) -> Len {
        if vertical { self.height } else { self.width }
    }
}

pub fn block(width: impl Into<Len>, height: impl Into<Len>) -> Node {
    Node::block(width, height)
}
pub fn row(children: impl IntoIterator<Item = Node>) -> Node {
    Node::row(children)
}
pub fn col(children: impl IntoIterator<Item = Node>) -> Node {
    Node::col(children)
}
pub fn stack(children: impl IntoIterator<Item = Node>) -> Node {
    Node::stack(children)
}
pub fn grid(cols: usize, children: impl IntoIterator<Item = Node>) -> Node {
    Node::grid(cols, children)
}
/// See [`Node::fits`].
pub fn fits(candidates: impl IntoIterator<Item = Node>) -> Node {
    Node::fits(candidates)
}
