//! Exercise the public 0.4 work API through the actual native scheduler.
#![cfg(target_os = "linux")]
#![allow(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    reason = "bounded native fixture assertions"
)]
use moose_baseview::{
    Event, EventStatus, FrameDemand, HandlerError, TimerHandle, Window, WindowContext, WindowEvent,
    WindowHandler, WindowSettings, WindowSize,
};
use std::{
    cell::RefCell,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{channel, Sender},
        Arc,
    },
    time::{Duration, Instant},
};

#[test]
#[ignore = "requires an isolated X11 display"]
fn native_poll_timer_delayed_draw_and_close_revoke_work() {
    struct Probe {
        cx: WindowContext,
        timer: RefCell<Option<TimerHandle>>,
        trace: Sender<(&'static str, Instant)>,
        close: Arc<AtomicBool>,
    }
    impl WindowHandler for Probe {
        fn frame_demand(&self) -> FrameDemand {
            FrameDemand::Idle
        }
        fn draw(&self) -> Result<(), HandlerError> {
            self.trace.send(("draw", Instant::now())).unwrap();
            Ok(())
        }
        fn poll(&self) {
            self.trace.send(("poll", Instant::now())).unwrap();
            if self.close.load(Ordering::Acquire) {
                self.cx.request_close();
            }
        }
        fn on_timer(&self, timer: &TimerHandle) {
            assert!(self.timer.borrow().as_ref().unwrap() == timer);
            self.trace.send(("timer", Instant::now())).unwrap();
        }
        fn resized(&self, _: WindowSize) -> Result<(), HandlerError> {
            Ok(())
        }
        fn on_event(&self, event: Event) -> EventStatus {
            if matches!(event, Event::Window(WindowEvent::WillClose)) {
                self.trace.send(("closed", Instant::now())).unwrap();
            }
            EventStatus::Ignored
        }
    }
    let (send, receive) = channel();
    let (built, ready) = channel();
    let close = Arc::new(AtomicBool::new(false));
    let should_close = Arc::clone(&close);
    let window =
        Window::create(WindowSettings::new().with_title("baseview current work API"), move |cx| {
            let timer = cx.create_timer(Duration::from_millis(80))?;
            built.send(cx.waker()).unwrap();
            Ok(Probe { cx, timer: RefCell::new(Some(timer)), trace: send, close: should_close })
        })
        .unwrap();
    window.show().unwrap();
    let waker = ready.recv_timeout(Duration::from_secs(1)).unwrap();
    // Settle native map/focus/exposure before testing internal-only wake semantics.
    std::thread::sleep(Duration::from_millis(300));
    let initial = receive.try_iter().collect::<Vec<_>>();
    assert!(initial.iter().any(|(event, _)| *event == "draw"));
    assert!(initial.iter().any(|(event, _)| *event == "timer"));
    window.hide().unwrap();
    window.show().unwrap();
    let mut restored = false;
    let until = Instant::now() + Duration::from_millis(200);
    while let Ok((event, _)) = receive.recv_timeout(until.saturating_duration_since(Instant::now()))
    {
        restored |= event == "draw";
    }
    assert!(restored, "show did not redraw an idle handler");
    let external = waker.clone();
    std::thread::spawn(move || external.request_poll()).join().unwrap();
    let until = Instant::now() + Duration::from_millis(100);
    let mut polled = false;
    while let Ok((event, _)) = receive.recv_timeout(until.saturating_duration_since(Instant::now()))
    {
        assert_ne!(event, "draw", "poll/timer work forced a draw");
        polled |= event == "poll";
    }
    assert!(polled);
    let requested = Instant::now();
    let delay = Duration::from_millis(120);
    waker.request_redraw_after(delay);
    loop {
        let (event, at) = receive.recv_timeout(Duration::from_secs(1)).unwrap();
        if event == "draw" {
            assert!(at >= requested + delay);
            break;
        }
    }
    close.store(true, Ordering::Release);
    waker.request_poll();
    loop {
        let (event, _) = receive.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_ne!(event, "draw", "poll close must suppress presentation");
        if event == "closed" {
            break;
        }
    }
    window.close();
    waker.request_poll();
    waker.request_redraw();
    assert!(receive.try_iter().next().is_none());
}

/// A transparent utility window needs ARGB without taking input from a covering
/// application. Run under Xvfb; xdotool supplies actual server input, not SendEvent.
#[test]
#[ignore = "requires an isolated X11 display and xdotool"]
fn transparent_window_preserves_overlay_input() {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use x11rb::{
        connection::Connection,
        protocol::{
            xproto::{
                ConfigureWindowAux, ConnectionExt, CreateWindowAux, EventMask, InputFocus,
                StackMode, WindowClass,
            },
            Event as XEvent,
        },
        CURRENT_TIME,
    };
    struct Probe;
    impl WindowHandler for Probe {
        fn draw(&self) -> Result<(), HandlerError> {
            Ok(())
        }
        fn resized(&self, _: WindowSize) -> Result<(), HandlerError> {
            Ok(())
        }
        fn on_event(&self, _: Event) -> EventStatus {
            EventStatus::Ignored
        }
    }
    let (send, receive) = channel();
    let window = Window::create(
        WindowSettings::new().with_title("transparent baseview probe").with_transparent(true),
        move |cx| {
            let id = match cx.window_handle().unwrap().as_raw() {
                RawWindowHandle::Xlib(h) => h.window as u32,
                RawWindowHandle::Xcb(h) => h.window.get(),
                _ => panic!("requires X11"),
            };
            send.send(id).unwrap();
            Ok(Probe)
        },
    )
    .unwrap();
    window.show().unwrap();
    let id = receive.recv_timeout(Duration::from_secs(2)).unwrap();
    let (conn, screen) = x11rb::connect(None).unwrap();
    assert_eq!(conn.get_geometry(id).unwrap().reply().unwrap().depth, 32);
    let root = conn.setup().roots.get(screen).unwrap().root;
    let overlay = conn.generate_id().unwrap();
    conn.create_window(
        0,
        overlay,
        root,
        0,
        0,
        500,
        400,
        0,
        WindowClass::INPUT_OUTPUT,
        0,
        &CreateWindowAux::new().event_mask(EventMask::BUTTON_PRESS | EventMask::KEY_PRESS),
    )
    .unwrap()
    .check()
    .unwrap();
    conn.map_window(overlay).unwrap().check().unwrap();
    conn.configure_window(overlay, &ConfigureWindowAux::new().stack_mode(StackMode::ABOVE))
        .unwrap()
        .check()
        .unwrap();
    conn.set_input_focus(InputFocus::POINTER_ROOT, overlay, CURRENT_TIME).unwrap().check().unwrap();
    assert!(std::process::Command::new("xdotool")
        .args(["mousemove", "100", "100", "click", "1", "type", "overlay"])
        .status()
        .unwrap()
        .success());
    let mut clicks = 0;
    let mut keys = 0;
    let until = Instant::now() + Duration::from_millis(300);
    while Instant::now() < until {
        assert_eq!(conn.get_input_focus().unwrap().reply().unwrap().focus, overlay);
        while let Some(event) = conn.poll_for_event().unwrap() {
            match event {
                XEvent::ButtonPress(e) if e.event == overlay => clicks += 1,
                XEvent::KeyPress(e) if e.event == overlay => keys += 1,
                _ => {}
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(clicks, 1);
    assert_eq!(keys, 7);
    conn.destroy_window(overlay).unwrap().check().unwrap();
    window.close();
}
