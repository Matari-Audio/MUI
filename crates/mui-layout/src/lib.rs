//! Small, renderer-independent intrinsic layout for plugin UIs.
//!
//! Not CSS, but it reads like the good half of it. Containers hug content by
//! default, a parent may offer an exact size, and surplus goes out by `grow`
//! and comes back by `shrink`. Alignment is automatic: containers stretch to
//! fill their cross axis, content centres in it, and nothing is ever placed by
//! coordinate -- a float names a region around another node with [`Pin`], and
//! an `offset` is the nudge left over.
//!
//! A node carries a payload `P` so a styling layer can ride the same tree
//! instead of mirroring it by id.
#![forbid(unsafe_code)]

use std::{collections::BTreeMap, sync::Arc};

mod incremental;
pub use incremental::{LayoutCache, LayoutStats, resolve_cached_with};
mod arrange;
mod id;
mod len;
mod measure;
mod node;
mod pin;

pub use id::Id;
pub use len::{Align, Insets, Justify, Len, Size};
mod spacing;
pub use node::{Node, block, col, fits, grid, row, stack};
pub use spacing::{Spacing, SpacingScale, SpacingToken};

/// What a measurer says about a content leaf: its size in the room it was
/// given, and the narrowest a flex parent may squeeze it to -- for text, its
/// widest word, the way CSS `min-width: auto` keeps a flex item at its
/// min-content. A bare [`Size`] is squeezable to nothing.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Intrinsic {
    pub size: Size,
    pub min_width: f64,
}
impl From<Size> for Intrinsic {
    fn from(size: Size) -> Self {
        Self {
            size,
            min_width: 0.0,
        }
    }
}
pub use pin::{Area, Match, Pin};

pub(crate) use arrange::{arrange, distribute};
pub(crate) use measure::{Measured, Pass, cell_default, grid_rows, measure, place, wrap_lines};
pub(crate) use node::{Kind, Rare};
pub(crate) use pin::{Pins, Viewport, inside};

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
    min: Size,
    /// Every named node and its index into `order`, in tree order. Shared,
    /// as is `order`, so the layout cache hands back an unchanged frame's
    /// layout without copying it. Nothing on a frame's path looks a name
    /// up, so none is sorted or hashed for it.
    named: Arc<Vec<(Id, u32)>>,
    /// Every node's frame, in tree order (parent first, then children in
    /// declaration order). A walk of the same tree indexes straight into it,
    /// so nothing needs a name to be found.
    order: Arc<Vec<Frame>>,
}
impl Layout {
    /// The smallest this tree can be squeezed to: every `minimum`, padding and
    /// unsqueezable leaf in it, summed along the axis it sits on. A scroll
    /// node contributes nothing on its scrolling axis, which is the point of
    /// one. Below it nothing is refused: children keep their floors and the
    /// content overflows, clipped wherever a clip is set. A host that owns a
    /// window can scale by `offered / min_size()` or refuse to go smaller.
    ///
    /// ```
    /// use mui_layout::{col, block, resolve, Size};
    /// let fixed = block(40., 30.).min_size(Size::new(40., 30.));
    /// let tree = col([fixed, block(40., 30.)]).pad(8.);
    /// let l = resolve(&tree, Some(Size::new(400., 300.)), Default::default()).unwrap();
    /// // The second leaf states no minimum, so it may be squeezed to nothing.
    /// assert_eq!(l.min_size(), Size::new(56., 46.));
    /// ```
    pub fn min_size(&self) -> Size {
        self.min
    }
    pub fn frame(&self, key: &str) -> Option<Frame> {
        let (_, at) = self.named.iter().find(|(id, _)| id.as_str() == key)?;
        Some(self.order[*at as usize])
    }
    pub fn all(&self) -> &[Frame] {
        &self.order
    }
    /// Publish contour-derived frames in the same tree order, keeping named lookup consistent.
    pub fn reframe<P>(mut self, root: &Node<P>, frames: Vec<Frame>) -> Result<Self, Error> {
        if frames.len() != self.order.len()
            || frames.iter().any(|f| {
                ![f.x, f.y, f.size.width, f.size.height]
                    .iter()
                    .all(|v| v.is_finite())
                    || f.size.width < 0.
                    || f.size.height < 0.
            })
        {
            return Err(Error::InvalidValue);
        }
        let mut nodes = vec![root];
        let (mut named, mut seen) = (Vec::new(), rustc_hash::FxHashSet::default());
        for at in 0..frames.len() {
            let node = nodes.pop().ok_or(Error::InvalidValue)?;
            if let Some(id) = &node.id {
                if !seen.insert(id.as_str()) {
                    return Err(Error::DuplicateKey(id.to_string()));
                }
                named.push((id.clone(), at as u32));
            }
            nodes.extend(node.children().iter().rev());
        }
        if !nodes.is_empty() {
            return Err(Error::InvalidValue);
        }
        self.named = Arc::new(named);
        self.order = Arc::new(frames);
        Ok(self)
    }
    /// Every named node's frame, in tree order.
    pub fn frames(&self) -> impl Iterator<Item = (&str, Frame)> {
        let order = &self.order;
        self.named
            .iter()
            .map(|(k, at)| (k.as_str(), order[*at as usize]))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    InvalidValue,
    DuplicateKey(String),
    BudgetExceeded,
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
            Self::RevisionExhausted => f.write_str("the layout revision counter overflowed"),
        }
    }
}
impl std::error::Error for Error {}

#[derive(Clone, Copy, Debug, PartialEq)]
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

/// `None` means hug intrinsic content. `Some` is an exact offered parent size.
/// Content leaves measure as empty; use [`resolve_with`] to size them.
pub fn resolve<P>(root: &Node<P>, offered: Option<Size>, limits: Limits) -> Result<Layout, Error> {
    resolve_with(root, offered, limits, SpacingScale::DEFAULT, |_, _| {
        Size::ZERO
    })
}

/// [`resolve`] with a spacing scale for tokens and a measurer for
/// `Node::content` leaves, called with its payload and the room it has: the
/// narrowest definite inner width above it, `None` under a hugging parent or
/// inside a scroll. A leaf in a squeezed, non-wrapping row is measured a
/// second time at the main size the row deals it; that last call is the
/// authoritative one. Text shaping lives outside this crate on purpose.
pub fn resolve_with<P, M: Into<Intrinsic>>(
    root: &Node<P>,
    offered: Option<Size>,
    limits: Limits,
    scale: SpacingScale,
    measurer: impl FnMut(&P, Option<f64>) -> M,
) -> Result<Layout, Error> {
    resolve_impl(root, offered, limits, scale, measurer, None, None)
}

/// [`resolve_with`] with the root's box given rather than declared: the root
/// is exactly `size`, padded by `padding`, whatever its own width, height,
/// aspect and padding say. This is how a subtree is laid out again inside a
/// region other geometry decided, without cloning it to restyle its root.
///
/// ```
/// use mui_layout::{col, resolve_boxed_with, row, Insets, Size};
/// let tree = col([row([]).grow(1.)]).size(500., 500.).pad(40.);
/// let size = Size::new(100., 60.);
/// let unpadded = Insets::ZERO;
/// let l = resolve_boxed_with(&tree, size, unpadded, Default::default(), Default::default(), |_, _| {
///     Size::ZERO
/// })
/// .unwrap();
/// assert_eq!(l.all()[1].size, size, "the child fills the given box");
/// ```
pub fn resolve_boxed_with<P, M: Into<Intrinsic>>(
    root: &Node<P>,
    size: Size,
    padding: Insets,
    limits: Limits,
    scale: SpacingScale,
    measurer: impl FnMut(&P, Option<f64>) -> M,
) -> Result<Layout, Error> {
    if !size.valid(limits.extent) || !padding.valid(limits.extent) {
        return Err(Error::InvalidValue);
    }
    resolve_impl(
        root,
        Some(size),
        limits,
        scale,
        measurer,
        None,
        Some(padding),
    )
}
fn resolve_impl<P, M: Into<Intrinsic>>(
    root: &Node<P>,
    offered: Option<Size>,
    limits: Limits,
    scale: SpacingScale,
    measurer: impl FnMut(&P, Option<f64>) -> M,
    cache: Option<&mut LayoutCache>,
    boxed: Option<Insets>,
) -> Result<Layout, Error> {
    if !limits.extent.is_finite()
        || limits.extent <= 0.0
        || limits.nodes == 0
        || limits.depth > 256
        || !scale.is_valid()
    {
        return Err(Error::InvalidValue);
    }
    let mut measurer = crate::measure::intrinsic(measurer);
    let definite = offered.map_or([None; 2], |s| [Some(s.width), Some(s.height)]);
    let mut pass = Pass {
        left: limits.nodes,
        limits,
        scale,
        // Filled only by a solve without the cache -- every one of a live
        // resize -- one id per keyed node: room for a window's worth at once.
        keys: rustc_hash::FxHashSet::with_capacity_and_hasher(
            limits.nodes.min(256),
            rustc_hash::FxBuildHasher,
        ),
        redo: false,
        pinned: false,
        measurer: &mut measurer,
        cache,
        boxed,
    };
    // The root's own offered size is the outermost container there is.
    let m = measure(root, "root", definite, None, definite, 0, &mut pass)?;
    let size = offered.unwrap_or(m.size);
    if !size.valid(limits.extent) {
        return Err(Error::InvalidValue);
    }
    // A root offered more than its maximum is its maximum, like any node.
    let size = root.rare().maximum.map_or(size, |max| {
        Size::new(size.width.min(max.width), size.height.min(max.height))
    });
    // Every measured node produces at most one frame.
    let mut out = (Vec::new(), Vec::with_capacity(limits.nodes - pass.left));
    let empty = BTreeMap::new();
    let pins = |anchors| Pins {
        anchors,
        root: size,
        scale,
    };
    arrange(&m, "root", [0.0, 0.0], size, &pins(&empty), None, &mut out)?;
    // ponytail: one extra arrange resolves every pin, because a float takes no
    // space and so cannot move an anchor. A pin whose anchor is itself inside a
    // pinned float reads that float's first-pass position; give the pass a
    // dependency order if that ever matters.
    if pass.pinned {
        let named = std::mem::take(&mut out.0);
        let anchors = named
            .into_iter()
            .map(|(id, at)| (id, out.1[at as usize]))
            .collect();
        out.1.clear();
        arrange(
            &m,
            "root",
            [0.0, 0.0],
            size,
            &pins(&anchors),
            None,
            &mut out,
        )?;
    }
    Ok(Layout {
        size,
        min: m.floor,
        named: Arc::new(out.0),
        order: Arc::new(out.1),
    })
}

#[cfg(test)]
mod tests;
