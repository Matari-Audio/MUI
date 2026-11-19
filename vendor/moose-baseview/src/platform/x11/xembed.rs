//! XEmbed, client side (freedesktop XEmbed spec 0.5). An embedder that speaks
//! it keeps the X input focus for itself, forwards keys to us, and tells us
//! when we hold the logical focus. A click asks the embedder for the focus
//! instead of taking it. Hosts that don't speak it never send `_XEMBED`, and
//! the plain click-to-focus path stays.

/// `_XEMBED_INFO`: protocol version 0, `XEMBED_MAPPED`.
pub(super) const INFO: [u32; 2] = [0, 1];

const EMBEDDED_NOTIFY: u32 = 0;
const REQUEST_FOCUS: u32 = 3;
const FOCUS_IN: u32 = 4;
const FOCUS_OUT: u32 = 5;

/// The `_XEMBED` messages a client acts on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Message {
    EmbeddedNotify {
        embedder: u32,
    },
    FocusIn,
    FocusOut,
    /// Window (de)activation, modality, accelerators: nothing to do.
    Other,
}

/// Reads an `_XEMBED` client message: `[time, message, detail, data1, data2]`.
pub(super) fn parse(data: [u32; 5]) -> Message {
    let [_, message, _, data1, _] = data;
    match message {
        EMBEDDED_NOTIFY => Message::EmbeddedNotify { embedder: data1 },
        FOCUS_IN => Message::FocusIn,
        FOCUS_OUT => Message::FocusOut,
        _ => Message::Other,
    }
}

/// The `_XEMBED` message asking the embedder for the focus, at `time`.
pub(super) fn request_focus(time: u32) -> [u32; 5] {
    [time, REQUEST_FOCUS, 0, 0, 0]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_messages_a_client_handles() {
        assert_eq!(parse([1, 0, 0, 0x42, 0]), Message::EmbeddedNotify { embedder: 0x42 });
        assert_eq!(parse([1, 4, 1, 0, 0]), Message::FocusIn);
        assert_eq!(parse([1, 5, 0, 0, 0]), Message::FocusOut);
        assert_eq!(parse([1, 1, 0, 0, 0]), Message::Other, "window activate");
    }

    #[test]
    fn requests_focus_with_the_clicks_time() {
        assert_eq!(request_focus(77), [77, 3, 0, 0, 0]);
    }
}
