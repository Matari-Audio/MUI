pub mod dsl;
pub mod module_shell;
pub mod control_panel;
pub mod controls;
pub mod curve_editor;
mod group_header;
pub mod kurv;
pub mod live_theme;
pub mod modulation;
pub mod oscillator;
mod panel;
pub mod pie_container;
pub mod text_input;
use gpui::{App, Context, prelude::*};
use mui_truce::{Document, Edit, Parameter};
use std::sync::{Arc, mpsc};
use truce::prelude::*;

#[derive(Params)]
pub struct ProbeParams {
    // Preserve the original Truce name-hash ID even if this Rust field is renamed.
    #[param(id = 5515006, name = "Gain", range = "linear(0, 1)")]
    pub gain: FloatParam,
    #[persist]
    pub document: Document,
}
pub struct ProbePlugin;
impl PurePluginLogic for ProbePlugin {
    type Params = ProbeParams;
    fn process(
        params: &ProbeParams,
        buffer: &mut AudioBuffer,
        _: &EventList,
        _: &mut ProcessContext,
    ) -> ProcessStatus {
        for i in 0..buffer.num_samples() {
            let gain = params.gain.read();
            for ch in 0..buffer.channels() {
                let (input, output) = buffer.io(ch);
                output[i] = input[i] * gain;
            }
        }
        ProcessStatus::Normal
    }
    fn editor(params: Arc<ProbeParams>) -> Box<dyn Editor> {
        Box::new(GpuiEditor::<ProbeView>::new(params))
    }
}
#[cfg(feature = "clap")]
truce::plugin! { logic: ProbePlugin, params: ProbeParams }

pub struct PanelSnapshot {
    pub normalized_gain: f64,
    pub preset_name: String,
    pub scroll_y: f32,
    pub inner_scroll_y: f32,
    pub painted_scroll: (f32, f32),
    pub baseline_error: f32,
    pub nested_y: f32,
    pub render_error: Option<String>,
}
pub use mui_gpui::EmbeddedView;
pub type GpuiEditor<V = ProbeView> = mui_gpui::GpuiEditor<V>;
pub struct ProbeView {
    params: Arc<ProbeParams>,
    document_revision: u64,
    observed_host_value: f64,
    value: f64,
    gain: Parameter,
    name: gpui::Entity<text_input::TextInput>,
    scroll: gpui::ScrollHandle,
    inner_scroll: gpui::ScrollHandle,
    painted_scroll: std::rc::Rc<std::cell::Cell<(f32, f32)>>,
    baseline_error: f32,
    nested_y: f32,
    render_error: Option<String>,
    gain_focus: gpui::FocusHandle,
    drag: Option<(gpui::Point<gpui::Pixels>, f64, bool)>,
    click_armed: bool,
}
impl ProbeView {
    fn toggle(&mut self, cx: &mut Context<Self>) {
        if self.drag.is_some() {
            self.end_drag();
            self.click_armed = false;
        }
        self.value = if self.value < 0.5 { 1.0 } else { 0.0 };
        if self.gain.begin() {
            self.gain.set(self.value);
            self.gain.end();
        }
        cx.notify();
    }
}
impl EmbeddedView for ProbeView {
    fn initialize(cx: &mut App) { text_input::bind_keys(cx); }
    type Params = ProbeParams;
    type Snapshot = PanelSnapshot;
    fn create(
        params: Arc<ProbeParams>,
        edits: mpsc::Sender<Edit>,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let value = params
            .get_normalized(ProbeParamsParamId::Gain.into())
            .expect("Gain parameter");
        cx.observe_window_activation(window, |view: &mut ProbeView, window, _| {
            if !window.is_window_active() {
                view.click_armed = false;
                view.end_drag();
            }
        })
        .detach();
        let name = cx.new(|cx| {
            text_input::TextInput::with_value(cx, &params.document.snapshot().name, "Preset name…")
        });
        let document = params.clone();
        cx.observe(&name, move |_, name, cx| {
            let value = name.read(cx).value().to_owned();
            if document
                .document
                .edit(|state| {
                    state.name = value;
                    Ok(())
                })
                .is_err()
            {
                let saved = document.document.snapshot().name;
                name.update(cx, |input, cx| input.set_value(&saved, cx));
            }
        })
        .detach();
        ProbeView {
            document_revision: params.document.revision(),
            observed_host_value: value,
            params: params.clone(),
            value,
            gain: Parameter::new(params.clone(), ProbeParamsParamId::Gain.into(), true, edits)
                .expect("declared Gain parameter"),
            name,
            scroll: gpui::ScrollHandle::new(),
            inner_scroll: gpui::ScrollHandle::new(),
            painted_scroll: Default::default(),
            baseline_error: 0.,
            nested_y: 0.,
            render_error: None,
            gain_focus: cx.focus_handle().tab_index(0).tab_stop(true),
            drag: None,
            click_armed: false,
        }
    }
    fn synchronize(&mut self, cx: &mut Context<Self>) {
        let value = self
            .params
            .get_normalized(ProbeParamsParamId::Gain.into())
            .expect("Gain parameter");
        if self.drag.is_none() && value != self.observed_host_value {
            self.observed_host_value = value;
            if self.value != value {
                self.value = value;
                cx.notify();
            }
        }
        let revision = self.params.document.revision();
        if revision != self.document_revision {
            self.document_revision = revision;
            let name = self.params.document.snapshot().name;
            if self.name.read(cx).value() != name {
                self.name.update(cx, |input, cx| input.set_value(&name, cx));
            }
        }
    }
    fn snapshot(&self, cx: &App) -> PanelSnapshot {
        PanelSnapshot {
            normalized_gain: self.value,
            preset_name: self.name.read(cx).value().to_owned(),
            scroll_y: self.scroll.offset().y.into(),
            inner_scroll_y: self.inner_scroll.offset().y.into(),
            painted_scroll: self.painted_scroll.get(),
            baseline_error: self.baseline_error,
            nested_y: self.nested_y,
            render_error: self.render_error.clone(),
        }
    }
    fn closing(&mut self, _: &mut Context<Self>) {
        self.end_drag();
    }
}
