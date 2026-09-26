//! truce's `Editor` over the window in [`crate::window`].
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use mui::Ui;
use mui::scene::{El, Size};
use truce_core::editor::{Editor, PluginContext, RawWindowHandle};
use truce_params::Params;

use crate::Bridge;
use crate::platform::{ParentWindow, editor_window_scale};
use crate::window::{self, Requests, Shared, View, lock};

/// The last scale any host passed to `set_scale_factor`, f64 bits, 0 = never.
///
/// truce's CLAP wrapper builds a new editor on every `gui.create` and tells
/// it the host scale only when the host calls `gui.set_scale`, which hosts
/// that set it once per instance do not repeat, so a reopened editor came
/// back at 1.0 inside a 1.5x frame. (truce's VST3 wrapper replays it.)
// ponytail: one scale per process; per-instance if a host ever mixes scales.
static HOST_SCALE: AtomicU64 = AtomicU64::new(0);

fn last_host_scale() -> Option<f64> {
    match HOST_SCALE.load(Ordering::Relaxed) {
        0 => None,
        bits => Some(f64::from_bits(bits)),
    }
}

type Build<P> = Box<dyn FnMut(&mut Ui, &mut Bridge<P>) -> El + Send>;

/// The model half: the app's build closure and the parameters it binds.
pub(crate) struct Session<P: Params> {
    pub(crate) bridge: Bridge<P>,
    build: Build<P>,
}

impl<P: Params> View for Session<P> {
    fn build(&mut self, ui: &mut Ui) -> El {
        let root = (self.build)(ui, &mut self.bridge);
        self.bridge.end_unbound();
        root
    }
    fn changed(&mut self) -> bool {
        self.bridge.changed()
    }
    fn request_resize(&mut self, width: u32, height: u32) -> bool {
        self.bridge
            .context()
            .is_some_and(|c| c.request_resize(width, height))
    }
}

/// A MUI editor for any truce plugin: `build` makes the tree every frame
/// from the retained `Ui` and a [`Bridge`] to the plugin's parameters.
/// State the editor keeps between frames lives in the closure's captures;
/// it and the `Ui` survive a close and reopen.
///
/// ```ignore
/// fn editor(params: Arc<GainParams>) -> Box<dyn Editor> {
///     let ui = Ui::default().font(font);
///     MuiEditor::new(params, ui, (320, 200), |ui, bridge| {
///         bridge.bind(ui, P::Gain, |ui, id, v| knob(ui, id, "Gain", v, 0.0..=1.0))
///     })
///     .resizable((240, 160))
///     .into_editor()
/// }
/// ```
///
/// # Panics and build profile
///
/// The editor's window callbacks catch panics so a UI bug stays in the UI.
/// That only works when the plugin cdylib unwinds: build it with the
/// workspace's `plugin` profile (`cargo build --profile plugin`), never plain
/// `--release`, whose `panic = "abort"` makes every UI panic abort the DAW.
pub struct MuiEditor<P: Params> {
    shared: Arc<Mutex<Shared<Session<P>>>>,
    requests: Arc<Requests>,
    params: Arc<P>,
    size: (u32, u32),
    min: Option<(u32, u32)>,
    system_scale: bool,
    host_scale: Option<f64>,
    window: Option<Handle>,
}

/// Whole logical points, as the host's window API takes them.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a window is a few thousand points, never negative"
)]
fn points(s: Size) -> (u32, u32) {
    (
        s.width.round().max(0.0) as u32,
        s.height.round().max(0.0) as u32,
    )
}

/// The one field of [`MuiEditor`] that is not auto-`Send`.
struct Handle(baseview::WindowHandle);

// SAFETY: `baseview::WindowHandle` wraps a native window pointer and is not
// auto-`Send`. truce calls `open`, `set_size` and `close` from one GUI
// thread, never concurrently, so the handle never leaves the thread that
// made it; `Send` is only what `Box<dyn Editor>` asks for. truce-gui's own
// `GpuEditor` makes the same argument.
#[expect(unsafe_code, reason = "vouches Send for the baseview handle alone")]
unsafe impl Send for Handle {}

impl<P: Params> MuiEditor<P> {
    /// A fixed-size editor, `size` logical points.
    pub fn new(
        params: Arc<P>,
        ui: Ui,
        size: impl Into<Size>,
        build: impl FnMut(&mut Ui, &mut Bridge<P>) -> El + Send + 'static,
    ) -> Self {
        let session = Session {
            bridge: Bridge::new(Arc::clone(&params)),
            build: Box::new(build),
        };
        Self {
            shared: Arc::new(Mutex::new(Shared { ui, view: session })),
            requests: Arc::default(),
            params,
            size: points(size.into()),
            min: None,
            system_scale: false,
            host_scale: None,
            window: None,
        }
    }

    /// Let the host resize the window, down to `min` logical points.
    pub fn resizable(mut self, min: impl Into<Size>) -> Self {
        self.min = Some(points(min.into()));
        self
    }

    /// This editor's host scale, else the last one any editor was given.
    fn host_scale(&self) -> Option<f64> {
        self.host_scale.or_else(last_host_scale)
    }

    fn close_window(&mut self) {
        if let Some(Handle(mut window)) = self.window.take() {
            window.close();
        }
    }
}

impl<P: Params> Editor for MuiEditor<P> {
    fn size(&self) -> (u32, u32) {
        self.size
    }

    fn can_resize(&self) -> bool {
        self.min.is_some()
    }

    fn min_size(&self) -> (u32, u32) {
        self.min.unwrap_or(self.size)
    }

    fn open(&mut self, parent: RawWindowHandle, context: PluginContext) {
        if self.window.is_some() {
            self.close();
        }
        lock(&self.shared)
            .view
            .bridge
            .attach(context.with_params(Arc::clone(&self.params)));
        // A request made while closed was for the last window.
        self.requests = Arc::default();
        // Linux: an embedded editor follows the host's scale, not the
        // desktop's, which a non-DPI-aware host does not share. Elsewhere
        // the OS reports a reliable per-window scale.
        let host_scale = self.host_scale();
        let scale = match editor_window_scale(
            self.system_scale,
            host_scale.is_some(),
            host_scale.unwrap_or(1.0),
        ) {
            Some(s) => baseview::WindowScalePolicy::ScaleFactor(s),
            None => baseview::WindowScalePolicy::SystemScaleFactor,
        };
        self.window = Some(Handle(window::open(
            &ParentWindow(parent),
            "MUI",
            self.size,
            scale,
            Arc::clone(&self.shared),
            Arc::clone(&self.requests),
        )));
    }

    fn close(&mut self) {
        {
            // Released before the window closes: on macOS and Windows
            // baseview tears the handler down on this thread, inside
            // `close`, and its last event would wait on this lock.
            let mut s = lock(&self.shared);
            s.view.bridge.close();
            // The bridge ended the host's gestures; these are the same edges.
            s.ui.close();
        }
        self.close_window();
    }

    fn set_size(&mut self, width: u32, height: u32) -> bool {
        match self.min {
            Some((w, h)) if width >= w && height >= h => {
                self.size = (width, height);
                self.requests.resize(width, height);
                true
            }
            _ => false,
        }
    }

    fn set_scale_factor(&mut self, factor: f64) {
        if factor.is_finite() && factor > 0.0 {
            self.host_scale = Some(factor);
            HOST_SCALE.store(factor.to_bits(), Ordering::Relaxed);
            self.requests.scale(factor);
        }
    }

    fn set_uses_system_scale(&mut self, yes: bool) {
        self.system_scale = yes;
    }

    fn state_changed(&mut self) {
        // A preset replaced what a gesture in flight was editing: end it,
        // and stop the drag so it cannot keep writing the old value.
        let mut s = lock(&self.shared);
        s.ui.cancel();
        s.view.bridge.end_all();
        self.requests.redraw();
    }
}

impl<P: Params> Drop for MuiEditor<P> {
    fn drop(&mut self) {
        // The host may have torn its side down already: no callbacks here.
        lock(&self.shared).view.bridge.detach();
        self.close_window();
    }
}

#[cfg(test)]
mod tests;
