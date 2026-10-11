use crate::dpi::{PhysicalSize, Size};
use crate::platform::win::dpi::DpiScalingStrategy;
use crate::platform::win::keyboard::KeyboardState;
use crate::platform::PlatformHandle;
use crate::utils::SizingStrategy;
use crate::window::WindowInitializer;
use crate::wrappers::win32::cursor::SystemCursor;
use crate::wrappers::win32::h_instance::HInstance;
use crate::wrappers::win32::window::HWnd;
use crate::wrappers::win32::{Dpi, DpiAwarenessGuard, ExtendedUser32, LibraryModule};
use crate::WindowSettings;
use crate::{MouseCursor, WindowSize};
use raw_window_handle::{DisplayHandle, Win32WindowHandle};
use std::cell::{Cell, Ref, RefCell};
use std::num::NonZeroIsize;
use std::rc::Rc;
use windows_sys::Win32::UI::WindowsAndMessaging::PostMessageW;

/// All data associated with the window.
pub(crate) struct WindowState {
    /// The HWND belonging to this window.
    pub hwnd: HWnd,
    pub keyboard_state: RefCell<KeyboardState>,
    pub(crate) ime: super::ime::NativeIme,
    pub mouse_button_counter: Cell<usize>,
    pub mouse_was_outside_window: Cell<bool>,
    pub cursor_icon: Cell<MouseCursor>,

    pub user32: LibraryModule<ExtendedUser32>,
    pub shared: Rc<WindowSharedState>,

    #[cfg(feature = "opengl")]
    pub gl_context: std::cell::OnceCell<super::gl::GlContext>,
}

impl WindowState {
    pub fn new(
        hwnd: HWnd, user32: LibraryModule<ExtendedUser32>, shared: Rc<WindowSharedState>,
    ) -> Self {
        Self {
            hwnd,
            keyboard_state: RefCell::new(KeyboardState::new()),
            ime: Default::default(),
            mouse_button_counter: Cell::new(0),
            mouse_was_outside_window: true.into(),
            cursor_icon: Cell::new(MouseCursor::Default),
            user32,
            shared,

            #[cfg(feature = "opengl")]
            gl_context: std::cell::OnceCell::new(),
        }
    }

    /// Returns the current size of this window.
    pub fn size(&self) -> WindowSize {
        self.shared.size()
    }

    /// Returns the current scale factor of this window.
    pub fn scale_factor(&self) -> f64 {
        self.shared.scale_factor()
    }

    pub(crate) fn keyboard_state(&self) -> Ref<'_, KeyboardState> {
        self.keyboard_state.borrow()
    }

    pub fn request_close(&self) {
        if !self.shared.is_alive.get() {
            return;
        }
        unsafe {
            PostMessageW(
                self.hwnd.as_raw(),
                crate::platform::win::window::BV_WINDOW_MUST_CLOSE,
                0,
                0,
            );
        }
    }

    pub fn has_focus(&self) -> bool {
        self.shared.is_alive.get() && HWnd::get_focused_window() == self.hwnd.as_raw()
    }

    pub fn focus(&self) -> Result<(), super::PlatformError> {
        if !self.shared.is_alive.get() {
            return Ok(());
        }
        self.hwnd.set_focus()?;
        Ok(())
    }

    pub fn resize(&self, size: Size) -> Result<(), super::PlatformError> {
        if !self.shared.is_alive.get() {
            return Ok(());
        }
        // `self.window_info` will be modified in response to the `WM_SIZE` event that
        // follows the `SetWindowPos()` call
        let dpi = self.shared.current_dpi.get();
        let new_size = size.to_physical(self.shared.scale_factor());

        let ctx = DpiAwarenessGuard::new(&self.user32, self.shared.dpi_scaling_strategy.get())?;

        super::native::resize(self.hwnd, new_size, dpi, &ctx)?;
        Ok(())
    }

    pub fn set_scale_factor_override(
        &self, scale_factor: Option<f64>,
    ) -> Result<(), super::PlatformError> {
        self.shared.scale_factor_override.set(scale_factor);
        Ok(())
    }

    pub fn set_ime_configuration(&self, configuration: Option<crate::ImeConfiguration>) {
        if self.shared.is_alive.get() {
            self.ime.configure(self.hwnd.as_raw(), configuration);
        }
    }

    pub fn set_keyboard_capture(&self, capture: bool) {
        if self.shared.is_alive.get() {
            set_keyboard_capture(self.hwnd, capture);
        }
    }

    pub fn set_mouse_cursor(&self, mouse_cursor: MouseCursor) -> Result<(), super::PlatformError> {
        self.cursor_icon.set(mouse_cursor);
        if let Ok(cursor) = SystemCursor::load(mouse_cursor) {
            cursor.set()
        }

        Ok(())
    }

    #[cfg(feature = "opengl")]
    pub fn gl_context(&self) -> Option<crate::gl::GlContext> {
        Some(crate::gl::GlContext::new(Rc::clone(self.gl_context.get()?)))
    }

    pub fn window_handle(&self) -> Option<raw_window_handle::WindowHandle<'_>> {
        if !self.shared.is_alive.get() {
            return None;
        }
        let hwnd = NonZeroIsize::new(self.hwnd.as_raw() as _)?;
        let mut handle = Win32WindowHandle::new(hwnd);
        handle.hinstance = Some(HInstance::get_from_dll().addr());

        Some(unsafe { raw_window_handle::WindowHandle::borrow_raw(handle.into()) })
    }

    pub fn display_handle(&self) -> DisplayHandle<'_> {
        DisplayHandle::windows()
    }

    pub fn platform_handle(&self) -> PlatformHandle {
        // SAFETY: HWnd is constructed from NonNull, so this integer is nonzero.
        let hwnd = unsafe { NonZeroIsize::new_unchecked(self.hwnd.as_raw() as _) };
        PlatformHandle { hwnd }
    }
}

pub struct WindowSharedState {
    pub(super) native_class: Cell<Option<super::native::RegisteredClass>>,
    pub(super) frame_signal: std::sync::Arc<super::frame::FrameSignal>,
    pub parented: Cell<bool>,
    pub is_alive: Cell<bool>,
    pub current_size: Cell<PhysicalSize<u32>>,
    pub current_dpi: Cell<Option<Dpi>>, // None if Win32 HiDPI isn't supported
    pub fallback_scale_factor: Cell<Option<f64>>,
    pub scale_factor_override: Cell<Option<f64>>,
    pub resize_host_originated: Cell<bool>,
    pub destroy_host_originated: Cell<bool>,
    pub dpi_scaling_strategy: Cell<DpiScalingStrategy>,

    pub user32: LibraryModule<ExtendedUser32>,
    pub sizing_strategy: SizingStrategy,
}

impl WindowSharedState {
    pub fn new(user32: LibraryModule<ExtendedUser32>, settings: &WindowSettings) -> Rc<Self> {
        Self {
            native_class: None.into(),
            frame_signal: super::frame::FrameSignal::new(),
            parented: (settings.parent.is_some() || settings.wait_for_parent).into(),
            is_alive: true.into(),
            current_dpi: None.into(),
            // With an override the final physical size is already known, so the window is
            // created at that size (no resize flash in `after_create`).
            current_size: settings
                .size
                .to_physical(settings.scale_factor_override.unwrap_or(1.0))
                .into(),
            fallback_scale_factor: settings.fallback_scale_factor.into(),
            scale_factor_override: settings.scale_factor_override.into(),
            resize_host_originated: false.into(),
            destroy_host_originated: false.into(),
            sizing_strategy: SizingStrategy::from_settings(settings),
            user32,
            dpi_scaling_strategy: DpiScalingStrategy::default().into(),
        }
        .into()
    }

    pub fn init(&self, init: &WindowInitializer) {
        let parent = init.settings.parent.as_ref().map(|p| p.inner.handle);
        let strategy = DpiScalingStrategy::get(
            Some(&self.user32),
            parent,
            #[cfg(feature = "opengl")]
            &init.settings,
        );

        let dpi = if strategy.assume_96_dpi {
            Some(Dpi::default())
        } else {
            parent.and_then(|p| p.get_dpi(&self.user32))
        };
        self.current_dpi.set(dpi);
        self.parented.set(init.settings.parent.is_some() || init.settings.wait_for_parent);
        self.dpi_scaling_strategy.set(strategy);
        self.current_size.set(init.settings.size.to_physical(self.scale_factor()));
    }

    pub fn size(&self) -> WindowSize {
        WindowSize::from_physical(self.current_size.get(), self.scale_factor())
    }

    pub fn scale_factor(&self) -> f64 {
        if let Some(scale_factor) = self.scale_factor_override.get() {
            return scale_factor;
        }

        self.platform_scale_factor()
    }

    /// The scale factor from the OS DPI (or the fallback), ignoring the override.
    pub fn platform_scale_factor(&self) -> f64 {
        let strategy = self.dpi_scaling_strategy.get();
        if strategy.assume_96_dpi {
            return 1.0;
        }
        if strategy.should_use_host_suggested_scale_factor {
            return self.fallback_scale_factor.get().unwrap_or(1.0);
        }
        self.current_dpi
            .get()
            .map(|d| d.scale_factor())
            .unwrap_or_else(|| self.fallback_scale_factor.get().unwrap_or(1.0))
    }

    pub fn originate_host_resize(&self) -> impl Drop + use<'_> {
        self.resize_host_originated.set(true);
        Guard(&self.resize_host_originated)
    }

    // A host close can wait for an active callback; its origin must survive
    // that callback rather than being cleared by a synchronous scope guard.
    pub fn mark_host_destroy(&self) {
        self.destroy_host_originated.set(true);
    }
}

struct Guard<'a>(&'a Cell<bool>);
impl<'a> Drop for Guard<'a> {
    fn drop(&mut self) {
        self.0.set(false);
    }
}

/// Updates the keyboard hook right away and moves focus asynchronously (a synchronous
/// `SetFocus` would re-enter the window handler from inside its own callback).
pub(crate) fn set_keyboard_capture(hwnd: HWnd, capture: bool) {
    if !super::hook::set_keyboard_capture(hwnd.as_raw(), capture) {
        return;
    }
    unsafe {
        PostMessageW(
            hwnd.as_raw(),
            crate::platform::win::window::BV_KEYBOARD_CAPTURE_FOCUS,
            usize::from(capture),
            0,
        );
    }
}
