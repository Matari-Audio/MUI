//! Input-only HWND adapter. The worker owns no native handle or native context.
use super::*;
use mui::vello::host::{BitmapPatch as Patch, Offscreen};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};

pub(super) fn build<V: View + Send + 'static>(
    shared: Arc<Mutex<Shared<V>>>,
    requests: Arc<Requests>,
    parented: bool,
) -> impl FnOnce(WindowContext) -> Result<WindowsAdapter, HandlerError> + Send + 'static {
    move |cx| {
        if !baseview::pin_current_image_for_detached_work() {
            return Err(std::io::Error::other("cannot pin plugin image for editor worker").into());
        }
        let size = cx.size();
        #[expect(
            unsafe_code,
            reason = "attach provider to builder's live HWND on its owning thread"
        )]
        // SAFETY: the native endpoint stays here and drops before cx.
        let endpoints = cx
            .window_handle()
            .ok()
            .and_then(|h| unsafe { NativeAccessibility::new(h.as_raw()) });
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
        let pending = tick_pending.clone();
        let wake = requests.clone();
        let mut drag = drag::Host::new(&requests);
        drag.parented = parented;
        std::thread::Builder::new()
            .name("mui-editor-render".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    worker(
                        shared,
                        wake.clone(),
                        size,
                        accessibility,
                        receive,
                        publish,
                        stop,
                        pending,
                    );
                }));
                if let Err(payload) = result {
                    mui::diagnostics::error(
                        "mui-baseview",
                        "render_worker_panic",
                        mui::diagnostics::panic_message(&*payload),
                    );
                }
            })?;
        let hook = requests
            .close
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        Ok(WindowsAdapter {
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
            expose: Cell::new(false),
            input_pending: Cell::new(true),
        })
    }
}

enum Command {
    Event(Event),
    Resize(WindowSize),
    Tick(Option<(u64, u64)>),
    Stop,
}
struct Output {
    patches: Vec<Patch>,
    size: (u32, u32),
    software: bool,
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

pub(super) struct WindowsAdapter {
    close: NativeClose,
    requests: Arc<Requests>,
    native: RefCell<Option<NativeAccessibility>>,
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
    expose: Cell<bool>,
    input_pending: Cell<bool>,
}
impl WindowsAdapter {
    fn stop(&self) {
        if !self.stopped.swap(true, Ordering::AcqRel) {
            self.close.fire();
            self.native.borrow_mut().take();
            self.drag.borrow_mut().close(&self.requests.drag);
            let _ = self.send.send(Command::Stop);
            self.requests.bind_requester(None);
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
            return;
        }
        if !output.patches.is_empty() {
            let mut bitmap = self.bitmap.borrow_mut();
            if bitmap.as_ref().is_none_or(|b| b.size != output.size) {
                *bitmap = Some(Bitmap {
                    size: output.size,
                    pixels: vec![0; output.size.0 as usize * output.size.1 as usize],
                });
            }
            let bitmap = bitmap.as_mut().expect("allocated above");
            for patch in output.patches {
                let [x, y, w, h] = patch.rect;
                for row in 0..h as usize {
                    let dest = (y as usize + row) * output.size.0 as usize + x as usize;
                    let source = row * w as usize;
                    bitmap.pixels[dest..dest + w as usize]
                        .copy_from_slice(&patch.pixels[source..source + w as usize]);
                }
                blit(&self.cx, patch.rect, &patch.pixels);
            }
            self.requests.presented(
                output.size,
                output.software,
                if output.software {
                    "CPU worker damage"
                } else {
                    "GPU worker damage"
                },
            );
        }
        self.requests.drag.rendered(output.drag);
    }
}
impl Drop for WindowsAdapter {
    fn drop(&mut self) {
        self.stop();
    }
}
impl WindowHandler for WindowsAdapter {
    fn frame_demand(&self) -> baseview::FrameDemand {
        self.demand.get()
    }
    fn on_frame(&self) -> Result<(), HandlerError> {
        if self.stopped.load(Ordering::Acquire) {
            return Ok(());
        }
        self.native_requests();
        // Take each message outside the receiver borrow before native callbacks.
        loop {
            let output = self.output.borrow().try_recv().ok();
            let Some(output) = output else { break };
            self.apply(output);
        }
        if self.expose.replace(false) {
            if let Some(bitmap) = self.bitmap.borrow().as_ref() {
                blit(
                    &self.cx,
                    [0, 0, bitmap.size.0, bitmap.size.1],
                    &bitmap.pixels,
                );
            }
        }
        let due = match self.demand.get() {
            baseview::FrameDemand::Continuous => true,
            baseview::FrameDemand::At(at) => Instant::now() >= at,
            baseview::FrameDemand::Idle => false,
        };
        if (due || self.input_pending.get() || self.requests.redraw.load(Ordering::Acquire))
            && !self.tick_pending.swap(true, Ordering::AcqRel)
        {
            self.input_pending.set(false);
            let drag = self.drag.borrow().frame_token(&self.requests.drag);
            self.send(Command::Tick(drag));
        }
        if self.drag.borrow_mut().drive(&self.requests, &self.cx) {
            self.send(Command::Event(Event::Window(WindowEvent::Unfocused)));
        }
        self.requests.idle();
        Ok(())
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
        self.drag.borrow().observe(&self.requests.drag, &event);
        if matches!(event, Event::Window(WindowEvent::RedrawRequested)) {
            self.expose.set(true);
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

fn worker<V: View>(
    shared: Arc<Mutex<Shared<V>>>,
    requests: Arc<Requests>,
    size: WindowSize,
    mut accessibility: Option<AccessibilityUi>,
    commands: Receiver<Command>,
    output: SyncSender<Output>,
    stopped: Arc<AtomicBool>,
    tick_pending: Arc<AtomicBool>,
) {
    let mut driver = Driver::new(
        (size.physical.width, size.physical.height),
        size.scale_factor,
        Box::new(Clipboard::default()),
    );
    driver.display_paced = true;
    let mut scale = size.scale_factor;
    let mut gpu = None::<Offscreen>;
    let software_only = std::env::var("MUI_RENDERER").is_ok_and(|v| v.eq_ignore_ascii_case("cpu"));
    let mut init = (!software_only).then(|| GpuInit::new(true));
    let mut cpu = None::<mui::vello::software::Renderer>;
    let mut retry = Retry::default();
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
            Command::Stop => break,
        };
        let now = Instant::now();
        let size = driver.size();
        let valid =
            size.0 > 0 && size.1 > 0 && u64::from(size.0) * u64::from(size.1) <= 16 * 1024 * 1024;
        if valid && gpu.is_none() {
            if let Some(result) = init.as_mut().and_then(|i| i.poll_offscreen(size)) {
                match result {
                    Ok(renderer) => {
                        gpu = Some(renderer);
                        lock(&shared).ui.set_gpu_welding_available(true);
                        driver.redraw();
                    }
                    Err(error) => mui::diagnostics::error(
                        "mui-baseview",
                        "worker_gpu_init",
                        &error.to_string(),
                    ),
                }
            }
        }
        if requests.redraw.swap(false, Ordering::AcqRel) {
            driver.redraw();
        }
        let (scene, capture, ime, update) = {
            let mut shared = lock(&shared);
            if let Some(a) = &mut accessibility {
                if a.wants_tree() || a.apply(&mut shared.ui) {
                    driver.redraw();
                }
            }
            let fresh = driver.advance(&mut shared, now);
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
                if fresh && valid {
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
                    Ok(damage) => patches = damage,
                    Err(error) => {
                        let class = error.failure_class();
                        mui::diagnostics::error(
                            "mui-baseview",
                            "worker_gpu_fault",
                            &error.to_string(),
                        );
                        gpu = None;
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
