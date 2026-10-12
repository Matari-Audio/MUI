//! Native-thread gesture authority and a non-overwriting start-ticket queue.
use matari_audio_drag_and_drop::{RejectedStart, StartTicket};
use std::cell::{Cell, RefCell};

#[derive(Default)]
pub(crate) struct NativeDrag {
    primary: Cell<bool>,
    generation: Cell<u64>,
    pending: RefCell<Option<(u64, StartTicket)>>,
    pub(crate) active: Cell<bool>,
}
impl NativeDrag {
    pub(crate) fn press(&self) {
        self.cancel_pending();
        self.generation.set(self.generation.get().wrapping_add(1));
        self.primary.set(true);
    }
    pub(crate) fn release(&self) {
        self.primary.set(false);
        self.cancel_pending();
    }
    pub(crate) fn cancel_pending(&self) {
        let pending = self.pending.borrow_mut().take();
        // A ticket's wake callback may reenter queue inspection.
        drop(pending);
    }
    pub(crate) fn available(&self) -> bool {
        self.primary.get() && !self.active.get() && self.pending.borrow().is_none()
    }
    pub(crate) fn queue(
        &self, ticket: StartTicket,
    ) -> Result<(), RejectedStart<crate::MatariScheduleError>> {
        if self.active.get() || self.pending.borrow().is_some() {
            return Err(ticket.reject(crate::MatariScheduleError::Busy));
        }
        if !self.primary.get() {
            return Err(ticket.reject(crate::MatariScheduleError::AuthorityUnavailable));
        }
        *self.pending.borrow_mut() = Some((self.generation.get(), ticket));
        Ok(())
    }
    pub(crate) fn take(&self) -> Option<StartTicket> {
        let (generation, ticket) = self.pending.borrow_mut().take()?;
        if !self.primary.get() || self.active.get() || generation != self.generation.get() {
            drop(ticket);
            return None;
        }
        Some(ticket)
    }
    pub(crate) fn begin(&self) -> Active<'_> {
        self.active.set(true);
        Active(self)
    }
}
pub(crate) struct Active<'a>(&'a NativeDrag);
impl Drop for Active<'_> {
    fn drop(&mut self) {
        self.0.active.set(false);
        self.0.release();
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "regression setup must fail the test on errors")]
mod tests {
    use super::*;
    use matari_audio_drag_and_drop as dnd;
    struct Queue<'a>(&'a NativeDrag);
    impl dnd::ToolkitAdapter for Queue<'_> {
        type Error = crate::MatariScheduleError;
        fn outbound_route(&self) -> Option<dnd::SessionRoute> {
            Some(crate::MatariDragAdapter::route(true))
        }
        fn schedule_outbound(
            &mut self, ticket: StartTicket,
        ) -> Result<(), RejectedStart<Self::Error>> {
            self.0.queue(ticket)
        }
        fn drive_inbound(
            &mut self, _: &mut dyn dnd::InboundHandler,
        ) -> Result<dnd::InboundDisposition, Self::Error> {
            Ok(dnd::InboundDisposition::Unhandled)
        }
    }
    fn files() -> dnd::FileSet {
        // The test executable is an existing readable absolute file on every target.
        dnd::FileSet::try_from_paths([std::env::current_exe().unwrap()]).unwrap()
    }
    #[test]
    fn native_slot_is_non_overwriting_and_release_cancels_once() {
        let native = NativeDrag::default();
        native.press();
        let mut first = dnd::Controller::new();
        first.start_outbound(&mut Queue(&native), files()).unwrap();
        assert!(!native.available());
        let mut second = dnd::Controller::new();
        let error = second.start_outbound(&mut Queue(&native), files()).unwrap_err();
        assert!(matches!(error, dnd::StartError::Adapter(crate::MatariScheduleError::Busy)));
        native.release();
        assert_eq!(first.update().events().len(), 1);
        assert!(native.take().is_none());
        native.release();
        assert!(first.update().events().is_empty());
        assert!(!native.available());
    }
    #[test]
    fn new_press_cannot_inherit_an_old_ticket_or_active_session() {
        let native = NativeDrag::default();
        native.press();
        let mut first = dnd::Controller::new();
        first.start_outbound(&mut Queue(&native), files()).unwrap();
        native.press();
        assert!(native.take().is_none());
        assert_eq!(first.update().events().len(), 1);
        let active = native.begin();
        let mut second = dnd::Controller::new();
        assert!(second.start_outbound(&mut Queue(&native), files()).is_err());
        assert!(!native.available());
        drop(active);
        assert!(!native.available());
        native.press();
        assert!(native.available());
    }
}
