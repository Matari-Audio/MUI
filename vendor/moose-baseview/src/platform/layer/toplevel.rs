//! The floating-window role: an xdg toplevel on the same runtime as the dock.
//! Server-side decorations are requested; when the compositor declines (GNOME,
//! Weston) the toolkit's fallback frame is drawn on subsurfaces around it.
use super::*;
use smithay_client_toolkit::{
    reexports::csd_frame::{
        CursorIcon, DecorationsFrame, FrameAction, FrameClick, ResizeEdge as Edge,
    },
    seat::pointer::{PointerData, PointerEvent, PointerEventKind},
    shell::xdg::{
        fallback_frame::FallbackFrame,
        window::{DecorationMode, Window, WindowConfigure, WindowHandler},
    },
};
use wayland_client::protocol::wl_pointer::WlPointer;
use wayland_protocols::{
    wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::Shape,
    xdg::shell::client::xdg_toplevel::ResizeEdge,
};

impl Runtime {
    pub(super) fn toplevel(
        &mut self, globals: &wayland_client::globals::GlobalList, qh: &QueueHandle<Self>,
        compositor: &CompositorState, surface: wl_surface::WlSurface, settings: &WindowSettings,
        logical: (u32, u32),
    ) -> linux::Result<Window> {
        let xdg = XdgShell::bind(globals, qh).map_err(failure)?;
        // Without a subcompositor there is no fallback frame; server decorations may still come.
        self.subcompositor =
            SubcompositorState::bind(compositor.wl_compositor().clone(), globals, qh)
                .ok()
                .map(Arc::new);
        let window = xdg.create_window(surface, WindowDecorations::RequestServer, qh);
        window.set_title(settings.title.clone());
        let size = |s: Option<Size>| {
            s.map(|s| {
                let l = s.to_logical::<f64>(1.0);
                ((l.width.round() as u32).max(1), (l.height.round() as u32).max(1))
            })
        };
        if settings.resizable {
            window.set_min_size(size(settings.min_size));
            window.set_max_size(size(settings.max_size));
        } else {
            window.set_min_size(Some(logical));
            window.set_max_size(Some(logical));
        }
        self._xdg = Some(xdg);
        Ok(window)
    }

    /// Pointer input on a decoration subsurface.
    pub(super) fn frame_pointer(&mut self, pointer: &WlPointer, event: &PointerEvent) {
        let Some(cx) = &self.context else { return };
        let Role::Toplevel(window) = &cx.role else { return };
        let mut guard = cx.frame.borrow_mut();
        let Some(frame) = guard.as_mut() else { return };
        let (x, y) = event.position;
        let action = match event.kind {
            PointerEventKind::Enter { serial } => {
                cx.pointer_enter(pointer, serial);
                cursor(cx, frame.click_point_moved(Duration::ZERO, &event.surface.id(), x, y));
                None
            }
            PointerEventKind::Motion { time } => {
                let at = Duration::from_millis(u64::from(time));
                cursor(cx, frame.click_point_moved(at, &event.surface.id(), x, y));
                None
            }
            PointerEventKind::Leave { .. } => {
                frame.click_point_left();
                cx.pointer_leave();
                None
            }
            PointerEventKind::Press { button, serial, time }
            | PointerEventKind::Release { button, serial, time } => {
                let pressed = matches!(event.kind, PointerEventKind::Press { .. });
                let click = match button {
                    0x110 => FrameClick::Normal,
                    0x111 => FrameClick::Alternate,
                    _ => return,
                };
                frame
                    .on_click(Duration::from_millis(u64::from(time)), click, pressed)
                    .map(|action| (action, serial))
            }
            _ => None,
        };
        drop(guard);
        let Some((action, serial)) = action else { return };
        if matches!(action, FrameAction::Close) {
            return self.request_close_from_user();
        }
        let seat = pointer.data::<PointerData<()>>().map(|d| d.seat().clone());
        match action {
            FrameAction::Minimize => window.set_minimized(),
            FrameAction::Maximize => window.set_maximized(),
            FrameAction::UnMaximize => window.unset_maximized(),
            FrameAction::ShowMenu(x, y) => {
                if let Some(seat) = seat {
                    window.show_window_menu(&seat, serial, (x, y));
                }
            }
            FrameAction::Move => {
                if let Some(seat) = seat {
                    window.move_(&seat, serial);
                }
            }
            FrameAction::Resize(edge) => {
                let edge = match edge {
                    Edge::Top => ResizeEdge::Top,
                    Edge::Bottom => ResizeEdge::Bottom,
                    Edge::Left => ResizeEdge::Left,
                    Edge::TopLeft => ResizeEdge::TopLeft,
                    Edge::BottomLeft => ResizeEdge::BottomLeft,
                    Edge::Right => ResizeEdge::Right,
                    Edge::TopRight => ResizeEdge::TopRight,
                    Edge::BottomRight => ResizeEdge::BottomRight,
                    _ => ResizeEdge::None,
                };
                if let Some(seat) = seat {
                    window.resize(&seat, serial, edge);
                }
            }
            _ => {}
        }
    }

    /// The compositor or our frame's close button: the same rule as X11 `WM_DELETE_WINDOW`.
    fn request_close_from_user(&mut self) {
        if self.defer_close {
            self.event(Event::Window(WindowEvent::WillClose));
        } else {
            self.exit = true;
        }
    }
}

fn cursor(cx: &Context, icon: Option<CursorIcon>) {
    let shape = match icon {
        Some(CursorIcon::NResize) => Shape::NResize,
        Some(CursorIcon::NeResize) => Shape::NeResize,
        Some(CursorIcon::EResize) => Shape::EResize,
        Some(CursorIcon::SeResize) => Shape::SeResize,
        Some(CursorIcon::SResize) => Shape::SResize,
        Some(CursorIcon::SwResize) => Shape::SwResize,
        Some(CursorIcon::WResize) => Shape::WResize,
        Some(CursorIcon::NwResize) => Shape::NwResize,
        _ => Shape::Default,
    };
    cx.set_frame_cursor(shape);
}

impl WindowHandler for Runtime {
    fn request_close(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &Window) {
        self.request_close_from_user();
    }
    fn configure(
        &mut self, _: &Connection, qh: &QueueHandle<Self>, window: &Window,
        configure: WindowConfigure, _: u32,
    ) {
        let Some(cx) = &self.context else { return };
        let client = configure.decoration_mode == DecorationMode::Client;
        {
            let mut frame = cx.frame.borrow_mut();
            if client && frame.is_none() {
                if let Some(sub) = &self.subcompositor {
                    *frame =
                        FallbackFrame::new(window, self.data.shm(), Arc::clone(sub), qh.clone())
                            .ok();
                    if let Some(frame) = frame.as_mut() {
                        frame.set_resizable(!self.shared.fixed.load(Ordering::Acquire));
                    }
                }
            }
            if let Some(frame) = frame.as_mut() {
                frame.set_hidden(!client);
                frame.update_state(configure.state);
                frame.update_wm_capabilities(configure.capabilities);
                frame.set_scaling_factor(cx.scale_factor());
            }
        }
        // A size the compositor chose (tiled, maximized, user drag) is the outer
        // geometry; ours excludes the frame. `None` leaves the axis to us.
        let (w, h) = match (configure.new_size, cx.frame.borrow().as_ref()) {
            ((Some(w), Some(h)), Some(frame)) if !frame.is_hidden() => {
                let (w, h) = frame.subtract_borders(w, h);
                (w.map_or(0, |w| w.get()), h.map_or(0, |h| h.get()))
            }
            ((w, h), _) => (w.map_or(0, |w| w.get()), h.map_or(0, |h| h.get())),
        };
        cx.imposed.set(w != 0 && h != 0);
        cx.set_size((w, h));
        self.configured = true;
        self.resized();
    }
}
