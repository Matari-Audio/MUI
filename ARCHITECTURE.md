# Architecture

```text
Rust DSL ------------------┐
                          v
TypeScript -> generated -> SceneSpec
                          |
                          v
                    mui-layout
                 intrinsic frames
                          |
                          v
                    mui-core
             surface dependency graph
            /             |             \
        frame           merge          inset/outset
   sharp basis +      boolean first     final-path offset
   render radius      fillet second
            \             |             /
                          v
                         Path
                       /      \
                      /        \
             mui-vello       mui-tessellate
                 |                  |
          vello_hybrid        TriangleMesh
                 |                  |
                wgpu          egui/debug meshes
```

Layout answers **where content gets space**. Geometry answers **what shape gets painted**. Interaction should remain attached to logical nodes even when multiple surfaces are visually merged.

`mui-vello` is the current path-to-pixels seam used by the preview and headless
examples. `mui-tessellate` is the renderer-independent mesh branch used by the
egui/debug adapter and other mesh consumers. The `mui-egui` low-level `prepare` helper
starts from `PlacedShape` inputs rather than consuming a resolved `SceneSpec`.

The most important invariant is that a parallel child is generated from the parent's final outline. It never independently guesses a child radius.
