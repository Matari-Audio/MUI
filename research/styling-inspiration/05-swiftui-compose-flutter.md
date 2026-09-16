# Declarative native UI DSLs: SwiftUI, Jetpack Compose, Flutter

What three production declarative toolkits do about the two questions MUI's
`El` chain has to answer: **does the order of `.pad().fill().clip()` mean
anything**, and **how does a user escape into their own painting**.

---

## 1. What each system actually is

**SwiftUI.** A view is a value. Every modifier returns a *new wrapper type* —
`Text("x").padding()` is `ModifiedContent<Text, _PaddingLayout>`. The chain is
a type-level onion built bottom-up, so `.padding().background(.blue)` means
"pad the text, then paint blue behind the *padded* thing". Layout is a
negotiation: parent proposes a size, child replies with the size it wants,
parent places it. `Layout`, `ViewThatFits` and `GeometryReader` are the three
escape hatches into that negotiation, `Canvas`/`Shape`/`ShapeStyle` the escape
hatch into drawing.

**Compose.** A `Modifier` is a *list*, not an onion, but it behaves like one:
`Modifier.padding(8.dp).background(Cyan)` and `.background(Cyan).padding(8.dp)`
draw differently, and the docs say so out loud — Compose deliberately dropped
`margin` because "you have ordering, that's enough". Since 1.3 the recommended
way to write your own is `Modifier.Node` + `ModifierNodeElement`: a cheap
immutable *element* (the value in the chain) that creates/updates a long-lived
*node* (the thing that measures and draws). Layout escape hatches: `Layout {}`
with a `MeasurePolicy`, `BoxWithConstraints`, and overridable intrinsics.

**Flutter.** No modifiers at all — everything is a widget wrapping another
widget. `Padding(child: DecoratedBox(child: ...))`. Order is literal nesting,
so it's never ambiguous and always verbose. Escape hatches are widgets too:
`LayoutBuilder` (constraints → subtree), `CustomPaint`/`CustomPainter` (a
`Canvas` and a `Size`), `ShaderMask` (a shader + a `BlendMode` applied to the
child's raster), and `FragmentProgram` for user GLSL.

---

## 2. The exact syntax, six examples

**SwiftUI — order is the whole API:**

```swift
Text("Cutoff")
    .padding(8)                  // inset the text
    .background(.thinMaterial)   // paint behind the padded box
    .clipShape(.rect(cornerRadius: 12))
    .padding(4)                  // outer gap, OUTSIDE the fill = a margin
```

**SwiftUI — a named bundle of modifiers (`ViewModifier` + extension):**

```swift
struct BorderedCaption: ViewModifier {
    func body(content: Content) -> some View {
        content.font(.caption2).padding(10)
            .overlay(RoundedRectangle(cornerRadius: 15).stroke(lineWidth: 1))
    }
}
extension View { func borderedCaption() -> some View { modifier(BorderedCaption()) } }

Text("Downtown Bus").borderedCaption()
```

**SwiftUI — `ViewThatFits`: candidates in order of preference, first one that fits wins:**

```swift
ViewThatFits(in: .horizontal) {
    HStack { Text(pct); ProgressView(value: p).frame(width: 100) }  // best
    ProgressView(value: p).frame(width: 100)                         // ok
    Text(pct)                                                        // last resort
}
```

**Compose — the same padding/background trick, both sides of one chain:**

```kotlin
Text("Hello", Modifier
    .padding(80.dp)              // outer padding, outside the background
    .background(Color.Cyan)      // the fill
    .padding(16.dp))             // inner padding, inside the fill
```

Same principle for hit-testing: `.clickable().padding()` makes the padding
clickable; `.padding().clickable()` does not.

**Compose — a custom modifier the modern way, node + element:**

```kotlin
private class DrawWithContentModifier(var onDraw: ContentDrawScope.() -> Unit)
    : Modifier.Node(), DrawModifierNode {
    override fun ContentDrawScope.draw() { onDraw() }   // calls drawContent() where it wants
}
private class DrawWithContentElement(val onDraw: ContentDrawScope.() -> Unit)
    : ModifierNodeElement<DrawWithContentModifier>() {
    override fun create() = DrawWithContentModifier(onDraw)
    override fun update(node: DrawWithContentModifier) { node.onDraw = onDraw }
}
fun Modifier.drawWithContent(onDraw: ContentDrawScope.() -> Unit) = this then DrawWithContentElement(onDraw)
```

The load-bearing detail: the *element* is a cheap data class compared by
`equals`, the *node* is the retained object. Recompose swaps values, not objects.

**Flutter — painter and shader, both as plain widgets:**

```dart
class Needle extends CustomPainter {
  @override void paint(Canvas c, Size size) { c.drawCircle(Offset(100, 200), 40, Paint()..color = amber); }
  @override bool shouldRepaint(Needle old) => false;   // explicit repaint gate
}
// ...
ShaderMask(
  blendMode: BlendMode.srcATop,
  shaderCallback: (bounds) => _shimmerGradient.createShader(bounds),
  child: child,
)
// user GLSL:
final shader = program.fragmentShader()..setFloat(0, 42.0);
canvas.drawRect(rect, Paint()..shader = shader);
```

---

## 3. Worth stealing, in the order I'd do it

### 3.1 `fits![..]` — `ViewThatFits`, and it's a layout-only change

This is the one. Plugin editors get resized by the host to whatever the DAW
feels like, and today MUI's only answers are `clamp()` and `.wrap()`. A
container that measures candidates against the proposed size and keeps the
first that fits is ~30 lines in `mui-layout` (it already measures children to
decide `grow`/`shrink`) and it kills every "if width < 400 use the compact
tree" branch a plugin will otherwise grow.

```rust
// candidates largest-first; the first whose measured size fits the parent's
// offer survives, the rest are dropped before paint.
fits![
    row![knob("drive"), label("Drive"), value_field("drive")].gap(M),
    col![knob("drive"), caption("Drive")].center(),
    knob("drive"),
]
.axis(Axis::X)      // default: both axes
```

`Node::fits(children)` sits beside `Node::row/column/overlay/grid`. Unlike
`BoxWithConstraints` it needs no second build pass — the candidates are already
built, the layout pass just picks one. That is the entire reason to prefer it.

### 3.2 `.behind(..)` / `.over(..)` — SwiftUI's `background(content:)`, not an ordered modifier chain

MUI's `Style` is a flat struct: one fill, one stroke, one radius, one shadow.
`.fill(a).fill(b)` is last-wins and order is meaningless, which is *good* and I
would not trade it. But one fill per node is genuinely limiting (a gradient
over a fill over a noise texture is three nested `stack![]` today, and the
inner node has to be told to stretch).

Steal SwiftUI's `.background`/`.overlay` instead of Compose's ordered chain:
a child pinned to *this* node's resolved frame, painted under or over it,
taking no part in sizing.

```rust
knob_body()
    .fill(Role::Field)
    .behind(canvas(|s| vec![Draw::fill(noise(s), Role::Level(-1))]))
    .over(text("12 dB").fill(Role::Dim))
    .radius(8.)
```

Cheap: it's an `El` stored on `Element` (or two), laid out against the parent's
own frame in the same walk that already handles `.float()`.

### 3.3 `.paint(|size, ctx| Vec<Draw>)` as a *layer hook*, not a node

`canvas()` today is Flutter's `CustomPainter` exactly — a size in, draws out.
What's missing is Compose's `drawBehind` / `drawWithContent`: painting attached
to an existing node rather than needing a node of its own. Combined with 3.2:

```rust
El::paint_over(self, f: impl Fn(Size) -> Vec<Draw> + 'static)   // after the fill and children
El::paint_under(self, f: ...)                                   // between the shadow and the fill
```

And the piece Vello makes nearly free that nobody else in MUI exposes yet —
`ShaderMask`. Vello has blend layers; `srcATop` a fill through the child's
own alpha is a `push_layer` with `Mix::Normal, Compose::SrcAtop`:

```rust
icon_row().mask(Gradient::linear(90., [(0.0, Role::Ink), (1.0, Color::TRANSPARENT)]))
```

That is scroll-edge fades, shimmer-loading, and gradient-tinted glyphs for one
method and one blend layer. Do this before anything resembling user GLSL.

### 3.4 `.apply(f)` — `ViewModifier`, except Rust already has it for free

SwiftUI needs a `ViewModifier` protocol *because* modifiers are types. In Rust
a reusable bundle of chain calls is `fn(El) -> El`. One method makes it read in
chain position instead of wrapping:

```rust
fn card(el: El) -> El { el.pad(M).fill(Role::Raised).radius(10.).shadow(Shadow::soft(12.)) }
fn danger(el: El) -> El { el.fill(Role::Danger).stroke(Role::Ink) }

col![title("Filter"), curve].apply(card).apply_if(bypassed, danger)
```

`fn apply(self, f: impl FnOnce(El) -> El) -> El { f(self) }`. Three lines,
and it is the whole of "presets/variants" — no preset registry, no style
struct, no theme-level component slots. Put `card`, `field`, `chip` in a
`mui::presets` module as plain functions and let plugins write their own.

### 3.5 Compose's element/node split, but only as a caching rule

Don't build `Modifier.Node`. Do steal its *invariant*: the thing in the chain
is a cheap value compared by `==`, the expensive thing lives across frames keyed
by identity. MUI already half-does this — `Canvas` compares by `Arc::ptr_eq`,
`Image` by buffer pointer. Extend the rule to the new hooks: a `.paint_over`
closure compares by `Arc` pointer, so a scene that rebuilds the tree every frame
with the same `Arc` skips re-tessellation. That's the difference between "MUI
rebuilds the tree every frame" being fine and being a Vello path-rebuild storm.

---

## 4. What not to copy

**Compose/SwiftUI's order-dependent style chain.** `.pad(8).fill(X)` vs
`.fill(X).pad(8)` meaning different things is the single most-asked question
about both frameworks, and it exists to avoid having a `margin`. MUI's flat
`Style` is order-free and that property is worth more than the saved keyword.
If MUI needs outer space, add `.margin(..)` and be done. `.shell()` and
`.weld()` are the two places order *does* matter in MUI today, and they earn it
because they are geometric operations on the preceding outline — that's a
deliberate exception, not a precedent.

**SwiftUI's type-level `ModifiedContent<A, B>` onion.** In Rust this becomes
`Pad<Fill<Clip<Text>>>`: monomorphization blowup, no `Vec<El>`, no `dyn`, no
storing a tree in a struct, compile times that make a preview gallery painful.
`El = Node<Element>` — one concrete type — is why `row![]` can take a `Vec`
and why `scenes.rs` can be `Vec<Box<dyn PreviewScene>>`. Never give that up.

**Flutter's nesting-as-composition.** `Padding(child: Container(child: ...))`
is the thing MUI's builder chain exists to avoid. Steal Flutter's *hooks*
(`CustomPainter`, `ShaderMask`), never its shape.

**Compose intrinsics (`minIntrinsicWidth` et al.) and SwiftUI's proposal
negotiation.** Each intrinsic query costs an extra measure pass over the
subtree, and both systems' layout algorithms are famously hard to predict
(SwiftUI's "ideal size" is unspecified). MUI's flexbox+grid model is documented
CSS semantics that a plugin author already knows. `fits![]` covers the adaptive
case intrinsics are usually reached for; stop there.

**`GeometryReader` / `BoxWithConstraints` / `LayoutBuilder` as a general
primitive.** Building children *from* the resolved size means layout → build →
layout, a second pass every frame plus a re-entrancy hazard (SwiftUI's
`GeometryReader` is the #1 source of "my layout collapsed"). If a scene truly
needs it, the honest shape is a *canvas* — MUI's `canvas(|size| ..)` already
hands you the resolved size for drawing, which is 90% of what people use
`LayoutBuilder` for.

**A `Layout` protocol / `MeasurePolicy` trait.** One `dyn Fn(&[Size]) -> Vec<Rect>`
seam sounds cheap and then it has to grow intrinsics, caching, baselines and
directionality to be usable. Row, column, overlay, grid, wrap and `fits` cover
audio-plugin editors. Add the seam when a real scene can't be written, not before.

**Flutter's `FragmentProgram` asset pipeline.** GLSL files declared in
`pubspec.yaml`, compiled at build time, uniforms set by *integer index*
(`setFloat(0, 42.0)` — no names, positional). Vello's own encoding gives MUI
gradients, blurs, blend modes and clips without a shader compiler in the build.
Expose `.mask(..)` and blend modes; leave user GLSL alone until a plugin
author has an effect that genuinely can't be expressed as paths + blends.

**Compose's `graphicsLayer` grab bag.** One modifier carrying alpha, scale,
rotation, shadow elevation, clip, blend and render effect is a discoverability
sink. MUI's one-method-one-concept naming (`.shadow`, `.clip`, `.radius`) reads
better in a table.

---

## 5. Sources

- SwiftUI `ViewThatFits` — https://developer.apple.com/documentation/swiftui/viewthatfits
- SwiftUI `Layout` protocol (`sizeThatFits`/`placeSubviews`) — https://developer.apple.com/documentation/swiftui/layout
- SwiftUI composing custom layouts — https://developer.apple.com/documentation/swiftui/composing-custom-layouts-with-swiftui
- SwiftUI `ViewModifier` / reducing modifier maintenance — https://developer.apple.com/documentation/swiftui/reducing-view-modifier-maintenance
- SwiftUI `View.modifier(_:)` — https://developer.apple.com/documentation/swiftui/view/modifier%28_%3A%29
- SwiftUI `padding(_:)` ordering discussion — https://developer.apple.com/documentation/swiftui/view/padding%28_%3A%29-6pgqq
- SwiftUI `background(alignment:content:)` — https://developer.apple.com/documentation/swiftui/view/background%28alignment%3Acontent%3A%29
- Compose modifiers, "order of modifiers matters" — https://developer.android.com/develop/ui/compose/modifiers
- Compose constraints and modifiers — https://developer.android.com/develop/ui/compose/layouts/constraints-modifiers
- Compose intrinsic measurements — https://developer.android.com/develop/ui/compose/layouts/intrinsic-measurements
- Compose layout basics / `BoxWithConstraints` — https://developer.android.com/develop/ui/compose/layouts/basics
- `ModifierNodeElement` source + `DrawWithContentElement` — https://github.com/aldefy/compose-skill/blob/master/skills/compose-expert/references/source-code/ui-source.md
- Flutter fragment shaders — https://github.com/flutter/website/blob/main/sites/docs/src/content/ui/design/graphics/fragment-shaders.md
- Flutter `ShaderMask` shimmer cookbook — https://github.com/flutter/website/blob/main/sites/docs/src/content/cookbook/effects/shimmer-loading.md
- Flutter `CustomPaint`/`CustomPainter` — https://github.com/flutter/website/blob/main/sites/docs/src/content/flutter-for/react-native-devs.md
