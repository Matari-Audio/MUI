//! Per-editor outbound ownership, queue admission, and lifecycle delivery.
use crate::{Requests, WindowContext, baseview};
use matari_audio_drag_and_drop as dnd;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Mutex;

/// Admission failure. An existing request is never replaced.
#[derive(Debug)]
pub enum DragQueueError {
    Busy,
    Closed,
    GestureExpired,
    InvalidFiles(dnd::FileSetError),
}
impl std::fmt::Display for DragQueueError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Busy => f.write_str("an outbound drag is queued or active"),
            Self::Closed => f.write_str("the editor is closed"),
            Self::GestureExpired => f.write_str("the initiating primary pointer gesture has ended"),
            Self::InvalidFiles(error) => error.fmt(f),
        }
    }
}
impl std::error::Error for DragQueueError {}

#[derive(Default)]
struct State {
    generation: u64,
    closed: bool,
    busy: bool,
    queued: Option<dnd::FileSet>,
    queue_id: u64,
    rendered_queue: Option<u64>,
    primary: bool,
    gesture: u64,
    queued_gesture: u64,
    active_gesture: Option<u64>,
    events: VecDeque<dnd::SessionEvent>,
}
#[derive(Default)]
pub(crate) struct Mailbox(Mutex<State>);
impl Mailbox {
    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
    pub(crate) fn queue(
        &self,
        paths: Vec<PathBuf>,
        preview: Option<dnd::DragPreview>,
    ) -> Result<(), DragQueueError> {
        let mut state = self.lock();
        if state.closed {
            return Err(DragQueueError::Closed);
        }
        if state.busy {
            return Err(DragQueueError::Busy);
        }
        if !state.primary {
            return Err(DragQueueError::GestureExpired);
        }
        let files = dnd::FileSet::try_from_paths(paths).map_err(DragQueueError::InvalidFiles)?;
        state.queued = Some(match preview {
            Some(preview) => files.with_preview(preview),
            None => files,
        });
        state.busy = true;
        state.queue_id = state.queue_id.wrapping_add(1);
        state.rendered_queue = None;
        state.queued_gesture = state.gesture;
        Ok(())
    }
    fn observe(&self, event: &crate::Event, generation: u64) {
        let mut state = self.lock();
        if state.generation != generation {
            return;
        }
        match event {
            crate::Event::Mouse(crate::MouseEvent::ButtonPressed {
                button: crate::MouseButton::Left,
                ..
            }) => {
                state.gesture = state.gesture.wrapping_add(1);
                state.primary = true;
            }
            crate::Event::Mouse(crate::MouseEvent::ButtonReleased {
                button: crate::MouseButton::Left,
                ..
            }) => state.primary = false,
            _ => {}
        }
    }
    /// Snapshot at frame entry, before a tree can admit new requests mid-build.
    fn frame_token(&self, generation: u64) -> Option<(u64, u64)> {
        let state = self.lock();
        (state.generation == generation && !state.closed && state.queued.is_some())
            .then_some((state.generation, state.queue_id))
    }
    /// A successful present or an unchanged-pixel result acknowledges this scene.
    pub(crate) fn rendered(&self, token: Option<(u64, u64)>) {
        let Some((generation, queue_id)) = token else {
            return;
        };
        let mut state = self.lock();
        if state.generation == generation
            && state.queue_id == queue_id
            && state.queued.is_some()
            && !state.closed
        {
            state.rendered_queue = Some(queue_id);
        }
    }
    fn take(&self, generation: u64, native_authority: bool) -> Option<(dnd::FileSet, bool)> {
        let mut state = self.lock();
        if state.generation != generation || state.closed {
            return None;
        }
        let expired = !state.primary || state.queued_gesture != state.gesture;
        if !expired && native_authority && state.rendered_queue != Some(state.queue_id) {
            return None;
        }
        let files = state.queued.take()?;
        state.active_gesture = Some(state.queued_gesture);
        Some((files, expired))
    }
    pub(crate) fn active(&self) -> bool {
        self.lock().busy
    }
    pub(crate) fn events(&self) -> Vec<dnd::SessionEvent> {
        self.lock().events.drain(..).collect()
    }
}

pub(crate) struct Host {
    controller: dnd::Controller,
    pub(crate) parented: bool,
    generation: u64,
}
impl Host {
    pub(crate) fn new(requests: &std::sync::Arc<Requests>) -> Self {
        let mut state = requests.drag.lock();
        let weak = std::sync::Arc::downgrade(requests);
        if state.generation != 0 {
            state.queued = None;
            state.busy = false;
            state.primary = false;
            state.active_gesture = None;
        }
        state.generation = state.generation.wrapping_add(1);
        state.closed = false;
        Self {
            controller: dnd::Controller::with_wake(std::sync::Arc::new(move || {
                if let Some(requests) = weak.upgrade() {
                    requests.redraw();
                }
            })),
            parented: true,
            generation: state.generation,
        }
    }
    pub(crate) fn observe(&self, mailbox: &Mailbox, event: &crate::Event) {
        mailbox.observe(event, self.generation);
    }
    pub(crate) fn frame_token(&self, mailbox: &Mailbox) -> Option<(u64, u64)> {
        mailbox.frame_token(self.generation)
    }
    pub(crate) fn collect(&mut self, mailbox: &Mailbox) -> bool {
        let mut reset_pointer = false;
        let events = self.controller.update().into_events();
        let mut state = mailbox.lock();
        if state.generation != self.generation {
            return false;
        }
        for event in events {
            if matches!(event, dnd::SessionEvent::OutboundTerminal { .. }) {
                state.busy = state.queued.is_some();
                reset_pointer |= state.active_gesture == Some(state.gesture);
                state.active_gesture = None;
            }
            state.events.push_back(event);
        }
        reset_pointer
    }
    pub(crate) fn drive(&mut self, requests: &Requests, window: &WindowContext) -> bool {
        let mut reset = self.collect(&requests.drag);
        // A coherent scene must have been built while this admission was active.
        let files = requests
            .drag
            .take(self.generation, window.can_schedule_matari_drag());
        if files.is_none() {
            let waiting = {
                let state = requests.drag.lock();
                state.generation == self.generation && !state.closed && state.queued.is_some()
            };
            if waiting {
                requests.redraw();
            }
        }
        if let Some((files, expired)) = files {
            if expired {
                let _ = self
                    .controller
                    .start_outbound(&mut Abandon(self.parented), files);
                reset |= self.collect(&requests.drag);
                return reset;
            }
            if let Err(error) = self.controller.start_outbound(
                &mut baseview::MatariDragAdapter::new(window, self.parented),
                files,
            ) {
                mui::diagnostics::breadcrumb("mui-baseview", "drag_out_failed", &error.to_string());
                requests.drag.lock().busy = false;
            }
        }
        reset |= self.collect(&requests.drag);
        reset
    }
    pub(crate) fn close(&mut self, mailbox: &Mailbox) {
        let queued = {
            let mut state = mailbox.lock();
            if state.generation != self.generation {
                self.controller.cancel_outbound();
                return;
            }
            state.closed = true;
            state.primary = false;
            state.queued.take()
        };
        if let Some(files) = queued {
            let _ = self
                .controller
                .start_outbound(&mut Abandon(self.parented), files);
        }
        self.controller.cancel_outbound();
        self.collect(mailbox);
        let mut state = mailbox.lock();
        if state.generation == self.generation {
            state.closed = true;
            state.queued = None;
            state.busy = false;
        }
    }
}

// Give admitted work an explicit terminal lifecycle even if the editor closes
// before its first presentation. No native handle is accessed on this path.
struct Abandon(bool);
impl dnd::ToolkitAdapter for Abandon {
    type Error = baseview::MatariScheduleError;
    fn outbound_route(&self) -> Option<dnd::SessionRoute> {
        Some(baseview::MatariDragAdapter::route(self.0))
    }
    fn schedule_outbound(
        &mut self,
        ticket: dnd::StartTicket,
    ) -> Result<(), dnd::RejectedStart<Self::Error>> {
        drop(ticket);
        Ok(())
    }
    fn drive_inbound(
        &mut self,
        _: &mut dyn dnd::InboundHandler,
    ) -> Result<dnd::InboundDisposition, Self::Error> {
        Ok(dnd::InboundDisposition::Unhandled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    struct File(PathBuf);
    impl File {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let path =
                std::env::temp_dir().join(format!("mui-drag-{}-{id}.wav", std::process::id()));
            std::fs::write(&path, b"owned export").unwrap();
            Self(path)
        }
    }
    impl Drop for File {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    fn press(requests: &Requests) {
        let generation = requests.drag.lock().generation;
        requests.drag.observe(
            &crate::Event::Mouse(crate::MouseEvent::ButtonPressed {
                button: crate::MouseButton::Left,
                modifiers: keyboard_types::Modifiers::empty(),
            }),
            generation,
        );
    }
    #[test]
    fn occupied_request_is_rejected_without_replacing_the_export() {
        let requests = Arc::new(Requests::default());
        let _host = Host::new(&requests);
        let first = File::new();
        let second = File::new();
        press(&requests);
        requests.drag_out(vec![first.0.clone()]).unwrap();
        assert!(matches!(
            requests.drag_out(vec![second.0.clone()]),
            Err(DragQueueError::Busy)
        ));
        assert_eq!(
            requests.drag.lock().queued.as_ref().unwrap().paths(),
            std::slice::from_ref(&first.0)
        );
        assert!(requests.drag_active());
    }

    #[test]
    fn close_cancels_admitted_work_once_and_rejects_new_requests() {
        let requests = Arc::new(Requests::default());
        let mut host = Host::new(&requests);
        let file = File::new();
        press(&requests);
        requests.drag_out(vec![file.0.clone()]).unwrap();
        host.close(&requests.drag);
        let events = requests.take_drag_events();
        assert_eq!(events.len(), 1);
        assert!(matches!(
            events[0],
            dnd::SessionEvent::OutboundTerminal {
                outcome: dnd::Outcome::Failed(dnd::SessionFailure {
                    kind: dnd::FailureKind::AdapterAbandoned,
                    ..
                }),
                ..
            }
        ));
        host.close(&requests.drag);
        assert!(requests.take_drag_events().is_empty());
        assert!(!requests.drag_active());
        assert!(matches!(
            requests.drag_out(vec![file.0.clone()]),
            Err(DragQueueError::Closed)
        ));
    }

    #[test]
    fn pending_start_requires_a_coherent_scene_and_cannot_migrate_to_a_new_press() {
        let requests = Arc::new(Requests::default());
        let mut host = Host::new(&requests);
        let file = File::new();
        press(&requests);
        let before_admission = requests.drag.frame_token(host.generation);
        requests.drag.queue(vec![file.0.clone()], None).unwrap();
        requests.drag.rendered(before_admission);
        assert!(requests.drag.take(host.generation, true).is_none());
        // A scene entered before admission cannot certify source suppression.
        // Release and a fresh press cannot authorize that admitted work either.
        let generation = requests.drag.lock().generation;
        requests.drag.observe(
            &crate::Event::Mouse(crate::MouseEvent::ButtonReleased {
                button: crate::MouseButton::Left,
                modifiers: keyboard_types::Modifiers::empty(),
            }),
            generation,
        );
        press(&requests);
        let (files, expired) = requests.drag.take(host.generation, true).unwrap();
        assert!(expired);
        host.controller
            .start_outbound(&mut Abandon(true), files)
            .unwrap();
        assert!(
            !host.collect(&requests.drag),
            "an old terminal reset the new press"
        );
        assert_eq!(requests.take_drag_events().len(), 1);
        assert!(!requests.drag_active());
        requests.drag.queue(vec![file.0.clone()], None).unwrap();
        // CPU/GPU unchanged-pixel optimizations are valid acknowledgements too.
        let coherent = requests.drag.frame_token(host.generation);
        requests.drag.rendered(coherent);
        let (_, expired) = requests.drag.take(host.generation, true).unwrap();
        assert!(!expired);
    }

    #[test]
    fn request_without_a_live_primary_press_is_rejected_at_admission() {
        let requests = Requests::default();
        let file = File::new();
        assert!(matches!(
            requests.drag_out(vec![file.0.clone()]),
            Err(DragQueueError::GestureExpired)
        ));
        assert!(!requests.drag_active());
    }

    #[test]
    fn retiring_old_editor_does_not_cancel_new_editor_work() {
        let requests = Arc::new(Requests::default());
        let mut old = Host::new(&requests);
        let mut current = Host::new(&requests);
        let file = File::new();
        press(&requests);
        requests.drag_out(vec![file.0.clone()]).unwrap();
        old.observe(
            &requests.drag,
            &crate::Event::Mouse(crate::MouseEvent::ButtonReleased {
                button: crate::MouseButton::Left,
                modifiers: keyboard_types::Modifiers::empty(),
            }),
        );
        assert!(
            requests.drag.lock().primary,
            "an old editor released the new press"
        );
        assert!(old.frame_token(&requests.drag).is_none());
        old.close(&requests.drag);
        assert!(requests.drag_active());
        assert!(requests.drag.lock().queued.is_some());
        current.close(&requests.drag);
        assert_eq!(requests.take_drag_events().len(), 1);
    }
}
