# mui-native-dialog

An optional Linux FileChooser portal client. Enable `xdg-portal` on Linux and
keep the existing native Windows/macOS dialog implementation. The desktop portal
selects the installed GNOME/KDE/other backend; this crate does not change desktop
configuration or start a GUI toolkit.

## Ownership

Keep a `DialogService` in the plugin/application lifetime owner, outside any
editor window. `service.spawn(DialogRequest { .. })` returns a `DialogJob` with
one result receiver. Poll `job.try_result()` from the GUI and perform file I/O in
the caller. `Ok(None)` is user cancellation; `Err(String)` reports missing or
failed portals, unsupported folder selection, and invalid responses.

Before destroying an editor's native parent, cancel that editor's job or its
cheap cloned `job.cancel_handle()`. Both cancellation and `DialogJob::drop` are
nonblocking. `CancelHandle` is `Clone + Send + Sync`, suitable for native close
callbacks that cannot borrow the application model; dropping the handle itself
does not cancel. Cancel the previous job when replacing a pending request.
Locally cancelled jobs suppress results. `try_result() == None` can mean pending
or locally cancelled: worker consumers must also check `is_cancelled()`;
`is_finished()` observes completed runtime cleanup. Never wait for a result that
local cancellation deliberately suppresses.

At final plugin teardown, call `service.shutdown()` **before unloading plugin
code**, even if callbacks retain `Arc<DialogService>` clones. Shutdown closes the
service to new requests, cancels active jobs and joins all its workers without
holding the request registry lock. It is idempotent; concurrent shutdown calls
wait for the drain. Service Drop is a fallback. Do not call shutdown from GUI
close callbacks. Caller-owned filesystem/processing workers remain caller-owned
and must also be joined before code unload.

Every request uses a dedicated D-Bus connection and an owned current-thread
Tokio runtime. The runtime is dropped before its worker exits, joining any
runtime-owned blocking work. This avoids async-io's permanent global reactor
thread in the dialog implementation. Five-second deadlines apply to connection
setup, the portal method handshake, Request.Close and disconnection, **never** to
user selection. Cancellation during a started handshake allows its bounded
method reply to reveal the actual request path before closing it. A stalled
handshake falls back to the protocol-derived path, then disconnects the private
connection. Request.Close need not emit Response. These are I/O deadlines, not a
hard bound on OS scheduling or uncancellable filesystem/runtime shutdown work.

## Requests

`Parent::X11(u32)` accepts a copied nonzero XID captured on the GUI thread.
`Parent::Wayland(String)` accepts an **exported xdg-foreign handle**, not a raw
surface pointer. The GUI owner retains that export until the request finishes or
is cancelled. `None` is an explicitly unparented dialog; this crate does not
infer native window identities. Cancelling before parent destruction is required
because an owned identifier does not prove the parent is still alive.

`DialogKind` supports OpenFile and PickFolder with optional multiple selection,
and SaveFile with an optional suggested file name. Folder selection requires
FileChooser version 3 or newer and errors on older backends. `directory` is a
native path encoded as NUL-terminated bytes; embedded NUL bytes are errors.
`DialogFilter` takes extensions without leading dots (`wav`, `flac`), or `*`;
empty extension lists are omitted. These become portal glob filters and are not
MIME validation or a promise of filesystem permissions. Successful responses
must contain local file URIs and a valid path count. The caller performs its
usual read/write and overwrite checks.

Response signals are subscribed before opening the chooser, buffered until the
method returns its actual request path, and filtered by that path and responder.
The client handles early responses and legacy paths without taking an answer
from another request. Response 1 means cancellation; response 2 is an error.
Backend owner loss is reported separately from cancellation.

## Verification

Linux tests require `dbus-daemon` and run a private fake portal with no service
activation directories. They cover parent/options, early and unrelated
responses, legacy request paths, cancellation/Close with no Response, service
teardown, missing/disappearing portals and malformed/non-file results. They do
not test GNOME/KDE visual appearance or establish a performance benchmark.

```sh
cargo test -p mui-native-dialog --features xdg-portal --locked
cargo clippy -p mui-native-dialog --all-targets --all-features --locked -- -D warnings
```

Protocol references: [FileChooser](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.FileChooser.html),
[Request](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.Request.html),
and [desktop backend selection](https://flatpak.github.io/xdg-desktop-portal/docs/portals.conf.html).
