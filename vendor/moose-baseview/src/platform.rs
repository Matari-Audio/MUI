#[cfg(any(target_os = "windows", target_os = "macos", test))]
mod focus_handoff;
pub(crate) mod frame_rate;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::*;

#[cfg(target_os = "linux")]
mod x11;
#[cfg(target_os = "linux")]
pub use x11::*;
#[cfg(target_os = "windows")]
mod win;
#[cfg(target_os = "windows")]
pub use win::*;

// Run the Windows scheduler's pure policy tests on Linux/macOS as well.
#[cfg(all(test, not(target_os = "windows")))]
#[path = "platform/win/frame_state.rs"]
mod windows_frame_state_tests;
