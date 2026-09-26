use mui_scene::prelude::*;
use mui_scene::{Layer, SceneError, TextCache, WeldCache, resolve_scene_cached};
fn tree() -> El {
    row![
        leaf(80., 60.)
            .radius(12.)
            .fill(Primary)
            .stroke(Ink)
            .stroke_width(2.)
            .id("a"),
        leaf(80., 60.)
            .radius(12.)
            .fill(Secondary)
            .stroke(Warning)
            .stroke_width(8.)
            .id("b")
    ]
    .gap(20.)
    .gpu_weld(Weld::all().reach(30.))
    .id("join")
}
#[test]
fn gpu_resolution_never_populates_the_cpu_bake_cache() {
    let mut text = TextCache::default();
    let mut welds = WeldCache::default();
    let scene = resolve_scene_cached(&SceneSpec::new(tree()), &mut text, &mut welds).unwrap();
    // Analytic boundary slots are retained by design; a CPU bake would count as a miss.
    assert!(welds.bytes() <= 64 * 1024, "no CPU bake retained");
    assert_eq!(welds.stats(), (0, 0));
    assert!(scene.paint.iter().any(|p| p.layer == Layer::External));
}
#[test]
fn morph_is_transactional_and_does_not_relayout() {
    let mut scene = resolve_scene(&SceneSpec::new(tree())).unwrap();
    let before = scene.clone();
    assert!(matches!(
        scene.set_weld_morph("join", f64::NAN),
        Err(SceneError::MaterialWeld(mui_weld::Error::Invalid(
            "GPU morph"
        )))
    ));
    assert_eq!(scene, before);
    scene.set_weld_morph("join", 0.).unwrap();
    assert_eq!(scene.layout, before.layout);
    assert_eq!(scene.paint, before.paint);
}
#[test]
fn ids_and_original_plate_surfaces_survive() {
    let scene = resolve_scene(&SceneSpec::new(tree())).unwrap();
    assert!(scene.surface("a").is_some());
    assert!(scene.surface("b").is_some());
    assert!(
        !scene
            .paint
            .iter()
            .any(|p| p.key.as_ref() == "a" && matches!(p.layer, Layer::Fill | Layer::Stroke))
    );
}
#[test]
fn changing_union_is_not_misrepresented_as_a_clip_rectangle() {
    assert!(matches!(
        resolve_scene(&SceneSpec::new(tree().clip())),
        Err(SceneError::UnsupportedWeld(m)) if m.starts_with("GPU weld cannot itself clip")
    ));
}
#[test]
fn fourth_source_is_rejected_not_cpu_baked() {
    let root = row((0..4).map(|_| leaf(20., 20.).fill(Primary))).gpu_weld(Weld::all());
    assert!(matches!(
        resolve_scene(&SceneSpec::new(root)),
        Err(SceneError::UnsupportedWeld(m)) if m.starts_with("analytic GPU weld requires at most three")
    ));
}
#[test]
fn material_only_update_preserves_the_geometry() {
    let mut scene = resolve_scene(&SceneSpec::new(tree())).unwrap();
    let before = scene.clone();
    scene
        .set_weld_solid_material(
            "join",
            0,
            Some(Color::oklcha(0.7, 0.2, 30., 1.)),
            Some(Color::oklcha(1., 0., 0., 1.)),
            5.,
        )
        .unwrap();
    assert_eq!(scene.layout, before.layout);
    assert_eq!(scene.paint, before.paint);
    assert_eq!(
        scene.external_weld("join").unwrap().bounds(),
        before.external_weld("join").unwrap().bounds()
    );
}
