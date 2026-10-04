# Native accessibility trees

`tree_update(scene, focus, scale)` exports stable surface IDs, authored
parentage, device-scaled bounds, control actions and the current text selection.
`Publisher` emits an empty node update when only focus changes and resets to a
full tree when a reader activates again.

Editable text uses `ResolvedSurface::text_geometry`, so readers receive the
same wrapped lines, bidi direction, character geometry and preedit text that
MUI paints. Each directional visual run is a separate AccessKit `TextRun`.
Convert `SetTextSelection` actions with
`selection_of(scene, request.target_node, &selection)` before queuing a MUI
semantic action: AccessKit positions are run-local, MUI offsets are source
scalars. This also maps transient IME display text back to the source value.

`mui-baseview` attaches AccessKit to its own live NSView/HWND before showing
an editor, including plugin child windows. Linux uses AccessKit's AT-SPI
adapter. Native activation/actions only set atomics or send queued requests;
updates are prepared under the model lock and published after releasing it.
UIA/macOS queued notifications are raised after the adapter borrow ends.
Closing explicitly drops the adapter before the model/native window, avoiding
retained-view cycles and stale action queues on reopening.

The native coordinate/reentry design was checked against GPUI's Apache-2.0
`gpui_macos/src/window.rs` and `gpui_windows/src/window.rs` at
`a84689073d296dfd39987bc7dd478e43ef76d83a`. No GPUI source was copied.
The Windows subclass handles native focus messages; macOS/Windows resolve
screen coordinates using their associated native view/window. Linux still
needs a host-provided screen-position integration for AT-SPI screen bounds.

`cargo test -p mui-access` checks the generated tree with AccessKit's consumer,
including bidi caret bounds, hard/soft line breaks, reversed cross-run
selections, preedit mapping and removal/reactivation. Native VoiceOver/UIA
screen-reader interaction requires macOS/Windows runtime verification.
