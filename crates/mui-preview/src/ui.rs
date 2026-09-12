//! The gallery's own chrome, built out of the library it is previewing.
//!
//! Every widget here is a hand-built [`Path`] routed by `mui-input` -- not a
//! [`SceneSpec`](mui_core::SceneSpec). That is deliberate: the chrome must not
//! run the code under test. If a boolean or fillet regression breaks the
//! resolver, the gallery has to stay usable enough to *show* you that.
//!
//! Everything is in physical pixels. Widget sizes are authored in logical
//! points and multiplied by the device scale exactly once, in [`Ui::px`]; if a
//! second scale conversion ever appears in this program, one of them is wrong.

use std::ops::RangeInclusive;
use std::sync::Arc;

use mui_core::{Color, Mode, Palette, Pigment};
use mui_geometry::{Bounds, Path, Point, RoundedRect};
use mui_input::{Hit, Interaction, PointerInput, Response};
use mui_text::TextRun;
use mui_vello::ARC_TOLERANCE;
use vello_common::peniko::color::AlphaColor;

pub type Rgba = AlphaColor<vello_common::peniko::color::Srgb>;

/// Two hues. Every other colour this file paints -- every surface, every ink,
/// every hover, and the whole light theme -- is derived from them, so there is
/// no second table of literals to keep in step with this one.
///
/// A [`Pigment`] carries no lightness on purpose: lightness is what [`Mode`]
/// assigns, which is why the same declaration serves both themes.
pub const SKIN: Palette = Palette {
    neutral: Pigment::new(264.0, 0.015),
    primary: Pigment::new(242.0, 0.131),
    secondary: Pigment::new(310.0, 0.120),
    tertiary: Pigment::new(190.0, 0.110),
    ..Palette::NEUTRAL
};

/// The gallery's skin, lit whichever way the sidebar checkbox says.
pub fn skin(light: bool) -> Palette {
    SKIN.with_mode(if light { Mode::Light } else { Mode::Dark })
}

/// Where a control rests when nothing is happening to it. Raised things sit
/// above the panel; recessed things -- list rows, a slider track, a field --
/// sit below it, so the panel reads as the ground between them.
const RAISED: i32 = 3;
const RECESSED: i32 = -1;

/// What the pointer is doing to a control, applied to whatever colour its own
/// state says it rests at. Every widget below uses this instead of its own
/// table of state combinations.
///
/// A press only counts while the pointer is still over the widget: a press
/// dragged off is no longer a click, so it must stop looking like one.
/// Logical points. Physical pixels are these times the device scale.
const PAD: f64 = 12.0;
const ROW: f64 = 24.0;
const GAP: f64 = 6.0;
const RADIUS: f64 = 5.0;
const TEXT_SIZE: f64 = 13.0;
const NOTE_SIZE: f64 = 11.0;

/// How many characters of `size`-pixel text fit in `width`.
///
/// ponytail: estimated from the advance of a typical character, not measured.
/// Hack is monospace so this is exact there and a little conservative on a
/// proportional face. Measuring per candidate line would mean laying every
/// line out twice; a column of labels does not need that.
fn budget(width: f64, size: f64) -> usize {
    (width / (size * 0.62)).floor().max(1.0) as usize
}

/// Greedy word wrap. A word longer than a line is cut rather than allowed to
/// run off the panel -- an id or a file path is still readable clipped, and a
/// line that overruns the column reads as a rendering bug.
fn wrap(text: &str, budget: usize) -> Vec<String> {
    // Zero would drain nothing and spin: `budget()` never returns it, but this
    // is called directly from tests.
    let budget = budget.max(1);
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let room = budget.saturating_sub(line.chars().count() + usize::from(!line.is_empty()));
        if word.chars().count() > room {
            if !line.is_empty() {
                lines.push(std::mem::take(&mut line));
            }
            let mut rest: Vec<char> = word.chars().collect();
            while rest.len() > budget {
                lines.push(rest.drain(..budget).collect());
            }
            line = rest.into_iter().collect();
            continue;
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() || lines.is_empty() {
        lines.push(line);
    }
    lines
}

/// Chrome state that has to outlive a frame.
///
/// The font arrives as bytes, not as a path, because every label is re-derived
/// as geometry through `mui-text` -- which never opens a file, which is the rule
/// that keeps the library crates compiling for wasm.
pub struct Chrome {
    interaction: Interaction,
    /// Committed at the end of the previous frame. A widget is laid out,
    /// hit-tested and painted in one pass; what it hit-tests against is where
    /// it was last frame, and no widget reads its own geometry to decide its
    /// value, so there is no lag anyone can see.
    hit: Hit,
    focus: Option<String>,
    font: Arc<Vec<u8>>,
    /// How far the column is scrolled, in physical pixels, and how tall it was
    /// the last time it was laid out. A scene with more axes than the window
    /// has room for -- Roboto Flex has thirteen -- is otherwise not merely
    /// clipped but unreachable: the sliders lay out past the bottom edge and
    /// the pointer can never get to them.
    scroll: f64,
    content: f64,
    /// Which way the gallery is lit. One field, because that is the whole
    /// theme switch -- see [`Palette::with_mode`].
    skin: Palette,
}

impl Chrome {
    pub fn new(font: Arc<Vec<u8>>) -> Self {
        Self {
            interaction: Interaction::new(),
            hit: Hit::default(),
            focus: None,
            font,
            scroll: 0.0,
            content: 0.0,
            skin: SKIN,
        }
    }

    /// Whether a gesture is in flight on a widget. The stage asks so that a
    /// slider drag that wanders off the column keeps the slider, not the
    /// specimen underneath it.
    /// Scroll the column by `delta` physical pixels. Clamped at the next
    /// layout, when the content height is known again.
    /// Repaint everything from here on in a different light.
    pub fn set_skin(&mut self, skin: Palette) {
        self.skin = skin;
    }

    pub fn scroll_by(&mut self, delta: f64) {
        self.scroll -= delta;
    }

    pub fn busy(&self) -> bool {
        self.interaction.held().is_some()
    }

    /// The widget id under a point, from the geometry committed last frame.
    /// Only the tests ask: the gallery itself never needs to know what a
    /// widget is before touching it.
    #[cfg(test)]
    pub fn at(&self, p: Point) -> Option<&str> {
        self.hit.at(p)
    }

    /// Begin a column of widgets inside `bounds`.
    ///
    /// Takes a raw [`PointerInput`] rather than a winit event so a test can
    /// drive a slider without a window; that is the only reason the seam is
    /// here. `bounds` and `pointer` are both physical pixels, and so is every
    /// path this produces -- the chrome converts nothing, so what responds is
    /// literally what was drawn.
    pub fn column(
        &mut self,
        bounds: Bounds,
        pointer: PointerInput,
        typed: Option<char>,
        scale: f64,
    ) -> Ui<'_> {
        self.interaction.update(&self.hit, pointer);
        // A press that landed on no widget at all -- the panel background, or
        // the stage -- drops keyboard focus. `Ui::register` covers a press that
        // landed on some *other* widget; between them that is the whole model.
        if pointer.primary_down && self.interaction.held().is_none() {
            self.focus = None;
        }
        // Clamped against what the *previous* frame measured, the same
        // one-frame-late bargain the hit geometry already makes.
        self.scroll = self
            .scroll
            .clamp(0.0, (self.content - bounds.height()).max(0.0));
        let mut paint = Vec::new();
        if let Ok(rect) = RoundedRect::new(bounds, 0.0) {
            paint.push((rect.path(), self.skin.layer(0).to_srgb()));
        }
        let cursor = bounds.min.y + PAD * scale - self.scroll;
        Ui {
            skin: self.skin,
            chrome: self,
            bounds,
            scale,
            pointer,
            typed,
            cursor,
            ordinal: 0,
            scope: String::new(),
            paint,
            next_hit: Hit::default(),
        }
    }
}

/// One frame of one column of widgets.
pub struct Ui<'a> {
    chrome: &'a mut Chrome,
    /// Copied out of [`Chrome`] for the frame, so a widget can read a colour
    /// while it is holding a mutable borrow of everything else.
    skin: Palette,
    bounds: Bounds,
    scale: f64,
    pointer: PointerInput,
    typed: Option<char>,
    /// Top of the next widget, in physical pixels.
    cursor: f64,
    /// Interactive widgets only, so a conditional [`Ui::note`] cannot shift the
    /// id of everything after it.
    ordinal: usize,
    /// Prefixed onto every id, so two scenes cannot collide on one.
    scope: String,
    paint: Vec<(Path, Rgba)>,
    next_hit: Hit,
}

impl Ui<'_> {
    /// Logical points to physical pixels. The only scale conversion in the
    /// program.
    pub fn px(&self, logical: f64) -> f64 {
        logical * self.scale
    }

    /// What the pointer is doing to a control, applied to whatever colour its
    /// own state says it rests at. Every widget below uses this instead of its
    /// own table of state combinations.
    ///
    /// A press only counts while the pointer is still over the widget: a press
    /// dragged off is no longer a click, so it must stop looking like one.
    fn fill_for(&self, base: Color, r: &Response) -> Color {
        if r.held && r.hovered {
            self.skin.pressed(base)
        } else if r.hovered {
            self.skin.hover(base)
        } else {
            base
        }
    }

    /// The left and right edges of a widget row.
    fn span(&self) -> (f64, f64) {
        (
            self.bounds.min.x + self.px(PAD),
            self.bounds.max.x - self.px(PAD),
        )
    }

    fn advance(&mut self, height: f64) {
        self.cursor += height + self.px(GAP);
    }

    fn rect(&mut self, bounds: Bounds, radius: f64, ink: Rgba) {
        if let Ok(shape) = RoundedRect::new(bounds, radius) {
            self.paint.push((shape.path(), ink));
        }
    }

    /// Lay out `text`, truncated to what `max_width` can hold.
    ///
    /// Truncation, not wrapping, because every caller of this is a widget with
    /// one row. Prose goes through [`Ui::note`], which wraps.
    fn run(&self, text: &str, size: f64, max_width: f64) -> Option<TextRun> {
        let budget = budget(max_width, size);
        let owned;
        let text = if text.chars().count() > budget {
            owned = text
                .chars()
                .take(budget.saturating_sub(2))
                .chain("..".chars())
                .collect::<String>();
            owned.as_str()
        } else {
            text
        };
        mui_text::text_run(&self.chrome.font, text, size, &[], ARC_TOLERANCE).ok()
    }

    /// Put a laid-out run down with its baseline centred in a row.
    fn place(&mut self, run: &TextRun, x: f64, row_top: f64, row_height: f64, ink: Rgba) {
        let baseline = row_top + (row_height - (run.ascent + run.descent)) / 2.0 + run.ascent;
        if let Ok(path) = run.path.rigid_transform(Point::new(x, baseline), 0.0) {
            self.paint.push((path, ink));
        }
    }

    /// Claim the next interactive widget's id.
    ///
    /// Separate from [`Ui::register`] so it can happen *before* the widget's
    /// shape is built: a row that fails to shape must still consume its
    /// ordinal, or it renumbers every widget below it on the frame it fails.
    fn claim(&mut self, label: &str) -> String {
        let id = format!("{}{}:{}", self.scope, self.ordinal, label);
        self.ordinal += 1;
        id
    }

    /// Read what the pointer did to `id` last frame, and register `path` as
    /// its target for the next one. A click on any other widget drops keyboard
    /// focus; a click on nothing at all is handled in [`Chrome::column`].
    fn register(&mut self, id: &str, path: &Path) -> Response {
        let response = self.chrome.interaction.get(id);
        if response.clicked && self.chrome.focus.as_deref() != Some(id) {
            self.chrome.focus = None;
        }
        if let Err(e) = self.next_hit.push(id.to_string(), path) {
            // A widget that paints but cannot be clicked is invisible to
            // everyone but the person wondering why it does nothing.
            eprintln!("{id}: unclickable: {e}");
        }
        response
    }

    /// Namespace every id claimed from here on.
    ///
    /// Scene controls all start at the same ordinal, so two scenes whose first
    /// slider happened to share a label would share an id -- and on the frame
    /// after a switch, a press resolved against the old geometry would land on
    /// the new scene's slider and fling it to the pointer. One prefix closes
    /// the class before a second scene grows a control.
    pub fn scope(&mut self, name: &str) {
        self.scope = format!("{name}/");
    }

    /// The preamble every full-width widget shares: claim an id, shape the
    /// row, register it. Returns `None` when the row cannot be shaped, having
    /// already advanced the cursor -- the caller cannot forget to, and both
    /// exit paths stay in one place.
    fn row(&mut self, label: &str, height: f64, radius: f64) -> Option<(Bounds, Path, Response)> {
        let (x0, x1) = self.span();
        let bounds = Bounds::new(x0, self.cursor, x1, self.cursor + height);
        let id = self.claim(label);
        let Ok(shape) = RoundedRect::new(bounds, radius) else {
            self.advance(height);
            return None;
        };
        let path = shape.path();
        let response = self.register(&id, &path);
        Some((bounds, path, response))
    }

    pub fn label(&mut self, text: &str) {
        self.text_row(
            text,
            TEXT_SIZE,
            self.skin.on(self.skin.background()).to_srgb(),
        );
    }

    /// Small and dimmed: provenance, counts, the line under a heading.
    pub fn note(&mut self, text: &str) {
        self.text_row(
            text,
            NOTE_SIZE,
            self.skin.dim(self.skin.background()).to_srgb(),
        );
    }

    /// A note in the error ink. Same row, so a failure never moves the layout
    /// around underneath the pointer.
    pub fn error(&mut self, text: &str) {
        self.text_row(
            text,
            NOTE_SIZE,
            self.skin
                .danger()
                .readable_on(self.skin.background(), Palette::AA_TEXT)
                .to_srgb(),
        );
    }

    /// Prose, wrapped across as many rows as it needs. The gap goes after the
    /// paragraph, not between its lines, so a wrapped note reads as one thing.
    fn text_row(&mut self, text: &str, size: f64, ink: Rgba) {
        let (x0, x1) = self.span();
        let size = self.px(size);
        let height = size * 1.4;
        for line in wrap(text, budget(x1 - x0, size)) {
            if let Some(run) = self.run(&line, size, x1 - x0) {
                let top = self.cursor;
                self.place(&run, x0, top, height, ink);
            }
            self.cursor += height;
        }
        self.cursor += self.px(GAP);
    }

    /// Every token the palette exposes, in two rows: the surfaces it derives
    /// from one ground, then the roles it places at one accent lightness.
    ///
    /// This is here to be looked at. Ticking "light theme" flips one field and
    /// both rows have to stay legible and stay in the same order -- if a theme
    /// switch were two tables of literals, this is where they would drift.
    pub fn swatches(&mut self) {
        let surfaces = [
            self.skin.field(),
            self.skin.background(),
            self.skin.surface(),
            self.skin.raised(),
        ];
        let roles = [
            self.skin.primary(),
            self.skin.secondary(),
            self.skin.tertiary(),
            self.skin.success(),
            self.skin.warning(),
            self.skin.danger(),
        ];
        for row in [&surfaces[..], &roles[..]] {
            let (x0, x1) = self.span();
            let y = self.cursor;
            let height = self.px(ROW * 0.6);
            let width = (x1 - x0) / row.len() as f64;
            for (i, colour) in row.iter().enumerate() {
                let x = x0 + width * i as f64;
                self.rect(
                    Bounds::new(x, y, x + width, y + height),
                    self.px(2.0),
                    colour.to_srgb(),
                );
            }
            self.advance(height);
        }
    }

    pub fn separator(&mut self) {
        let (x0, x1) = self.span();
        let y = self.cursor;
        let thickness = self.px(1.0).max(1.0);
        self.rect(
            Bounds::new(x0, y, x1, y + thickness),
            0.0,
            self.skin.layer(RAISED).to_srgb(),
        );
        self.advance(thickness);
    }

    pub fn button(&mut self, text: &str) -> bool {
        let height = self.px(ROW);
        let Some((b, path, r)) = self.row(text, height, self.px(RADIUS)) else {
            return false;
        };
        let (x0, x1, y) = (b.min.x, b.max.x, b.min.y);
        let fill = self.fill_for(self.skin.layer(RAISED), &r);
        self.paint.push((path, fill.to_srgb()));
        if let Some(run) = self.run(text, self.px(TEXT_SIZE), x1 - x0) {
            let x = x0 + ((x1 - x0) - run.advance) / 2.0;
            self.place(&run, x.max(x0), y, height, self.skin.on(fill).to_srgb());
        }
        self.advance(height);
        r.clicked
    }

    /// One row of a mutually exclusive list. Returns whether it was clicked, so
    /// the caller keeps the "which one" state and this keeps none of it.
    pub fn option(&mut self, selected: bool, text: &str) -> bool {
        let height = self.px(ROW);
        let Some((b, path, r)) = self.row(text, height, self.px(RADIUS)) else {
            return false;
        };
        let (x0, x1, y) = (b.min.x, b.max.x, b.min.y);
        // Selection picks the resting colour; the pointer lifts whichever one
        // that is. A selected row therefore still answers a hover instead of
        // sitting flat at the accent.
        let fill = self.fill_for(
            if selected {
                self.skin.primary()
            } else {
                self.skin.layer(RECESSED)
            },
            &r,
        );
        self.paint.push((path, fill.to_srgb()));
        if let Some(run) = self.run(text, self.px(TEXT_SIZE), x1 - x0 - self.px(PAD)) {
            self.place(
                &run,
                x0 + self.px(GAP),
                y,
                height,
                self.skin.on(fill).to_srgb(),
            );
        }
        self.advance(height);
        r.clicked
    }

    pub fn checkbox(&mut self, on: &mut bool, text: &str) -> bool {
        let height = self.px(ROW);
        let Some((b, _, r)) = self.row(text, height, self.px(RADIUS)) else {
            return false;
        };
        let (x0, x1, y) = (b.min.x, b.max.x, b.min.y);
        if r.clicked {
            *on = !*on;
        }
        let side = height * 0.62;
        let box_top = y + (height - side) / 2.0;
        let fill = self.fill_for(
            if *on {
                self.skin.primary()
            } else {
                self.skin.layer(RECESSED)
            },
            &r,
        );
        self.rect(
            Bounds::new(x0, box_top, x0 + side, box_top + side),
            self.px(3.0),
            fill.to_srgb(),
        );
        if let Some(run) = self.run(text, self.px(TEXT_SIZE), x1 - x0 - side) {
            self.place(
                &run,
                x0 + side + self.px(GAP),
                y,
                height,
                self.skin.on(self.skin.background()).to_srgb(),
            );
        }
        self.advance(height);
        r.clicked
    }

    /// Returns whether `value` moved this frame -- the signal a caller needs to
    /// know whether anything downstream has to be rebuilt.
    pub fn slider(&mut self, value: &mut f32, range: RangeInclusive<f32>, text: &str) -> bool {
        let label_h = self.px(TEXT_SIZE) * 1.4;
        let track_h = self.px(6.0);
        let knob = self.px(14.0);
        let height = label_h + knob;

        let Some((b, _, r)) = self.row(text, height, 0.0) else {
            return false;
        };
        let (x0, x1, y) = (b.min.x, b.max.x, b.min.y);

        // The track is inset by half a knob so the knob's centre can reach both
        // ends without any part of it leaving the row.
        let (t0, t1) = (x0 + knob / 2.0, x1 - knob / 2.0);
        let before = *value;
        if r.held {
            if let Some(p) = self.pointer.pos {
                // A row narrower than a knob would divide by zero, and NaN is
                // the worst possible value to hand back: it compares unequal to
                // itself, so `changed` would be true on every idle frame and
                // the caller would re-solve forever.
                let t = if t1 > t0 {
                    ((p.x - t0) / (t1 - t0)).clamp(0.0, 1.0) as f32
                } else {
                    0.0
                };
                let next = range.start() + (range.end() - range.start()) * t;
                if next.is_finite() {
                    *value = next;
                }
            }
        }
        let span = range.end() - range.start();
        let t = if span.abs() > f32::EPSILON {
            ((*value - range.start()) / span).clamp(0.0, 1.0) as f64
        } else {
            0.0
        };

        let track_y = y + label_h + (knob - track_h) / 2.0;
        self.rect(
            Bounds::new(x0, track_y, x1, track_y + track_h),
            track_h / 2.0,
            self.skin.layer(RECESSED).to_srgb(),
        );
        let filled = t0 + (t1 - t0) * t;
        // The knob and the filled track are one control, so they share a
        // colour -- and a drag that leaves the row keeps it, because a slider
        // being dragged is still being dragged out there.
        let lit = if r.held {
            self.skin.pressed(self.skin.primary())
        } else if r.hovered {
            self.skin.hover(self.skin.primary())
        } else {
            self.skin.primary()
        }
        .to_srgb();
        if filled > x0 {
            self.rect(
                Bounds::new(x0, track_y, filled, track_y + track_h),
                track_h / 2.0,
                lit,
            );
        }
        self.rect(
            Bounds::new(
                filled - knob / 2.0,
                y + label_h,
                filled + knob / 2.0,
                y + label_h + knob,
            ),
            knob / 2.0,
            lit,
        );
        if let Some(run) = self.run(&format!("{text}  {value:.2}"), self.px(TEXT_SIZE), x1 - x0) {
            self.place(
                &run,
                x0,
                y,
                label_h,
                self.skin.on(self.skin.background()).to_srgb(),
            );
        }
        self.advance(height);
        *value != before
    }

    /// Exactly one character, because that is what a glyph preview is. Takes
    /// `&mut char` rather than `&mut String` so there is no empty state to
    /// decide what to do about.
    pub fn char_field(&mut self, value: &mut char, text: &str) -> bool {
        let (x0, x1) = self.span();
        let (y, height) = (self.cursor, self.px(ROW + 8.0));
        let side = height;
        let field = Bounds::new(x0, y, x0 + side, y + height);
        // Claimed before the shape can fail: the ordinal has to be consumed
        // either way or every widget below this one is renumbered.
        let id = self.claim(text);
        let Ok(shape) = RoundedRect::new(field, self.px(RADIUS)) else {
            self.advance(height);
            return false;
        };
        let path = shape.path();
        let r = self.register(&id, &path);
        if r.clicked {
            self.chrome.focus = Some(id.clone());
        }
        let focused = self.chrome.focus.as_deref() == Some(id.as_str());

        let mut changed = false;
        if focused {
            if let Some(c) = self.typed.take() {
                if !c.is_control() && *value != c {
                    *value = c;
                    changed = true;
                }
            }
        }

        let fill = self.fill_for(
            if focused {
                self.skin.primary()
            } else {
                self.skin.layer(RECESSED)
            },
            &r,
        );
        self.paint.push((path, fill.to_srgb()));
        let glyph = value.to_string();
        if let Some(run) = self.run(&glyph, self.px(TEXT_SIZE + 3.0), side) {
            let x = x0 + (side - run.advance) / 2.0;
            self.place(&run, x.max(x0), y, height, self.skin.on(fill).to_srgb());
        }
        if let Some(run) = self.run(text, self.px(TEXT_SIZE), x1 - x0 - side) {
            self.place(
                &run,
                x0 + side + self.px(GAP),
                y,
                height,
                self.skin.dim(self.skin.background()).to_srgb(),
            );
        }
        self.advance(height);
        changed
    }

    /// The paint list in call order, and the point at which this frame's hit
    /// geometry is committed for the next one. Consuming `self` is the whole
    /// enforcement: a widget added after the commit could never be hit.
    pub fn finish(self) -> Vec<(Path, Rgba)> {
        self.chrome.content = self.cursor - self.bounds.min.y + self.chrome.scroll + self.px(PAD);
        self.chrome.hit = self.next_hit;
        self.paint
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chrome() -> Chrome {
        Chrome::new(Arc::new(epaint_default_fonts::HACK_REGULAR.to_vec()))
    }

    fn bounds() -> Bounds {
        Bounds::new(0., 0., 240., 600.)
    }

    /// Ticking "light theme" is one field, and it has to reach everything: the
    /// panel, every swatch, and the ink on top of them. Repainting the same
    /// frame in both modes must produce no colour twice in the same slot --
    /// anything that came out identical is a literal the flip did not reach.
    #[test]
    fn the_theme_flip_reaches_every_painted_colour() {
        fn frame(light: bool) -> Vec<Rgba> {
            let mut chrome = chrome();
            chrome.set_skin(skin(light));
            let mut ui = chrome.column(bounds(), PointerInput::default(), None, 1.0);
            ui.label("heading");
            ui.note("provenance");
            ui.swatches();
            ui.button("press me");
            ui.separator();
            ui.finish().iter().map(|(_, ink)| *ink).collect()
        }
        let (dark, light) = (frame(false), frame(true));
        assert_eq!(dark.len(), light.len(), "the flip changed the geometry");
        assert!(!dark.is_empty());
        for (i, (d, l)) in dark.iter().zip(&light).enumerate() {
            assert_ne!(d, l, "paint {i} is the same colour in both themes");
        }
    }

    /// Whatever the mode, a label has to clear AA against the panel it sits on.
    /// `mui-core` proves this for the palette; this proves the gallery asks for
    /// the right pair, which is the half a colour crate cannot check.
    #[test]
    fn both_themes_are_legible() {
        for light in [false, true] {
            let s = skin(light);
            let ground = s.background();
            for (name, ink) in [("on", s.on(ground)), ("dim", s.dim(ground))] {
                let got = ink.contrast(ground);
                assert!(
                    got >= Palette::AA_TEXT,
                    "{name} ink is {got:.2}:1 on the panel (light: {light})"
                );
            }
        }
    }

    /// Prose fills the column and never overruns it, and a word too long for
    /// one line is cut rather than allowed to escape the panel.
    #[test]
    fn notes_wrap_inside_the_column() {
        assert_eq!(wrap("", 10), vec![""], "an empty note is still one row");
        assert_eq!(wrap("one two three", 7), vec!["one two", "three"]);
        assert_eq!(
            wrap("a supercalifragilistic", 6),
            vec!["a", "superc", "alifra", "gilist", "ic"]
        );
        for line in wrap(
            "Boolean union first, fillets second, and the shell is an offset",
            20,
        ) {
            assert!(line.chars().count() <= 20, "{line:?} overruns");
        }
    }

    /// Device scale enters the program exactly once. If a second conversion
    /// ever appears, one of them is wrong.
    #[test]
    fn scale_enters_once() {
        // The same column at two scales, in bounds scaled to match, must land
        // on exactly proportional rows. Asserting that `px` multiplies could
        // never see a *second* multiplication somewhere else in the layout,
        // which is the thing the module comment actually promises.
        fn rows(scale: f64) -> Vec<f64> {
            let mut chrome = chrome();
            let (mut flag, mut value) = (false, 50.0_f32);
            let mut ui = chrome.column(
                Bounds::new(0., 0., 240. * scale, 600. * scale),
                PointerInput::default(),
                None,
                scale,
            );
            let mut out = vec![ui.cursor];
            ui.label("heading");
            out.push(ui.cursor);
            ui.button("press me");
            out.push(ui.cursor);
            ui.checkbox(&mut flag, "flag");
            out.push(ui.cursor);
            ui.slider(&mut value, 0.0..=100.0, "size");
            out.push(ui.cursor);
            ui.separator();
            out.push(ui.cursor);
            ui.finish();
            out
        }
        let (one, two) = (rows(1.0), rows(2.0));
        assert!(
            one.windows(2).all(|w| w[1] > w[0]),
            "the column never moved"
        );
        for (i, (a, b)) in one.iter().zip(&two).enumerate() {
            assert_eq!(*b, a * 2.0, "row {i} is not proportional to the scale");
        }
    }

    /// An idle frame reports nothing changed, whatever the widgets are.
    #[test]
    fn an_idle_frame_moves_nothing() {
        let mut chrome = chrome();
        let mut ui = chrome.column(bounds(), PointerInput::default(), None, 1.0);
        let mut on = false;
        let mut size = 220.0_f32;
        let mut glyph = 'a';
        assert!(!ui.button("Rebuild"));
        assert!(!ui.option(true, "Pill tab"));
        assert!(!ui.checkbox(&mut on, "Fill"));
        assert!(!ui.slider(&mut size, 24.0..=400.0, "size"));
        assert!(!ui.char_field(&mut glyph, "glyph"));
        assert!(!ui.finish().is_empty(), "an idle frame still paints");
        assert_eq!((on, size, glyph), (false, 220.0, 'a'));
    }

    /// Two frames: hit geometry is committed by the first, so the second is
    /// the one that can respond. A press on the track sets the value from the
    /// pointer, which is what makes a drag track the hand.
    #[test]
    fn a_pressed_slider_takes_the_value_under_the_pointer() {
        /// One frame of a column holding one slider. Returns the row the track
        /// was laid out on, and whether the value moved.
        fn frame(chrome: &mut Chrome, value: &mut f32, pointer: PointerInput) -> bool {
            let mut ui = chrome.column(bounds(), pointer, None, 1.0);
            ui.label("heading");
            let changed = ui.slider(value, 0.0..=100.0, "size");
            ui.finish();
            changed
        }
        // Where the row actually is, asked of the committed hit geometry rather
        // than recomputed from the slider's own internals -- a test that
        // duplicates the layout cannot notice the layout changing.
        fn row_y(chrome: &Chrome) -> f64 {
            (0..600)
                .map(|i| i as f64)
                .find(|y| {
                    chrome
                        .at(Point::new(120., *y))
                        .is_some_and(|id| id.ends_with("size"))
                })
                .expect("no slider in the column")
        }

        let mut chrome = chrome();
        let mut size = 24.0_f32;
        // Frame one lays the slider out; nothing exists to press yet.
        assert!(!frame(&mut chrome, &mut size, PointerInput::default()));
        let y = row_y(&chrome);

        // The track is inset by half a knob at each end, so those two x values
        // are the exact ends of the range -- and asserting exactness is what
        // makes the inset visible. The midpoint would map to 50 with or
        // without it.
        let (x0, x1) = (12.0, 228.0);
        let knob = 14.0;
        for (x, want) in [(x0 + knob / 2.0, 0.0), (x1 - knob / 2.0, 100.0)] {
            let press = PointerInput {
                pos: Some(Point::new(x, y)),
                primary_down: true,
            };
            assert!(frame(&mut chrome, &mut size, press), "the press missed");
            assert_eq!(size, want, "pressing x={x} gave {size}, not {want}");
            // Release, so the next press is a fresh one.
            frame(&mut chrome, &mut size, PointerInput::default());
        }

        // A press captures the slider, so dragging far past the left edge keeps
        // feeding it -- clamped at the bottom of the range, never negative.
        for x in [120.0, x0 - 100.0] {
            let held = PointerInput {
                pos: Some(Point::new(x, y)),
                primary_down: true,
            };
            frame(&mut chrome, &mut size, held);
        }
        assert_eq!(size, 0.0, "a drag left of the track was not clamped");
    }

    /// Typing reaches the field that was clicked, and only that one.
    #[test]
    fn a_click_focuses_the_field_and_typing_follows_it() {
        let mut chrome = chrome();
        let mut glyph = 'a';
        let field_y = {
            let mut ui = chrome.column(bounds(), PointerInput::default(), None, 1.0);
            let y = ui.cursor;
            ui.char_field(&mut glyph, "glyph");
            ui.finish();
            y + 10.0
        };
        let on_field = PointerInput {
            pos: Some(Point::new(20., field_y)),
            primary_down: true,
        };
        // Press, then release on the same spot: that is a click.
        for down in [true, false] {
            let mut ui = chrome.column(
                bounds(),
                PointerInput {
                    primary_down: down,
                    ..on_field
                },
                None,
                1.0,
            );
            ui.char_field(&mut glyph, "glyph");
            ui.finish();
        }
        let mut ui = chrome.column(bounds(), PointerInput::default(), Some('W'), 1.0);
        assert!(ui.char_field(&mut glyph, "glyph"), "typing was dropped");
        ui.finish();
        assert_eq!(glyph, 'W');

        // Press the empty panel below the field. Nothing is there to take
        // focus, so nothing would clear it if only `register` did the clearing
        // -- and the field would go on eating every keystroke in the gallery.
        for down in [true, false] {
            let mut ui = chrome.column(
                bounds(),
                PointerInput {
                    pos: Some(Point::new(120., 500.)),
                    primary_down: down,
                },
                None,
                1.0,
            );
            ui.char_field(&mut glyph, "glyph");
            ui.finish();
        }
        let mut ui = chrome.column(bounds(), PointerInput::default(), Some('Z'), 1.0);
        assert!(
            !ui.char_field(&mut glyph, "glyph"),
            "the field kept focus after a click on nothing"
        );
        ui.finish();
        assert_eq!(glyph, 'W');
    }

    /// A selected row's fill is the accent, which is lighter than the panel.
    /// The label on it therefore has to flip dark -- painting the light ink on
    /// it measured 1.7:1, which is not text, it is a watermark.
    #[test]
    fn a_selected_rows_label_flips_to_stay_on_the_accent() {
        let mut chrome = chrome();
        let label = |chrome: &mut Chrome, selected: bool| {
            let mut ui = chrome.column(bounds(), PointerInput::default(), None, 1.0);
            ui.option(selected, "a row");
            // The row paints its fill, then every glyph of its label in one ink.
            let paint = ui.finish();
            paint[2].1
        };
        let unselected = label(&mut chrome, false);
        let selected = label(&mut chrome, true);
        assert_eq!(unselected, SKIN.on(SKIN.layer(RECESSED)).to_srgb());
        assert_eq!(selected, SKIN.on(SKIN.primary()).to_srgb());
        assert_ne!(
            selected, unselected,
            "the label did not flip when the ground under it did"
        );
    }

    /// A press keeps its target when the pointer wanders off, but a release out
    /// there is not a click -- so the widget must not go on looking pressed.
    #[test]
    fn a_press_dragged_off_a_button_stops_looking_pressed() {
        let mut chrome = chrome();
        let ink = |chrome: &mut Chrome, pointer: PointerInput| {
            let mut ui = chrome.column(bounds(), pointer, None, 1.0);
            ui.button("press me");
            // The row's own fill is the first thing the button paints.
            ui.finish()[1].1
        };
        let on = Point::new(120., 20.);
        ink(&mut chrome, PointerInput::default());
        let held = ink(
            &mut chrome,
            PointerInput {
                pos: Some(on),
                primary_down: true,
            },
        );
        let away = ink(
            &mut chrome,
            PointerInput {
                pos: Some(Point::new(120., 500.)),
                primary_down: true,
            },
        );
        let rest = SKIN.layer(RAISED);
        assert_eq!(
            held,
            SKIN.pressed(rest).to_srgb(),
            "a press on the button did not light it"
        );
        assert_eq!(
            away,
            rest.to_srgb(),
            "the button still looks pressed off-target"
        );
    }

    /// A column taller than its window is not merely clipped -- the widgets
    /// past the bottom edge cannot be clicked at all. Scrolling has to make
    /// them reachable, not just visible, so this asks the hit geometry.
    #[test]
    fn scrolling_reaches_a_widget_past_the_bottom() {
        // Short enough that the last of thirteen axes is well off the end.
        let short = Bounds::new(0., 0., 240., 200.);
        let mut chrome = chrome();
        let mut axes = [0.0_f32; 13];
        let frame = |chrome: &mut Chrome, axes: &mut [f32; 13]| {
            let mut ui = chrome.column(short, PointerInput::default(), None, 1.0);
            for (i, a) in axes.iter_mut().enumerate() {
                ui.slider(a, 0.0..=100.0, &format!("axis{i}"));
            }
            ui.finish();
        };
        let reachable = |chrome: &Chrome| {
            (0..200).any(|y| {
                chrome
                    .at(Point::new(120., y as f64))
                    .is_some_and(|id| id.ends_with("axis12"))
            })
        };
        frame(&mut chrome, &mut axes);
        assert!(
            !reachable(&chrome),
            "the fixture is not tall enough to test"
        );

        // Far more than the overflow, to prove the clamp stops at the bottom.
        chrome.scroll_by(-10_000.0);
        frame(&mut chrome, &mut axes);
        assert!(reachable(&chrome), "the last axis is still out of reach");

        // And back: scrolling up past the top must not lift the first widget
        // off the panel.
        chrome.scroll_by(10_000.0);
        frame(&mut chrome, &mut axes);
        assert!(
            (0..200).any(|y| chrome
                .at(Point::new(120., y as f64))
                .is_some_and(|id| id.ends_with("axis0"))),
            "scrolling back up overshot the top"
        );
    }

    /// `budget` never returns zero, but `wrap` is called straight from tests
    /// and a zero budget used to drain nothing and spin forever.
    #[test]
    fn a_zero_budget_terminates() {
        assert_eq!(wrap("abc", 0), vec!["a", "b", "c"]);
    }
}
