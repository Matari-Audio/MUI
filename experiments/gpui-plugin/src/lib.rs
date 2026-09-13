mod panel;
#[path = "../../upstream/mui_text_input.rs"]
mod text_input;
use gpui::{App, Application, Bounds, Context, WindowBounds, WindowOptions, prelude::*, px, size};
use raw_window_handle::HasWindowHandle;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
    mpsc,
};
use truce::prelude::*;
use truce_core::editor::{PluginContext, RawWindowHandle};
use x11rb::{
    connection::Connection,
    protocol::xproto::{ConfigureWindowAux, ConnectionExt},
};

#[derive(Params)]
pub struct ProbeParams {
    #[param(name = "Gain", range = "linear(0, 1)")]
    pub gain: FloatParam,
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
    fn editor(_: Arc<ProbeParams>) -> Box<dyn Editor> {
        Box::new(GpuiEditor::default())
    }
}
truce::plugin! { logic: ProbePlugin, params: ProbeParams }

pub struct PanelSnapshot {
    pub preset_name: String,
    pub scroll_y: f32,
}
enum Edit {
    Begin,
    Value(f64),
    End,
}
enum Command {
    Inspect(mpsc::SyncSender<PanelSnapshot>),
    Resize(u32, u32),
    Close,
}
struct Session {
    commands: mpsc::SyncSender<Command>,
    edits: mpsc::Receiver<Edit>,
    worker: std::thread::JoinHandle<anyhow::Result<()>>,
    context: PluginContext,
    host_value: Arc<AtomicU64>,
    edit_open: bool,
}
impl Session {
    fn drain_edits(context: &PluginContext, edits: &mpsc::Receiver<Edit>, edit_open: &mut bool) {
        for event in edits.try_iter() {
            match event {
                Edit::Begin if !*edit_open => {
                    context.begin_edit(ProbeParamsParamId::Gain);
                    *edit_open = true;
                }
                Edit::Value(value) if *edit_open => {
                    context.set_param(ProbeParamsParamId::Gain, value)
                }
                Edit::End if *edit_open => {
                    context.end_edit(ProbeParamsParamId::Gain);
                    *edit_open = false;
                }
                _ => {}
            }
        }
    }
}
/// Linux/X11 compatibility probe, deliberately outside MUI's stable API.
/// GPUI's Rc objects stay on one worker; host callbacks stay in Editor::idle.
pub struct GpuiEditor {
    dimensions: (u32, u32),
    session: Option<Session>,
    pub last_error: Option<String>,
    pub child: Option<u32>,
}
impl Default for GpuiEditor {
    fn default() -> Self {
        Self {
            dimensions: (640, 360),
            session: None,
            last_error: None,
            child: None,
        }
    }
}
impl GpuiEditor {
    /// Read-only diagnostics for the runnable editor-contract check.
    pub fn snapshot(&self) -> anyhow::Result<PanelSnapshot> {
        let (send, receive) = mpsc::sync_channel(1);
        self.session
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Editor is closed"))?
            .commands
            .try_send(Command::Inspect(send))?;
        Ok(receive.recv_timeout(std::time::Duration::from_secs(5))?)
    }
}
impl Editor for GpuiEditor {
    fn size(&self) -> (u32, u32) {
        self.dimensions
    }
    fn open(&mut self, parent: RawWindowHandle, context: PluginContext) {
        self.close();
        self.last_error = None;
        let parent = match parent {
            RawWindowHandle::X11(id) => match u32::try_from(id) {
                Ok(id) if id != 0 => id,
                _ => {
                    self.last_error = Some("Invalid X11 parent".into());
                    return;
                }
            },
            _ => {
                self.last_error = Some("Probe requires X11".into());
                return;
            }
        };
        let (commands, receive) = mpsc::sync_channel(8);
        let (edits, events) = mpsc::channel();
        let (ready, started) = mpsc::sync_channel(1);
        let dimensions = self.dimensions;
        let value = context.bridge().get_param(ProbeParamsParamId::Gain.into());
        let host_value = Arc::new(AtomicU64::new(value.to_bits()));
        let worker_value = host_value.clone();
        let worker = std::thread::spawn(move || {
            run_editor(parent, dimensions, worker_value, receive, edits, ready)
        });
        match started.recv() {
            Ok(child) => {
                self.child = Some(child);
                self.session = Some(Session {
                    commands,
                    edits: events,
                    worker,
                    context,
                    host_value,
                    edit_open: false,
                });
            }
            Err(_) => {
                self.last_error = Some(worker_result(worker));
            }
        }
    }
    fn idle(&mut self) {
        if let Some(session) = &mut self.session {
            Session::drain_edits(&session.context, &session.edits, &mut session.edit_open);
            session.host_value.store(
                session
                    .context
                    .bridge()
                    .get_param(ProbeParamsParamId::Gain.into())
                    .to_bits(),
                Ordering::Relaxed,
            );
        }
    }
    fn close(&mut self) {
        if let Some(mut session) = self.session.take() {
            let _ = session.commands.send(Command::Close);
            let error = worker_result(session.worker);
            if !error.is_empty() {
                self.last_error = Some(error);
            }
            Session::drain_edits(&session.context, &session.edits, &mut session.edit_open);
            // Balance host automation even if the UI worker failed before sending End.
            if session.edit_open {
                session.context.end_edit(ProbeParamsParamId::Gain);
            }
        }
        self.child = None;
    }
    fn can_resize(&self) -> bool {
        true
    }
    fn min_size(&self) -> (u32, u32) {
        (320, 180)
    }
    fn max_size(&self) -> (u32, u32) {
        (1920, 1080)
    }
    fn set_size(&mut self, w: u32, h: u32) -> bool {
        if !(320..=1920).contains(&w) || !(180..=1080).contains(&h) {
            return false;
        }
        if let Some(session) = &self.session {
            if session.commands.try_send(Command::Resize(w, h)).is_err() {
                return false;
            }
        }
        self.dimensions = (w, h);
        true
    }
}
impl Drop for GpuiEditor {
    fn drop(&mut self) {
        self.close();
    }
}
fn worker_result(worker: std::thread::JoinHandle<anyhow::Result<()>>) -> String {
    match worker.join() {
        Ok(Ok(())) => String::new(),
        Ok(Err(e)) => format!("{e:#}"),
        Err(_) => "GPUI worker panicked".into(),
    }
}
struct ProbeView {
    value: f64,
    edits: mpsc::Sender<Edit>,
    name: gpui::Entity<text_input::TextInput>,
    scroll: gpui::ScrollHandle,
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
        let _ = self.edits.send(Edit::Begin);
        let _ = self.edits.send(Edit::Value(self.value));
        let _ = self.edits.send(Edit::End);
        cx.notify();
    }
}
fn run_editor(
    parent: u32,
    dimensions: (u32, u32),
    host_value: Arc<AtomicU64>,
    commands: mpsc::Receiver<Command>,
    edits: mpsc::Sender<Edit>,
    ready: mpsc::SyncSender<u32>,
) -> anyhow::Result<()> {
    let value = f64::from_bits(host_value.load(Ordering::Relaxed));
    let (connection, _) = x11rb::connect(None)?;
    connection.get_window_attributes(parent)?.reply()?;
    let runtime = gpui_linux::EmbeddedX11::new()?;
    let window_slot = std::rc::Rc::new(std::cell::RefCell::new(None));
    let slot = window_slot.clone();
    let app = Application::with_platform(runtime.platform()).run_embedded(move |cx: &mut App| {
        text_input::bind_keys(cx);
        *slot.borrow_mut() = Some(cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    Default::default(),
                    size(px(dimensions.0 as f32), px(dimensions.1 as f32)),
                ))),
                titlebar: None,
                ..Default::default()
            },
            |_, cx| {
                cx.new(|cx| ProbeView {
                    value,
                    edits,
                    name: cx.new(text_input::TextInput::new),
                    scroll: gpui::ScrollHandle::new(),
                    gain_focus: cx.focus_handle().tab_index(0).tab_stop(true),
                    drag: None,
                    click_armed: false,
                })
            },
        ));
    });
    let window = window_slot
        .borrow_mut()
        .take()
        .ok_or_else(|| anyhow::anyhow!("GPUI launch callback missing"))??;
    let child = app.update(|cx| {
        window.update(cx, |_, window, _| {
            if let Some(gpu) = window.gpu_specs() {
                eprintln!(
                    "GPUI device: {} ({}; software={})",
                    gpu.device_name, gpu.driver_info, gpu.is_software_emulated
                );
            }
            match HasWindowHandle::window_handle(window)?.as_raw() {
                raw_window_handle::RawWindowHandle::Xcb(handle) => Ok(handle.window.get()),
                raw_window_handle::RawWindowHandle::Xlib(handle) => {
                    Ok(u32::try_from(handle.window)?)
                }
                _ => anyhow::bail!("Expected GPUI X11 window"),
            }
        })
    })??;
    connection.reparent_window(child, parent, 0, 0)?.check()?;
    connection
        .configure_window(
            child,
            &ConfigureWindowAux::new()
                .width(dimensions.0)
                .height(dimensions.1),
        )?
        .check()?;
    connection.map_window(child)?.check()?;
    connection.flush()?;
    ready.send(child)?;
    let mut observed_host_value = value;
    let result = (|| -> anyhow::Result<()> {
        loop {
            match commands.recv_timeout(std::time::Duration::from_millis(8)) {
                Ok(Command::Close) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Ok(Command::Inspect(reply)) => app.update(|cx| {
                    window.update(cx, |view, _, cx| {
                        let _ = reply.send(PanelSnapshot {
                            preset_name: view.name.read(cx).value().to_owned(),
                            scroll_y: view.scroll.offset().y.into(),
                        });
                    })
                })?,
                Ok(Command::Resize(w, h)) => {
                    connection
                        .configure_window(child, &ConfigureWindowAux::new().width(w).height(h))?
                        .check()?;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            let value = f64::from_bits(host_value.load(Ordering::Relaxed));
            if value != observed_host_value {
                observed_host_value = value;
                app.update(|cx| {
                    window.update(cx, |view, _, cx| {
                        if view.drag.is_none() && view.value != value {
                            view.value = value;
                            cx.notify();
                        }
                    })
                })?;
            }
            runtime.pump()?;
        }
        Ok(())
    })();
    app.update(|cx| {
        window.update(cx, |view, window, _| {
            view.end_drag();
            window.remove_window();
        })
    })?;
    runtime.pump()?;
    result
}
