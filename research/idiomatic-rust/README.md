# Idiomatic Rust: patterns, counterexamples, and decisions

Research date: 2026-09-13. Starting collection: [mre/idiomatic-rust](https://github.com/mre/idiomatic-rust). Sixteen parallel research assignments examined distinct topics, followed the collection's relevant links, and checked language/library claims against primary documentation. The chapters include good and bad examples, correctness arguments, application criteria, exceptions, and source links. This is a topic-driven investigation, not a claim that every linked book, video, or repository was read in full.

## What makes a Rust pattern correct?

Our synthesis separates four questions that are too often compressed into “idiomatic”:

1. **Is it memory-safe and sound?** Safe Rust checks ownership and borrowing; unsafe code must uphold the additional contracts its operations require. Passing the compiler is not a proof of an unsafe abstraction's soundness. [Rust Reference: undefined behavior](https://doc.rust-lang.org/reference/behavior-considered-undefined.html).
2. **Does it preserve the intended behavior?** Deadlocks, dropped errors, unintended overflow, stale IDs, and cancellation-induced partial updates can occur without a memory-safety violation. State the invariant and failure policy before choosing syntax. [Safe Rust pitfalls](https://corrode.dev/blog/pitfalls-of-safe-rust/), [Tokio cancellation safety](https://docs.rs/tokio/latest/tokio/macro.select.html#cancellation-safety).
3. **Does the API communicate its contract?** Types, ownership, trait bounds, names, and errors should let a caller predict what is consumed, borrowed, validated, or allowed to fail. [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/).
4. **Is the complexity justified here?** A type or abstraction earns its place by preventing a plausible mistake or supporting real variation. Performance claims need representative measurements. [Be Simple](https://corrode.dev/blog/simple/).

A pattern marked “bad” in these chapters is bad for the stated requirement. It is not automatically forbidden syntax. Cloning, panicking, dynamic dispatch, runtime validation, and plain loops all have legitimate uses.

## The sixteen investigations

| Topic and chapter | Good default | Failure pattern to recognize | When to choose something else |
|---|---|---|---|
| [1. Ownership and lifetimes](01-ownership.md) | Borrow for temporary access; own retained data | Clone or add `'static` to suppress an unexplained ownership error | Clone for an independent snapshot; share ownership when multiple owners really exist |
| [2. Strings, slices, and paths](02-strings.md) | `&str` for text views, `String` for ownership, `Path` for paths | Require owned strings for inspection; assume filenames are UTF-8 | `Cow` when conditional allocation measurably matters; bytes for binary data |
| [3. Errors and context](03-errors.md) | Preserve actionable error types and operation context | Turn every cause into a string, or erase categories before recovery | Dynamic reports at an application boundary; typed errors where code branches |
| [4. Panics and validation](04-panic-validation.md) | `Result` for expected external failure; checked construction | `unwrap` on ordinary user/file/network input | Panic for a documented violated invariant or intentional top-level policy |
| [5. Domain types](05-domain-types.md) | Newtypes for distinct meanings; enums for alternatives | Aliases pretending to enforce units; public fields bypassing validation | Plain primitives for local values with no meaningful invariant or confusion risk |
| [6. Traits and dispatch](06-traits.md) | Minimal bounds; concrete types until variation matters | Add `Clone + Send + Sync + 'static` without a reason | Generics for compile-time variation; `dyn` for runtime heterogeneous implementations |
| [7. Conversions and arithmetic](07-conversions.md) | `From` for infallible conversions, `TryFrom` for rejection | Narrowing `as` silently changes external input | Explicit wrapping/saturating operations when those are the intended mathematics |
| [8. Iterators and loops](08-iterators.md) | Short pipelines for transformations; loops for complex control flow | Discard errors with `filter_map(Result::ok)` during validation | Best-effort filtering only when omission is the explicit policy |
| [9. Public API design](09-api-design.md) | Small typed contracts, deliberate visibility and naming | Public representation leakage or builders for trivial construction | Builders for substantial configuration; open traits for real extension points |
| [10. State and resources](10-state-resources.md) | Runtime enums for runtime states; guards for cleanup | Put essential fallible commit work only in `Drop` | Typestate for small compile-time phase protocols; explicit completion methods |
| [11. Shared state](11-shared-state.md) | Single ownership first; short mutex critical sections if needed | Assume `Arc` makes any payload thread-safe, or confuse deadlock avoidance with atomicity | `Rc`/`RefCell` within one thread; atomics for a justified small protocol |
| [12. Async and cancellation](12-async.md) | Async for substantial waiting; bounded queues; owned shutdown | Block an executor or abandon partially completed work without a policy | Threads for blocking work; CPU pools for bounded compute parallelism |
| [13. Collections and performance](13-collections-performance.md) | Pick collections by access/order contract; measure | Reference-counted graph machinery without an ownership need | Arenas for centralized ownership; generational handles for slot reuse |
| [14. Unsafe and FFI](14-unsafe-ffi.md) | Small unsafe boundaries with explicit proofs | Fabricate references from unchecked foreign pointers | Copy foreign data when proving borrowed validity is impractical |
| [15. Tests, docs, and tooling](15-testing-docs.md) | Check observable contracts; executable public examples | Treat lints or compilation as proof of application correctness | Target-specific and feature-specific checks when the supported matrix demands them |
| [16. Architecture and simplicity](16-architecture.md) | Concrete modules with private details and useful boundaries | Speculative traits, factories, layers, or macros | Extract abstractions for actual variation, invariants, or integration seams |

The table is our cross-topic synthesis; each linked chapter supplies supporting sources and the qualifications behind its row. Chapter 1 also includes a few local MUI ownership examples located through the repository graph; this project has not undergone a full code audit.

## A small example connecting several decisions

Suppose an application accepts a configured, nonzero service port. The domain policy excludes zero; this is not a universal networking rule—binding an OS socket to port zero can request an ephemeral port.

```rust
#[derive(Debug, PartialEq)]
struct Port(u16);

impl TryFrom<u16> for Port {
    type Error = &'static str;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        if value == 0 {
            Err("port must be nonzero")
        } else {
            Ok(Self(value))
        }
    }
}
```

The private field makes the constructor the external construction boundary. `TryFrom` advertises rejection, and callers can handle it before starting work. For this small standalone example, a static error message suffices; a published library whose callers distinguish failure categories should expose an appropriate error type. Within the defining module, construction can still bypass the check, so maintain that module's invariant deliberately. See [domain types](05-domain-types.md), [conversions](07-conversions.md), and [error design](03-errors.md).

For a sequence of input strings, choose the failure policy explicitly:

```rust
use std::num::ParseIntError;

fn parse_all(values: &[&str]) -> Result<Vec<u16>, ParseIntError> {
    values.iter().map(|s| s.parse()).collect()
}
```

The input is borrowed, parsing errors remain visible, and the operation returns at the first error. Replacing the mapping with `filter_map(|s| s.parse().ok())` would silently accept an incomplete list. That alternative is correct only if the requirement is to discard malformed entries. Short-circuit collection does not undo side effects already performed; it is not a transaction. See [iterators](08-iterators.md).

## Where the advice needs judgment

**Borrow versus clone.** Borrowing avoids copying but couples lifetimes. A cheap clone or owned value can make a long-lived API much simpler. `Arc::clone` shares a payload; it does not duplicate it. Start with the ownership contract, then measure cost. [Ownership](01-ownership.md), [collections](13-collections-performance.md).

**Enums versus typestate versus traits.** Enums model a closed set of alternatives; typestate limits the operations available in a statically known phase; traits abstract a capability. They answer different questions. Select the smallest one that preserves the actual invariant. [State machines](10-state-resources.md), [traits](06-traits.md).

**Static safety versus runtime validity.** A wrapper can prevent units being confused without proving a number is finite. A borrow can be memory-safe while a held lock deadlocks. A canceled operation can release its memory correctly but lose a message. Trace the business invariant through failure and cancellation paths. [Domain types](05-domain-types.md), [shared state](11-shared-state.md), [async](12-async.md).

**Simple code versus extensible APIs.** Application internals can remain concrete. A public library may justify wider input traits, stable errors, and evolution safeguards because downstream users have different needs. Do not apply application shortcuts or public-library ceremony indiscriminately. [API design](09-api-design.md), [architecture](16-architecture.md).

## Freshness and verification

Older collection entries remain useful for design reasoning, but their compiler constraints are not always current. The reports specifically address non-lexical lifetimes, stabilized conversion traits, Rust 2024 temporary scopes, current dyn-compatibility terminology, and unsafe FFI declarations. Check your own edition, minimum supported Rust version, dependencies, and runtime before copying an example.

[pattern_checks.rs](pattern_checks.rs) is a dependency-free executable covering selected examples: domain validation, rejected integer narrowing, fail-fast parsing, borrowed/owned `Cow` paths, UTF-8 slicing boundaries, and moving out of an `Option` with `take`.

```sh
rustc --edition=2024 pattern_checks.rs -o /tmp/idiomatic-rust-pattern-checks
/tmp/idiomatic-rust-pattern-checks
```

These checks passed with the installed `rustc 1.98.1`. They are smoke checks for selected patterns, not verification of every chapter fragment. Some chapter snippets deliberately show compile errors, unsafe counterexamples, or partial APIs with omitted domain types and external crates. Never execute the unsafe counterexamples. No application source was changed for this research.

The accompanying [resources.json](resources.json) is a snapshot of the collection's resource index used for provenance. Inline source links in the chapters identify the material supporting each topic; recommendations and tradeoff judgments are the research synthesis, while language and API guarantees are checked against their primary documentation.
