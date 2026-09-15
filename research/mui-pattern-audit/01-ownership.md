# MUI ownership / borrowing / lifetime audit

Review date: 2026-09-13. Scope: all indexed Rust crates in the local MUI workspace, with emphasis on scene/font ownership, geometry/layout borrows, cache ownership, and references crossing frame boundaries. No source changes were made.

## Findings

### P3 — `GlyphAxes` uses shared ownership without a sharing consumer

Evidence: [`GlyphAxes::font`](</mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-preview/src/scenes.rs:154>) is created as `Arc<Vec<u8>>` in [`GlyphAxes::new`](</mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-preview/src/scenes.rs:165>) and is only borrowed by `overlay` at line 246. The exhaustive `self.font` search found no clone or other owner for this field. `mui_text::glyph_path` accepts `&[u8]`, and `PreviewScene` has no `Send`/`Sync` requirement that would justify an atomic shared pointer.

Trigger and impact: every glyph scene allocates an `Arc` control block and pays atomic reference-counting costs even though the scene owns the only copy. This is a small clarity and overhead issue, not a correctness or lifetime bug; it does not duplicate the font bytes.

Minimal fix: store `Vec<u8>` in `GlyphAxes` and initialize it directly. Keep the borrow at the `glyph_path` call. Confidence: high. This is a style/ownership refinement only and can wait until the scene’s storage is otherwise touched.

No P0–P2 ownership, borrowing, or lifetime correctness defects were found in the inspected code. The remaining patterns are sound and generally explain their ownership boundaries in comments or types.

## Strong patterns

The preview app deliberately owns long-lived resources. [`App`](</mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-preview/src/main.rs:157>) owns `Vec<Box<dyn PreviewScene>>`, so heterogeneous scenes have stable ownership and no borrowed scene can outlive the app. Its `Arc<Vec<u8>>` font is justified: `App` keeps one owner while [`Chrome::new`](</mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-preview/src/ui.rs:103>) receives a cloned `Arc`; the bytes are shared without copying. [`Baked::build`](</mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-preview/src/main.rs:73>) borrows the selected scene and font only while deriving an owned resolved scene, paths, hit targets, and labels. That makes rebaking independent of the source borrow.

The GPU host uses the same ownership rule for the window. [`Gpu`](</mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-preview/src/host.rs:27>) stores `Arc<Window>`, passes a clone when constructing the surface, and retains the original `Arc`. This is the required lifetime shape for the stored `wgpu::Surface<'static>`: the surface owns a window handle while `Gpu` also exposes the window for resize and redraw operations. The pointer is not an ornamental lifetime workaround.

Geometry APIs borrow inputs and return owned results. [`Hit::push`](</mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-input/src/lib.rs:53>) accepts `&Path`, converts it immediately to an owned Bézier path, and stores an owned `String` identifier. [`mui_vello::bez_path`](</mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-vello/src/lib.rs:27>) follows the same rule. The caller may drop or mutate its input after the call without invalidating hit data. [`Hit::at`](</mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-input/src/lib.rs:74>) returns a short-lived `&str`; [`Interaction::update`](</mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-input/src/lib.rs:149>) immediately converts it to an owned `String` before capturing a target across frames. The borrow checker prevents a caller from mutating `Hit` while holding the returned reference, and the interaction state does not rely on that borrow surviving.

The layout and scene resolvers use lifetimes for a real temporary relationship. [`Measured<'a>` and `measure`](</mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-layout/src/lib.rs:340>) borrow the input `Node` tree while computing sizes, but `resolve` returns an owned `Layout`; no `Node` reference escapes the operation. [`Resolver<'a>`](</mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-core/src/scene.rs:237>) borrows `SceneSpec` and `Layout` while its `BTreeMap<String, ResolvedSurface>` owns derived geometry. `ResolvedScene::surface` and `surfaces` return references tied to `&self`, which is the right choice for read-only access to potentially large geometry.

The UI frame has a particularly good ownership boundary. [`Ui<'a>`](</mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-preview/src/ui.rs:189>) holds `&'a mut Chrome` while owning its frame paint list and next-frame hit list. [`Ui::finish`](</mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-preview/src/ui.rs:669>) consumes `self` before committing `next_hit` back to `Chrome`; this prevents widgets from being added after hit geometry is finalized and avoids aliasing `Chrome` during widget construction. Public controls borrow only the value they edit (`&mut f32`, `&mut bool`, `&mut char`) and borrow labels as `&str`.

`PathMeshCache::prepare` in [`mui-egui`](</mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-egui/src/lib.rs:254>) also has the correct cache shape: it borrows the candidate `Path`, clones it only on a cache miss because the cache must own its comparison key, and returns `&TriangleMesh` tied to the cache. `SurfaceState::commit`, [`LayoutState::commit`](</mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-layout/src/lib.rs:699>), and [`SceneState::commit`](</mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-core/src/scene.rs:479>) build an owned next snapshot before replacing the current one, so failures preserve the previously published value.

Font and text APIs are similarly clean: [`glyph_path`](</mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-text/src/lib.rs:92>) and `text_run` borrow font bytes, text, and axis tags only during conversion and return owned geometry. `Axis<'a>` correctly expresses that axis tags are borrowed for the call. The preview’s `run` helper creates a local owned truncation only when needed and passes its slice within that call; the local cannot escape.

## Ownership costs reviewed but not defects

Several clones are required by ownership boundaries. `Baked::build` clones IDs and resolved paths because it retains independent paint and hit-test data while the resolved scene remains available for frames. `Resolver::one` clones cached `ResolvedSurface` values because recursive resolution returns owned values while the resolver also retains them in its cache. `Interaction::update` clones IDs into independent edge-triggered slots (`pressed`, `clicked`, `released`) and the captured slot. `Mesh` construction clones indices because egui takes ownership of its mesh. These may be performance topics for a separate profile-driven review; changing them to shared pointers would add complexity and was not justified by this ownership audit.

## Coverage and limitations

Graft’s map covered 37 indexed files and 731 Rust/TypeScript symbols. I queried ownership-related occurrences across all indexed files for `Arc`, `Box`, `clone()`, explicit lifetime declarations, `&mut`, borrowed `Path` APIs, reference-returning APIs, and `mem::take`, then opened the returned exact spans and traced the major frame/build paths. `cargo test --workspace` passed: all non-ignored tests passed; two measurement tests and one GPU test remained ignored by their declarations. This was a static ownership review plus the workspace test suite, not a benchmark, sanitizer run, Miri run, or inspection of unindexed generated/build artifacts. The lone P3 finding should be treated as optional cleanup rather than a release blocker.
