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
   v  transitions           before anything is resolved: a node marked
   |                        .animate() has its declared fill, stroke width,
   |                        radius, text size, shadow blur and shell depths
   |                        pulled through springs keyed on its id, so the
   |                        tree the walk sees is already the interpolated
   |                        one. A new target retargets; nothing restarts.
   |                        ui.tween does the same for a number MUI cannot see.
   v
El tree  (row! / col! / stack! / grid!, Styled fills, roles, shells, welds,
   |      canvas draws, .scroll() / .clip() / .float())
   |
   v  mui-layout            intrinsic flex solve, frames in tree order
   |
   v  mui-core walk         per node, in z-order:
   |     plain  -> RoundedRect(frame, radius)
   |     weld   -> union(children's sharp frames) then fillet(convex, concave)
   |     shell  -> inset(previous outline, d)        exact or parallel offset
   |     text   -> shaped run from the TextCache      mui-text, kept across frames
   |               wrapped to the room its parent has, one Painted a line
   |     canvas -> the closure's own paths, in the node's space
   |     clip   -> Clip(outline) ... children ... Unclip
   |     blend  -> Blend(mix, opacity) ... subtree ... Unblend, outside the clip
   |     weld   -> the shadow is one blurred rect per welded child
   |     roles  -> Palette                            ink resolves on its ground
   |     image  -> Fill::Image                        straight RGBA, fitted to
   |                                                  the node's own outline
   |                                                  (vello_cpu: pixmap; vello_hybrid: atlas id,
   |                                                  no Atlas -> a grey stand-in)
   |
   v  ResolvedScene         paint: Vec<Painted>  (shadow, fill, shells, stroke,
   |                        text, draws, clip/unclip); floats are appended after
   |                        the root, so a tooltip or menu lands on top
   |                        surfaces: frame + path + clip + cursor + tip, in
   |                        paint order, with a key index beside them
   |
   +--> mui-input Hit       the same paths, pushed in paint order with their
   |                        clip rect, so nothing responds where nothing is drawn
   |
   +--> mui-access          the same surfaces plus a Semantics map as an
   |                        accesskit::TreeUpdate, for the host's adapter
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
same tree measured twice. A grid only auto-fits against a width it was
offered: inside a flex item, along the main axis, the share is not dealt until
`arrange` and the grid keeps its declared count.

Layout answers **where content gets space**. Geometry answers **what shape
gets painted**. Input answers **what the pointer and the keyboard mean**, and
it answers it against the paths that were actually painted last frame, which
is why hit testing and clipping never disagree with the picture.

The invariants that matter: a shell is derived from the outline before it,
never from a guessed child radius; a weld unions sharp frames before it
fillets; a clip is a layer pair in the paint list, not a state flag, so a
renderer that ignores it still draws something sane; a float keeps its
declaration slot in frame order but its paint slot at the end. Everything
above the paint list is renderer-independent and wasm-clean; `mui-preview` is
the only crate that owns a window.
