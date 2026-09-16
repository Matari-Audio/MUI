# Panda CSS, vanilla-extract/sprinkles, Tamagui

What three build-time styling systems figured out about *naming a variation*
instead of re-typing it, and which parts survive the trip to a Rust builder DSL
with no CSS, no cascade and no class names.

## What the systems actually do

**Panda CSS** — write style objects in TS, a build step statically extracts them
into atomic CSS. Three authoring shapes: `css({...})` (one-off), `cva({...})`
(a *recipe*: base + variants + compoundVariants + defaultVariants, returns a
class-name function), `sva({...})` (a *slot* recipe: the same, but every style
block is keyed by a named part — `root`, `control`, `label`). Values are token
references (`gap: '2'`, `bg: 'green.500'`), and any property accepts a
*conditional object* — `padding: { base: '0.5rem', md: '1rem' }` — where the
keys are breakpoints, container sizes (`'@sidebar/sm'`), or raw at-rules.

**vanilla-extract** — same zero-runtime idea, but the unit is a `.css.ts` file
that exports class names. `@vanilla-extract/recipes` is cva with typed style
objects instead of class strings. The interesting part is **sprinkles**:
`defineProperties({ conditions, defaultCondition, properties, shorthands,
responsiveArray })` generates one atomic class per (property, value, condition)
triple *and* a typed function that takes them as props. `responsiveArray` is the
one that matters here: declare the condition order once and every property then
accepts a positional array, `padding: ['small', 'medium', 'large']`.

**Tamagui** — runtime + optimizing compiler for React/React Native.
`styled(View, { variants: { ... } as const })` where a variant value can be a
style object, a `true`/`false` key (boolean variants), a catch-all `':string'` /
`':number'` function, or a **spread variant** `'...size'` that inherits every
key of a token group and receives `(val, { theme, tokens, props, font })`.
Style props live directly on the component (`<Text color="$white" fontSize={20}
hoverStyle={{...}} $sm={{...}} />`), and the compiler partially evaluates the
tree, extracts atomic CSS, evaluates `useMedia`/`useTheme` into media queries
and CSS variables, and flattens `<Circle />` down to a bare `<div>`.

## Exact syntax, six examples

Panda cva — the canonical recipe shape:

```ts
const button = cva({
  base: { display: 'flex', borderRadius: 'md', fontWeight: 'semibold' },
  variants: {
    visual: { solid: { bg: 'blue.600', color: 'white' },
              outline: { borderWidth: '1px', borderColor: 'blue.600' } },
    size:   { sm: { px: '3', py: '1.5', fontSize: 'sm' },
              lg: { px: '5', py: '3',   fontSize: 'lg' } },
  },
  compoundVariants: [{ visual: 'outline', size: 'lg', css: { borderWidth: '2px' } }],
  defaultVariants: { visual: 'solid', size: 'sm' },
})
button({ size: 'lg' })      // -> class string, visual defaults to solid
```

Panda sva — one recipe, several parts, variants cut across all of them:

```ts
const checkbox = sva({
  slots: ['root', 'control', 'label'],
  base: { root: { display: 'flex', gap: '2' },
          control: { borderWidth: '1px', borderRadius: 'sm' },
          label: { marginStart: '2' } },
  variants: {
    size:      { sm: { control: { width: '8' },  label: { fontSize: 'sm' } },
                 md: { control: { width: '10' }, label: { fontSize: 'md' } } },
    isChecked: { true:  { control: { borderColor: 'gray.300' } },
                 false: { control: { borderColor: 'gray.200' } } },
  },
  compoundVariants: [{ size: 'sm', isChecked: true,
                       css: { control: { borderColor: 'green.500' } } }],
  defaultVariants: { size: 'sm', isChecked: false },
})
const classes = checkbox({ size: 'sm', isChecked: true }) // { root, control, label }
```

Panda conditional values — the *property* carries the responsiveness, not the block:

```ts
css({ margin: { base: '10px', md: '20px' },
      fontSize: { base: 'sm', '@sidebar/sm': 'xl' } })
```

sprinkles — conditions declared once, then positional responsive arrays:

```ts
const responsive = defineProperties({
  conditions: { mobile: {}, tablet: { '@media': '(min-width: 768px)' },
                desktop: { '@media': '(min-width: 1024px)' } },
  defaultCondition: 'mobile',
  responsiveArray: ['mobile', 'tablet', 'desktop'],
  properties: { display: ['none','flex','block'], paddingTop: space, paddingLeft: space },
  shorthands: { padding: ['paddingTop','paddingLeft'], paddingX: ['paddingLeft'] },
})
sprinkles({ display: ['none', 'flex'], padding: 'medium' })
```

Tamagui variants — boolean keys, catch-all functions, spread over a token group:

```tsx
const MyButton = styled(View, {
  variants: {
    selectable: { true: { userSelect: 'auto' }, false: { userSelect: 'none' } },
    color: { ':string': (color) => ({ color, borderColor: color }) },
    pad:   { '...size': (val, { tokens }) => ({ padding: tokens.size[val] }) },
    size:  { md: { fontSize: '$sm', $gtMd: { fontSize: '$md' } } },
  } as const,
})
```

Tamagui style props — the same vocabulary inline, with media and pseudo prefixes:

```tsx
<Text color="$white" fontFamily="$body" fontSize={20}
      hoverStyle={{ color: '$colorHover' }} $sm={{ fontSize: 16 }} />
```

## Worth stealing, in MUI terms

### 1. `Recipe` = base `Style` + variants + compounds + defaults

MUI already has the two halves: `Style` is a plain value struct
(`fill/stroke/radius/shadow/shells/weld/cursor`) and `Styled::styled(&Style)`
applies one to an element. A recipe is the missing middle — a named table from
`(axis, value)` to a `Style` patch, resolved once per build. `crates/mui/src/widgets.rs`
currently hardcodes `Role::Primary` and `pad_xy(14.0, 8.0)` inside `button`;
that is a recipe with the variants inlined and no way to reach them.

Concrete: variants as a small enum per axis, the recipe as a `const` table, and
resolution as a fold of `Style` patches:

```rust
#[derive(Clone, Copy, PartialEq)] pub enum Visual { Solid, Outline, Ghost }
#[derive(Clone, Copy, PartialEq)] pub enum Size   { Sm, Md, Lg }

pub struct ButtonSpec { pub visual: Visual, pub size: Size, pub danger: bool }
impl Default for ButtonSpec { /* Solid, Md, false — the defaultVariants */ }

recipe! {
    button(ButtonSpec) {
        base                    => |e| e.pill().animate().fill(Role::Primary),
        visual: Outline         => |e| e.fill(Fill::None).stroke(Role::Primary),
        visual: Ghost           => |e| e.fill(Fill::None),
        size:   Sm              => |e| e.pad_xy(10.0, 6.0).text_size(12.0),
        size:   Lg              => |e| e.pad_xy(18.0, 11.0).text_size(16.0),
        danger: true            => |e| e.fill(Role::Danger),
        // compound: the outline of a danger button needs the stroke, not the fill
        visual: Outline, danger: true => |e| e.stroke(Role::Danger).fill(Fill::None),
    }
}

let (el, hit) = button(ui, "bypass", "Bypass")
    .visual(Outline).size(Lg).danger(bypassed).build();
```

The arms are `fn(El) -> El`, so a recipe arm is *exactly* what MUI already
writes by hand today; the macro only supplies ordering (base, then each axis in
declaration order, then compounds — later wins, Panda's rule) and the defaults.
No new style representation, no cascade, no runtime lookup.

Cheaper variant if the macro feels like too much machinery: a plain struct with
`.when()` (already on `Styled`), shipped as a function:

```rust
pub fn button_style(s: ButtonSpec) -> impl Fn(El) -> El { move |e| e
    .pill().animate().fill(Role::Primary)
    .when(s.visual == Outline, |e| e.fill(Fill::None).stroke(Role::Primary))
    .when(s.size == Lg, |e| e.pad_xy(18.0, 11.0))
    .when(s.danger, |e| e.fill(Role::Danger))
    .when(s.visual == Outline && s.danger, |e| e.stroke(Role::Danger).fill(Fill::None))
}
```

That is the whole idea in twelve lines with zero new concepts. Start there;
promote to a macro only when a third widget repeats the shape. **The value is
not the syntax, it is that a widget's variation space becomes a type that a
plugin author can enumerate, `impl Default`, and pattern-match on.**

### 2. Slot recipes for multi-part widgets — MUI already has slots, they are `id`s

`slider`, `knob` and `toggle` each build a small tree (track, fill, thumb,
label). Panda's `sva` names those parts and lets one variant reach all of them.
MUI's `.id("name")` is the same naming, already load-bearing for gesture
targeting and `scene.surface("tab")`. So a slot recipe is a recipe whose arms
are keyed by id:

```rust
slot_recipe! {
    slider(SliderSpec) { slots: [track, fill, thumb, label],
        base:       track => |e| e.h(6.0).pill().fill(Role::Field),
        base:       thumb => |e| e.square(16).pill().fill(Role::Ink).animate(),
        size: Sm,   thumb => |e| e.square(12),
        disabled: true, fill => |e| e.fill(Role::Dim),
    }
}
```

and, crucially, a host can override *one slot* of a stock widget without
forking the whole function — the thing plugin skinning always needs and that a
monolithic `fn slider(...) -> El` cannot give. Implementation is a
`HashMap<&'static str, Style>` consulted in the tree walk by id, or a
`Vec<(&'static str, fn(El) -> El)>` applied at build. Prefer the latter: no
lookup at paint, and it composes with `.styled()`.

### 3. Typed responsive values, keyed on the *plugin window*, not a phone

sprinkles' `responsiveArray` and Panda's `{ base, md }` are the same trick: the
value, not the block, carries the breakpoints. For an audio plugin the axis is
not a device — it is the editor's own size (a resizable window, a compact vs.
expanded panel) and the *container*, which is what Panda's container queries
(`'@sidebar/sm'`) are for. Plugin editors are almost entirely container-query
problems: a strip that shows knob + label + value at full width and just the
knob at 80px.

MUI already has `clamp(min, pct, max)` for the continuous case. What is missing
is the *discrete* case — a different tree or a different token at a threshold.
Concrete shape, staying in the existing `IntoLen`-style trait trick:

```rust
// declared once on Theme, like Spacing tokens
pub enum Bp { Compact, Normal, Wide }   // thresholds live in Theme

// value-per-breakpoint, resolved during layout when the parent width is known
.pad(resp([Xs, S, M]))                  // positional: sprinkles' responsiveArray
.w(resp([Len::Pct(100.), Len::Px(320.), Len::Px(420.)]))
.text_size(at(Bp::Wide, 16.0).or(13.0)) // named: Panda's { base, md }
```

Two honest caveats. (a) This costs a *second* layout pass or a resolve-time
container width, because MUI resolves a tree to a scene before painting; the
lazy version is to resolve breakpoints once per frame against the **window**
size (one number, known before the walk) and only later extend to per-container.
(b) Positional arrays read badly past three entries — cap the array at the
number of declared breakpoints and let the compiler enforce it with `[T; N]`,
which TS cannot do and Rust can. That is a strict improvement over sprinkles.

### 4. Style-props ergonomics without a `Box`

Chakra/Tamagui's `<Box p={4} bg="primary" />` is already MUI's builder chain —
`.pad(M).fill(Primary)` is the same information with better types. Nothing to
copy there. The part worth copying is Tamagui's **spread variant** `'...size'`:
a variant that inherits every key of a token group automatically, so adding a
size token to the theme extends every component that spreads it. In Rust:

```rust
// a variant whose values *are* the theme's spacing tokens, no per-value arm
size: Spacing => |e, tok| e.pad(tok).text_size(tok.scale(1.1))
```

i.e. allow a recipe axis to be a *token type* with a single function arm rather
than an enum with one arm per value. Falls straight out of the closure-arm
design; costs nothing, and it is how `Theme` growth stays cheap.

### 5. Defaults that are *values*, not absent-ness

`defaultVariants` is a boring feature that removes a whole class of bugs: every
recipe call has a fully-determined variant tuple, so there is no "unset means
whatever the cascade says". In Rust this is `#[derive(Default)]` plus
`ButtonSpec { size: Lg, ..Default::default() }` — free, idiomatic, and it makes
the variation space printable, diffable and serialisable (a preset file is then
literally a `ButtonSpec`).

## What NOT to copy

**The compiler.** Tamagui's static extractor exists because JS style objects are
otherwise rebuilt and diffed every render, and because CSS class names must be
emitted ahead of time. MUI has neither problem: `Style` is a POD struct, a
recipe arm is a closure the optimizer already inlines, and the output is Vello
paths, not a stylesheet. A build-time step here would buy nanoseconds and cost
the entire "just write Rust" property. Tamagui's own docs admit the compiler
*bails out* whenever it cannot partially evaluate — a class of failure MUI
should not import.

**Atomic anything.** Sprinkles generates one class per (property, value,
condition) because CSS specificity makes overriding hard. MUI applies a `Style`
patch by assignment — last write wins, trivially. Atomicity would be a solution
to a problem MUI does not have, and a combinatorial explosion of generated code.

**String keys.** `'@sidebar/sm'`, `$gtMd`, `'blue.500'` are stringly-typed
because TS cannot do better inside an object literal. Rust can: enums, token
types, `[T; N]`. Every place these systems reach for a magic string, MUI should
reach for a type — and refuse a shorthand-string API (`.style("p-4 bg-primary")`)
for the same reason. Typos become compile errors or they become bug reports.

**Slot recipes as class-name bundles.** Panda's `checkbox()` returns
`{ root, control, label }` and the *caller* must wire each class to the right
element. That is a decoupling MUI does not need and would suffer from: MUI's
recipe should build the tree, not hand back styles the caller re-attaches.

**Media-query *props* (`$sm={{...}}`).** Prefixed prop keys are a JSX-shaped
workaround. MUI's equivalent should be responsive *values* (idea 3) — one
concept instead of a parallel prop namespace — and, where the tree itself must
change, an explicit branch, which is just Rust `if`.

**`compoundVariants` past two axes.** Panda allows N-way compounds with array
matching (`intent: ['primary','secondary']`). Every compound is a rule that
fires at a distance; three of them in one recipe and nobody can predict the
output. Cap MUI's compounds at pairs, and when a widget needs more, that is the
signal it should be two widgets.

## Sources

- Panda CSS recipes / cva: https://panda-css.com/docs/concepts/recipes
- Panda CSS slot recipes (sva, compoundVariants, RecipeVariantProps): https://github.com/chakra-ui/panda/blob/main/website/content/docs/concepts/slot-recipes.mdx
- Panda CSS conditional styles, breakpoints, container queries: https://github.com/chakra-ui/panda/blob/main/website/content/docs/concepts/conditional-styles.mdx
- Panda CSS Stitches/styled-components migration (responsive object syntax): https://github.com/chakra-ui/panda/blob/main/website/content/docs/migration/stitches.mdx
- cva reference (base, variants, compoundVariants, defaultVariants): https://cva.style/docs/getting-started/variants
- vanilla-extract sprinkles (defineProperties, conditions, responsiveArray, shorthands): https://github.com/vanilla-extract-css/vanilla-extract/blob/master/packages/sprinkles/README.md
- vanilla-extract sprinkles docs page: https://vanilla-extract.style/documentation/packages/sprinkles/
- vanilla-extract recipes: https://vanilla-extract.style/documentation/packages/recipes/
- Tamagui variants (boolean, ':string', '...size' spread, functional with tokens/theme): https://github.com/tamagui/tamagui/blob/main/code/tamagui.dev/data/docs/core/variants.mdx
- Tamagui styled() and .styleable(): https://github.com/tamagui/tamagui/blob/main/code/tamagui.dev/data/docs/core/styled.mdx
- Tamagui "why a compiler" (atomic CSS extraction, partial evaluation, tree flattening, useMedia/useTheme evaluation): https://github.com/tamagui/tamagui/blob/main/code/tamagui.dev/data/docs/intro/why-a-compiler.mdx
- Tamagui 1.0 post (view flattening across module boundaries): https://github.com/tamagui/tamagui/blob/main/code/tamagui.dev/data/blog/version-one.mdx
- Tamagui useStyle (shorthand expansion, media merge, token resolution): https://github.com/tamagui/tamagui/blob/main/code/tamagui.dev/data/docs/core/exports.mdx

## Local references

- `/mnt/Windows11/DEV_PROJECTS/Repos/MUI-research/crates/mui-core/src/style.rs` — `Style` is already the "style object" these systems build; a recipe arm patches it.
- `/mnt/Windows11/DEV_PROJECTS/Repos/MUI-research/crates/mui-core/src/element.rs` — `Styled::styled(&Style)` and `Styled::when(cond, f)` are the two primitives a recipe needs; nothing new is required for the cheap version.
- `/mnt/Windows11/DEV_PROJECTS/Repos/MUI-research/crates/mui/src/widgets.rs` — `button`, `toggle`, `slider`, `knob` are four recipes with their variants hardcoded; this is the file a recipe API should shrink.
