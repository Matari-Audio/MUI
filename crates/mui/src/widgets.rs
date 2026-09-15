//! Controls as compositions. Nothing here is placed absolutely: a thumb sits
//! where two flex weights put it, a knob's pointer is an anchored offset.
use std::ops::RangeInclusive;

use mui_core::prelude::*;

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
