//! AppKit-independent lifecycle/pacing policy; also runnable with a tiny rustc test harness.
use crate::FrameDemand;
use std::cell::{Cell, RefCell};
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Pace {
    Stopped,
    Refresh,
    Deadline(Instant),
}

pub(super) fn pacing(demand: FrameDemand, requested: bool, drawable: bool, now: Instant) -> Pace {
    if !drawable {
        return Pace::Stopped;
    }
    if requested {
        return Pace::Refresh;
    }
    match demand {
        FrameDemand::Continuous => Pace::Refresh,
        FrameDemand::Idle => Pace::Stopped,
        FrameDemand::At(deadline) if deadline <= now => Pace::Refresh,
        FrameDemand::At(deadline) => Pace::Deadline(deadline),
    }
}

pub(super) fn top_origin(
    parent_flipped: bool, parent_origin: f64, parent_height: f64, child_height: f64,
) -> f64 {
    if parent_flipped {
        parent_origin
    } else {
        parent_origin + parent_height - child_height
    }
}

/// Closing revokes callbacks immediately, but drops a renderer only after the
/// active callback's borrow ends. Notify/drop never hold a RefCell borrow.
pub(super) struct HandlerSlot<T> {
    inner: RefCell<Option<T>>,
    closed: Cell<bool>,
    notify_close: fn(&T),
    dispose: fn(T),
}

impl<T> HandlerSlot<T> {
    pub fn new(notify_close: fn(&T), dispose: fn(T)) -> Self {
        Self { inner: RefCell::new(None), closed: Cell::new(false), notify_close, dispose }
    }

    pub fn set(&self, handler: T) {
        if !self.closed.get() {
            self.inner.replace(Some(handler));
        }
    }

    pub fn with<R>(&self, user: impl FnOnce(&T) -> R) -> Option<R> {
        if self.closed.get() {
            return None;
        }
        // Declared before the borrow so it also runs after borrow release on panic.
        struct Finish<'a, T>(&'a HandlerSlot<T>);
        impl<T> Drop for Finish<'_, T> {
            fn drop(&mut self) {
                self.0.finish_close();
            }
        }
        let _finish = Finish(self);
        let inner = self.inner.try_borrow().ok()?;
        Some(user(inner.as_ref()?))
    }

    pub fn close(&self) {
        self.closed.set(true);
        self.finish_close();
    }

    fn finish_close(&self) {
        if !self.closed.get() {
            return;
        }
        let Ok(mut inner) = self.inner.try_borrow_mut() else { return };
        let handler = inner.take();
        drop(inner);
        if let Some(handler) = handler {
            (self.notify_close)(&handler);
            (self.dispose)(handler);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{rc::Rc, time::Duration};

    #[test]
    fn idle_has_no_pacer_but_explicit_wake_draws_once() {
        let now = Instant::now();
        assert_eq!(pacing(FrameDemand::Idle, false, true, now), Pace::Stopped);
        assert_eq!(pacing(FrameDemand::Idle, true, true, now), Pace::Refresh);
        assert_eq!(pacing(FrameDemand::Idle, false, true, now), Pace::Stopped);
    }

    #[test]
    fn deadline_waits_without_refresh_callbacks() {
        let now = Instant::now();
        let deadline = now + Duration::from_secs(1);
        assert_eq!(pacing(FrameDemand::At(deadline), false, true, now), Pace::Deadline(deadline));
        assert_eq!(pacing(FrameDemand::At(deadline), false, true, deadline), Pace::Refresh);
        assert_eq!(pacing(FrameDemand::Continuous, false, true, now), Pace::Refresh);
    }

    #[test]
    fn hidden_detached_zero_size_or_closed_always_stop_even_when_requested() {
        for demand in [FrameDemand::Continuous, FrameDemand::Idle, FrameDemand::At(Instant::now())]
        {
            assert_eq!(pacing(demand, true, false, Instant::now()), Pace::Stopped);
        }
    }

    #[test]
    fn top_anchor_uses_superview_bounds_and_coordinate_direction() {
        assert_eq!(top_origin(false, 0.0, 600.0, 400.0), 200.0);
        assert_eq!(top_origin(true, 0.0, 600.0, 400.0), 0.0);
        assert_eq!(top_origin(false, 12.0, 600.0, 400.0), 212.0);
        assert_eq!(top_origin(true, 12.0, 600.0, 400.0), 12.0);
    }

    #[test]
    fn reentrant_close_revokes_then_notifies_then_drops_after_callback() {
        struct Handler(Rc<RefCell<Vec<&'static str>>>);
        impl Drop for Handler {
            fn drop(&mut self) {
                self.0.borrow_mut().push("drop surface");
            }
        }
        let events = Rc::new(RefCell::new(Vec::new()));
        let slot = HandlerSlot::new(|h: &Handler| h.0.borrow_mut().push("WillClose"), drop);
        slot.set(Handler(Rc::clone(&events)));
        slot.with(|_| {
            events.borrow_mut().push("callback");
            slot.close();
            assert!(slot.with(|_| panic!("late callback")).is_none());
            assert_eq!(*events.borrow(), ["callback"]);
            events.borrow_mut().push("return");
        });
        slot.close();
        assert_eq!(*events.borrow(), ["callback", "return", "WillClose", "drop surface"]);
    }

    #[test]
    fn callback_panic_still_finishes_deferred_close() {
        let drops = Rc::new(Cell::new(0));
        struct Handler(Rc<Cell<u32>>);
        impl Drop for Handler {
            fn drop(&mut self) {
                self.0.set(self.0.get() + 1);
            }
        }
        let slot = HandlerSlot::new(|_| {}, drop);
        slot.set(Handler(Rc::clone(&drops)));
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            slot.with(|_| {
                slot.close();
                panic!("injected panic after close");
            })
        }));
        assert_eq!(drops.get(), 1);
    }

    #[test]
    fn closing_one_editor_does_not_revoke_another() {
        let a = HandlerSlot::new(|_: &u8| {}, drop);
        let b = HandlerSlot::new(|_: &u8| {}, drop);
        a.set(1);
        b.set(2);
        a.close();
        assert!(a.with(|v| *v).is_none());
        assert_eq!(b.with(|v| *v), Some(2));
    }
}
