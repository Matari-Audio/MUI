//! Tabs, toasts, a table and tree rows: the rest of an editor's kit, built
//! from the same rows, cards and lists as everything else.
use std::sync::Arc;

use mui_input::Key;
use mui_layout::{Insets, Pad};
use mui_scene::prelude::*;

use crate::Ui;
use crate::widgets::presets::card;
use crate::widgets::{ListEvent, ListItem, ListOptions, ListState, Response, uniform_list};

/// One tab, for [`Tabs::face`] to dress.
#[derive(Clone, Copy, Debug)]
pub struct Tab<'a> {
    pub label: &'a str,
    pub index: usize,
    pub selected: bool,
}

/// A strip of tabs, one selected: a press picks one, and the arrow keys
/// along the strip (Left/Right, or Up/Down when [`vertical`](Self::vertical))
/// move the selection and the focus with it. The face is the caller's -- an
/// icon and a label, a pill, anything -- and MUI adds the id (`"{id}/{i}"`),
/// focus, the role, the name and the cursor.
///
/// [`items`](Self::items) hands the tabs back loose, for a sidebar that
/// puts other things between them; [`show`](Self::show) strings them in a
/// row or a column.
///
/// ```
/// use mui::prelude::*;
/// let mut ui = Ui::default();
/// let mut page = 0;
/// let face = |t: Tab<'_>| row![icon(sym::SETTINGS), text(t.label)].gap(Xs);
/// let side = Tabs::default().vertical().face(&face).show(&mut ui, "side", &["Audio", "MIDI"], &mut page);
/// assert!(!side.changed);
/// ```
#[derive(Clone, Copy)]
pub struct Tabs<'a> {
    vertical: bool,
    face: &'a dyn for<'r> Fn(Tab<'r>) -> El,
}

impl Default for Tabs<'_> {
    fn default() -> Self {
        Self {
            vertical: false,
            face: &tab_face,
        }
    }
}

impl<'a> Tabs<'a> {
    /// A column of tabs, stepped with Up and Down.
    pub fn vertical(mut self) -> Self {
        self.vertical = true;
        self
    }
    /// A tab's face.
    pub fn face(mut self, f: &'a dyn for<'r> Fn(Tab<'r>) -> El) -> Self {
        self.face = f;
        self
    }

    /// The tabs, one element each, in order. `changed` is whether
    /// `*selected` moved this frame.
    pub fn items(
        &self,
        ui: &mut Ui,
        id: impl Into<Id>,
        labels: &[&str],
        selected: &mut usize,
    ) -> Response<bool, Vec<El>> {
        let id: Id = id.into();
        let before = *selected;
        let (back, on) = if self.vertical {
            (Key::Up, Key::Down)
        } else {
            (Key::Left, Key::Right)
        };
        for i in 0..labels.len() {
            if ui.get(id.slot(i)).activated() {
                *selected = i;
            }
            let step: isize = ui.keys(id.slot(i)).iter().fold(0, |d, k| match k.key {
                k if k == on => d + 1,
                k if k == back => d - 1,
                _ => d,
            });
            if step != 0 {
                *selected = (i as isize + step).rem_euclid(labels.len() as isize) as usize;
                // Focus moves on, so the tab it lands on would read these keys again.
                ui.focus(id.slot(*selected));
                break;
            }
        }
        let el = labels
            .iter()
            .enumerate()
            .map(|(index, &label)| {
                let selected = index == *selected;
                (self.face)(Tab {
                    label,
                    index,
                    selected,
                })
                .cursor(Cursor::Hand)
                .a11y(A11y::Toggle { on: selected })
                .named(label)
                .focusable()
                .id(id.slot(index))
            })
            .collect();
        Response {
            el,
            changed: *selected != before,
        }
    }

    /// The tabs in a row, or a column when vertical, as one group `id`.
    pub fn show(
        &self,
        ui: &mut Ui,
        id: impl Into<Id>,
        labels: &[&str],
        selected: &mut usize,
    ) -> Response<bool, El> {
        let id: Id = id.into();
        let Response { el, changed } = self.items(ui, id.clone(), labels, selected);
        let strip = if self.vertical { col(el) } else { row(el) };
        Response {
            el: strip.gap(Xs).a11y(A11y::Group).id(id),
            changed,
        }
    }
}

/// MUI's tab: the label, the selected one in the accent on a field.
fn tab_face(t: Tab<'_>) -> El {
    let ink = if t.selected { Role::Primary } else { Role::Dim };
    let tab = row([text(t.label).fill(ink)])
        .h(32.0)
        .align(Align::Center)
        .pad((12.0, 0.0))
        .radius(6.0)
        .on(State::Hover, |s| s.fill(Role::Field))
        .on(State::FocusVisible, |s| s.fill(Role::Field));
    if t.selected {
        tab.fill(Role::Field)
    } else {
        tab
    }
}

/// A row of `labels` in MUI's look, one selected: [`Tabs::show`].
///
/// ```
/// use mui::prelude::*;
/// let mut ui = Ui::default();
/// let mut page = 0;
/// let t = tabs(&mut ui, "pages", &["Osc", "Filter", "FX"], &mut page);
/// assert!(!t.changed);
/// ```
pub fn tabs(
    ui: &mut Ui,
    id: impl Into<Id>,
    labels: &[&str],
    selected: &mut usize,
) -> Response<bool, El> {
    Tabs::default().show(ui, id, labels, selected)
}

/// A segmented control: `labels` side by side in one track, sharing its
/// width, one selected -- [`Tabs`] with every segment the same share. For
/// another look, give [`Tabs::face`] your own segment and wrap
/// [`Tabs::items`] in your own track.
///
/// ```
/// use mui::prelude::*;
/// let mut ui = Ui::default();
/// let mut mode = 0;
/// let s = segmented(&mut ui, "mode", &["Mono", "Poly", "Legato"], &mut mode);
/// assert!(!s.changed);
/// ```
pub fn segmented(
    ui: &mut Ui,
    id: impl Into<Id>,
    labels: &[&str],
    selected: &mut usize,
) -> Response<bool, El> {
    let id: Id = id.into();
    let Response { el, changed } =
        Tabs::default()
            .face(&segment_face)
            .items(ui, id.clone(), labels, selected);
    let cells = el.into_iter().map(|e| e.grow(1.0).basis(0.0).min_w(0.0));
    Response {
        el: row(cells)
            .gap(2.0)
            .pad(2.0)
            .radius(Corner::Field)
            .fill(Role::Field)
            .a11y(A11y::Group)
            .id(id),
        changed,
    }
}

/// MUI's segment: the label centred, the selected one raised.
fn segment_face(t: Tab<'_>) -> El {
    let ink = if t.selected { Role::Ink } else { Role::Dim };
    row([text(t.label).fill(ink).lines(1)])
        .h(28.0)
        .center()
        .pad((8.0, 0.0))
        .radius(Corner::Field)
        .when(t.selected, |e| e.fill(Role::Raised))
        .on(State::Hover, |s| s.fill(Role::Raised))
}

/// How long a toast stays up, in seconds.
pub const TOAST_SECS: f64 = 4.0;
const TOASTS: &str = "mui/toasts";

/// Show `message` for [`TOAST_SECS`] at the bottom of the window. Render
/// [`toasts`] every frame for it to appear.
pub fn toast(ui: &mut Ui, message: impl Into<Arc<str>>) {
    let until = ui.time() + TOAST_SECS;
    let mut list: Vec<(Arc<str>, f64)> = ui.stash(TOASTS).cloned().unwrap_or_default();
    list.push((message.into(), until));
    ui.set_stash(TOASTS, Some(list));
}

/// The toasts still up, stacked at the bottom centre of the window, newest
/// last; `None` when there are none. A press on one dismisses it. Push it as
/// a late child of the root; the layer takes no presses of its own.
///
/// ```
/// use mui::prelude::*;
/// let mut ui = Ui::default();
/// toast(&mut ui, "Preset saved");
/// assert!(toasts(&mut ui).is_some());
/// ```
pub fn toasts(ui: &mut Ui) -> Option<El> {
    let now = ui.time();
    let mut list: Vec<(Arc<str>, f64)> = ui.stash(TOASTS).cloned().unwrap_or_default();
    let key = |i: usize| Id::runtime_slot(TOASTS, i);
    let mut i = 0;
    list.retain(|(_, until)| {
        let keep = *until > now && !ui.get(key(i).as_str()).activated();
        i += 1;
        keep
    });
    let next = list
        .iter()
        .map(|(_, until)| until - now)
        .fold(f64::INFINITY, f64::min);
    if next.is_finite() {
        ui.repaint_in(next);
    }
    let cards: Vec<El> = list
        .iter()
        .enumerate()
        .map(|(i, (message, _))| {
            row([text(&**message)])
                .pad((14.0, 10.0))
                .preset(card())
                .cursor(Cursor::Hand)
                .a11y(A11y::Label)
                .named(&**message)
                .id(key(i))
        })
        .collect();
    let empty = cards.is_empty();
    ui.set_stash(TOASTS, (!empty).then_some(list));
    if empty {
        return None;
    }
    Some(
        // The id (which keeps the stash) goes on the cards' column: on the
        // full-window layer it would make the layer take every press.
        stack([col(cards)
            .gap(Xs)
            .anchor(Align::Center, Align::End)
            .id(TOASTS)])
        .pad(M)
        .w(Len::Pct(100.0))
        .h(Len::Pct(100.0))
        .float(),
    )
}

/// A column of a [`table`]: its header and fixed width.
#[derive(Clone, Copy, Debug)]
pub struct Column<'a> {
    pub title: &'a str,
    pub width: f64,
}

/// A header over a virtualised list of rows `ROW` high, each `cell(ui, row,
/// column)` laid out at its column's width. Rows scroll under a fixed
/// header; `options.height` covers both. Only visible rows are built.
pub fn table(
    ui: &mut Ui,
    id: impl Into<Id>,
    state: &mut ListState,
    options: ListOptions,
    columns: &[Column<'_>],
    mut cell: impl FnMut(&mut Ui, &ListItem, usize) -> El,
) -> Response<ListEvent, El> {
    const HEADER: f64 = 28.0;
    let id: Id = id.into();
    let header = row(columns.iter().map(|c| {
        row([text(c.title).fill(Role::Dim)])
            .w(c.width)
            .pad((8.0, 0.0))
            .align(Align::Center)
    }))
    .h(HEADER);
    let body_options = ListOptions {
        height: (options.height - HEADER).max(1.0),
        ..options
    };
    let rows = uniform_list(ui, id.field("rows"), state, body_options, |ui, item| {
        let cells: Vec<El> = (0..columns.len())
            .map(|c| {
                row([cell(ui, &item, c)])
                    .w(columns[c].width)
                    .pad((8.0, 0.0))
                    .align(Align::Center)
            })
            .collect();
        let line = row(cells).h(item.height).w(Len::Pct(100.0));
        if item.active {
            line.fill(Role::Field)
        } else {
            line
        }
    });
    Response {
        el: col([header, rows.el.into_el()]).id(id),
        changed: rows.changed,
    }
}

/// One row of a tree: `label` indented `depth` levels, with a disclosure
/// arrow when `expanded` is `Some`, which a press on the arrow flips. A tree
/// is these rows for the open branches, flattened, in a list or a column.
///
/// ```
/// use mui::prelude::*;
/// let mut ui = Ui::default();
/// let mut open = false;
/// let r = tree_row(&mut ui, "bank/drums", "Drums", 0, Some(&mut open));
/// assert!(!r.changed);
/// ```
pub fn tree_row(
    ui: &mut Ui,
    id: impl Into<Id>,
    label: &str,
    depth: usize,
    expanded: Option<&mut bool>,
) -> Response<bool, El> {
    let id: Id = id.into();
    let toggle = id.field("toggle");
    let mut changed = false;
    let arrow = match expanded {
        Some(open) => {
            if ui.get(toggle.as_str()).activated() {
                *open = !*open;
                changed = true;
            }
            icon(if *open {
                sym::ARROW_DROP_DOWN
            } else {
                sym::ARROW_RIGHT
            })
            .cursor(Cursor::Hand)
            .a11y(A11y::Toggle { on: *open })
            .named(label)
            .focusable()
            .id(toggle)
        }
        None => row([]).w(24.0),
    };
    let el = row([arrow, text(label)])
        .gap(Xs)
        .align(Align::Center)
        .pad(Pad::Insets(Insets {
            left: 8.0 + 16.0 * depth as f64,
            ..Insets::symmetric(8.0, 0.0)
        }))
        .h(28.0)
        .id(id);
    Response { el, changed }
}
