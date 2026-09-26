use mui_material::prelude::*;
use mui_scene::{Layer, Paint, Resolver, SceneError};
use std::sync::Arc;

fn plate(id: &str, fill: impl Into<Fill>, width: f64) -> El {
    block(24., 24.)
        .radius(5.)
        .fill(fill)
        .border(Role::Ink, width)
        .id(id)
}
fn tree(options: Weld) -> El {
    row![
        plate("a", Role::Primary, 1.),
        plate("b", Role::Secondary, 4.)
    ]
    .gap(2.)
    .weld(options)
    .id("group")
}
fn image(scene: &mui_scene::ResolvedScene) -> Arc<mui_scene::Image> {
    scene
        .paint
        .iter()
        .find_map(|p| match &p.paint {
            Paint::Image { image, .. } if p.key.as_ref() == "group" => Some(image.clone()),
            _ => None,
        })
        .expect("the group produces a material image")
}

#[test]
fn default_dsl_welds_body_and_border_without_replacing_identity() {
    let scene = resolve(&SceneSpec::new(tree(Weld::default()))).unwrap();
    for key in ["a", "b", "group"] {
        assert!(scene.surface(key).is_some());
    }
    assert!(
        scene
            .paint
            .iter()
            .all(|p| !(p.key.as_ref() == "a" || p.key.as_ref() == "b")
                || !matches!(p.layer, Layer::Fill | Layer::Stroke))
    );
    assert!(image(&scene).width > 50);
}
#[test]
fn shape_only_is_not_border_omission() {
    let keep = resolve(&SceneSpec::new(tree(Weld::shape()))).unwrap();
    let omit = resolve(&SceneSpec::new(tree(Weld::all().border(WeldChannel::Omit)))).unwrap();
    assert_ne!(&*image(&keep).rgba, &*image(&omit).rgba);
}
#[test]
fn default_and_explicit_macros_construct_the_same_policy() {
    let a = weld![
        plate("a", Role::Primary, 1.),
        plate("b", Role::Secondary, 2.)
    ];
    let b = weld![Weld::all(); plate("a", Role::Primary, 1.), plate("b", Role::Secondary, 2.)];
    assert_eq!(a.payload().extras().welding, b.payload().extras().welding);
    assert_eq!(
        weld![Weld::default().morph(0.25); plate("a", Role::Primary, 1.)]
            .payload()
            .extras()
            .welding
            .unwrap()
            .progress,
        0.25
    );
    assert_eq!(
        weld![Weld::shape().morph(0.75); plate("a", Role::Primary, 1.)]
            .payload()
            .extras()
            .welding
            .unwrap()
            .border,
        WeldChannel::Keep
    );
}
#[test]
fn unchanged_weld_reuses_the_pixel_buffer() {
    let mut r = Resolver::default();
    let a = r
        .resolve(&SceneSpec::new(tree(Weld::all())))
        .unwrap()
        .clone();
    let b = r.resolve(&SceneSpec::new(tree(Weld::all()))).unwrap();
    assert!(Arc::ptr_eq(&image(&a).rgba, &image(b).rgba));
    assert_eq!(r.welds.stats(), (1, 1));
}
#[test]
fn style_changes_invalidate_pixels() {
    let mut r = Resolver::default();
    let a = r
        .resolve(&SceneSpec::new(tree(Weld::all())))
        .unwrap()
        .clone();
    let b = r
        .resolve(&SceneSpec::new(tree(Weld::all().blend(3.))))
        .unwrap();
    assert!(!Arc::ptr_eq(&image(&a).rgba, &image(b).rgba));
    assert_eq!(r.welds.stats(), (0, 2));
}
#[test]
fn descendants_are_not_consumed_with_the_source_plate() {
    let a = col![block(5., 5.).fill(Role::Danger).id("child")]
        .pad(8.)
        .fill(Role::Primary)
        .border(Role::Ink, 2.)
        .id("a");
    let scene = resolve(&SceneSpec::new(
        row![a, plate("b", Role::Secondary, 1.)]
            .weld(Weld::all())
            .id("group"),
    ))
    .unwrap();
    assert!(
        scene
            .paint
            .iter()
            .any(|p| p.key.as_ref() == "child" && p.layer == Layer::Fill)
    );
    assert_eq!(scene.surface("child").unwrap().parent.as_deref(), Some("a"));
}
#[test]
fn excluded_members_keep_their_paints() {
    let scene = resolve(&SceneSpec::new(
        row![
            plate("a", Role::Primary, 1.),
            plate("excluded", Role::Danger, 2.).unwelded(),
        ]
        .weld(Weld::all())
        .id("group"),
    ))
    .unwrap();
    assert!(
        scene
            .paint
            .iter()
            .any(|p| p.key.as_ref() == "excluded" && p.layer == Layer::Stroke)
    );
}
#[test]
fn custom_outline_is_not_a_rectangular_hit_proxy() {
    let shape = block(24., 24.)
        .outline(|s| {
            Path::polyline(
                [
                    Point::new(0., 0.),
                    Point::new(s.width, 0.),
                    Point::new(0., s.height),
                ],
                true,
            )
        })
        .fill(Role::Primary)
        .id("triangle");
    let scene = resolve(&SceneSpec::new(shape)).unwrap();
    assert!(scene.surface("triangle").unwrap().rect.is_none());
    assert_eq!(scene.surface("triangle").unwrap().path.commands.len(), 4);
}
#[test]
fn unsupported_effects_are_not_silently_lost() {
    let root = row![plate("a", Role::Primary, 1.).shadow(Shadow::soft(4.))].weld(Weld::all());
    assert!(matches!(
        resolve(&SceneSpec::new(root)),
        Err(SceneError::UnsupportedWeld(_))
    ));
}
#[test]
fn explicit_off_returns_to_ordinary_painting() {
    let scene = resolve(&SceneSpec::new(tree(Weld::all()).weld(Weld::off()))).unwrap();
    assert!(
        scene
            .paint
            .iter()
            .any(|p| p.key.as_ref() == "a" && p.layer == Layer::Stroke)
    );
    assert!(
        scene
            .paint
            .iter()
            .all(|p| !matches!(&p.paint, Paint::Image { .. }))
    );
}
#[test]
fn morph_does_not_change_intrinsic_layout() {
    let a = resolve(&SceneSpec::new(tree(Weld::all().morph(0.)))).unwrap();
    let b = resolve(&SceneSpec::new(tree(Weld::all().morph(1.)))).unwrap();
    assert_eq!(a.layout, b.layout);
    assert_ne!(&*image(&a).rgba, &*image(&b).rgba);
}
#[test]
fn baked_weld_clip_is_carried_as_its_actual_shape() {
    let child = plate("a", Role::Primary, 1.);
    let root = row![child, plate("b", Role::Secondary, 2.)]
        .weld(Weld::all())
        .clip()
        .id("group");
    let s = resolve(&SceneSpec::new(root)).unwrap();
    let group = s.surface("group").unwrap();
    let a = s.surface("a").unwrap();
    assert!(
        a.clip_paths()
            .is_some_and(|paths| paths.contains(&(group.path.clone(), group.offset)))
    );
}
#[test]
fn a_union_is_a_vector_operation() {
    let union = row![block(24., 24.), block(24., 24.)].union(Role::Surface);
    let s = resolve(&SceneSpec::new(union)).unwrap();
    assert!(
        s.paint
            .iter()
            .all(|p| !matches!(&p.paint, Paint::Image { .. }))
    );
}

#[test]
fn a_custom_outline_is_not_silently_reinterpreted_as_solid_when_carved() {
    let root = stack![]
        .square(24.)
        .outline(|s| {
            Path::polyline(
                [
                    Point::new(0., 0.),
                    Point::new(s.width, 0.),
                    Point::new(0., s.height),
                ],
                true,
            )
        })
        .fill(Role::Primary)
        .cut(block(4., 4.));
    assert!(matches!(
        resolve(&SceneSpec::new(root)),
        Err(SceneError::Geometry(mui_geometry::Error::InvalidOptions(m)))
            if m.starts_with("cut/keep on a custom outline")
    ));
}

#[test]
fn fractional_layout_origin_does_not_shift_the_baked_device_grid() {
    let root = stack![tree(Weld::all()).offset(0.3, 0.25)].pad(20.);
    let s = resolve(&SceneSpec::new(root).scale(1.5)).unwrap();
    let p = s
        .paint
        .iter()
        .find(|p| p.key.as_ref() == "group" && matches!(&p.paint, Paint::Image { .. }))
        .unwrap();
    let b = p.rect.unwrap().bounds();
    for edge in [b.x0, b.y0, b.x1, b.y1] {
        assert!((edge * 1.5 - (edge * 1.5).round()).abs() < 1e-8);
    }
}
