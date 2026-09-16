# 05 — UI quality and breakage: what the user actually sees

Read-only audit of both KURV editor generations against what current MUI can
express. Every number came from a `wc`/`grep` run here; every claim cites a
line range that was opened and read.

## Corpus and measured sizes

| Thing | Path | Lines | Files |
|---|---|---:|---:|
| A — egui reference editor | `KURV/src/editor_*.rs` + `src/editor_*/` | 54,625 | 121 |
| B1 — first GPUI preview | `KURV/src/editor_mui.rs` | 3,352 | 1 |
| B2 — live Linux editor | `KURV/src/editors/mui/` | 13,120 | 21 |
| Pinned MUI snapshot | `.worktrees/kurv-mui-runtime-0.20.37/crates/` + `experiments/gpui-plugin` | 15,593 | — |
| Current MUI | `MUI-migration/crates/` | 21,392 | 15 crates |

`src/lib.rs:9-18` shows both GPUI generations are compiled: `mod editor_mui`
(B1) and `#[path = "editors/mui/view.rs"] mod mui_shell` (B2), each exporting
its own `create`. B2 still imports `editor_mui::{binding, workspace,
distribution_with_gain, SourceWaveform}` — the *data* layer is shared, the
*layout* layer is duplicated. B1's root render is a second, worse layout tree
that nothing in the product needs.

**Verdict: Redundant.** 3,352 lines of B1 are a second layout language kept
alive for four helper items. Delete the render, keep `binding`/`workspace`.

---

## 1. Absolute placement and magic numbers

### Counts

Generation A, `pos2( | Rect::from | vec2( | Vec2::new | Vec2::splat`:
**835 sites across 121 files**. Top modules:

```
76 src/editor_shell/browser.rs      30 src/editor_card/header.rs
48 src/editor_shell/header.rs       28 src/editor_performance.rs
33 src/editor_oscillator/va_table.rs 27 src/editor_card/layout.rs
32 src/editor_resynth.rs            26 src/editor_generator/group_output.rs
25 src/editor_ports/param_tests.rs  25 src/editor_lfo/controls.rs
24 src/editor_filter/painting.rs    23 src/editor_manual/mod.rs
21 src/editor_generator/insertion/drag_reorder.rs  18 src/editor_distortion.rs
18 src/editor_card/header/group_tab.rs
```

Generation B (`px( | pos2( | Rect::from | point( | absolute`), per module:

```
85 src/editors/mui/editing.rs       15 src/editor_mui.rs
28 src/editors/mui/view.rs           8 src/editors/mui/performance.rs
28 src/editors/mui/modulators/surface.rs  7 src/editors/mui/synth/pan.rs
27 src/editors/mui/samples.rs        5 src/editors/mui/wavetable.rs
18 src/editors/mui/groups.rs         5 src/editors/mui/warps/mod.rs
                                     5 src/editors/mui/structure.rs
                                     3 presets.rs / curve_brush.rs, 1 aux.rs
```

`.absolute()` alone: **30 sites** in B (view 8, editing 10, structure 3,
groups 3, surface 3, samples 2, warps 1).

Token discipline, the number that matters: B has **118 `px(<literal>)` sites
against 19 `px(crate::editor_theme::…)` sites** — 86% of B's pixel values are
untokenized. A has **669 `editor_theme::{space,shape,font}` references** and
44 literal `Color32::from_*`. A is the disciplined generation; B threw the
scale away.

Scale coherence in B: 8 distinct gap/pad literals (`0 2 4 5 6 8 10 12`),
**5 distinct radii** (`5 6 10 16 20`), **9 distinct text sizes**
(`9 10 11 12 13 16 18 24 30`). A's theme declares exactly 5 spacings
(`editor_theme.rs:205-209`), 4 font sizes (`:220-223`) and **one** radius
(`shape::CONTROL_RADIUS = 2.0`, `:258`).

### The worst 15

1. `src/editor_mui.rs:526,621,766` — `.w(px(300.))` modulators, `.w(px(248.))`
   warps, `.min_w(px(760.))` groups. Three fixed columns.
2. `src/editor_mui.rs:511,520,616,707` — `.h(px(228.))`, `.h(px(376.))`,
   `.h(px(268.))`×2. Every card a fixed height regardless of content.
3. `src/editors/mui/editing.rs:1482` — `div().w(px(600.)).h(px(280.))` around
   the curve graph, inside a modal that has no idea how big the window is.
4. `src/editors/mui/editing.rs:1590,1601-1602` — `.w(px(420.)).h(px(130.))`
   then `.w(px(420.)).h(px(24.))`: two hard-coded widths that must agree.
5. `src/editors/mui/editing.rs:1619,1651-1652` — `.flex_wrap().w(px(360.))`
   then options `.w(px(360.)).h(px(28.))`. A wrap container whose width is a
   literal is a breakpoint with no media query.
6. `src/editors/mui/editing.rs:1390` —
   `.max_h((window.viewport_size().height - px(88.)).max(px(100.)))`. `88.` is
   the header+footer height, hand-measured and unowned.
7. `src/editors/mui/editing.rs:602-605` — `.px(px(8.)).py(px(5.)).rounded(px(5.))
   .text_size(px(12.))`. Four values, four scales, none of them shared.
8. `src/editors/mui/editing.rs:655-660,677` — `.w(px(24.)).h(px(24.))` beside
   `.size(px(24.))` beside a tokenized `rounded(px(shape::CONTROL_RADIUS))`:
   the one token in the block sits next to three literals.
9. `src/editors/mui/editing.rs:951` and `:1411` — `.rounded(px(20.))` and
   `.rounded(px(10.))` on sibling containers.
10. `src/editors/mui/modulators/surface.rs:411-417` — `.absolute().ml(px(-6.))
    .mt(px(-6.)).w(px(12.)).h(px(12.))`: an envelope handle centred by hand
    with the negative half of its own size.
11. `src/editors/mui/modulators/surface.rs:308-317` — dashed guide built by
    `separator.line_to(point(x, (cursor + px(3.)).min(bottom)))` with
    `cursor += px(7.)`. A dash pattern as a loop counter.
12. `src/editors/mui/groups.rs:168-187` and `:244-247` — the ADSR graph inset
    is `px(2.)` and `px(4.)` written eight times in two path builders, while
    `editor_theme::graph_inset(ui)` exists (`editor_theme.rs:339`) and is the
    documented token (`AGENTS.md`AGENTS.md "Layout").
13. `src/editors/mui/components.rs:27-37` — `parameter_tinted` reserves width
    with a **string table keyed on the control's title**: `"WAVE" => "4.00"`,
    `"PAN" => "L 100%"`, `"GAIN" => "−60.0 dB"`. A hand-maintained min-content
    width, wrong the moment a formatter changes.
14. `src/editors/mui/view.rs:798-799` — `height = viewport.height - 68.`, and
    `:804` the popup anchor is `window.mouse_position() + point(px(0.), px(4.))`.
15. `.worktrees/…/experiments/gpui-plugin/src/control_panel.rs:52-55` —
    `engine_panel` is `.p(px(16.))`, title `.h(px(20.))`, graph `.mt(px(14.))`,
    row `.mt(px(12.)).h(px(46.))`. The fixed **46 px control row** the KURV
    migration doc already blames for the screenshot overflow.

Honourable mentions, generation A: `src/editor_shell/header.rs:37-38` is a
hand-rolled CSS `clamp`:
`(width * 0.17).max(unit * 8.0).min(width * 0.21)` — and there are **62**
`rect.width() * 0.NN` sites across A. `src/editor_card/layout.rs:379-411` computes five unison cells as
`Rect::from_min_size(pos2(left + i*cell, top), vec2(cell, height))` — a flex
row written as arithmetic, with six literal rects as its test fixture.

**Verdict A: Bad but honest.** The absolute math is everywhere, but it is
*derived* from `ui.max_rect()` and fed by real tokens, so it survives resize.
**Verdict B: Bad.** The literals are terminal values, not derived ones.

**What current MUI fixes structurally:** the whole class. `.w(clamp(min, pct,
max))` replaces header.rs:37-38 exactly (README, "Responsive"). `row!`/`col!`
with `.gap(M).pad(L)` replaces layout.rs's cell arithmetic. `.radius(Corner::
Field | Selector | Box)` collapses B's five ad-hoc radii onto the theme's
`Corners` vocabulary. `.pad(step(1.5))` covers the off-scale values that are
genuinely wanted. A handle centred on a point is `.anchor(x, y)` on a float,
not `ml(-6)`. `graph_inset` becomes `.pad(Xs)` on the canvas node.

**What MUI still lacks:** a *dash pattern* on a stroke. `surface.rs:308-317`
would still be a `canvas(|size| …)` loop. `Draw`/`Style.stroke` carry a width,
not a dash array. Name it: **stroke dash patterns**.

---

## 2. Layouts that break on resize

`src/editor_mui.rs:755-773` is the clearest failure in the tree. The root is
`div().flex().flex_1().min_h_0().gap_2()` with three children:
`.w(px(248.))` warps, `.flex_1().min_w(px(760.))` groups, `.w(px(300.))`
sources. Floor = 248 + 760 + 300 + two `gap_2` = **1,324 px**. The row declares
no `overflow_x_scroll`; each *column* declares `overflow_y_scroll`. Narrower
than 1,324 px, the third column is pushed out of the window with nothing to
scroll it back.

The live shell B2 is better — `Resolved::responsive(tree.min(0., height),
width, height, window)` at `view.rs:964`, and the pinned `dsl.rs:46-48`
measures an intrinsic `Hug` pass first — but the modals inside it are not.
`editing.rs:1619` wraps at a literal 360 px; `presets.rs:390` is
`.w(px(460.))`, `samples.rs:322` `.w(px(630.))`, `wavetable.rs:308`
`.w(px(440.))`; `performance.rs:124,130-131` is a `flex_wrap` row of
`.w(px(206.)).h(px(32.))` fields inside a `.w(px(420.))` box — exactly two
per line, forever, whatever the window does.

Declared minimums in 13,120 lines of B2: **2 `min_w`, 3 `max_w`.** One of the
two is `surface.rs:119`'s `.min_w(px(1.))`, which is not a minimum, it is a
disabling of one. The migration doc already wrote this down:
`docs/MUI-COMPONENT-MIGRATION.md`, "Audit of the current preview" — *"allows
cells to shrink to zero width … has no measured minimum width policy for
title/value pairs. This explains the screenshot's wrapping and overflow."*

`flex_wrap` appears **5 times** in B2. Generation A has **393** `wrap|truncate`
sites and **11** `ScrollArea`s. A knows what to do when text does not fit; B
mostly does not.

**Verdict A: Good.** **Verdict B: Bad, and documented as bad.**

**What current MUI fixes:** `.min_col(120.0)` is the auto-fit grid B's modals
are pretending to be (`performance.rs:124`'s two-per-line field box is
`grid![2; ..].min_col(206.0)` and it drops to one column by itself).
`fits![wide, mid, thin]` is the right answer for the VA control row the
migration contract says must never wrap into vertical letters
(`MUI-COMPONENT-MIGRATION.md`, "Layout contract" §4). `clamp()` and `cq()`
size a modal against the window without reading the window. `.scroll()` on the
rack gives the horizontal scroll §4 asks for. And `Error::InsufficientSpace`
carrying the tree's floor means the host can scale by `offered / needs` instead
of silently clipping — B has no equivalent signal at all.

**What MUI still lacks:** `.min_col` on a grid that *hugs* (ROADMAP,
"Missing"). KURV's modals are hugging containers by nature, so the one reflow
primitive they most need is the one with the known hole. Name it:
**`min_col` against a hugging container**.

---

## 3. Alignment, baselines, radii

- **Baselines.** B2 centres label-over-value cells with
  `.items_center().justify_center()` (`control_panel.rs:13-14`) and A stacks
  them with painter offsets. Neither sits text on a shared baseline, so a row
  mixing `"4.00"` and `"−60.0 dB"` at two sizes has two optical baselines.
- **Mixed padding.** `editing.rs:602-603` is `px(8.)/py(5.)`; `:1410` is
  `.p(px(SM))`; `groups.rs:326` is `.p(px(XXS))`; `control_panel.rs:52` is
  `.p(px(16.))`. Four padding vocabularies within one screen.
- **Gaps.** Eight distinct gap literals in B, none of them `MD=12` from the
  theme except by accident.
- **Radii.** A: one radius, `CONTROL_RADIUS = 2.0` — tight corners, exactly
  what `AGENTS.md` demands. B: 5, 6, 10, 16, 20. B broke the house style.
- **Type.** A: four sizes (9.5, 10, 12, 10.75). B: nine, up to `px(30.)` for
  an icon glyph (`editing.rs:1020`) and `px(24.)` for three column headings
  (`editor_mui.rs:530,616,769`).

**Verdict A: Good.** **Verdict B: Bad.**

**What current MUI fixes:** `.baseline()` on a row (ROADMAP: "per-line
baselines … a `.baseline()` row taller than its text keeps its letters in
their frames"). `Corners { selector, field, box_, concave }` with
`.radius(Corner::Field)` gives B's five radii three names. `title/label/caption`
at 18/13/11 plus `Theme.control`'s `Xs..Xl` gives the nine sizes five.
`Spacing` tokens `Xs S M L Xl` resolved *in the solver* mean `.gap(M)` cannot
drift into `.gap(px(10.))`.

**What MUI still lacks:** nothing here. This class is fully covered.

---

## 4. Interaction

### Hit target vs drawn shape

`src/editors/mui/editing.rs:81-101`, `curve_hit`: points are filtered by
`distance(p) <= 144.` — a **squared** distance, so a 12 px grab radius. The
handle it grabs is drawn 12×12 (`modulators/surface.rs:416-417`), a 6 px
radius. The target is four times the area of the dot. Worse, `144.` is in
layout pixels and never touches `window.scale_factor()`, while the *stroke*
beside it does (`groups.rs:195,266` pass `scale_factor()` into
`ui::response_stroke`). Two coordinate systems, one function apart.

`bounds.contains(&position)` at `:86` rejects any grab that begins outside the
graph rect, so a point drawn at phase 0.0 or 1.0 — on the boundary — is
unreachable from the outside half of its own dot.

### Gestures that lose the pointer

`dsl.rs:684-687` (`plot_position`) clamps to `0..1`, so a drag that leaves the
graph pins to the edge rather than jumping — good. But `editor_mui.rs:717-722`
has to install `on_mouse_up` *and* `on_mouse_up_out` on the root to end a
routing drag, and `view.rs:812-814` clears `dragged_module` from
`cx.has_active_drag()` polled during render. Capture is reconstructed by hand
at three levels.

### States

**3 `.hover()` sites in 13,120 lines** (`view.rs:1059`, `editing.rs:611`,
`editor_mui.rs:433`). **Zero press/active styles.** One `.focus_visible`
(`editing.rs:1001`). Nothing you click in KURV's live editor looks pressed.
The pinned snapshot's own `parameter_cell` does better
(`control_panel.rs:13-19`: `tab_index(0)`, `role(Role::Slider)`,
`aria_label`, `cursor(ResizeUpDown)`, `hover`, `focus_visible`) — the KURV
code around it did not adopt the pattern.

By contrast A: **101 `on_hover_text`**, **144 `Sense::`/`interact()`**, and a
`control_visuals()` function (`editor_theme.rs:300`) returning enabled / hover
/ active / focus / disabled in one struct. B has **7** tooltip sites.

### Keyboard, focus, accessibility, IME

B2 has 18 `on_key_down|KeyDownEvent` sites and 16 `focus` mentions, but look at
what they are: `editing.rs:2295` is a **1,173-character single line** carrying
`on_mouse_down` + `on_key_down` + `on_mouse_up` + arrow-key nudging + Shift
fine-mode + undo, minified into one expression. Lines `:2280` (1,058 chars) and
`:2301` (769 chars) are the same. That is not keyboard support, it is keyboard
support hidden where nobody will maintain it.

The pinned snapshot has **no `accesskit`** (0 hits across `mui-core`,
`mui-gpui`, `mui-layout`). Screen readers get whatever GPUI's own `Role`
annotations produce, which for KURV's canvas-drawn graphs is nothing.

IME: the pinned tree has 35 `ime` hits, all inside GPUI's own text input
(`experiments/gpui-plugin/src/text_input.rs`, 669 lines KURV inherited
wholesale). KURV's preset-name and curve-value fields ride it; nothing else
does.

### Tooltips positioned by hand

`view.rs:801-806`: the context menu anchor is captured as
`window.mouse_position() + point(px(0.), px(4.))` the frame the menu opens,
then held. No flip, no fallback area, no clamp to the window. Open a menu near
the bottom edge and it grows off-screen; the only mitigation is
`editing.rs:1390`'s `max_h(viewport.height - 88.)`, which makes the menu
*scroll* instead of *move*.

**Verdict B: Missing** across hit-target fidelity, press states, focus rings,
accessibility and popup placement. **Verdict A: Good** on tooltips, cursors and
focus; **Bad** on nothing in this section.

**What current MUI fixes:**

- Hit vs drawn: solved by construction. *"hit regions [are rebuilt] from the
  paths it painted, so what responds and what you see cannot drift apart"*
  (README) — `mui-input` hit-tests `mui-vello`'s own paths in paint order
  (ARCHITECTURE, "mui-input Hit"). `curve_hit`'s `144.` disappears; you give
  the handle a `.square(12)` frame and MUI tests that frame.
- Lost pointer: `mui-input`'s documented rule — *"a press captures its target…
  Without capture a slider stops tracking the instant the pointer leaves it"*
  (`mui-input/src/lib.rs:216-218`). The three hand-installed `mouse_up_out`
  handlers go away.
- States: `.on(State::Hover, ..)` / `State::Press` / `State::Focus`, resolved
  by the runtime and *sprung* into the fill, one declaration beside the resting
  style. B's three hover sites become a preset.
- Focus & keyboard: `.focusable()` and `Ui`'s Tab/Shift+Tab/Escape focus walk.
- Accessibility: `mui-access` turns the resolved scene into an
  `accesskit::TreeUpdate`, and `.role(Kind::Slider).label("Cutoff")` is on the
  node, not a parallel annotation. Every widget sets it already.
- IME: `Input::ime` carries the platform's four events, the preedit paints
  under the caret without joining the value, and `Frame::ime` places the
  candidate window (ROADMAP, "Done").
- Popups: `.pin(Pin::to("cutoff").area(Area::Bottom).gap(Xs).fallback(Area::Top))`
  — nine named regions, ordered fallbacks tried until one fits the root.
  `view.rs:804`'s `mouse_position() + (0, 4)` is one `Pin`.

**What MUI still lacks, named:**
1. **`State::Disabled`** — ROADMAP, "Missing". KURV disables controls
   constantly (`control_panel.rs` threads an `enabled: bool` through every
   cell, `AGENTS.md` requires "disabled content dims but remains legible").
   Today that is a manual `.opacity()`.
2. **A pin anchored inside another pinned float** — ROADMAP, "Missing". KURV's
   parameter pies float off controls that are themselves in a floating menu.
3. **Stable node identity** — ROADMAP: focus rings, scroll offsets and
   `mui-access` key on `String` paths, so renaming resets state. KURV renames
   groups and reorders oscillators as a core gesture; `"group-3"` becoming
   `"group-2"` on a delete would drop focus and scroll.

---

## 5. Animation

`with_animation|Animation::` in B2: **0**. `ctx.animate|animate_bool|
animate_value` in A: **1**, across 54,625 lines. Neither editor animates
anything. `AGENTS.md` explicitly forbids decoration — *"Playheads may use a
short fading trail; avoid permanent animation"* — so this is policy, not
neglect.

**Verdict: Good (A and B).** Nothing to fix. The risk runs the other way:
current MUI springs hover and press into fills by default, and `.animate()`
covers fill, stroke, radius, text size and shadow. The rewrite must keep the
prohibition and use motion only where a value is being *read*: the sprung
press on a control, `ui.tween` on a meter. `Spring::new(0.3, 1.0)` on a
44-oscillator card stack would be exactly the decoration AGENTS bans.

**What MUI lacks:** spring interpolation of gradient *stops* (ROADMAP). The
ADSR gradient `KURV-REDESIGN-PLAN`/`MUI-COMPONENT-MIGRATION` §6 asks for
("User-requested ADSR gradient/fillets take precedence") cannot animate its
stops today. Name it: **per-stop gradient springs**.

---

## 6. Colour outside the semantic roles

Generation A calls `editor_theme::semantic()` **198 times** and declares a
22-field `KurvPalette` (`editor_theme.rs:175-199`: background, chrome, surface,
control, well, control_hover, grid, text, text_muted, disabled, disabled_text,
primary, pan_shape, unison, envelope, modulator, danger, warp, live, masthead,
masthead_ink). 44 literal `Color32::from_*` remain, most of them the accent
constants at `:16-18` and `:40-42` that *define* the roles.

Generation B calls `semantic()` **once**. It has 25 literal-colour sites:
`components.rs:2-4` hardcodes `SURFACE = Rgb(29,29,29)`, `WELL = Rgb(17,17,17)`,
`RAISED = Rgb(37,37,37)`; `editor_mui.rs:731-732` hardcodes the app background
`0x171d1a` and ink `0xe0e5da`; `view.rs:1725` a *second* background `0x0f0f0f`;
`view.rs:1752` an error colour `0xffaaaa` that is not `palette.danger`, and
`editing.rs:1412,1415` a menu at `0x242424`/`0xeeeeee`. The pinned snapshot is
the same — `control_panel.rs:17-19` reads `live_theme::color(0x9ba697)` and
`0xbadc91` by hex, and `dsl.rs:692-693` hardcodes the drag ghost.

Worse, B derives disabled states by `.grayscale()` on a literal
(`view.rs:1328,1616`, `editing.rs:593,651,952`) rather than from a role at an
alpha, so disabled surfaces drift in luminance against the theme.

**Verdict A: Good.** **Verdict B: Bad.** This is the single largest regression
from A to B and `AGENTS.md` names it directly: *"Use `editor_theme::semantic()`
roles, never literal UI colors."*

**What current MUI fixes:** `Role` + the Oklch palette with checked legibility;
`ink` resolves against its ground automatically (ARCHITECTURE, "roles ->
Palette"). `Role::alpha(0.12)` replaces every `.grayscale()` and every
`rgba(0xffffff08)` hairline with a value that tracks the palette. Group accents
become `.fill(role)` where role is the group's own; `KurvPalette`'s 22 fields
map onto `Theme` + a handful of app roles.

**What MUI still lacks:** nothing structural. A per-instance accent (KURV's
eight group colours, `editor_theme.rs:480`) is a theme clone or a role
override — worth confirming the palette can carry N app-defined roles without
a fork, but nothing is missing.

---

## 7. DPI and scale

The pinned snapshot has no coherent scale story. `control_panel.rs:17-19`
threads a `scale: f32` into `text_size(px(8.5 * scale))` and
`px(16. * scale)` — *text only*. The panel's own `p(px(16.))`, `h(px(20.))`,
`h(px(46.))` at `:52-55` do not scale, so at any factor ≠ 1 the type outgrows
its row. KURV's side: `view.rs:891` mixes `window.scale_factor().to_bits()`
into a cache key, `editing.rs:847` passes it to a renderer, `groups.rs:195,266`
pass it to `ui::response_stroke` — four sites in 13,120 lines. `curve_hit`'s
`144.` (`editing.rs:99`) does not. The native checker pins the problem away:
`tools/check_mui_editor.py:48` sets `'GPUI_X11_SCALE_FACTOR': '1'`, so **every
piece of evidence KURV has about its own editor was captured at 1×.**

**Verdict B: Missing, and untested.**

**What current MUI fixes:** `SceneSpec::scale` / `Ui::scale`. ARCHITECTURE is
explicit: *"Every outline, weld rect and clip comes from one `bounds(frame,
scale)`, and every baseline from one `snap`, so `SceneSpec::device_scale` puts
paint, hit paths and clips on the same device grid or none of them."* One
number at the root, and because hit paths come from the same `bounds`, the
grab radius scales with the dot for free.

**What MUI still lacks:** nothing in the pipeline. What is missing is the
*evidence*: `check_mui_editor.py` must run at 1×, 1.5× and 2×, and MUI's own
CPU snapshot test should gain a 2× case. Name it: **a fractional-scale
snapshot case**.

---

## 8. What generation A got right and must survive

Do not let the rewrite lose these; B already lost most of them once.

- `editor_theme.rs:175-199` — a 22-role semantic palette, used 198 times.
- `editor_theme.rs:202-214,217-246` — a 5-step spacing scale and 4 named font
  roles, used 669 times.
- `editor_theme.rs:300` — `control_visuals()`: one function returning enabled /
  hover / active / focus / disabled.
- 101 `on_hover_text` sites, 144 `Sense`/`interact` sites, 393 `wrap|truncate`
  sites: A knows what every target does and what every string does when it
  does not fit.
- `editor_theme.rs:258` — one radius. Tight corners as house style.
- Zero decorative animation, deliberately.

## What the rewrite should do about it

1. **Delete B1's render.** `src/editor_mui.rs:430-775` is the fixed 248/760/300
   column tree and nothing needs it. Keep `binding`, `workspace`,
   `distribution_with_gain`, `SourceWaveform`; drop 3,352 lines to a few
   hundred. Remove `create_mui_editor` from `lib.rs:16`.
2. **Port A's theme, not B's.** `editor_theme::{semantic, space, font, shape,
   control_visuals}` maps onto `Theme` + `Role` + `Corners` + `.on(State, ..)`
   almost field for field. Budget: `semantic()` must be called everywhere or
   nowhere; the target is **zero** `Rgb(..)`/`rgb(0x..)` literals outside one
   theme module, against B's 25.
3. **Ban `px(<literal>)` in view code.** Today B is 118 literals to 19 tokens.
   Target the inverse. `.gap(M) .pad(L) .radius(Corner::Field)` for structure;
   `step(n)` for the values that genuinely fall between; a canvas closure for
   graph interiors and nothing else.
4. **Declare a minimum, once, per control.** `components.rs:27-37`'s reserve
   string table dies: a value cell measures its own widest formatted string
   through `mui-text` and the row distributes what is left. This is
   `MUI-COMPONENT-MIGRATION.md` §3's "content-based minimum widths" and it is
   a `mui-layout` property, not a lookup table.
5. **Make the three racks `clamp`/`cq`, not `px`.** The 1,324 px floor becomes
   `.w(clamp(200.0, 22.0, 320.0))` on the side racks and a `.scroll()` rack in
   the middle. When the tree genuinely cannot fit, surface
   `Error::InsufficientSpace` and let the host scale by `offered / needs`
   rather than pushing a column off-screen.
6. **Every modal becomes `fits!` or `min_col`.** `editing.rs:1482,1590,1619`,
   `presets.rs:390`, `samples.rs:322`, `wavetable.rs:308`,
   `performance.rs:124` — seven hard-coded modal widths, seven candidates for
   `fits![wide, mid, thin]` or `grid![n; ..].min_col(px)`. Fix `min_col` on a
   hugging grid first (ROADMAP) or these modals inherit the hole.
7. **Every popup becomes a `Pin`.** `view.rs:801-806`'s mouse-anchored menu and
   `editing.rs:1390`'s viewport-minus-88 clamp both collapse into
   `.pin(Pin::to(anchor).area(..).gap(Xs).fallback(..))`.
8. **Delete `curve_hit`.** `editing.rs:81-101` exists only because painted
   shapes and hit regions were separate. In MUI they are the same paths. The
   grab radius becomes the handle's own frame and scales with DPI for free.
9. **Declare press and focus, not just hover.** Three `.hover()` sites and zero
   press states become one preset with
   `.on(State::Hover, ..).on(State::Press, ..).on(State::Focus, ..)`, applied
   at the control layer. Build `State::Disabled` in MUI first — KURV needs it
   on day one and it is the only widely-used state MUI does not have.
10. **Un-minify the gesture code before porting.** `editing.rs:2280,2295,2301`
    are 1,058 / 1,173 / 769 characters on single lines. Whatever those three
    lines mean is the curve editor's entire keyboard and pointer contract, and
    it cannot be reviewed in that shape.
11. **Set roles and labels as you build.** `.role(Kind::Slider).label(..)` on
    every parameter node, so `mui-access` publishes a real tree. The pinned
    stack had zero `accesskit`; there is no reason to ship that twice. Watch
    ROADMAP's node-identity hole: KURV renames and reorders constantly, so key
    scroll and focus on stable module IDs, not display names.
12. **Re-run the native checker at 1×, 1.5× and 2×.**
    `tools/check_mui_editor.py:48` pins `GPUI_X11_SCALE_FACTOR=1`; every
    existing screenshot assertion in `:56-64` is 1×-only evidence. Add the two
    other factors before claiming the rewrite looks right.
13. **Keep the animation prohibition.** Zero decorative animation in either
    generation is a feature. Use springs only for press feedback and sprung
    numbers; do not let `.animate()` reach the card stack.
14. **Hold the line at a third the size.** MUI issue #8 targets 13k → ~4.5k.
    The 118 pixel literals, 30 `.absolute()` sites, 835 A-side rect
    computations and the whole of `curve_hit` are the deletions that get you
    there; they are not incidental to the size goal, they *are* the size goal.
