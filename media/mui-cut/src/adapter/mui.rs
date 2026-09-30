//! A plain MUI crate: `mui_editor()` hands over its editor.
use std::sync::{Arc, Mutex};

fn main() {
    if let Err(e) = run() {
        eprintln!("mui-cut adapter: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let (ui, size, view) = plugin::ENTRY();
    mui::host::headless::claim();
    let shared = Arc::new(Mutex::new(mui::host::Shared { ui, view }));
    mui::host::headless::offer(&shared, size);
    let view = mui::host::headless::take().ok_or("no editor")?;
    let edit = |_: &serde_json::Value| Err("NAME has no parameters to set".to_owned());
    mui_motion_bridge::run_headless(mui_motion_bridge::describe("NAME"), view, edit)
}
