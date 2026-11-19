//! CLAP `wayland-surface-embedding-v1`: an editor on its own connection to a
//! host's nested compositor, placed by token. The compositor here is a fake
//! with just the globals the editor binds, on a socketpair.
#![cfg(target_os = "linux")]

use moose_baseview::{
    EmbeddedWaylandWindow, Event, EventStatus, HandlerError, WindowHandler, WindowSettings,
    WindowSize,
};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use wayland_backend::protocol::{Argument, Message};
use wayland_backend::server::{
    Backend, ClientData, ClientId, GlobalHandler, GlobalId, Handle, ObjectData, ObjectId,
};

#[allow(non_upper_case_globals, clippy::all, reason = "wayland-scanner output")]
mod interfaces {
    use wayland_client::protocol::__interfaces::*;
    use wayland_protocols::xdg::shell::client::__interfaces::*;
    wayland_scanner::generate_interfaces!("protocols/clap-surface-embedding-v1.xml");
}

const TOKEN: &str = "slot-1";
const CLOSED: u16 = 1;

/// What the compositor saw, shared with the test.
#[derive(Default)]
struct Seen {
    tokens: Vec<String>,
    /// Embedded surfaces whose `closed` is not sent yet.
    refused: Vec<ObjectId>,
}

struct Ignore;
impl ObjectData<()> for Ignore {
    fn request(
        self: Arc<Self>, _: &Handle, _: &mut (), _: ClientId,
        msg: Message<ObjectId, std::os::fd::OwnedFd>,
    ) -> Option<Arc<dyn ObjectData<()>>> {
        let creates = msg.args.iter().any(|arg| matches!(arg, Argument::NewId(_)));
        creates.then(|| Arc::new(Ignore) as Arc<dyn ObjectData<()>>)
    }
    fn destroyed(self: Arc<Self>, _: &Handle, _: &mut (), _: ClientId, _: ObjectId) {}
}

/// `clap_surface_embedding_v1`: records each token and refuses unknown ones.
struct Embedder(Arc<Mutex<Seen>>);
impl ObjectData<()> for Embedder {
    fn request(
        self: Arc<Self>, _: &Handle, _: &mut (), _: ClientId,
        msg: Message<ObjectId, std::os::fd::OwnedFd>,
    ) -> Option<Arc<dyn ObjectData<()>>> {
        // get_embedded_surface(new_id, surface, token); destroy has no args.
        let [Argument::NewId(id), _, Argument::Str(Some(token))] = msg.args.as_slice() else {
            return None;
        };
        let token = token.to_string_lossy().into_owned();
        let mut seen = self.0.lock().expect("lock poisoned");
        if token != TOKEN {
            seen.refused.push(id.clone());
        }
        seen.tokens.push(token);
        Some(Arc::new(Ignore))
    }
    fn destroyed(self: Arc<Self>, _: &Handle, _: &mut (), _: ClientId, _: ObjectId) {}
}

struct Global(Option<Arc<Mutex<Seen>>>);
impl GlobalHandler<()> for Global {
    fn bind(
        self: Arc<Self>, _: &Handle, _: &mut (), _: ClientId, _: GlobalId, _: ObjectId,
    ) -> Arc<dyn ObjectData<()>> {
        match &self.0 {
            Some(seen) => Arc::new(Embedder(Arc::clone(seen))),
            None => Arc::new(Ignore),
        }
    }
}

struct Client;
impl ClientData for Client {}

struct Compositor {
    seen: Arc<Mutex<Seen>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}
impl Compositor {
    /// Serves one client on the returned socket; `embedding` offers the global.
    fn start(embedding: bool) -> (Self, UnixStream) {
        use wayland_client::protocol::__interfaces::*;
        let (ours, theirs) = UnixStream::pair().expect("socketpair");
        let seen = Arc::new(Mutex::new(Seen::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let (seen, stop) = (Arc::clone(&seen), Arc::clone(&stop));
            std::thread::spawn(move || {
                let mut backend = Backend::<()>::new().expect("server backend");
                let handle = backend.handle();
                handle.create_global(&WL_COMPOSITOR_INTERFACE, 6, Arc::new(Global(None)));
                handle.create_global(&WL_SHM_INTERFACE, 1, Arc::new(Global(None)));
                handle.create_global(&WL_DATA_DEVICE_MANAGER_INTERFACE, 3, Arc::new(Global(None)));
                if embedding {
                    handle.create_global(
                        &interfaces::CLAP_SURFACE_EMBEDDING_V1_INTERFACE,
                        1,
                        Arc::new(Global(Some(Arc::clone(&seen)))),
                    );
                }
                backend.handle().insert_client(ours, Arc::new(Client)).expect("client");
                while !stop.load(Ordering::Acquire) {
                    let _ = backend.dispatch_all_clients(&mut ());
                    let refused = std::mem::take(&mut seen.lock().expect("lock poisoned").refused);
                    for id in refused {
                        let closed =
                            Message { sender_id: id, opcode: CLOSED, args: Default::default() };
                        let _ = backend.handle().send_event(closed);
                    }
                    let _ = backend.flush(None);
                    std::thread::sleep(Duration::from_millis(1));
                }
            })
        };
        (Self { seen, stop, thread: Some(thread) }, theirs)
    }
    fn tokens(&self) -> Vec<String> {
        self.seen.lock().expect("lock poisoned").tokens.clone()
    }
}
impl Drop for Compositor {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            thread.join().expect("compositor thread");
        }
    }
}

struct Idle;
impl WindowHandler for Idle {
    fn resized(&self, _: WindowSize) -> Result<(), HandlerError> {
        Ok(())
    }
    fn on_event(&self, _: Event) -> EventStatus {
        EventStatus::Ignored
    }
}

fn open(socket: UnixStream, token: &str) -> Result<EmbeddedWaylandWindow, moose_baseview::Error> {
    EmbeddedWaylandWindow::open_nested(WindowSettings::new(), socket.into(), token, |_| Ok(Idle))
}

/// Pumps `window` as a host would, until `done` or a second passes.
fn pump_until(window: &mut EmbeddedWaylandWindow, done: impl Fn(&EmbeddedWaylandWindow) -> bool) {
    let deadline = Instant::now().checked_add(Duration::from_secs(1)).expect("deadline");
    while !done(window) && Instant::now() < deadline {
        window.dispatch().expect("dispatch");
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn embeds_with_the_hosts_token() {
    let (compositor, socket) = Compositor::start(true);
    let mut window = open(socket, TOKEN).expect("open");
    pump_until(&mut window, |_| false);
    assert!(window.is_open());
    assert_eq!(compositor.tokens(), [TOKEN]);
}

#[test]
fn a_refused_token_closes_the_window() {
    let (compositor, socket) = Compositor::start(true);
    let mut window = open(socket, "stale").expect("open");
    pump_until(&mut window, |w| !w.is_open());
    assert!(!window.is_open());
    assert_eq!(compositor.tokens(), ["stale"]);
}

#[test]
fn a_token_with_a_nul_fails_open() {
    let (compositor, socket) = Compositor::start(true);
    assert!(open(socket, "slot\0").is_err());
    assert!(compositor.tokens().is_empty());
}

#[test]
fn a_compositor_without_embedding_fails_open() {
    let (_compositor, socket) = Compositor::start(false);
    assert!(open(socket, TOKEN).is_err());
}
