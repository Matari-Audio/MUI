use mui::prelude::*;

#[test]
fn deadline_allows_build_to_observe_expiry_before_sleeping() {
    let mut ui = Ui::default();
    let deadline = 0.2;
    let mut expired = false;
    let mut dt = 0.0;
    for _ in 0..4 {
        expired = ui.time() >= deadline;
        if !expired {
            ui.repaint_in(deadline - ui.time());
        }
        let frame = ui
            .frame(block(20., 20.), None, Input::default(), dt)
            .unwrap();
        if !frame.animating && frame.repaint_after.is_none() {
            break;
        }
        dt = frame
            .repaint_after
            .map_or(0.016, |delay| delay.as_secs_f64());
    }
    assert!(
        expired,
        "the host went idle before the build observed the deadline"
    );
}
