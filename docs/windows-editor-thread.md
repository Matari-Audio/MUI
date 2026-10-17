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
* There are no frame caps or CPU throttles. Due/input/animation wakes pass
  through compositor pacing on the existing pacer thread; no DwmFlush or GPU
  wait happens on the host. Unchanged paint emits no bitmap and no native blit.

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

## Damage tracking

`mui-vello/src/damage.rs` is the shared classifier extracted from the existing
retained GPU renderer. It compares the old and new `Painted` streams by stable
node key and layer, with a bounded lookahead for insertions/deletions. Exact
unchanged entries do not invalidate pixels. Changed entries contribute **both**
the old and new paint footprints, including transforms, stroke, text estimates,
shadow reach and antialiasing padding. Device bounds snap to 16-pixel tiles.
Overlapping/touching rectangles merge; sparse regions stay separate, with a
bounded two-cluster coalescing step. The classifier falls back to full repaint
on rectangle-budget overflow or damage above half the target, rather than
spending more on scene replay and copies than a full repaint.

Analytic welds already retain material textures and changed uniform lanes.
`GpuRenderer` adds the old/new bounds of a changed or moved external weld to
ordinary paint damage, including time-driven materials. Only affected regions
are rasterized into the existing patch/atlas targets, then copied into the
retained scene texture. The Windows offscreen output pass uses scissor rectangles
and LoadOp::Load; readback copies and returned bitmap patches contain only those
rectangles. A persistent output view is essential: swapping it invalidates the
whole output. An unchanged render produces no readback and no blit.

CPU fallback resolves reference welds, whose changed geometry enters the same
paint stream. `software::Renderer::render_damage` replays intersecting paint in
stacking order into region-sized CPU targets. It must include unchanged
backgrounds, overlapping siblings, masks/clips and blend layers to reconstruct
the affected pixels correctly. Only those rows replace retained pixels; only
those native rectangles are blitted. Full-window CPU rasterization is limited
to actual full invalidations and happens entirely on the worker, without a
frame-rate throttle. The existing full-raster API remains the default elsewhere.

Resize/DPI/zoom transforms invalidate the full target. Theme changes enter the
paint diff (a changed full-window background naturally invalidates everything).
Backdrop sampling, unbounded effects, unsupported partial shadow dependencies,
external texture mapping changes, renderer handover and transport resynchronizing
use conservative full invalidation. Native WM_PAINT damage repairs the retained
bitmap's exposed rectangles without rebuilding or rerasterizing the UI.

## Lifecycle and integration

The Windows `mui-truce` adapter attaches a host-thread pump before starting its
worker. Parameter gestures and resize requests enqueue there; native delivery
runs outside the model lock. Close revokes the pump first, then cancels the
native editor; worker cancellation releases UI memo TLS and model references.
State-load cancellation is queued on the worker. A new worker waits for its
predecessor's model cleanup on the worker thread only. A permanently wedged
predecessor therefore delays rendering that editor's reopen, while the DAW and
its other windows keep running. It is not safe for custom adapters to lock
`Shared` or call host callbacks directly from worker build/key hooks; use the
native pump and `Requests::dispatch_render_work` for such integrations.

The output mailbox has one slot, so a paused host cannot accumulate completed
bitmaps. Ordered input uses a separate FIFO and never waits for rendering.
Polling View::changed is still supported: an idle model poll does not build a
tree, raster, submit GPU work, read back pixels or blit. Push-based integrations
can request redraw when model state changes. This separates model polling from
actual rendered frames; there is no unconditional animation/presentation loop.

## Release note

Windows baseview editors now render off the DAW UI thread, with display-paced
input/animation and incremental CPU/GPU bitmap presentation. GPU sessions survive
editor reopen; recoverable runtime faults retry, Vulkan is preferred within a
device class, and GNU DX12 discovers DXC beside the plugin or through
`MUI_DXCOMPILER_PATH` (a DLL file or containing directory), otherwise using FXC.
`WGPU_DX12_COMPILER` remains an explicit override. No versions or dependency pins
are changed. Linux/macOS keep their existing native rendering paths.
