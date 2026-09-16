# StyleX — merging semantics and theming, read as a model for MUI's `Style`

Meta's StyleX is a compile-time styling system for React. It is worth a long look not
because MUI needs atomic CSS (it has no CSS at all) but because StyleX is the only
mainstream system that treated **"which declaration wins"** as a design problem with a
provable answer instead of a cascade you learn by bruising. That answer transfers
directly to `Style`, `Styled::styled`, and whatever preset mechanism MUI grows.

---

## What the system does

Three moving parts:

1. **`stylex.create({...})`** — a compile-time macro-ish call. Every property in every
   style object is compiled away into one atomic CSS class (`color: red` → `.x1e2nbdu`),
   emitted once for the whole app and deduplicated globally. What survives at runtime is
   a plain object mapping property names → class names.
2. **`stylex.props(a, b, c)`** — merges those objects left to right and returns
   `{className, style}` to spread onto an element. Merging is a **property-keyed object
   merge**, not a CSS cascade: for each property, the last argument that sets it wins.
   Specificity never enters. Order of arguments is the only rule.
3. **`defineVars` / `createTheme`** — tokens compile to CSS custom properties; a theme is
   a *partial override* of a token set, applied as a style to any subtree root.

The compiler enforces that style values are statically analyzable: object/string/number/
array literals, `null`/`undefined`, simple local constants, and other StyleX calls. No
function calls, no spreads, no imported values (except vars). That restriction is what
buys the guarantee — everything mergeable is known at build time.

Sources: <https://stylexjs.com/docs/learn/thinking-in-stylex>,
<https://stylexjs.com/docs/learn/styling-ui/using-styles>,
<https://stylexjs.com/docs/learn/styling-ui/defining-styles>,
<https://stylexjs.com/docs/api/javascript/create>,
<https://stylexjs.com/docs/api/javascript/defineVars>,
<https://stylexjs.com/docs/api/javascript/createTheme>,
<https://stylexjs.com/docs/learn/theming/using-variables>,
<https://stylexjs.com/docs/learn/recipes/shareable-tokens>,
<https://stylexjs.com/docs/learn/static-types>.

---

## The exact syntax

### 1. Define and merge — last wins, order is the whole API

```tsx
const styles = stylex.create({
  base:        { backgroundColor: 'gainsboro', color: 'black' },
  highlighted: { backgroundColor: 'blueviolet', color: 'white' },
});

<div {...stylex.props(styles.base, styles.highlighted)} />  // violet
<div {...stylex.props(styles.highlighted, styles.base)} />  // gainsboro
```

No `!important`, no specificity war, no "why did the library's class beat mine". Swap the
arguments, swap the winner. That is the entire mental model.

### 2. Conditionals are just JavaScript; falsy is skipped

```tsx
<div
  {...stylex.props(
    styles.base,
    props.isHighlighted && styles.highlighted,
    isActive ? styles.active : styles.inactive,
  )}
/>
```

`null` / `undefined` / `false` are ignored. There is no `variant()` helper and no
`cva`-style config object — a variant is a key in `create` plus an index expression.

### 3. Pseudo-states live *inside* the property value, not in a selector

```js
const styles = stylex.create({
  violet: {
    backgroundColor: { default: 'blueviolet', ':hover': 'darkviolet' },
    color: 'white',
  },
});
```

This inversion is the clever bit: state is a property of the *value*, so merging still
happens per-property. `styles.gray` overriding `styles.violet` replaces the whole
`backgroundColor` map — default *and* hover — rather than leaving a stale `:hover` rule
winning on specificity. States can never desynchronise from their base value.

### 4. Tokens: `defineVars`, consumed by import

```js
// tokens.stylex.js
const DARK = '@media (prefers-color-scheme: dark)';
export const colors = stylex.defineVars({
  primaryText: { default: 'black', [DARK]: 'white' },
  accent:      { default: 'blue',  [DARK]: 'lightblue' },
});
export const spacing = stylex.defineVars({
  none: '0px', xsmall: '4px', small: '8px', medium: '12px', large: '20px',
});

// component.js
const styles = stylex.create({
  card: { color: colors.accent, padding: spacing.medium },
});
```

### 5. Themes are *partial* overrides, applied to a subtree

```js
export const darkTheme = stylex.createTheme(colors, {
  background: '#1a1a1a',
  text: 'white',          // everything not named keeps the base value
});

<div {...stylex.props(darkTheme)}><ContentToBeThemed /></div>
```

A theme is itself a style object, so it merges by the same last-wins rule and nests:
inner theme beats outer theme, because it is applied closer.

### 6. Dynamic styles — deliberately ugly, deliberately narrow

```js
const styles = stylex.create({
  bar:        (height) => ({ height }),              // args must be plain identifiers
  positioned: (x, y) => ({ transform: `translate(${x}px, ${y}px)` }),
});
<div {...stylex.props(styles.bar(height))} />
```

The body must be a single object literal. No destructuring, no defaults, no `return`.
Compiles to a class plus a CSS custom property set via the inline `style` attribute.

---

## Worth stealing for MUI

### A. `Style` merge that is per-field, with a real "last wins" rule

Today `Styled::styled(&CARD)` does `*self.style_mut() = s.clone()` — it **clobbers**.
`leaf(..).fill(Primary).styled(&CARD)` silently loses the fill; `.styled(&CARD).fill(Primary)`
keeps it. That is an order-dependence nobody can predict, which is precisely what StyleX
set out to kill. The fix is not to remove order-dependence but to make it *the rule*:

```rust
impl Style {
    /// Every field `other` states wins; every field it leaves at its
    /// default is inherited from `self`. Last call wins, per field.
    pub fn over(&self, other: &Style) -> Style { … }
}

pub trait Styled {
    /// Merge a prepared style over whatever is set so far.
    fn preset(self, s: &Style) -> Self;   // .preset(&CARD) — CARD wins
    /// Merge a prepared style *under* it: a default the chain can override.
    fn base(self, s: &Style) -> Self;     // .base(&CARD)   — the chain wins
}
```

Then both of these read correctly and neither surprises:

```rust
leaf(120., 40.).base(&CARD).fill(Role::Danger)   // a danger card
leaf(120., 40.).fill(Role::Danger).preset(&CARD) // a card, full stop
```

This needs "field the caller set" to be distinguishable from "field at its default".
`fill: Fill::None`, `radius: Radius::Theme` and the `Option` fields already encode that;
`weld: bool` and `shells: Vec<_>` do not. Either make `weld: Option<bool>` or keep a
`set: u16` bitmask on `Style` (cheap, `Copy`, one bit per field, merge is
`self.set | other.set` and pick per bit). The bitmask is the lazier option and makes
`over` a 12-line function with no per-field `is_default` heuristics.

### B. Make shorthand vs longhand deterministic — there is a live bug

StyleX's second guarantee: "the more specific property wins (`margin-top` over `margin`)
regardless of application order". MUI has the same shape of conflict and currently gets
it wrong in one place. In `mui-layout/src/lib.rs`:

```rust
pub fn pad(mut self, padding: impl Into<Spacing>) -> Self {
    match padding.into() {
        Spacing::Px(v)    => self.padding = Insets::all(v),   // does NOT clear self.pad
        Spacing::Token(t) => self.pad = Some(t),
    }
}
// padding() resolves: self.pad.map_or(self.padding, …) — token always wins
```

So `.pad(M).pad(12.0)` keeps `M` and silently drops the 12. (`pad_xy` and `insets` both
do `self.pad = None`, so only the `Spacing::Px` arm leaks.) One line fixes it —
`Spacing::Px(v) => { self.padding = Insets::all(v); self.pad = None; }` — but the
*principle* is the steal: **one storage slot per concept**, never two representations of
padding that both survive. Same audit applies to `Stroke { fill, width }` (already
correct: `stroke()` preserves an earlier `stroke_width`) and to `radius` vs `pill`.

### C. States belong to the value, not to a second style

StyleX's `backgroundColor: { default: …, ':hover': … }` is the pattern MUI should copy
for interaction states, instead of a second `Style` merged in on hover. MUI half has it
already — `Fill::map(palette, under, f)` derives a hover tint from the resolved colour,
which is strictly better than a second colour table. Make it declarative:

```rust
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Fill { /* … */ }

impl Styled {
    /// This node's fill, lightened while hovered; ditto press.
    fn hover(self, f: impl Fn(Color) -> Color + 'static) -> Self;
    fn on_hover(self, fill: impl Into<Fill>) -> Self;   // explicit override
}

leaf(80., 28.).pill().fill(Role::Primary).hover(Color::lighten_5).cursor(Hand)
```

The invariant to keep is StyleX's: a later `.fill()` must replace the hover variant too.
If hover lives as a closure in a separate `Style` field, `.fill(Role::Danger)` after
`.hover(..)` leaves a hover derived from a colour that is no longer there. Store hover as
part of the fill slot — `struct Slot<T> { default: T, hover: Option<T>, press: Option<T> }`
— so replacing the slot replaces the states, exactly as StyleX does.

### D. `createTheme` = partial override, which MUI already nails — extend it to presets

`stylex.createTheme(colors, { background: '#1a1a1a', text: 'white' })` is, in Rust,
already the documented `Theme` pattern:

```rust
pub const SKIN: Theme = Theme {
    palette: Palette { primary: Pigment::new(242.0, 0.131), ..Palette::NEUTRAL },
    ..Theme::DEFAULT
};
```

Struct-update syntax *is* `createTheme`, for free, at compile time, type-checked. Nothing
to steal at the theme level — but nothing stops the same shape being the preset story:

```rust
pub const CARD: Style = Style {
    fill: Fill::Role(Role::Raised),
    radius: Radius::Px(12.0),
    ..Style::NONE
};
pub const DANGER_CARD: Style = Style { fill: Fill::Role(Role::Danger), ..CARD };
```

That needs `Style::NONE` as a `const` (like `Theme::DEFAULT`) and `Fill`/`Radius`
variants that are const-constructible. `shells: Vec<_>` blocks a `const Style` today;
either box it as `&'static [(Spacing, Fill)]` in a const-friendly `Cow`, or accept that
presets are `fn card() -> Style` and lose nothing but a hair of ergonomics. The
`fn preset() -> Style` route is the lazy one and should be the default recommendation —
`const` matters for theme files, not for a handful of card styles.

### E. Compile-time-only, no runtime style language

StyleX bans function calls, spreads and imported values inside `create` so the compiler
can see every declaration. MUI gets the equivalent for free — `Style` is a Rust struct,
and there is no runtime CSS to parse — but the discipline is worth writing down as a
rule: **never add a `.style("fill: primary; radius: 8")` string DSL**. It would reopen
exactly the parse-at-runtime, fail-at-runtime hole StyleX spent a compiler closing, and
in a plugin editor the failure lands on an audio thread's UI at load time. If a shorthand
string ever feels necessary, make it a proc-macro (`style!(fill: Primary, radius: 8)`)
that expands to the same struct literal and errors at build time.

---

## What NOT to copy

- **Atomic classes and hashed names.** They exist to defeat the CSS cascade and to
  deduplicate bytes over the wire. MUI paints through Vello from a struct — there is no
  cascade to defeat and no stylesheet to shrink. Do not build an interning table of
  "atoms" for `Style` fields; a 9-field struct merged per-field is faster and readable in
  a debugger.
- **The `default` / `:hover` map spelled as a map.** In Rust, a `HashMap<State, Fill>` per
  property would allocate per node per frame. Take the *semantics* (states owned by the
  value slot) with a fixed-shape struct, not the JSON shape.
- **Media-query-valued tokens** (`{ default: 'black', [DARK]: 'white' }`). MUI resolves
  roles against a `Palette` and the colour underneath, which already handles light/dark
  *and* contrast-on-whatever-it-sits-on. Adding a second, parallel light/dark mechanism at
  the token level would let the two disagree.
- **Dynamic styles via CSS custom properties.** The whole `(height) => ({ height })`
  escape hatch exists because CSS classes are static. In MUI every value in the tree is
  already a Rust expression evaluated per frame; `leaf(w, h)` *is* the dynamic style.
  Nothing to add.
- **`stylex.props` returning `{className, style}` to spread.** The "styles as props,
  passed into components" pattern is React plumbing. MUI's equivalent is passing an `El`
  or a `&Style` — already simpler.
- **`styleResolution` as a configuration option.** StyleX ships alternative merge modes
  for migration. One merge rule, no knob. A styling system with a configurable precedence
  rule has no precedence rule.

---

## The short version

Steal the *rule*, not the machinery: one slot per property, the last writer wins, a
preset is a merge and says which side it merges on, states ride inside the slot they
modify. Everything StyleX built on top of that — atoms, hashes, compilers, custom
properties — is scaffolding for a language MUI does not speak.
