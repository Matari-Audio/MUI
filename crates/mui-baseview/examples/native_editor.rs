//! Native parent/child editor smoke test. Set MUI_EXPECT_SOFTWARE=1 when
//! deliberately selecting an unavailable WGPU_BACKEND to test CPU fallback.

use baseview::{
    Event, EventStatus, HandlerError, Window, WindowContext, WindowEvent, WindowHandler,
    WindowSettings, WindowSize, dpi::LogicalSize,
};
use mui::host::{Shared, View};
use mui::prelude::*;
use mui_baseview::Requests;
use std::cell::{Cell, RefCell};
use std::process::ExitCode;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

const INITIAL: (u32, u32) = (240, 200);
const RESIZED: (u32, u32) = (320, 240);
type Outcome = Arc<Mutex<Option<Result<(), String>>>>;

struct Green;

impl View for Green {
    fn build(&mut self, _: &mut Ui, _: &Input) -> El {
        block(320., 240.).fill(Color::srgb(0., 1., 0.))
    }

    fn changed(&mut self) -> bool {
        false
    }

    fn request_resize(&mut self, _: u32, _: u32) -> bool {
        false
    }

    fn log(&mut self, line: &str) {
        eprintln!("{line}");
    }
}

enum Phase {
    Open,
    Frame,
    Resized,
}

struct State {
    // The child is closed before its model or the parent's native context drops.
    child: Option<Window>,
    shared: Arc<Mutex<Shared<Green>>>,
    requests: Arc<Requests>,
    closes: Arc<AtomicUsize>,
    phase: Phase,
    round: usize,
    first_frame: u64,
}

struct Parent {
    state: RefCell<State>,
    cx: WindowContext,
    outcome: Outcome,
    busy: Cell<bool>,
    started: Instant,
    software: bool,
}

impl Parent {
    fn advance(&self) -> Result<bool, String> {
        if self.started.elapsed() > Duration::from_secs(35) {
            return Err("native editor phases did not finish within 35 seconds".into());
        }
        let mut state = self.state.borrow_mut();
        match state.phase {
            Phase::Open => {
                if state.closes.load(Ordering::Acquire) != state.round {
                    return Err("previous editor did not signal exactly one close".into());
                }
                state.requests = Arc::new(Requests::default());
                let closes = Arc::clone(&state.closes);
                state.requests.on_close(Arc::new(move || {
                    closes.fetch_add(1, Ordering::AcqRel);
                }));
                eprintln!("native_editor: round {} opening child", state.round + 1);
                state.child = Some(
                    mui_baseview::open(
                        &self.cx,
                        "MUI native editor smoke",
                        INITIAL,
                        Some(1.0),
                        Arc::clone(&state.shared),
                        Arc::clone(&state.requests),
                    )
                    .ok_or("MUI child creation failed")?,
                );
                state.phase = Phase::Frame;
            }
            Phase::Frame | Phase::Resized => {
                let child = state.child.as_ref().ok_or("missing child")?;
                if !child.is_open() {
                    return Err("native child is not open during its presentation checks".into());
                }
                if state.closes.load(Ordering::Acquire) != state.round {
                    return Err("child closed before its presentation checks completed".into());
                }
                let Some(presented) = state.requests.presentation() else {
                    return Ok(false);
                };
                if presented.software != self.software {
                    return Err(format!(
                        "expected software={}, observed {presented:?}",
                        self.software
                    ));
                }
                let resized = matches!(state.phase, Phase::Resized);
                let wanted = if resized { RESIZED } else { INITIAL };
                if presented.size != wanted || presented.frames <= state.first_frame {
                    return Ok(false);
                }
                let native = child.size();
                if (native.physical.width, native.physical.height) != wanted {
                    return Ok(false);
                }
                if state
                    .shared
                    .lock()
                    .map_err(|e| e.to_string())?
                    .ui
                    .scene()
                    .is_none()
                {
                    return Err("presentation did not have a resolved MUI scene".into());
                }
                eprintln!(
                    "native_editor: round {} {} presented {presented:?}",
                    state.round + 1,
                    if resized { "resized" } else { "initial" }
                );
                if resized {
                    eprintln!("native_editor: round {} closing child", state.round + 1);
                    state.child.take().ok_or("missing child at close")?.close();
                    state.round += 1;
                    if state.closes.load(Ordering::Acquire) != state.round {
                        return Err("child close hook did not fire exactly once".into());
                    }
                    state.first_frame = 0;
                    state.phase = Phase::Open;
                    return Ok(state.round == 2);
                }
                state.first_frame = presented.frames;
                self.cx
                    .resize(LogicalSize::new(420., 340.))
                    .map_err(|e| e.to_string())?;
                state.requests.resize(RESIZED.0, RESIZED.1);
                state.requests.redraw();
                state.phase = Phase::Resized;
            }
        }
        Ok(false)
    }

    fn finish(&self, result: Result<(), String>) {
        *self.outcome.lock().unwrap() = Some(result);
        let child = self.state.borrow_mut().child.take();
        if let Some(child) = child {
            child.close();
        }
        self.cx.request_close();
    }
}

impl WindowHandler for Parent {
    fn on_frame(&self) -> Result<(), HandlerError> {
        // Opening, resizing and closing can synchronously re-enter native callbacks.
        if self.busy.replace(true) {
            return Ok(());
        }
        let result = self.advance();
        match result {
            Ok(true) => self.finish(Ok(())),
            Err(error) => self.finish(Err(error)),
            Ok(false) => {}
        }
        self.busy.set(false);
        Ok(())
    }

    fn resized(&self, _: WindowSize) -> Result<(), HandlerError> {
        Ok(())
    }

    fn on_event(&self, event: Event) -> EventStatus {
        if matches!(event, Event::Window(WindowEvent::WillClose)) {
            // Never borrow the model or block a re-entrant callback on a state borrow.
            let child = self
                .state
                .try_borrow_mut()
                .ok()
                .and_then(|mut state| state.child.take());
            if let Some(child) = child {
                child.close();
            }
        }
        EventStatus::Ignored
    }
}

fn run() -> Result<(), String> {
    let software = std::env::var("MUI_EXPECT_SOFTWARE").as_deref() == Ok("1");
    eprintln!("native_editor: expected software={software}");
    // SAFETY: this executable owns the process and uses only this baseview instance.
    #[expect(unsafe_code, reason = "standalone native smoke executable")]
    unsafe {
        baseview::assume_standalone_in_process();
    }
    let outcome: Outcome = Arc::new(Mutex::new(None));
    let result = Arc::clone(&outcome);
    let parent = Window::create(
        WindowSettings::new()
            .with_title("MUI native editor smoke parent")
            .with_size(LogicalSize::new(400., 300.))
            .with_scale_factor_override(Some(1.0)),
        move |cx| {
            Ok(Parent {
                state: RefCell::new(State {
                    child: None,
                    shared: Arc::new(Mutex::new(Shared {
                        ui: Ui::default(),
                        view: Green,
                    })),
                    requests: Arc::new(Requests::default()),
                    closes: Arc::new(AtomicUsize::new(0)),
                    phase: Phase::Open,
                    round: 0,
                    first_frame: 0,
                }),
                cx,
                outcome: result,
                busy: Cell::new(false),
                started: Instant::now(),
                software,
            })
        },
    )
    .map_err(|e| e.to_string())?;
    // macOS and Windows pump their native event loops on this executable's main thread.
    parent.run_until_closed().map_err(|e| e.to_string())?;
    outcome
        .lock()
        .unwrap()
        .take()
        .unwrap_or_else(|| Err("parent closed before both editor rounds completed".into()))
}

fn main() -> ExitCode {
    // Covers a native call that stalls before the callback deadline can run.
    // This thread never touches native handles or the UI model and is joined on normal exit.
    let done = Arc::new((Mutex::new(false), Condvar::new()));
    let signal = Arc::clone(&done);
    let watchdog = std::thread::spawn(move || {
        let (lock, wake) = &*signal;
        let (finished, _) = wake
            .wait_timeout_while(lock.lock().unwrap(), Duration::from_secs(45), |done| !*done)
            .unwrap();
        if !*finished {
            eprintln!("native_editor: FAIL: native event loop or teardown stalled for 45 seconds");
            std::process::exit(1);
        }
    });
    let result = run();
    *done.0.lock().unwrap() = true;
    done.1.notify_one();
    watchdog.join().unwrap();
    match result {
        Ok(()) => {
            eprintln!("native_editor: PASS: child presented, resized, closed and reopened twice");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("native_editor: FAIL: {error}");
            ExitCode::FAILURE
        }
    }
}
