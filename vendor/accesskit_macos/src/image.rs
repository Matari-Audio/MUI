// Copyright 2026 Matari Audio. Licensed under MIT OR Apache-2.0.
//! Objective-C class names belong to one linked Rust image, including reloads.

use objc2::{ClassType, declare::ClassBuilder, runtime::AnyClass};
use objc2_foundation::NSObject;
use std::{
    ffi::c_void,
    sync::{OnceLock, atomic::AtomicU8},
};

// Mutable, non-ZST storage cannot be folded into a shared constant. Its
// address is stable and distinct while separately linked images are loaded.
static IMAGE_MARKER: AtomicU8 = AtomicU8::new(0);
static NAMESPACE: OnceLock<String> = OnceLock::new();

pub(crate) fn marker_key() -> *const c_void {
    (&IMAGE_MARKER as *const AtomicU8).cast()
}

pub(crate) fn reserve_namespace() -> String {
    for generation in 0u64.. {
        let name = format!("AccessKitImage_{:x}_{generation}", marker_key() as usize);
        if let Some(builder) = ClassBuilder::new(&name, NSObject::class()) {
            // The anchor owns no Rust ivars or callbacks. It can survive
            // image unload, reserving this namespace in the ObjC runtime.
            builder.register();
            return name;
        }
        assert!(
            AnyClass::get(&name).is_some(),
            "could not reserve image namespace {name}"
        );
        // An old image may have occupied the same address before unload.
        // Reserve a fresh generation; never reuse its Rust-bearing classes.
    }
    unreachable!("image namespace generation exhausted")
}

pub(crate) fn class_name(base: &str) -> String {
    #[cfg(feature = "image-probe")]
    if base != "AccessKitImageDropProbe"
        && std::env::var_os("MUI_ACCESSKIT_PROBE_UNISOLATED").is_some()
    {
        return base.to_owned();
    }
    let namespace = NAMESPACE.get_or_init(reserve_namespace);
    format!("{base}_{namespace}")
}
