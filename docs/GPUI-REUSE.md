# GPUI reuse decisions

The plugin reference uses **GPUI as its renderer and native UI runtime**. Vello stays in
`experiments/render-lab` for comparisons; it is not part of this plugin's rendering path.
We are not composing two renderers for the same panel.

## Reuse from the pinned GPUI checkout now

Inspected Zed revision `7960b2a7c9568e90fbe0727332149e5b2a5fd57a`, used by our prepare script.
These are existing APIs, not proposed MUI reimplementations.

| Need | GPUI facility | MUI responsibility |
| --- | --- | --- |
| GPU paths, glyph atlas and redraw scheduling | `Window::paint_path`, GPUI text painting and frame invalidation | Supply resolved MUI geometry/styles; preserve the embedded host pump |
| Font shaping, wrapping and baseline metrics | `Window::text_system`, `WrappedLine`, ascent/descent | Measure and paint with identical font/line height; pass first baseline into MUI layout |
| Text editing, selection, clipboard and IME plumbing | Pinned `examples/input.rs`, already adapted by `prepare.py` | Mount the persistent entity and connect value/state; real platform IME behavior still needs validation |
| Focus, Tab traversal, keyboard actions | `FocusHandle`, keymaps, actions, `focus_next` / `focus_prev` | Stable IDs, parameter-specific keyboard behavior and visible state |
| Click, hover, drag/drop, tooltips | `InteractiveElement` / `Interactivity` | MUI path hit policy and audio-parameter begin/change/end gestures; no new general event dispatcher |
| Scrolling and clipping | `ScrollHandle`, overflow containers, `Window::with_content_mask` | Match MUI viewport limits and use one snapshot for geometry, text and input |
| Large lists | `uniform_list`, `list` / `ListState` | Provide rows and stable keys when a real preset/browser list needs virtualization |
| Images and SVG icons | `img`, `svg`, `ImageCache` | Asset selection and MUI styling |
| Popovers and overlays | `anchored`, `deferred`, tooltip facilities | Positioning policy within the plugin's host window |
| State and async work | `Entity`, subscriptions, background executor | Plugin state ownership and UI/host-thread handoff |
| Accessibility metadata | GPUI roles, labels and focus primitives | Control semantics and verification in the actual host/OS |

Pinned source: [GPUI elements](https://github.com/zed-industries/zed/tree/7960b2a7c9568e90fbe0727332149e5b2a5fd57a/crates/gpui/src/elements),
[text layout/painting](https://github.com/zed-industries/zed/blob/7960b2a7c9568e90fbe0727332149e5b2a5fd57a/crates/gpui/src/text_system/line.rs),
[window APIs](https://github.com/zed-industries/zed/blob/7960b2a7c9568e90fbe0727332149e5b2a5fd57a/crates/gpui/src/window.rs).

## What is wired into the panel

The header aligns two GPUI font sizes through MUI's baseline API. The panel has outer and
inner GPUI overflow containers; their native children include accessible label elements,
the gain hitbox and the persistent text-input entity.

A small bridge routes GPUI wheel events through the existing `ViewState::scroll_at`, then
writes the resulting offsets to GPUI `ScrollHandle`s. This prevents the pinned native
listeners from moving both nested containers for the same event. Remainders reach ancestors
only after the inner range is exhausted. No second wheel-propagation algorithm was added.

During canvas prepaint, the bridge clamps offsets against current MUI limits and constructs
one view snapshot. Path painting and GPUI glyph painting apply its translated origins and
intersected clip stack through native content masks. Gain picking calls `View::tap_at`.
Input controls use the corresponding native scroll containers, keeping their clipping,
focus geometry and text-input coordinates aligned. The adapter currently uses **scroll
translations and rectangular clips**, not arbitrary rotated/scaled text or native widgets.

The X11 check asserts first-baseline equality, nested scroll isolation, boundary propagation,
matching native/painted offsets, clipped gain rejection and the existing input/lifecycle
contract. It runs on the RX 6600, including with GPU readback enabled.

## Reuse for full controls before building our own

GPUI itself is infrastructure rather than a complete ready-styled widget library. Zed's
`ui` crate supplies buttons, menus and other application components, but also depends on
Zed's `theme`, `icons`, `menu` and component crates. Its manifest declares GPL-3.0-or-later;
GPUI's manifest declares Apache-2.0. Treat them as different packages rather than assuming
all Zed widgets are part of GPUI. [Pinned ui manifest](https://github.com/zed-industries/zed/blob/7960b2a7c9568e90fbe0727332149e5b2a5fd57a/crates/ui/Cargo.toml).

**Evaluate `gpui-base` before writing a MUI control toolkit.** GPUI Kit (formerly GPUI
Component) separates unstyled behavior from its styled component layer. Its base catalog
includes buttons, toggles, sliders, inputs, selection controls and popovers, while allowing
an application to supply its presentation. This fits MUI's ownership of appearance and
geometry. [GPUI Base](https://gpui-kit.com/base/), [slider parts](https://gpui-kit.com/base/primitives/slider/).

Compatibility is not yet proven. On 2026-09-13, the inspected manifests declare
`gpui-base` 0.6.1 using **`gpui-pre` 0.3.1**, while this experiment uses a patched, path-sourced
**`gpui` 0.2.2** from the pinned Zed revision. Those are distinct Cargo package identities;
simply adding the dependency would not make their GPUI entity/window types interchangeable.
No GPUI Kit dependency or upgrade was added in this change.
[Base manifest](https://raw.githubusercontent.com/longbridge/gpui-component/main/crates/base/Cargo.toml),
[workspace dependency versions](https://raw.githubusercontent.com/longbridge/gpui-component/main/Cargo.toml).

The next bounded experiment is one `gpui-base` input and slider against an aligned GPUI
revision, preserving the embedded-editor smoke test and the parameter gesture contract.
Adopt compatible behavior before building general buttons, dropdowns, text editors or list
virtualization. Custom audio knobs, parameter mappings, MUI geometry and host integration
remain the parts specific to this framework.
