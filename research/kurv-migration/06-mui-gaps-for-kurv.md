# What a full synth editor needs that MUI main cannot express yet

Audit date 2026-09-16. Read-only against KURV @ `feat/preview-gallery`-era tree,
the pinned GPUI snapshot at `.worktrees/kurv-mui-runtime-0.20.37`, and MUI main at
`/mnt/Windows11/DEV_PROJECTS/Repos/MUI-migration`. Nothing was built or edited.

Every verdict below cites lines I opened. Counts are `wc -l` / `grep -c` I ran.

## The three numbers that frame everything

- KURV editor A (egui reference): **54,625** lines across `src/editor_*`
  (`find src -name 'editor_*' | xargs cat | wc -l`). Editor B (GPUI/MUI):
  **13,120** lines in `src/editors/mui/` + **3,352** in `src/editor_mui.rs` =
  **16,472**. KURV `src/` total: **186,272**.
- The pinned snapshot's `experiments/gpui-plugin` is **8,863** lines, and it is
  where the widget vocabulary actually lives: `dsl.rs:109 sticky_slot`,
  `dsl.rs:298 waveform`, `dsl.rs:543 control_point`, `dsl.rs:552 plot_bounds`,
  `dsl.rs:600 color_picker`, `dsl.rs:684 plot_position`, `dsl.rs:697 reorder_drag`,
  `dsl.rs:735 tooltip`, `text_input.rs:36 TextInput`.
- MUI main ships **five** interactive widgets, total **632** lines:
  `slider`, `knob`, `toggle`, `button`, `text_input` in
  `crates/mui-widgets/src/widgets.rs` (`lib.rs:12` re-exports the list).
  Issue #8's "a third of the size" target is ~4.4k lines. The delta between
  five widgets and the surface list below is the whole job.

## KURV's surfaces, as they actually exist

Sizes from `find src/editors/mui -name '*.rs' | xargs wc -l`:

| file | lines | surface |
|---|---:|---|
| `editing.rs` | 2371 | curve/envelope/pan editors, colour picker, popup menus |
| `view.rs` | 1938 | root shell, pane resize, sticky racks, key routing |
| `warps/mod.rs` | 1153 | processor cards, mode bodies |
| `samples.rs` | 783 | sample import job, waveform + slice painting |
| `modulators/surface.rs` | 769 | LFO/env/gate source cards |
| `routing.rs` | 672 | modulation routes, pies, depth |
| `presets.rs` | 564 | browser, search, audition, import/export, undo |
| `modulators/mod.rs` | 542 | source rack |
| `structure.rs` | 531 | drag-reorder of cards/groups/warps |
| `groups.rs` | 513 | group header, inline ADSR |
| `synth/binding.rs` | 510 | host binding + `display_samples` |
| `wavetable.rs` | 407 | wavetable library/function editor |
| `synth/pan.rs` | 340 | pan curve with segment bends |
| `curve_brush.rs` | 265 | freehand curve painting |
| `performance.rs` | 246 | global/settings page (18 fields, `performance.rs:15-33`) |
| `control.rs` | 218 | one gesture lifecycle + `history_shortcut:149` |

Interaction primitive counts inside `src/editors/mui` (grep -o | wc -l):
`modifiers` **43**, `paint_path`/`PathBuilder` **22**, `TextInput`/`field(` **26**,
`ScrollHandle`/`overflow_y_scroll` **5**, `anchored()` **1**, right-button **4**.

The 43 modifier reads are the single most load-bearing number in this document.

---

## The table

Verdicts: **Good** = expressible today, **Partial** = expressible with
KURV-side glue but something generic is missing, **Missing** = cannot be written
at all against current MUI main.

| # | KURV need | MUI main today | Verdict |
|---|---|---|---|
| 1 | Static layout: racks, cards, 3-column shell, panel/card/chip presets | `mui-layout/src/node.rs:116-135,215-330` (row/column/overlay/grid, gap, pad, size, align, anchor, offset), `mui-scene/src/dsl.rs:43-148` (`.w .h .center .between .full .join .cut .keep`), `mui-widgets/src/presets.rs` (card/chip/glass/panel/tile). `shell.rs:4-64`'s three columns with `min(180.,0.).shrink(1.)` map to `.min_width` + `.shrink` directly | **Good** |
| 2 | Responsive narrow-window layout / DAW host resize | `Ui::frame(root, offered, ..)` takes the offered size every frame (`ui.rs:371-378`); `Len::Clamp`/`cq()`, `.min_col`, `fits![..]` (`node.rs:167,402`), and `Error::InsufficientSpace` carrying the floor (`mui-layout/src/lib.rs:89-140`). KURV's `set_size` path (`editor_mui.rs:1334`) becomes one `offered` argument | **Good** |
| 3 | Knobs/sliders with host automation bracketing | `knob`/`slider` (`widgets.rs` ~235 and ~300) call `Host::drag`; `Ui::edit(id)` + `Frame::edits` give Begin/End per capture, cancelled ones included (`ui.rs:50-56,172-182`). `mui-truce::Parameter` has `begin/set/drag/end/cancel/step/reset` (`parameter.rs:104-160`) | **Good** |
| 4 | Readouts with editable text and units | `Parameter::text()` and `Parameter::parse(&str)` (`parameter.rs:99,165`) give units and round-trip; `text_input` (`widgets.rs:450-620`) gives a real caret, selection, double-click word select, clipboard and IME | **Good** |
| 5 | Live waveform previews at display cadence | `canvas(\|size\| Vec<Draw>)` (`element.rs:220`, `Draw` at `element.rs:21-44`) takes an arbitrary `Path`; `Path::polyline` (`mui-geometry/src/path.rs:265`) is exactly what `samples.rs:487-502` builds by hand. KURV already keeps `display_samples: Arc<[f32]>` (`synth/binding.rs:21,185`). **Partial**: the closure re-runs and re-tessellates every frame in the scene walk (`scene.rs:786`); `mui-vello`'s `PathCache` keys on the path, so a changing waveform reconverts. No damage/dirty region either — 16 oscillator cards at 60 Hz is the first thing that will be measured and found wanting | **Partial** |
| 6 | Modifier keys during a pointer gesture (fine drag, snap bypass, constrain) | **Nothing.** `PointerInput { pos, primary_down }` (`mui-input/src/lib.rs:105-112`) carries no modifiers. `Mods` exists (`:130-137`) but only inside `KeyPress`, and `Ui::keys(id)` returns `&[]` unless `id` is focused (`ui.rs:226-233`). KURV reads `e.modifiers.shift` for fine drag (`editing.rs:1773,1829`) and `e.modifiers.alt` to bypass snapping (`editing.rs:1804,1833`), 43 sites total | **Missing** |
| 7 | Right-click / context menus | **Nothing.** `PointerInput` has one button (`:105-112`); `Response` has no secondary anything (`:191-212`). The popup body itself is expressible — `Pin::to(anchor).area(..).fallback(..)` (`mui-layout/src/pin.rs:72-119`) is a better dropdown than GPUI's `anchored()` at `editing.rs:1750` — but there is no gesture to open it from | **Missing** |
| 8 | Modulation drag-and-drop, source pill → carrier port | `Response::drop_target` / `dropped_on` and `Ui::dropped() -> Option<(&str,&str)>` (`input/lib.rs:209-211`, `ui.rs:248`) give source and target by id. KURV's `routing.rs:8 attach` / `442 set_depth` is domain logic that stays in KURV. **Partial**: no drag *payload* beyond the source key (KURV's GPUI path carries a typed `Move` payload, `structure.rs:176`), no drag ghost following the pointer, and no way to express a cable between two resolved frames except by reading `Frame`s back out of last frame's scene and drawing a `canvas` overlay | **Partial** |
| 9 | Route depth overlays / parameter pies | Painting an arc is `Path::cubic_to` + `canvas` (`path.rs:242`), or `Arc::point_at` (`path.rs:8-38`). `.float()` (`node.rs:372`) keeps the layout slot and paints last, which is what a pie outside flow needs. **Partial**: a pie must be hit-tested as a ring segment, and `Hit::push` takes the node's painted path (`ui.rs:540`, `input/lib.rs:56`) — a `canvas` node's hit shape is its *frame*, not its drawn paths, so a pie's ring and a curve's control points get one rectangular target | **Partial** |
| 10 | Spline/envelope editors: draggable points, handles, insert/delete, snapping | The *model* is already there and is better than KURV's: `mui-motion/src/curve.rs:86-290` has `Curve`, `CubicSegment`, `move_point`, `move_handle`, `reset_segment`, `split`, `insert`, `remove`, `evaluate_wrapped`, `sample_into`, plus `CurveHistory` with undo/redo at `:333-370`. **Missing**: the *editor* — no hit test for a knot (KURV: `editing.rs:81 curve_hit`), no magnetic snapping (`editing.rs:73 magnetic_snap`), no snap guides, no per-axis grid, no freehand brush (`curve_brush.rs`, 265 lines). And it blocks on #6 for the alt/shift modifiers | **Missing** |
| 11 | Drag-reorder of cards, groups and warps | `Ui::dropped()` gives a released pair, so a reorder is expressible as drop-on-target. **Partial**: no insertion-point feedback (KURV computes above/below from `e.event.position.y > e.bounds.center().y`, `structure.rs:304`), because `Response` exposes no pointer position relative to the drop target — `Ui::local(id)` exists (`ui.rs:242`) but returns the pointer vs *any* id's frame, which is enough; what is missing is the ghost and the gap-opening animation. `.order(i32)` (`node.rs:408`) covers the post-drop reflow | **Partial** |
| 12 | Resizable side panes | **Missing** as a widget. Expressible in ~20 KURV lines: a `divider` node with `Cursor::ColResize` and `Ui::drag(id, &mut width, ..)` (`ui.rs:341`) — which is exactly what KURV does today (`shell.rs:65-72` + `view.rs:61,1804-1826`). No generic splitter in MUI, and none needed | **Missing (KURV-side, trivial)** |
| 13 | Scrolling racks | `.scroll()` (`node.rs:340`), wheel routed to the innermost scrollable surface under the pointer and clamped to content (`ui.rs:582-610`), offset read back with `Ui::scroll(id)` (`ui.rs:252`). Hits rejected outside their clip (`ui.rs:540 push_clipped`) | **Good** |
| 14 | Sticky group headers inside those racks | **Missing.** The snapshot has `sticky_slot(id, scroll, stack_top, child)` (`gpui-plugin/src/dsl.rs:109`) and KURV uses it at `view.rs:1632-1637`. MUI main has no sticky: `Pin` anchors to a *node* (`pin.rs:82`), not to a scroll viewport edge, and `.float()` leaves the flow entirely | **Missing** |
| 15 | Preset browser: list, search, audition | List + search are `col![]` over a filtered `Vec` plus one `text_input` — KURV's own filter is four lines (`presets.rs:438-455`). Audition is `preview.stop()`/start on the DSP side (`presets.rs:179,335`). **Partial**: no virtualised list (a 2000-preset column resolves 2000 nodes every frame; `mui-scene` has no windowing), and no keyboard list navigation primitive — `Key` has `Up/Down/Home/End` (`input/lib.rs:114-127`) but arrows only reach a focused node | **Partial** |
| 16 | Preset / sample / wavetable import from disk | **Missing, and correctly so.** KURV uses `rfd`-style dialogs (`presets.rs:217 add_filter`) and a bounded worker job it polls each frame (`samples.rs:85-130,158-165`). MUI has no file I/O anywhere and should keep it that way. What MUI *does* lack is any async/job story: `Ui::frame` is synchronous and there is no "repaint again in N ms" besides `Frame::animating` (`ui.rs:33`) | **Missing (belongs in KURV)** |
| 17 | Wavetable browser with per-frame function editor | Composed of #5, #15, #16 and a text field per frame (`wavetable.rs:276,358` keep a parallel `functions: Vec<String>`). Nothing new in MUI beyond those | **Partial (derives from above)** |
| 18 | Undo history UI | `mui-truce::Document` has `snapshot`, `revision`, `edit`, `restore` (`document.rs:300-365`) and `CurveHistory` covers one curve (`curve.rs:333-370`). **Missing**: no document-level undo stack, no named entries to render as a list. KURV has its own (`presets.rs:150-155,301,306`, `control.rs:149-198`) and it is domain-shaped (per-parameter redo, `ParameterRedo(u32)`) | **Missing (mostly KURV)** |
| 19 | Global keyboard shortcuts (ctrl+Z, ctrl+shift+Z, ctrl+Y) | **Missing.** `Ui::keys(id)` is focus-gated (`ui.rs:226`); `Ui` keeps `self.keys` privately and exposes no unfiltered stream. KURV's `history_shortcut(key, cmd, shift, alt, param)` (`control.rs:149-198`) must run whatever is focused. `Key` also has no F-keys, no Space, no PageUp/Down (`input/lib.rs:114-127`) | **Missing** |
| 20 | Colour picker (group tints) | **Missing.** `mui-style/src/color.rs` has a full Oklch `Color` with `hue()`, `lightness`, chroma clamping (`color.rs:13-195`) — the maths is done. There is no widget. Snapshot has one at `gpui-plugin/src/dsl.rs:600`, KURV calls it at `editing.rs:1542` | **Missing** |
| 21 | Disabled controls (bypassed module greys out) | **Missing, and on the roadmap as missing**: `ROADMAP.md:147-149` — "`State::Disabled`: nothing in `Ui` reports disabled, and the variant only makes sense beside the flag that gates hit testing." `Variant` has four members and none of them is disabled (`widgets.rs:36-70`). KURV fakes it with `.opacity(0.4)` (`view.rs:1931`) and still hit-tests | **Missing** |
| 22 | Tooltips | `.tip("..")` + the runtime's 0.5 s pinned float, `TIP_DELAY` and `Frame::tip` (`ui.rs:17-22,33-37`) | **Good** |
| 23 | Accessibility for a 200-control editor | `mui-access::tree_update(scene, focus)` (`access/lib.rs:90`), `Kind::{Button,Toggle,Slider,TextInput}` set by every widget (`element.rs:125-147`). **Partial**: no `Kind` for a group/list/menu/canvas, and no live region for the readouts | **Partial** |
| 24 | Node identity across a rebuilt tree | **Partial, and a known defect**: `ROADMAP.md:180-184` — focus rings, scroll offsets and access all key on `String` paths, so renaming a node resets its state. KURV reorders cards constantly (`structure.rs:317-402`), which is precisely the rename case | **Partial** |

---

## Ranked: what is missing, by how many surfaces block on it

| rank | missing thing | KURV surfaces blocked | est. lines in MUI | where it belongs |
|---:|---|---:|---:|---|
| 1 | **Modifiers + secondary button on `PointerInput`**, surfaced on `Response` | 7 — spline editor, pan curve, curve brush, knobs (fine drag), wavetable frame drag, reorder (constrain), every context menu | ~60 (`input/lib.rs` fields + threading through `Interaction::update` + `Ui::frame`; the preview host already decodes winit modifiers) | **MUI** |
| 2 | **Curve editor widget** on top of `mui_motion::Curve`: knot hit test, drag, handle drag, insert/delete, per-axis magnetic snap with guides | 5 — group ADSR, LFO/envelope sources, pan curve, wavetable function, warp response | ~350 (`mui-widgets/src/curve.rs`; the model and history already exist, `curve.rs:86-370`) | **MUI** (generic), KURV supplies the evaluator/units |
| 3 | **Per-node hit shapes for `canvas`**, so drawn geometry is targetable | 4 — pies, cables, curve knots, waveform scrub | ~80 (let `Canvas` return hit paths alongside `Draw`, push them in `ui.rs:538-541`) | **MUI** |
| 4 | **Unfocused / global key stream** (`Ui::shortcuts() -> &[KeyPress]`), plus F-keys and Space in `Key` | 4 — undo/redo, delete-selected, structural keyboard edits (`structure.rs:119`), preset list nav | ~40 | **MUI** |
| 5 | **Sticky headers** inside a scroll viewport | 2 — group rack, source rack | ~70 (a `Pin::Area::ViewportTop` variant resolved against the enclosing scroll frame in `arrange.rs`) | **MUI** |
| 6 | **`State::Disabled` + a hit-test gate** | 4 — bypassed oscillators, unavailable warp modes, illegal routes, locked presets | ~60 (a `.disabled()` on `Element`, skipped in `Hit`, a fifth `State`) | **MUI** |
| 7 | **Drag payload + drag ghost** (typed payload keyed to the drag, a float that follows the pointer) | 3 — modulation drag, card reorder, wavetable frame reorder | ~90 | **MUI** |
| 8 | **Colour picker widget** | 1 — group tint (`editing.rs:1542`) | ~150 (`mui-widgets`; `mui-style::Color` already does Oklch, `color.rs:13-195`) | **MUI** — it is generic and the maths is already there |
| 9 | **Canvas / path caching keyed on content**, plus a repaint budget | 1 surface but ~16 live instances — oscillator cards at display cadence | ~120 (hash the `Vec<Draw>`, reuse `mui_vello::PathCache`) | **MUI** |
| 10 | **Virtualised list** (render only the visible window of a scrolled column) | 2 — preset browser, wavetable library | ~120, or 0 if KURV slices the `Vec` itself using `Ui::scroll(id)` and a known row height | **KURV first**, promote to MUI only if a second caller appears |
| 11 | **Document-level undo with named entries** | 2 — history UI, preset recall | ~0 in MUI | **KURV** — the semantics (per-parameter redo, deferred commits) are domain |
| 12 | **File dialogs / async import jobs** | 3 — presets, samples, wavetables | ~0 in MUI | **KURV** — keep MUI free of I/O; KURV polls its worker between frames as it already does (`samples.rs:88`) |
| 13 | **Resizable pane splitter** | 1 — the three-column shell | ~0 in MUI | **KURV** — `Ui::drag` + `Cursor::ColResize` is ~20 lines, no widget needed |
| 14 | **Stable node identity across reorder** | all of them, quietly | ~100 (a `key` distinct from the path, threaded through `springs`, `scrolls`, `sel`, `focus`) | **MUI** |

Sum of the MUI-side estimates: **~1,240 lines**, roughly doubling `mui-widgets`
(816) and adding ~15% to the workspace's interactive core. That is the honest
price of issue #8's 4.4k-line KURV editor.

---

## Opinionated verdicts

**Good — MUI main's layout and styling vocabulary is already ahead of the
snapshot.** `shell.rs:4-64` is 60 lines of GPUI-flavoured builder that
translates almost token-for-token into `row![..].gap(8.).pad(2.)` with
`.min_width(180.).shrink(1.)`. `.cut`/`.keep`/`.weld`/`.shell` (`element.rs`
`Paints`) express KURV's convex/concave shell language, which
`MUI-COMPONENT-MIGRATION.md:120-126` says the current editor has two competing
sources of geometry for. Nothing in KURV's layout needs new MUI layout.

**Bad — the input model is a demo's input model.** `PointerInput { pos,
primary_down }` (`input/lib.rs:105-112`) cannot express a synth editor. Not
"is awkward for": cannot. 43 modifier reads in `src/editors/mui` and 4
right-button reads have no expression at all. This is one struct, two fields and
a thread-through, and it is the single highest-leverage change in the workspace.

**Bad — `canvas` is write-only.** `element.rs:220` paints anything and responds
to nothing but its bounding box. Every graph KURV has — knots, handles, pies,
cables, slice markers (`samples.rs:487-582`, 22 `paint_path` sites) — needs its
drawn geometry to be the hit geometry. `Hit::push` already takes an arbitrary
`Path` (`input/lib.rs:56`) and `Path::flatten` already exists
(`path.rs:104`); the plumbing is 80 lines and it is simply not wired.

**Good — `mui_motion::Curve` is a real gift and KURV should delete code for
it.** `curve.rs:86-290` plus `CurveHistory:333-370` is a cleaner cubic model
than KURV's `WaveCurveData` + `WaveEdit` (`editing.rs:44-180`, and `editing.rs`
is 2,371 lines). `MUI-COMPONENT-MIGRATION.md:145` warns against "an invented
universal Bézier data model" — that warning is now stale: the model exists, is
tested, and KURV's segment bends map onto `move_handle`/`reset_segment`.

**Missing — nothing in MUI knows what a plugin editor window is.**
`mui-truce` is 574 lines of `Document` + `Parameter` and nothing else
(`mui-truce/src/lib.rs`), `mui-vello` has no window handle, and the only host is
`mui-preview/src/host.rs` (238 lines, winit). KURV's `editor_mui.rs:1257
request_resize` / `:1334 set_size` has no counterpart. Someone has to write a
`raw_window_handle`-parented Vello surface and a `truce::Editor` impl before a
single pixel of the rewrite is visible in a DAW. Nobody has scoped it here, and
it is not in `ROADMAP.md`'s Missing list.

**Redundant — three of KURV's "needs" should not become MUI features.**
The splitter (#12), the file import (#16) and the undo stack (#18) all have
domain shape and one caller. `docs/MUI-COMPONENT-MIGRATION.md:60-63` already
says KURV owns musical semantics and persistence; the splitter joins that list
because `Ui::drag` covers it in 20 lines.

**Bad — `State::Disabled` is listed as missing on the roadmap
(`ROADMAP.md:147`) and it is not a cosmetic gap.** A bypassed oscillator in
KURV today is `.opacity(0.4)` (`view.rs:1929-1938`) over a fully live hit
target. On MUI that would be a control the user can still drag while it is
greyed out, which is worse than either editor A or B.

**Bad — string-path identity (`ROADMAP.md:180-184`) meets a UI whose primary
gesture is reordering.** `structure.rs:317-402` moves modules between groups,
warps within a rack, and groups within the stack. Every one of those renames the
path of a subtree, dropping its springs, its scroll offset and its focus.
The roadmap files this under node identity; from KURV's side it is a correctness
bug that will be reported as "the rack jumps when I drag a card."

---

## What the rewrite should do about it

1. **Land modifiers and a secondary button on `PointerInput` before anything
   else.** `{ pos, primary_down, secondary_down, mods: Mods }`, threaded through
   `Interaction::update` (`input/lib.rs:290`) onto `Response`, and populated by
   the host. ~60 lines, unblocks 7 surfaces, and every later widget assumes it.
2. **Give `canvas` hit paths in the same call that gives it `Draw`s.** Then
   build pies, cables and knots as ordinary keyed surfaces with `Ui::local(id)`
   for the pointer, instead of the `plot_bounds`/`plot_position` pair the
   snapshot needs (`gpui-plugin/src/dsl.rs:552,684`).
3. **Write `mui_widgets::curve` on top of `mui_motion::Curve`**, with knot hit
   test, handle drag, insert/split/remove, per-axis magnetic snap and snap
   guides. Port `magnetic_snap` (`editing.rs:73`) and `curve_hit`
   (`editing.rs:81`) verbatim — they are generic and 60 lines between them.
   Leave the freehand brush (`curve_brush.rs`, 265 lines) in KURV until a second
   caller wants it.
4. **Add `Ui::shortcuts()` returning the unfocused key stream**, and extend
   `Key` with F-keys, Space and PageUp/Down. Then `control.rs:149-198` moves
   across unchanged and stops needing GPUI's `key_context`.
5. **Add `State::Disabled` and `.disabled()` together**, with the flag gating
   `Hit::push` in `ui.rs:538-541`. Do not ship the variant without the gate.
6. **Add sticky as a `Pin` area resolved against the enclosing scroll frame**,
   not as a second positioning system. Two racks need it; `sticky_slot`'s
   `stack_top` parameter (`gpui-plugin/src/dsl.rs:109`) is the thing not to copy.
7. **Add a typed drag payload and a ghost float.** `Ui::dropped()` returning
   `(&str, &str)` (`ui.rs:248`) is enough for reorder-by-id, but the modulation
   drag wants a source kind and the reorder wants an above/below hint; both are
   cheap once the payload exists.
8. **Promote the colour picker into `mui-widgets`.** `mui-style::Color`
   (`color.rs:13-195`) already does Oklch with legibility clamping — a picker
   that edits `Color` directly is better than the snapshot's `[u8;3]` one
   (`gpui-plugin/src/dsl.rs:600`).
9. **Fix node identity before KURV ships reordering.** A stable key separate
   from the tree path, honoured by `springs`, `scrolls`, `sel` and `focus`.
   Otherwise every reorder gesture visibly resets the cards it moved.
10. **Keep in KURV, explicitly:** the splitter, file dialogs and import jobs,
    the document undo stack with per-parameter redo, preset audition, route
    legality (`routing.rs:8,442`), parameter formatting and units, and the
    virtualised preset list until something else needs it. Write these as KURV
    code and resist promoting them.
11. **Scope the host.** A `truce::Editor` impl with a parented Vello surface,
    `set_size` → `Ui::frame(root, Some(offered), ..)`, `Frame::animating` →
    repaint scheduling, `Frame::edits` → `Automation::dispatch`
    (`parameter.rs:21`), `Frame::clipboard`/`ime` → the host. This is not on
    `ROADMAP.md` and it gates everything visible.
12. **Measure the waveform cards before optimising them.** 16 `canvas` nodes
    re-running their closure and re-tessellating every frame (`scene.rs:786`) is
    the plausible first performance wall. `PathCache` exists
    (`mui-vello`, `ROADMAP.md` Done); a content hash on `Vec<Draw>` is the lazy
    fix. Do not build damage regions until a number says to.
