//! Native bridges for hosts that retain their own `Driver` and window handler:
//! accessibility, and the translation of every baseview event and request
//! MUI's own handlers use (keys, buttons, modifiers, IME, cursors), so a
//! host of your own translates exactly as `mui-baseview` does.
//!
//! Construct accessibility in the pre-show window builder. Its native endpoint
//! stays on the window thread; its UI endpoint may move to the model thread.
//! Apply actions and prepare trees with the UI, then release all model locks
//! and mutable borrows before publishing or changing native focus/IME state.
//! Queue reentrant events rather than dropping them while a frame is borrowed.

use std::hash::{DefaultHasher, Hash, Hasher};

use baseview::dpi::PhysicalPosition;
use baseview::{MouseButton, MouseCursor};
use keyboard_types::{Key as HostKey, KeyState, KeyboardEvent, Modifiers, NamedKey};
use mui::host::{KeyEvent, Modifier, NativeKey};
use mui::prelude::{Button, Cursor, Key, Mods};

pub use crate::a11y::{AccessibilityUi, NativeAccessibility};
pub use mui_access::accesskit::TreeUpdate;

/// Translate a native composition event without losing UTF-8 text ranges.
pub fn ime_event(event: &baseview::Ime) -> mui::prelude::Ime {
    match event {
        baseview::Ime::Selection(range) => mui::prelude::Ime::Selection(range.clone()),
        baseview::Ime::Enabled => mui::prelude::Ime::Enabled,
        baseview::Ime::Preedit { text, cursor } => mui::prelude::Ime::Preedit {
            text: text.clone(),
            cursor: *cursor,
        },
        baseview::Ime::Commit(text) => mui::prelude::Ime::Commit(text.clone()),
        baseview::Ime::Disabled => mui::prelude::Ime::Disabled,
    }
}

/// Convert a driver's text state and caret geometry to native physical pixels.
/// Pass `Driver::ui_scale()` so both DPI and the UI's own zoom are included.
pub fn ime_configuration(
    mut config: mui::host::ImeConfiguration,
    scale: f64,
) -> mui::host::ImeConfiguration {
    config.area.0.x *= scale;
    config.area.0.y *= scale;
    config.area.1.width *= scale;
    config.area.1.height *= scale;
    config
}

/// A driver's text state as baseview's IME request. Scene geometry is in UI
/// points; baseview's contract uses client pixels, so pass `Driver::ui_scale()`.
pub fn native_ime(config: mui::host::ImeConfiguration, scale: f64) -> baseview::ImeConfiguration {
    let config = ime_configuration(config, scale);
    baseview::ImeConfiguration {
        id: config.id,
        position: PhysicalPosition::new(config.area.0.x, config.area.0.y),
        size: baseview::dpi::PhysicalSize::new(config.area.1.width, config.area.1.height),
        text: config.text,
        selection: config.selection,
        marked: config.marked,
    }
}

/// A baseview key as `mui::host` names it.
pub fn key_event(key: &KeyboardEvent) -> KeyEvent {
    let mut code = DefaultHasher::new();
    key.code.hash(&mut code);
    KeyEvent {
        code: code.finish(),
        key: match &key.key {
            HostKey::Character(s) => NativeKey::Text(s.clone()),
            HostKey::Named(NamedKey::Shift) => NativeKey::Modifier(Modifier::Shift),
            HostKey::Named(NamedKey::Control) => NativeKey::Modifier(Modifier::Ctrl),
            HostKey::Named(NamedKey::Alt | NamedKey::AltGraph) => {
                NativeKey::Modifier(Modifier::Alt)
            }
            HostKey::Named(NamedKey::Meta) => NativeKey::Modifier(Modifier::Cmd),
            // keyboard-types prints the W3C name `Key::from_name` reads.
            HostKey::Named(other) => {
                Key::from_fmt(format_args!("{other}")).map_or(NativeKey::Other, NativeKey::Named)
            }
        },
        down: key.state == KeyState::Down,
        mods: mods(key.modifiers),
    }
}

pub const fn mouse_button(b: MouseButton) -> Option<Button> {
    match b {
        MouseButton::Left => Some(Button::Primary),
        MouseButton::Right => Some(Button::Secondary),
        MouseButton::Middle => Some(Button::Middle),
        _ => None,
    }
}

pub const fn mods(m: Modifiers) -> Mods {
    Mods {
        shift: m.contains(Modifiers::SHIFT),
        ctrl: m.contains(Modifiers::CONTROL),
        alt: m.contains(Modifiers::ALT),
        cmd: m.contains(Modifiers::META),
    }
}

pub const fn native_cursor(cursor: Cursor) -> MouseCursor {
    match cursor {
        Cursor::Arrow => MouseCursor::Default,
        Cursor::Hand | Cursor::Grab => MouseCursor::Hand,
        Cursor::Grabbing => MouseCursor::HandGrabbing,
        Cursor::Text => MouseCursor::Text,
        Cursor::ResizeH => MouseCursor::EwResize,
        Cursor::ResizeV => MouseCursor::NsResize,
        Cursor::Crosshair => MouseCursor::Crosshair,
        Cursor::Forbidden => MouseCursor::NotAllowed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_ime_helpers_keep_unicode_ranges_and_convert_the_caret_once() {
        let config = mui::host::ImeConfiguration {
            id: "field".into(),
            area: (
                mui::prelude::Point::new(10., 20.),
                mui::scene::Size::new(1., 16.),
            ),
            text: "a😀é".into(),
            selection: 1..5,
            marked: Some(1..5),
        };
        let physical = ime_configuration(config.clone(), 1.5);
        assert_eq!(
            physical.area,
            (
                mui::prelude::Point::new(15., 30.),
                mui::scene::Size::new(1.5, 24.)
            )
        );
        assert_eq!(
            (physical.text, physical.selection, physical.marked),
            (config.text, config.selection, config.marked)
        );
        assert!(
            matches!(ime_event(&baseview::Ime::Selection(1..5)), mui::prelude::Ime::Selection(r) if r == (1..5))
        );
        assert!(
            matches!(ime_event(&baseview::Ime::Preedit { text: "😀".into(), cursor: Some((0, 4)) }), mui::prelude::Ime::Preedit { text, cursor: Some((0, 4)) } if text == "😀")
        );
        assert!(
            matches!(ime_event(&baseview::Ime::Commit("é".into())), mui::prelude::Ime::Commit(text) if text == "é")
        );
    }
}
