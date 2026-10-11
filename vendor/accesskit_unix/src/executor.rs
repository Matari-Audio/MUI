// Copyright 2024 The AccessKit Authors. All rights reserved.
// Licensed under the Apache License, Version 2.0 (found in
// the LICENSE-APACHE file) or the MIT license (found in
// the LICENSE-MIT file), at your option.

// Derived from zbus.
// Copyright 2024 Zeeshan Ali Khan.
// Licensed under the MIT license (found in the LICENSE-MIT file).

use tokio::task::JoinHandle;

/// Tasks belong to the current generation's owned Tokio runtime.
#[derive(Debug, Clone)]
pub(crate) struct Executor;

impl Executor {
    pub(crate) fn spawn<T: Send + 'static>(
        &self,
        future: impl std::future::Future<Output = T> + Send + 'static,
        _name: &str,
    ) -> Task<T> {
        Task(tokio::task::spawn(future))
    }

    pub(crate) fn new() -> Self {
        Self
    }
}

#[derive(Debug)]
pub(crate) struct Task<T>(JoinHandle<T>);

impl<T> Drop for Task<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}
