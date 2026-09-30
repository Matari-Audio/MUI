
// ---- mui-cut live adapter: appended to gallery.rs by media/tools/kurv-live/build.py ----
// KURV's real editor and its real `PluginLogic::process`, in one process,
// behind mui-motion-bridge's live protocol. The DSP and the editor share one
// `KurvParams` and one meter store, so the captured UI shows what the sound
// is doing: meters, envelopes and LFO playheads, modulated knobs, and the
// keys the host holds lit on the keyboard.

/// The notes the host holds, lit on the floating keyboard (keys.rs reads it
/// through [`cut_held`]).
static CUT_HELD: [std::sync::atomic::AtomicU64; 2] =
    [std::sync::atomic::AtomicU64::new(0), std::sync::atomic::AtomicU64::new(0)];

/// Whether the host holds `note`.
pub(crate) fn cut_held(note: u8) -> bool {
    let word = CUT_HELD[usize::from(note / 64)].load(std::sync::atomic::Ordering::Relaxed);
    word >> (note % 64) & 1 == 1
}

fn cut_hold(note: u8, on: bool) {
    let bit = 1u64 << (note % 64);
    let word = &CUT_HELD[usize::from(note / 64)];
    if on {
        word.fetch_or(bit, std::sync::atomic::Ordering::Relaxed);
    } else {
        word.fetch_and(!bit, std::sync::atomic::Ordering::Relaxed);
    }
}

/// `kurv-cut-live [--story NAME] [--scale S]`: the story seeds the patch
/// (default `showcase`: a routed LFO, a warp, noise and an envelope).
#[expect(clippy::too_many_lines, reason = "one adapter, read top to bottom")]
pub fn cut_live() -> Result<(), String> {
    use moose::prelude::*;
    use serde_json::{Value, json};

    let args: Vec<String> = std::env::args().collect();
    let arg = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let story = arg("--story").unwrap_or_else(|| "showcase".into());
    let scale: f64 = arg("--scale").and_then(|s| s.parse().ok()).unwrap_or(1.0);
    let rate = mui_motion_bridge::sample_rate();

    let meters = moose_core::meters::MeterStore::new();
    let transport_slot = moose_core::transport::TransportSlot::new();
    let context = cut_context(Arc::clone(&meters), Arc::clone(&transport_slot));
    let params = Arc::clone(context.params());
    let pointer = script(&story, &context)?;
    params.set_sample_rate(f64::from(rate));
    params.snap_smoothers();

    let mut state = <crate::Kurv as PluginLogic>::init(&params, &InitContext::new(None));
    <crate::Kurv as PluginLogic>::reset(
        &mut state,
        &params,
        &AudioConfig::new(f64::from(rate), mui_motion_bridge::BLOCK_FRAMES),
    );
    let dsp_params = Arc::clone(&params);
    let dsp_meters = Arc::clone(&meters);
    let mut position = 0i64;
    let mut left = vec![0.0f32; mui_motion_bridge::BLOCK_FRAMES];
    let mut right = vec![0.0f32; mui_motion_bridge::BLOCK_FRAMES];
    let mut events = EventList::with_capacity(256);
    let audio = move |notes: &[mui_motion_bridge::NoteEvent], samples: &mut [[f32; 2]]| {
        use mui_motion_bridge::NoteEvent as N;
        events.clear();
        for event in notes {
            match *event {
                N::On { note, velocity } => {
                    cut_hold(note, true);
                    events.push(Event::new(0, EventBody::NoteOn { group: 0, channel: 0, note, velocity }));
                }
                N::Off { note } => {
                    cut_hold(note, false);
                    events.push(Event::new(0, EventBody::NoteOff { group: 0, channel: 0, note, velocity: 0 }));
                }
                N::Panic => {
                    for note in 0..128 {
                        cut_hold(note, false);
                        events.push(Event::new(0, EventBody::NoteOff { group: 0, channel: 0, note, velocity: 0 }));
                    }
                }
            }
        }
        let n = samples.len();
        if n == 0 {
            return;
        }
        let (l, r) = (&mut left[..n], &mut right[..n]);
        l.fill(0.);
        r.fill(0.);
        let inputs: [&[f32]; 0] = [];
        let mut outputs: [&mut [f32]; 2] = [l, r];
        let mut buffer = AudioBuffer::from_slices_checked(&inputs, &mut outputs, n);
        let mut out = EventList::with_capacity(0);
        let seconds = position as f64 / f64::from(rate);
        let transport = TransportInfo {
            playing: true,
            tempo: 120.,
            time_sig_num: 4,
            time_sig_den: 4,
            position_samples: position,
            position_seconds: seconds,
            position_beats: seconds * 2.,
            ..TransportInfo::default()
        };
        transport_slot.write(&transport);
        position += n as i64;
        let sink = |id, value| dsp_meters.write(id, value);
        let mut ctx = ProcessContext::new(&transport, f64::from(rate), n, &mut out).with_meters(&sink);
        let _ = <crate::Kurv as PluginLogic>::process(&mut state, &dsp_params, &mut buffer, &events, &mut ctx);
        for (i, s) in samples.iter_mut().enumerate() {
            *s = [left[i], right[i]];
        }
    };

    let skin = theme::skin_for(
        &params.editor_state.lock().unwrap_or_else(std::sync::PoisonError::into_inner),
    );
    let mut ui = theme::ui(skin);
    ui.set_scale(Some(scale));
    let mut editor = mui_motion_bridge::Editor::new(ui);
    let mut racks = Racks::new(context.clone());
    let mut capture = mui_motion_bridge::CaptureStream::default();
    let view = context.clone();
    let mut booted = false;
    let frame = move |_revision: u64, clock: u64, commands: &[Value]| -> Result<Value, String> {
        let mut step = |ui: &mut mui2::prelude::Ui, input: Input, dt: f64, sizes: &std::collections::BTreeMap<String, Size>| -> Result<(), String> {
            let tree = mui_motion_bridge::Editor::layout(racks.tree(ui, &input), sizes)?;
            let view = mui_motion_bridge::Editor::viewport(sizes, SIZE);
            ui.frame(tree, Some(view), input, dt).map_err(|e| format!("{e:?}"))?;
            racks.after(ui);
            Ok(())
        };
        if !booted {
            // Open the floating keyboard (it starts collapsed) and let the
            // springs settle, the way the `keys` story clicks it.
            booted = true;
            let ui = &mut editor.ui;
            step(ui, Input { pointer, ..Input::default() }, 0., &Default::default())?;
            if let Some(f) = ui.scene().and_then(|s| s.surface(super::keys::TOGGLE)).map(|s| s.frame) {
                let at = Point::new(f.x + f.size.width * 0.5, f.y + f.size.height * 0.5);
                for down in [true, false] {
                    let mut input = Input::default();
                    input.pointer.pos = Some(at);
                    input.pointer.buttons = mui2::prelude::Buttons::default().set(mui2::prelude::Button::Primary, down);
                    step(ui, input, 0., &Default::default())?;
                }
            }
            for _ in 0..SETTLE * 4 {
                step(ui, Input::default(), 1. / 30., &Default::default())?;
            }
        }
        editor.advance(commands, clock, &mut step)?;
        let scene = editor.ui.scene().ok_or("empty scene")?;
        let view_size = editor.view(SIZE);
        let roots = editor.roots(view_size.width, view_size.height);
        #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "a window fits u16")]
        let (width, height) = ((view_size.width * scale).round() as u16, (view_size.height * scale).round() as u16);
        let mut value = capture.frame(scene, width, height, scale, &roots)?;
        value["patch"] = cut_patch(&view);
        Ok(value)
    };
    let edit_context = context.clone();
    let edit = move |command: &Value| -> Result<(), String> {
        if command["op"] != "set" {
            return Err("KURV takes `set` (and notes)".into());
        }
        let value = command["value"].as_f64().filter(|v| v.is_finite()).ok_or("invalid value")?;
        let params = edit_context.params();
        // A host parameter by name or id: `value` is plain, `norm` 0..1.
        let info = params.param_infos().into_iter().find(|i| {
            command["id"].as_u64() == Some(u64::from(i.id))
                || command["id"].as_str().is_some_and(|s| s.eq_ignore_ascii_case(i.name) || s.eq_ignore_ascii_case(i.short_name))
        });
        let info = info.ok_or_else(|| format!("no KURV parameter {}", command["id"]))?;
        match command["field"].as_str() {
            Some("norm") if (0.0..=1.0).contains(&value) => params.set_normalized(info.id, value),
            Some("value" | "plain") => params.set_plain(info.id, value),
            _ => return Err("field is `value` (plain) or `norm` (0..1)".into()),
        }
        Ok(())
    };
    mui_motion_bridge::run_live(
        json!({"name": "KURV", "notes": true, "patch": true, "addKinds": [], "move": false, "delete": false}),
        audio,
        edit,
        frame,
    )
}

/// What the patch is right now: every parameter off its default (plain,
/// formatted, normalised) and every modulation route with its live source
/// value, for mui-cut's patch view.
fn cut_patch(context: &PluginContext<KurvParams>) -> serde_json::Value {
    use serde_json::json;
    let params = context.params();
    let mut changed = Vec::new();
    for info in params.param_infos() {
        let Some(plain) = params.get_plain(info.id) else { continue };
        if (plain - info.default_plain).abs() <= 1e-6 || changed.len() >= 96 {
            continue;
        }
        changed.push(json!({
            "id": info.id,
            "name": info.name,
            "group": info.group,
            "value": plain,
            "text": params.format_value(info.id, plain).unwrap_or_default(),
            "norm": params.get_normalized(info.id).unwrap_or_default(),
        }));
    }
    let routes: Vec<_> = crate::modulation::RouteGraph::capture(context)
        .iter()
        .map(|r| {
            let source = match r.source {
                crate::modulators::routing::ResolvedRouteSource::Rack(i) => format!(
                    "{} {:02}",
                    params.modulator_rack.config(usize::from(i)).kind.label(),
                    i + 1
                ),
                other => format!("{other:?}"),
            };
            json!({
                "source": source,
                "target": crate::modulation::target::label(context, r.target),
                "depth": r.amount,
                "live": crate::modulation::live::source_value(context, r.source),
            })
        })
        .collect();
    let held: Vec<u8> = (0..128).filter(|&n| cut_held(n)).collect();
    json!({"plugin": "KURV", "params": changed, "routes": routes, "held": held})
}

fn cut_context(
    meters: Arc<moose_core::meters::MeterStore>,
    transport: Arc<moose_core::transport::TransportSlot>,
) -> PluginContext<KurvParams> {
    let params = Arc::new(KurvParams::default());
    let (set, get, plain, format) = (
        Arc::clone(&params),
        Arc::clone(&params),
        Arc::clone(&params),
        Arc::clone(&params),
    );
    let bridge = Arc::new(ClosureBridge {
        begin_edit: Box::new(|_| {}),
        set_param: Box::new(move |id, v| set.set_normalized(id, v)),
        end_edit: Box::new(|_| {}),
        request_resize: Box::new(|_, _| false),
        get_param: Box::new(move |id| get.get_normalized(id).unwrap_or_default()),
        get_param_plain: Box::new(move |id| plain.get_plain(id).unwrap_or_default()),
        format_param: Box::new(move |id| {
            let v = format.get_plain(id).unwrap_or_default();
            format.format_value(id, v).unwrap_or_default()
        }),
        get_meter: Box::new(move |id| meters.read(id)),
        get_state: Box::new(Vec::new),
        set_state: Box::new(|_| {}),
        transport: Box::new(move || transport.read()),
    });
    PluginContext::new(bridge, params)
}
