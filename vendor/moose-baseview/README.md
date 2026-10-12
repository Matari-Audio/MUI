# moose-baseview

MOOSE's vendored copy of [baseview](https://github.com/RustAudio/baseview), the
low-level windowing library for audio plugin UIs.

- Derived API/behavior baseline: RustAudio/baseview **v0.4.1**, exact commit
  `8c6e6bab4b3c10f34b23dc12e878814349578777`, forward-integrated into the
  owned native fork rather than replacing its lifetime, IME and host fixes.
  [UPSTREAM.md](UPSTREAM.md) classifies every change since the original
  0.3.4 base `bc987b96c22cbdb36507196dbbcad00c768e135b`, including deliberate
  differences in pacing, timers, damage and optional GL backends.
- Package version: `0.4.1-moose.1`; this identifies the derived baseline and
  downstream fork, and does not claim stock backend parity.
- License: unchanged, MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`).
- Legacy `on_frame` implementations remain source compatible. Current
  `draw`, `poll`, `damage`, timer, waker and sizing APIs are additive.
  Depend on it under the upstream name:

  ```toml
  baseview = { package = "moose-baseview", path = "vendor/moose-baseview" }
  ```

## MOOSE additions (vs upstream 0.3.4)

All additive; nothing upstream exposes was renamed or removed.

API:

- `WindowSettings::scale_factor_override` / `with_scale_factor_override`,
  `Window::set_scale_factor_override`, `WindowContext::set_scale_factor_override`:
  a host-provided scale that wins over the OS scale on Windows and X11 (no-op
  on macOS). It does not resize the window; call `resize(logical)` after it.
- `WindowEvent::ScaleFactorChanged(f64)`: the platform scale changed.
- `Window::set_keyboard_capture` / `WindowContext::set_keyboard_capture`
  (KURV K23): Windows only; with capture off, keys stay in the host's pump.
- `pin_current_image_for_detached_work()` (KURV K24/K27): pins the plugin
  binary so a detached render thread can never outlive its code.

Behaviour:

- Windows: plugins never set process DPI awareness; the per-thread DPI guard
  restores the host thread's context; `ProcessDpiAwareness` mapping fixed.
  Child windows handle `WM_DPICHANGED_AFTERPARENT` (keep the logical size,
  emit `ScaleFactorChanged`); top-level windows emit it on `WM_DPICHANGED`.
  With an override, hit-testing and sizes use the override consistently.
- Windows: `OleInitialize` balanced with `OleUninitialize`; an MTA host
  thread disables drag and drop instead of failing window creation. System
  DLLs load with `LOAD_LIBRARY_SEARCH_SYSTEM32`.
- X11: core auto-repeat pairs become one repeated key-down (MOOSE X01);
  ignored key events are forwarded to the embed parent, with synthesized
  key-ups on focus loss and close (KURV K26); `Xft.dpi` clamped to 0.5-4
  (KURV K25); override and fallback scale honoured at creation.
- macOS: the backing scale is re-read when the view moves into a window, so a
  view created before the host attached it gets its real scale.

The subsequent MUI native lifetime, bounded-close, IME, drag-and-drop and
frame-pacing patches are documented in [README-MUI.md](README-MUI.md).

`rustfmt.toml` carries upstream's formatting settings so the workspace
`cargo fmt --check` leaves these sources in upstream style.
