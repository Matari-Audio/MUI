# Architecture

```text
Input (pointer, wheel, keys, text)
   |
   v  mui::Ui::frame        gestures + springs -> state mixed into fills
   |                        focus (press, Tab, Escape), wheel -> scroll offsets,
   |                        hover clock -> the tip that comes due,
   |                        capture edges -> Frame::edits (begin/end edit),
   |                        clipboard in and out
   |
   v  declared states       a node's .on(Hover | Press | Focus, ..) closures
   |                        run first, so the style the springs chase is the
   |                        one the node asked for in the state it is in
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
   |      per field, .on(State, ..) looks, roles, shells, welds, carves,
   |      canvas draws, .scroll() / .clip() / .float() / .pin(..))
   |
   v  mui-layout            intrinsic flex solve, frames in tree order; a
   |                        squeezed item is re-measured at its dealt share.
   |                        A `fits` node picks its candidate in that same
   |                        pass, and a `Pin` costs one more arrange over the
   |                        floats, fed the anchor frames the first one found
   |
   v  mui-scene walk         per node, in z-order:
   |     plain  -> RoundedRect(frame, radius)              a radius may be a
   |               theme token (selector / field / box); a squircle corner
   |               rewrites the rounded rect's arcs as cubics and gives up
   |               the analytic blur for that node
   |     weld   -> union(children's sharp frames) then fillet(convex, concave)
   |     carve  -> boolean(outline, a .cut/.keep child's shape, Difference |
   |               Intersection); the child is placed by layout and never paints
   |     shell  -> inset(previous outline, d)        exact or parallel offset
   |     text   -> shaped run from the TextCache      mui-text, kept across frames
   |               wrapped to the room its parent has, one Painted a line
   |     canvas -> the closure's own paths, in the node's space
   |     clip   -> Clip(outline) ... children ... Unclip
   |     blend  -> Blend(mix, opacity) ... subtree ... Unblend, outside the clip
   |     mask   -> Blend(Normal, 1) ... subtree ... Mask(fill, source-atop) ...
   |               Unblend; a paint over what the subtree drew, not an alpha mask
   |     weld   -> the shadow is one blurred rect per welded child
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
   |                        clip rect, so nothing responds where nothing is drawn
   |
   +--> mui-access          the same surfaces, each with the role and label it
   |                        declared, as an accesskit::TreeUpdate for the
   |                        host's adapter
   |
   v  mui-vello paint       Canvas: Gpu { scene, resources } over vello_hybrid,
                            Cpu { ctx, resources } over vello_cpu.
                            Fill / stroke / blurred rect / push_clip / pop_clip
                            / push_layer / pop_layer (blend mode + opacity),
                            and text as a hinted glyph run (a font blob is
                            interned by Arc pointer, so Vello's hinted-outline
                            cache survives the frame). paint_cached
                            keeps each Painted's arc-to-cubic conversion in a
                            PathCache keyed on a fingerprint of the path
                            itself, so a still frame re-encodes without
                            reconverting anything.
```

Every outline, weld rect and clip comes from one `bounds(frame, scale)`, and
every baseline from one `snap`, so `SceneSpec::device_scale` puts paint, hit
paths and clips on the same device grid or none of them.

Reflow is three declarations, not a breakpoint: `clamp(min, pct, max)` is a
length with two stops, `.min_col(px)` makes a grid's declared column count a
ceiling it drops from, and `.wrap()` breaks a line. They resolve in the same
single pass as everything else, so a 240x600 window and a 2000x300 one are the
same tree measured twice. A grid, or a paragraph, only fits against a width it
was offered -- but a flex share counts as one: a row deals its shares inside
the measure pass and measures a squeezed item again at the size it got, so
height-for-width resolves in the one solve. Only a hugging container, offered
no width at all, has nothing to fit against.

Layout answers **where content gets space**. Geometry answers **what shape
gets painted**. Input answers **what the pointer and the keyboard mean**, and
it answers it against the paths that were actually painted last frame, which
is why hit testing and clipping never disagree with the picture.

The invariants that matter: a shell is derived from the outline before it,
never from a guessed child radius; a preset merges per field and never
clobbers the chain around it, and no style is ever a string parsed at runtime;
a corner style rides the outline, so a
squircle stays inside the rounded rect it replaces; a weld unions sharp frames before it
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
   |
   v
mui                  Ui runtime, Frame, Edit, prelude, re-exports
   |                 (mui::core is a deprecated alias of mui::scene)
   +--> mui-widgets  controls and presets; reads state through `Host`
   +--> mui-vello    Canvas, paint, PathCache      +--> mui-access
   +--> mui-input    hit testing, gestures (over mui-vello's paths)
   |
   v
mui-scene            El DSL, Styled/Paints, Theme resolution, the scene walk
   |
   +--> mui-style    colours, roles, fills, shadows, Style, Theme (+ color)
   +--> mui-motion   Spring, curve (std only)
   +--> mui-text     glyph and string outlines      +--> mui-tessellate
   +--> mui-layout   the flex solve
         |
         v
      mui-geometry   Booleans, fillets, offsets, and the Spacing scale
```

`Spacing`/`SpacingScale` live in `mui-geometry` because both `mui-style` (a
shell's thickness, a theme's scale) and `mui-layout` (`gap`, `pad`) are
written in them; putting them in either would point an edge sideways.
`mui-egui` is an optional debug adapter off `mui-geometry`/`mui-tessellate`,
and `mui-truce` stands alone. `mui-widgets` dev-depends on `mui` so its
doctests can call a real `Ui`; the library graph stays one-way.
