//! A plugin's editor in this process. `mui-cut render` and `still` build
//! the plugin's adapter with the bridge's `cut` feature and run it with
//! [`ENV`] set; the adapter hands its headless editor and DSP to [`run`],
//! which runs the same command here. Plugin layers on that source then
//! draw straight from the editor's paint, as vectors at whatever size the
//! camera needs: no second process, no pixels in between. Time is still
//! the sample clock's alone, so a frame is the same however it is reached.
use std::cell::RefCell;
use std::sync::Arc;

use mui_scene::ResolvedScene;
use serde_json::Value;

use crate::plugin::Source;

/// Set (to the source, as JSON) when an adapter should run mui-cut.
pub const ENV: &str = "MUI_CUT_INPROC";
/// The mui-cut command line to run, as a JSON array: the adapter's own
/// arguments stay its own.
pub const ARGV: &str = "MUI_CUT_INPROC_ARGV";

/// The plugin, as the bridge's `Live` runs it.
pub trait LivePlugin {
    /// `{"op": "advance", "to": sample, "notes": [..]}`: the sound up to
    /// `to`, stereo interleaved.
    fn advance(&mut self, command: &Value) -> Result<Vec<f32>, String>;
    /// Any other command a replay step sends (`set`, `input`, notes).
    fn command(&mut self, command: &Value) -> Result<(), String>;
    /// The editor now: its capture manifest, and the paint of each
    /// fragment it names (`src`), moved to its rect's origin.
    #[expect(clippy::type_complexity, reason = "the manifest and its scenes")]
    fn frame(&mut self) -> Result<(Value, Vec<(String, Arc<ResolvedScene>)>), String>;
}

thread_local! {
    static LIVE: RefCell<Option<(Source, Box<dyn LivePlugin>)>> = const { RefCell::new(None) };
}

/// Run the command line in [`ARGV`] with `plugin` as the source in
/// [`ENV`].
pub fn run(plugin: Box<dyn LivePlugin>) -> Result<(), String> {
    let var = |k: &str| std::env::var(k).map_err(|_| format!("{k} is not set"));
    let source: Source = serde_json::from_str(&var(ENV)?).map_err(|e| format!("{ENV}: {e}"))?;
    let argv: Vec<String> =
        serde_json::from_str(&var(ARGV)?).map_err(|e| format!("{ARGV}: {e}"))?;
    LIVE.with(|l| *l.borrow_mut() = Some((source, plugin)));
    crate::cli::run(&argv)
}

/// Whether this process hosts `source`'s editor.
pub fn hosts(source: &Source) -> bool {
    LIVE.with(|l| l.borrow().as_ref().is_some_and(|(s, _)| s == source))
}

/// Whether this process is an adapter running mui-cut.
pub fn here() -> bool {
    LIVE.with(|l| l.borrow().is_some())
}

/// `f` with the plugin hosted here for `source`, if it is.
pub fn with<R>(source: &Source, f: impl FnOnce(&mut dyn LivePlugin) -> R) -> Option<R> {
    LIVE.with(|l| match &mut *l.borrow_mut() {
        Some((s, p)) if s == source => Some(f(p.as_mut())),
        _ => None,
    })
}
