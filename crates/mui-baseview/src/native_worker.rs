//! Input-only native window adapter. The worker owns no native handle or native context.
use super::*;
use mui::vello::host::{BitmapPatch as Patch, Frame, Host, HostError, Offscreen};
use policies::{PointerState, WheelState};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};

// A model may be reopened with a fresh Requests while its old worker exits.
// Sequence its workers without taking the model lock on the host thread.
static MODELS: std::sync::LazyLock<
    Mutex<std::collections::HashMap<usize, std::sync::Weak<AtomicBool>>>,
> = std::sync::LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));
struct ModelLease {
    key: usize,
    active: Arc<AtomicBool>,
}
impl ModelLease {
    fn new<V>(shared: &Arc<Mutex<Shared<V>>>) -> (Self, Option<Arc<AtomicBool>>) {
        let key = Arc::as_ptr(shared) as usize;
        let active = Arc::new(AtomicBool::new(true));
        let previous = MODELS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(key, Arc::downgrade(&active))
            .and_then(|weak| weak.upgrade());
        (Self { key, active }, previous)
    }
}
impl Drop for ModelLease {
    fn drop(&mut self) {
        self.active.store(false, Ordering::Release);
        let mut models = MODELS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if models
            .get(&self.key)
            .is_some_and(|weak| weak.ptr_eq(&Arc::downgrade(&self.active)))
        {
            models.remove(&self.key);
        }
    }
}

pub(super) fn build<V: View + Send + 'static>(
    shared: Arc<Mutex<Shared<V>>>,
    requests: Arc<Requests>,
    parented: bool,
) -> impl FnOnce(WindowContext) -> Result<NativeAdapter, HandlerError> + Send + 'static {
    move |cx| {
        if !baseview::pin_current_image_for_detached_work() {
            return Err(std::io::Error::other("cannot pin plugin image for editor worker").into());
        }
        // Bind inside construction, before a worker or reentrant callback can
        // publish work. Custom Window::create users need the same wake contract
        // as open/run; the weak waker never extends native window lifetime.
        let waker = cx.waker();
        requests.bind_requester(Some(baseview::FrameRequester::new(move || {
            waker.request_redraw();
        })));
        let start = requests
            .native_start
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let Some(start) = start {
            start();
        }
        let size = cx.size();
        let a11y_wake = Arc::downgrade(&requests);
        #[expect(
            unsafe_code,
            reason = "attach provider to builder's live HWND on its owning thread"
        )]
        // SAFETY: the native endpoint stays here and drops before cx.
        let endpoints = cx.window_handle().ok().and_then(|h| {
            // SAFETY: builder owns this HWND; native drops before cx.
            unsafe {
                NativeAccessibility::new_with_wake(
                    h.as_raw(),
                    Arc::new(move || {
                        if let Some(requests) = a11y_wake.upgrade() {
                            requests.redraw();
                        }
                    }),
                )
            }
        });
        let (mut native, accessibility) =
            endpoints.map_or((None, None), |(n, a)| (Some(n), Some(a)));
        if let Some(native) = &mut native {
            native.focus(cx.has_focus());
        }
        let mut presenter =
            mui::vello::software::BitmapWindow::new(cx.clone()).map_err(std::io::Error::other)?;
        presenter.set_scale_factor(size.scale_factor);
        let (send, receive) = mpsc::channel();
        let (publish, output) = mpsc::sync_channel(1);
        let stopped = Arc::new(AtomicBool::new(false));
        let tick_pending = Arc::new(AtomicBool::new(false));
        let stop = stopped.clone();
        let (lease, previous) = ModelLease::new(&shared);
        let generation = requests
            .render_generation
            .fetch_add(1, Ordering::AcqRel)
            .wrapping_add(1);
        let pending = tick_pending.clone();
        let wake = requests.clone();
        let mut drag = drag::Host::new(&requests);
        drag.parented = parented;
        requests.render_cancelled.store(false, Ordering::Release);
        requests.render_active.store(true, Ordering::Release);
        let spawned = std::thread::Builder::new()
            .name("mui-editor-render".into())
            .stack_size(8 * 1024 * 1024)
            .spawn(move || {
                while previous
                    .as_ref()
                    .is_some_and(|active| active.load(Ordering::Acquire))
                {
                    // Cancelled generations still keep their place in the
                    // chain until the predecessor exits; subsequent opens
                    // cannot jump past a stalled model owner.
                    std::thread::park_timeout(Duration::from_millis(1));
                }
                let start = wake
                    .render_start
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .take();
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    if stop.load(Ordering::Acquire) {
                        return;
                    }
                    if let Some(start) = start {
                        start();
                    }
                    worker(
                        &shared,
                        &wake,
                        size,
                        accessibility,
                        WorkerChannels {
                            commands: receive,
                            output: publish,
                            stopped: stop,
                            tick_pending: pending,
                        },
                    );
                }));
                if let Err(payload) = result {
                    mui::diagnostics::error(
                        "mui-baseview",
                        "render_worker_panic",
                        mui::diagnostics::panic_message(&*payload),
                    );
                }
                let stop = wake
                    .render_stop
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .take();
                if let Some(stop) = stop {
                    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(stop));
                }
                if wake.render_generation.load(Ordering::Acquire) == generation {
                    wake.render_active.store(false, Ordering::Release);
                }
                drop(lease);
            });
        if let Err(error) = spawned {
            requests.render_active.store(false, Ordering::Release);
            return Err(error.into());
        }
        #[cfg(target_os = "linux")]
        let x11 = policies::X11Lifetime::new(
            requests.clone(),
            cx.window_handle()
                .ok()
                .map_or(0, |handle| match handle.as_raw() {
                    raw_window_handle::RawWindowHandle::Xlib(h) => {
                        u32::try_from(h.window).unwrap_or(0)
                    }
                    raw_window_handle::RawWindowHandle::Xcb(h) => h.window.get(),
                    _ => 0,
                }),
        );
        let wheel_policy = *requests
            .wheel
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let hook = requests
            .close
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        Ok(NativeAdapter {
            close: NativeClose {
                hook,
                fired: Cell::new(false),
            },
            requests,
            native: RefCell::new(native),
            cx,
            send,
            output: RefCell::new(output),
            stopped,
            tick_pending,
            drag: RefCell::new(drag),
            demand: Cell::new(baseview::FrameDemand::Continuous),
            capture: Cell::new(false),
            ime: RefCell::new(None),
            cursor: Cell::new(None),
            pointer: RefCell::new(PointerState::default()),
            wheel: RefCell::new(WheelState::default()),
            wheel_policy,
            #[cfg(target_os = "linux")]
            x11,
            bitmap: RefCell::new(None),
            expose: RefCell::new(Vec::new()),
            input_pending: Cell::new(true),
            flash_at: Cell::new(None),
            flash_rects: RefCell::new(Vec::new()),
            direct: Cell::new(false),
            layer: RefCell::new(crate::surface::Layer::default()),
            presenter: RefCell::new(Some(presenter)),
        })
    }
}

enum Command {
    Event(Event, u64),
    KeyReservation(u64, Option<KeyHook>),
    Resize(WindowSize),
    Tick(Option<(u64, u64)>),
    Invalidate,
    /// The window thread's surface for the worker to present to, made from
    /// the instance the worker asked with; `None` if it could not be made.
    Surface(Option<mui_hal::Surface<'static>>),
    Stop,
}
struct Output {
    patches: Vec<Patch>,
    size: (u32, u32),
    /// Backing pixel dimensions of `patches`: equals `size` within the 16 Mi
    /// pixel 1:1 budget, or a proportionally scaled target above it.
    render_size: (u32, u32),
    /// Graphics API that rendered this frame.
    api: mui_present::GraphicsApi,
    /// The direct host's surface composites alpha (selects the D3D11 backend).
    translucent: bool,
    /// Why this frame is not a GPU one.
    fallback: Option<mui_present::FallbackReason>,
    reason: &'static str,
    capture: bool,
    ime: Option<baseview::ImeConfiguration>,
    cursor: MouseCursor,
    hide_pointer: bool,
    input_revision: u64,
    accessibility: Option<mui_access::accesskit::TreeUpdate>,
    demand: baseview::FrameDemand,
    drag: Option<(u64, u64)>,
    /// Make a surface on the window from this instance and send it back.
    surface: Option<mui_hal::Instance>,
    /// The worker presents to the window itself: no patches, no blit, and
    /// an expose is a repaint there.
    direct: bool,
}
struct Bitmap {
    size: (u32, u32),
    pixels: Vec<u32>,
}

pub(super) struct NativeAdapter {
    close: NativeClose,
    requests: Arc<Requests>,
    native: RefCell<Option<NativeAccessibility>>,
    /// Owned `mui-present` CPU presenter (dropped before `cx` and detached when
    /// handing the native window to a direct GPU swapchain).
    presenter: RefCell<Option<mui::vello::software::BitmapWindow<WindowContext>>>,
    cx: WindowContext,
    send: Sender<Command>,
    output: RefCell<Receiver<Output>>,
    stopped: Arc<AtomicBool>,
    tick_pending: Arc<AtomicBool>,
    drag: RefCell<drag::Host>,
    demand: Cell<baseview::FrameDemand>,
    capture: Cell<bool>,
    ime: RefCell<Option<Option<baseview::ImeConfiguration>>>,
    cursor: Cell<Option<MouseCursor>>,
    pointer: RefCell<PointerState>,
    wheel: RefCell<WheelState>,
    wheel_policy: WheelPolicy,
    #[cfg(target_os = "linux")]
    x11: policies::X11Lifetime,
    bitmap: RefCell<Option<Bitmap>>,
    expose: RefCell<Vec<[u32; 4]>>,
    input_pending: Cell<bool>,
    flash_at: Cell<Option<Instant>>,
    flash_rects: RefCell<Vec<[u32; 4]>>,
    /// The worker presents to the window: an expose asks it to repaint.
    direct: Cell<bool>,
    /// The layer of the GPU surface lent to the worker. The surface retains
    /// its window handle and layer, so closing never waits for the worker.
    layer: RefCell<crate::surface::Layer>,
}
impl NativeAdapter {
    fn blit(&self, rect: [u32; 4], pixels: &[u32]) -> bool {
        let decorated = mui::vello::software::damage_flash(pixels, (rect[2], rect[3]));
        if decorated.is_some() {
            self.flash_at
                .set(Some(Instant::now() + Duration::from_millis(120)));
            let mut rects = self.flash_rects.borrow_mut();
            if !rects.contains(&rect) {
                rects.push(rect);
            }
            if rects.len() > 64 {
                let (mut x, mut y, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
                for r in &*rects {
                    x = x.min(r[0]);
                    y = y.min(r[1]);
                    x1 = x1.max(r[0] + r[2]);
                    y1 = y1.max(r[1] + r[3]);
                }
                *rects = vec![[x, y, x1 - x, y1 - y]];
            }
        }
        self.blit_raw(rect, decorated.as_deref().unwrap_or(pixels))
    }
    fn ensure_presenter(&self) -> Result<(), String> {
        let mut slot = self.presenter.borrow_mut();
        if slot.is_none() {
            let mut presenter = mui::vello::software::BitmapWindow::new(self.cx.clone())?;
            presenter.set_scale_factor(self.cx.size().scale_factor);
            *slot = Some(presenter);
        }
        Ok(())
    }
    fn cpu_backend(&self) -> mui_present::PresentationBackend {
        self.presenter.borrow().as_ref().map_or(
            mui_present::PresentationBackend::Offscreen,
            mui::vello::software::BitmapWindow::backend,
        )
    }
    fn blit_raw(&self, rect: [u32; 4], pixels: &[u32]) -> bool {
        if let Err(error) = self.ensure_presenter() {
            mui::diagnostics::error("mui-baseview", "native_bitmap", &error);
            return false;
        }
        let size = self.cx.size();
        let mut slot = self.presenter.borrow_mut();
        let presenter = slot.as_mut().expect("presenter ensured above");
        presenter.set_scale_factor(size.scale_factor);
        let result =
            presenter.present_patch(rect, pixels, (size.physical.width, size.physical.height));
        if let Err(error) = result {
            mui::diagnostics::error("mui-baseview", "native_bitmap", &error);
            return false;
        }
        true
    }
    fn blit_scaled(&self, src_size: (u32, u32), pixels: &[u32], dst_size: (u32, u32)) -> bool {
        if let Err(error) = self.ensure_presenter() {
            mui::diagnostics::error("mui-baseview", "native_bitmap", &error);
            return false;
        }
        let mut slot = self.presenter.borrow_mut();
        let presenter = slot.as_mut().expect("presenter ensured above");
        presenter.set_scale_factor(self.cx.size().scale_factor);
        let result = presenter.present_scaled(src_size, pixels, dst_size);
        if let Err(error) = result {
            mui::diagnostics::error("mui-baseview", "native_bitmap", &error);
            return false;
        }
        true
    }
    fn stop(&self) {
        if !self.stopped.swap(true, Ordering::AcqRel) {
            self.requests
                .render_cancelled
                .store(true, Ordering::Release);
            self.close.fire();
            if let Ok(mut native) = self.native.try_borrow_mut() {
                native.take();
            }
            if let Ok(mut drag) = self.drag.try_borrow_mut() {
                drag.close(&self.requests.drag);
            }
            let _ = self.send.send(Command::Stop);
            self.requests.bind_requester(None);
            #[cfg(target_os = "linux")]
            self.x11.cancel();
            // A worker blocked handing over a frame sees the stop now.
            if let Ok(output) = self.output.try_borrow() {
                while output.try_recv().is_ok() {}
            }
        }
    }
    fn send(&self, command: Command) {
        if !self.stopped.load(Ordering::Acquire) {
            if matches!(
                command,
                Command::Event(_, _) | Command::KeyReservation(_, _) | Command::Resize(_)
            ) {
                self.input_pending.set(true);
            }
            let _ = self.send.send(command);
        }
    }
    fn native_requests(&self) {
        #[cfg(feature = "test-support")]
        {
            let events = {
                let mut queued = self
                    .requests
                    .test_events
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                std::mem::take(&mut *queued)
            };
            for event in events {
                let _ = self.on_event(event);
            }
        }
        if self.requests.focus.swap(false, Ordering::AcqRel) && !self.cx.has_focus() {
            let _ = self.cx.focus();
        }
        let packed = self.requests.size.swap(0, Ordering::AcqRel);
        let bits = self.requests.scale.swap(0, Ordering::AcqRel);
        if packed != 0 || bits != 0 {
            let logical = if packed == 0 {
                self.cx.size().logical
            } else {
                LogicalSize::new((packed >> 32) as f64, (packed & u64::from(u32::MAX)) as f64)
            };
            if bits != 0 {
                let _ = self
                    .cx
                    .set_scale_factor_override(Some(f64::from_bits(bits)));
            }
            let _ = self.cx.resize(logical);
            self.send(Command::Resize(self.cx.size()));
        }
    }
    fn apply(&self, output: Output) {
        self.demand.set(output.demand);
        self.capture.set(output.capture);
        self.cx
            .set_keyboard_capture(output.capture || self.requests.offers_keys());
        if self.ime.borrow().as_ref() != Some(&output.ime) {
            self.cx.set_ime_configuration(output.ime.clone());
            *self.ime.borrow_mut() = Some(output.ime);
        }
        let (restore, hidden) = {
            let mut pointer = self.pointer.borrow_mut();
            let restore = pointer.apply(output.input_revision, output.hide_pointer);
            (restore, pointer.hidden())
        };
        if let Some(at) = restore {
            platform::warp_pointer(&self.cx, at.x, at.y, self.cx.size().scale_factor);
        }
        let cursor = if hidden {
            MouseCursor::Hidden
        } else {
            output.cursor
        };
        if self.cursor.replace(Some(cursor)) != Some(cursor) {
            let _ = self.cx.set_mouse_cursor(cursor);
        }
        // Native publication may reenter, but never acquires the model lock.
        if let Some(update) = output.accessibility
            && let Some(native) = self.native.borrow_mut().as_mut()
        {
            native.publish(update);
        }
        if let Some(instance) = output.surface {
            // Detach the CPU presenter before handing the native window to GPU
            // (drops macOS IOSurface CALayer, Win32 WM_PAINT subclass, and
            // Linux Wayland wl_shm child subsurface cleanly with zero wait).
            drop(self.presenter.borrow_mut().take());
            // Made here, on the window's thread (AppKit allows nothing else),
            // and presented from the worker.
            let logical = self.cx.size().logical;
            let surface = crate::surface::create(
                &instance,
                self.cx.platform_handle(),
                (logical.width.round() as u32, logical.height.round() as u32),
            )
            .map_err(|error| mui::diagnostics::error("mui-baseview", "native_surface", &error))
            .ok()
            .map(|(surface, layer)| {
                *self.layer.borrow_mut() = layer;
                surface
            });
            self.send(Command::Surface(surface));
        }
        self.direct.set(output.direct);
        let actual = self.cx.size();
        if output.size != (actual.physical.width, actual.physical.height) {
            self.send(Command::Invalidate);
            self.input_pending.set(true);
            self.requests.wake();
            return;
        }
        if output.direct {
            // On screen already: the worker presented it.
            *self.bitmap.borrow_mut() = None;
            self.requests
                .presented_gpu(output.size, output.api, output.translucent, output.reason);
            self.requests.drag.rendered(output.drag);
            return;
        }
        let mut bitmap = self.bitmap.borrow_mut();
        let blitting = std::time::Instant::now();
        let scaled = output.render_size != output.size;
        let submitted = submit_patches(
            &self.requests,
            output.size,
            output.render_size,
            self.cpu_backend(),
            output.api,
            output.fallback,
            output.reason,
            output.patches,
            |patch| {
                if patch.rect == [0, 0, output.render_size.0, output.render_size.1] {
                    let success = if scaled {
                        self.blit_scaled(output.render_size, &patch.pixels, output.size)
                    } else {
                        self.blit(patch.rect, &patch.pixels)
                    };
                    *bitmap = Some(Bitmap {
                        size: output.render_size,
                        pixels: patch.pixels,
                    });
                    return success;
                }
                let Some(bitmap) = bitmap.as_mut().filter(|b| b.size == output.render_size) else {
                    return false;
                };
                let [x, y, w, h] = patch.rect;
                for row in 0..h as usize {
                    let dest = (y as usize + row) * output.render_size.0 as usize + x as usize;
                    let source = row * w as usize;
                    bitmap.pixels[dest..dest + w as usize]
                        .copy_from_slice(&patch.pixels[source..source + w as usize]);
                }
                if scaled {
                    self.blit_scaled(output.render_size, &bitmap.pixels, output.size)
                } else {
                    self.blit(patch.rect, &patch.pixels)
                }
            },
        );
        crate::timing::note("blit ms", blitting.elapsed().as_secs_f64() * 1e3);
        if !submitted {
            self.send(Command::Invalidate);
            self.input_pending.set(true);
            self.requests.wake();
            return;
        }
        self.requests.drag.rendered(output.drag);
    }
}
#[expect(
    clippy::too_many_arguments,
    reason = "records both legacy Presentation and full PresentationState atomically"
)]
fn submit_patches(
    requests: &Requests,
    size: (u32, u32),
    render_size: (u32, u32),
    backend: mui_present::PresentationBackend,
    api: mui_present::GraphicsApi,
    fallback: Option<mui_present::FallbackReason>,
    reason: &'static str,
    patches: Vec<Patch>,
    mut submit: impl FnMut(Patch) -> bool,
) -> bool {
    if patches.is_empty() {
        return true;
    }
    if !patches
        .into_iter()
        .all(|patch| patch.rect[2] > 0 && patch.rect[3] > 0 && submit(patch))
    {
        return false;
    }
    let software = api == mui_present::GraphicsApi::Software;
    let first_gpu = !software && requests.presentation().is_none_or(|old| old.software);
    let mode = if !software {
        mui_present::PresentationMode::ReadbackGpu
    } else if render_size != size {
        mui_present::PresentationMode::CpuScaled
    } else {
        mui_present::PresentationMode::CpuDirect
    };
    // A readback frame is blitted by the CPU path, never alpha-composited.
    requests.presented_with(
        size,
        render_size,
        mode,
        backend,
        api,
        false,
        fallback,
        reason,
    );
    if first_gpu {
        mui::diagnostics::breadcrumb(
            "mui-baseview",
            "gpu_frame_presented",
            "native GPU-rendered bitmap presentation succeeded",
        );
    }
    true
}
impl Drop for NativeAdapter {
    fn drop(&mut self) {
        self.stop();
    }
}
impl WindowHandler for NativeAdapter {
    fn captures_key(&self, key: &KeyboardEvent) -> bool {
        self.capture.get() || self.requests.claims_key(&key_event(key))
    }
    fn frame_demand(&self) -> baseview::FrameDemand {
        if self.stopped.load(Ordering::Acquire) {
            return baseview::FrameDemand::Idle;
        }
        // Input can arrive after on_frame queued its tick, including from a
        // reentrant callback. Keep those FIFO edges due until a later tick
        // consumes them, while parking if that tick is already queued.
        if (self.input_pending.get() || self.requests.redraw.load(Ordering::Acquire))
            && !self.tick_pending.load(Ordering::Acquire)
        {
            return baseview::FrameDemand::At(Instant::now());
        }
        match (self.demand.get(), self.flash_at.get()) {
            (baseview::FrameDemand::Idle, Some(at)) => baseview::FrameDemand::At(at),
            (baseview::FrameDemand::At(at), Some(flash)) => {
                baseview::FrameDemand::At(at.min(flash))
            }
            (demand, _) => demand,
        }
    }
    fn on_frame(&self) -> Result<(), HandlerError> {
        if self.stopped.load(Ordering::Acquire) {
            return Ok(());
        }
        if self.flash_at.get().is_some_and(|at| Instant::now() >= at) {
            self.flash_at.set(None);
            let rects = std::mem::take(&mut *self.flash_rects.borrow_mut());
            if let Some(bitmap) = self.bitmap.borrow().as_ref() {
                for [x, y, w, h] in rects {
                    if x + w > bitmap.size.0 || y + h > bitmap.size.1 {
                        continue;
                    }
                    let mut pixels = Vec::with_capacity(w as usize * h as usize);
                    for row in y..y + h {
                        let start = row as usize * bitmap.size.0 as usize + x as usize;
                        pixels.extend_from_slice(&bitmap.pixels[start..start + w as usize]);
                    }
                    let _ = self.blit_raw([x, y, w, h], &pixels);
                }
            }
        }
        self.native_requests();
        let clipboard = self
            .requests
            .clipboard_write
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let Some(text) = clipboard {
            baseview::copy_to_clipboard(&text);
        }
        #[cfg(target_os = "linux")]
        if let (Some(native), Ok(display), Ok(handle)) = (
            self.native.borrow_mut().as_mut(),
            self.cx.display_handle(),
            self.cx.window_handle(),
        ) {
            // SAFETY: both handles borrow this live window on its native thread.
            #[expect(unsafe_code, reason = "native bounds query on owning thread")]
            unsafe {
                native.update_bounds(display.as_raw(), handle.as_raw());
            }
        }
        // Take each message outside the receiver borrow before native callbacks.
        loop {
            let output = self.output.borrow().try_recv().ok();
            let Some(output) = output else { break };
            self.apply(output);
        }
        let exposed = std::mem::take(&mut *self.expose.borrow_mut());
        if !exposed.is_empty()
            && let Some(bitmap) = self.bitmap.borrow().as_ref()
        {
            let actual = self.cx.size().physical;
            let actual_size = (actual.width, actual.height);
            if bitmap.size == actual_size {
                for [x, y, w, h] in exposed {
                    let x = x.min(bitmap.size.0);
                    let y = y.min(bitmap.size.1);
                    let w = w.min(bitmap.size.0 - x);
                    let h = h.min(bitmap.size.1 - y);
                    if [x, y, w, h] == [0, 0, bitmap.size.0, bitmap.size.1] {
                        self.blit([x, y, w, h], &bitmap.pixels);
                    } else if w > 0 && h > 0 {
                        let mut pixels = Vec::with_capacity(w as usize * h as usize);
                        for row in y..y + h {
                            let start = row as usize * bitmap.size.0 as usize + x as usize;
                            pixels.extend_from_slice(&bitmap.pixels[start..start + w as usize]);
                        }
                        self.blit([x, y, w, h], &pixels);
                    }
                }
            } else if actual_size.0 > 0 && actual_size.1 > 0 {
                self.blit_scaled(bitmap.size, &bitmap.pixels, actual_size);
            }
        }
        let due = match self.demand.get() {
            baseview::FrameDemand::Continuous => true,
            baseview::FrameDemand::At(at) => Instant::now() >= at,
            _ => false,
        };
        if (due || self.input_pending.get() || self.requests.redraw.load(Ordering::Acquire))
            && !self.tick_pending.swap(true, Ordering::AcqRel)
        {
            self.input_pending.set(false);
            // The worker publishes the next deadline with its result. Leaving
            // an expired demand armed here would wake the native thread at
            // every refresh while a slow build or GPU completion is pending.
            self.demand.set(baseview::FrameDemand::Idle);
            let drag = self.drag.borrow().frame_token(&self.requests.drag);
            self.send(Command::Tick(drag));
        }
        if self.drag.borrow_mut().drive(&self.requests, &self.cx) {
            self.on_event(Event::Window(WindowEvent::Unfocused));
        }
        self.requests.idle();
        Ok(())
    }
    fn damage(&self, area: baseview::DamageArea<'_>) {
        if self.direct.get() {
            // No bitmap here to show again: the worker repaints the surface.
            self.send(Command::Invalidate);
            self.input_pending.set(true);
            return;
        }
        let mut rects = self.expose.borrow_mut();
        let mut add = |r: baseview::DamageRect| {
            let position = r.position();
            let size = r.size();
            rects.push([position.x, position.y, size.width, size.height]);
        };
        match area {
            baseview::DamageArea::Rect(rect) => add(rect),
            baseview::DamageArea::Rects(areas) => {
                for &area in areas {
                    add(area);
                }
            }
            _ => {
                let size = self.cx.size().physical;
                rects.push([0, 0, size.width, size.height]);
            }
        }
    }
    fn resized(&self, size: WindowSize) -> Result<(), HandlerError> {
        self.layer.borrow().resize((
            size.logical.width.round() as u32,
            size.logical.height.round() as u32,
        ));
        self.send(Command::Resize(size));
        self.requests.wake();
        Ok(())
    }
    fn on_event(&self, mut event: Event) -> EventStatus {
        if let Event::KeyboardCapture { key, captured } = &event {
            self.send(Command::KeyReservation(
                key_event(key).code,
                self.requests.reserved_key_hook(*captured),
            ));
            self.requests.wake();
            return EventStatus::Captured;
        }
        if !matches!(event, Event::Window(WindowEvent::RedrawRequested)) {
            self.pointer.borrow_mut().observe(&event);
        }
        if matches!(
            event,
            Event::Mouse(MouseEvent::CursorLeft | MouseEvent::DragLeft)
                | Event::Window(WindowEvent::Unfocused | WindowEvent::WillClose)
        ) && self.cursor.get() == Some(MouseCursor::Hidden)
        {
            self.cursor.set(Some(MouseCursor::Default));
            let _ = self.cx.set_mouse_cursor(MouseCursor::Default);
        }
        if let Event::Mouse(MouseEvent::WheelScrolled { delta, .. }) = &mut event {
            *delta = self
                .wheel
                .borrow_mut()
                .apply(*delta, self.wheel_policy, Instant::now());
        }
        if matches!(event, Event::Window(WindowEvent::WillClose)) {
            self.stop();
            return EventStatus::Captured;
        }
        if let Ok(drag) = self.drag.try_borrow() {
            drag.observe(&self.requests.drag, &event);
        }
        if matches!(event, Event::Window(WindowEvent::RedrawRequested)) {
            if self.expose.borrow().is_empty() {
                let size = self.cx.size().physical;
                self.expose
                    .borrow_mut()
                    .push([0, 0, size.width, size.height]);
            }
        } else {
            self.send(Command::Event(
                event.clone(),
                self.pointer.borrow().revision,
            ));
        }
        self.requests.wake();
        match event {
            Event::Keyboard(ref key)
                if !self.capture.get() && !self.requests.claims_key(&key_event(key)) =>
            {
                EventStatus::Ignored
            }
            Event::Mouse(
                MouseEvent::DragEntered { ref data, .. }
                | MouseEvent::DragMoved { ref data, .. }
                | MouseEvent::DragDropped { ref data, .. },
            ) if matches!(data, DropData::Files(_)) => EventStatus::AcceptDrop(DropEffect::Copy),
            _ => EventStatus::Captured,
        }
    }
}

struct WorkerChannels {
    commands: Receiver<Command>,
    output: SyncSender<Output>,
    stopped: Arc<AtomicBool>,
    tick_pending: Arc<AtomicBool>,
}

fn worker<V: View>(
    shared: &Arc<Mutex<Shared<V>>>,
    requests: &Arc<Requests>,
    size: WindowSize,
    accessibility: Option<AccessibilityUi>,
    channels: WorkerChannels,
) {
    worker_with_mode(
        shared,
        requests,
        size,
        accessibility,
        channels,
        mui::vello::host::force_cpu(),
    );
}

fn worker_with_mode<V: View>(
    shared: &Arc<Mutex<Shared<V>>>,
    requests: &Arc<Requests>,
    size: WindowSize,
    mut accessibility: Option<AccessibilityUi>,
    channels: WorkerChannels,
    software_only: bool,
) {
    let WorkerChannels {
        commands,
        output,
        stopped,
        tick_pending,
    } = channels;
    let mut driver = Driver::new(
        (size.physical.width, size.physical.height),
        size.scale_factor,
        Box::new(WorkerClipboard {
            #[cfg(not(target_os = "linux"))]
            requests: Arc::clone(requests),
            clipboard: Clipboard::default(),
        }),
    );
    driver.display_paced = true;
    let mut scale = size.scale_factor;
    let mut input_revision = 0;
    let mut key_routes = KeyRoutes::new();
    // The window's own surface, presented from here: the default. Offscreen
    // targets read back to the window thread only where a direct surface is
    // unavailable or asked against (`MUI_PRESENT=readback`).
    let mut direct = None::<Host>;
    let mut surface = None::<mui_hal::Surface<'static>>;
    let mut asked = false;
    let direct_wanted = presents_directly();
    let mut gpu = None::<Offscreen>;
    let mut init = (!software_only).then(|| GpuInit::new(true));
    let mut reason = if software_only {
        "CPU forced"
    } else {
        "CPU startup"
    };
    let mut fallback = software_only.then_some(mui_present::FallbackReason::ForceCpu);
    let mut cpu = None::<mui::vello::software::Renderer>;
    let mut retry = Retry::default();
    let mut unpainted = true;
    lock(shared).ui.set_gpu_welding_available(false);
    let mut scheduled_poll = None;
    loop {
        let command = match commands.recv_timeout(Duration::from_millis(250)) {
            Ok(command) => command,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if stopped.load(Ordering::Acquire) {
                    break;
                }
                let poll = requests.poll_interval();
                if scheduled_poll != poll {
                    scheduled_poll = poll;
                    // Ask the native pump for a tick so it can replace its
                    // cached demand. The audio thread only published a value.
                    requests.redraw();
                }
                continue;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };
        if stopped.load(Ordering::Acquire) || matches!(command, Command::Stop) {
            break;
        }
        let drag = match command {
            Command::Resize(size) => {
                scale = size.scale_factor;
                driver.resized((size.physical.width, size.physical.height), scale);
                continue;
            }
            Command::KeyReservation(code, hook) => {
                key_routes.insert(code, hook);
                continue;
            }
            Command::Event(event, revision) => {
                input_revision = revision;
                dispatch(&mut driver, shared, requests, &mut key_routes, scale, event);
                continue;
            }
            Command::Tick(drag) => {
                tick_pending.store(false, Ordering::Release);
                drag
            }
            Command::Surface(made) => {
                surface = made;
                if surface.is_none() {
                    // The window could not make one: read back instead.
                    asked = false;
                    if let Some(init) = &mut init {
                        init.recover(FailureClass::Transient);
                    }
                }
                continue;
            }
            Command::Invalidate => {
                driver.redraw();
                if let Some(direct) = &mut direct {
                    direct.invalidate();
                }
                if let Some(gpu) = &mut gpu {
                    gpu.invalidate();
                }
                if let Some(cpu) = &mut cpu {
                    cpu.invalidate();
                }
                unpainted = true;
                continue;
            }
            Command::Stop => break,
        };
        loop {
            let task = requests
                .render_work
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .pop_front();
            let Some(task) = task else { break };
            task();
        }
        let now = Instant::now();
        let size = driver.size();
        let has_area = size.0 > 0 && size.1 > 0;
        let cpu_plan = cpu_render_plan(size, CPU_PIXEL_BUDGET);
        let mut surface_request = None;
        if has_area
            && direct_wanted
            && direct.is_none()
            && gpu.is_none()
            && let Some(init) = init.as_mut()
        {
            if surface.is_some() {
                let made = init.poll(size, |_| {
                    surface.take().ok_or_else(|| "no window surface".to_owned())
                });
                match made {
                    Some(Ok(host)) => {
                        direct = Some(host);
                        reason = "GPU ready";
                        fallback = None;
                        lock(shared).ui.set_gpu_welding_available(true);
                        driver.redraw();
                    }
                    Some(Err(error)) => {
                        surface = None;
                        asked = false;
                        fallback = Some(error.fallback_reason());
                        if reason != "CPU GPU initialization failed" {
                            log(
                                shared,
                                &format!(
                                    "mui-baseview: GPU unavailable ({error}); using CPU rendering"
                                ),
                            );
                        }
                        reason = "CPU GPU initialization failed";
                    }
                    None => {}
                }
            } else if !asked {
                surface_request = init.ready_instance();
                asked = surface_request.is_some();
            }
        }
        if let Some(plan) = cpu_plan
            && !direct_wanted
            && gpu.is_none()
            && let Some(result) = init.as_mut().and_then(|i| i.poll_offscreen(plan.target))
        {
            match result {
                Ok(renderer) => {
                    gpu = Some(renderer);
                    reason = "GPU ready";
                    fallback = None;
                    lock(shared).ui.set_gpu_welding_available(true);
                    driver.redraw();
                }
                Err(error) => {
                    fallback = Some(error.fallback_reason());
                    if reason != "CPU GPU initialization failed" {
                        log(
                            shared,
                            &format!(
                                "mui-baseview: GPU unavailable ({error}); using CPU rendering"
                            ),
                        );
                    }
                    reason = "CPU GPU initialization failed";
                }
            }
        }
        if requests.redraw.swap(false, Ordering::AcqRel) {
            driver.redraw();
        }
        let pointer_hook = requests
            .pointer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let (scene, capture, ime, update, hide_pointer) = {
            let mut shared = lock(shared);
            if let Some(a) = &mut accessibility {
                let requested = a.wants_tree();
                if a.apply(&mut shared.ui) || requested {
                    driver.redraw();
                }
            }
            let fresh = crate::timing::time("advance ms", || driver.advance(&mut shared, now));
            unpainted |= fresh;
            let ime = driver
                .ime_configuration(&shared.ui)
                .map(|c| native_ime(c, driver.ui_scale()));
            let update = accessibility.as_mut().and_then(|a| {
                if fresh || a.wants_tree() {
                    a.prepare(&shared.ui)
                } else {
                    None
                }
            });
            (
                if unpainted && has_area {
                    crate::timing::time("snapshot ms", || shared.ui.scene_snapshot())
                } else {
                    None
                },
                shared.ui.focus_is_text(),
                ime,
                update,
                pointer_hook.is_some_and(|hook| {
                    hook.lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)(
                        &shared.ui
                    )
                }),
            )
        };
        let mut patches = Vec::new();
        let mut render_size = size;
        if let Some(scene) = scene {
            let transform = Affine::scale(driver.ui_scale());
            let drawn = render_gpu(&mut direct, |host| {
                crate::timing::time("gpu render+present ms", || {
                    host.resize(size.0, size.1)?;
                    host.present(&scene, transform)
                })
            });
            if let Some(result) = drawn {
                match result {
                    Ok(Frame::Presented(_) | Frame::Current) => unpainted = false,
                    Ok(Frame::Skipped) => driver.redraw(),
                    Ok(Frame::SurfaceLost) => {
                        // A new surface from the window, then paint again.
                        fallback = Some(mui_present::FallbackReason::SurfaceLost);
                        direct = None;
                        asked = false;
                        if let Some(init) = &mut init {
                            init.recover(FailureClass::Transient);
                        }
                        unpainted = true;
                        driver.redraw();
                    }
                    Err(error) => {
                        mui::diagnostics::error(
                            "mui-baseview",
                            "worker_gpu_fault",
                            &error.to_string(),
                        );
                        reason = "CPU GPU recovery";
                        fallback = Some(error.fallback_reason());
                        if let Some(cpu) = &mut cpu {
                            cpu.invalidate();
                        }
                        unpainted = true;
                        asked = false;
                        if let Some(init) = &mut init {
                            init.recover(error.failure_class());
                        }
                        lock(shared).ui.set_gpu_welding_available(false);
                        driver.redraw();
                    }
                }
            } else if let Some(plan) = cpu_plan
                && let Some(result) = render_gpu(&mut gpu, |renderer| {
                    let scaled = plan.transform(driver.ui_scale());
                    crate::timing::time("gpu render+readback ms", || {
                        renderer.render(&scene, scaled, plan.target)
                    })
                })
            {
                render_size = plan.target;
                match result {
                    Ok(damage) => {
                        patches = damage;
                        unpainted = false;
                    }
                    Err(error) => {
                        let class = error.failure_class();
                        mui::diagnostics::error(
                            "mui-baseview",
                            "worker_gpu_fault",
                            &error.to_string(),
                        );
                        reason = "CPU GPU recovery";
                        fallback = Some(error.fallback_reason());
                        // Its retained bitmap predates the GPU frames now on
                        // screen. A presenter change needs one complete repaint.
                        if let Some(cpu) = &mut cpu {
                            cpu.invalidate();
                        }
                        unpainted = true;
                        if let Some(init) = &mut init {
                            init.recover(class);
                        }
                        lock(shared).ui.set_gpu_welding_available(false);
                        driver.redraw();
                    }
                }
            } else if let Some(plan) = cpu_plan
                && retry.ready(now)
            {
                render_size = plan.target;
                let scaled = plan.transform(driver.ui_scale());
                let result = (|| {
                    if cpu.is_none() {
                        cpu = Some(mui::vello::software::Renderer::new(plan.target)?);
                    }
                    let cpu = cpu.as_mut().expect("created above");
                    cpu.resize(plan.target)?;
                    let rects =
                        crate::timing::time("cpu render ms", || cpu.render_damage(&scene, scaled))?;
                    Ok::<_, String>(
                        rects
                            .into_iter()
                            .map(|rect @ [x, y, w, h]| {
                                let mut pixels = Vec::with_capacity(w as usize * h as usize);
                                for row in y..y + h {
                                    let start =
                                        (row as usize * plan.target.0 as usize + x as usize) * 4;
                                    pixels.extend(
                                        cpu.pixels()[start..start + w as usize * 4]
                                            .as_chunks::<4>()
                                            .0
                                            .iter()
                                            .map(|rgba| {
                                                (u32::from(rgba[0]) << 16)
                                                    | (u32::from(rgba[1]) << 8)
                                                    | u32::from(rgba[2])
                                            }),
                                    );
                                }
                                Patch { rect, pixels }
                            })
                            .collect::<Vec<_>>(),
                    )
                })();
                match result {
                    Ok(damage) => {
                        patches = damage;
                        unpainted = false;
                        retry.reset();
                    }
                    Err(error) => {
                        mui::diagnostics::error("mui-baseview", "worker_cpu_fault", &error);
                        retry.fail(now, FailureClass::Transient);
                        driver.redraw();
                    }
                }
            }
        }
        if !patches.is_empty() {
            let px: u64 = (patches.iter())
                .map(|p| u64::from(p.rect[2]) * u64::from(p.rect[3]))
                .sum();
            crate::timing::note(
                "damage % of window",
                px as f64 * 100.
                    / (u64::from(render_size.0) * u64::from(render_size.1)).max(1) as f64,
            );
        }
        if stopped.load(Ordering::Acquire) {
            break;
        }
        scheduled_poll = requests.poll_interval();
        let demand = worker_frame_demand(
            now,
            scheduled_poll,
            driver.next_wake(),
            direct.is_some() || gpu.is_some(),
            init.as_ref().is_some_and(GpuInit::pending),
            init.as_ref().and_then(GpuInit::deadline),
            retry.deadline(),
            unpainted && has_area,
        );
        if output
            .send(Output {
                patches,
                size,
                render_size,
                api: direct
                    .as_ref()
                    .map(Host::api)
                    .or_else(|| gpu.as_ref().map(Offscreen::api))
                    .unwrap_or_default(),
                translucent: direct.as_ref().is_some_and(Host::translucent),
                fallback,
                reason,
                capture,
                ime,
                cursor: native_cursor(driver.cursor()),
                hide_pointer,
                input_revision,
                accessibility: update,
                demand,
                drag,
                surface: surface_request,
                direct: direct.is_some(),
            })
            .is_err()
        {
            break;
        }
        requests.wake();
    }
    // No HWND or host callbacks here. The owner revokes host delivery first.
    // The window is about to go: give up its surface before anything slow.
    drop((direct, surface));
    driver.close(&mut lock(shared));
}

/// Pixel budget for 1:1 CPU / offscreen-readback raster targets (64 MiB RGBA8).
/// Direct GPU surfaces render at full native physical resolution; CPU and
/// offscreen readback scale proportionally above this cap instead of stopping.
const CPU_PIXEL_BUDGET: u64 = 16 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq)]
struct RenderPlan {
    target: (u32, u32),
    scale_x: f64,
    scale_y: f64,
}

impl RenderPlan {
    fn transform(self, ui_scale: f64) -> Affine {
        if (self.scale_x - 1.0).abs() == 0.0 && (self.scale_y - 1.0).abs() == 0.0 {
            Affine::scale(ui_scale)
        } else {
            Affine::scale_non_uniform(ui_scale * self.scale_x, ui_scale * self.scale_y)
        }
    }
}

/// Choose a CPU / offscreen-readback target size within `max_pixels` and
/// `u16::MAX` per axis. Returns `None` only for empty windows; large HiDPI or
/// ultrawide surfaces scale proportionally rather than ceasing to paint.
fn cpu_render_plan(window: (u32, u32), max_pixels: u64) -> Option<RenderPlan> {
    if window.0 == 0 || window.1 == 0 || max_pixels == 0 {
        return None;
    }
    let max_dim = u32::from(u16::MAX);
    let pixels = u64::from(window.0) * u64::from(window.1);
    if pixels <= max_pixels && window.0 <= max_dim && window.1 <= max_dim {
        return Some(RenderPlan {
            target: window,
            scale_x: 1.0,
            scale_y: 1.0,
        });
    }
    let area_scale = (max_pixels as f64 / pixels as f64).sqrt();
    let dim_scale =
        (f64::from(max_dim) / f64::from(window.0)).min(f64::from(max_dim) / f64::from(window.1));
    let factor = area_scale.min(dim_scale).min(1.0);
    let mut tw = ((f64::from(window.0) * factor).floor() as u32).clamp(1, max_dim);
    let mut th = ((f64::from(window.1) * factor).floor() as u32).clamp(1, max_dim);
    while u64::from(tw) * u64::from(th) > max_pixels {
        if tw >= th && tw > 1 {
            tw -= 1;
        } else if th > 1 {
            th -= 1;
        } else {
            break;
        }
    }
    Some(RenderPlan {
        target: (tw, th),
        scale_x: f64::from(tw) / f64::from(window.0),
        scale_y: f64::from(th) / f64::from(window.1),
    })
}

/// Push-driven worker schedule: truly `Idle` (0 Hz) when settled, waking only
/// for active animation/tooltip/caret deadlines, explicit live telemetry
/// polling (`Requests::set_poll_interval`), pending GPU initialization/backoff,
/// or CPU retry backoff. External changes wake immediately via `Requests`.
#[expect(
    clippy::too_many_arguments,
    reason = "pure scheduler inputs kept explicit for deterministic unit testing"
)]
fn worker_frame_demand(
    now: Instant,
    poll_interval: Option<Duration>,
    driver_wake: Option<Instant>,
    gpu_active: bool,
    gpu_init_pending: bool,
    gpu_init_deadline: Option<Instant>,
    retry_deadline: Option<Instant>,
    unpainted: bool,
) -> baseview::FrameDemand {
    let mut at: Option<Instant> = None;
    let mut sooner = |candidate: Instant| {
        at = Some(match at {
            Some(existing) => existing.min(candidate),
            None => candidate,
        });
    };
    if let Some(interval) = poll_interval {
        sooner(now.checked_add(interval).unwrap_or(now));
    }
    if let Some(wake) = driver_wake {
        sooner(wake);
    }
    if !gpu_active {
        if let Some(deadline) = gpu_init_deadline {
            sooner(deadline);
        } else if gpu_init_pending {
            sooner(now + Duration::from_millis(25));
        }
    }
    let retry_deadline = if gpu_active { None } else { retry_deadline };
    if let Some(deadline) = retry_deadline {
        sooner(deadline);
    } else if unpainted {
        sooner(now + Duration::from_millis(16));
    }
    match at {
        Some(deadline) => {
            let deadline = match retry_deadline {
                Some(min_retry) if driver_wake.is_none() && poll_interval.is_none() => {
                    deadline.max(min_retry)
                }
                _ => deadline,
            };
            baseview::FrameDemand::At(deadline)
        }
        None => baseview::FrameDemand::Idle,
    }
}

/// Whether the worker presents to the window's own surface. macOS keeps the
/// readback transport until its CPU layer hands over to Metal on the window
/// thread; `MUI_PRESENT=readback` asks for it anywhere.
fn presents_directly() -> bool {
    !cfg!(target_os = "macos")
        && !std::env::var("MUI_PRESENT").is_ok_and(|v| v.eq_ignore_ascii_case("readback"))
}

/// A renderer/readback unwind must take the same CPU/backoff path as an error,
/// rather than escaping the outer worker boundary and ending input processing.
fn render_gpu<R, T>(
    gpu: &mut Option<R>,
    render: impl FnOnce(&mut R) -> Result<T, HostError>,
) -> Option<Result<T, HostError>> {
    let renderer = gpu.as_mut()?;
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| render(renderer)))
        .unwrap_or_else(|payload| {
            Err(HostError::Unavailable {
                message: format!(
                    "GPU render/readback panic: {}",
                    mui::diagnostics::panic_message(&*payload)
                ),
                class: FailureClass::Transient,
            })
        });
    if result.is_err() {
        let failed = gpu.take();
        // A renderer may also panic while destroying partially initialized
        // resources. Do not let that second unwind terminate the worker.
        if let Err(payload) =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(failed)))
        {
            mui::diagnostics::error(
                "mui-baseview",
                "worker_gpu_drop_panic",
                mui::diagnostics::panic_message(&*payload),
            );
        }
    }
    Some(result)
}

fn dispatch<V: View>(
    driver: &mut Driver,
    shared: &Mutex<Shared<V>>,
    requests: &Requests,
    key_routes: &mut KeyRoutes,
    scale: f64,
    event: Event,
) {
    if matches!(
        event,
        Event::Window(WindowEvent::Unfocused | WindowEvent::WillClose)
    ) {
        key_routes.clear();
    }
    let points = |p: PhysicalPosition<f64>| Point::new(p.x / scale, p.y / scale);
    match event {
        Event::Keyboard(key) => {
            let event = key_event(&key);
            let hook = requests.key_hook_for(key_routes, &lock(shared).ui, &event);
            let kept = hook.is_some_and(|hook| {
                hook.lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)(
                    &lock(shared).ui, &event
                )
            });
            if !kept {
                driver.key(&lock(shared), &event);
            }
        }
        Event::Ime(event) => driver.ime(native::ime_event(&event)),
        Event::Window(WindowEvent::Focused) => driver.focus(true),
        Event::Window(WindowEvent::Unfocused) => driver.focus(false),
        Event::Mouse(MouseEvent::CursorMoved {
            position,
            modifiers,
        }) => driver.pointer_moved(points(position), mods(modifiers)),
        Event::Mouse(MouseEvent::ButtonPressed { button, modifiers }) => {
            if let Some(button) = mouse_button(button) {
                driver.button(button, true, mods(modifiers));
            }
        }
        Event::Mouse(MouseEvent::ButtonReleased { button, modifiers }) => {
            if let Some(button) = mouse_button(button) {
                driver.button(button, false, mods(modifiers));
            }
        }
        Event::Mouse(MouseEvent::WheelScrolled { delta, modifiers }) => {
            driver.wheel(
                match delta {
                    ScrollDelta::Lines { x, y } => Wheel::Lines(f64::from(x), f64::from(y)),
                    ScrollDelta::Pixels { x, y } => Wheel::Pixels(f64::from(x), f64::from(y)),
                },
                mods(modifiers),
            );
        }
        Event::Mouse(MouseEvent::CursorLeft | MouseEvent::DragLeft) => driver.pointer_left(),
        Event::Mouse(
            MouseEvent::DragEntered {
                position,
                modifiers,
                data: DropData::Files(files),
            }
            | MouseEvent::DragMoved {
                position,
                modifiers,
                data: DropData::Files(files),
            },
        ) => {
            driver.drop_files(
                &mut lock(shared),
                points(position),
                mods(modifiers),
                &files,
                false,
            );
        }
        Event::Mouse(MouseEvent::DragDropped {
            position,
            modifiers,
            data: DropData::Files(files),
        }) => {
            driver.drop_files(
                &mut lock(shared),
                points(position),
                mods(modifiers),
                &files,
                true,
            );
        }
        _ => {}
    }
}

struct WorkerClipboard {
    #[cfg(not(target_os = "linux"))]
    requests: Arc<Requests>,
    clipboard: Clipboard,
}
impl mui::Clipboard for WorkerClipboard {
    fn get(&mut self) -> Option<String> {
        self.clipboard.read()
    }
    fn set(&mut self, text: &str) {
        #[cfg(target_os = "linux")]
        self.clipboard.write(text);
        #[cfg(not(target_os = "linux"))]
        {
            self.clipboard.last = Some(text.to_owned());
            *self
                .requests
                .clipboard_write
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(text.to_owned());
            self.requests.wake();
        }
    }
}

#[cfg(test)]
mod gpu_recovery_tests {
    use super::*;

    #[test]
    fn presentation_requires_every_native_patch_to_succeed() {
        let requests = Requests::default();
        let patches = || {
            vec![
                Patch {
                    rect: [0, 0, 1, 1],
                    pixels: vec![0],
                },
                Patch {
                    rect: [1, 0, 1, 1],
                    pixels: vec![0],
                },
            ]
        };
        let mut attempts = 0;
        assert!(!submit_patches(
            &requests,
            (2, 1),
            (2, 1),
            mui_present::PresentationBackend::Offscreen,
            mui_present::GraphicsApi::Vulkan,
            None,
            "gpu",
            patches(),
            |_| {
                attempts += 1;
                attempts == 1
            }
        ));
        assert!(requests.presentation().is_none());
        assert!(submit_patches(
            &requests,
            (2, 1),
            (2, 1),
            mui_present::PresentationBackend::Offscreen,
            mui_present::GraphicsApi::Vulkan,
            None,
            "gpu",
            patches(),
            |_| true
        ));
        let presented = requests
            .presentation()
            .expect("successful native submission");
        assert_eq!(presented.frames, 1);
        assert!(!presented.software);
        assert_eq!(
            requests.presentation_state().mode,
            mui_present::PresentationMode::ReadbackGpu
        );
        assert!(!submit_patches(
            &requests,
            (2, 1),
            (2, 1),
            mui_present::PresentationBackend::Offscreen,
            mui_present::GraphicsApi::Software,
            None,
            "cpu",
            patches(),
            |_| false
        ));
        assert_eq!(
            requests
                .presentation()
                .expect("prior successful frame")
                .frames,
            1
        );
    }

    #[test]
    fn render_panic_falls_back_to_cpu_and_the_worker_accepts_another_frame() {
        struct FakeRenderer {
            render: fn() -> Result<(), HostError>,
        }
        let (commands, receive) = mpsc::channel();
        let (publish, frames) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let mut gpu = Some(FakeRenderer {
                render: || panic!("injected render/readback failure"),
            });
            let mut retry = Retry::default();
            while receive.recv().is_ok() {
                let cpu_frame = match render_gpu(&mut gpu, |renderer| (renderer.render)()) {
                    Some(Err(error)) => {
                        assert_eq!(error.failure_class(), FailureClass::Transient);
                        assert!(
                            error
                                .to_string()
                                .contains("injected render/readback failure")
                        );
                        retry.fail(Instant::now(), error.failure_class());
                        false
                    }
                    Some(Ok(())) => panic!("fake renderer must fail"),
                    None => true,
                };
                publish
                    .send((gpu.is_none(), cpu_frame, retry.deadline().is_some()))
                    .unwrap();
            }
        });
        commands.send(()).unwrap();
        assert_eq!(
            frames.recv_timeout(Duration::from_secs(5)).unwrap(),
            (true, false, true)
        );
        // A second and third request must reach CPU painting on the same worker.
        for _ in 0..2 {
            commands.send(()).unwrap();
            assert_eq!(
                frames.recv_timeout(Duration::from_secs(5)).unwrap(),
                (true, true, true)
            );
        }
        drop(commands);
        worker.join().unwrap();
    }

    #[test]
    fn large_hidpi_and_ultrawide_surfaces_scale_within_budget_instead_of_stopping() {
        assert_eq!(cpu_render_plan((0, 1080), CPU_PIXEL_BUDGET), None);
        assert_eq!(cpu_render_plan((1920, 0), CPU_PIXEL_BUDGET), None);

        // Normal and 5K surfaces stay 1:1 with zero scaling overhead.
        for normal in [(1920, 1080), (3840, 2160), (5120, 2880)] {
            let plan = cpu_render_plan(normal, CPU_PIXEL_BUDGET).expect("valid normal plan");
            assert_eq!(plan.target, normal);
            assert_eq!(plan.scale_x, 1.0);
            assert_eq!(plan.scale_y, 1.0);
        }

        // 6K, dual-4K ultrawide, 8K, and extreme >u16::MAX surfaces all produce a
        // non-empty budgeted target strictly within 16 Mi pixels and u16::MAX.
        for huge in [
            (6016, 3384),
            (7680, 2160),
            (7680, 4320),
            (8192, 4096),
            (70_000, 1000),
        ] {
            let plan = cpu_render_plan(huge, CPU_PIXEL_BUDGET).expect("valid large plan");
            assert!(plan.target.0 >= 1 && plan.target.0 <= u32::from(u16::MAX));
            assert!(plan.target.1 >= 1 && plan.target.1 <= u32::from(u16::MAX));
            assert!(
                u64::from(plan.target.0) * u64::from(plan.target.1) <= CPU_PIXEL_BUDGET,
                "{huge:?} -> {:?}",
                plan.target
            );
            assert!(plan.scale_x > 0.0 && plan.scale_x <= 1.0);
            assert!(plan.scale_y > 0.0 && plan.scale_y <= 1.0);
            // Verify vello_cpu::Renderer accepts the planned target directly.
            assert!(mui::vello::software::Renderer::new(plan.target).is_ok());
        }
    }

    #[test]
    fn settled_worker_is_truly_idle_zero_hz_while_deadlines_and_wakes_fire_promptly() {
        let now = Instant::now();
        // Settled GPU-active window: zero timers, true 0 Hz idle.
        assert_eq!(
            worker_frame_demand(now, None, None, true, true, None, None, false),
            baseview::FrameDemand::Idle
        );
        // Settled CPU-only / permanently-failed GPU window: also true 0 Hz idle.
        assert_eq!(
            worker_frame_demand(now, None, None, false, false, None, None, false),
            baseview::FrameDemand::Idle
        );
        // Exact animation / tooltip / caret deadline wakes at that instant.
        let caret = now + Duration::from_millis(530);
        assert_eq!(
            worker_frame_demand(now, None, Some(caret), true, false, None, None, false),
            baseview::FrameDemand::At(caret)
        );
        // Explicit live telemetry poll interval schedules until cleared.
        let meter = Duration::from_millis(16);
        assert_eq!(
            worker_frame_demand(now, Some(meter), None, true, false, None, None, false),
            baseview::FrameDemand::At(now + meter)
        );
        // Pending GPU init polls only while GPU is not yet active.
        assert_eq!(
            worker_frame_demand(now, None, None, false, true, None, None, false),
            baseview::FrameDemand::At(now + Duration::from_millis(25))
        );
        // Explicit GPU init backoff deadline overrides the 25 ms poll.
        let backoff = now + Duration::from_secs(2);
        assert_eq!(
            worker_frame_demand(now, None, None, false, false, Some(backoff), None, false),
            baseview::FrameDemand::At(backoff)
        );
        // A CPU retry still matters in software mode but must not keep
        // a settled GPU window awake after promotion.
        assert_eq!(
            worker_frame_demand(now, None, None, false, false, None, Some(backoff), false),
            baseview::FrameDemand::At(backoff)
        );
        assert_eq!(
            worker_frame_demand(now, None, None, true, false, None, Some(backoff), false),
            baseview::FrameDemand::Idle
        );
        // Unpainted frame that needs a retry does not go permanently silent.
        assert_eq!(
            worker_frame_demand(now, None, None, true, false, None, None, true),
            baseview::FrameDemand::At(now + Duration::from_millis(16))
        );
    }

    #[test]
    fn worker_thread_settles_to_idle_and_wakes_when_polling_is_enabled() {
        use mui::prelude::Paints as _;

        struct StaticView;
        impl View for StaticView {
            fn build(&mut self, _: &mut mui::Ui, _: &mui::prelude::Input) -> mui::prelude::El {
                mui::prelude::block(64., 64.).fill(mui::prelude::Color::srgb(0.2, 0.3, 0.4))
            }
            fn changed(&mut self) -> bool {
                false
            }
            fn request_resize(&mut self, _: u32, _: u32) -> bool {
                false
            }
        }
        let shared = Arc::new(Mutex::new(Shared {
            ui: mui::Ui::default(),
            view: StaticView,
        }));
        let requests = Arc::new(Requests::default());
        let woke = Arc::new(AtomicBool::new(false));
        let woke_flag = Arc::clone(&woke);
        requests.bind_requester(Some(baseview::FrameRequester::new(move || {
            woke_flag.store(true, Ordering::Release);
        })));
        let (send, commands) = mpsc::channel();
        let (publish, output) = mpsc::sync_channel(4);
        let stopped = Arc::new(AtomicBool::new(false));
        let tick_pending = Arc::new(AtomicBool::new(false));
        let worker_shared = Arc::clone(&shared);
        let worker_requests = Arc::clone(&requests);
        let worker_stopped = Arc::clone(&stopped);
        let worker_tick = Arc::clone(&tick_pending);
        let handle = std::thread::spawn(move || {
            worker_with_mode(
                &worker_shared,
                &worker_requests,
                WindowSize::from_physical(baseview::dpi::PhysicalSize::new(64, 64), 1.0),
                None,
                WorkerChannels {
                    commands,
                    output: publish,
                    stopped: worker_stopped,
                    tick_pending: worker_tick,
                },
                true,
            );
        });

        // First tick paints the initial static frame and settles immediately to
        // FrameDemand::Idle (0 Hz) with no periodic timer armed.
        send.send(Command::Tick(None)).unwrap();
        let frame = output.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(frame.demand, baseview::FrameDemand::Idle);
        // Wait for the initial frame's wake before checking idle behavior.
        let wait_for_wake = || {
            let deadline = Instant::now() + Duration::from_secs(3);
            while !woke.swap(false, Ordering::AcqRel) {
                assert!(Instant::now() < deadline, "worker did not wake native pump");
                std::thread::sleep(Duration::from_millis(5));
            }
        };
        wait_for_wake();
        std::thread::sleep(Duration::from_millis(300));
        assert!(
            !woke.load(Ordering::Acquire),
            "disabled polling must not wake the window"
        );

        requests.set_poll_interval(Some(Duration::from_millis(16)));
        wait_for_wake();
        assert!(
            requests.redraw.load(Ordering::Acquire),
            "wake must queue a native tick"
        );
        send.send(Command::Tick(None)).unwrap();
        assert!(matches!(
            output.recv_timeout(Duration::from_secs(5)).unwrap().demand,
            baseview::FrameDemand::At(_)
        ));

        stopped.store(true, Ordering::Release);
        let _ = send.send(Command::Stop);
        handle.join().unwrap();
    }
}

#[cfg(all(test, target_os = "linux"))]
mod native_pointer_tests {
    use super::*;

    #[test]
    #[ignore = "requires an isolated X11 display"]
    fn native_pointer_release_warps_to_the_hidden_origin() {
        use x11rb::connection::Connection;
        use x11rb::protocol::xproto::ConnectionExt;

        struct Empty;
        impl View for Empty {
            fn build(&mut self, _: &mut mui::Ui, _: &mui::prelude::Input) -> mui::prelude::El {
                mui::prelude::block(100., 100.)
            }
            fn changed(&mut self) -> bool {
                false
            }
            fn request_resize(&mut self, _: u32, _: u32) -> bool {
                false
            }
        }
        struct Probe {
            adapter: NativeAdapter,
            hide: Arc<AtomicBool>,
            phase: Cell<u8>,
            done: Sender<()>,
        }
        impl WindowHandler for Probe {
            fn frame_demand(&self) -> baseview::FrameDemand {
                self.adapter.frame_demand()
            }
            fn on_frame(&self) -> Result<(), HandlerError> {
                self.adapter.on_frame()?;
                let move_to = |x, y| {
                    self.adapter.on_event(Event::Mouse(MouseEvent::CursorMoved {
                        position: PhysicalPosition::new(x, y),
                        modifiers: Modifiers::empty(),
                    }))
                };
                match self.phase.get() {
                    0 => {
                        platform::warp_pointer(&self.adapter.cx, 20., 30., 1.);
                        move_to(20., 30.);
                        self.hide.store(true, Ordering::Release);
                        self.adapter.requests.redraw();
                        self.phase.set(1);
                    }
                    1 if self.adapter.pointer.borrow().hidden() => {
                        platform::warp_pointer(&self.adapter.cx, 70., 80., 1.);
                        move_to(70., 80.);
                        self.hide.store(false, Ordering::Release);
                        self.adapter.requests.redraw();
                        self.phase.set(2);
                    }
                    2 if !self.adapter.pointer.borrow().hidden() => {
                        self.done.send(()).unwrap();
                        self.phase.set(3);
                    }
                    _ => {}
                }
                Ok(())
            }
            fn resized(&self, size: WindowSize) -> Result<(), HandlerError> {
                self.adapter.resized(size)
            }
            fn on_event(&self, event: Event) -> EventStatus {
                self.adapter.on_event(event)
            }
        }
        let requests = Arc::new(Requests::default());
        let hide = Arc::new(AtomicBool::new(false));
        let hook_hide = hide.clone();
        requests.on_pointer(Arc::new(Mutex::new(move |_: &mui::Ui| {
            hook_hide.load(Ordering::Acquire)
        })));
        let shared = Arc::new(Mutex::new(Shared {
            ui: mui::Ui::default(),
            view: Empty,
        }));
        let (done, receive) = mpsc::channel();
        let worker_requests = requests.clone();
        let window = Window::create(
            settings("MUI native pointer restoration", (100, 100))
                .with_scale_factor_override(Some(1.)),
            move |cx| {
                Ok(Probe {
                    adapter: build(shared, worker_requests, false)(cx)?,
                    hide,
                    phase: Cell::new(0),
                    done,
                })
            },
        )
        .expect("pointer fixture window");
        window.show().expect("show pointer fixture");
        receive
            .recv_timeout(Duration::from_secs(10))
            .expect("pointer restored by native output");
        let (connection, _) = x11rb::connect(None).expect("pointer query connection");
        connection.flush().unwrap();
        let pointer = connection
            .query_pointer(requests.x11_window().expect("live X11 window"))
            .unwrap()
            .reply()
            .unwrap();
        window.close();
        assert_eq!(
            (pointer.win_x, pointer.win_y),
            (20, 30),
            "release restores the hidden origin"
        );
    }
}
