use std::fmt::{Debug, Display, Formatter};
use x11_dl::xlib;

use super::xlib_connection::XlibConnection;
use std::cell::Cell;
use std::error::Error;
use std::os::fd::AsRawFd;
use std::os::raw::{c_int, c_uchar, c_ulong};
use std::panic::AssertUnwindSafe;
use std::sync::{Mutex, MutexGuard};

// A Rust static alone is image-local: two different plugins can contain two
// copies. The flock on our process's /proc directory is the cross-image mutex.
// No filesystem creation, stale lock file or shared plugin symbol is required.
static HANDLER_LOCK: Mutex<()> = Mutex::new(());
type Handler = unsafe extern "C" fn(*mut xlib::Display, *mut xlib::XErrorEvent) -> i32;
static PREVIOUS_HANDLER: Mutex<Option<Handler>> = Mutex::new(None);

struct ProcessHandlerLock {
    _local: MutexGuard<'static, ()>,
    file: std::fs::File,
}

impl ProcessHandlerLock {
    fn acquire() -> std::io::Result<Self> {
        let local = HANDLER_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let file = std::fs::File::open("/proc/self")?;
        loop {
            // SAFETY: file owns a valid fd; independent opens lock the same
            // process inode even when this code is in different plugin images.
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } == 0 {
                return Ok(Self { _local: local, file });
            }
            let e = std::io::Error::last_os_error();
            if e.kind() != std::io::ErrorKind::Interrupted {
                return Err(e);
            }
        }
    }
}

impl Drop for ProcessHandlerLock {
    fn drop(&mut self) {
        // SAFETY: file still owns the fd. Closing also releases the lock.
        let _ = unsafe { libc::flock(self.file.as_raw_fd(), libc::LOCK_UN) };
    }
}

struct RestoreHandler<'a> {
    conn: &'a XlibConnection,
    previous: Option<Handler>,
}

impl Drop for RestoreHandler<'_> {
    fn drop(&mut self) {
        // Deliver even unchecked GLX errors before restoring the host handler.
        self.conn.sync();
        self.conn.set_error_handler(self.previous);
        let _ = CURRENT_DISPLAY.try_with(|d| d.set(std::ptr::null_mut()));
    }
}

thread_local! {
    /// Used as part of [`XErrorHandler::handle()`]. When an X11 error occurs during this function,
    /// the error gets copied to this Cell after which the program is allowed to resume. The
    /// error can then be converted to a regular Rust Result value afterward.
    static CURRENT_X11_ERROR: Cell<Option<CaughtXLibError>> = const { Cell::new(None) };
    static CURRENT_DISPLAY: Cell<*mut xlib::Display> = const { Cell::new(std::ptr::null_mut()) };
}

/// A helper struct for safe X11 error handling.
pub struct XErrorHandler<'a> {
    conn: &'a XlibConnection,
    error: &'a Cell<Option<CaughtXLibError>>,
}

impl<'a> XErrorHandler<'a> {
    /// Syncs and checks if any previous X11 calls from the given display returned an error.
    pub fn check(&self) -> Result<(), XLibError> {
        // Flush all possible previous errors
        self.conn.sync();

        let error = self.error.take();

        match error {
            None => Ok(()),
            Some(inner) => Err(XLibError::from_inner(inner, self.conn)),
        }
    }

    /// Sets up a temporary X11 error handler for the duration of the given closure, and allows
    /// that closure to check on the latest X11 error at any time.
    pub fn handle<T, F: FnOnce(&mut XErrorHandler) -> T>(
        conn: &XlibConnection, handler: F,
    ) -> crate::platform::Result<T> {
        /// # Safety
        /// The given display and error pointers *must* be valid for the duration of this function.
        unsafe extern "C" fn error_handler(
            dpy: *mut xlib::Display, err: *mut xlib::XErrorEvent,
        ) -> i32 {
            let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
                if err.is_null() {
                    return 0;
                }
                let ours = CURRENT_DISPLAY.try_with(|d| d.get() == dpy).unwrap_or(false);
                if !ours {
                    let previous =
                        *PREVIOUS_HANDLER.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                    // Do not swallow errors from the host's other connections.
                    return previous.map_or(0, |callback| unsafe { callback(dpy, err) });
                }
                // SAFETY: Xlib supplies a live XErrorEvent for this callback.
                let event = unsafe { err.read() };
                let _ = CURRENT_X11_ERROR.try_with(|error| {
                    if error.get().is_none() {
                        error.set(Some(CaughtXLibError::from_event(event)));
                    }
                });
                0
            }));
            match result {
                Ok(value) => value,
                Err(_) => {
                    // Even a custom logger must not unwind through Xlib.
                    let _ =
                        std::panic::catch_unwind(|| crate::warn!("Panic in Xlib error callback"));
                    0
                }
            }
        }

        if CURRENT_DISPLAY.with(|d| !d.get().is_null()) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::WouldBlock,
                "Nested Xlib error-handler scope is not supported",
            )
            .into());
        }
        let _lock = ProcessHandlerLock::acquire()?;
        // Flush prior errors while the original handler is still installed.
        conn.sync();

        CURRENT_X11_ERROR.with(|error| {
            // Make sure to clear any errors from the last call to this function
            error.set(None);

            CURRENT_DISPLAY.with(|d| d.set(conn.as_raw()));
            // Hold this separate lock across installation so a concurrent
            // foreign-display callback cannot read a stale previous handler.
            let mut forwarding =
                PREVIOUS_HANDLER.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let previous = conn.set_error_handler(Some(error_handler));
            *forwarding = previous;
            drop(forwarding);
            let restore = RestoreHandler { conn, previous };
            let panic_result = std::panic::catch_unwind(AssertUnwindSafe(|| {
                let mut h = XErrorHandler { conn, error };
                handler(&mut h)
            }));
            // Whatever happened, restore old error handler
            drop(restore);

            match panic_result {
                Ok(v) => Ok(v),
                Err(e) => std::panic::resume_unwind(e),
            }
        })
    }
}

#[derive(Copy, Clone)]
struct CaughtXLibError {
    type_: c_int,
    resourceid: xlib::XID,
    serial: c_ulong,
    error_code: c_uchar,
    request_code: c_uchar,
    minor_code: c_uchar,
}

impl CaughtXLibError {
    fn from_event(error: xlib::XErrorEvent) -> CaughtXLibError {
        Self {
            type_: error.type_,
            resourceid: error.resourceid,
            serial: error.serial,

            error_code: error.error_code,
            request_code: error.request_code,
            minor_code: error.minor_code,
        }
    }
}

pub struct XLibError {
    inner: CaughtXLibError,
    display_name: Box<str>,
}

impl XLibError {
    fn from_inner(inner: CaughtXLibError, conn: &XlibConnection) -> Self {
        let mut buf = [0; 255];
        let cstr = conn.get_error_text(&mut buf, inner.error_code);

        Self { display_name: cstr.to_string_lossy().into(), inner }
    }
}

impl Debug for XLibError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("XLibError")
            .field("error_code", &self.inner.error_code)
            .field("error_message", &self.display_name)
            .field("minor_code", &self.inner.minor_code)
            .field("request_code", &self.inner.request_code)
            .field("type", &self.inner.type_)
            .field("resource_id", &self.inner.resourceid)
            .field("serial", &self.inner.serial)
            .finish()
    }
}

impl Display for XLibError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "XLib error: {} (error code {})", self.display_name, self.inner.error_code)
    }
}

impl Error for XLibError {}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "native error-handler regression setup must fail on errors")]
mod tests {
    use super::*;

    #[test]
    fn process_mutex_is_shared_between_independent_plugin_file_descriptions() {
        let first = std::fs::File::open("/proc/self").unwrap();
        let second = std::fs::File::open("/proc/self").unwrap();
        assert_eq!(unsafe { libc::flock(first.as_raw_fd(), libc::LOCK_EX) }, 0);
        assert_ne!(unsafe { libc::flock(second.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) }, 0);
        assert_eq!(std::io::Error::last_os_error().kind(), std::io::ErrorKind::WouldBlock);
        drop(first);
        assert_eq!(unsafe { libc::flock(second.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) }, 0);
    }

    #[test]
    #[ignore = "requires a live X11 display"]
    fn native_concurrent_scopes_restore_handler_even_after_panic() {
        unsafe extern "C" fn sentinel(_: *mut xlib::Display, _: *mut xlib::XErrorEvent) -> i32 {
            0
        }
        let conn = XlibConnection::open().unwrap();
        let original = conn.set_error_handler(Some(sentinel));
        let restore = RestoreHandler { conn: &conn, previous: original };
        let threads: Vec<_> = (0..2)
            .map(|_| {
                std::thread::spawn(|| {
                    let conn = XlibConnection::open().unwrap();
                    for i in 0..20 {
                        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
                            XErrorHandler::handle(&conn, |errors| {
                                // A checked XCB request would avoid this global handler;
                                // GLX has no checked-cookie equivalent. Exercise Xlib.
                                unsafe { (conn.xlib().XDestroyWindow)(conn.as_raw(), 0) };
                                assert!(errors.check().is_err());
                                std::thread::sleep(std::time::Duration::from_millis(1));
                                if i == 10 {
                                    panic!("scope panic regression");
                                }
                            })
                            .unwrap();
                        }));
                        assert_eq!(result.is_err(), i == 10);
                    }
                })
            })
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }
        let installed = conn.set_error_handler(Some(sentinel));
        assert_eq!(installed.map(|f| f as usize), Some(sentinel as *const () as usize));
        drop(restore);
    }
}
