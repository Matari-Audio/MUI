//! Controls as compositions. Nothing here is placed absolutely: a thumb sits
//! where two flex weights put it, a knob's pointer is an anchored offset.
use std::ops::RangeInclusive;

use mui_core::prelude::*;
use mui_core::Spring;
use mui_input::Key;

use crate::Ui;

fn unit(value: f64, range: &RangeInclusive<f64>) -> f64 {
    // A fixed parameter reports min == max; without this the divide is NaN,
    // `clamp` passes NaN through, and the flex weight fails validation.
    // An inverted range still divides correctly, so only zero bails.
    let span = range.end() - range.start();
    if span == 0.0 {
        return 0.0;
    }
    ((value - range.start()) / span).clamp(0.0, 1.0)
}

/// Label, readout, and a track whose fill and thumb are flex shares.
///
/// Call [`Ui::edit`](crate::Ui::edit) with the same id to bracket the drag
/// for a host's automation: `Begin` on the press, `End` on the release.
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

/// A dial: vertical drag, pointer on a 270° sweep. The pointer follows a
/// tween, so a value set from outside -- a preset, a host automation curve --
/// glides instead of jumping, while a drag still tracks the pointer.
///
/// Bracket it with [`Ui::edit`](crate::Ui::edit), as for [`slider`].
pub fn knob(
    ui: &mut Ui,
    id: &str,
    label: &str,
    value: &mut f64,
    range: RangeInclusive<f64>,
    size: f64,
) -> El {
    ui.drag(id, value, range.clone(), 120.0, true);
    // Fast enough that a drag still feels direct, slow enough that a
    // preset change is a glide.
    let t = ui.tween_with(id, unit(*value, &range), Spring::new(0.12, 1.0));
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
        .animate()
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
    .animate()
    .id(id)
}

fn byte(s: &str, chars: usize) -> usize {
    s.char_indices().nth(chars).map_or(s.len(), |(b, _)| b)
}

/// The selection as a string, and the two helpers that edit it. Indices are
/// characters; `byte` turns them into slice offsets.
fn selected(value: &str, a: usize, c: usize) -> String {
    value[byte(value, a.min(c))..byte(value, a.max(c))].to_owned()
}
/// Remove the selection: the caret afterwards, and whether there was one.
fn take(value: &mut String, a: usize, c: usize) -> (usize, bool) {
    let (lo, hi) = (a.min(c), a.max(c));
    if lo == hi {
        return (c, false);
    }
    let (x, y) = (byte(value, lo), byte(value, hi));
    value.replace_range(x..y, "");
    (lo, true)
}
/// The run of alphanumerics around `at`, for a double click.
fn word(value: &str, at: usize) -> (usize, usize) {
    let ch: Vec<char> = value.chars().collect();
    let (mut lo, mut hi) = (at.min(ch.len()), at.min(ch.len()));
    while lo > 0 && ch[lo - 1].is_alphanumeric() {
        lo -= 1;
    }
    while hi < ch.len() && ch[hi].is_alphanumeric() {
        hi += 1;
    }
    (lo, hi)
}
fn insert(value: &mut String, caret: &mut usize, c: char) {
    value.insert(byte(value, *caret), c);
    *caret += 1;
}

/// A single-line field: the text, a selection, a blinking caret, and the
/// edits the focused keys imply. Click to focus and set the caret, drag to
/// select, double click for a word, shift+arrows to extend. ctrl/cmd+A, C, X
/// and V select all, copy, cut and paste -- a copy leaves the text in
/// [`Frame::clipboard`](crate::Frame) for the host to hand to the OS, and a
/// paste reads `Input::clipboard`, which the host fills on the paste key.
// ponytail: single line. A multi-line field wants the caret on a line index,
// not a byte, and `mui_text::break_lines` to place it.
pub fn text_input(ui: &mut Ui, id: &str, value: &mut String) -> El {
    let size = ui.theme.text;
    let focused = ui.focused(id);
    let n = value.chars().count();
    let (anchor, caret) = ui.sel(id);
    let (mut anchor, mut caret) = (anchor.min(n), caret.min(n));

    // The pointer, against last frame's frame: pad_xy's 8 px is where the
    // text starts.
    let r = ui.get(id);
    if r.pressed || r.dragged {
        if let Some(p) = ui.local(id) {
            caret = ui.hit(value, size, p.x - 8.0);
            if r.pressed {
                anchor = caret;
            }
        }
    }
    if ui.double_click(id) {
        (anchor, caret) = word(value, caret);
    }

    if focused {
        for c in ui.text(id).to_owned().chars().filter(|c| !c.is_control()) {
            (caret, _) = take(value, anchor, caret);
            insert(value, &mut caret, c);
            anchor = caret;
        }
        for k in ui.keys(id).to_vec() {
            let cmd = k.mods.ctrl || k.mods.cmd;
            match k.key {
                Key::Char(c) if cmd => match c.to_ascii_lowercase() {
                    'a' => (anchor, caret) = (0, value.chars().count()),
                    // An empty selection copies nothing: handing the host
                    // "" would wipe whatever is already on the clipboard.
                    'c' if anchor != caret => {
                        ui.set_clipboard(selected(value, anchor, caret));
                    }
                    'x' if anchor != caret => {
                        ui.set_clipboard(selected(value, anchor, caret));
                        (caret, _) = take(value, anchor, caret);
                        anchor = caret;
                    }
                    'v' => {
                        if let Some(s) = ui.pasted().map(str::to_owned) {
                            (caret, _) = take(value, anchor, caret);
                            for c in s.chars().filter(|c| !c.is_control()) {
                                insert(value, &mut caret, c);
                            }
                            anchor = caret;
                        }
                    }
                    _ => {}
                },
                Key::Char(c) if !c.is_control() => {
                    (caret, _) = take(value, anchor, caret);
                    insert(value, &mut caret, c);
                    anchor = caret;
                }
                Key::Backspace => {
                    let (at, had) = take(value, anchor, caret);
                    caret = at;
                    if !had && caret > 0 {
                        value.remove(byte(value, caret - 1));
                        caret -= 1;
                    }
                    anchor = caret;
                }
                Key::Delete => {
                    let (at, had) = take(value, anchor, caret);
                    caret = at;
                    if !had && caret < value.chars().count() {
                        value.remove(byte(value, caret));
                    }
                    anchor = caret;
                }
                Key::Left | Key::Right | Key::Home | Key::End => {
                    caret = match k.key {
                        Key::Left => caret.saturating_sub(1),
                        Key::Right => (caret + 1).min(value.chars().count()),
                        Key::Home => 0,
                        _ => value.chars().count(),
                    };
                    if !k.mods.shift {
                        anchor = caret;
                    }
                }
                _ => {}
            }
        }
    }
    ui.set_sel(id, anchor, caret);

    let x = |at: usize| ui.caret_x(value, size, byte(value, at));
    let (lo, hi) = (x(anchor.min(caret)), x(anchor.max(caret)));
    let on = focused && ui.blink();
    overlay([
        leaf(hi - lo, size)
            .anchor(Align::Start, Align::Center)
            .offset(lo, 0.0)
            .when(hi > lo, |e| e.fill(Role::Primary)),
        text(value.clone()).anchor(Align::Start, Align::Center),
        leaf(2.0, size)
            .anchor(Align::Start, Align::Center)
            .offset(x(caret), 0.0)
            .when(on, |e| e.fill(Role::Ink)),
    ])
    .pad_xy(8.0, 6.0)
    .radius(6.0)
    .fill(Role::Field)
    .cursor(Cursor::Text)
    .focusable()
    .id(id)
}
