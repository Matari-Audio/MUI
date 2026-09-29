# vendor/vello: MUI's patches

Upstream: [`vello` 0.10.0](https://crates.io/crates/vello/0.10.0), from
linebender/vello at commit `fc0baddd06c63287ef516180d276333aa2401e6e`
(the crate's `.cargo_vcs_info.json`; `path_in_vcs = "vello"`). `vello_shaders`
is unpatched registry 0.10.0; `vello_encoding` 0.10.0 (same commit) is vendored
next door in `vendor/vello_encoding` for patch 5.

Diff it against the registry copy:

```sh
diff -ru ~/.cargo/registry/src/index.crates.io-*/vello-0.10.0 vendor/vello
```

Every source patch carries a `MUI patch:` comment (`grep -rn "MUI patch" vendor/vello`).

## 1. wgpu 30 port

Upstream pins wgpu 29.0.3; MUI's workspace is on wgpu 30.

- `Cargo.toml`: `wgpu = "30.0.1"`; the `wgpu-profiler` feature and dependency
  are cut (no wgpu-30 release of it). Its `cfg`s stay in the source so upstream
  diffs stay small, hence `unexpected_cfgs = "allow"`. Upstream's clippy table
  is trimmed to what compiles clean under the workspace toolchain.
- `src/lib.rs`, `src/debug/renderer.rs`: `get_mapped_range()` returns a
  `Result` in wgpu 30; `.expect` on buffers that were just mapped.
- `src/util.rs`: `SurfaceConfiguration` gains `color_space: Auto`.
- `src/wgpu_engine.rs`: an absent vertex buffer is zero slots, not one empty slot.

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
autohint instance for nearly every run of every frame (~35-40% of a KURV
frame's CPU). The session also looks the instance up lazily, on its first glyph miss
(`Hinter`): a steady frame hits every glyph, yet KURV's editor draws more
(face, size) runs than 32 slots, and resolving the instance per run
reconfigured ~56 autohinters a frame for glyphs already cached (2.7 of a
2.9 ms resolve). Vendored as a path dependency of `vendor/vello`, not a
`[patch]`, so it reaches downstream workspaces that depend on MUI by git.
`vello_shaders` (its `cpu` feature) still links the registry copy; it only
shares plain buffer types with it, never glyph caches.
