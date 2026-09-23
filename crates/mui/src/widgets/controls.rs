//! Controls as compositions. Nothing here is placed absolutely: a thumb sits
//! where two flex weights put it, a knob's pointer is an anchored offset.
use std::ops::RangeInclusive;

use mui_geometry::Point;
use mui_input::{Button, Key, FINE_DRAG};
use mui_scene::prelude::*;
use mui_scene::{Palette, SpacingToken, Spring, Stroke};

use crate::Ui;

/// How solid a control looks. daisyUI's four button styles, resolved from
/// the role and the palette rather than from a table of colours.
///
/// ```
/// use mui::prelude::*;
/// let mut ui = Ui::new(Theme::DEFAULT);
/// let (quiet, _) = button(&mut ui, "bypass", "Bypass");
/// // Ink only: the resting box paints nothing.
/// assert_eq!(quiet.variant(Variant::Ghost).el().payload().style.fill, Fill::None);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Variant {
    /// The role, filled. The one live action on a panel.
    #[default]
    Solid,
    /// The role at a low alpha: the same colour, mixed into whatever it sits
    /// on, with the role as its ink.
    Soft,
    /// The role as a stroke and as ink; nothing filled.
    Outline,
    /// Ink only, until the pointer arrives.
    Ghost,
}

/// A control's look, resolved: the pixel height it was sized to, the
/// palette the variant does its arithmetic with, and what the readout was
/// told to say.
struct Look {
    px: f64,
    variant: Variant,
    role: Role,
    palette: Palette,
    text: Option<String>,
}

/// A keyboard activation is the same primary action as a click. `Ui::keys`
/// is already gated to the focused surface, so widgets do not need to invent
/// a second focus test or consume another key stream.
fn activated(ui: &Ui, id: &str) -> bool {
    ui.get(id).clicked_with(Button::Primary)
        || ui
            .keys(id)
            .iter()
            .any(|k| matches!(k.key, Key::Enter | Key::Space))
}
impl Look {
    /// What the control's own box is painted with, where the role is the
    /// fill: a button, a toggle that is on.
    fn style(&self) -> Style {
        self.face(self.role)
    }
    /// The same, for a control whose resting box is *not* the role -- a
    /// knob's face, a slider's track: `Solid` leaves it alone.
    fn face(&self, resting: Role) -> Style {
        match self.variant {
            Variant::Solid => Style::default().fill(resting),
            // A faded role *is* the mix with the ground: it composites over
            // whatever is under it, so it tracks the panel it lands on.
            Variant::Soft => Style::default().fill(self.role.alpha(0.18)),
            Variant::Outline => Style {
                stroke: Some(Stroke {
                    fill: self.role.into(),
                    width: None,
                }),
                ..Style::default()
            },
            Variant::Ghost => Style::default(),
        }
    }
    /// The ink that reads on that box: `Palette::on` computes the contrast
    /// for a filled one, and the role speaks for itself on the rest.
    fn ink(&self) -> Fill {
        match self.variant {
            Variant::Solid => Role::Ink.into(),
            _ => self.role.into(),
        }
    }
    /// The state closure that lifts whatever the control is filled with, so
    /// a hover is the palette's own step and not a second colour.
    fn hover(&self) -> impl Fn(Style) -> Style + 'static {
        let (p, role, variant) = (self.palette, self.role, self.variant);
        move |s: Style| {
            let ground = p.surface();
            match variant {
                // Nothing to lift: the hover is the role, arriving.
                Variant::Outline | Variant::Ghost => s.fill(role.alpha(0.12)),
                _ => {
                    let lifted = s.fill.map(&p, ground, |c| p.hover(c));
                    s.fill(lifted)
                }
            }
        }
    }
}

/// A control the widget has taken its value from, with what is left to say
/// about how it looks: the variant, the size, and the pixels as an escape
/// hatch. Finish it with [`Control::el`], or drop it straight into a
/// `row![..]`.
///
/// ```
/// use mui::prelude::*;
/// let mut ui = Ui::new(Theme::DEFAULT);
/// let mut cutoff = 0.5;
/// let (dial, _) = knob(&mut ui, "cut", "Cutoff", &mut cutoff, 0.0..=1.0);
/// let strip = row![dial.size(L), label("post")];
/// ```
pub struct Control {
    look: Look,
    size: SpacingToken,
    px: Option<f64>,
    build: Box<dyn FnOnce(&Look) -> El>,
}
impl Control {
    fn new(ui: &Ui, build: impl FnOnce(&Look) -> El + 'static) -> Self {
        Self {
            look: Look {
                px: ui.theme.control,
                variant: Variant::Solid,
                role: Role::Primary,
                palette: ui.theme.palette,
                text: None,
            },
            size: SpacingToken::M,
            px: None,
            build: Box::new(build),
        }
    }
    /// How solid this control looks.
    ///
    /// ```
    /// use mui::prelude::*;
    /// let mut ui = Ui::new(Theme::DEFAULT);
    /// let mut on = false;
    /// let (sw, _) = toggle(&mut ui, "bypass", &mut on);
    /// let sw = sw.variant(Variant::Outline);
    /// assert!(sw.el().payload().style.stroke.is_some(), "outlined");
    /// ```
    pub fn variant(mut self, v: Variant) -> Self {
        self.look.variant = v;
        self
    }
    /// The palette role this control speaks with: `Primary` unless it is a
    /// destructive action.
    ///
    /// ```
    /// use mui::prelude::*;
    /// let mut ui = Ui::new(Theme::DEFAULT);
    /// let (clear, _) = button(&mut ui, "clear", "Clear");
    /// let el = clear.role(Danger).variant(Variant::Outline).el();
    /// let ring = el.payload().style.stroke.clone().map(|s| s.fill);
    /// assert_eq!(ring, Some(Fill::Role(Danger)));
    /// ```
    pub fn role(mut self, r: Role) -> Self {
        self.look.role = r;
        self
    }
    /// One of the five sizes, the same five everywhere: `Xs` through `Xl`
    /// multiply the theme's [`control`](mui_scene::Theme::control) unit.
    ///
    /// ```
    /// use mui::prelude::*;
    /// let mut ui = Ui::new(Theme::DEFAULT);
    /// let mut on = false;
    /// let (small, _) = toggle(&mut ui, "bypass", &mut on);
    /// let small = small.size(Xs);
    /// ```
    pub fn size(mut self, s: SpacingToken) -> Self {
        self.size = s;
        self
    }
    /// What the readout says, instead of the default two decimals: the
    /// value in the units the parameter actually has. A knob has no header,
    /// so it says this under the dial in place of its label.
    ///
    /// ```
    /// use mui::prelude::*;
    /// let mut ui = Ui::new(Theme::DEFAULT);
    /// let mut cutoff = 440.0;
    /// let (dial, _) = knob(&mut ui, "cut", "Cutoff", &mut cutoff, 20.0..=20_000.0);
    /// let dial = dial.value_text(format!("{cutoff:.0} Hz"));
    /// ```
    pub fn value_text(mut self, s: impl Into<String>) -> Self {
        self.look.text = Some(s.into());
        self
    }
    /// The control's height in pixels, for the knob that has to match a
    /// hardware panel. Overrides [`Control::size`].
    ///
    /// ```
    /// use mui::prelude::*;
    /// let mut ui = Ui::new(Theme::DEFAULT);
    /// let mut v = 0.5;
    /// let (dial, _) = knob(&mut ui, "cut", "Cutoff", &mut v, 0.0..=1.0);
    /// let dial = dial.px(37.0);
    /// ```
    pub fn px(mut self, px: f64) -> Self {
        self.px = Some(px);
        self
    }
    /// The tree, finished.
    ///
    /// ```
    /// use mui::prelude::*;
    /// let mut ui = Ui::new(Theme::DEFAULT);
    /// let (go, _clicked) = button(&mut ui, "go", "Go");
    /// let el: El = go.el();
    /// ```
    pub fn el(mut self) -> El {
        // Ten units at M, two either side: daisyUI's -xs .. -xl scale, so one
        // `Theme.control` rescales every control in the tree.
        let steps = match self.size {
            SpacingToken::Xs => 6.0,
            SpacingToken::S => 8.0,
            SpacingToken::M => 10.0,
            SpacingToken::L => 12.0,
            SpacingToken::Xl => 14.0,
        };
        self.look.px = self.px.unwrap_or(self.look.px * steps);
        (self.build)(&self.look)
    }
}
impl From<Control> for El {
    fn from(c: Control) -> Self {
        c.el()
    }
}
impl IntoEl for Control {
    fn into_el(self) -> El {
        self.el()
    }
}

/// One arrow-key step of a slider or knob across `range`: a hundredth of it,
/// signed with the range, so an inverted control still steps toward its end.
/// A platform's `Increment` and `Decrement` take the same step.
pub fn step(range: &RangeInclusive<f64>) -> f64 {
    (range.end() - range.start()) / 100.0
}

/// Step `value` by the focused control's keys: arrows by [`step`] (a tenth
/// of it with Shift), Page Up and Down by ten steps, Home and End to the
/// ends. The runtime brackets the frame as one edit.
fn stepped(ui: &Ui, id: &str, value: &mut f64, range: &RangeInclusive<f64>) {
    let (lo, hi) = (*range.start(), *range.end());
    if !(lo.is_finite() && hi.is_finite()) {
        return;
    }
    for k in ui.keys(id) {
        let one = step(range) * if k.mods.shift { FINE_DRAG } else { 1.0 };
        *value = match k.key {
            Key::Right | Key::Up => *value + one,
            Key::Left | Key::Down => *value - one,
            Key::PageUp => *value + 10.0 * step(range),
            Key::PageDown => *value - 10.0 * step(range),
            Key::Home => lo,
            Key::End => hi,
            _ => continue,
        }
        .clamp(lo.min(hi), lo.max(hi));
    }
}

/// Whether a drag or a key moved the value. Bitwise, so a NaN the caller
/// handed in is not a change on every frame.
fn moved(before: f64, after: f64) -> bool {
    before.to_bits() != after.to_bits()
}

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

/// What the header says: the text the caller gave, or the value at two
/// decimals. Either way it is measured for the widest string it can say, so
/// digits coming and going never shuffle the label beside it.
///
/// ponytail: a caller's own text is its own reserve -- one string is all the
/// widget is told -- so pad it to its widest form if the units change width.
fn readout(given: Option<String>, value: f64, min: f64, max: f64) -> El {
    match given {
        Some(t) => text(t.clone()).reserve(t),
        None => {
            let (lo, hi) = (format!("{min:.2}"), format!("{max:.2}"));
            text(format!("{value:.2}")).reserve(if hi.len() > lo.len() { hi } else { lo })
        }
    }
}

/// The thumb's diameter and the lane it slides in, as shares of the
/// control's height.
const THUMB: f64 = 0.35;
const LANE: f64 = 0.45;

/// Label, readout, and a track whose fill and thumb are flex shares. Returns
/// the control and whether the value changed.
///
/// The whole lane is the target: a press on the track jumps the value there
/// and the drag carries on from it, a full-width drag sweeps the full range,
/// and a focused slider steps with the arrow keys. Call `Ui::edit` with the
/// same id to bracket the gesture for a host's automation: `Begin` on the
/// press, `End` on the release.
///
/// ```
/// use mui::prelude::*;
/// let mut ui = Ui::new(Theme::DEFAULT);
/// let mut gain = 0.5;
/// let (fader, changed) = slider(&mut ui, "gain", "Gain", &mut gain, 0.0..=1.0);
/// assert!(!changed && gain == 0.5, "no gesture, no change");
/// let fader = fader.size(S);
/// ```
pub fn slider(
    ui: &mut Ui,
    id: &str,
    label: &str,
    value: &mut f64,
    range: RangeInclusive<f64>,
) -> (Control, bool) {
    let before = *value;
    let (h, _) = ui.state(id);
    // Last frame's lane, less the thumb riding it, is the travel a
    // full-range drag covers; before the first frame nothing can drag.
    let (grip, travel) = ui
        .scene()
        .and_then(|s| s.surface(id))
        .map_or((0.0, 0.0), |s| {
            let grip = s.frame.size.height * THUMB / LANE + 2.0 * h;
            (grip, s.frame.size.width - grip)
        });
    if ui.get(id).pressed && travel > 0.0 {
        if let Some(p) = ui.local(id) {
            // Off the thumb, a press puts the thumb's centre under it.
            let at = unit(*value, &range) * travel + grip / 2.0;
            if (p.x - at).abs() > grip / 2.0 {
                let t = ((p.x - grip / 2.0) / travel).clamp(0.0, 1.0);
                *value = range.start() + t * (range.end() - range.start());
            }
        }
    }
    ui.drag(id, value, range.clone(), travel, false);
    stepped(ui, id, value, &range);
    let changed = moved(before, *value);
    let t = unit(*value, &range);
    let (id, label, value) = (id.to_owned(), label.to_owned(), *value);
    let (min, max) = (*range.start(), *range.end());
    let control = Control::new(ui, move |look| {
        // The rail is a fraction of the control's height, so one size token
        // moves the track, the thumb and the row together.
        let (track, thumb, lane) = (look.px * 0.15, look.px * THUMB, look.px * LANE);
        let grip = leaf(thumb + 2.0 * h, thumb + 2.0 * h)
            .pill()
            .fill(look.role);
        column([
            row([
                text(label.clone()),
                spacer(),
                readout(look.text.clone(), value, min, max).fill(Role::Dim),
            ])
            .gap(S),
            overlay([
                row([
                    leaf(0.0, track).grow(t).pill().fill(look.role),
                    leaf(0.0, track).grow(1.0 - t),
                ])
                .anchor(Align::Stretch, Align::Center)
                .pill()
                .preset(&look.face(Role::Field)),
                row([spacer().grow(t), grip, spacer().grow(1.0 - t)])
                    .anchor(Align::Stretch, Align::Center),
            ])
            .height(lane)
            .role(Kind::Slider { value, min, max })
            .label(label)
            .focusable()
            .id(id),
        ])
        .gap(Xs)
    });
    (control, changed)
}

/// A dial: vertical drag, pointer on a 270° sweep; returns the control and
/// whether the value changed. The pointer follows a
/// tween, so a value set from outside -- a preset, a host automation curve --
/// glides instead of jumping, while a drag still tracks the pointer.
///
/// Bracket it with `Ui::edit`, as for [`slider`]. The
/// diameter comes from the theme and the size token; `.px(72.0)` is the
/// hatch for a dial that has to match a hardware panel.
///
/// ```
/// use mui::prelude::*;
/// let mut ui = Ui::new(Theme::DEFAULT);
/// let mut cutoff = 0.5;
/// let (dial, changed) = knob(&mut ui, "cut", "Cutoff", &mut cutoff, 0.0..=1.0);
/// assert!(!changed && cutoff == 0.5, "no gesture, no change");
/// let dial = dial.size(Xl);
/// ```
pub fn knob(
    ui: &mut Ui,
    id: &str,
    label: &str,
    value: &mut f64,
    range: RangeInclusive<f64>,
) -> (Control, bool) {
    let before = *value;
    // Unlike a slider's, a dial's travel is a hand distance, not a geometry:
    // the whole face is the target and 120 px of vertical drag sweeps the
    // range whatever the diameter, so a small knob is not a twitchy one.
    ui.drag(id, value, range.clone(), 120.0, true);
    stepped(ui, id, value, &range);
    let changed = moved(before, *value);
    // Fast enough that a drag still feels direct, slow enough that a
    // preset change is a glide.
    let t = ui.tween_with(id, unit(*value, &range), Spring::new(0.12, 1.0));
    let (h, _) = ui.state(id);
    let (id, label, value) = (id.to_owned(), label.to_owned(), *value);
    let (min, max) = (*range.start(), *range.end());
    let control = Control::new(ui, move |look| {
        // A dial reads bigger than a button of the same size token: the
        // label sits under it rather than inside it.
        let size = look.px * 1.8;
        let a = (135.0 + 270.0 * t).to_radians();
        let r = size / 2.0 - size / 12.0;
        column([
            overlay([
                leaf(size, size)
                    .pill()
                    .preset(&look.face(Role::Raised))
                    .shell(size / 24.0 + 1.0 * h, Role::Field)
                    .role(Kind::Slider { value, min, max })
                    .label(label.clone())
                    .focusable()
                    .id(id),
                // The pointer is the reading, so it keeps the role at full
                // strength whatever the variant does to the face.
                leaf(size / 12.0, size / 12.0)
                    .pill()
                    .fill(look.role)
                    .anchor(Align::Center, Align::Center)
                    .offset(r * a.cos(), r * a.sin()),
            ]),
            text(look.text.clone().unwrap_or(label)).fill(Role::Dim),
        ])
        .gap(Xs)
        .align(Align::Center)
    });
    (control, changed)
}

/// A labelled action. Returns the control and whether it was clicked last
/// frame.
///
/// ```
/// use mui::prelude::*;
/// let mut ui = Ui::new(Theme::DEFAULT);
/// let (save, clicked) = button(&mut ui, "save", "Save");
/// assert!(!clicked, "nothing pressed it last frame");
/// let save = save.variant(Variant::Soft).size(S);
/// ```
pub fn button(ui: &mut Ui, id: &str, label: &str) -> (Control, bool) {
    let clicked = activated(ui, id);
    let (id, label) = (id.to_owned(), label.to_owned());
    let el = Control::new(ui, move |look| {
        let pad_y = ((look.px - 14.0) / 2.0).max(2.0);
        row([text(label.clone()).fill(look.ink())])
            .pad_xy(look.px * 0.4, pad_y)
            .pill()
            .preset(&look.style())
            .on(State::Hover, look.hover())
            .animate()
            .role(Kind::Button)
            .label(label)
            .focusable()
            .id(id)
    });
    (el, clicked)
}

/// A switch: the knob's side is a flex share, the click flips it. Returns
/// the control and whether it flipped.
///
/// ```
/// use mui::prelude::*;
/// let mut ui = Ui::new(Theme::DEFAULT);
/// let mut bypass = false;
/// let (sw, flipped) = toggle(&mut ui, "bypass", &mut bypass);
/// assert!(!flipped && !bypass, "nothing clicked it, so it did not flip");
/// let sw = sw.size(Xs);
/// ```
pub fn toggle(ui: &mut Ui, id: &str, on: &mut bool) -> (Control, bool) {
    let flipped = activated(ui, id);
    if flipped {
        *on = !*on;
    }
    let (id, on) = (id.to_owned(), *on);
    let t = f64::from(on);
    let control = Control::new(ui, move |look| {
        // A track is as wide as the control's height and a bit over half as
        // tall: 40 x 22 at the default theme's `M`.
        let (w, h) = (look.px, look.px * 0.55);
        row([
            spacer().grow(t),
            leaf(h * 0.73, h * 0.73).pill().fill(Role::Ink),
            spacer().grow(1.0 - t),
        ])
        .size(w, h)
        .pad(h * 0.14)
        .pill()
        .preset(&if on {
            look.style()
        } else {
            look.face(Role::Field)
        })
        .animate()
        .role(Kind::Toggle { on })
        .focusable()
        .id(id)
    });
    (control, flipped)
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
/// Unicode word boundaries, falling back to one grapheme for punctuation.
fn word(value: &str, at: usize) -> (usize, usize) {
    super::grapheme::word(value, at)
}
fn insert(value: &mut String, caret: &mut usize, c: char) {
    value.insert(byte(value, *caret), c);
    *caret += 1;
}

/// A single-line field: the text, a selection, a blinking caret, and the
/// edits the focused keys imply. Click to focus and set the caret, drag to
/// select, double click for a word, shift+arrows to extend. ctrl/cmd+A, C, X
/// and V select all, copy, cut and paste -- a copy leaves the text in
/// `Frame::clipboard` for the host to hand to the OS, and a
/// paste reads `Input::clipboard`, which the host fills on the paste key.
/// Returns the field and whether the value changed.
///
/// ```
/// use mui::prelude::*;
/// let mut ui = Ui::new(Theme::DEFAULT);
/// let mut name = String::from("Init");
/// let (field, changed) = text_input(&mut ui, "name", &mut name);
/// assert!(!changed, "nothing is focused, so nothing was typed");
/// let field = field.w(140);
/// ```
// ponytail: single line. A multi-line field wants the caret on a line index,
// not a byte, and `mui_text::break_lines` to place it.
pub fn text_input(ui: &mut Ui, id: &str, value: &mut String) -> (El, bool) {
    let size = ui.theme.text;
    let focused = ui.focused(id);
    let n = value.chars().count();
    let (anchor, caret) = ui.sel(id);
    let (mut anchor, mut caret) = (
        super::grapheme::floor(value, anchor.min(n)),
        super::grapheme::floor(value, caret.min(n)),
    );

    // The pointer, against last frame's field: the text starts `PAD` in,
    // slid left by the shift that kept last frame's caret in the room.
    // ponytail: a composition shown last frame is not in that shift; a click
    // mid-composition lands as if the preedit were not there.
    const PAD: f64 = 8.0;
    let room = ui
        .scene()
        .and_then(|s| s.surface(id))
        .map(|s| s.frame.size.width - 2.0 * PAD);
    let r = ui.get(id);
    if r.pressed || r.dragged {
        if let Some(p) = ui.local(id) {
            let shift = room.map_or(0.0, |room| {
                (ui.caret_x(value, size, byte(value, caret)) - room).max(0.0)
            });
            caret = super::grapheme::floor(value, ui.hit(value, size, p.x - PAD + shift));
            if r.pressed {
                anchor = caret;
            }
        }
    }
    if ui.double_click(id) {
        (anchor, caret) = word(value, caret);
    }

    // Only a frame with input can edit, so only that frame pays for the copy
    // the change is judged against.
    let before =
        (focused && !(ui.text(id).is_empty() && ui.keys(id).is_empty())).then(|| value.clone());
    if focused {
        let committed = ui.text(id).to_owned();
        let has_committed_text = !committed.is_empty();
        for c in committed.chars().filter(|c| !c.is_control()) {
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
                Key::Char(c) if !c.is_control() && !has_committed_text => {
                    (caret, _) = take(value, anchor, caret);
                    insert(value, &mut caret, c);
                    anchor = caret;
                }
                Key::Backspace => {
                    let (at, had) = take(value, anchor, caret);
                    caret = at;
                    if !had && caret > 0 {
                        let start = super::grapheme::previous(value, caret);
                        value.replace_range(byte(value, start)..byte(value, caret), "");
                        caret = start;
                    }
                    anchor = caret;
                }
                Key::Delete => {
                    let (at, had) = take(value, anchor, caret);
                    caret = at;
                    if !had && caret < value.chars().count() {
                        let end = super::grapheme::next(value, caret);
                        value.replace_range(byte(value, caret)..byte(value, end), "");
                    }
                    anchor = caret;
                }
                Key::Left | Key::Right | Key::Home | Key::End => {
                    caret = match k.key {
                        Key::Left if !k.mods.shift && anchor != caret => anchor.min(caret),
                        Key::Right if !k.mods.shift && anchor != caret => anchor.max(caret),
                        Key::Left => super::grapheme::previous(value, caret),
                        Key::Right => super::grapheme::next(value, caret),
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
    let changed = before.is_some_and(|b| b != *value);

    // The input method's composing text is shown at the caret and measured
    // with the value, but never joins it: only a commit, which arrives as
    // typed text above, edits `value`.
    let pre = focused
        .then(|| ui.preedit())
        .flatten()
        .map(|(t, c)| (t.to_owned(), c));
    let base = byte(value, caret);
    let mut shown = value.clone();
    if let Some((t, _)) = &pre {
        shown.insert_str(base, t);
    }
    let x = |b: usize| ui.caret_x(&shown, size, b);
    // The caret sits inside the preedit, where the IME put its cursor.
    let at = match &pre {
        Some((t, c)) => base + c.map_or(t.len(), |(s, _)| s.min(t.len())),
        None => base,
    };
    // ponytail: a selection is not painted under a composition -- its ends
    // were measured against the value and the preedit sits between them, so
    // the highlight is dropped for the frames the composition lasts. The
    // commit still replaces the selection. Measure the two runs separately if
    // composing over a selection ever needs to look right.
    let (lo, hi) = match &pre {
        Some(_) => (0.0, 0.0),
        None => (
            x(byte(value, anchor.min(caret))),
            x(byte(value, anchor.max(caret))),
        ),
    };
    let (plo, phi) = match &pre {
        Some((t, _)) => (x(base), x(base + t.len())),
        None => (0.0, 0.0),
    };
    let on = focused && ui.blink();
    // The value keeps its whole measured advance so the field stays one line
    // -- a plain `text()` would wrap to the frame and grow the field -- and
    // the frame clips it. Room comes from last frame's field, the only inner
    // width the widget can see, and the caret scrolls the three layers
    // together so it never leaves the box.
    // ponytail: the first frame of an over-long value shows its head; it
    // catches up on the next one.
    let run = x(shown.len());
    let room = room.unwrap_or(run);
    let caret_x = x(at);
    let shift = (caret_x - room).max(0.0);
    let el = overlay([
        leaf(hi - lo, size)
            .anchor(Align::Start, Align::Center)
            .offset(lo - shift, 0.0)
            .when(hi > lo, |e| e.fill(Role::Primary)),
        text(shown.clone())
            .width(run)
            .lines(1)
            .anchor(Align::Start, Align::Center)
            .offset(-shift, 0.0),
        leaf(2.0, size)
            .anchor(Align::Start, Align::Center)
            .offset(caret_x - shift, 0.0)
            .when(on, |e| e.fill(Role::Ink)),
        // Underline, last so the earlier children keep their keys.
        leaf(phi - plo, 2.0)
            .anchor(Align::Start, Align::End)
            .offset(plo - shift, 0.0)
            .when(phi > plo, |e| e.fill(Role::Ink)),
    ])
    .clip()
    .pad_xy(PAD, 6.0)
    .radius(6.0)
    .fill(Role::Field)
    // The ring is declared beside the resting look rather than rebuilt from
    // `focused` every frame; the runtime knows who has the focus.
    .on(State::Focus, |s| s.stroke(Role::Primary))
    .cursor(Cursor::Text)
    .focusable()
    .role(Kind::TextInput {
        value: value.clone(),
    })
    .id(id);
    if focused {
        ui.set_ime_caret(id, Point::new(PAD + caret_x - shift, 6.0), size);
    }
    (el, changed)
}
