# Adaptive full-window rendering

The opt-in `TiledEffects` renderer can raster the window once when more than half its tiles are dirty, then copy the guarded regions into its persistent tile textures. This removes repeated scene encoding and GPU submissions for the initial frame, resize and full invalidation. The majority threshold also covers widespread animation. Sparse updates continue to raster only dirty tiles.

The additional full-window texture is admitted only when the tile textures plus that target fit the caller's existing `tile_bytes` budget and device limits. Single-tile viewports need no additional target. A smaller budget uses the previous per-tile algorithm, which also supplies the benchmark reference.

`TileStats::full_redraw` reports selection of the new path. `tile_submissions` includes the one render-and-copy submission when it is selected. The dirty counters describe requested damage; `full_redraw` indicates that the whole cache was refreshed. Presentation remains a separate submission. Both paths keep identical guard-band geometry, premultiplied texture encoding and paint order. There are no new CPU/GPU waits in the renderer.

Validation compares the whole-scene reference, a tiled renderer with the full target, and a renderer whose budget permits tiles alone. It checks scales 1/1.5/2, translucent groups, text, animated effects, unchanged frames, local updates, forced full invalidation, and resize through 1×1 and non-multiple-of-256 dimensions.

The benchmark's `tiles per-tile` and `tiles adaptive` rows use the same application scene and GPU. For these rows `encode` measures host rendering/submission work and `render` is the completion wait; compare actual `total` values rather than interpreting their phase names as GPU-only timings. Timings are headless completion latency, not KURV presented FPS.

Build-storage maintenance: old task binaries were archived under `/home/derpcat/projects/mui-research-binary-archive`. Cargo's download cache and the perf example-output directory were relocated there with symlinks preserving their original paths after the shared Windows-mounted disk filled. No downloaded crates or unrelated project files were deleted.

## Reproduced measurements

Ryzen 7 7800X3D and RX 6600 (RADV Vulkan), 1280×800. Median of three run medians, each five warm-up and 50 measured frames. Both paths run in the same executable with identical scene content; the tile-only path runs first each time. Other desktop activity and GPU scheduling can affect the small timings. These are development-desktop results, not validation on the target weak laptop.

| Fixture/case | Per-tile total (ms) | Adaptive total (ms) | Speedup |
|---|---:|---:|---:|
| text: cold | 33.594 | 13.971 | 2.40× |
| text: static | 2.503 | 2.541 | 0.99× |
| text: one knob turning | 4.132 | 4.416 | 0.94× |
| vectors: all curves moving | 28.847 | 18.571 | 1.55× |
| vectors: static | 0.632 | 0.627 | 1.01× |

Full redraws reduced tile-render submissions from 1,100 to 55 across 55 frames (20× fewer); final presentation submissions are separate. Sparse-case differences remain visible in the table rather than being rounded into an improvement claim. The principal gain is avoiding repeated encoding on widespread damage. Classic compute Vello remains substantially faster for the synthetic dense-vector fixture; this change does not replace Hybrid's CPU vector processing or migrate KURV.

Validation: 34 renderer library tests, strict all-target/all-feature Clippy, formatting, and the GPU contract at all three scales passed. The GPU contract checks maximum per-channel differences of 2/255 against reference rendering. Raw logs and summary JSON are stored beside this file.
