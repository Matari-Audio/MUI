//! Composable engine surfaces and parameter cells; synth state stays with the owner.
use gpui::{prelude::*, *};
use crate::{controls, live_theme, modulation::{Target}};

/// One independently editable value, including its own focus, hitbox and route target.
pub fn parameter_cell(
    id: impl Into<ElementId>, title: impl Into<SharedString>, value: impl Into<SharedString>,
    target: Option<Target>, enabled: bool, scale: f32,
    change: impl Fn(&f32, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let title = title.into();
    let value = value.into();
    let cell = div().id(id).flex_1().min_w_0().h_full().flex().flex_col()
        .items_center().justify_center().rounded_sm().tab_index(0).key_context("MUIParameter")
        .role(Role::Slider).aria_label(format!("{title} {value}; drag or use arrow keys"))
        .cursor(CursorStyle::ResizeUpDown).hover(|s| s.bg(rgba(0xffffff08)))
        .focus_visible(|s| s.bg(rgba(0xbadc9118)))
        .child(div().text_size(px(8.5 * scale)).text_color(live_theme::color(0x9ba697)).child(title))
        .child(div().text_size(px(16. * scale)).text_color(live_theme::color(if enabled { 0xbadc91 } else { 0x9ba697 })).child(value));
    // A target is optional for controls such as MIDI channel; no fake route is manufactured.
    match target {
        Some(target) => controls::parameter(cell, target, enabled, change),
        None => {
            let change = std::rc::Rc::new(change);
            let up = change.clone(); let down = change.clone(); let fine = change.clone();
            cell.on_action(move |_: &controls::Increase, w, cx| up(&1., w, cx))
                .on_action(move |_: &controls::Decrease, w, cx| down(&-1., w, cx))
                .on_action(move |_: &controls::FineIncrease, w, cx| fine(&0.1, w, cx))
                .on_action(move |_: &controls::FineDecrease, w, cx| change(&-0.1, w, cx))
        }
    }
}

/// A shared heading over related values. Each child keeps its own hitbox and modulation.
pub fn parameter_group(id: impl Into<ElementId>, title: impl Into<SharedString>, values: Vec<AnyElement>) -> Stateful<Div> {
    div().id(id).flex_1().min_w_0().h_full().flex().flex_col().items_center()
        .child(div().text_size(px(9.)).text_color(live_theme::color(0x9ba697)).child(title.into()))
        .child(parameter_row(values).flex_1())
}

/// Every cell/group gets the same share of the row, including at narrow widths.
pub fn parameter_row(cells: Vec<AnyElement>) -> Div {
    div().w_full().flex().items_center().children(cells.into_iter().map(|cell| {
        div().flex_1().min_w_0().h_full().child(cell)
    }))
}

/// Used by waveform, unison, LFO and other engine views: graph above an equal-width control row.
pub fn engine_panel(id: impl Into<ElementId>, title: impl Into<SharedString>, graph: impl IntoElement, cells: Vec<AnyElement>) -> Stateful<Div> {
    div().id(id).size_full().min_w_0().p(px(16.)).flex().flex_col().rounded_lg()
        .bg(live_theme::color(0x111612))
        .child(div().h(px(20.)).flex_shrink_0().text_size(px(16.)).text_color(live_theme::color(0xe0e5da)).child(title.into()))
        .child(div().mt(px(14.)).flex_1().min_h_0().child(graph))
        .child(parameter_row(cells).mt(px(12.)).h(px(46.)).flex_shrink_0())
}
