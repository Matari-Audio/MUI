use mui_access::{
    Publisher,
    accesskit::{self, Action, NodeId, Rect, TextDirection, TextPosition, TextSelection, TreeId},
    node_id, run_id, selection_is_upstream, selection_of, tree_update,
};
use mui_scene::{EditableText, ResolvedScene, prelude::*};

fn field(
    source: &str,
    shown: &str,
    selection: std::ops::Range<usize>,
    caret: usize,
    replacement: Option<std::ops::Range<usize>>,
    marked: std::ops::Range<usize>,
    width: f64,
) -> ResolvedScene {
    let mut field = text(shown)
        .size(width, 160.)
        .id("field")
        .focusable()
        .a11y(A11y::TextInput {
            value: source.into(),
            selection: (0, 0),
            carets: Vec::new(),
        });
    field.payload_mut().extras_mut().editable_text = Some(EditableText {
        value: source.into(),
        selection,
        caret,
        caret_upstream: false,
        previous_viewport: None,
        marked,
        replacement,
        multiline: true,
        caret_visible: true,
        scroll: 0.,
        follow_caret: false,
        insets: [8., 4.],
    });
    resolve(&SceneSpec::new(field).font(Font::new(epaint_default_fonts::HACK_REGULAR).unwrap()))
        .unwrap()
}

fn node(update: &accesskit::TreeUpdate, id: NodeId) -> &accesskit::Node {
    &update.nodes.iter().find(|(key, _)| *key == id).unwrap().1
}

#[test]
fn rtl_caret_geometry_is_positive_and_tracks_source_offsets() {
    let scene = field("אבג", "אבג", 0..0, 0, None, 0..0, 200.);
    let update = tree_update(&scene, Some("field"), 2.);
    let run = node(&update, run_id("field"));
    assert_eq!(run.text_direction(), Some(TextDirection::RightToLeft));
    assert!(
        run.character_widths()
            .unwrap()
            .iter()
            .all(|width| *width >= 0.)
    );
    assert!(
        run.character_positions()
            .unwrap()
            .windows(2)
            .all(|w| w[0] <= w[1])
    );
    let native = accesskit_consumer::Tree::new(update, true);
    let input = native
        .state()
        .node_by_tree_local_id(node_id("field"), TreeId::ROOT)
        .unwrap();
    let caret = input.text_selection().unwrap().bounding_boxes();
    assert_eq!(caret.len(), 1);
    let screen = native
        .state()
        .node_by_tree_local_id(run_id("field"), TreeId::ROOT)
        .unwrap()
        .bounding_box()
        .unwrap();
    assert!(
        (caret[0].x0 - screen.x1).abs() < 0.001,
        "RTL start caret must be on the right edge"
    );
    assert_eq!(
        selection_of(
            &scene,
            node_id("field"),
            &TextSelection {
                anchor: TextPosition {
                    node: run_id("field"),
                    character_index: 3
                },
                focus: TextPosition {
                    node: run_id("field"),
                    character_index: 1
                },
            }
        ),
        Some((3, 1))
    );
}

#[test]
fn multiline_mixed_direction_selection_and_trailing_newline_round_trip() {
    let source = "ab אב\ncd\n";
    let scene = field(source, source, 1..source.len(), 1, None, 0..0, 200.);
    let update = tree_update(&scene, Some("field"), 1.);
    let input = node(&update, node_id("field"));
    assert_eq!(input.role(), accesskit::Role::MultilineTextInput);
    assert!(input.children().len() >= 4);
    let text: String = input
        .children()
        .iter()
        .map(|id| node(&update, *id).value().unwrap())
        .collect();
    assert_eq!(text, source, "hard line breaks must not disappear");
    let selection = input.text_selection().unwrap();
    assert_ne!(selection.anchor.node, selection.focus.node);
    assert_eq!(
        selection_of(&scene, node_id("field"), selection),
        Some((source.chars().count(), 1))
    );
    let native = accesskit_consumer::Tree::new(update, true);
    let input = native
        .state()
        .node_by_tree_local_id(node_id("field"), TreeId::ROOT)
        .unwrap();
    assert_eq!(input.text_selection().unwrap().text(), source[1..]);
    let boxes = input.text_selection().unwrap().bounding_boxes();
    assert!(boxes.len() >= 3);
    assert!(boxes.iter().all(|b| b.x0 <= b.x1 && b.y0 < b.y1));
}

#[test]
fn wrapping_preedit_and_geometry_changes_republish_without_losing_source_mapping() {
    let source = "abécd";
    let shown = "ab😀cd";
    let scene = field(source, shown, 2..6, 6, Some(2..4), 2..6, 40.);
    let mut publisher = Publisher::default();
    let update = publisher.update(&scene, Some("field"), 1.);
    let input = node(&update, node_id("field"));
    assert_eq!(input.value(), Some(shown));
    assert_eq!(
        selection_of(&scene, node_id("field"), input.text_selection().unwrap()),
        Some((2, 3))
    );
    assert!(
        input.children().len() > 1,
        "text must wrap at the current field width"
    );
    assert!(publisher.update(&scene, Some("field"), 1.).nodes.is_empty());
    let wider = field(source, shown, 2..6, 6, Some(2..4), 2..6, 200.);
    assert!(!publisher.update(&wider, Some("field"), 1.).nodes.is_empty());
    let foreign = TextSelection {
        anchor: TextPosition {
            node: run_id("other"),
            character_index: 0,
        },
        focus: TextPosition {
            node: run_id("field"),
            character_index: 0,
        },
    };
    assert!(selection_of(&wider, node_id("field"), &foreign).is_none());
}

struct Changes;
impl accesskit_consumer::TreeChangeHandler for Changes {
    fn node_added(&mut self, _: &accesskit_consumer::Node) {}
    fn node_updated(&mut self, _: &accesskit_consumer::Node, _: &accesskit_consumer::Node) {}
    fn node_removed(&mut self, _: &accesskit_consumer::Node) {}
    fn focus_moved(
        &mut self,
        _: Option<&accesskit_consumer::Node>,
        _: Option<&accesskit_consumer::Node>,
    ) {
    }
}

#[test]
fn stable_ids_survive_focus_only_reorder_removal_and_reactivation() {
    let scene = |reverse: bool, remove: bool| {
        resolve(&SceneSpec::new(row(if remove {
            vec![block(10., 10.).id("b").focusable()]
        } else if reverse {
            vec![
                block(10., 10.).id("b").focusable(),
                block(10., 10.).id("a").focusable(),
            ]
        } else {
            vec![
                block(10., 10.).id("a").focusable(),
                block(10., 10.).id("b").focusable(),
            ]
        })))
        .unwrap()
    };
    let first = scene(false, false);
    let mut publisher = Publisher::default();
    let update = publisher.update(&first, Some("a"), 2.);
    assert_eq!(
        node(&update, NodeId(0)).bounds(),
        Some(Rect::new(0., 0., 20., 10.))
    );
    let mut native = accesskit_consumer::Tree::new(update, true);
    let focus = publisher.update(&first, Some("b"), 2.);
    assert!(
        focus.nodes.is_empty(),
        "focus alone must not rebuild the tree"
    );
    assert_eq!(focus.focus, node_id("b"));
    native.update_and_process_changes(focus, &mut Changes);
    native.update_and_process_changes(
        publisher.update(&scene(true, false), Some("b"), 2.),
        &mut Changes,
    );
    native.update_and_process_changes(
        publisher.update(&scene(false, true), Some("a"), 2.),
        &mut Changes,
    );
    assert!(
        native
            .state()
            .node_by_tree_local_id(node_id("a"), TreeId::ROOT)
            .is_none()
    );
    assert_eq!(
        native.state().focus_in_tree().data().role(),
        accesskit::Role::Window
    );
    publisher.reset();
    assert!(!publisher.update(&first, Some("a"), 2.).nodes.is_empty());
    let invalid = tree_update(&first, None, f64::NAN);
    assert_eq!(
        node(&invalid, NodeId(0)).transform(),
        Some(&accesskit::Affine::scale(1.))
    );
    assert!(node(&invalid, node_id("a")).supports_action(Action::Focus));
}

#[test]
fn soft_wrap_upstream_caret_keeps_its_visual_line() {
    let source = "abcdef";
    let state = |upstream| EditableText {
        value: source.into(),
        selection: 2..2,
        caret: 2,
        caret_upstream: upstream,
        marked: 0..0,
        replacement: None,
        multiline: true,
        caret_visible: true,
        scroll: 0.,
        follow_caret: false,
        insets: [8., 4.],
        previous_viewport: None,
    };
    let scene = |upstream| {
        let mut el = text(source)
            .size(40., 160.)
            .focusable()
            .id("field")
            .a11y(A11y::TextInput {
                value: source.into(),
                selection: (2, 2),
                carets: Vec::new(),
            });
        el.payload_mut().extras_mut().editable_text = Some(state(upstream));
        resolve(&SceneSpec::new(el).font(Font::new(epaint_default_fonts::HACK_REGULAR).unwrap()))
            .unwrap()
    };
    let upstream = scene(true);
    let geometry = upstream
        .surface("field")
        .unwrap()
        .text_geometry
        .as_ref()
        .unwrap();
    assert_eq!(
        geometry.lines[0].range.end, 2,
        "fixture must place caret at a soft wrap"
    );
    let update = tree_update(&upstream, Some("field"), 1.);
    assert_eq!(
        node(&update, node_id("field"))
            .text_selection()
            .unwrap()
            .focus
            .node,
        run_id("field")
    );
    assert_eq!(
        selection_is_upstream(
            &upstream,
            node_id("field"),
            &node(&update, node_id("field"))
                .text_selection()
                .unwrap()
                .focus
        ),
        Some(true)
    );
    let downstream_scene = scene(false);
    let downstream = tree_update(&downstream_scene, Some("field"), 1.);
    assert_eq!(
        selection_is_upstream(
            &downstream_scene,
            node_id("field"),
            &node(&downstream, node_id("field"))
                .text_selection()
                .unwrap()
                .focus
        ),
        Some(false)
    );
    assert_ne!(
        node(&downstream, node_id("field"))
            .text_selection()
            .unwrap()
            .focus
            .node,
        run_id("field")
    );
}
