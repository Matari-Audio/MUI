use gpui::{
    div, prelude::*, px, rgb, size, App, Application, Bounds, Context, Window, WindowBounds,
    WindowOptions,
};
use raw_window_handle::HasWindowHandle;
use std::sync::{mpsc, Arc};
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

enum Command {
    Value(f64),
    Resize(u32, u32),
    Close,
}
struct Session {
    commands: mpsc::SyncSender<Command>,
    edits: mpsc::Receiver<f64>,
    worker: std::thread::JoinHandle<anyhow::Result<()>>,
    context: PluginContext,
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
        let worker = std::thread::spawn(move || {
            run_editor(parent, dimensions, value, receive, edits, ready)
        });
        match started.recv() {
            Ok(child) => {
                self.child = Some(child);
                self.session = Some(Session {
                    commands,
                    edits: events,
                    worker,
                    context,
                });
            }
            Err(_) => {
                self.last_error = Some(worker_result(worker));
            }
        }
    }
    fn idle(&mut self) {
        if let Some(session) = &self.session {
            for value in session.edits.try_iter() {
                session.context.begin_edit(ProbeParamsParamId::Gain);
                session.context.set_param(ProbeParamsParamId::Gain, value);
                session.context.end_edit(ProbeParamsParamId::Gain);
            }
            let _ = session.commands.try_send(Command::Value(
                session
                    .context
                    .bridge()
                    .get_param(ProbeParamsParamId::Gain.into()),
            ));
        }
    }
    fn close(&mut self) {
        if let Some(session) = self.session.take() {
            let _ = session.commands.send(Command::Close);
            let error = worker_result(session.worker);
            if !error.is_empty() {
                self.last_error = Some(error);
            }
            for value in session.edits.try_iter() {
                session.context.begin_edit(ProbeParamsParamId::Gain);
                session.context.set_param(ProbeParamsParamId::Gain, value);
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
    edits: mpsc::Sender<f64>,
}
impl ProbeView {
    fn toggle(&mut self, cx: &mut Context<Self>) {
        self.value = if self.value < 0.5 { 1.0 } else { 0.0 };
        let _ = self.edits.send(self.value);
        cx.notify();
    }
}
impl Render for ProbeView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .p_4()
            .bg(rgb(0x181c24))
            .text_color(rgb(0xffffff))
            .child("MUI • GPUI plugin runtime")
            .child(
                div()
                    .id("gain")
                    .role(gpui::Role::Button)
                    .aria_label("Toggle gain between zero and full")
                    .focusable()
                    .focus(|style| style.border_2().border_color(rgb(0xffffff)))
                    .mt_4()
                    .p_4()
                    .bg(rgb(0x375b85))
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.toggle(cx);
                    }))
                    .child(format!(
                        "Gain: {:.0}% — click to toggle",
                        self.value * 100.0
                    )),
            )
    }
}
fn run_editor(
    parent: u32,
    dimensions: (u32, u32),
    value: f64,
    commands: mpsc::Receiver<Command>,
    edits: mpsc::Sender<f64>,
    ready: mpsc::SyncSender<u32>,
) -> anyhow::Result<()> {
    let (connection, _) = x11rb::connect(None)?;
    connection.get_window_attributes(parent)?.reply()?;
    let runtime = gpui_linux::EmbeddedX11::new()?;
    let window_slot = std::rc::Rc::new(std::cell::RefCell::new(None));
    let slot = window_slot.clone();
    let app = Application::with_platform(runtime.platform()).run_embedded(move |cx: &mut App| {
        *slot.borrow_mut() = Some(cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    Default::default(),
                    size(px(dimensions.0 as f32), px(dimensions.1 as f32)),
                ))),
                titlebar: None,
                ..Default::default()
            },
            |_, cx| cx.new(|_| ProbeView { value, edits }),
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
    let result = (|| -> anyhow::Result<()> {
        loop {
            match commands.recv_timeout(std::time::Duration::from_millis(8)) {
                Ok(Command::Close) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Ok(Command::Resize(w, h)) => {
                    connection
                        .configure_window(child, &ConfigureWindowAux::new().width(w).height(h))?
                        .check()?;
                }
                Ok(Command::Value(value)) => app.update(|cx| {
                    window.update(cx, |view, _, cx| {
                        if view.value != value {
                            view.value = value;
                            cx.notify();
                        }
                    })
                })?,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            runtime.pump()?;
        }
        Ok(())
    })();
    app.update(|cx| window.update(cx, |_, window, _| window.remove_window()))?;
    runtime.pump()?;
    result
}
