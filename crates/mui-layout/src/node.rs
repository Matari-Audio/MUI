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
    /// Candidates in order of preference. See [`Node::fits`].
    Fits(Vec<Node<P>>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Node<P = ()> {
    pub(crate) id: Option<String>,
    pub(crate) kind: Kind<P>,
    pub(crate) payload: P,
    pub(crate) gap: Spacing,
    /// Pixel insets, unless `pad` names a token for all four sides.
    pub(crate) padding: Insets,
    pub(crate) pad: Option<Spacing>,
    pub(crate) minimum: Size,
    pub(crate) maximum: Option<Size>,
    pub(crate) width: Len,
    pub(crate) height: Len,
    pub(crate) aspect: Option<f64>,
    pub(crate) grow: f64,
    pub(crate) basis: Option<f64>,
    pub(crate) shrink: f64,
    pub(crate) align: Align,
    pub(crate) align_self: Option<Align>,
    pub(crate) justify: Justify,
    pub(crate) anchor: Option<(Align, Align)>,
    pub(crate) offset: [f64; 2],
    /// Anchored placement for a float; see [`Node::pin`].
    pub(crate) pin: Option<Pin>,
    /// Children may overflow the main axis; the frame clips them and
    /// `scrolled` slides them. Implies `clip`.
    pub(crate) scroll: bool,
    pub(crate) clip: bool,
    pub(crate) scrolled: [f64; 2],
    /// Pinned to the enclosing scroll viewport's leading edge; see
    /// [`Node::sticky`]. Stays in flow, unlike a float.
    pub(crate) sticky: bool,
    /// Out of flow: takes no space in its parent and sits like an overlay
    /// child, anchored and offset within the parent's padding box. Tooltips,
    /// popups, drag ghosts.
    pub(crate) float: bool,
    /// Break the flow into lines when the children no longer fit the main
    /// axis. Branches only.
    pub(crate) wrap: bool,
    /// Grid cells only: how many columns this cell occupies.
    pub(crate) span: usize,
    /// Grids only: the narrowest a column may get before the grid drops one.
    pub(crate) min_col: Option<f64>,
    /// Placement order among siblings; ties keep declaration order. Frames
    /// stay in declaration order regardless.
    pub(crate) order: i32,
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
            pin: None,
            scroll: false,
            clip: false,
            scrolled: [0.0; 2],
            sticky: false,
            float: false,
            wrap: false,
            span: 1,
            min_col: None,
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
    /// use mui_layout::{leaf, resolve, Node, Size};
    /// let bar = Node::fits([
    ///     leaf(300., 20.).id("wide"),
    ///     leaf(120., 20.).id("mid"),
    ///     leaf(40., 20.).id("thin"),
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
            | Kind::Grid { children, .. }
            | Kind::Fits(children) => children,
            _ => &[],
        }
    }
    pub fn children_mut(&mut self) -> &mut [Self] {
        match &mut self.kind {
            Kind::Branch { children, .. }
            | Kind::Overlay(children)
            | Kind::Grid { children, .. }
            | Kind::Fits(children) => children,
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
    /// The same on all four sides: `.pad(12.0)`, `.pad(M)` or
    /// `.pad(Spacing::step(3.))`. One slot per concept: the last call wins,
    /// whichever spelling it used.
    pub fn pad(mut self, padding: impl Into<Spacing>) -> Self {
        match padding.into() {
            Spacing::Px(v) => {
                self.padding = Insets::all(v);
                self.pad = None;
            }
            scaled => self.pad = Some(scaled),
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
    /// Place this node against another node by name rather than inside its
    /// own parent: see [`Pin`]. Implies [`float`](Node::float), and the
    /// position is absolute, so the parent's padding and alignment no longer
    /// apply. Keep [`offset`](Node::offset) for a nudge no region can name.
    ///
    /// ```
    /// use mui_layout::{leaf, overlay, resolve, Area, Pin, Size};
    /// let menu = leaf(10., 20.).pin(Pin::to("field").area(Area::Bottom).match_width()).id("m");
    /// let l = resolve(&overlay([leaf(90., 24.).id("field"), menu]),
    ///                 Some(Size::new(200., 200.)), Default::default()).unwrap();
    /// assert_eq!(l.frame("m").unwrap().size.width, 90.);
    /// ```
    pub fn pin(mut self, pin: Pin) -> Self {
        self.pin = Some(pin);
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
    /// use mui_layout::{column, leaf, resolve, Size};
    /// let section = |k: &str| column([leaf(80., 20.).id(k).sticky(), leaf(80., 200.)]);
    /// let header_y = |dy: f64| {
    ///     let list = column([section("a"), section("b")]).scroll().scrolled(0., dy);
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
    /// Grids only: CSS `repeat(auto-fit, minmax(px, 1fr))`. The declared
    /// column count becomes a ceiling, and the grid drops columns until each
    /// one is at least `px` wide. One primitive covers most reflow: the same
    /// tree is three columns in a wide window and one in a thin one.
    ///
    /// ```
    /// use mui_layout::{grid, leaf, resolve, Size};
    /// let cells = (0..6).map(|i| leaf(20., 20.).id(format!("c{i}")));
    /// let g = grid(3, cells).gap(10.).min_col(120.).id("g");
    /// let cols = |w: f64| {
    ///     let l = resolve(&g, Some(Size::new(w, 300.)), Default::default()).unwrap();
    ///     (0..6).filter(|i| l.frame(&format!("c{i}")).unwrap().y == l.frame("c0").unwrap().y).count()
    /// };
    /// assert_eq!((cols(800.), cols(260.), cols(240.)), (3, 2, 1));
    /// ```
    pub fn min_col(mut self, px: f64) -> Self {
        self.min_col = Some(px);
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
            | Kind::Grid { children, .. }
            | Kind::Fits(children) => children.push(child),
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
    /// Whether [`Node::sticky`] was called: the scene walk asks, because a
    /// sticky child paints after the siblings that scroll under it.
    ///
    /// ```
    /// use mui_layout::leaf;
    /// assert!(leaf(10., 10.).sticky().is_sticky());
    /// assert!(!leaf(10., 10.).is_sticky());
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
/// See [`Node::fits`].
pub fn fits(candidates: impl IntoIterator<Item = Node>) -> Node {
    Node::fits(candidates)
}
