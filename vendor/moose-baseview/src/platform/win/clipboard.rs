//! Eager Unicode clipboard data; no delayed-rendering callback can outlive us.
use std::ffi::c_void;
use windows_sys::Win32::{
    Foundation::HWND,
    UI::{
        Input::KeyboardAndMouse::GetActiveWindow,
        WindowsAndMessaging::{GetAncestor, GA_ROOT},
    },
};

// OS imports (not callbacks). These feature-gated windows-sys APIs are declared
// locally to avoid changing the shared manifest in this platform work package.
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GlobalAlloc(flags: u32, bytes: usize) -> *mut c_void;
    fn GlobalLock(memory: *mut c_void) -> *mut c_void;
    fn GlobalUnlock(memory: *mut c_void) -> i32;
    fn GlobalFree(memory: *mut c_void) -> *mut c_void;
}
#[link(name = "user32")]
unsafe extern "system" {
    fn OpenClipboard(owner: HWND) -> i32;
    fn CloseClipboard() -> i32;
    fn EmptyClipboard() -> i32;
    fn SetClipboardData(format: u32, memory: *mut c_void) -> *mut c_void;
}
struct Memory(*mut c_void);
impl Drop for Memory {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                GlobalFree(self.0);
            }
        }
    }
}
struct Clipboard;
impl Drop for Clipboard {
    fn drop(&mut self) {
        unsafe {
            CloseClipboard();
        }
    }
}
pub(super) fn copy(text: &str) -> windows_core::Result<()> {
    let mut text: Vec<u16> = text.encode_utf16().collect();
    text.push(0);
    let Some(bytes) = text.len().checked_mul(std::mem::size_of::<u16>()) else {
        return Err(windows_core::Error::new(
            windows_core::HRESULT(0x8007000Eu32 as i32),
            "Clipboard data too large",
        ));
    };
    // GMEM_MOVEABLE: ownership transfers to Windows only on successful publication.
    let mut memory = Memory(unsafe { GlobalAlloc(0x2, bytes) });
    if memory.0.is_null() {
        return Err(windows_core::Error::from_thread());
    }
    let ptr = unsafe { GlobalLock(memory.0) };
    if ptr.is_null() {
        return Err(windows_core::Error::from_thread());
    }
    // SAFETY: GlobalAlloc/GlobalLock provided at least `bytes` writable bytes.
    unsafe {
        std::ptr::copy_nonoverlapping(text.as_ptr(), ptr.cast::<u16>(), text.len());
        GlobalUnlock(memory.0);
    }
    // A real owner is required by EmptyClipboard/SetClipboardData. Prefer the
    // active host root rather than setting up a plugin-owned hidden window.
    let owner = unsafe { GetAncestor(GetActiveWindow(), GA_ROOT) };
    if owner.is_null() || unsafe { OpenClipboard(owner) } == 0 {
        return Err(windows_core::Error::from_thread());
    }
    let _clipboard = Clipboard;
    if unsafe { EmptyClipboard() } == 0 || unsafe { SetClipboardData(13, memory.0) }.is_null() {
        return Err(windows_core::Error::from_thread());
    }
    memory.0 = std::ptr::null_mut();
    Ok(())
}
