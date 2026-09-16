//! Floats: [`Pin`], the [`Area`] regions around an anchor and [`Match`].
//!
//! A pinned node is placed in a second pass, once the anchor frames from
//! the first one exist. This module holds the vocabulary and the
//! candidate search; the pass that calls it lives in `arrange`.

use super::*;

/// One of the nine named regions around an anchor, CSS `position-area`
/// without the coordinates. The four sides centre on the anchor's other
/// axis; the four corners align to the anchor's near edge, which is what a
/// dropdown under a field wants. [`Area::Center`] sits over the anchor.
///
/// ```
/// use mui_layout::{leaf, overlay, resolve, Align, Area, Pin, Size};
/// let field = leaf(40., 40.).anchor(Align::Start, Align::Start).id("f");
/// let menu = leaf(30., 20.).pin(Pin::to("f").area(Area::End)).id("menu");
/// let l = resolve(&overlay([field, menu]),
///                 Some(Size::new(200., 200.)), Default::default()).unwrap();
/// // Past the anchor's right edge, centred on its height.
/// assert_eq!((l.frame("menu").unwrap().x, l.frame("menu").unwrap().y), (40., 10.));
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Area {
    TopStart,
    /// Above, centred. The default.
    #[default]
    Top,
    TopEnd,
    Start,
    Center,
    End,
    BottomStart,
    Bottom,
    BottomEnd,
}

/// Take one axis of the size from the anchor, CSS `anchor-size()`. See
/// [`Pin::match_width`] and [`Pin::match_height`].
///
/// ```
/// use mui_layout::{leaf, overlay, resolve, Pin, Size};
/// let menu = leaf(10., 20.).pin(Pin::to("f").match_width()).id("menu");
/// let l = resolve(&overlay([leaf(90., 24.).id("f"), menu]),
///                 Some(Size::new(200., 200.)), Default::default()).unwrap();
/// assert_eq!(l.frame("menu").unwrap().size.width, 90.);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Match {
    #[default]
    None,
    Width,
    Height,
}

/// Where a float sits relative to another node, by name: the anchor, a named
/// region around it, a gap, an optional size taken from it, and an ordered
/// list of regions to try when the preferred one leaves the root rect. The
/// first candidate that fits wins; if none does, the preferred one is pulled
/// back inside.
///
/// ```
/// use mui_layout::{leaf, overlay, resolve, Align, Pin, Size, SpacingToken::Xs};
/// let tip = leaf(30., 20.).pin(Pin::to("knob").gap(Xs)).id("tip");
/// let knob = leaf(40., 40.).anchor(Align::Start, Align::Start).id("knob");
/// let tree = overlay([knob, tip]);
/// let l = resolve(&tree, Some(Size::new(200., 200.)), Default::default()).unwrap();
/// // No room above at the top of the window, so it flips under the knob.
/// assert_eq!(l.frame("tip").unwrap().y, 44.);
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct Pin {
    pub(crate) anchor: String,
    pub(crate) area: Area,
    pub(crate) gap: Spacing,
    pub(crate) size: Match,
    pub(crate) fallbacks: Vec<Area>,
}
impl Pin {
    /// Pin to the node with this id, above it by default, flipping below if
    /// there is no room.
    pub fn to(anchor: impl Into<String>) -> Self {
        Self {
            anchor: anchor.into(),
            area: Area::Top,
            gap: Spacing::Px(0.0),
            size: Match::None,
            fallbacks: vec![Area::Bottom],
        }
    }
    /// The preferred region. Setting it clears the default flip; add your own
    /// with [`fallback`](Pin::fallback).
    pub fn area(mut self, area: Area) -> Self {
        self.area = area;
        self.fallbacks.clear();
        self
    }
    /// Distance from the anchor's edge.
    pub fn gap(mut self, gap: impl Into<Spacing>) -> Self {
        self.gap = gap.into();
        self
    }
    /// Take the anchor's width: a menu as wide as its field.
    pub fn match_width(mut self) -> Self {
        self.size = Match::Width;
        self
    }
    /// Take the anchor's height: a side panel as tall as its row.
    pub fn match_height(mut self) -> Self {
        self.size = Match::Height;
        self
    }
    /// Another region to try, in order, when the ones before it overflow.
    pub fn fallback(mut self, area: Area) -> Self {
        self.fallbacks.push(area);
        self
    }
    pub(crate) fn sized(&self, anchor: Frame, s: Size) -> Size {
        match self.size {
            Match::None => s,
            Match::Width => Size::new(anchor.size.width, s.height),
            Match::Height => Size::new(s.width, anchor.size.height),
        }
    }
    /// The top-left corner for `area`, in the same space as `anchor`.
    pub(crate) fn corner(&self, area: Area, anchor: Frame, s: Size, gap: f64) -> [f64; 2] {
        let (a, w, h) = (anchor, s.width, s.height);
        let x = match area {
            Area::Start => a.x - w - gap,
            Area::End => a.right() + gap,
            Area::TopStart | Area::BottomStart => a.x,
            Area::TopEnd | Area::BottomEnd => a.right() - w,
            Area::Top | Area::Bottom | Area::Center => a.x + (a.size.width - w) / 2.0,
        };
        let y = match area {
            Area::TopStart | Area::Top | Area::TopEnd => a.y - h - gap,
            Area::BottomStart | Area::Bottom | Area::BottomEnd => a.bottom() + gap,
            Area::Start | Area::End | Area::Center => a.y + (a.size.height - h) / 2.0,
        };
        [x, y]
    }
    /// The winning corner: the first candidate inside `root`, else the
    /// preferred one pulled back in.
    pub(crate) fn place(
        &self,
        anchor: Frame,
        s: Size,
        root: Size,
        scale: SpacingScale,
    ) -> [f64; 2] {
        let gap = self.gap.resolve(scale);
        let fits = |p: [f64; 2]| {
            p[0] >= 0.0
                && p[1] >= 0.0
                && p[0] + s.width <= root.width
                && p[1] + s.height <= root.height
        };
        std::iter::once(self.area)
            .chain(self.fallbacks.iter().copied())
            .map(|a| self.corner(a, anchor, s, gap))
            .find(|p| fits(*p))
            .unwrap_or_else(|| {
                let p = self.corner(self.area, anchor, s, gap);
                [
                    inside(p[0], s.width, root.width),
                    inside(p[1], s.height, root.height),
                ]
            })
    }
}

/// Everything a pinned float needs that its parent does not know: the anchor
/// frames from the previous arrange pass, the root rect they live in, and the
/// scale a [`Spacing`] gap resolves against.
pub(crate) struct Pins<'a> {
    pub(crate) anchors: &'a BTreeMap<String, Frame>,
    pub(crate) root: Size,
    pub(crate) scale: SpacingScale,
}

/// The scroll frame a [`sticky`](Node::sticky) child pins itself against: the
/// enclosing scroll node's main axis and the absolute coordinate of its
/// leading edge. Sticky is the same second thought as a pin -- a position the
/// parent's flow does not know -- applied against the viewport instead of
/// against an anchor node.
#[derive(Clone, Copy)]
pub(crate) struct Viewport {
    pub(crate) vertical: bool,
    pub(crate) edge: f64,
}
impl Viewport {
    /// Where a sticky child goes: its flow position, held at the viewport edge
    /// once it would scroll past it, and pushed back off by `end`, the far
    /// edge of its section. It never moves ahead of its flow position.
    pub(crate) fn stick(self, mut flow: [f64; 2], size: Size, end: f64) -> [f64; 2] {
        let a = self.vertical as usize;
        let last = (end - size.main(self.vertical)).max(flow[a]);
        flow[a] = flow[a].max(self.edge).min(last);
        flow
    }
}

/// Pull `v` back inside `avail`. One too big to fit keeps its place: there is
/// no inside to pull it to.
pub(crate) fn inside(v: f64, extent: f64, avail: f64) -> f64 {
    if extent <= avail {
        v.clamp(0.0, avail - extent)
    } else {
        v
    }
}
