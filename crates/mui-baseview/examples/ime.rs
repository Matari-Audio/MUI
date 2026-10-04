//! Native IME smoke view: `cargo run -p mui-baseview --example ime`.
use mui::prelude::*;
use mui_baseview::{Requests, Shared, View, run};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct App {
    first: String,
    second: String,
    commits: usize,
}
impl View for App {
    fn build(&mut self, ui: &mut Ui, input: &Input) -> El {
        self.commits += input
            .ime
            .iter()
            .filter(|event| matches!(event, Ime::Commit(_)))
            .count();
        let first = text_input(ui, "first", &mut self.first).el;
        let second = text_input(ui, "second", &mut self.second).el;
        col![
            title("Native composition"),
            caption("Compose in either field, switch focus, resize and reopen."),
            first,
            second,
            caption(format!("Native commit events: {}", self.commits)),
            caption(format!("First value: {}", self.first)),
            caption(format!("Second value: {}", self.second)),
        ]
        .gap(M)
        .pad(L)
        .fill(Role::Surface)
    }
    fn changed(&mut self) -> bool {
        false
    }
    fn request_resize(&mut self, _: u32, _: u32) -> bool {
        false
    }
}
fn main() {
    let mut ui = Ui::default();
    if let Ok(font) = Font::new(epaint_default_fonts::HACK_REGULAR) {
        ui = ui.font(font);
    }
    let shared = Arc::new(Mutex::new(Shared {
        ui,
        view: App {
            first: "a😀é".into(),
            second: "a😀é".into(),
            ..App::default()
        },
    }));
    run(
        "MUI native IME",
        (560, 360),
        shared,
        Arc::new(Requests::default()),
    );
}
