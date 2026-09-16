# Tailwind CSS v4 and UnoCSS

What to steal from the two utility-class engines, for MUI's builder DSL and for
a possible compact shorthand string.

## What the systems do

**Tailwind v4** is a build-time scanner. It reads your source as plain text,
finds every substring that looks like a class name, and emits exactly the CSS
those names need. There is no runtime, no CSS-in-JS, no style object. The class
name *is* the source of truth, and the grammar is deliberately tiny:

    <utility>-<token>[/<modifier>]      p-4, gap-2, rounded-lg, bg-red-500/50
    <variant>:<utility>                 hover:bg-red-600, md:p-8, dark:text-white
    <utility>-[<arbitrary>]             top-[117px], bg-[#bada55], text-[22px]

v4's headline change is that the theme is CSS custom properties, not a JS
config. `@theme` declares tokens, and declaring a token *creates* the utilities:

```css
@theme {
  --spacing: 0.25rem;          /* p-4 = 1rem, mt-7 = 1.75rem, ... */
  --radius-5xl: 3rem;          /* creates rounded-5xl */
  --text-tiny: 0.625rem;       /* creates text-tiny */
  --color-brand: oklch(0.7 0.16 250);  /* creates bg-brand, text-brand, ... */
}
```

That inversion — *a token is a generator, not a table entry* — is the one
architectural idea in v4 worth more than all its syntax.

**UnoCSS** is the same idea with the grammar made user-definable. Rules are
regex-to-CSS pairs; presets bundle rules; transformers rewrite the source before
matching, which is how it gets syntax Tailwind can't have:

```ts
rules: [[/^m-([.\d]+)$/, ([, n]) => ({ margin: `${n}px` })]]
shortcuts: {
  btn: 'py-2 px-4 font-semibold rounded-lg shadow-md',
}
// dynamic shortcut
shortcuts: [[/^btn-(.*)$/, ([, c]) => `bg-${c}-400 text-${c}-100 py-2 px-4 rounded-lg`]]
```

```html
<!-- variant-group transformer -->
<div class="hover:(bg-gray-400 font-medium) font-(light mono)" />
<!-- expands to: hover:bg-gray-400 hover:font-medium font-light font-mono -->

<!-- attributify preset -->
<button bg="blue-400 hover:blue-500" text="sm white" p="y-2 x-4" border="~ rounded blue-200">
```

## The token scales, as numbers

Two of these are worth adopting wholesale, one is worth rejecting.

**Spacing — one multiplier, infinite scale.** v4 has no spacing table. It has
`--spacing: 0.25rem` and `p-<n>` means `n * --spacing`. Every integer works,
`p-1.5` works, and there is nothing to memorise beyond "4px per step".

MUI's `SpacingScale { xs: 4, s: 8, m: 12, l: 18, xl: 28 }` (crates/mui-layout/src/lib.rs:98)
is five hand-tuned values. It is *not* a linear multiple — 18 and 28 are chosen,
not derived — which is defensible for a plugin editor where vertical room is
scarce. But it means `.pad(22.0)` in the README example is a raw pixel, off-scale
and invisible to a theme swap. Five tokens is too few to cover real layouts, so
people escape to pixels, and the escape hatch is the common case.

**Radius (v4 default):** xs 2, sm 4, md 6, lg 8, xl 12, 2xl 16, 3xl 24, 4xl 32,
`rounded-full` = `calc(infinity * 1px)`. Roughly geometric, ~1.5x per step.
MUI's `CornerProfile::DEFAULT` is a single pair (convex 18, concave 14) plus
`Radius::Scale(f64)` and `Radius::Pill` — already the right shape, just with no
named steps on it.

**Shadow (v4 default):** 2xs, xs, sm, md, lg, xl, 2xl. Each is one or two layers
of `0 Ypx Bpx -Spx rgb(0 0 0 / A)`; the y-offset and blur climb together
(md = 4/6, lg = 10/15, xl = 20/25) and alpha stays ~0.1 until 2xl (0.25). MUI's
`Shadow::soft(blur)` already encodes the relationship as `dy = blur / 2`, which
is the same curve with one knob instead of four. Good.

**Font size (v4 default):** xs 12, sm 14, base 16, lg 18, xl 20, 2xl 24, 3xl 30,
then 36/48/60/72/96/128. Each carries a paired line-height, overridable inline
as `text-sm/6`. MUI has `Theme::text: 14.0` and three hardcoded helpers —
`title` 18, `label` 13, `caption` 11 (crates/mui-core/src/dsl.rs, already flagged
with a `ponytail:` comment). A theme with one text size and three magic numbers
in the DSL is the gap Tailwind's `--text-*` namespace fills.

**Breakpoints (sm/md/lg/xl/2xl at 640/768/1024/1280/1536px):** irrelevant. MUI
paints a plugin editor at a host-given scale. See "what not to copy".

## Ideas worth stealing

### 1. A spacing multiplier under the named tokens

Keep `Xs S M L Xl` as the vocabulary designers use, but define them on top of a
base unit so off-scale values stay on-grid and a theme swap moves everything:

```rust
// mui-layout
pub struct SpacingScale { pub unit: f64, pub steps: [f64; 5] }  // unit 4.0, steps [1., 2., 3., 4.5, 7.]

// and in Spacing, one new variant
Spacing::Step(f64)   // Step(5.5) == 22px at unit 4
```

Then the README's `.pad(22.0)` becomes `.pad(step(5.5))` and re-tunes with the
theme. `Spacing::Px` stays for the genuinely absolute case (a 1px hairline).
This is the highest-value item on the list and costs one enum variant.

### 2. The `/alpha` modifier, as a method

`bg-primary/50` is Tailwind's single best syntactic invention: one token, one
knob, no second colour in the palette, no `rgba()` spelled out. In MUI it is a
method, not a string:

```rust
.fill(Primary.alpha(0.5))          // Role  -> Fill with alpha applied post-resolve
.stroke(Ink.alpha(0.12))           // the hairline every panel edge wants
.shadow(Shadow::soft(12.0).alpha(0.4))
```

`Role::alpha` returns a `Fill::Role`-plus-alpha (or reuses the existing
`Fill::map` machinery in crates/mui-core/src/style.rs, which already pushes every
resolved colour through a closure). Today a 12% ink hairline means constructing a
literal `Color::oklcha`, which defeats the palette and breaks in light mode.

### 3. A named step scale for radius, shadow and text — generated, not tabled

Follow v4: the token *creates* the step. One geometric ratio per namespace,
`Theme` carrying the base and the ratio, steps as `i8`:

```rust
Radius::Step(-1)   // 0.66x theme radius        rounded-sm
Radius::Step(0)    // theme radius (default)    rounded-lg
Radius::Step(2)    // 2.25x                     rounded-2xl
Shadow::step(2)    // blur = base * ratio^2, dy = blur/2, alpha from the curve
Text::step(1)      // 1.15x theme text -> replaces title/label/caption
```

`Radius::Scale(f64)` already exists and is the continuous version; `Step` is the
discrete one that keeps a tree consistent. Deleting `title`/`label`/`caption`'s
hardcoded 18/13/11 in favour of `text_step(1)/(-1)/(-2)` closes the
`ponytail:` note in dsl.rs and makes type size theme-driven for free.

### 4. Shortcuts are functions; variant groups are closures

UnoCSS shortcuts and Tailwind `@apply` both exist to give a name to a bundle of
utilities, because HTML has no way to call a function. Rust does:

```rust
fn btn(label: &str) -> El {
    text(label).pad(S).radius(Radius::Step(1)).fill(Primary).cursor(Cursor::Hand)
}
```

That is `@apply`, done, with types and no registry. Same for variant groups:
`hover:(bg-red text-white)` exists because `hover:bg-red hover:text-white`
repeats a prefix in a flat string. A closure has no prefix to repeat:

```rust
.on(State::Hover, |s| s.fill(Primary.lighten(0.06)).stroke(Ink.alpha(0.2)))
.on(State::Press, |s| s.shadow(Shadow::step(0)))
```

This is the one *new* MUI capability the research turns up: states today are
imperative, reached through `Ui` and springs, so a node cannot declare its own
hover look next to its resting look. A `.on(state, closure)` builder that stores
`Vec<(State, fn(Style) -> Style)>` and is applied during `resolve_scene`, with
the existing `.animate()`/`Spring` doing the interpolation, would remove most of
the per-frame state juggling in mui-preview/src/scenes.rs.

### 5. A narrow `sty!` proc macro — only if hot-reload is a goal

If MUI wants skins editable without a recompile, then and only then is a string
grammar worth writing. Keep it to the subset that is pure style, no layout
structure, no variants, no breakpoints:

```rust
let panel = leaf(520., 230.).sty(sty!("p-3 gap-2 rounded-2 fill-surface stroke-ink/12 shadow-1"));
let chip  = leaf(28., 28.).sty(sty!("p-1.5 rounded-full fill-primary/80"));
```

Design rules that fall out of MUI's situation, not CSS's:

- **No `[...]` brackets.** They exist because a CSS class name cannot contain a
  `#` or a space. A Rust string can. Write `fill-#bada55` and `p-13.5` directly.
- **Bare numbers are steps, not pixels.** `p-3` = 3 units = 12px; `p-13px` is the
  explicit escape. Inverting Tailwind's default (where `p-[13px]` is the ugly one)
  because MUI's audience reaches for pixels too readily.
- **One parser, two call sites.** `mui-style/src/parse.rs` exposes
  `fn style(s: &str) -> Result<Style, ParseError>`; the proc macro calls it at
  compile time and emits a const `Style`, the runtime skin loader calls it on
  file load. Do not write the grammar twice.
- **Compile-time errors with real spans.** The macro's whole justification over
  `Style::parse("...")` is that `sty!("rouned-2")` fails to build with a caret
  under the typo. If that is not delivered, skip the macro and ship the runtime
  parser alone.
- **~20 rules, no more:** `p- m- gap- w- h- min-w- max-w- rounded- fill- stroke-
  shadow- text- grow- shrink- items- justify-`. Anything past that is the builder's
  job, and the builder is better at it.

Honest caveat: MUI's builder is already terse — `.gap(M).pad(22.).center()` is
not meaningfully longer than `"gap-m p-5.5 center"` and it autocompletes. The
macro buys hot-reload and skin files. If neither is on the roadmap, item 5 is
speculative and the first four are the whole take.

## What not to copy

**Breakpoint variants (`md:`, `lg:`).** Viewport breakpoints assume a document
reflowing in a browser window. A plugin editor is a fixed-aspect canvas the host
scales; MUI already has the right primitives — `Len::Pct`, `clamp(min, pct, max)`,
`.min_col(120.)` (auto-fit/minmax), `.wrap()`. Those are container queries, which
is the correct model. Adding viewport breakpoints would invite trees that only
look right at one window size.

**Attributify mode.** It solves "the `class` attribute is 400 characters long" by
spreading it across HTML attributes. Rust has no attributes on values and a
builder chain already groups by property. Nothing to port.

**The full variant/modifier stack.** `dark:hover:focus-within:md:*` composition is
where Tailwind's mental model gets expensive. MUI's palette already resolves
light/dark through roles and `Palette::on(under)` — there is no `dark:` to write,
which is strictly better. Keep it that way; add `hover`/`press`/`focus`/`disabled`
and stop.

**Arbitrary-property syntax (`[mask-type:luminance]`).** An escape hatch into raw
CSS. MUI has no CSS to escape into, and `canvas(|size| ...)` is already the
typed escape hatch for anything the style struct cannot say.

**Regex-based dynamic rules (UnoCSS's core).** Great for a system whose users
extend the language at runtime. A Rust library wants the opposite: a closed set of
utilities checked by the compiler. A user-extensible rule table would push errors
from build time to paint time — exactly the trade MUI made by having no runtime CSS.

**Scanning source text for class names.** Tailwind's scanner is why `bg-${color}`
silently fails and why safelists exist. A proc macro sees an AST and a real string
literal; never build a scanner.

**The 22-step colour palette (red-50 .. red-950).** MUI's roles
(`Primary Surface Raised Field Ink Dim Level(n)`) plus `Level(i32)` elevation are
a better model for a themed plugin than 22 frozen hues per colour. `Role::Level`
is already the generative version of what `gray-100..900` hardcodes.

**`@apply` itself.** Tailwind's own docs now steer away from it ("avoid premature
abstraction"), and v4 replaced the component layer with `@utility`. In Rust the
equivalent is a function returning an `El`; no directive needed.

## Sources

- Tailwind theme variables and `@theme`: https://tailwindcss.com/docs/theme
- Spacing multiplier: https://tailwindcss.com/docs/padding and https://tailwindcss.com/docs/max-height
- Radius scale: https://tailwindcss.com/docs/border-radius
- Shadow scale and colour modifiers: https://tailwindcss.com/docs/box-shadow
- Font size scale and `text-sm/6`: https://tailwindcss.com/docs/font-size
- Arbitrary values, `/opacity`, variants: https://tailwindcss.com/docs/adding-custom-styles and https://tailwindcss.com/docs/styling-with-utility-classes
- `@apply`, `@utility`, v4 migration: https://tailwindcss.com/docs/functions-and-directives and https://tailwindcss.com/docs/upgrade-guide
- UnoCSS rules, shortcuts, presets: https://unocss.dev/guide/ and https://unocss.dev/config/shortcuts
- Variant groups: https://unocss.dev/transformers/variant-group
- Attributify: https://unocss.dev/presets/attributify

## MUI files these touch

- `/mnt/Windows11/DEV_PROJECTS/Repos/MUI-research/crates/mui-layout/src/lib.rs` — `SpacingScale` (line 98), `Spacing` (line 137)
- `/mnt/Windows11/DEV_PROJECTS/Repos/MUI-research/crates/mui-core/src/theme.rs` — `Theme`, `CornerProfile`
- `/mnt/Windows11/DEV_PROJECTS/Repos/MUI-research/crates/mui-core/src/style.rs` — `Role`, `Fill::map`, `Radius`, `Shadow::soft`
- `/mnt/Windows11/DEV_PROJECTS/Repos/MUI-research/crates/mui-core/src/dsl.rs` — `title`/`label`/`caption`, existing `ponytail:` note on the type scale
