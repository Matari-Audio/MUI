//! A node shrunk to nothing paints nothing; it does not fail the frame. A
//! level meter at silence is `leaf(200. * level, 8.).pill()`.
use mui::prelude::*;

#[test]
fn a_meter_at_silence_does_not_fail_the_frame() {
    let mut ui = Ui::new(Theme::DEFAULT);
    for level in [0.5, 0.0, 1e-9, 1e-6, 1e-4, 0.002, 0.5] {
        let meter = row([leaf(200. * level, 8.).pill().fill(Primary)])
            .size(220., 8.)
            .pill()
            .fill(Field);
        let f = ui.frame(
            meter,
            Some(Size::new(300., 40.)),
            Input::default(),
            1. / 60.,
        );
        assert!(f.is_ok(), "level {level}: {:?}", f.err());
    }
}
