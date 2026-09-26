# mui-migrate

Rewrites Rust code and `Cargo.toml` files to the MUI DSL v2 API
(`docs/DSL-V2.md`). Standalone crate, not a member of the main workspace.

```sh
cd tools/mui-migrate
cargo run --release -- --dry-run --diff ../../examples   # preview
cargo run --release -- path/to/crate/src                 # rewrite in place
```

`mui-migrate [--dry-run] [--diff] [--include-vendored] [--no-docs] [--widgets] <paths...>`

- Walks directories for `.rs`, `.md` and `Cargo.toml`, skipping `target/`, `.git/`,
  and (unless `--include-vendored`) `vendor/` and `.build-inputs/`.
- Prints edits per file, `file:line: note` for anything needing a human, and
  a summary. `--diff` prints a unified diff. Exit code 1 on unparsable files.
- Docs mode (on; `--no-docs` turns it off): Rust code blocks in `.md` files
  and in `///`/`//!` doc comments are migrated like code. A `.md` file named
  on the command line is processed even when not under a walked directory.
- `--widgets` adds the widget renames (named results, `id` arguments); they
  touch common names, so they are opt-in.
- Idempotent: a second run changes nothing.

## How it works

Each file is lexed with `proc-macro2`, so text in strings and comments is
never touched while tokens inside macro calls (`row![..]`, `vec![..]`) are.
Rules produce byte-range edits on the original text; passes repeat until
nothing changes, so formatting survives.

Renames only apply where a name resolves to mui: through `use mui::..`
(also `mui2`, `mui_scene`, ... and any `package = "mui-*"` alias found in
`Cargo.toml`), a mui glob (`use mui2::prelude::*`), a mui-rooted path, or a
`use super::{..}` / `crate::..` import traced to another indexed file that
gets it from mui. Local definitions and foreign imports win, so a project's
own `button` or `Kind` is left alone. Inside a mui crate its own names count
as mui (a crate's tests calling `super::leaf`), and a `Node::<T>::old` path is
renamed through the turbofish. Method renames of common names
(`.label`, `.role`, `.disabled`, ...) only apply in files that use mui;
`.disabled(x)` only on a builder chain (`Gate::MuiChain`).

Manual notes come from the original text, so a note still shows where a rule
already rewrote the code. A file that defines or `let`-binds a name a rule
renames to (`block`, `resolve`, ...) and still uses the old mui name (for a
function: calls it) gets a collision warning: the rename would shadow.

## Adding a rule

One line in `src/rules.rs`; the rule kinds are documented at the top of that
file. Examples:

```rust
Function { old: "leaf", new: "block" },
Call { chain: &[("disabled", &[Is("true")])], to: ".disabled()", gate: Mui, needs: &[] },
Manual { pattern: "resolve_scene_cached", note: "use `Resolver`" },
```

Tuple results that became structs go in `TUPLES`; moved crates in
`CARGO_MOVES`. Add a fixture to `src/tests.rs` and run `cargo test`.

## Limitations

- `.disabled(x)` on a builder chain is assumed to be the element switch; a
  `palette().disabled(color)` chain would be misread.
- A tuple result stored in a variable (`let r = knob(..); r.0`) is not traced.
- `.anchor(..).offset(..)` is only folded when the two calls are adjacent.
- Bare `Role` variants are qualified only in files with a mui glob and no
  other glob import.
- Modules declared with `#[path]` are not followed for cross-file imports.
