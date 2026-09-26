# 03 — The pinned MUI snapshot vs current MUI main

Scope: `/mnt/Windows11/DEV_PROJECTS/Repos/.worktrees/kurv-mui-runtime-0.20.37`
(branch `kurv/runtime-0.20.37`, tip `34791fa`) against
`/mnt/Windows11/DEV_PROJECTS/Repos/MUI-migration` (`main`, tip `bb67e73`).
Every number below came out of `wc`/`grep`/`git diff --stat` run in those two
trees. Read-only: nothing was built, edited or committed in KURV or in the
snapshot worktree.

## 1. There is no fork. The snapshot is an ancestor.

```
git -C .worktrees/kurv-mui-runtime-0.20.37 log --oneline -1
  34791fa Restore pinned native dependencies before CI verification
git -C MUI-migration merge-base HEAD 34791fa
  34791faa54318221f82cb41049870fd88feceb00
git -C MUI-migration rev-list --count 34791fa..HEAD
  10
```

`34791fa` is literally in main's history, merged as `863c644`
("Merge pull request #4 from Matari-Audio/kurv/runtime-0.20.37"). Main is
that same commit plus ten commits: PRs #5, #6, #7, #16, #17, #18, #19, #20,
#21. Dates: snapshot tip `2026-09-15 19:52`, main tip `2026-09-16 12:34` —
**seventeen hours**. The whole divergence is one day of work, and it is
one-directional.

Verdict: **Good.** There is no merge to do and no lost snapshot work to
rescue. "Diverged" is the wrong frame; main is a rewrite of the same trunk
and the snapshot's only unique content is what main deliberately deleted.

The shape of those seventeen hours:

```
git diff --stat 863c644 HEAD
  303 files changed, 18081 insertions(+), 51232 deletions(-)
```

Net −33 151 lines. Main is **smaller** than the thing KURV is pinned to.

## 2. Crate lists

`find crates -name '*.rs' | xargs wc -l`, target dirs excluded.

| Snapshot crate | LOC | Main crate | LOC | Gap |
| --- | --- | --- | --- | --- |
| `mui-core` | 6339 | `mui-scene` 2871 + `mui-style` 1761 + `mui-motion` 656 | 5288 | **rename + split** (`item.rs` dropped, see §3) |
| `mui-layout` | 2703 | `mui-layout` | 2945 | rename of internals; modular (`arrange/len/measure/node/pin`) |
| `mui-preview` | 2956 | `mui-preview` | 2804 | none (rewritten: 2109+/2257−) |
| `mui-geometry` | 1980 | `mui-geometry` | 2767 | none (+787: squircles, boolean ops) |
| `mui-truce` | 745 | `mui-truce` | 831 | **none** — see §7 |
| `mui-text` | 629 | `mui-text` | 879 | none (UAX#14, one-pass wrap) |
| `mui-input` | 603 | `mui-input` | 760 | none (+keys/IME/DnD, §6) |
| `mui-vello` | 444 | `mui-vello` | 2145 | none (+1799: became *the* renderer) |
| `mui` | 350 | `mui` | 1474 | **redesigned** — facade became a runtime (`ui.rs`) |
| `mui-egui` | 324 | `mui-egui` | 308 | none |
| `mui-tessellate` | 160 | `mui-tessellate` | 160 | none (1 line changed) |
| `mui-gpui` | 391 | — | — | **dropped on purpose** |
| `mui-demo` | 361 | — | — | dropped on purpose |
| `mui-playground` | 614 | — | — | dropped on purpose |
| — | — | `mui-widgets` | 879 | **new** |
| — | — | `mui-access` | 152 | **new** (AccessKit) |
| `experiments/gpui-plugin` | 8863 | — | — | **dropped on purpose** — and this is the one that hurts (§5) |
| `experiments/render-lab` | 1017 | — | — | dropped |
| `experiments/upstream` (vendored GPUI) | 1 648 554 | — | — | dropped |
| `vendor/truce-clap` | ~5100 | — | — | dropped (patched crates-io override) |

Snapshot library total (crates only, no experiments): **18 599**. Main:
**21 391**. Main is 2.8k lines *larger* in libraries and 1.66M lines smaller
overall, because the snapshot vendors the entire GPUI tree to build at all.
Tests: snapshot 179 `#[test]`, main 263.

Verdict on `experiments/upstream`: **Good riddance.** A 1.65-million-line
vendored UI toolkit inside the plugin's dependency graph is the single
largest fact about generation (B), and main deleting it is the whole
argument for the rewrite.

## 3. Public DSL: `Item` → `El`

The snapshot carries **two** DSLs at once. `crates/mui-core/src/item.rs`
(742 lines, `item()` at :121, `container()` at :145, `Item` at :100) is the
old string-id builder, and `crates/mui-core/src/styled/element.rs` (128
lines) is the newer `pub type El = Node<Element>` payload tree. Main kept
only the second, moved to `crates/mui-scene/src/element.rs`, and deleted
`item.rs` outright (`git diff --stat`: `crates/mui-core/src/item.rs | 742 -`).

KURV is on the **old** one: `grep -rn 'item(' src/editors/mui` = 32,
`Item::` = 2, and `styled::`/`El`/`row![`/`col![`/`.radius(`/`.shell(`/
`.weld(` = **0**. So generation (B) never adopted the DSL that main kept,
even though the snapshot it pins already shipped it.

| Snapshot spelling | Main equivalent | Gap |
| --- | --- | --- |
| `item("id")` / `container([..])` (`item.rs:121,145`) | `block(w,h)` / `row![]` `col![]` `stack![]` `grid![n;]` (`mui-scene/src/dsl.rs`) | **rename**, plus id is now optional (`.id()`) instead of mandatory |
| `Item` (owns `Ui` build, `item.rs:376 build()`) | `El` = `Node<Element>` (`element.rs:35`) | rename; `build()` → `resolve(&SceneSpec)` |
| `.size(Fill, Hug)` (`item.rs:158`), `Sizing::{Hug,Fill,Fixed,Percent}` (`mui-layout/src/lib.rs:139`) | `.w(len)/.h(len)/.square()` + `Len::{Auto,Px,Pct,Clamp}` (`mui-layout/src/len.rs:114`), `Fill`→`.grow(1.)`, `Hug`→`Len::Auto` | **rename + strictly better**: `Len::Clamp{min,pct,max}` has no snapshot equivalent |
| `.pack(SpaceBetween)` (`item.rs:337`) | `.between()` (`dsl.rs:110`), `.center()`, `.start()`, `.end()`, or `.justify(Justify::…)` | rename |
| `.round((20., XL))` → `Rounding::separate(outer,inner)` (`item.rs:58,280`) | `.radius(r)` / `.pill()` (`element.rs` `Styled::radius`), `Corner::{Selector,Field,Box,Concave}` on `Theme::corners` (`mui-style/src/theme.rs:16-88`) | **rename + redesign.** Outer/inner pairs are gone; main gets nesting from `.shell(d, fill)` parallel insets instead |
| `Flow::Auto` (`mui-layout/src/lib.rs:113`) — container flips row↔column to fit | **nothing equivalent**. Main has `fits![a,b,c]` (`node.rs:167`, pick the widest candidate that fits), `.min_col(px)` (`node.rs:402`, auto-fit grid), `.wrap()` (`node.rs:378`), `Len::Clamp` | **missing in main — on purpose.** Three sharper tools replaced one magic one. KURV uses `Flow::` 5 times only, so the port cost is trivial |
| `Direction`/`.extend_toward(dir,"id")`/`.extend_to("id")`/`.merge([ids])` (`item.rs:267-279`) | `.weld(fill)` (`element.rs` `Styled::weld`) paints the filleted union of children; `Carve::{Cut,Keep}` via `.cut()/.keep()` (`dsl.rs:126-136`) | **rename + generalisation.** KURV uses `.merge(` once and `.extend_to` once |
| `.typography(size, weight)` (`item.rs:224`) | `.text_size(px)` (`element.rs` `Styled::text_size`) + `title()/label()/caption()` (`dsl.rs:146-160`) | **weight is missing in main.** KURV calls `.typography(` 13 times |
| `.vertical_text()` (`item.rs:234`) | — | **missing in main.** 2 uses in KURV |
| `.hover_only()` (`item.rs:243`), `.hover_color()` (:262) | `.on(State::Hover, \|s\| …)` (`element.rs` `State`, `StateStyle`) | rename + better (Hover/Press/Focus, not just hover) |
| `.replace_color(from,to)` (`item.rs:202`), `.outline_color()` (:189) | `.fill()`, `.stroke()`, `Fill::{Role,Solid,Gradient}` (`mui-style/src/style.rs`) | rename |
| `.on_tap("action")` string actions (`item.rs:248`) | `Ui::get(id) -> Response`, `Frame::edits` (`mui/src/ui.rs:200,50`) | **redesign — better.** KURV has 8 `.on_tap(` |
| `.reserve_text()` (`item.rs:184`) + `Resolved::set_text()` (`gpui-plugin/src/dsl.rs:105`) | — | **missing in main.** §8 |
| `.scope()` (`item.rs:173`) | id paths are implicit in `Node` keys | rename |
| `.opacity(f)` (`item.rs:239`) | `Color::with_alpha` (`mui-style/src/color.rs:83`), `Mix` | rename. KURV uses `.opacity(` 18 times |
| `.columns()/.rows()/.cell()/.span()/.place()` (`item.rs:356-372`) | `grid(n,..)`, `.span(cols)`, `.min_col(px)` | rename; KURV uses **none** of them (0 hits) |
| `Colors`/`Seeds`/`Accent`/`Rgb::contrast_on` (`mui-core/src/rgb.rs:42,132,160,167`) | `Color` (Oklch `AlphaColor`), `Pigment` (`color.rs:320`), `Palette::layer(level)` (:448), `readable_on(bg, ratio)` (:136), `levels_for(ratio)` (:477) | **redesign.** `grep -c Seeds crates` in main = **0**, `contrast_on` = **0** — the names are gone but the capability is larger (per-level separation, gamut validity) |

Verdict on the DSL: **Good, and the rename table is the whole port.** `El`
says the same things in fewer tokens — the README proves it at
`README.md:165` with a `col![row![…].between().w(240)]` one-liner against
the four-line `column([row([...]).align(..).justify(..).width(..)])`. Three
real losses: font weight, vertical text, live text (§8).

Verdict on `item.rs`: **Redundant, correctly deleted.** Mandatory string ids
on every node, `Result` on every build, and a parallel `Ui` type that was not
the runtime `Ui`. 742 lines to say what `Node<Element>` says in 128.

## 4. Renderer: GPUI → Vello

Snapshot `crates/mui-gpui/src/lib.rs` is **391 lines** and contains exactly
three public items: `GpuiEditor<V: EmbeddedView>` (:45), `new` (:53),
`snapshot` (:63). It is a plugin-window host shell, not a renderer. The
actual GPUI painting is in `experiments/gpui-plugin/src/dsl.rs`, which
imports `gpui::{App, CursorStyle, Div, Hsla, PathBuilder, ..., canvas, div,
font, px, rgb}` at line 3 and rebuilds MUI frames as GPUI `Div`s.

Main's `mui-vello` went 444 → 2145 lines (`+1799/−84`) and gained
`Gpu<'a>` (:226), `Cpu<'a>` (:363), `Atlas` (:235), `ImageIds` (:247),
`PathCache` (:550, with `hits()`/`misses()`), and `paint_cached` (:673).
Snapshot `mui-vello` had only `bez_path`, `brush`, `paint`.

| Snapshot | Main | Gap |
| --- | --- | --- |
| `mui-gpui::GpuiEditor`/`EmbeddedView` | — | **dropped on purpose.** KURV must supply its own window/embedding |
| `gpui-plugin/src/dsl.rs::Resolved` → `Div` tree | `mui_vello::paint(_cached)` over `ResolvedScene` | **redesign** |
| GPUI text system (`shape_text`, `dsl.rs:20-37`) | `mui-text` (879 lines, UAX#14 line breaking) | rename |
| vendored `experiments/upstream` GPUI | — | dropped; **biggest single win** |
| `render-lab` benchmark harness | `BENCHMARKS.md` (315 lines changed) | rename |

Verdict: **Good.** The snapshot's renderer was never MUI's — it was 8.9k
lines of KURV-specific GPUI glue living in an `experiments/` folder, with a
1.65M-line vendored toolkit under it. Main's Vello path is the renderer MUI
actually owns. The one thing still open is `ROADMAP.md:151` — `vello_hybrid`
vs classic Vello was decided on Linux numbers only.

## 5. The real blast radius is `experiments/gpui-plugin`, not `mui-core`

`grep -rhn 'use mui' src/editors/mui src/editor_mui.rs` in KURV:

```
  5  mui_gpui_plugin_probe::dsl::*;
  5  mui_gpui_plugin_probe::{ ...
  2  mui_gpui_plugin_probe::text_input::TextInput;
  2  mui_truce::{Edit, Parameter};
  1  mui_gpui_plugin_probe::modulation::{Route, RouteOwner};
  1  mui_gpui_plugin_probe::live_theme;
  … (11 of 13 import lines are mui_gpui_plugin_probe)
```

KURV's 16 472-line mui editor imports **`mui` itself zero times**. It imports
`mui_gpui_plugin_probe` (the 8 863-line `experiments/gpui-plugin` crate that
main deleted) and `mui_truce`. Everything KURV thinks of as "MUI" is
re-exported through `gpui-plugin/src/dsl.rs:6` (`pub use mui::prelude::*;`).

Inside that crate: `modulation.rs` 1948, `oscillator.rs` 1756, `dsl.rs` 760,
`curve_editor.rs` 687, `text_input.rs` 669, `group_header.rs` 624,
`kurv.rs` 580, `panel.rs` 559, `main.rs` 550. That is not a framework — it is
a second KURV editor wearing an experiments folder as a disguise.

Verdict: **Bad, and it is generation (B)'s original sin.** The real KURV
editor is 16 472 + 8 863 = **25 335 lines**, not ~14k, and a third of it sits
in the *MUI* repo where nobody counts it. The rewrite's line-count target
(MUI issue #8, "a third of the size") must be measured against 25 335.

### The slot contract

`Resolved::slot(id, child)` (`gpui-plugin/src/dsl.rs:119`) looks up a frame
by id and drops an arbitrary GPUI element into it. KURV calls `.slot(` **56
times**. This is the architecture: MUI lays out, GPUI paints anything real.

Main's equivalent is `canvas(|size| -> Vec<Draw>)`
(`mui-scene/src/element.rs:220`, `Content::Canvas` at :116, `Canvas` at :46),
which is a *paint* callback returning `Draw::fill`/`Draw::stroke` paths, not
an element host. It covers the drawing-only slots (waveforms, response
curves, the axis cross) and not the interactive ones (text input, drag
handles, tooltips) — but main covers those with real widgets instead
(`mui-widgets/src/widgets.rs`: `slider` :235, `knob` :299, `button` :352,
`toggle` :379, `text_input` :452).

Verdict: **slots are Redundant once the renderer is MUI's own.** A slot only
exists because two layout systems had to be reconciled. One renderer, no
slots. Budget: of KURV's 56, the drawing ones become `canvas()`, the
control ones become `mui-widgets` calls, and the count should land near zero.

## 6. Gesture and input

`crates/mui-input` went 603 → 760 (`+165/−8`). Snapshot had `Hit`, `push`,
`at`, `PointerInput`, `Response`, `Interaction`, `get`, `hovered`, `held`.
Main added, per `git diff 863c644 HEAD -- crates/mui-input/src/lib.rs`:

- `Hit::push_clipped` (:63) — scroll containers clip their own hit regions
- `Key` (:114), `Mods` (:130), `KeyPress` (:138) — keyboard at all
- `Ime` (:147) — composition
- `Input` (:165) with `wheel`, `keys`, `text`, `clipboard`, `ime`
- `Response.drop_target` / `.dropped_on`; `Interaction::pressed` (:373),
  `::dropped` (:379) — drag and drop

The snapshot had no keyboard, no IME, no clipboard, no drag-and-drop in
`mui-input`; `experiments/gpui-plugin/src/text_input.rs` (669 lines) and
`reorder_drag`/`reorder_handle` (`dsl.rs:697,705`) reimplemented all of it
against GPUI.

Verdict: **Good — 669 lines of KURV text editing are now framework code**
(`mui-widgets::text_input`, `widgets.rs:452`, plus `Ui::preedit`,
`set_ime_caret`, `set_clipboard` at `mui/src/ui.rs:272,278,283`).

## 7. `mui-truce`: the one thing that did not move

```
git diff --stat 863c644 HEAD -- crates/mui-truce
 crates/mui-truce/src/document.rs   | 111 ++++++++++++++-------
 crates/mui-truce/src/lib.rs        |   2 +-
 crates/mui-truce/tests/contract.rs |  37 +++++-
```

`src/parameter.rs` is **byte-identical** (`git diff --stat` on it is empty).
`Edit`, `Automation`, `Parameter::{new, new_many, value, text, begin, set,
drag, end, cancel, set_enabled, step, reset, parse}` (`parameter.rs:9-165`)
are unchanged. KURV's only non-probe MUI import is
`mui_truce::{Edit, Parameter}` — so KURV's entire parameter binding ports
with **zero changes**.

`document.rs`'s only change is `Result<_, &'static str>` → a typed
`Error` enum (`document.rs:14-46`: `UnsupportedVersion`, `TooLarge`,
`Malformed`, `Invalid`, `IdentityReuse`, `IdsExhausted`, `PoisonedLock`,
`Other`), with `Display` and `std::error::Error`. Call sites that used the
string need `.to_string()` or a match.

Verdict: **Good, and the best news in this document.** The layer that talks
to the host and owns the state envelope is the one layer the rewrite does not
touch.

## 8. What the snapshot learned from KURV that main never got

These five were invented in `experiments/gpui-plugin` under product
pressure. `grep -rn --include='*.rs' <name> crates` in main returns 0 for
all of them.

| KURV-learned feature | Snapshot site | Main | Verdict |
| --- | --- | --- | --- |
| **Native slots** — `slot(id, child)`, `element(..)` | `dsl.rs:119,130`; 56 call sites in KURV | `canvas()` only | **Redundant.** Artefact of two layout systems. Do not port. |
| **Responsive contract** — `Resolved::responsive()` vs `::new()`, `minimum_width` computed from an intrinsic `Hug` pass | `dsl.rs:41-95`; 2 KURV call sites | `Len::Clamp` (`len.rs:137`), `.min_col` (`node.rs:402`), `.wrap()`, `fits!` | **Mostly covered, one gap: main has no "what is the smallest this tree can be" query.** A plugin window has a minimum size the host must be told. Worth adding to main: `ResolvedScene`/`resolve` should expose the hug-pass extent. |
| **Sticky positioning** — `sticky_slot(id, scroll, stack_top, child)`, `struct Sticky` | `dsl.rs:109`, `:715-717`; 1 KURV call site | nothing (`grep sticky` = 0) | **Missing, and main needs it.** `Pin`/`Area`/`Match` (`mui-layout/src/pin.rs:20,49`) solve anchored floats, not scroll-pinned headers, and a scrolling parameter list with group headers is the exact shape of a synth editor. One flag on `Node` inside the scroll pass. |
| **Live projected values** — `.reserve_text("widest value")` (`item.rs:184`) + `set_text(id, s)` (`dsl.rs:105`), update a readout without re-resolving geometry | 2 + 5 KURV call sites | nothing | **Missing, and main needs it.** A modulated parameter readout changes every frame at 60 Hz; re-resolving the scene to change "1.2 kHz" to "1.3 kHz" is the wrong shape and the jitter from a re-measured label is a visible bug. Reserve-widest-then-substitute is the correct fix and it is small. |
| **HSV picker** — `color_picker(id, [u8;3], change)` with hue and SV planes, keyboard, `Role::Slider`, aria labels, and a round-trip test (`hsv_preserves_rgb_and_wraps_hue`) | `dsl.rs:600-663`; 1 KURV call site | nothing | **Missing, low priority.** 64 lines, one call site, entirely paintable with `canvas()` + two `Interaction` regions. Belongs in KURV, not in `mui-widgets`, until a second product wants it. |

Also snapshot-only and worth a verdict:

- **`live_theme.rs`** (146 lines) — derives a whole `Colors` from one seed
  via `Palette::resolve(Mode::Dark)` then forces WCAG ratios with
  `contrast_on(&backgrounds, 4.5)` (`live_theme.rs:6-40`). KURV touches
  `live_theme` **38 times**, more than any other probe module. Main's
  replacement is stronger — `Palette::layer(i32)`, `separation(a,b)`,
  `levels_for(ratio)`, `readable_on(bg, ratio)`
  (`mui-style/src/color.rs:448,467,477,136`) on Oklch — but there is **no
  seed→palette constructor**: `grep Seeds` in main = 0. **Missing.** KURV's
  user-chosen group colours need `Palette::from_seed(Color) -> Palette`.
- **`fitted_label()`** (`dsl.rs:664`) — shrink a label to its column using
  shaped advances. Main's `fits!` (`element.rs:207`) picks among candidate
  *subtrees*, which is the better primitive. **Redundant.**
- **`AxisLock`** (`dsl.rs:361-390`) — shift-constrains a 2D drag to one axis.
  Generic gesture behaviour, 30 lines, nothing like it in `mui-input`.
  **Missing, small, belongs in `mui-input::Interaction`.**
- **Font weight** — `.typography(size, weight)` (`item.rs:224`), 13 KURV
  call sites. Main's `Styled::text_size` takes px only. **Missing.** The
  `ponytail:` note at `mui-scene/src/dsl.rs:139` already admits `Theme` has
  no type scale. Weight is the same hole.
- **Vertical text** — `.vertical_text()` (`item.rs:234`), 2 KURV call sites.
  **Missing**, and honestly it is one rotated `canvas()`. Do not add to core.

## 9. What main has that the snapshot never did

Reading `git diff --stat 863c644 HEAD`: `mui-vello +1799/−84`,
`mui +1307/−179`, `mui-layout +2463/−2220`, `mui-geometry +1163/−376`.

- `Ui` as a real runtime — `Ui::frame(root, size, input, dt) -> Frame` with
  `cursor`, `tip`, `animating`, `edits`, `clipboard`
  (`mui/src/lib.rs:8-20`, `ui.rs:25,50,371`). The snapshot's `Ui`
  (`item.rs:459`) was a resolved-scene holder with no input and no time.
- `mui-widgets` (879 lines): `slider`, `knob`, `button`, `toggle`,
  `text_input`, `Control`/`Variant`, presets `panel/card/glass/chip/tile`.
  KURV hand-rolled every one of these in `gpui-plugin`.
- `mui-access` (152 lines) + `Kind`/`Semantics` (`element.rs:125,138`) —
  AccessKit roles on the tree.
- `mui-motion` (656 lines): `Spring`, `curve`; `Ui::tween`/`tween_with`
  (`ui.rs:184,189`).
- `Pin`/`Area`/`Match` anchored floats with flip-to-fit
  (`mui-layout/src/pin.rs`).
- `Carve::{Cut,Keep}` boolean shape ops, squircles, `CornerStyle`.
- `State::{Hover,Press,Focus}` + `StateStyle` declarative state styling.
- `PathCache` with hit/miss counters (`mui-vello/src/lib.rs:550-575`).
- 263 tests vs 179, and README/ARCHITECTURE/ROADMAP are doctested
  (`mui/src/lib.rs:52-55` compiles every runnable README block).

Verdict: **Good.** Main did in seventeen hours what `gpui-plugin` spent 8.9k
lines faking, and the README's "Kurv on MUI" sketch
(`README.md:140-193`) is a 46-line editor shell with no coordinates in it.

## 10. Summary table

| Snapshot concept | Main equivalent | Gap |
| --- | --- | --- |
| `mui-core` | `mui-scene` + `mui-style` + `mui-motion` | rename (split) |
| `Item`, `item()`, `container()` | `El`, `block()`, `row!/col!/stack!/grid!` | rename |
| `.size(Fill, Hug)` | `.grow(1.)` / `Len::Auto`, `.w()/.h()` | rename |
| `.pack(SpaceBetween)` | `.between()` | rename |
| `.round((20., XL))` | `.radius()` / `Corner` + `.shell()` | rename + redesign |
| `Flow::Auto` | `fits!`, `.min_col`, `.wrap`, `Len::Clamp` | dropped on purpose |
| `.extend_to`/`.merge` | `.weld()`, `Carve` | rename |
| `.typography(size, weight)` | `.text_size(px)` | **weight missing in main** |
| `.vertical_text()` | — | missing (do not add) |
| `.on_tap("action")` | `Ui::get()`, `Frame::edits` | redesign |
| `.reserve_text` + `set_text` | — | **missing in main — add** |
| `Seeds`/`Colors`/`contrast_on` | `Pigment`/`Palette::layer`/`readable_on` | rename + redesign; **seed constructor missing** |
| `mui-gpui`, `experiments/gpui-plugin` | `mui-vello` + `mui-widgets` | dropped on purpose |
| `Resolved::slot` (56 uses) | `canvas()` + `mui-widgets` | dropped on purpose |
| `Resolved::responsive` / `minimum_width` | `Len::Clamp`, `.min_col`, `.wrap` | **min-extent query missing** |
| `sticky_slot` / `Sticky` | — | **missing in main — add** |
| `color_picker` | — | missing (keep in KURV) |
| `AxisLock` | — | missing (add to `mui-input`) |
| `mui-truce::parameter` | identical | **none** |
| `mui-truce::document` `&'static str` | `document::Error` enum | rename |
| `mui-demo`, `mui-playground`, `render-lab`, `experiments/upstream`, `vendor/truce-clap` | — | dropped on purpose |

## What the rewrite should do about it

1. **Stop calling it a 14k-line editor.** It is 16 472 + 8 863 = 25 335 lines
   across two repos. Issue #8's "a third of the size" target is ~8 400, not
   ~4 600. Say so in the plan before anyone measures success against the
   wrong denominator.
2. **Do not merge, do not cherry-pick.** The snapshot is an ancestor of main
   (`merge-base` = `34791fa`). Port KURV forward; there is nothing to pull
   back except the five items in §8.
3. **Delete `experiments/gpui-plugin` from the plan entirely**, including
   `oscillator.rs` (1756) and `modulation.rs` (1948). Those are KURV product
   code that got filed in the wrong repo. Move what survives into KURV as
   ordinary modules; do not recreate a probe crate.
4. **Port `mui-truce` usage with a `sed`.** `parameter.rs` is byte-identical;
   `document.rs` needs `&'static str` → `document::Error` at the call sites.
   This is the one part of KURV that should compile on day one.
5. **Add scroll-sticky to `mui-layout`.** A flag on `Node` honoured inside
   the scroll pass, plus a stack offset for nested headers. It is the one
   §8 feature with no workaround, it is why `sticky_slot` exists, and a
   scrolling module list with pinned group headers is the KURV editor.
6. **Add reserve-and-substitute text to `mui-scene`.** `.reserve(s)` widens
   the measured box to the widest value; `ResolvedScene::set_text(id, s)`
   swaps the glyphs without re-resolving. Without it, every modulated readout
   either re-layouts at 60 Hz or jitters.
7. **Add a min-extent query.** `resolve` already runs an intrinsic pass; have
   it return `min_size`. A plugin window has to report its minimum to the
   host, and `Resolved::responsive` (`dsl.rs:46`) was KURV's workaround.
8. **Add `Palette::from_seed(Color)`.** KURV touches `live_theme` 38 times
   and lets the user pick per-group colours. Main deleted `Seeds` and
   `contrast_on` and did not replace the constructor — only the query side
   (`readable_on`, `levels_for`). Thirty lines on top of `Pigment`.
9. **Add font weight to `Styled`.** `.text_weight(f64)` beside
   `.text_size(f64)`. 13 call sites in KURV, and the `ponytail:` comment at
   `mui-scene/src/dsl.rs:139` already flags the missing type scale.
10. **Add axis-lock to `mui-input::Interaction`.** 30 lines
    (`gpui-plugin/src/dsl.rs:361-390`), generic drag behaviour, wanted by
    every 2D pad in the editor.
11. **Keep the HSV picker and vertical labels in KURV.** One call site each.
    `canvas()` plus two `Interaction` regions covers the picker; a rotated
    `canvas()` covers vertical text. Do not put either in core until a
    second product asks.
12. **Budget the slot rewrite honestly.** 56 `.slot(` sites split into
    `canvas()` draws and `mui-widgets` controls. The `mui-widgets` side
    (`slider`/`knob`/`button`/`toggle`/`text_input`) should delete
    `text_input.rs` (669), `controls.rs` (83) and most of `panel.rs` (559)
    outright — call it ~1 300 lines gone for free.
13. **Do not port `Flow::Auto`.** Five uses in KURV; replace each with
    `Len::Clamp`, `.wrap()`, `.min_col()` or `fits!` and write down which,
    because that mapping is the honest test of whether main's three
    primitives really beat the one magic container.
14. **Re-measure before locking the renderer.** `ROADMAP.md:151` says
    `vello_hybrid` vs classic Vello was decided on Linux numbers only, and
    KURV ships CLAP/VST3 on Windows.
