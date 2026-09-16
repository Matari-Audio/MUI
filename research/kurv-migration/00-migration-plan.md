# 00 — KURV on MUI: the migration plan

Written 2026-09-16 against the five audits in this folder (`01`–`06`), MUI
`ROADMAP.md`, and MUI issue #8: *"Kurv rewritten on MUI: the ~13k-line GPUI
editor at a third of the size, from a fresh worktree off main; the item that
can still change the DSL."*

Every number here comes from `01`–`06`, which ran `wc`/`grep`/`git diff` against
the trees. Where the audits disagree, this document picks one denominator and
says which.

## 0. The accounting, once

| Tree | Path | Lines | Note |
|---|---|---:|---|
| A — egui reference editor | `KURV/src/editor_*.rs`, `src/editor_*/`, `src/editors/egui*` | **50,898** | 118 files; 6,684 in `#[cfg(test)]`; 719 absolute placements (`01`) |
| A″ — vendored plugcat | `KURV/vendor/plugcat` | 5,932 | 4,572 never called; 4 symbols used, all in `editor_theme.rs:6-8` (`04`) |
| B1 — first GPUI editor | `KURV/src/editor_mui.rs` + `src/editor_mui/` | 4,514 | only caller `examples/mui_editor.rs:65` (`02`, `04`) |
| B2 — live Linux editor | `KURV/src/editors/mui/` | **13,141** | 11,086 code / 2,055 test (`02`) |
| B′ — pinned MUI probe | `.worktrees/kurv-mui-runtime-0.20.37/experiments/gpui-plugin` | 8,863 | 3,661 reachable from KURV; 3,312 + ~1,500 dead (`02`, `04`) |
| C — MUI main | `MUI-migration/crates/*` | 21,391 | 15 crates, 263 tests; `mui-widgets` 879, of which 632 is five widgets (`03`, `06`) |

**Generation B is 13,141 + 4,514 + 3,661 = 21,316 lines across two repos**, not
13k (`03` §5). Issue #8's denominator is wrong in the issue. The honest targets:

- **View code on MUI: ~4,600 lines**, budgeted per surface in §3.
- **Domain code moved, not rewritten: ~6,500 lines** (history, presets, ports
  geometry, source config, admission rules). It was never view code and it does
  not count against the target.
- **Deletions: ~66,700 lines** (`04` §7.12), plus KURV's path dependency on an
  `experiments/` crate.

A third of 13k is 4.4k and a third of 21.3k is 7.1k. The plan targets 4,600
view lines because the domain split (§2, K0) moves the difference out of the
view rather than deleting it. Say "4.6k of view code over 6.5k of moved domain
code" in the PR, not "a third".

---

## 1. Executive verdict

### Generation A — the egui reference editor (50,898 lines)

| Verdict | What | Evidence |
|---|---|---|
| **Good** | `editor_theme.rs` (834) — 22-role palette called 198×, 5-step spacing + 4 font roles used 669×, one radius (`:258`). The "hardcoded colours" story is false: 28 `Color32` sites outside the theme, 19 of them alpha blends, ~9 real bypasses in 50,898 lines (`01` §10, `05` §6) | `editor_theme.rs:175-199,202-261,300` |
| **Good** | `editor_ports/` + `editor_ghost.rs` (5,503) — the only module that separates geometry from state, 44 tests / 2,316 test lines, and B duplicates it untested with zero ghost equivalent (`01` §8) | `editor_ports/mod.rs:1-2`, `editor_ghost.rs:19-183` |
| **Good** | `editor_history.rs` (821) + `editor_presets/` (2,291) — egui-free, tested, already shared with B (`01` §9, §12) | `editor_history.rs:345-552`, `editor_presets/browse.rs:1-2` |
| **Bad** | 719 absolute placements and `ui.data_mut` frame-state side channels in shell/browser/performance (5,357 + 1,172 lines, 156 of the placements) (`01` §1, §11) | `editor_shell/browser.rs` (56 sites), `editor_performance.rs` (44/1,095) |
| **Bad** | Not feature-gated. `src/lib.rs:19-38` declares all 20 `editor_*` modules unconditionally; `legacy-egui = []` only flips which `create` runs (`04` §0) | `Cargo.toml:114`, `src/editors/mod.rs:9-23` |
| **Missing** | Nothing — A is the feature-complete generation. It is the *only* home of the preset browser, the settings pages and the F1 manual (`01` §13) | `editor_manual/mod.rs:1-3` |
| **Redundant** | `editor_card/layout.rs` (593) is a private flexbox (`Lane`/`share`/`split`/`columns_dyn`); `editor_controls/parameter_gesture.rs` (727) is triplicated; plugcat's 4,572 uncalled widget lines (`01` §2, §14, `04` §4) | `editor_card/layout.rs:50-158`, `parameter_gesture.rs:99,596` |

### Generation B — the GPUI/pinned-MUI editor (21,316 lines, two repos)

| Verdict | What | Evidence |
|---|---|---|
| **Good** | `mui-truce` is byte-identical between the snapshot and main. `Parameter` + `Automation` port with zero work, and so do `editor_mui/binding.rs` (196) and `synth/binding.rs`'s host-bank resolution (`02` §2, `03` §7) | `mui-truce/src/parameter.rs:9-181`, `editor_mui/binding.rs:5-190` |
| **Good** | `shell.rs:4-62` — the whole top-level composition in 58 declarative lines, and `components.rs:5-21` wraps `Item::row`/`column` once (raw use: 1 each in 13k lines) (`02` §3) | `shell.rs:4-62`, `components.rs:5-21` |
| **Good** | One gesture adapter (`control.rs:36-105`) and route legality kept in KURV (`routing.rs:43-510`) — the right split, even where the implementation is not (`02` §3, §6) | `control.rs:36-105`, `routing.rs:43-510` |
| **Bad** | `view.rs::render` is 1,135 lines over a 49-field `Shell` with 19 `Option` drag states, a 14-branch `finish()`, and an undo guard that is a hand-maintained 13-term boolean (`02` §3.6) | `view.rs:33-83,144-183,534-552,793-1927` |
| **Bad** | Layout declared then patched: 374 `format!` slot ids, 71 `scene.frame` read-backs, a manual cache keyed on `width^height.rotate_left(17)`, three conflicting minimum-width policies (`shell.rs` `min()`, `rack_min=[220,260]`, a magic `-560.`) (`02` §3) | `view.rs:887-1027`, `shell.rs:19-54`, `view.rs:188-197` |
| **Bad** | Theme thrown away: `semantic()` called **once** in 13,120 lines against A's 198; `SURFACE/WELL/RAISED` as `Rgb` literals; 118 `px(<literal>)` to 19 `px(token)`; 5 radii, 9 text sizes; disabled derived by `.grayscale()` (`05` §1, §3, §6) | `components.rs:2-4`, `view.rs:1328,1725,1752`, `editing.rs:1412` |
| **Bad** | Hit shapes drift from drawn shapes: `curve_hit` grabs at squared distance 144 (12 px) against a 12×12 drawn handle (6 px), in unscaled layout pixels, while the stroke beside it takes `scale_factor()` (`05` §4) | `editing.rs:81-101`, `modulators/surface.rs:416-417`, `groups.rs:195,266` |
| **Missing** | Interaction states: 3 `.hover()` sites, **zero** press styles, 1 `focus_visible` in 13,120 lines; the curve editor's whole keyboard+pointer contract is minified onto three lines of 1,173/1,058/769 characters (`05` §4) | `editing.rs:2280,2295,2301` |
| **Missing** | Evidence. `tools/check_mui_editor.py` is 69 lines, runs two ignored tests behind XWayland with `GPUI_X11_SCALE_FACTOR=1`, and asserts 8 `PASS:` substrings — real scenarios, but all at 1× and none of them a geometry assertion. 165 fps is unmeasured; the "4 ms pump" is `recv_timeout` running a full `synchronize` unconditionally (`02` §5, `05` §7) | `check_mui_editor.py:48,56-64`, `mui-gpui/src/lib.rs:282-308` |
| **Missing** | The F1 manual. `grep editor_manual src/editors/mui` returns nothing; a whole product feature exists only in the editor being retired (`01` §13) | — |
| **Redundant** | B1 (4,514) is a second non-shipping editor kept alive by a `pub use`, for four helper items; 57% of the pinned probe never renders; `pie_container`/`group_header`/`curve_editor`/`kurv.rs` are KURV product code filed in MUI's repo (`02` §1b, §1c, `03` §5) | `lib.rs:10-18`, `gpui-plugin/src/kurv.rs:1` |
| **Redundant** | The 56 `.slot(` sites. A slot exists only to reconcile two layout systems; one renderer means zero slots (`03` §5) | `gpui-plugin/src/dsl.rs:119` |

### Generation C — MUI main as a target (21,391 lines)

| Verdict | What | Evidence |
|---|---|---|
| **Good** | There is no fork to merge. The snapshot tip `34791fa` is an ancestor of main (merged as `863c644`); main is that plus ten commits, seventeen hours, net −33,151 lines, and it drops 1.65M lines of vendored GPUI (`03` §1) | `git merge-base` |
| **Good** | The DSL rename table *is* the port: `item()`→`row!/col!`, `.size(Fill,Hug)`→`.grow/.w/.h`, `.round((o,i))`→`.radius(Corner::…)`/`.shell()`, `.on_tap`→`Ui::get`. `El` says in 128 lines what `item.rs` said in 742 (`03` §3) | `mui-scene/src/element.rs`, `dsl.rs:43-185` |
| **Good** | `mui_motion::Curve` + `CurveHistory` is a cleaner cubic model than KURV's `WaveCurveData`/`WaveEdit` — but see §2: KURV's evaluator wins on the audio side (`06` #10, `04` §3) | `mui-motion/src/curve.rs:86-370` |
| **Bad** | `PointerInput { pos, primary_down }` has no modifiers and no secondary button; `Mods` exists only inside `KeyPress` and `Ui::keys(id)` is focus-gated. KURV reads modifiers at 43 sites and right-button at 4. This is not "awkward", it is *cannot* (`06` #6, #7) | `mui-input/src/lib.rs:105-112,130-137`, `mui/src/ui.rs:226-233` |
| **Bad** | `canvas` is write-only: its hit shape is its frame, not its drawn paths. 22 `paint_path` sites in KURV — pies, cables, knots, slice markers — have no targetable geometry, while `Hit::push` already takes an arbitrary `Path` (`06` #9, #3) | `mui-scene/src/element.rs:220`, `mui-input/src/lib.rs:56` |
| **Missing** | Nothing in MUI knows what a plugin editor window is. `mui-truce` is `Document` + `Parameter`; `mui-vello` has no window handle; the only host is the 238-line winit `mui-preview/src/host.rs`. No counterpart to `request_resize`/`set_size`, and it is not on `ROADMAP.md` (`06` §verdicts) | `editor_mui.rs:1257,1334` |
| **Missing** | Sticky headers in a scroll viewport; a min-extent query; reserve-text + `set_text` live readouts; `Palette::from_seed`; font weight; `State::Disabled`; a global key stream; stable node identity under reorder (`03` §8, `06` table, `ROADMAP.md:147-184`) | — |
| **Redundant** | Do not build: a splitter widget (`Ui::drag` + `Cursor::ColResize`, ~20 KURV lines), file dialogs, document undo, preset audition, a virtualised list until a second caller exists (`06` #12, #13, #16) | `shell.rs:65-72` |

---

## 2. The decision: keep, port, delete

### Kept as-is — moved, not rewritten (~6,500 lines)

These compile today, have no frontend import, and are the reason the rewrite can
be small. **K0 moves them into `kurv::editor_model` before any MUI code is
written.** Nothing in this list is re-typed.

| What | Lines | Why |
|---|---:|---|
| `editor_history.rs` | 821 | bounded whole-plugin *and* per-parameter undo, memory-accounted; B already calls it (`01` §9) |
| `editor_presets/` (8 files) | 2,291 | zero placements, zero colours, 14 tests, `browse.rs` deliberately egui-free; only `thumbs.rs` needs a new render target |
| `editor_ports/geometry.rs`, `pie.rs`, `registry.rs`, `param.rs` + their tests | ~1,500 of 5,503 | cable/pie math is pure; 2,316 test lines are the best coverage in the repo |
| `editor_ghost.rs` | 286 | live-modulation evaluation, 0 placements, absent from B — cheapest parity win available |
| `editor_lfo/source.rs`, `envelope_editor/parameter_edit.rs` | 615 | already the share boundary with B |
| `editor_generator/insertion/actions.rs`, `group_output.rs:1183-1198` | ~700 | add/remove/reorder admission + host-automation restore, egui-free and tested |
| `editor_mui/binding.rs` (`SourceField`, `Binding`), `synth/binding.rs:30-60` | ~400 | host-bank resolution and musical formatting, no GPUI |
| gesture math from `parameter_gesture.rs` (`accumulate_drag`, `semantic_snap`, `magnetic_shape_snap`, the keyboard step table, `CONFIG_DRAG_SPEED`, `FINE_DRAG_SCALE`) | ~150 of 727 | the tuned part; the other 577 are an egui adapter |
| `editor_manual/{describe,library,book,overview}.rs` + 5 per-surface `manual.rs` | 811 | content, egui-free, portable today |
| `src/wave_curve/` (the evaluator) | 9,530 | not editor code at all; previews sample it and must keep doing so (`AGENTS.md:32-33`) |
| `mui-truce` usage | — | `parameter.rs` byte-identical; `document.rs` needs `&'static str` → `document::Error` at call sites |

### Ported — logic rewritten against MUI (~4,600 view lines)

Everything in §3's budget. The rule: a surface is *ported* when its arrangement
is re-declared in `row!`/`col!`/`grid!` and its content comes from the domain
module. `shell.rs:4-62`, `control.rs:16-34`, `components.rs:5-21` and
`routing.rs:43-510` port nearly line for line. The 17 `canvas(` surfaces port as
`canvas(|size| Vec<Draw>)` closures over the real evaluator — once `canvas` has
hit paths (M2), or they port as decoration with separate hit frames and that is
a regression.

### Deleted, and when it is safe

| # | Delete | Lines | Safe when |
|---|---|---:|---|
| D1 | `src/editor_mui.rs` + `editor_mui/{panel,routes,workspace}.rs`, the `create_mui_editor` export | 4,514 | after K0 lifts `binding.rs`, `workspace::bindings`, `values`, `distribution_with_gain`. **Independent of the rewrite — do it first.** |
| D2 | `gpui-plugin`'s unreachable half (`main`, `kurv`, `panel`, `group_header`, `curve_editor`, `pie_container`, `control_panel`, `module_shell`, ~1,500 of `oscillator.rs`) | ~4,900 | immediately; 0 references from `KURV/src`. It lives in a worktree nobody ships. |
| D3 | `vendor/plugcat` | 5,932 | after K2 re-declares the ~60 token numbers from `editor_theme.rs:202-261` onto `mui_style::Theme`. Four symbols block it, all in one file. |
| D4 | `src/editors/mui/` (B2) + the `mui_shell` export | 13,141 | after K9: every §3 surface green in the rewritten checker at 1×/1.5×/2×, on Linux, for one release cycle |
| D5 | The pinned snapshot dependency (`mui-gpui-plugin-probe`, `mui-gpui`) and the worktree | 8,863 | same commit as D4; `Cargo.toml:142` is the last reference |
| D6 | Generation A (`src/editor_*`, `src/editors/egui*`) | 51,898 | after D4 **and** after a MUI host exists on Windows and macOS (§5, R1). Until then A is the fallback on every non-Linux target (`Cargo.toml:102,112-113`). It is a git tag, not a module, but the tag cannot be cut while it is the only Windows editor. |
| D7 | `mui-motion::CurveHistory` | 38 | whenever; a second undo stack in a motion crate with no caller |

D1+D2 are **~9,400 lines deletable this week**, before a line of the rewrite is
written, and they are the fastest way to make the remaining tree legible.

---

## 3. Target architecture

### Crate and module layout

```
KURV/
  src/
    editor_model/           # NEW. no frontend dependency, no `use mui`.
      history.rs            #   moved editor_history.rs (821)
      presets/              #   moved editor_presets/ (2,291)
      routing/              #   moved editor_ports geometry+pie+registry+tests
      ghost.rs              #   moved editor_ghost.rs (286)
      sources.rs            #   moved editor_lfo/source.rs + parameter_edit.rs
      admission.rs          #   moved insertion/actions.rs + host-automation restore
      binding.rs            #   moved editor_mui/binding.rs (SourceField, Binding)
      gesture.rs            #   ~150 lines of snap/step/accumulate math
      format.rs             #   ONE formatter keyed on ParamInfo/ValueSemantic
      manual.rs             #   moved editor_manual content (811)
      theme.rs              #   const KURV: Theme over mui_style::Theme + group_accents()
    editor/                 # NEW. the MUI view. `use mui::prelude::*` only.
      host.rs               #   truce::Editor impl, parented Vello surface, frame clock
      shell.rs  components.rs  synth/  warps/  modulators/  routing.rs
      groups.rs  curves.rs  samples.rs  wavetable.rs  presets.rs  settings.rs  manual.rs
    wave_curve/             # unchanged. the evaluator both editors sample.
    editor_*                # generation A, unchanged, until D6.
```

Three rules, enforced as review gates, not aspirations (B wrote the same
contract at `MUI-COMPONENT-MIGRATION.md:118-121` and broke it ~200 times):

1. `src/editor_model/` may not `use mui` or `use egui`. A `grep` in CI.
2. `src/editor/` may not contain a colour literal or a `px(<literal>)` outside a
   `canvas` closure's own coordinate transform. Target: **0 literals against B's
   118**, and `semantic()`-equivalent role lookups everywhere.
3. Slot ids are a typed enum whose `Display` produces the string. B's 374
   `format!("osc-{i}-wave-graph")` calls are a runtime-string API for a
   compile-time tree (`02` §7.5).

The truce binding sits in `editor/host.rs` and `editor_model/binding.rs` only.
`Frame::edits` → `mui_truce::Automation::dispatch`; `Ui::edit(id)` brackets every
capture. Rename `mui::Edit` → `mui::Gesture` (M1) so it stops colliding with
`mui_truce::Edit` — KURV imports both in one file today (`04` §5).

### The DSL changes MUI must make first

From `06`'s ranking and `03` §8, in dependency order. Sum **~1,240 lines**,
roughly doubling `mui-widgets`.

| Change | Lines | Blocks | Phase |
|---|---:|---|---|
| `PointerInput { …, secondary_down, mods }` threaded onto `Response`; axis-lock in `Interaction` | ~90 | 7 surfaces: spline, pan, brush, fine drag, wavetable, reorder, every context menu | M1 |
| `canvas` returns hit paths beside its `Draw`s | ~80 | pies, cables, knots, waveform scrub | M2 — shipped |
| `State::Disabled` + `.disabled()` gating `Hit::push` | ~60 | bypassed modules, illegal routes | M3 — shipped |
| `Ui::shortcuts()` unfocused key stream; F-keys, Space, PageUp/Down in `Key` | ~40 | undo/redo, F1, list nav | M3 — shipped |
| Sticky as a `Pin` area against the enclosing scroll frame | ~70 | group rack, source rack | M4 — shipped |
| Min-extent query from the intrinsic pass (`resolve` → `min_size`) | ~40 | the host's minimum window size | M4 — shipped |
| `.reserve(s)` + `ResolvedScene::set_text(id, s)` | ~60 | every modulated readout at 60 Hz | M5 — shipped |
| `Palette::from_seed(Color)`; `.text_weight()` | ~60 | 8 group accents, 13 `.typography` sites | M5 — shipped |
| `mui_widgets::curve` over `mui_motion::Curve` (knot hit, handle drag) | ~350 | ADSR, LFO/env, pan, wavetable, warp response | M6 — shipped; insert/remove, magnetic snap and guides left to K4's gestures over `Curve::insert`/`split`/`remove` |
| Typed drag payload + ghost float | ~90 | modulation drag, card reorder | M7 — shipped; the ghost stays the caller's `.float()` |
| Stable node identity (a key distinct from the path, honoured by springs/scrolls/focus/access) | ~100 | every reorder gesture, quietly | M7 — **deferred**: `Id` makes the path cheap to compose, but it is still the path that keys springs/scroll/focus/access (`ROADMAP.md` Missing) |
| `.min_col` against a hugging container (`ROADMAP.md` Missing) | ~60 | every KURV modal | M8 — shipped |

Explicitly **not** MUI's: splitter, file dialogs, import jobs, document undo,
preset audition, route legality, unit formatting, the HSV picker (1 call site),
vertical labels (2 call sites), the virtualised list (promote on a second caller).

### Per-surface line budget

| Surface | Budget | Replaces |
|---|---:|---|
| `host.rs` — truce editor, Vello surface, frame clock | 300 | `mui-gpui` (391) + the 4 ms pump |
| `shell.rs` — masthead, three racks, splitter, scroll | 250 | `shell.rs` 378 + `view.rs`'s 1,135-line `render` |
| `components.rs` — readout, parameter cell, card, chip, graph frame | 250 | `components.rs` 174 + `control_panel.rs` 55 + `readout.rs` 717 |
| `theme.rs` (view side) | 60 | `live_theme.rs` 146 + `components.rs:2-4` |
| synth: engine cards, VA table, unison, pan | 700 | `synth/` 1,309 + `va_table.rs` 1,807 |
| warps rack | 400 | `warps/mod.rs` 1,153 |
| modulators rack + LFO/env/gate surfaces | 450 | `modulators/` 1,311 |
| routing: ports, pies, cables, depth | 400 | `routing.rs` 672 + `modulation.rs` 1,948 |
| groups: header, inline ADSR, output deck | 300 | `groups.rs` 513 + `group_header.rs` 624 |
| curves: bindings onto `mui_widgets::curve` + brush | 250 | `editing.rs`'s 186 curve branches + `curve_editor.rs` 687 |
| samples / grain / resynth | 350 | `samples.rs` 783 |
| wavetable library + function editor | 250 | `wavetable.rs` 407 |
| preset browser | 300 | `presets.rs` 564 + `browser.rs` 1,804 |
| settings / performance form | 150 | `performance.rs` 246 + `settings.rs` 980 |
| manual / F1 overlay | 200 | `editor_manual/mod.rs` 1,365 |
| gesture + shortcut adapter | 120 | `control.rs` 218 + `controls.rs` 83 |
| **Total view** | **4,730** | against B's 21,316 |

Plus ~6,500 domain lines moved by K0 and ~1,240 new MUI lines. If a surface
overruns its budget by more than 50%, that is the signal that a MUI gap is being
hacked around — which is exactly how B reached 13k.

---

## 4. Phased plan

Each row is one PR. Worktrees are `git worktree add ../<name> -b <branch>` off
`main` in the named repo (`git-sprout`, per the house rule). MUI phases M1–M5
land before K1; M6–M8 land before the surfaces that need them.

| # | Branch (worktree) | Repo | Does | Acceptance | Size | After |
|---|---|---|---|---|---:|---|
| M1 ✅ | `mui/pointer-mods` | MUI | modifiers + secondary button on `PointerInput`→`Response`, axis-lock (`mui::Edit`→`Gesture` deliberately not done: `Edit` keeps its name, so KURV disambiguates the `mui_truce::Edit` import at its one call site) | unit tests in `mui-input`; preview gallery still green; one gallery scene drags with Shift | 90 | — |
| M2 ✅ | `mui/canvas-hits` | MUI | `canvas` returns hit paths beside `Draw`s; `Hit::push` takes them | a gallery scene where a drawn arc responds only inside the arc; CPU snapshot unchanged | 80 | — |
| M3 ✅ | `mui/disabled-and-keys` | MUI | `State::Disabled` + `.disabled()` gating `Hit::push`; `Ui::shortcuts()`; F-keys/Space/PageUp | a disabled widget neither paints lit nor hit-tests; a shortcut fires with nothing focused | 100 | M1 |
| M4 ✅ | `mui/sticky-and-extent` | MUI | sticky as a `Pin` area against the scroll frame; `resolve` returns `min_size` | reflow test: a sticky header holds y at 3 scroll offsets; `min_size` matches the intrinsic pass | 110 | — |
| M5 ✅ | `mui/live-text-and-seed` | MUI | `.reserve(s)` + `set_text(id, s)`; `Palette::from_seed`; `.text_weight()` | a readout changes text without re-resolving (assert one resolve per N frames); seeded palette passes the legibility check | 120 | — |
| K0 | `kurv/editor-model` | KURV | extract `kurv::editor_model` (§2); repoint A's and B's 150 `crate::editor*` references; delete D1 + D2 | full `cargo test`; `check_mui_editor.py` unchanged (8 PASS scenarios); no behaviour change | −9,400 | — |
| K1 | `kurv/mui2-skeleton` | KURV | `editor/host.rs` + `shell.rs` behind `--features mui2`: masthead, three racks, splitter, scroll, no content | reflow at 1000×600, 1400×900, 2000×1200: no `InsufficientSpace`, no horizontal overflow, splitter drags; CPU snapshot per size | 550 | M1,M4,K0 |
| K2 | `kurv/mui2-theme-controls` | KURV | `editor_model/theme.rs` + `editor/components.rs`; one real oscillator card bound to truce | a knob drag emits Begin/Value/End through `Automation`; 0 colour literals (CI grep); snapshot at 3 sizes | 400 | K1,M5 |
| K3 | `kurv/mui2-synth` | KURV | engine cards, VA table, unison, pan | `native_shell_recall`-equivalent scenario for VA controls staying in bounds; snapshots | 700 | K2 |
| M6 ✅ | `mui/curve-widget` | MUI | `mui_widgets::curve` over `mui_motion::Curve`: knot hit, handle drag (insert/remove, magnetic snap and guides deferred to K4 over `Curve::insert`/`split`/`remove`) | port `magnetic_snap`/`curve_hit` tests verbatim; a gallery curve scene; drag under 1.5× | 350 | M1,M2 |
| K4 | `kurv/mui2-groups-curves` | KURV | groups + inline ADSR + the curve bindings + brush | the checker's Alt-bend, ENV-stage-drag and Ctrl-brush scenarios, rewritten against the hit map | 550 | K3,M6 |
| K5 | `kurv/mui2-modulators` | KURV | modulator rack, LFO/env/gate surfaces, sticky source headers | sticky header y under scroll; source curve drag + undo scenario | 450 | K4 |
| M7 ✅ | `mui/drag-payload-identity` | MUI | typed drag payload (the ghost stays a caller's `.float()`); composable `Id` — a *stable* identity distinct from the name is still deferred | reorder a gallery list: focus, scroll offset and springs survive; access tree keeps ids | 190 | M1 |
| K6 | `kurv/mui2-routing` | KURV | ports, pies, cables, depth, drag-to-route; `editor_ghost` restored | `editor_ports`' moved tests run against the new hit map; a pie hits as a ring, not a rect | 400 | K5,M7,M2 |
| K7 | `kurv/mui2-structure` | KURV | drag-reorder of cards/groups/warps, add/remove menus | the checker's add-Noise / add-groups / outside-group-extraction scenarios | 350 | K6 |
| M8 ✅ | `mui/min-col-hug` | MUI | `.min_col` against a hugging container; the CPU snapshot at 1×/1.5×/2× with the scale contract asserted | the Responsive gallery scene at 240×600 and 2000×300; two new snapshot factors | 80 | — |
| K8 | `kurv/mui2-browsers` | KURV | preset browser, samples/grain/resynth, wavetable, import jobs | round-trip: import a sample, audition a preset, undo; modals reflow at 3 sizes | 900 | K7,M8 |
| K9 | `kurv/mui2-settings-manual` | KURV | settings/performance form, F1 manual overlay | the form drops to one column at 1000 px; F1 pops the component under the pointer | 350 | K8 |
| K10 | `kurv/mui2-checker` | KURV | rewrite `tools/check_mui_editor.py` against the hit map: assert frames and `Interaction` responses, run at 1×/1.5×/2×, publish measured idle and drag frame times | the whole scenario list green at three scale factors; a number replaces the 165 fps claim | 250 | K9 |
| K11 | `kurv/mui2-default` | KURV | flip `mui2` to default on Linux, keep `mui-editor` and `legacy-egui` behind flags for one release | one release cycle of the new default with no regression report | 20 | K10 |
| K12 | `kurv/drop-old-editors` | KURV | delete D3, D4, D5 (plugcat, B2, the pinned probe and its worktree) | `cargo build` on Linux; the checker still green | −27,900 | K11 |
| K13 | `kurv/drop-egui` | KURV | delete D6 (generation A) and tag it | **gated on R1** — a MUI host on Windows and macOS. Not before. | −51,900 | K12, R1 |

Ordering rationale: MUI gaps first because every one of them is a thing B hacked
around; a walking skeleton second because the shell is where B's three
minimum-width policies and 1,135-line `render` came from; surfaces by user value
(synth → groups → modulators → routing → structure → browsers → settings); and
deletion last, gated on evidence rather than on the rewrite merely existing.

---

## 5. Risks and open questions

**R1 — Windows and macOS hosts.** Generation A is the fallback on every
non-Linux target (`Cargo.toml:102,112-113`), and nothing in MUI knows what a
plugin editor window is: `mui-vello` has no window handle and the only host is
`mui-preview/src/host.rs` (238 lines, winit) (`06`). D6 cannot happen until
`editor/host.rs` runs on all three platforms. **Question for the user: is
`editor/host.rs` a KURV file, a new `mui-host` crate, or a fork of
`mui-preview/host.rs`? And does KURV ship a Windows build from this plan, or
does egui stay the Windows editor indefinitely?**

**R2 — baseview / raw window handle embedding.** The plan assumes a
`raw_window_handle`-parented Vello surface driven by `truce`'s editor callbacks.
Nobody has scoped it, it is not on `ROADMAP.md`, and it gates every visible
pixel. It is the single largest unknown in K1. **Question: which embedding path
— baseview, truce's own window, or direct RWH?**

**R3 — the 165 fps target.** It has never been measured. Today's "4 ms pump" is
`recv_timeout` running a full `synchronize` unconditionally 250×/s
(`mui-gpui/src/lib.rs:282-308`), and `MUI-MIGRATION.md:35-36` already admits the
pump is not a frame-rate guarantee. K10 publishes measured idle and drag frame
times. **Question: is 165 fps a requirement or an aspiration? If a requirement,
it should be a gate on K1, not a doc line.**

**R4 — `vello_hybrid` on Windows.** `ROADMAP.md:151` says the hybrid-vs-classic
choice was made on Linux numbers only, and KURV ships CLAP/VST3 on Windows.
Re-measure before K11, not after.

**R5 — canvas cost at display cadence.** 16 oscillator cards, each a `canvas`
closure re-running and re-tessellating every frame (`mui-scene/src/scene.rs:786`),
with `PathCache` keyed on the path so a changing waveform always misses. Measure
in K3 before building damage regions; a content hash on `Vec<Draw>` is the lazy
fix (`06` #9).

**R6 — node identity meets a UI whose primary gesture is reordering.**
`ROADMAP.md:180-184`: focus rings, scroll offsets and access key on `String`
paths. KURV moves modules between groups constantly. M7 must land before K7 or
the bug ships as "the rack jumps when I drag a card".

**R7 — the test tax.** `editor_ports` has 2,316 test lines driving the real egui
editor frame by frame (`01`); they die with egui. B has 59 tests in 17,634
lines. K0 moves the pure ones into `editor_model`; the frame-driving ones must be
re-typed against `mui-input` in K6 or coverage drops by three quarters.

**R8 — two known MUI holes with no owner:** stroke dash patterns (`05` §1, one
call site) and per-stop gradient springs (`05` §5, the requested ADSR gradient).
Both are `canvas` workarounds today. Decide in K4 whether they are worth
crate-level fixes.

**R9 — scope of the manual.** 811 lines of F1 content are portable now; the
1,365-line pop-out presenter is new work on MUI's float layer (K9). If it is out
of scope, say so out loud — silently dropping F1 is how generation B lost it.

---

## What the rewrite should do about it

1. **Delete D1 and D2 this week** — 9,400 lines of non-shipping editor and dead
   probe code, with no dependency on any of the rest of this plan.
2. **Do K0 before any MUI code.** The 150 `crate::editor*` references from B into
   A's view modules are the single reason generation A cannot be deleted and the
   reason B came out at 13k instead of a third.
3. **Land M1 before K1.** An input model with no modifiers and no second button
   cannot express a synth editor, and every widget after it assumes them.
4. **Re-declare A's theme, do not re-derive it.** A called `semantic()` 198×; B
   called it once. Zero colour literals and zero `px(<literal>)` outside a canvas
   closure, enforced by a CI grep, not by a contract in a doc.
5. **Make the hit shape the drawn shape** (M2), then delete `curve_hit` and its
   squared-144 grab radius rather than porting it.
6. **Budget per surface** (§3) and treat a 50% overrun as a MUI gap report.
7. **Rewrite the checker against the hit map and run it at three scale factors**
   before claiming the rewrite looks right. Every screenshot KURV owns today was
   captured at 1×.
8. **Gate D6 on a Windows and macOS host**, and answer R1/R2 before K1 starts.
9. **Say "4.6k of view code over 6.5k of moved domain code", not "a third".**
   The denominator in issue #8 is 13k; the real generation-B figure is 21.3k
   across two repos.
