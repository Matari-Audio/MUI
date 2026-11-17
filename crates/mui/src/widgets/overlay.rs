//! Things that float over the editor: a popover pinned to what opened it,
//! a menu in one, a select that opens one, and a modal dialog.
//!
//! Each returns the floating element, or `None` while closed, for the caller
//! to push anywhere in its tree: a float is absolute and painted after
//! everything, so where it is pushed does not change where it shows. Each
//! closes itself on a press outside it (and outside its anchor) or Escape,
//! unless it is [`persistent`](Overlay::persistent).
//!
//! The free functions wear MUI's look. [`Overlay`] is the same mechanics
//! with every look a slot: the frame around the content, a menu row, a
//! select's head, the dialog's scrim, and where the float is pinned. A
//! consumer with its own design system keeps its look and drops its own
//! open/close, dismissal, wheel and focus handling.
use mui_scene::prelude::*;

use crate::Ui;
use crate::widgets::Response;
use crate::widgets::presets::card;

/// A look: what wraps an element.
type Look<'a> = &'a dyn Fn(El) -> El;

/// One row of a menu or select list, for [`Overlay::row`] to dress.
#[derive(Clone, Copy, Debug)]
pub struct MenuRow<'a> {
    pub label: &'a str,
    pub index: usize,
    /// The select's current option. Always `false` in a menu.
    pub selected: bool,
}

/// A select's head, for [`Overlay::head`] to dress: the current option's
/// label, and whether the list is open. With [`weld`](Overlay::weld) the
/// head is drawn a second time, open, as the top of the list.
#[derive(Clone, Copy, Debug)]
pub struct SelectHead<'a> {
    pub label: &'a str,
    pub open: bool,
}

/// How an overlay floats and what it wears. `Overlay::default()` is what
/// [`popover`], [`menu`], [`select`] and [`dialog`] use; set the slots you
/// want to own and call the same-named method.
///
/// Every slot returns the *face*: MUI adds the id, focus, the role and the
/// cursor, and reads the activation, so a styled row is still a keyboard-
/// and screen-reader-complete row. A frame replaces MUI's card entirely
/// (fill, radius, padding, shadow): nothing of the card is merged under it.
///
/// ```
/// use mui::prelude::*;
/// let mut ui = Ui::default();
/// let mut open = true;
/// let frame = |el: El| el.pad(S).fill(Role::Surface).radius(4.0);
/// let row = |r: MenuRow<'_>| text(r.label).pad((8.0, 0.0));
/// let m = Overlay::default()
///     .frame(&frame)
///     .row(&row)
///     .area(Area::BottomEnd)
///     .menu(&mut ui, "file", "file-button", &mut open, &["Save", "Load"]);
/// assert!(m.el.is_some());
/// ```
#[derive(Clone, Copy)]
pub struct Overlay<'a> {
    area: Area,
    fallback: Option<Area>,
    gap: Spacing,
    match_width: bool,
    min_w: f64,
    row_h: f64,
    persistent: bool,
    weld: bool,
    frame: Option<Look<'a>>,
    row: &'a dyn for<'r> Fn(MenuRow<'r>) -> El,
    head: &'a dyn for<'r> Fn(SelectHead<'r>) -> El,
    scrim: Look<'a>,
}

impl Default for Overlay<'_> {
    fn default() -> Self {
        Self {
            area: Area::BottomStart,
            fallback: Some(Area::TopStart),
            gap: Spacing::Token(Xs),
            match_width: false,
            min_w: 140.0,
            row_h: 32.0,
            persistent: false,
            weld: false,
            frame: None,
            row: &row_face,
            head: &head_face,
            scrim: &scrim_face,
        }
    }
}

impl<'a> Overlay<'a> {
    /// Where the float sits against its anchor (default `BottomStart`).
    /// Clears the fallback; set one with [`fallback`](Self::fallback).
    pub fn area(mut self, area: Area) -> Self {
        self.area = area;
        self.fallback = None;
        self
    }
    /// The area tried when [`area`](Self::area) leaves the window.
    pub fn fallback(mut self, area: Area) -> Self {
        self.fallback = Some(area);
        self
    }
    /// Distance from the anchor (default `Xs`).
    pub fn gap(mut self, gap: impl Into<Spacing>) -> Self {
        self.gap = gap.into();
        self
    }
    /// As wide as the anchor: a select's list under its field.
    pub fn match_width(mut self) -> Self {
        self.match_width = true;
        self
    }
    /// A menu or select list's narrowest width (default 140).
    pub fn min_w(mut self, w: f64) -> Self {
        self.min_w = w;
        self
    }
    /// A list row's height (default 32). Rows are this tall whatever the
    /// face asks, so a list knows its height before it is laid out.
    pub fn row_h(mut self, h: f64) -> Self {
        self.row_h = h;
        self
    }
    /// No closing on an outside press or Escape: the caller closes it.
    pub fn persistent(mut self) -> Self {
        self.persistent = true;
        self
    }
    /// A select opens *from* its field: the list floats over the head, its
    /// top a second, open [`head`](Self::head), as wide as the field.
    pub fn weld(mut self) -> Self {
        self.weld = true;
        self
    }
    /// The frame around a popover's, menu's or dialog's content. Replaces
    /// MUI's card, padding included.
    pub fn frame(mut self, f: Look<'a>) -> Self {
        self.frame = Some(f);
        self
    }
    /// A menu or select row's face.
    pub fn row(mut self, f: &'a dyn for<'r> Fn(MenuRow<'r>) -> El) -> Self {
        self.row = f;
        self
    }
    /// A select's head face.
    pub fn head(mut self, f: &'a dyn for<'r> Fn(SelectHead<'r>) -> El) -> Self {
        self.head = f;
        self
    }
    /// The window-sized layer under a dialog: its dim, a blur, anything.
    pub fn scrim(mut self, f: Look<'a>) -> Self {
        self.scrim = f;
        self
    }

    fn pin(&self, anchor: &str) -> Pin {
        let mut pin = Pin::to(anchor).area(self.area).gap(self.gap);
        if let Some(fallback) = self.fallback {
            pin = pin.fallback(fallback);
        }
        if self.match_width {
            pin = pin.match_width();
        }
        pin
    }

    /// Closes `*open` on a dismissal, claims the wheel; whether it is up.
    fn mount(&self, ui: &mut Ui, id: &Id, outside: &[&str], open: &mut bool) -> bool {
        if *open && !self.persistent && ui.dismissed(outside) {
            *open = false;
        }
        if *open {
            ui.capture_popup_wheel(id.clone());
        }
        *open
    }

    /// `content` in the frame under `anchor` (above it when there is no room
    /// below), while `*open`. Wheel input stays inside it while it is up.
    pub fn popover(
        &self,
        ui: &mut Ui,
        id: impl Into<Id>,
        anchor: &str,
        open: &mut bool,
        content: impl IntoEl,
    ) -> Option<El> {
        let id: Id = id.into();
        if !self.mount(ui, &id, &[id.as_str(), anchor], open) {
            return None;
        }
        let body = col([content.into_el()]).anchor(Align::Start, Align::Start);
        let body = match self.frame {
            Some(frame) => frame(body),
            None => body.pad(Xs).preset(card()),
        };
        Some(body.pin(self.pin(anchor)).id(id))
    }

    /// `labels` in a popover under `anchor`, while `*open`. `changed` is the
    /// index picked this frame; picking one closes the menu.
    pub fn menu(
        &self,
        ui: &mut Ui,
        id: impl Into<Id>,
        anchor: &str,
        open: &mut bool,
        labels: &[&str],
    ) -> Response<Option<usize>, Option<El>> {
        let id: Id = id.into();
        let (list, chosen) = self.items(ui, &id, anchor, labels, None);
        if chosen.is_some() {
            *open = false;
        }
        Response {
            el: self.popover(ui, id, anchor, open, list),
            changed: chosen,
        }
    }

    /// A head showing `options[*selected]` that opens the options under it
    /// while `*open`; picking one sets `*selected` and closes the list.
    /// Returns the head, with the list floated from it while open, and
    /// whether `*selected` moved. The list is `"{id}/list"`, its rows
    /// `"{id}/list/{i}"`.
    pub fn select(
        &self,
        ui: &mut Ui,
        id: impl Into<Id>,
        options: &[&str],
        selected: &mut usize,
        open: &mut bool,
    ) -> Response<bool, El> {
        let id: Id = id.into();
        let list_id = id.field("list");
        let head_id = list_id.field("head");
        if ui.get(id.clone()).activated() || (*open && ui.get(head_id.clone()).activated()) {
            *open = !*open;
        }
        let (list, chosen) = self.items(ui, &list_id, id.as_str(), options, Some(*selected));
        let changed = chosen.is_some_and(|i| i != *selected);
        if let Some(i) = chosen {
            *selected = i;
            *open = false;
        }
        let label = options.get(*selected).copied().unwrap_or("");
        let head = |open: bool, key: Id| {
            press((self.head)(SelectHead { label, open }), label, key).a11y(A11y::Button)
        };
        let field = head(*open, id.clone());
        let pop = if self.weld {
            // The open field: its head again on top, the rows seamed under
            // it, floated over the closed one by its own height.
            let tall = ui
                .scene()
                .and_then(|s| s.surface(id.as_str()))
                .map_or(self.row_h, |s| s.frame.size.height);
            Overlay {
                area: Area::Bottom,
                fallback: Some(Area::Top),
                gap: Spacing::Px(-tall),
                match_width: true,
                ..*self
            }
            .popover(
                ui,
                list_id,
                id.as_str(),
                open,
                col([head(true, head_id).w(Len::Pct(100.0)), list]),
            )
        } else {
            self.popover(ui, list_id, id.as_str(), open, list)
        };
        let el = match pop {
            Some(pop) => stack([field, pop]),
            None => field,
        };
        Response { el, changed }
    }

    /// `content` in the frame centred over the [`scrim`](Self::scrim),
    /// while `*open`. The scrim takes every press, so nothing under it
    /// responds; a press on it or Escape sets `*open` false. Keyboard focus
    /// stays inside while it is up and goes back to what had it when it
    /// closes. Push it as the last child of the root.
    pub fn dialog(
        &self,
        ui: &mut Ui,
        id: impl Into<Id>,
        open: &mut bool,
        content: impl IntoEl,
    ) -> Option<El> {
        let id: Id = id.into();
        if !self.mount(ui, &id, &[id.as_str()], open) {
            return None;
        }
        ui.trap_focus(id.clone());
        let body = col([content.into_el()]);
        let body = match self.frame {
            Some(frame) => frame(body),
            None => body.pad(M).preset(card()),
        };
        let layer = stack([body.a11y(A11y::Group).id(id.clone())])
            .center()
            .w(Len::Pct(100.0))
            .h(Len::Pct(100.0));
        Some((self.scrim)(layer).float().id(id.field("scrim")))
    }

    /// The rows of a list, each `"{id}/{index}"`; returns the index
    /// activated. A list taller than the room beside `anchor` scrolls
    /// inside that room.
    fn items(
        &self,
        ui: &mut Ui,
        id: &Id,
        anchor: &str,
        labels: &[&str],
        selected: Option<usize>,
    ) -> (El, Option<usize>) {
        let mut chosen = None;
        let rows: Vec<El> = labels
            .iter()
            .enumerate()
            .map(|(index, &label)| {
                let key = id.slot(index);
                if ui.get(key.clone()).activated() {
                    chosen = Some(index);
                }
                let row = MenuRow {
                    label,
                    index,
                    selected: selected == Some(index),
                };
                press((self.row)(row), label, key)
                    .a11y(A11y::Button)
                    .h(self.row_h)
                    .w(Len::Pct(100.0))
            })
            .collect();
        let mut list = col(rows).gap(0.0).min_w(self.min_w);
        let tall = self.row_h * labels.len() as f64;
        if let Some(room) = room(ui, anchor, self.row_h).filter(|&room| tall > room) {
            let scroller = id.field("rows");
            reveal(ui, id, &scroller, labels.len(), room, self.row_h);
            list = list.h(room).scroll().id(scroller);
        }
        (list, chosen)
    }
}

/// `face` as something to press: named, focusable, a hand, under `key`.
fn press(face: El, label: &str, key: Id) -> El {
    face.cursor(Cursor::Hand).named(label).focusable().id(key)
}

/// MUI's row: the label, the selected one in the accent, highlighted under
/// the pointer and when focused.
fn row_face(r: MenuRow<'_>) -> El {
    let ink = if r.selected { Role::Primary } else { Role::Ink };
    row([text(r.label).fill(ink)])
        .align(Align::Center)
        .pad((10.0, 0.0))
        .radius(6.0)
        .on(State::Hover, |s| s.fill(Role::Field))
        .on(State::FocusVisible, |s| s.fill(Role::Field))
}

/// MUI's select head: the label and a drop-down arrow on a field.
fn head_face(h: SelectHead<'_>) -> El {
    let arrow = if h.open {
        sym::ARROW_DROP_UP
    } else {
        sym::ARROW_DROP_DOWN
    };
    row([text(h.label), icon(arrow)])
        .gap(Xs)
        .align(Align::Center)
        .h(32.0)
        .pad((10.0, 0.0))
        .radius(6.0)
        .fill(Role::Field)
        .on(State::Hover, |s| s.fill(Role::Raised))
}

/// MUI's scrim: the window dimmed.
fn scrim_face(layer: El) -> El {
    layer.fill(Color::srgba(0.0, 0.0, 0.0, 0.45))
}

/// Scroll a list just far enough to show the row keyboard focus moved to.
/// Only on a move, so a wheel scroll away from a focused row stays put.
fn reveal(ui: &mut Ui, id: &Id, scroller: &Id, rows: usize, room: f64, row_h: f64) {
    let focused = (0..rows).find(|&i| ui.focus_visible(id.slot(i)));
    if focused == ui.stash::<usize>(scroller.as_str()).copied() {
        return;
    }
    ui.set_stash(scroller.as_str(), focused);
    let Some(i) = focused else {
        return;
    };
    let [x, y] = ui.scroll(scroller.clone());
    let (top, bottom) = (i as f64 * row_h, (i + 1) as f64 * row_h);
    let y = if top < y {
        top
    } else if bottom > y + room {
        bottom - room
    } else {
        return;
    };
    ui.set_scroll(scroller.clone(), [x, y]);
}

/// The larger of the window's room below and above `anchor`, less the
/// popover's gap and padding, as last frame laid them out.
fn room(ui: &Ui, anchor: &str, row_h: f64) -> Option<f64> {
    let scene = ui.scene()?;
    let window = *scene.layout.all().first()?;
    let a = scene.surface(anchor)?.frame;
    // The gap, the card's padding above and below, and a margin to the edge.
    let spare = 4.0 * 8.0;
    let below = window.size.height - (a.y + a.size.height) - spare;
    let above = a.y - window.y - spare;
    Some(below.max(above).max(row_h))
}

/// `content` in a raised card under `anchor` (above it when there is no room
/// below), while `*open`. Sets `*open` false on a press outside both, or
/// Escape. Wheel input stays inside it while it is up. [`Overlay::popover`]
/// in another look.
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
    Overlay::default().popover(ui, id, anchor, open, content)
}

/// A list of `labels` in a [`popover`] under `anchor`, while `*open`.
/// `changed` is the index picked this frame; picking one closes the menu.
/// [`Overlay::menu`] in another look.
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
    Overlay::default().menu(ui, id, anchor, open, labels)
}

/// A field showing `options[*selected]` that opens the options under it;
/// picking one sets `*selected` and closes the list. The open state is the
/// `Ui`'s, kept under `id`; own it with [`Overlay::select`].
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
    let mut open = ui.stash::<bool>(id.as_str()).copied().unwrap_or(false);
    let r = Overlay::default().select(ui, id.clone(), options, selected, &mut open);
    ui.set_stash(id.as_str(), open.then_some(true));
    r
}

/// `content` in a card centred over a dimmed window, while `*open`. The dim
/// takes every press, so nothing under it responds; a press on it or Escape
/// sets `*open` false. Keyboard focus stays inside it while it is up and
/// goes back to what had it when it closes. Push it as the last child of
/// the root. [`Overlay::dialog`] in another look.
///
/// ```
/// use mui::prelude::*;
/// let mut ui = Ui::default();
/// let mut open = true;
/// let d = dialog(&mut ui, "confirm", &mut open, col![text("Discard changes?")]);
/// assert!(d.is_some());
/// ```
pub fn dialog(ui: &mut Ui, id: impl Into<Id>, open: &mut bool, content: impl IntoEl) -> Option<El> {
    Overlay::default().dialog(ui, id, open, content)
}
