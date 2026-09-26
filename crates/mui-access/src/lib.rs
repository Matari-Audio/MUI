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

use accesskit::{
    Action, Affine, Node, NodeId, Rect, Role, TextDirection, TextPosition, TextSelection, Tree,
    TreeId, TreeUpdate,
};
pub use mui_scene::{Kind, Semantics};
use mui_scene::{ResolvedScene, ResolvedSurface};
use std::collections::HashMap;

/// FNV-1a: a node id that is the same on every frame for the same surface id.
pub fn node_id(key: &str) -> NodeId {
    fnv(key.bytes())
}

/// The id of text field `key`'s `TextRun` child: the key's hash continued
/// over a NUL-led suffix, so it never meets a surface's [`node_id`] in
/// practice.
pub fn run_id(key: &str) -> NodeId {
    fnv(key.bytes().chain(*b"\0run"))
}

fn fnv(bytes: impl Iterator<Item = u8>) -> NodeId {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x1000_0000_01b3);
    }
    // 0 is the window.
    NodeId(h | 1)
}

const WINDOW: NodeId = NodeId(0);

/// A text field's line as AccessKit reads text: one `TextRun` with each
/// character's byte length and, when the field measured them, its x and
/// advance in the field's space; the selection lands on the field.
///
/// ponytail: a character here is a `char`, which is what the runtime's
/// selection counts. A reader stepping by character can stop inside a
/// cluster the field's arrows step over; report graphemes if that matters.
fn text_run(
    field: &mut Node,
    s: &ResolvedSurface,
    value: &str,
    sel: (usize, usize),
    carets: &[f64],
) -> Node {
    let run = run_id(&s.key);
    let n = value.chars().count();
    let at = |i: usize| TextPosition {
        node: run,
        character_index: i.min(n),
    };
    field.push_child(run);
    field.set_text_selection(TextSelection {
        anchor: at(sel.0),
        focus: at(sel.1),
    });
    let mut t = Node::new(Role::TextRun);
    t.set_value(value);
    t.set_character_lengths(
        value
            .chars()
            .map(|c| c.len_utf8() as u8)
            .collect::<Vec<_>>(),
    );
    if carets.len() == n + 1 {
        t.set_character_positions(carets[..n].iter().map(|&x| x as f32).collect::<Vec<_>>());
        t.set_character_widths(
            carets
                .windows(2)
                .map(|w| (w[1] - w[0]) as f32)
                .collect::<Vec<_>>(),
        );
    }
    t.set_text_direction(TextDirection::LeftToRight);
    let f = s.frame;
    t.set_bounds(Rect::new(f.x, f.y, f.right(), f.bottom()));
    t
}

fn node(s: &ResolvedSurface, sem: Option<&Semantics>, runs: &mut Vec<(NodeId, Node)>) -> Node {
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
        Kind::Image => Role::Image,
    });
    match &sem.role {
        Kind::Button if !s.disabled => n.add_action(Action::Click),
        Kind::Slider { value, min, max } => {
            n.set_numeric_value(*value);
            n.set_min_numeric_value(*min);
            n.set_max_numeric_value(*max);
            // The arrow keys' step, a hundredth of the range, which is what
            // the runtime's `Increment` and `Decrement` take. Unsigned here.
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
        Kind::TextInput {
            value,
            selection,
            carets,
        } => {
            n.set_value(value.clone());
            runs.push((
                run_id(&s.key),
                text_run(&mut n, s, value, *selection, carets),
            ));
            if !s.disabled {
                n.add_action(Action::SetTextSelection);
            }
        }
        _ => {}
    }
    // A group goes unnamed rather than read out as `osc/3/gain`, but a
    // control with neither label nor text (`toggle`, `text_input`) keeps its
    // id: an unnamed switch is worse than a noisy one.
    let control = !matches!(
        sem.role,
        Kind::Label | Kind::Group | Kind::Scroll | Kind::Image
    );
    let name = sem.label.clone().or_else(|| s.text_value.clone());
    if let Some(name) = name.or_else(|| control.then(|| s.key.to_string())) {
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
/// A text input carries its line as a `TextRun` child, [`run_id`], with the
/// selection on the field, so a reader follows the caret.
pub fn tree_update(scene: &ResolvedScene, focus: Option<&str>, scale: f64) -> TreeUpdate {
    let named: Vec<&ResolvedSurface> = scene
        .surfaces()
        .filter(|s| !s.key.is_empty() && !s.key.starts_with('/'))
        .collect();

    let mut runs = Vec::new();
    let mut nodes: Vec<(NodeId, Node)> = named
        .iter()
        .map(|s| (node_id(&s.key), node(s, s.semantics.as_ref(), &mut runs)))
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
    nodes.extend(runs);

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
            .map_or(WINDOW, node_id),
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

    /// A reader follows the caret: the field's line is a `TextRun` with a
    /// byte length, an x and an advance per character, and the selection
    /// sits on the field in those characters.
    #[test]
    fn a_text_field_carries_its_run_and_selection() {
        let field = |disabled: bool| {
            leaf(80., 20.)
                .role(Kind::TextInput {
                    value: "aéc".into(),
                    selection: (3, 1),
                    carets: vec![8., 16., 26., 34.],
                })
                .focusable()
                .disabled(disabled)
                .id("name")
        };
        let scene = resolve_scene(&SceneSpec::new(row![field(false)].pad(10.))).unwrap();
        let u = tree_update(&scene, Some("name"), 1.0);
        let by = |id: NodeId| &u.nodes.iter().find(|(k, _)| *k == id).unwrap().1;
        let (input, run) = (by(node_id("name")), by(run_id("name")));
        assert_eq!(input.children(), [run_id("name")]);
        let sel = input.text_selection().unwrap();
        assert_eq!(
            (sel.anchor.node, sel.anchor.character_index),
            (run_id("name"), 3)
        );
        assert_eq!(
            (sel.focus.node, sel.focus.character_index),
            (run_id("name"), 1)
        );
        assert!(input.supports_action(Action::SetTextSelection));
        assert_eq!(run.role(), accesskit::Role::TextRun);
        assert_eq!(run.value(), Some("aéc"));
        assert_eq!(run.character_lengths(), [1, 2, 1]);
        assert_eq!(run.character_positions(), Some(&[8., 16., 26.][..]));
        assert_eq!(run.character_widths(), Some(&[8., 10., 8.][..]));
        assert_eq!(run.bounds(), input.bounds());

        let scene = resolve_scene(&SceneSpec::new(row![field(true)])).unwrap();
        let u = tree_update(&scene, None, 1.0);
        let (_, input) = u.nodes.iter().find(|(k, _)| *k == node_id("name")).unwrap();
        assert!(!input.supports_action(Action::SetTextSelection), "off");
    }

    #[test]
    fn an_unlabelled_control_is_named_by_its_id() {
        let root = row![
            leaf(40., 20.).role(Kind::Toggle { on: true }).id("bypass"),
            leaf(40., 20.)
                .role(Kind::TextInput {
                    value: "x".into(),
                    selection: (0, 0),
                    carets: Vec::new(),
                })
                .id("preset"),
        ];
        let scene = resolve_scene(&SceneSpec::new(root)).unwrap();
        let u = tree_update(&scene, None, 1.0);
        let label = |k: &str| {
            let (_, n) = u.nodes.iter().find(|(id, _)| *id == node_id(k)).unwrap();
            n.label().map(str::to_owned)
        };
        assert_eq!(label("bypass").as_deref(), Some("bypass"));
        assert_eq!(label("preset").as_deref(), Some("preset"));
    }
}
