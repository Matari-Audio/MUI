use mui_scene::Resolver;
use mui_scene::prelude::*;

fn font() -> Font {
    Font::new(epaint_default_fonts::HACK_REGULAR).unwrap()
}

#[test]
fn an_animated_label_keeps_only_the_run_it_drew() {
    let font = font();
    let mut cache = Resolver::default();
    for i in 0..100 {
        let spec = SceneSpec::new(text("x").text_size(10.0 + i as f64 * 0.001)).font(font.clone());
        cache.resolve(&spec).unwrap();
        assert_eq!(cache.text_runs(), 1);
    }
}

#[test]
fn live_text_updates_preserve_frames_and_explicit_labels() {
    let tree = text("0.0 dB")
        .reserve("-88.8 dB")
        .named("Gain readout")
        .id("readout");
    let mut scene = resolve(&SceneSpec::new(tree).font(font())).unwrap();
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

#[test]
fn a_stand_in_measured_value_changes_without_relayout() {
    let tree = |value: &str| {
        row![
            text("GAIN").id("name"),
            text(value).measure_as("-88.8 dB").id("readout"),
        ]
    };
    let (font, mut cache) = (font(), Resolver::default());
    let first = cache
        .resolve(&SceneSpec::new(tree("0.0 dB")).font(font.clone()))
        .unwrap()
        .surface("readout")
        .unwrap()
        .frame;
    // The first resolve at a size solves bare; the second fills the cache.
    cache
        .resolve(&SceneSpec::new(tree("0.0 dB")).font(font.clone()))
        .unwrap();
    for value in ["-12.4 dB", "-88.8 dB", "3 dB"] {
        let scene = cache
            .resolve(&SceneSpec::new(tree(value)).font(font.clone()))
            .unwrap();
        let readout = scene.surface("readout").unwrap();
        assert_eq!(readout.frame, first);
        assert_eq!(readout.text_value.as_deref(), Some(value));
        assert_eq!(cache.layout_stats().measured_nodes, 0, "{value}");
    }
}
