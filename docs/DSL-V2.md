# DSL v2 and the 2026 cleanup

The spec every change in this pass follows. Where a builder has to choose a
name not listed here, it follows the principles, adds a migration rule, and
records the name in the "Decided during implementation" table at the bottom.

## Principles

1. **One way to say a thing.** No alias verbs, no parallel constructor and
   macro names. If two builders do the same thing, delete one.
2. **Names say what you see, not how it is implemented.** `block`, not `leaf`;
   `a11y`, not `role(Kind)`.
3. **No trait imports for the core vocabulary beyond the prelude.** Layout
   verbs are inherent on `Node`; paint verbs on `Paints` (shared with `Style`);
   everything else on `Styled`. `Sugar` and `IntoLen` are gone.
4. **Switches take no arguments.** `.focusable()`, `.disabled()`, `.clip()`.
   Conditional use goes through `.when(cond, |e| e.disabled())`. The one
   exception is a value that is *inherently* dynamic data, never a switch.
5. **`_with(x)` is the only way to pass an explicit option to a verb that has
   a default.** `.animate()` / `.animate_with(spring)`.
6. **Ids are `Id`, everywhere.** Every API that names a node takes
   `impl Into<Id>`; every map keyed by a node is keyed by `Id` (or its hash).
   Names starting with `/` are reserved for the runtime; `Id::of` rejects them
   with a debug assertion and `Id::is_named()` is public, the one copy of the
   rule.
7. **Widgets return named results.** No tuples, no `.0`.
8. **Renderer choice never lives in the tree.** Backends are chosen by the
   `SceneSpec` or the host.
9. **Every rename ships a migration rule** in `tools/mui-migrate`, and the
   repo's own examples/tests/preview are migrated *by running the tool*, not
   by hand. That is the tool's test.

## Vocabulary changes

### Constructors (mui-scene / mui-layout)

| old | new |
|---|---|
| `leaf(w, h)` | `block(w, h)` |
| `column(children)` | `col(children)` |
| `overlay(children)` | `stack(children)` |
| `row`, `grid`, `fits`, `spacer`, `text`, `icon`, `canvas` | unchanged |
| `canvas_cached(key, f)` | `canvas_keyed(key, f)` |
| macros `row! col! stack! grid! fits!` | unchanged (now match the functions) |

### Text roles

Sizes come from a new `Theme::type_scale` (`TypeScale { title, body, caption }`,
defaults 18/13/11) read at resolve time, not hard-coded px.

| old | new |
|---|---|
| `title(s)` | `title(s)` |
| `label(s)` (13 px text) | `body(s)` |
| `caption(s)` | `caption(s)` |

### Layout sugar (inherent on `Node`, moved from `Sugar`)

`w`, `h`, `square`, `center`, `start`, `end`, `between`, `full` move to
mui-layout as inherent methods. `Len` gets `From<i32>`, `From<f64>`,
`From<f32>`, so `.width(120)` and `.w(120)` both compile; `IntoLen` is deleted.

| old | new |
|---|---|
| `.anchor(ax, ay).offset(dx, dy)` for absolute placement | `.at(dx, dy)` (start/start anchored) — `anchor`/`offset` stay for the general case |
| `.anchor(Align::Center, Align::Center).offset(dx, dy)` | `.centered_at(dx, dy)` |
| `.min_width(w).min_height(h)` pairs | unchanged, plus `min_size`/`max_size` taking `impl Into<Size>` from `(f64, f64)` |

### Accessibility

| old | new |
|---|---|
| `Kind` (a11y enum) | `A11y` |
| `.role(Kind::X)` | `.a11y(A11y::X)` |
| `.label(name)` (accessible name) | `.named(name)` |
| `Semantics` | unchanged |

`Role` (the colour role) keeps its name: with `Kind` gone it no longer
collides. `pub use Role::*` leaves the prelude; write `Role::Raised`.
The spacing tokens `Xs S M L Xl` stay in the prelude — they are the compact
core of the DSL.

### Switches

| old | new |
|---|---|
| `.disabled(bool)` | `.disabled()` (+ `.when`) |
| `.scroll_bar(bool)` | `.scrollbar()` (off by default) or `.no_scrollbar()` if on by default — keep whichever matches today's default |
| `.focusable()`, `.baseline()`, `.captures_wheel()`, `.tracks_pointer()`, `.clip()`, `.scroll()`, `.float()`, `.sticky()`, `.wrap()` | unchanged |

### Motion

| old | new |
|---|---|
| `.animate()` | `.animate()` |
| `.transition(spring)` | `.animate_with(spring)` |
| `.animate_layout()` | `.animate_layout()` |
| `.layout_transition(spring)` | `.animate_layout_with(spring)` |

### Weld and material

One verb, options on the value:

| old | new |
|---|---|
| `.weld_with(w)` | `.weld(w)` |
| `.weld_shape()` | `.weld(Weld::shape())` |
| `.weld_borders()` | `.weld(Weld::borders())` |
| `.weld_morph(p)` | `.weld(Weld::default().morph(p))` |
| `.weld_quality(q)` | `.weld(Weld::default().quality(q))` |
| `.without_weld()` | `.weld(Weld::off())` |
| `.exclude_from_weld()` | `.unwelded()` |
| `.gpu_weld(w)` / `.reference_weld(w)` | `.weld(w)` + `SceneSpec::weld_backend(WeldBackend::..)` |
| `weld_morph![p; ..]` | `weld![Weld::default().morph(p); ..]` |

Material/surface verbs (`union`, `join`, `cut`, `keep`, `join_border`,
`inset_surface`, `inset_surface_of`, `surface_layout`, `shell`,
`border_ramp`) move with the material code into the new `mui-material` crate
as the `Material` extension trait (re-exported by the prelude). Renames:

| old | new |
|---|---|
| `.join()` (squares children, clips ends) | `.segmented()` — and it must also square children pushed *after* it (resolve-time, not build-time) |
| `.cut(el)` / `.keep(el)` on a leaf | compile to a wrapping `stack` instead of a silent no-op |

### Identity and cross-references

`.id(..)`, `join_border(..)`, `inset_surface_of(..)`, `glide` callbacks all
take `impl Into<Id>`. A dangling cross-reference names the missing id in its
error. Icons: `mui_symbols::sym::HOME` generated `char` consts replace
`material_symbols::codepoint("home")`; the table moves to its own
`mui-symbols` crate with a sortedness test.

### Resolving

`resolve_scene`, `resolve_scene_with`, `resolve_scene_cached`,
`resolve_scene_animated`, `resolve_scene_retained` →
one `Resolver` that owns the caches:

```rust
let mut r = Resolver::new();          // caches live here
let scene = r.resolve(&spec)?;        // was resolve_scene_cached/_retained
let scene = r.resolve_animated(&spec, dt)?; // only if the animated path needs dt
resolve(&spec)?                       // free fn for the uncached one-shot
```

### Style merging

`Style` fields that "may be unset" become `Option<T>` so `over` is
`other.x.or(self.x)` for every field. `Radius::Theme`, `CornerStyle::Round`,
`Fill::None`, blur `0` stop meaning "unset". `union` becomes `Option<bool>`.
`Elevation::shadows()` returns `&'static [Shadow]`.

### Widgets (crate `mui`)

Every widget returns `#[must_use] Response<C = bool>`:

```rust
pub struct Response<C = bool> { pub el: El, pub changed: C }
```

`C` is `bool` for value widgets and `Option<Edit>` for curve/bins. Widgets
that used to return `Control` take their size token as part of the call or
through a `Look` option so the caller never reaches into `.0`. Every widget
takes `id: impl Into<Id>`. Positional bools (`color_picker(.., alpha)`)
become option structs or builder methods. `Ui::state` returns
`Interaction { hover, press }`. `bins` and `curve` agree on one mutation
model (`&mut` value in, `changed` out).

`mui-truce::Bridge::bind(ui, P::Gain, |ui, id, v| knob(ui, id, "Gain", v, 0.0..=1.0).el)`
— the id is derived from the parameter and handed to the closure, so it
cannot be typed twice. `bind_bool` exists for toggles.

### Before / after

```rust
// before
column([
    overlay([
        leaf(size, size).pill().preset(look.face(Role::Raised))
            .role(Kind::Slider { value, min, max }).label(label.clone())
            .focusable().id(id),
        leaf(dot, dot).pill().fill(look.role)
            .anchor(Align::Center, Align::Center).offset(x, y),
    ]),
    text(caption).fill(Role::Dim),
]).gap(Xs).align(Align::Center)

// after
col![
    stack![
        block(size, size).pill().preset(look.face(Role::Raised))
            .a11y(A11y::Slider { value, min, max }).named(&label)
            .focusable().id(id),
        block(dot, dot).pill().fill(look.role).centered_at(x, y),
    ],
    body(caption).fill(Role::Dim),
].gap(Xs).align(Align::Center)
```

## Structure changes

- Edition 2024, `rust-version`, `[workspace.lints]`, `[workspace.dependencies]`
  (every internal crate + shared external versions).
- `mui-stage`, `mui-reel`, `mui-motion-bridge` and `tools/kurv-*` move to
  `media/`, a separate workspace depending on `../crates/*` by path.
- New crates: `mui-material` (split from mui-scene), `mui-symbols`.
- Spacing tokens move from mui-geometry to mui-layout.
- kurbo is the one `Point`/`Rect`/`Affine`/`BezPath`; mui-weld and
  mui-geometry drop their copies.
- `mui::Ui` splits into `ui/{mod,input,scroll,drag,motion,memo}.rs` + `ui/tests`;
  per-key maps merge into one `Id`-keyed `NodeState` map.
- `Walk` splits its state; dense pre-order maps become `Vec`s.
- One shared GPU host (`mui-vello::host`) and one keymap for preview and truce.

## Decided during implementation

| old | new | why |
|---|---|---|
