//! A [`ResolvedScene`] becomes an AccessKit tree.
//!
//! The scene knows where every named surface is and whether it takes focus;
//! it does not know that one of them is a slider. So a node says what it is
//! with `.role(..)` and `.label(..)`, and the surface carries it here:
//!
//! ```
//! use mui_access::tree_update;
//! use mui_scene::prelude::*;
//!
//! let ok = leaf(40., 20.).role(Kind::Button).label("OK").id("ok").focusable();
//! let scene = resolve_scene(&SceneSpec::new(ok)).unwrap();
//! let update = tree_update(&scene, Some("ok"), 1.0);
//! assert_eq!(update.nodes.len(), 2); // window + button
//! ```
//!
//! Host side: on winit, keep an `accesskit_winit::Adapter` and build the
//! update lazily, so a frame costs nothing when no screen reader listens —
//! `adapter.update_if_active(|| tree_update(&scene, focus, scale))`, where
//! `scale` is the window's device pixels per scene unit. A plugin
//! with no window of its own hands the same `TreeUpdate` to whatever wrapper
//! owns the host's platform adapter.
#![forbid(unsafe_code)]

pub use accesskit;

use accesskit::{Action, Affine, Node, NodeId, Rect, Role, Tree, TreeId, TreeUpdate};
pub use mui_scene::{Kind, Semantics};
use mui_scene::{ResolvedScene, ResolvedSurface};
use std::collections::HashMap;

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

fn node(s: &ResolvedSurface, sem: Option<&Semantics>) -> Node {
    let default = Semantics::new(if s.text_value.is_some() {
        Kind::Label
    } else {
        Kind::Group
    });
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
        Kind::Button if !s.disabled => n.add_action(Action::Click),
        Kind::Slider { value, min, max } => {
            n.set_numeric_value(*value);
            n.set_min_numeric_value(*min);
            n.set_max_numeric_value(*max);
            // `mui::widgets::step`, the arrow keys' step, which is what the
            // runtime's `Increment` and `Decrement` take. Unsigned here.
            n.set_numeric_value_step(((max - min) / 100.0).abs());
            if !s.disabled {
                n.add_action(Action::SetValue);
                n.add_action(Action::Increment);
                n.add_action(Action::Decrement);
            }
        }
        Kind::Toggle { on } => {
            n.set_toggled((*on).into());
            if !s.disabled {
                n.add_action(Action::Click);
            }
        }
        Kind::TextInput { value } => n.set_value(value.clone()),
        _ => {}
    }
    // No name rather than the id: `osc/3/gain` read aloud is noise.
    if let Some(name) = sem.label.clone().or_else(|| s.text_value.clone()) {
        n.set_label(name);
    }
    let f = s.frame;
    n.set_bounds(Rect::new(f.x, f.y, f.right(), f.bottom()));
    if s.focusable && !s.disabled {
        n.add_action(Action::Focus);
    }
    // A screen reader is told the same thing the pointer is: this one is off.
    if s.disabled {
        n.set_disabled();
    }
    n
}

/// Every named surface (an id, not a `/0/2` tree path) becomes a node under
/// a window root. Bounds stay in scene units; the window carries `scale`,
/// the device pixels per unit, as its transform, which AccessKit applies to
/// every node under it.
///
/// Hierarchy follows authored semantic parentage, including floated and
/// overlapping elements. Geometry never determines ownership.
///
/// ponytail: a text input reports its value but no caret or selection.
/// AccessKit places those in `TextRun` children with per-character lengths
/// and positions, which needs the field's shaped run here; add them when the
/// scene carries a field's glyph run and selection.
pub fn tree_update(scene: &ResolvedScene, focus: Option<&str>, scale: f64) -> TreeUpdate {
    let named: Vec<&ResolvedSurface> = scene
        .surfaces()
        .filter(|s| !s.key.is_empty() && !s.key.starts_with('/'))
        .collect();

    let mut nodes: Vec<(NodeId, Node)> = named
        .iter()
        .map(|s| (node_id(&s.key), node(s, s.semantics.as_ref())))
        .collect();

    let indices: HashMap<&str, usize> = named
        .iter()
        .enumerate()
        .map(|(i, s)| (s.key.as_ref(), i))
        .collect();
    let mut root_kids = Vec::new();
    for s in &named {
        let parent = s.parent.as_deref().and_then(|p| indices.get(p)).copied();
        match parent {
            Some(p) => nodes[p].1.push_child(node_id(&s.key)),
            None => root_kids.push(node_id(&s.key)),
        }
    }

    let mut window = Node::new(Role::Window);
    window.set_children(root_kids);
    window.set_transform(Affine::scale(scale));
    nodes.push((WINDOW, window));

    TreeUpdate {
        nodes,
        tree: Some(Tree::new(WINDOW)),
        tree_id: TreeId::ROOT,
        focus: focus
            .filter(|k| {
                !k.is_empty()
                    && !k.starts_with('/')
                    && scene.surface(k).is_some_and(|s| s.focusable && !s.disabled)
            })
            .map(node_id)
            .unwrap_or(WINDOW),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mui_scene::prelude::*;

    #[test]
    fn nests_by_containment_and_ids_are_stable() {
        let root = col![
            leaf(40., 20.).id("a").focusable(),
            row![leaf(30., 10.).id("b")].id("r"),
        ];
        let scene = resolve_scene(&SceneSpec::new(root)).unwrap();
        let u = tree_update(&scene, Some("a"), 1.0);
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
        assert_eq!(by("a").label(), None, "no name, not the id");
        assert_eq!(tree_update(&scene, None, 1.0).nodes[0].0, u.nodes[0].0);
    }

    #[test]
    fn the_window_scales_to_device_pixels_and_a_slider_steps() {
        let fader = leaf(100., 20.)
            .role(Kind::Slider {
                value: 0.5,
                min: -24.,
                max: 6.,
            })
            .label("Gain")
            .id("gain");
        let scene = resolve_scene(&SceneSpec::new(row![fader].pad(10.))).unwrap();
        let u = tree_update(&scene, None, 2.0);
        let (_, window) = u.nodes.iter().find(|(id, _)| *id == WINDOW).unwrap();
        assert_eq!(window.transform(), Some(&Affine::scale(2.0)));
        let (_, gain) = u
            .nodes
            .iter()
            .find(|(id, _)| *id == node_id("gain"))
            .unwrap();
        assert_eq!(gain.bounds(), Some(Rect::new(10., 10., 110., 30.)));
        assert_eq!(gain.numeric_value_step(), Some(0.3));
        assert!(gain.supports_action(Action::Increment));
        assert!(gain.supports_action(Action::Decrement));
    }
}
