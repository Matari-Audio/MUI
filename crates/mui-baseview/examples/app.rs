//! A standalone MUI app: a knob and the label that reads it, in a top-level
//! window. `cargo run --example app -p mui-baseview`.
use std::sync::{Arc, Mutex};

use mui::prelude::*;
use mui_baseview::{Requests, Shared, View, run};

struct App {
    gain: f64,
}

impl View for App {
    fn build(&mut self, ui: &mut Ui, _: &Input) -> El {
        let knob = knob(ui, "gain", "Gain", &mut self.gain, 0.0..=1.0).size(L);
        col![knob, title(format!("{:.0} %", self.gain * 100.0))]
            .gap(M)
            .pad(L)
            .center()
            .fill(Role::Surface)
    }
    // Nothing outside the `Ui` moves the model.
    fn changed(&mut self) -> bool {
        false
    }
    // A top-level window is resized by its user, not asked.
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
        view: App { gain: 0.5 },
    }));
    run("MUI app", (240, 200), shared, Arc::new(Requests::default()));
}
