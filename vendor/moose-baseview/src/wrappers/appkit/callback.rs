use objc2::rc::autoreleasepool;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};

/// AppKit and block callbacks must return to their caller, even after a Rust panic.
/// Keep the pool inside the guard: draining it can invoke our dealloc implementation.
pub(crate) fn callback<T>(name: &'static str, fallback: T, body: impl FnOnce() -> T) -> T {
    static REPORTED: AtomicBool = AtomicBool::new(false);
    match catch_unwind(AssertUnwindSafe(|| autoreleasepool(|_| body()))) {
        Ok(value) => value,
        Err(payload) => {
            // A user panic payload (or tracing subscriber) can itself panic on drop.
            // Neither may cause a second unwind across this boundary.
            std::mem::forget(payload);
            if !REPORTED.swap(true, Ordering::Relaxed) {
                if let Err(payload) = catch_unwind(AssertUnwindSafe(|| {
                    #[cfg(feature = "tracing")]
                    crate::warn!("Contained panic in AppKit callback: {}", name);
                    #[cfg(not(feature = "tracing"))]
                    {
                        // The default package has no tracing subscriber or feature.
                        // Still emit one containment diagnostic, never one per frame.
                        use std::io::Write;
                        let _ = writeln!(
                            std::io::stderr().lock(),
                            "baseview: contained panic in AppKit callback: {name}"
                        );
                    }
                })) {
                    std::mem::forget(payload);
                }
            }
            fallback
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn callback_returns_fallback_after_panic() {
        assert_eq!(super::callback("test", 7, || panic!("injected callback panic")), 7);
        assert_eq!(super::callback("test", 7, || 8), 8);
    }

    #[test]
    fn panic_payload_destructor_cannot_escape_the_guard() {
        struct Payload;
        impl Drop for Payload {
            fn drop(&mut self) {
                panic!("payload destructor");
            }
        }
        assert!(!super::callback("test payload", false, || std::panic::panic_any(Payload)));
    }
}
