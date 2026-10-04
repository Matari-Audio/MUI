//! `cargo run -p mui-winit --example standalone`
//! A real top-level native window; click the text field to exercise the OS IME.
use mui::prelude::*;
use mui_winit::{Options, View};

struct Demo {
    text: String,
    gain: f64,
}
impl View for Demo {
    fn build(&mut self, ui: &mut Ui, _: &Input) -> El {
        col([
            text_input(ui, "text", &mut self.text).into(),
            knob(ui, "gain", "Gain", &mut self.gain, 0.0..=1.0).into(),
        ])
    }
    fn changed(&mut self) -> bool {
        false
    }
    fn request_resize(&mut self, _: u32, _: u32) -> bool {
        false
    }
}
fn main() -> Result<(), String> {
    let font = Font::new(epaint_default_fonts::HACK_REGULAR).map_err(|e| e.to_string())?;
    mui_winit::run(
        Demo {
            text: "Native MUI".into(),
            gain: 0.5,
        },
        Ui::default().font(font),
        Options {
            title: "MUI standalone".into(),
            size: (640, 480),
            ..Options::default()
        },
    )
}
