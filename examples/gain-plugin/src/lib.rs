//! The smallest real plugin with a MUI editor: a gain knob and a bypass
//! switch bound to truce parameters, and an output meter.
//!
//! Build it with `cargo build --profile plugin -p mui-gain-plugin`, or with
//! `CARGO_PROFILE_RELEASE_PANIC=unwind cargo truce build` for bundles: the
//! release profile aborts on panic, and a plugin must unwind so a UI panic
//! stays in the editor instead of killing the host.
#![forbid(unsafe_code)]
use mui::prelude::*;
use mui_truce::MuiEditor;
use truce::prelude::*;

#[derive(Params)]
pub struct GainParams {
    #[param(
        id = 0,
        name = "Gain",
        range = "linear(-60, 6)",
        unit = "dB",
        smooth = "exp(5)"
    )]
    pub gain: FloatParam,
    #[param(id = 1, name = "Bypass", default = false)]
    pub bypass: BoolParam,
    #[meter]
    pub level: MeterSlot,
}

use GainParamsParamId as P;

pub struct Gain;

impl PurePluginLogic for Gain {
    type Params = GainParams;

    fn process(
        params: &GainParams,
        buffer: &mut AudioBuffer,
        _events: &EventList,
        context: &mut ProcessContext,
    ) -> ProcessStatus {
        let bypass = params.bypass.value();
        let mut peak = 0.0f32;
        for i in 0..buffer.num_samples() {
            let gain = if bypass {
                1.0
            } else {
                db_to_linear(params.gain.read())
            };
            for ch in 0..buffer.channels() {
                let (inp, out) = buffer.io(ch);
                out[i] = inp[i] * gain;
                peak = peak.max(out[i].abs());
            }
        }
        context.set_meter(&params.level, peak.min(1.0));
        ProcessStatus::Normal
    }

    fn editor(params: Arc<GainParams>) -> Box<dyn Editor> {
        let mut ui = Ui::new(mui::prelude::Theme::DEFAULT);
        // Bundled, so this cannot fail short of a corrupt build.
        if let Ok(font) = Font::new(epaint_default_fonts::HACK_REGULAR) {
            ui = ui.font(font);
        }
        MuiEditor::new(params, ui, (300, 200), |ui, bridge| {
            let gain = bridge.bind(ui, P::Gain, |ui, id, v| {
                knob(ui, id, "Gain", v, 0.0..=1.0).0.size(L).el()
            });
            let bypass = bridge.bind_bool(ui, P::Bypass, |ui, id, on| toggle(ui, id, on).0.el());
            let level = f64::from(bridge.meter(bridge.params().level.id()));
            let meter = row([leaf(200.0 * level.clamp(0.0, 1.0), 8.0)
                .pill()
                .fill(Primary)])
            .size(200.0, 8.0)
            .pill()
            .fill(Field);
            col![
                row![
                    gain,
                    col![
                        title(bridge.text(P::Gain)),
                        row![caption("Bypass"), bypass].gap(S).center(),
                    ]
                    .gap(S),
                ]
                .gap(L)
                .center(),
                meter,
            ]
            .gap(M)
            .pad(L)
            .fill(Surface)
        })
        .resizable((260, 180))
        .into_editor()
    }
}

truce::plugin! {
    logic: Gain,
    params: GainParams,
}
