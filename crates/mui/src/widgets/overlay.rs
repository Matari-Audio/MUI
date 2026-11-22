//! Things that float over the editor: a popover pinned to what opened it,
//! a menu in one, a select or a dropdown that opens one, and a modal dialog.
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
use mui_input::Key;
use mui_scene::prelude::*;

use mui_scene::{Palette, Px, SpacingToken};

use crate::Ui;
use crate::widgets::controls::button_paint;
use crate::widgets::presets::card;
use crate::widgets::{Response, Variant};

/// A look: what wraps an element.
type Look<'a> = &'a dyn Fn(El) -> El;

/// One row of a menu or select list, for [`Overlay::row`] to dress.
#[derive(Clone, Copy, Debug)]
pub struct MenuRow<'a> {
    pub label: &'a str,
    pub index: usize,
    /// The select's or dropdown's current option; in a menu, the row
    /// [`Overlay::current`] names.
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
    columns: usize,
    current: Option<usize>,
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
            columns: 1,
            current: None,
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
    /// Lay a list out `n` to a row: a grid of short picks (notes, MIDI
    /// channels). The arrow keys walk it in two dimensions. Default 1.
    pub fn columns(mut self, n: usize) -> Self {
        self.columns = n.max(1);
        self
    }
    /// The menu row marked selected, and where Down on the anchor enters.
    pub fn current(mut self, index: Option<usize>) -> Self {
        self.current = index;
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
        // Down on the anchor enters the list.
        if *open && ui.keys(anchor).iter().any(|k| k.key == Key::Down) {
            ui.focus(id.slot(self.current.unwrap_or(0)));
        }
        let (list, chosen) = self.items(ui, &id, anchor, labels, self.current);
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
    /// Returns the head as a [`Select`], with the list floated from it
    /// while open, and whether `*selected` moved. The list is
    /// `"{id}/list"`, its rows `"{id}/list/{i}"`.
    pub fn select(
        &self,
        ui: &mut Ui,
        id: impl Into<Id>,
        options: &[&str],
        selected: &mut usize,
        open: &mut bool,
    ) -> Response<bool, Select> {
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
        let theme = ui.theme();
        let el = Select {
            trigger: field,
            list: pop,
            variant: None,
            role: None,
            size: None,
            px: None,
            palette: theme.palette,
            unit: theme.control,
        };
        Response { el, changed }
    }

    /// `face` as a button that opens `labels` under it while `*open`; the
    /// index picked comes back and closes the list. `current` is marked
    /// selected and is where the Down key enters the list. The face is the
    /// caller's whole head -- an icon, a [`chooser`], a waveform -- and MUI
    /// makes it pressable as `id`, named `name`. The list is `"{id}/list"`,
    /// its rows `"{id}/list/{i}"`.
    #[expect(
        clippy::too_many_arguments,
        reason = "a select's arguments plus its face and name"
    )]
    pub fn dropdown(
        &self,
        ui: &mut Ui,
        id: impl Into<Id>,
        name: &str,
        face: impl IntoEl,
        open: &mut bool,
        labels: &[&str],
        current: Option<usize>,
    ) -> Response<Option<usize>, El> {
        let id: Id = id.into();
        let list_id = id.field("list");
        if ui.get(id.clone()).activated() {
            *open = !*open;
        }
        if *open && ui.keys(id.clone()).iter().any(|k| k.key == Key::Down) {
            ui.focus(list_id.slot(current.unwrap_or(0)));
        }
        let (list, chosen) = self.items(ui, &list_id, id.as_str(), labels, current);
        if chosen.is_some() {
            *open = false;
        }
        let head = press(face.into_el(), name, id.clone()).a11y(A11y::Button);
        // The pointer rests on the head that opened the list: its tip would
        // float over the list.
        let el = match self.popover(ui, list_id, id.as_str(), open, list) {
            Some(pop) => stack([head, pop]),
            None => head,
        };
        Response {
            el,
            changed: chosen,
        }
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
        let columns = self.columns;
        walk(ui, id, labels.len(), columns);
        let mut cells: Vec<El> = labels
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
                let cell = press((self.row)(row), label, key)
                    .a11y(A11y::Button)
                    .h(self.row_h);
                // A list's rows reach its edge; a grid's cells share a row.
                if columns == 1 {
                    cell
                } else {
                    cell.grow(1.0).basis(0.0)
                }
            })
            .collect();
        let rows = if columns == 1 {
            cells
        } else {
            let mut rows = Vec::new();
            while !cells.is_empty() {
                let rest = cells.split_off(columns.min(cells.len()));
                rows.push(row(std::mem::replace(&mut cells, rest)).gap(0.0));
            }
            rows
        };
        let lines = rows.len();
        let mut list = col(rows).gap(0.0).min_w(self.min_w);
        let tall = self.row_h * lines as f64;
        if let Some(room) = room(ui, anchor, self.row_h).filter(|&room| tall > room) {
            let scroller = id.field("rows");
            reveal(ui, id, &scroller, labels.len(), columns, room, self.row_h);
            list = list.h(room).scroll().id(scroller);
        }
        (list, chosen)
    }
}

/// A select's trigger, still taking its look: the same `variant`, `role`,
/// `size` and `px` as a [`button`](crate::widgets::button). Finished, it
/// *is* the trigger, with the open list floated from inside it, so every
/// paint verb after it (`.fill`, `.radius`, `.stroke`, `.pad`, ..) lands on
/// the trigger whether the list is open or not.
///
/// With no look set the trigger wears [`Overlay::head`]'s face as it
/// is. A variant or role replaces that face's box paint and hover with the
/// button's, and its leaf children (the label and arrow) take the
/// variant's ink. A [`weld`](Overlay::weld)ed list's own copy of the head
/// is the head face, untouched: dress it through [`Overlay::head`].
///
/// ```
/// use mui::prelude::*;
/// let mut ui = Ui::default();
/// let mut wave = 0;
/// let plate = select(&mut ui, "wave", &["Saw", "Square"], &mut wave)
///     .variant(Variant::Outline)
///     .size(S)
///     .fill(Role::Surface)
///     .radius(3);
/// let el: El = plate.el;
/// assert_eq!(el.payload().style.fill, Some(Fill::Role(Role::Surface)));
/// assert_eq!(el.payload().style.radius, Some(Radius::Px(3.0)));
/// ```
pub struct Select {
    trigger: El,
    list: Option<El>,
    variant: Option<Variant>,
    role: Option<Role>,
    size: Option<SpacingToken>,
    px: Option<f64>,
    palette: Palette,
    unit: f64,
}
impl Select {
    /// How solid the trigger looks, as a button's [`Variant`].
    pub fn variant(mut self, v: Variant) -> Self {
        self.variant = Some(v);
        self
    }
    /// The role the trigger speaks with (`Primary` once a variant is set).
    /// A role alone paints it `Solid`, like a button.
    pub fn role(mut self, r: Role) -> Self {
        self.role = Some(r);
        self
    }
    /// One of the five control sizes: the trigger's height off the theme's
    /// [`control`](mui_scene::Theme::control) unit, as a button's.
    pub fn size(mut self, s: SpacingToken) -> Self {
        self.size = Some(s);
        self
    }
    /// The trigger's height in pixels. Overrides [`Select::size`].
    pub fn px(mut self, px: impl Px) -> Self {
        self.px = Some(px.px());
        self
    }
    /// The trigger, finished, its list floated from it while open.
    pub fn el(self) -> El {
        let mut t = self.trigger;
        if let Some(variant) = self.variant.or(self.role.map(|_| Variant::Solid)) {
            let role = self.role.unwrap_or(Role::Primary);
            let (style, ink, hover) = button_paint(self.palette, variant, role);
            for c in t.children_mut().iter_mut().filter(|c| !c.is_container()) {
                c.style_mut().fill = Some(ink.clone());
            }
            t.style_mut().fill = None;
            t = t.preset(style).on(State::Hover, hover);
        }
        let steps = self.size.map(|s| match s {
            SpacingToken::Xs => 6.0,
            SpacingToken::S => 8.0,
            SpacingToken::M => 10.0,
            SpacingToken::L => 12.0,
            SpacingToken::Xl => 14.0,
        });
        if let Some(px) = self.px.or(steps.map(|n| n * self.unit)) {
            t = t.h(px).pad((px * 0.3, 0.0));
        }
        match self.list {
            // A float is placed against its anchor and painted after
            // everything, so inside the trigger it neither sizes nor
            // covers it; the trigger stays the outermost node.
            Some(list) => t.push(list),
            None => t,
        }
    }
}
impl IntoEl for Select {
    fn into_el(self) -> El {
        self.el()
    }
}
impl From<Select> for El {
    fn from(s: Select) -> Self {
        s.el()
    }
}

/// The arrow keys on a focused row move the focus a row (Up, Down) or a
/// cell (Left, Right, in a grid), stopping at the ends.
fn walk(ui: &mut Ui, id: &Id, len: usize, columns: usize) {
    for i in 0..len {
        let by = ui
            .keys(id.slot(i))
            .iter()
            .fold(0isize, |at, k| match k.key {
                Key::Down => at + columns as isize,
                Key::Up => at - columns as isize,
                Key::Right if columns > 1 => at + 1,
                Key::Left if columns > 1 => at - 1,
                _ => at,
            });
        if by != 0 {
            let to = i.saturating_add_signed(by).min(len - 1);
            ui.focus(id.slot(to));
            return;
        }
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

/// MUI's select head: a [`chooser`] on a field. No hover look of its
/// own: the runtime lifts whatever fill it ends up with, a caller's
/// included.
fn head_face(h: SelectHead<'_>) -> El {
    chooser(text(h.label), h.open)
        .h(32.0)
        .pad((10.0, 0.0))
        .radius(6.0)
        .fill(Role::Field)
}

/// What says "this opens a list": `face` (the current pick, an icon and a
/// name, a waveform) and a dim drop arrow after it, pointing up while
/// `open`. Hand it to [`Overlay::dropdown`] or
/// [`Overlay::head`]; it carries no id, so it is not yet pressable.
///
/// ```
/// use mui::prelude::*;
/// let head = chooser(text("Saw").fill(Role::Primary), false).pad(S);
/// ```
pub fn chooser(face: impl IntoEl, open: bool) -> El {
    let arrow = if open {
        sym::ARROW_DROP_UP
    } else {
        sym::ARROW_DROP_DOWN
    };
    row([face.into_el(), icon(arrow).fill(Role::Dim)])
        .gap(Xs)
        .align(Align::Center)
}

/// MUI's scrim: the window dimmed.
fn scrim_face(layer: El) -> El {
    layer.fill(Color::srgba(0.0, 0.0, 0.0, 0.45))
}

/// Scroll a list just far enough to show the row keyboard focus moved to.
/// Only on a move, so a wheel scroll away from a focused row stays put.
fn reveal(ui: &mut Ui, id: &Id, scroller: &Id, len: usize, columns: usize, room: f64, row_h: f64) {
    let focused = (0..len).find(|&i| ui.focus_visible(id.slot(i)));
    if focused == ui.stash::<usize>(scroller.as_str()).copied() {
        return;
    }
    ui.set_stash(scroller.as_str(), focused);
    let Some(i) = focused else {
        return;
    };
    let [x, y] = ui.scroll(scroller.clone());
    let line = (i / columns) as f64;
    let (top, bottom) = (line * row_h, (line + 1.0) * row_h);
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
/// let s = s.variant(Variant::Soft).size(S);
/// ```
pub fn select(
    ui: &mut Ui,
    id: impl Into<Id>,
    options: &[&str],
    selected: &mut usize,
) -> Response<bool, Select> {
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

/// `face` as a button that opens `labels` in a list under it; the index
/// picked comes back. The open state is the `Ui`'s, kept under `id`; own it
/// with [`Overlay::dropdown`], which also takes a grid and a look.
///
/// ```
/// use mui::prelude::*;
/// let mut ui = Ui::default();
/// let d = dropdown(&mut ui, "voices", "Voices", chooser(text("8"), false), &["1", "8", "16"], Some(1));
/// assert_eq!(d.changed, None);
/// ```
pub fn dropdown(
    ui: &mut Ui,
    id: impl Into<Id>,
    name: &str,
    face: impl IntoEl,
    labels: &[&str],
    current: Option<usize>,
) -> Response<Option<usize>, El> {
    let id: Id = id.into();
    let mut open = ui.stash::<bool>(id.as_str()).copied().unwrap_or(false);
    let r = Overlay::default().dropdown(ui, id.clone(), name, face, &mut open, labels, current);
    ui.set_stash(id.as_str(), open.then_some(true));
    r
}
