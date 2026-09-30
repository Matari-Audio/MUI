//! A nice-plug plugin: its type, made with `Default`, spawns its editor
//! as a host would, on a headless window, and its `process` renders the
//! host's notes on the bridge's sample clock (MIDI channel 1).
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use nice_plug::context::gui::GuiContext;
use nice_plug::context::process::SendEventError;
use nice_plug::midi::{Channel, Key, VoiceID};
use nice_plug::prelude::*;
use mui_motion_bridge::NoteEvent as Note;

type P = plugin::ENTRY;

/// The host side of the editor: parameter gestures go straight to the
/// parameters.
struct Host;

impl GuiContextInner for Host {
    fn plugin_api(&self) -> PluginApi {
        PluginApi::Clap
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
    fn request_restart(&self) {}
}

/// The host side of `activate` and `process`: a transport, the block's
/// notes, nothing sent back.
struct Ctx {
    transport: Transport,
    events: VecDeque<PluginNoteEvent<P>>,
}

impl ActivateContext<P> for Ctx {
    fn plugin_api(&self) -> PluginApi {
        PluginApi::Clap
    }
    fn execute(&self, _: <P as Plugin>::BackgroundTask) {}
    fn set_latency_samples(&self, _: u32) {}
    fn set_current_voice_capacity(&self, _: u32) {}
}

impl ProcessContext<P> for Ctx {
    fn plugin_api(&self) -> PluginApi {
        PluginApi::Clap
    }
    fn execute_background(&self, _: <P as Plugin>::BackgroundTask) {}
    fn execute_gui(&self, _: <P as Plugin>::BackgroundTask) {}
    fn transport(&self) -> &Transport {
        &self.transport
    }
    fn next_event(&mut self) -> Option<PluginNoteEvent<P>> {
        self.events.pop_front()
    }
    fn try_send_event(&mut self, _: PluginNoteEvent<P>) -> Result<(), (PluginNoteEvent<P>, SendEventError)> {
        Ok(())
    }
    fn set_latency_samples(&self, _: u32) {}
    fn request_restart(&self) {}
    fn set_current_voice_capacity(&self, _: u32) {}
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

fn main() {
    if let Err(e) = run() {
        eprintln!("mui-cut adapter: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    mui_motion_bridge::mui::host::headless::claim();
    let mut plugin = P::default();
    let params = plugin.params().param_map();
    let executor = AsyncExecutor::new(Arc::new(|_| {}), Arc::new(|_| {}));
    let editor = plugin.editor(executor).ok_or("NAME has no editor")?;
    let window = editor
        .spawn(Some(ParentWindowHandle::XlibWindow(0)), false, None, GuiContext::new(Arc::new(Host)), None)
        .map_err(|e| format!("NAME's editor: {e}"))?;
    let view = mui_motion_bridge::mui::host::headless::take().ok_or("NAME's editor opened no MUI window")?;

    // The DSP, as a host runs it: activated at the bridge's rate, then a
    // block per `advance` span with its notes at the block's start.
    let rate = mui_motion_bridge::sample_rate() as f32;
    let block = mui_motion_bridge::BLOCK_FRAMES;
    let layout = P::AUDIO_IO_LAYOUTS.first().copied().unwrap_or_default();
    let channels = layout.main_output_channels.map_or(0, |c| c.get() as usize);
    let config = BufferConfig {
        sample_rate: rate,
        min_buffer_size: None,
        max_buffer_size: block as u32,
        process_mode: ProcessMode::Offline,
    };
    let mut ctx = Ctx { transport: Transport::new(rate), events: VecDeque::new() };
    // SAFETY (each `unsafe` on a parameter pointer below): the plugin, and
    // so its parameters, outlive this function and its closures.
    let smooth = |reset| params.iter().for_each(|(_, p, _)| unsafe { p._internal_update_smoother(rate, reset) });
    smooth(true);
    if !plugin.activate(&layout, &config, &mut ctx) {
        return Err("NAME did not activate".into());
    }
    plugin.reset();
    let held = Arc::new(Held::default());
    let dsp_held = Arc::clone(&held);
    let mut outs = vec![vec![0f32; block]; channels.max(1)];
    let mut position = 0i64;
    let audio = move |notes: &[Note], samples: &mut [[f32; 2]]| {
        let mut note = |key: u8, on: bool, velocity: f32| {
            dsp_held.set(key, on);
            let (voice_id, channel, key) = (VoiceID::Wildcard, Channel::Number(0), Key::Number(key));
            ctx.events.push_back(if on {
                NoteEvent::NoteOn { timing: 0, voice_id, channel, key, velocity }
            } else {
                NoteEvent::NoteOff { timing: 0, voice_id, channel, key, velocity: 0. }
            });
        };
        for n in notes {
            match *n {
                Note::On { note: k, velocity } => note(k, true, f32::from(velocity) / 127.),
                Note::Off { note: k } => note(k, false, 0.),
                Note::Panic => (0..128).for_each(|k| note(k, false, 0.)),
            }
        }
        for chunk in samples.chunks_mut(block) {
            let n = chunk.len();
            let t = &mut ctx.transport;
            t.playing = true;
            t.tempo = Some(120.);
            t.pos_samples = Some(position);
            t.pos_seconds = Some(position as f64 / f64::from(rate));
            position += n as i64;
            let mut buffer = Buffer::default();
            // SAFETY: the slices live until `process` returns.
            unsafe {
                buffer.set_slices(n, |s| {
                    s.clear();
                    s.extend(outs.iter_mut().take(channels).map(|o| {
                        o[..n].fill(0.);
                        &mut o[..n]
                    }));
                });
            }
            let mut aux = AuxiliaryBuffers { inputs: &mut [], outputs: &mut [] };
            let _ = plugin.process(&mut buffer, &mut aux, &mut ctx);
            drop(buffer);
            // The notes land at the span's start: once.
            ctx.events.clear();
            let right = usize::from(channels > 1);
            for (i, s) in chunk.iter_mut().enumerate() {
                *s = if channels == 0 { [0.; 2] } else { [outs[0][i], outs[right][i]] };
            }
        }
    };

    // The patch: the parameters off their defaults or set by the host, and
    // the notes held.
    let automated = Arc::new(std::sync::Mutex::new(std::collections::BTreeSet::<usize>::new()));
    let (patch_automated, patch_params) = (Arc::clone(&automated), params.clone());
    let patch = move || {
        let automated = patch_automated.lock().map(|a| a.clone()).unwrap_or_default();
        let list: Vec<_> = patch_params
            .iter()
            .enumerate()
            .filter_map(|(i, (id, p, group))| unsafe {
                let norm = p.unmodulated_normalized_value();
                let auto = automated.contains(&i);
                let modded = (p.modulated_normalized_value() - norm).abs() > 1e-6;
                ((norm - p.default_normalized_value()).abs() > 1e-6 || auto || modded).then(|| {
                    serde_json::json!({
                        "id": id, "name": p.name(), "group": group,
                        "value": p.unmodulated_plain_value(), "text": p.normalized_value_to_string(norm, true),
                        "norm": norm, "automated": auto, "modulated": modded,
                    })
                })
            })
            .take(96)
            .collect();
        serde_json::json!({"plugin": "NAME", "params": list, "routes": [], "held": held.notes()})
    };

    let edit = move |c: &serde_json::Value| {
        // nice-plug parameters have string ids: numbers index them.
        let s = mui_motion_bridge::param_set(c, |n| {
            params
                .iter()
                .position(|(id, p, _)| id == n || unsafe { p.name() } == n)
                .and_then(|i| u32::try_from(i).ok())
        })?;
        let (_, p, _) = params.get(s.id as usize).ok_or("no such parameter")?;
        unsafe {
            let norm = if s.norm { s.value as f32 } else { p.preview_normalized(s.value as f32) };
            p._internal_set_normalized_value(norm);
            p._internal_update_smoother(rate, false);
        }
        if let Ok(mut a) = automated.lock() {
            a.insert(s.id as usize);
        }
        Ok(())
    };
    let mut describe = mui_motion_bridge::describe("NAME");
    describe["notes"] = true.into();
    describe["patch"] = true.into();
    let served = mui_motion_bridge::run_headless_with(describe, view, edit, audio, patch);
    drop(window);
    served
}
