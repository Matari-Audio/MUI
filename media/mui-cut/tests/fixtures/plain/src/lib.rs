//! Two panels of knobs: the part tree mui-cut discovers.
use mui::host::View;
use mui::prelude::*;

/// Only the local MUI tree has headless editors (until it is merged): this
/// crate builds only against it.
pub type LocalOnly = mui::host::headless::Headless;

pub struct Knobs {
    values: [f64; 4],
}

impl View for Knobs {
    fn build(&mut self, ui: &mut Ui, _: &Input) -> El {
        let [a, b, c, d] = &mut self.values;
        let panel = |name: &str, body: El| {
            col([text(name.to_uppercase()).fill(Role::Dim), body])
                .gap(10.)
                .pad(16.)
                .radius(12.)
                .fill(Role::Surface)
                .grow(1.)
                .id(name)
        };
        let tone = panel(
            "tone",
            row([
                knob(ui, "tone-color", "Color", a, 0.0..=1.0).el.into_el(),
                knob(ui, "tone-body", "Body", b, 0.0..=1.0).el.into_el(),
            ])
            .gap(16.),
        );
        let space = panel(
            "space",
            row([
                knob(ui, "space-size", "Size", c, 0.0..=1.0).el.into_el(),
                knob(ui, "space-mix", "Mix", d, 0.0..=1.0).el.into_el(),
            ])
            .gap(16.),
        );
        row([tone, space])
            .gap(12.)
            .pad(12.)
            .size(480., 220.)
            .fill(Role::Background)
            .id("root")
    }
    fn changed(&mut self) -> bool {
        false
    }
    fn request_resize(&mut self, _: u32, _: u32) -> bool {
        false
    }
}

/// mui-cut's one convention for a plugin with no framework.
pub fn mui_editor() -> (Ui, (u32, u32), Knobs) {
    let mut ui = Ui::default();
    ui.set_font(Font::new(ttf_inter::REGULAR).ok());
    (ui, (480, 220), Knobs { values: [0.3, 0.6, 0.5, 0.2] })
}
