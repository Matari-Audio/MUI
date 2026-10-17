//! Returning native keyboard focus to the host does not cancel a pointer gesture.
use std::cell::Cell;

pub(crate) fn report_focus_loss(keyboard_capture_handoff: bool) -> bool {
    !keyboard_capture_handoff
}

/// Native focus setters synchronously reenter focus callbacks. Scope the flag
/// to that call, restoring even a nested handoff's previous value on unwind.
pub(crate) struct KeyboardCaptureHandoff<'a> {
    flag: &'a Cell<bool>,
    previous: bool,
}

impl<'a> KeyboardCaptureHandoff<'a> {
    pub(crate) fn new(flag: &'a Cell<bool>) -> Self {
        Self { flag, previous: flag.replace(true) }
    }
}

impl Drop for KeyboardCaptureHandoff<'_> {
    fn drop(&mut self) {
        self.flag.set(self.previous);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kill_focus_reports_genuine_loss_but_not_keyboard_capture_handoff() {
        assert!(!report_focus_loss(true));
        assert!(report_focus_loss(false));
    }

    #[test]
    fn nested_handoffs_restore_reporting_for_later_genuine_loss() {
        let flag = Cell::new(false);
        {
            let _outer = KeyboardCaptureHandoff::new(&flag);
            assert!(!report_focus_loss(flag.get()));
            {
                let _inner = KeyboardCaptureHandoff::new(&flag);
                assert!(!report_focus_loss(flag.get()));
            }
            assert!(!report_focus_loss(flag.get()));
        }
        assert!(report_focus_loss(flag.get()));
    }

    #[test]
    fn an_unwind_cannot_suppress_later_genuine_focus_loss() {
        let flag = Cell::new(false);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _handoff = KeyboardCaptureHandoff::new(&flag);
            panic!("native focus callback unwound");
        }));
        assert!(result.is_err());
        assert!(report_focus_loss(flag.get()));
    }
}
