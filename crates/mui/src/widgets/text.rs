//! Text fields: one line or many, a selection, a blinking caret, and the
//! edits the focused keys imply.
use std::ops::Range;

use mui_geometry::Point;
use mui_input::Key;
use mui_scene::prelude::*;

use super::grapheme;
use crate::Ui;
use crate::widgets::Response;

/// What Enter does in a [`text_edit`] field, and so whether it is one line
/// or many.
///
/// ```
/// use mui::prelude::*;
/// assert_eq!(TextOpts::default().newline, Newline::None);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Newline {
    /// One line: Enter submits and nothing makes a newline.
    #[default]
    None,
    /// Many lines: Enter (or Shift+Enter) is a newline, ctrl/cmd+Enter
    /// submits. A description box.
    Enter,
    /// Many lines: Shift+Enter is a newline, Enter submits. A chat box.
    ShiftEnter,
}

/// How a [`text_edit`] field behaves. The default is [`text_input`]'s:
/// one line that keeps the focus when Enter submits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextOpts {
    pub newline: Newline,
    /// Rows a multi-line field is tall; past them it scrolls, by the wheel
    /// and to follow the caret. A later `.h(..)` wins: the field measures
    /// the height it was given.
    pub rows: usize,
    /// Let go of the keyboard focus when the field submits, as a rename box
    /// does.
    pub blur_on_submit: bool,
}
impl Default for TextOpts {
    fn default() -> Self {
        Self {
            newline: Newline::None,
            rows: 4,
            blur_on_submit: false,
        }
    }
}

/// What a [`text_edit`] field did last frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextEdit {
    /// The value is not what it was.
    pub changed: bool,
    /// The submit key arrived: Enter on one line, and on many whichever of
    /// Enter and ctrl/cmd+Enter [`Newline`] left over.
    pub submitted: bool,
}

/// The text starts this far in from the field's edges.
const PAD: f64 = 8.0;
const PAD_Y: f64 = 6.0;

/// A caret's x from [`Ui::carets`]; `0` for a byte that is not a boundary.
fn caret_at(carets: &[(usize, f64)], byte: usize) -> f64 {
    carets
        .binary_search_by_key(&byte, |&(b, _)| b)
        .map_or(0.0, |i| carets[i].1)
}

fn byte(s: &str, chars: usize) -> usize {
    s.char_indices().nth(chars).map_or(s.len(), |(b, _)| b)
}
fn chars(s: &str, byte: usize) -> usize {
    s[..byte].chars().count()
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
fn insert(value: &mut String, caret: &mut usize, c: char) {
    value.insert(byte(value, *caret), c);
    *caret += 1;
}

/// The row a caret at byte `b` sits on: the last line starting at or
/// before it, so a caret at a soft break starts the next line.
fn row_of(lines: &[Range<usize>], b: usize) -> usize {
    lines.iter().rposition(|l| l.start <= b).unwrap_or(0)
}

/// Where a multi-line field's text is laid out: its lines, as byte ranges,
/// against last frame's inner width.
struct Rows {
    size: f64,
    width: f64,
}
impl Rows {
    fn lines(&self, ui: &Ui, s: &str) -> Vec<Range<usize>> {
        ui.lines(s, self.size, self.width)
    }
    /// The caret `rows` lines above (negative) or below `caret`, at the
    /// same x; past either end it lands on that end.
    fn vertical(&self, ui: &Ui, value: &str, caret: usize, rows: isize) -> usize {
        let lines = self.lines(ui, value);
        let b = byte(value, caret);
        let row = row_of(&lines, b);
        let l = &lines[row];
        let x = caret_at(&ui.carets(&value[l.clone()], self.size), b - l.start);
        match usize::try_from(row as isize + rows) {
            Err(_) => 0,
            Ok(t) if t >= lines.len() => value.chars().count(),
            Ok(t) => {
                let l = &lines[t];
                chars(value, l.start) + ui.hit(&value[l.clone()], self.size, x)
            }
        }
    }
    /// The start or the end of the caret's own line. A soft-wrapped line
    /// ends before the spaces it broke at, or End would land on the next.
    fn home_end(&self, ui: &Ui, value: &str, caret: usize, end: bool) -> usize {
        let lines = self.lines(ui, value);
        let row = row_of(&lines, byte(value, caret));
        let l = &lines[row];
        let at = if !end {
            l.start
        } else if lines.get(row + 1).is_some_and(|n| n.start == l.end) {
            l.start + value[l.clone()].trim_end().len()
        } else {
            l.end
        };
        chars(value, at)
    }
}

/// Apply this frame's typed text and focused keys to `value`. Returns
/// whether the submit key arrived.
fn edit_keys(
    ui: &mut Ui,
    id: &str,
    value: &mut String,
    (anchor, caret): (&mut usize, &mut usize),
    newline: Newline,
    rows: Option<(&Rows, usize)>,
) -> bool {
    let multi = rows.is_some();
    let mut submitted = false;
    let committed = ui.text(id).to_owned();
    let has_committed_text = !committed.is_empty();
    for c in committed.chars().filter(|c| !c.is_control()) {
        (*caret, _) = take(value, *anchor, *caret);
        insert(value, caret, c);
        *anchor = *caret;
    }
    for k in ui.keys(id).to_vec() {
        let cmd = k.mods.ctrl || k.mods.cmd;
        // Where the caret goes, for the keys that only move it.
        let mut moved: Option<usize> = None;
        match k.key {
            Key::Char(c) if cmd => match c.to_ascii_lowercase() {
                'a' => (*anchor, *caret) = (0, value.chars().count()),
                // An empty selection copies nothing: handing the host
                // "" would wipe whatever is already on the clipboard.
                'c' if anchor != caret => {
                    ui.set_clipboard(selected(value, *anchor, *caret));
                }
                'x' if anchor != caret => {
                    ui.set_clipboard(selected(value, *anchor, *caret));
                    (*caret, _) = take(value, *anchor, *caret);
                    *anchor = *caret;
                }
                'v' => {
                    if let Some(s) = ui.pasted().map(str::to_owned) {
                        (*caret, _) = take(value, *anchor, *caret);
                        let s = s.replace("\r\n", "\n").replace('\r', "\n");
                        for c in s
                            .chars()
                            .filter(|&c| !c.is_control() || (multi && c == '\n'))
                        {
                            insert(value, caret, c);
                        }
                        *anchor = *caret;
                    }
                }
                _ => {}
            },
            Key::Char(c) if !c.is_control() && !has_committed_text => {
                (*caret, _) = take(value, *anchor, *caret);
                insert(value, caret, c);
                *anchor = *caret;
            }
            Key::Enter => {
                let breaks = match newline {
                    Newline::None => false,
                    Newline::Enter => !cmd,
                    Newline::ShiftEnter => k.mods.shift,
                };
                if breaks {
                    (*caret, _) = take(value, *anchor, *caret);
                    insert(value, caret, '\n');
                    *anchor = *caret;
                } else {
                    submitted = true;
                }
            }
            Key::Backspace => {
                let (at, had) = take(value, *anchor, *caret);
                *caret = at;
                if !had && *caret > 0 {
                    let start = grapheme::previous(value, *caret);
                    value.replace_range(byte(value, start)..byte(value, *caret), "");
                    *caret = start;
                }
                *anchor = *caret;
            }
            Key::Delete => {
                let (at, had) = take(value, *anchor, *caret);
                *caret = at;
                if !had && *caret < value.chars().count() {
                    let end = grapheme::next(value, *caret);
                    value.replace_range(byte(value, *caret)..byte(value, end), "");
                }
                *anchor = *caret;
            }
            Key::Left if !k.mods.shift && anchor != caret => moved = Some((*anchor).min(*caret)),
            Key::Right if !k.mods.shift && anchor != caret => moved = Some((*anchor).max(*caret)),
            Key::Left => moved = Some(grapheme::previous(value, *caret)),
            Key::Right => moved = Some(grapheme::next(value, *caret)),
            Key::Home | Key::End => {
                let end = k.key == Key::End;
                moved = Some(match rows {
                    Some((r, _)) if !cmd => r.home_end(ui, value, *caret, end),
                    _ if end => value.chars().count(),
                    _ => 0,
                });
            }
            Key::Up | Key::Down | Key::PageUp | Key::PageDown => {
                if let Some((r, page)) = rows {
                    let step = match k.key {
                        Key::Up => -1,
                        Key::Down => 1,
                        Key::PageUp => -(page as isize),
                        _ => page as isize,
                    };
                    moved = Some(r.vertical(ui, value, *caret, step));
                }
            }
            _ => {}
        }
        if let Some(to) = moved {
            *caret = to;
            if !k.mods.shift {
                *anchor = *caret;
            }
        }
    }
    submitted
}

/// A single-line field: the text, a selection, a blinking caret, and the
/// edits the focused keys imply. Click to focus and set the caret, drag to
/// select, double click for a word, shift+arrows to extend. ctrl/cmd+A, C, X
/// and V select all, copy, cut and paste -- a copy reaches the host's
/// [`Clipboard`](crate::Clipboard), or `Frame::clipboard` for a host
/// without one, and a paste reads it back. Returns the field and whether
/// the value changed. [`text_edit`] is the same field with more to say:
/// many lines, and whether Enter submitted.
///
/// ```
/// use mui::prelude::*;
/// let mut ui = Ui::new(Theme::DEFAULT);
/// let mut name = String::from("Init");
/// let field = text_input(&mut ui, "name", &mut name);
/// assert!(!field.changed, "nothing is focused, so nothing was typed");
/// let field = field.el.w(140);
/// ```
pub fn text_input(ui: &mut Ui, id: impl Into<Id>, value: &mut String) -> Response {
    let r = text_edit(ui, id, value, TextOpts::default());
    Response {
        el: r.el,
        changed: r.changed.changed,
    }
}

/// A text field, one line or many as [`TextOpts::newline`] says. Many lines
/// wrap to the field's width, Up and Down move between them at the caret's
/// x, Home and End go to the ends of a line (with ctrl/cmd, of the text),
/// and the lines scroll inside the field -- by the wheel, and to keep the
/// caret in view. Selected text is re-inked on the selection's fill.
/// Returns the field and what it did last frame.
///
/// ```
/// use mui::prelude::*;
/// let mut ui = Ui::new(Theme::DEFAULT);
/// let mut notes = String::from("first\nsecond");
/// let opts = TextOpts { newline: Newline::Enter, rows: 6, ..TextOpts::default() };
/// let field = text_edit(&mut ui, "notes", &mut notes, opts);
/// assert!(!field.changed.changed && !field.changed.submitted, "nothing is focused");
/// ```
pub fn text_edit(
    ui: &mut Ui,
    id: impl Into<Id>,
    value: &mut String,
    opts: TextOpts,
) -> Response<TextEdit> {
    let id: Id = id.into();
    let id = id.as_str();
    let multi = opts.newline != Newline::None;
    let size = ui.theme.text;
    let lh = ui.line_height(size);
    let focused = ui.focused(id);
    let n = value.chars().count();
    let (anchor, caret) = ui.sel(id);
    let (mut anchor, mut caret) = (
        grapheme::floor(value, anchor.min(n)),
        grapheme::floor(value, caret.min(n)),
    );
    // Last frame's field is the only inner box the widget can see.
    let last = ui.scene().and_then(|s| s.surface(id)).map(|s| s.frame.size);
    let room = last.map(|s| s.width - 2.0 * PAD);
    let rows = multi.then(|| Rows {
        size,
        // ponytail: before the first frame there is no width to wrap to, so
        // the first frame breaks at newlines only; the next one wraps.
        width: room.filter(|w| *w > 0.0).unwrap_or(1e9),
    });
    let view = last.map_or(opts.rows.max(1) as f64 * lh, |s| s.height - 2.0 * PAD_Y);
    let mut scroll = if multi { ui.text_scroll(id) } else { 0.0 };

    // The pointer, against last frame's field: the text starts `PAD` in,
    // slid by what kept last frame's caret in the room.
    // ponytail: a composition shown last frame is not in that shift; a click
    // mid-composition lands as if the preedit were not there.
    let r = ui.get(id);
    if (r.pressed || r.dragged)
        && let Some(p) = ui.local(id)
    {
        caret = if let Some(rows) = &rows {
            let lines = rows.lines(ui, value);
            let row = ((p.y - PAD_Y + scroll) / lh).floor().max(0.0) as usize;
            let l = &lines[row.min(lines.len() - 1)];
            chars(value, l.start) + ui.hit(&value[l.clone()], size, p.x - PAD)
        } else {
            let shift = room.map_or(0.0, |room| {
                (caret_at(&ui.carets(value, size), byte(value, caret)) - room).max(0.0)
            });
            ui.hit(value, size, p.x - PAD + shift)
        };
        caret = grapheme::floor(value, caret);
        if r.pressed {
            anchor = caret;
        }
    }
    if r.double_clicked {
        (anchor, caret) = grapheme::word(value, caret);
    }

    // Only a frame with input can edit, so only that frame pays for the copy
    // the change is judged against.
    let typing = focused && !(ui.text(id).is_empty() && ui.keys(id).is_empty());
    let before = typing.then(|| value.clone());
    let page = ((view / lh).floor() as usize).max(1);
    let submitted = focused
        && edit_keys(
            ui,
            id,
            value,
            (&mut anchor, &mut caret),
            opts.newline,
            rows.as_ref().map(|r| (r, page)),
        );
    ui.set_sel(id, anchor, caret);
    if submitted && opts.blur_on_submit {
        ui.blur();
    }
    let edit = TextEdit {
        changed: before.is_some_and(|b| b != *value),
        submitted,
    };

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
    let sel = match &pre {
        Some(_) => 0..0,
        None => byte(value, anchor.min(caret))..byte(value, anchor.max(caret)),
    };
    let pre_range = pre.as_ref().map_or(0..0, |(t, _)| base..base + t.len());
    let on = focused && ui.blink();

    let (body, caret_at_px, reader) = match &rows {
        None => {
            let layers = Layers {
                ui,
                shown: &shown,
                at,
                sel,
                pre: pre_range,
                on,
                lh,
            };
            one_line(layers, room, value, base)
        }
        Some(rows) => {
            let lines = rows.lines(ui, &shown);
            let row = row_of(&lines, at);
            // The wheel scrolls; an edit or a click brings the caret back.
            scroll += r.wheel.y;
            if typing || r.pressed || r.dragged {
                let top = row as f64 * lh;
                scroll = scroll.min(top).max(top + lh - view);
            }
            let content = lines.len() as f64 * lh;
            scroll = scroll.clamp(0.0, (content - view).max(0.0));
            ui.set_text_scroll(id, scroll);
            let layers = Layers {
                ui,
                shown: &shown,
                at,
                sel,
                pre: pre_range,
                on,
                lh,
            };
            let (el, x) = many_lines(layers, &lines, scroll, view);
            let el = el.when(content > view, mui_scene::Styled::captures_wheel);
            // ponytail: no per-character carets for a reader across lines;
            // the value and the selection are still reported.
            (el, Point::new(x, row as f64 * lh - scroll), Vec::new())
        }
    };
    let el = body
        .clip()
        .pad_xy(PAD, PAD_Y)
        .radius(6.0)
        .fill(Role::Field)
        // The ring is declared beside the resting look rather than rebuilt
        // from `focused` every frame; the runtime knows who has the focus.
        .on(State::Focus, |s| s.stroke(Role::Primary))
        .cursor(Cursor::Text)
        .focusable()
        .when(multi, |e| {
            e.height(opts.rows.max(1) as f64 * lh + 2.0 * PAD_Y)
        })
        .a11y(A11y::TextInput {
            value: value.as_str().into(),
            selection: (anchor, caret),
            carets: reader,
        })
        .id(id);
    if focused {
        ui.set_ime_caret(
            id,
            Point::new(PAD + caret_at_px.x, PAD_Y + caret_at_px.y),
            lh,
        );
    }
    Response { el, changed: edit }
}

/// What a field's layers are built from, one line or many: the shown text
/// (the value with any preedit spliced in at the caret), the caret's byte,
/// the selection and preedit as byte ranges of it, whether the caret is lit
/// this frame, and the line height.
struct Layers<'a> {
    ui: &'a Ui,
    shown: &'a str,
    at: usize,
    sel: Range<usize>,
    pre: Range<usize>,
    on: bool,
    lh: f64,
}

/// The selected part of a run, re-inked: the selected text on its own, in
/// a box of the selection's fill, so its ink resolves on that fill. It sits
/// where the run put that text, so it paints over its own glyphs.
// ponytail: shaped on its own, a ligature or kerning pair across the
// selection's edge may land a hair off the run's; shape once and split the
// glyphs if a script ever shows it.
fn reinked(selected: &str, x0: f64, x1: f64, lh: f64) -> El {
    stack([text(selected.to_owned())
        .width(x1 - x0)
        .lines(1)
        .anchor(Align::Start, Align::Center)])
    .width(x1 - x0)
    .height(lh)
    .radius(2.0)
    .fill(Role::Primary)
}

/// One line, scrolled sideways so the caret never leaves the box. The
/// children keep the keys the single-line field always had -- `/0`
/// selection, `/1` value, `/2` caret, `/3` preedit underline -- and the
/// re-inked selection comes last, over the value.
fn one_line(layers: Layers, room: Option<f64>, value: &str, base: usize) -> (El, Point, Vec<f64>) {
    let Layers {
        ui,
        shown,
        at,
        sel,
        pre,
        on,
        lh,
    } = layers;
    let size = ui.theme.text;
    let carets = ui.carets(shown, size);
    let x = |b: usize| caret_at(&carets, b);
    let (lo, hi) = (x(sel.start), x(sel.end));
    let (plo, phi) = (x(pre.start), x(pre.end));
    // The value keeps its whole measured advance so the field stays one line
    // -- a plain `text()` would wrap to the frame and grow the field -- and
    // the frame clips it. Room comes from last frame's field, and the caret
    // scrolls the layers together so it never leaves the box.
    // ponytail: the first frame of an over-long value shows its head; it
    // catches up on the next one.
    let run = x(shown.len());
    let room = room.unwrap_or(run);
    let caret_x = x(at);
    let shift = (caret_x - room).max(0.0);
    // What a screen reader follows: a caret before each character of the
    // value and after the last, in the field's space. A composition sits in
    // the value's gap at the caret, so the characters after it sit after it.
    let tail = pre.len();
    let reader = (value.char_indices().map(|(b, _)| b))
        .chain([value.len()])
        .map(|b| PAD - shift + x(if b < base { b } else { b + tail }))
        .collect();
    let mut children = vec![
        block(hi - lo, size)
            .anchor(Align::Start, Align::Center)
            .offset(lo - shift, 0.0)
            .when(hi > lo, |e| e.fill(Role::Primary)),
        text(shown.to_owned())
            .width(run)
            .lines(1)
            .anchor(Align::Start, Align::Center)
            .offset(-shift, 0.0),
        block(2.0, size)
            .anchor(Align::Start, Align::Center)
            .offset(caret_x - shift, 0.0)
            .when(on, |e| e.fill(Role::Ink)),
        block(phi - plo, 2.0)
            .anchor(Align::Start, Align::End)
            .offset(plo - shift, 0.0)
            .when(phi > plo, |e| e.fill(Role::Ink)),
    ];
    if hi > lo {
        children.push(
            reinked(&shown[sel.clone()], lo, hi, lh)
                .anchor(Align::Start, Align::Center)
                .offset(lo - shift, 0.0),
        );
    }
    (stack(children), Point::new(caret_x - shift, 0.0), reader)
}

/// Many lines, only the rows in view built: a line's run, the re-inked
/// selection over it, then the caret and the preedit underline on top.
fn many_lines(layers: Layers, lines: &[Range<usize>], scroll: f64, view: f64) -> (El, f64) {
    let Layers {
        ui,
        shown,
        at,
        sel,
        pre,
        on,
        lh,
    } = layers;
    let size = ui.theme.text;
    let first = (scroll / lh).floor().max(0.0) as usize;
    let last = (((scroll + view) / lh).ceil().max(0.0) as usize).min(lines.len());
    let caret_row = row_of(lines, at);
    let mut caret_x = 0.0;
    let mut children = Vec::new();
    for (row, l) in lines.iter().enumerate().take(last).skip(first) {
        let line = &shown[l.clone()];
        let carets = ui.carets(line, size);
        let x = |b: usize| caret_at(&carets, b.clamp(l.start, l.end) - l.start);
        let run = x(l.end);
        let y = row as f64 * lh - scroll;
        children.push(text(line.to_owned()).width(run).lines(1).at(0.0, y));
        // A selection running on past this line's end takes a sliver more,
        // so a selected newline is visible.
        if sel.start < l.end.max(l.start + 1) && sel.end > l.start {
            let x0 = x(sel.start);
            let x1 = if sel.end > l.end {
                run + size * 0.3
            } else {
                x(sel.end)
            };
            if x1 > x0 {
                let (b0, b1) = (sel.start.max(l.start), sel.end.min(l.end));
                children.push(reinked(&shown[b0..b1], x0, x1, lh).at(x0, y));
            }
        }
        if row == caret_row {
            caret_x = x(at);
            let (plo, phi) = (x(pre.start), x(pre.end));
            if phi > plo {
                children.push(block(phi - plo, 2.0).at(plo, y + lh - 2.0).fill(Role::Ink));
            }
        }
    }
    let caret_y = caret_row as f64 * lh - scroll;
    children.push(
        block(2.0, lh)
            .at(caret_x, caret_y)
            .when(on, |e| e.fill(Role::Ink)),
    );
    (stack(children), caret_x)
}
