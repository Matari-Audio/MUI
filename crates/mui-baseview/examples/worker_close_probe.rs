//! Native regression: a blocked View::build must leave the parent responsive,
//! and child close must return before the blocked frame is released.
//! Run on Windows; Linux/X11 can exercise the same lifecycle contract.
use baseview::{
    Event, EventStatus, HandlerError, Window, WindowContext, WindowEvent, WindowHandler,
    WindowSettings, WindowSize, dpi::LogicalSize,
};
use mui::host::{Shared, View};
use mui::prelude::*;
use mui_baseview::{Requests, baseview, open};
use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

#[derive(Default)]
struct Gate {
    entered: AtomicBool,
    released: Mutex<bool>,
    changed: Condvar,
}
impl Gate {
    fn release(&self) {
        *self.released.lock().unwrap() = true;
        self.changed.notify_all();
    }
}
struct Blocked(Arc<Gate>);
impl View for Blocked {
    fn build(&mut self, _: &mut Ui, _: &Input) -> El {
        self.0.entered.store(true, Ordering::Release);
        drop(
            self.0
                .changed
                .wait_while(self.0.released.lock().unwrap(), |released| !*released)
                .unwrap(),
        );
        block(240., 200.).fill(Color::srgb(0., 1., 0.))
    }
    fn changed(&mut self) -> bool {
        false
    }
    fn request_resize(&mut self, _: u32, _: u32) -> bool {
        false
    }
}
struct Parent {
    cx: WindowContext,
    child: RefCell<Option<Window>>,
    requests: Arc<Requests>,
    gate: Arc<Gate>,
    started: Cell<bool>,
    beats: Cell<u32>,
    busy: Cell<bool>,
    result: Arc<Mutex<Option<Result<(), String>>>>,
}
impl Parent {
    fn finish(&self, result: Result<(), String>) {
        self.result.lock().unwrap().get_or_insert(result);
        // Release only after the measured close returned. Cleanup is safe on
        // every error path, including parent destruction.
        self.gate.release();
        if let Some(child) = self.child.borrow_mut().take() {
            child.close();
        }
        self.cx.request_close();
    }
}
impl WindowHandler for Parent {
    fn on_frame(&self) -> Result<(), HandlerError> {
        if self.busy.replace(true) {
            return Ok(());
        }
        if !self.started.replace(true) {
            let shared = Arc::new(Mutex::new(Shared {
                ui: Ui::default(),
                view: Blocked(self.gate.clone()),
            }));
            let child = open(
                &self.cx,
                "Blocked editor",
                (240, 200),
                Some(1.),
                shared,
                self.requests.clone(),
            );
            if child.is_none() {
                self.finish(Err("child did not open".into()));
            }
            *self.child.borrow_mut() = child;
        } else if self.gate.entered.load(Ordering::Acquire) {
            let beats = self.beats.get() + 1;
            self.beats.set(beats);
            if beats == 4 {
                let child = self.child.borrow_mut().take();
                let start = Instant::now();
                if let Some(child) = child {
                    child.close();
                }
                let elapsed = start.elapsed();
                if elapsed >= Duration::from_millis(100) {
                    self.finish(Err(format!(
                        "native close waited {elapsed:?} for blocked build"
                    )));
                } else {
                    eprintln!(
                        "worker_close_probe: parent handled four frames during blocked build; close={elapsed:?}"
                    );
                    self.finish(Ok(()));
                }
            }
        }
        self.busy.set(false);
        Ok(())
    }
    fn resized(&self, _: WindowSize) -> Result<(), HandlerError> {
        Ok(())
    }
    fn on_event(&self, event: Event) -> EventStatus {
        if matches!(event, Event::Window(WindowEvent::WillClose)) {
            self.gate.release();
        }
        EventStatus::Ignored
    }
}
fn main() -> std::process::ExitCode {
    // This watchdog is independent of the GUI: the old Windows architecture
    // deadlocks in build before the parent's next frame can release the gate.
    let done = Arc::new((Mutex::new(false), Condvar::new()));
    let signal = done.clone();
    let watchdog = std::thread::spawn(move || {
        let (lock, changed) = &*signal;
        let (finished, _) = changed
            .wait_timeout_while(lock.lock().unwrap(), Duration::from_secs(15), |done| !*done)
            .unwrap();
        if !*finished {
            eprintln!("worker_close_probe: host event loop blocked");
            std::process::exit(2);
        }
    });
    // SAFETY: this standalone probe is the only window library in its process.
    #[expect(unsafe_code, reason = "standalone probe process")]
    unsafe {
        baseview::assume_standalone_in_process();
    }
    let result = Arc::new(Mutex::new(None));
    let outcome = result.clone();
    let parent = Window::create(
        WindowSettings::new()
            .with_title("Editor worker close probe")
            .with_size(LogicalSize::new(400., 300.)),
        move |cx| {
            Ok(Parent {
                cx,
                child: RefCell::new(None),
                requests: Arc::default(),
                gate: Arc::default(),
                started: Cell::new(false),
                beats: Cell::new(0),
                busy: Cell::new(false),
                result: outcome,
            })
        },
    );
    let native = parent
        .and_then(Window::run_until_closed)
        .map_err(|e| e.to_string());
    let verdict = native.and_then(|()| {
        result
            .lock()
            .unwrap()
            .take()
            .unwrap_or_else(|| Err("parent closed without verdict".into()))
    });
    let (lock, changed) = &*done;
    *lock.lock().unwrap() = true;
    changed.notify_all();
    let _ = watchdog.join();
    match verdict {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("worker_close_probe: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
