//! CPU gallery of the compact semantic controls, in both themes.
//! cargo run -p mui --features cpu --example semantic_gallery -- /tmp/controls
use mui::prelude::*;
use mui::vello::vello_cpu::{Pixmap, RenderContext, Resources};

const SIZE: Size = Size {
    width: 480.0,
    height: 340.0,
};

fn panel(ui: &mut Ui) -> El {
    let (mut gain, mut voices, mut sync) = (-6.0, 4.0, true);
    let settings = [
        setting("sync", "Tempo sync")
            .description("Follow the host tempo")
            .toggle(ui, &mut sync)
            .into_el(),
        slider(ui, "gain", "Output gain", &mut gain, -60.0..=6.0)
            .value_text("-6.0 dB")
            .value_reserve("-60.0 dB")
            .described("Level after processing")
            .into_el(),
        row![
            setting("voices", "Voices").control(ui, |ui, id, label| {
                knob(ui, id, label, &mut voices, 1.0..=8.0)
                    .step(1.0)
                    .size(Xs)
                    .value_text("4")
                    .value_reserve("8")
            }),
            spacer(),
            button(ui, "save", "Save").size(S),
            button(ui, "unavailable", "Unavailable").size(S).disabled(),
        ]
        .gap(M)
        .align(Align::Center),
    ];
    stack([group("controls", "Complete controls", settings)
        .w(432)
        .centered()])
    .fill(Role::Background)
}

fn main() {
    let stem = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "semantic-controls".into());
    for (name, mode) in [("dark", Mode::Dark), ("light", Mode::Light)] {
        let mut ui = Ui::default().font(Font::new(ttf_inter::REGULAR).unwrap());
        let mut theme = Theme::default();
        theme.palette.mode = mode;
        ui.set_theme(theme);
        for frame in 0..64 {
            let el = panel(&mut ui);
            ui.frame(el, Some(SIZE), Input::default(), 1.0 / 60.0)
                .unwrap();
            if frame == 0 {
                ui.focus("save");
            }
        }
        let mut ctx = RenderContext::new(480, 340);
        let mut resources = Resources::default();
        mui::vello::paint(
            &mut mui::vello::Cpu {
                ctx: &mut ctx,
                resources: &mut resources,
                cache: &mut mui::vello::Cache::default(),
            },
            ui.scene().unwrap(),
            mui::vello::kurbo::Affine::IDENTITY,
        )
        .unwrap();
        ctx.flush();
        let mut pix = Pixmap::new(480, 340);
        ctx.render(&mut pix, &mut resources);
        let rgba: Vec<u8> = pix
            .take_unpremultiplied()
            .iter()
            .flat_map(|p| [p.r, p.g, p.b, p.a])
            .collect();
        let path = format!("{stem}-{name}.png");
        let file = std::fs::File::create(&path).unwrap();
        let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), 480, 340);
        encoder.set_color(png::ColorType::Rgba);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&rgba)
            .unwrap();
        println!("wrote {path}");
    }
}
