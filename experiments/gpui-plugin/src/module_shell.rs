//! A header and an arbitrary panel tree. GPUI flex/grid styles decide placement.
use gpui::{prelude::*, *};

pub fn module_shell(id: impl Into<ElementId>, header: impl IntoElement, panels: impl IntoElement) -> Stateful<Div> {
    div().id(id).size_full().min_w_0().flex().gap(px(12.))
        .child(header).child(div().flex_1().min_w_0().h_full().child(panels))
}

pub fn panel_row(panels: Vec<AnyElement>) -> Div {
    div().size_full().min_w_0().flex().gap(px(12.)).children(panels.into_iter().map(|panel| {
        div().flex_1().min_w_0().h_full().child(panel)
    }))
}

pub fn header(id: impl Into<ElementId>, content: impl IntoElement) -> Stateful<Div> {
    div().id(id).flex_shrink_0().h_full().flex().flex_col().items_center().justify_between()
        .p(px(12.)).gap(px(12.)).child(content)
}
