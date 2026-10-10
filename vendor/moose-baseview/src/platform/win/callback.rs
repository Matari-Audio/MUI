//! Panic containment at the Windows ABI boundary (requires panic = "unwind").
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};

pub(super) fn guard<T>(
    name: &'static str, fallback: impl FnOnce() -> T, body: impl FnOnce() -> T,
) -> T {
    static REPORTED: AtomicBool = AtomicBool::new(false);
    match catch_unwind(AssertUnwindSafe(body)) {
        Ok(result) => result,
        Err(payload) => {
            // User panic payloads can themselves panic in Drop. Neither that nor a
            // host-provided tracing subscriber may unwind out of our ABI boundary.
            std::mem::forget(payload);
            if !REPORTED.swap(true, Ordering::Relaxed) {
                if let Err(payload) = catch_unwind(|| crate::error!("Panic contained in {}", name))
                {
                    std::mem::forget(payload);
                }
            }
            fallback()
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn panic_payload_drop_cannot_unwind_a_second_time() {
        struct PanicOnDrop;
        impl Drop for PanicOnDrop {
            fn drop(&mut self) {
                panic!("payload destructor must never run");
            }
        }
        assert_eq!(super::guard("payload test", || 9, || std::panic::panic_any(PanicOnDrop)), 9);
    }

    #[test]
    fn callback_panic_returns_safe_default() {
        assert_eq!(super::guard("test", || 7, || panic!("fixture")), 7);
    }
}
