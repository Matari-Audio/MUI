// Copyright 2022 The AccessKit Authors. All rights reserved.
// Licensed under the Apache License, Version 2.0 (found in
// the LICENSE-APACHE file) or the MIT license (found in
// the LICENSE-MIT file), at your option.

#![deny(unsafe_op_in_unsafe_fn)]

mod class;
mod class_ivars;
mod class_macro;
mod context;
mod filters;
mod image;
#[cfg(feature = "image-probe")]
pub mod image_probe;
mod node;
mod util;

mod adapter;
pub use adapter::Adapter;

mod event;
pub use event::QueuedEvents;

mod patch;
pub use patch::add_focus_forwarder_to_window_class;

mod subclass;
pub use subclass::SubclassingAdapter;

pub use objc2_foundation::{NSArray, NSObject, NSPoint};
