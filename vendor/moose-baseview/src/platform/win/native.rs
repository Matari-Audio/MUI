//! HWND ownership for the embedded Windows platform. Keep the ABI boundary here,
//! rather than invoking the generic wrapper's unguarded extern procedure.
use super::{callback, window::BaseviewWindow, window_state::WindowSharedState};
use crate::dpi::PhysicalSize;
use crate::wrappers::win32::{
    h_instance::HInstance,
    window::{HWnd, WindowImpl},
    WindowStyle,
};
use std::{
    cell::{Cell, OnceCell},
    ptr::{self, NonNull},
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
};
use windows_core::{Error, Result, HSTRING};
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    UI::WindowsAndMessaging::*,
};

pub(super) struct RegisteredClass {
    atom: u16,
    instance: HInstance,
}

fn class_name(image: usize, serial: u64) -> String {
    format!("MUI-Baseview-{image:x}-{serial:x}")
}

impl RegisteredClass {
    fn new() -> Result<Self> {
        static SERIAL: AtomicU64 = AtomicU64::new(0);
        let instance = HInstance::get_from_dll();
        let name = HSTRING::from(class_name(
            instance.as_raw() as usize,
            SERIAL.fetch_add(1, Ordering::Relaxed),
        ));
        let info = WNDCLASSW {
            lpfnWndProc: Some(wnd_proc),
            hInstance: instance.as_raw(),
            lpszClassName: name.as_ptr(),
            style: CS_OWNDC,
            hCursor: unsafe { LoadCursorW(ptr::null_mut(), IDC_ARROW) },
            ..Default::default()
        };
        let atom = unsafe { RegisterClassW(&info) };
        if atom == 0 {
            return Err(Error::from_thread());
        }
        Ok(Self { atom, instance })
    }
    fn atom_ptr(&self) -> *const u16 {
        self.atom as usize as *const u16
    }
}

impl Drop for RegisteredClass {
    fn drop(&mut self) {
        if unsafe { UnregisterClassW(self.atom_ptr(), self.instance.as_raw()) } == 0 {
            // Creation pins the image before publishing any callback, including
            // the class procedure. A failed unregister cannot leave an unmapped proc.
            crate::warn!("UnregisterClassW failed: {}", Error::from_thread());
        }
    }
}

type Initializer = dyn FnOnce(HWnd) -> BaseviewWindow;
struct WindowData {
    initializer: Cell<Option<Box<Initializer>>>,
    inner: OnceCell<BaseviewWindow>,
    attached: Cell<bool>,
}

pub(super) fn create_window(
    title: &HSTRING, style: WindowStyle, size: PhysicalSize<u32>, parent: Option<HWnd>,
    shared: &WindowSharedState, initializer: impl FnOnce(HWnd) -> BaseviewWindow + 'static,
) -> Result<HWnd> {
    // COM clients can retain IDropTarget after revoke, and DwmFlush can wedge.
    // Pin before publishing pointers/spawning work, not after an unload race begins.
    if !crate::pin_current_image_for_detached_work() {
        return Err(Error::new(
            windows_core::HRESULT(0x80004005u32 as i32),
            "Cannot pin window callback image",
        ));
    }
    let class = RegisteredClass::new()?;
    let data = Rc::new(WindowData {
        initializer: Cell::new(Some(Box::new(initializer))),
        inner: OnceCell::new(),
        attached: false.into(),
    });
    // CreateWindowEx is synchronous. WM_NCCREATE acquires the window's own Rc;
    // the caller retains this one even on an early failure or reentrant destroy.
    let hwnd = unsafe {
        CreateWindowExW(
            style.style_ex,
            class.atom_ptr(),
            title.as_ptr(),
            style.style,
            0,
            0,
            size.width.try_into().unwrap_or(i32::MAX),
            size.height.try_into().unwrap_or(i32::MAX),
            parent.map(|p| p.as_raw()).unwrap_or(ptr::null_mut()),
            ptr::null_mut(),
            class.instance.as_raw(),
            Rc::as_ptr(&data).cast(),
        )
    };
    let Some(hwnd) = NonNull::new(hwnd) else {
        return Err(Error::from_thread());
    };
    // Retain the class until WindowHandle is dropped, AFTER DestroyWindow returns.
    // Unregistering inside WM_DESTROY/WM_NCDESTROY is too early.
    shared.native_class.set(Some(class));
    Ok(unsafe { HWnd::from_raw(hwnd) })
}

pub(super) unsafe fn with_window<T>(
    window: HWnd, f: impl FnOnce(&BaseviewWindow) -> T,
) -> Option<T> {
    let ptr = window.get_userdata_ptr::<WindowData>()?;
    // SAFETY: window owns one Rc until WM_NCDESTROY clears GWLP_USERDATA.
    // Retain independently so nested DestroyWindow cannot free this callback's data.
    unsafe {
        Rc::increment_strong_count(ptr.as_ptr());
    }
    let data = unsafe { Rc::from_raw(ptr.as_ptr()) };
    data.inner.get().map(f)
}

pub(super) unsafe extern "system" fn wnd_proc(
    hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM,
) -> LRESULT {
    callback::guard(
        "wnd_proc",
        || match msg {
            WM_NCCREATE => 0,
            WM_CREATE => -1,
            _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
        },
        || unsafe { wnd_proc_inner(hwnd, msg, wp, lp) },
    )
}

unsafe fn wnd_proc_inner(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let default = || unsafe { DefWindowProcW(hwnd, msg, wp, lp) };
    let Some(window) = NonNull::new(hwnd) else {
        return default();
    };
    let window = unsafe { HWnd::from_raw(window) };
    if msg == WM_NCCREATE {
        let Some(create) = (unsafe { (lp as *const CREATESTRUCTW).as_ref() }) else {
            return 0;
        };
        let Some(ptr) = NonNull::new(create.lpCreateParams as *mut WindowData) else {
            return 0;
        };
        let data = unsafe { ptr.as_ref() };
        if data.attached.replace(true) {
            return 0;
        }
        unsafe {
            Rc::increment_strong_count(ptr.as_ptr());
        }
        if window.set_userdata_ptr(ptr.as_ptr()).is_err() {
            data.attached.set(false);
            drop(unsafe { Rc::from_raw(ptr.as_ptr()) });
            return 0;
        }
        let Some(init) = data.initializer.take() else {
            return 0;
        };
        if data.inner.set(init(window)).is_err() {
            return 0;
        }
        return unsafe {
            with_window(window, |inner| inner.non_client_create(window).is_ok() as LRESULT)
        }
        .unwrap_or(0);
    }
    if msg == WM_NCDESTROY {
        // Revoke userdata BEFORE cleanup (OleUninitialize/handlers can reenter).
        let Some(ptr) = window.get_userdata_ptr::<WindowData>() else {
            return default();
        };
        let data = unsafe { Rc::from_raw(ptr.as_ptr()) };
        let _ = window.set_userdata_ptr(ptr::null::<WindowData>());
        data.attached.set(false);
        if let Some(inner) = data.inner.get() {
            callback::guard("native teardown", || (), || inner.before_destroy(window));
        }
        default();
        return 0;
    }
    unsafe {
        with_window(window, |inner| match msg {
            WM_CREATE => match inner.after_create(window) {
                Ok(()) => 0,
                Err(e) => {
                    crate::error!("Window initialization failed: {}", e);
                    -1
                }
            },
            WM_DESTROY => {
                inner.before_destroy(window);
                0
            }
            _ => inner.handle_message(window, msg, wp, lp).unwrap_or_else(default),
        })
    }
    .unwrap_or_else(default)
}

/// Resize without raising the editor or changing the DAW's keyboard focus.
pub(super) fn resize(
    window: HWnd, size: PhysicalSize<u32>, dpi: Option<crate::wrappers::win32::Dpi>,
    ctx: &crate::wrappers::win32::DpiAwarenessGuard,
) -> Result<()> {
    let size = ctx.client_area_to_nc_area(size.into(), window.get_style()?, dpi)?.size();
    if unsafe {
        SetWindowPos(
            window.as_raw(),
            ptr::null_mut(),
            0,
            0,
            size.width.try_into().unwrap_or(i32::MAX),
            size.height.try_into().unwrap_or(i32::MAX),
            SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOZORDER,
        )
    } == 0
    {
        return Err(Error::from_thread());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn class_names_separate_images_and_windows() {
        assert_ne!(super::class_name(0x1000, 0), super::class_name(0x2000, 0));
        assert_ne!(super::class_name(0x1000, 0), super::class_name(0x1000, 1));
    }
}
