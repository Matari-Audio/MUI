//! Headless agent tooling for MUI editors: look at pixels and trees, drive
//! the UI with a script, and trace what moved, with no window and no GPU.
//!
//! An editor hands over its tree-building function as a [`Scene`]; [`main`]
//! gives its binary the command line below. Everything runs on one fixed
//! virtual clock (step `k` is `k / fps` seconds, handed to `Ui::frame` as
//! `dt`) and the CPU renderer, so the same script gives byte-identical PNGs,
//! trees and traces on repeat runs. No wall time is read unless `--cost` is on.
//!
//! ```no_run
//! use mui::prelude::*;
//! use mui_headless::Scene;
//!
//! let mut gain = -6.0;
//! let scene = Scene::new("editor", (300, 200), Ui::default(), move |ui| {
//!     slider(ui, "gain", "Gain", &mut gain, -24.0..=6.0).el.into_el().w(220).pad(L)
//! });
//! std::process::exit(mui_headless::main(vec![scene]));
//! ```
//!
//! ```text
//! app --snap out.png [--scene NAME] [--at 0.4] [--size 300x200] [--scale 2] [--theme dark|light]
//! app --tree tree.json [--at T]
//! app --script s.json [--snap out.png | --tree t.json | --frames N --out dir/ | --strip s.png]
//! app --script s.json --trace run.jsonl
//! app --list-scenes
//! ```
//!
//! See [`script`] for the event format, [`tree`](mod@tree) and [`trace`] for the output formats,
//! and `AGENTS.md` at the repository root for the five-line version.
//!
//! The design follows Rune's agent tooling (anomalyco/rune, MIT, README
//! "Agent tooling"): same flag names, same script event vocabulary, same
//! `mark:` times. The fixed-step input driver is `media/mui-reel`'s.
#![forbid(unsafe_code)]

mod cli;
mod render;
pub mod script;
mod session;
pub mod trace;
pub mod tree;

#[cfg(test)]
mod tests;

use mui::prelude::*;

pub use cli::{USAGE, run, slug, wants};
pub use render::{Raster, contact_sheet, write_png};
pub use script::Script;
/// The JSON type reports and trees are returned as.
pub use serde_json::{self, Value};
pub use session::{Session, Step};

/// Why a run failed. The message says what to change.
#[derive(Debug)]
pub struct Error(String);

impl Error {
    pub(crate) fn new(m: impl Into<String>) -> Self {
        Self(m.into())
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Error {}
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self(format!("io: {e}"))
    }
}
impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Self(format!("json: {e}"))
    }
}
impl From<png::EncodingError> for Error {
    fn from(e: png::EncodingError) -> Self {
        Self(format!("png: {e}"))
    }
}

/// One thing to look at: a name, a logical size, the `Ui` it runs in (its
/// theme and fonts, as the editor would build it) and the function that builds
/// its tree every frame. The closure owns the scene's state.
pub struct Scene {
    pub(crate) name: String,
    pub(crate) size: (u32, u32),
    pub(crate) ui: Ui,
    pub(crate) build: Box<dyn FnMut(&mut Ui) -> El>,
}

impl Scene {
    pub fn new(
        name: impl Into<String>,
        size: (u32, u32),
        ui: Ui,
        build: impl FnMut(&mut Ui) -> El + 'static,
    ) -> Self {
        Self {
            name: name.into(),
            size,
            ui,
            build: Box::new(build),
        }
    }
}

/// Run the command line in `std::env::args` against `scenes`; the exit code.
/// `--help` prints the flags, `--list-scenes` the scene names, anything else
/// runs and prints a JSON report on stdout. Errors go to stderr.
pub fn main(scenes: Vec<Scene>) -> i32 {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print!("{USAGE}");
        return 0;
    }
    match run(scenes, &args) {
        Ok(report) => {
            match report.get("scenes").and_then(|s| s.as_array()) {
                Some(names) => names
                    .iter()
                    .filter_map(|n| n.as_str())
                    .for_each(|n| println!("{n}")),
                None => println!("{report}"),
            }
            0
        }
        Err(e) => {
            eprintln!("error: {e}");
            2
        }
    }
}
