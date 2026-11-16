use super::{callback, native};
use std::{
    collections::HashMap,
    ffi::c_int,
    ptr,
    sync::{LazyLock, PoisonError, RwLock},
};
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    System::Threading::GetCurrentThreadId,
    UI::{
        Input::KeyboardAndMouse::GetFocus,
        WindowsAndMessaging::{
            CallNextHookEx, GetParent, SetWindowsHookExW, UnhookWindowsHookEx, HC_ACTION, HHOOK,
            MSG, PM_REMOVE, WH_GETMESSAGE, WM_CHAR, WM_KEYDOWN, WM_KEYUP, WM_NULL, WM_SYSCHAR,
            WM_SYSKEYDOWN, WM_SYSKEYUP,
        },
    },
};

// One hook per host GUI thread, per DLL image. Editors on another thread must
// not accidentally reuse a hook which only sees the first thread's message pump.
static HOOK_STATE: LazyLock<RwLock<HashMap<u32, ThreadHook>>> = LazyLock::new(RwLock::default);
pub(crate) struct KeyboardHookHandle {
    hwnd: usize,
    thread: u32,
}
#[derive(Default)]
struct ThreadHook {
    hook: usize,
    windows: HashMap<usize, KeyboardOwnership>,
}
struct KeyboardOwnership {
    capture: bool,
    held: [Option<bool>; 512],
}
impl KeyboardOwnership {
    fn new() -> Self {
        Self { capture: false, held: [None; 512] }
    }
}

pub(crate) fn set_keyboard_capture(hwnd: HWND, capture: bool) -> bool {
    let mut state = HOOK_STATE.write().unwrap_or_else(PoisonError::into_inner);
    let Some(thread) = state.get_mut(&unsafe { GetCurrentThreadId() }) else {
        return false;
    };
    let Some(owner) = thread.windows.get_mut(&(hwnd as usize)) else {
        return false;
    };
    let changed = owner.capture != capture;
    owner.capture = capture;
    changed
}

pub(crate) fn init_keyboard_hook(hwnd: HWND) -> KeyboardHookHandle {
    let id = unsafe { GetCurrentThreadId() };
    let mut state = HOOK_STATE.write().unwrap_or_else(PoisonError::into_inner);
    let thread = state.entry(id).or_default();
    let mut error = None;
    thread.windows.insert(hwnd as usize, KeyboardOwnership::new());
    if thread.hook == 0 {
        // For a thread in this process hMod must be NULL, not the host EXE.
        // The native creation path pins the image before registering this proc.
        let hook = unsafe {
            SetWindowsHookExW(WH_GETMESSAGE, Some(keyboard_hook_callback), ptr::null_mut(), id)
        };
        thread.hook = hook as usize;
        if hook.is_null() {
            error = Some(windows_core::Error::from_thread());
        }
    }
    drop(state);
    if let Some(error) = error {
        callback::guard(
            "hook install diagnostic",
            || (),
            || crate::warn!("SetWindowsHookExW failed: {}", error),
        );
    }
    KeyboardHookHandle { hwnd: hwnd as usize, thread: id }
}

impl Drop for KeyboardHookHandle {
    fn drop(&mut self) {
        let mut state = HOOK_STATE.write().unwrap_or_else(PoisonError::into_inner);
        let Some(thread) = state.get_mut(&self.thread) else {
            return;
        };
        let mut error = None;
        if super::frame_state::remove_window(&mut thread.windows, self.hwnd) {
            if thread.hook != 0 && unsafe { UnhookWindowsHookEx(thread.hook as HHOOK) } == 0 {
                // Keep the handle for a later retry. No windows remain to capture
                // keys; the pinned image makes the still-installed proc safe.
                error = Some(windows_core::Error::from_thread());
            } else {
                state.remove(&self.thread);
            }
        }
        drop(state);
        if let Some(error) = error {
            callback::guard(
                "hook uninstall diagnostic",
                || (),
                || crate::warn!("UnhookWindowsHookEx failed: {}", error),
            );
        }
    }
}

unsafe extern "system" fn keyboard_hook_callback(
    n_code: c_int, wparam: WPARAM, lparam: LPARAM,
) -> isize {
    callback::guard(
        "keyboard hook",
        || unsafe { CallNextHookEx(ptr::null_mut(), n_code, wparam, lparam) },
        || unsafe {
            if n_code == HC_ACTION as i32
                && wparam == PM_REMOVE as usize
                && lparam != 0
                && offer_message_to_baseview(&mut *(lparam as *mut MSG))
            {
                // Consume exactly once; dispatching WM_NULL cannot call a plugin proc.
                (*(lparam as *mut MSG)).hwnd = ptr::null_mut();
                (*(lparam as *mut MSG)).message = WM_NULL;
                0
            } else {
                CallNextHookEx(ptr::null_mut(), n_code, wparam, lparam)
            }
        },
    )
}

unsafe fn offer_message_to_baseview(msg: &mut MSG) -> bool {
    if !matches!(
        msg.message,
        WM_KEYDOWN | WM_SYSKEYDOWN | WM_KEYUP | WM_SYSKEYUP | WM_CHAR | WM_SYSCHAR
    ) {
        return false;
    }
    let scan = (msg.lParam as usize >> 16) & 0x1ff;
    let mut state = HOOK_STATE.write().unwrap_or_else(PoisonError::into_inner);
    let Some(thread) = state.get_mut(&unsafe { GetCurrentThreadId() }) else {
        return false;
    };
    let Some(owner) = thread.windows.get_mut(&(msg.hwnd as usize)) else {
        if matches!(msg.message, WM_KEYUP | WM_SYSKEYUP) {
            for owner in thread.windows.values_mut() {
                if let Some(held) = owner.held.get_mut(scan) {
                    *held = None;
                }
            }
        }
        return false;
    };
    // An unfocused or no-longer-capturing editor must never eat DAW shortcuts,
    // including queued keys whose HWND predates a focus transition.
    let focused = unsafe { GetFocus() == msg.hwnd };
    if !focused {
        owner.held.fill(None);
        return false;
    }
    let Some(held) = owner.held.get_mut(scan) else {
        return false;
    };
    let enabled = owner.capture;
    let previous = *held;
    // Never hold the registry lock over a reentrant window/handler callback.
    drop(state);
    // Finish observed presses even after capture is disabled, including
    // ignored releases that update the editor's modifier state.
    let offered = (enabled || previous.is_some())
        && unsafe { native::offer_keyboard_message(msg.hwnd, msg.message, msg.wParam, msg.lParam) };
    let capture = previous.unwrap_or(offered);
    let mut state = HOOK_STATE.write().unwrap_or_else(PoisonError::into_inner);
    if let Some(owner) = state
        .get_mut(&unsafe { GetCurrentThreadId() })
        .and_then(|thread| thread.windows.get_mut(&(msg.hwnd as usize)))
    {
        if let Some(held) = owner.held.get_mut(scan) {
            *held = match msg.message {
                WM_KEYUP | WM_SYSKEYUP => None,
                WM_KEYDOWN | WM_SYSKEYDOWN => Some(capture),
                _ => *held,
            };
        }
    }
    drop(state);
    if !capture {
        let parent = unsafe { GetParent(msg.hwnd) };
        if !parent.is_null() {
            msg.hwnd = parent;
        }
    }
    capture
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_editor_shortcuts_are_consumed_and_other_keys_reach_the_parent() {
        use crate::{
            Event, EventStatus, HandlerError, Window, WindowHandler, WindowSettings, WindowSize,
        };
        use keyboard_types::{Key, KeyState};
        use raw_window_handle::{
            HasWindowHandle, RawWindowHandle, Win32WindowHandle, WindowHandle,
        };
        use std::num::NonZeroIsize;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::{Arc, Mutex};
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::SetFocus;
        use windows_sys::Win32::UI::WindowsAndMessaging::*;

        struct Parent(HWND);
        impl HasWindowHandle for Parent {
            fn window_handle(&self) -> Result<WindowHandle<'_>, raw_window_handle::HandleError> {
                let handle = Win32WindowHandle::new(NonZeroIsize::new(self.0 as isize).unwrap());
                Ok(unsafe { WindowHandle::borrow_raw(RawWindowHandle::Win32(handle)) })
            }
        }
        impl Drop for Parent {
            fn drop(&mut self) {
                unsafe {
                    DestroyWindow(self.0);
                }
            }
        }
        struct Probe(Arc<Mutex<Vec<KeyState>>>, Arc<AtomicUsize>);
        impl WindowHandler for Probe {
            fn on_frame(&self) -> Result<(), HandlerError> {
                Ok(())
            }
            fn resized(&self, _: WindowSize) -> Result<(), HandlerError> {
                Ok(())
            }
            fn on_event(&self, event: Event) -> EventStatus {
                if let Event::Keyboard(key) = event {
                    if key.key == Key::Character(" ".into()) {
                        self.0.lock().unwrap().push(key.state);
                        // A hook may only claim the down. Its up must still stay out of the host.
                        return if key.state == KeyState::Down {
                            EventStatus::Captured
                        } else {
                            EventStatus::Ignored
                        };
                    }
                    self.1.fetch_add(1, Ordering::Relaxed);
                }
                EventStatus::Ignored
            }
        }
        unsafe {
            let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
            let parent = Parent(CreateWindowExW(
                0,
                class.as_ptr(),
                class.as_ptr(),
                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                0,
                0,
                100,
                100,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null(),
            ));
            assert!(!parent.0.is_null());
            let handle = Arc::new(AtomicUsize::new(0));
            let native = Arc::clone(&handle);
            let heard = Arc::new(Mutex::new(Vec::new()));
            let log = Arc::clone(&heard);
            let other_events = Arc::new(AtomicUsize::new(0));
            let other_log = Arc::clone(&other_events);
            let child = Window::create(WindowSettings::new().with_parent(&parent), move |cx| {
                let RawWindowHandle::Win32(hwnd) =
                    cx.platform_handle().window_handle().unwrap().as_raw()
                else {
                    unreachable!()
                };
                native.store(hwnd.hwnd.get() as usize, Ordering::Relaxed);
                Ok(Probe(log, other_log))
            })
            .unwrap();
            child.show().unwrap();
            let hwnd = handle.load(Ordering::Relaxed) as HWND;
            SetForegroundWindow(parent.0);
            SetFocus(hwnd);
            assert_eq!(GetFocus(), hwnd);
            assert!(set_keyboard_capture(hwnd, true));
            let message = |kind, key, scan: isize| MSG {
                hwnd,
                message: kind,
                wParam: key,
                lParam: scan << 16,
                ..MSG::default()
            };
            let mut space = message(WM_KEYDOWN, 0x20, 0x39);
            assert!(offer_message_to_baseview(&mut space));
            let mut repeat = message(WM_KEYDOWN, 0x20, 0x39);
            repeat.lParam |= 1 << 30;
            assert!(offer_message_to_baseview(&mut repeat));
            let mut up = message(WM_KEYUP, 0x20, 0x39);
            assert!(offer_message_to_baseview(&mut up));
            assert_eq!(*heard.lock().unwrap(), [KeyState::Down, KeyState::Down, KeyState::Up]);
            for kind in [WM_KEYDOWN, WM_KEYUP] {
                let mut other = message(kind, 0x41, 0x1e);
                assert!(!offer_message_to_baseview(&mut other));
                assert_eq!(other.hwnd, parent.0);
            }
            assert_eq!(
                other_events.load(Ordering::Relaxed),
                2,
                "the editor observes ignored releases too"
            );
            SetFocus(parent.0);
            let mut unfocused = message(WM_KEYDOWN, 0x20, 0x39);
            assert!(!offer_message_to_baseview(&mut unfocused));
            assert_eq!(heard.lock().unwrap().len(), 3);
            drop(child);
        }
    }

    #[test]
    fn editors_share_only_their_own_thread_hook() {
        let mut threads = HashMap::<u32, ThreadHook>::new();
        threads.entry(1).or_default().windows.insert(10, KeyboardOwnership::new());
        threads.entry(1).or_default().windows.insert(11, KeyboardOwnership::new());
        threads.entry(2).or_default().windows.insert(20, KeyboardOwnership::new());
        if let Some(t) = threads.get_mut(&1) {
            t.windows.remove(&10);
            assert!(!t.windows.is_empty());
            t.windows.remove(&11);
            assert!(t.windows.is_empty());
        }
        assert!(threads.get(&2).is_some_and(|t| t.windows.len() == 1));
    }
    #[test]
    fn new_editor_does_not_capture_host_keys() {
        assert!(!KeyboardOwnership::new().capture);
    }
}
