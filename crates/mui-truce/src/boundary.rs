//! Panic containment without installing a process-wide panic hook.
use std::panic::{AssertUnwindSafe, catch_unwind};

pub(crate) fn guard<T>(operation: &str, f: impl FnOnce() -> T) -> Option<T> {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(value) => Some(value),
        Err(payload) => {
            // A custom panic payload's destructor can itself panic.
            if let Err(second) = catch_unwind(AssertUnwindSafe(|| drop(payload))) {
                std::mem::forget(second);
            }
            // Diagnostics are best effort too; they must not replace the panic.
            if let Err(payload) = catch_unwind(AssertUnwindSafe(|| {
                mui::diagnostics::breadcrumb("mui-truce", "callback_panic", operation);
            })) {
                std::mem::forget(payload);
            }
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callback_and_payload_drop_panics_are_contained() {
        struct BadDrop;
        impl Drop for BadDrop {
            fn drop(&mut self) {
                panic!("payload destructor");
            }
        }
        assert_eq!(guard("test", || std::panic::panic_any(BadDrop)), None::<()>);
        assert_eq!(guard("test", || 42), Some(42));
    }

    #[test]
    fn abort_opt_out_is_declared_in_the_manifest_and_guard() {
        assert!(
            include_str!("../Cargo.toml")
                .contains("allow-panic-abort = [\"mui-baseview/allow-panic-abort\"]")
        );
        let source = include_str!("lib.rs");
        assert!(source.contains("panic = \"abort\""));
        assert!(source.contains("not(feature = \"allow-panic-abort\")"));
        assert!(source.contains("compile_error!"));
    }
}
