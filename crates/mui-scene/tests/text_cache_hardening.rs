use mui_scene::prelude::*;
use mui_scene::{resolve_scene_with, TextCache};

fn font() -> Font {
    Font::new(epaint_default_fonts::HACK_REGULAR).unwrap()
}

#[test]
fn an_animated_label_keeps_only_the_run_it_drew() {
    let font = font();
    let mut cache = TextCache::default();
    for i in 0..100 {
        let spec = SceneSpec::new(text("x").text_size(10.0 + i as f64 * 0.001)).font(font.clone());
        resolve_scene_with(&spec, &mut cache).unwrap();
        assert_eq!(cache.len(), 1);
    }
}

#[test]
fn changing_tolerance_invalidates_existing_outlines() {
    let both = row![text("x").text_size(10.0), text("x").text_size(12.0)];
    let spec = SceneSpec::new(both).font(font());
    let mut cache = TextCache::default();
    resolve_scene_with(&spec, &mut cache).unwrap();
    resolve_scene_with(&spec, &mut cache).unwrap();
    let warm = cache.layout_stats();
    assert!(warm.measure_hits + warm.arrange_hits > 0, "{warm:?}");
    let mut finer = spec.clone();
    finer.tolerance *= 0.5;
    resolve_scene_with(&finer, &mut cache).unwrap();
    // The reset that drops the shaped runs drops the cached layout with
    // them, so nothing measured at the old tolerance answers this frame.
    let cold = cache.layout_stats();
    assert_eq!((cold.measure_hits, cold.arrange_hits), (0, 0), "{cold:?}");
    assert_eq!(cache.len(), 2);
}

#[test]
fn live_text_updates_preserve_frames_and_explicit_labels() {
    let tree = text("0.0 dB")
        .reserve("-88.8 dB")
        .label("Gain readout")
        .id("readout");
    let mut scene = resolve_scene(&SceneSpec::new(tree).font(font())).unwrap();
    let before = scene.surface("readout").unwrap().frame;
    scene.set_text("readout", "-12.4 dB").unwrap();
    let after = scene.surface("readout").unwrap();
    assert_eq!(before, after.frame);
    assert_eq!(after.text_value.as_deref(), Some("-12.4 dB"));
    assert_eq!(
        after.semantics.as_ref().unwrap().label.as_deref(),
        Some("Gain readout")
    );
}
