use gpui::{App, Application, Bounds, Context, WindowBounds, WindowOptions, prelude::*, px, size};
use mui_truce::{Automation, Edit};
use raw_window_handle::HasWindowHandle;
use std::sync::{Arc, mpsc};
use truce_core::editor::{Editor, PluginContext, RawWindowHandle};
use x11rb::{
    connection::Connection,
    protocol::xproto::{ConfigureWindowAux, ConnectionExt},
};

/// A plugin-owned GPUI root. The runtime owns embedding and host-thread automation;
/// the view reads its existing synth state and sends balanced parameter edits.
pub trait EmbeddedView: Render + Sized + 'static {
    fn initialize(_: &mut App) {}
    const INITIAL_SIZE: (u32, u32) = (640, 360);
    const MIN_SIZE: (u32, u32) = (320, 180);
    const MAX_SIZE: (u32, u32) = (1920, 1080);
    type Params: Send + Sync + 'static;
    type Snapshot: Send + 'static;
    fn create(
        params: Arc<Self::Params>,
        edits: mpsc::Sender<Edit>,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) -> Self;
    /// Called before synchronization; history must wait until host dispatch drains UI edits.
    fn host_edits_pending(&mut self, _: bool) {}
    fn synchronize(&mut self, cx: &mut Context<Self>);
    fn snapshot(&self, cx: &App) -> Self::Snapshot;
    fn closing(&mut self, cx: &mut Context<Self>);
}
enum Command<S> {
    Inspect(mpsc::SyncSender<S>),
    Close,
}
struct Session<S> {
    resize: Arc<std::sync::Mutex<Option<(u32, u32)>>>,
    commands: mpsc::SyncSender<Command<S>>,
    edits: mpsc::Receiver<Edit>,
    worker: std::thread::JoinHandle<anyhow::Result<()>>,
    context: PluginContext,
    automation: Automation,
    pending: Arc<std::sync::atomic::AtomicUsize>,
}
pub struct GpuiEditor<V: EmbeddedView> {
    params: Arc<V::Params>,
    dimensions: (u32, u32),
    session: Option<Session<V::Snapshot>>,
    pub last_error: Option<String>,
    pub child: Option<u32>,
}
impl<V: EmbeddedView> GpuiEditor<V> {
    pub fn new(params: Arc<V::Params>) -> Self {
        Self {
            params,
            dimensions: V::INITIAL_SIZE,
            session: None,
            last_error: None,
            child: None,
        }
    }
    /// Read-only diagnostics for the runnable editor-contract check.
    pub fn snapshot(&self) -> anyhow::Result<V::Snapshot> {
        let (send, receive) = mpsc::sync_channel(1);
        self.session
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Editor is closed"))?
            .commands
            .try_send(Command::Inspect(send))?;
        Ok(receive.recv_timeout(std::time::Duration::from_secs(5))?)
    }
}
impl<V: EmbeddedView> Editor for GpuiEditor<V> {
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
                self.last_error = Some("GPUI embedding requires X11".into());
                return;
            }
        };
        let (commands, receive) = mpsc::sync_channel(8);
        let (edits, events) = mpsc::channel();
        let (ready, started) = mpsc::sync_channel(1);
        let dimensions = self.dimensions;
        let resize = Arc::new(std::sync::Mutex::new(None));
        let pending_resize = resize.clone();
        let pending = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let worker_pending = pending.clone();
        let params = self.params.clone();
        let worker = std::thread::spawn(move || {
            run_editor::<V>(
                parent,
                dimensions,
                params,
                receive,
                edits,
                ready,
                pending_resize,
                worker_pending,
            )
        });
        match started.recv() {
            Ok(child) => {
                self.child = Some(child);
                self.session = Some(Session {
                    resize,
                    commands,
                    edits: events,
                    worker,
                    context,
                    automation: Automation::default(),
                    pending,
                });
            }
            Err(_) => {
                self.last_error = Some(worker_result(worker));
            }
        }
    }
    fn idle(&mut self) {
        if let Some(session) = &mut self.session {
            for edit in session.edits.try_iter() {
                session.automation.dispatch(&session.context, edit);
                session
                    .pending
                    .fetch_sub(1, std::sync::atomic::Ordering::Release);
            }
        }
    }
    fn close(&mut self) {
        if let Some(mut session) = self.session.take() {
            let _ = session.commands.send(Command::Close);
            // Keep host-thread dispatch alive while the worker completes its closing hook.
            while !session.worker.is_finished() {
                if let Ok(edit) = session
                    .edits
                    .recv_timeout(std::time::Duration::from_millis(1))
                {
                    session.automation.dispatch(&session.context, edit);
                    session
                        .pending
                        .fetch_sub(1, std::sync::atomic::Ordering::Release);
                }
            }
            let error = worker_result(session.worker);
            if !error.is_empty() {
                self.last_error = Some(error);
            }
            for edit in session.edits.try_iter() {
                session.automation.dispatch(&session.context, edit);
                session
                    .pending
                    .fetch_sub(1, std::sync::atomic::Ordering::Release);
            }
            // Balance host automation even if the UI worker failed before sending End.
            session.automation.close(&session.context);
        }
        self.child = None;
    }
    fn can_resize(&self) -> bool {
        true
    }
    fn min_size(&self) -> (u32, u32) {
        V::MIN_SIZE
    }
    fn max_size(&self) -> (u32, u32) {
        V::MAX_SIZE
    }
    fn set_size(&mut self, w: u32, h: u32) -> bool {
        // Limits describe preferred host requests, not an already allocated parent.
        // Refusing a real resize strands the child at its previous dimensions.
        if w == 0 || h == 0 {
            return false;
        }
        if let Some(session) = &self.session {
            // A resize burst replaces the pending size; the final host allocation cannot be dropped.
            *session
                .resize
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some((w, h));
        }
        self.dimensions = (w, h);
        true
    }
}
impl<V: EmbeddedView> Drop for GpuiEditor<V> {
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
#[expect(
    clippy::too_many_arguments,
    reason = "the worker owns distinct host window, parameter and lifecycle channel handles"
)]
fn run_editor<V: EmbeddedView>(
    parent: u32,
    dimensions: (u32, u32),
    params: Arc<V::Params>,
    commands: mpsc::Receiver<Command<V::Snapshot>>,
    edits: mpsc::Sender<Edit>,
    ready: mpsc::SyncSender<u32>,
    resize: Arc<std::sync::Mutex<Option<(u32, u32)>>>,
    pending: Arc<std::sync::atomic::AtomicUsize>,
) -> anyhow::Result<()> {
    let (ui_edits, queued) = mpsc::channel();
    let forward = || forward_edits(&queued, &edits, &pending);
    let (connection, _) = x11rb::connect(None)?;
    connection.get_window_attributes(parent)?.reply()?;
    let runtime = gpui_linux::EmbeddedX11::new()?;
    let window_slot = std::rc::Rc::new(std::cell::RefCell::new(None));
    let slot = window_slot.clone();
    let app = Application::with_platform(runtime.platform()).run_embedded(move |cx: &mut App| {
        V::initialize(cx);
        *slot.borrow_mut() = Some(cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    Default::default(),
                    size(px(dimensions.0 as f32), px(dimensions.1 as f32)),
                ))),
                titlebar: None,
                ..Default::default()
            },
            |window, cx| cx.new(|cx| V::create(params, ui_edits, window, cx)),
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
            // Embedded child input and redraw share this pump. 8 ms capped both below
            // 165 Hz; bounded 4 ms polling leaves rendering headroom without busy-waiting.
            match commands.recv_timeout(std::time::Duration::from_millis(4)) {
                Ok(Command::Close) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Ok(Command::Inspect(reply)) => app.update(|cx| {
                    window.update(cx, |view, _, cx| {
                        let _ = reply.send(view.snapshot(cx));
                    })
                })?,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            if let Some((w, h)) = resize
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take()
            {
                connection
                    .configure_window(child, &ConfigureWindowAux::new().width(w).height(h))?
                    .check()?;
                connection.flush()?;
            }
            forward()?;
            app.update(|cx| {
                window.update(cx, |view, _, cx| {
                    view.host_edits_pending(
                        pending.load(std::sync::atomic::Ordering::Acquire) != 0,
                    );
                    view.synchronize(cx);
                })
            })?;
            runtime.pump()?;
        }
        Ok(())
    })();
    forward()?;
    // A closing hook may restore state. Apply all earlier edits before that restore.
    while pending.load(std::sync::atomic::Ordering::Acquire) != 0 {
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    app.update(|cx| {
        window.update(cx, |view, window, cx| {
            view.closing(cx);
            window.remove_window();
        })
    })?;
    forward()?;
    runtime.pump()?;
    result
}

// Publish the pending count before the host can observe the corresponding event.
fn forward_edits(
    queue: &mpsc::Receiver<Edit>,
    host: &mpsc::Sender<Edit>,
    pending: &std::sync::atomic::AtomicUsize,
) -> anyhow::Result<()> {
    use std::sync::atomic::Ordering;
    for edit in queue.try_iter() {
        pending.fetch_add(1, Ordering::Release);
        if let Err(error) = host.send(edit) {
            pending.fetch_sub(1, Ordering::Release);
            return Err(error.into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod acknowledgement_tests {
    use super::{Edit, forward_edits};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc,
    };
    #[test]
    fn history_barrier_stays_pending_until_last_host_dispatch() {
        let (ui, queued) = mpsc::channel();
        let (host, received) = mpsc::channel();
        let pending = AtomicUsize::new(0);
        for edit in [Edit::Begin(1), Edit::Value(1, 0.75), Edit::End(1)] {
            ui.send(edit).unwrap();
        }
        forward_edits(&queued, &host, &pending).unwrap();
        let mut live = 0.;
        for expected in [3, 2, 1] {
            assert_eq!(pending.load(Ordering::Acquire), expected);
            if let Edit::Value(_, value) = received.recv().unwrap() {
                live = value;
            }
            pending.fetch_sub(1, Ordering::Release);
        }
        assert_eq!(pending.load(Ordering::Acquire), 0);
        assert_eq!(live, 0.75);
        ui.send(Edit::Begin(1)).unwrap();
        forward_edits(&queued, &host, &pending).unwrap();
        assert_eq!(
            pending.load(Ordering::Acquire),
            1,
            "a later gesture reopens the barrier"
        );
        received.recv().unwrap();
        pending.fetch_sub(1, Ordering::Release);
        drop(received);
        ui.send(Edit::End(1)).unwrap();
        assert!(forward_edits(&queued, &host, &pending).is_err());
        assert_eq!(
            pending.load(Ordering::Acquire),
            0,
            "failed sends cannot strand the barrier"
        );
    }
}
