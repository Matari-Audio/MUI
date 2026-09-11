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

use mui_geometry::{Bounds, Path, Point, RoundedRect};
use mui_input::{Hit, Interaction, PointerInput, Response};
use mui_text::TextRun;
use mui_vello::ARC_TOLERANCE;
use vello_common::peniko::color::AlphaColor;

pub type Rgba = AlphaColor<vello_common::peniko::color::Srgb>;

pub const PANEL: Rgba = AlphaColor::new([0.13, 0.14, 0.17, 1.0]);
pub const WELL: Rgba = AlphaColor::new([0.09, 0.10, 0.12, 1.0]);
pub const IDLE: Rgba = AlphaColor::new([0.27, 0.29, 0.34, 1.0]);
pub const HOVER: Rgba = AlphaColor::new([0.40, 0.43, 0.50, 1.0]);
pub const ACCENT: Rgba = AlphaColor::new([0.35, 0.72, 0.98, 1.0]);
pub const ACCENT_LIT: Rgba = AlphaColor::new([0.56, 0.83, 1.0, 1.0]);
pub const TEXT: Rgba = AlphaColor::new([0.88, 0.90, 0.94, 1.0]);
pub const TEXT_DIM: Rgba = AlphaColor::new([0.55, 0.58, 0.64, 1.0]);
pub const ERROR: Rgba = AlphaColor::new([0.86, 0.35, 0.35, 1.0]);

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
}

impl Chrome {
    pub fn new(font: Arc<Vec<u8>>) -> Self {
        Self {
            interaction: Interaction::new(),
            hit: Hit::default(),
            focus: None,
            font,
        }
    }

    /// Whether a gesture is in flight on a widget. The stage asks so that a
    /// slider drag that wanders off the column keeps the slider, not the
    /// specimen underneath it.
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
        let mut paint = Vec::new();
        if let Ok(rect) = RoundedRect::new(bounds, 0.0) {
            paint.push((rect.path(), PANEL));
        }
        let cursor = bounds.min.y + PAD * scale;
        Ui {
            chrome: self,
            bounds,
            scale,
            pointer,
            typed,
            cursor,
            ordinal: 0,
            paint,
            next_hit: Hit::default(),
        }
    }
}

/// One frame of one column of widgets.
pub struct Ui<'a> {
    chrome: &'a mut Chrome,
    bounds: Bounds,
    scale: f64,
    pointer: PointerInput,
    typed: Option<char>,
    /// Top of the next widget, in physical pixels.
    cursor: f64,
    /// Interactive widgets only, so a conditional [`Ui::note`] cannot shift the
    /// id of everything after it.
    ordinal: usize,
    paint: Vec<(Path, Rgba)>,
    next_hit: Hit,
}

impl Ui<'_> {
    /// Logical points to physical pixels. The only scale conversion in the
    /// program.
    pub fn px(&self, logical: f64) -> f64 {
        logical * self.scale
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

    fn rect(&mut self, bounds: Bounds, radius: f64, ink: Rgba) -> Option<Path> {
        let path = RoundedRect::new(bounds, radius).ok()?.path();
        self.paint.push((path.clone(), ink));
        Some(path)
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

    /// Claim an id for an interactive widget and read what the pointer did to
    /// it last frame. A click anywhere else drops keyboard focus, which is the
    /// whole of the focus model.
    fn target(&mut self, label: &str, path: &Path) -> (String, Response) {
        let id = format!("{}:{}", self.ordinal, label);
        self.ordinal += 1;
        let response = self.chrome.interaction.get(&id);
        if response.clicked && self.chrome.focus.as_deref() != Some(id.as_str()) {
            self.chrome.focus = None;
        }
        let _ = self.next_hit.push(id.clone(), path);
        (id, response)
    }

    pub fn label(&mut self, text: &str) {
        self.text_row(text, TEXT_SIZE, TEXT);
    }

    /// Small and dimmed: provenance, counts, the line under a heading.
    pub fn note(&mut self, text: &str) {
        self.text_row(text, NOTE_SIZE, TEXT_DIM);
    }

    /// A note in the error ink. Same row, so a failure never moves the layout
    /// around underneath the pointer.
    pub fn error(&mut self, text: &str) {
        self.text_row(text, NOTE_SIZE, ERROR);
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

    pub fn separator(&mut self) {
        let (x0, x1) = self.span();
        let y = self.cursor;
        let thickness = self.px(1.0).max(1.0);
        self.rect(
            Bounds {
                min: Point::new(x0, y),
                max: Point::new(x1, y + thickness),
            },
            0.0,
            IDLE,
        );
        self.advance(thickness);
    }

    pub fn button(&mut self, text: &str) -> bool {
        let (x0, x1) = self.span();
        let (y, height) = (self.cursor, self.px(ROW));
        let bounds = Bounds {
            min: Point::new(x0, y),
            max: Point::new(x1, y + height),
        };
        let Ok(shape) = RoundedRect::new(bounds, self.px(RADIUS)) else {
            self.advance(height);
            return false;
        };
        let path = shape.path();
        let (_, r) = self.target(text, &path);
        let ink = if r.held {
            ACCENT
        } else if r.hovered {
            HOVER
        } else {
            IDLE
        };
        self.paint.push((path, ink));
        if let Some(run) = self.run(text, self.px(TEXT_SIZE), x1 - x0) {
            let x = x0 + ((x1 - x0) - run.advance) / 2.0;
            self.place(&run, x.max(x0), y, height, TEXT);
        }
        self.advance(height);
        r.clicked
    }

    /// One row of a mutually exclusive list. Returns whether it was clicked, so
    /// the caller keeps the "which one" state and this keeps none of it.
    pub fn option(&mut self, selected: bool, text: &str) -> bool {
        let (x0, x1) = self.span();
        let (y, height) = (self.cursor, self.px(ROW));
        let bounds = Bounds {
            min: Point::new(x0, y),
            max: Point::new(x1, y + height),
        };
        let Ok(shape) = RoundedRect::new(bounds, self.px(RADIUS)) else {
            self.advance(height);
            return false;
        };
        let path = shape.path();
        let (_, r) = self.target(text, &path);
        // A selected row still has to answer the pointer, so it gets its own
        // hover step rather than falling through to the flat accent.
        let ink = match (r.held, selected, r.hovered) {
            (true, _, _) => ACCENT,
            (_, true, true) => ACCENT_LIT,
            (_, true, false) => ACCENT,
            (_, false, true) => HOVER,
            (_, false, false) => WELL,
        };
        self.paint.push((path, ink));
        if let Some(run) = self.run(text, self.px(TEXT_SIZE), x1 - x0 - self.px(PAD)) {
            self.place(&run, x0 + self.px(GAP), y, height, TEXT);
        }
        self.advance(height);
        r.clicked
    }

    pub fn checkbox(&mut self, on: &mut bool, text: &str) -> bool {
        let (x0, x1) = self.span();
        let (y, height) = (self.cursor, self.px(ROW));
        let row = Bounds {
            min: Point::new(x0, y),
            max: Point::new(x1, y + height),
        };
        let Ok(shape) = RoundedRect::new(row, self.px(RADIUS)) else {
            self.advance(height);
            return false;
        };
        let (_, r) = self.target(text, &shape.path());
        if r.clicked {
            *on = !*on;
        }
        let side = height * 0.62;
        let box_top = y + (height - side) / 2.0;
        let ink = match (*on, r.hovered || r.held) {
            (true, true) => ACCENT_LIT,
            (true, false) => ACCENT,
            (false, true) => HOVER,
            (false, false) => WELL,
        };
        self.rect(
            Bounds {
                min: Point::new(x0, box_top),
                max: Point::new(x0 + side, box_top + side),
            },
            self.px(3.0),
            ink,
        );
        if let Some(run) = self.run(text, self.px(TEXT_SIZE), x1 - x0 - side) {
            self.place(&run, x0 + side + self.px(GAP), y, height, TEXT);
        }
        self.advance(height);
        r.clicked
    }

    /// Returns whether `value` moved this frame -- the signal a caller needs to
    /// know whether anything downstream has to be rebuilt.
    pub fn slider(&mut self, value: &mut f32, range: RangeInclusive<f32>, text: &str) -> bool {
        let (x0, x1) = self.span();
        let y = self.cursor;
        let label_h = self.px(TEXT_SIZE) * 1.4;
        let track_h = self.px(6.0);
        let knob = self.px(14.0);
        let height = label_h + knob;

        let row = Bounds {
            min: Point::new(x0, y),
            max: Point::new(x1, y + height),
        };
        let Ok(shape) = RoundedRect::new(row, 0.0) else {
            self.advance(height);
            return false;
        };
        let (_, r) = self.target(text, &shape.path());

        // The track is inset by half a knob so the knob's centre can reach both
        // ends without any part of it leaving the row.
        let (t0, t1) = (x0 + knob / 2.0, x1 - knob / 2.0);
        let before = *value;
        if r.held {
            if let Some(p) = self.pointer.pos {
                let t = ((p.x - t0) / (t1 - t0)).clamp(0.0, 1.0) as f32;
                *value = range.start() + (range.end() - range.start()) * t;
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
            Bounds {
                min: Point::new(x0, track_y),
                max: Point::new(x1, track_y + track_h),
            },
            track_h / 2.0,
            WELL,
        );
        let filled = t0 + (t1 - t0) * t;
        let lit = if r.held || r.hovered {
            ACCENT_LIT
        } else {
            ACCENT
        };
        if filled > x0 {
            self.rect(
                Bounds {
                    min: Point::new(x0, track_y),
                    max: Point::new(filled, track_y + track_h),
                },
                track_h / 2.0,
                lit,
            );
        }
        self.rect(
            Bounds {
                min: Point::new(filled - knob / 2.0, y + label_h),
                max: Point::new(filled + knob / 2.0, y + label_h + knob),
            },
            knob / 2.0,
            lit,
        );
        if let Some(run) = self.run(&format!("{text}  {value:.2}"), self.px(TEXT_SIZE), x1 - x0) {
            self.place(&run, x0, y, label_h, TEXT);
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
        let field = Bounds {
            min: Point::new(x0, y),
            max: Point::new(x0 + side, y + height),
        };
        let Ok(shape) = RoundedRect::new(field, self.px(RADIUS)) else {
            self.advance(height);
            return false;
        };
        let path = shape.path();
        let (id, r) = self.target(text, &path);
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

        let ink = match (focused, r.hovered) {
            (true, _) => ACCENT,
            (false, true) => HOVER,
            (false, false) => WELL,
        };
        self.paint.push((path, ink));
        let glyph = value.to_string();
        if let Some(run) = self.run(&glyph, self.px(TEXT_SIZE + 3.0), side) {
            let x = x0 + (side - run.advance) / 2.0;
            self.place(&run, x.max(x0), y, height, TEXT);
        }
        if let Some(run) = self.run(text, self.px(TEXT_SIZE), x1 - x0 - side) {
            self.place(&run, x0 + side + self.px(GAP), y, height, TEXT_DIM);
        }
        self.advance(height);
        changed
    }

    /// The paint list in call order, and the point at which this frame's hit
    /// geometry is committed for the next one. Consuming `self` is the whole
    /// enforcement: a widget added after the commit could never be hit.
    pub fn finish(self) -> Vec<(Path, Rgba)> {
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
        Bounds {
            min: Point::new(0., 0.),
            max: Point::new(240., 600.),
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
        let mut chrome = chrome();
        let ui = chrome.column(bounds(), PointerInput::default(), None, 1.25);
        assert_eq!(ui.px(10.0), 12.5);
        assert_eq!(ui.px(0.0), 0.0);
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
        fn frame(chrome: &mut Chrome, value: &mut f32, pointer: PointerInput) -> (f64, bool) {
            let mut ui = chrome.column(bounds(), pointer, None, 1.0);
            ui.label("heading");
            let track = ui.cursor + ui.px(TEXT_SIZE) * 1.4 + ui.px(7.0);
            let changed = ui.slider(value, 0.0..=100.0, "size");
            ui.finish();
            (track, changed)
        }

        let mut chrome = chrome();
        let mut size = 24.0_f32;
        // Frame one lays the slider out; nothing exists to press yet.
        let (track, changed) = frame(&mut chrome, &mut size, PointerInput::default());
        assert!(!changed);
        // Frame two presses the middle of the track.
        let press = PointerInput {
            pos: Some(Point::new(120., track)),
            primary_down: true,
        };
        let (_, changed) = frame(&mut chrome, &mut size, press);
        assert!(changed, "the press did not reach the track");
        assert!(
            (size - 50.0).abs() < 6.0,
            "pressing the middle gave {size}, not about 50"
        );
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
    }
}
