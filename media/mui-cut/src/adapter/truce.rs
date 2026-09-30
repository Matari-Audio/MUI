//! A truce plugin: `crate::Plugin` (its `plugin!` macro makes it) opens its
//! editor as a host would, on a headless window. No sound yet.
use std::sync::Arc;

use FRAMEWORK::core::editor::{ClosureBridge, PluginContext, RawWindowHandle};
use FRAMEWORK::core::export::PluginExport;
use FRAMEWORK::params::Params;

fn main() {
    if let Err(e) = run() {
        eprintln!("mui-cut adapter: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    mui_motion_bridge::mui::host::headless::claim();
    let plugin = <plugin::ENTRY as PluginExport>::create();
    let params = plugin.params_arc();
    let meters = plugin.meter_store();
    let mut editor = plugin.editor_builder()(Arc::clone(&params)).ok_or("NAME has no editor")?;
    let all: Arc<dyn Params> = params;
    let (set, get, plain, text) = (all.clone(), all.clone(), all.clone(), all.clone());
    let bridge = ClosureBridge {
        begin_edit: Box::new(|_| {}),
        set_param: Box::new(move |id, v| set.set_normalized(id, v)),
        end_edit: Box::new(|_| {}),
        request_resize: Box::new(|_, _| false),
        get_param: Box::new(move |id| get.get_normalized(id).unwrap_or_default()),
        get_param_plain: Box::new(move |id| plain.get_plain(id).unwrap_or_default()),
        format_param: Box::new(move |id| {
            let v = text.get_plain(id).unwrap_or_default();
            text.format_value(id, v).unwrap_or_default()
        }),
        get_meter: Box::new(move |id| meters.read(id)),
        get_state: Box::new(Vec::new),
        set_state: Box::new(|_| {}),
        transport: Box::new(|| None),
    };
    editor.open(RawWindowHandle::X11(0), PluginContext::from_closures(bridge, all.clone()));
    let view = mui_motion_bridge::mui::host::headless::take().ok_or("NAME's editor opened no MUI window")?;
    let infos = all.param_infos();
    let edit = move |c: &serde_json::Value| {
        let s = mui_motion_bridge::param_set(c, |n| {
            infos.iter().find(|i| i.name == n || i.short_name == n).map(|i| i.id)
        })?;
        let info = infos.iter().find(|i| i.id == s.id).ok_or("no such parameter")?;
        all.set_normalized(s.id, if s.norm { s.value } else { info.range.normalize(s.value) });
        Ok(())
    };
    let served = mui_motion_bridge::run_headless(mui_motion_bridge::describe("NAME"), view, edit);
    drop(editor);
    served
}
