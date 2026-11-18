//! Typing an exact value into a control that is otherwise dragged.
//!
//! A knob or a readout shows a formatted value; [`ExactEntry`] swaps it for a
//! text field holding that text, selected, and turns what is typed back into
//! a value with the caller's parser. The caller owns the value, its units and
//! the host transaction; this owns the draft and the keys that end it.

use mui_input::{Button, Key};
use mui_scene::prelude::*;

use crate::Ui;
use crate::widgets::{Response, TextOpts, text_edit};

/// How an exact entry ended this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Entered {
    /// A value to apply: Enter on a draft that parsed, or the focus leaving
    /// an edited one that did.
    Commit(f64),
    /// Nothing to apply: Escape, or the focus leaving a draft that was
    /// untouched or did not parse.
    Cancel,
}

/// A text field for typing an exact value, open on request. Keep one per
/// control that offers it, beside the control's other state.
///
/// Enter commits a draft that parses and keeps a bad one open, marked
/// [`invalid`](Self::is_invalid). Escape cancels. The focus leaving commits
/// as Enter would, or cancels a draft that was not edited or will not
/// parse. Confirming the untouched text gives back the value it was opened
/// with, so a concise display does not round the value it shows.
///
/// ```
/// use mui::prelude::*;
/// let mut ui = Ui::default();
/// let mut entry = ExactEntry::default();
/// let id = Id::of("cutoff");
/// if ExactEntry::requested(&ui, &id) {
///     entry.open(&mut ui, &id, 440.0, "440 Hz".into());
/// }
/// let typed = entry.show(&mut ui, &id, |s| s.trim_end_matches(" Hz").parse().ok());
/// assert!(typed.el.is_none() && typed.changed.is_none(), "closed until asked");
/// ```
#[derive(Clone, Debug, Default)]
pub struct ExactEntry {
    draft: String,
    shown: String,
    value: f64,
    open: bool,
    invalid: bool,
    /// The text is still to be selected whole: the field exists from the
    /// frame after it opens.
    selecting: bool,
    /// The frame it opened on, whose keys and focus belong to the control.
    opening: bool,
}

impl ExactEntry {
    /// Whether `id` asked for exact entry last frame: Enter while it had
    /// the focus, or a right-click with Alt held (a plain right-click is
    /// the host's automation menu).
    pub fn requested(ui: &Ui, id: impl Into<Id>) -> bool {
        let id: Id = id.into();
        let r = ui.get(id.clone());
        (r.clicked_with(Button::Secondary) && r.press_mods.alt)
            || ui.keys(id).iter().any(|k| k.key == Key::Enter)
    }

    /// Opens on `shown`, the control's formatted `value`, selected whole,
    /// with the focus in the field.
    pub fn open(&mut self, ui: &mut Ui, id: &Id, value: f64, shown: String) {
        self.draft.clone_from(&shown);
        self.shown = shown;
        self.value = value;
        self.open = true;
        self.invalid = false;
        self.selecting = true;
        self.opening = true;
        ui.focus(Self::field(id));
    }

    /// Closes without an outcome.
    pub fn close(&mut self) {
        self.open = false;
        self.invalid = false;
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Enter was pressed on a draft that does not parse.
    pub fn is_invalid(&self) -> bool {
        self.invalid
    }

    /// What is typed so far.
    pub fn draft(&self) -> &str {
        &self.draft
    }

    /// Replace what is typed, as a paste would.
    pub fn set_draft(&mut self, text: impl Into<String>) {
        self.draft = text.into();
    }

    /// The field's id under the control's `id`.
    pub fn field(id: &Id) -> Id {
        id.field("entry")
    }

    /// The field while open, `None` while closed; `changed` is how it ended
    /// this frame. `parse` reads a draft in the control's units; a
    /// non-finite result does not parse. Enter and Escape hand the focus
    /// back to `id`.
    pub fn show(
        &mut self,
        ui: &mut Ui,
        id: &Id,
        parse: impl FnOnce(&str) -> Option<f64>,
    ) -> Response<Option<Entered>, Option<El>> {
        let opening = std::mem::take(&mut self.opening);
        if !self.open {
            return Response {
                el: None,
                changed: None,
            };
        }
        let input = Self::field(id);
        // Escape releases MUI's focus as it is taken, so by the next frame
        // it reaches the shortcuts rather than the field.
        let escaped = ui
            .keys(input.clone())
            .iter()
            .chain(ui.shortcuts())
            .any(|k| k.key == Key::Escape);
        if !opening && escaped {
            self.close();
            ui.focus(id.clone());
            return Response {
                el: None,
                changed: Some(Entered::Cancel),
            };
        }
        if self.selecting
            && !opening
            && let Some(state) = ui.text_input_state().filter(|s| s.id == input.as_str())
        {
            self.selecting = !ui.set_text_input_selection(0..state.text.len());
        }
        let edited = text_edit(ui, input.clone(), &mut self.draft, TextOpts::default());
        let left = !opening && !self.selecting && !ui.focused(input);
        let mut changed = None;
        if !opening && (edited.changed.submitted || left) {
            let untouched = self.draft == self.shown;
            let value = if untouched {
                Some(self.value)
            } else {
                parse(&self.draft).filter(|v| v.is_finite())
            };
            match value {
                Some(v) => {
                    self.close();
                    changed = Some(if left && untouched {
                        Entered::Cancel
                    } else {
                        Entered::Commit(v)
                    });
                    if !left {
                        ui.focus(id.clone());
                    }
                }
                None if left => {
                    self.close();
                    changed = Some(Entered::Cancel);
                }
                None => self.invalid = true,
            }
        }
        Response {
            el: self.open.then_some(edited.el),
            changed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mui_input::{Input, KeyPress, Mods};

    fn frame(ui: &mut Ui, entry: &mut ExactEntry, input: Input) -> Option<Entered> {
        let id = Id::of("v");
        let mut out = None;
        let mut root = || {
            let r = entry.show(ui, &id, |s| s.parse().ok());
            out = r.changed;
            col([block(10., 10.).id("v").focusable()]
                .into_iter()
                .chain(r.el))
        };
        let el = root();
        ui.frame(el, Some(Size::new(200., 100.)), input, 1. / 60.)
            .expect("frame");
        out
    }

    fn key(k: Key) -> Input {
        Input {
            keys: vec![KeyPress {
                key: k,
                mods: Mods::default(),
            }],
            ..Input::default()
        }
    }

    #[test]
    fn escape_cancels_and_enter_commits_what_parses() {
        let mut ui = Ui::default();
        let mut e = ExactEntry::default();
        e.open(&mut ui, &Id::of("v"), 2.0, "2".into());
        for _ in 0..3 {
            assert_eq!(frame(&mut ui, &mut e, Input::default()), None);
        }
        assert!(e.is_open());
        // The untouched text confirms the value it was opened with.
        frame(&mut ui, &mut e, key(Key::Enter));
        assert_eq!(
            frame(&mut ui, &mut e, Input::default()),
            Some(Entered::Commit(2.0))
        );
        assert!(!e.is_open());

        e.open(&mut ui, &Id::of("v"), 2.0, "2".into());
        frame(&mut ui, &mut e, Input::default());
        frame(&mut ui, &mut e, key(Key::Escape));
        let ended = frame(&mut ui, &mut e, Input::default());
        assert!(!e.is_open() && ended.is_none_or(|x| x == Entered::Cancel));
    }
}
