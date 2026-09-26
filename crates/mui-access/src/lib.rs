//! A [`ResolvedScene`] becomes an AccessKit tree.
//!
//! The scene knows where every named surface is and whether it takes focus;
//! it does not know that one of them is a slider. So a node says what it is
//! with `.a11y(..)` and `.named(..)`, and the surface carries it here:
//!
//! ```
//! use mui_access::tree_update;
//! use mui_scene::prelude::*;
//!
//! let ok = block(40., 20.).a11y(A11y::Button).named("OK").id("ok").focusable();
//! let scene = resolve(&SceneSpec::new(ok)).unwrap();
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
pub use mui_scene::{A11y, Semantics};
use mui_scene::{ResolvedScene, ResolvedSurface};
use std::collections::{HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};

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
        A11y::Label
    } else {
        A11y::Group
    });
    let sem = sem.unwrap_or(&default);
    let mut n = Node::new(match &sem.role {
        A11y::Button => Role::Button,
        A11y::Slider { .. } => Role::Slider,
        A11y::Toggle { .. } => Role::Switch,
        A11y::TextInput { .. } => Role::TextInput,
        A11y::Label => Role::Label,
        A11y::Group => Role::Group,
        A11y::Scroll => Role::ScrollView,
        A11y::Image => Role::Image,
    });
    match &sem.role {
        A11y::Button if !s.disabled => n.add_action(Action::Click),
        A11y::Slider { value, min, max } => {
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
        A11y::Toggle { on } => {
            n.set_toggled((*on).into());
            if !s.disabled {
                n.add_action(Action::Click);
            }
        }
        A11y::TextInput {
            value,
            selection,
            carets,
        } => {
            n.set_value(&**value);
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
        A11y::Label | A11y::Group | A11y::Scroll | A11y::Image
    );
    let name = sem.label.as_deref().or(s.text_value.as_deref());
    if let Some(name) = name.or_else(|| control.then(|| s.key.as_str())) {
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
    build(scene, focus, scale).0
}

/// The surfaces that become nodes: named ones, in scene order.
fn named(scene: &ResolvedScene) -> Vec<&ResolvedSurface> {
    scene
        .surfaces()
        .filter(|s| mui_scene::Id::is_named(&s.key))
        .collect()
}

/// Each named surface's node id: [`node_id`] of its key, unless two keys
/// hash alike. Never seen at 63 bits, so a debug build says so and a release
/// build salts the second id; focus and action targets go through these
/// ids ([`surface_of`]), so the salted node is still reachable.
fn ids(named: &[&ResolvedSurface]) -> Vec<NodeId> {
    let mut seen = HashSet::with_capacity(named.len());
    named
        .iter()
        .map(|s| {
            let mut id = node_id(&s.key);
            let mut salt = 0u32;
            while !seen.insert(id) {
                debug_assert!(salt > 0, "node id collision at {:?}", s.key);
                salt += 1;
                id = fnv(s.key.bytes().chain(*b"\0dup").chain(salt.to_le_bytes()));
            }
            id
        })
        .collect()
}

/// The surface behind accesskit node `target`, for routing a reader's
/// action back to the scene. It follows the same (salted) ids a
/// [`tree_update`] of this scene gave out.
pub fn surface_of(scene: &ResolvedScene, target: NodeId) -> Option<&ResolvedSurface> {
    let named = named(scene);
    let i = ids(&named).iter().position(|&id| id == target)?;
    Some(named[i])
}

/// [`tree_update`], and the ids that were salted, by key.
fn build(
    scene: &ResolvedScene,
    focus: Option<&str>,
    scale: f64,
) -> (TreeUpdate, Vec<(Box<str>, NodeId)>) {
    let named = named(scene);
    let ids = ids(&named);
    let mut runs = Vec::new();
    let mut nodes: Vec<(NodeId, Node)> = named
        .iter()
        .zip(&ids)
        .map(|(s, &id)| (id, node(s, s.semantics.as_ref(), &mut runs)))
        .collect();

    let indices: HashMap<&str, usize> = named
        .iter()
        .enumerate()
        .map(|(i, s)| (s.key.as_ref(), i))
        .collect();
    let mut root_kids = Vec::new();
    for (s, &id) in named.iter().zip(&ids) {
        let parent = s.parent.as_deref().and_then(|p| indices.get(p)).copied();
        match parent {
            Some(p) => nodes[p].1.push_child(id),
            None => root_kids.push(id),
        }
    }

    let mut window = Node::new(Role::Window);
    window.set_children(root_kids);
    window.set_transform(Affine::scale(scale));
    nodes.push((WINDOW, window));
    nodes.extend(runs);

    let salted: Vec<(Box<str>, NodeId)> = named
        .iter()
        .zip(&ids)
        .filter(|(s, id)| node_id(&s.key) != **id)
        .map(|(s, &id)| (Box::from(s.key.as_str()), id))
        .collect();
    let update = TreeUpdate {
        nodes,
        tree: Some(Tree::new(WINDOW)),
        tree_id: TreeId::ROOT,
        focus: tree_update_focus(scene, focus, &salted),
    };
    (update, salted)
}

/// The focused surface's node, through `salted` for a key whose id was.
fn tree_update_focus(
    scene: &ResolvedScene,
    focus: Option<&str>,
    salted: &[(Box<str>, NodeId)],
) -> NodeId {
    focus
        .filter(|k| {
            mui_scene::Id::is_named(k)
                && scene.surface(k).is_some_and(|s| s.focusable && !s.disabled)
        })
        .map_or(WINDOW, |k| {
            salted
                .iter()
                .find(|(key, _)| **key == *k)
                .map_or_else(|| node_id(k), |&(_, id)| id)
        })
}

/// [`tree_update`] for a host that publishes every frame: a frame whose tree
/// would come out the same as the last one sent gets an empty update (the
/// focus only), so a screen reader costs a hash per frame, not a tree.
#[derive(Debug, Default)]
pub struct Publisher {
    last: Option<u64>,
    /// The last tree's salted ids (almost always none): an unchanged tree
    /// still focuses through them.
    salted: Vec<(Box<str>, NodeId)>,
}

impl Publisher {
    /// This frame's update: the whole tree, or nothing new.
    ///
    /// ```
    /// use mui_access::Publisher;
    /// use mui_scene::prelude::*;
    ///
    /// let scene = resolve(&SceneSpec::new(block(4., 4.).id("a"))).unwrap();
    /// let mut p = Publisher::default();
    /// assert_eq!(p.update(&scene, None, 1.0).nodes.len(), 2);
    /// assert!(p.update(&scene, None, 1.0).nodes.is_empty());
    /// p.reset();
    /// assert_eq!(p.update(&scene, None, 1.0).nodes.len(), 2);
    /// ```
    pub fn update(&mut self, scene: &ResolvedScene, focus: Option<&str>, scale: f64) -> TreeUpdate {
        let hash = tree_hash(scene, focus, scale);
        if self.last.replace(hash) == Some(hash) {
            return TreeUpdate {
                nodes: Vec::new(),
                tree: None,
                tree_id: TreeId::ROOT,
                focus: tree_update_focus(scene, focus, &self.salted),
            };
        }
        let (update, salted) = build(scene, focus, scale);
        self.salted = salted;
        update
    }

    /// Forget what was sent: the next update is whole. Call it when a
    /// reader (re)activates and asks for the initial tree.
    pub fn reset(&mut self) {
        self.last = None;
    }
}

/// Everything [`tree_update`] reads, hashed without building a node: the
/// named surfaces only (an unnamed meter ticking is no tree change), and
/// only the fields a node is built from.
fn tree_hash(scene: &ResolvedScene, focus: Option<&str>, scale: f64) -> u64 {
    let mut h = DefaultHasher::new();
    focus.hash(&mut h);
    scale.to_bits().hash(&mut h);
    for s in scene.surfaces().filter(|s| mui_scene::Id::is_named(&s.key)) {
        s.key.hash(&mut h);
        let f = s.frame;
        for v in [f.x, f.y, f.size.width, f.size.height] {
            v.to_bits().hash(&mut h);
        }
        s.parent.as_deref().hash(&mut h);
        (s.focusable, s.disabled).hash(&mut h);
        s.text_value.as_deref().hash(&mut h);
        let Some(sem) = &s.semantics else {
            0u8.hash(&mut h);
            continue;
        };
        sem.label.as_deref().hash(&mut h);
        match &sem.role {
            A11y::Slider { value, min, max } => {
                1u8.hash(&mut h);
                for v in [value, min, max] {
                    v.to_bits().hash(&mut h);
                }
            }
            A11y::Toggle { on } => (2u8, on).hash(&mut h),
            A11y::TextInput {
                value,
                selection,
                carets,
            } => {
                3u8.hash(&mut h);
                (&**value, selection).hash(&mut h);
                carets.iter().for_each(|c| c.to_bits().hash(&mut h));
            }
            A11y::Button => 4u8.hash(&mut h),
            A11y::Label => 5u8.hash(&mut h),
            A11y::Group => 6u8.hash(&mut h),
            A11y::Scroll => 7u8.hash(&mut h),
            A11y::Image => 8u8.hash(&mut h),
        }
    }
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mui_scene::prelude::*;

    #[test]
    fn an_unnamed_surface_changing_sends_no_tree() {
        let at = |w: f64| {
            let tree = row([
                block(4., 4.).id("a"),
                block(w, 4.).fill(mui_scene::Role::Primary),
            ]);
            resolve(&SceneSpec::new(tree)).unwrap()
        };
        assert!(at(9.).surfaces().any(|s| !mui_scene::Id::is_named(&s.key)));
        let mut p = Publisher::default();
        assert!(!p.update(&at(4.), None, 1.0).nodes.is_empty());
        assert!(
            p.update(&at(9.), None, 1.0).nodes.is_empty(),
            "a meter tick resent the tree"
        );
    }

    #[test]
    fn an_action_target_resolves_to_its_surface() {
        let scene = resolve(&SceneSpec::new(row([
            block(4., 4.).id("a"),
            block(4., 4.).id("b"),
        ])))
        .unwrap();
        assert_eq!(
            surface_of(&scene, node_id("b")).map(|s| s.key.as_str()),
            Some("b")
        );
        assert!(surface_of(&scene, WINDOW).is_none());
    }

    #[test]
    fn nests_by_containment_and_ids_are_stable() {
        let root = col![
            block(40., 20.).id("a").focusable(),
            row![block(30., 10.).id("b")].id("r"),
        ];
        let scene = resolve(&SceneSpec::new(root)).unwrap();
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
        let fader = block(100., 20.)
            .a11y(A11y::Slider {
                value: 0.5,
                min: -24.,
                max: 6.,
            })
            .named("Gain")
            .id("gain");
        let scene = resolve(&SceneSpec::new(row![fader].pad(10.))).unwrap();
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
            block(80., 20.)
                .a11y(A11y::TextInput {
                    value: "aéc".into(),
                    selection: (3, 1),
                    carets: vec![8., 16., 26., 34.],
                })
                .focusable()
                .when(disabled, Styled::disabled)
                .id("name")
        };
        let scene = resolve(&SceneSpec::new(row![field(false)].pad(10.))).unwrap();
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

        let scene = resolve(&SceneSpec::new(row![field(true)])).unwrap();
        let u = tree_update(&scene, None, 1.0);
        let (_, input) = u.nodes.iter().find(|(k, _)| *k == node_id("name")).unwrap();
        assert!(!input.supports_action(Action::SetTextSelection), "off");
    }

    #[test]
    fn an_unlabelled_control_is_named_by_its_id() {
        let root = row![
            block(40., 20.).a11y(A11y::Toggle { on: true }).id("bypass"),
            block(40., 20.)
                .a11y(A11y::TextInput {
                    value: "x".into(),
                    selection: (0, 0),
                    carets: Vec::new(),
                })
                .id("preset"),
        ];
        let scene = resolve(&SceneSpec::new(root)).unwrap();
        let u = tree_update(&scene, None, 1.0);
        let label = |k: &str| {
            let (_, n) = u.nodes.iter().find(|(id, _)| *id == node_id(k)).unwrap();
            n.label().map(str::to_owned)
        };
        assert_eq!(label("bypass").as_deref(), Some("bypass"));
        assert_eq!(label("preset").as_deref(), Some("preset"));
    }
}
