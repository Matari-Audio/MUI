# Standalone native MUI

`cargo run -p mui-winit --example standalone` opens a native top-level window.
Click the text field to compose through the operating system's input method;
copy and paste use the system clipboard. The example uses a bundled font.

```rust,no_run
use mui_winit::{Options, View};

fn launch(view: impl View, ui: mui::Ui) -> Result<(), String> {
    mui_winit::run(view, ui, Options::default())
}
```

Call `run` on the application's main thread. `run_shared` accepts the same
`Arc<Mutex<mui::host::Shared<V>>>` used by the other native hosts. Its model is
polled at the current monitor refresh rate, or `Options::poll_interval`. The
shared driver skips idle builds; native redraws and repaint deadlines wake the
window, and animations are limited to the current display cadence. Zero-sized
surfaces and occluded windows stop drawing. Focus loss delivers cancellation
before further presentation; closing cancels and releases the native resources.

Winit selects native Wayland or X11 on Linux and the native Windows/macOS
backend. Scale changes update the physical surface and scene scale together.
Monitor moves update the polling cadence. AccessKit publishes the resolved
semantic tree and routes actions to the requested control, including text
selection. IME commits and preedit are queued independently of pointer samples;
the native candidate rectangle follows the focused field's caret.

Linux clipboard support enables arboard's Wayland data-control feature as well
as X11. A compositor must expose the data-control protocol for that path;
arboard can fall back to X11. When a system clipboard cannot be opened or read,
the host retains the application's last copied text in memory.

`Options::motion_policy` supports an explicit reduced-motion override. Winit
has no portable native reduced-motion preference API; `System` uses the UI's
system preference state, which the embedding application can update through
`Ui::set_system_reduced_motion`.

Set `Options::profile_output` to a CSV path to enable the shared driver's
bounded CPU timings and work counters, exported when the window closes. The
backend timing covers the CPU call that encodes, submits and presents; it does
not measure GPU completion, scanout or input-to-photon latency.

## Plugin window contract

This host owns an application's event loop and top-level window. Continue to
use `mui-baseview::open` (and `mui-truce::MuiEditor`) for foreign-parent CLAP/VST3
editors. On Linux that embedding accepts an X11 parent, including an XWayland
X11 parent. A native Wayland DAW surface is not an X11 parent handle: standalone
Wayland support does not establish native Wayland plugin embedding.

`Gpu` is the gallery's original adapter factored into this crate. Applications
with their own winit event loop can use its fallible `try_new`, resize and
present operations; `mui-preview` uses the same adapter.
