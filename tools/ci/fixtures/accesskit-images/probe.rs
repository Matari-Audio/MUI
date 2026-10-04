use accesskit_macos::image_probe::Probe;
use std::ffi::c_void;

#[unsafe(no_mangle)]
pub extern "C" fn mui_probe_open() -> *mut c_void {
    Box::into_raw(Box::new(Probe::new())).cast()
}

/// # Safety
/// `handle` is live from this image; `out` holds `capacity` writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mui_probe_names(handle: *mut c_void, out: *mut u8, capacity: usize) -> usize {
    let probe = unsafe { &*handle.cast::<Probe>() };
    let names = probe.names().join("\n");
    assert!(names.len() <= capacity);
    unsafe { std::ptr::copy_nonoverlapping(names.as_ptr(), out, names.len()) };
    names.len()
}

/// # Safety
/// `handle` is live from this image and exclusively accessed on the main thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mui_probe_action(handle: *mut c_void) -> usize {
    unsafe { &mut *handle.cast::<Probe>() }.action()
}

/// # Safety
/// `handle` is live from this image and is consumed exactly once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mui_probe_close(handle: *mut c_void) {
    let probe = unsafe { Box::from_raw(handle.cast::<Probe>()) };
    probe.finish();
}
