use mui_scene::prelude::*;
use mui_scene::{Layer, Paint, SceneError, TextCache, WeldCache, resolve_scene_cached};
use std::sync::Arc;

fn plate(id: &str, fill: impl Into<Fill>, width: f64) -> El {
    leaf(24., 24.).radius(5.).fill(fill).border(Ink, width).id(id)
}
fn tree(options: Weld) -> El {
    row![plate("a", Primary, 1.), plate("b", Secondary, 4.)]
        .gap(2.).weld_with(options).id("group")
}
fn image(scene: &mui_scene::ResolvedScene) -> Arc<mui_scene::Image> {
    scene.paint.iter().find_map(|p| match &p.paint {
        Paint::Image { image, .. } if p.key.as_ref() == "group" => Some(image.clone()),
        _ => None,
    }).expect("the group produces a material image")
}

#[test]
fn default_dsl_welds_body_and_border_without_replacing_identity() {
    let scene = resolve_scene(&SceneSpec::new(tree(Weld::default()))).unwrap();
    for key in ["a", "b", "group"] { assert!(scene.surface(key).is_some()); }
    assert!(scene.paint.iter().all(|p| !(p.key.as_ref() == "a" || p.key.as_ref() == "b")
        || !matches!(p.layer, Layer::Fill | Layer::Stroke)));
    assert!(image(&scene).width > 50);
}
#[test]
fn shape_only_is_not_border_omission() {
    let keep = resolve_scene(&SceneSpec::new(tree(Weld::shape()))).unwrap();
    let omit = resolve_scene(&SceneSpec::new(tree(Weld::all().border(WeldChannel::Omit)))).unwrap();
    assert_ne!(&*image(&keep).rgba, &*image(&omit).rgba);
}
#[test]
fn default_and_explicit_macros_construct_the_same_policy() {
    let a = weld![plate("a", Primary, 1.), plate("b", Secondary, 2.)];
    let b = weld![Weld::all(); plate("a", Primary, 1.), plate("b", Secondary, 2.)];
    assert_eq!(a.payload().welding, b.payload().welding);
    assert_eq!(weld_morph![0.25; plate("a", Primary, 1.)].payload().welding.unwrap().progress, 0.25);
    assert_eq!(weld_morph![Weld::shape(), 0.75; plate("a", Primary, 1.)].payload().welding.unwrap().border, WeldChannel::Keep);
}
#[test]
fn unchanged_weld_reuses_the_pixel_buffer() {
    let mut text = TextCache::default();
    let mut weld = WeldCache::default();
    let a = resolve_scene_cached(&SceneSpec::new(tree(Weld::all())), &mut text, &mut weld).unwrap();
    let b = resolve_scene_cached(&SceneSpec::new(tree(Weld::all())), &mut text, &mut weld).unwrap();
    assert!(Arc::ptr_eq(&image(&a).rgba, &image(&b).rgba));
    assert_eq!(weld.stats(), (1, 1));
}
#[test]
fn style_changes_invalidate_pixels() {
    let mut text = TextCache::default(); let mut weld = WeldCache::default();
    let a = resolve_scene_cached(&SceneSpec::new(tree(Weld::all())), &mut text, &mut weld).unwrap();
    let b = resolve_scene_cached(&SceneSpec::new(tree(Weld::all().blend(3.))), &mut text, &mut weld).unwrap();
    assert!(!Arc::ptr_eq(&image(&a).rgba, &image(&b).rgba));
    assert_eq!(weld.stats(), (0, 2));
}
#[test]
fn descendants_are_not_consumed_with_the_source_plate() {
    let a = col![leaf(5., 5.).fill(Danger).id("child")]
        .pad(8.).fill(Primary).border(Ink, 2.).id("a");
    let scene = resolve_scene(&SceneSpec::new(row![a, plate("b", Secondary, 1.)].weld_with(Weld::all()).id("group"))).unwrap();
    assert!(scene.paint.iter().any(|p| p.key.as_ref() == "child" && p.layer == Layer::Fill));
    assert_eq!(scene.surface("child").unwrap().parent.as_deref(), Some("a"));
}
#[test]
fn excluded_members_keep_their_paints() {
    let scene = resolve_scene(&SceneSpec::new(row![
        plate("a", Primary, 1.), plate("excluded", Danger, 2.).exclude_from_weld(),
    ].weld_with(Weld::all()).id("group"))).unwrap();
    assert!(scene.paint.iter().any(|p| p.key.as_ref() == "excluded" && p.layer == Layer::Stroke));
}
#[test]
fn custom_outline_is_not_a_rectangular_hit_proxy() {
    let shape = leaf(24., 24.).outline(|s| Path::polyline([
        Point::new(0., 0.), Point::new(s.width, 0.), Point::new(0., s.height),
    ], true)).fill(Primary).id("triangle");
    let scene = resolve_scene(&SceneSpec::new(shape)).unwrap();
    assert!(scene.surface("triangle").unwrap().rect.is_none());
    assert_eq!(scene.surface("triangle").unwrap().path.commands.len(), 4);
}
#[test]
fn unsupported_effects_are_not_silently_lost() {
    let root = row![plate("a", Primary, 1.).shadow(Shadow::soft(4.))]
        .weld_with(Weld::all());
    assert!(matches!(resolve_scene(&SceneSpec::new(root)), Err(SceneError::UnsupportedWeld(_))));
}
#[test]
fn explicit_off_returns_to_ordinary_painting() {
    let scene = resolve_scene(&SceneSpec::new(tree(Weld::all()).without_weld())).unwrap();
    assert!(scene.paint.iter().any(|p| p.key.as_ref() == "a" && p.layer == Layer::Stroke));
    assert!(scene.paint.iter().all(|p| !matches!(&p.paint, Paint::Image { .. })));
}
#[test]
fn morph_does_not_change_intrinsic_layout() {
    let a = resolve_scene(&SceneSpec::new(tree(Weld::all().morph(0.)))).unwrap();
    let b = resolve_scene(&SceneSpec::new(tree(Weld::all().morph(1.)))).unwrap();
    assert_eq!(a.layout, b.layout);
    assert_ne!(&*image(&a).rgba, &*image(&b).rgba);
}
#[test]
fn baked_weld_clip_is_carried_as_its_actual_shape() {
    let child = plate("a", Primary, 1.);
    let root = row![child, plate("b", Secondary, 2.)].weld_with(Weld::all()).clip().id("group");
    let s = resolve_scene(&SceneSpec::new(root)).unwrap();
    let group = s.surface("group").unwrap();
    let a = s.surface("a").unwrap();
    assert!(a.clip_paths.iter().any(|p| p.as_ref() == &group.path));
}
#[test]
fn legacy_weld_remains_a_vector_operation() {
    let old = row![leaf(24., 24.), leaf(24., 24.)].weld(Surface);
    let s = resolve_scene(&SceneSpec::new(old)).unwrap();
    assert!(s.paint.iter().all(|p| !matches!(&p.paint, Paint::Image { .. })));
}

#[test]
fn a_custom_outline_is_not_silently_reinterpreted_as_solid_when_carved() {
    let root = stack![].square(24.).outline(|s| Path::polyline([
        Point::new(0., 0.), Point::new(s.width, 0.), Point::new(0., s.height),
    ], true)).fill(Primary).cut(leaf(4., 4.));
    assert!(resolve_scene(&SceneSpec::new(root)).is_err());
}

#[test]
fn fractional_layout_origin_does_not_shift_the_baked_device_grid() {
    let root=stack![tree(Weld::all()).offset(0.3,0.25)].pad(20.);
    let s=resolve_scene(&SceneSpec::new(root).scale(1.5)).unwrap();
    let p=s.paint.iter().find(|p|p.key.as_ref()=="group"&&matches!(&p.paint,Paint::Image{..})).unwrap();
    let b=p.rect.unwrap().bounds();
    for edge in [b.min.x,b.min.y,b.max.x,b.max.y] {
        assert!((edge*1.5-(edge*1.5).round()).abs()<1e-8);
    }
}
