//! Managed EWMH dock windows. Struts are root-relative, never monitor-relative.
use super::{PlatformError, Result, X11Connection};
use crate::{dpi::LogicalSize, DockEdge, DockOptions};
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
    selected: Option<u32>,
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
        let mut dock = Self { connection, window, options, scale, visible: false, selected: None };
        dock.sync(false)?;
        Ok(dock)
    }
    pub fn refresh(&mut self) -> Result<()> {
        self.sync(self.visible)
    }
    pub fn resize(&mut self, size: LogicalSize<f64>) -> Result<()> {
        self.options.height = self.options.thickness(size);
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
            self.selected
                .and_then(|name| monitors.monitors.iter().find(|m| m.name == name))
                .or_else(|| {
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
                })
        };
        self.selected = selected.map(|m| m.name);
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
        let vertical = self.options.edge.is_vertical();
        let span = if vertical { width } else { height };
        let thickness =
            ((f64::from(self.options.height) * self.scale).round() as u32).min(span / 2).max(1);
        if x < 0
            || y < 0
            || match self.options.edge {
                DockEdge::Top => y != 0,
                DockEdge::Bottom => y + height as i32 != i32::from(root_rect.height),
                DockEdge::Left => x != 0,
                DockEdge::Right => x + width as i32 != i32::from(root_rect.width),
            }
        {
            return Err(PlatformError::CreationFailed(
                "this X11 monitor edge cannot be reserved without covering another display".into(),
            ));
        }
        let thick = thickness as i32;
        let (panel_x, panel_y, panel_width, panel_height) = match self.options.edge {
            DockEdge::Top => (x, y, width, thickness),
            DockEdge::Bottom => (x, y + height as i32 - thick, width, thickness),
            DockEdge::Left => (x, y, thickness, height),
            DockEdge::Right => (x + width as i32 - thick, y, thickness, height),
        };
        conn.configure_window(
            self.window,
            &ConfigureWindowAux::new()
                .x(panel_x)
                .y(panel_y)
                .width(panel_width)
                .height(panel_height),
        )?
        .check()?;
        let mut strut = [0u32; 12];
        if visible {
            // _NET_WM_STRUT_PARTIAL: left, right, top, bottom, then the start/end
            // pair of each edge's span (left/right span y, top/bottom span x).
            let (side, span_start) = match self.options.edge {
                DockEdge::Left => (0, 4),
                DockEdge::Right => (1, 6),
                DockEdge::Top => (2, 8),
                DockEdge::Bottom => (3, 10),
            };
            let (start, length) = if vertical { (y, height) } else { (x, width) };
            strut[side] = thickness;
            strut[span_start] = start as u32;
            strut[span_start + 1] = (start as u32).saturating_add(length).saturating_sub(1);
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
