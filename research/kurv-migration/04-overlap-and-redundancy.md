# 04 — Overlap and redundancy across KURV's three editor generations

Audit date 2026-09-16. Read-only. Every count below comes from `wc -l` / `grep -c`
run against the three trees named here; every claim carries `file:line`.

The three trees:

| Gen | Tree | Lines | Reachable how |
|---|---|---|---|
| A | `KURV/src/editor_*.rs` + `src/editor_*/` **minus** `editor_mui*` | **49,898** (121 files total 54,412, minus 4,514) | `src/editors/mod.rs:16-23` → `crate::editor::create` |
| A′ | `KURV/src/editors/egui.rs` + `editors/egui/` | 1,000 | same |
| A″ | `KURV/vendor/plugcat` | 5,932 | `Cargo.toml:120`, always compiled |
| B.0 | `KURV/src/editor_mui.rs` + `src/editor_mui/` | **4,514** | *only* `examples/mui_editor.rs:65` |
| B | `KURV/src/editors/mui/` | **13,145** | `src/editors/mod.rs:9-15` → `crate::mui_shell::create` (`lib.rs:12-14`) |
| B′ | `.worktrees/kurv-mui-runtime-0.20.37/experiments/gpui-plugin` | 8,863, of which **3,661 consumed** | `Cargo.toml:142` |
| C | `MUI-migration/crates/{mui,mui-scene,mui-style,mui-widgets,mui-input,mui-motion,mui-truce}` | 9,232 | target |

Total editor-ish code KURV compiles today: **68,489 lines** (A+A′+A″+B.0+B) plus
8,863 of pinned MUI experiment. The rewrite target in MUI issue #8 is ~4,400.

---

## 0. The structural finding that outranks every widget

`src/lib.rs:19-38` declares all 20 `editor_*` modules **unconditionally**. The
`legacy-egui` feature is `legacy-egui = []` (`Cargo.toml:114`) — an empty marker
that only flips which `create` is called in `src/editors/mod.rs:9-23`. Gen A is
not behind a feature gate; it is in every shipped binary, next to gen B.

Worse, gen B cannot be built without it. `grep -ohE 'crate::editor_[a-z_]+' src/editors/mui`
returns 92 hits across 10 gen-A modules, and `crate::editor::` another 58:

```
 51  crate::editor::notify_persisted_state_changed
  5  crate::editor_oscillator::processor_preview
  4  crate::editor_lfo::source::set_source_active
  4  crate::editor_generator::remove_module
  3  crate::editor_lfo::source::source_config
  3  crate::editor_lfo::source::active_source_mask
  3  crate::editor_history::EditorHistory
  3  crate::editor_filter::{processor_label, format_morph_rotation}
  2  crate::editor_presets::{EXTENSION, atomic_write}
 41  crate::editor_theme::{space,shape,font}
```

**Verdict: Bad.** Every one of those 20 symbols is *domain* logic — state
mutation, file IO, unit formatting, spacing constants — filed inside an egui
*view* module. That misfiling is the single reason the legacy editor cannot be
deleted, and it is the reason the gen-B rewrite came out at 13k lines instead of
the promised third.

**Where it should live:** a `kurv::editor_model` (or `kurv::ui`) domain crate/module
with no frontend dependency, holding `EditorHistory`, source/group/module mutation,
preset IO, unit formatting, and the token *values* (not the egui types). Gen A then
deletes cleanly.

---

## 1. Theme and tokens — three systems, none of them the source of truth

| Copy | File | Lines | What it is |
|---|---|---|---|
| A | `KURV/src/editor_theme.rs` + `editor_theme/library.rs` | **834** | egui `Color32`, `space/font/shape/dim` const modules, `semantic() -> KurvPalette`, `control_visuals`, WCAG contrast fix-up, 8 group accents |
| A″ | `vendor/plugcat/src/theme.rs` + `widgets/tokens.rs` | 722 | `UiTheme`/`ThemeTokens`/`WidgetTokens` that A wraps (`editor_theme.rs:6-8`, `:484-543`) |
| B′ | `gpui-plugin/src/live_theme.rs` | **146** | thread-local `Colors`, a **hex-value-as-role dispatch table** (`live_theme.rs:88-106`: `0x171d1a => canvas`, `0xbadc91 => primary[0]` …) |
| B | `src/editors/mui/components.rs:2-4` | 3 | `SURFACE/WELL/RAISED` as literal `Rgb(29,29,29)` etc. |
| C | `mui-style/{theme.rs,color.rs,style.rs}` | **1,761** | `Theme{corners,spacing,palette,stroke_width,text,control}`, OKLCH `Pigment`/`Palette`, `Role` enum with 13 members, `Fill::paint` |

Gen B is the worst of all worlds: it imports gen A's egui spacing/typography
constants 41 times (`header.rs:9-10`, `components.rs:47`), defines three raw
surface colours of its own, and calls `live_theme::color(0xbadc91)` — a *hex
literal used as a role key* — 7 times. `grep -coE 'Rgb\([0-9]+, *[0-9]+, *[0-9]+\)|0x[0-9a-fA-F]{6}' src/editors/mui` = **34 raw colour literals**, in a
codebase whose own `AGENTS.md` says "Use `editor_theme::semantic()` roles, never
literal UI colors."

**Best single source of truth: C** (`mui-style`). It is the only one with a real
role enum (`style.rs:28-45`), perceptual colour, and a const-spreadable `Theme::DEFAULT`
(`theme.rs:110-135`). `live_theme.rs`'s contrast-resolution idea is already in
`mui-style` (`Palette` carries `step`/`hover`, `Role::Ink`/`Dim` resolve against
what they sit on, `style.rs:60-61`).

**Verdicts.** `editor_theme.rs` → **Redundant**, delete 834 lines; port only two
things: the *numbers* of `space`/`font`/`shape` (`:202-261`, ~60 lines) into one
KURV `Theme` const, and `group_accents()` (`:452-482`, 31 lines) which `mui-style`
genuinely cannot do — `Palette` has exactly three accent slots (`color.rs:361-367`)
and KURV needs 8 hue-stepped, contrast-corrected group accents. That is domain,
keep it in KURV. `live_theme.rs` → **Delete** (146). `components.rs:2-4` → **Delete**.
plugcat's theme layer → **Delete with gen A**.

**Missing in C:** nothing for this family except a documented way to extend a
palette with N indexed accents. One `fn accents(&self, n) -> Vec<Color>` on
`Palette` would close it; until then KURV owns it.

---

## 2. Gestures — three implementations, only one of them correct

| Copy | File | Lines | Shape |
|---|---|---|---|
| A | `src/editor_controls/parameter_gesture.rs` | **727** | `ValueSemantic` (24 variants, `:20-48`), `update_parameter_drag` (`:99-194`), keyboard nudge (`:195-240`), snapping (`:241-268`, `:484-607`), three more drag entry points (`:336`, `:370`, `:434`) |
| B | `src/editors/mui/control.rs` | 218 | `Gesture::{Begin,Reset,Step}` + gpui `on_mouse_down`, delegating to `editor_mui::binding::Binding` (from the **dead** B.0 tree) |
| B′ | `gpui-plugin/src/controls.rs` | 83 | gpui `actions!` keybindings + `parameter()` wrapper; arrow/shift-arrow only |
| C | `mui-input` | **760** (395 + 365 test) | `Hit`/`Interaction`/`Response`, press-captures-target (`lib.rs:214-221`), drag threshold, IME, key presses |
| C | `mui-widgets` `Host::drag` | `lib.rs:47-54` | one drag → value mapping used by `knob`/`slider` |

Gen B's gesture layer is **not** one implementation: `control.rs:37-105` owns
press/double-click, `gpui-plugin/controls.rs:64-83` owns keyboard, and
`editor_mui/binding.rs:115+` owns stepping/reset semantics per model — three
files, two repos, for one gesture.

**Best single source of truth: C.** `mui-input::Interaction` is the only copy
with pointer capture, which is the one thing that makes a knob not break when
the cursor leaves it (its own doc comment says so, `mui-input/src/lib.rs:216-221`).
`Host::drag` gives pixels-per-span; `Ui::edit` (`mui/src/ui.rs:172-177`) gives the
host `Begin`/`End` bracket.

**Verdicts.** `controls.rs` (83) → **Delete**, `mui-input` covers it. `control.rs`
(218) → **Delete**, keep only `history_shortcut` (`:138-167`, 30 lines) as domain.
`parameter_gesture.rs` (727) → **Split**: `ValueSemantic` + `semantic_snap` +
`snap_pitch_landmark`/`snap_125`/`magnetic_shape_snap` (`:484-607`, ~124 lines) are
genuine KURV musical knowledge and move to the KURV domain module; the remaining
~600 lines of egui plumbing are **Redundant**.

**Missing in C:** value *snapping* as a first-class concept. `Host::drag` has
`fine`-style behaviour nowhere — `mui-widgets/widgets.rs:235-351` never reads a
modifier. KURV needs shift=fine, alt=bypass-snap, ctrl=coarse (`parameter_gesture.rs:122`,
`:140`). Either `Host::drag` grows a `Mods` argument or KURV wraps it. Add it to
`mui-input`/`mui-widgets`, not to KURV.

---

## 3. Curve and spline math — five copies of a cubic

| Copy | File | Lines | Model |
|---|---|---|---|
| domain | `KURV/src/wave_curve.rs` (+ `bandlimit`, `function`, `lfo`) | 2,187 + 396 + 881 + 395 | `WaveCurveData`/`WaveKnot`, `shape_segment_progress` bend (`:340-405`), RT compile + SIMD `eval4`/`eval8` (`:948-1035`), `insert_knot`/`move_knot`/`remove_knot`/`set_segment_bend` (`:1514-1600`) |
| A | `src/editor_lfo/spline_editor.rs` + `spline_editor/{interaction,painting}.rs` | **911 + 665 = 1,576** | egui hit-testing/painting over `WaveCurveData` |
| A | `src/editor_oscillator/va_table.rs` + `va_table/curve_editor.rs` | 1,807 (contains a *second* curve editor, `va_table.rs:3-7`) | same data, wavetable framing |
| B | `src/editors/mui/editing.rs` | 2,371, **186 `curve` hits** | gpui hit-testing over the same `WaveCurveData` |
| B | `src/editors/mui/curve_brush.rs` | 265 | freehand paint into `WaveCurveData` |
| B′ | `gpui-plugin/src/curve_editor.rs` | **687** | a *different* model, **unused by KURV** (0 references) |
| B′/C | `mui-core/src/curve.rs` ≡ `mui-motion/src/curve.rs` | **490, byte-identical** | `Curve`/`CurvePoint`/`CurveHandles`/`CubicSegment`, plus its own `CurveHistory` (`curve.rs:333-370`) |

`diff` confirms `mui-core/src/curve.rs` and `mui-motion/src/curve.rs` are
identical, so gen B and gen C carry the same 490-line spline — and neither editor
ever uses it, because KURV's curves are `WaveCurveData`, whose bend parameterisation
(`curve`, `curve_x` per segment, `wave_curve.rs:340-434`) is not cubic Béziers.

**Best single source of truth: `wave_curve.rs`.** It is the evaluator playback
uses, which `AGENTS.md` ("Graphs and curves") makes mandatory. Everything else
is a *view* of it.

**Verdicts.** `gpui-plugin/curve_editor.rs` (687) → **Delete**, dead. `mui-motion::Curve`
(490) → **Redundant for KURV**; keep in MUI only if the gallery needs it, but
`CurveHistory` (`curve.rs:333-370`, 38 lines) is a second undo stack and should go
regardless — see §5. `spline_editor*` (1,576) + `va_table/curve_editor.rs` → **delete
with gen A**, after extracting the interaction *rules* (snap, shift-fine, alt-bypass,
double-click add/remove) which are already restated in `editing.rs`. `curve_brush.rs`
(265) → **Good, keep**: it is pure `WaveCurveData` math with no frontend import
(`curve_brush.rs:1`), and it belongs in `kurv::wave_curve` beside `insert_knot`.

**Missing in C:** nothing structural. `mui_scene::canvas(|Size| -> Vec<Draw>)`
(`mui-scene/src/element.rs:220`) is the right escape hatch for a graph, and
`Hit::push_clipped` (`mui-input/src/lib.rs:63`) gives per-knot hit regions. The
curve editor becomes a KURV `canvas` closure over `WaveCurveData` plus N knot hit
ids — realistically 250-350 lines replacing 2,528.

---

## 4. Widgets — implemented two to four times each

Counts are the implementation site, not every call site.

| Widget | A (egui) | A″ (plugcat) | B (gpui) | C (MUI) | Best | Destination |
|---|---|---|---|---|---|---|
| Knob | via `plugcat::widgets::knob` | **680**, `widgets/knob.rs` | none (bare div + `control.rs`) | `mui-widgets/widgets.rs:299-351`, **53** | **C** | `mui-widgets` |
| Slider | `editor_card/readout.rs` cells | 72, `widgets/slider.rs` | `view.rs`/`samples.rs` ad hoc | `widgets.rs:235-298`, **64** | **C** | `mui-widgets` |
| Toggle / power | `editor_widgets.rs:158` `paint_power_icon` | **1,117**, `widgets/toggle.rs` | `editing.rs` inline | `widgets.rs:379-407`, **29** | **C** | `mui-widgets` |
| Text input | egui built-in | — | `gpui-plugin/text_input.rs` **669** | `widgets.rs:452-586`, **135** + IME | **C** | `mui-widgets` |
| Card header | `editor_card/header.rs` **765** + `header/group_tab.rs` 289; `editor_generator/group_output.rs:56-159` | `widgets/chrome.rs` 206 | `gpui-plugin/group_header.rs` **624** (unused by KURV); `editors/mui/header.rs` 71 | `presets::card` `mui-widgets/presets.rs:33-54`, **22** | **C** for the shell, **KURV** for the slots | `mui-widgets::card` + KURV `module_header()` |
| Readout / value cell | `editor_card/readout.rs` **717**, `editor_resynth/readouts.rs` **1,144** | `widgets/tokens.rs` | `synth/binding.rs:240-312` (42 `format!`), `components.rs:26-50` reserve-width table | — | **none yet** | one KURV `readout(ParamInfo, value)` |
| Pill / port | `editor_ports/pill.rs` 258 (+752 tests) | — | `gpui-plugin/modulation.rs` **1,948** (marker/anchor/port) | — | **B′, extracted** | KURV domain + `mui-widgets::chip` |
| Pie (depth) | `editor_ports/pie.rs` 397 | — | `gpui-plugin/pie_container.rs` 138 (unused) + `modulation.rs` | — | **B′** | KURV `canvas` |
| Cable | `editor_ports/{cable,geometry,paint}.rs` 79+448+312 | — | `gpui-plugin/modulation.rs` | — | **A geometry** | KURV domain (`geometry.rs` is pure math) |
| HSV picker | `egui::widgets::color_picker` (`editor_generator/group_output/identity.rs:91-94`) | **445**, `widgets/color_picker.rs`, **0 KURV references** | `gpui-plugin/dsl.rs:600-663`, 64 | — | **B′ shape** | `mui-widgets` |
| Browser list | `editor_shell/browser.rs` **1,804**, `editor_presets/` 1,857, `editor_manual/` 2,076 | — | `presets.rs` 564 + `samples.rs` 783 + `wavetable.rs` 407 | — | **B** | KURV, on `El::scroll()` |
| Settings panel | `editor_shell/settings.rs` **980** | — | `performance.rs` 246 + `shell.rs` 378 | — | **B** | KURV |
| Waveform/response plot | `editor_oscillator/preview.rs` 598, `editor_filter/painting.rs` 684 | `widgets/meter.rs` 554, `scope.rs` 122 | `gpui-plugin/dsl.rs:298-542` (`waveform`, `response`, `axis_cross`, `control_point`) | `canvas()` | **C primitive + KURV content** | KURV `canvas` closures |

**Verdicts.**

- **plugcat widgets are dead.** `grep -ohE 'plugcat::[a-z_]+' src` returns exactly
  four symbols, all in `editor_theme.rs:6-8`: `layout::UiMetrics`, `theme::{ThemeTokens,UiTheme,mix}`,
  `widgets::{WidgetColors,WidgetRadius,WidgetSpacing,WidgetStroke,WidgetTokens}`.
  `knob.rs` (680), `toggle.rs` (1,117), `meter.rs` (554), `color_picker.rs` (445),
  `slider.rs` (72), `button.rs` (161), `segmented.rs` (130), `surface.rs` (86),
  `scope.rs` (122), `motion.rs` (50) and `window/` (1,155) — **4,572 lines never
  called from KURV** — compile on every build. **Bad. Delete the dependency.**
- **`gpui-plugin` is half KURV.** `kurv.rs` (580) is literally
  "KURV's placement, local state, local modulation routing mock" (`kurv.rs:1`),
  `oscillator.rs` (1,756) is a KURV oscillator card, `modulation.rs` (1,948) is
  KURV's routing UI. `MUI-COMPONENT-MIGRATION.md:137-141` already calls this out
  ("KURV is the owner and build target of its editor, never an oscillator
  implementation inside MUI's experiment"). **Bad, still true.**
- **The genuinely reusable ~8% of `dsl.rs`** — `reorder_drag`/`reorder_handle`
  (`:697-734`), `tooltip` (`:735`), `vertical_label` (`:232-251`), `fitted_label`
  (`:664`) — is the only part worth porting, and three of those four already exist
  in C (`Ui` owns tooltips via `Pin`, `Ui::dropped` owns drag-and-drop,
  `mui/src/ui.rs:248-250`). **Port `vertical_label` only.**
- **Value formatting is written four times** (`readout.rs`, `readouts.rs` 12
  `format!`, `synth/binding.rs` 42 `format!`, `editing.rs` 65 `format!`) while
  `mui_truce::Parameter::text()` (`mui-truce/src/parameter.rs:99-103`) already
  asks Truce for the host's own string. **Redundant.** One KURV formatter, keyed
  off `ParamInfo`/`ValueSemantic`, called from one readout widget.

---

## 5. Undo / history and host-edit plumbing — the one thing that is *not* duplicated

- `src/editor_history.rs` (**821**) — `EditorSnapshot::capture`/`apply` (`:55`, `:229`),
  `EditorHistory::{commit,undo,redo,parameter_undo,parameter_redo}` (`:372-464`),
  retained-byte trimming (`:308`, `:524`), `handle_shortcuts` (`:470`).
- Gen B **uses it directly**: `src/editors/mui/view.rs:38,458` and `groups.rs:449`.
- `mui-truce/src/parameter.rs` is **byte-identical** between the pinned snapshot
  and MUI main (`diff` clean). `Automation::dispatch` (`:21-36`) and `Edit`
  (`:9-15`) own only the host `begin/set/end` bracket — not undo.
- `mui/src/ui.rs:172-177` exposes `Ui::edit(id) -> Option<Edit>`, a *second* type
  named `Edit` with the same `Begin`/`End` meaning. **Name collision, not logic
  duplication** — but a KURV file importing both will be unreadable.
- `mui-motion::curve::CurveHistory` (`curve.rs:333-370`) is a second, unused
  undo stack.

**Verdict: Good.** History is the only family with a single implementation.
`EditorHistory` is the source of truth; it has no egui import and moves to the
KURV domain module unchanged. Delete `CurveHistory` (38). Rename `mui::Edit` →
`mui::Gesture` (or re-export `mui_truce::Edit` and drop MUI's) so the two do not
collide in KURV's imports.

---

## 6. Dead code reachable from neither shipped editor

| What | Lines | Evidence |
|---|---|---|
| `src/editor_mui.rs` + `src/editor_mui/` | **4,514** | only caller is `examples/mui_editor.rs:65`, behind an argv flag; `src/editors/mod.rs` never reaches it. Yet `editors/mui/control.rs:22` imports `editor_mui::binding`, so it cannot simply be deleted — extract `binding.rs` (196) first. |
| `vendor/plugcat` widgets + window | **4,572** | §4, `grep plugcat::` = 4 symbols |
| `gpui-plugin/oscillator.rs` | 1,756 | referenced only by `editor_mui.rs:15-18` (dead) and `gpui-plugin/main.rs` |
| `gpui-plugin/{kurv,curve_editor,group_header,panel,main,pie_container,module_shell}.rs` | 3,156 | 0 references from `KURV/src` |
| `src/wave_curve/compiler_experiment.rs` | **4,080** | `#[cfg(test)]`, `wave_curve.rs:5-7` |
| `src/wave_curve/phase_prism.rs` (+ nested `warp_research.rs` 1,200) | 1,591 | `#[cfg(test)]`, `wave_curve.rs:12-14`; `warp_research.rs` is `#[path]`-included only from `phase_prism.rs:175` |
| `src/editor_ports/{param_tests,pill_tests,cable_tests}.rs` | 2,316 | tests for a module gen B does not use |
| `src/editor_manual/` | 2,076 | egui-only manual overlay; gen B has no equivalent and never calls it |

That is **~24,000 lines** that ship, or block deletion, without either editor
reaching them. The `wave_curve` research modules are honest `cfg(test)` and cost
only test time; the rest is compile time and reviewer attention.

---

## 7. What the rewrite should do about it

1. **Cut the domain out of gen A first, before writing any MUI code.** Create
   `kurv::editor_model` and move, unchanged: `editor_history.rs` (821),
   `editor_lfo::source` (352), `editor_generator::{remove_module,remove_generator_group}`,
   `editor_presets::{storage,context}` (610), `editor_filter::{processor_label,format_morph_rotation}`,
   `editor_oscillator::processor_preview`, `editor_unison::normalized_unison_rate`,
   `editor_resynth::browse_source`, `editor::notify_persisted_state_changed`.
   Gen B's 150 `crate::editor*` references then point at a frontend-free module.
2. **Delete gen A in one commit** once (1) lands: 49,898 + 1,000 lines. Do not
   keep it "for study" in-tree; it is a git tag, not a module.
3. **Drop the `plugcat` dependency** (`Cargo.toml:120`). Port the ~60 numbers in
   `editor_theme.rs:202-261` into one `const KURV: Theme` on `mui_style::Theme::DEFAULT`
   and delete 5,932 + 834 lines.
4. **Delete `src/editor_mui.rs` + `src/editor_mui/`** (4,514) after lifting
   `binding.rs`'s `SourceField`/`Binding` (196) into the domain module, and delete
   `examples/mui_editor.rs`'s non-`--shell` branch.
5. **Take every generic control from `mui-widgets`**: knob (53), slider (64),
   toggle (29), text_input (135), button (27), `presets::{card,panel,chip,tile}`.
   Do not re-implement one. That replaces plugcat's 2,700 lines of widgets and
   `gpui-plugin/text_input.rs`'s 669 with 308.
6. **Every graph is a `mui_scene::canvas` closure over the playback evaluator.**
   One `curve_canvas(&WaveCurveData)` replaces `spline_editor*` (1,576),
   `va_table/curve_editor.rs`, `gpui-plugin/curve_editor.rs` (687) and the 186
   curve branches in `editing.rs`. Keep `curve_brush.rs` (265) as domain math.
7. **Delete `mui-motion::Curve` and `CurveHistory`** (490) unless the gallery
   needs them: KURV's curve is `WaveCurveData`, and a second undo stack in a
   motion crate is a trap.
8. **One readout, one formatter.** `fn readout(&ParamInfo, f64, ValueSemantic) -> El`,
   fed by `mui_truce::Parameter::text()` where Truce already knows the units.
   Deletes `editor_card/readout.rs` (717), `editor_resynth/readouts.rs` (1,144),
   the 42 `format!`s in `synth/binding.rs` and the reserve-width table in
   `components.rs:26-50`.
9. **Keep the modulation graph in KURV, take only the visuals from MUI.** Port
   `editor_ports/geometry.rs` (448, pure math, no egui) and the pill/pie *shapes*
   from `gpui-plugin/modulation.rs`; the routing legality and mutation stay KURV's.
   Budget ~700 lines against today's 5,217 (A) + 1,948 (B′).
10. **Push three things up into MUI before starting**, because KURV will need
    them on day one and hacking around them is how gen B got to 13k:
    modifier-aware drag (shift-fine / alt-bypass-snap) on `Host::drag`;
    N indexed accents on `Palette`; and an embedded-window host adapter beside
    `mui-preview/src/host.rs` (whose own doc at `:3-6` says the only other
    consumer is KURV).
11. **Rename `mui::Edit` → `mui::Gesture`** so it stops colliding with
    `mui_truce::Edit`. KURV imports both in the same file today
    (`editors/mui/view.rs:28`, `synth/binding.rs`).
12. **Budget check.** Deletions above: 49,898 (A) + 1,000 (A′) + 5,932 (plugcat)
    + 4,514 (B.0) + 4,912 (unused `gpui-plugin`) + 490 (`mui-motion::Curve`)
    = **66,746 lines**. New KURV editor on gen C, with the domain module split
    out and every control taken from `mui-widgets`, should land at 4,000-5,000
    view lines plus ~3,500 domain lines that were never view code to begin with.
    That is issue #8's "a third of the size", and it is only reachable if step 1
    happens first.
