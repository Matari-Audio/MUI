//! Run `cargo run -p mui --example material_welding`. This resolves the real
//! scene pipeline and checks cache reuse. It intentionally opens no window.
use mui::prelude::*;

fn panel(progress: f64) -> El {
    let a = col![text("Oscillator")]
        .pad(L)
        .size(130., 80.)
        .fill(Gradient::vertical(Role::Primary, Role::Raised))
        .stroke(Role::Ink).stroke_width(2.)
        .radius(18.)
        .id("a");
    let b = col![text("Filter")]
        .pad(L)
        .size(110., 80.)
        .fill(Role::Secondary)
        .stroke(Role::Warning).stroke_width(7.)
        .radius(26.)
        .id("b");
    row![a, b]
        .gap(10.)
        .weld(Weld::all().reach(20.).blend(70.).morph(progress))
        .id("rack")
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut ui = Ui::new(Theme::DEFAULT);
    for _ in 0..2 {
        ui.frame(panel(1.), None, Input::default(), 1. / 60.)?;
    }
    let (hits, misses, bytes) = ui.weld_cache_stats();
    assert!(hits >= 1 && misses == 1);
    println!("material welding: {hits} cache hits, {misses} bake, {bytes} retained bytes");
    Ok(())
}
