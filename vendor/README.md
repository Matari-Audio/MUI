# Vendored forks

Every crate here is a patched copy MUI ships on purpose. None can go back to
crates.io until its patch is upstream (or upstream fixes the same bug); each
row says what to watch for.

| Crate | Base | Why it is patched | Drop when |
|---|---|---|---|
| `moose-baseview` | RustAudio/baseview 0.4.1 (`8c6e6ba`) | MUI's owned window layer: plugin-lifetime, IME and host fixes, frame pacing, damage, timers, desktop dock surfaces. Diverged API; see `UPSTREAM.md` there. | Not planned: this is MUI's windowing, not a temporary patch. |
| `xim-rs` | zed-xim 0.4.0 | X11 input method: keys serialized through protocol acknowledgements so typed text is not reordered or lost (`3d713d7c`). | zed-xim or xim-rs takes acknowledgement-ordered key forwarding. |
| `accesskit_macos` | 0.26.3 | ObjC classes named per binary image, so two MUI plugins in one host process don't collide on `AccessKitSubclassOf…` (`fd8a83c5`); `image-probe` feature for the test. | AccessKit names its runtime classes per image (or stops subclassing). |
| `accesskit_unix` | 0.22.1 | Provider worker threads owned and joined on drop, so unloading a plugin leaves no thread running in unmapped code (`78a80acc`; check: `tools/ci/check-linux-accesskit-images.py`). | AccessKit joins its Unix workers on adapter drop. |
| `accesskit_winit` | 0.33.2 | Unpatched; path-depends on the two forks above so they are the ones linked. | Both of those are dropped. |
| `vello_cpu` | 0.3.0 | Single-threaded dirty-region render: the existing nonzero-origin bucketer exposed for `RasterizerSettings::viewport` (`MUI-PATCHES.md` there). | vello_cpu gains a viewport/dirty-rect render. |

Upstreaming the four small patches (xim, the two AccessKit ones, vello_cpu)
is what shrinks this directory; MUI's tests for each stay as the regression
check after the swap.
