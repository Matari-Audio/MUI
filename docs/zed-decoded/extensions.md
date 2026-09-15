# Zed Decoded: Life of a Zed Extension → MUI GPUI audio-plugin UI

Research date: 2026-09-13

Primary article: [Life of a Zed Extension: Rust, WIT, Wasm](https://zed.dev/blog/zed-decoded-extensions) — 2024-10-21

Priority: **P0** for the editor/host boundary; **P2** for adopting Wasm (no current need)

This note summarizes the complete article and translates the useful boundary, state, async, capability, and Rust lessons to the existing MUI GPUI plug-in experiment. A Zed extension and an audio plug-in are different products: a Zed extension is third-party editor code downloaded and executed behind a host API; an audio plug-in is already a native binary loaded by a DAW, with a real-time DSP callback and a separate editor UI. The analogy is about ownership and contracts, not a recommendation to put MUI in Wasm.

## What the article establishes (under 180 derived words)

[Life of a Zed Extension](https://zed.dev/blog/zed-decoded-extensions) traces `zed-metals` from Rust source to a call made by Zed. CI compiles an extension ahead of installation and publishes metadata plus an archive; the user downloads the compiled extension, grammar, and query assets.

The author-facing contract is a Rust trait with default methods. Its boundary is a versioned WIT world containing records, resources, and functions. `wit_bindgen` generates guest bindings, while the host uses Wasmtime component bindings; the [pinned WIT world](https://github.com/zed-industries/zed/blob/6341ad2f7ac92c86b1fb3a5e0e01c1758b04cd92/crates/extension_api/wit/since_v0.2.0/extension.wit) gives strings, lists, results, and borrowed resources declared representations.

On install, the [extension store](https://github.com/zed-industries/zed/blob/ea460014ab502bb56515745a733f56246efcc237/crates/extension/src/extension_store.rs) extracts the archive. The [host loader](https://github.com/zed-industries/zed/blob/ea460014ab502bb56515745a733f56246efcc237/crates/extension/src/wasm_host.rs) instantiates a versioned component, initializes it, and serializes calls on an async executor. The article's 2024 API scope was language support, themes, snippets, and slash commands; arbitrary UI, HTTP, and filesystem access were outside that scope. Its practical lesson for MUI is explicit lifecycle and capability ownership, not Wasm adoption.

## Direct primary sources

- [Zed extension WIT world, `extension.wit`](https://github.com/zed-industries/zed/blob/6341ad2f7ac92c86b1fb3a5e0e01c1758b04cd92/crates/extension_api/wit/since_v0.2.0/extension.wit)
- [Zed host loader, `wasm_host.rs`](https://github.com/zed-industries/zed/blob/ea460014ab502bb56515745a733f56246efcc237/crates/extension/src/wasm_host.rs)
- [Zed extension extraction/update path, `extension_store.rs`](https://github.com/zed-industries/zed/blob/ea460014ab502bb56515745a733f56246efcc237/crates/extension/src/extension_store.rs)
- [WebAssembly Component Model](https://component-model.bytecodealliance.org/) and [WIT reference](https://component-model.bytecodealliance.org/design/wit.html)
- [Wasmtime](https://wasmtime.dev/)

## Current MUI shape

The closest existing implementation is the Linux/X11 compatibility probe in [`experiments/gpui-plugin/src/lib.rs`](../../experiments/gpui-plugin/src/lib.rs#L101). It already has the boundary discipline the article is pointing at, using native Rust types:

- `GpuiEditor::open` validates the host window handle, creates a bounded command channel and an edit channel, snapshots the host gain into `Arc<AtomicU64>`, and starts a worker thread running embedded GPUI. The worker creates a child window, reparents it into the host, and sends the child handle back ([`lib.rs`](../../experiments/gpui-plugin/src/lib.rs#L131)).
- UI-to-host traffic is an `Edit` enum (`Begin`, `Value`, `End`). `Editor::idle` drains it on the host side and calls `begin_edit`, `set_param`, and `end_edit` in order. The same idle pass refreshes the atomic host-to-UI gain snapshot ([`lib.rs`](../../experiments/gpui-plugin/src/lib.rs#L63), [`lib.rs`](../../experiments/gpui-plugin/src/lib.rs#L178)).
- Worker state stays with GPUI: `ProbeView` owns the displayed value, text input, scroll handles, drag state, and diagnostics. The worker loop receives `Inspect`, `Resize`, and `Close`, polls the embedded runtime every 8 ms, and applies host value changes only when the view is not being dragged ([`lib.rs`](../../experiments/gpui-plugin/src/lib.rs#L240), [`lib.rs`](../../experiments/gpui-plugin/src/lib.rs#L267)).
- Shutdown is part of the contract. `close` sends `Close`, joins the worker, drains pending edits, and closes an unmatched host automation gesture if the worker failed before sending `End` ([`lib.rs`](../../experiments/gpui-plugin/src/lib.rs#L191)).
- The host bridge is intentionally broader than the probe currently uses: the runnable host supplies parameter edit/get callbacks, formatting, resize, meters, state, and transport hooks. These are internal Rust bridge calls; keep them distinct from the plug-in format's external C ABI ([`experiments/gpui-plugin/src/main.rs`](../../experiments/gpui-plugin/src/main.rs#L15)).
- The real-time side is separate. `ProbePlugin::process` reads the typed `FloatParam` and multiplies audio samples; it does not touch GPUI, channels, window handles, or locks ([`lib.rs`](../../experiments/gpui-plugin/src/lib.rs#L25)).
- Modulation is currently UI-only, as the module comment says. `Routing` owns sources, targets, routes, gestures, hit sites, and animation state in the GPUI application; `connect` rejects invalid/self routes and `overlay` renders the interaction layer. It is not yet the audio engine's modulation graph ([`modulation.rs`](../../experiments/gpui-plugin/src/modulation.rs#L1), [`modulation.rs`](../../experiments/gpui-plugin/src/modulation.rs#L72)).

## Mapping the lessons without conflating the systems

| Boundary concern | Existing MUI analogue | MUI guidance | Priority |
| --- | --- | --- | --- |
| Typed lifecycle | `Editor` lifecycle (`open`, `idle`, `close`, resize) and `PluginContext` | Keep host/UI contracts typed in Rust. Add a method only when a real host feature needs it. | P0 |
| Explicit boundary records | `Edit`, `Command`, `PanelSnapshot`, `ParamId`, `RawWindowHandle` | Keep using enums and structs. Introduce an IDL only for an independently shipped binary boundary. | P0 |
| Single-owner UI state | Per-editor `Session` plus one GPUI worker and command loop | Preserve one owner for GPUI state. Do not share GPUI `Rc` state with the audio or host callback thread. | P0 |
| Capability surface | `PluginContext::bridge()` callbacks and the parent window handle | Treat parameter edits, meters, state, transport, resize, and window parenting as explicit capabilities. Keep them small and thread-checked. | P0 |
| Non-blocking work | Worker `recv_timeout` + `runtime.pump()` and host `Editor::idle` | Keep waits and UI work off the audio callback. For future UI I/O, use the native GPUI/background-executor path and return typed results. | P0 |
| Compatibility | Native plug-in/host API compatibility | Keep MUI's internal Rust contracts separate from the external plug-in C ABI. Add an adapter/version only when multiple host API generations must coexist. | P1 |
| Trust boundary | Native plug-in is trusted code loaded by a DAW | Do not claim sandboxing for native plug-ins. A separate process or Wasm component would be a new product/security decision, not a UI refactor. | P1 |

The most useful MUI rule is therefore simple: the audio callback owns real-time DSP state, the host/editor callback owns DAW automation and host calls, and the GPUI worker owns UI state. `AtomicU64` is a suitable one-value snapshot for the current probe; a larger editor can keep the same direction of flow with a compact typed snapshot and typed edit messages. The existing `Begin`/`Value`/`End` sequence is the right shape for automation and should remain balanced on error and shutdown.

Rust lessons worth carrying forward are small and concrete: use enums and newtypes at boundaries, make invalid transitions impossible or ignored, return `Result` at host/worker edges, keep cleanup symmetrical, and put one runnable contract check behind non-trivial interaction code. MUI already has interaction contracts for routing and editor diagnostics; the missing work is product integration with the eventual audio parameter/modulation model, not a new serialization runtime.

## Zed Decoded tag audit

The [Zed Decoded tag listing](https://zed.dev/blog/tagged/zed-decoded) currently exposes eight article links and no older page of results. Checking the listing and its `?page=2` form produced the same eight entries; there is no separate pagination link. There are **no additional Zed Decoded posts** beyond the eight titles supplied in the task.

Priority below is relevance to this MUI audio-plugin UI work, not a quality ranking.

| Article | Date | URL | MUI priority |
| --- | --- | --- | --- |
| [Async Rust](https://zed.dev/blog/zed-decoded-async-rust) | 2024-04-09 | `zed-decoded-async-rust` | P0 — executor/GUI work must stay away from DSP |
| [Linux when?](https://zed.dev/blog/zed-decoded-linux-when) | 2024-05-07 | `zed-decoded-linux-when` | P1 — platform and native-window constraints |
| [Rope & SumTree](https://zed.dev/blog/zed-decoded-rope-sumtree) | 2024-04-23 | `zed-decoded-rope-sumtree` | P2 — data-structure context |
| [Syntax-Aware Task Spawning With Tree-Sitter](https://zed.dev/blog/zed-decoded-tasks) | 2024-05-21 | `zed-decoded-tasks` | P2 — command/context pattern, no direct audio analogue |
| [Why not just embed Neovim?](https://zed.dev/blog/zed-decoded-vim) | 2024-06-13 | `zed-decoded-vim` | P1 — integration boundary and ownership trade-offs |
| [Text Coordinate Systems](https://zed.dev/blog/zed-decoded-text-coordinate-systems) | 2024-06-27 | `zed-decoded-text-coordinate-systems` | P1 — coordinate conversion and hit-testing discipline |
| [Life of a Zed Extension: Rust, WIT, Wasm](https://zed.dev/blog/zed-decoded-extensions) | 2024-10-21 | `zed-decoded-extensions` | P0 — host/UI contract and capability model |
| [Rope Optimizations, Part 1](https://zed.dev/blog/zed-decoded-rope-optimizations-part-1) | 2024-11-18 | `zed-decoded-rope-optimizations-part-1` | P2 — measured hot-path optimization |

No MUI source was changed by this research note, and no Wasm adoption is proposed. The actionable result is to preserve the existing typed Rust `Editor`/`PluginContext` boundary while keeping it separate from the external plug-in C ABI.
