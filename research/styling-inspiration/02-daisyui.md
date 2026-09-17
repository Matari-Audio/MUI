# daisyUI — the vocabulary, not the CSS

daisyUI is a Tailwind plugin that adds ~65 *component* class names on top of
Tailwind's utilities. It ships no JavaScript and no components in any
framework sense: it is a dictionary. `btn` is a noun, `btn-primary` is the
same noun with a colour role, `btn-sm` with a size, `btn-ghost` with a style,
`btn-active` with a state. Everything else is Tailwind.

Why it matters to MUI: MUI already has the hard parts daisyUI fakes with CSS
(resolved palette, spring transitions, a real scene graph). What MUI does not
have is daisyUI's *naming discipline* — the claim that a readable UI line
names a thing, then a role, then a size, then a state, in that order, and
nothing else.

## What the system actually does

Six categories, and daisyUI's own docs name them
([SKILL.md](https://daisyui.com/SKILL.md)):

| category | example | meaning |
|---|---|---|
| **component** | `btn`, `card`, `join`, `menu`, `tabs` | the noun. Required, always first. |
| **part** | `card-body`, `card-title`, `join-item`, `menu-title` | a named child slot of that noun |
| **style** | `btn-outline`, `btn-soft`, `btn-ghost`, `btn-dash`, `btn-link` | how solid it looks |
| **color** | `btn-primary`, `alert-error`, `badge-success` | semantic role, never a hue |
| **size** | `btn-xs … btn-xl` | one scale, five stops, same five words everywhere |
| **placement/direction** | `join-vertical`, `tooltip-top`, `dropdown-end`, `chat-start` | where it sits or which way it runs |
| **behavior/state** | `btn-active`, `btn-disabled`, `tab-active`, `menu-focus`, `collapse-open` | a state you set, not one the browser owns |

The rule that makes it readable: **a modifier is always `<component>-<word>`**.
There is no orphan `.primary` or `.sm` floating in the namespace. You can read
`btn-sm` and know it sizes a button, not a card, without knowing CSS
specificity.

The colour vocabulary is eleven roles, each with a paired `-content`:

```
base-100  base-200  base-300  base-content
primary   secondary  accent   neutral
info      success    warning  error
```

`base-100/200/300` is a three-step elevation ramp (100 is the page, 300 is the
deepest chrome) and `base-content` is the ink that reads on all three. Every
brand and status role has a `*-content` for the ink on top of it. A theme is
exactly these as CSS variables plus three radii, two sizes, a border width and
two effect flags — thirty-odd numbers, nothing else.

## The syntax, six real examples

A button, four ways ([button](https://daisyui.com/components/button)):

```html
<button class="btn">Neutral-ish</button>
<button class="btn btn-primary btn-sm">Small primary</button>
<button class="btn btn-outline btn-error">Delete</button>
<button class="btn btn-ghost btn-circle btn-lg">✕</button>
```

A card, showing *parts* ([card](https://daisyui.com/components/card)):

```html
<div class="card bg-base-100 w-96 shadow-sm">
  <div class="card-body">
    <h2 class="card-title">Filter</h2>
    <p>Two poles, one cutoff.</p>
    <div class="card-actions justify-end">
      <button class="btn btn-primary">Apply</button>
    </div>
  </div>
</div>
```

`join`, which is nothing but a container that fixes the corners of its
children ([join](https://daisyui.com/components/join)):

```html
<div class="join">
  <button class="join-item btn">1</button>
  <button class="join-item btn btn-active">2</button>
  <button class="join-item btn">3</button>
</div>
<div class="join join-vertical">…</div>
```

Note what `join` does *not* do: it does not know about buttons. Anything with
`join-item` gets its outer corners rounded and inner ones squared. Input +
button + select in one pill, no special case.

Semantic colours used as bare Tailwind utilities — this is the composition
seam ([utilities](https://daisyui.com/docs/utilities)):

```html
<div class="bg-base-200 text-base-content p-4 rounded-box">
  <span class="text-success">ok</span>
  <span class="badge badge-soft badge-warning">clipping</span>
</div>
```

A whole theme ([themes](https://daisyui.com/docs/themes)):

```css
@plugin "daisyui/theme" {
  name: "studio";
  color-scheme: dark;
  --color-base-100: oklch(22% 0.01 260);
  --color-base-200: oklch(18% 0.01 260);
  --color-base-300: oklch(14% 0.01 260);
  --color-base-content: oklch(92% 0.01 260);
  --color-primary: oklch(62% 0.19 260);
  --color-primary-content: oklch(98% 0.01 260);
  --color-error: oklch(60% 0.22 25);
  --radius-selector: 1rem;   /* checkbox, toggle, badge */
  --radius-field: 0.25rem;   /* button, input, select, tab */
  --radius-box: 0.5rem;      /* card, modal, alert */
  --size-field: 0.25rem;     /* the unit btn-xs…btn-xl multiply */
  --border: 1px;
  --depth: 1;                /* 0 or 1 */
  --noise: 0;                /* 0 or 1 */
}
```

That last block is the most transferable artefact in the library. Three radii
named by *what kind of thing* they round, not by pixel size. One `--size-field`
unit that every `-xs…-xl` derives from, so a theme scales all controls with
one number. And `--depth`/`--noise` as booleans — a theme picks a look, it does
not hand-tune a shadow.

---

## Worth stealing, concretely

### 1. Split `Role` into a layer ramp and a content pair

MUI's `Role` today mixes elevation (`Background`, `Surface`, `Raised`, `Field`,
`Level(n)`) with semantics (`Primary`, `Success`, …) with ink (`Ink`, `Dim`).
daisyUI's split is cleaner to *read* at the call site, and MUI already has the
better mechanism: `Palette::on(bg)` computes readable ink automatically, which
is what daisyUI hand-authors as twenty `*-content` variables.

So: keep `on()`, but add the daisyUI word so a tree can say it out loud.

```rust
.fill(Role::Base(2))          // alias for Level(2); Base(0) == Background
.fill(Role::Content)          // == today's Ink, but names the pairing
.fill(Role::ContentOn(Primary)) // explicit, resolves through Palette::on
.fill(Role::Dim)              // keep, daisyUI has no word for this and needs one
```

`Base(n)` beats `Surface`/`Raised`/`Field` as the *primitive* because the ramp
is a number; keep `Surface`, `Raised`, `Field` as `const` aliases so the
readable name survives. And rename `Danger` → keep it. daisyUI says `error`,
MUI says `Danger`; `Danger` is the better word (it covers destructive actions,
not just failures) — do not churn it for conformity.

Missing from MUI and worth adding, because audio UIs need it: `Role::Info`
(neutral-informational, distinct from `Primary` which means *the live control*)
and `Role::Accent` is **not** worth adding — MUI's `Secondary`/`Tertiary`
already cover the second and third axis, and `accent` vs `secondary` is the
single most confusing pair in daisyUI's palette.

### 2. One size scale, five words, on every widget

daisyUI's real win: `xs sm md lg xl` means the same thing on a button, a badge,
a tab, a toggle and a table. MUI already has `Xs S M L Xl` for *spacing*. Use
the identical enum for control size:

```rust
button(ui, "apply", "Apply").size(Sz::Sm)
knob(ui, "drive", "Drive", &mut v, 0.0..=1.0).size(Sz::Lg)
toggle(ui, "bypass", &mut on).size(Sz::Xs)
```

Today `knob` takes a raw `size: f64` and `button` has no size at all. Replace
the `f64` with `Sz`, derive the pixels from one theme field —
`Theme { control: 4.0 }`, daisyUI's `--size-field` — so `Sz::Md` is
`control * 10.0` and a plugin scales every control by editing one number.
Keep a `.px(f64)` escape hatch; a knob that must match a hardware panel needs
the literal.

### 3. Style modifiers as a `Variant`, not as five hand-built trees

`btn-outline / btn-soft / btn-ghost / btn-dash / btn-link` is a four-value
enum, and it is exactly the set an audio plugin needs: a solid primary action,
a quiet outlined secondary, a barely-there ghost in a toolbar, a link.

```rust
pub enum Variant { Solid, Soft, Outline, Ghost }

button(ui, "bypass", "Bypass").variant(Ghost)
button(ui, "clear",  "Clear").variant(Outline).role(Role::Danger)
```

Resolution rules, all of which MUI's palette can already compute — no tables:

| variant | fill | stroke | ink |
|---|---|---|---|
| `Solid` | `role` | none | `palette.on(role)` |
| `Soft` | `role.mix(background, 0.85)` | none | `role` |
| `Outline` | none | `role` | `role` |
| `Ghost` | none | none | `Content`, gains `palette.hover()` fill on hover |

That is ~15 lines in `widgets.rs` and it kills the four copy-pasted button
trees every plugin writes.

### 4. `join` as a container method, and MUI already half has it

MUI's `.weld(fill)` unions children into one filleted shape. daisyUI's `join`
is the *segmented* version: children keep their gaps-of-zero, outer corners
round, inner corners square. For a plugin's mode switch (`LP | BP | HP`) that
is the shape you want, and MUI's `SegmentedRow` preview scene is exactly this
built by hand.

```rust
row![btn("lp"), btn("bp"), btn("hp")].join()   // corners from theme.corners.convex
col![a, b].join()                              // vertical, same method, axis from the container
```

One method, no `join-vertical` needed — the container already knows its axis.
This is a case where Rust's builder beats the class string outright, and it is
worth writing down as the house rule: **daisyUI needs a word for anything CSS
cannot infer; MUI should only add a word for what the tree cannot infer.**

### 5. Radii named by the kind of thing, not by size

`--radius-selector / --radius-field / --radius-box` is better than MUI's
current `CornerProfile { convex, concave }` for authoring, and they compose:
keep `convex`/`concave` as the *geometry*, add the three semantic radii as the
*theme surface*.

```rust
pub struct Corners {
    pub selector: f64,  // toggles, badges, knob caps  — pill by default
    pub field:    f64,  // buttons, inputs, tabs
    pub box_:     f64,  // panels, cards, dialogs
    pub concave:  f64,  // MUI's own; daisyUI has no concept of it
}
```

Then `.radius(Corner::Field)` in a tree, and a plugin goes from rounded to
brutalist by setting three numbers. MUI's `concave` has no daisyUI analogue and
is one of MUI's genuine differentiators — do not let a daisyUI-shaped theme
struct drop it.

### 6. Named parts, but only where a widget really has slots

`card-title / card-body / card-actions` is worth copying as **`.id()`
conventions**, not as new types. MUI's `.id("name")` already exists and already
means "this node is addressable". Standardise the strings a widget emits —
`"apply"`, `"apply.label"`, `"apply.track"` — so a host can theme or automate a
sub-part without the widget growing a config struct. daisyUI's parts are CSS
hooks; MUI's would be scene-graph hooks, same purpose.

---

## What not to copy

- **`-content` colours as stored theme fields.** daisyUI ships twenty colour
  variables because CSS cannot compute contrast. MUI's `Palette::on(bg)` and
  `readable_on(bg, AA_TEXT)` do it correctly at resolve time, including the AA
  floor. Adopting `primary_content: Pigment` would be a strictly worse system
  that also lets a theme author ship an illegible pair.
- **`base-100 / 200 / 300` as exactly three steps.** MUI's `Level(i32)` is
  open-ended and signed — `Field` is `layer(-1)`, a *recess*, which daisyUI
  cannot express at all. Three fixed steps is a CSS-variable-count compromise,
  not a design insight.
- **`accent` alongside `secondary`.** Two words for "a second emphasis colour"
  with no rule distinguishing them. MUI's `Secondary`/`Tertiary` at least
  implies an order.
- **The component catalogue.** 65 components including `mockup-phone`,
  `hover-gallery`, `aura-rainbow` and `mask-heart`. An audio plugin needs maybe
  twelve widgets. Steal the *grammar*; the catalogue is a landing-page business.
- **`--depth` and `--noise` as booleans.** A tempting shortcut, but MUI paints
  with Vello and has real shadows, gradients and blur layers. Reducing that to
  `depth: 0|1` throws away the one thing MUI's renderer is for. If a preset is
  wanted, make it a `Shadow` preset constant, not a flag that changes geometry.
- **Prefixed everything (`btn-`, `card-`, `badge-`).** That prefix exists to
  avoid CSS namespace collisions. Rust has modules and methods; `.size(Sz::Sm)`
  on a button is already unambiguous. Do not invent `.btn_size()`.
- **Style strings.** No `.class("btn btn-primary btn-sm")` shorthand. It is
  tempting and it would parse, but it moves errors from compile time to run
  time for zero gain — the whole reason MUI has no runtime CSS.

## The one-line readability test

daisyUI's real claim is that this reads in one pass:

```html
<button class="btn btn-soft btn-error btn-sm">Reset</button>
```

noun, style, role, size. The MUI equivalent should read the same way and does,
once 2/3 land:

```rust
button(ui, "reset", "Reset").variant(Soft).role(Danger).size(Sz::Sm)
```

If a widget call needs more than noun + four modifiers, the widget is wrong,
not the caller.

## Sources

- https://daisyui.com/SKILL.md — the category taxonomy (component, part, style, behavior, color, size, placement, modifier)
- https://daisyui.com/llms.txt — every component with its full modifier set
- https://daisyui.com/docs/themes — theme plugin, the full CSS-variable list
- https://daisyui.com/docs/colors — semantic colour names and the `-content` pairing
- https://daisyui.com/docs/utilities — `rounded-box`, `bg-base-*`, composition with Tailwind
- https://daisyui.com/components/button — colour/style/behavior/size/shape modifier grid
- https://daisyui.com/components/join — `join`, `join-item`, `join-vertical`
- https://daisyui.com/components/card — part classes (`card-body`, `card-title`, `card-actions`)
- https://daisyui.com/docs/v5 — v5 rename to explicit variable names, `--depth`/`--noise`
