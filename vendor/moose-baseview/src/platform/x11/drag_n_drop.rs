use super::xcb_connection::{Atoms, GetPropertyError};
use super::*;
use crate::dpi::PhysicalPosition;
use crate::handler::WindowHandler;
use crate::platform::x11::error::ReplyExt;
use crate::warn;
use crate::{DropData, DropEffect, Event, EventStatus, MouseEvent};
use core::result::Result;
use keyboard_types::Modifiers;
use percent_encoding::percent_decode;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::{
    io, mem,
    path::{Path, PathBuf},
    str::Utf8Error,
};
use x11rb::cookie::VoidCookie;
use x11rb::errors::ConnectionError;
use x11rb::protocol::xproto::{Atom, ClientMessageEvent, SelectionNotifyEvent, Timestamp};
use x11rb::xcb_ffi::XCBConnection;
use x11rb::{
    protocol::xproto::{self, ConnectionExt},
    x11_utils::Serialize,
};
use DragNDropState::*;

/// The Drag-N-Drop session state of a `baseview` X11 window, for which it is the target.
///
/// For more information about what the heck is going on here, see the
/// [XDND (X Drag-n-Drop) specification](https://www.freedesktop.org/wiki/Specifications/XDND/).
pub(crate) enum DragNDropState {
    /// There is no active XDND session for this window.
    NoCurrentSession,
    /// At some point in this session's lifetime, we have decided we couldn't possibly handle the
    /// source's Drop data. Every request from this source window from now on will be rejected,
    /// until either a Leave or Drop event is received.
    PermanentlyRejected {
        /// The source window the rejected drag session originated from.
        source_window: xproto::Window,
        protocol_version: u8,
    },
    /// We have registered a new session (after receiving an Enter event), and are now waiting
    /// for a position event.
    WaitingForPosition {
        /// The protocol version used in this session.
        protocol_version: u8,
        /// The source window the current drag session originates from.
        source_window: xproto::Window,
    },
    /// We have performed a request for data (via `XConvertSelection`), and are now waiting for a
    /// reply.
    ///
    /// More position events can still be received to further update the position data.
    WaitingForData {
        /// The protocol version used in this session.
        protocol_version: u8,
        /// The source window the current drag session originates from.
        source_window: xproto::Window,
        /// The current position of the pointer, from the last received position event.
        position: PhysicalPosition<i16>,
        /// The timestamp of the event we made the selection request from.
        ///
        /// This is either from the first position event, or from the drop event if it arrived first.
        ///
        /// In very old versions of the protocol (v0), this timestamp isn't provided. In that case,
        /// this will be `None`.
        requested_at: Option<Timestamp>,

        requested_action: Option<DndAction>,
        /// This will be true if we received a drop event *before* we managed to fetch the data.
        ///
        /// If this is true, this means we must complete the drop upon receiving the data, instead
        /// of just going to [`Ready`].
        dropped: bool,
    },
    /// We have completed our quest for the drop data. All fields are populated, and the
    /// [`WindowHandler`] has been notified about the drop session.
    ///
    /// We are now waiting for the user to either drop the file, or leave the window.
    ///
    /// More position events can still be received to further update the position data.
    Ready {
        /// The protocol version used in this session.
        protocol_version: u8,
        /// The source window the current drag session originates from.
        source_window: xproto::Window,
        position: PhysicalPosition<i16>,
        data: DropData,
        requested_action: Option<DndAction>,
        accepted_action: Option<DndAction>,
    },
}

// Note: For all X11 operations on this type, only ConnectionErrors are considered fatal.
// Other errors (protocol errors, transfer errors) should be dealt with as gracefully as possible.
impl DragNDropState {
    pub fn handle_enter_event(
        &mut self, window: &WindowInner, handler: &dyn WindowHandler, event: &ClientMessageEvent,
    ) -> Result<(), ConnectionError> {
        let data = event.data.as_data32();

        let source_window = data[0] as xproto::Window;
        let [protocol_version, _, _, flags] = data[1].to_be_bytes();
        if protocol_version > 5 {
            return Ok(());
        }

        // Fetch the list of supported data types. It can be either stored inline in the event, or
        // in a separate property on the source window.
        const FLAG_HAS_MORE_TYPES: u8 = 1 << 0;
        let has_more_types = (FLAG_HAS_MORE_TYPES & flags) == FLAG_HAS_MORE_TYPES;

        let extra_types;
        let supported_types = if !has_more_types {
            &data[2..5]
        } else {
            let result = window.connection.get_property(
                source_window,
                window.connection.atoms.XdndTypeList,
                xproto::Atom::from(xproto::AtomEnum::ATOM),
            );

            match result {
                Err(GetPropertyError::ConnectionError(e)) => return Err(e),
                Ok(v) => {
                    extra_types = v;
                    &extra_types
                }
                Err(e) => {
                    warn!(
                        "XdndEnter: Failed to fetch XdndTypeList from source window {source_window}: {}",
                        e
                    );
                    &data[2..5]
                }
            }
        };

        // We only support the TextUriList type
        let data_type_supported = supported_types.contains(&window.connection.atoms.TextUriList);

        // If there was an active drag session that we informed the handler about, we need to
        // generate the matching DragLeft before cancelling the previous session.
        let interrupted_active_drag = matches!(self, Ready { .. });

        // Clear any previous state, and mark the new session as started if we can handle the drop.
        *self = if data_type_supported {
            WaitingForPosition { source_window, protocol_version }
        } else {
            // Permanently reject the drop if the data isn't supported.
            PermanentlyRejected { source_window, protocol_version }
        };

        // Do this at the end, in case the handler panics, so that it doesn't poison our internal state.
        if interrupted_active_drag {
            handler.on_event(Event::Mouse(MouseEvent::DragLeft));
        }

        Ok(())
    }

    pub fn handle_position_event(
        &mut self, window: &WindowInner, handler: &dyn WindowHandler, event: &ClientMessageEvent,
    ) -> Result<(), ConnectionError> {
        let event_data = event.data.as_data32();

        let event_source_window = event_data[0] as xproto::Window;
        let (event_x, event_y) = decode_xy(event_data[2]);

        match self {
            // Someone sent us a position event without first sending an enter event.
            // Weird, but we'll still politely tell them we reject the drop.
            NoCurrentSession => Ok(send_status_rejected(event_source_window, window)?),

            // The current session's source window does not match the given event.
            // This means it can either be from a stale session, or a misbehaving app.
            // In any case, we ignore the event but still tell the source we reject the drop.
            WaitingForPosition { source_window, .. }
            | PermanentlyRejected { source_window, .. }
            | WaitingForData { source_window, .. }
            | Ready { source_window, .. }
                if *source_window != event_source_window =>
            {
                Ok(send_status_rejected(event_source_window, window)?)
            }

            // We decided to permanently reject this drop.
            // This means the WindowHandler can't do anything with the data, so we reject the drop.
            PermanentlyRejected { .. } => Ok(send_status_rejected(event_source_window, window)?),

            // This is the position event we were waiting for. Now we can request the selection data.
            // The code above already checks that source_window == event_source_window.
            WaitingForPosition { protocol_version, source_window } => {
                let protocol_version = *protocol_version;
                let source_window = *source_window;

                // In version 0, time isn't specified
                let timestamp = (protocol_version >= 1).then_some(event_data[3] as Timestamp);
                // In version <2, action isn't specified
                let requested_action =
                    requested_action(protocol_version, event_data[4], &window.connection.atoms);

                if let Err(e) = request_convert_selection(window, timestamp)?.check() {
                    warn!("XdndPosition: ConvertSelection request failed: {}", e);

                    // Permanently reject the drop if we couldn't convert selection
                    *self = PermanentlyRejected { source_window, protocol_version };
                    send_status_rejected(event_source_window, window)?;
                    return Ok(());
                }

                // We set our state before translating position data, in case that fails.
                *self = WaitingForData {
                    protocol_version,
                    requested_at: timestamp,
                    source_window: event_source_window,
                    position: PhysicalPosition::new(0, 0),
                    requested_action,
                    dropped: false,
                };

                let WaitingForData { position, .. } = self else { unreachable!() };

                if let Some(result) = translate_root_coordinates(window, event_x, event_y)? {
                    *position = result;
                }

                Ok(())
            }

            // We are still waiting for the data. So we'll just update the position in the meantime.
            WaitingForData {
                position,
                protocol_version,
                requested_action: action,
                dropped,
                ..
            } => {
                if *dropped {
                    return Ok(());
                }
                *action =
                    requested_action(*protocol_version, event_data[4], &window.connection.atoms);
                if let Some(result) = translate_root_coordinates(window, event_x, event_y)? {
                    *position = result;
                }

                Ok(())
            }

            // We have already received the data. We can update the position and notify the handler
            Ready {
                protocol_version,
                position,
                data,
                requested_action: action,
                accepted_action,
                ..
            } => {
                if let Some(result) = translate_root_coordinates(window, event_x, event_y)? {
                    *position = result;
                }

                // In version <2, action isn't specified
                *action =
                    requested_action(*protocol_version, event_data[4], &window.connection.atoms);

                let status = handler.on_event(Event::Mouse(MouseEvent::DragMoved {
                    position: position.cast(),
                    data: data.clone(),
                    // We don't get modifiers for drag n drop events.
                    modifiers: Modifiers::empty(),
                }));
                *accepted_action = accepted_drop_action(*protocol_version, *action, status);
                send_status_event(
                    event_source_window,
                    window,
                    *protocol_version,
                    *accepted_action,
                )?;
                Ok(())
            }
        }
    }

    pub fn handle_leave_event(&mut self, handler: &dyn WindowHandler, event: &ClientMessageEvent) {
        let data = event.data.as_data32();
        let event_source_window = data[0] as xproto::Window;

        let current_source_window = match self {
            NoCurrentSession => return,
            WaitingForPosition { source_window, .. }
            | PermanentlyRejected { source_window, .. }
            | WaitingForData { source_window, .. }
            | Ready { source_window, .. } => *source_window,
        };

        // Only accept the leave event if it comes from the source window of the current drag session.
        if event_source_window != current_source_window {
            return;
        }

        // If there was an active drag session that we informed the handler about, we need to
        // generate the matching DragLeft before cancelling the previous session.
        let left_active_drag = matches!(self, Ready { .. });

        // Clear everything.
        *self = NoCurrentSession;

        // Do this at the end, in case the handler panics, so that it doesn't poison our internal state.
        if left_active_drag {
            handler.on_event(Event::Mouse(MouseEvent::DragLeft));
        }
    }

    pub fn handle_drop_event(
        &mut self, window: &WindowInner, handler: &dyn WindowHandler, event: &ClientMessageEvent,
    ) -> Result<(), ConnectionError> {
        let data = event.data.as_data32();

        let event_source_window = data[0] as xproto::Window;

        match self {
            // There is no active drop to finish (including a repeated Drop).
            NoCurrentSession => return Ok(()),

            // The current session's source window does not match the given event.
            // This means it can either be from a stale session, or a misbehaving app.
            // Do not terminate a different source's session.
            WaitingForPosition { source_window, .. }
            | PermanentlyRejected { source_window, .. }
            | WaitingForData { source_window, .. }
            | Ready { source_window, .. }
                if *source_window != event_source_window =>
            {
                return Ok(());
            }

            // We decided to permanently reject this drop.
            // This means the WindowHandler can't do anything with the data, so we reject the drop.
            PermanentlyRejected { protocol_version, .. } => {
                let protocol_version = *protocol_version;
                *self = NoCurrentSession;

                send_finished_event(event_source_window, window, protocol_version, None)?;
            }

            // We received a drop event without any position event. That's very weird, but not
            // irrecoverable: the drop event provides enough data as it is.
            // The code above already checks that source_window == event_source_window.
            WaitingForPosition { protocol_version, .. } => {
                // In version 0, time isn't specified
                let timestamp = (*protocol_version >= 1).then_some(data[2] as Timestamp);

                // We have the timestamp, we can use it to request to convert the selection,
                // even in this state.

                // If we fail to send the request when the drop has completed, we can't do anything.
                // Just cancel the drop.
                if let Err(e) = request_convert_selection(window, timestamp)?.check() {
                    warn!("XdndDrop: ConvertSelection request failed: {}", e);
                    let protocol_version = *protocol_version;
                    *self = NoCurrentSession;

                    // Try to inform the source that we ended up rejecting the drop.
                    return send_finished_event(
                        event_source_window,
                        window,
                        protocol_version,
                        None,
                    );
                };

                *self = WaitingForData {
                    protocol_version: *protocol_version,
                    requested_at: timestamp,
                    source_window: event_source_window,
                    // We don't have usable position data. Maybe we'll receive a position later,
                    // but otherwise this will have to do.
                    position: PhysicalPosition::new(0, 0),
                    requested_action: (*protocol_version < 2).then_some(DndAction::Copy),
                    dropped: true,
                };
            }

            // We are still waiting to receive the data.
            // In that case, we'll wait to receive all of it before finalizing the drop.
            WaitingForData { dropped, requested_at, .. } => {
                // If we have a timestamp, that means this is version >= 1.
                if let Some(requested_at) = *requested_at {
                    let event_timestamp = data[2] as Timestamp;

                    // Just in case, check if this drop event isn't stale
                    if requested_at > event_timestamp {
                        return Ok(());
                    }
                }

                // Indicate to the selection_notified handler that the user has performed the drop.
                // Now it should complete the drop instead of just waiting for more events.
                *dropped = true;
            }

            // The normal case.
            Ready { .. } => {
                let Ready {
                    data,
                    position,
                    requested_action,
                    accepted_action,
                    protocol_version,
                    ..
                } = mem::replace(self, NoCurrentSession)
                else {
                    unreachable!()
                };

                let action = complete_drop(
                    handler,
                    data,
                    position,
                    protocol_version,
                    requested_action,
                    accepted_action,
                );
                send_finished_event(event_source_window, window, protocol_version, action)?;
            }
        };

        Ok(())
    }

    pub fn handle_selection_notify_event(
        &mut self, window: &WindowInner, handler: &dyn WindowHandler, event: &SelectionNotifyEvent,
    ) -> Result<(), ConnectionError> {
        // Ignore the event if we weren't actually waiting for a selection notify event
        let WaitingForData {
            source_window,
            requested_at,
            position,
            dropped,
            protocol_version,
            requested_action,
        } = *self
        else {
            return Ok(());
        };

        // Ignore if this was meant for another window (?)
        if event.requestor != window.xcb_window.id().get() {
            return Ok(());
        }
        if event.selection != window.connection.atoms.XdndSelection
            || event.target != window.connection.atoms.TextUriList
            || (event.property != x11rb::NONE
                && event.property != window.connection.atoms.XdndSelection)
        {
            return Ok(());
        }

        // Ignore if this is stale selection data.
        if let Some(requested_at) = requested_at {
            if requested_at != event.time {
                return Ok(());
            }
        }

        // The sender should have set the data on our window, let's fetch it.
        let data = if event.property == x11rb::NONE { None } else { fetch_dnd_data(window)? };
        match data {
            None => {
                if dropped {
                    *self = NoCurrentSession;
                    send_finished_event(source_window, window, protocol_version, None)?;
                } else {
                    *self = PermanentlyRejected { source_window, protocol_version };
                    send_status_rejected(source_window, window)?;
                }
            }
            Some(data) => {
                // Handle the case where the user already dropped, but we only received the data later.
                if dropped {
                    *self = NoCurrentSession;

                    // Now that we have actual drop data, we can inform the handler about the drag AND drop events.
                    let status = handler.on_event(Event::Mouse(MouseEvent::DragEntered {
                        position: position.cast(),
                        data: data.clone(),
                        // We don't get modifiers for drag n drop events.
                        modifiers: Modifiers::empty(),
                    }));

                    let accepted_action =
                        accepted_drop_action(protocol_version, requested_action, status);
                    let action = complete_drop(
                        handler,
                        data,
                        position,
                        protocol_version,
                        requested_action,
                        accepted_action,
                    );
                    send_finished_event(source_window, window, protocol_version, action)?;
                } else {
                    // Save the data, now that we finally have it!
                    *self = Ready {
                        data: data.clone(),
                        source_window,
                        position,
                        requested_action,
                        protocol_version,
                        accepted_action: None,
                    };

                    // Now that we have actual drop data, we can inform the handler about the drag event.
                    let status = handler.on_event(Event::Mouse(MouseEvent::DragEntered {
                        position: position.cast(),
                        data,
                        // We don't get modifiers for drag n drop events.
                        modifiers: Modifiers::empty(),
                    }));

                    let action = accepted_drop_action(protocol_version, requested_action, status);
                    let Ready { accepted_action, .. } = self else { unreachable!() };
                    *accepted_action = action;
                    send_status_event(source_window, window, protocol_version, action)?;
                }
            }
        }

        Ok(())
    }
}

fn requested_action(version: u8, atom: Atom, atoms: &Atoms) -> Option<DndAction> {
    if version < 2 {
        Some(DndAction::Copy)
    } else {
        DndAction::from_atom(atom, atoms)
    }
}

// XDND permits Copy as a fallback, but Move/Link must match the source request.
// Scroll has no corresponding XDND action; an unknown request is never accepted.
fn accepted_drop_action(
    version: u8, requested: Option<DndAction>, status: EventStatus,
) -> Option<DndAction> {
    let requested = requested?;
    let action = match status {
        EventStatus::AcceptDrop(DropEffect::Copy) => DndAction::Copy,
        EventStatus::AcceptDrop(DropEffect::Move) => DndAction::Move,
        EventStatus::AcceptDrop(DropEffect::Link) => DndAction::Link,
        _ => return None,
    };
    if version < 2 && action != DndAction::Copy {
        return None;
    }
    (action == DndAction::Copy || action == requested).then_some(action)
}

fn complete_drop(
    handler: &dyn WindowHandler, data: DropData, position: PhysicalPosition<i16>, version: u8,
    requested: Option<DndAction>, accepted: Option<DndAction>,
) -> Option<DndAction> {
    // A rejecting hover (including a self reflection) must not trigger an import.
    let result = accepted.and_then(|accepted| {
        let status = handler.on_event(Event::Mouse(MouseEvent::DragDropped {
            position: position.cast(),
            data,
            modifiers: Modifiers::empty(),
        }));
        accepted_drop_action(version, requested, status).filter(|action| *action == accepted)
    });
    if result.is_none() {
        handler.on_event(Event::Mouse(MouseEvent::DragLeft));
    }
    result
}

fn status_data(target: u32, version: u8, action: Option<Atom>) -> [u32; 5] {
    [
        target,
        u32::from(action.is_some()),
        0,
        0,
        if version >= 2 { action.unwrap_or(x11rb::NONE) } else { 0 },
    ]
}

fn finished_data(target: u32, version: u8, action: Option<Atom>) -> [u32; 5] {
    if version >= 5 {
        [target, u32::from(action.is_some()), action.unwrap_or(x11rb::NONE), 0, 0]
    } else {
        [target, 0, 0, 0, 0]
    }
}

fn send_status_rejected(
    source_window: xproto::Window, window: &WindowInner,
) -> Result<(), ConnectionError> {
    send_status_event(source_window, window, 5, None)
}

fn send_status_event(
    source_window: xproto::Window, window: &WindowInner, version: u8, action: Option<DndAction>,
) -> Result<(), ConnectionError> {
    let conn = &window.connection;
    let event = ClientMessageEvent {
        response_type: xproto::CLIENT_MESSAGE_EVENT,
        window: source_window,
        format: 32,
        data: status_data(
            window.xcb_window.id().get(),
            version,
            action.map(|a| a.to_atom(&conn.atoms)),
        )
        .into(),
        sequence: 0,
        type_: conn.atoms.XdndStatus,
    };
    conn.conn
        .send_event(false, source_window, xproto::EventMask::NO_EVENT, event.serialize())?
        .check_warn();
    Ok(())
}

fn send_finished_event(
    source_window: xproto::Window, window: &WindowInner, version: u8, action: Option<DndAction>,
) -> Result<(), ConnectionError> {
    // Finished was introduced in v2. Older sessions have no terminal message.
    if version < 2 {
        return Ok(());
    }
    let conn = &window.connection;
    let event = ClientMessageEvent {
        response_type: xproto::CLIENT_MESSAGE_EVENT,
        window: source_window,
        format: 32,
        data: finished_data(
            window.xcb_window.id().get(),
            version,
            action.map(|a| a.to_atom(&conn.atoms)),
        )
        .into(),
        sequence: 0,
        type_: conn.atoms.XdndFinished,
    };
    conn.conn
        .send_event(false, source_window, xproto::EventMask::NO_EVENT, event.serialize())?
        .check_warn();
    Ok(())
}

fn request_convert_selection(
    window: &WindowInner, timestamp: Option<Timestamp>,
) -> Result<VoidCookie<'_, XCBConnection>, ConnectionError> {
    window.connection.conn.convert_selection(
        window.xcb_window.id().get(),
        window.connection.atoms.XdndSelection,
        window.connection.atoms.TextUriList,
        window.connection.atoms.XdndSelection,
        timestamp.unwrap_or(x11rb::CURRENT_TIME),
    )
}

fn decode_xy(data: u32) -> (i16, i16) {
    ((data >> 16) as i16, data as i16)
}

fn translate_root_coordinates(
    window: &WindowInner, x: i16, y: i16,
) -> Result<Option<PhysicalPosition<i16>>, ConnectionError> {
    let root_id = window.connection.default_screen().root;

    if root_id == window.xcb_window.id().get() {
        return Ok(Some(PhysicalPosition::new(x, y)));
    }

    let reply = window
        .connection
        .conn
        .translate_coordinates(root_id, window.xcb_window.id().get(), x, y)?
        .reply_or_warn();

    let Some(reply) = reply else { return Ok(None) };

    Ok(Some(PhysicalPosition::new(reply.dst_x, reply.dst_y)))
}

#[cfg(test)]
mod tests;

fn fetch_dnd_data(window: &WindowInner) -> Result<Option<DropData>, ConnectionError> {
    let conn = &window.connection;

    let data: Vec<u8> = match conn.get_property(
        window.xcb_window.id().get(),
        conn.atoms.XdndSelection,
        conn.atoms.TextUriList,
    ) {
        Ok(data) => data,
        Err(GetPropertyError::ConnectionError(e)) => return Err(e),
        Err(e) => {
            warn!("{}", e);
            return Ok(None);
        }
    };

    match parse_data(&data) {
        Ok(path_list) => Ok(Some(DropData::Files(path_list))),
        Err(e) => {
            warn!("{}", e);
            Ok(None)
        }
    }
}

// See: https://edeproject.org/spec/file-uri-spec.txt
// TL;DR: format is "file://<hostname>/<path>", hostname is optional and can be "localhost"
fn parse_data(data: &[u8]) -> Result<Vec<PathBuf>, ParseError> {
    if data.is_empty() {
        return Err(ParseError::EmptyData);
    }

    let decoded = percent_decode(data).decode_utf8().map_err(ParseError::InvalidUtf8)?;

    let mut path_list = Vec::new();
    for uri in decoded.split("\r\n").filter(|u| !u.is_empty()) {
        // We only support the file:// protocol
        let Some(mut uri) = uri.strip_prefix("file://") else {
            return Err(ParseError::UnsupportedProtocol(uri.into()));
        };

        if !uri.starts_with('/') {
            // Try (and hope) to see if it's just localhost
            if let Some(stripped) = uri.strip_prefix("localhost") {
                if !stripped.starts_with('/') {
                    // There is something else after "localhost" but before '/'
                    return Err(ParseError::UnsupportedHostname(uri.into()));
                }

                uri = stripped;
            } else {
                // We don't support hostnames.
                return Err(ParseError::UnsupportedHostname(uri.into()));
            }
        }

        let path = Path::new(uri).canonicalize().map_err(ParseError::CanonicalizeError)?;
        path_list.push(path);
    }
    Ok(path_list)
}

#[derive(Debug)]
pub enum ParseError {
    EmptyData,
    InvalidUtf8(Utf8Error),
    UnsupportedHostname(String),
    UnsupportedProtocol(String),
    CanonicalizeError(io::Error),
}

impl Display for ParseError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str("Failed to parse Drag-n-Drop data: ")?;

        match self {
            ParseError::EmptyData => f.write_str("data is empty"),
            ParseError::InvalidUtf8(e) => e.fmt(f),
            ParseError::UnsupportedHostname(uri) => write!(f, "unsupported hostname in URI: {uri}"),
            ParseError::UnsupportedProtocol(uri) => write!(f, "unsupported protocol in URI: {uri}"),
            ParseError::CanonicalizeError(e) => write!(f, "unable to resolve path: {e}"),
        }
    }
}

impl Error for ParseError {}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum DndAction {
    Copy,
    Move,
    Link,
    Ask,
    Private,
}

impl DndAction {
    pub fn from_atom(atom: Atom, atoms: &Atoms) -> Option<Self> {
        if atom == atoms.XdndActionCopy {
            Some(DndAction::Copy)
        } else if atom == atoms.XdndActionMove {
            Some(DndAction::Move)
        } else if atom == atoms.XdndActionLink {
            Some(DndAction::Link)
        } else if atom == atoms.XdndActionAsk {
            Some(DndAction::Ask)
        } else if atom == atoms.XdndActionPrivate {
            Some(DndAction::Private)
        } else {
            None
        }
    }

    pub fn to_atom(self, atoms: &Atoms) -> Atom {
        match self {
            DndAction::Copy => atoms.XdndActionCopy,
            DndAction::Move => atoms.XdndActionMove,
            DndAction::Link => atoms.XdndActionLink,
            DndAction::Ask => atoms.XdndActionAsk,
            DndAction::Private => atoms.XdndActionPrivate,
        }
    }
}
