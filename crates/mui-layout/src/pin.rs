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
/// use mui_layout::{block, stack, resolve, Align, Area, Pin, Size};
/// let field = block(40., 40.).anchor(Align::Start, Align::Start).id("f");
/// let menu = block(30., 20.).pin(Pin::to("f").area(Area::End)).id("menu");
/// let l = resolve(&stack([field, menu]),
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
/// use mui_layout::{block, stack, resolve, Pin, Size};
/// let menu = block(10., 20.).pin(Pin::to("f").match_width()).id("menu");
/// let l = resolve(&stack([block(90., 24.).id("f"), menu]),
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
/// use mui_layout::{block, stack, resolve, Align, Pin, Size, SpacingToken::Xs};
/// let tip = block(30., 20.).pin(Pin::to("knob").gap(Xs)).id("tip");
/// let knob = block(40., 40.).anchor(Align::Start, Align::Start).id("knob");
/// let tree = stack([knob, tip]);
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
    pub(crate) anchors: &'a BTreeMap<Id, Frame>,
    pub(crate) root: Size,
    pub(crate) scale: SpacingScale,
}

/// Resolve a pin after the pin that owns its anchor and its parent. The first
/// arrange already supplied every intrinsic size and frame slot; replaying a
/// pinned subtree updates those slots without disturbing tree/named order.
pub(crate) fn resolve_pins<P>(
    root: &Measured<'_, P>,
    size: Size,
    scale: SpacingScale,
    out: &mut (Vec<(Id, u32)>, Vec<Frame>, Vec<Size>),
) -> Result<(), Error> {
    struct Pending<'a, 'n, P> {
        node: &'a Measured<'n, P>,
        at: usize,
        end: usize,
        parent: Option<usize>,
    }
    fn collect<'a, 'n, P>(
        node: &'a Measured<'n, P>,
        next: &mut usize,
        parent: Option<usize>,
        pending: &mut Vec<Pending<'a, 'n, P>>,
        owners: &mut BTreeMap<Id, usize>,
        frames: &[Frame],
    ) {
        let at = *next;
        *next += 1;
        let owner =
            if frames[at].size != Size::ZERO && node.node.float && node.node.rare().pin.is_some() {
                let i = pending.len();
                pending.push(Pending {
                    node,
                    at,
                    end: 0,
                    parent,
                });
                Some(i)
            } else {
                parent
            };
        if let (Some(id), Some(owner)) = (&node.node.id, owner) {
            owners.insert(id.clone(), owner);
        }
        for child in &node.children {
            collect(child, next, owner, pending, owners, frames);
        }
        if owner != parent {
            pending[owner.unwrap()].end = *next;
        }
    }
    let mut pending = Vec::new();
    let mut owners = BTreeMap::new();
    collect(root, &mut 0, None, &mut pending, &mut owners, &out.1);
    let dependencies: Vec<Vec<usize>> = pending
        .iter()
        .map(|p| {
            p.parent
                .into_iter()
                .chain(
                    owners
                        .get(p.node.node.rare().pin.as_ref().unwrap().anchor.as_str())
                        .copied(),
                )
                .collect()
        })
        .collect();
    let mut order = Vec::with_capacity(pending.len());
    let mut remaining: Vec<_> = dependencies.iter().map(Vec::len).collect();
    let mut dependents = vec![Vec::new(); pending.len()];
    for (i, dependencies) in dependencies.iter().enumerate() {
        for &dependency in dependencies {
            dependents[dependency].push(i);
        }
        if dependencies.is_empty() {
            order.push(i);
        }
    }
    let mut ready = 0;
    while ready < order.len() {
        for &dependent in &dependents[order[ready]] {
            remaining[dependent] -= 1;
            if remaining[dependent] == 0 {
                order.push(dependent);
            }
        }
        ready += 1;
    }
    if order.len() != pending.len() {
        return Err(Error::InvalidValue);
    }
    let mut anchors: BTreeMap<_, _> = out
        .0
        .iter()
        .map(|(id, at)| (id.clone(), out.1[*at as usize]))
        .collect();
    for i in order {
        let p = &pending[i];
        let frame = out.1[p.at];
        // A losing `fits` branch stays hidden, including its floats.
        if frame.size == Size::ZERO {
            continue;
        }
        let pin = p.node.node.rare().pin.as_ref().unwrap();
        let Some(&anchor) = anchors.get(pin.anchor.as_str()) else {
            continue;
        };
        let extent = pin.sized(anchor, frame.size);
        let origin = pin.place(anchor, extent, size, scale);
        let mut resolved = (
            Vec::new(),
            Vec::with_capacity(p.end - p.at),
            Vec::with_capacity(p.end - p.at),
        );
        arrange(
            p.node,
            "root",
            origin,
            extent,
            &Pins {
                anchors: &anchors,
                root: size,
                scale,
            },
            None,
            &mut resolved,
        )?;
        out.1[p.at..p.end].copy_from_slice(&resolved.1);
        out.2[p.at..p.end].copy_from_slice(&resolved.2);
        for (id, at) in resolved.0 {
            anchors.insert(id, resolved.1[at as usize]);
        }
    }
    Ok(())
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
