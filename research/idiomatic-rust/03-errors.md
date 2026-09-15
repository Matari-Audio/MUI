# Idiomatic Rust: errors, context, and API boundaries

The `idiomatic-rust` repository defines idiomatic code as the language’s
concise, common way of expressing an operation, and links both BurntSushi’s
foundational [Error Handling in Rust](https://burntsushi.net/rust-error-handling/)
and newer articles on [context-preserving errors](https://kazlauskas.me/entries/errors),
[wrapping errors](https://edgl.dev/blog/wrapping-errors-in-rust/), and
[designing library error types](https://d34dl0ck.me/rust-bites-designing-error-types-in-rust-libraries/index.html).
The consistent lesson is that the error type is part of the program’s design:
preserve structured information until the layer that knows how to act, attach
the operation’s context, then render a useful report at the boundary.

## Correctness first: `Result`, `Error`, and `panic!`

Rust distinguishes recoverable failure (`Result<T, E>`) from an unrecoverable
broken invariant (`panic!`). The [Rust Book’s error-handling chapter](https://doc.rust-lang.org/stable/book/ch09-00-error-handling.html)
describes this as a deliberate type-level choice: callers must acknowledge a
fallible operation. The `?` operator propagates an error and applies `From` to
convert it to the enclosing function’s error type, so a good function returns
the decision to the caller instead of guessing too early. Use `panic!`,
`unwrap`, or `expect` only for a proven invariant, test assertion, concise
example, or a genuinely unrecoverable bug. The Book specifically recommends
`expect` over a bare `unwrap` when an invariant is intended, because the
message records the assumption; an external report in the repository also
warns that panics should not be a general exception mechanism.

Bad application/library code:

```rust
use std::path::Path;

// Illustrative excerpt; `Config` is the application's domain type.
pub fn load(path: &Path) -> Result<Config, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    serde_json::from_str(&text).map_err(|e| e.to_string())
}
```

Converting immediately to `String` is lossy: callers cannot reliably distinguish
not-found, permission, malformed input, or a retryable transport error. A
public function returning `Box<dyn Error>` has a similar semantic-erasure cost
when callers need to branch. A `String` can be acceptable at a final display
boundary or in a throwaway program; it is a poor reusable API.

Every public error should implement `Debug`, `Display`, and
`std::error::Error`, and should generally be `Send + Sync` when possible. The
[Rust API Guidelines C-GOOD-ERR](https://rust-lang.github.io/api-guidelines/interoperability.html#c-good-err)
also reject `()` as an error because it carries no useful message or structure.
`Display` should be concise, lowercase, and without trailing punctuation. The
standard [`Error` documentation](https://doc.rust-lang.org/std/error/trait.Error.html)
defines `source()` as the way to cross an abstraction boundary while retaining
the lower-level cause. An outer `Display` should not also print the source, or
reports will duplicate it; let a reporter walk the chain.

## Libraries: make failure part of the contract

A library caller may recover, retry, map an error to a protocol status, or
match on a stable domain category. Define a dedicated enum or opaque wrapper,
preserve `source`, and expose only the distinctions callers can act on. The
[`thiserror` documentation](https://docs.rs/thiserror/latest/thiserror/)
provides a derive macro for exactly this: it generates ordinary `Error`,
`Display`, and `From` implementations and intentionally does not appear in the
public API. `#[from]` also marks the source, but that variant may contain no
other fields besides the source (apart from a backtrace). Use explicit
`map_err` when the operation needs its own context.

```rust
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("failed to open configuration {path:?}")]
    Open {
        path: std::path::PathBuf,
        #[source] source: std::io::Error,
    },
    #[error("failed to parse configuration {path:?}")]
    Parse {
        path: std::path::PathBuf,
        #[source] source: serde_json::Error,
    },
}

// Contextual excerpt; `Config` is the library's domain type.
pub fn load(path: &Path) -> Result<Config, LoadError> {
    let text = std::fs::read_to_string(path).map_err(|source| LoadError::Open {
        path: path.to_owned(), source,
    })?;
    serde_json::from_str(&text).map_err(|source| LoadError::Parse {
        path: path.to_owned(), source,
    })
}
```

A generic `Io(#[from] io::Error)` variant for every I/O operation becomes a bad
pattern when callers need to distinguish opening, reading, flushing, and
writing: “permission denied” alone does not identify the failed operation. It
is perfectly reasonable when the API intentionally treats all I/O as one
category and another layer already records the operation. The
context-preserving article shows the multi-operation problem and recommends
one distinct context-bearing variant per meaningful fallible expression. That
is guidance, not a requirement to create hundreds of variants: one variant
with an operation/path field can cover repeated calls.

Do not blindly expose third-party errors in a public enum. A variant containing
`sqlx::Error` or `reqwest::Error` makes those types part of your API and can
force consumers to carry your exact dependency versions. The library-design
article recommends private wrapper types, an opaque `Box<dyn Error + Send + Sync>`
when callers only need a cause chain, or an `#[error(transparent)]` public
wrapper around a private representation. Mark public enums `#[non_exhaustive]`
when adding future variants must remain compatible. For an internal library
inside one application, direct `#[from]` variants are often a sensible smaller
choice because dependency leakage is under the same team’s control.

## Applications: report rich context at the edge

An application usually owns the top-level policy and does not need every
internal error type in `main`. [`anyhow`](https://docs.rs/anyhow/latest/anyhow/)
provides a concrete, trait-object-based error type, `anyhow::Result<T>`,
downcasting, and `context`/`with_context` for human-readable operation context.
The README’s useful pattern is:

```rust
use anyhow::Context;
use std::path::Path;

// Contextual excerpt; `Config` and `start` are application domain code.
fn run(path: &Path) -> anyhow::Result<()> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("read input {}", path.display()))?;
    let config: Config = serde_json::from_slice(&bytes)
        .context("parse input as configuration")?;
    start(config).context("start service")
}

fn main() {
    if let Err(error) = run(Path::new("config.json")) {
        eprintln!("{error:?}"); // chain and optional backtrace
        std::process::exit(1);
    }
}
```

`context` is good when the caller will report or log the failure. `with_context`
is useful when formatting depends on a path, identifier, or other value, and
its closure avoids work on success. A bad application pattern is sprinkling
`unwrap()` on user files, network calls, or CLI arguments: malformed external
input becomes an unexplained crash. Another bad pattern is adding context only
at the top, producing “operation failed” without the path, phase, or item that
failed.

The “`thiserror` for libraries, `anyhow` for applications” rule is a useful
default, not a law. An application’s domain/service layer should use a typed
`thiserror` enum when it must branch on `NotFound`, `InvalidInput`, or
`Conflict`; convert to `anyhow` at the orchestration/reporting boundary. A
library may use `anyhow` internally behind a private function, but should return
a dedicated stable error where downstream code can make decisions. Conversely,
an internal application can use `thiserror` throughout when explicit domain
contracts and testable matching matter more than minimum boilerplate.

As of 2026-09-13, the current docs show `thiserror` 2.0.20 and `anyhow` 1.0.104;
check the chosen MSRV and feature flags before copying examples. The crates’
roles are complementary: `thiserror` names and stabilizes failure cases,
while `anyhow` aggregates unknown failures and adds report context. Preserve
the source chain in either case, and only erase type information once the next
layer truly cannot make a meaningful decision from it.
