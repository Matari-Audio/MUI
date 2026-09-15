# Panics, `Result`, `unwrap`, and validation in idiomatic Rust

The `idiomatic-rust` repository points readers to three especially useful
sources for this topic: Cameron’s *To panic or not to panic*, Rising’s *Three
Kinds Of Unwrap*, and Endler’s *Patterns for Defensive Programming in Rust*.
Together with the [Rust Book’s chapter on panic decisions](https://doc.rust-lang.org/book/ch09-03-to-panic-or-not-to-panic.html), they
support one practical rule: make expected failure explicit in the type system;
reserve panics for bugs, violated contracts, or states the program cannot
usefully continue from.

## The core distinction: expected failure versus broken assumptions

Rust’s own `panic!` documentation says that `panic!` represents a bug detected
in the program, while `Result<T, E>` represents an anticipated runtime failure
mode such as I/O or parsing. The caller must propagate or report a `Result`,
whereas panic propagation is automatic ([`std::panic::panic!`](https://doc.rust-lang.org/std/macro.panic.html#when-to-use-panic-vs-result)).
The Rust Book consequently calls `Result` the good default for a function that
might fail: the caller can recover, retry, substitute a value, report a user
error, or decide at the application boundary to stop ([Book, “To `panic!` or
Not”](https://doc.rust-lang.org/book/ch09-03-to-panic-or-not-to-panic.html#guidelines-for-error-handling)).

Good application boundary:

```rust
fn read_config(path: &Path) -> Result<Config, ConfigError> {
    let text = std::fs::read_to_string(path)?;
    Ok(toml::from_str(&text)?)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = read_config(Path::new("app.toml"))?;
    run(config)
}
```

This keeps an absent file or malformed configuration visible to the caller.
The `?` operator returns an `Err` early and lets the caller choose the policy;
it does not silently discard the failure ([Rust Book, “Propagating Errors”](https://doc.rust-lang.org/book/ch09-02-recoverable-errors-with-result.html#propagating-errors)).

Bad library pattern:

```rust
pub fn read_config(path: &Path) -> Config {
    toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}
```

The file system and input are outside the function’s control, so these failures
are ordinary runtime possibilities. Panicking here removes recovery choices
from every caller and turns malformed or missing input into a crash.

## Choosing `unwrap`, `expect`, and explicit handling

`unwrap` is a panic on `None` or `Err`; the standard library generally
discourages it and suggests `?`, matching, or fallback methods instead
([`Option::unwrap`](https://doc.rust-lang.org/std/option/enum.Option.html#method.unwrap),
[`Result::unwrap`](https://doc.rust-lang.org/stable/std/result/enum.Result.html#method.unwrap)).
`expect` has the same behavior but adds a message. The message should explain
why the value *should* exist, rather than merely repeat that extraction failed;
the `Option` docs give “slice should not be empty” as the model
([`Option::expect`](https://doc.rust-lang.org/std/option/enum.Option.html#method.expect)).
The Book notes that production code commonly prefers `expect` over a bare
`unwrap` when a checked assumption really is intentional, because the context
helps debug a violated assumption ([Book, “Shortcuts for Panic on Error”](https://doc.rust-lang.org/book/ch09-02-recoverable-errors-with-result.html#shortcuts-for-panic-on-error)).

Rising usefully separates three semantics that are often hidden behind the same
`.unwrap()` spelling ([*Three Kinds Of Unwrap*](https://zkrising.com/writing/three-unwraps/)):

1. **“If this fails, terminate.”** A top-level server may intentionally stop if
   it cannot bind its only listening socket. Use `expect` when a diagnostic
   message matters.
2. **“This cannot happen.”** A hard-coded, reviewed regex or invariant may be
   impossible to construct incorrectly. Document the proof in `expect` and
   keep the assumption local.
3. **“I will handle it later.”** A prototype may use `unwrap`, but this is a
   TODO, not an idiom to leave in production. Mark it visibly or replace it
   before release.

Good invariant proof:

```rust
let address: IpAddr = "127.0.0.1"
    .parse()
    .expect("hard-coded loopback address should be valid");
```

Bad “it should be fine” proof:

```rust
let first = records[0]; // panics if input is empty
```

Use `records.first().ok_or(Error::MissingRecord)?` when emptiness is possible.
If an index is genuinely guaranteed by a local check, keep the check and the
use together; Cameron distinguishes such local invariants from fragile
non-local promises ([*To panic or not to panic*](https://www.ncameron.org/blog/to-panic-or-not-to-panic/)).

## Put validation at construction boundaries

Input validation belongs where untrusted or fallible data enters a domain
type. Return `Result` for user, file, network, or configuration input. Then make
the validated type impossible to construct incorrectly: keep fields private and
provide a validating constructor, or use a newtype. Endler’s defensive
programming article demonstrates why a public struct plus a validating `new`
method is insufficient: callers can bypass `new` with a struct literal
([constructor section](https://corrode.dev/blog/defensive-programming/#pattern-defensively-handle-constructors)).

```rust
pub struct Port(NonZeroU16);

impl Port {
    pub fn new(value: u16) -> Result<Self, PortError> {
        NonZeroU16::new(value).map(Self).ok_or(PortError::Zero)
    }
}

fn connect(port: Port) { /* port is known nonzero here */ }
```

The caller handles bad external input once; downstream functions accept `Port`
and do not repeatedly re-check the same invariant. For a public library, a
private field or `#[non_exhaustive]` blocks external struct literals; nested
private modules can also block bypasses inside the crate, though that extra
defense is warranted only when the invariant is worth the complexity. Avoid
fallible `From` implementations that hide failure behind a default or panic;
use `TryFrom` when conversion can fail ([Rust API Guidelines, conversion traits](https://rust-lang.github.io/api-guidelines/interoperability.html#c-conv-traits)).

## Panic policy and API documentation

The Book’s sharper test is whether continuing would leave the program in a bad
state: a violated contract, contradictory values, or an invalid value that
later code must assume away. A caller passing nonsense to a library should
usually receive `Err`; panic is reasonable when the contract violation signals
a caller bug or continuing would be unsafe or harmful. Malformed parser input,
rate limits, and ordinary network failures are expected and should be `Result`,
not panic.

Any intentional panic must be part of the public contract. Rust’s API Guidelines
require `# Errors` and `# Panics` sections in function documentation, and
recommend examples use `?` so copied code does not silently teach unwrapping
([C-FAILURE and C-QUESTION-MARK](https://rust-lang.github.io/api-guidelines/documentation.html#c-failure)).
This makes the “why” reviewable and warns future maintainers when a formerly
hard-coded assumption becomes external input.

Finally, no serious program can promise that it never panics: indexing,
assertions, dependencies, allocation, and nested calls can introduce panic
paths. Cameron recommends an explicit high-level panic strategy, minimizing
panics and deciding where a panic may be caught or allowed to terminate. The
goal is not cosmetic removal of every `unwrap`; it is preserving recovery for
expected failures and making every remaining panic correspond to a justified,
documented invariant or deliberate process policy.
