use xkbcommon_dl as xkbc;

pub(crate) type Keycode = xkbcommon_dl::xkb_keycode_t;
/// A xkbcommon state object.
pub struct XkbcommonState {
    state: *mut xkbc::xkb_state,
    xkb_common: &'static xkbc::XkbCommon,
}

impl XkbcommonState {
    #[cfg(test)]
    pub(crate) fn for_test(layout: &std::ffi::CStr) -> Self {
        let xkb_common = xkbc::xkbcommon_option().expect("libxkbcommon for native keyboard tests");
        let names = xkbc::xkb_rule_names {
            rules: c"evdev".as_ptr(),
            model: c"pc105".as_ptr(),
            layout: layout.as_ptr(),
            variant: c"".as_ptr(),
            options: c"".as_ptr(),
        };
        unsafe {
            let context =
                (xkb_common.xkb_context_new)(xkbc::xkb_context_flags::XKB_CONTEXT_NO_FLAGS);
            assert!(!context.is_null());
            let keymap = (xkb_common.xkb_keymap_new_from_names)(
                context,
                &names,
                xkbc::xkb_keymap_compile_flags::XKB_KEYMAP_COMPILE_NO_FLAGS,
            );
            assert!(!keymap.is_null());
            let state = (xkb_common.xkb_state_new)(keymap);
            assert!(!state.is_null());
            (xkb_common.xkb_keymap_unref)(keymap);
            (xkb_common.xkb_context_unref)(context);
            Self { state, xkb_common }
        }
    }

    pub fn new(xcb_connection: &crate::platform::X11Connection) -> Option<Self> {
        let xkb_common = xkbc::xkbcommon_option()?;
        let xkb_x11 = xkbc::x11::xkbcommon_x11_option()?;

        // libxkbcommon is avail, let's allocate a new context
        let context =
            unsafe { (xkb_common.xkb_context_new)(xkbc::xkb_context_flags::XKB_CONTEXT_NO_FLAGS) };

        let conn: *mut xkbc::x11::xcb_connection_t =
            xcb_connection.conn.xcb_connection().get_raw_xcb_connection();

        let state = unsafe {
            let device_id = (xkb_x11.xkb_x11_get_core_keyboard_device_id)(conn);
            assert!(device_id >= 0);
            // keymap should be unref when XkbcommonState drop
            let keymap = (xkb_x11.xkb_x11_keymap_new_from_device)(
                context,
                conn,
                device_id,
                xkbc::xkb_keymap_compile_flags::XKB_KEYMAP_COMPILE_NO_FLAGS,
            );
            // state will be unref on drop
            (xkb_x11.xkb_x11_state_new_from_device)(keymap, conn, device_id)
        };

        // free the context, it's no longer needed
        unsafe { (xkb_common.xkb_context_unref)(context) };

        Some(XkbcommonState { state, xkb_common })
    }

    pub fn key_get_utf8(&self, code: Keycode) -> String {
        // A buffer to store the cstr
        let buffer_size = 32;
        let mut buffer = vec![0; buffer_size];
        let result = unsafe {
            (self.xkb_common.xkb_state_key_get_utf8)(
                self.state,
                code,
                buffer.as_mut_ptr(),
                buffer_size,
            )
        };

        // Convert back to String
        if result < 0 {
            "".to_string()
        } else {
            let c_str = unsafe { std::ffi::CStr::from_ptr(buffer.as_ptr()) };
            match c_str.to_str() {
                Ok(s) => s.to_string(),
                Err(_) => "".to_string(),
            }
        }
    }

    /// Synchronize the client translator with an X11 core key event's state.
    /// The server owns modifier and lock transitions. Replaying local key edges
    /// misses releases outside this window and keys filtered by an input method.
    pub fn update_core_state(&mut self, state: u16) {
        // X11's low eight bits are the real modifier mask; bits 13-14 are
        // the effective keyboard group. Button bits must not become modifiers.
        let modifiers = u32::from(state & 0xff);
        let group = u32::from((state >> 13) & 3);
        unsafe {
            (self.xkb_common.xkb_state_update_mask)(self.state, modifiers, 0, 0, 0, 0, group);
        }
    }
}

impl Drop for XkbcommonState {
    fn drop(&mut self) {
        unsafe { (self.xkb_common.xkb_state_unref)(self.state) };
    }
}
