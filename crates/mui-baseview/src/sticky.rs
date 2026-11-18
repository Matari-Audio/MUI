//! Keeping the editor window above others ("sticky window"): the platform
//! call behind the Sticky Window setting. Each returns what the window
//! manager said, for the activity log.

pub fn set_sticky_window(
    window: &baseview::WindowContext,
    enabled: bool,
) -> Result<String, String> {
    #[cfg(target_os = "linux")]
    return linux::set_sticky_window(window, enabled);
    #[cfg(target_os = "macos")]
    return macos::set_sticky_window(window, enabled);
    #[cfg(target_os = "windows")]
    return windows::set_sticky_window(window, enabled);
    #[expect(unreachable_code, reason = "every supported platform returned above")]
    {
        let _ = (window, enabled);
        Err("unsupported platform".to_string())
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{
        Atom, ClientMessageEvent, ConnectionExt, EventMask, Window as XWindow,
    };

    const NET_WM_STATE_REMOVE: u32 = 0;
    const NET_WM_STATE_ADD: u32 = 1;

    pub(super) fn set_sticky_window(
        window: &baseview::WindowContext,
        enabled: bool,
    ) -> Result<String, String> {
        let raw = window
            .window_handle()
            .map_err(|err| err.to_string())?
            .as_raw();
        let origin = match raw {
            RawWindowHandle::Xlib(handle) if handle.window != 0 => handle.window as XWindow,
            RawWindowHandle::Xcb(handle) => handle.window.get() as XWindow,
            RawWindowHandle::Xlib(_) => {
                return Err("window does not have a valid X11/XWayland handle".to_string());
            }
            RawWindowHandle::Wayland(_) => {
                return Err(
                    "native Wayland has no standard always-on-top request for normal plugin windows"
                        .to_string(),
                );
            }
            other => return Err(format!("sticky window needs X11/XWayland; got {other:?}")),
        };

        let (conn, screen_num) = x11rb::connect(None).map_err(|err| err.to_string())?;
        let root = conn.setup().roots[screen_num].root;
        let target = top_level_window(&conn, root, origin)?;
        let net_wm_state = atom(&conn, b"_NET_WM_STATE")?;
        let above = atom(&conn, b"_NET_WM_STATE_ABOVE")?;
        let sticky = atom(&conn, b"_NET_WM_STATE_STICKY")?;
        let action = if enabled {
            NET_WM_STATE_ADD
        } else {
            NET_WM_STATE_REMOVE
        };
        let event =
            ClientMessageEvent::new(32, target, net_wm_state, [action, above, sticky, 1, 0]);

        conn.send_event(
            false,
            root,
            EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
            event,
        )
        .map_err(|err| err.to_string())?;
        conn.flush().map_err(|err| err.to_string())?;

        Ok(format!(
            "Sticky window {} via X11 _NET_WM_STATE on 0x{target:x}",
            if enabled { "enabled" } else { "disabled" }
        ))
    }

    fn atom(conn: &impl Connection, name: &[u8]) -> Result<Atom, String> {
        conn.intern_atom(false, name)
            .map_err(|err| err.to_string())?
            .reply()
            .map(|reply| reply.atom)
            .map_err(|err| err.to_string())
    }

    fn top_level_window(
        conn: &impl Connection,
        root: XWindow,
        mut window: XWindow,
    ) -> Result<XWindow, String> {
        for _ in 0..64 {
            let tree = conn
                .query_tree(window)
                .map_err(|err| err.to_string())?
                .reply()
                .map_err(|err| err.to_string())?;
            if tree.parent == root || tree.parent == 0 {
                return Ok(window);
            }
            window = tree.parent;
        }
        Err("could not resolve X11 top-level window".to_string())
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSFloatingWindowLevel, NSNormalWindowLevel, NSView};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    pub(super) fn set_sticky_window(
        window: &baseview::WindowContext,
        enabled: bool,
    ) -> Result<String, String> {
        let raw = window
            .window_handle()
            .map_err(|err| err.to_string())?
            .as_raw();
        let ns_view: *mut NSView = match raw {
            RawWindowHandle::AppKit(handle) => handle.ns_view.as_ptr().cast(),
            other => {
                return Err(format!(
                    "sticky window needs an AppKit NSView; got {other:?}"
                ));
            }
        };

        let _mtm = MainThreadMarker::new()
            .ok_or_else(|| "sticky window must run on the AppKit main thread".to_string())?;
        // SAFETY: baseview hands out a live NSView for the open window, and
        // this runs on the AppKit main thread (checked above).
        let view = unsafe { &*ns_view };
        let Some(window) = view.window() else {
            return Err("AppKit NSView is not attached to an NSWindow".to_string());
        };

        let level = if enabled {
            NSFloatingWindowLevel
        } else {
            NSNormalWindowLevel
        };
        window.setLevel(level);

        Ok(format!(
            "Sticky window {} via AppKit NSWindow level",
            if enabled { "enabled" } else { "disabled" }
        ))
    }
}

#[cfg(target_os = "windows")]
mod windows {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        GWL_STYLE, GetWindowLongPtrW, HWND_NOTOPMOST, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE,
        SWP_NOOWNERZORDER, SWP_NOSIZE, SetWindowPos, WS_CHILD,
    };

    pub(super) fn set_sticky_window(
        window: &baseview::WindowContext,
        enabled: bool,
    ) -> Result<String, String> {
        let raw = window
            .window_handle()
            .map_err(|err| err.to_string())?
            .as_raw();
        let hwnd = match raw {
            RawWindowHandle::Win32(handle) => HWND(handle.hwnd.get() as *mut _),
            other => return Err(format!("sticky window needs a Win32 HWND; got {other:?}")),
        };

        // SAFETY: `hwnd` is the live window baseview handed us.
        let style = unsafe { GetWindowLongPtrW(hwnd, GWL_STYLE) } as u32;
        if style & WS_CHILD.0 != 0 {
            return Err(
                "embedded Win32 child windows cannot be made topmost without host floating-window support"
                    .to_string(),
            );
        }

        let insert_after = if enabled {
            HWND_TOPMOST
        } else {
            HWND_NOTOPMOST
        };

        // SAFETY: `hwnd` is live and the flags move nothing but its z-order.
        unsafe {
            SetWindowPos(
                hwnd,
                Some(insert_after),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
            )
            .map_err(|err| format!("SetWindowPos failed for {hwnd:?}: {err}"))?;
        }

        Ok(format!(
            "Sticky window {} via Win32 SetWindowPos on {hwnd:?}",
            if enabled { "enabled" } else { "disabled" }
        ))
    }
}
