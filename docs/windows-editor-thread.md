# Windows embedded editor threading

## Decision (2026-10-17)

Keep the child HWND, its input queue, native accessibility provider, IME and
host-facing resize/focus operations on the host UI thread. Move model polling,
input dispatch, tree building, layout, scene snapshots, shader compilation,
GPU submission/readback and CPU rasterization to one editor worker.

A worker-created child HWND is rejected: cross-thread parent/child relationships
attach input queues, and synchronous window messages can propagate a hung
window thread into the DAW. A worker must never own or pump a window.

A native swapchain on the host-owned HWND also needs a native lifetime lease.
A host can destroy its parent (and thus the child) while a graphics driver is
stalled; waiting for that lease in WM_DESTROY would recreate the freeze.
Instead use a **window-independent GPU target** and read completed pixels back
on the worker, followed by a bounded native bitmap blit. CPU fallback uses the
same handoff. This is deliberately a variation of the input-window/render-worker
option, rather than a cross-thread HWND or DXGI swapchain lifetime gamble.
GPU readback bandwidth is the main cost and must be measured before release.
A future DirectComposition transport can replace the bitmap handoff only with
an equally nonblocking parent-destruction contract.

## Invariants

* No model/UI lock, GPU call, raster or worker join in a native callback.
* Worker closures contain no WindowContext, HWND, native provider or borrowed
  plugin instance. The host owns all window APIs, including set_size and close.
* Input and size/DPI edges cross one FIFO; only adjacent hover moves may merge.
  Button, key, wheel, focus, IME, drop and resize edges are barriers.
* Native keyboard capture follows the last completed UI frame, as before; the
  worker runs key hooks. Hosts' accelerator handling and fast click-to-type
  sequences require explicit Windows validation.
* Native focus, capture, cursor, IME and accessibility updates are applied after
  the worker releases its model lock. Accessibility actions use the existing
  portable endpoint and are processed by the worker.
* Close invalidates publication and drops the native endpoint immediately.
  Cancellation runs on the worker. No native lifetime is extended by rendering.
  A pinned image lets a stalled worker finish safely after editor destruction.
  No worker is started if the image cannot be pinned.
* Only admitted pinned sessions are retained process-wide. The image remains
  mapped until process exit, so device callbacks and pipeline destructors cannot
  execute unmapped plugin code. This intentionally prevents physical DLL unload;
  it does not retain plugin instances or HWNDs.
* Frame work has a configurable 16 ms floor (minimum 8 ms); Windows due/input
  wakes also go through compositor pacing. CPU rasterization has a 100 ms floor.

## Validation required

On REAPER, Bitwig, Ableton and FL Studio, test both VST3 and CLAP: sustained XY
and resize drags, mixed key down/up and text/IME composition, host shortcuts,
focus loss, DPI changes, accessibility activation/actions, drag/drop, multiple
instances, close/reopen, parent destruction, device loss and plugin unload.
Inject a long View::build and a stalled GPU completion: DAW transport, other
windows and native input must continue, and close must return promptly.
Measure readback/blit time and worker frame latency at 1280x800 and 4K. Bitmap
allocation and copying are bounded to 16M pixels; extreme sizes retain pixels.

## References

* [Microsoft: cross-thread window relationships attach input queues](https://devblogs.microsoft.com/oldnewthing/20130619-00/?p=4043).
* [Microsoft: SendMessage blocks the sender until the receiver processes it](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-sendmessage).
* [Microsoft: DestroyWindow destroys child windows](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-destroywindow).
* [wgpu 30 DX12 compiler policy](https://wgpu.rs/doc/src/wgpu_types/backend.rs.html).
