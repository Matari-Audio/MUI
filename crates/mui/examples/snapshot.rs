//! Every widget, rendered on the CPU with no window and no GPU.
//!
//!     cargo run -p mui --features cpu --example snapshot -- /tmp/widgets.png

use mui::prelude::*;
use mui::vello::vello_cpu::{Pixmap, RenderContext, Resources};

fn main() {
    let out = std::env::args().nth(1).unwrap_or("widgets.png".into());
    let (w, h) = (360u16, 300u16);
    let mut ui = Ui::new(Theme::DEFAULT).font(epaint_default_fonts::HACK_REGULAR.to_vec());
    let (mut cutoff, mut res, mut gain, mut on) = (0.35, 0.7, -6.0, true);
    // Two frames: the first has no gesture state, the second is what a real
    // host draws every frame.
    for _ in 0..2 {
        let (go, _) = button(&ui, "go", "Trigger");
        let card = column([
            row([
                knob(&mut ui, "cutoff", "Cutoff", &mut cutoff, 0.0..=1.0).el(),
                knob(&mut ui, "res", "Res", &mut res, 0.0..=1.0).el(),
            ])
            .gap(L)
            .justify(Justify::Center),
            slider(&mut ui, "gain", "Gain", &mut gain, -24.0..=6.0).el(),
            row([
                text("Bypass").fill(Role::Dim),
                toggle(&ui, "bypass", &mut on).el(),
                spacer(),
                go.el(),
            ])
            .gap(S)
            .align(Align::Center),
        ])
        .gap(M)
        .pad(L)
        .radius(20.0)
        .fill(Role::Surface)
        .shadow(Shadow::soft(16.0))
        .anchor(Align::Center, Align::Center);
        let root = overlay([card]).fill(Role::Background);
        let frame = ui
            .frame(
                root,
                Some(Size::new(w.into(), h.into())),
                PointerInput::default(),
                1.0 / 60.0,
            )
            .expect("frame");
        let mut ctx = RenderContext::new(w, h);
        let mut res = Resources::default();
        mui::vello::paint(
            &mut mui::vello::Cpu {
                ctx: &mut ctx,
                resources: &mut res,
            },
            frame.scene,
            mui::vello::kurbo::Affine::IDENTITY,
        )
        .expect("paint");
        ctx.flush();
        let mut pix = Pixmap::new(w, h);
        ctx.render(&mut pix, &mut res);
        let rgba = pix.take_unpremultiplied();
        let bytes: Vec<u8> = rgba.iter().flat_map(|p| [p.r, p.g, p.b, p.a]).collect();
        let file = std::fs::File::create(&out).expect("create");
        let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w.into(), h.into());
        enc.set_color(png::ColorType::Rgba);
        enc.write_header()
            .and_then(|mut e| e.write_image_data(&bytes))
            .expect("png");
    }
    println!("wrote {out}");
}
