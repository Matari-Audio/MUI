# Architecture

```text
Input (pointer with its Buttons and Mods, wheel, keys, text)
   |
   v  mui::Ui::frame        gestures + springs -> state mixed into fills
   |                        one capture per button, with the modifiers at the
   |                        press and now (Shift is the fine drag), the travel
   |                        since the press and the axis it favours,
   |                        focus (press, Tab, Escape), wheel -> scroll targets
   |                        that the drawn offsets spring to (0.12 s),
   |                        hover clock -> the tip that comes due,
   |                        capture edges -> Frame::edits (begin/end edit),
   |                        clipboard in and out. ui.keys(id) is focus-gated;
   |                        ui.shortcuts() is not, except by a typing field
   |
   v  declared states       a node's .on(Hover | Press | Focus | Disabled, ..)
   |                        closures run first, so the style the springs chase
   |                        is the one the node asked for in the state it is
   |                        in. .disabled(flag) is declared, not discovered,
   |                        and it flows down the subtree: the off look wins,
   |                        the hover spring is not mixed in, and the node
   |                        leaves the hit map, Tab and any gesture in flight
   |
   v  transitions           before anything is resolved: a node marked
   |                        .animate() has its declared fill, stroke width,
   |                        radius, text size, shadow blur and shell depths
   |                        pulled through springs keyed on its id, so the
   |                        tree the walk sees is already the interpolated
   |                        one. A new target retargets; nothing restarts.
   |                        ui.tween does the same for a number MUI cannot see.
   v
El tree  (row! / col! / stack! / grid! / fits!, Paints fills, presets merged
   |      per field, .on(State, ..) looks, roles, shells, unions, welds, carves,
   |      canvas draws, .scroll() / .clip() / .float() / .pin(..))
   |
   v  mui-layout            intrinsic flex solve, frames in tree order; a
   |                        squeezed item is re-measured at its dealt share.
   |                        A `fits` node picks its candidate in that same
   |                        pass, and a `Pin` costs one more arrange over the
   |                        floats, fed the anchor frames the first one found.
   |                        A `.sticky()` child is placed inside that same
   |                        arrange, against the enclosing scroll viewport
   |                        rather than an anchor node. The measure pass's
   |                        floor comes back as Layout::min_size, which is
   |                        the smallest window a host may offer
   |
   v  mui-scene walk         per node, in z-order:
   |     plain  -> RoundedRect(frame, radius)              a radius may be a
   |               theme token (selector / field / box); a squircle corner
   |               rewrites the rounded rect's arcs as cubics and gives up
   |               the analytic blur for that node
   |     union  -> union(children's sharp frames) then fillet(convex, concave)
   |     carve  -> boolean(outline, a .cut/.keep child's shape, Difference |
   |               Intersection); the child is placed by layout and never paints
   |     shell  -> inset(previous outline, d)        exact or parallel offset
   |     text   -> shaped run from the TextCache      mui-text, kept across frames
   |               wrapped to the room its parent has, one Painted a line,
   |               keyed on (string, size, weight) and carrying the face's
   |               normalized coords so the renderer draws the instance that
   |               was measured; .reserve(s) widens the box to hold s, and
   |               ResolvedScene::set_text then swaps one run's glyphs
   |               against the frame already solved -- no second walk
   |     canvas -> the closure's own paths, in the node's space; a Draw
   |               with a .tag becomes the surface's hit geometry instead of
   |               its outline, and Draw::hit is that geometry unpainted
   |     clip   -> Clip(outline) ... children ... Unclip
   |     blend  -> Blend(mix, opacity) ... subtree ... Unblend, outside the clip
   |     frost  -> Backdrop(outline, blur) first inside the blend: the renderer
   |               replays the list before it, clipped, through a blur layer
   |     mask   -> Blend(Normal, 1) ... subtree ... Mask(fill, source-atop) ...
   |               Unblend; a paint over what the subtree drew, not an alpha mask
   |     union  -> the shadow is one blurred rect per child
   |     shadow -> drop shadows under the fill, inset ones over the shells
   |               inside a Clip of the outline; both are one analytic
   |               blurred rounded rect, inverted for the inset case
   |     roles  -> Palette                            ink resolves on its ground
   |     image  -> Fill::Image                        straight RGBA, fitted to
   |                                                  the node's own outline
   |                                                  (vello_cpu: pixmap; vello_hybrid: atlas id,
   |                                                  no Atlas -> a grey stand-in)
   |
   v  ResolvedScene         paint: Vec<Painted>  (shadows, fill, shells, stroke,
   |                        text, draws, clip/unclip); floats are appended after
   |                        the root, so a tooltip or menu lands on top
   |                        surfaces: frame + path + clip + cursor + tip +
   |                        semantics, in paint order, with their keys
   |
   +--> mui-input Hit       the same paths, pushed in paint order with their
   |                        clip rect, so nothing responds where nothing is
   |                        drawn -- a canvas's tagged paths in place of its
   |                        outline, a disabled surface not at all
   |
   +--> mui-access          the same surfaces, each with the role and label it
   |                        declared, as an accesskit::TreeUpdate for the
   |                        host's adapter; a disabled one says so and offers
   |                        no Focus action
   |
   v  mui-vello paint       Canvas: Gpu { scene, resources, cache, atlas } over
                            vello_hybrid, Cpu { ctx, resources, cache } over
                            vello_cpu.
                            Fill / stroke / blurred rect / push_clip / pop_clip
                            / push_layer / pop_layer (blend mode + opacity),
                            and text as a hinted glyph run (a font blob is
                            interned by Font id in the renderer's Cache, so
                            Vello's hinted-outline cache survives the frame).
                            Paths convert arc-to-cubic every frame; caching
                            that saved ~0.08 ms and was deleted.
```

Every outline, weld rect and clip comes from one `bounds(frame, scale)`, and
every baseline from one `snap`, so `SceneSpec::device_scale` puts paint, hit
paths and clips on the same device grid or none of them.

### The scale contract

A host hands MUI a device scale (`Ui::scale`, `SceneSpec::device_scale`) and a
size in *logical* units. Three rules follow, and `crates/mui/examples/snapshot.rs`
renders the gallery at 1x, 1.5x and 2x to hold them:

1. **Layout is scale-free.** Every length -- `gap`, `pad`, a font size, a knob's
   radius -- is logical, at every scale. The resolved frames at 2x are the
   frames at 1x, to the bit. Nothing in the tree multiplies by the scale, so
   there is no scale for a widget to forget to apply.
2. **The scale is a snapping grid.** `device_scale` moves each painted edge and
   baseline onto the nearest device pixel, so abutting fills composite opaque.
   A snapped edge is therefore within **half a device pixel** of the layout
   edge at every scale: `painted * scale` never drifts from `frame * scale`.
3. **The scale is a paint transform.** The renderer draws the logical scene
   under `Affine::scale(s)` into an `s`-times-larger target. Glyphs and curves
   are rasterised from outlines through that transform, so 2x is a 2x
   rendering, not an upscaled 1x.

A host that instead multiplies its lengths by the scale breaks (1), and the
snapshot's own tests fail: that is the whole point of running the gallery at a
fractional scale as well as an integer one.

Reflow is three declarations, not a breakpoint: `clamp(min, pct, max)` is a
length with two stops, `.min_col(px)` makes a grid's declared column count a
ceiling it drops from, and `.wrap()` breaks a line. They resolve in the same
single pass as everything else, so a 240x600 window and a 2000x300 one are the
same tree measured twice. A grid, or a paragraph, only fits against a width it
was offered -- but a flex share counts as one: a row deals its shares inside
the measure pass and measures a squeezed item again at the size it got, so
height-for-width resolves in the one solve. Only a hugging container, offered
no width at all, has nothing to fit against -- it still honours the floors it
was given there: a hugging grid keeps its declared column count and widens
itself to `.min_col(px)` instead of squeezing a column under it.

Layout answers **where content gets space**. Geometry answers **what shape
gets painted**. Input answers **what the pointer and the keyboard mean**, and
it answers it against the paths that were actually painted last frame, which
is why hit testing and clipping never disagree with the picture.

The invariants that matter: a shell is derived from the outline before it,
never from a guessed child radius; a preset merges per field and never
clobbers the chain around it, and no style is ever a string parsed at runtime;
a corner style rides the outline, so a
squircle stays inside the rounded rect it replaces; what responds is what was
drawn, which is why a tagged canvas answers in its ring and not in its hole,
and why switching a node off is one call and not a grey fill beside a live
gesture; a `.union` joins sharp frames before it
fillets; a clip is a layer pair in the paint list, not a state flag, so a
renderer that ignores it still draws something sane; a float keeps its
declaration slot in frame order but its paint slot at the end. Everything
above the paint list is renderer-independent and wasm-clean; `mui-preview` is
the only crate that owns a window.

## Crates

The pipeline above is cut into crates along the same lines. Every edge points
down; nothing below knows what is above it.

```text
mui-preview          window, wgpu surface, the gallery as one tree
mui-truce            plugin editor: baseview child window + wgpu (`window`),
   |                 truce `Editor` + parameter `Bridge` (over truce)
   v
mui                  Ui runtime, Frame, Edit, prelude, re-exports, and
   |                 the controls and presets (`mui::widgets`), which
   |                 read last frame's state straight off `&mut Ui`
   +--> mui-vello    Canvas, paint, Cache          +--> mui-access
   +--> mui-input    hit testing, gestures (over mui-vello's paths)
   |
   v
mui-scene            El DSL, Styled/Paints, Theme resolution, the scene walk
   |
   +--> mui-weld     material welds: plates' paint blended, baked or on the GPU
   +--> mui-style    colours, roles, fills, shadows, Style, Theme (+ color)
   +--> mui-motion   Spring, curve (std only)
   +--> mui-text     glyph and string outlines
   +--> mui-layout   the flex solve
         |
         v
      mui-geometry   Booleans, fillets, offsets, and the Spacing scale
```

`Spacing`/`SpacingScale` live in `mui-geometry` because both `mui-style` (a
shell's thickness, a theme's scale) and `mui-layout` (`gap`, `pad`) are
written in them; putting them in either would point an edge sideways.
`mui-playground` is the browser playground's DSL, straight over `mui-scene` and
`mui-vello`.

`mui-truce` is two halves. `window` knows no plugin framework: a `View`
trait (build a tree, report outside changes, ask for a size), the native
event queue, and the GPU surface painted with `HybridEffects`. `MuiEditor`
and `Bridge` are the truce half: a `View` whose model is truce's parameter
store, and truce's `Editor` around the window. An adapter for another
framework (nih-plug, say) is another `View` plus that framework's editor
trait; it does not touch `window`. `examples/gain-plugin` is the consumer.
