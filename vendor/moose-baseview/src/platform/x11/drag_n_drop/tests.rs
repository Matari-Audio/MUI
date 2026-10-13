#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "native protocol fixture assertions"
)]
//! Unit policy checks and opt-in recipient wire checks on an isolated X server.
use super::*;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::Arc;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{CreateWindowAux, PropMode, WindowClass};
use x11rb::wrapper::ConnectionExt as _;

#[test]
fn xdnd_root_coordinates_preserve_negative_monitor_positions() {
    assert_eq!(decode_xy(0xffff_fffe), (-1, -2));
    assert_eq!(decode_xy(0x8000_7fff), (i16::MIN, i16::MAX));
    assert_eq!(decode_xy(0x7fff_8000), (i16::MAX, i16::MIN));
    assert_eq!(decode_xy(0x0000_0000), (0, 0));
    assert_eq!(decode_xy(0x007b_01c8), (123, 456));
}

#[test]
fn xdnd_handler_effect_must_be_compatible_with_the_source() {
    let accepted =
        |requested, effect| accepted_drop_action(5, requested, EventStatus::AcceptDrop(effect));
    for action in
        [DndAction::Copy, DndAction::Move, DndAction::Link, DndAction::Ask, DndAction::Private]
    {
        assert_eq!(accepted(Some(action), DropEffect::Copy), Some(DndAction::Copy));
        assert_eq!(accepted_drop_action(5, Some(action), EventStatus::Ignored), None);
        assert_eq!(accepted_drop_action(5, Some(action), EventStatus::Captured), None);
        assert_eq!(accepted(Some(action), DropEffect::Scroll), None);
    }
    assert_eq!(accepted(Some(DndAction::Copy), DropEffect::Move), None);
    assert_eq!(accepted(Some(DndAction::Copy), DropEffect::Link), None);
    assert_eq!(accepted(Some(DndAction::Move), DropEffect::Move), Some(DndAction::Move));
    assert_eq!(accepted(Some(DndAction::Link), DropEffect::Link), Some(DndAction::Link));
    assert_eq!(accepted(None, DropEffect::Copy), None);
    assert_eq!(
        accepted_drop_action(1, Some(DndAction::Copy), EventStatus::AcceptDrop(DropEffect::Move)),
        None
    );
}

#[test]
fn xdnd_replies_use_zero_for_rejection_and_reserved_legacy_fields() {
    assert_eq!(status_data(42, 5, None), [42, 0, 0, 0, 0]);
    assert_eq!(status_data(42, 5, Some(123)), [42, 1, 0, 0, 123]);
    assert_eq!(status_data(42, 1, Some(123)), [42, 1, 0, 0, 0]);
    assert_eq!(finished_data(42, 5, None), [42, 0, 0, 0, 0]);
    assert_eq!(finished_data(42, 5, Some(123)), [42, 1, 123, 0, 0]);
    assert_eq!(finished_data(42, 4, Some(123)), [42, 0, 0, 0, 0]);
}

#[derive(Default)]
struct Handler {
    statuses: RefCell<VecDeque<EventStatus>>,
    events: RefCell<Vec<&'static str>>,
}
impl Handler {
    fn with_statuses(statuses: impl IntoIterator<Item = EventStatus>) -> Self {
        Self { statuses: RefCell::new(statuses.into_iter().collect()), events: RefCell::default() }
    }
}
impl WindowHandler for Handler {
    fn on_frame(&self) -> Result<(), crate::HandlerError> {
        Ok(())
    }
    fn resized(&self, _: crate::WindowSize) -> Result<(), crate::HandlerError> {
        Ok(())
    }
    fn on_event(&self, event: Event) -> EventStatus {
        let kind = match event {
            Event::Mouse(MouseEvent::DragEntered { .. }) => "enter",
            Event::Mouse(MouseEvent::DragMoved { .. }) => "move",
            Event::Mouse(MouseEvent::DragDropped { .. }) => "drop",
            Event::Mouse(MouseEvent::DragLeft) => {
                self.events.borrow_mut().push("left");
                return EventStatus::Ignored;
            }
            _ => panic!("unexpected event"),
        };
        self.events.borrow_mut().push(kind);
        self.statuses.borrow_mut().pop_front().expect("unexpected drag callback")
    }
}

struct Fixture {
    window: Rc<WindowInner>,
    source: x11rb::rust_connection::RustConnection,
    source_window: u32,
}
impl Fixture {
    fn new() -> Self {
        let ev_loop = calloop::EventLoop::try_new().unwrap();
        let window = WindowInner::create(
            crate::WindowSettings::new(),
            &ev_loop,
            Arc::new(super::super::window_thread::WindowThreadShared::new()),
        )
        .unwrap();
        let (source, screen) = x11rb::rust_connection::RustConnection::connect(None).unwrap();
        let source_window = source.generate_id().unwrap();
        source
            .create_window(
                x11rb::COPY_DEPTH_FROM_PARENT,
                source_window,
                source.setup().roots[screen].root,
                0,
                0,
                32,
                32,
                0,
                WindowClass::INPUT_OUTPUT,
                x11rb::COPY_FROM_PARENT,
                &CreateWindowAux::default(),
            )
            .unwrap()
            .check()
            .unwrap();
        Self { window, source, source_window }
    }
    fn waiting(&self, dropped: bool, version: u8) -> DragNDropState {
        WaitingForData {
            protocol_version: version,
            source_window: self.source_window,
            position: PhysicalPosition::new(10, 20),
            requested_at: Some(1),
            requested_action: Some(DndAction::Copy),
            dropped,
        }
    }
    fn payload(&self) -> SelectionNotifyEvent {
        let atoms = &self.window.connection.atoms;
        self.source
            .change_property8(
                PropMode::REPLACE,
                self.window.xcb_window.id().get(),
                atoms.XdndSelection,
                atoms.TextUriList,
                b"file:///tmp\r\n",
            )
            .unwrap()
            .check()
            .unwrap();
        SelectionNotifyEvent {
            response_type: xproto::SELECTION_NOTIFY_EVENT,
            sequence: 0,
            time: 1,
            requestor: self.window.xcb_window.id().get(),
            selection: atoms.XdndSelection,
            target: atoms.TextUriList,
            property: atoms.XdndSelection,
        }
    }
    fn message(&self, kind: Atom, action: Atom) -> ClientMessageEvent {
        ClientMessageEvent {
            response_type: xproto::CLIENT_MESSAGE_EVENT,
            format: 32,
            sequence: 0,
            window: self.window.xcb_window.id().get(),
            type_: kind,
            data: [self.source_window, 0, 0xff9c_ffce, 2, action].into(),
        }
    }
    fn drop_message(&self) -> ClientMessageEvent {
        let mut event = self.message(self.window.connection.atoms.XdndDrop, 0);
        event.data = [self.source_window, 0, 2, 0, 0].into();
        event
    }
    fn replies(&self) -> Vec<ClientMessageEvent> {
        // A round trip orders server delivery; no wall-clock delay is needed.
        self.source.get_input_focus().unwrap().reply().unwrap();
        let mut replies = Vec::new();
        while let Some(event) = self.source.poll_for_event().unwrap() {
            match event {
                x11rb::protocol::Event::ClientMessage(event) => replies.push(event),
                x11rb::protocol::Event::Error(error) => panic!("X protocol error: {error:?}"),
                _ => {}
            }
        }
        replies
    }
    fn assert_reply(&self, kind: Atom, data: [u32; 5]) {
        let replies = self.replies();
        assert_eq!(replies.len(), 1);
        assert_eq!(replies[0].window, self.source_window);
        assert_eq!(replies[0].format, 32);
        assert_eq!(replies[0].type_, kind);
        assert_eq!(replies[0].data.as_data32(), data);
    }
}

#[test]
#[ignore = "requires an isolated live X11 display"]
fn native_xdnd_rejected_hover_can_recover_and_later_reject_without_importing() {
    let f = Fixture::new();
    let target = f.window.xcb_window.id().get();
    let atoms = &f.window.connection.atoms;
    let handler = Handler::with_statuses([
        EventStatus::Ignored,
        EventStatus::AcceptDrop(DropEffect::Copy),
        EventStatus::Ignored,
    ]);
    let mut state = f.waiting(false, 5);
    state.handle_selection_notify_event(&f.window, &handler, &f.payload()).unwrap();
    f.assert_reply(atoms.XdndStatus, [target, 0, 0, 0, 0]);
    assert!(matches!(state, Ready { .. }), "hover rejection must retain the payload");
    let movement = f.message(atoms.XdndPosition, atoms.XdndActionMove);
    state.handle_position_event(&f.window, &handler, &movement).unwrap();
    f.assert_reply(atoms.XdndStatus, [target, 1, 0, 0, atoms.XdndActionCopy]);
    let expected = f
        .source
        .translate_coordinates(f.window.connection.default_screen().root, target, -100, -50)
        .unwrap()
        .reply()
        .unwrap();
    let Ready { position, .. } = &state else { panic!("lost Ready state") };
    assert_eq!((position.x, position.y), (expected.dst_x, expected.dst_y));
    state.handle_position_event(&f.window, &handler, &movement).unwrap();
    f.assert_reply(atoms.XdndStatus, [target, 0, 0, 0, 0]);
    state.handle_drop_event(&f.window, &handler, &f.drop_message()).unwrap();
    f.assert_reply(atoms.XdndFinished, [target, 0, 0, 0, 0]);
    assert_eq!(*handler.events.borrow(), ["enter", "move", "move", "left"]);
    assert!(matches!(state, NoCurrentSession));
    state.handle_drop_event(&f.window, &handler, &f.drop_message()).unwrap();
    assert!(f.replies().is_empty(), "duplicate Drop emitted another Finished");
}

#[test]
#[ignore = "requires an isolated live X11 display"]
fn native_xdnd_finish_follows_drop_result_and_delayed_enter_policy() {
    let f = Fixture::new();
    let target = f.window.xcb_window.id().get();
    let atoms = &f.window.connection.atoms;
    let accept = EventStatus::AcceptDrop(DropEffect::Copy);
    for (dropped, statuses, expected, events) in [
        (false, vec![accept, EventStatus::Ignored], None, vec!["enter", "drop", "left"]),
        (false, vec![accept, accept], Some(atoms.XdndActionCopy), vec!["enter", "drop"]),
        (true, vec![EventStatus::Ignored], None, vec!["enter", "left"]),
        (true, vec![accept, accept], Some(atoms.XdndActionCopy), vec!["enter", "drop"]),
        (true, vec![accept, EventStatus::Ignored], None, vec!["enter", "drop", "left"]),
    ] {
        let handler = Handler::with_statuses(statuses);
        let mut state = f.waiting(dropped, 5);
        let notify = f.payload();
        state.handle_selection_notify_event(&f.window, &handler, &notify).unwrap();
        if !dropped {
            f.assert_reply(atoms.XdndStatus, [target, 1, 0, 0, atoms.XdndActionCopy]);
            state.handle_drop_event(&f.window, &handler, &f.drop_message()).unwrap();
        }
        f.assert_reply(atoms.XdndFinished, finished_data(target, 5, expected));
        assert_eq!(*handler.events.borrow(), events);
        assert!(matches!(state, NoCurrentSession));
        state.handle_selection_notify_event(&f.window, &handler, &notify).unwrap();
        state.handle_drop_event(&f.window, &handler, &f.drop_message()).unwrap();
        assert!(f.replies().is_empty());
    }
}

#[test]
#[ignore = "requires an isolated live X11 display"]
fn native_xdnd_current_action_legacy_finish_and_failed_selection() {
    let f = Fixture::new();
    let target = f.window.xcb_window.id().get();
    let atoms = &f.window.connection.atoms;
    let accept = EventStatus::AcceptDrop(DropEffect::Copy);
    let handler = Handler::with_statuses([accept, EventStatus::AcceptDrop(DropEffect::Move)]);
    let mut state = f.waiting(false, 5);
    // An action change while data is pending must update the current request.
    state
        .handle_position_event(
            &f.window,
            &handler,
            &f.message(atoms.XdndPosition, atoms.XdndActionLink),
        )
        .unwrap();
    assert!(matches!(state, WaitingForData { requested_action: Some(DndAction::Link), .. }));
    state.handle_selection_notify_event(&f.window, &handler, &f.payload()).unwrap();
    f.assert_reply(atoms.XdndStatus, [target, 1, 0, 0, atoms.XdndActionCopy]);
    state
        .handle_position_event(
            &f.window,
            &handler,
            &f.message(atoms.XdndPosition, atoms.XdndActionCopy),
        )
        .unwrap();
    f.assert_reply(atoms.XdndStatus, [target, 0, 0, 0, 0]);
    state.handle_drop_event(&f.window, &handler, &f.drop_message()).unwrap();
    f.assert_reply(atoms.XdndFinished, [target, 0, 0, 0, 0]);
    for version in [0, 1, 2, 4] {
        let handler = Handler::with_statuses([accept, accept]);
        let mut state = f.waiting(true, version);
        state.handle_selection_notify_event(&f.window, &handler, &f.payload()).unwrap();
        if version < 2 {
            assert!(f.replies().is_empty());
        } else {
            f.assert_reply(atoms.XdndFinished, [target, 0, 0, 0, 0]);
        }
        assert!(matches!(state, NoCurrentSession));
    }
    // A failure reply must not consume an older successful property.
    let mut notify = f.payload();
    notify.property = x11rb::NONE;
    let handler = Handler::default();
    let mut state = f.waiting(true, 5);
    state.handle_selection_notify_event(&f.window, &handler, &notify).unwrap();
    f.assert_reply(atoms.XdndFinished, [target, 0, 0, 0, 0]);
    assert!(matches!(state, NoCurrentSession));
    assert!(handler.events.borrow().is_empty());
}
