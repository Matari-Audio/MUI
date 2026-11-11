//! Things that float over the editor: a popover pinned to what opened it,
//! a menu in one, a select that opens one, and a modal dialog.
//!
//! Each returns the floating element, or `None` while closed, for the caller
//! to push anywhere in its tree: a float is absolute and painted after
//! everything, so where it is pushed does not change where it shows. Each
//! closes itself on a press outside it (and outside its anchor) or Escape.
use mui_scene::prelude::*;

use crate::Ui;
use crate::widgets::presets::card;
use crate::widgets::{Response, button};

/// `content` in a raised card under `anchor` (above it when there is no room
/// below), while `*open`. Sets `*open` false on a press outside both, or
/// Escape. Wheel input stays inside it while it is up.
///
/// ```
/// use mui::prelude::*;
/// let mut ui = Ui::default();
/// let mut open = true;
/// let pop = popover(&mut ui, "tips", "help", &mut open, text("Hold Shift to fine-tune"));
/// assert!(pop.is_some() && open);
/// ```
pub fn popover(
    ui: &mut Ui,
    id: impl Into<Id>,
    anchor: &str,
    open: &mut bool,
    content: impl IntoEl,
) -> Option<El> {
    let id: Id = id.into();
    if *open && ui.dismissed(&[id.as_str(), anchor]) {
        *open = false;
    }
    if !*open {
        return None;
    }
    ui.capture_popup_wheel(id.clone());
    Some(
        col([content.into_el()])
            .anchor(Align::Start, Align::Start)
            .pad(Xs)
            .preset(card())
            .pin(
                Pin::to(anchor)
                    .area(Area::BottomStart)
                    .gap(Xs)
                    .fallback(Area::TopStart),
            )
            .id(id),
    )
}

/// One row of a menu or select list: the label, highlighted under the
/// pointer and when focused.
fn item(id: Id, label: &str, checked: bool) -> El {
    let weight = if checked { Role::Primary } else { Role::Ink };
    row([text(label).fill(weight)])
        .h(ROW)
        .align(Align::Center)
        .pad((10.0, 0.0))
        .radius(6.0)
        .w(Len::Pct(100.0))
        .on(State::Hover, |s| s.fill(Role::Field))
        .on(State::FocusVisible, |s| s.fill(Role::Field))
        .cursor(Cursor::Hand)
        .a11y(A11y::Button)
        .named(label)
        .focusable()
        .id(id)
}

/// The rows of a menu, each `"{id}/{index}"`; returns the index activated.
/// A list taller than the room beside `anchor` scrolls inside that room.
fn items(
    ui: &mut Ui,
    id: &Id,
    anchor: &str,
    labels: &[&str],
    checked: Option<usize>,
) -> (El, Option<usize>) {
    let mut chosen = None;
    let rows = labels.iter().enumerate().map(|(i, label)| {
        let key = Id::runtime(&format!("{}/{i}", id.as_str()));
        if ui.get(key.as_str()).activated() {
            chosen = Some(i);
        }
        item(key, label, checked == Some(i))
    });
    let rows: Vec<El> = rows.collect();
    let mut list = col(rows).min_w(140.0);
    let tall = ROW * labels.len() as f64;
    if let Some(room) = room(ui, anchor).filter(|&room| tall > room) {
        let scroller = Id::runtime(&format!("{}/rows", id.as_str()));
        reveal(ui, id, &scroller, labels.len(), room);
        list = list.h(room).scroll().id(scroller);
    }
    (list, chosen)
}

/// Scroll a list just far enough to show the row keyboard focus moved to.
/// Only on a move, so a wheel scroll away from a focused row stays put.
fn reveal(ui: &mut Ui, id: &Id, scroller: &Id, rows: usize, room: f64) {
    let row = |i: usize| Id::runtime(&format!("{}/{i}", id.as_str()));
    let focused = (0..rows).find(|&i| ui.focus_visible(row(i)));
    if focused == ui.stash::<usize>(scroller.as_str()).copied() {
        return;
    }
    ui.set_stash(scroller.as_str(), focused);
    let Some(i) = focused else {
        return;
    };
    let [x, y] = ui.scroll(scroller.clone());
    let (top, bottom) = (i as f64 * ROW, (i + 1) as f64 * ROW);
    let y = if top < y {
        top
    } else if bottom > y + room {
        bottom - room
    } else {
        return;
    };
    ui.set_scroll(scroller.clone(), [x, y]);
}

/// A menu row's height: fixed, so a list knows its height before it is laid
/// out and scrolls only when it would not fit.
const ROW: f64 = 32.0;

/// The larger of the window's room below and above `anchor`, less the
/// popover's gap and padding, as last frame laid them out.
fn room(ui: &Ui, anchor: &str) -> Option<f64> {
    let scene = ui.scene()?;
    let window = *scene.layout.all().first()?;
    let a = scene.surface(anchor)?.frame;
    // The gap, the card's padding above and below, and a margin to the edge.
    let spare = 4.0 * 8.0;
    let below = window.size.height - (a.y + a.size.height) - spare;
    let above = a.y - window.y - spare;
    Some(below.max(above).max(ROW))
}

/// A list of `labels` in a [`popover`] under `anchor`, while `*open`.
/// `changed` is the index picked this frame; picking one closes the menu.
///
/// ```
/// use mui::prelude::*;
/// let mut ui = Ui::default();
/// let mut open = true;
/// let m = menu(&mut ui, "file", "file-button", &mut open, &["Save", "Load"]);
/// assert_eq!(m.changed, None);
/// assert!(m.el.is_some());
/// ```
pub fn menu(
    ui: &mut Ui,
    id: impl Into<Id>,
    anchor: &str,
    open: &mut bool,
    labels: &[&str],
) -> Response<Option<usize>, Option<El>> {
    let id: Id = id.into();
    let (list, chosen) = items(ui, &id, anchor, labels, None);
    if chosen.is_some() {
        *open = false;
    }
    Response {
        el: popover(ui, id, anchor, open, list),
        changed: chosen,
    }
}

/// A button showing `options[*selected]` that opens the options under it;
/// picking one sets `*selected` and closes the list. Returns the button,
/// with the list floated from it while open, and whether `*selected` moved.
/// The open state is the `Ui`'s, kept under `id`.
///
/// ```
/// use mui::prelude::*;
/// let mut ui = Ui::default();
/// let mut wave = 0;
/// let s = select(&mut ui, "wave", &["Saw", "Square"], &mut wave);
/// assert!(!s.changed);
/// ```
pub fn select(
    ui: &mut Ui,
    id: impl Into<Id>,
    options: &[&str],
    selected: &mut usize,
) -> Response<bool, El> {
    let id: Id = id.into();
    let list_id = Id::runtime(&format!("{}/list", id.as_str()));
    let mut open = ui.stash::<bool>(id.as_str()).copied().unwrap_or(false);
    let label = options.get(*selected).copied().unwrap_or("");
    let toggle = button(ui, id.clone(), label);
    if toggle.changed {
        open = !open;
    }
    let (list, chosen) = items(ui, &list_id, id.as_str(), options, Some(*selected));
    let changed = chosen.is_some_and(|i| i != *selected);
    if let Some(i) = chosen {
        *selected = i;
        open = false;
    }
    let pop = popover(ui, list_id, id.as_str(), &mut open, list);
    ui.set_stash(id.as_str(), open.then_some(true));
    let mut el = row([toggle.el.into_el(), icon(sym::ARROW_DROP_DOWN)])
        .gap(Xs)
        .align(Align::Center);
    if let Some(pop) = pop {
        el = el.push(pop);
    }
    Response { el, changed }
}

/// `content` in a card centred over a dimmed window, while `*open`. The dim
/// takes every press, so nothing under it responds; a press on it or Escape
/// sets `*open` false. Keyboard focus stays inside it while it is up and
/// goes back to what had it when it closes. Push it as the last child of
/// the root.
///
/// ```
/// use mui::prelude::*;
/// let mut ui = Ui::default();
/// let mut open = true;
/// let d = dialog(&mut ui, "confirm", &mut open, col![text("Discard changes?")]);
/// assert!(d.is_some());
/// ```
pub fn dialog(ui: &mut Ui, id: impl Into<Id>, open: &mut bool, content: impl IntoEl) -> Option<El> {
    let id: Id = id.into();
    if *open && ui.dismissed(&[id.as_str()]) {
        *open = false;
    }
    if !*open {
        return None;
    }
    ui.capture_popup_wheel(id.clone());
    ui.trap_focus(id.clone());
    let body = col([content.into_el()])
        .pad(M)
        .preset(card())
        .a11y(A11y::Group)
        .id(id.clone());
    Some(
        stack([body])
            .center()
            .w(Len::Pct(100.0))
            .h(Len::Pct(100.0))
            .fill(Color::srgba(0.0, 0.0, 0.0, 0.45))
            .float()
            .id(Id::runtime(&format!("{}/scrim", id.as_str()))),
    )
}
