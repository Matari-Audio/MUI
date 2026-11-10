//! Deadline requests are separate from spring animation. The host does not need
//! to redraw 60/160 times per second just to blink a caret or show a tooltip.
use crate::Ui;
use mui_scene::A11y;
use std::time::Duration;
const HALF_CARET_PERIOD: f64 = 0.625;
#[derive(Clone, Copy)]
pub(super) enum CallerWake {
    Waiting(f64),
    BuildDue,
}
impl Ui {
    pub(super) fn advance_caller_wake(&mut self) {
        self.wake = match self.wake {
            Some(CallerWake::Waiting(at)) if at <= self.time => Some(CallerWake::BuildDue),
            Some(CallerWake::BuildDue) => None,
            waiting => waiting,
        };
    }

    /// Elapsed frame time in seconds.
    pub fn time(&self) -> f64 {
        self.time
    }

    /// Request a frame after this delay, keeping the earliest outstanding request.
    pub fn repaint_in(&mut self, secs: f64) {
        if !secs.is_finite() {
            return;
        }
        let at = self.time + secs.max(0.0);
        if at.is_finite() {
            let at = match self.wake {
                Some(CallerWake::Waiting(previous)) if previous > self.time => previous.min(at),
                _ => at,
            };
            self.wake = Some(CallerWake::Waiting(at));
        }
    }

    /// Delay until the next timer-dependent visual change. Input/model changes
    /// still invalidate immediately, and `Frame::animating` requests an animation
    /// frame. Hosts should use the earlier of their own deadline and this one.
    pub fn repaint_after(&self) -> Option<Duration> {
        if !self.time.is_finite() || self.time < 0. {
            return None;
        }
        let tip = self.hover.as_ref().and_then(|(key, elapsed)| {
            if !elapsed.is_finite() || *elapsed >= crate::ui::TIP_DELAY {
                return None;
            }
            self.scene.as_ref()?.surface(key)?.tip.as_ref()?;
            Some((crate::ui::TIP_DELAY - elapsed).max(0.001))
        });
        let caret = self
            .focus
            .as_deref()
            .and_then(|key| self.scene.as_ref()?.surface(key))
            .filter(|s| {
                !s.disabled
                    && matches!(
                        s.semantics.as_ref().map(|s| &s.role),
                        Some(A11y::TextInput { .. })
                    )
            })
            .map(|_| (HALF_CARET_PERIOD - self.time.rem_euclid(HALF_CARET_PERIOD)).max(0.001));
        let roll = self
            .wheel_turned
            .map(|at| crate::host::WHEEL_ROLL - (self.time - at))
            .filter(|left| *left > 0.0);
        let wake = self.wake.map(|wake| match wake {
            CallerWake::Waiting(at) => (at - self.time).max(0.0),
            CallerWake::BuildDue => 0.0,
        });
        [tip, caret, roll, wake]
            .into_iter()
            .flatten()
            .reduce(f64::min)
            .and_then(|seconds| Duration::try_from_secs_f64(seconds).ok())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use mui_input::Input;
    use mui_scene::prelude::*;
    #[test]
    fn requested_deadline_uses_earliest_and_expires_on_frame() {
        let mut ui = Ui::default();
        ui.repaint_in(0.4);
        ui.repaint_in(0.2);
        ui.repaint_in(0.3);
        assert_eq!(ui.repaint_after(), Some(Duration::from_millis(200)));
        ui.frame(block(20., 20.), None, Input::default(), 0.1)
            .unwrap();
        assert!((ui.time() - 0.1).abs() < 1e-9);
        assert_eq!(ui.repaint_after(), Some(Duration::from_millis(100)));
        ui.frame(block(20., 20.), None, Input::default(), 0.1)
            .unwrap();
        assert_eq!(ui.repaint_after(), Some(Duration::ZERO));
        ui.frame(block(20., 20.), None, Input::default(), 0.0)
            .unwrap();
        assert_eq!(ui.repaint_after(), None);
        ui.repaint_in(f64::NAN);
        assert_eq!(ui.repaint_after(), None);
        ui.repaint_in(0.0);
        assert_eq!(ui.repaint_after(), Some(Duration::ZERO));
    }
    #[test]
    fn idle_has_no_deadline() {
        let mut u = Ui::default();
        u.frame(block(20., 20.), None, Input::default(), 0.)
            .unwrap();
        assert!(u.repaint_after().is_none());
    }
    #[test]
    fn focused_input_gets_a_deadline_not_continuous_animation() {
        let mut u = Ui::default();
        let tree = || {
            block(100., 30.)
                .id("edit")
                .focusable()
                .a11y(A11y::TextInput {
                    value: "".into(),
                    selection: (0, 0),
                    carets: Vec::new(),
                })
        };
        u.frame(tree(), None, Input::default(), 0.).unwrap();
        u.focus("edit");
        let f = u.frame(tree(), None, Input::default(), 0.);
        let f = f.unwrap();
        assert!(!f.animating);
        assert_eq!(f.repaint_after, Some(Duration::from_millis(625)));
    }
    #[test]
    fn real_elapsed_time_crosses_blink_boundary() {
        let mut u = Ui::default();
        let tree = || {
            block(100., 30.)
                .id("edit")
                .focusable()
                .a11y(A11y::TextInput {
                    value: "".into(),
                    selection: (0, 0),
                    carets: Vec::new(),
                })
        };
        u.frame(tree(), None, Input::default(), 0.).unwrap();
        u.focus("edit");
        u.frame(tree(), None, Input::default(), 0.7).unwrap();
        let d = u.repaint_after().unwrap().as_secs_f64();
        assert!((d - 0.55).abs() < 1e-9);
    }
}
