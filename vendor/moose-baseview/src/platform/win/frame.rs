use super::{
    frame_state::{FrameState, Wake},
    window::BV_FRAME,
};
use crate::{FrameDemand, FrameRequester};
use std::{
    sync::{Arc, Condvar, Mutex, PoisonError},
    thread::JoinHandle,
    time::{Duration, Instant},
};
use windows_sys::Win32::{Foundation::HWND, Graphics::Dwm::DwmFlush, UI::WindowsAndMessaging::*};

pub(super) struct FrameSignal {
    serial: usize,
    image_marker: usize,
    state: Mutex<FrameState>,
    changed: Condvar,
}
impl FrameSignal {
    pub fn new() -> Arc<Self> {
        static SERIAL: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        Arc::new(Self {
            serial: SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            image_marker: std::ptr::addr_of!(SERIAL) as usize,
            state: Mutex::new(FrameState::new()),
            changed: Condvar::new(),
        })
    }
    pub fn matches_message(&self, serial: usize, marker: isize) -> bool {
        serial == self.serial && marker as usize == self.image_marker
    }
    pub fn requester(self: &Arc<Self>) -> FrameRequester {
        let signal = Arc::clone(self);
        FrameRequester::new(move || signal.request())
    }
    pub fn request(&self) {
        self.state.lock().unwrap_or_else(PoisonError::into_inner).request();
        self.changed.notify_all();
    }
    pub fn demand(&self, demand: FrameDemand) {
        self.state.lock().unwrap_or_else(PoisonError::into_inner).set_demand(demand);
        self.changed.notify_all();
    }
    pub fn visible(&self, visible: bool) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.visible = visible;
        if visible {
            state.request();
        }
        drop(state);
        self.changed.notify_all();
    }
    pub fn complete(&self) {
        self.state.lock().unwrap_or_else(PoisonError::into_inner).pending = false;
        self.changed.notify_all();
    }
    pub fn revoke(&self) {
        self.state.lock().unwrap_or_else(PoisonError::into_inner).revoke();
        self.changed.notify_all();
    }
    fn wait_for(&self, delay: Duration) {
        let started = Instant::now();
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        while !state.revoked && state.visible && started.elapsed() < delay {
            let left = delay.saturating_sub(started.elapsed());
            state =
                self.changed.wait_timeout(state, left).unwrap_or_else(PoisonError::into_inner).0;
        }
    }
    fn wait_fallback(&self, delay: Duration) {
        let started = Instant::now();
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        // Unchanged demand notifications must not shorten the no-DWM cadence.
        // Close or a change to Idle/At still interrupts the wait immediately.
        while matches!(state.next(Instant::now()), Wake::Frame { compositor: true })
            && started.elapsed() < delay
        {
            let left = delay.saturating_sub(started.elapsed());
            state =
                self.changed.wait_timeout(state, left).unwrap_or_else(PoisonError::into_inner).0;
        }
    }
}

/// Only this thread waits on DwmFlush. The host GUI thread never waits on it.
/// Idle/pending/hidden waits have no timer; deadlines and compositor demand are
/// explicit. No callback, renderer, or host pointer crosses to the worker.
pub(super) struct FramePacer {
    signal: Arc<FrameSignal>,
    thread: Option<JoinHandle<()>>,
}
impl FramePacer {
    pub fn start(hwnd: HWND, signal: Arc<FrameSignal>) -> std::io::Result<Self> {
        signal.state.lock().unwrap_or_else(PoisonError::into_inner).hwnd = Some(hwnd as usize);
        let worker = Arc::clone(&signal);
        let thread =
            std::thread::Builder::new().name("baseview-frame-pacer".into()).spawn(move || {
                let fallback = crate::platform::frame_rate::frame_interval(None);
                loop {
                    let mut state = worker.state.lock().unwrap_or_else(PoisonError::into_inner);
                    match state.next(Instant::now()) {
                        Wake::Stop => break,
                        Wake::Wait => {
                            drop(
                                worker.changed.wait(state).unwrap_or_else(PoisonError::into_inner),
                            );
                            continue;
                        }
                        Wake::At(at) => {
                            let delay = at.saturating_duration_since(Instant::now());
                            drop(
                                worker
                                    .changed
                                    .wait_timeout(state, delay)
                                    .unwrap_or_else(PoisonError::into_inner),
                            );
                            continue;
                        }
                        Wake::Frame { compositor } => {
                            let Some(hwnd) = state.hwnd else {
                                continue;
                            };
                            // Preserve due work while an ancestor is hidden/iconic.
                            // Only due work polls here; settled Idle has no timers.
                            if unsafe {
                                IsIconic(GetAncestor(hwnd as HWND, GA_ROOT)) != 0
                                    || IsWindowVisible(hwnd as HWND) == 0
                            } {
                                drop(state);
                                worker.wait_for(Duration::from_millis(50));
                                continue;
                            }
                            if compositor {
                                drop(state);
                                let started = Instant::now();
                                let flushed = unsafe { DwmFlush() } >= 0;
                                if !flushed || started.elapsed() < Duration::from_millis(1) {
                                    worker.wait_fallback(fallback);
                                }
                                state = worker.state.lock().unwrap_or_else(PoisonError::into_inner);
                            }
                        }
                    }
                    // Demand can change while DwmFlush blocks. Serialize the final
                    // check/post with revoke, so no recycled HWND can be targeted.
                    if matches!(state.next(Instant::now()), Wake::Frame { .. }) {
                        let Some(hwnd) = state.hwnd else {
                            continue;
                        };
                        if unsafe {
                            PostMessageW(
                                hwnd as HWND,
                                BV_FRAME,
                                worker.serial,
                                worker.image_marker as isize,
                            )
                        } != 0
                        {
                            state.posted();
                        } else {
                            state.revoke();
                        }
                    }
                }
            })?;
        Ok(Self { signal, thread: Some(thread) })
    }
}
impl Drop for FramePacer {
    fn drop(&mut self) {
        self.signal.revoke();
        if let Some(thread) = self.thread.take() {
            // Kernel completion includes TLS destructors. is_finished() alone
            // can be true just before TLS cleanup, which could still block join.
            use std::os::windows::io::AsRawHandle;
            let finished = unsafe {
                windows_sys::Win32::System::Threading::WaitForSingleObject(
                    thread.as_raw_handle(),
                    25,
                ) == windows_sys::Win32::Foundation::WAIT_OBJECT_0
            };
            if finished {
                let _ = thread.join();
            } else {
                // create_window pinned the image before publishing callbacks.
                // A stuck driver wait may resume, but its target is revoked.
                super::callback::guard(
                    "pacer stop diagnostic",
                    || (),
                    || {
                        crate::warn!("Frame pacer stop exceeded 25 ms; detached from pinned image");
                    },
                );
            }
        }
    }
}
