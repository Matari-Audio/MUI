# Idiomatic Rust conversions and numeric contracts

The [idiomatic-rust collection](https://github.com/mre/idiomatic-rust) links [Convenient and idiomatic conversions in Rust](https://ricardomartins.cc/2016/08/03/convenient_and_idiomatic_conversions_in_rust), whose central idea still holds: use the standard conversion traits so callers see a uniform API instead of a family of bespoke `new_from_*` constructors. The article is dated, though: its discussion calls `TryFrom` unstable, while `TryFrom`/`TryInto` have been stable since Rust 1.34. The current [standard-library conversion module](https://doc.rust-lang.org/std/convert/) and [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/interoperability.html#conversions-use-the-standard-traits-from-asref-asmut-c-conv-traits) are the authority for present-day code.

## Value conversions: `From`, `Into`, `TryFrom`, `TryInto`

`From<T> for U` consumes a `T` and returns a `U`. It is the right contract when the conversion is infallible, lossless in the semantic sense, value-preserving, and obvious. `String::from("hi")`, `u32::from(7u16)`, and an application error enum implementing `From<io::Error>` are good patterns. The error case is particularly useful: `?` invokes `From` to turn several lower-level errors into one public error type without manual mapping.

```rust
#[derive(Debug)]
enum ConfigError { Io(std::io::Error), Parse(std::num::ParseIntError) }
impl From<std::io::Error> for ConfigError { fn from(e: std::io::Error) -> Self { Self::Io(e) } }
impl From<std::num::ParseIntError> for ConfigError { fn from(e: std::num::ParseIntError) -> Self { Self::Parse(e) } }

fn read_port(path: &str) -> Result<u16, ConfigError> {
    Ok(std::fs::read_to_string(path)?.trim().parse()?)
}
```

Implement `From`, not `Into`: the standard library supplies the reverse-shaped `Into<U> for T` implementation automatically. At a call site, `Target::from(value)` makes the target explicit; `value.into()` is concise when the target is clear from the surrounding type. In a generic function, an `Into<Target>` bound is often more permissive because it also admits legacy or foreign types that provide only `Into`.

`TryFrom<T> for U` is for a conversion that can reject input. It returns `Result<U, E>`, making the failure part of the API rather than hiding it behind a panic or silent data loss. Use `u16::try_from(port)` or `port.try_into()` for narrowing or signed-to-unsigned numeric conversions, and use `TryFrom` for domain types that enforce an invariant at their boundary:

```rust
use std::convert::TryFrom;

struct Port(u16);
impl TryFrom<u32> for Port {
    type Error = &'static str;
    fn try_from(n: u32) -> Result<Self, Self::Error> {
        let value = u16::try_from(n).map_err(|_| "port out of range")?;
        Ok(Self(value))
    }
}
```

Library authors normally implement `TryFrom`; it supplies `TryInto` for free. Generic bounds should usually use `TryInto` for the same reason as `Into`. A common bad pattern is `impl From<u32> for Port` that panics for invalid values: `From` must not fail. Another is `value as u16` at a trust boundary, which silently changes a large value; `TryFrom` makes the caller choose whether to reject, report, or deliberately apply another policy.

## Borrowed views: `AsRef`, `Borrow`, and `Deref`

`AsRef<T>` means a cheap, infallible reference-to-reference view: `String` and `&str` can both satisfy `AsRef<str>`, and `PathBuf`/`Path` can both satisfy `AsRef<Path>`. It is a strong generic argument bound when a function only needs to inspect a borrowed view:

```rust
fn has_extension<P: AsRef<std::path::Path>>(path: P) -> bool {
    path.as_ref().extension().is_some()
}
```

Do not implement `AsRef` for a conversion that allocates, validates, or can fail; use a named method, `Option`/`Result`, or an owned `From`/`TryFrom` conversion. `AsRef` is also not universally reflexive: the standard library cannot provide one blanket `AsRef<T> for T` without overlap, so do not assume every `T` implements `AsRef<T>`. The trait auto-dereferences references, but it is not a general substitute for dereferencing a smart pointer; borrowing it as `&boxed` or relying on normal deref coercion is clearer.

`Borrow<T>` has the same shape but a stricter semantic promise. If `K: Borrow<Q>`, then `K` and `Q` must agree on `Hash`, `Eq`, and `Ord`; this is what makes `HashMap<String, V>::get("key")` correct without allocating a temporary `String`. A case-insensitive wrapper may expose its inner `str`, but because its equality and hash differ from `str`, it must use `AsRef<str>`, not `Borrow<str>`. Use `Borrow` for interchangeable owned/borrowed lookup keys, and `AsRef` for a merely convenient view such as one field of a larger struct.

`Deref<Target = U>` should make a type transparently pointer-like: dereferencing is cheap, infallible, and unsurprising. The compiler inserts deref calls in coercions and method lookup, so `Deref` is part of the public API even when no `.deref()` appears in source. A `Box<T>`-like wrapper is a good fit; a validating or expensive wrapper is not. Do not add `DerefMut` merely to expose a field if mutation could violate the wrapper’s invariant; provide a checked mutation method or a narrower view instead. The official [Deref documentation](https://doc.rust-lang.org/stable/core/ops/trait.Deref.html) explicitly warns about method collisions and implicit behavior.

## Numeric casts and arithmetic

The `as` operator is explicit syntax, but it is not a checked numeric conversion. Integer-to-integer casts can truncate; equal-width signed/unsigned casts reinterpret the two’s-complement bit pattern (`-1i8 as u8 == 255`), and signedness/range changes can produce a mathematically different value. Float-to-int casts truncate toward zero and saturate at the target integer bounds, with `NaN` becoming zero; integer-to-float can round and can produce infinity. These rules are defined in the [Rust Reference](https://doc.rust-lang.org/reference/expressions/operator-expr.html#numeric-cast), so the operation is not undefined, but “defined” does not mean “the value the application intended.”

Use `as` when the representation change is deliberately part of the algorithm and is documented (for example, a bit mask or a known enum discriminant). For protocol bytes, prefer explicit `to_le_bytes`/`from_le_bytes` or a `match`/`TryFrom` for an enum, because an arbitrary byte may not represent a valid variant. For ordinary integer range conversion, prefer `TryFrom`/`TryInto`:

```rust
fn to_index(count: u32) -> Result<usize, std::num::TryFromIntError> {
    count.try_into() // reject values unrepresentable on this target
}
```

For arithmetic, plain `+`, `-`, and `*` can panic on overflow when overflow checks are enabled (normally debug builds); compiler flags can change this, so correctness must not depend on build mode. Choose a method whose name states the required contract: `checked_add`/`checked_mul` return `Option` and are appropriate when overflow is an ordinary input error; `strict_*` always panics and is suitable only when overflow proves a programmer or invariant violation; `saturating_*` clamps at numeric bounds for bounded quantities such as UI levels; `wrapping_*` intentionally performs modular arithmetic for counters, hashes, and bit-level algorithms; `overflowing_*` returns the wrapped result plus an overflow flag. `unchecked_*` is `unsafe` and causes undefined behavior if its precondition is false; do not use it merely to avoid debug-mode panics, though a proven invariant may justify it in a tightly reviewed hot path.

The useful pattern is to make the contract visible at the operation, then test its boundary:

```rust
fn reserve(total: u32, extra: u32) -> Option<u32> {
    total.checked_add(extra)
}

assert_eq!(reserve(u32::MAX, 1), None);
assert_eq!(200u8.wrapping_add(100), 44); // only if modular arithmetic is intended
```

The decision rule is simple: `From` for an obvious perfect value conversion, `TryFrom` for rejection, `AsRef` for a cheap view, `Borrow` only when lookup traits are identical, `Deref` only for pointer-like transparency, `as` only for an intentional representation rule, and named checked/saturating/wrapping arithmetic whenever overflow has meaning.

### Sources

- [idiomatic-rust README and curated links](https://github.com/mre/idiomatic-rust)
- [Convenient and idiomatic conversions in Rust](https://ricardomartins.cc/2016/08/03/convenient_and_idiomatic_conversions_in_rust)
- [`std::convert`](https://doc.rust-lang.org/std/convert/), [`From`](https://doc.rust-lang.org/std/convert/trait.From.html), [`TryFrom`](https://doc.rust-lang.org/std/convert/trait.TryFrom.html), [`TryInto`](https://doc.rust-lang.org/std/convert/trait.TryInto.html), [`AsRef`](https://doc.rust-lang.org/std/convert/trait.AsRef.html)
- [`Borrow`](https://doc.rust-lang.org/std/borrow/trait.Borrow.html), [`Deref`](https://doc.rust-lang.org/stable/core/ops/trait.Deref.html)
- [Rust API Guidelines: conversion traits](https://rust-lang.github.io/api-guidelines/interoperability.html#conversions-use-the-standard-traits-from-asref-asmut-c-conv-traits) and [ad-hoc conversion names](https://rust-lang.github.io/api-guidelines/naming.html#ad-hoc-conversions-follow-as-to-into-conventions-c-conv)
- [Rust Reference: numeric casts and overflow](https://doc.rust-lang.org/reference/expressions/operator-expr.html)
- [Current integer arithmetic methods (`u32`)](https://doc.rust-lang.org/std/primitive.u32.html)
- [Rust Number Conversion: Don’t Follow the Book…](https://blog.notmet.net/2021/12/rust-number-conversion-dont-follow-the-book.../) (useful critique of unchecked `as`; not normative)
