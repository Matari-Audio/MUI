//! Controls as compositions. Nothing here is placed absolutely: a thumb sits
//! where two flex weights put it, a knob's pointer is an anchored offset.
use std::ops::RangeInclusive;

use mui_input::{FINE_DRAG, Key};
use mui_scene::prelude::*;
use mui_scene::{Palette, SpacingToken, Spring, Stroke};

use crate::Ui;
use crate::widgets::{TextOpts, text_edit};

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
    ui.get(id).activated()
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
    fn hover(&self) -> impl Fn(Style) -> Style + use<> {
        let (p, role, variant) = (self.palette, self.role, self.variant);
        move |s: Style| {
            let ground = p.surface();
            match variant {
                // Nothing to lift: the hover is the role, arriving.
                Variant::Outline | Variant::Ghost => s.fill(role.alpha(0.12)),
                _ => {
                    let lifted = s
                        .fill
                        .as_ref()
                        .unwrap_or(&Fill::None)
                        .map(&p, ground, |c| p.hover(c));
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
/// let strip = row![dial.size(L), body("post")];
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
    /// let el = clear.role(Role::Danger).variant(Variant::Outline).el();
    /// let ring = el.payload().style.stroke.clone().map(|s| s.fill);
    /// assert_eq!(ring, Some(Fill::Role(Role::Danger)));
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
pub(crate) fn step(range: &RangeInclusive<f64>) -> f64 {
    (range.end() - range.start()) / 100.0
}

/// Step `value` by the focused control's keys: arrows by a hundredth of
/// `range` (a tenth of that with Shift), Page Up and Down by ten steps, Home
/// and End to the ends. The runtime brackets the frame as one edit. Returns
/// whether the value moved.
///
/// The keyboard half of [`slider`] and [`knob`], for a control of your own:
///
/// ```
/// use mui::prelude::*;
/// let ui = Ui::new(Theme::DEFAULT);
/// let mut trim = 0.0;
/// assert!(!stepped(&ui, "trim", &mut trim, &(-12.0..=12.0)), "nothing is focused");
/// ```
pub fn stepped(ui: &Ui, id: &str, value: &mut f64, range: &RangeInclusive<f64>) -> bool {
    let (lo, hi) = (*range.start(), *range.end());
    if !(lo.is_finite() && hi.is_finite()) {
        return false;
    }
    let before = *value;
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
    moved(before, *value)
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
    if let Some(t) = given {
        text(t.clone()).reserve(t)
    } else {
        let (lo, hi) = (format!("{min:.2}"), format!("{max:.2}"));
        text(format!("{value:.2}")).reserve(if hi.len() > lo.len() { hi } else { lo })
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
/// and a focused slider steps with the arrow keys. As on a [`knob`], the
/// drawn thumb follows a tween, so a value set from outside glides. Call `Ui::edit` with the
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
    id: impl Into<Id>,
    label: &str,
    value: &mut f64,
    range: RangeInclusive<f64>,
) -> (Control, bool) {
    let id: Id = id.into();
    let before = *value;
    let (h, _) = ui.state(&id);
    // Last frame's lane, less the thumb riding it, is the travel a
    // full-range drag covers; before the first frame nothing can drag.
    let (grip, travel) = ui
        .scene()
        .and_then(|s| s.surface(&id))
        .map_or((0.0, 0.0), |s| {
            let grip = s.frame.size.height * THUMB / LANE + 2.0 * h;
            (grip, s.frame.size.width - grip)
        });
    if ui.get(&id).pressed
        && travel > 0.0
        && let Some(p) = ui.local(&id)
    {
        // Off the thumb, a press puts the thumb's centre under it.
        let at = unit(*value, &range) * travel + grip / 2.0;
        if (p.x - at).abs() > grip / 2.0 {
            let t = ((p.x - grip / 2.0) / travel).clamp(0.0, 1.0);
            *value = range.start() + t * (range.end() - range.start());
        }
    }
    ui.drag(&id, value, range.clone(), travel, false);
    stepped(ui, &id, value, &range);
    let changed = moved(before, *value);
    // The drawn thumb glides like a knob's pointer; the value, the readout
    // and the grab test above stay exact.
    let t = ui.tween_with(&id, unit(*value, &range), Spring::new(0.12, 1.0));
    let (label, value) = (label.to_owned(), *value);
    let (min, max) = (*range.start(), *range.end());
    let control = Control::new(ui, move |look| {
        // The rail is a fraction of the control's height, so one size token
        // moves the track, the thumb and the row together.
        let (track, thumb, lane) = (look.px * 0.15, look.px * THUMB, look.px * LANE);
        let grip = block(thumb + 2.0 * h, thumb + 2.0 * h)
            .pill()
            .fill(look.role);
        col([
            row([
                text(label.clone()),
                spacer(),
                readout(look.text.clone(), value, min, max).fill(Role::Dim),
            ])
            .gap(S),
            stack([
                row([
                    block(0.0, track).grow(t).pill().fill(look.role),
                    block(0.0, track).grow(1.0 - t),
                ])
                .anchor(Align::Stretch, Align::Center)
                .pill()
                .preset(look.face(Role::Field)),
                row([spacer().grow(t), grip, spacer().grow(1.0 - t)])
                    .anchor(Align::Stretch, Align::Center),
            ])
            .height(lane)
            .a11y(A11y::Slider { value, min, max })
            .named(label)
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
    id: impl Into<Id>,
    label: &str,
    value: &mut f64,
    range: RangeInclusive<f64>,
) -> (Control, bool) {
    let id: Id = id.into();
    let before = *value;
    // Unlike a slider's, a dial's travel is a hand distance, not a geometry:
    // the whole face is the target and 120 px of vertical drag sweeps the
    // range whatever the diameter, so a small knob is not a twitchy one.
    ui.drag(&id, value, range.clone(), 120.0, true);
    stepped(ui, &id, value, &range);
    let changed = moved(before, *value);
    // Fast enough that a drag still feels direct, slow enough that a
    // preset change is a glide.
    let t = ui.tween_with(&id, unit(*value, &range), Spring::new(0.12, 1.0));
    let (h, _) = ui.state(&id);
    let (label, value) = (label.to_owned(), *value);
    let (min, max) = (*range.start(), *range.end());
    let control = Control::new(ui, move |look| {
        // A dial reads bigger than a button of the same size token: the
        // label sits under it rather than inside it.
        let size = look.px * 1.8;
        let a = (135.0 + 270.0 * t).to_radians();
        let r = size / 2.0 - size / 12.0;
        col([
            stack([
                block(size, size)
                    .pill()
                    .preset(look.face(Role::Raised))
                    .shell(size / 24.0 + 1.0 * h, Role::Field)
                    .a11y(A11y::Slider { value, min, max })
                    .named(label.clone())
                    .focusable()
                    .id(id),
                // The pointer is the reading, so it keeps the role at full
                // strength whatever the variant does to the face.
                block(size / 12.0, size / 12.0)
                    .pill()
                    .fill(look.role)
                    .centered_at(r * a.cos(), r * a.sin()),
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
pub fn button(ui: &mut Ui, id: impl Into<Id>, label: &str) -> (Control, bool) {
    let id: Id = id.into();
    let clicked = activated(ui, &id);
    let label = label.to_owned();
    let el = Control::new(ui, move |look| {
        let pad_y = ((look.px - 14.0) / 2.0).max(2.0);
        row([text(label.clone()).fill(look.ink())])
            .pad_xy(look.px * 0.4, pad_y)
            .pill()
            .preset(look.style())
            .on(State::Hover, look.hover())
            .animate()
            .a11y(A11y::Button)
            .named(label)
            .focusable()
            .id(id)
    });
    (el, clicked)
}

/// A switch: the knob's side is a flex share it slides between, the click
/// flips it. Returns the control and whether it flipped.
///
/// ```
/// use mui::prelude::*;
/// let mut ui = Ui::new(Theme::DEFAULT);
/// let mut bypass = false;
/// let (sw, flipped) = toggle(&mut ui, "bypass", &mut bypass);
/// assert!(!flipped && !bypass, "nothing clicked it, so it did not flip");
/// let sw = sw.size(Xs);
/// ```
pub fn toggle(ui: &mut Ui, id: impl Into<Id>, on: &mut bool) -> (Control, bool) {
    let id: Id = id.into();
    let flipped = activated(ui, &id);
    if flipped {
        *on = !*on;
    }
    let on = *on;
    let t = f64::from(on);
    let control = Control::new(ui, move |look| {
        // A track is as wide as the control's height and a bit over half as
        // tall: 40 x 22 at the default theme's `M`.
        let (w, h) = (look.px, look.px * 0.55);
        row([
            spacer().grow(t),
            // The knob's side is a flex share; its frame glides between them.
            block(h * 0.73, h * 0.73)
                .pill()
                .fill(Role::Ink)
                .animate_layout(),
            spacer().grow(1.0 - t),
        ])
        .size(w, h)
        .pad(h * 0.14)
        .pill()
        .preset(if on {
            look.style()
        } else {
            look.face(Role::Field)
        })
        .animate()
        .a11y(A11y::Toggle { on })
        .focusable()
        .id(id)
    });
    (control, flipped)
}

/// How far a [`drag_value`] is dragged to sweep its whole range.
const DRAG_TRAVEL: f64 = 200.0;

/// A number to drag: a horizontal drag of 200 px sweeps `range` (Shift is
/// fine), the arrow keys step it, and a double click -- or Enter while it
/// is focused -- turns it into a field to type the number into. Enter or a
/// click away takes what was typed, Escape leaves the value alone. Returns
/// the control and whether the value changed. `.value_text(..)` says it
/// in units, as for a [`slider`].
///
/// ```
/// use mui::prelude::*;
/// let mut ui = Ui::new(Theme::DEFAULT);
/// let mut bpm = 120.0;
/// let (tempo, changed) = drag_value(&mut ui, "bpm", &mut bpm, 20.0..=300.0);
/// assert!(!changed, "no gesture, no change");
/// ```
pub fn drag_value(
    ui: &mut Ui,
    id: impl Into<Id>,
    value: &mut f64,
    range: RangeInclusive<f64>,
) -> (Control, bool) {
    let id: Id = id.into();
    let before = *value;
    let (lo, hi) = (
        range.start().min(*range.end()),
        range.start().max(*range.end()),
    );
    let field = format!("{id}/edit");
    // What was typed, and the width the number had when typing started.
    let mut typing = ui.stash::<(String, f64)>(&field).cloned();
    let r = ui.get(&id);
    // Focus moves to the field only once it is built: this frame's keys --
    // the Enter that opened it -- are not the field's to type.
    let opening = typing.is_none() && (r.double_clicked || r.key_activated);
    if opening {
        let width = ui
            .scene()
            .and_then(|s| s.surface(&id))
            .map_or(ui.theme.control * 16.0, |s| s.frame.size.width);
        typing = Some((format!("{value}"), width));
    }
    if let Some((mut s, width)) = typing {
        let take = |s: &str, value: &mut f64| {
            if let Ok(v) = s.trim().parse::<f64>()
                && v.is_finite()
            {
                *value = v.clamp(lo, hi);
            }
        };
        if opening || ui.focused(&field) {
            let opts = TextOpts {
                blur_on_submit: true,
                ..TextOpts::default()
            };
            let (el, e) = text_edit(ui, &field, &mut s, opts);
            if opening {
                ui.set_sel(&field, 0, s.chars().count());
                ui.focus(field.clone());
            }
            if !e.submitted {
                ui.set_stash(&field, Some((s, width)));
                let control = Control::new(ui, move |_| el.w(width));
                return (control, false);
            }
            take(&s, value);
        } else if !ui.shortcuts().iter().any(|k| k.key == Key::Escape) {
            // The focus went elsewhere: a click away keeps what was typed.
            take(&s, value);
        }
        ui.set_stash::<(String, f64)>(&field, None);
    } else {
        ui.drag(&id, value, range.clone(), DRAG_TRAVEL, false);
        stepped(ui, &id, value, &range);
    }
    let changed = moved(before, *value);
    let (value, min, max) = (*value, *range.start(), *range.end());
    let control = Control::new(ui, move |look| {
        let pad_y = ((look.px - 14.0) / 2.0).max(2.0);
        row([readout(look.text.clone(), value, min, max)])
            .pad_xy(look.px * 0.3, pad_y)
            .radius(4.0)
            .preset(look.face(Role::Field))
            .cursor(Cursor::ResizeH)
            .a11y(A11y::Slider { value, min, max })
            .focusable()
            .id(id)
    });
    (control, changed)
}
