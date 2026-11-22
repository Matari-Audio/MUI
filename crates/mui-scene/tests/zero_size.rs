//! A node that has shrunk to nothing paints nothing; it does not fail the
//! frame. A level meter at silence is `block(200. * level, 8.).pill()`.
use mui_material::prelude::*;

#[test]
fn a_zero_sized_filled_node_resolves() {
    for el in [
        block(0., 8.).pill().fill(Role::Primary),
        block(0., 0.).fill(Role::Primary).shadow(Shadow::soft(8.)),
        block(20., 0.)
            .radius(4.)
            .fill(Role::Primary)
            .stroke(Role::Primary)
            .stroke_width(1.),
        block(0., 8.)
            .pill()
            .fill(Role::Primary)
            .shell(4., Role::Raised),
    ] {
        let s = resolve(&SceneSpec::new(row([el.id("m"), block(10., 10.)])));
        assert!(s.is_ok(), "{:?}", s.err());
    }
}

#[test]
fn a_collapsed_shadow_is_omitted_and_other_nodes_still_paint() {
    let mut shadow = Shadow::soft(8.);
    shadow.spread = -20.;
    let scene = resolve(&SceneSpec::new(row([
        block(32., 32.)
            .pill()
            .fill(Role::Primary)
            .shadow(shadow)
            .id("small"),
        block(100., 100.)
            .fill(Role::Primary)
            .shadow(Shadow::soft(8.))
            .id("large"),
    ])))
    .unwrap();
    assert!(
        !scene
            .paint
            .iter()
            .any(|p| p.key.as_str() == "small" && matches!(p.layer, mui_scene::Layer::Shadow(_)))
    );
    assert!(
        scene
            .paint
            .iter()
            .any(|p| p.key.as_str() == "large" && matches!(p.layer, mui_scene::Layer::Shadow(_)))
    );
}

#[test]
fn a_collapsed_inset_shadow_fills_the_clipped_outline() {
    let mut shadow = Shadow::inset(8.);
    shadow.spread = 20.;
    shadow.dx = 7.;
    let scene = resolve(&SceneSpec::new(
        block(32., 32.).pill().shadow(shadow).id("small"),
    ))
    .unwrap();
    let at = scene
        .paint
        .iter()
        .position(|p| {
            p.key.as_str() == "small"
                && p.layer == mui_scene::Layer::Shadow(mui_scene::ShadowKind::Inset)
        })
        .expect("collapsed inset still paints");
    let paint = &scene.paint[at];
    assert_eq!(paint.blur, 0.);
    let clip = scene.paint[..at]
        .iter()
        .rev()
        .find(|p| p.layer == mui_scene::Layer::Clip)
        .unwrap();
    assert_eq!(paint.path, clip.path);
    assert_eq!(paint.offset, clip.offset);
    assert!(
        scene.paint[at + 1..]
            .iter()
            .any(|p| p.layer == mui_scene::Layer::Unclip)
    );
}
