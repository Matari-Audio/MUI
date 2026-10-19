//! Optional Linux X11 registration handoff to an existing backend worker.
//!
//! This module performs no native I/O, starts no threads, consumes no native
//! events and accepts/completes no payload. Native registration remains the
//! worker's responsibility through the reviewed typed winit capability.

use std::cell::Cell;
use std::fmt;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use winit::platform::x11::{ExternalXdndLease, ExternalXdndPending, WindowExtX11};
use winit::window::Window;

pub use winit::platform::x11::{
    ExternalXdndChallenge as Challenge, ExternalXdndError as NativeError,
    ExternalXdndPhase as NativePhase, ExternalXdndRegistration as Registration,
    ExternalXdndStatus as NativeStatus,
};

/// Opaque process-owner and checked local generation; display sequence alone is
/// never accepted as authority for callbacks from a previous/recreated pair.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct AttachmentId {
    owner: u64,
    sequence: u64,
}

impl AttachmentId {
    /// Display-only local sequence. Compare the full opaque ID for identity.
    pub const fn sequence(self) -> u64 {
        self.sequence
    }
}

/// Invalid configuration, detected before native window creation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PairError {
    /// Another live native host still owns this pair.
    Busy,
    /// The backend receiver has been dropped.
    Closed,
    /// A checked owner/generation allocator is exhausted; it never wraps.
    Exhausted,
}

impl fmt::Display for PairError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "external XDND pair: {self:?}")
    }
}

impl std::error::Error for PairError {}

/// Registration lifecycle only; none of these states accepts a transfer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    Waiting,
    Preparing,
    Offered,
    BackendActive,
    Registered,
    UnsupportedBackend,
    Unavailable(NativeError),
    Revoked,
    Closed,
}

/// Cheap bounded state. Native cleanup can remain uncertain after logical revoke.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Status {
    pub attachment: Option<AttachmentId>,
    pub phase: Phase,
    pub native: Option<NativeStatus>,
}

struct Control {
    id: AttachmentId,
    revoked: AtomicBool,
    offered: AtomicBool,
    unsupported: AtomicBool,
    unavailable: OnceLock<NativeError>,
    native: OnceLock<Arc<ExternalXdndLease>>,
}

impl Control {
    fn revoke(&self) {
        self.revoked.store(true, Ordering::SeqCst);
        if let Some(native) = self.native.get() {
            native.revoke();
        }
    }

    fn status(&self) -> Status {
        let native = self.native.get().map(|lease| lease.status());
        let phase =
            if self.revoked.load(Ordering::SeqCst) || native.is_some_and(|native| native.revoked) {
                Phase::Revoked
            } else if self.unsupported.load(Ordering::SeqCst) {
                Phase::UnsupportedBackend
            } else if let Some(error) = self.unavailable.get() {
                Phase::Unavailable(*error)
            } else if native.is_some_and(|native| native.phase == NativePhase::Installed) {
                Phase::Registered
            } else if native.is_some_and(|native| native.phase == NativePhase::Active) {
                Phase::BackendActive
            } else if self.offered.load(Ordering::SeqCst) {
                Phase::Offered
            } else {
                Phase::Preparing
            };
        Status {
            attachment: Some(self.id),
            phase,
            native,
        }
    }
}

struct Item<P> {
    control: Arc<Control>,
    payload: P,
}

struct State<P> {
    next: Option<u64>,
    closed: bool,
    active: Option<Arc<Control>>,
    // One retired small control keeps native cleanup status observable. There
    // are no catalogs, payload buffers or native connections in these controls.
    last: Option<Arc<Control>>,
    pending: Option<Item<P>>,
}

struct Shared<P> {
    owner: u64,
    state: Mutex<State<P>>,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl<P> Shared<P> {
    fn new(owner: u64) -> Arc<Self> {
        Arc::new(Self {
            owner,
            state: Mutex::new(State {
                next: Some(1),
                closed: false,
                active: None,
                last: None,
                pending: None,
            }),
        })
    }

    fn check(&self) -> Result<(), PairError> {
        let state = lock(&self.state);
        if state.closed {
            Err(PairError::Closed)
        } else if state.active.is_some() {
            Err(PairError::Busy)
        } else if state.next.is_none() {
            Err(PairError::Exhausted)
        } else {
            Ok(())
        }
    }

    fn reserve(self: &Arc<Self>) -> Result<Token<P>, PairError> {
        let mut state = lock(&self.state);
        if state.closed {
            return Err(PairError::Closed);
        }
        if state.active.is_some() {
            return Err(PairError::Busy);
        }
        let sequence = state.next.ok_or(PairError::Exhausted)?;
        state.next = sequence.checked_add(1);
        let control = Arc::new(Control {
            id: AttachmentId {
                owner: self.owner,
                sequence,
            },
            revoked: AtomicBool::new(false),
            offered: AtomicBool::new(false),
            unsupported: AtomicBool::new(false),
            unavailable: OnceLock::new(),
            native: OnceLock::new(),
        });
        state.active = Some(Arc::clone(&control));
        Ok(Token {
            shared: Arc::clone(self),
            control,
        })
    }

    fn status(&self) -> Status {
        let (closed, control) = {
            let state = lock(&self.state);
            (
                state.closed,
                state.active.as_ref().or(state.last.as_ref()).cloned(),
            )
        };
        let mut status = control.map_or(
            Status {
                attachment: None,
                phase: Phase::Waiting,
                native: None,
            },
            |control| control.status(),
        );
        if closed {
            status.phase = Phase::Closed;
        }
        status
    }
}

struct Token<P> {
    shared: Arc<Shared<P>>,
    control: Arc<Control>,
}

impl<P> Token<P> {
    fn publish(&self, payload: P) {
        let item = Item {
            control: Arc::clone(&self.control),
            payload,
        };
        let discarded = {
            let mut state = lock(&self.shared.state);
            if !state.closed
                && !self.control.revoked.load(Ordering::SeqCst)
                && state
                    .active
                    .as_ref()
                    .is_some_and(|active| active.id == self.control.id)
            {
                self.control.offered.store(true, Ordering::SeqCst);
                state.pending.replace(item)
            } else {
                Some(item)
            }
        };
        // Pending winit capabilities are memory-only; never drop an activated
        // native connection on this producer/UI path, or while holding its lock.
        drop(discarded);
    }
}

impl<P> Drop for Token<P> {
    fn drop(&mut self) {
        self.control.revoke();
        let discarded = {
            let mut state = lock(&self.shared.state);
            if state
                .active
                .as_ref()
                .is_some_and(|active| active.id == self.control.id)
            {
                state.last = state.active.take();
            }
            if state
                .pending
                .as_ref()
                .is_some_and(|pending| pending.control.id == self.control.id)
            {
                state.pending.take()
            } else {
                None
            }
        };
        drop(discarded);
    }
}

struct Receiver<P>(Arc<Shared<P>>, PhantomData<Cell<()>>);

impl<P> Receiver<P> {
    fn new(shared: Arc<Shared<P>>) -> Self {
        Self(shared, PhantomData)
    }
    fn poll(&self) -> Option<Item<P>> {
        lock(&self.0.state).pending.take()
    }

    fn is_current(&self, id: AttachmentId) -> bool {
        let active = {
            let state = lock(&self.0.state);
            if state.closed {
                return false;
            }
            state.active.clone()
        };
        active.is_some_and(|active| {
            active.id == id && !matches!(active.status().phase, Phase::Revoked)
        })
    }
}

impl<P> Drop for Receiver<P> {
    fn drop(&mut self) {
        let (active, last, discarded) = {
            let mut state = lock(&self.0.state);
            state.closed = true;
            (
                state.active.clone(),
                state.last.clone(),
                state.pending.take(),
            )
        };
        if let Some(active) = active {
            active.revoke();
        }
        if let Some(last) = last {
            last.revoke();
        }
        drop(discarded);
    }
}

static NEXT_OWNER: AtomicU64 = AtomicU64::new(1);

fn owner(allocator: &AtomicU64) -> Result<u64, PairError> {
    allocator
        .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |next| {
            next.checked_add(1)
        })
        .map_err(|_| PairError::Exhausted)
}

/// Create one bounded pair. Put Host in Options and move Worker to an existing
/// backend thread. Each live window needs its own pair; cloning Options does not
/// authorize two concurrent native hosts on the same pair.
pub fn channel() -> Result<(Host, Worker), PairError> {
    let shared = Shared::new(owner(&NEXT_OWNER)?);
    Ok((Host(Arc::clone(&shared)), Worker(Receiver::new(shared))))
}

/// Cloneable options/configuration endpoint. Dropping a clone does not detach.
#[derive(Clone)]
pub struct Host(Arc<Shared<ExternalXdndPending>>);

impl fmt::Debug for Host {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExternalXdndHost")
            .field("owner", &self.0.owner)
            .finish_non_exhaustive()
    }
}

impl Host {
    /// Read bounded state, without native I/O or backend waits.
    pub fn status(&self) -> Status {
        self.0.status()
    }

    pub(crate) fn check(&self) -> Result<(), PairError> {
        self.0.check()
    }

    pub(crate) fn reserve(&self) -> Result<Reservation, PairError> {
        self.0.reserve().map(Reservation)
    }
}

/// Non-clone receiver for one backend worker. Drop revokes the active generation
/// and pending invitation without joining or reclaiming native connections.
pub struct Worker(Receiver<ExternalXdndPending>);

impl Worker {
    /// Take the latest bounded invitation; no native I/O. Call outside rendering.
    pub fn poll(&self) -> Option<Invitation> {
        self.0.poll().map(Invitation)
    }

    /// Check logical host identity using the complete opaque generation. A true
    /// value is not transfer admission or proof that native properties remain.
    pub fn is_current(&self, id: AttachmentId) -> bool {
        self.0.is_current(id)
    }

    pub fn status(&self) -> Status {
        self.0.0.status()
    }
}

/// One-shot typed live-window offer. Dropping before connect is memory-only.
pub struct Invitation(Item<ExternalXdndPending>);

impl Invitation {
    pub fn id(&self) -> AttachmentId {
        self.0.control.id
    }

    /// Bounded server/target/proxy proof metadata; never consult mutable DISPLAY.
    pub fn challenge(&self) -> &Challenge {
        self.0.payload.challenge()
    }

    /// Blocking setup belongs to the existing backend thread. The returned
    /// registration cannot be sent back to the UI thread; native cleanup remains
    /// on that backend and may report uncertainty after logical host revocation.
    pub fn connect(self) -> Result<Registration, NativeError> {
        self.0.payload.connect()
    }
}

pub(crate) struct Reservation(Token<ExternalXdndPending>);

impl Reservation {
    pub(crate) fn bind(self, window: &Window) -> Attachment {
        let pending = match window.prepare_external_xdnd() {
            Ok((lease, pending)) => {
                let lease = Arc::new(lease);
                if self.0.control.revoked.load(Ordering::SeqCst) {
                    lease.revoke();
                }
                let _ = self.0.control.native.set(Arc::clone(&lease));
                // Receiver Drop can race the first check/native publication.
                if self.0.control.revoked.load(Ordering::SeqCst) {
                    lease.revoke();
                }
                Some(pending)
            }
            Err(NativeError::UnsupportedBackend) => {
                self.0.control.unsupported.store(true, Ordering::SeqCst);
                None
            }
            Err(error) => {
                let _ = self.0.control.unavailable.set(error);
                None
            }
        };
        Attachment {
            token: self.0,
            pending,
        }
    }
}

/// Stored before Gpu in App and explicitly dropped before normal GPU teardown.
pub(crate) struct Attachment {
    token: Token<ExternalXdndPending>,
    pending: Option<ExternalXdndPending>,
}

impl Attachment {
    pub(crate) fn publish(&mut self) {
        if let Some(pending) = self.pending.take() {
            self.token.publish(pending);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Barrier;
    use std::sync::atomic::AtomicUsize;

    use super::*;

    struct DropProbe {
        shared: std::sync::Weak<Shared<DropProbe>>,
        dropped: Arc<AtomicUsize>,
        assert_unlocked: bool,
    }

    impl Drop for DropProbe {
        fn drop(&mut self) {
            if let Some(shared) = self.shared.upgrade().filter(|_| self.assert_unlocked) {
                assert!(
                    shared.state.try_lock().is_ok(),
                    "mailbox payload dropped under its mutex"
                );
            }
            self.dropped.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn one_live_host_and_clone_configuration_checked_without_native_io() {
        let (host, worker) = channel().unwrap();
        let clone = host.clone();
        let reservation = host.reserve().unwrap();
        assert_eq!(clone.check(), Err(PairError::Busy));
        assert_eq!(worker.status().phase, Phase::Preparing);
        drop(reservation);
        assert_eq!(clone.check(), Ok(()));
        let next = clone.reserve().unwrap();
        drop(worker);
        assert_eq!(clone.check(), Err(PairError::Closed));
        assert_eq!(clone.status().phase, Phase::Closed);
        assert!(next.0.control.revoked.load(Ordering::SeqCst));
    }

    #[test]
    fn abandoned_reservation_releases_then_old_generation_cannot_revoke_new() {
        let shared = Shared::<u8>::new(11);
        let receiver = Receiver::new(Arc::clone(&shared));
        let first = shared.reserve().unwrap();
        let old = Arc::clone(&first.control);
        first.publish(1);
        let invitation = receiver.poll().unwrap();
        assert_eq!(invitation.payload, 1);
        drop(first);
        let second = shared.reserve().unwrap();
        second.publish(2);
        old.revoke();
        assert!(!receiver.is_current(invitation.control.id));
        assert!(receiver.is_current(second.control.id));
        assert_eq!(receiver.poll().unwrap().payload, 2);
        assert!(receiver.poll().is_none());
    }

    #[test]
    fn foreign_pairs_never_accept_equal_display_sequences() {
        let first = Shared::<()>::new(17);
        let second = Shared::<()>::new(18);
        let first_token = first.reserve().unwrap();
        let second_token = second.reserve().unwrap();
        let receiver = Receiver::new(second);
        assert_eq!(
            first_token.control.id.sequence(),
            second_token.control.id.sequence()
        );
        assert_ne!(first_token.control.id, second_token.control.id);
        assert!(!receiver.is_current(first_token.control.id));
        assert!(receiver.is_current(second_token.control.id));
    }

    #[test]
    fn mailbox_is_latest_only_and_all_payload_drops_are_outside_lock() {
        let shared = Shared::new(21);
        let receiver = Receiver::new(Arc::clone(&shared));
        let token = shared.reserve().unwrap();
        let dropped = Arc::new(AtomicUsize::new(0));
        for _ in 0..100 {
            token.publish(DropProbe {
                shared: Arc::downgrade(&shared),
                dropped: Arc::clone(&dropped),
                assert_unlocked: true,
            });
        }
        assert_eq!(dropped.load(Ordering::SeqCst), 99);
        let invitation = receiver.poll().unwrap();
        assert!(receiver.poll().is_none());
        drop(invitation);
        assert_eq!(dropped.load(Ordering::SeqCst), 100);
        token.publish(DropProbe {
            shared: Arc::downgrade(&shared),
            dropped: Arc::clone(&dropped),
            assert_unlocked: true,
        });
        drop(token);
        assert!(receiver.poll().is_none());
        assert_eq!(dropped.load(Ordering::SeqCst), 101);
    }

    #[test]
    fn receiver_drop_racing_offer_revokes_and_discards_exactly_once() {
        for _ in 0..32 {
            let shared = Shared::new(25);
            let receiver = Receiver::new(Arc::clone(&shared));
            let token = shared.reserve().unwrap();
            let barrier = Arc::new(Barrier::new(2));
            let remote = Arc::clone(&barrier);
            let dropped = Arc::new(AtomicUsize::new(0));
            let worker = std::thread::spawn(move || {
                remote.wait();
                drop(receiver);
            });
            barrier.wait();
            token.publish(DropProbe {
                shared: Arc::downgrade(&shared),
                dropped: Arc::clone(&dropped),
                // Another thread may legitimately hold the mutex here. The
                // serial mailbox test proves drops are outside our own guard.
                assert_unlocked: false,
            });
            worker.join().unwrap();
            assert!(token.control.revoked.load(Ordering::SeqCst));
            assert_eq!(shared.check(), Err(PairError::Closed));
            assert!(lock(&shared.state).pending.is_none());
            assert_eq!(dropped.load(Ordering::SeqCst), 1);
        }
    }

    #[test]
    fn checked_owner_and_local_sequences_never_wrap() {
        let allocator = AtomicU64::new(u64::MAX - 1);
        assert_eq!(owner(&allocator), Ok(u64::MAX - 1));
        assert_eq!(owner(&allocator), Err(PairError::Exhausted));
        assert_eq!(owner(&allocator), Err(PairError::Exhausted));
        let shared = Shared::<()>::new(29);
        lock(&shared.state).next = Some(u64::MAX);
        let token = shared.reserve().unwrap();
        assert_eq!(token.control.id.sequence(), u64::MAX);
        drop(token);
        assert_eq!(shared.check(), Err(PairError::Exhausted));
        assert!(matches!(shared.reserve(), Err(PairError::Exhausted)));
    }

    #[test]
    fn unsupported_backend_is_status_without_a_transfer_invitation() {
        let shared = Shared::<()>::new(31);
        let receiver = Receiver::new(Arc::clone(&shared));
        let token = shared.reserve().unwrap();
        token.control.unsupported.store(true, Ordering::SeqCst);
        assert_eq!(shared.status().phase, Phase::UnsupportedBackend);
        assert!(receiver.poll().is_none());
        drop(token);
        assert_eq!(shared.status().phase, Phase::Revoked);
    }
}
