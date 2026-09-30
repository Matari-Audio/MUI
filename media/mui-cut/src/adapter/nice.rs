//! A nice-plug plugin: its type, made with `Default`, spawns its editor
//! as a host would, on a headless window.
use std::sync::Arc;

use nice_plug::prelude::*;

/// The host side of the editor: parameter gestures go straight to the
/// parameters.
struct Host;

impl GuiContext for Host {
    fn plugin_api(&self) -> PluginApi {
        PluginApi::Clap
    }
    fn request_resize(&self) -> bool {
        false
    }
    unsafe fn raw_begin_set_parameter(&self, _: ParamPtr) {}
    unsafe fn raw_set_parameter_normalized(&self, param: ParamPtr, normalized: f32) {
        // SAFETY: the pointer came from the plugin's own live parameters.
        unsafe { param._internal_set_normalized_value(normalized) };
    }
    unsafe fn raw_end_set_parameter(&self, _: ParamPtr) {}
    fn get_state(&self) -> PluginState {
        PluginState { version: String::new(), params: Default::default(), fields: Default::default() }
    }
    fn set_state(&self, _: PluginState) {}
}

fn main() {
    if let Err(e) = run() {
        eprintln!("mui-cut adapter: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    mui::host::headless::claim();
    let mut plugin = plugin::ENTRY::default();
    let params = plugin.params().param_map();
    let executor = AsyncExecutor::new(Arc::new(|_| {}), Arc::new(|_| {}));
    let editor = plugin.editor(executor).ok_or("NAME has no editor")?;
    let window = editor.spawn(ParentWindowHandle::X11Window(0), Arc::new(Host));
    let view = mui::host::headless::take().ok_or("NAME's editor opened no MUI window")?;
    let edit = move |c: &serde_json::Value| {
        // nice-plug parameters have string ids: numbers index them.
        let s = mui_motion_bridge::param_set(c, |n| {
            params
                .iter()
                // SAFETY: the plugin, and so its parameters, outlive this.
                .position(|(id, p, _)| id == n || unsafe { p.name() } == n)
                .and_then(|i| u32::try_from(i).ok())
        })?;
        let (_, p, _) = params.get(s.id as usize).ok_or("no such parameter")?;
        // SAFETY: as above.
        unsafe {
            let norm = if s.norm { s.value as f32 } else { p.preview_normalized(s.value as f32) };
            p._internal_set_normalized_value(norm);
        }
        Ok(())
    };
    let served = mui_motion_bridge::run_headless(mui_motion_bridge::describe("NAME"), view, edit);
    drop(window);
    drop(plugin);
    served
}
