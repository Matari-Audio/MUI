//! Managed EWMH dock windows. Struts are root-relative, never monitor-relative.
use super::{PlatformError, Result, X11Connection};
use crate::{DockEdge, DockOptions};
use std::rc::Rc;
use x11rb::{
    connection::Connection,
    protocol::{
        randr::{ConnectionExt as _, NotifyMask},
        xproto::{AtomEnum, ConfigureWindowAux, ConnectionExt as _, PropMode},
    },
    wrapper::ConnectionExt as _,
};

pub(crate) struct Dock {
    connection: Rc<X11Connection>,
    window: u32,
    options: DockOptions,
    scale: f64,
    visible: bool,
}
impl Dock {
    pub fn new(
        connection: Rc<X11Connection>, window: u32, options: DockOptions, scale: f64,
    ) -> Result<Self> {
        let conn = &connection.conn;
        let root = connection.default_screen().root;
        let supported = atom(&connection, b"_NET_SUPPORTED")?;
        let partial = atom(&connection, b"_NET_WM_STRUT_PARTIAL")?;
        let wm_type = atom(&connection, b"_NET_WM_WINDOW_TYPE")?;
        let dock_type = atom(&connection, b"_NET_WM_WINDOW_TYPE_DOCK")?;
        let response =
            conn.get_property(false, root, supported, AtomEnum::ATOM, 0, 4096)?.reply()?;
        if !response.value32().is_some_and(|mut atoms| atoms.any(|a| a == partial)) {
            return Err(PlatformError::CreationFailed(
                "the X11 window manager does not advertise dock reservation".into(),
            ));
        }
        conn.change_property32(PropMode::REPLACE, window, wm_type, AtomEnum::ATOM, &[dock_type])?
            .check()?;
        let motif = atom(&connection, b"_MOTIF_WM_HINTS")?;
        conn.change_property32(PropMode::REPLACE, window, motif, motif, &[2, 0, 0, 0, 0])?
            .check()?;
        conn.randr_select_input(
            root,
            NotifyMask::SCREEN_CHANGE | NotifyMask::OUTPUT_CHANGE | NotifyMask::CRTC_CHANGE,
        )?
        .check()?;
        let mut dock = Self { connection, window, options, scale, visible: false };
        dock.sync(false)?;
        Ok(dock)
    }
    pub fn resize(&mut self, height: u32) -> Result<()> {
        self.options.height = height.clamp(100, 1024);
        self.sync(self.visible)
    }
    pub fn sync(&mut self, visible: bool) -> Result<()> {
        self.visible = visible;
        let connection = &self.connection;
        let conn = &connection.conn;
        let root = connection.default_screen().root;
        let root_rect = conn.get_geometry(root)?.reply()?;
        let monitors = conn.randr_get_monitors(root, true)?.reply()?;
        let pointer = conn.query_pointer(root)?.reply()?;
        let selected = if let Some(name) = &self.options.output {
            monitors.monitors.iter().find(|m| {
                conn.get_atom_name(m.name)
                    .ok()
                    .and_then(|c| c.reply().ok())
                    .is_some_and(|a| a.name == name.as_bytes())
            })
        } else {
            monitors
                .monitors
                .iter()
                .find(|m| {
                    i32::from(pointer.root_x) >= i32::from(m.x)
                        && i32::from(pointer.root_x) < i32::from(m.x) + i32::from(m.width)
                        && i32::from(pointer.root_y) >= i32::from(m.y)
                        && i32::from(pointer.root_y) < i32::from(m.y) + i32::from(m.height)
                })
                .or_else(|| monitors.monitors.iter().find(|m| m.primary))
                .or_else(|| monitors.monitors.first())
        };
        let (x, y, width, height) = match selected {
            Some(m) => (i32::from(m.x), i32::from(m.y), u32::from(m.width), u32::from(m.height)),
            None if self.options.output.is_none() => {
                (0, 0, u32::from(root_rect.width), u32::from(root_rect.height))
            }
            None => {
                return Err(PlatformError::CreationFailed(
                    "selected X11 dock monitor is unavailable".into(),
                ))
            }
        };
        let thickness =
            ((f64::from(self.options.height) * self.scale).round() as u32).min(height / 2).max(1);
        if x < 0
            || y < 0
            || match self.options.edge {
                DockEdge::Top => y != 0,
                DockEdge::Bottom => y + height as i32 != i32::from(root_rect.height),
            }
        {
            return Err(PlatformError::CreationFailed(
                "this X11 monitor edge cannot be reserved without covering another display".into(),
            ));
        }
        let top = self.options.edge == DockEdge::Top;
        let panel_y = if top { y } else { y + height as i32 - thickness as i32 };
        conn.configure_window(
            self.window,
            &ConfigureWindowAux::new().x(x).y(panel_y).width(width).height(thickness),
        )?
        .check()?;
        let mut strut = [0u32; 12];
        if visible {
            let side = if top { 2 } else { 3 };
            strut[side] = thickness;
            let range = if top { 8 } else { 10 };
            strut[range] = x as u32;
            strut[range + 1] = (x as u32).saturating_add(width).saturating_sub(1);
        }
        let partial = atom(connection, b"_NET_WM_STRUT_PARTIAL")?;
        conn.change_property32(
            PropMode::REPLACE,
            self.window,
            partial,
            AtomEnum::CARDINAL,
            &strut,
        )?
        .check()?;
        conn.flush()?;
        Ok(())
    }
}
impl Drop for Dock {
    fn drop(&mut self) {
        if let Ok(partial) = atom(&self.connection, b"_NET_WM_STRUT_PARTIAL") {
            let _ = self.connection.conn.delete_property(self.window, partial);
            let _ = self.connection.conn.flush();
        }
    }
}
fn atom(connection: &X11Connection, name: &[u8]) -> Result<u32> {
    Ok(connection.conn.intern_atom(false, name)?.reply()?.atom)
}
