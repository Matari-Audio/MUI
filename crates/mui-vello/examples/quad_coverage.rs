//! How much of the synth-editor scene the quad pass draws today, per reason
//! it cannot, as [`mui_vello::quad::classify`] reports it.
//!
//!     cargo run -p mui-vello --features gpu-quad --example quad_coverage
//!
//! The same scene `bench` renders, resolved once at 1280x800.

use mui::Ui;
use mui::prelude::*;
use mui_vello::quad::{Reason, classify};

mod bench_scene;

fn main() {
    let mut ui = Ui::default().font(Font::new(epaint_default_fonts::HACK_REGULAR).unwrap());
    let mut app = bench_scene::App::new();
    let root = bench_scene::editor(&mut ui, &mut app);
    let frame = ui
        .frame(
            root,
            Some(Size::new(1280., 800.)),
            PointerInput::default(),
            0.016,
        )
        .expect("scene resolves");
    let paint = &frame.scene.paint;
    let coverage = classify(paint);
    let total = paint.len();
    let pct = |n: usize| 100. * n as f64 / total as f64;
    println!("paint entries: {total}");
    println!(
        "supported:     {:5} ({:.1}%)",
        coverage.supported,
        pct(coverage.supported)
    );
    for reason in Reason::ALL {
        let n = coverage.unsupported(reason);
        if n > 0 {
            println!("{:<14} {n:5} ({:.1}%)", format!("{reason}:"), pct(n));
        }
    }
    // Which layers the path-shaped entries are, and how many share a key:
    // what a path-mask stage has to pick up.
    let mut layers = std::collections::BTreeMap::<String, usize>::new();
    for p in paint.iter().filter(|p| {
        p.rect.is_none() && p.text.is_none() && !matches!(p.layer, mui_scene::Layer::Unclip)
    }) {
        let what = format!("{:?} width>0={}", p.layer, p.width > 0.);
        *layers.entry(what).or_default() += 1;
    }
    println!("without an analytic rect, by layer:");
    for (what, n) in layers {
        println!("  {what:<28} {n:5}");
    }
}
