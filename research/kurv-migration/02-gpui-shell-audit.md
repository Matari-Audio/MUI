# 02 — Generation B audit: the GPUI shell

Read-only audit, 2026-09-16. Scope: `KURV/src/editors/mui/**`, `KURV/src/editor_mui.rs` +
`KURV/src/editor_mui/**`, and the pinned MUI snapshot at
`.worktrees/kurv-mui-runtime-0.20.37` (`experiments/gpui-plugin`, `crates/mui-gpui`,
`crates/mui-truce`). Every line range below was opened; every count comes from a
`wc`/`grep`/`python` run in this session. Target for the rewrite: MUI issue #8,
"the ~13k-line GPUI editor at a third of the size".

---

## 1. Module map and line counts

### 1a. `src/editors/mui/` — the live shell (`mui_shell`, selected by `src/editors/mod.rs:8-24`)

Total **13,141 lines** across 21 files. Splitting off the trailing `#[cfg(test)] mod tests`
blocks: **11,086 lines of code, 2,055 lines of test**.

| File | Lines | Code/test split | What it is |
|---|---:|---|---|
| `editing.rs` | 2,371 | 2,304 / 67 | action dispatch, curve/knot editing, buttons, theme push |
| `view.rs` | 1,938 | ~1,938 / 0 | `Shell` struct, `EmbeddedView` impl, `render` |
| `warps/mod.rs` | 1,153 | 782 / 371 | warp rack cards, mode bodies, assignment |
| `samples.rs` | 783 | 746 / 37 | Grain/Resynth import, waveform + spectrum canvases |
| `modulators/surface.rs` | 769 | 691 / 78 | LFO/envelope graph surfaces |
| `routing.rs` | 672 | 509 / 163 | route ports/cables adapter onto KURV's `RouteGraph` |
| `presets.rs` | 564 | 511 / 53 | preset browser/audition |
| `modulators/mod.rs` | 542 | 314 / 228 | modulator rack + `SourceCard` |
| `structure.rs` | 531 | 500 / 31 | reorder/drag/drop, group extraction |
| `groups.rs` | 513 | 330 / 183 | group header, ADSR graph, output controls |
| `synth/binding.rs` | 510 | 358 / 152 | `Oscillator` ↔ `KurvParams` binding |
| `synth/mod.rs` | 459 | 431 / 28 | per-engine card composition |
| `wavetable.rs` | 407 | 361 / 46 | wavetable library/import/function editor |
| `shell.rs` | 378 | 80 / 298 | **top-level tree — 58 real lines** (`:4-62`) |
| `synth/pan.rs` | 340 | 286 / 54 | pan XY / pan-shape editing |
| `curve_brush.rs` | 265 | 144 / 121 | Ctrl-drag grid paint |
| `performance.rs` | 246 | 208 / 38 | global performance/quality page |
| `control.rs` | 218 | 173 / 45 | **the one gesture adapter** |
| `aux.rs` | 216 | 206 / 10 | AUX cards |
| `components.rs` | 174 | 174 / 0 | local DSL wrappers (row/column/label/panel/parameter) |
| `header.rs` | 71 | 71 / 0 | masthead tree |

### 1b. `src/editor_mui.rs` + `src/editor_mui/` — generation B-early, still compiled

**4,514 lines.** `editor_mui.rs` is 3,352 lines of which only **1,054 are code**
(`:1-1054`); lines `1055-3352` are one `mod tests`. The submodules: `workspace.rs` 386,
`panel.rs` 328, `routes.rs` 252, `binding.rs` 196.

`lib.rs:10-18` compiles **both** entry points (`create_mui_editor` from the old file,
`create_mui_shell` from `editors/mui/view.rs`). Only `mui_shell` is reachable from
`editors/mod.rs`. The new shell reaches back into the old module for exactly four things
(`grep -o 'editor_mui::[a-z_]*'` over `src/editors/mui`): `binding` (8×),
`workspace::bindings` (3×), `distribution_with_gain` (3×), `values` (1×).
`editor_mui/panel.rs` and `editor_mui/routes.rs` have **zero** consumers outside the dead
`editor_mui.rs`.

**Verdict: Redundant.** ~3,100 of the 4,514 lines (`editor_mui.rs:1-1054` product code,
`panel.rs` 328, `routes.rs` 252, plus most of the 2,298-line test module that exercises the
dead view) are a second, non-shipping editor kept alive by a `pub use`. The parts the live
shell actually uses are `binding.rs` (196) and `workspace.rs` (386) — and those are the two
files that are *not* GPUI-shaped.

### 1c. Pinned `experiments/gpui-plugin` — 8,863 lines, more than half unreachable

| Module | Lines | Used by the live shell? |
|---|---:|---|
| `modulation.rs` | 1,948 | **Yes** — 39 references (ports, pies, cables, `Routing` global) |
| `oscillator.rs` | 1,756 | Only 4 helpers: `Ink`, `label_weighted` (`:638`), `border_ink` (`:1477`), `geometry_builder` (`:1711`), pulled in by `dsl.rs:54,236,254,432` and `modulation.rs:659,931,934`. The 1,200+ lines of demo `Oscillator`/`OscillatorGroup` entities are reachable only from the dead `editor_mui.rs:14-17`. |
| `dsl.rs` | 760 | **Yes** — `Resolved`, `slot`, `element`, plots, `AxisLock` |
| `text_input.rs` | 669 | **Yes** — 10 references |
| `curve_editor.rs` | 687 | No |
| `group_header.rs` | 624 | No |
| `kurv.rs` | 580 | No (a mock KURV inside MUI) |
| `panel.rs` | 559 | No |
| `main.rs` | 550 | No (standalone demo) |
| `live_theme.rs` | 146 | **Yes** — 31 references |
| `pie_container.rs` | 138 | No (only `oscillator.rs`/`kurv.rs`) |
| `controls.rs` | 83 | **Yes** — `init`, `parameter`, `navigation` |
| `lib.rs` | 189 | `EmbeddedView`/`GpuiEditor` re-export only; also declares a whole `ProbePlugin` gain plugin |
| `control_panel.rs` | 55 | Only by the dead `editor_mui/panel.rs:5` |
| `module_shell.rs` | 18 | No |
| `examples/oscillator.rs` | 101 | No |

**3,312 lines** (`main` + `kurv` + `panel` + `group_header` + `curve_editor` +
`pie_container` + `control_panel` + `module_shell` + the example) are unreferenced by KURV,
plus ~1,500 unreferenced lines inside `oscillator.rs`. **≈57% of the pinned dependency is
dead weight KURV compiles on every build.** **Verdict: Bad.** KURV's editor takes a path
dependency on an *experiment* crate that also declares its own CLAP plugin
(`lib.rs:19-48`), and the KURV-specific header/curve/pie code sitting in MUI's experiment
is exactly the coupling `docs/MUI-COMPONENT-MIGRATION.md:139-145` (step 6) said to remove
and never did.

---

## 2. How it binds to KurvParams / host edits / undo

This is the good half. The chain is:

```
KurvParams (Arc, truce Params)
  └─ mui_truce::Parameter  (crates/mui-truce/src/parameter.rs:44-181)
       └─ mpsc::Sender<Edit>   Edit = Begin(u32) | Value(u32,f64) | End(u32)  (:11-15)
            └─ mui_truce::Automation::dispatch  (:18-41)   — host thread
                 └─ PluginContext::{begin_edit,set_param,end_edit}
```

- **`crates/mui-truce/src/parameter.rs` is byte-identical between the pinned snapshot and
  current MUI main** (`diff -q` → SAME, 181 lines both sides). The host bridge does not
  need porting at all. **Verdict: Good.**
- `Automation` refuses ids the host does not know (`:24-26`), tracks an active set so a
  `Value` without a `Begin` is dropped (`:29-33`), and `close()` balances every open gesture
  (`:35-40`). That is the correct trust boundary and it survives the rewrite unchanged.
- **`src/editor_mui/binding.rs:105-190` — `Binding::{Host(Parameter), Source(Arc<KurvParams>,
  usize, SourceField)}`.** One enum gives `value/text/begin/end/set/step/reset` for both
  host-bank parameters and KURV-native persisted modulator fields that have no host slot.
  `SourceField` (`:5-103`) carries the normalization, the musical formatting
  (`"{:.2} Hz"`, `"BEAT"`, `"−∞ dB"`) and the per-field step size. **Verdict: Good — pure
  logic, zero GPUI, port verbatim.**
- **`src/editors/mui/synth/binding.rs:30-60`** resolves an oscillator's host-automation bank
  slots by searching `params.host_automation_targets.snapshot()` for each
  `ModulationRouteTarget::oscillator(id, slot, control)` and errors loudly on a miss.
  **Verdict: Good, port as logic.**
- **`Entity`/`Subscription`**: the live shell keeps only three GPUI entities
  (`view.rs:36,80,81,86`: `group_name`, `curve_phase`, `curve_value` `TextInput`s, plus the
  `modulation::Overlay`). The dead generation-B-early file is the one with
  `Entity<Oscillator>` + `_subscription: Subscription` per card (`editor_mui.rs:62-75`).
  So the current shell already avoids per-widget entities; there is nothing there to port.
- **Undo**: `editor_history::EditorHistory` is KURV's, not MUI's. The shell calls
  `history.defer_commit()` on every action/gesture start (`view.rs:174, 201`) and
  `history.flush_deferred(&self.context)` from `synchronize` only when
  `!host_edits_pending` and all thirteen drag states are `None`
  (`view.rs:534-552`). `host_edits_pending` is driven from the runtime thread by
  the pending-edit atomic (`crates/mui-gpui/src/lib.rs:303-307`). **Verdict: Good idea, bad
  shape** — the correctness condition is a hand-maintained 13-term boolean that must be
  updated every time someone adds a drag field. One `Interaction::pressed().is_none()` in
  current MUI (`mui-input/src/lib.rs:222-389`) replaces the whole conjunction.

---

## 3. What it builds on the pinned MUI vs what it hand-rolls

### Uses the DSL (Good)

- `src/editors/mui/components.rs:1-80` wraps `Item::row`/`Item::column` once
  (`:5-21`) and everything else composes through those. Raw `Item::row`/`Item::column`
  appear exactly **once each** in 13k lines; the wrappers `row(` / `column(` / `item(` are
  used **64 / 24 / 32** times. That is the right layering.
- `src/editors/mui/shell.rs:4-62` — the whole top-level composition is **58 lines**, three
  columns + two dividers, declarative. This meets the "<100 readable lines" gate in
  `docs/MUI-COMPONENT-MIGRATION.md:60-63`. **Verdict: Good, and the best thing in
  generation B.**
- `Resolved::responsive` (`dsl.rs:46-48`) is used for the main tree (`view.rs:964`). It
  exists precisely to fix the viewport bug: `Resolved::new` offers
  `width.max(minimum_width)` (`dsl.rs:83`), which is what made the 1120×720 capture wider
  than the window — recorded at `docs/migration/KURV-REFERENCE-AUDIT.md` "Why the migration
  diverged", item 2. **But `Resolved::new` is still called 5× in the live shell**
  (`view.rs:1679, 1701`, plus three more), each one a place that can still overflow.
  **Verdict: Missing** — a responsive-only API.
- Native slots: `scene.slot(...)` 50×, `sticky_slot` 1× (`dsl.rs:109-112`, the sticky group
  header). Hit targets and layout share one coordinate system, as claimed.
- One gesture adapter: `src/editors/mui/control.rs:36-105`,
  `Shell::parameter_gestures`. Double-click → `Gesture::Reset`, wheel → `Gesture::Step`,
  drag → `Gesture::Begin`, all funnelled into `Binding`. Route-arm mode short-circuits
  (`:85-87, :101-103`). **Verdict: Good.** The duplicate gesture path that
  `docs/MUI-COMPONENT-MIGRATION.md:28-30` complained about is gone from the live shell.

### Hand-rolled in raw GPUI (Bad)

Counts over `src/editors/mui` only:

| Construct | Count |
|---|---:|
| `div()` | 135 |
| `.child(` | 288 |
| `.absolute()` / `.relative()` | 30 / 32 |
| `.size_full()` | 57 |
| `.flex()` / `.flex_col()` | 54 / 21 |
| `canvas(` (hand-painted surfaces) | 17 |
| `point(` | 75 |
| `px(...)` | 180, of which **132 are bare numeric literals** |
| `Bounds::` literals | 7 |
| `relative(` fractional placement | 40 |
| `PathBuilder` / `paint_path` / `paint_quad` | 10 / 12 / 2 |
| `cx.listener` | 55 |
| `on_mouse_*` | 29 |
| `format!(` (stringly-typed slot ids) | **374** |
| `scene.frame(` (re-querying resolved geometry) | 71 |

Top repeated pixel constants: `px(0.)` 15, `px(8.)` 14, `px(4.)` 10, `px(2.)` 8, `px(1.)` 8,
`px(6.)` 7, `px(100.)` 7, `px(24.)` 6, `px(12.)` 6.

**~200 absolute placements.** The per-file concentration is `editing.rs` (62 `px`),
`view.rs` (22), `samples.rs` (22), `groups.rs` (20), `modulators/surface.rs` (18) — i.e.
every graph surface.

### Imperative layout instead of declared layout

1. **`view.rs:887-966` — the whole tree is rebuilt behind a manual cache key.**
   `key = (width.to_bits() ^ height.to_bits().rotate_left(17), scale_factor.to_bits())`,
   and `self.layout = None` is sprinkled across `rebuild` (`:142`), `action` (`:198`),
   `set_rack_width` (`:196`) and the port-count diff (`:885`). Any missed invalidation is a
   stale layout. **Verdict: Bad.**
2. **Conflicting minimum-width policies.** `shell.rs:19-54` declares `min(180.,0.)`,
   `min(520.,0.)`, `min(240.,0.)` in the tree; `view.rs:896` then overwrites with
   `self.rack_min = [220., 260.]` as a hardcoded runtime constant, with the comment
   "Wrapped control rows must not turn their unwrapped intrinsic width into the minimum rack
   width." Two sources of truth for the same number. `set_rack_width` (`view.rs:188-197`)
   adds a third magic constant, `- 560.`, for the centre column. **Verdict: Bad.**
3. **Decorations computed from resolved frames in imperative loops.**
   `view.rs:977-987` walks the insertion slots, calls `scene.frame(id)`, computes
   `(b.y, center - 17., center + 17.)` and pushes the result back into the scene via
   `set_horizontal_stroke_gaps`. Layout → read back → patch layout. `view.rs:1660-1699`
   does the same for the watermark labels, running a *second* `Resolved::new` per column and
   placing the result with `.left(px(frame.x as f32))`.
4. **Text is patched into the tree after resolve.** `view.rs:988-1027` pushes every readout
   string with `scene.set_text(format!("value-osc-{i}-{n}"), ...)`, guarded by a magic index
   list `[2, 3, 8, 9, 18, 19].contains(&n)` for which VA controls get a modulation target
   label. `reserve_text` is used only **2×** (`components.rs:29-47`), so the reserved widths
   are a hardcoded lookup table keyed on the *label string* (`"WAVE" => "4.00"`,
   `"PAN" => "L 100%"`). **Verdict: Bad — a formatting table per view, exactly what
   `docs/MUI-COMPONENT-MIGRATION.md:33-35` said not to build.**
5. **Graph painting re-derives its own inset three times per function.**
   `groups.rs:158-290`: the ADSR curve canvas, the per-stage highlight canvas and the
   feedback canvas each recompute `px(2.) + (bounds.size.width - px(4.)) * x` and
   `px(2.) + (bounds.size.height - px(4.)) * (0.5 - value * 0.5)`; the stage canvas adds
   `px(2. - 4. * x)` to un-skew the inset it just applied. Stage boundaries are indexed by a
   magic `[2, 4, 6, 7]` into `GroupControl::ALL`. `docs/mui-curve-fidelity.md:7` claims
   "Graphs share one inset for rendering and hit testing" — they share the *number*, not the
   code.
6. **`render` is 1,135 lines** (`view.rs:793-1927`), containing the port-count diff, the
   layout cache, the text patch, the slot-attachment loops, the resize handles, the
   watermark, the masthead, and the global mouse-move handler that fans out to six different
   drag states (`view.rs:1758-1815`). The `Shell` struct itself has **49 fields, 19 of them
   `Option`** (`view.rs:33-83`), and `finish()` (`view.rs:144-183`) is a 14-branch manual
   reset of those transients. **Verdict: Bad.**
7. **Test-only geometry probing.** Because the tree exposes no hit map, the native checker
   is fed bounds through `canvas` side-effects into two global `Mutex` statics,
   `MENU_BOUNDS`/`ACTION_BOUNDS` (`view.rs:1-7`, written at `editing.rs:628-640` and
   `structure.rs:97-113`, read at `editor_mui.rs:1415-1990`). `#[cfg(test)]` geometry in
   production render paths. **Verdict: Bad** — current MUI's `Hit`/`Interaction`
   (`MUI-migration/crates/mui-input/src/lib.rs:47-105, 222-389`) gives this for free.

---

## 4. Duplicated widgets

| Widget | Pinned MUI | KURV generation B | Status |
|---|---|---|---|
| parameter cell / group / panel | `control_panel.rs:6-55` (`parameter_cell`, `parameter_group`, `engine_panel`) | `components.rs:29-80` (`parameter_tinted`, `parameter_group`, `panel`) | **Duplicated.** The MUI one survives only for the dead `editor_mui/panel.rs:5`. |
| module shell / header | `module_shell.rs` (18), `group_header.rs` (624) | `shell.rs:4-62`, `groups.rs`, `header.rs` | **Duplicated**, MUI side unused. |
| curve editor | `curve_editor.rs` (687) | `editing.rs:16-155` + `curve_brush.rs` (265) | **Duplicated.** KURV's is the real one (native `WaveCurveData`, knots + bends); MUI's is an invented Bézier model. |
| oscillator card | `oscillator.rs` (1,756) | `synth/mod.rs` + `synth/binding.rs` + `synth/pan.rs` (1,309) | **Duplicated**, MUI side is the mock. |
| pie / port | `pie_container.rs` (138) and `modulation.rs::{port, sized_pie}` | `routing.rs:8-41` adapter | **Duplicated inside MUI** — two pie implementations; KURV uses `modulation.rs`, `pie_container.rs` is orphaned. |
| routing graph | `modulation.rs` global `Routing` (1,948) | `routing.rs:43-510` `Owner` over KURV's `RouteGraph`/`legal` | Correct split (MUI visuals, KURV legality) but MUI's routing state is a **`cx.global::<Routing>()` singleton** (`control.rs:85, 101`) — one editor instance per process. |
| text input | `text_input.rs` (669) | — | **Not duplicated. Good.** |
| theme | `live_theme.rs` (146), set from `editing.rs:526-540` | `editor_theme::` (41 refs) + three literal `Color::Custom(Rgb(..))` constants (`components.rs:2-4`) and 4 `rgb(0x…)` | **Duplicated/violating.** `AGENTS.md` "Type, color, and interaction": "Use `editor_theme::semantic()` roles, never literal UI colors." |

---

## 5. UI breakages and known issues

### Recorded in KURV docs

1. **165 fps is not measured; the 4 ms pump is not a frame-rate guarantee** —
   `docs/MUI-MIGRATION.md:35-36`. The pump is
   `commands.recv_timeout(Duration::from_millis(4))` in
   `crates/mui-gpui/src/lib.rs:282`, with the comment "8 ms capped both below 165 Hz;
   bounded 4 ms polling leaves rendering headroom without busy-waiting." Every iteration of
   that loop calls `view.synchronize(cx)` (`:302-308`) — the full parameter/preset/sample
   poll — regardless of whether anything changed. **Verdict: Bad.** A fixed-interval poll
   loop is not a frame clock, and nobody has measured it.
2. **Sticky scrolling / narrow-window layout / resizing** — listed as *fixed* in
   `docs/MUI-COMPONENT-MIGRATION.md:13-18` ("native sticky positioning, responsive
   narrow-window layout") and as *covered by the checker* in `docs/MUI-MIGRATION.md:20-22`
   and `docs/mui-completion-pass.md:26`. The checker is **69 lines**
   (`tools/check_mui_editor.py`) and asserts on **one** string:
   `'PASS: native add Noise, add groups, live snapshots and sticky group header'`
   (`:58`). **Verdict: Missing.** The evidence for the three most-reported breakages is a
   single substring match.
3. **Wheel events swallowed by controls** — `docs/mui-completion-pass.md:26`: "Control/header
   surfaces can consume wheel events; scrolling was exercised from the rack gutter." That is
   a live bug, documented as a test workaround. Root cause is visible: `controls::parameter`
   binds the wheel to `Gesture::Step` (`control.rs:98-105`) with no "only when focused/hovered
   *and* the value is at neither rail" condition, so every parameter cell is a scroll trap.
4. **Hover lock reentry** — `KURV-REDESIGN-PLAN.md` "Current editor and modulation work":
   "An egui lock reentry in warp-target hover … was found during integration and corrected."
   That is **generation A**, not B. Generation B has no equivalent lock, but it does keep
   hover state in the view (`hovered_parameter`, `control.rs:107-119`) and uses it to route
   undo (`history_shortcut(..., hovered)`, `control.rs:141-166`) — so undo depends on pointer
   position, which is faithful to the legacy editor and worth keeping, but only if hover
   enter/leave is reliable.
5. **Hit targets vs slots** — claimed solved: `docs/MUI-COMPONENT-MIGRATION.md:24-26`,
   "`Resolved::responsive` honors available width while keeping native slots and hit targets
   in the same coordinate system." Confirmed for the 50 `scene.slot` call sites. **But** the
   17 `canvas` surfaces and the 40 `relative(...)` placements do their own hit math
   (e.g. `groups.rs:209-218` positions stage regions with `relative(left)`/`relative(width)`
   computed from a separate `hits[]` array than the `boundaries[]` array used to draw).
   Two arrays, one geometry. **Verdict: Bad.**
6. **Viewport overflow** — `docs/migration/KURV-REFERENCE-AUDIT.md`, "Why the migration
   diverged" items 2-3: the Hug-then-`max(minimum_width)` contract makes a valid scene wider
   than the window, and "the test checked the wrong containment boundary." Mitigated by
   `Resolved::responsive` for the main tree only; `Resolved::new` survives at 5 call sites.
7. **Not-parity, by KURV's own admission** — `docs/MUI-COMPONENT-MIGRATION.md:38-44`:
   preset browser metadata/import-export, advanced wavetable function editor, global
   settings pages, and "promotion of experimental GPUI modules to MUI's production package
   boundary" all still open.

### Visible in code, not recorded

8. **Two live editor entry points.** `lib.rs:10-18` compiles and exports both
   `create_mui_editor` (dead) and `create_mui_shell` (live). `editor_mui.rs:1202` even runs
   `native_editor_check::<crate::mui_shell::Shell>()` from the dead module's test suite.
9. **`Routing` is a GPUI global.** `cx.global::<Routing>()` / `cx.global_mut::<Routing>()`
   (`control.rs:85,101`, `view.rs:900`) means modulation drag state is per-process, not
   per-editor. Two plugin instances in one DAW share it.
10. **Errors are rendered as text.** `view.rs:1737-1743` and `:1747-1753`:
    `eprintln!("KURV DSL: {e}")` then `div().child(e.to_string())`. A layout failure
    degrades to a string in the window.
11. **`unwrap()` on the layout cache.** `view.rs:981` and `:1028`
    (`self.layout.as_mut().unwrap()`), safe only because `:963` just assigned it.
12. **Resize is fire-and-forget.** `crates/mui-gpui/src/lib.rs:183-195` stores the request
    in a `Mutex<Option<(u32,u32)>>` that the 4 ms pump drains (`:291-299`); a burst collapses
    to the last value, and the comment admits "Refusing a real resize strands the child at
    its previous dimensions." Combined with the `layout = None` cache key on
    `(width, height, scale)` this means every resize frame re-resolves the whole tree.

---

## 6. Verdicts

**Good — port as logic, unchanged or nearly so**

- `mui_truce::{Edit, Parameter, Automation}` — identical in current MUI. Zero work.
- `src/editor_mui/binding.rs:5-190` — `SourceField` + `Binding`. 196 lines, no GPUI.
- `src/editors/mui/synth/binding.rs:30-…` — host-bank resolution and the loud error on a
  missing slot.
- `src/editors/mui/control.rs:16-34, 141-166` — the `Gesture` enum and `history_shortcut`,
  with its test (`:174-217`). Pure.
- `src/editors/mui/shell.rs:4-62` — the 58-line composition. Re-express in current MUI's
  DSL almost line for line.
- `src/editors/mui/components.rs:5-80` — the wrapper idea. Keep the shape, drop the literal
  colors and the `reserve` string table.
- `src/editors/mui/routing.rs:43-510` — the `Owner` adapter that keeps route legality in
  KURV. Keep the split; re-point at current MUI's route visuals.
- `editing.rs:16-155` + `curve_brush.rs` — the knot/bend curve model and the magnetic snap.
  This is the native representation `docs/mui-curve-fidelity.md:5-7` fought for; do not
  regress to a generic Bézier.
- The `#[cfg(test)]`-free pieces of `warps/`, `modulators/`, `samples.rs`, `presets.rs`
  that compute *what to show* (evaluator sampling, warp preview, spectrum) rather than
  *where*.

**Bad — GPUI-shaped, must be rewritten**

- `view.rs:793-1927` (`render`, 1,135 lines) and `view.rs:33-83` (49-field `Shell`) —
  everything here is GPUI element-tree plumbing plus a hand-rolled layout cache.
- All 17 `canvas(` surfaces and their ~200 absolute placements. Current MUI's scene/vello
  path (`mui-scene`, `mui-vello`) draws from resolved geometry; none of this pixel
  arithmetic survives.
- `finish()` (`view.rs:144-183`) and the 13-term drag conjunction in `synchronize`
  (`:534-552`) — replaced by `Interaction`.
- The 374 `format!` slot ids and 71 `scene.frame` read-backs.
- The `MENU_BOUNDS`/`ACTION_BOUNDS` test statics and the `#[cfg(test)]` canvases in
  `editing.rs:626-641` and `structure.rs:96-113`.
- The `Routing` GPUI global.

**Redundant — delete, do not port**

- `src/editor_mui.rs` (3,352) + `src/editor_mui/panel.rs` (328) + `routes.rs` (252) and the
  `create_mui_editor` export — after lifting `binding.rs`, `workspace::bindings`,
  `values` and `distribution_with_gain` out. ~3,900 lines.
- The 3,312 unreachable lines of `experiments/gpui-plugin` plus ~1,500 of `oscillator.rs`.

**Missing**

- Any measured frame budget. The 4 ms pump is a poll interval nobody has profiled.
- A real native-UI check. 69 lines and one substring is not coverage for sticky scrolling,
  narrow layout and resize.
- A single minimum-width policy (three today: `shell.rs`, `rack_min`, `- 560.`).
- Semantic theming in the shell (`Rgb(29,29,29)` etc. in `components.rs:2-4`).
- Responsive-only resolution (5 surviving `Resolved::new` call sites).

---

## 7. What the rewrite should do about it

1. **Keep `mui-truce` as the only host bridge.** It is byte-identical across the snapshot
   and current main. Port `editor_mui/binding.rs` on top of it verbatim and delete
   `editor_mui.rs`, `editor_mui/panel.rs`, `editor_mui/routes.rs` and the
   `create_mui_editor` export in the same commit — ~3,900 lines gone before a single new
   line is written.
2. **Take a crates.io-shaped dependency on `mui`, never on an `experiments/` crate.**
   The pinned dependency ships a mock KURV, a demo plugin and two orphaned widget families;
   57% of it never renders. Nothing KURV builds should be able to reach `kurv.rs` inside MUI.
3. **Declare every panel, header, rail and readout; reserve absolute placement for graph
   interiors, cables and floating pies** — the contract KURV already wrote at
   `docs/MUI-COMPONENT-MIGRATION.md:118-121` and then broke ~200 times. Budget: zero `px(...)`
   outside a graph's own coordinate transform.
4. **Replace the 49-field `Shell`, `finish()` and the 13-term undo guard with one
   `Interaction`.** `mui-input`'s `Hit`/`Interaction`/`Response` already model
   press/drag/hover/drop with cancel; `Ui::edit(id)` and `Ui::get(id)` give per-id state.
   Commit deferred history when `Interaction::pressed().is_none()`, one condition instead of
   thirteen.
5. **Type the slot ids.** 374 `format!("osc-{i}-wave-graph")` strings and 71 `scene.frame`
   lookups are a runtime-string API for a compile-time tree. Give KURV an id enum whose
   `Display` produces the string, so a renamed slot is a compile error rather than a
   `"Missing DSL slot"` at runtime.
6. **One inset, one hit geometry per graph.** `groups.rs:158-290` proves the current shape
   fails: three canvases, three copies of a 2px inset, two separate boundary arrays for
   drawing and hitting. Emit graph geometry once, feed both the scene and the hit map from it.
7. **Push the readout formatting into the binding, not the view.** `SourceField::text`
   (`binding.rs:84-103`) and `group_text` (`editing.rs:683-…`) already do this correctly;
   `components.rs:29-47`'s `reserve` table keyed on the label string does not. The reserved
   width should come from the formatter's widest output, not a hand-written lookup.
8. **One minimum-width policy, declared in the tree.** Delete `rack_min = [220., 260.]`
   (`view.rs:896`) and the `- 560.` in `set_rack_width` (`view.rs:193`); let the DSL's
   `min()` be authoritative, and resolve responsively everywhere — no surviving
   `Resolved::new`.
9. **Theme through `editor_theme::semantic()` only.** Remove `SURFACE`/`WELL`/`RAISED`
   (`components.rs:2-4`), the 4 `rgb(0x…)` and the `live_theme::color(0x…)` calls.
10. **Make the routing state per-editor.** Whatever replaces `modulation::Routing` must hang
    off the view, not `cx.global`, or two plugin instances in one project share drag state.
11. **Replace the poll pump with a real frame clock, and measure it.** If the rewrite keeps
    a worker loop, `synchronize` should run on parameter/document revision change, not 250
    times a second unconditionally. Publish a measured idle and drag frame time before any
    fps claim appears in a doc again.
12. **Rewrite `tools/check_mui_editor.py` against the new hit map.** Assert on
    `Interaction` responses and resolved frames — sticky header y under scroll, no
    horizontal overflow at 1000×600 (the declared `MIN_SIZE`, `view.rs:434`), rack width
    after a resize burst — not on one PASS string.
13. **Size target.** Honest code in generation B is 11,086 lines in `editors/mui` plus
    582 kept from `editor_mui`. Removing `render`'s plumbing (~1,100), the canvas geometry
    (~1,500 across `editing.rs`/`groups.rs`/`samples.rs`/`surface.rs`), the drag-state
    machinery and the string-id layer plausibly reaches issue #8's one-third — but only if
    steps 3, 4 and 6 are enforced as gates, not aspirations. Generation B claimed the same
    contract in writing and shipped 200 absolute placements anyway.
