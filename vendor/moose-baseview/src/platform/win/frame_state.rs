//! Pure pacing policy, also executable without a Windows runtime.
use crate::FrameDemand;
use std::time::Instant;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Wake {
    Stop,
    Wait,
    At(Instant),
    Frame { compositor: bool },
}

pub(super) struct FrameState {
    pub hwnd: Option<usize>,
    pub revoked: bool,
    pub visible: bool,
    pub pending: bool,
    requested: bool,
    demand: FrameDemand,
    consumed_deadline: Option<Instant>,
}

impl FrameState {
    pub fn new() -> Self {
        Self {
            hwnd: None,
            revoked: false,
            visible: false,
            pending: false,
            requested: true,
            demand: FrameDemand::Continuous,
            consumed_deadline: None,
        }
    }
    pub fn request(&mut self) {
        if !self.revoked {
            self.requested = true;
        }
    }
    pub fn set_demand(&mut self, demand: FrameDemand) {
        self.demand = match demand {
            FrameDemand::At(at) if self.consumed_deadline == Some(at) => FrameDemand::Idle,
            demand => demand,
        };
    }
    pub fn next(&self, now: Instant) -> Wake {
        if self.revoked {
            return Wake::Stop;
        }
        if self.hwnd.is_none() || !self.visible || self.pending {
            return Wake::Wait;
        }
        if self.requested {
            // Explicit work must not bypass refresh pacing under Continuous.
            return Wake::Frame { compositor: self.demand == FrameDemand::Continuous };
        }
        match self.demand {
            FrameDemand::Idle => Wake::Wait,
            FrameDemand::Continuous => Wake::Frame { compositor: true },
            FrameDemand::At(at) if at > now => Wake::At(at),
            FrameDemand::At(_) => Wake::Frame { compositor: false },
        }
    }
    pub fn posted(&mut self) {
        self.requested = false;
        self.pending = true;
        if let FrameDemand::At(at) = self.demand {
            // An explicit request BEFORE the deadline must not consume it.
            if at <= Instant::now() {
                self.consumed_deadline = Some(at);
                self.demand = FrameDemand::Idle;
            }
        }
    }
    pub fn revoke(&mut self) {
        self.revoked = true;
        self.hwnd = None;
        self.requested = false;
    }
}

pub(super) fn scale_to_dpi(scale: f64) -> Option<u32> {
    // Reject invalid magnitudes before multiplication, rather than overflowing
    // a host-provided value and inspecting infinity afterwards.
    if !scale.is_finite() || scale < 1.0 / 96.0 || scale > f64::from(u32::MAX) / 96.0 {
        return None;
    }
    Some((scale * 96.0) as u32)
}

pub(super) fn class_name(image: usize, serial: u64) -> String {
    format!("MUI-Baseview-{image:x}-{serial:x}")
}

/// The returned value is the sole condition for uninstalling a thread's hook.
pub(super) fn remove_window<T>(
    windows: &mut std::collections::HashMap<usize, T>, hwnd: usize,
) -> bool {
    windows.remove(&hwnd);
    windows.is_empty()
}

/// VST3 resize negotiation is legal only once the editor has been shown.
/// Hiding a not-yet-shown editor must not consume its initial resize request.
pub(super) struct DeferredResize<T: Copy> {
    shown: std::cell::Cell<bool>,
    previous: std::cell::Cell<Option<T>>,
}
impl<T: Copy> DeferredResize<T> {
    pub fn new() -> Self {
        Self { shown: false.into(), previous: None.into() }
    }
    pub fn defer(&self, previous: T) -> bool {
        if self.shown.get() {
            return false;
        }
        if self.previous.get().is_none() {
            self.previous.set(Some(previous));
        }
        true
    }
    pub fn on_show(&self, visible: bool) -> Option<T> {
        if !visible {
            return None;
        }
        self.shown.set(true);
        self.previous.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn initial_resize_waits_for_visible_show_and_keeps_original_extent() {
        let resize = DeferredResize::new();
        assert!(resize.defer((400, 300)));
        assert!(resize.defer((600, 450)));
        assert_eq!(resize.on_show(false), None);
        assert_eq!(resize.on_show(true), Some((400, 300)));
        assert!(!resize.defer((800, 600)));
        assert_eq!(resize.on_show(true), None);
    }
    #[test]
    fn continuous_demand_uses_compositor_after_initial_frame() {
        assert_eq!(visible().next(Instant::now()), Wake::Frame { compositor: true });
    }
    fn visible() -> FrameState {
        let mut state = FrameState::new();
        state.hwnd = Some(10);
        state.visible = true;
        state.posted();
        state.pending = false;
        state
    }
    #[test]
    fn explicit_requests_do_not_bypass_continuous_pacing() {
        let mut state = visible();
        state.request();
        assert_eq!(state.next(Instant::now()), Wake::Frame { compositor: true });
    }
    #[test]
    fn idle_has_no_timer_deadline() {
        let mut state = visible();
        state.set_demand(FrameDemand::Idle);
        assert_eq!(state.next(Instant::now()), Wake::Wait);
    }
    #[test]
    fn explicit_request_wakes_idle_exactly_once() {
        let mut state = visible();
        state.set_demand(FrameDemand::Idle);
        state.request();
        assert_eq!(state.next(Instant::now()), Wake::Frame { compositor: false });
        state.posted();
        state.pending = false;
        assert_eq!(state.next(Instant::now()), Wake::Wait);
    }
    #[test]
    fn repeated_requests_coalesce_while_callback_is_pending() {
        let mut state = visible();
        state.set_demand(FrameDemand::Idle);
        state.request();
        state.posted();
        state.request();
        state.request();
        assert_eq!(state.next(Instant::now()), Wake::Wait);
        state.pending = false;
        assert_eq!(state.next(Instant::now()), Wake::Frame { compositor: false });
    }
    #[test]
    fn deadline_schedules_one_frame_not_a_past_deadline_spin() {
        let mut state = visible();
        let at = Instant::now();
        state.set_demand(FrameDemand::At(at));
        assert_eq!(state.next(at), Wake::Frame { compositor: false });
        state.posted();
        state.pending = false;
        state.set_demand(FrameDemand::At(at));
        assert_eq!(state.next(at), Wake::Wait);
    }
    #[test]
    fn future_deadline_survives_an_earlier_explicit_wake() {
        let mut state = visible();
        let at = Instant::now()
            .checked_add(std::time::Duration::from_secs(10))
            .unwrap_or_else(Instant::now);
        state.set_demand(FrameDemand::At(at));
        assert_eq!(state.next(Instant::now()), Wake::At(at));
        state.request();
        state.posted();
        state.pending = false;
        assert_eq!(state.next(Instant::now()), Wake::At(at));
    }
    #[test]
    fn revoked_requester_cannot_target_a_reused_hwnd() {
        let mut state = visible();
        state.revoke();
        state.request();
        assert_eq!(state.hwnd, None);
        assert_eq!(state.next(Instant::now()), Wake::Stop);
    }
    #[test]
    fn hidden_window_keeps_request_until_show() {
        let mut state = FrameState::new();
        state.hwnd = Some(10);
        assert_eq!(state.next(Instant::now()), Wake::Wait);
        state.visible = true;
        assert_eq!(state.next(Instant::now()), Wake::Frame { compositor: true });
    }
    #[test]
    fn dpi_hints_are_finite_positive_and_representable() {
        for scale in [f64::NAN, f64::INFINITY, -1.0, 0.0, 0.0001, f64::MAX] {
            assert_eq!(scale_to_dpi(scale), None);
        }
        assert_eq!(scale_to_dpi(1.25), Some(120));
        assert_eq!(scale_to_dpi(1.5), Some(144));
        assert_eq!(scale_to_dpi(2.0), Some(192));
    }
    #[test]
    fn class_names_separate_images_and_windows() {
        assert_ne!(class_name(0x1000, 0), class_name(0x2000, 0));
        assert_ne!(class_name(0x1000, 0), class_name(0x1000, 1));
    }
    #[test]
    fn uninstall_only_when_last_window_on_that_thread_closes() {
        let mut thread1 = std::collections::HashMap::from([(10, ()), (11, ())]);
        let mut thread2 = std::collections::HashMap::from([(20, ())]);
        assert!(!remove_window(&mut thread1, 11));
        assert!(remove_window(&mut thread1, 10));
        assert_eq!(thread2.len(), 1);
        assert!(remove_window(&mut thread2, 20));
    }
}
