# Architecture

```text
Input (pointer, wheel, keys, text)
   |
   v  mui::Ui::frame        gestures + springs -> state mixed into fills
   |                        focus (press, Tab, Escape), wheel -> scroll offsets,
   |                        hover clock -> the tip that comes due
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
   |     canvas -> the closure's own paths, in the node's space
   |     clip   -> Clip(outline) ... children ... Unclip
   |     roles  -> Palette                            ink resolves on its ground
   |
   v  ResolvedScene         paint: Vec<Painted>  (shadow, fill, shells, stroke,
   |                        text, draws, clip/unclip); floats are appended after
   |                        the root, so a tooltip or menu lands on top
   |                        surfaces: key -> frame + path + clip + cursor + tip
   |
   +--> mui-input Hit       the same paths, pushed in paint order with their
   |                        clip rect, so nothing responds where nothing is drawn
   |
   v  mui-vello paint       Canvas: Gpu { scene, resources } over vello_hybrid,
                            Cpu { ctx, resources } over vello_cpu.
                            Fill / stroke / blurred rect / push_clip / pop_clip,
                            and text as a hinted glyph run through Vello's atlas
                            (a font blob is interned by Arc pointer so the
                            atlas is not rebuilt every frame).
```

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
