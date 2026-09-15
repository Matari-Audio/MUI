//! Controls as compositions. Nothing here is placed absolutely: a thumb sits
//! where two flex weights put it, a knob's pointer is an anchored offset.
use std::ops::RangeInclusive;

use mui_core::prelude::*;
use mui_input::Key;

use crate::Ui;

fn unit(value: f64, range: &RangeInclusive<f64>) -> f64 {
    ((value - range.start()) / (range.end() - range.start())).clamp(0.0, 1.0)
}

/// Label, readout, and a track whose fill and thumb are flex shares.
pub fn slider(
    ui: &mut Ui,
    id: &str,
    label: &str,
    value: &mut f64,
    range: RangeInclusive<f64>,
) -> El {
    ui.drag(id, value, range.clone(), 160.0, false);
    let t = unit(*value, &range);
    let (h, _) = ui.state(id);
    let thumb = leaf(14.0 + 2.0 * h, 14.0 + 2.0 * h)
        .pill()
        .fill(Role::Primary)
        .id(id);
    column([
        row([
            text(label),
            spacer(),
            text(format!("{value:.2}")).fill(Role::Dim),
        ])
        .gap(S),
        overlay([
            row([
                leaf(0.0, 6.0).grow(t).pill().fill(Role::Primary),
                leaf(0.0, 6.0).grow(1.0 - t),
            ])
            .anchor(Align::Stretch, Align::Center)
            .pill()
            .fill(Role::Field),
            row([spacer().grow(t), thumb, spacer().grow(1.0 - t)])
                .anchor(Align::Stretch, Align::Center),
        ])
        .height(18.0),
    ])
    .gap(Xs)
}

/// A dial: vertical drag, pointer on a 270° sweep.
pub fn knob(
    ui: &mut Ui,
    id: &str,
    label: &str,
    value: &mut f64,
    range: RangeInclusive<f64>,
    size: f64,
) -> El {
    ui.drag(id, value, range.clone(), 120.0, true);
    let t = unit(*value, &range);
    let a = (135.0 + 270.0 * t).to_radians();
    let r = size / 2.0 - 6.0;
    let (h, _) = ui.state(id);
    column([
        overlay([
            leaf(size, size)
                .pill()
                .fill(Role::Raised)
                .shell(3.0 + 1.0 * h, Role::Field)
                .id(id),
            leaf(6.0, 6.0)
                .pill()
                .fill(Role::Primary)
                .anchor(Align::Center, Align::Center)
                .offset(r * a.cos(), r * a.sin()),
        ]),
        text(label).fill(Role::Dim),
    ])
    .gap(Xs)
    .align(Align::Center)
}

/// Returns the element and whether it was clicked last frame.
pub fn button(ui: &Ui, id: &str, label: &str) -> (El, bool) {
    let el = row([text(label)])
        .pad_xy(14.0, 8.0)
        .pill()
        .fill(Role::Primary)
        .id(id);
    (el, ui.get(id).clicked)
}

/// A switch: the knob's side is a flex share, the click flips it.
pub fn toggle(ui: &Ui, id: &str, on: &mut bool) -> El {
    if ui.get(id).clicked {
        *on = !*on;
    }
    let t = f64::from(*on);
    row([
        spacer().grow(t),
        leaf(16.0, 16.0).pill().fill(Role::Ink),
        spacer().grow(1.0 - t),
    ])
    .size(40.0, 22.0)
    .pad(3.0)
    .pill()
    .fill(if *on { Role::Primary } else { Role::Field })
    .id(id)
}

fn byte(s: &str, chars: usize) -> usize {
    s.char_indices().nth(chars).map_or(s.len(), |(b, _)| b)
}

/// A single-line field: the text, a blinking caret, and the edits the focused
/// keys imply. Click it to focus, Tab to walk to it.
///
// ponytail: no selection, no clipboard.
pub fn text_input(ui: &mut Ui, id: &str, value: &mut String) -> El {
    let focused = ui.focused(id);
    let mut caret = ui.caret(id).min(value.chars().count());
    if focused {
        for c in ui.text(id).to_owned().chars().filter(|c| !c.is_control()) {
            value.insert(byte(value, caret), c);
            caret += 1;
        }
        for k in ui.keys(id).to_vec() {
            match k.key {
                Key::Char(c) if !c.is_control() => {
                    value.insert(byte(value, caret), c);
                    caret += 1;
                }
                Key::Backspace if caret > 0 => {
                    value.remove(byte(value, caret - 1));
                    caret -= 1;
                }
                Key::Delete if caret < value.chars().count() => {
                    value.remove(byte(value, caret));
                }
                Key::Left => caret = caret.saturating_sub(1),
                Key::Right => caret = (caret + 1).min(value.chars().count()),
                Key::Home => caret = 0,
                Key::End => caret = value.chars().count(),
                _ => {}
            }
        }
        ui.set_caret(id, caret);
    }
    let size = ui.theme.text;
    let x = ui.advance(&value[..byte(value, caret)], size);
    let on = focused && ui.blink();
    overlay([
        text(value.clone()).anchor(Align::Start, Align::Center),
        leaf(2.0, size)
            .anchor(Align::Start, Align::Center)
            .offset(x, 0.0)
            .when(on, |e| e.fill(Role::Ink)),
    ])
    .pad_xy(8.0, 6.0)
    .radius(6.0)
    .fill(Role::Field)
    .cursor(Cursor::Text)
    .focusable()
    .id(id)
}
