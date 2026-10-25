//! Shell AppBar registration belongs to the toolkit-owned top-level HWND.
use crate::{DockEdge, DockOptions};
use std::cell::Cell;
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, RECT, WPARAM},
    Graphics::Gdi::{
        EnumDisplayMonitors, GetMonitorInfoW, MonitorFromWindow, HDC, HMONITOR, MONITORINFO,
        MONITORINFOEXW, MONITOR_DEFAULTTONEAREST,
    },
    UI::{HiDpi::GetDpiForWindow, Shell::*, WindowsAndMessaging::*},
};

const CALLBACK: u32 = WM_USER + 100;

pub(super) fn output_names() -> Vec<String> {
    monitors().into_iter().map(|(_, name)| name).collect()
}
fn monitors() -> Vec<(HMONITOR, String)> {
    unsafe extern "system" fn visit(monitor: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> i32 {
        // SAFETY: synchronous EnumDisplayMonitors borrows the caller's vector.
        let monitors = unsafe { &mut *(data as *mut Vec<(HMONITOR, String)>) };
        let mut info = MONITORINFOEXW::default();
        info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        // SAFETY: correctly sized writable structure and live monitor handle.
        if unsafe { GetMonitorInfoW(monitor, &mut info.monitorInfo) } != 0 {
            let end = info.szDevice.iter().position(|c| *c == 0).unwrap_or(info.szDevice.len());
            monitors.push((monitor, String::from_utf16_lossy(&info.szDevice[..end])));
        }
        1
    }
    let mut monitors = Vec::new();
    // SAFETY: callback context lives for the complete synchronous enumeration.
    unsafe {
        EnumDisplayMonitors(
            std::ptr::null_mut(),
            std::ptr::null(),
            Some(visit),
            &mut monitors as *mut Vec<(HMONITOR, String)> as LPARAM,
        );
    }
    monitors
}

pub(super) struct Dock {
    hwnd: HWND,
    options: DockOptions,
    height: Cell<u32>,
    registered: Cell<bool>,
    syncing: Cell<bool>,
    visible: Cell<bool>,
    taskbar_created: u32,
}
impl Dock {
    pub fn new(hwnd: HWND, options: DockOptions) -> Self {
        // SAFETY: the static string is terminated and this only registers a message ID.
        let taskbar_created = unsafe { RegisterWindowMessageW(windows_sys::w!("TaskbarCreated")) };
        Self {
            hwnd,
            height: Cell::new(options.height),
            options,
            registered: Cell::new(false),
            syncing: Cell::new(false),
            visible: Cell::new(false),
            taskbar_created,
        }
    }
    pub fn resize(&self, height: u32) {
        self.height.set(height.clamp(100, 1024));
        self.sync();
    }
    fn monitor(&self) -> HMONITOR {
        if let Some(name) = &self.options.output {
            monitors()
                .into_iter()
                .find(|(_, n)| n == name)
                .map(|(m, _)| m)
                .unwrap_or(std::ptr::null_mut())
        } else {
            // SAFETY: this HWND is owned by the calling native thread.
            unsafe { MonitorFromWindow(self.hwnd, MONITOR_DEFAULTTONEAREST) }
        }
    }
    pub fn register(&self) -> bool {
        if self.monitor().is_null() {
            return false;
        }
        if self.registered.get() {
            return true;
        }
        // SAFETY: this initialized descriptor belongs to the live owner-thread HWND.
        let registered = unsafe { SHAppBarMessage(ABM_NEW, &mut self.data()) != 0 };
        self.registered.set(registered);
        registered
    }
    fn data(&self) -> APPBARDATA {
        APPBARDATA {
            cbSize: std::mem::size_of::<APPBARDATA>() as u32,
            hWnd: self.hwnd,
            uCallbackMessage: CALLBACK,
            ..Default::default()
        }
    }
    pub fn visible(&self, visible: bool) {
        self.visible.set(visible);
        if visible {
            self.sync();
        } else {
            self.remove();
        }
    }
    pub fn sync(&self) {
        if !self.visible.get() || self.syncing.replace(true) {
            return;
        }
        struct Syncing<'a>(&'a Cell<bool>);
        impl Drop for Syncing<'_> {
            fn drop(&mut self) {
                self.0.set(false);
            }
        }
        let _syncing = Syncing(&self.syncing);
        let mut data = self.data();
        // SAFETY: data is initialized, hwnd is this live toolkit window, and
        // these calls run on its owning thread. No host HWND is altered.
        unsafe {
            if !self.registered.get() {
                if SHAppBarMessage(ABM_NEW, &mut data) == 0 {
                    return;
                }
                self.registered.set(true);
            }
            let monitor = self.monitor();
            let mut info = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            if GetMonitorInfoW(monitor, &mut info) == 0 {
                self.remove();
                // A named output vanished. Retire the surface so its owner
                // can restore the editor instead of leaving an unreserved bar.
                PostMessageW(self.hwnd, WM_CLOSE, 0, 0);
                return;
            }
            let height =
                (u64::from(self.height.get()) * u64::from(GetDpiForWindow(self.hwnd).max(96)) / 96)
                    .min((info.rcMonitor.bottom - info.rcMonitor.top).max(2) as u64 / 2)
                    as i32;
            data.uEdge = match self.options.edge {
                DockEdge::Top => ABE_TOP,
                DockEdge::Bottom => ABE_BOTTOM,
            };
            data.rc = info.rcMonitor;
            SHAppBarMessage(ABM_QUERYPOS, &mut data);
            match self.options.edge {
                DockEdge::Top => data.rc.bottom = data.rc.top + height,
                DockEdge::Bottom => data.rc.top = data.rc.bottom - height,
            }
            if SHAppBarMessage(ABM_SETPOS, &mut data) == 0 {
                self.remove();
                PostMessageW(self.hwnd, WM_CLOSE, 0, 0);
                return;
            }
            let mut current = RECT::default();
            GetWindowRect(self.hwnd, &mut current);
            if current.left != data.rc.left
                || current.top != data.rc.top
                || current.right != data.rc.right
                || current.bottom != data.rc.bottom
            {
                SetWindowPos(
                    self.hwnd,
                    HWND_TOPMOST,
                    data.rc.left,
                    data.rc.top,
                    data.rc.right - data.rc.left,
                    data.rc.bottom - data.rc.top,
                    SWP_NOACTIVATE | SWP_NOOWNERZORDER,
                );
            }
        }
    }
    pub fn message(&self, msg: u32, wparam: WPARAM, lparam: LPARAM) {
        if msg == self.taskbar_created {
            self.registered.set(false);
            self.sync();
        } else if msg == CALLBACK {
            match wparam as u32 {
                ABN_POSCHANGED => self.sync(),
                ABN_FULLSCREENAPP => {
                    // SAFETY: only this dock's z-order is changed; fullscreen
                    // applications may cover it without being resized by BUFFR.
                    unsafe {
                        SetWindowPos(
                            self.hwnd,
                            if lparam != 0 { HWND_BOTTOM } else { HWND_TOPMOST },
                            0,
                            0,
                            0,
                            0,
                            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                        );
                    }
                }
                _ => {}
            }
        } else if msg == WM_SHOWWINDOW {
            self.visible(wparam != 0);
        } else if msg == WM_DISPLAYCHANGE || msg == WM_DPICHANGED {
            self.sync();
        } else if self.registered.get() && (msg == WM_ACTIVATE || msg == WM_WINDOWPOSCHANGED) {
            let mut data = self.data();
            // SAFETY: synchronous Shell notification borrows initialized data.
            unsafe {
                SHAppBarMessage(
                    if msg == WM_ACTIVATE { ABM_ACTIVATE } else { ABM_WINDOWPOSCHANGED },
                    &mut data,
                );
            }
        }
    }
    pub fn remove(&self) {
        if self.registered.replace(false) {
            // SAFETY: registration is owned uniquely and removed before HWND teardown.
            unsafe {
                SHAppBarMessage(ABM_REMOVE, &mut self.data());
            }
        }
    }
}
impl Drop for Dock {
    fn drop(&mut self) {
        self.remove();
    }
}
