use mui::prelude::*;

#[test]
fn custom_editor_carets_follow_scene_fonts_and_utf8_boundaries() {
    let mut ui = Ui::default()
        .font(Font::new(ttf_inter::REGULAR).unwrap())
        .fallback_font(Font::new(epaint_default_fonts::NOTO_EMOJI_REGULAR).unwrap());
    let value = "a😀b";
    let size = 20.0;
    ui.frame(
        text(value).text_size(size).id("label"),
        None,
        Input::default(),
        0.016,
    )
    .unwrap();
    let carets = ui.carets(value, size);
    assert_eq!(
        carets.iter().map(|&(byte, _)| byte).collect::<Vec<_>>(),
        [0, 1, 5, 6],
        "custom editors receive byte offsets at Unicode scalar boundaries"
    );
    let width = ui
        .scene()
        .unwrap()
        .surface("label")
        .unwrap()
        .frame
        .size
        .width;
    assert!(
        (carets.last().unwrap().1 - width).abs() < 1e-6,
        "caret extent must match the scene's font fallback layout"
    );
    assert!(carets.windows(2).all(|p| p[0].1 < p[1].1));
}

#[test]
fn fontless_carets_match_the_scene_measurement() {
    let ui = Ui::default();
    assert_eq!(ui.carets("éx", 10.0), [(0, 0.0), (2, 6.0), (3, 12.0)]);
    assert_eq!(ui.carets("", 10.0), [(0, 0.0)]);
}
