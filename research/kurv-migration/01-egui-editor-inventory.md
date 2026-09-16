# Generation A inventory — KURV's egui editor
Scope: every `src/editor_*` module plus `src/editors/egui*` in
`/mnt/Windows11/DEV_PROJECTS/Repos/KURV`, excluding `src/editor_mui*` and
`src/editors/mui/` (generation B). `.delta/` ignored.

Measured, not estimated:

```
118 files      50,898 lines     (task brief said "~54k"; the 3k gap is editor_mui*)
 14,282 lines  generation B under src/editors/mui + src/editor_mui  (+3,352 editor_mui.rs = 17,634)
  6,684 lines  inside #[cfg(test)] blocks in generation A (13.1%), 154 #[test] fns
    719 sites  absolute placement (pos2/Rect::from_*/allocate_exact_size/allocate_space)
     28 sites  Color32:: literals outside editor_theme — of which ~9 are real bypasses
     55        `const NAME: f32 = <number>` declarations, 15 of them inside editor_theme
  4,964 lines  in 22 files with zero egui type usage (the portable core)
```

`src/editors/mod.rs:8-24` selects the frontend; `Cargo.toml:102,112-113` makes
`mui-editor` default and `legacy-egui` an explicit opt-out. Generation A is
still the fallback on every non-Linux target, so "delete it" is not yet a
one-line change.

---

## Surface table

| # | Surface | Files | Lines | abs | Color32 | theme refs | tests | Verdict |
|---|---|---:|---:|---:|---:|---:|---:|---|
| 1 | Shell / header / browser / settings (`editor_shell*`, `editors/egui*`) | 12 | 5,357 | 112 | 3 | 235 | 11 | **Bad** |
| 2 | Card kit (`editor_card/`) | 16 | 6,580 | 157 | 3 | 149 | 30 | **Redundant** |
| 3 | Generator rack (`editor_generator/`) | 14 | 7,607 | 101 | 5 | 156 | 16 | **Mixed** |
| 4 | Oscillator VA table + unison (`editor_oscillator/`, `editor_unison/`) | 16 | 5,536 | 82 | 3 | 137 | 18 | **Mixed** |
| 5 | Resynth / Sample / Grain (`editor_resynth/`) | 3 | 2,562 | 40 | 1 | 39 | 4 | **Mixed** |
| 6 | Filters / distortion / warps (`editor_filter/`, `editor_distortion.rs`, `editor_native.rs`) | 5 | 3,126 | 41 | 1 | 82 | 1 | **Mixed** |
| 7 | LFO / envelope / spline (`editor_lfo/`) | 15 | 5,994 | 55 | 4 | 123 | 5 | **Good core, bad shell** |
| 8 | Routing / pills / ports / ghost (`editor_ports/`, `editor_ghost.rs`) | 16 | 5,503 | 57 | 7 | 30 | 50 | **Good** |
| 9 | History / undo / host automation | 2 | 993 | 0 | 0 | 7 | 2 | **Good — already shared** |
| 10 | Theme (`editor_theme*`) | 2 | 834 | 0 | 15 | — | 0 | **Good — already shared** |
| 11 | Performance (`editor_performance/`) | 4 | 1,172 | 44 | 0 | 70 | 0 | **Bad** |
| 12 | Presets (`editor_presets/`) | 8 | 2,291 | 0 | 0 | 0 | 14 | **Good — port as-is** |
| 13 | Manual / F1 (`editor_manual/`) | 5 | 2,076 | 14 | 0 | 65 | 7 | **Missing in B** |
| 14 | Shared controls/widgets (`editor_controls*`, `editor_widgets.rs`) | 4 | 1,267 | 11 | 2 | 21 | 1 | **Redundant** |

Totals reconcile exactly to 50,898. `abs` per-surface sums to 714; the extra
5 are `Rect::from_x_y_ranges`, not counted per-directory.

---

## 1. Shell, header, browser, settings — 5,357 lines — **Bad**

- `editor_shell.rs` (676) is frame orchestration: `workspace_split` at
  `editor_shell.rs:39-50` is a hand-rolled two-pane splitter with its own
  pointer/keyboard/reset behaviour, keyed through `egui::Id::new(name)`.
  `browser_is_open` (`:17-20`), `register_parameter_hover` (`:22-27`) and
  `request_structural_commit` (`:29-37`) all smuggle frame state through
  `ui.data_mut` temp storage — a global side channel, not a tree.
- `editor_shell/browser.rs` (1,804) is the single worst file in the codebase for
  absolute placement: **56 hand-placed rects/points**, the densest in generation A.
  A rail of shelves/types/creators/tags, a filtered list and a detail pane with a
  live scope — all positioned by arithmetic.
- `editor_shell/header.rs` (653, 35 abs) hand-builds the transport/title bar and
  ends with a literal gradient (`header.rs:637-638`).
- `editor_shell/settings.rs` (980) plus `settings/theme_state.rs` (109, egui-free)
  and `settings/manual.rs` (135, egui-free).
- `editors/egui.rs` (492) + `editors/egui/factory_reset.rs` (351) +
  `detached_import.rs` (157) are the entry point and are **already egui-free by
  type usage** — they are lifecycle, not drawing.

Non-UI logic to keep: `settings/theme_state.rs` (persisted theme translation),
`factory_reset.rs`, `detached_import.rs`. That is 617 of 5,357 lines.
Everything else is 4,700 lines of layout arithmetic that MUI's
`row!`/`col!`/`Flow` + `pin` delete outright.

Shared with B? No. B's shell is `editors/mui/shell.rs` (378) +
`structure.rs` (531). B did not port the browser; `MUI-COMPONENT-MIGRATION.md:35-38`
admits "Full preset browser metadata workflows/import-export … still require
migration". So the browser exists **once**, in dead-end egui code.

## 2. Card kit (`editor_card/`) — 6,580 lines — **Redundant**

This is a private layout engine. `editor_card/layout.rs:50-120` defines `Lane`
with `fixed`/`share`/`weight`/`at_least`/`at_most`/`clamped` and `FILL`, then
`split::<N>` (`:143`) and `columns_dyn` (`:158`) arrange them. That is flexbox,
reimplemented in 593 lines of `egui::Rect` algebra.
`crates/mui-layout` (2,945 lines, `arrange.rs`/`measure.rs`/`len.rs`/`pin.rs`)
already does it, intrinsically, with `Size::Fill`/`Hug`.

`editor_card/metrics.rs` is the honest part: `metric_row` (`:11-16`) and
`collapsed_row` (`:19-23`) derive every card height from theme tokens, and
`CardMetrics::from_rack` (`:54-60`) clamps a density preference to `0.65..=1.35`.
The one smell is the bare `4.60` multiplier at `metrics.rs:60`.

The rest: `binding.rs` (1,389 — what a readout cell reads and writes),
`header.rs` (765 + `group_tab.rs` 289), `readout.rs` (717), `chrome.rs` (469),
`paint.rs` (455), `session.rs` (410 — the lifecycle of one card's config edit),
`frame.rs` (343), `kind.rs` (276), `port.rs` (259), `help.rs` (187),
`metrics.rs` (140), `panel.rs` (128), `graph.rs` (72), `mod.rs` (88).

Keep: `session.rs`'s edit lifecycle and `binding.rs`'s read/write contract, which
are about parameters, not pixels. B already rebuilt both as
`editor_mui/binding.rs` (196) and `editors/mui/synth/binding.rs` (510) — **that is
a duplicate, not a share**. 1,389 lines became 706 in B; expect the same again.

Verdict: **Redundant**. 157 absolute placements and a bespoke flex engine to
express "a card is a row of columns with a graph on top".

## 3. Generator rack (`editor_generator/`) — 7,607 lines — **Mixed**

- `group_output.rs` (1,463, 24 abs) — group identity header, envelope overview
  (`:198 paint_envelope_overview`, `:352 envelope_line_points`,
  `:403 nearest_envelope_line`, `:436 edit_envelope_graph`), output deck
  (`:734`), routing popup (`:892-1150`), and host-automation restore
  (`:1183 apply_host_automation_to_group`, `:1198 restore_host_automated_group_controls`).
  The last two are pure state admission and must survive.
- `oscillator_card.rs` (1,202) + `readouts.rs` (718) + `readouts/unison.rs` (365).
  `AGENTS.md:23-27` names `oscillator_card.rs` as the canonical layout reference —
  the rewrite's visual spec lives here even though the code does not.
- `insertion/` (2,760 total): `actions.rs` (679) is **egui-free** — add/remove/
  reorder/reset admission against the real generator state. `drag_reorder.rs`
  (765, 20 abs) is the drag ghost. `add_menu.rs` (437), `group_card.rs` (487),
  `layout.rs` (167).
- `native_processors.rs` (498) — warp routing cables between pills.
- `mod.rs` (292) — `remove_generator_group`, `remove_module`.

Shared with B: B imports exactly `crate::editor_generator::remove_generator_group`
and `::remove_module`. It did **not** import `insertion/actions.rs` (679 lines of
tested, egui-free admission) — B rebuilt equivalent logic in
`editors/mui/structure.rs` (531) and `groups.rs` (513). Duplication.

Verdict: **Mixed**. Keep `insertion/actions.rs`, the host-automation restore
pair, and the *arrangement* described by `oscillator_card.rs`. Drop the 101
absolute placements.

## 4. Oscillator VA table + unison — 5,536 lines — **Mixed**

- `editor_oscillator/va_table.rs` (1,807, 29 abs) — table selection, frame
  actions, `pencil_toggle_rect` (`:998`) hand-placed.
- `va_table/wavetable_import.rs` (767) — teardown-safe drag/drop wavetable load.
  Real IO + a round-trip test (`:642`). Keep whole.
- `va_table/curve_editor.rs` (222) — `snap_curve_point` (`:68`), `knot_pos`
  (`:161`), `value_pos` (`:166`). The math is right; the types are `egui::Pos2`.
- `preview.rs` (598) — samples the **real** DSP evaluator:
  `use crate::wave_curve::WaveCurveRt` (`preview.rs:9`), then `cycle_plot`
  (`:107`), `cycle_points` (`:237`), `sample` (`:316`),
  `build_audio_rate_cycle_points` (`:444`), `build_cycle_points` (`:532`),
  `reduced_cycle_points` (`:589`). This honours `AGENTS.md:32-33` ("sample the
  same evaluator used by playback"). The evaluator itself is `src/wave_curve/`
  (9,530 lines) and is **not editor code** — the rewrite inherits it free.
- `editor_unison/` (2,001, 11 files): `distribution*` (693), `pan_panel*` (514),
  `pan_shape*` (644), `manual.rs` (101). `normalized_unison_rate` is the only
  symbol B imports.

Shared with B: B imports `editor_oscillator::processor_preview`,
`WarpPreview::new`, `wavetable_import as files`, and
`editor_unison::normalized_unison_rate`. Genuine sharing, and the right four
symbols. The 5,000 lines of drawing around them are not shared.

Verdict: **Mixed**. Preview sampling and wavetable IO are the assets; the table
UI is 1,807 lines of rect arithmetic that a MUI grid replaces.

## 5. Resynth / Sample / Grain — 2,562 lines — **Mixed**

`editor_resynth.rs` (1,030, 34 abs) — `draw_resynth_body` (`:49`),
`source_interaction` (`:197`), `sync_build_status` (`:223`),
`track_sounding_revision` (`:267`), `paint_source_timeline` (`:456`),
`update_source_timeline` (`:501`), `paint_sample_loop_region` (`:569`),
`paint_live_grains` (`:603`). `readouts.rs` (1,144) is readout rows.
`source_io.rs` (388) is file loading.

`sync_build_status` and `track_sounding_revision` are the interesting ones:
they reconcile editor view state with an asynchronously built analysis. That is
not UI. `hz_to_midi_note`/`midi_note_label` (`:547,:557`) are formatting that
should live next to the parameter, per `MUI-COMPONENT-MIGRATION.md:44-46`
("do not add local formatting tables to every view").

B has `editors/mui/samples.rs` (783) covering the same product surface at 31%
of the size — no shared code. Duplication.

Verdict: **Mixed**. Keep `source_io.rs` and the build/revision reconciliation;
rebuild the timeline.

## 6. Filters, distortion, warps — 3,126 lines — **Mixed**

`editor_filter.rs` (1,667) + `painting.rs` (684, 20 abs, `ratio_points` `:289`,
`response_points` `:463`) + `spectral.rs` (180). `editor_distortion.rs` (403,
**16 abs in 403 lines** — the worst absolute-placement density in generation A).
`editor_native.rs` (192) is the independent warp rack.

`painting.rs` has `MAX_RESPONSE_DB = 18.0` and `RESPONSE_OVERFLOW = 10.0` —
domain constants, correctly not theme tokens. The filter response curve is
computed from `FilterConfig`, i.e. the real coefficients.

Shared with B: only `format_morph_rotation` and `processor_label`. B's
`editors/mui/warps/mod.rs` is 1,153 lines against A's 3,126 for the same surface.

Verdict: **Mixed**, leaning Bad. Keep the response-curve sampling (it reads real
config); `editor_distortion.rs` is a rewrite candidate on density alone.

## 7. LFO / envelope / spline — 5,994 lines — **Good core, Bad shell**

- `controls.rs` (1,475, 14 abs) — the control column. Largest file here, mostly chrome.
- `spline_editor.rs` (911) + `painting.rs` (335) + `interaction.rs` (330).
  `spline_editor.rs:65-67` reaches `crate::wave_curve::default_keytrack_curve()`
  and `default_lfo_curve()`; `:323 try_curve_rt` builds a real `WaveCurveRt`.
  `interaction.rs:254 segment_curve_for_value` is pure curve math —
  **and it is the one curve function B imports** (`crate::editor_lfo::segment_curve_for_value`).
- `envelope_editor.rs` (273) + `interaction.rs` (252, `envelope_points` `:6`,
  `envelope_path` `:186`) + `painting.rs` (218) + `parameter_edit.rs` (263,
  **egui-free**).
- `source.rs` (352, **egui-free**) — `active_source_mask` (`:189`),
  `source_config` (`:204`), `lfo_curve` (`:332`). All four symbols imported by B,
  plus `set_source_active`/`set_source_enabled`. This is the cleanest
  share boundary in the whole editor.
- `source_card.rs` (443), `add_menu.rs` (333), `rack_reorder.rs` (210),
  `gate_editor.rs` (144, 9 abs), `mod.rs` (386).

Verdict: **Good core** — `source.rs` + `parameter_edit.rs` + `segment_curve_for_value`
(867 lines, egui-free, already shared with B) is exactly the pattern the rewrite
wants. **Bad shell** — 5,100 lines of drawing around it.

## 8. Routing, pills, ports, ghost — 5,503 lines — **Good**

`editor_ports/` is the best-factored module in generation A. Its `mod.rs:1-2`
states the contract: "geometry and paint, no state". Files:
`param_tests.rs` (1,372), `pill_tests.rs` (752), `cable_tests.rs` (192) — **2,316
lines, 44% of the module, are tests** driving the real editor frame by frame.
Production: `param.rs` (561 — "Every parameter is a modulation destination"),
`geometry.rs` (448), `pie.rs` (397 — the gesture every route pie answers to),
`paint.rs` (312), `pill.rs` (258), `mode.rs` (242), `registry.rs` (171 — where
every port was drawn this frame), `slot.rs` (124), `cable.rs` (79),
`popup.rs` (54), `head.rs` (212).

`editor_ghost.rs` (286, **0 absolute placements**) — `LiveModulation` (`:19-31`),
`modular` (`:64`), `filter_delta` (`:90`), `ghost_filter_config` (`:122`),
`quantized` (`:141`), `oscillator_ghost` (`:147`), `warp_connection` (`:183`).
Pure "what is modulation doing right now" evaluation.

Note the theme-reference count: 30 across 5,217 lines, the lowest ratio of any
drawing surface. Ports carry their colour from the owning source accent rather
than looking up roles, which is correct.

Shared with B: **nothing**. B has `editors/mui/routing.rs` (672) +
`modulators/surface.rs` (769) + `editor_mui/routes.rs` (252), and 0 references
to `editor_ghost`. `MUI-COMPONENT-MIGRATION.md:60-62` names this explicitly:
"Native routes and mock routes are separate implementations." So the best-tested
module in the editor is duplicated by an untested one, and the ghost layer is
simply absent from B.

Verdict: **Good**, and the largest single loss if it is not carried forward.

## 9. History, undo, host automation — 993 lines — **Good, already shared**

`editor_history.rs` (821, 3 incidental egui mentions, 0 absolute placements):
`EditorSnapshot::capture` (`:55`), `matches_live` (`:164`), `apply` (`:229`),
`set_param_bits` (`:321`); `EditorHistory` (`:345`) with `capture_initial`
(`:356`), `commit` (`:372`), `flush_deferred` (`:413`), `undo`/`redo`
(`:429,:443`), `parameter_undo`/`parameter_redo` (`:457,:461`),
`handle_shortcuts` (`:470`), `trim` (`:524`), `retained_bytes` (`:539`),
`resynth_retained_bytes` (`:552`). Bounded whole-plugin **and** per-parameter
history, memory-accounted.

B imports `crate::editor_history::EditorHistory` directly. This is the one
surface that is genuinely shared and not duplicated — though B added its own
`history_shortcut` at `editors/mui/control.rs:149`, with a test named
`history_shortcuts_match_legacy_parameter_and_global_fallbacks` (`:178`) that
exists precisely because the shortcut table was re-typed.

`editor_host_automation.rs` (172) — host-visible assignment and frame-local
lookup. B references host automation in 30 places.

Verdict: **Good.** Port verbatim. Do not re-implement the shortcut table a third time.

## 10. Theme — 834 lines — **Good, already shared**

`editor_theme.rs` (621) + `library.rs` (213, egui-free). Tokens:
accents `:16-18`, `color::{DANGER,LIVE}` `:37-43`, `space::{XXS..LG,ITEM_GAP}`
`:202-214`, `font::{CAPTION,LABEL,TITLE,VALUE}_SIZE` and constructors `:217-237`,
`dim::{READOUT,RAIL,GRAPH}` `:248-254`, `shape::{CONTROL_RADIUS,STROKE,
FOCUS_STROKE,GROUP_STROKE}` `:257-261`, `control_visuals` `:300`,
`metrics`/`title_height`/`graph_inset`/`compact_gap` `:331-346`,
`semantic_palette` `:348`, `semantic()` `:440`, `on_accent` `:444`,
`group_accents` `:480`, `palette*`/`theme_for`/`apply_with` `:529-541`.

**The bypass count is the surprise, and it is good news.** 28 `Color32::` sites
outside `editor_theme` across 50,898 lines, and 19 of them are
`from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha)` — alpha
variations of a token that was already resolved (`editor_generator/mod.rs:28`,
`editor_card/chrome.rs:23`, `editor_widgets.rs:338`,
`editor_oscillator/preview.rs:555-556`, `editor_shell/header.rs:637-638`,
`editor_lfo/rack_reorder.rs:54`, `editor_lfo/add_menu.rs:181,190`,
`editor_generator/oscillator_card.rs:850,856`). The genuine literals are six
`Color32::WHITE` used only as a layout-measurement placeholder
(`editor_card/readout.rs:575,579`, `editor_controls.rs:25`,
`editor_oscillator/va_table.rs:1250`, `editor_lfo/spline_editor.rs:838`), one
`Color32::BLACK` ring at `editor_ports/head.rs:68`, two inside
`editor_ports/paint.rs:262,292`, and three in `editor_ghost.rs:274-276` tests.

Same for sizes: of 55 `const NAME: f32` declarations, 15 are the theme's own and
most of the rest are **ratios**, not pixels — `READOUT_SHARE 0.22`,
`KNOB_RADIUS_RATIO 0.24`, `IDENTITY_SHARE 0.055`, `INLINE_SHARE 0.70`,
`SIZE 0.42`, `FIT_WIDTH 0.86`, `FIT_HEIGHT 0.62`, `REST 0.22/0.42`. True fixed
pixels are rare: `TRAVEL 120.0` (drag-reorder), `VEIL_ALPHA 150.0` (manual
backdrop). Domain constants (`MIN_CUTOFF_HZ`, `PREVIEW_NOTE_HZ 110.0`,
`HOST_PREVIEW_SAMPLE_RATE 48_000.0`) are correctly not tokens.

B imports 13 distinct theme symbols (41 reference sites) — the most-shared
module in the codebase.

Verdict: **Good.** The theme layer was enforced. The narrative that generation A
is full of hardcoded colours and pixel sizes is false; what it is full of is
**hand-computed rects**.

## 11. Performance — 1,172 lines — **Bad**

`editor_performance.rs` (636, 19 abs), `wheels.rs` (268, 13 abs),
`xy_source.rs` (191, 12 abs), `manual.rs` (77, egui-free). **44 absolute
placements in 1,095 lines of drawing** — the highest density of any surface.
`performance_field_grid` (`:129`), `octave_semitone_field` (`:200`),
`pitch_range_field` (`:274`), `global_tuning_entry` (`:350`),
`drag_symmetric_pitch_range` (`:415`), `voice_mode_selector` (`:538`),
`voice_mode_text` (`:595`). `FIELD_COLUMNS: f32 = 4.0` is a literal grid.

`drag_symmetric_pitch_range` is the one piece of real gesture semantics here
(symmetric two-ended drag). `voice_mode_text` is another local formatting table.

B has `editors/mui/performance.rs` (246). No sharing.

Verdict: **Bad.** A field grid built by arithmetic when it is a form. Rebuild.

## 12. Presets — 2,291 lines — **Good, port as-is**

Eight files, **zero absolute placements, zero Color32, zero theme references**:
`editor_presets.rs` (434), `thumbs.rs` (397), `storage.rs` (345),
`browse.rs` (318 — "kept free of egui so the browser's behaviour has a unit
test", `browse.rs:1-2`), `context.rs` (265 — truce state envelope capture/apply),
`factory.rs` (181), `settings.rs` (181), `manual.rs` (170). 14 tests.

`thumbs.rs` renders one oscillator cycle on a background thread and caches by
uuid — that is a renderer-agnostic job that only needs a different output target.

B imports `atomic_write`, `context::plugin_id_hash`, `EXTENSION`,
`PresetStore`, `SaveRequest`. Partial share; B's `editors/mui/presets.rs` (564)
re-does the browsing UI but reuses the store.

Verdict: **Good.** 2,291 lines that move to the new editor unchanged.

## 13. Manual / F1 help — 2,076 lines — **Missing in B**

`mod.rs` (1,365, 14 abs) — F1 pops the component under the pointer out of the
workspace, enlarged over a dimmed backdrop (`mod.rs:1-3`).
`describe.rs` (441, egui-free) — one-line descriptions matched by parameter-name
tail so "LFO 2 Rate" and "LFO 5 Rate" share text. `library.rs` (139, egui-free),
`book.rs` (77, egui-free), `overview.rs` (54, egui-free). Plus per-module manual
pages scattered into their surfaces: `editor_presets/manual.rs` (170),
`editor_shell/settings/manual.rs` (135), `editor_unison/manual.rs` (101),
`editor_performance/manual.rs` (77), `editor_card/help.rs` (187).

**Generation B has zero references to the manual.** grep for `editor_manual` in
`src/editors/mui` and `src/editor_mui.rs` returns nothing. A whole product
feature exists only in the editor that is being retired.

Verdict: **Missing.** 811 lines of content (describe/library/book/overview/
per-module pages) are egui-free and portable today; the 1,365-line pop-out
presenter needs MUI's float/overlay.

## 14. Shared controls and widgets — 1,267 lines — **Redundant**

`editor_controls/parameter_gesture.rs` (727): `KnobDrag` (`:9`),
`from_label` (`:49`), `CustomDrag` (`:90`), `pointer_gesture_aborted` (`:95`),
`update_parameter_drag` (`:99`), keyboard stepping (`:196-220`),
`accumulate_drag` (`:332`), `update_custom_value_drag` (`:336`),
`update_custom_value_drag_axis` (`:370`), `update_custom_pitch_ratio_drag` (`:434`),
`semantic_snap` (`:484`), `magnetic_shape_snap` (`:596`),
`CONFIG_DRAG_SPEED = 1.0/150.0`, `FINE_DRAG_SCALE = 0.18`.
`editor_controls.rs` (201) — `fit_font_to_width` and bound controls.
`editor_widgets.rs` (339) — `stroke_mesh` (`:28`), `cached_stroke_mesh` (`:73`),
`cached_gradient_stroke_mesh` (`:89`) with a `MeshCache` (`:42`),
`with_dragged_layer` (`:114`), `drag_edge_scroll` (`:255`).

This is the **clearest duplication in the repo**. B does not import
`editor_controls` at all. Instead:
`editors/mui/control.rs:38 parameter_gestures` and
`editors/mui/editing.rs:73 magnetic_snap` re-implement
`parameter_gesture.rs:99 update_parameter_drag` and `:596 magnetic_shape_snap`;
`editors/mui/editing.rs:139 authored_curve_plot_samples` and `:81 curve_hit`
re-implement the plot sampling and hit-testing that
`editor_oscillator/preview.rs:532,589` and
`editor_oscillator/va_table/curve_editor.rs:68` already do.
`MUI-COMPONENT-MIGRATION.md:29-30` flagged this a year of work ago: "The mock
oscillator implements another gesture path."

The mesh caching in `editor_widgets.rs` is obsolete under MUI: `crates/mui-vello`
and `crates/mui-scene` own the paint list, and `AGENTS.md:37-38`'s caching rule
("cache static meshes by every input that changes geometry") becomes MUI's job.

Verdict: **Redundant.** Snap/step/accumulate semantics are worth ~150 lines of
pure math; the other 1,100 are an egui adapter and a mesh cache with no future.

---

## Cross-cutting facts

**What B actually imports from A** (43 distinct paths, `grep -rhoE 'crate::editor_[a-z_]+'`):
`editor_theme` (41 sites), `editor_lfo` (21), `editor_oscillator` (7),
`editor_generator` (6), `editor_presets` (4), `editor_filter` (4),
`editor_history` (3), `editor_unison` (2), `editor_resynth` (1).
Zero from `editor_controls`, `editor_ports`, `editor_ghost`, `editor_card`,
`editor_manual`, `editor_performance`, `editor_widgets`, `editor_shell`.

So the share boundary is: **tokens, curve evaluation, structural mutations,
preset storage, history.** Everything about *interaction* — gestures, snapping,
routing, ghosts, hit-testing — is duplicated. That is backwards: the tokens are
cheap to re-declare and the gesture semantics are the expensive, tuned part.

**The portable core.** 22 files with no egui type usage, 4,964 lines. The largest: `editor_generator/insertion/actions.rs` (679), `editor_manual/describe.rs` (441), `editor_presets.rs` (434), `editor_presets/thumbs.rs` (397), `editor_lfo/source.rs` (352), `editors/egui/factory_reset.rs` (351), `editor_presets/storage.rs` (345), `editor_presets/browse.rs` (318), `editor_presets/context.rs` (265), `editor_lfo/envelope_editor/parameter_edit.rs` (263), `editor_theme/library.rs` (213); the remaining eleven are the per-surface `manual.rs` pages, `editor_presets/{settings,factory,manual}.rs` and `editors/egui/detached_import.rs` (157). Add `editor_history.rs` (821, 3 incidental mentions) and `editor_ghost.rs` (286, 0 absolute placements) and the portable core is **~6,070 lines, 12% of generation A**.

**The real DSP evaluators are not in the editor.** `src/wave_curve/` is 9,530
lines and previews already sample it (`editor_oscillator/preview.rs:9`,
`editor_lfo/spline_editor.rs:5,65-67,323`). The rewrite inherits correct curves
for free; only the *screen-space* wrappers (`cycle_plot`, `knot_pos`,
`value_pos`, `envelope_points`) need retyping into `mui-geometry`.

**Hand-rolled layout math**, by `.shrink/.expand/.with_max_x/.translate/from_x_y_ranges` count: `editor_card` 13, `editor_ports` 13, `editor_shell` 12, `editor_lfo` 10, `editor_manual` 9, `editor_generator` 8, then 4 or fewer everywhere else and 0 in `editor_presets`. Low, because the real arithmetic is the 719 `pos2`/`Rect::from_*` sites, not rect adjustments.

**The 13% test tax.** 6,684 lines in `#[cfg(test)]`, 154 tests. Of those,
`editor_ports`' 2,316 lines drive the real egui editor frame by frame
(`param_tests.rs:1-3`, `pill_tests.rs:1-3`, `cable_tests.rs:1-3`). Those tests
die with egui. B has 59 tests across 17,634 lines — a quarter of the coverage.

---

## What the rewrite should do about it

1. **Port `editor_history.rs` (821) verbatim, and delete the second shortcut
   table.** `editors/mui/control.rs:149 history_shortcut` exists only because
   nobody moved `editor_history.rs:470 handle_shortcuts` behind a renderer-neutral
   input type. One `handle_shortcuts(&Input) -> Option<Action>` over `mui-input`
   serves both.

2. **Port `editor_presets/` (2,291) unchanged.** Zero absolute placements, zero
   colours, 14 tests, `browse.rs` deliberately egui-free. Only `thumbs.rs` needs
   its render target swapped to a `mui-scene` snapshot.

3. **Delete `editor_card/layout.rs` (593) and do not port `Lane`.**
   `crates/mui-layout` (2,945) with `Size::Fill`/`Hug`, `gap`, `pad` is the
   replacement. Keep `editor_card/metrics.rs` (140) as the height *policy*
   (it derives from tokens already) and drop the `4.60` at `metrics.rs:60` into
   a named token.

4. **Extract ~150 lines of gesture semantics from
   `editor_controls/parameter_gesture.rs` and throw away the other 577.** Keep
   `accumulate_drag` (`:332`), `semantic_snap` (`:484`), `magnetic_shape_snap`
   (`:596`), `CONFIG_DRAG_SPEED`, `FINE_DRAG_SCALE`, the keyboard step table
   (`:196-220`) as pure `f32 -> f32` functions over `mui-input`. Then delete
   `editors/mui/editing.rs:73 magnetic_snap` — the third copy.

5. **Carry `editor_ports/` forward as the routing model, tests and all.** It is
   the only module that already separates geometry from state
   (`editor_ports/mod.rs:1-2`) and the only one with 44 tests. Retype
   `geometry.rs` (448), `pie.rs` (397) and `registry.rs` (171) onto
   `mui-geometry`; let MUI's hit regions replace `registry.rs` if they can.
   Do **not** keep `editors/mui/routing.rs` (672) — it is the untested copy.

6. **Rescue `editor_ghost.rs` (286).** Zero absolute placements, pure live-
   modulation evaluation, and generation B has no equivalent. It is the cheapest
   feature-parity win available.

7. **Port `editor_generator/insertion/actions.rs` (679) and
   `group_output.rs:1183-1198` (host-automation restore) as non-UI modules**, and
   reconcile them with `editors/mui/structure.rs` (531) + `groups.rs` (513),
   which duplicate the admission rules without the tests.

8. **Keep sampling the real evaluator.** `editor_oscillator/preview.rs` and
   `editor_lfo/spline_editor.rs` already call `crate::wave_curve::*`. Preserve
   that contract (`AGENTS.md:32-33`); only the `Pos2`/`Rect` wrappers change.
   `editor_lfo/source.rs` (352) and
   `envelope_editor/parameter_edit.rs` (263) move as-is.

9. **Rebuild, do not port: shell/browser (5,357), performance (1,172),
   distortion (403), the VA table chrome (1,807).** Together 8,739 lines holding
   231 of the 719 absolute placements. Every one of them is a form, a list or a
   grid — `row!`/`col!` cases.

10. **Do not re-derive the theme; re-declare it.** `editor_theme.rs`'s token set
    is sound and was actually obeyed (9 real bypasses in 50,898 lines). Map
    `space/font/dim/shape/semantic/group_accents` onto `mui-style` roles and move
    on. This is a half-day, not a project.

11. **Budget the manual honestly.** 811 lines of `editor_manual` content plus
    five per-surface `manual.rs` files are portable now; the 1,365-line pop-out
    presenter is new work on MUI's float layer. If it is out of scope, say so —
    silently dropping F1 is how generation B lost it.

12. **The size target is credible.** Generation A is 50,898 lines; 6,070 are
    portable, 6,684 are tests, and 719 absolute placements plus a 593-line flex
    engine are pure layout tax. MUI issue #8's "a third of the size" against B's
    17,634 means ~6k — which is roughly the portable core plus a DSL tree. The
    risk is not layout; it is the 2,316 lines of `editor_ports` tests and the
    interaction tuning in `parameter_gesture.rs` being re-typed a third time
    instead of moved once.
