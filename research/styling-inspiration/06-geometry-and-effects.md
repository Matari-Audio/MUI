# Geometry and effects: what the web learned, what MUI should take

Scope: CSS/Figma geometry and effect techniques, judged against what Vello 0.2
(`vello_cpu` / `vello_hybrid`, the two backends MUI ships) can actually paint,
and what the builder DSL should look like if we take them.

## The renderer's real ceiling

Everything below is decided by this table, so it goes first. Checked against the
vendored `vello_{common,cpu,hybrid}-0.2.0` sources, not from memory.

| capability | status | evidence |
|---|---|---|
| Linear, two-point radial, sweep (conic) gradients, all with extend modes | **free today** | `PaintType = peniko::Brush<Image, Gradient>` (`vello_common/src/paint.rs:278`) |
| Analytic blurred rounded rect, **and its inverse** | **free today** | `fill_blurred_rounded_rect(&Rect, radius: f32, std_dev: f32, invert: bool)` — `invert: true` is an inner shadow with no filter pass |
| Clip layers, blend modes, opacity layers | **free today** | `push_clip_layer`, `push_blend_layer`, `push_opacity_layer` |
| Boolean ops on paths (union / intersect / difference / xor) | **free today, already in-tree** | `mui-geometry::boolean(lhs, rhs, BooleanOp, opts)` — the DSL only exposes `Union` via `.weld` |
| Alpha / luminance masks | **CPU only** | `Mask::new_alpha(&Pixmap)`; `vello_hybrid::Scene::push_mask_layer` *"currently always panics since masks are not supported yet"* |
| Gaussian blur, flood, offset, drop-shadow as filter layers | **single-threaded CPU only** | only `filter/{flood,gaussian_blur,offset,drop_shadow}.rs` exist; `vello_cpu/src/dispatch/multi_threaded.rs:557` is `unimplemented!("Filter effects are not yet supported in multi-threaded rendering")` — MUI's `cpu-threads` feature would panic |
| feTurbulence, feDisplacementMap, feColorMatrix, InnerShadow, lighting | **declared, not implemented** | the variants exist in `filter_effects.rs` but nothing consumes them |
| Backdrop / background sampling | **not implemented** | `FilterInput::BackgroundImage` is an enum variant with no backend |
| Arbitrary fragment shaders | **never** | sparse-strip renderer, no user shader hook |

Blunt reading: **paint-level tricks are cheap, filter-level tricks are a trap.**
Any design that routes through `push_filter_layer` breaks the GPU backend, the
multithreaded CPU backend, or both. Design around geometry and gradients.

---

## Geometry

### 1. `clip-path` / `mask` — and the new `shape()` function

CSS moved past `path()` (SVG `d`, px only) to `shape()`, which is the same
geometry in CSS words with CSS units and `calc()` inside:

```css
.notch { clip-path: shape(from 0 0, hline to 100%, vline to 100%,
                          curve by -40px 0 with -10px -20px, close); }
.fade  { mask-image: linear-gradient(to bottom, black 60%, transparent); }
```

**Vello:** clipping natively. Masks are the CPU-only trap above — but a gradient
mask is *not* a mask, it is a gradient-filled shape in a blend layer, which both
backends do.

**MUI:** `Path::from_svg_data` is already `path()`. The `shape()` lesson is
about readability of the source, and MUI's answer should be the boolean engine,
not a path mini-language. We ship `BooleanOp::{Intersection, Difference}` and
expose neither:

```rust
// clip-path, without a clip layer or a mask: cut the hole in the outline itself
let ring = leaf(64, 64).pill().fill(Primary)
    .cut(leaf(40, 40).pill());          // Difference against a child's frame
let badge = panel.keep(leaf(120, 40).radius(8.0)); // Intersection
```

`.cut(el)` and `.keep(el)` are `.weld`'s missing siblings: same `Topology` →
`fillet` → outline pipeline, a few lines of dispatch each, composing with shells
and strokes for free because they produce an *outline*, not a layer. Highest
capability-per-line on the list.

### 2. `corner-shape` / `superellipse()` — squircles as a corner *style*

Chrome shipped `corner-shape` in 2025. The key design move: corner **radius**
and corner **shape** are separate properties, so one value changes every corner
of a whole tree without touching sizes.

```css
.card {
  border-radius: 24px;
  corner-shape: squircle;              /* == superellipse(2) */
}
.tag  { border-radius: 12px; corner-shape: bevel;  }   /* superellipse(0)  */
.slot { border-radius: 12px; corner-shape: scoop;  }   /* superellipse(-1) */
```

`superellipse(n)`: `1` = circle (today's arc), `2` = iOS continuous corner,
`∞` = square, `0` = a straight bevel, negative = concave scoop, `-∞` = notch.

**Vello:** trivially yes. A superellipse corner is a cubic approximation
replacing one `Arc`; it is path data, not an effect.

**MUI:** belongs in `CornerStyle` in `mui-geometry::fillet`, which already
carries `convex_radius` / `concave_radius` / `clearance_fraction`. One extra
field flows through welds, shells and strokes automatically — including the
concave joins a weld produces, exactly where a circular arc looks worst.

```rust
// theme-level, one line, changes every corner in the tree
Theme { corner: Corner::Squircle, ..Theme::DEFAULT }

// per node, when a control wants to disagree
tab.radius(12.0).corner(Corner::Superellipse(2.4))
let chip = leaf(80, 24).radius(8.0).corner(Corner::Bevel);
```

`Corner::{Round, Squircle, Superellipse(f64), Bevel, Scoop, Notch}` mirrors the
CSS keyword set; `Round` stays the default so nothing changes until asked.
Continuous corners are what makes a panel read as "designed" rather than "drawn
by a layout engine" — the most visible upgrade per line on this list.

### 3. Figma boolean operations and "corner smoothing"

Figma's Union/Subtract/Intersect/Exclude on a *live, editable* group plus a
0–100% corner-smoothing slider is the same two features as #1 and #2 — worth
noting that a design tool landed on the same shape: booleans belong on the
*group*, not a path, and smoothing is a *slider on the corner*, not a separate
primitive. `.weld` is already Figma's Union-on-a-group; `.cut`/`.keep` finish
the set. If a slider reads better than an exponent, `Corner::Smooth(0.0..=1.0)`
mapping `0 → n=1`, `1 → n≈4` is a one-line remap and springs cleanly, which the
keyword form does not.

### 4. `border-image` — skip it

```css
.frame { border-image: url(frame.png) 30 fill / 30px round; }
```

Nine-slice scaling of a bitmap border. **Vello:** possible (nine image draws).
**MUI: do not.** It exists because CSS could not describe the shape; MUI can.
A bevelled rack panel or a screw-hole border is `shell` + `Corner::Bevel` + a
gradient: vector, DPI-free, animatable.

---

## Positioning and layout

### 5. Anchor positioning — `anchor()`, `position-area`, `position-try`

The most under-rated CSS feature of the decade, and the one MUI's own docs
half-apologise for. Today `.offset(dx, dy)` is described in `mui-layout` as
*"The only coordinates in the system, and relative ones at that."* — a tell that
it exists because there was nothing better.

```css
.btn     { anchor-name: --send; }
.tooltip { position: fixed;
           position-anchor: --send;
           position-area: block-start center;   /* above, centred */
           width: anchor-size(width);            /* match the anchor */
           position-try-fallbacks: flip-block;   /* flip below if clipped */ }
```

Three ideas MUI wants: **`position-area`** (placement as a named region of a 3×3
grid around the anchor, never numbers), **`anchor-size()`** (size derived from
the anchor, so a dropdown matching its field is declarative), and
**`position-try-fallbacks`** (an ordered list, engine picks the first that fits —
the whole reason tooltip libraries exist). Pure layout; Vello is not involved.

**MUI:** a `Pin` primitive replacing `.offset` for everything but a true
one-pixel nudge, and that `.tip()` and `.float()` are both built on:

```rust
menu.pin(Pin::to("field").area(Area::BlockEnd).match_width()
                         .try_(&[Area::BlockStart]))
tip.pin(Pin::to("knob").area(Area::Top).gap(Xs))
```

MUI's `.tip("..")` already hardcodes a placement somewhere in the runtime;
making `Pin` the primitive means the tooltip, the dropdown, the context menu and
the drag-ghost stop being four special cases. A plugin editor is a fixed-size
window, so the fallback list is short and cheap to evaluate — one rect test per
candidate at layout time. **Take the region names and the fallback list; skip
`anchor()`'s per-side arithmetic form** (`top: anchor(bottom)`), which is the
part web developers actually get wrong.

### 6. Container queries and `cqw` units

```css
.panel { container-type: inline-size; container-name: rack; }
@container rack (min-width: 480px) { .row { flex-direction: row; } }
.title { font-size: clamp(14px, 4cqw, 28px); }
```

**MUI:** already 80% here. `.min_col(120.0)` is `repeat(auto-fit, minmax())` and
`clamp(min, pct, max)` is the fluid part. The missing 20% is `cqw` — a length
relative to the *nearest sized ancestor* rather than the viewport — which for a
resizable plugin editor is the only percentage that means anything:

```rust
label.size(clamp(14.0, 4.0, 28.0))   // already reads as 4% of... what, exactly?
```

The honest fix is making explicit what `Len::Pct` is a percentage *of*:
`Len::Container(pct)` resolving against the nearest `.clip()`/`.scroll()`/sized
ancestor. Small layout change, removes a real ambiguity. **Skip `@container`
blocks entirely** — `if w > 480.0 { row![..] } else { col![..] }` beats a query
language in a tree rebuilt every frame. One place where "no runtime CSS" is a
straight win; do not give it back.

### 7. Scroll-driven animations

```css
.bar   { animation: grow linear; animation-timeline: scroll(nearest block); }
.card  { animation: fade linear; animation-timeline: view();
         animation-range: entry 0% cover 50%; }
```

A timeline driven by scroll offset instead of clock. **MUI:** the runtime owns
`ui.tween(id, target)` and scroll offsets already, so a scroll-linked value is
`ui.tween` fed from `scroll_offset()` — three lines in a scene, no new concept.
**Do not build a timeline system.** Plugin editors have one scrolling thing and
the useful effect is a fade at the clipped edge: a gradient overlay, not an
animation.

### 8. View Transitions

```css
@view-transition { navigation: auto; }
.card { view-transition-name: card-7; }
```

The clever part is not the crossfade — it is `view-transition-name` making the
browser **match elements across two different trees by name** and tween the
geometry between them.

**MUI:** MUI rebuilds every frame and matches nodes by `.id("name")` — same
mechanism, already present. The gap: `.animate()` springs fill, stroke, radius,
text size and shadow, *not frame*. Spring position and size too, and a node that
moved because the tree changed shape slides there:

```rust
row![tab("a"), tab("b")].animate()    // reorder the tabs, they slide
```

Same field on `Style`, same `Spring`, one more animated property. **Skip the
snapshot/crossfade machinery** — it exists because the DOM cannot rebuild two
trees at once, and MUI can.

---

## Paint

### 9. Radial and conic gradients

```css
.knob { background: conic-gradient(from -135deg, #3b82f6 0 70%, #1f2937 70%); }
.led  { background: radial-gradient(circle at 30% 30%, #fff, #0af 40%, #036); }
```

**Vello: already supported, MUI just does not expose it.** `Fill::Gradient`
carries one `angle: f64` and `Paint::Linear` hardcodes the linear case, while
the backend takes any `peniko::Gradient`. Biggest gap on the list between what
the renderer can do and what the DSL can say — and a conic gradient is *the*
audio-plugin primitive: knob arc, ring meter, pan indicator, zero paths and zero
per-frame geometry.

```rust
Gradient::conic(-135.0, [(0.0, Primary), (0.7, Primary), (0.7, Field)])
Gradient::radial((0.3, 0.3), 0.6, [(0.0, Raised), (1.0, Surface)])
```

Shape-wise: `Gradient { kind: Kind::{Linear{angle}, Radial{..}, Conic{..}},
stops }` plus two `Paint` variants. `Fill::map` and the spring interpolation
already walk stops generically, so hover tints and transitions come free.

### 10. Multiple box-shadows and inner shadows

```css
.key { box-shadow:
         inset 0 1px 0 rgb(255 255 255 / .35),   /* top highlight */
         inset 0 -2px 3px rgb(0 0 0 / .25),      /* inner floor   */
         0 1px 2px rgb(0 0 0 / .4),              /* contact       */
         0 8px 24px rgb(0 0 0 / .25); }          /* ambient       */
```

Two shadows in one stack — tight dark contact plus wide soft ambient — is the
whole difference between "CSS default" and "designed". MUI has exactly one.

**Vello: both are free and analytic.** `fill_blurred_rounded_rect`'s `invert:
bool` *is* `inset`, no filter layer and no mask, on both backends.

**MUI:** `Style.shadow: Option<Shadow>` → `Vec<Shadow>`, plus `inset: bool`.
Painted before `Fill` for outer, after `Fill` (clipped to the outline) for
inset — one extra arm in `scene.rs`. Then theme presets, because nobody should
hand-tune four shadows per node:

```rust
.shadow(Elevation::Raised)    // contact + ambient, from the theme
.shadow(Shadow::inset(2.0).dy(1.0).fill(Ink.alpha(0.3)))
```

Caveat: the analytic path is exact only for a *rounded rect*, and `scene.rs`
already tracks this — `Painted.rect` is `Some` only when the outline is one and
not welded. A welded outline's shadow would need the filter path, broken on
`cpu-threads`. So: **analytic shadow on rects; for welds, offset the outline and
fill it with a gradient-to-transparent** — vector, works everywhere, not a real
Gaussian, and at plugin sizes nobody will tell.

### 11. `backdrop-filter` / glassmorphism

```css
.overlay { backdrop-filter: blur(20px) saturate(1.4);
           background: rgb(255 255 255 / .12); }
```

**Vello: no.** `FilterInput::BackgroundImage` is an unimplemented enum variant;
`push_filter_layer` is single-threaded-CPU-only anyway. There is no path to a
true backdrop blur without a render-target read-back that neither backend
exposes.

**MUI: fake it and move on.** "Glass" at plugin scale is 90% a translucent fill,
a bright 1px top edge and a soft inner shadow — all of which #10 delivers. A
`Fill::Glass { tint, opacity }` preset expanding to exactly that is honest and
free. **Do not ship a real `.blur()`**; it panics on `cpu-threads` and no-ops on
GPU.

### 12. SVG filters: `feTurbulence` + `feDisplacementMap`

```svg
<filter id="grain">
  <feTurbulence type="fractalNoise" baseFrequency="0.8" numOctaves="3"/>
  <feColorMatrix type="saturate" values="0"/>
  <feComposite operator="in" in2="SourceGraphic"/>
</filter>
```

Film grain, brushed metal, warped edges — the reason plugin skins ship as
bitmaps. **Vello: no.** `FilterPrimitive::{Turbulence, DisplacementMap,
ColorMatrix}` are declared in `filter_effects.rs` and implemented nowhere; only
`Flood`, `GaussianBlur`, `Offset`, `DropShadow`, `DropShadowOnly` have backends.

**MUI:** the escape hatch exists and is the right answer — grain is a tiled RGBA
noise buffer through `Fill::Image(img, Fit::Cover)` in a multiply blend layer,
generated once at startup. A dozen lines in a scene, no core change. **Do not
add a noise primitive to the DSL** until a real skin needs one.

### 13. CSS Paint API (Houdini `paintWorklet`)

```js
registerPaint('checker', { paint(ctx, size, props) { /* draw */ } });
```
```css
.el { background: paint(checker); --size: 12; }
```

A user-supplied painter invoked with the element's own size. **MUI: already
shipped, and better.** `canvas(|size| vec![Draw::fill(path, Ink)])` is the same
contract — own size, own space, returns geometry — typed, with no worklet
registration and no string properties. The one idea left is Houdini's *input*
half: the worklet declares what it depends on, so the engine knows when to
re-run it. If `canvas` closures show up in a profile, memoise on a declared key.

### 14. Mesh gradients

Four-corner Coons-patch colour blends (Figma, `conic`+blur fakes on the web,
CSS `mesh()` still a proposal).

**Vello: no** — no patch primitive, no shaders. **MUI:** two stacked radial
gradients in a `screen` blend layer gets 80% of the look from #9 plus the blend
layer Vello already has. Worth a preview scene; not worth a `Fill` variant.

---

## Ranked: what to actually build

1. **`Gradient::conic` / `Gradient::radial`** — the renderer does it, the DSL
   cannot say it, and it is the native shape of every knob and meter.
2. **`Vec<Shadow>` + `Shadow::inset`** — `invert: bool` makes inner shadows
   analytic and free; two shadows are the line between default and designed.
3. **`Corner::Squircle` / `Superellipse(n)` on `CornerStyle`** — one field in
   `mui-geometry::fillet`, flows through welds, shells and strokes untouched.
4. **`.cut(el)` / `.keep(el)`** — `boolean()` with `Difference`/`Intersection`
   is in-tree and unexposed; clip-path shapes as *outlines*, so shells and
   strokes follow them.
5. **`Pin` replacing `.offset`** — named regions plus an ordered fallback list;
   collapses tooltip, dropdown, menu and float into one primitive.

## What to refuse, and why

- **`backdrop-filter` / a real `.blur()`** — no backend; panics under
  `cpu-threads`, no-ops on GPU. Fake glass with fill + edge + inner shadow.
- **`border-image`** — solves a problem CSS has and MUI does not; drags in an
  asset pipeline to avoid describing a shape we can describe.
- **`@container` query blocks** — an `if` on the measured width beats a query
  language in a tree rebuilt every frame.
- **A scroll/view timeline system** — one scrolling list, one useful effect,
  `ui.tween` covers it.
- **View-transition snapshot machinery** — MUI matches by `.id` natively; just
  extend `.animate()` to frame.
- **feTurbulence / mesh gradients as `Fill` variants** — unimplemented in Vello;
  `canvas()` and an image fill are enough.

## Sources

- https://docs.rs/vello_cpu/0.2.0/vello_cpu/struct.RenderContext.html
- https://docs.rs/vello_hybrid/0.2.0/vello_hybrid/struct.Scene.html
- https://docs.rs/vello_common/0.2.0/vello_common/mask/struct.Mask.html
- https://docs.rs/vello/0.9.0/vello/struct.Scene.html — `draw_blurred_rounded_rect`, `push_layer`
- `vello_common-0.2.0/src/{paint.rs,filter_effects.rs,filter/}` (vendored, cargo registry)
- https://developer.mozilla.org/en-US/docs/Web/CSS/corner-shape
- https://developer.mozilla.org/en-US/docs/Web/CSS/basic-shape/shape
- https://developer.mozilla.org/en-US/docs/Web/CSS/CSS_anchor_positioning/Using
- https://developer.mozilla.org/en-US/docs/Web/CSS/CSS_scroll-driven_animations
- https://developer.mozilla.org/en-US/docs/Web/API/CSS_Painting_API
- https://drafts.fxtf.org/filter-effects/
- https://help.figma.com/hc/en-us/articles/360039957534-Boolean-operations
- https://www.figma.com/blog/desperately-seeking-squircles/
