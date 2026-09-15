# Architecture

```text
El tree  (column / row / overlay / grid, Styled fills, roles, shells, welds)
   |
   v  mui::Ui::frame        gestures + springs -> state mixed into fills
   |
   v  mui-layout            intrinsic flex solve, frames in tree order
   |
   v  mui-core walk         per node, in z-order:
   |     plain  -> RoundedRect(frame, radius)
   |     weld   -> union(children's sharp frames) then fillet(convex, concave)
   |     shell  -> inset(previous outline, d)        exact or parallel offset
   |     text   -> glyph outline at the baseline     mui-text
   |     roles  -> Palette                            ink resolves on its ground
   |
   v  ResolvedScene         paint: Vec<Painted>  (shadow, fill, shells, stroke, text)
   |                        surfaces: key -> frame + path;  keys in z-order
   |
   +--> mui-input Hit       the same paths, pushed in paint order
   |
   v  mui-vello paint       Canvas: vello_hybrid (wgpu) or vello_cpu (pixmap)
```

Layout answers **where content gets space**. Geometry answers **what shape
gets painted**. The invariant that matters: a shell is derived from the
outline before it, never from a guessed child radius; a weld unions sharp
frames before it fillets. Everything above the paint list is
renderer-independent and wasm-clean; `mui-preview` is the only crate that
owns a window.
