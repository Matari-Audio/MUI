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

use std::collections::BTreeMap;

mod incremental;
pub use incremental::{resolve_cached_with, LayoutCache, LayoutStats};
mod arrange;
mod id;
mod len;
mod measure;
mod node;
mod pin;

pub use id::Id;
pub use len::{Align, Insets, Justify, Len, Size};
pub use mui_geometry::{Spacing, SpacingScale, SpacingToken};
pub use node::{column, fits, grid, leaf, overlay, row, Node};
pub use pin::{Area, Match, Pin};

pub(crate) use arrange::{arrange, distribute};
pub(crate) use measure::{
    cell_default, grid_rows, label, measure, place, wrap_lines, Measured, Pass,
};
pub(crate) use node::Kind;
pub(crate) use pin::{inside, Pins, Viewport};

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
    frames: BTreeMap<Id, Frame>,
    /// Every node's frame, in tree order (parent first, then children in
    /// declaration order). A walk of the same tree indexes straight into it,
    /// so nothing needs a name to be found.
    order: Vec<Frame>,
}
impl Layout {
    /// The smallest this tree can be squeezed to: every `minimum`, padding and
    /// unsqueezable leaf in it, summed along the axis it sits on. A scroll
    /// node contributes nothing on its scrolling axis, which is the point of
    /// one. A host that owns a window refuses anything smaller; below it
    /// [`resolve`] answers [`Error::InsufficientSpace`].
    ///
    /// ```
    /// use mui_layout::{column, leaf, resolve, Size};
    /// let fixed = leaf(40., 30.).min_size(Size::new(40., 30.));
    /// let tree = column([fixed, leaf(40., 30.)]).pad(8.);
    /// let l = resolve(&tree, Some(Size::new(400., 300.)), Default::default()).unwrap();
    /// // The second leaf states no minimum, so it may be squeezed to nothing.
    /// assert_eq!(l.min_size(), Size::new(56., 46.));
    /// ```
    pub fn min_size(&self) -> Size {
        self.min
    }
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
    /// `needs` is the whole tree's floor, so a host can work out the uniform
    /// scale that would make it fit: `min(offered / needs)`. A node refused by
    /// its own `maximum` carries what that node asked for instead -- the
    /// tree's floor is not known until the measure pass it failed in ends.
    InsufficientSpace {
        node: String,
        needs: Size,
    },
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
            Self::InsufficientSpace { node, needs } => write!(
                f,
                "node {node} does not fit in the space offered; the tree needs {}x{}",
                needs.width, needs.height
            ),
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
pub fn resolve_with<P>(
    root: &Node<P>, offered: Option<Size>, limits: Limits, scale: SpacingScale,
    measurer: impl FnMut(&P, Option<f64>) -> Size,
) -> Result<Layout, Error> {
    resolve_impl(root, offered, limits, scale, measurer, None)
}
fn resolve_impl<P>(
    root: &Node<P>, offered: Option<Size>, limits: Limits, scale: SpacingScale,
    mut measurer: impl FnMut(&P, Option<f64>) -> Size,
    cache: Option<&mut LayoutCache>,
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
        redo: false,
        pinned: false,
        measurer: &mut measurer,
        cache,
    };
    // The root's own offered size is the outermost container there is.
    let m = measure(root, "root", definite, None, definite, 0, &mut pass)?;
    let size = offered.unwrap_or(m.size);
    if !size.valid(limits.extent) {
        return Err(Error::InvalidValue);
    }
    if root
        .maximum
        .is_some_and(|max| size.width > max.width || size.height > max.height)
    {
        return Err(Error::InsufficientSpace {
            node: label(root, "root"),
            needs: m.floor,
        });
    }
    // An upper bound, not a node count: a re-measured item spends budget
    // twice and produces one frame.
    let mut out = (
        BTreeMap::new(),
        Vec::with_capacity(limits.nodes - pass.left),
    );
    // Only `resolve` knows the whole tree's floor, and that is the number a
    // host scales by; the sites that raise the error only know their own node.
    let fix = |e| match e {
        Error::InsufficientSpace { node, .. } => Error::InsufficientSpace {
            node,
            needs: m.floor,
        },
        e => e,
    };
    let empty = BTreeMap::new();
    let memo = pass.cache.as_deref().filter(|_| !pass.pinned).map(|c| &c.arrangement);
    let pins = |anchors| Pins { anchors, root: size, scale, memo };
    arrange(&m, "root", [0.0, 0.0], size, &pins(&empty), None, &mut out).map_err(fix)?;
    // ponytail: one extra arrange resolves every pin, because a float takes no
    // space and so cannot move an anchor. A pin whose anchor is itself inside a
    // pinned float reads that float's first-pass position; give the pass a
    // dependency order if that ever matters.
    if pass.pinned {
        let anchors = std::mem::take(&mut out.0);
        out.1.clear();
        arrange(
            &m,
            "root",
            [0.0, 0.0],
            size,
            &pins(&anchors),
            None,
            &mut out,
        )
        .map_err(fix)?;
    }
    Ok(Layout {
        size,
        min: m.floor,
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
