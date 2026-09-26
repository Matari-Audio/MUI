//! Motion, in pictures: one state change, rendered on the CPU at a handful
//! of moments after it, stacked into one filmstrip.
//!
//!     cargo run -p mui --features cpu --example motion_strip -- /tmp/motion.png
//!
//! Row by row: the panel's frame glides wider, the toggle knob slides, the
//! transport morphs play into pause, a toast slides and fades in while
//! another fades out where it stood, the rack rotates by identity, and five
//! dots play staggered keyframes.

use mui::prelude::*;
use mui::vello::vello_cpu::{Pixmap, RenderContext, Resources};

const SIZE: Size = Size {
    width: 420.0,
    height: 250.0,
};
const SCALE: f64 = 1.5;

#[derive(Default)]
struct State {
    after: bool,
    bypass: bool,
}

fn tree(ui: &mut Ui, s: &mut State) -> El {
    let after = s.after;
    s.bypass = after;
    let sw = toggle(ui, "bypass", "Bypass", &mut s.bypass).el;
    let transport = block(28.0, 28.0)
        .outline(move |sz| {
            let p = |x: f64, y: f64| Point::new(x * sz.width, y * sz.height);
            if after {
                let bar = |x0: f64| {
                    Path::polyline(
                        [p(x0, 0.), p(x0 + 0.3, 0.), p(x0 + 0.3, 1.), p(x0, 1.)],
                        true,
                    )
                };
                Path {
                    commands: [bar(0.1).commands, bar(0.6).commands].concat(),
                }
            } else {
                Path::polyline([p(0.15, 0.), p(1., 0.5), p(0.15, 1.)], true)
            }
        })
        .fill(Role::Primary)
        .morph(if after { "pause" } else { "play" })
        .id("transport");
    let toast = |on: bool, id: &str, label: &str| {
        if on {
            text(label.to_owned())
                .pad(S)
                .radius(8.0)
                .fill(Role::Primary)
                .appear(Appear::Slide(0.0, 14.0))
                .id(id)
        } else {
            block(0.0, 0.0)
        }
    };
    let order: [u64; 3] = if after { [2, 3, 1] } else { [1, 2, 3] };
    let rack = row(order.iter().enumerate().map(|(slot, &m)| {
        text(format!("M{m}"))
            .pad(S)
            .radius(6.0)
            .fill(if m == 1 { Role::Primary } else { Role::Raised })
            .animate_layout()
            .identity(m)
            .id(format!("slot/{slot}"))
    }))
    .gap(S);
    let dots = row((0..5).map(|i| {
        let k = Keys::new(0.2)
            .to(0.3, 1.0, Ease::EMPHASIZED)
            .to(0.8, 0.2, Ease::IN_OUT)
            .delay(f64::from(i) * 0.06);
        let v = if after {
            ui.play(&format!("dot-{i}"), &k)
        } else {
            0.2
        };
        block(12.0, 12.0)
            .pill()
            .fill(Role::Primary)
            .opacity(v as f32)
    }))
    .gap(S);
    let card = col([
        row([
            block(if after { 220.0 } else { 90.0 }, 30.0)
                .radius(15.0)
                .fill(Role::Raised)
                .animate_layout()
                .id("panel"),
            spacer(),
            sw.el(),
            transport,
        ])
        .gap(M)
        .align(Align::Center),
        row([
            toast(!after, "saved", "Saved"),
            toast(after, "loaded", "Preset loaded"),
        ])
        .gap(M)
        .h(30.0),
        rack,
        dots,
    ])
    .gap(M)
    .pad(L)
    .radius(18.0)
    .fill(Role::Surface)
    .w(SIZE.width - 20.0);
    stack([card.centered()]).fill(Role::Background)
}

fn main() {
    let out = std::env::args().nth(1).unwrap_or("motion.png".into());
    let (w, h) = (
        (SIZE.width * SCALE).round() as u16,
        (SIZE.height * SCALE).round() as u16,
    );
    let mut ui =
        Ui::new(Theme::DEFAULT).font(Font::new(epaint_default_fonts::HACK_REGULAR).unwrap());
    ui.set_scale(Some(SCALE));
    let mut s = State::default();
    // Frames after the change to keep: its first frame, then 50, 120, 250
    // and 600 ms on.
    let keep = [0usize, 3, 7, 15, 36];
    let mut strip: Vec<u8> = Vec::new();
    for _ in 0..10 {
        let root = tree(&mut ui, &mut s);
        ui.frame(root, Some(SIZE), PointerInput::default(), 1.0 / 60.0)
            .expect("frame");
    }
    s.after = true;
    for f in 0..=*keep.last().unwrap() {
        let root = tree(&mut ui, &mut s);
        let frame = ui
            .frame(root, Some(SIZE), PointerInput::default(), 1.0 / 60.0)
            .expect("frame");
        if !keep.contains(&f) {
            continue;
        }
        let mut ctx = RenderContext::new(w, h);
        let mut res = Resources::default();
        mui::vello::paint(
            &mut mui::vello::Cpu {
                ctx: &mut ctx,
                resources: &mut res,
                cache: &mut mui::vello::Cache::default(),
            },
            frame.scene,
            mui::vello::kurbo::Affine::scale(SCALE),
        )
        .expect("paint");
        ctx.flush();
        let mut pix = Pixmap::new(w, h);
        ctx.render(&mut pix, &mut res);
        strip.extend(
            pix.take_unpremultiplied()
                .iter()
                .flat_map(|p| [p.r, p.g, p.b, p.a]),
        );
    }
    let file = std::fs::File::create(&out).expect("create");
    let mut enc = png::Encoder::new(
        std::io::BufWriter::new(file),
        w.into(),
        u32::from(h) * keep.len() as u32,
    );
    enc.set_color(png::ColorType::Rgba);
    enc.write_header()
        .and_then(|mut e| e.write_image_data(&strip))
        .expect("png");
    println!("wrote {out}: {} frames of {w}x{h}", keep.len());
}
