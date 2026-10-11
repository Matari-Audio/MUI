//! A truce plugin: `crate::Plugin` (its `plugin!` macro makes it) opens its
//! editor as a host would, on a headless window, and its real DSP renders
//! the host's notes on the bridge's sample clock. Editor and DSP share the
//! parameters, meters and transport, so the capture shows what it plays.
//! Notes arrive on MIDI channel 1 (0 on the wire).
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use FRAMEWORK::core::buffer::AudioBuffer;
use FRAMEWORK::core::config::AudioConfig;
use FRAMEWORK::core::editor::{ClosureBridge, PluginContext, RawWindowHandle};
use FRAMEWORK::core::events::{Event, EventBody, EventList, TransportInfo};
use FRAMEWORK::core::export::PluginExport;
use FRAMEWORK::core::plugin::PluginRuntime;
use FRAMEWORK::core::process::ProcessContext;
use FRAMEWORK::core::sample::Float;
use FRAMEWORK::core::TransportSlot;
use FRAMEWORK::params::Params;
use mui_motion_bridge::NoteEvent;

type S = <plugin::ENTRY as PluginRuntime>::Sample;

fn main() {
    if let Err(e) = run() {
        eprintln!("mui-cut adapter: {e}");
        std::process::exit(1);
    }
}

/// The notes the DSP holds, a bit each.
#[derive(Default)]
struct Held([AtomicU64; 2]);

impl Held {
    fn set(&self, note: u8, on: bool) {
        let (word, bit) = (&self.0[usize::from(note / 64 % 2)], 1u64 << (note % 64));
        if on {
            word.fetch_or(bit, Ordering::Relaxed);
        } else {
            word.fetch_and(!bit, Ordering::Relaxed);
        }
    }
    fn notes(&self) -> Vec<u8> {
        (0..128u8).filter(|&n| self.0[usize::from(n / 64)].load(Ordering::Relaxed) >> (n % 64) & 1 == 1).collect()
    }
}

fn run() -> Result<(), String> {
    let channel = 0;
    let name = <plugin::ENTRY as PluginRuntime>::info().name;
    mui_motion_bridge::mui::host::headless::claim();
    let mut plugin = <plugin::ENTRY as PluginExport>::create();
    let params = plugin.params_arc();
    let meters = plugin.meter_store();
    let slot = TransportSlot::new();
    let mut editor = plugin.editor_builder()(Arc::clone(&params)).ok_or("NAME has no editor")?;
    let all: Arc<dyn Params> = params;
    let (set, get, plain, text) = (all.clone(), all.clone(), all.clone(), all.clone());
    let transport = Arc::clone(&slot);
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
        transport: Box::new(move || transport.read()),
    };
    editor.open(RawWindowHandle::X11(0), PluginContext::from_closures(bridge, all.clone()));
    let view = mui_motion_bridge::mui::host::headless::take().ok_or("NAME's editor opened no MUI window")?;

    // The DSP, as a host runs it: reset at the bridge's rate, then a block
    // per `advance` span with its notes at the block's start.
    let rate = f64::from(mui_motion_bridge::sample_rate());
    let block = mui_motion_bridge::BLOCK_FRAMES;
    plugin.init();
    plugin.reset(&AudioConfig::new(rate, block));
    // Parameters the host has set: automated, so the patch lists them.
    let automated = Arc::new(std::sync::Mutex::new(std::collections::BTreeSet::<u32>::new()));
    let patch_automated = Arc::clone(&automated);
    let held = Arc::new(Held::default());
    let dsp_held = Arc::clone(&held);
    let (mut left, mut right) = (vec![S::default(); block], vec![S::default(); block]);
    let mut events = EventList::with_capacity(256);
    let mut out_events = EventList::with_capacity(64);
    let mut position = 0i64;
    let audio = move |notes: &[NoteEvent], samples: &mut [[f32; 2]]| {
        events.clear();
        let mut note = |note, on, velocity| {
            dsp_held.set(note, on);
            let body = if on {
                EventBody::NoteOn { group: 0, channel, note, velocity }
            } else {
                EventBody::NoteOff { group: 0, channel, note, velocity: 0 }
            };
            events.push(Event::new(0, body));
        };
        for n in notes {
            match *n {
                NoteEvent::On { note: k, velocity } => note(k, true, velocity),
                NoteEvent::Off { note: k } => note(k, false, 0),
                NoteEvent::Panic => (0..128).for_each(|k| note(k, false, 0)),
            }
        }
        for chunk in samples.chunks_mut(block) {
            let n = chunk.len();
            let (l, r) = (&mut left[..n], &mut right[..n]);
            l.fill(S::default());
            r.fill(S::default());
            let seconds = position as f64 / rate;
            let info = TransportInfo {
                playing: true,
                tempo: 120.,
                time_sig_num: 4,
                time_sig_den: 4,
                position_samples: position,
                position_seconds: seconds,
                position_beats: seconds * 2.,
                ..TransportInfo::default()
            };
            slot.write(&info);
            position += n as i64;
            out_events.clear();
            let inputs: [&[S]; 0] = [];
            let mut outputs: [&mut [S]; 2] = [l, r];
            let mut buffer = AudioBuffer::from_slices_checked(&inputs, &mut outputs, n);
            let mut ctx = ProcessContext::new(&info, rate, n, &mut out_events);
            let _ = plugin.process(&mut buffer, &events, &mut ctx);
            // The notes land at the span's start: once.
            events.clear();
            for (i, s) in chunk.iter_mut().enumerate() {
                *s = [left[i].to_f32(), right[i].to_f32()];
            }
        }
    };


    // The patch: every parameter off its default, automated or modulated,
    // the modulation routes its `<X> Source` / `<X> Target` /
    // `<X> Amount` parameters hold, and the notes held.
    let patched = all.clone();
    let patch = move || {
        let infos = patched.param_infos();
        let text = |id, v| patched.format_value(id, v).unwrap_or_default();
        let value = |i: &FRAMEWORK::params::ParamInfo| patched.get_plain(i.id).unwrap_or(i.default_plain);
        let find = |n: String| infos.iter().find(|i| i.name == n);
        let mut routes = Vec::new();
        for src in infos.iter().filter(|i| i.name.ends_with(" Source")) {
            let stem = &src.name[..src.name.len() - " Source".len()];
            let (Some(target), Some(amount)) = (find(format!("{stem} Target")), find(format!("{stem} Amount"))) else {
                continue;
            };
            let depth = value(amount);
            if (value(src) - src.default_plain).abs() < 1e-6 || depth.abs() < 1e-6 {
                continue;
            }
            routes.push(serde_json::json!({
                "source": text(src.id, value(src)), "target": text(target.id, value(target)),
                "depth": depth, "ids": [src.id, target.id, amount.id],
            }));
        }
        let modulated: Vec<String> = routes.iter().filter_map(|r| r["target"].as_str().map(str::to_owned)).collect();
        let route_ids: Vec<u64> = routes.iter().flat_map(|r| r["ids"].as_array().cloned().unwrap_or_default()).filter_map(|v| v.as_u64()).collect();
        let automated = patch_automated.lock().map(|a| a.clone()).unwrap_or_default();
        let params: Vec<_> = infos
            .iter()
            .filter(|i| !route_ids.contains(&u64::from(i.id)))
            .filter_map(|i| {
                let v = patched.get_plain(i.id)?;
                let (name, group) = (i.name, i.group);
                let off = (v - i.default_plain).abs() > 1e-6;
                let auto = automated.contains(&i.id);
                let modded = modulated.iter().any(|t| *t == name);
                (off || auto || modded).then(|| {
                    serde_json::json!({
                        "id": i.id, "name": name, "group": group, "value": v,
                        "text": text(i.id, v),
                        "norm": patched.get_normalized(i.id).unwrap_or_default(),
                        "automated": auto, "modulated": modded,
                    })
                })
            })
            .take(96)
            .collect();
        let routes: Vec<_> = routes
            .into_iter()
            .map(|mut r| {
                r.as_object_mut().map(|o| o.remove("ids"));
                r
            })
            .collect();
        serde_json::json!({"plugin": name, "params": params, "routes": routes, "held": held.notes()})
    };

    let infos = all.param_infos();
    let edit = move |c: &serde_json::Value| {
        let s = mui_motion_bridge::param_set(c, |n| {
            infos.iter().find(|i| i.name == n || i.short_name == n).map(|i| i.id)
        })?;
        let info = infos.iter().find(|i| i.id == s.id).ok_or("no such parameter")?;
        all.set_normalized(s.id, if s.norm { s.value } else { info.range.normalize(s.value) });
        if let Ok(mut a) = automated.lock() {
            a.insert(s.id);
        }
        Ok(())
    };
    let mut describe = mui_motion_bridge::describe(name);
    describe["notes"] = true.into();
    describe["patch"] = true.into();
    let served = mui_motion_bridge::run_headless_with(describe, view, edit, audio, patch);
    drop(editor);
    served
}
