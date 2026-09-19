use mui_access::{accesskit::Action, node_id, tree_update};
use mui_scene::prelude::*;

#[test]
fn overlapping_siblings_do_not_become_each_others_children() {
    let tree = stack![leaf(100.0, 100.0).id("back"), leaf(20.0, 20.0).id("front"),].id("root");
    let scene = resolve_scene(&SceneSpec::new(tree)).unwrap();
    let update = tree_update(&scene, None);
    let find = |id: &str| {
        &update
            .nodes
            .iter()
            .find(|(k, _)| *k == node_id(id))
            .unwrap()
            .1
    };
    assert!(find("back").children().is_empty());
    assert_eq!(
        find("root").children(),
        &[node_id("back"), node_id("front")]
    );
}

#[test]
fn float_keeps_authored_parent_while_escaping_the_clip() {
    let tree = stack![leaf(20.0, 20.0).id("popup").float().offset(150.0, 0.0),]
        .size(100.0, 100.0)
        .id("panel")
        .clip();
    let scene = resolve_scene(&SceneSpec::new(tree)).unwrap();
    let popup = scene.surface("popup").unwrap();
    assert_eq!(popup.parent.as_deref(), Some("panel"));
    assert!(popup.clip_paths().is_none_or(|p| p.is_empty()));
    let update = tree_update(&scene, None);
    let panel = &update
        .nodes
        .iter()
        .find(|(id, _)| *id == node_id("panel"))
        .unwrap()
        .1;
    assert_eq!(panel.children(), &[node_id("popup")]);
}

#[test]
fn disabled_controls_advertise_no_mutating_actions_or_focus() {
    let tree = leaf(100.0, 40.0)
        .id("gain")
        .focusable()
        .disabled(true)
        .role(Kind::Slider {
            value: 0.5,
            min: 0.0,
            max: 1.0,
        });
    let scene = resolve_scene(&SceneSpec::new(tree)).unwrap();
    let update = tree_update(&scene, Some("gain"));
    let gain = &update
        .nodes
        .iter()
        .find(|(id, _)| *id == node_id("gain"))
        .unwrap()
        .1;
    assert!(!gain.supports_action(Action::SetValue));
    assert!(!gain.supports_action(Action::Focus));
    assert_ne!(update.focus, node_id("gain"));
}

#[test]
fn text_is_an_accessible_label_unless_explicitly_overridden() {
    let tree = col![
        text("440 Hz").id("value"),
        text("880 Hz").label("Reference").id("explicit")
    ];
    let scene = resolve_scene(&SceneSpec::new(tree)).unwrap();
    let update = tree_update(&scene, None);
    let label = |id: &str| {
        update
            .nodes
            .iter()
            .find(|(k, _)| *k == node_id(id))
            .unwrap()
            .1
            .label()
            .unwrap()
    };
    assert_eq!(label("value"), "440 Hz");
    assert_eq!(label("explicit"), "Reference");
}
