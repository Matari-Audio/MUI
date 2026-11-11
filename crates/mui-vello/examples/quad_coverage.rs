//! How much of each scene the quad pass draws, per reason it cannot, as
//! [`mui_vello::quad::classify`] reports it: the synth editor `bench`
//! renders, the `headless` example's scene, and every mui-preview gallery
//! scene's specimen, each resolved once at 1280x800 with no window.
//!
//!     cargo run -p mui-vello --features gpu-quad --example quad_coverage

use mui::Ui;
use mui::prelude::*;
use mui_vello::quad::{Reason, classify};

mod bench_scene;
// The gallery's scenes need only `mui` and `mui-text`; the window is the
// binary's.
#[expect(dead_code, reason = "the gallery's host calls the rest")]
#[path = "../../mui-preview/src/scenes.rs"]
mod scenes;

const SIZE: Size = Size::new(1280., 800.);

fn hack() -> Font {
    Font::new(epaint_default_fonts::HACK_REGULAR).expect("font")
}

fn main() {
    let mut ui = Ui::default().font(hack());
    let mut app = bench_scene::App::new();
    let root = bench_scene::editor(&mut ui, &mut app);
    report("bench editor", &frame(&mut ui, root));
    let headless = headless();
    report("headless", &headless);
    for mut scene in scenes::all() {
        let mut ui = Ui::default().font(hack());
        let root = scene.specimen(&mut ui);
        report(scene.name(), &frame(&mut ui, root));
    }
}

fn frame(ui: &mut Ui, root: El) -> Vec<mui_scene::Painted> {
    ui.frame(root, Some(SIZE), PointerInput::default(), 0.016)
        .expect("scene resolves")
        .scene
        .paint
        .clone()
}

/// The `headless` example's scene: a shell welded to a panel.
fn headless() -> Vec<mui_scene::Painted> {
    use mui_material::prelude::*;
    let control = |id: &str| block(28.0, 28.0).pill().fill(Role::Primary).id(id);
    let tab = col([control("plus"), control("phase"), control("warp")])
        .gap(10.0)
        .pad(22.0)
        .min_w(92.0)
        .align(Align::Center)
        .id("tab")
        .shell(12.0, Role::Raised);
    let panel = row([text("welded").text_size(22.0)])
        .size(520.0, 230.0)
        .pad(L)
        .id("panel");
    let root = col([tab, panel])
        .align(Align::Start)
        .id("root")
        .union(Role::Surface);
    let spec = SceneSpec::new(root).font(hack());
    resolve(&spec).expect("scene resolves").paint
}

fn report(name: &str, paint: &[mui_scene::Painted]) {
    let coverage = classify(paint);
    let refused: Vec<String> = Reason::ALL
        .into_iter()
        .filter_map(|r| {
            let n = coverage.unsupported(r);
            (n > 0).then(|| format!("{r} {n}"))
        })
        .collect();
    println!(
        "{name:<20} {:5} entries {:5} drawn  {}",
        paint.len(),
        coverage.supported,
        if refused.is_empty() {
            "all".into()
        } else {
            refused.join(", ")
        }
    );
}
