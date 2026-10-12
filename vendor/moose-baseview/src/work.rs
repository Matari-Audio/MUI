//! Current baseview work APIs adapted to the fork's native lifetime/pacing guards.
use crate::{
    DamageArea, Error, Event, EventStatus, FrameDemand, FrameRequester, HandlerError, WindowEvent,
    WindowHandler, WindowSize,
};
use std::{
    marker::PhantomData,
    sync::{Arc, Mutex, MutexGuard, Weak},
    time::{Duration, Instant},
};

/// A weak, thread-safe window wake handle. It never owns native resources.
#[derive(Clone)]
pub struct WindowWaker {
    work: Weak<WindowWork>,
}

impl WindowWaker {
    /// Schedules a frame without reporting external damage.
    pub fn request_redraw(&self) {
        if let Some(work) = self.work.upgrade() {
            work.request_redraw(None);
        }
    }

    /// Schedules a frame after a delay. Earlier requests take precedence.
    pub fn request_redraw_after(&self, duration: Duration) {
        if let Some(work) = self.work.upgrade() {
            if let Some(deadline) = Instant::now().checked_add(duration) {
                work.request_redraw(Some(deadline));
            }
        }
    }

    /// Schedules a poll callback without forcing presentation.
    pub fn request_poll(&self) {
        if let Some(work) = self.work.upgrade() {
            let _ = work.request_poll();
        }
    }
}

/// Weak identity for a repeating GUI timer. Dropping it does not stop the timer.
/// The timer is revoked when the window closes. Handles stay on the GUI thread.
#[derive(Clone)]
pub struct TimerHandle {
    token: Weak<()>,
    _nonsend: PhantomData<*mut ()>,
}

impl PartialEq for TimerHandle {
    fn eq(&self, other: &Self) -> bool {
        self.token.ptr_eq(&other.token)
    }
}
impl Eq for TimerHandle {}
impl PartialEq<&TimerHandle> for TimerHandle {
    fn eq(&self, other: &&TimerHandle) -> bool {
        self == *other
    }
}
impl PartialEq<TimerHandle> for &TimerHandle {
    fn eq(&self, other: &TimerHandle) -> bool {
        *self == other
    }
}

#[derive(Debug)]
struct WorkError(&'static str);
impl std::fmt::Display for WorkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for WorkError {}
fn error(message: &'static str) -> Error {
    HandlerError::from(WorkError(message)).into()
}

struct Timer {
    token: Arc<()>,
    interval: Duration,
    next: Instant,
}

struct WorkState {
    closed: bool,
    wake: Option<FrameRequester>,
    redraw: bool,
    redraw_at: Option<Instant>,
    poll: bool,
    full_damage: bool,
    precise_damage: bool,
    timers: Vec<Timer>,
}
impl Default for WorkState {
    fn default() -> Self {
        Self {
            closed: false,
            wake: None,
            redraw: true,
            redraw_at: None,
            poll: false,
            full_damage: true,
            precise_damage: false,
            timers: Vec::new(),
        }
    }
}

#[derive(Default)]
pub(crate) struct WindowWork(Mutex<WorkState>);
impl WindowWork {
    fn state(&self) -> MutexGuard<'_, WorkState> {
        self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }
    pub(crate) fn waker(self: &Arc<Self>) -> WindowWaker {
        WindowWaker { work: Arc::downgrade(self) }
    }
    pub(crate) fn bind_wake(&self, wake: FrameRequester) {
        let mut state = self.state();
        if !state.closed {
            state.wake = Some(wake);
        }
    }
    #[cfg(target_os = "macos")]
    pub(crate) fn invalidate(&self) {
        {
            let mut state = self.state();
            if state.closed {
                return;
            }
            state.full_damage = true;
        }
        self.request_redraw(None);
    }
    pub(crate) fn close(&self) {
        let mut state = self.state();
        state.closed = true;
        state.wake = None;
        state.timers.clear();
    }
    fn is_closed(&self) -> bool {
        self.state().closed
    }
    fn request_redraw(&self, at: Option<Instant>) {
        let wake = {
            let mut state = self.state();
            if state.closed {
                return;
            }
            if let Some(at) = at {
                state.redraw_at = Some(state.redraw_at.map_or(at, |pending| pending.min(at)));
            } else {
                state.redraw = true;
            }
            state.wake.clone()
        };
        if let Some(wake) = wake {
            wake.request_frame();
        }
    }
    pub(crate) fn request_poll(&self) -> Result<(), Error> {
        let wake = {
            let mut state = self.state();
            if state.closed {
                return Err(error("window is closed"));
            }
            state.poll = true;
            state.wake.clone()
        };
        if let Some(wake) = wake {
            wake.request_frame();
        }
        Ok(())
    }
    pub(crate) fn create_timer(&self, interval: Duration) -> Result<TimerHandle, Error> {
        let next = Instant::now()
            .checked_add(interval)
            .filter(|_| !interval.is_zero())
            .ok_or_else(|| error("invalid GUI timer interval"))?;
        let token = Arc::new(());
        let wake = {
            let mut state = self.state();
            if state.closed {
                return Err(error("window is closed"));
            }
            // A faulty GUI must not grow a native work registry without a bound.
            if state.timers.len() >= 1024 {
                return Err(error("GUI timer capacity exceeded"));
            }
            state.timers.push(Timer { token: Arc::clone(&token), interval, next });
            state.wake.clone()
        };
        if let Some(wake) = wake {
            wake.request_frame();
        }
        Ok(TimerHandle { token: Arc::downgrade(&token), _nonsend: PhantomData })
    }
    fn demand(&self, user: FrameDemand, now: Instant) -> FrameDemand {
        let state = self.state();
        if state.closed {
            return FrameDemand::Idle;
        }
        if state.redraw || state.poll {
            return FrameDemand::At(now);
        }
        let scheduled =
            state.redraw_at.into_iter().chain(state.timers.iter().map(|t| t.next)).min();
        match (user, scheduled) {
            (FrameDemand::Continuous, _) => FrameDemand::Continuous,
            (FrameDemand::At(a), Some(b)) => FrameDemand::At(a.min(b)),
            (FrameDemand::Idle, Some(at)) => FrameDemand::At(at),
            _ => user,
        }
    }
    fn take_redraw(state: &mut WorkState, now: Instant) -> bool {
        let due = state.redraw_at.is_some_and(|at| at <= now);
        if due {
            state.redraw_at = None;
        }
        std::mem::take(&mut state.redraw) || due
    }
}

/// Revokes work before the host drops its native handle, including bounded X11 close.
/// Kept separate from Window's Drop so run_until_closed can move its native handle.
pub(crate) struct WindowWorkOwner(pub(crate) Arc<WindowWork>);
impl std::ops::Deref for WindowWorkOwner {
    type Target = Arc<WindowWork>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl Drop for WindowWorkOwner {
    fn drop(&mut self) {
        self.0.close();
    }
}

pub(crate) struct ScheduledHandler {
    inner: Box<dyn WindowHandler>,
    work: Arc<WindowWork>,
    user_demand: std::cell::Cell<Option<FrameDemand>>,
}
impl ScheduledHandler {
    pub(crate) fn new(inner: Box<dyn WindowHandler>, work: Arc<WindowWork>) -> Self {
        // Never invoke user callbacks inside the window builder/open budget.
        let user_demand = std::cell::Cell::new(None);
        Self { inner, work, user_demand }
    }
    fn frame(&self, now: Instant) -> Result<(), HandlerError> {
        let demand = self.user_demand.get().unwrap_or_else(|| self.inner.frame_demand());
        let (mut draw, poll, full_damage, timers) = {
            let mut state = self.work.state();
            if state.closed {
                return Ok(());
            }
            let draw = WindowWork::take_redraw(&mut state, now)
                || match demand {
                    FrameDemand::Continuous => true,
                    FrameDemand::At(at) => at <= now,
                    FrameDemand::Idle => false,
                };
            let poll = std::mem::take(&mut state.poll);
            let full_damage = std::mem::take(&mut state.full_damage);
            state.precise_damage = false;
            let timers = state
                .timers
                .iter_mut()
                .filter_map(|timer| {
                    if timer.next > now {
                        return None;
                    }
                    // Skip missed periods: never flood callbacks after a stall.
                    timer.next = now.checked_add(timer.interval).unwrap_or(now);
                    Some(TimerHandle { token: Arc::downgrade(&timer.token), _nonsend: PhantomData })
                })
                .collect::<Vec<_>>();
            (draw, poll, full_damage, timers)
        };
        if full_damage {
            self.inner.damage(DamageArea::FullWindow);
        }
        for timer in &timers {
            if self.work.is_closed() {
                return Ok(());
            }
            self.inner.on_timer(timer);
        }
        if self.work.is_closed() {
            return Ok(());
        }
        if draw || poll || !timers.is_empty() {
            self.inner.poll();
        }
        {
            let mut state = self.work.state();
            if state.closed {
                return Ok(());
            }
            // Updates found by poll/timers may request this draw. Draw's own requests
            // remain pending until a later native frame, preventing callback recursion.
            draw |= WindowWork::take_redraw(&mut state, now);
        }
        if draw {
            self.inner.draw()?;
        }
        Ok(())
    }
}
impl WindowHandler for ScheduledHandler {
    fn frame_demand(&self) -> FrameDemand {
        if self.work.is_closed() {
            return FrameDemand::Idle;
        }
        let demand = self.inner.frame_demand();
        self.user_demand.set(Some(demand));
        self.work.demand(demand, Instant::now())
    }
    fn on_frame(&self) -> Result<(), HandlerError> {
        let result = self.frame(Instant::now());
        if result.is_err() {
            self.work.close();
        }
        result
    }
    fn damage(&self, area: DamageArea<'_>) {
        {
            let mut state = self.work.state();
            if state.closed {
                return;
            }
            state.precise_damage = true;
            state.redraw = true;
        }
        self.inner.damage(area);
    }
    fn resized(&self, size: WindowSize) -> Result<(), HandlerError> {
        if self.work.is_closed() {
            return Ok(());
        }
        self.inner.resized(size)?;
        let mut state = self.work.state();
        if !state.closed {
            state.full_damage = true;
            state.redraw = true;
        }
        Ok(())
    }
    fn on_event(&self, event: Event) -> EventStatus {
        {
            let mut state = self.work.state();
            if matches!(event, Event::Window(WindowEvent::WillClose)) {
                state.closed = true;
                state.wake = None;
                state.timers.clear();
            } else {
                if state.closed {
                    return EventStatus::Ignored;
                }
                if matches!(event, Event::Window(WindowEvent::RedrawRequested))
                    && !state.precise_damage
                {
                    state.full_damage = true;
                }
                // Preserve the fork's input-barrier redraw semantics, including Idle handlers.
                state.redraw = true;
            }
        }
        self.inner.on_event(event)
    }
}
impl Drop for ScheduledHandler {
    fn drop(&mut self) {
        self.work.close();
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
        sync::atomic::{AtomicUsize, Ordering},
    };

    struct Probe {
        events: Rc<RefCell<Vec<&'static str>>>,
        work: Arc<WindowWork>,
        close_in_poll: bool,
        redraw_in_draw: Cell<bool>,
        demand: FrameDemand,
    }
    impl WindowHandler for Probe {
        fn frame_demand(&self) -> FrameDemand {
            self.demand
        }
        fn draw(&self) -> Result<(), HandlerError> {
            self.events.borrow_mut().push("draw");
            if self.redraw_in_draw.replace(false) {
                self.work.waker().request_redraw();
            }
            Ok(())
        }
        fn damage(&self, _: DamageArea<'_>) {
            self.events.borrow_mut().push("damage");
        }
        fn poll(&self) {
            self.events.borrow_mut().push("poll");
            if self.close_in_poll {
                self.work.close();
            }
        }
        fn on_timer(&self, _: &TimerHandle) {
            self.events.borrow_mut().push("timer");
        }
        fn resized(&self, _: WindowSize) -> Result<(), HandlerError> {
            Ok(())
        }
        fn on_event(&self, _: Event) -> EventStatus {
            EventStatus::Ignored
        }
    }
    fn fixture() -> (ScheduledHandler, Arc<WindowWork>, Rc<RefCell<Vec<&'static str>>>) {
        let work = Arc::new(WindowWork::default());
        let events = Rc::new(RefCell::new(Vec::new()));
        let handler = ScheduledHandler::new(
            Box::new(Probe {
                events: Rc::clone(&events),
                work: Arc::clone(&work),
                close_in_poll: false,
                redraw_in_draw: false.into(),
                demand: FrameDemand::Idle,
            }),
            Arc::clone(&work),
        );
        (handler, work, events)
    }
    #[test]
    fn poll_wake_does_not_draw_and_delayed_redraw_never_fires_early() {
        let (handler, work, events) = fixture();
        let now = Instant::now();
        handler.frame(now).unwrap();
        assert_eq!(*events.borrow(), ["damage", "poll", "draw"]);
        events.borrow_mut().clear();
        work.waker().request_poll();
        handler.frame(now).unwrap();
        assert_eq!(*events.borrow(), ["poll"]);
        events.borrow_mut().clear();
        let later = now + Duration::from_secs(2);
        work.request_redraw(Some(later));
        work.request_redraw(Some(later + Duration::from_secs(1)));
        assert_eq!(work.demand(FrameDemand::Idle, now), FrameDemand::At(later));
        handler.frame(later - Duration::from_millis(1)).unwrap();
        assert!(events.borrow().is_empty());
        handler.frame(later).unwrap();
        assert_eq!(*events.borrow(), ["poll", "draw"]);
    }
    #[test]
    fn redraw_requested_inside_draw_is_deferred_to_next_callback() {
        let work = Arc::new(WindowWork::default());
        let events = Rc::new(RefCell::new(Vec::new()));
        let handler = ScheduledHandler::new(
            Box::new(Probe {
                events: Rc::clone(&events),
                work: Arc::clone(&work),
                close_in_poll: false,
                redraw_in_draw: true.into(),
                demand: FrameDemand::Idle,
            }),
            Arc::clone(&work),
        );
        handler.frame(Instant::now()).unwrap();
        assert_eq!(*events.borrow(), ["damage", "poll", "draw"]);
        assert!(work.state().redraw);
        handler.frame(Instant::now()).unwrap();
        assert_eq!(*events.borrow(), ["damage", "poll", "draw", "poll", "draw"]);
        assert_eq!(handler.frame_demand(), FrameDemand::Idle);
    }
    #[test]
    fn close_during_poll_suppresses_draw_and_revokes_all_weak_work() {
        let work = Arc::new(WindowWork::default());
        let timer = work.create_timer(Duration::from_millis(1)).unwrap();
        let events = Rc::new(RefCell::new(Vec::new()));
        let wakes = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&wakes);
        work.bind_wake(FrameRequester::new(move || {
            count.fetch_add(1, Ordering::Relaxed);
        }));
        let waker = work.waker();
        let handler = ScheduledHandler::new(
            Box::new(Probe {
                events: Rc::clone(&events),
                work: Arc::clone(&work),
                close_in_poll: true,
                redraw_in_draw: false.into(),
                demand: FrameDemand::Continuous,
            }),
            Arc::clone(&work),
        );
        handler.frame(Instant::now()).unwrap();
        assert_eq!(*events.borrow(), ["damage", "poll"]);
        assert!(timer.token.upgrade().is_none());
        waker.request_redraw();
        waker.request_poll();
        assert_eq!(wakes.load(Ordering::Relaxed), 0);
        handler.frame(Instant::now()).unwrap();
        assert_eq!(*events.borrow(), ["damage", "poll"]);
        assert_eq!(handler.frame_demand(), FrameDemand::Idle);
    }
    #[test]
    fn dropped_timer_handles_do_not_stop_timer_and_stalls_do_not_flood() {
        let (handler, work, events) = fixture();
        handler.frame(Instant::now()).unwrap();
        events.borrow_mut().clear();
        let timer = work.create_timer(Duration::from_millis(10)).unwrap();
        assert!(timer == timer.clone());
        let other = work.create_timer(Duration::from_millis(100)).unwrap();
        assert!(timer != other);
        let next = work.state().timers.first().unwrap().next;
        drop(timer);
        drop(other);
        handler.frame(next + Duration::from_secs(5)).unwrap();
        assert_eq!(*events.borrow(), ["timer", "timer", "poll"]);
        events.borrow_mut().clear();
        handler.frame(next + Duration::from_secs(5)).unwrap();
        assert!(events.borrow().is_empty());
        assert!(work.create_timer(Duration::ZERO).is_err());
    }
    #[test]
    fn precise_damage_precedes_poll_draw_without_full_damage_fallback() {
        let (handler, _, events) = fixture();
        handler.frame(Instant::now()).unwrap();
        events.borrow_mut().clear();
        handler.damage(DamageArea::Rect(crate::DamageRect::new(
            crate::dpi::PhysicalPosition::new(2, 3),
            crate::dpi::PhysicalSize::new(4, 5),
        )));
        handler.on_event(Event::Window(WindowEvent::RedrawRequested));
        handler.frame(Instant::now()).unwrap();
        assert_eq!(*events.borrow(), ["damage", "poll", "draw"]);
    }
    #[test]
    fn legacy_continuous_demand_keeps_polling_and_drawing() {
        struct Legacy(Rc<Cell<usize>>);
        impl WindowHandler for Legacy {
            fn on_frame(&self) -> Result<(), HandlerError> {
                self.0.set(self.0.get() + 1);
                Ok(())
            }
            fn resized(&self, _: WindowSize) -> Result<(), HandlerError> {
                Ok(())
            }
            fn on_event(&self, _: Event) -> EventStatus {
                EventStatus::Ignored
            }
        }
        let count = Rc::new(Cell::new(0));
        let handler =
            ScheduledHandler::new(Box::new(Legacy(Rc::clone(&count))), Default::default());
        for _ in 0..3 {
            handler.frame(Instant::now()).unwrap();
        }
        assert_eq!(count.get(), 3);
        assert_eq!(handler.frame_demand(), FrameDemand::Continuous);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "test assertions")]
mod deadline_tests {
    use super::*;
    use std::{cell::Cell, rc::Rc};
    #[test]
    fn relative_legacy_poll_deadline_is_consumed_before_asking_for_the_next_one() {
        struct Heartbeat(Rc<Cell<usize>>);
        impl WindowHandler for Heartbeat {
            fn frame_demand(&self) -> FrameDemand {
                FrameDemand::At(Instant::now() + Duration::from_millis(250))
            }
            fn on_frame(&self) -> Result<(), HandlerError> {
                self.0.set(self.0.get() + 1);
                Ok(())
            }
            fn resized(&self, _: WindowSize) -> Result<(), HandlerError> {
                Ok(())
            }
            fn on_event(&self, _: Event) -> EventStatus {
                EventStatus::Ignored
            }
        }
        let count = Rc::new(Cell::new(0));
        let handler =
            ScheduledHandler::new(Box::new(Heartbeat(Rc::clone(&count))), Default::default());
        handler.frame(Instant::now()).unwrap();
        assert_eq!(count.get(), 1);
        let FrameDemand::At(deadline) = handler.frame_demand() else {
            panic!("missing heartbeat");
        };
        handler.frame(deadline).unwrap();
        assert_eq!(count.get(), 2);
    }
}
