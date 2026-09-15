# Zed Decoded → MUI integration index

Reviewed 2026-09-13 by exactly five `gpt-5.6-luna` agents with `xhigh`
reasoning. The [tag listing](https://zed.dev/blog/tagged/zed-decoded) exposes eight
articles; all eight are covered below. Reports distinguish historical posts,
independently inspected primary implementation sources, and MUI recommendations.
They summarize the technical material rather than reproduce the articles.

| Agent report | Articles covered | Useful boundary for MUI |
| --- | --- | --- |
| [Async Rust](async.md) | Async Rust | Foreground/background ownership, cancellation, bounded work, stale-result rejection and deterministic scheduling tests |
| [Platform and embedding](platform-and-embedding.md) | Linux when?; Why not just embed Neovim? | Native platform contracts, event pumping, one authoritative model, external behavioral test oracles |
| [Rope and performance](rope-and-performance.md) | Rope & SumTree; Rope Optimizations, Part 1 | Immutable snapshots, summaries and measured optimization; avoid adopting editor data structures without a workload |
| [Coordinates and tasks](coordinates-and-tasks.md) | Text Coordinate Systems; Syntax-Aware Task Spawning With Tree-Sitter | Logical/device/local coordinates, stable identity, captured context and explicit execution boundaries |
| [Extension boundaries](extensions.md) | Life of a Zed Extension: Rust, WIT, Wasm | Typed capabilities, ownership, versioned external contracts; Zed extensions are not audio plugins |

## Consolidated work order

These priorities are MUI engineering decisions based on the linked reports and
our existing implementation, not features promised by the blog series.

1. **Finish rendering quality first.** Keep GPUI as the runtime. Use native text,
   quads, SVGs and supported effects; preserve one adapter for merged MUI geometry.
   The [shared gradient correction](../render-lab/rx6600-gradient-fix-2026-09-13/RESULTS.md)
   now passes RX 6600 sRGB/Oklab checks. Thin-stroke AA remains open.
2. **Make coordinate and state ownership explicit.** Paint, clip and pick from the
   same resolved snapshot. Distinguish GPUI logical `Pixels` from `DevicePixels`;
   apply DPI once. Finish stable parameter/node/route IDs before persistence or
   host automation can depend on view positions.
3. **Reuse GPUI task lifetimes when background work is needed.** Foreground tasks
   still run on the UI thread. Return owned results from background tasks, reject
   stale revisions and cancel view-owned work on close. Keep DSP outside this path.
4. **Test the integration boundaries.** Retain real input/gesture tests and image
   oracles. Add stale-completion, restricted-viewport, multi-instance and lifecycle
   cases where the actual implementation introduces those failure modes.
5. **Measure before adding structures.** Record preparation, cache behavior,
   presentation and tail latency separately. Adopt a summary tree, bitmap index,
   worker or second renderer only for an observed bottleneck.
6. **Then continue the [plugin plan](../KURV-PLUGIN-PLAN.md).** Typed internal Rust
   contracts do not replace the external audio-plugin ABI. Bind one audible slice
   through the existing bridge, preserving host-thread and real-time constraints.

Current-main Zed links in individual reports are research references, not a GPUI
upgrade decision. The implementation target remains the pinned checkout and its
explicit local patches. Historical benchmark/validator results are not freshly
rerun DAW validation. The render correction report records this turn's exact
checks, including the restricted-viewport interaction failure.
