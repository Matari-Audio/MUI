use mui::prelude::*;
fn tree(pad: f64, width: f64) -> mui::scene::El {
    row![stack![].flex(1.).id("a"), stack![].flex(1.).id("b")]
        .inside(pad)
        .border_ramp(BorderRamp::horizontal((Primary, width), (Dim, 1.)))
        .radius(0.)
        .w(200.)
        .h(100.)
        .id("root")
        .transition(Spring::DEFAULT)
}
#[test]
fn padding_gap_and_border_morph_together_and_settle() {
    let mut ui = Ui::new(Theme::DEFAULT);
    let mut sample = |pad, width| {
        let frame = ui
            .frame(
                tree(pad, width),
                Some(Size::new(200., 100.)),
                Input::default(),
                1. / 60.,
            )
            .unwrap();
        let a = frame.scene.surface("a").unwrap().frame;
        let b = frame.scene.surface("b").unwrap().frame;
        (a.x, b.x - a.right())
    };
    let start = sample(2., 2.);
    let middle = sample(10., 20.);
    assert!(middle.0 > start.0 && middle.0 < 30.);
    assert!(middle.1 > 2. && middle.1 < 10.);
    let mut end = middle;
    for _ in 0..300 {
        end = sample(10., 20.);
    }
    assert!((end.0 - 30.).abs() < 0.05);
    assert!((end.1 - 10.).abs() < 0.05);
}
