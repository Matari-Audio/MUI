# Typed external XDND registration handoff

`mui-winit`'s optional `external-xdnd` feature is disabled by default. On Linux it
adds a narrow handoff from the actual native MUI window to an existing backend
worker. It performs no payload transfer, accepts no drop, sends no Finished and
implements no filesystem operation. `forbid(unsafe_code)` remains in force.

The host uses reviewed winit release commit
`9e75824b11ac05b0d28be772b91e4df25a260708`. Its opt-out suppresses built-in XDND
before parsing, shared state mutation and automatic success replies, including
direct messages that bypass a proxy. Ignoring `DroppedFile` after the callback
would be too late. Normal MUI windows with no configured endpoint retain the
ordinary built-in behavior.

## Dependency boundary

MUI's workspace patch unifies the direct host and AccessKit adapter on one winit
source. A **consumer workspace enabling this feature must mirror the patch**;
dependency workspaces cannot impose their own root patch:

```toml
[dependencies]
mui-winit = { git = "https://github.com/Matari-Audio/MUI", rev = "<reviewed MUI commit>", features = ["external-xdnd"] }

[patch.crates-io]
winit = { git = "https://github.com/DerpcatMusic/winit", rev = "9e75824b11ac05b0d28be772b91e4df25a260708" }
```

Audit the resolved graph, rather than introducing a second Window type:

```sh
cargo tree --locked -p mui-winit --features external-xdnd -i winit
cargo tree --locked -p mui-winit --features external-xdnd -e features -i x11rb
```

The qualified graph excludes **x11rb/tracing**: user tracing/log callbacks inside
native library requests can bypass the server-grab transport deadline. Repeat
this audit in the final consumer's complete graph. Default consumers that do not
enable the feature can continue resolving ordinary registry winit.

## API and lifetime

`external_xdnd::channel()` creates `(Host, Worker)` or reports checked owner-ID
exhaustion. It starts no thread and performs no native I/O. Put `Some(host)` in
`Options::external_xdnd`, and move the non-clone, non-Sync Worker to the existing
backend thread. Options/Host cloning is configuration sharing, not permission to
attach a pair to two concurrent windows. Busy, closed and exhausted configuration
is rejected before native creation.

The actual host opts out **before** creating its X11 window. It obtains winit's
memory-only typed capability, retains the host lease, and offers one Invitation
only after renderer initialization. `Worker::poll()` takes the bounded latest
invitation. `Invitation::id()` is an opaque process owner plus checked generation;
`sequence()` is display-only. Keep the full ID with asynchronous callbacks and
reject stale IDs with `Worker::is_current`.

The invitation exposes only bounded `Challenge` metadata, including the original
connection's captured address, actual target/root and creation/registration
proofs. It exposes no raw pointer, Window callback, connection or property API.
The backend can create a self-referencing proxy on its existing protocol
connection and use `Invitation::connect()` on its backend thread. The returned
winit Registration is thread-affine and cannot return to the UI thread. It
validates the real native target/proxy before publication; the backend must retain
its proxy connection until matching detach is established.

The mailbox has one pending item, one active small control and one retired small
control for cleanup status. It owns no active native connection, file catalog or
payload buffer. Pending tokens are discarded outside the mailbox mutex and do
not perform native I/O. Drop/close/suspend/native recreation revoke the complete
matching generation **before GPU/window teardown**. Backend receiver Drop also
revokes. None of those host operations joins a worker or waits for native cleanup.
An old guard, lease or ID cannot clear or revoke its replacement.

`Host::status()` and `Worker::status()` read bounded memory only. Registered means
the last native install was acknowledged; it is not a new server query, transfer
admission or transaction success. Logical revoke is immediate. Already queued
publication can still arrive; native cleanup remains cooperative and can report
uncertainty or superseded properties. A retained retired control keeps that native
fact observable without an unbounded retirement queue. Each invitation is
one-shot; recreating the native host produces a fresh generation.

The native primitive uses a private timed connection for rare install/cleanup
server-grab transactions. A server grab can stall other X clients. Fixed native
steps check one 500 ms deadline on all transport paths, and traffic is capped for
the complete private connection lifetime (1 MiB/256 packets). Timeout/panic closes
the unique descriptor without protocol-lock cleanup. Process/OS/server suspension
still prevents an absolute desktop-latency guarantee. Public X11 properties catch
accidental wrong-server/recycled-ID errors; they do not authenticate hostile X
clients that can read/write properties.

## Unsupported backends

On Linux Wayland the same option leaves the normal application window usable,
reports `UnsupportedBackend` through status and offers no X11 invitation. It does
not launch XWayland or advertise a fake target. Native Wayland needs a separate
data-device/surface capability and is not implemented here. This API is currently
Linux-only; Windows/macOS native transfer integration is not provided.

## Local qualification

```sh
cargo test --locked -p mui-winit --features external-xdnd --lib external_xdnd
cargo clippy --locked -p mui-winit --features external-xdnd --all-targets -- -D warnings
timeout --kill-after=5s 90s env -u WAYLAND_DISPLAY -u WAYLAND_SOCKET MUI_RENDERER=cpu \
  xvfb-run -a cargo run --locked -p mui-winit --features external-xdnd --example external_xdnd
```

The owned-native fixture requires two actual MUI windows to present and register,
close and recreate. It checks distinct full IDs, old-ID rejection and logical
revoke before the close observer. The worker retains the proxy until clean/gone
native teardown. An anchor window keeps the event loop alive across recreation;
no independent proxy is claimed to be an application window. No payload is sent.

The same example can run under an owned native Wayland compositor with DISPLAY
unset and `MUI_XDND_EXPECT_UNSUPPORTED=1`. Both ordinary host windows must present,
close and recreate while status is unsupported and the invitation mailbox empty.
The compositor must be owned by the fixture, with a private dev-drive runtime
directory; never replace the user's compositor or change global environment
inside the application process.

These checks qualify the handoff/lifetime boundary. The application backend still
needs bounded selection/INCR transport, native path decoding, operation admission,
cancellation and verified transaction receipts before it reports native success.
