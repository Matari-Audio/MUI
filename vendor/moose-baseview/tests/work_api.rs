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
