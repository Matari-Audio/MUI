# MUI pixels in a native GPUI window

Source prototype, 2026-10-08. **Hosted compilation and native validation are
pending.** Static checks pass; no benefit or native-device compatibility
result has been established.

This isolated package uses GPUI's stock native platform and public `RenderImage`
API to display the complete output of MUI's existing Vello CPU renderer. It
directly includes the existing `mui-preview/src/scenes.rs` gallery fixtures,
so text, image fitting, clipping, gradients, shadows, material geometry and
gesture behavior come from the same scene implementations as the gallery.
The `material` selector includes the existing `material_welding.rs`
`tree(Weld::all())` regression fixture rather than approximating its output.
There is no GPUI scene conversion or reduced rectangle-only reconstruction.

This is a **standalone window**, including native Wayland when available.
GPUI's public window API cannot create a child under a DAW-supplied
HWND/NSView/X11/wl_surface. See the
[contract audit](../../research/gpui-vello-integration.md#native-host-contract-audit-2026-10-08)
before treating any screenshot as plugin embedding evidence.

Run on each native platform:

```sh
cargo test --locked --manifest-path tools/gpui-host-probe/Cargo.toml
cargo run --release --locked --manifest-path tools/gpui-host-probe/Cargo.toml -- editor
cargo run --release --locked --manifest-path tools/gpui-host-probe/Cargo.toml -- effects
cargo run --release --locked --manifest-path tools/gpui-host-probe/Cargo.toml -- material
cargo run --release --locked --manifest-path tools/gpui-host-probe/Cargo.toml -- images
cargo run --release --locked --manifest-path tools/gpui-host-probe/Cargo.toml -- gestures
cargo run --release --locked --manifest-path tools/gpui-host-probe/Cargo.toml -- --test-ui=material
```

Keep this experimental package's lockfile with measurement artifacts when comparing runs.
GPUI is pinned to Zed revision
`f1a10a5227a331e86bdb006301e1090a21d33e7a`. The package is its own workspace
and adds no GPUI dependencies or default backend features to production MUI.

`--test-ui` or `--test-ui=<scene>` runs bounded scripted checks: the selected
initial fixture, an ordered pointer gesture that must alter CPU pixels, an
image fixture, and a native resize that must change the raster extent. It
exits after a completion summary or fails after 30 seconds/1800 callbacks.
The deadline requires a responsive native loop; the external collector must
also enforce a process timeout for driver hangs. Synthetic scene input does
not establish OS input, focus or IME correctness.

Set `MUI_TEST_RESULTS` to a fresh results directory for flushed
`gpui-events.jsonl` evidence. In Linux X11 CI, `MUI_TEST_CAPTURE=1` uses the
existing collector's `capture-<case>.json`/`.ack` protocol at five milestones.
The native window is titled `MUI tester`, and every stage is its entire client
pixel extent. Each request identifies a CPU reference PNG and its dimensions;
the reference is saved before publishing the request. It retains premultiplied
RGBA, so comparison uses opaque pixels (alpha exactly 255) and tolerates four
levels of channel error rather than treating transparency/AA edges as parity
evidence. The probe waits for each screenshot acknowledgement before advancing.
Native Wayland capture needs a compositor-specific collector rather than
xdotool.
Windows/macOS without capture acknowledgements report callback evidence only.

Resize the editor at 100% and 200% display scale, including narrow/tall and
wide/short windows. Compare native screenshots against the same MUI gallery
fixtures. In `gestures`, drag the cutoff and gain, hold Shift for fine travel,
right-click to reset, and drag a wave into a slot. Pointer button edges are
queued and consumed in order, even when several arrive before a native redraw.
Keyboard navigation keys are forwarded; this probe does not implement text
entry/IME, clipboard, accessibility or host automation.

Changed scenes repaint completely on one CPU thread, copy/convert their
premultiplied RGBA pixels into GPUI's straight BGRA image format, and upload
through GPUI's image atlas. Superseded atlas entries are removed with
`Window::drop_image`; unchanged raster output retains the same image identity.
The renderer enforces its existing 64 MiB pixel-target limit. Native scale and
viewport dimensions determine the raster extent. An unsupported scene or
extent reports an error instead of silently dropping paints.

JSON lines on stderr report scene resolve time, CPU raster time, CPU image
preparation time, physical extent, image payload size, process RSS on Linux,
and startup elapsed time. The next-frame callback interval is scheduling
evidence; **it is not GPU completion, upload duration, or presentation latency**.
The summary always sets `native_gpu_submission_verified=false`; screenshot
acknowledgements are counted separately. `run_end` follows return from GPUI's
native application loop and failures return a nonzero exit status.
GPU specs are available from GPUI on Linux and absent from that API on Windows
and macOS. Driver capture is needed to measure GPU memory and transfer timing.

Full-frame transfer payload is `4 * physical_width * physical_height` bytes
per changed image: 1200×800 is 3.84 MB; at 200% it is 15.36 MB. At 60 changed
frames/s those payloads alone are 230.4 and 921.6 MB/s. These are byte counts,
not measured bandwidth. Atlas allocation, staging, CPU scratch and driver
memory are additional. Log output also affects timings; use identical logging
and release builds in the comparison.

On Windows this exercises GPUI's D3D11 image renderer; on macOS its Metal image
renderer; on Linux its wgpu backend. MUI's wgpu 30 types never cross into
GPUI's wgpu 29 renderer: the only bridge is CPU pixels. This can investigate
the native backend's behavior on older GPUs, but it still requires GPUI's GPU
device and does not provide a GPU-free fallback.

No production migration is justified until the native screenshot, input,
resize, sustained allocation and device-loss checks run successfully on the
affected hardware and beat the existing host on the same workloads. Plugin
parenting and multi-editor unload safety require a separate custom-platform
implementation; this probe does not satisfy those gates.
