use super::*;
use raw_window_handle::{
    DisplayHandle, RawDisplayHandle, RawWindowHandle, WaylandDisplayHandle, WaylandWindowHandle,
    WindowHandle,
};
use smithay_client_toolkit::dispatch2::Dispatch2;
use smithay_client_toolkit::shell::wlr_layer::{Anchor, KeyboardInteractivity};
use std::ptr::NonNull;
use wayland_client::{globals::GlobalList, protocol::wl_data_device::WlDataDevice};
use wayland_protocols::wp::cursor_shape::v1::client::{
    wp_cursor_shape_device_v1::{Shape, WpCursorShapeDeviceV1},
    wp_cursor_shape_manager_v1::WpCursorShapeManagerV1,
};
use wayland_protocols::wp::{
    fractional_scale::v1::client::{
        wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1,
        wp_fractional_scale_v1::{self, WpFractionalScaleV1},
    },
    viewporter::client::{wp_viewport::WpViewport, wp_viewporter::WpViewporter},
};

#[derive(Clone, Debug)]
pub struct PlatformHandle {
    layer: LayerSurface,
    connection: Connection,
    live: Arc<AtomicBool>,
}
impl PlatformHandle {
    pub fn window_handle(&self) -> Option<WindowHandle<'_>> {
        if !self.live.load(Ordering::Acquire) {
            return None;
        }
        let pointer = NonNull::new(self.layer.wl_surface().id().as_ptr().cast())?;
        // SAFETY: the retained LayerSurface owns this wl_surface until all
        // presentation handles have dropped; close revokes future borrows.
        Some(unsafe {
            WindowHandle::borrow_raw(RawWindowHandle::Wayland(WaylandWindowHandle::new(pointer)))
        })
    }
    pub fn display_handle(&self) -> DisplayHandle<'_> {
        let pointer = NonNull::new(self.connection.backend().display_ptr())
            .expect("a connected Wayland display is non-null")
            .cast();
        // SAFETY: this handle retains the connection and its system display.
        unsafe {
            DisplayHandle::borrow_raw(RawDisplayHandle::Wayland(WaylandDisplayHandle::new(pointer)))
        }
    }
}

pub struct Context {
    pub(super) layer: LayerSurface,
    pub(super) conn: Connection,
    pub(super) qh: QueueHandle<Runtime>,
    options: RefCell<DockOptions>,
    shared: Arc<Shared>,
    live: Arc<AtomicBool>,
    signal: LoopSignal,
    focused: Cell<bool>,
    logical: Cell<(u32, u32)>,
    fractional: Option<WpFractionalScaleV1>,
    viewport: Option<WpViewport>,
    pub(super) text: RefCell<super::text_input::TextInput>,
    cursor_manager: Option<WpCursorShapeManagerV1>,
    cursor_device: RefCell<Option<WpCursorShapeDeviceV1>>,
    cursor: Cell<MouseCursor>,
    cursor_serial: Cell<Option<u32>>,
    pub(super) primary: Cell<bool>,
    pub(super) press: RefCell<Option<(WlDataDevice, u32)>>,
    pub(super) pending_drag: RefCell<Option<matari_audio_drag_and_drop::StartTicket>>,
    pub(super) drag_active: Cell<bool>,
}
impl Context {
    pub(super) fn new(
        conn: Connection, layer: LayerSurface, options: DockOptions, qh: QueueHandle<Runtime>,
        shared: Arc<Shared>, signal: LoopSignal, globals: &GlobalList,
    ) -> linux::Result<Self> {
        let viewporter: Option<WpViewporter> = globals.bind(&qh, 1..=1, Noop).ok();
        let viewport = viewporter.as_ref().map(|v| v.get_viewport(layer.wl_surface(), &qh, Noop));
        let manager: Option<WpFractionalScaleManagerV1> = globals.bind(&qh, 1..=1, Noop).ok();
        let fractional = if viewport.is_some() {
            manager.as_ref().map(|m| m.get_fractional_scale(layer.wl_surface(), &qh, FractionData))
        } else {
            None
        };
        if let Some(m) = manager {
            m.destroy();
        }
        if let Some(v) = viewporter {
            v.destroy();
        }
        let cursor_manager = globals.bind(&qh, 1..=1, Noop).ok();
        let logical = (800, options.height);
        Ok(Self {
            conn,
            layer,
            options: RefCell::new(options),
            qh: qh.clone(),
            shared,
            signal,
            focused: Cell::new(false),
            live: Arc::new(AtomicBool::new(true)),
            logical: Cell::new(logical),
            fractional,
            viewport,
            text: RefCell::new(super::text_input::TextInput::bind(globals, &qh)),
            cursor_manager,
            cursor_device: RefCell::new(None),
            cursor: Cell::new(MouseCursor::Default),
            cursor_serial: Cell::new(None),
            primary: Cell::new(false),
            press: RefCell::new(None),
            pending_drag: RefCell::new(None),
            drag_active: Cell::new(false),
        })
    }
    pub(super) fn surface(&self) -> &wl_surface::WlSurface {
        self.layer.wl_surface()
    }
    pub(super) fn configure(&self) {
        let options = self.options.borrow();
        let edge = match options.edge {
            DockEdge::Top => Anchor::TOP,
            DockEdge::Bottom => Anchor::BOTTOM,
        };
        self.layer.set_anchor(edge | Anchor::LEFT | Anchor::RIGHT);
        self.layer.set_size(0, options.height);
        self.layer.set_exclusive_zone(options.height as i32);
        self.layer.set_keyboard_interactivity(KeyboardInteractivity::OnDemand);
        self.surface().commit();
    }
    pub(super) fn set_size(&self, size: (u32, u32)) {
        let old = self.logical.get();
        self.logical.set((
            if size.0 == 0 { old.0 } else { size.0 },
            if size.1 == 0 { self.options.borrow().height } else { size.1 },
        ));
        let (w, h) = self.logical.get();
        let scale = self.scale_factor();
        let physical = (
            (f64::from(w) * scale).round().max(1.0) as u32,
            (f64::from(h) * scale).round().max(1.0) as u32,
        );
        self.shared
            .size
            .store((u64::from(physical.0) << 32) | u64::from(physical.1), Ordering::Release);
        if let Some(viewport) = &self.viewport {
            viewport.set_destination(w as i32, h as i32);
        }
    }
    pub(super) fn set_integer_scale(&self, factor: i32) {
        if self.fractional.is_none() {
            let factor = factor.max(1);
            self.shared.scale.store(f64::from(factor).to_bits(), Ordering::Release);
            self.surface().set_buffer_scale(factor);
            self.set_size(self.logical.get());
        }
    }
    pub fn platform_handle(&self) -> PlatformHandle {
        PlatformHandle {
            layer: self.layer.clone(),
            connection: self.conn.clone(),
            live: Arc::clone(&self.live),
        }
    }
    pub fn window_handle(&self) -> Option<WindowHandle<'_>> {
        if !self.live.load(Ordering::Acquire) {
            return None;
        }
        let pointer = NonNull::new(self.surface().id().as_ptr().cast())?;
        // SAFETY: this owner keeps the layer and system connection live.
        Some(unsafe { WindowHandle::borrow_raw(WaylandWindowHandle::new(pointer).into()) })
    }
    pub fn display_handle(&self) -> DisplayHandle<'_> {
        let pointer = NonNull::new(self.conn.backend().display_ptr())
            .expect("a connected Wayland display is non-null")
            .cast();
        // SAFETY: the connection is retained by this owner.
        unsafe { DisplayHandle::borrow_raw(WaylandDisplayHandle::new(pointer).into()) }
    }
    pub fn size(&self) -> WindowSize {
        self.shared.size()
    }
    pub fn scale_factor(&self) -> f64 {
        f64::from_bits(self.shared.scale.load(Ordering::Acquire))
    }
    pub fn resize(&self, size: Size) -> linux::Result<()> {
        let size: LogicalSize<f64> = size.to_logical(self.scale_factor());
        self.options.borrow_mut().height = (size.height.round() as u32).clamp(100, 1024);
        self.configure();
        Ok(())
    }
    pub fn request_close(&self) {
        self.shared.open.store(false, Ordering::Release);
        self.signal.wakeup();
    }
    pub fn has_focus(&self) -> bool {
        self.focused.get()
    }
    pub(super) fn focused(&self, focused: bool) {
        self.focused.set(focused);
    }
    pub fn focus(&self) -> linux::Result<()> {
        Ok(())
    }
    pub fn set_scale_factor_override(&self, _: Option<f64>) -> linux::Result<()> {
        Ok(())
    }
    pub fn set_keyboard_capture(&self, _: bool) {}
    pub fn set_ime_configuration(&self, config: Option<ImeConfiguration>) {
        self.text.borrow_mut().configure(config, self.scale_factor());
    }
    pub(super) fn pointer_enter(
        &self, pointer: &wayland_client::protocol::wl_pointer::WlPointer, serial: u32,
    ) {
        self.cursor_serial.set(Some(serial));
        if let Some(manager) = &self.cursor_manager {
            if let Some(old) = self.cursor_device.borrow_mut().take() {
                old.destroy();
            }
            *self.cursor_device.borrow_mut() = Some(manager.get_pointer(pointer, &self.qh, Noop));
        }
        if self.cursor.get() == MouseCursor::Hidden {
            pointer.set_cursor(serial, None, 0, 0);
        } else {
            let _ = self.set_mouse_cursor(self.cursor.get());
        }
    }
    pub(super) fn pointer_leave(&self) {
        self.cursor_serial.set(None);
    }
    pub fn set_mouse_cursor(&self, cursor: MouseCursor) -> linux::Result<()> {
        self.cursor.set(cursor);
        let Some(serial) = self.cursor_serial.get() else {
            return Ok(());
        };
        let device = self.cursor_device.borrow();
        let Some(device) = device.as_ref() else {
            return Ok(());
        };
        let shape = match cursor {
            MouseCursor::Default | MouseCursor::Hidden => Shape::Default,
            MouseCursor::Hand => Shape::Pointer,
            MouseCursor::HandGrabbing => Shape::Grabbing,
            MouseCursor::Help => Shape::Help,
            MouseCursor::Text => Shape::Text,
            MouseCursor::VerticalText => Shape::VerticalText,
            MouseCursor::Working => Shape::Wait,
            MouseCursor::PtrWorking => Shape::Progress,
            MouseCursor::NotAllowed | MouseCursor::PtrNotAllowed => Shape::NotAllowed,
            MouseCursor::ZoomIn => Shape::ZoomIn,
            MouseCursor::ZoomOut => Shape::ZoomOut,
            MouseCursor::Alias => Shape::Alias,
            MouseCursor::Copy => Shape::Copy,
            MouseCursor::Move => Shape::Move,
            MouseCursor::AllScroll => Shape::AllScroll,
            MouseCursor::Cell => Shape::Cell,
            MouseCursor::Crosshair => Shape::Crosshair,
            MouseCursor::EResize => Shape::EResize,
            MouseCursor::NResize => Shape::NResize,
            MouseCursor::NeResize => Shape::NeResize,
            MouseCursor::NwResize => Shape::NwResize,
            MouseCursor::SResize => Shape::SResize,
            MouseCursor::SeResize => Shape::SeResize,
            MouseCursor::SwResize => Shape::SwResize,
            MouseCursor::WResize => Shape::WResize,
            MouseCursor::EwResize => Shape::EwResize,
            MouseCursor::NsResize => Shape::NsResize,
            MouseCursor::NwseResize => Shape::NwseResize,
            MouseCursor::NeswResize => Shape::NeswResize,
            MouseCursor::ColResize => Shape::ColResize,
            MouseCursor::RowResize => Shape::RowResize,
        };
        device.set_shape(serial, shape);
        Ok(())
    }
    pub fn drag_resize(&self, _: ResizeEdge) -> std::result::Result<(), DragResizeError> {
        Err(DragResizeError::NotSupported)
    }
    pub fn can_schedule_drag(&self) -> bool {
        self.primary.get()
            && self.press.borrow().is_some()
            && self.pending_drag.borrow().is_none()
            && !self.drag_active.get()
    }
    pub fn schedule_drag(
        &self, ticket: matari_audio_drag_and_drop::StartTicket,
    ) -> std::result::Result<(), matari_audio_drag_and_drop::RejectedStart<MatariScheduleError>>
    {
        if !self.can_schedule_drag() {
            return Err(ticket.reject(MatariScheduleError::AuthorityUnavailable));
        }
        *self.pending_drag.borrow_mut() = Some(ticket);
        self.signal.wakeup();
        Ok(())
    }
    pub(super) fn close(&self) {
        self.live.store(false, Ordering::Release);
        self.layer.set_exclusive_zone(0);
        self.surface().attach(None, 0, 0);
        self.surface().commit();
        let _ = self.conn.flush();
        self.pending_drag.borrow_mut().take();
    }
}
impl Drop for Context {
    fn drop(&mut self) {
        if let Some(d) = self.cursor_device.get_mut().take() {
            d.destroy();
        }
        if let Some(m) = self.cursor_manager.take() {
            m.destroy();
        }
        if let Some(f) = self.fractional.take() {
            f.destroy();
        }
        if let Some(v) = self.viewport.take() {
            v.destroy();
        }
    }
}
#[derive(Debug)]
struct Noop;
impl<I: Proxy> Dispatch2<I, Runtime> for Noop {
    fn event(&self, _: &mut Runtime, _: &I, _: I::Event, _: &Connection, _: &QueueHandle<Runtime>) {
    }
}
impl Dispatch2<WpFractionalScaleV1, Runtime> for FractionData {
    fn event(
        &self, state: &mut Runtime, _: &WpFractionalScaleV1, event: wp_fractional_scale_v1::Event,
        _: &Connection, _: &QueueHandle<Runtime>,
    ) {
        if let wp_fractional_scale_v1::Event::PreferredScale { scale } = event {
            if let Some(cx) = &state.context {
                cx.shared
                    .scale
                    .store((f64::from(scale.max(1)) / 120.0).to_bits(), Ordering::Release);
                cx.surface().set_buffer_scale(1);
                cx.set_size(cx.logical.get());
            }
            state.resized();
        }
    }
}

#[derive(Debug)]
struct FractionData;
