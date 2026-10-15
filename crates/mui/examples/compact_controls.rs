//! Compiling call-site comparisons: complete controls, no macro-only shorthand.
use mui::prelude::*;

// Six semantic operations plus conversion: construction, conditional action,
// cursor, state override, stroke, stroke width. The same APIs remain available.
fn explicit_action(ui: &mut Ui, calls: &mut usize) -> El {
    let response = button(ui, "save", "Save");
    if response.changed {
        *calls += 1;
    }
    response
        .el
        .el()
        .cursor(Cursor::Hand)
        .on(State::FocusVisible, |style| {
            style.stroke(Role::Ink).stroke_width(2.0)
        })
}

// Two operations: construct the complete control and attach its action.
fn compact_action(ui: &mut Ui, calls: &mut usize) -> El {
    button(ui, "save", "Save")
        .on_change(|| *calls += 1)
        .into_el()
}

fn main() {
    let mut ui = Ui::default();
    let (mut calls, mut sync, mut gain) = (0, false, 0.5);
    let action = compact_action(&mut ui, &mut calls);
    let sync = setting("sync", "Sync")
        .description("Follow the host tempo")
        .toggle(&mut ui, &mut sync);
    let level = setting("gain", "Gain").control(&mut ui, |ui, id, label| {
        knob(ui, id, label, &mut gain, 0.0..=1.0)
            .value_text("0.0 dB")
            .value_reserve("-60.00 dB")
    });
    let panel = group(
        "output",
        "Output",
        [action, sync.into_el(), level.into_el()],
    );
    ui.frame(panel, None, Input::default(), 0.016).unwrap();
    // The explicit equivalent is compiled too, on a separate tree (no duplicate id).
    let old = explicit_action(&mut ui, &mut calls);
    ui.frame(old, None, Input::default(), 0.016).unwrap();
    assert_eq!(calls, 0);
}
