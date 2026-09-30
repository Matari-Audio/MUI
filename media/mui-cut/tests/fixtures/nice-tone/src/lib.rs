//! A sine at `Pitch` while any note is held, `Level` loud, with a MUI
//! editor in a nice-plug window.
use std::sync::{Arc, Mutex};

use mui::host::{Shared, View};
use mui::prelude::*;
use nice_plug::context::gui::GuiContext;
use nice_plug::editor::{HostMethods, SpawnedEditor, dpi::NativeSize};
use nice_plug::prelude::*;

#[derive(Params)]
pub struct ToneParams {
    #[id = "level"]
    pub level: FloatParam,
    #[id = "pitch"]
    pub pitch: FloatParam,
}

impl Default for ToneParams {
    fn default() -> Self {
        Self {
            level: FloatParam::new("Level", 0.5, FloatRange::Linear { min: 0., max: 1. }),
            pitch: FloatParam::new(
                "Pitch",
                440.,
                FloatRange::Linear {
                    min: 100.,
                    max: 1000.,
                },
            ),
        }
    }
}

#[derive(Default)]
pub struct Tone {
    params: Arc<ToneParams>,
    rate: f32,
    phase: f32,
    held: u32,
}

impl Plugin for Tone {
    const NAME: &'static str = "Nice Tone";
    const VENDOR: &'static str = "mui-cut";
    const URL: &'static str = "";
    const EMAIL: &'static str = "";
    const VERSION: &'static str = "0.1.0";
    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[AudioIOLayout {
        main_input_channels: None,
        main_output_channels: NonZeroU32::new(2),
        ..AudioIOLayout::const_default()
    }];
    const MIDI_INPUT: MidiConfig = MidiConfig::Basic;
    type Editor = ToneEditor;
    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _: AsyncExecutor<Self>) -> Option<ToneEditor> {
        Some(ToneEditor)
    }

    fn activate(
        &mut self,
        _: &AudioIOLayout,
        config: &BufferConfig,
        _: &mut impl ActivateContext<Self>,
    ) -> bool {
        self.rate = config.sample_rate;
        true
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        while let Some(e) = context.next_event() {
            match e {
                NoteEvent::NoteOn { .. } => self.held += 1,
                NoteEvent::NoteOff { .. } => self.held = self.held.saturating_sub(1),
                _ => {}
            }
        }
        let (level, pitch) = (self.params.level.value(), self.params.pitch.value());
        for frame in buffer.iter_samples() {
            let s = if self.held > 0 {
                level * (self.phase * std::f32::consts::TAU).sin()
            } else {
                0.
            };
            self.phase = (self.phase + pitch / self.rate).fract();
            for out in frame {
                *out = s;
            }
        }
        ProcessStatus::Normal
    }
}

impl ClapPlugin for Tone {
    const CLAP_ID: &'static str = "com.mui-cut.nice-tone";
    const CLAP_DESCRIPTION: Option<&'static str> = None;
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[ClapFeature::Instrument];
}

nice_export_clap!(Tone);

pub struct ToneEditor;

struct Panel(f64);

impl View for Panel {
    fn build(&mut self, ui: &mut Ui, _: &Input) -> El {
        col([knob(ui, "level", "Level", &mut self.0, 0.0..=1.0)
            .el
            .into_el()])
        .pad(16.)
        .size(200., 120.)
        .fill(Role::Surface)
        .id("root")
    }
    fn changed(&mut self) -> bool {
        false
    }
    fn request_resize(&mut self, _: u32, _: u32) -> bool {
        false
    }
}

impl Editor for ToneEditor {
    type Handle = ();

    fn spawn(
        &self,
        parent: Option<ParentWindowHandle>,
        _: bool,
        _: Option<f64>,
        _: GuiContext,
        _: Option<HostMethods>,
    ) -> Result<SpawnedEditor<()>, Box<dyn std::error::Error>> {
        let parent = parent.ok_or("no parent window")?;
        let shared = Arc::new(Mutex::new(Shared {
            ui: Ui::default(),
            view: Panel(0.5),
        }));
        let _ = mui_baseview::open(
            &parent,
            "Nice Tone",
            (200, 120),
            None,
            shared,
            Default::default(),
        );
        Ok(SpawnedEditor {
            handle: (),
            window: (),
        })
    }

    fn size(&self) -> NativeSize<u32> {
        NativeSize::new(200, 120)
    }
}
