# Moved paths

The media crates left the root workspace for `media/`, their own workspace
(see docs/DSL-V2.md, "Structure changes"). One line per move, old → new, for
`mui-migrate` and anyone with an old checkout.

| Old path | New path |
| --- | --- |
| `crates/mui-stage` | `media/mui-stage` |
| `crates/mui-reel` | `media/mui-reel` |
| `crates/mui-motion-bridge` | `media/mui-motion-bridge` |
| `tools/kurv-live` | `media/tools/kurv-live` |
| `tools/kurv-motion` | `media/tools/kurv-motion` |
| `tools/mui-motion` | `tools/film` |

`tools/mui-motion` was renamed (not moved into media/) so it stops colliding
with the `mui-motion` spring crate. Build media crates with
`cargo build --manifest-path media/Cargo.toml -p <crate>`; their workspace
dependency keys (`mui-stage.workspace = true`, ...) are unchanged.
