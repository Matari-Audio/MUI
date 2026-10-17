//! Input-only native window adapter. The worker owns no native handle or native context.
use super::*;
use mui::vello::host::{BitmapPatch as Patch, Offscreen};
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
        let start = requests
            .native_start
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let Some(start) = start {
            start();
        }
        let size = cx.size();
        #[expect(
            unsafe_code,
            reason = "attach provider to builder's live HWND on its owning thread"
        )]
        // SAFETY: the native endpoint stays here and drops before cx.
        let endpoints = cx.window_handle().ok().and_then(|h| {
            // SAFETY: builder owns this HWND; native drops before cx.
            unsafe { NativeAccessibility::new(h.as_raw()) }
        });
        let (mut native, accessibility) =
            endpoints.map_or((None, None), |(n, a)| (Some(n), Some(a)));
        if let Some(native) = &mut native {
            native.focus(cx.has_focus());
        }
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
                        shared,
                        wake.clone(),
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
        if let Ok(handle) = cx.window_handle() {
            let id = match handle.as_raw() {
                raw_window_handle::RawWindowHandle::Xlib(h) => u32::try_from(h.window).unwrap_or(0),
                raw_window_handle::RawWindowHandle::Xcb(h) => h.window.get(),
                _ => 0,
            };
            requests.x11_window.store(id, Ordering::Release);
        }
        #[cfg(target_os = "macos")]
        let presenter = mac_transport::Presenter::new(&cx).map_err(std::io::Error::other)?;
        #[cfg(target_os = "linux")]
        let presenter =
            mui::vello::software::BitmapWindow::new(cx.clone()).map_err(std::io::Error::other)?;
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
            bitmap: RefCell::new(None),
            expose: RefCell::new(Vec::new()),
            input_pending: Cell::new(true),
            flash_at: Cell::new(None),
            flash_rects: RefCell::new(Vec::new()),
            #[cfg(not(target_os = "windows"))]
            presenter: RefCell::new(presenter),
        })
    }
}

enum Command {
    Event(Event),
    Resize(WindowSize),
    Tick(Option<(u64, u64)>),
    Invalidate,
    Stop,
}
struct Output {
    patches: Vec<Patch>,
    size: (u32, u32),
    software: bool,
    reason: &'static str,
    capture: bool,
    ime: Option<baseview::ImeConfiguration>,
    cursor: MouseCursor,
    accessibility: Option<mui_access::accesskit::TreeUpdate>,
    demand: baseview::FrameDemand,
    drag: Option<(u64, u64)>,
}
struct Bitmap {
    size: (u32, u32),
    pixels: Vec<u32>,
}

pub(super) struct NativeAdapter {
    close: NativeClose,
    requests: Arc<Requests>,
    native: RefCell<Option<NativeAccessibility>>,
    #[cfg(target_os = "macos")]
    presenter: RefCell<mac_transport::Presenter>,
    #[cfg(target_os = "linux")]
    presenter: RefCell<mui::vello::software::BitmapWindow<WindowContext>>,
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
    bitmap: RefCell<Option<Bitmap>>,
    expose: RefCell<Vec<[u32; 4]>>,
    input_pending: Cell<bool>,
    flash_at: Cell<Option<Instant>>,
    flash_rects: RefCell<Vec<[u32; 4]>>,
}
impl NativeAdapter {
    fn blit(&self, rect: [u32; 4], pixels: &[u32]) {
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
        self.blit_raw(rect, decorated.as_deref().unwrap_or(pixels));
    }
    fn blit_raw(&self, rect: [u32; 4], pixels: &[u32]) {
        #[cfg(target_os = "windows")]
        blit(&self.cx, rect, pixels);
        #[cfg(target_os = "macos")]
        if let Err(error) = self
            .presenter
            .borrow_mut()
            .present(&self.cx, rect, pixels.to_vec())
        {
            mui::diagnostics::error("mui-baseview", "native_bitmap", &error);
        }
        #[cfg(target_os = "linux")]
        if let Err(error) = self.presenter.borrow_mut().present_patch(
            rect,
            pixels,
            (
                self.cx.size().physical.width,
                self.cx.size().physical.height,
            ),
        ) {
            mui::diagnostics::error("mui-baseview", "native_bitmap", &error);
        }
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
            self.requests.x11_window.store(0, Ordering::Release);
        }
    }
    fn send(&self, command: Command) {
        if !self.stopped.load(Ordering::Acquire) {
            if matches!(command, Command::Event(_) | Command::Resize(_)) {
                self.input_pending.set(true);
            }
            let _ = self.send.send(command);
        }
    }
    fn native_requests(&self) {
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
        if self.capture.replace(output.capture) != output.capture {
            self.cx.set_keyboard_capture(output.capture);
        }
        if self.ime.borrow().as_ref() != Some(&output.ime) {
            self.cx.set_ime_configuration(output.ime.clone());
            *self.ime.borrow_mut() = Some(output.ime);
        }
        if self.cursor.replace(Some(output.cursor)) != Some(output.cursor) {
            let _ = self.cx.set_mouse_cursor(output.cursor);
        }
        // Native publication may reenter, but never acquires the model lock.
        if let Some(update) = output.accessibility {
            if let Some(native) = self.native.borrow_mut().as_mut() {
                native.publish(update);
            }
        }
        let actual = self.cx.size();
        if output.size != (actual.physical.width, actual.physical.height) {
            self.send(Command::Invalidate);
            self.input_pending.set(true);
            self.requests.wake();
            return;
        }
        #[cfg(target_os = "macos")]
        if !output.patches.is_empty() {
            for patch in output.patches {
                let pixels = patch.pixels;
                if let Err(error) = self
                    .presenter
                    .borrow_mut()
                    .present(&self.cx, patch.rect, pixels)
                {
                    mui::diagnostics::error("mui-baseview", "native_bitmap", &error);
                }
            }
            self.requests
                .presented(output.size, output.software, output.reason);
        }
        #[cfg(not(target_os = "macos"))]
        if !output.patches.is_empty() {
            let mut bitmap = self.bitmap.borrow_mut();
            // A full update already owns a completed bitmap: move it into
            // native retention instead of allocating/copying a full frame here.
            let mut patches = output.patches;
            if patches.len() == 1 && patches[0].rect == [0, 0, output.size.0, output.size.1] {
                let patch = patches.pop().expect("one patch");
                self.blit(patch.rect, &patch.pixels);
                *bitmap = Some(Bitmap {
                    size: output.size,
                    pixels: patch.pixels,
                });
            }
            if bitmap.as_ref().is_none_or(|b| b.size != output.size) {
                self.send(Command::Invalidate);
                self.input_pending.set(true);
                self.requests.wake();
                return;
            }
            let bitmap = bitmap.as_mut().expect("allocated above");
            for patch in patches {
                let [x, y, w, h] = patch.rect;
                for row in 0..h as usize {
                    let dest = (y as usize + row) * output.size.0 as usize + x as usize;
                    let source = row * w as usize;
                    bitmap.pixels[dest..dest + w as usize]
                        .copy_from_slice(&patch.pixels[source..source + w as usize]);
                }
                self.blit(patch.rect, &patch.pixels);
            }
            self.requests
                .presented(output.size, output.software, output.reason);
        }
        self.requests.drag.rendered(output.drag);
    }
}
impl Drop for NativeAdapter {
    fn drop(&mut self) {
        self.stop();
    }
}
impl WindowHandler for NativeAdapter {
    fn frame_demand(&self) -> baseview::FrameDemand {
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
                    self.blit_raw([x, y, w, h], &pixels);
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
        if let Some(bitmap) = self.bitmap.borrow().as_ref() {
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
        }
        let due = match self.demand.get() {
            baseview::FrameDemand::Continuous => true,
            baseview::FrameDemand::At(at) => Instant::now() >= at,
            baseview::FrameDemand::Idle => false,
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
            self.send(Command::Event(Event::Window(WindowEvent::Unfocused)));
        }
        self.requests.idle();
        Ok(())
    }
    fn damage(&self, area: baseview::DamageArea<'_>) {
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
        self.send(Command::Resize(size));
        self.requests.wake();
        Ok(())
    }
    fn on_event(&self, event: Event) -> EventStatus {
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
            self.send(Command::Event(event.clone()));
        }
        self.requests.wake();
        match event {
            Event::Keyboard(_) if !self.capture.get() => EventStatus::Ignored,
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
    shared: Arc<Mutex<Shared<V>>>,
    requests: Arc<Requests>,
    size: WindowSize,
    mut accessibility: Option<AccessibilityUi>,
    channels: WorkerChannels,
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
            requests: requests.clone(),
            clipboard: Clipboard::default(),
        }),
    );
    driver.display_paced = true;
    let mut scale = size.scale_factor;
    let mut gpu = None::<Offscreen>;
    let software_only = std::env::var("MUI_RENDERER").is_ok_and(|v| v.eq_ignore_ascii_case("cpu"));
    let mut init = (!software_only).then(|| GpuInit::new(true));
    let mut reason = if software_only {
        "CPU forced"
    } else {
        "CPU startup"
    };
    let mut cpu = None::<mui::vello::software::Renderer>;
    let mut retry = Retry::default();
    let mut unpainted = true;
    lock(&shared).ui.set_gpu_welding_available(false);
    while let Ok(command) = commands.recv() {
        if stopped.load(Ordering::Acquire) || matches!(command, Command::Stop) {
            break;
        }
        let drag = match command {
            Command::Resize(size) => {
                scale = size.scale_factor;
                driver.resized((size.physical.width, size.physical.height), scale);
                continue;
            }
            Command::Event(event) => {
                dispatch(&mut driver, &shared, &requests, scale, event);
                continue;
            }
            Command::Tick(drag) => {
                tick_pending.store(false, Ordering::Release);
                drag
            }
            Command::Invalidate => {
                driver.redraw();
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
        let valid =
            size.0 > 0 && size.1 > 0 && u64::from(size.0) * u64::from(size.1) <= 16 * 1024 * 1024;
        if valid && gpu.is_none() {
            if let Some(result) = init.as_mut().and_then(|i| i.poll_offscreen(size)) {
                match result {
                    Ok(renderer) => {
                        gpu = Some(renderer);
                        reason = "GPU ready";
                        lock(&shared).ui.set_gpu_welding_available(true);
                        driver.redraw();
                    }
                    Err(error) => {
                        if reason != "CPU GPU initialization failed" {
                            log(
                                &shared,
                                &format!(
                                    "mui-baseview: GPU unavailable ({error}); using CPU rendering"
                                ),
                            );
                        }
                        reason = "CPU GPU initialization failed";
                    }
                }
            }
        }
        if requests.redraw.swap(false, Ordering::AcqRel) {
            driver.redraw();
        }
        let (scene, capture, ime, update) = {
            let mut shared = lock(&shared);
            if let Some(a) = &mut accessibility {
                let requested = a.wants_tree();
                if a.apply(&mut shared.ui) || requested {
                    driver.redraw();
                }
            }
            let fresh = driver.advance(&mut shared, now);
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
                if unpainted && valid {
                    shared.ui.scene_snapshot()
                } else {
                    None
                },
                shared.ui.focus_is_text(),
                ime,
                update,
            )
        };
        let mut patches = Vec::new();
        if let Some(scene) = scene {
            let transform = Affine::scale(driver.ui_scale());
            if let Some(renderer) = &mut gpu {
                match renderer.render(&scene, transform, size) {
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
                        gpu = None;
                        reason = "CPU GPU recovery";
                        if let Some(init) = &mut init {
                            init.recover(class);
                        }
                        lock(&shared).ui.set_gpu_welding_available(false);
                        driver.redraw();
                    }
                }
            } else if retry.ready(now) {
                let result = (|| {
                    if cpu.is_none() {
                        cpu = Some(mui::vello::software::Renderer::new(size)?);
                    }
                    let cpu = cpu.as_mut().expect("created above");
                    cpu.resize(size)?;
                    let rects = cpu.render_damage(&scene, transform)?;
                    Ok::<_, String>(
                        rects
                            .into_iter()
                            .map(|rect @ [x, y, w, h]| {
                                let mut pixels = Vec::with_capacity(w as usize * h as usize);
                                for row in y..y + h {
                                    let start = (row as usize * size.0 as usize + x as usize) * 4;
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
        if stopped.load(Ordering::Acquire) {
            break;
        }
        let heartbeat = requests
            .poll_interval()
            .unwrap_or(Duration::from_millis(250));
        let mut at = now + heartbeat;
        if let Some(wake) = driver.next_wake() {
            at = at.min(wake);
        }
        if let Some(init) = &init {
            if gpu.is_none() {
                at = at.min(init.deadline().unwrap_or(now + Duration::from_millis(25)));
            }
        }
        if let Some(deadline) = retry.deadline() {
            at = at.max(deadline);
        }
        if output
            .send(Output {
                patches,
                size,
                software: gpu.is_none(),
                reason,
                capture,
                ime,
                cursor: native_cursor(driver.cursor()),
                accessibility: update,
                demand: baseview::FrameDemand::At(at),
                drag,
            })
            .is_err()
        {
            break;
        }
        requests.wake();
    }
    // No HWND or host callbacks here. The owner revokes host delivery first.
    driver.close(&mut lock(&shared));
}

fn dispatch<V: View>(
    driver: &mut Driver,
    shared: &Mutex<Shared<V>>,
    requests: &Requests,
    scale: f64,
    event: Event,
) {
    let points = |p: PhysicalPosition<f64>| Point::new(p.x / scale, p.y / scale);
    match event {
        Event::Keyboard(key) => {
            let event = key_event(&key);
            let hook = requests
                .keys
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone();
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
                data,
            }
            | MouseEvent::DragMoved {
                position,
                modifiers,
                data,
            },
        ) => {
            if let DropData::Files(files) = data {
                driver.drop_files(
                    &mut lock(shared),
                    points(position),
                    mods(modifiers),
                    &files,
                    false,
                );
            }
        }
        Event::Mouse(MouseEvent::DragDropped {
            position,
            modifiers,
            data,
        }) => {
            if let DropData::Files(files) = data {
                driver.drop_files(
                    &mut lock(shared),
                    points(position),
                    mods(modifiers),
                    &files,
                    true,
                );
            }
        }
        _ => {}
    }
}

/// GDI only: completed pixels are copied by dirty rectangle, without a GPU
/// surface, worker mutex, fence, shader compiler or native cross-thread call.
#[expect(
    unsafe_code,
    reason = "bounded GDI bitmap blit on the HWND's owning thread"
)]
#[cfg(target_os = "windows")]
fn blit(window: &WindowContext, [x, y, w, h]: [u32; 4], pixels: &[u32]) {
    #[repr(C)]
    struct Info {
        size: u32,
        width: i32,
        height: i32,
        planes: u16,
        bits: u16,
        compression: u32,
        bytes: u32,
        xppm: i32,
        yppm: i32,
        colors: u32,
        important: u32,
        rgb: u32,
    }
    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetDC(hwnd: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
        fn ReleaseDC(hwnd: *mut std::ffi::c_void, dc: *mut std::ffi::c_void) -> i32;
    }
    #[link(name = "gdi32")]
    unsafe extern "system" {
        fn StretchDIBits(
            dc: *mut std::ffi::c_void,
            x: i32,
            y: i32,
            w: i32,
            h: i32,
            sx: i32,
            sy: i32,
            sw: i32,
            sh: i32,
            bits: *const std::ffi::c_void,
            info: *const Info,
            usage: u32,
            rop: u32,
        ) -> i32;
    }
    if w == 0 || h == 0 || pixels.len() != w as usize * h as usize {
        return;
    }
    let Ok(handle) = window.window_handle() else {
        return;
    };
    let raw_window_handle::RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return;
    };
    let hwnd = handle.hwnd.get() as *mut std::ffi::c_void;
    let info = Info {
        size: 40,
        width: w as i32,
        height: -(h as i32),
        planes: 1,
        bits: 32,
        compression: 0,
        bytes: 0,
        xppm: 0,
        yppm: 0,
        colors: 0,
        important: 0,
        rgb: 0,
    };
    // SAFETY: cx owns this live HWND on this thread. pixels contains exactly
    // w*h 32-bit pixels, and the top-down BITMAPINFO describes those bytes.
    unsafe {
        let dc = GetDC(hwnd);
        if !dc.is_null() {
            StretchDIBits(
                dc,
                x as i32,
                y as i32,
                w as i32,
                h as i32,
                0,
                0,
                w as i32,
                h as i32,
                pixels.as_ptr().cast(),
                &info,
                0,
                0x00cc0020,
            );
            ReleaseDC(hwnd, dc);
        }
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
#[cfg(target_os = "macos")]
#[path = "mac_transport.rs"]
mod mac_transport;
