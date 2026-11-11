//! `cargo run -p mui-gain-plugin --example headless -- --snap gain.png`
//! (see `--help`): the editor with no window and no GPU.
use std::sync::Arc;

use mui_gain_plugin::{GainParams, SIZE, ui, view};
use mui_headless::Scene;

fn main() {
    let mut bridge = mui_truce::Bridge::new(Arc::new(GainParams::new()));
    let scene = Scene::new("gain", SIZE, ui(), move |ui| view(ui, &mut bridge));
    std::process::exit(mui_headless::main(vec![scene]));
}
