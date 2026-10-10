//! Optional host-main-thread delivery. No lock here is shared with audio.
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, ThreadId};

use truce_core::editor::EditorBridge;

use crate::boundary::guard;

const MAX_PENDING: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Mutation {
    Begin(u32),
    Set(u32, f64),
    End(u32),
    Resize(u32, u32),
}

#[derive(Default)]
struct State {
    host: Option<Arc<dyn EditorBridge>>,
    thread: Option<ThreadId>,
    pending: VecDeque<Mutation>,
    delivered: Vec<u32>,
    flushing: bool,
    closing: bool,
    failed: bool,
    wake: Option<Arc<dyn Fn() + Send + Sync>>,
}

/// An opt-in queue for frameworks with a host-main-thread pump.
///
/// Retain [`Bridge::host_pump`](crate::Bridge::host_pump) or
/// [`MuiEditor::host_pump`](crate::MuiEditor::host_pump) and call [`Self::flush`]
/// from the host GUI thread, **outside** the framework's editor/model lock.
/// The opening thread owns delivery. A call from another thread does nothing.
/// `Editor::idle` also flushes, but truce 6.3 CLAP/VST3 do not call it: do not
/// select this mode unless the framework supplies a pump.
///
/// Sets coalesce within a pending gesture. Begin/end edges keep their order.
/// If the pump stalls past 4096 pending commands, the channel fails closed:
/// undelivered gestures are discarded and delivered gestures receive an End
/// at the next flush. Reattach to start a fresh channel. No worker or permanent
/// timer is created by MUI.
#[derive(Default)]
pub struct HostPump {
    state: Mutex<State>,
}

impl HostPump {
    pub(crate) fn attach(&self, host: Arc<dyn EditorBridge>) -> bool {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if state.host.is_some() || state.flushing {
            return false;
        }
        let wake = state.wake.take();
        *state = State {
            host: Some(host),
            thread: Some(thread::current().id()),
            wake,
            ..State::default()
        };
        true
    }

    /// Set a thread-safe wake notification, such as CLAP's request_callback.
    /// Called on the clean-to-pending transition, or on registration when
    /// commands are already pending, without the queue lock.
    /// It must only schedule a later host callback: never flush inline or
    /// synchronously re-enter the editor, whose model can still be locked.
    pub fn set_waker(&self, wake: Arc<dyn Fn() + Send + Sync>) {
        let (retired, notify) = {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            let retired = state.wake.replace(Arc::clone(&wake));
            let notify = state.host.is_some()
                && !state.pending.is_empty()
                && !state.closing
                && !state.failed;
            (retired, notify)
        };
        guard("release host waker", || drop(retired));
        if notify {
            guard("wake host pump", || wake());
        }
        guard("release host waker", || drop(wake));
    }

    pub(crate) fn enqueue(&self, mutation: Mutation) -> bool {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if state.host.is_none() || state.closing || state.failed {
            return false;
        }
        if let Mutation::Set(id, _) = mutation {
            for pending in state.pending.iter_mut().rev() {
                match *pending {
                    Mutation::Set(other, _) if other == id => {
                        *pending = mutation;
                        return true;
                    }
                    Mutation::Begin(other) | Mutation::End(other) if other == id => break,
                    _ => {}
                }
            }
        }
        if let Mutation::Resize(..) = mutation
            && let Some(pending) = state
                .pending
                .iter_mut()
                .find(|m| matches!(m, Mutation::Resize(..)))
        {
            *pending = mutation;
            return true;
        }
        if state.pending.len() == MAX_PENDING {
            state.pending.clear();
            state.failed = true;
            return false;
        }
        let wake = state
            .pending
            .is_empty()
            .then(|| state.wake.clone())
            .flatten();
        state.pending.push_back(mutation);
        drop(state);
        if let Some(wake) = wake {
            guard("wake host pump", || wake());
            guard("release host waker", || drop(wake));
        }
        true
    }

    /// Deliver pending mutations only on the opening host thread.
    ///
    /// The queue lock is released before every callback. Recursive flushes
    /// return immediately; newly queued commands wait for the next pump.
    /// Returns false for a wrong-thread call or a failed/revoked channel.
    pub fn flush(&self) -> bool {
        let (host, pending, mut delivered, mut failed) = {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            if state.thread != Some(thread::current().id()) || state.flushing {
                return false;
            }
            let Some(host) = state.host.clone() else {
                return false;
            };
            state.flushing = true;
            (
                host,
                std::mem::take(&mut state.pending),
                std::mem::take(&mut state.delivered),
                state.failed,
            )
        };
        if !failed {
            for mutation in pending {
                // Record a Begin before calling the host: it can panic after
                // accepting the gesture. Cleanup still owes it an End.
                match mutation {
                    Mutation::Begin(id) if !delivered.contains(&id) => delivered.push(id),
                    _ => {}
                }
                let ok = guard("host mutation", || match mutation {
                    Mutation::Begin(id) => host.begin_edit(id),
                    Mutation::Set(id, value) => host.set_param(id, value),
                    Mutation::End(id) => host.end_edit(id),
                    Mutation::Resize(w, h) => {
                        let _ = host.request_resize(w, h);
                    }
                })
                .is_some();
                if !ok {
                    failed = true;
                    break;
                }
                if let Mutation::End(id) = mutation {
                    delivered.retain(|&open| open != id);
                }
            }
        }
        let closing = {
            let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            failed |= state.failed;
            state.closing
        };
        if failed || closing {
            for id in delivered.drain(..) {
                guard("end gesture during teardown", || host.end_edit(id));
            }
        }
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.flushing = false;
        state.failed |= failed;
        let retired = if state.failed || state.closing {
            state.pending.clear();
            (state.host.take(), state.wake.take())
        } else {
            state.delivered = delivered;
            (None, None)
        };
        let ok = !state.failed;
        drop(state);
        guard("release host channel", || drop(retired));
        guard("release host callback", || drop(host));
        ok
    }

    pub(crate) fn close(&self) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.closing = true;
        state.pending.retain(|m| !matches!(m, Mutation::Resize(..)));
    }

    /// Revoke callbacks without calling the host. Use during plugin drop,
    /// after the host may have torn its own objects down.
    pub fn detach(&self) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let retired = (state.host.take(), state.wake.take());
        state.pending.clear();
        state.delivered.clear();
        state.closing = true;
        drop(state);
        guard("release host channel", || drop(retired));
    }
}

#[cfg(test)]
mod tests;
