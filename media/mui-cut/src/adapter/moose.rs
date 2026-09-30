//! A moose plugin: `crate::Plugin` (its `plugin!` macro makes it) opens its
//! editor as a host would, on a headless window, and its real DSP renders
//! the host's notes on the bridge's sample clock. Editor and DSP share the
//! parameters, meters and transport, so the capture shows what it plays.
//!
//! `--channel N` (0..15, default 0): the MIDI channel notes arrive on.
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
    let args: Vec<String> = std::env::args().collect();
    let channel: u8 = args
        .iter()
        .position(|a| a == "--channel")
        .and_then(|i| args.get(i + 1))
        .map_or(Ok(0), |c| c.parse().ok().filter(|c| *c < 16).ok_or("--channel is 0..15"))?;
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

    // The patch: every parameter off its default, and the notes held.
    let patched = all.clone();
    let patch = move || {
        let params: Vec<_> = patched
            .param_infos()
            .into_iter()
            .filter_map(|i| {
                let v = patched.get_plain(i.id)?;
                ((v - i.default_plain).abs() > 1e-6).then(|| {
                    serde_json::json!({
                        "id": i.id, "name": i.name, "group": i.group, "value": v,
                        "text": patched.format_value(i.id, v).unwrap_or_default(),
                        "norm": patched.get_normalized(i.id).unwrap_or_default(),
                    })
                })
            })
            .take(96)
            .collect();
        serde_json::json!({"plugin": name, "params": params, "routes": [], "held": held.notes()})
    };

    let infos = all.param_infos();
    let edit = move |c: &serde_json::Value| {
        let s = mui_motion_bridge::param_set(c, |n| {
            infos.iter().find(|i| i.name == n || i.short_name == n).map(|i| i.id)
        })?;
        let info = infos.iter().find(|i| i.id == s.id).ok_or("no such parameter")?;
        all.set_normalized(s.id, if s.norm { s.value } else { info.range.normalize(s.value) });
        Ok(())
    };
    let mut describe = mui_motion_bridge::describe(name);
    describe["notes"] = true.into();
    describe["patch"] = true.into();
    let served = mui_motion_bridge::run_headless_with(describe, view, edit, audio, patch);
    drop(editor);
    served
}
