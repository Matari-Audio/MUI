//! Where a frame's time goes, measured rather than guessed. `#[ignore]`
//! because it is a measurement, not an assertion.
//!
//! ```text
//! cargo test -p mui-preview --release --test frame_cost -- --ignored --nocapture
//! ```
use std::hint::black_box;
use std::time::Instant;

use mui::prelude::*;

/// A canvas that draws nothing, so the number below is MUI's half of painting
/// -- the paint-list walk and the arc-to-cubic conversion -- with Vello's
/// rasteriser left out. A real `Gpu` canvas needs a device, and this test runs
/// without one.
struct Sink;
impl mui::vello::Canvas for Sink {
    fn set_transform(&mut self, _: mui::vello::kurbo::Affine) {}
    fn set_paint(&mut self, _: mui::vello::PaintType) {}
    fn set_stroke(&mut self, _: mui::vello::kurbo::Stroke) {}
    fn fill_path(&mut self, p: &mui::vello::kurbo::BezPath) {
        black_box(p);
    }
    fn stroke_path(&mut self, p: &mui::vello::kurbo::BezPath) {
        black_box(p);
    }
    fn fill_blurred_rounded_rect(&mut self, _: &mui::vello::kurbo::Rect, _: f32, _: f32) {}
    fn push_clip(&mut self, _: &mui::vello::kurbo::BezPath) {}
    fn pop_clip(&mut self) {}
    fn glyphs(&mut self, _: &std::sync::Arc<Vec<u8>>, _: f32, _: (f64, f64), _: &[(u32, f32)]) {}
}

fn ms(mut f: impl FnMut()) -> f64 {
    let n = 50;
    let t = Instant::now();
    for _ in 0..n {
        f();
    }
    t.elapsed().as_secs_f64() * 1000. / n as f64
}

fn pill() -> El {
    let control = |id: &str| leaf(28.0, 28.0).pill().fill(Role::Primary).id(id);
    let tab = column([control("plus"), control("phase"), control("warp")])
        .gap(10.0)
        .pad(22.0)
        .min_width(92.0)
        .id("tab")
        .shell(12.0, Role::Raised);
    column([
        tab,
        row([text("welded")]).size(520.0, 230.0).pad(L).id("panel"),
    ])
    .align(Align::Start)
    .weld(Role::Surface)
}

#[test]
#[ignore = "a measurement"]
fn what_a_frame_costs() {
    let font = epaint_default_fonts::HACK_REGULAR;
    let spec = SceneSpec::new(pill()).font(font.to_vec());
    println!(
        "pill resolve (weld + shell + text) {:8.3} ms",
        ms(|| {
            black_box(resolve_scene(black_box(&spec)).unwrap());
        })
    );
    let mut ui = Ui::new(Theme::DEFAULT).font(font.to_vec());
    let mut v = 0.3;
    println!(
        "Ui::frame, sliders + knob         {:8.3} ms",
        ms(|| {
            let root = column([
                slider(&mut ui, "a", "A", &mut v, 0.0..=1.0),
                knob(&mut ui, "k", "K", &mut v, 0.0..=1.0, 64.0),
            ])
            .pad(M);
            black_box(
                ui.frame(
                    root,
                    Some(Size::new(300.0, 300.0)),
                    PointerInput::default(),
                    0.016,
                )
                .unwrap()
                .scene
                .paint
                .len(),
            );
        })
    );
    let resolved = resolve_scene(&spec).unwrap();
    println!(
        "mui paint walk 1600x1000          {:8.3} ms",
        ms(|| {
            mui::vello::paint(&mut Sink, &resolved, mui::vello::kurbo::Affine::IDENTITY).unwrap();
        })
    );
}
