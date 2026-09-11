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
                          |
                  mui-tessellate
                          |
                  generic TriangleMesh
                          |
             egui / future wgpu / Skia
```

Layout answers **where content gets space**. Geometry answers **what shape gets painted**. Interaction should remain attached to logical nodes even when multiple surfaces are visually merged.

The most important invariant is that a parallel child is generated from the parent's final outline. It never independently guesses a child radius.
