//! A sine at `Pitch` while any note is held, `Level` loud, and a
//! modulation route bank (`Mod 1 Source/Target/Amount`).
use mui::prelude::*;
use mui_truce::MuiEditor;
use truce::prelude::*;

#[derive(ParamEnum)]
pub enum Source {
    Off,
    Lfo,
}

#[derive(ParamEnum)]
pub enum Target {
    Level,
    Pitch,
}

#[derive(Params)]
pub struct ToneParams {
    #[param(id = 0, name = "Level", range = "linear(0, 1)", default = 0.5)]
    pub level: FloatParam,
    #[param(
        id = 1,
        name = "Pitch",
        range = "linear(100, 1000)",
        default = 440.0,
        unit = "Hz"
    )]
    pub pitch: FloatParam,
    #[param(id = 2, name = "Mod 1 Source", default = 0)]
    pub source: EnumParam<Source>,
    #[param(id = 3, name = "Mod 1 Target", default = 0)]
    pub target: EnumParam<Target>,
    #[param(id = 4, name = "Mod 1 Amount", range = "linear(-1, 1)", default = 0.0)]
    pub amount: FloatParam,
}

#[derive(Default)]
pub struct ToneState {
    held: u32,
    phase: f64,
    rate: f64,
}

pub struct Tone;

impl PluginLogic for Tone {
    type Params = ToneParams;
    type DspState = ToneState;

    fn bus_layouts() -> Vec<BusLayout> {
        BusLayout::stereo_and_mono_output()
    }

    fn reset(state: &mut ToneState, _: &ToneParams, config: &AudioConfig) {
        *state = ToneState {
            rate: config.sample_rate,
            ..ToneState::default()
        };
    }

    fn process(
        state: &mut ToneState,
        params: &ToneParams,
        buffer: &mut AudioBuffer,
        events: &EventList,
        _: &mut ProcessContext,
    ) -> ProcessStatus {
        for e in events.iter() {
            match e.body {
                EventBody::NoteOn { .. } => state.held += 1,
                EventBody::NoteOff { .. } => state.held = state.held.saturating_sub(1),
                _ => {}
            }
        }
        let (level, pitch) = (
            f64::from(params.level.value()),
            f64::from(params.pitch.value()),
        );
        for i in 0..buffer.num_samples() {
            let s = if state.held > 0 {
                level * (state.phase * std::f64::consts::TAU).sin()
            } else {
                0.
            };
            state.phase = (state.phase + pitch / state.rate).fract();
            for ch in 0..buffer.num_output_channels() {
                buffer.output(ch)[i] = s as f32;
            }
        }
        ProcessStatus::Normal
    }

    fn editor(params: Arc<ToneParams>) -> Box<dyn Editor> {
        MuiEditor::new(params, Ui::default(), (200, 120), |ui, bridge| {
            let level = bridge.bind(ui, ToneParamsParamId::Level, |ui, id, v| {
                knob(ui, id, "Level", v, 0.0..=1.0)
            });
            col![level].pad(L).fill(Role::Surface)
        })
        .into_editor()
    }
}

truce::plugin! { logic: Tone, params: ToneParams }
