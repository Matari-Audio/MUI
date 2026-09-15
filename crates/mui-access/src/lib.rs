//! A [`ResolvedScene`] becomes an AccessKit tree.
//!
//! The scene knows where every named surface is and whether it takes focus;
//! it does not know that one of them is a slider. So a node says what it is
//! with `.role(..)` and `.label(..)`, and the surface carries it here:
//!
//! ```
//! use mui_access::tree_update;
//! use mui_core::prelude::*;
//!
//! let ok = leaf(40., 20.).role(Kind::Button).label("OK").id("ok").focusable();
//! let scene = resolve_scene(&SceneSpec::new(ok)).unwrap();
//! let update = tree_update(&scene, Some("ok"));
//! assert_eq!(update.nodes.len(), 2); // window + button
//! ```
//!
//! Host side: on winit, keep an `accesskit_winit::Adapter` and build the
//! update lazily, so a frame costs nothing when no screen reader listens —
//! `adapter.update_if_active(|| tree_update(&scene, focus))`. A plugin
//! with no window of its own hands the same `TreeUpdate` to whatever wrapper
//! owns the host's platform adapter.
#![forbid(unsafe_code)]

pub use accesskit;

use accesskit::{Action, Node, NodeId, Rect, Role, Tree, TreeId, TreeUpdate};
pub use mui_core::{Kind, Semantics};
use mui_core::{ResolvedScene, ResolvedSurface};

/// FNV-1a: a node id that is the same on every frame for the same surface id.
pub fn node_id(key: &str) -> NodeId {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in key.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x1000_0000_01b3);
    }
    // 0 is the window.
    NodeId(h | 1)
}

const WINDOW: NodeId = NodeId(0);

fn contains(a: &ResolvedSurface, b: &ResolvedSurface) -> bool {
    let (a, b) = (a.frame, b.frame);
    a.x <= b.x && a.y <= b.y && a.right() >= b.right() && a.bottom() >= b.bottom()
}

fn node(s: &ResolvedSurface, sem: Option<&Semantics>) -> Node {
    let default = Semantics::new(Kind::Group);
    let sem = sem.unwrap_or(&default);
    let mut n = Node::new(match &sem.role {
        Kind::Button => Role::Button,
        Kind::Slider { .. } => Role::Slider,
        Kind::Toggle { .. } => Role::Switch,
        Kind::TextInput { .. } => Role::TextInput,
        Kind::Label => Role::Label,
        Kind::Group => Role::Group,
        Kind::Scroll => Role::ScrollView,
    });
    match &sem.role {
        Kind::Button => n.add_action(Action::Click),
        Kind::Slider { value, min, max } => {
            n.set_numeric_value(*value);
            n.set_min_numeric_value(*min);
            n.set_max_numeric_value(*max);
            n.add_action(Action::SetValue);
        }
        Kind::Toggle { on } => {
            n.set_toggled((*on).into());
            n.add_action(Action::Click);
        }
        Kind::TextInput { value } => n.set_value(value.clone()),
        _ => {}
    }
    n.set_label(sem.label.clone().unwrap_or_else(|| s.key.to_string()));
    let f = s.frame;
    n.set_bounds(Rect::new(f.x, f.y, f.right(), f.bottom()));
    if s.focusable {
        n.add_action(Action::Focus);
    }
    n
}

/// Every named surface (an id, not a `/0/2` tree path) becomes a node under
/// a window root.
///
/// ponytail: containment nesting — the nearest preceding surface whose frame
/// encloses this one is its parent. Replace with real tree paths when the
/// scene exposes a surface's ancestors.
pub fn tree_update(scene: &ResolvedScene, focus: Option<&str>) -> TreeUpdate {
    let named: Vec<&ResolvedSurface> = scene
        .surfaces()
        .filter(|s| !s.key.starts_with('/'))
        .collect();

    let mut nodes: Vec<(NodeId, Node)> = named
        .iter()
        .map(|s| (node_id(&s.key), node(s, s.semantics.as_ref())))
        .collect();

    let mut root_kids = Vec::new();
    for (i, s) in named.iter().enumerate() {
        let parent = named[..i].iter().rposition(|p| contains(p, s));
        match parent {
            Some(p) => nodes[p].1.push_child(node_id(&s.key)),
            None => root_kids.push(node_id(&s.key)),
        }
    }

    let mut window = Node::new(Role::Window);
    window.set_children(root_kids);
    nodes.push((WINDOW, window));

    TreeUpdate {
        nodes,
        tree: Some(Tree::new(WINDOW)),
        tree_id: TreeId::ROOT,
        focus: focus
            .filter(|k| scene.surface(k).is_some())
            .map(node_id)
            .unwrap_or(WINDOW),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mui_core::prelude::*;

    #[test]
    fn nests_by_containment_and_ids_are_stable() {
        let root = col![
            leaf(40., 20.).id("a").focusable(),
            row![leaf(30., 10.).id("b")].id("r"),
        ];
        let scene = resolve_scene(&SceneSpec::new(root)).unwrap();
        let u = tree_update(&scene, Some("a"));
        let by = |k: &str| {
            u.nodes
                .iter()
                .find(|(id, _)| *id == node_id(k))
                .map(|(_, n)| n)
                .unwrap()
        };
        assert_eq!(by("r").children(), [node_id("b")]);
        assert!(by("b").children().is_empty());
        assert!(by("a").supports_action(Action::Focus));
        assert_eq!(u.focus, node_id("a"));
        assert_eq!(by("a").label().unwrap(), "a");
        assert_eq!(tree_update(&scene, None).nodes[0].0, u.nodes[0].0);
    }
}
