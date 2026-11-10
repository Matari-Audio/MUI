use super::*;

/// Gain for a run of wheel notches. Pixel scrolling is always passed through.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WheelAcceleration {
    #[default]
    Disabled,
    /// Notches less than 90 ms apart in the same direction add 0.25 gain,
    /// up to three times the initial gain. A pause or reversal resets it.
    Fling,
}

/// Per-editor conversion from native wheel notches to MUI text lines.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WheelPolicy {
    lines_per_notch: f32,
    acceleration: WheelAcceleration,
}
impl Default for WheelPolicy {
    fn default() -> Self {
        Self {
            lines_per_notch: 1.0,
            acceleration: WheelAcceleration::Disabled,
        }
    }
}
impl WheelPolicy {
    /// Returns `None` unless `lines_per_notch` is finite and positive.
    pub fn new(lines_per_notch: f32, acceleration: WheelAcceleration) -> Option<Self> {
        (lines_per_notch.is_finite() && lines_per_notch > 0.0).then_some(Self {
            lines_per_notch,
            acceleration,
        })
    }
}

#[derive(Default)]
pub(super) struct WheelState {
    axes: [Option<(Instant, f32, u8)>; 2],
}
impl WheelState {
    pub(super) fn apply(
        &mut self,
        delta: ScrollDelta,
        policy: WheelPolicy,
        now: Instant,
    ) -> ScrollDelta {
        let ScrollDelta::Lines { x, y } = delta else {
            return delta;
        };
        let mut line = |axis: usize, value: f32| {
            if value == 0.0 || !value.is_finite() {
                return value;
            }
            let run = self.axes[axis]
                .filter(|(previous, direction, _)| {
                    now.saturating_duration_since(*previous) < Duration::from_millis(90)
                        && *direction == value.signum()
                })
                .map_or(1, |(_, _, run)| run.saturating_add(1).min(9));
            self.axes[axis] = Some((now, value.signum(), run));
            let gain = match policy.acceleration {
                WheelAcceleration::Disabled => 1.0,
                WheelAcceleration::Fling => 1.0 + 0.25 * f32::from(run - 1),
            };
            value * policy.lines_per_notch * gain
        };
        ScrollDelta::Lines {
            x: line(0, x),
            y: line(1, y),
        }
    }
}

/// Native positions never cross to the model worker. The gesture revision rejects
/// stale button/leave/focus decisions without starving hiding during continuous motion.
#[derive(Default)]
pub(super) struct PointerState {
    pub(super) revision: u64,
    at: Option<PhysicalPosition<f64>>,
    hidden_at: Option<PhysicalPosition<f64>>,
}
impl PointerState {
    pub(super) fn observe(&mut self, event: &Event) {
        match event {
            Event::Mouse(MouseEvent::CursorMoved { position, .. }) => self.at = Some(*position),
            Event::Mouse(MouseEvent::ButtonPressed { .. } | MouseEvent::ButtonReleased { .. }) => {
                self.revision = self.revision.wrapping_add(1);
            }
            Event::Mouse(MouseEvent::CursorLeft | MouseEvent::DragLeft)
            | Event::Window(WindowEvent::Unfocused | WindowEvent::WillClose) => {
                self.revision = self.revision.wrapping_add(1);
                self.at = None;
                self.hidden_at = None;
            }
            _ => {}
        }
    }
    pub(super) fn apply(&mut self, revision: u64, hide: bool) -> Option<PhysicalPosition<f64>> {
        if revision != self.revision {
            return None;
        }
        if hide {
            if self.hidden_at.is_none() {
                self.hidden_at = self.at;
            }
            None
        } else {
            self.hidden_at.take()
        }
    }
    pub(super) fn hidden(&self) -> bool {
        self.hidden_at.is_some()
    }
}

#[cfg(target_os = "linux")]
pub(super) struct X11Lifetime {
    requests: Arc<Requests>,
    hook: Option<X11WindowHook>,
    id: u32,
    cancelled: Cell<bool>,
}
#[cfg(target_os = "linux")]
impl X11Lifetime {
    pub(super) fn new(requests: Arc<Requests>, id: u32) -> Self {
        let hook = requests
            .x11_hook
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        requests.x11_window.store(id, Ordering::Release);
        let lifetime = Self {
            requests,
            hook,
            id,
            cancelled: Cell::new(false),
        };
        if id != 0 {
            lifetime.notify(Some(id));
        }
        lifetime
    }
    pub(super) fn cancel(&self) {
        if self.cancelled.replace(true) || self.id == 0 {
            return;
        }
        if self
            .requests
            .x11_window
            .compare_exchange(self.id, 0, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            self.notify(None);
        }
    }
    fn notify(&self, id: Option<u32>) {
        if let Some(hook) = &self.hook
            && let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                hook.lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)(id);
            }))
        {
            mui::diagnostics::error(
                "mui-baseview",
                "x11_window_hook_panic",
                mui::diagnostics::panic_message(&*payload),
            );
        }
    }
}
#[cfg(target_os = "linux")]
impl Drop for X11Lifetime {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn lines(delta: ScrollDelta) -> (f32, f32) {
        match delta {
            ScrollDelta::Lines { x, y } => (x, y),
            _ => panic!("expected lines"),
        }
    }
    #[test]
    fn wheel_policies_are_independent_and_pixels_are_exact() {
        let now = Instant::now();
        let mut linear = WheelState::default();
        let mut fling = WheelState::default();
        let linear_policy = WheelPolicy::new(10.0, WheelAcceleration::Disabled).unwrap();
        let fling_policy = WheelPolicy::new(10.0, WheelAcceleration::Fling).unwrap();
        for i in 0..20 {
            let at = now + Duration::from_millis(i * 10);
            assert_eq!(
                lines(linear.apply(ScrollDelta::Lines { x: 0., y: 1. }, linear_policy, at)),
                (0., 10.)
            );
            assert_eq!(
                lines(fling.apply(ScrollDelta::Lines { x: 0., y: 1. }, fling_policy, at)),
                (0., 10. * (1. + 0.25 * (i as f32).min(8.)))
            );
        }
        assert_eq!(
            lines(fling.apply(
                ScrollDelta::Lines { x: 1., y: -1. },
                fling_policy,
                now + Duration::from_millis(200)
            )),
            (10., -10.)
        );
        assert_eq!(
            lines(fling.apply(
                ScrollDelta::Lines { x: 1., y: -1. },
                fling_policy,
                now + Duration::from_millis(300)
            )),
            (10., -10.)
        );
        let pixels = fling.apply(ScrollDelta::Pixels { x: 0.25, y: -1.5 }, fling_policy, now);
        assert!(matches!(pixels, ScrollDelta::Pixels { x: 0.25, y: -1.5 }));
        for invalid in [0., -1., f32::NAN, f32::INFINITY] {
            assert!(WheelPolicy::new(invalid, WheelAcceleration::Disabled).is_none());
        }
    }
    #[test]
    fn pointer_restores_origin_but_discards_stale_decisions_after_leave() {
        let mut pointer = PointerState::default();
        let origin = PhysicalPosition::new(20., 30.);
        pointer.observe(&Event::Mouse(MouseEvent::CursorMoved {
            position: origin,
            modifiers: Modifiers::empty(),
        }));
        assert!(pointer.apply(pointer.revision, true).is_none());
        let revision = pointer.revision;
        pointer.observe(&Event::Mouse(MouseEvent::CursorMoved {
            position: PhysicalPosition::new(70., 80.),
            modifiers: Modifiers::empty(),
        }));
        assert_eq!(
            pointer.revision, revision,
            "motion must not starve worker decisions"
        );
        assert_eq!(pointer.apply(revision, false), Some(origin));
        pointer.apply(pointer.revision, true);
        let revision = pointer.revision;
        pointer.observe(&Event::Mouse(MouseEvent::CursorLeft));
        assert!(pointer.apply(revision, false).is_none());
        assert!(pointer.apply(revision, true).is_none());
        assert!(!pointer.hidden());
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn stale_x11_lifetime_does_not_cancel_new_window_hook() {
        let requests = Arc::new(Requests::default());
        let current = Arc::new(Mutex::new(None));
        let observed = Arc::clone(&current);
        requests.on_x11_window(Arc::new(Mutex::new(move |id| {
            *observed.lock().unwrap() = id;
        })));
        let old = X11Lifetime::new(Arc::clone(&requests), 10);
        let new = X11Lifetime::new(requests, 11);
        drop(old);
        assert_eq!(*current.lock().unwrap(), Some(11));
        drop(new);
        assert_eq!(*current.lock().unwrap(), None);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn x11_creation_cancellation_is_once_and_old_drop_preserves_new_identifier() {
        let requests = Arc::new(Requests::default());
        let events = Arc::new(Mutex::new(Vec::new()));
        let observed = events.clone();
        requests.on_x11_window(Arc::new(Mutex::new(move |id| {
            observed.lock().unwrap().push(id);
        })));
        let old = X11Lifetime::new(requests.clone(), 10);
        old.cancel();
        let new = X11Lifetime::new(requests.clone(), 11);
        drop(old);
        assert_eq!(requests.x11_window(), Some(11));
        drop(new);
        assert_eq!(requests.x11_window(), None);
        assert_eq!(
            *events.lock().unwrap(),
            vec![Some(10), None, Some(11), None]
        );
    }
}
