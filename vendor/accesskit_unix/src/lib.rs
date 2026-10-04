// Copyright 2022 The AccessKit Authors. All rights reserved.
// Licensed under the Apache License, Version 2.0 (found in
// the LICENSE-APACHE file) or the MIT license (found in
// the LICENSE-MIT file), at your option.

//! Linux accessibility uses a generation-scoped, owned Tokio worker.
//! The last adapter stops and joins it, including all provider callbacks and
//! runtime tasks, before returning from `Drop`. Handlers must not wait for
//! the thread closing the adapter. If the last adapter is dropped inside its
//! own worker callback, shutdown completes when that callback returns; the
//! caller must not unload its library from an executing callback.

mod adapter;
mod atspi;
mod context;
mod executor;
mod util;

pub use adapter::Adapter;
