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
    thread.windows.insert(hwnd as usize, KeyboardOwnership::new());
    if thread.hook == 0 {
        // For a thread in this process hMod must be NULL, not the host EXE.
        // The native creation path pins the image before registering this proc.
        let hook = unsafe {
            SetWindowsHookExW(WH_GETMESSAGE, Some(keyboard_hook_callback), ptr::null_mut(), id)
        };
        thread.hook = hook as usize;
        if hook.is_null() {
            crate::warn!("SetWindowsHookExW failed: {}", windows_core::Error::from_thread());
        }
    }
    KeyboardHookHandle { hwnd: hwnd as usize, thread: id }
}

impl Drop for KeyboardHookHandle {
    fn drop(&mut self) {
        let mut state = HOOK_STATE.write().unwrap_or_else(PoisonError::into_inner);
        let Some(thread) = state.get_mut(&self.thread) else {
            return;
        };
        if super::frame_state::remove_window(&mut thread.windows, self.hwnd) {
            if thread.hook != 0 && unsafe { UnhookWindowsHookEx(thread.hook as HHOOK) } == 0 {
                // Keep the handle for a later retry. No windows remain to capture
                // keys; the pinned image makes the still-installed proc safe.
                crate::warn!("UnhookWindowsHookEx failed: {}", windows_core::Error::from_thread());
            } else {
                state.remove(&self.thread);
            }
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
    let capture = if !owner.capture {
        *held = None;
        false
    } else {
        match msg.message {
            WM_KEYDOWN | WM_SYSKEYDOWN => *held.get_or_insert(true),
            WM_KEYUP | WM_SYSKEYUP => held.take().unwrap_or(true),
            _ => held.unwrap_or(true),
        }
    };
    // Never hold the registry lock over a reentrant window/handler callback.
    drop(state);
    if !capture {
        let parent = unsafe { GetParent(msg.hwnd) };
        if !parent.is_null() {
            msg.hwnd = parent;
        }
        return false;
    }
    unsafe {
        native::wnd_proc(msg.hwnd, msg.message, msg.wParam, msg.lParam);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
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
