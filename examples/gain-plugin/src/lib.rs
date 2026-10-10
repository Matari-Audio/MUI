//! The smallest real plugin with a MUI editor: a gain knob and a bypass
//! switch bound to truce parameters, and an output meter.
//!
//! Build with `cargo build --release -p mui-gain-plugin` (or `--profile plugin`),
//! or `cargo truce build --clap --vst3 -p mui-gain-plugin` for bundles.
//! Both workspace profiles unwind, and mui-truce rejects abort builds. When
//! copying this example, set panic = "unwind" in YOUR final workspace; dependency
//! profiles do not propagate. Never enable mui-truce/allow-panic-abort in a DAW.
//! See crates/mui-truce/README.md for the shipping checklist and the remaining
//! truce 6.3 Linux host-thread limitation. Do not select with_host_pump until
//! the format wrapper supplies a host-main-thread callback outside editor locks.
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
        // Both preludes export a `Theme`; the default one is `Ui::default()`,
        // so neither is named here. Name it as `mui::prelude::Theme` if needed.
        let mut ui = Ui::default();
        // Bundled, so this cannot fail short of a corrupt build.
        if let Ok(font) = Font::new(epaint_default_fonts::HACK_REGULAR) {
            ui = ui.font(font);
        }
        MuiEditor::new(params, ui, (300, 200), |ui, bridge| {
            let gain = bridge.bind(ui, P::Gain, |ui, id, v| {
                knob(ui, id, "Gain", v, 0.0..=1.0).size(L)
            });
            let bypass = bridge.bind_bool(ui, P::Bypass, |ui, id, on| toggle(ui, id, "Bypass", on));
            let level = meter(ui, "level", bridge.meter(P::Level)).w(200);
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
                level,
            ]
            .gap(M)
            .pad(L)
            .fill(Role::Surface)
        })
        .resizable((260, 180))
        .into_editor()
    }
}

truce::plugin! {
    logic: Gain,
    params: GainParams,
}
