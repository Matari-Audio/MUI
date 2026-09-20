# Adaptive full-window rendering

The opt-in `TiledEffects` renderer can raster the window once when more than half its tiles are dirty, then copy the guarded regions into its persistent tile textures. This removes repeated scene encoding and GPU submissions for the initial frame, resize and full invalidation. The majority threshold also covers widespread animation. Sparse updates continue to raster only dirty tiles.

The additional full-window texture is admitted only when the tile textures plus that target fit the caller's existing `tile_bytes` budget and device limits. Single-tile viewports need no additional target. A smaller budget uses the previous per-tile algorithm, which also supplies the benchmark reference.

`TileStats::full_redraw` reports selection of the new path. `tile_submissions` includes the one render-and-copy submission when it is selected. The dirty counters describe requested damage; `full_redraw` indicates that the whole cache was refreshed. Presentation remains a separate submission. Both paths keep identical guard-band geometry, premultiplied texture encoding and paint order. There are no new CPU/GPU waits in the renderer.

Validation compares the whole-scene reference, a tiled renderer with the full target, and a renderer whose budget permits tiles alone. It checks scales 1/1.5/2, translucent groups, text, animated effects, unchanged frames, local updates, forced full invalidation, and resize through 1×1 and non-multiple-of-256 dimensions.

The benchmark's `tiles per-tile` and `tiles adaptive` rows use the same application scene and GPU. For these rows `encode` measures host rendering/submission work and `render` is the completion wait; compare actual `total` values rather than interpreting their phase names as GPU-only timings. Timings are headless completion latency, not KURV presented FPS.

Build-storage maintenance: old task binaries were archived under `/home/derpcat/projects/mui-research-binary-archive`. Cargo's download cache and the perf example-output directory were relocated there with symlinks preserving their original paths after the shared Windows-mounted disk filled. No downloaded crates or unrelated project files were deleted.
