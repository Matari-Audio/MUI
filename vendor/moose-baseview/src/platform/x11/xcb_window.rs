use crate::dpi::PhysicalSize;
use crate::platform::x11::error::CookieExt;
use crate::platform::x11::visual_info::WindowVisualConfig;
use crate::platform::X11Connection;
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::num::{NonZero, NonZeroU32};
use std::rc::Rc;
use x11rb::connection::Connection;
use x11rb::cookie::VoidCookie;
use x11rb::errors::{ConnectionError, ReplyOrIdError};
use x11rb::properties::WmSizeHints;
use x11rb::protocol::xproto::{
    AtomEnum, ConfigureWindowAux, ConnectionExt as _, CreateWindowAux, EventMask, PropMode,
    WindowClass,
};
use x11rb::wrapper::ConnectionExt as _;
use x11rb::xcb_ffi::XCBConnection;

pub struct XcbWindow {
    connection: Rc<X11Connection>,
    window_id: NonZeroU32,
    destroyed: Cell<bool>,
    color_map: Option<x11rb::protocol::xproto::Colormap>,
    /// Sizes asked of the server, oldest first, whose ConfigureNotify has not arrived.
    requested: RefCell<VecDeque<(u32, u32)>>,
}

impl XcbWindow {
    pub fn new(
        connection: Rc<X11Connection>, size: PhysicalSize<u16>, visual_info: &WindowVisualConfig,
        parent_id: Option<NonZeroU32>,
    ) -> Result<Self, ReplyOrIdError> {
        let window_id =
            NonZero::new(connection.conn.generate_id()?).ok_or(ReplyOrIdError::IdsExhausted)?;

        connection
            .conn
            .create_window(
                visual_info.visual_depth,
                window_id.get(),
                parent_id.map_or(connection.default_screen().root, NonZeroU32::get),
                0,                 // x coordinate of the new window
                0,                 // y coordinate of the new window
                size.width.max(1), // X11 rejects zero-sized native drawables
                size.height.max(1),
                0, // window border
                WindowClass::INPUT_OUTPUT,
                visual_info.visual_id,
                &CreateWindowAux::new()
                    .event_mask(
                        EventMask::EXPOSURE
                            | EventMask::POINTER_MOTION
                            | EventMask::BUTTON_PRESS
                            | EventMask::BUTTON_RELEASE
                            | EventMask::KEY_PRESS
                            | EventMask::KEY_RELEASE
                            | EventMask::STRUCTURE_NOTIFY
                            | EventMask::ENTER_WINDOW
                            | EventMask::LEAVE_WINDOW
                            | EventMask::FOCUS_CHANGE,
                    )
                    // As mentioned above, these two values are needed to be able to create a window
                    // with a depth of 32-bits when the parent window has a different depth
                    .colormap(visual_info.color_map)
                    .background_pixel(0)
                    .border_pixel(0)
                    // Keep the last frame in place on resize: the default
                    // ForgetGravity clears the whole window to the background
                    // (black) until the next frame lands.
                    .bit_gravity(x11rb::protocol::xproto::Gravity::NORTH_WEST),
            )?
            .check()?;

        Ok(Self {
            window_id,
            connection,
            destroyed: Cell::new(false),
            color_map: visual_info.color_map,
            requested: RefCell::default(),
        })
    }

    pub fn map_window(&self) -> Result<VoidCookie<'_, XCBConnection>, ConnectionError> {
        self.connection.conn.map_window(self.window_id.get())
    }

    pub fn unmap_window(&self) -> Result<VoidCookie<'_, XCBConnection>, ConnectionError> {
        self.connection.conn.unmap_window(self.window_id.get())
    }

    pub fn resize(
        &self, size: PhysicalSize<u32>,
    ) -> Result<VoidCookie<'_, XCBConnection>, ConnectionError> {
        let (width, height) = (size.width.max(1), size.height.max(1));
        let mut requested = self.requested.borrow_mut();
        if requested.back() != Some(&(width, height)) {
            if requested.len() == 16 {
                requested.pop_front();
            }
            requested.push_back((width, height));
        }
        drop(requested);
        self.connection.conn.configure_window(
            self.id().get(),
            &ConfigureWindowAux::new().width(width).height(height),
        )
    }

    /// Whether a ConfigureNotify of `size` only echoes one of our own resizes
    /// that a later one has superseded. Forwarding it would hand the host a
    /// stale size, and the host would snap the editor back to it.
    pub fn is_stale_echo(&self, size: (u32, u32)) -> bool {
        stale_echo(&mut self.requested.borrow_mut(), size)
    }

    pub fn reparent(
        &self, parent: Option<NonZeroU32>,
    ) -> Result<VoidCookie<'_, XCBConnection>, ConnectionError> {
        self.connection.conn.reparent_window(
            self.id().get(),
            parent.map_or(self.connection.default_screen().root, NonZeroU32::get),
            0,
            0,
        )
    }

    pub fn set_title(&self, title: &str) -> Result<VoidCookie<'_, XCBConnection>, ConnectionError> {
        self.connection.conn.change_property8(
            PropMode::REPLACE,
            self.window_id.get(),
            AtomEnum::WM_NAME,
            AtomEnum::STRING,
            title.as_bytes(),
        )
    }

    pub fn enable_wm_protocols(&self) -> Result<VoidCookie<'_, XCBConnection>, ConnectionError> {
        self.connection.conn.change_property32(
            PropMode::REPLACE,
            self.window_id.get(),
            self.connection.atoms.WM_PROTOCOLS,
            AtomEnum::ATOM,
            &[self.connection.atoms.WM_DELETE_WINDOW],
        )
    }

    pub fn enable_dnd_protocols(&self) -> Result<VoidCookie<'_, XCBConnection>, ConnectionError> {
        self.connection.conn.change_property32(
            PropMode::REPLACE,
            self.window_id.get(),
            self.connection.atoms.XdndAware,
            AtomEnum::ATOM,
            &[5u32], // Latest version; hasn't changed since 2002
        )
    }

    pub fn set_size_hints(
        &self, size_hints: WmSizeHints,
    ) -> Result<VoidCookie<'_, XCBConnection>, ConnectionError> {
        size_hints.set_normal_hints(&self.connection.conn as &XCBConnection, self.window_id.get())
    }

    pub fn mark_destroyed(&self) {
        self.destroyed.set(true);
    }

    pub fn is_destroyed(&self) -> bool {
        self.destroyed.get()
    }

    #[inline]
    pub fn id(&self) -> NonZeroU32 {
        self.window_id
    }
}

impl Drop for XcbWindow {
    fn drop(&mut self) {
        // A host destroying its parent already released this server resource.
        if !self.is_destroyed() {
            match self.connection.conn.destroy_window(self.window_id.get()) {
                Err(e) => crate::warn!("Failed to send request to destroy X window: {}", e),
                Ok(cookie) => cookie.check_warn(),
            }
        }
        if let Some(color_map) = self.color_map {
            if let Ok(cookie) = self.connection.conn.free_colormap(color_map) {
                cookie.check_warn();
            }
        }
    }
}

fn stale_echo(requested: &mut VecDeque<(u32, u32)>, size: (u32, u32)) -> bool {
    let Some(i) = requested.iter().position(|r| *r == size) else { return false };
    requested.drain(..=i);
    !requested.is_empty()
}

#[cfg(test)]
mod echo_tests {
    use super::*;

    #[test]
    fn only_superseded_echoes_are_stale() {
        let mut q = VecDeque::from([(800, 500), (900, 600)]);
        assert!(stale_echo(&mut q, (800, 500)), "older request, newer pending");
        assert!(!stale_echo(&mut q, (900, 600)), "latest request applies");
        assert!(!stale_echo(&mut q, (1000, 700)), "a size we never asked for applies");
        assert!(q.is_empty());
    }
}
