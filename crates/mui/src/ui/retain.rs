//! Automatic retention. Every frame's tree is compared with the one before,
//! and each subtree declared as it was, at rest, is laid out and painted from
//! last frame's work instead of again. There is no `Ui::memo` and there are
//! no deps: the build still runs (it is cheap), and what it built is the
//! comparison.
//!
//! The runtime keeps last frame's whole resolved tree and walks the new one
//! beside it. Children pair by position, or by key when a list changed
//! length. A *boundary* is any node below the root that has children, at
//! least [`MIN_NODES`] nodes in its subtree and no float in it. It carries a
//! memo mark, so layout replays it and the walk copies its paint. Children
//! are judged first, so a changed boundary still reuses every unchanged one
//! inside it: a knob turning re-walks its row, not the track around it, and
//! not the other knobs.
//!
//! A boundary is reused when:
//! - every node in it declares what last frame's did
//!   ([`Element::same_declaration`]: closures by identity or, for a state
//!   look, by what it makes of the resting style; a keyed canvas by its key),
//! - every boundary inside it is reused, and
//! - no runtime state in it moves this frame: hover, press, focus, a spring,
//!   glide or scroll in flight, a `set_scroll`, or a theme, font or scale
//!   change.
//!
//! Last frame's tree is the runtime's own (hover and states applied,
//! springs stepped, scroll offsets written). A node whose look the runtime
//! changed therefore never compares equal, and reuse never skips something
//! the sweep would apply.
use super::*;
use mui_scene::Element;
use rustc_hash::{FxHashMap, FxHashSet, FxHasher};
use std::hash::{Hash, Hasher};

/// The fewest nodes a subtree needs to be worth a memo of its own.
const MIN_NODES: usize = 4;

thread_local! {
    /// Each `Ui`'s last resolved tree, by [`Ui::me`], with the frame that
    /// judged it. Like [`TREES`](super::memo::TREES), it holds an `El`,
    /// which is not `Send`, so it stays on the thread that built it; a `Ui`
    /// that frames elsewhere and comes back finds it stale by its frame.
    pub(super) static PREVIOUS: std::cell::RefCell<HashMap<u64, (u64, El)>> =
        std::cell::RefCell::new(HashMap::new());
}

/// One automatic boundary, between frames.
#[derive(Clone, Copy)]
pub(super) struct Boundary {
    /// The memo number the scene knows it by.
    number: u64,
    /// The frame it was last met in.
    seen: u64,
}

/// What judging a subtree found.
struct Judged {
    nodes: usize,
    floats: bool,
    /// Declared as last frame's, and reused wherever it is a boundary.
    same: bool,
}

impl Ui {
    /// Mark every boundary in `root` reused or rebuilt. `was` holds the
    /// hovered, held and tagged keys from before this frame's input.
    pub(super) fn retain_auto(&mut self, root: &mut El, was: &[Option<Id>; 3]) {
        let mut previous = PREVIOUS.with(|p| p.borrow_mut().remove(&self.me));
        // A `Ui::memo` root is the caller's own: last frame holds only its
        // placeholder, and a reused one is replayed whole.
        if self.auto_retain && root.payload().extras().memo.is_none() {
            let dirty = self.dirty_memos(was);
            let fresh = !self.auto_cold;
            let frames = self.frames;
            let mut last = (previous.as_mut())
                .filter(|(frame, _)| fresh && *frame == frames)
                .map(|(_, tree)| tree);
            self.frames += 1;
            let pairs = Pairs::new(
                root.children(),
                last.as_deref().map_or(&[][..], El::children),
            );
            for (j, c) in root.children_mut().iter_mut().enumerate() {
                let k = (last.as_deref_mut()).and_then(|p| pairs.get(j, c.key(), p.children_mut()));
                let at = c.key().map_or_else(|| slot(0, j), keyed);
                self.judge(c, k, at, &dirty);
            }
            let frame = self.frames;
            self.boundaries.retain(|_, b| b.seen == frame);
        }
        self.auto_cold = false;
        self.touched.clear();
        // Last frame's tree may hold closures: drop it outside the table.
        if let Some((_, previous)) = previous {
            Self::retire_tree(previous);
        }
    }

    /// Keep `root` to judge the next frame against: this frame's resolved
    /// tree, with each kept `Ui::memo` subtree taken out.
    pub(super) fn keep_previous(&mut self, root: El) {
        // A `Ui::memo` root is never judged (see `retain_auto`): keeping its
        // placeholder would only hold the caller's closures a frame longer.
        let old = if self.auto_retain && root.payload().extras().memo.is_none() {
            let kept = (self.frames, root);
            PREVIOUS
                .with(|p| p.borrow_mut().insert(self.me, kept))
                .map(|(_, t)| t)
        } else {
            Some(root)
        };
        if let Some(old) = old {
            Self::retire_tree(old);
        }
    }

    /// The memos holding a key whose runtime state moves: last frame's
    /// [`Ui::heat`], plus whatever stirred since.
    fn dirty_memos(&self, was: &[Option<Id>; 3]) -> FxHashSet<u64> {
        let mut dirty: FxHashSet<u64> = self.hot.iter().copied().collect();
        let Some(scene) = self.scene.as_deref() else {
            return dirty;
        };
        for k in self.stirred(was) {
            dirty.extend(scene.memos_at(k).map(|(id, _)| id));
        }
        dirty
    }

    /// Judge fresh `f` against `k`, last frame's node in its place: first
    /// its children, then `f` itself if it is a boundary. `at` is `f`'s
    /// identity across frames: its key, or its slot under its parent. A
    /// reused boundary trades places with `k`, so the frame carries last
    /// frame's styled subtree, scroll offsets and all, as a reused
    /// `Ui::memo` does.
    fn judge(&mut self, f: &mut El, k: Option<&mut El>, at: u64, dirty: &FxHashSet<u64>) -> Judged {
        let memo = f.payload().extras().memo;
        if let Some(m) = memo.filter(|m| m.reused) {
            // A kept `Ui::memo` subtree, spliced in whole. Last frame's tree
            // holds its placeholder.
            return Judged {
                nodes: MIN_NODES,
                floats: self.kept.get(&m.id).is_some_and(|k| k.layout_floats),
                same: k
                    .and_then(|k| k.payload().extras().memo)
                    .is_some_and(|p| p.id == m.id),
            };
        }
        // A fresh `Ui::memo` root is the caller's boundary, not ours. Last
        // frame holds only its placeholder, so nothing below it pairs up.
        let user = memo.is_some();
        let mut k = k.filter(|_| !user);
        let mut judged = Judged {
            nodes: 1,
            floats: f.is_float(),
            same: k.as_deref().is_some_and(|k| {
                let declared = k.payload().extras().declared.as_deref();
                f.children().len() == k.children().len()
                    && f.same_shell(declared.unwrap_or(k), Element::same_declaration)
            }),
        };
        let pairs = Pairs::new(f.children(), k.as_deref().map_or(&[][..], El::children));
        for (j, c) in f.children_mut().iter_mut().enumerate() {
            let child_at = c.key().map_or_else(|| slot(at, j), keyed);
            let pair = (k.as_deref_mut()).and_then(|k| pairs.get(j, c.key(), k.children_mut()));
            let child = self.judge(c, pair, child_at, dirty);
            judged.nodes = judged.nodes.saturating_add(child.nodes);
            judged.floats |= child.floats;
            judged.same &= child.same;
        }
        if user || judged.floats || judged.nodes < MIN_NODES || f.children().is_empty() {
            return judged;
        }
        let Some(number) = self.boundary(at) else {
            return judged;
        };
        judged.same &= !dirty.contains(&number);
        if let Some(k) = k.filter(|_| judged.same) {
            std::mem::swap(f, k);
        }
        f.payload_mut().extras_mut().memo = Some(mui_scene::Memo {
            id: number,
            reused: judged.same,
        });
        judged
    }

    /// The memo number for the boundary at `at`: last frame's, or a new
    /// one. `None` if another boundary claimed `at` this frame (a key used
    /// twice): that one keeps the mark, and this subtree is not memoised.
    fn boundary(&mut self, at: u64) -> Option<u64> {
        let frame = self.frames;
        if let Some(b) = self.boundaries.get_mut(&at) {
            if b.seen == frame {
                return None;
            }
            b.seen = frame;
            return Some(b.number);
        }
        self.next_memo += 1;
        let number = self.next_memo;
        self.boundaries.insert(
            at,
            Boundary {
                number,
                seen: frame,
            },
        );
        Some(number)
    }
}

/// Which of last frame's children each fresh child pairs with: the one in
/// its place, or, when the list changed length, the one with its key. A
/// pair always has the child's key, or none on both sides.
enum Pairs {
    /// Same length: child `j` pairs with `j`, keys permitting.
    InPlace,
    /// Changed length: by key, and nothing for an unkeyed child.
    Keyed(Vec<Option<usize>>),
}

impl Pairs {
    fn new(fresh: &[El], kids: &[El]) -> Self {
        if kids.len() == fresh.len() {
            return Self::InPlace;
        }
        let by_key: FxHashMap<&str, usize> = (kids.iter().enumerate())
            .filter_map(|(i, c)| Some((c.key()?, i)))
            .collect();
        Self::Keyed(
            fresh
                .iter()
                .map(|c| by_key.get(c.key()?).copied())
                .collect(),
        )
    }

    /// Last frame's child for fresh child `j`, which has `key`.
    fn get<'a>(&self, j: usize, key: Option<&str>, kids: &'a mut [El]) -> Option<&'a mut El> {
        let i = match self {
            Self::InPlace => j,
            Self::Keyed(at) => at[j]?,
        };
        kids.get_mut(i).filter(|k| k.key() == key)
    }
}

/// The identity of the `j`th child under `parent`.
fn slot(parent: u64, j: usize) -> u64 {
    let mut h = FxHasher::default();
    (parent, j).hash(&mut h);
    h.finish()
}

/// The identity of a keyed node, wherever it sits.
fn keyed(key: &str) -> u64 {
    let mut h = FxHasher::default();
    (u8::MAX, key).hash(&mut h);
    h.finish()
}
