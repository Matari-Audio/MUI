# Proposal: a compacter MUI DSL, and the paint it deserves

**Status: shipped on `feat/styling` (`0df76b6..`), all three PRs plus the
queue behind them. Per item, below. What is deferred is listed under
[Deferred](#deferred) at the end; nothing here was abandoned.**

Synthesis of `01-stylex` … `06-geometry-and-effects`, read against the DSL as it
stands today (`mui-core/src/{element.rs,style.rs,dsl.rs}`, `mui/src/widgets.rs`,
`mui-preview/src/scenes.rs`). Ranked, opinionated, sized. Nothing here is a
framework; every item is a function, a field, or an enum variant.

---

## Part 1 — The DSL

Four candidate directions were on the table. The ranking:

### 1. Typed presets as plain functions, plus a per-field merge — **shipped**

`Style::over` (per-field, a field's default *is* its unset -- no `set: u16`
bitmask was needed), `Paints::{preset, base, apply}` with `.styled` deleted,
`Styled::on(State, f)` resolved by `Ui` before transitions, `Sugar::full`,
`Spacing::Step` over `SpacingScale::unit`, `Role::alpha` via `Fill::Faded`,
`mui::presets::{panel, card, glass, chip, tile}`, and the `pad` bug fixed.
`Paints` is a second trait so a bare `Style` takes the same builders.


The whole "variants / recipes / design system" feature, in Rust, is:

```rust
// mui/src/presets.rs — no macro, no registry, no trait
pub fn panel() -> Style { Style { fill: Surface.into(), radius: 16.0.into(), ..<_>::default() } }
pub fn chip(s: &str) -> El { row![caption(s)].pad_xy(10., 4.).pill().fill(Field) }
```

plus two methods that do not exist yet:

```rust
fn preset(self, s: &Style) -> Self;   // merge s OVER self, per field
fn apply(self, f: impl FnOnce(El) -> El) -> Self;  // SwiftUI ViewModifier, 3 lines
```

`.styled(&CARD)` today is `*self.style_mut() = s.clone()` — it clobbers
everything set before it, so presets are unusable mid-chain. StyleX's only real
rule is *last write wins, per field*; `Style` is already a POD patch, so a
`set: u16` bitmask on it (01-stylex §1) makes `.preset()` and a `.base()` sibling
a 25-line change. `Styled::when` already exists and is half of a cva recipe
(04 §1); `.apply` is the other half.

This ranks first because it costs almost nothing, needs no new vocabulary, and
every other item below composes through it.

### 2. Semantic widget vocabulary, daisyUI-style — **shipped**

`Variant`, `Control` (`.variant .role .size .px .el`), `Theme.control`,
`knob`'s `f64 size` deleted, faces from palette math only, and `Sugar::join`.


`Variant { Solid, Soft, Outline, Ghost }` and one size scale reusing the existing
`Xs S M L Xl`, resolved from `role` + the palette math already in `Palette::on`
/ `mix` / `hover`. No lookup tables (02-daisyui §1, §2):

```rust
button(ui, "save", "Save").variant(Variant::Soft).size(S)
knob(ui, "cut", "Cutoff", &mut v, 0.0..=1.0)   // f64 `size` arg deleted,
                                               // derived from Theme.control
```

`knob`'s raw `f64 size` parameter is the tell: every caller invents a number.
One `Theme.control` field (daisyUI's `--size-field`) rescales every control at
once. ~15 lines in `widgets.rs` + 1 theme field. Keep `.px(f64)` as the hatch.

Also take `.join()` — outer corners round, inner square — because
`SegmentedRow` in `scenes.rs` builds it by hand today, and the container already
knows its own axis.

### 3. Shorter names and token sugar — **shipped**, except `.behind` / `.over`

`Spacing::Step`, `Role::alpha`, `.on(State, ..)` and `.full()` all landed.
`.behind(el)` / `.over(el)` did not: `stack![a, b]` already orders two
children, and neither the gallery nor a widget wanted a second spelling.


Four independent one-liners, in value order:

- `Spacing::Step(f64)` over a `Theme.unit: 4.0` (03-tailwind §1). The five-value
  scale forces raw pixels for anything off it — the README's own `.pad(22.0)`
  proves it. One enum variant.
- `Role::alpha(f)` → `.stroke(Ink.alpha(0.12))` (03 §2). Today a hairline needs a
  literal `Color::oklcha`, which abandons the palette and breaks in light mode.
  `Fill::map` is already the machinery.
- `.on(State::Hover, |s| s.fill(...))` (03 §4). The one genuinely *missing*
  capability: a node cannot declare its hover look beside its resting look, so
  scenes juggle state imperatively via `ui.state(id)`. Closure form, not a
  string prefix — grouping (`hover:(a b)`) exists only because strings repeat.
- `.full()` = `.w(pct(100.)).h(pct(100.))`, `.behind(el)` / `.over(el)` (05 §2).

### 4. A shorthand string — **refused, and written down**

The ban is in README.md's opening paragraph and in ARCHITECTURE.md's
invariants. `sty!` stays unbuilt.


`.class("btn btn-sm p-4")` moves every error from compile time to runtime and
buys nothing: every MUI value is already a Rust expression, and there is no
cascade to defeat. Four of six research docs independently reject it.

A `sty!` proc macro (compile-time, expands to a struct literal, errors with
spans) is defensible *only if* hot-reloadable host skins become a goal — at
which point the parser is shared with a runtime loader and the cost is honest.
Not now. Write the ban down.

### Side by side: `mui-preview/src/scenes.rs::editor()`

Today — 46 lines:

```rust
let tabs = row(["Osc", "Filter", "Env"].map(|n| {
    row([caption(n)]).justify(Justify::Center)
        .w(clamp(64.0, 18.0, 120.0)).pad_xy(0.0, 8.0)
        .radius(8.0).fill(Role::Field).id(format!("tab-{n}"))
})).gap(S).wrap().id("tabs");
let knob = |i: usize| column([
    leaf(40.0, 40.0).pill().fill(Role::Primary).id(format!("k{i}")),
    caption(["cut", "res", "drv", "mix"][i]),
]).gap(Xs).align(Align::Center).pad(S).radius(10.0).fill(Role::Raised);
let chips = row(["A", "B", "C", "D"].map(|n| {
    row([caption(n)]).pad_xy(10.0, 4.0).pill()
        .fill(Role::Field).id(format!("chip-{n}"))
})).gap(S).wrap().id("chips");
column([
    row([title("Kurv"), spacer(), caption("v1.0")]).baseline().id("head"),
    tabs,
    grid(4, (0..4).map(knob)).gap(S).min_col(120.0).id("bank"),
    chips,
]).gap(M).pad(M).w(pct(100.0)).h(pct(100.0))
  .radius(16.0).fill(Role::Surface).clip().id("editor")
```

Proposed — 16 lines, with `chip`, `tile` and `panel()` from `mui::presets`:

```rust
let tab  = |n: &str| chip(n).w(clamp(64., 18., 120.)).radius(Corner::Field)
                            .on(Hover, |s| s.fill(Field.lift(1)))
                            .id(format!("tab-{n}"));
let knob = |i: usize| tile(col![
    leaf(40).pill().fill(Primary).id(format!("k{i}")),
    caption(["cut", "res", "drv", "mix"][i]),
]);
col![
    row![title("Kurv"), spacer(), caption("v1.0")].baseline().id("head"),
    row(["Osc", "Filter", "Env"].map(tab)).gap(S).wrap().id("tabs"),
    grid(4, (0..4).map(knob)).gap(S).min_col(120.).id("bank"),
    row(["A", "B", "C", "D"].map(chip)).gap(S).wrap().id("chips"),
]
.gap(M).pad(M).full().preset(&panel()).clip().id("editor")
```

What did the work: `chip`/`tile` are three-line preset *functions* (not a style
system), `.preset()` merges instead of clobbering, `.full()` and `Corner::Field`
delete the numbers, `.on(Hover, ..)` puts a state where the resting style is.
The tree is unchanged — same ids, same reflow test.

### Rejected outright, from all six docs

Atomic classes and hashing (no cascade to defeat), the compiler/extractor
(no class-name problem to solve), stringly keys (`$gtMd`, `'blue.500'`),
stored `*-content` colours (`Palette::on` computes contrast at resolve time),
viewport breakpoints and `dark:` (roles already handle both), `@apply`,
attributify, N-way compound variants, `GeometryReader`-style
layout→build→layout re-entrancy, and SwiftUI's `ModifiedContent<A, B>` type
onion — it would kill `Vec<El>` and `dyn`.

---

## Part 2 — Paint and effects

Every row of the table below shipped except #8, which was refused as planned.


Ranked by capability per line. Vello feasibility is from the vendored sources
(`vello_common 0.2`, `vello_cpu`, `vello_hybrid`), not from the docs.

| # | Feature | Vello | Type change | Lines |
|---|---|---|---|---|
| 1 | Radial + conic gradients | **yes, already** | `Gradient { kind, stops }`, 2 `Paint` variants | ~80 |
| 2 | `Vec<Shadow>` + `inset` | **yes, analytic** | `Style.shadow: Vec<Shadow>`, `Shadow.inset: bool` | ~70 |
| 3 | Squircle / superellipse corners | yes (geometry) | `CornerStyle` field in `mui-geometry::fillet` | ~60 |
| 4 | Glass preset (faked) | n/a — it is 1+2 | a `presets::glass()` function | ~15 |
| 5 | `.cut(el)` / `.keep(el)` | yes, in-tree | expose `boolean()`'s other two ops | ~90 |
| 6 | `Pin` replacing `.offset` | not involved | `Pin { anchor, area, fallbacks }` in layout | ~140 |
| 7 | `Len::Container(pct)` | not involved | one `Len` variant + resolve rule | ~40 |
| 8 | Host shader layer | **no** | — | skip |

| # | shipped as |
|---|---|
| 1 | `GradientKind::{Linear, Radial, Conic}`, one `Paint::Gradient` (not two variants), `mui_vello::brush` |
| 2 | `Style.shadow: Vec<Shadow>`, `ShadowKind::Inset`, `Shadow.spread`, `Paints::{shadow, shadows, elevation}`, `Elevation` |
| 3 | `mui_geometry::CornerStyle { Round, Squircle }`, `Paints::corners`; the old `CornerStyle` struct is now `Fillet` |
| 4 | `mui::presets::glass()` |
| 5 | `Sugar::{cut, keep}`, `Carve`, `Element.carve` |
| 6 | `Pin`, `Area`, `Match`, `Node::pin`; `Ui`'s tooltip rides it |
| 7 | `Len::Container`, `cq()`; plus `fits!` / `Node::fits` from SwiftUI's `ViewThatFits` |
| 8 | not built, and `.mask(fill)` shipped in its place as planned |

**1. Gradients.** The biggest gap between what the renderer can do and what the
DSL can say. `PaintType = peniko::Brush` in `vello_common 0.2`, so both backends
paint conic and radial today; only `Gradient`'s single `angle: f64` and
`Paint::Linear` block it. A conic gradient *is* the knob arc and the ring meter,
with zero paths and zero per-frame geometry — the thing an audio plugin draws
most. `Fill::map` and the spring interpolation already walk stops generically,
so hover tints and transitions come free.

```rust
Gradient::conic(-135., [(0., Primary), (0.7, Primary), (0.7, Field)])
Gradient::radial((0.3, 0.3), 0.6, [(0., Raised), (1., Surface)])
```

**2. Shadows.** `fill_blurred_rounded_rect(rect, radius, std_dev, invert: bool)`
— `invert` *is* `inset`, analytic, no filter layer, no mask, both backends. Two
shadows (tight contact + wide ambient) is the entire difference between "CSS
default" and "designed"; MUI has exactly one, non-inset. Add `Elevation::Raised`
theme presets so nobody hand-tunes four shadows per node. **Caveat:** analytic
only where `Painted.rect` is `Some` — a welded outline needs an offset-outline
gradient fake. Fine at plugin sizes.

**3. Squircles.** CSS `corner-shape` separates corner *shape* from *radius*. One
extra field on `mui-geometry::fillet::CornerStyle` and it flows through welds,
shells and strokes untouched — including the concave joins, which is exactly
where a circular arc looks worst. MUI's `concave` corner has no CSS analogue;
keep it.

**4. Glass.** There is no backdrop blur and there will not be:
`FilterInput::BackgroundImage` is unimplemented, `vello_cpu`'s multi-threaded
dispatch is `unimplemented!()` for filters (`dispatch/multi_threaded.rs:557`),
and `vello_hybrid::Scene::push_mask_layer` always panics. Glass at plugin scale
is a translucent fill, a bright 1px top edge, and a soft inner shadow — all of
which #1 and #2 deliver. Ship the preset, never ship `.blur()`.

**5. Booleans.** `mui-geometry::boolean` already ships `Difference` and
`Intersection`; the DSL exposes only `Union`, via `.weld`. `.cut(el)` gives
clip-path shapes as *outlines* rather than layers, so shells and strokes follow
them. Highest capability-per-line of the geometry items.

**6. Pin.** `.offset(dx, dy)` is documented in `mui-layout` as *"the only
coordinates in the system"* — an apology. CSS anchor positioning's three good
ideas are `position-area` (a named 3×3 region, never numbers), `anchor-size()`,
and `position-try-fallbacks` (an ordered list, first that fits). A plugin window
is small and fixed, so the fallback list is one rect test per candidate.

```rust
menu.pin(Pin::to("field").area(Area::BlockEnd).match_width().try_(&[Area::BlockStart]))
tip.pin(Pin::to("knob").area(Area::Top).gap(Xs))
```

It collapses `.tip()`, `.float()`, dropdown and drag-ghost into one primitive —
but it touches the layout walk, so it is a PR of its own, after the paint work.
Skip `anchor()`'s per-side arithmetic form; that is the part web developers get
wrong.

**7. Container queries.** MUI is 80% there: `.min_col()` is
`repeat(auto-fit, minmax())` and `clamp()` is the fluid part. The missing 20% is
`cqw` — `Len::Container(pct)` resolving against the nearest sized ancestor,
because today `clamp(14., 4., 28.)` reads as 4% of *something unstated*. Take
the unit; **refuse `@container` blocks** — `if w > 480. { row![..] } else
{ col![..] }` beats a query language in a tree rebuilt every frame.

**8. Host shaders.** No. There is no fragment-shader seam both backends share,
Flutter's `FragmentProgram` drags in an asset pipeline with positional uniforms,
and `canvas(|size| ..) -> Vec<Draw>` already covers the cases a plugin has.
Before any user GLSL, ship `.mask(fill)` — one `push_layer` with
`Compose::SrcAtop`, which buys scroll fades, shimmer and gradient-tinted glyphs
for one method.

---

## Part 3 — Skip, and why

- **Runtime style strings** (`.class("btn btn-sm")`, `.style("p-4 bg-primary")`)
  — runtime errors in exchange for nothing. Write the ban into the README.
- **A build-time extractor / compiler** — MUI has no class names and no render
  diff; Tamagui's own compiler bails out on the interesting cases.
- **`backdrop-filter`, `.blur()`, feTurbulence, mesh gradients** — no backend;
  panics under `cpu-threads`, no-ops on GPU.
- **Scroll / view timelines and view-transition snapshots** — `ui.tween` and
  `.id`-matched `.animate()` already cover the one useful case each.
- **`border-image`** — an asset pipeline to avoid describing a shape we can
  describe.
- **`--depth` / `--noise` booleans** — throws away Vello to buy a checkbox.
- **Map-shaped state values** (`{ default, hover }` as data) — per-frame
  allocation; the closure form costs nothing.
- **A `Layout` / `MeasurePolicy` trait, intrinsics, `GeometryReader`** — YAGNI;
  `canvas()` already hands back a resolved size.

### One live bug found on the way — **fixed**

`mui-layout/src/lib.rs:438` — `pad(Spacing::Px(v))` sets `self.padding` without
clearing `self.pad`, and `padding()` resolves token-first, so `.pad(M).pad(12.)`
silently keeps `M`. One-line fix; audit `radius`/`pill` and
`stroke`/`stroke_width` for the same shape (01-stylex §2).

---

## Deferred

Everything in the plan shipped. What was deliberately left, each with the
condition that would justify it:

- `State::Disabled` — nothing in `Ui` reports disabled, and the variant
  without the flag that gates hit testing would be a lie.
- `.behind(el)` / `.over(el)` — `stack![..]` already does it.
- Spring interpolation of gradient *stops* — the channel slots are indexed by
  a fixed layout and no scene springs a ramp. `Fill::map` already walks stops.
- A dependency-ordered pin pass — a pin anchored inside another pinned float
  reads the first pass's position (`ponytail:` in `mui-layout`).
- `CornerStyle::pull` is one kappa derived for a 90-degree turn, so a shallow
  weld junction reads a little full (`ponytail:` in `mui-geometry::fillet`).
- No gallery scene for squircle corners; the geometry and `.corners()` have
  tests, not pixels.
- `Elevation` is not a `Theme` field: the steps are black at an alpha and
  nothing in `Theme` would change them.

## Plan — three PRs

**PR 1 — "presets merge instead of clobbering" (DSL, no renderer work).**
`set: u16` on `Style` + `Style::over`; `.preset(&s)` / `.base(&s)` replacing
`.styled`; `.apply(f)`; `.on(State, f)`; `.full()`; `Spacing::Step` over
`Theme.unit`; `Role::alpha`; fix the `pad` bug. A `mui::presets` module with
`panel`, `chip`, `tile`, `card`. Rewrite `editor()` and one widget against it;
the existing reflow test is the check. ~250 lines, mostly deletions downstream.

**PR 2 — "paint the renderer already does".**
`Gradient { kind, stops }` with conic and radial, `Paint` variants, both vello
backends wired; `Style.shadow: Vec<Shadow>` with `inset`, `Elevation` presets,
welded-outline fallback; `presets::glass()`. A preview scene with a conic knob
and a two-shadow key, and a snapshot test per backend. ~200 lines.

**PR 3 — "semantic widgets and squircle corners".**
`Variant` + size scale on `button`/`knob`/`toggle`/`slider`, `Theme.control`,
`knob`'s `f64 size` deleted; `.join()` on a row/col; `CornerStyle::Squircle` in
`mui-geometry::fillet` and `.radius(Corner::Field)` reading `Theme.corners`.
~180 lines, and it deletes the hand-copied button trees.

`Pin`, `.cut`/`.keep`, `.mask` and `Len::Container` queue behind these; each is
its own PR and none blocks the three above.

---

## Sources

Per-document sources are listed in `01-stylex.md` … `06-geometry-and-effects.md`.
The load-bearing ones:

- StyleX — <https://stylexjs.com/docs/learn/>
- daisyUI 5 — <https://daisyui.com/docs/>
- Tailwind CSS v4 theme variables — <https://tailwindcss.com/docs/theme>
- UnoCSS rules and variants — <https://unocss.dev/config/rules>
- Panda CSS recipes / `sva` — <https://panda-css.com/docs/concepts/recipes>
- vanilla-extract sprinkles — <https://vanilla-extract.style/documentation/packages/sprinkles/>
- Tamagui variants — <https://tamagui.dev/docs/core/variants>
- SwiftUI `ViewThatFits` — <https://developer.apple.com/documentation/swiftui/viewthatfits>
- Compose `drawBehind` — <https://developer.android.com/develop/ui/compose/graphics/draw/overview>
- CSS `corner-shape` — <https://developer.mozilla.org/en-US/docs/Web/CSS/corner-shape>
- CSS anchor positioning — <https://developer.mozilla.org/en-US/docs/Web/CSS/CSS_anchor_positioning>
- Vello — <https://github.com/linebender/vello>
