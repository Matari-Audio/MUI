//! A detached layer surface and an embedded X11 window share the same toolkit API.
use super::{layer, x11};
use crate::{dpi::Size, window::WindowInitializer, *};
use raw_window_handle::{DisplayHandle, WindowHandle as RawWindowHandle};
use std::rc::Rc;

pub use x11::{ParentWindowHandle, PlatformError};
pub(crate) type Result<T> = std::result::Result<T, PlatformError>;
pub(crate) use x11::X11Connection;
pub use x11::{assume_standalone_in_process, copy_to_clipboard};

pub enum WindowHandle {
    X11(x11::WindowHandle),
    Layer(layer::WindowHandle),
}

macro_rules! forward {
    ($self:ident, $method:ident $(, $arg:expr)*) => {
        match $self { Self::X11(inner) => inner.$method($($arg),*), Self::Layer(inner) => inner.$method($($arg),*) }
    };
}

impl WindowHandle {
    pub fn create_window(init: WindowInitializer) -> Result<Self> {
        if init.settings.dock.is_some() && std::env::var_os("WAYLAND_DISPLAY").is_some() {
            layer::WindowHandle::create_window(init).map(Self::Layer)
        } else {
            x11::WindowHandle::create_window(init).map(Self::X11)
        }
    }
    pub fn size(&self) -> WindowSize {
        forward!(self, size)
    }
    pub fn resize(&self, size: Size) -> Result<()> {
        forward!(self, resize, size)
    }
    pub fn suggest_scale_factor(&self, scale: f64) -> Result<()> {
        forward!(self, suggest_scale_factor, scale)
    }
    pub fn set_scale_factor_override(&self, scale: Option<f64>) -> Result<()> {
        forward!(self, set_scale_factor_override, scale)
    }
    pub fn set_keyboard_capture(&self, capture: bool) {
        forward!(self, set_keyboard_capture, capture)
    }
    pub fn set_close_timeout(&self, timeout: std::time::Duration) {
        forward!(self, set_close_timeout, timeout)
    }
    pub fn visibility_control(&self) -> WindowVisibility {
        forward!(self, visibility_control)
    }
    pub fn frame_requester(&self) -> FrameRequester {
        forward!(self, frame_requester)
    }
    pub fn run_until_closed(&self) -> Result<()> {
        forward!(self, run_until_closed)
    }
    pub fn show(&self) -> Result<()> {
        forward!(self, show)
    }
    pub fn hide(&self) -> Result<()> {
        forward!(self, hide)
    }
    pub fn is_open(&self) -> bool {
        forward!(self, is_open)
    }
    pub fn is_resizable(&self) -> bool {
        forward!(self, is_resizable)
    }
    pub fn min_size(&self) -> Option<Size> {
        forward!(self, min_size)
    }
    pub fn max_size(&self) -> Option<Size> {
        forward!(self, max_size)
    }
    pub fn handle_main_thread_callback(&self) {
        forward!(self, handle_main_thread_callback)
    }
    pub fn set_parent(&self, parent: ParentWindowHandle) -> Result<()> {
        forward!(self, set_parent, parent)
    }
}

#[derive(Clone)]
pub enum WindowContext {
    X11(x11::WindowContext),
    Layer(Rc<layer::Context>),
}
impl From<x11::WindowContext> for WindowContext {
    fn from(value: x11::WindowContext) -> Self {
        Self::X11(value)
    }
}
impl WindowContext {
    pub fn set_mouse_cursor(&self, cursor: MouseCursor) -> Result<()> {
        forward!(self, set_mouse_cursor, cursor)
    }
    pub fn request_close(&self) {
        forward!(self, request_close)
    }
    pub fn has_focus(&self) -> bool {
        forward!(self, has_focus)
    }
    pub fn focus(&self) -> Result<()> {
        forward!(self, focus)
    }
    pub fn resize(&self, size: Size) -> Result<()> {
        forward!(self, resize, size)
    }
    pub fn set_scale_factor_override(&self, scale: Option<f64>) -> Result<()> {
        forward!(self, set_scale_factor_override, scale)
    }
    pub fn set_keyboard_capture(&self, capture: bool) {
        forward!(self, set_keyboard_capture, capture)
    }
    pub fn set_ime_configuration(&self, config: Option<ImeConfiguration>) {
        forward!(self, set_ime_configuration, config)
    }
    pub fn size(&self) -> WindowSize {
        forward!(self, size)
    }
    pub fn scale_factor(&self) -> f64 {
        forward!(self, scale_factor)
    }
    pub fn drag_resize(&self, edge: ResizeEdge) -> std::result::Result<(), DragResizeError> {
        forward!(self, drag_resize, edge)
    }
    pub fn window_handle(&self) -> Option<RawWindowHandle<'_>> {
        forward!(self, window_handle)
    }
    pub fn display_handle(&self) -> DisplayHandle<'_> {
        forward!(self, display_handle)
    }
    pub fn platform_handle(&self) -> PlatformHandle {
        match self {
            Self::X11(c) => PlatformHandle::X11(c.platform_handle()),
            Self::Layer(c) => PlatformHandle::Layer(c.platform_handle()),
        }
    }
    pub fn can_schedule_matari_x11_drag(&self) -> bool {
        match self {
            Self::X11(c) => c.can_schedule_matari_x11_drag(),
            Self::Layer(c) => c.can_schedule_drag(),
        }
    }
    pub fn schedule_matari_drag(
        &self, ticket: matari_audio_drag_and_drop::StartTicket,
    ) -> std::result::Result<(), matari_audio_drag_and_drop::RejectedStart<MatariScheduleError>>
    {
        match self {
            Self::X11(c) => {
                let window = crate::WindowContext::new(self.clone());
                let Ok(origin) = matari_audio_drag_and_drop::DragOrigin::from_window(&window)
                else {
                    return Err(ticket.reject(MatariScheduleError::MissingWindowHandle));
                };
                c.schedule_matari_x11_drag(ticket, origin)
            }
            Self::Layer(c) => c.schedule_drag(ticket),
        }
    }
    #[cfg(feature = "opengl")]
    pub fn gl_context(&self) -> Option<crate::gl::GlContext> {
        match self {
            Self::X11(c) => c.gl_context(),
            Self::Layer(_) => None,
        }
    }
}

#[derive(Clone, Debug)]
pub enum PlatformHandle {
    X11(x11::PlatformHandle),
    Layer(layer::PlatformHandle),
}
impl PlatformHandle {
    pub fn window_handle(&self) -> Option<RawWindowHandle<'_>> {
        forward!(self, window_handle)
    }
    pub fn display_handle(&self) -> DisplayHandle<'_> {
        forward!(self, display_handle)
    }
}
