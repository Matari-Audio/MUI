use mui_scene::prelude::*;
use mui_scene::{resolve_scene_with, TextCache};
use std::sync::Arc;

fn font() -> Arc<[u8]> {
    Arc::from(epaint_default_fonts::HACK_REGULAR)
}

#[test]
fn one_animated_label_cannot_bypass_the_variant_cap() {
    let font = font();
    let mut cache = TextCache::default();
    for i in 0..4100 {
        let spec = SceneSpec::new(text("x").text_size(10.0 + i as f64 * 0.001)).font(font.clone());
        resolve_scene_with(&spec, &mut cache).unwrap();
        assert!(cache.len() <= 4096);
    }
}

#[test]
fn changing_tolerance_invalidates_existing_outlines() {
    let font = font();
    let mut cache = TextCache::default();
    for size in [10.0, 12.0] {
        resolve_scene_with(&SceneSpec::new(text("x").text_size(size)).font(font.clone()), &mut cache).unwrap();
    }
    assert_eq!(cache.len(), 2);
    let mut spec = SceneSpec::new(text("x").text_size(10.0)).font(font);
    spec.tolerance *= 0.5;
    resolve_scene_with(&spec, &mut cache).unwrap();
    assert_eq!(cache.len(), 1);
}

#[test]
fn live_text_updates_preserve_frames_and_explicit_labels() {
    let tree = text("0.0 dB").reserve("-88.8 dB").label("Gain readout").id("readout");
    let mut scene = resolve_scene(&SceneSpec::new(tree).font(font())).unwrap();
    let before = scene.surface("readout").unwrap().frame;
    scene.set_text("readout", "-12.4 dB").unwrap();
    let after = scene.surface("readout").unwrap();
    assert_eq!(before, after.frame);
    assert_eq!(after.text_value.as_deref(), Some("-12.4 dB"));
    assert_eq!(after.semantics.as_ref().unwrap().label.as_deref(), Some("Gain readout"));
}
