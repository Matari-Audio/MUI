//! Tabs, toasts, a table and tree rows: the rest of an editor's kit, built
//! from the same rows, cards and lists as everything else.
use std::sync::Arc;

use mui_input::Key;
use mui_layout::{Insets, Pad};
use mui_scene::prelude::*;

use crate::Ui;
use crate::widgets::presets::card;
use crate::widgets::{ListEvent, ListItem, ListOptions, ListState, Response, uniform_list};

/// A row of `labels`, one selected. A press picks a tab; Left and Right on a
/// focused tab move the selection and the focus with it. `changed` is whether
/// `*selected` moved this frame.
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
    let id: Id = id.into();
    let before = *selected;
    let key = |i: usize| Id::runtime(&format!("{}/{i}", id.as_str()));
    for i in 0..labels.len() {
        if ui.get(key(i).as_str()).activated() {
            *selected = i;
        }
        let step: isize = ui.keys(key(i)).iter().fold(0, |d, k| match k.key {
            Key::Right => d + 1,
            Key::Left => d - 1,
            _ => d,
        });
        if step != 0 {
            *selected = (i as isize + step).rem_euclid(labels.len() as isize) as usize;
            // Focus moves on, so the tab it lands on would read these keys again.
            ui.focus(key(*selected));
            break;
        }
    }
    let row_el = row(labels.iter().enumerate().map(|(i, label)| {
        let on = i == *selected;
        let tab = row([text(*label).fill(if on { Role::Primary } else { Role::Dim })])
            .h(32.0)
            .align(Align::Center)
            .pad((12.0, 0.0))
            .radius(6.0)
            .on(State::Hover, |s| s.fill(Role::Field))
            .on(State::FocusVisible, |s| s.fill(Role::Field))
            .cursor(Cursor::Hand)
            .a11y(A11y::Toggle { on })
            .named(*label)
            .focusable()
            .id(key(i));
        if on { tab.fill(Role::Field) } else { tab }
    }))
    .gap(Xs)
    .a11y(A11y::Group)
    .id(id);
    Response {
        el: row_el,
        changed: *selected != before,
    }
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
    let key = |i: usize| Id::runtime(&format!("{TOASTS}/{i}"));
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
