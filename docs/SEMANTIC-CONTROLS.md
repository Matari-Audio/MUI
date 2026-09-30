# Complete controls with less application code

This increment keeps MUI's intrinsic solver, ordinary row/column composition,
retained `Ui`, and existing widget/binding APIs. It puts repeated control policy
in the components that already own it. No new macro language or runtime is added.

## Complete action controls

```rust
use mui::prelude::*;
let mut ui = Ui::default();
let mut saves = 0;
let save = button(&mut ui, "save", "Save").on_change(|| saves += 1);
```

Button and toggle defaults include a hand cursor and a keyboard-visible focus
ring. Knobs, sliders and drag values have appropriate resize cursors and focus
rings on their actual input target. Pointer focus does not show a keyboard ring.
All colors are semantic roles, so the defaults follow light and dark themes.
Disabled controls dim the whole control subtree, including labels/readouts,
in addition to rejecting input and reporting their disabled semantic state.

`on_change` is the existing `changed` result as a composable callback: buttons
report activation and value controls report edits. It runs while the tree is
built, exactly like `if response.changed`. It does not install an event loop or
promise to deduplicate multiple constructions of the same widget. Build each
stable id once per frame. The runtime already coalesces pointer/key/semantic
activation to one boolean per control per frame; new tests cover all routes,
repeated keys, duplicate semantic requests, idle frames and interruption.
A target disabled in the presented tree no longer delivers a release/drag/key
left over from the preceding hit map; its host `End` edit is still delivered.

`crates/mui/examples/compact_controls.rs` contains the compiling explicit and
compact forms. Count the same decisions in each, excluding the identical action
body and element conversion: the explicit form has six (construct, changed
branch, cursor, focus state, stroke color, stroke width); the compact form has
two (construct, handler). Line wrapping is not part of this count.

## Readouts measured with the actual font

```rust
use mui::prelude::*;
let mut ui = Ui::default();
let mut frequency = 440.0;
let dial = knob(&mut ui, "frequency", "Frequency", &mut frequency, 20.0..=20_000.0)
    .value_text("440 Hz")
    .value_reserve_all(vec!["999.99 Hz".into(), "20.0 kHz".into()]);
```

`value_reserve` takes one known widest sample; `value_reserve_all` measures the
maximum of several candidates, using the actual font, size and variation axes.
They apply to slider/drag-value readouts and the caption under a knob. Their
lower-level counterpart is `Styled::reserve_all`; existing `reserve` still works
and is supplemented rather than replaced. `Arc<[String]>` samples may be shared
between builds. Current text remains a floor, so an unanticipated wider value is
never clipped. Default two-decimal readouts measure both endpoints instead of
choosing an endpoint by byte count.

A sample is a domain contract, not a guess that every format is widest at an
endpoint. Arbitrary unit switches, custom words and localization require samples
covering those cases. This API cannot prove the maximum of an arbitrary formatter.
The layout cache includes every sample; the same intrinsic solver handles cached
and fresh layouts. No per-pixel position arithmetic was added.

## Metadata-backed plugin parameters

```rust,ignore
let gain = bridge.knob(ui, P::Gain).size(L);
let cutoff = bridge.slider(ui, P::Cutoff);
let bypass = bridge.toggle(ui, P::Bypass);
```

These are real `Control` builders over the same edit protocol as `Bridge::bind`.
The parameter table supplies the stable id, accessible name, normalized mapping,
formatted plain value, discrete step and read-only status. Unknown/read-only
parameters are disabled throughout the subtree. Existing `bind`, `bind_as` and
`bind_bool` remain available for custom controls and multiple views of one
parameter. Call `end_unbound` after each build when driving a Bridge directly;
`MuiEditor` already does this.

The gain-plugin example uses these methods and remains a real plugin build target.
A styled knob now takes two calls (`bridge.knob`, `size`). A fair hand-written
comparison must also include metadata lookup, formatting, reserve samples,
discrete stepping and read-only handling; a bare `bind + knob` is not equivalent.

Discrete keyboard/accessibility increments use the range's actual quantum, so a
1..8 control advances one integer instead of losing a hundredth-of-range step to
quantization. `Control::step` is also available without a plugin. Explicit steps
are minima: Shift does not reduce them; Page Up/Down use ten. Continuous controls
keep hundredth-of-range stepping and Shift fine adjustment. Pointer dragging
stays continuous and Bridge quantization remains at the host boundary.

Small discrete domains are exhaustively formatted once (up to 256 intervals).
Continuous/larger domains sample their endpoints and default. For a custom
formatter, configure wider candidates once at editor setup:

```rust,ignore
bridge.reserve_readout(P::Cutoff, vec!["999.99 Hz".into(), "20.0 kHz".into()]);
```

Knob/slider accessibility actions still use the normalized numeric 0..1 range;
the formatted value (including units) is supplied separately to the platform.
This preserves nonlinear mapping and does not pretend a logarithmic parameter is
a linear plain-value control. `Bridge::toggle` has the same 0/1 contract as
`bind_bool` and is intended for boolean parameters.

## Settings and groups

```rust
use mui::prelude::*;
let mut ui = Ui::default();
let mut sync = false;
let sync = setting("sync", "Sync")
    .description("Follow the host tempo")
    .toggle(&mut ui, &mut sync);
let panel = group("engine", "Engine", [sync]);
```

Three setting operations supply a visible label, visible helper text, the input's
accessible name/description, a stable input id and a semantic parent. `control`
accepts a closure for another built-in control without repeating its id/label.
`group` provides an intrinsic titled card and semantic ownership, with no fixed
width or fill-parent default. These are ordinary Rust functions over rows and
columns. Applications can still compose those primitives directly.

Descriptions and formatted values travel through the resolved semantic tree into
AccessKit. Their changes invalidate the accessibility publisher even when bounds
and numeric values do not change. A drag value retains its name/help when it
becomes an editable text field.

## Compatibility and cost

- Existing widget constructors, responses, `bind` APIs, ids, layout algorithms and
  host edit ordering are retained. Explicit numeric step overrides are opt-in
- Default focus paint/cursors change intentionally. Existing style overrides are
  still applied in order; no hard-coded light/dark colors were introduced
- New metadata lives in the already boxed `Extras` and resolved surfaces. Code
  constructing a full raw `Extras` literal must add the new fields or use
  `..Default::default()`; ordinary builder-based clients need no migration
- Parameter reserve samples are cached per Bridge, with bounded setup work.
  Formatted current values still allocate a string per numeric control per build,
  as `Bridge::text` did; this is not an allocation-free API
- Focus state closures add small tree-construction allocations. No timing or
  binary-size performance claim is made. No new dependencies or macro expansion
  layer were added
- A caller's `Ui::memo` dependencies must still include any changing description,
  value, step or reserve policy. Framework metadata does not repair missing app
  dependencies

## Remaining roadmap

A select/preset picker still needs a complete keyboard/popup/focus-return contract,
ComboBox/ListBox/Option semantics, stable keyed selection and large-list policy.
It is deliberately not implemented as a few buttons labelled as options here.
Search and virtualization, semantic test selectors, more intrinsic-layout
explanations, cross-platform native screen-reader wiring and performance work
remain separate increments. The existing macOS hover golden mismatch is an
independent baseline issue, not a reason to change unrelated pixels in this patch.

## Checks

For a CPU-only visual check of the real controls in both themes:

```text
cargo run -p mui --features cpu --example semantic_gallery -- /tmp/controls
```

This writes `/tmp/controls-dark.png` and `/tmp/controls-light.png`, with the Save
button keyboard-focused. The example is separate from existing golden snapshots.

The new tests live in `mui/tests/semantic_controls.rs`, `mui-truce/src/bridge/tests.rs`
and `mui-access`'s unit tests. They cover semantic publication, measurement/cache
parity, integer bounds/negative ranges, nonlinear mapping, disabled actions and
keyboard/pointer interruption. Existing layout and editor/host gesture suites are
also part of the gate. Native OS/DAW and screen-reader certification is separate
from these headless tests.
