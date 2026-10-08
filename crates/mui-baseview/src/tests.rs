//! Event translation checks and an opt-in native presentation regression.
use super::*;
use keyboard_types::Code;
use mui::Ui;
use mui::prelude::{El, Input, knob};

/// WGPU_BACKEND=metal on Linux exercises automatic fallback. Otherwise this
/// forces CPU on the handler, without changing process environment in tests.
#[cfg(target_os = "linux")]
#[test]
#[ignore = "requires an X11 display"]
fn native_cpu_fallback_presents_and_reopens() {
    cpu_presentation_fixture(false);
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "requires a live X11 display and graphics driver"]
fn native_resize_failure_falls_back_and_presents() {
    cpu_presentation_fixture(true);
}

#[cfg(target_os = "linux")]
fn cpu_presentation_fixture(resize_failure: bool) {
    use mui::prelude::*;
    use std::sync::mpsc::{Sender, channel};
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{ConnectionExt, ImageFormat};

    struct Green;
    impl View for Green {
        fn build(&mut self, _: &mut Ui, _: &Input) -> El {
            block(240., 200.).fill(Color::srgb(0., 1., 0.))
        }
        fn changed(&mut self) -> bool {
            false
        }
        fn request_resize(&mut self, _: u32, _: u32) -> bool {
            false
        }
    }
    struct Probe {
        handler: RefCell<Handler<Green>>,
        cx: WindowContext,
        phase: std::cell::Cell<u8>,
        exposed: std::cell::Cell<bool>,
        resize_failure: bool,
        result: RefCell<Option<Sender<Result<(), String>>>>,
    }
    impl WindowHandler for Probe {
        fn on_frame(&self) -> Result<(), HandlerError> {
            if self.phase.get() == 0 {
                let result = (|| {
                    let mut handler = self.handler.borrow_mut();
                    handler.tick(&self.cx);
                    if self.resize_failure {
                        let presented = handler
                            .requests
                            .presentation()
                            .ok_or("missing initial GPU presentation")?;
                        if presented.software || presented.size != (240, 200) {
                            return Err("fixture did not establish a GPU frame".into());
                        }
                        let gpu = handler.gpu.as_ref().ok_or("missing initial GPU")?;
                        // wgpu marks this device invalid immediately. Changing the
                        // extent forces real surface configuration before present
                        // could rebuild the lost device.
                        gpu.device().0.destroy();
                        handler.requests.resize(200, 160);
                        handler.tick(&self.cx);
                        if handler.gpu.is_some()
                            || handler.software.is_some()
                            || !handler.software_only
                            || !handler.unpainted
                            || handler.driver.size() != (200, 160)
                        {
                            return Err("resize failure did not detach GPU for CPU recovery".into());
                        }
                    }
                    Ok(())
                })();
                if result.is_err() {
                    if let Some(send) = self.result.borrow_mut().take() {
                        let _ = send.send(result);
                    }
                    return Ok(());
                }
                // Baseview flushes its X11 connection after this callback.
                self.phase.set(1);
                return Ok(());
            }
            if self.phase.get() == 2 {
                if self.exposed.get() {
                    self.handler.borrow_mut().tick(&self.cx);
                    self.phase.set(3);
                }
                return Ok(());
            }
            let Some(send) = self.result.borrow_mut().take() else {
                return Ok(());
            };
            let result = (|| {
                let mut handler = self.handler.borrow_mut();
                handler.tick(&self.cx);
                if handler.gpu.is_some() || handler.software.is_none() || handler.unpainted {
                    return Err("expected a successfully presented CPU fallback".into());
                }
                let expected = if self.resize_failure {
                    (200, 160)
                } else {
                    (240, 200)
                };
                let presented = handler
                    .requests
                    .presentation()
                    .ok_or("no CPU presentation")?;
                if !presented.software
                    || presented.size != expected
                    || (self.resize_failure && presented.frames < 2)
                {
                    return Err("CPU presentation did not use the recovered extent".into());
                }
                let window = handler.requests.x11_window().ok_or("no X11 window")?;
                let (connection, _) = x11rb::rust_connection::RustConnection::connect(None)
                    .map_err(|e| e.to_string())?;
                let visual_id = connection
                    .get_window_attributes(window)
                    .map_err(|e| e.to_string())?
                    .reply()
                    .map_err(|e| e.to_string())?
                    .visual;
                let visual = connection
                    .setup()
                    .roots
                    .iter()
                    .flat_map(|s| &s.allowed_depths)
                    .flat_map(|d| &d.visuals)
                    .find(|v| v.visual_id == visual_id)
                    .ok_or("missing visual")?;
                let geometry = connection
                    .get_geometry(window)
                    .map_err(|e| e.to_string())?
                    .reply()
                    .map_err(|e| e.to_string())?;
                if (u32::from(geometry.width), u32::from(geometry.height)) != expected {
                    return Err("native window did not use the recovered extent".into());
                }
                let depth = geometry.depth;
                let deadline = Instant::now() + Duration::from_secs(1);
                loop {
                    let pixels = connection
                        .get_image(
                            ImageFormat::Z_PIXMAP,
                            window,
                            20,
                            i16::try_from(expected.1 - 20).unwrap(),
                            1,
                            1,
                            u32::MAX,
                        )
                        .map_err(|e| e.to_string())?
                        .reply()
                        .map_err(|e| e.to_string())?;
                    let bytes: [u8; 4] = pixels
                        .data
                        .get(..4)
                        .ok_or("expected 32-bit native pixel")?
                        .try_into()
                        .unwrap();
                    let pixel = if connection.setup().image_byte_order
                        == x11rb::protocol::xproto::ImageOrder::LSB_FIRST
                    {
                        u32::from_le_bytes(bytes)
                    } else {
                        u32::from_be_bytes(bytes)
                    };
                    if pixel & visual.green_mask == visual.green_mask
                        && pixel & (visual.red_mask | visual.blue_mask) == 0
                    {
                        if depth == 32 && pixel >> 24 != 255 {
                            return Err(
                                "CPU window pixels are transparent to the compositor".into()
                            );
                        }
                        break;
                    }
                    if Instant::now() >= deadline {
                        return Err(format!("CPU pixel was {:?}", pixels.data));
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                // A settled tick must not ask for another UI resolve/presentation.
                let frame = handler.driver.last_frame();
                handler.tick(&self.cx);
                if handler.driver.last_frame() != frame || handler.unpainted {
                    return Err("idle CPU editor did work".into());
                }
                if self.phase.get() == 1 {
                    // Erase the native pixels and request an actual X11 Expose.
                    connection
                        .clear_area(true, window, 0, 0, 0, 0)
                        .map_err(|e| e.to_string())?
                        .check()
                        .map_err(|e| e.to_string())?;
                }
                Ok(())
            })();
            if result.is_ok() && self.phase.get() == 1 {
                self.phase.set(2);
                *self.result.borrow_mut() = Some(send);
                return Ok(());
            }
            let _ = send.send(result);
            Ok(())
        }
        fn resized(&self, size: WindowSize) -> Result<(), HandlerError> {
            if let Ok(mut h) = self.handler.try_borrow_mut() {
                h.resized(size);
            }
            Ok(())
        }
        fn on_event(&self, event: Event) -> EventStatus {
            if self.phase.get() == 2 && matches!(event, Event::Window(WindowEvent::RedrawRequested))
            {
                self.exposed.set(true);
            }
            self.handler.borrow_mut().on_event_inner(&event)
        }
    }
    for _ in 0..2 {
        let (send, recv) = channel();
        let shared = Arc::new(Mutex::new(Shared {
            ui: Ui::default(),
            view: Green,
        }));
        let window = Window::create(
            settings("MUI CPU regression", (240, 200))
                .with_resizable(false)
                .with_scale_factor_override(Some(1.0)),
            move |cx| {
                let requests = Arc::new(Requests::default());
                let mut handler = Handler::new(shared, requests, (240, 200), 1.0);
                handler.software_only = !resize_failure
                    && (handler.software_only
                        || std::env::var("WGPU_BACKEND").as_deref() != Ok("metal"));
                handler.x11_window = match cx.window_handle()?.as_raw() {
                    raw_window_handle::RawWindowHandle::Xlib(h) => h.window as u32,
                    raw_window_handle::RawWindowHandle::Xcb(h) => h.window.get(),
                    _ => unreachable!(),
                };
                handler
                    .requests
                    .x11_window
                    .store(handler.x11_window, Ordering::Release);
                Ok(Probe {
                    handler: RefCell::new(handler),
                    cx,
                    phase: std::cell::Cell::new(0),
                    exposed: std::cell::Cell::new(false),
                    resize_failure,
                    result: RefCell::new(Some(send)),
                })
            },
        )
        .expect("CPU window");
        window.show().unwrap();
        let result = recv.recv_timeout(Duration::from_secs(30));
        window.close();
        result
            .expect("CPU frame callback")
            .expect("CPU presentation");
    }
}

/// A knob that claims Escape.
struct Knob {
    value: f64,
}

impl View for Knob {
    fn build(&mut self, ui: &mut Ui, _: &Input) -> El {
        knob(ui, "k", "K", &mut self.value, 0.0..=1.0).into()
    }
    fn changed(&mut self) -> bool {
        false
    }
    fn request_resize(&mut self, _: u32, _: u32) -> bool {
        false
    }
    fn claims_key(&self, key: &Key, _: Mods) -> bool {
        *key == Key::Escape
    }
}

fn handler(size: (u32, u32), scale: f64) -> Handler<Knob> {
    let shared = Arc::new(Mutex::new(Shared {
        ui: Ui::default(),
        view: Knob { value: 0.5 },
    }));
    Handler::new(shared, Arc::default(), size, scale)
}

#[test]
fn an_error_logger_cannot_unwind_out_of_the_native_callback_guard() {
    struct PanickingLogger;
    impl View for PanickingLogger {
        fn build(&mut self, _: &mut Ui, _: &Input) -> El {
            mui::prelude::block(64., 64.)
        }
        fn changed(&mut self) -> bool {
            false
        }
        fn request_resize(&mut self, _: u32, _: u32) -> bool {
            false
        }
        fn log(&mut self, _: &str) {
            panic!("logger failed too");
        }
    }
    let shared = Arc::new(Mutex::new(Shared {
        ui: Ui::default(),
        view: PanickingLogger,
    }));
    let mut handler = Handler::new(shared, Arc::default(), (64, 64), 1.0);
    assert_eq!(
        guard(&mut handler, |_| panic!("window callback failed")),
        None::<()>
    );
}

#[test]
fn native_close_signal_is_model_free_and_fires_once_before_drop() {
    let model = Arc::new(Mutex::new(()));
    let called = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let hook: CloseHook = {
        let model = Arc::clone(&model);
        let called = Arc::clone(&called);
        Arc::new(move || {
            assert!(
                model.try_lock().is_err(),
                "fixture must retain the model borrow"
            );
            called.fetch_add(1, Ordering::Relaxed);
        })
    };
    let _borrow = model.lock().unwrap();
    let signal = NativeClose {
        hook: Some(Arc::clone(&hook)),
        fired: Cell::new(false),
    };
    signal.fire();
    signal.fire();
    drop(signal);
    assert_eq!(called.load(Ordering::Relaxed), 1);
    drop(NativeClose {
        hook: Some(hook),
        fired: Cell::new(false),
    });
    assert_eq!(called.load(Ordering::Relaxed), 2);
}

#[cfg(target_os = "linux")]
#[test]
fn dialog_parent_is_cleared_on_close_and_old_handler_drop_preserves_reopen() {
    let mut old = handler((240, 200), 1.0);
    assert_eq!(old.requests.x11_window(), None);
    old.x11_window = 42;
    old.requests.x11_window.store(42, Ordering::Release);
    assert_eq!(old.requests.x11_window(), Some(42));
    old.on_event_inner(&Event::Window(WindowEvent::WillClose));
    assert_eq!(old.requests.x11_window(), None);
    let requests = Arc::clone(&old.requests);
    requests.x11_window.store(43, Ordering::Release);
    drop(old);
    assert_eq!(requests.x11_window(), Some(43));
    let mut current = handler((240, 200), 1.0);
    current.requests = Arc::clone(&requests);
    current.x11_window = 43;
    drop(current);
    assert_eq!(requests.x11_window(), None);
}

/// Run with an X11 display and compute-capable EGL driver:
/// `WGPU_BACKEND=gl cargo test -p mui-baseview native_surface_presents_and_reopens -- --ignored`
#[cfg(target_os = "linux")]
#[test]
#[ignore = "requires a live X11 display and graphics driver"]
fn native_surface_presents_and_reopens() {
    use mui::prelude::Paints;
    use std::sync::mpsc::{Sender, channel};
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{ConnectionExt, ImageFormat};

    struct Green;
    impl View for Green {
        fn build(&mut self, _: &mut Ui, _: &Input) -> El {
            mui::prelude::block(240., 200.).fill(mui::prelude::Color::srgb(0., 1., 0.))
        }
        fn changed(&mut self) -> bool {
            false
        }
        fn request_resize(&mut self, _: u32, _: u32) -> bool {
            false
        }
    }

    struct Probe {
        // Drop graphics before the native context, as the production handler does.
        gpu: RefCell<Option<Host>>,
        cx: WindowContext,
        result: RefCell<Option<Sender<Result<(), String>>>>,
    }
    impl WindowHandler for Probe {
        fn on_frame(&self) -> Result<(), HandlerError> {
            if let Some(send) = self.result.borrow_mut().take() {
                let result = (|| {
                    let mut gpu = open_gpu(&self.cx, (240, 200))?;
                    let shared = Arc::new(Mutex::new(Shared {
                        ui: Ui::default(),
                        view: Green,
                    }));
                    let mut h = Handler::new(shared, Arc::default(), (240, 200), 1.0);
                    h.step();
                    let scene = lock(&h.shared).ui.scene_snapshot().ok_or("no scene")?;
                    for replacement in 0..3 {
                        if replacement != 0 {
                            // SAFETY: cx outlives gpu and the replacement surface.
                            #[expect(unsafe_code, reason = "exercises native surface recovery")]
                            let surface = unsafe { surface::create(gpu.instance(), &self.cx) }
                                .ok_or("surface recreation failed")?;
                            gpu.try_replace_surface(surface)
                                .map_err(|e| format!("replacement {replacement}: {e}"))?;
                            assert_eq!(gpu.generation(), replacement);
                        }
                        if !matches!(
                            gpu.present(&scene, Affine::IDENTITY)
                                .map_err(|e| e.to_string())?,
                            Frame::Presented(_)
                        ) {
                            return Err(format!(
                                "frame after replacement {replacement} was not presented"
                            ));
                        }
                    }
                    // Check actual presented pixels, including the bottom edge.
                    // An oversized GL swapchain previously shifted this image
                    // upward and left a black strip although present succeeded.
                    let window = match self.cx.window_handle().map_err(|e| e.to_string())?.as_raw()
                    {
                        raw_window_handle::RawWindowHandle::Xlib(h) => {
                            u32::try_from(h.window).map_err(|e| e.to_string())?
                        }
                        raw_window_handle::RawWindowHandle::Xcb(h) => h.window.get(),
                        _ => return Err("expected X11 native fixture".into()),
                    };
                    let (connection, _) = x11rb::rust_connection::RustConnection::connect(None)
                        .map_err(|e| e.to_string())?;
                    let visual_id = connection
                        .get_window_attributes(window)
                        .map_err(|e| e.to_string())?
                        .reply()
                        .map_err(|e| e.to_string())?
                        .visual;
                    let geometry = connection
                        .get_geometry(window)
                        .map_err(|e| e.to_string())?
                        .reply()
                        .map_err(|e| e.to_string())?;
                    if (geometry.width, geometry.height) != (240, 200) {
                        return Err(
                            "native pixel fixture must use its configured physical extent".into(),
                        );
                    }
                    let visual = connection
                        .setup()
                        .roots
                        .iter()
                        .flat_map(|s| &s.allowed_depths)
                        .flat_map(|d| &d.visuals)
                        .find(|v| v.visual_id == visual_id)
                        .ok_or("missing native visual")?;
                    let deadline = Instant::now() + Duration::from_secs(1);
                    loop {
                        let pixels = [20, 180]
                            .into_iter()
                            .map(|y| {
                                connection
                                    .get_image(ImageFormat::Z_PIXMAP, window, 20, y, 1, 1, u32::MAX)
                                    .map_err(|e| e.to_string())?
                                    .reply()
                                    .map_err(|e| e.to_string())
                            })
                            .collect::<Result<Vec<_>, _>>()?;
                        if pixels.iter().all(|p| {
                            let Some(format) = connection
                                .setup()
                                .pixmap_formats
                                .iter()
                                .find(|f| f.depth == p.depth)
                            else {
                                return false;
                            };
                            let count = usize::from(format.bits_per_pixel / 8);
                            if count == 0 || count > 4 || p.data.len() < count {
                                return false;
                            }
                            let mut bytes = [0; 4];
                            let pixel = if connection.setup().image_byte_order
                                == x11rb::protocol::xproto::ImageOrder::LSB_FIRST
                            {
                                bytes[..count].copy_from_slice(&p.data[..count]);
                                u32::from_le_bytes(bytes)
                            } else {
                                bytes[4 - count..].copy_from_slice(&p.data[..count]);
                                u32::from_be_bytes(bytes)
                            };
                            visual.green_mask != 0
                                && f64::from(pixel & visual.green_mask)
                                    / f64::from(visual.green_mask)
                                    > 0.78
                                && pixel & (visual.red_mask | visual.blue_mask) == 0
                        }) {
                            break;
                        }
                        if Instant::now() >= deadline {
                            return Err(format!(
                                "native surface lost top/bottom pixels: {:?}",
                                pixels.iter().map(|p| &p.data).collect::<Vec<_>>(),
                            ));
                        }
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    *self.gpu.borrow_mut() = Some(gpu);
                    Ok(())
                })();
                let _ = send.send(result);
            }
            Ok(())
        }
        fn resized(&self, _: WindowSize) -> Result<(), HandlerError> {
            Ok(())
        }
        fn on_event(&self, _: Event) -> EventStatus {
            EventStatus::Ignored
        }
    }

    for (parented, map_first) in [(false, false), (true, true), (true, false)] {
        for _ in 0..2 {
            let parent = parented.then(|| {
                let (send, recv) = channel();
                let window = Window::create(
                    settings("MUI regression parent", (240, 200))
                        .with_resizable(false)
                        .with_scale_factor_override(Some(1.0)),
                    move |cx| {
                        send.send(cx.platform_handle()).expect("parent handle");
                        Ok(Probe {
                            gpu: RefCell::new(None),
                            cx,
                            result: RefCell::new(None),
                        })
                    },
                )
                .expect("parent creation");
                let handle = recv
                    .recv_timeout(Duration::from_secs(30))
                    .expect("parent handle");
                if map_first {
                    window.show().expect("parent mapping before attachment");
                }
                (window, handle)
            });
            let mut options = settings("MUI presentation regression", (240, 200))
                .with_resizable(false)
                .with_scale_factor_override(Some(1.0));
            if let Some((_, handle)) = &parent {
                options = options.with_parent(handle);
            }
            let (send, recv) = channel();
            let window = Window::create(options, |cx| {
                Ok(Probe {
                    gpu: RefCell::new(None),
                    cx,
                    result: RefCell::new(Some(send)),
                })
            })
            .expect("native window creation");
            window.show().expect("native window mapping");
            if let Some((window, _)) = &parent
                && !map_first
            {
                window.show().expect("parent mapping after attachment");
            }
            let result = recv.recv_timeout(Duration::from_secs(30));
            window.close();
            result
                .expect("first frame callback")
                .expect("native presentation");
        }
    }
}

fn key(key: HostKey, code: Code, state: KeyState, modifiers: Modifiers) -> Event {
    Event::Keyboard(KeyboardEvent {
        state,
        key,
        code,
        modifiers,
        ..KeyboardEvent::default()
    })
}

#[test]
fn baseview_events_reach_the_driver_in_its_terms() {
    let mut h = handler((640, 400), 1.0);
    h.resized(WindowSize::from_logical(
        LogicalSize::new(320.0, 200.0),
        2.0,
    ));
    assert_eq!((h.driver.size(), h.driver.ui_scale()), ((640, 400), 2.0));
    h.on_event_inner(&Event::Mouse(MouseEvent::CursorMoved {
        // baseview's pointer is in pixels.
        position: PhysicalPosition::new(20.0, 40.0),
        modifiers: Modifiers::default(),
    }));
    assert_eq!(h.driver.pointer().pos, Some(Point::new(10.0, 20.0)));
    // X11 samples a press's state before the modifier is set...
    let alt = |state, m| key(HostKey::Named(NamedKey::Alt), Code::AltLeft, state, m);
    h.on_event_inner(&alt(KeyState::Down, Modifiers::default()));
    assert!(h.driver.pointer().mods.alt);
    // ...and a release's while it is still held.
    h.on_event_inner(&alt(KeyState::Up, Modifiers::ALT));
    assert!(!h.driver.pointer().mods.alt);

    h.step();
    let space = key(
        HostKey::Character(" ".into()),
        Code::Space,
        KeyState::Down,
        Modifiers::default(),
    );
    assert_eq!(h.on_event_inner(&space), EventStatus::Ignored);
    let escape = key(
        HostKey::Named(NamedKey::Escape),
        Code::Escape,
        KeyState::Down,
        Modifiers::default(),
    );
    assert_eq!(h.on_event_inner(&escape), EventStatus::Captured);
    lock(&h.shared).ui.focus("k");
    let up = key(
        HostKey::Named(NamedKey::ArrowUp),
        Code::ArrowUp,
        KeyState::Down,
        Modifiers::default(),
    );
    assert_eq!(h.on_event_inner(&up), EventStatus::Captured);
}

#[test]
fn a_redraw_request_from_the_host_thread_is_a_frame() {
    let mut h = handler((400, 300), 1.0);
    assert!(h.step(), "the first tick paints");
    for _ in 0..600 {
        h.step();
    }
    assert!(!h.step(), "a settled window paints nothing");
    h.requests.redraw();
    assert!(h.step());
}

#[test]
fn a_key_hook_hears_downs_and_ups_first_and_can_take_them() {
    let mut h = handler((400, 300), 1.0);
    let heard = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&heard);
    h.requests
        .on_key(Arc::new(Mutex::new(move |_: &Ui, e: &KeyEvent| {
            log.lock().unwrap().push((e.key.clone(), e.down));
            // Takes "a" only.
            e.key == NativeKey::Text("a".into())
        })));
    h.step();
    let a = |state| {
        key(
            HostKey::Character("a".into()),
            Code::KeyA,
            state,
            Modifiers::default(),
        )
    };
    assert_eq!(h.on_event_inner(&a(KeyState::Down)), EventStatus::Captured);
    assert_eq!(h.on_event_inner(&a(KeyState::Up)), EventStatus::Captured);
    // Not taken: MUI routes it as before, and the knob claims Escape.
    let escape = key(
        HostKey::Named(NamedKey::Escape),
        Code::Escape,
        KeyState::Down,
        Modifiers::default(),
    );
    assert_eq!(h.on_event_inner(&escape), EventStatus::Captured);
    let space = key(
        HostKey::Character(" ".into()),
        Code::Space,
        KeyState::Down,
        Modifiers::default(),
    );
    assert_eq!(h.on_event_inner(&space), EventStatus::Ignored);
    assert_eq!(
        *heard.lock().unwrap(),
        [
            (NativeKey::Text("a".into()), true),
            (NativeKey::Text("a".into()), false),
            (NativeKey::Named(Key::Escape), true),
            (NativeKey::Text(" ".into()), true),
        ]
    );
}

#[test]
fn ime_configuration_converts_geometry_and_keeps_native_text_ranges() {
    let source = mui::host::ImeConfiguration {
        id: "field".into(),
        area: (Point::new(10.0, 20.0), mui::scene::Size::new(1.0, 16.0)),
        text: "a😀é".into(),
        selection: 1..5,
        marked: Some(1..5),
    };
    let a = native_ime(source.clone(), 1.5);
    assert_eq!(a.position, PhysicalPosition::new(15.0, 30.0));
    assert_eq!(a.size, baseview::dpi::PhysicalSize::new(1.5, 24.0));
    assert_eq!((a.selection.clone(), a.marked.clone()), (1..5, Some(1..5)));
    assert_eq!(a, native_ime(source.clone(), 1.5));
    assert_ne!(
        a,
        native_ime(source, 2.0),
        "DPI changes update candidate placement"
    );
}

#[test]
fn native_composition_reaches_the_driver_once() {
    struct InputSpy {
        seen: Vec<mui::prelude::Ime>,
        text: String,
    }
    impl View for InputSpy {
        fn build(&mut self, _: &mut Ui, input: &Input) -> El {
            self.seen.extend(input.ime.clone());
            self.text.push_str(&input.text);
            mui::prelude::block(20.0, 20.0)
        }
        fn changed(&mut self) -> bool {
            false
        }
        fn request_resize(&mut self, _: u32, _: u32) -> bool {
            false
        }
    }
    let shared = Arc::new(Mutex::new(Shared {
        ui: Ui::default(),
        view: InputSpy {
            seen: Vec::new(),
            text: String::new(),
        },
    }));
    let mut h = Handler::new(Arc::clone(&shared), Arc::default(), (200, 100), 1.0);
    h.step();
    for event in [
        baseview::Ime::Enabled,
        baseview::Ime::Selection(1..5),
        baseview::Ime::Preedit {
            text: "日本".into(),
            cursor: Some((6, 6)),
        },
        baseview::Ime::Commit("日本".into()),
        baseview::Ime::Disabled,
    ] {
        assert_eq!(h.on_event_inner(&Event::Ime(event)), EventStatus::Captured);
    }
    h.step();
    let s = lock(&shared);
    assert_eq!(s.view.seen.len(), 5);
    assert!(matches!(&s.view.seen[3], mui::prelude::Ime::Commit(s) if s == "日本"));
    assert!(
        s.view.text.is_empty(),
        "composition has its own channel, never duplicated as typed text"
    );
}

#[test]
fn scene_snapshot_does_not_hold_the_model_lock() {
    let mut h = handler((640, 400), 1.0);
    h.step();
    let snapshot = lock(&h.shared).ui.scene_snapshot().unwrap();
    let mut model = h.shared.try_lock().expect("native callback can reenter");
    assert!(Arc::ptr_eq(&snapshot, &model.ui.scene_snapshot().unwrap()));
    model.ui.blur();
    assert!(snapshot.surface("k").is_some());
}

#[test]
fn queued_native_callbacks_can_reenter_and_preserve_event_order() {
    let queue = RefCell::new(VecDeque::from([1, 2]));
    let mut delivered = Vec::new();
    drain_events(&queue, |event| {
        delivered.push(event);
        if event == 1 {
            queue.borrow_mut().push_back(3);
        }
    });
    assert_eq!(delivered, [1, 2, 3]);
    assert!(queue.borrow().is_empty());
}
