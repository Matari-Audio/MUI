# Idiomatic Rust: tests, documentation, and the quality loop

The [idiomatic-rust collection](https://github.com/mre/idiomatic-rust) defines idiomatic code as the concise, conventional way to accomplish a task in Rust, and it links both Clippy and the Rust API Guidelines as practical references. For testing and documentation, the useful standard is stronger than “the code compiles”: an example should teach the intended contract, an automated check should exercise that contract, and CI should make the same checks repeatable for the toolchain versions users actually support.

## Test the contract at the right boundary

Rust’s built-in test harness makes `#[test]` functions easy to run with `cargo test`; Cargo also discovers integration tests under `tests/`, which compile as separate crates and therefore exercise the public API as a downstream user sees it ([The Rust Book, Writing Tests](https://doc.rust-lang.org/book/ch11-01-writing-tests.html); [Cargo test](https://doc.rust-lang.org/cargo/commands/cargo-test.html)). A good pattern is to keep small unit tests beside the implementation when they verify private invariants, and use `tests/` for public behavior, cross-module wiring, and compatibility. Name tests after observable behavior (`rejects_empty_token`, `round_trip_preserves_unicode`) and assert values, errors, and boundary cases.

```rust
#[test]
fn rejects_empty_token() {
    assert_eq!(Token::parse(""), Err(ParseError::Empty));
}
```

The bad pattern is a test that merely calls a function, asserts nothing meaningful, or reaches into private fields to freeze an implementation. A test that sleeps, depends on the network, wall-clock time, or test order is also a poor default: it fails intermittently and says little about the contract. Use a fake clock, an in-memory transport, or deterministic fixtures at those boundaries. Keep a small number of end-to-end tests when the integration itself is the behavior that matters; unit tests are not a substitute for checking a real serialization, database, process, or protocol boundary.

## Treat public examples as executable documentation

Rustdoc code blocks are compiled and run as documentation tests by `cargo test`; `cargo test --doc` isolates them ([Rustdoc documentation tests](https://doc.rust-lang.org/rustdoc/documentation-tests.html); [Rust By Example, Documentation testing](https://doc.rust-lang.org/rust-by-example/testing/doc_testing.html)). This gives a public API example two jobs: explain how to use the item and detect drift when the API changes. The API Guidelines recommend examples for public items, applied with judgment, and emphasize that an example should show why a reader wants the item rather than mechanically showing a call ([Rust API Guidelines: Documentation](https://rust-lang.github.io/api-guidelines/documentation.html)).

```rust
/// Parse a token from user input.
///
/// # Examples
///
/// ```
/// # use example_crate::Token; // replace `example_crate` with this package's name
/// # use std::error::Error;
/// # fn main() -> Result<(), Box<dyn Error>> {
/// let token = Token::parse("abc")?;
/// assert_eq!(token.as_str(), "abc");
/// # Ok(())
/// # }
/// ```
```

In that snippet, `example_crate` is a placeholder for the package name. Hidden `#` lines let a doctest use a `Result`-returning `main` while keeping setup out of the rendered snippet. Prefer `?` in fallible examples: the API Guidelines warn that copied examples make `unwrap` an accidental design decision. Document `# Errors`, `# Panics`, and, for unsafe APIs, `# Safety` so the prose states the same contract the test demonstrates. A good doctest asserts an output or error, is short enough to read, and uses stable, deterministic inputs.

The bad pattern is a README or rustdoc snippet that is never compiled, silently uses `unwrap` on a realistic failure path, or marks everything `ignore` merely to get CI green. Use `no_run` when compilation is valuable but running would perform an external side effect, `should_panic` only when panic behavior is the point, and `compile_fail` for an intentionally rejected usage; explain the reason. For a syntax-highlighted fragment that is not intended to be a complete example, use a non-Rust fence tagged `text`; current rustdoc guidance calls `ignore` the generic fallback and recommends `text` for non-code. The collection’s article [Teaching libraries through good documentation](https://deterministic.space/teaching-libraries.html) makes the useful connection explicit: guides are integration tests, and their snippets should be treated as doctests where possible.

## Use Clippy and rustfmt as targeted feedback

`cargo fmt --all -- --check` is a cheap CI gate: rustfmt’s `--check` exits nonzero when it would change formatting ([rustfmt README](https://github.com/rust-lang/rustfmt#verifying-code-is-formatted)). Run `cargo fmt` locally to apply changes, then review the diff. Formatting is a consistency tool, not a substitute for tests; macro-heavy or generated fragments may need a narrowly scoped `#[rustfmt::skip]` with a reason.

`cargo clippy` runs the default `clippy::all` group, covering correctness, suspicious code, style, complexity, and performance. In CI, `cargo clippy --all-targets --all-features -- -D warnings` catches warnings in tests, examples, and optional feature code when all features are valid together ([Clippy usage](https://doc.rust-lang.org/clippy/usage.html); [Clippy CI](https://doc.rust-lang.org/clippy/continuous_integration/index.html)). If the project wants only Clippy diagnostics to fail the job, deny `clippy::all` specifically instead of turning every rustc warning into an error. Pin the same toolchain for compilation and Clippy; the Clippy documentation recommends this for compatibility.

Avoid cargo-cult linting. `clippy::pedantic` is allow-by-default and has occasional false positives; the `restriction` group can contradict reasonable code and should not be enabled wholesale ([Clippy lint groups](https://doc.rust-lang.org/clippy/)). Enable individual lints when they encode a real project rule, and put a narrow `#[allow(clippy::name)]` at the exceptional item with a comment explaining the invariant. Blanket `allow(clippy::all)` hides useful diagnostics; blanket `deny(clippy::pedantic)` turns style preferences into needless churn. Treat `cargo clippy --fix` as a suggestion generator and inspect its changes, especially around ownership and arithmetic.

## Make editions, MSRV, and CI explicit

Set `edition` explicitly in `Cargo.toml`; it applies to libraries, binaries, examples, tests, and benchmarks in the package ([Cargo manifest](https://doc.rust-lang.org/cargo/reference/manifest.html#the-edition-field)). Editions are opt-in language changes and crates from different editions interoperate, so an edition migration should be a deliberate, tested choice ([Edition Guide](https://doc.rust-lang.org/edition-guide/editions/)). Set `rust-version` to the minimum supported compiler. Cargo then gives a direct diagnostic on unsupported toolchains, and Clippy can use that value when deciding whether a suggestion is MSRV-compatible ([Cargo Rust version](https://doc.rust-lang.org/cargo/reference/rust-version.html)).

A practical CI matrix has a formatting job, a default-feature test, tests for supported feature combinations, Clippy over all targets, and (for a library with a stated MSRV) a job on the minimum toolchain plus current stable. `--all-features` is useful when features are designed to compose; when features are mutually exclusive, enumerate valid combinations instead. Add beta or nightly as an early-warning job only if the project can absorb occasional failures. A bad setup runs only `cargo test` on the developer’s newest compiler, omits doctests or feature-gated targets, and lets dependency or edition upgrades change the supported language accidentally.

## Minimal review checklist

```text
cargo fmt --all -- --check
cargo test --workspace
# Use --all-features on both commands only when every feature combination is valid.
cargo test --workspace --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo doc --workspace --no-deps
```

Ordinary `cargo test` already runs library doctests; use `cargo test --doc` as an optional focused command when working on documentation. Use the smallest command set that matches the crate’s real targets and features, replacing the `--all-features` lines with explicit supported combinations when necessary. Make every public example and documented failure mode executable somewhere. The idiomatic pattern is a short, readable test or example at the boundary where users depend on behavior, backed by a CI check whose toolchain and lint policy are written down.
