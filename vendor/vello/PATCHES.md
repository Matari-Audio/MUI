# vendor/vello: MUI's patches

Upstream: [`vello` 0.11.0](https://crates.io/crates/vello/0.11.0), from
linebender/vello at commit `f3000c8d9e7a9c7e6abb09b9587238bf8b4860ff`
(the crate's `.cargo_vcs_info.json`; `path_in_vcs = "research/vello_research"`). `vello_shaders`
is unpatched registry 0.11.0; `vello_encoding` 0.11.0 (same commit) is vendored
next door in `vendor/vello_encoding` for patch 5.

Diff it against the registry copy:

```sh
diff -ru ~/.cargo/registry/src/index.crates.io-*/vello-0.11.0 vendor/vello
```

Every source patch carries a `MUI patch:` comment (`grep -rn "MUI patch" vendor/vello`).

## 0. Exact 0.11 source coverage

The forward port was compared file by file against the above commit, including
both complete `src/` trees: all 10 GPU source files and all 16 encoding source
files are present, with no added or missing source files. The GPU source differs
only in `debug/renderer.rs` and `lib.rs` (mapped-buffer `expect` messages),
`util.rs` (the ordering of the `color_space` initializer), and `wgpu_engine.rs`
(the retained patches below and absent vertex-buffer slots). Encoding differs
only in `glyph_cache.rs` (patch 5). In particular, `scene.rs` and
`vello_encoding/src/encoding.rs` match the 0.11 source exactly.

The old source baseline was 0.10 at
`fc0baddd06c63287ef516180d276333aa2401e6e`. The 0.11 delta was merged onto
that baseline with the downstream patches retained, rather than replacing the
fork. Manifest deviations are the path encoding dependency, the additional
`rustc-hash` dependency, the excluded optional profiler, and the workspace lint
table. The registry 0.11 shaders are unmodified.

The authored `non_apple_sbix.ttf` regression fixture checks the upstream bitmap
bearing correction on CPU and GPU, including a second retained rendering. Its
generator and provenance are stored alongside the fixture. Reverting only
`scene.rs` to the former patched 0.10 source makes the GPU bearing assertion
fail; 0.11 passes on the Radeon RX 6600. This is a rendering correctness check,
not a renderer throughput claim.

## 1. wgpu 30 integration

Upstream 0.11 includes its own wgpu 30 port. MUI retains its explanatory
`expect` messages on mapped buffers and the zero-slot representation of an
absent vertex buffer. The optional `wgpu-profiler` feature/dependency remains
excluded from this vendor manifest; the profiling cfgs stay in source. The
upstream clippy table is trimmed to the workspace's compatible lint surface.
`SurfaceColorSpace::Auto` and the other wgpu 30 API adaptations are now upstream.

The 0.11 upgrade merged the exact upstream source delta onto the patched 0.10
source. All retained binding/pool/pass and lazy hint-instance patches below
remain. Upstream's non-Apple sbix placement and bearing-sign corrections are
included, and encoding/shaders move together to 0.11. Registry shader CPU
helpers still consume registry encoding, while classic rendering uses the
vendored encoding; their serialized buffer layout is from the same release.

## 2. Bind-group cache (`src/wgpu_engine.rs`, `BindGroupCache`)

Bind groups are kept across recordings, keyed by layout plus the bound
buffers/views (the key holds handle clones, so an address cannot be reused
while its entry lives), evicted after 4 recordings unused. Buffers go back to
the pool in reverse free order (`free_bufs` is a `Vec`, not a `HashSet`), so a
frame like the last pops the same buffers for the same proxies and hits the
cache. Adds the `rustc-hash` dependency for the per-dispatch maps.

## 3. Shared compute pass (`src/wgpu_engine.rs`, `run_recording`)

Consecutive dispatches share one `ComputePass` (`forget_lifetime`); any other
encoder command (clear, copy, upload) ends it first. Upstream opened a pass per
dispatch, which cost a hal command buffer and a usage scope each.

## 4. Image texture pool (`src/wgpu_engine.rs`, `ResourcePool::get_image`)

Images a recording frees go into `pool.images` for the next recording to reuse
(a gradient ramp is a fresh image every frame); whatever the next recording
does not take is destroyed when it ends. Upstream destroyed and re-created
them every frame (its own `TODO: have a pool`).

## 5. Hint-instance cache (`vendor/vello_encoding/src/glyph_cache.rs`)

`MAX_CACHED_HINT_INSTANCES` 8 -> 32. The LRU keys on (font, size, coords), and
an editor frame draws 10-14 such combinations, so 8 slots rebuilt a skrifa
autohint instance for nearly every run of every frame (~35-40% of a synth
frame's CPU). The session also looks the instance up lazily, on its first glyph miss
(`Hinter`): a steady frame hits every glyph, yet a synth editor draws more
(face, size) runs than 32 slots, and resolving the instance per run
reconfigured ~56 autohinters a frame for glyphs already cached (2.7 of a
2.9 ms resolve). Vendored as a path dependency of `vendor/vello`, not a
`[patch]`, so it reaches downstream workspaces that depend on MUI by git.
`vello_shaders` (its `cpu` feature) still links the registry 0.11 copy; it only
shares plain buffer types with it, never glyph caches.
