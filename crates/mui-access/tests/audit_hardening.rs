use mui_access::{accesskit::Action, node_id, tree_update};
use mui_scene::prelude::*;

#[test]
fn overlapping_siblings_do_not_become_each_others_children() {
    let tree = stack![block(100.0, 100.0).id("back"), block(20.0, 20.0).id("front"),].id("root");
    let scene = resolve(&SceneSpec::new(tree)).unwrap();
    let update = tree_update(&scene, None, 1.0);
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
    let tree = stack![block(20.0, 20.0).id("popup").float().offset(150.0, 0.0),]
        .size(100.0, 100.0)
        .id("panel")
        .clip();
    let scene = resolve(&SceneSpec::new(tree)).unwrap();
    let popup = scene.surface("popup").unwrap();
    assert_eq!(popup.parent.as_deref(), Some("panel"));
    assert!(popup.clip_paths().is_none_or(
        <[(
            std::sync::Arc<mui_scene::prelude::Path>,
            mui_scene::prelude::Point
        )]>::is_empty
    ));
    let update = tree_update(&scene, None, 1.0);
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
    let tree = block(100.0, 40.0)
        .id("gain")
        .focusable()
        .disabled()
        .a11y(A11y::Slider {
            value: 0.5,
            min: 0.0,
            max: 1.0,
        });
    let scene = resolve(&SceneSpec::new(tree)).unwrap();
    let update = tree_update(&scene, Some("gain"), 1.0);
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
        text("880 Hz").named("Reference").id("explicit")
    ];
    let scene = resolve(&SceneSpec::new(tree)).unwrap();
    let update = tree_update(&scene, None, 1.0);
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
