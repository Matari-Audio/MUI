# mui-migrate

Rewrites Rust code and `Cargo.toml` files to the MUI DSL v2 API
(`docs/DSL-V2.md`). Standalone crate, not a member of the main workspace.

```sh
cd tools/mui-migrate
cargo run --release -- --dry-run --diff ../../examples   # preview
cargo run --release -- path/to/crate/src                 # rewrite in place
```

`mui-migrate [--dry-run] [--diff] [--include-vendored] [--no-docs] [--no-widgets] [--mui-root <dir>] <paths...>`

- Walks directories for `.rs`, `.md` and `Cargo.toml`, skipping `target/`, `.git/`,
  and (unless `--include-vendored`) `vendor/` and `.build-inputs/`.
- Prints edits per file, `file:line: note` for anything needing a human, and
  a summary. `--diff` prints a unified diff. Exit code 1 on unparsable files.
- Docs mode (on; `--no-docs` turns it off): Rust code blocks in `.md` files
  and in `///`/`//!` doc comments are migrated like code. A `.md` file named
  on the command line is processed even when not under a walked directory.
  A block with a `// old` or `// before` line is a before/after example and
  is left as written.
- The widget rules (`Response` fields, `Interaction`, control labels,
  `ColorOpts`, `&mut Bins`) are on; `--no-widgets` turns them off.
- Idempotent: a second run changes nothing.
- A path dependency on a v1 mui checkout (its workspace has no
  `crates/mui-scene`, or, when not checked out, a path under
  `.build-inputs/mui/`) is not mui: files that import only it are left alone.
  `--mui-root <dir>` overrides the guess: path deps inside `<dir>` are the
  mui to migrate, all others are v1.

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
`.label(x)`, `.disabled(x)` and `.join()` only on a builder chain
(`Gate::MuiChain`: element builder calls from a constructor, a widget's `.el`,
this file's fn returning `El`, or a variable of an element type).
A bare name that another glob import could also supply (`use crate::x::*`
next to the mui prelude) is left with a note.

Rules keyed to a type (`.min.x` on a `Bounds`, `half`/`wheel` on a `Plate` /
`Input`, `.finite()` on a `Point`, `.state(id)` on the `Ui`, `.len()` on a
`Resolver`) resolve the receiver from its binding in the enclosing fn: a
typed parameter or `let`, `let x = Ty::new(..);`. Any other binding (a
pattern, a closure parameter, a field) is unknown, and gets a note instead.

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

Widget rules go in `WIDGET_RULES`. Tuple results that became structs go in
`TUPLES` (`.0` -> field, `let (a, _) =` -> `let a = f(..).el`, `.0.el()` ->
`.el.into_el()`); moved crates in
`CARGO_MOVES`. Add a fixture to `src/tests.rs` and run `cargo test`.

## Limitations

- Receiver types are not traced across files, through fields, or through
  tuple patterns (`let (el, hit) = button(..)`): those get notes.
- A tuple result stored in a variable (`let r = knob(..); r.0`) is not traced.
- A `let Response { .. }` pattern resolves to whatever `Response` the file
  imports: a file that also imports `mui_input::Response` needs
  `widgets::Response { .. }` by hand.
- The label the tool gives `toggle` and `drag_value` is `""`.
- `.anchor(..).offset(..)` is only folded when the two calls are adjacent.
- Bare `Role` variants are qualified only in files with a mui glob and no
  other glob import.
- Modules declared with `#[path]` are not followed for cross-file imports.
