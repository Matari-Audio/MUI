use mui_scene::prelude::*;

#[test]
fn wrapped_text_aligns_its_first_line_without_collapsing_later_lines() {
    let root = row![
        text("alpha beta gamma").text_size(20.).w(70.).id("wrapped"),
        text("small").text_size(11.).id("small"),
    ]
    .baseline()
    .gap(8.);
    let scene = resolve_scene(
        &SceneSpec::new(root).font(Font::new(epaint_default_fonts::HACK_REGULAR).unwrap()),
    )
    .unwrap();
    let ys = |key: &str| {
        scene
            .paint
            .iter()
            .filter(|p| p.key.as_ref() == key)
            .filter_map(|p| p.text.as_ref().map(|t| t.origin.y))
            .collect::<Vec<_>>()
    };
    let wrapped = ys("wrapped");
    assert!(wrapped.len() > 1);
    assert_eq!(wrapped[0], ys("small")[0]);
    assert!(wrapped.windows(2).all(|w| w[1] > w[0]));
}
