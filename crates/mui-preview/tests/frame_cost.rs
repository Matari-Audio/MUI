//! A frame stays inside a generous budget. The budget is a tripwire, not a
//! benchmark: it catches the accidental O(n^2), not a 10% regression. Real
//! numbers, and the hybrid/cpu/classic comparison, live in BENCHMARKS.md and
//! `cargo run -p mui-vello --release --features cpu --example bench`.
//!
//! ```text
//! cargo test -p mui-preview --test frame_cost -- --nocapture
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
    fn fill_blurred_rounded_rect(&mut self, _: &mui::vello::kurbo::Rect, _: f32, _: f32, _: bool) {}
    fn push_clip(&mut self, _: &mui::vello::kurbo::BezPath) {}
    fn pop_clip(&mut self) {}
    fn push_layer(&mut self, _: mui::vello::peniko::BlendMode, _: f32) {}
    fn pop_layer(&mut self) {}
    fn glyphs(&mut self, _: &mui::scene::Text) {}
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

/// Generous enough that a debug build on a slow machine passes, tight enough
/// that a quadratic walk over 500 paint ops does not.
const BUDGET_MS: f64 = 50.0;

#[test]
fn what_a_frame_costs() {
    let font = epaint_default_fonts::HACK_REGULAR;
    let spec = SceneSpec::new(pill()).font(font.to_vec());
    let resolve = ms(|| {
        black_box(resolve_scene(black_box(&spec)).unwrap());
    });
    println!("pill resolve (weld + shell + text) {resolve:8.3} ms");
    let mut ui = Ui::new(Theme::DEFAULT).font(font.to_vec());
    let mut v = 0.3;
    let frame = ms(|| {
        let root = column([
            slider(&mut ui, "a", "A", &mut v, 0.0..=1.0).el(),
            knob(&mut ui, "k", "K", &mut v, 0.0..=1.0).el(),
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
    });
    println!("Ui::frame, sliders + knob         {frame:8.3} ms");
    let resolved = resolve_scene(&spec).unwrap();
    // `paint_cached` is what mui-preview calls, so it is what is timed: the
    // first pass converts, the rest reuse.
    let mut cache = mui::vello::PathCache::new();
    let walk = ms(|| {
        mui::vello::paint_cached(
            &mut Sink,
            &resolved,
            mui::vello::kurbo::Affine::IDENTITY,
            &mut cache,
        )
        .unwrap();
    });
    println!("mui paint walk (cached)           {walk:8.3} ms");
    for (what, got) in [("resolve", resolve), ("frame", frame), ("paint walk", walk)] {
        assert!(
            got < BUDGET_MS,
            "{what} took {got:.3} ms, budget {BUDGET_MS}"
        );
    }
}
