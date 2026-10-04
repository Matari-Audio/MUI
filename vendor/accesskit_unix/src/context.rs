// Copyright 2023 The AccessKit Authors. All rights reserved.
// Licensed under the Apache License, Version 2.0 (found in
// the LICENSE-APACHE file) or the MIT license (found in
// the LICENSE-MIT file), at your option.

use accesskit::{ActivationHandler, DeactivationHandler};
use accesskit_atspi_common::{Adapter as AdapterImpl, AppContext, Event};
use atspi::proxy::bus::StatusProxy;
use std::sync::{Arc, Mutex, RwLock, Weak};
use std::thread::{self, JoinHandle};
use tokio::{
    pin, select,
    sync::{
        mpsc::{UnboundedReceiver as Receiver, UnboundedSender as Sender},
        oneshot,
    },
};
use tokio_stream::{StreamExt, wrappers::UnboundedReceiverStream};
use zbus::{Connection, connection::Builder, proxy::PropertyChanged};

use crate::{
    adapter::{AdapterState, Callback, Message},
    atspi::{Bus, map_or_ignoring_recoverable_error, zbus_error_is_unrecoverable},
    executor::Executor,
};

static WORKER: Mutex<Weak<Worker>> = Mutex::new(Weak::new());

/// Only public adapters own this object. Provider callbacks hold a sender,
/// never a strong owner, so the final adapter can stop the whole generation.
pub(crate) struct Worker {
    pub(crate) app: Arc<RwLock<AppContext>>,
    pub(crate) messages: Sender<Message>,
    stop: Option<oneshot::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for Worker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Worker").finish_non_exhaustive()
    }
}

fn app_name() -> Option<String> {
    std::env::current_exe().ok().and_then(|path| {
        path.file_name()
            .map(|name| name.to_string_lossy().to_string())
    })
}

pub(crate) fn get_or_init_worker() -> Arc<Worker> {
    let mut current = WORKER.lock().unwrap();
    if let Some(worker) = current.upgrade() {
        return worker;
    }
    let app = AppContext::new(app_name());
    let (messages, rx) = tokio::sync::mpsc::unbounded_channel();
    let (stop, stopped) = oneshot::channel();
    let app_copy = Arc::clone(&app);
    let messages_copy = messages.clone();
    let thread = thread::Builder::new().name("AccessKit Unix".into()).spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_io().enable_time().build()
            .expect("launch of owned accessibility runtime");
        runtime.block_on(async {
            // This stop race covers setup as well as every awaited bus action.
            // Cancelling drops all entries, bus connections and owned tasks.
            select! {
                biased;
                _ = stopped => {},
                _ = async {
                    let executor = Executor::new();
                    if let Ok(session_bus) = Builder::session() {
                        if let Ok(session_bus) = session_bus.internal_executor(false).build().await {
                            if let Err(error) = run_event_loop(&executor, session_bus, rx, &app_copy, &messages_copy).await {
                                if zbus_error_is_unrecoverable(&error) {
                                    panic!("Accessibility event loop failed: {error}");
                                }
                            }
                        }
                    }
                } => {},
            }
        });
        // Runtime drop cancels/drains tasks and waits for any owned blocking
        // jobs. Never abandon code that may be inside an unloadable library.
        drop(runtime);
    }).expect("launch of accessibility worker");
    let worker = Arc::new(Worker {
        app,
        messages,
        stop: Some(stop),
        thread: Some(thread),
    });
    *current = Arc::downgrade(&worker);
    worker
}

impl Drop for Worker {
    fn drop(&mut self) {
        // Clear only our generation, and release the registry before joining.
        let mut current = WORKER.lock().unwrap();
        if std::ptr::eq(current.as_ptr(), self) {
            *current = Weak::new();
        }
        drop(current);
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(thread) = self.thread.take() {
            // A handler may drop the last adapter on this very worker. The
            // stop request is observed after it returns; self-join is invalid.
            if thread.thread().id() != thread::current().id() {
                let _ = thread.join();
            }
        }
    }
}

struct AdapterEntry {
    id: usize,
    activation_handler: Box<dyn ActivationHandler>,
    deactivation_handler: Box<dyn DeactivationHandler>,
    state: Arc<Mutex<AdapterState>>,
}

fn activate_adapter(
    entry: &mut AdapterEntry,
    app: &Arc<RwLock<AppContext>>,
    messages: &Sender<Message>,
) {
    let mut state = entry.state.lock().unwrap();
    if let AdapterState::Inactive {
        is_window_focused,
        root_window_bounds,
        action_handler,
    } = &*state
    {
        *state = match entry.activation_handler.request_initial_tree() {
            Some(initial_state) => {
                let r#impl = AdapterImpl::with_wrapped_action_handler(
                    entry.id,
                    app,
                    Callback::new(messages.clone()),
                    initial_state,
                    *is_window_focused,
                    *root_window_bounds,
                    Arc::clone(action_handler),
                );
                AdapterState::Active(r#impl)
            }
            None => AdapterState::Pending {
                is_window_focused: *is_window_focused,
                root_window_bounds: *root_window_bounds,
                action_handler: Arc::clone(action_handler),
            },
        };
    }
}

fn deactivate_adapter(entry: &mut AdapterEntry) {
    let mut state = entry.state.lock().unwrap();
    match &*state {
        AdapterState::Inactive { .. } => (),
        AdapterState::Pending {
            is_window_focused,
            root_window_bounds,
            action_handler,
        } => {
            *state = AdapterState::Inactive {
                is_window_focused: *is_window_focused,
                root_window_bounds: *root_window_bounds,
                action_handler: Arc::clone(action_handler),
            };
            drop(state);
            entry.deactivation_handler.deactivate_accessibility();
        }
        AdapterState::Active(r#impl) => {
            *state = AdapterState::Inactive {
                is_window_focused: r#impl.is_window_focused(),
                root_window_bounds: r#impl.root_window_bounds(),
                action_handler: r#impl.wrapped_action_handler(),
            };
            drop(state);
            entry.deactivation_handler.deactivate_accessibility();
        }
    }
}

async fn bus_after_status_change(
    change: Option<PropertyChanged<'_, bool>>,
    session_bus: &Connection,
    executor: &Executor,
    app: &Arc<RwLock<AppContext>>,
) -> zbus::Result<Option<Bus>> {
    let enabled = match change {
        Some(change) => change.get().await?,
        None => false,
    };
    if enabled {
        map_or_ignoring_recoverable_error(Bus::new(session_bus, executor, app).await, None, Some)
    } else {
        Ok(None)
    }
}

fn sync_adapters(
    adapters: &mut [AdapterEntry],
    atspi_bus: &Option<Bus>,
    app: &Arc<RwLock<AppContext>>,
    messages: &Sender<Message>,
) {
    let active = atspi_bus.is_some();
    for entry in adapters {
        if active {
            activate_adapter(entry, app, messages);
        } else {
            deactivate_adapter(entry);
        }
    }
}

async fn run_event_loop(
    executor: &Executor,
    session_bus: Connection,
    rx: Receiver<Message>,
    app: &Arc<RwLock<AppContext>>,
    sender: &Sender<Message>,
) -> zbus::Result<()> {
    let session_bus_copy = session_bus.clone();
    let _session_bus_task = executor.spawn(
        async move {
            loop {
                session_bus_copy.executor().tick().await;
            }
        },
        "accesskit_session_bus_task",
    );

    let status = StatusProxy::new(&session_bus).await?;
    let changes = status.receive_is_enabled_changed().await.fuse();
    pin!(changes);

    let messages = UnboundedReceiverStream::new(rx).fuse();
    pin!(messages);

    let mut atspi_bus = None;
    let mut adapters: Vec<AdapterEntry> = Vec::new();

    loop {
        select! {
            change = changes.next() => {
                atspi_bus = bus_after_status_change(change, &session_bus, executor, app).await?;
                sync_adapters(&mut adapters, &atspi_bus, app, sender);

            }
            message = messages.next() => {
                if let Some(message) = message {
                    process_adapter_message(&atspi_bus, &mut adapters, message, app, sender).await?;
                }
            }
        }
    }
}

async fn process_adapter_message(
    atspi_bus: &Option<Bus>,
    adapters: &mut Vec<AdapterEntry>,
    message: Message,
    app: &Arc<RwLock<AppContext>>,
    messages: &Sender<Message>,
) -> zbus::Result<()> {
    match message {
        Message::AddAdapter {
            id,
            activation_handler,
            deactivation_handler,
            state,
        } => {
            adapters.push(AdapterEntry {
                id,
                activation_handler,
                deactivation_handler,
                state,
            });
            if atspi_bus.is_some() {
                let entry = adapters.last_mut().unwrap();
                activate_adapter(entry, app, messages);
            }
        }
        Message::RemoveAdapter { id } => {
            if let Ok(index) = adapters.binary_search_by(|entry| entry.id.cmp(&id)) {
                adapters.remove(index);
            }
        }
        Message::RegisterInterfaces { node, interfaces } => {
            if let Some(bus) = atspi_bus {
                bus.register_interfaces(node, interfaces).await?
            }
        }
        Message::UnregisterInterfaces {
            adapter_id,
            node_id,
            interfaces,
        } => {
            if let Some(bus) = atspi_bus {
                bus.unregister_interfaces(adapter_id, node_id, interfaces)
                    .await?
            }
        }
        Message::EmitEvent {
            adapter_id,
            event: Event::Object { target, event },
        } => {
            if let Some(bus) = atspi_bus {
                bus.emit_object_event(adapter_id, target, event).await?
            }
        }
        Message::EmitEvent {
            adapter_id,
            event:
                Event::Window {
                    target,
                    name,
                    event,
                },
        } => {
            if let Some(bus) = atspi_bus {
                bus.emit_window_event(adapter_id, target, name, event)
                    .await?;
            }
        }
        Message::EmitEvent {
            event: Event::Cache(_),
            ..
        } => unreachable!("cache events are sent as EmitCacheAdd/EmitCacheRemove"),
        Message::EmitCacheAdd { node } => {
            if let Some(bus) = atspi_bus {
                bus.emit_cache_add(node).await?;
            }
        }
        Message::EmitCacheRemove {
            adapter_id,
            node_id,
        } => {
            if let Some(bus) = atspi_bus {
                bus.emit_cache_remove(adapter_id, node_id).await?;
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn final_owner_disconnects_old_callbacks_and_reopen_has_a_fresh_generation() {
        let first = get_or_init_worker();
        let second = get_or_init_worker();
        assert!(Arc::ptr_eq(&first, &second));
        let previous = Arc::downgrade(&first);
        let app = Arc::downgrade(&first.app);
        let old_callback = first.messages.clone();
        drop(first);
        assert!(
            previous.upgrade().is_some(),
            "another adapter still owns this generation"
        );
        drop(second);
        assert!(previous.upgrade().is_none());
        assert!(
            app.upgrade().is_none(),
            "the old provider context survived join"
        );
        assert!(
            old_callback.is_closed(),
            "native callback queue survived final close"
        );
        let next = get_or_init_worker();
        assert!(!std::ptr::eq(previous.as_ptr(), Arc::as_ptr(&next)));
        assert!(
            old_callback
                .send(Message::RemoveAdapter { id: usize::MAX })
                .is_err()
        );
        drop(next);
    }
}
