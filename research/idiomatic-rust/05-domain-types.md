# Domain types in idiomatic Rust: newtypes, enums, units, and invariants

The [idiomatic-rust repository](https://github.com/mre/idiomatic-rust) defines idiomatic code as the concise, convenient, common way to solve a problem in Rust, rather than importing habits from another language. Its resource list points directly to the [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/), [Rust By Example’s newtype idiom](https://doc.rust-lang.org/rust-by-example/generics/new_types.html), an article on [units and dimensional arithmetic](https://www.ferrisellis.com/content/rust-implementing-units-for-types/), [compile-time invariants](https://corrode.dev/blog/compile-time-invariants/), and [enums instead of booleans](https://blakesmith.me/2019/05/07/rust-patterns-enums-instead-of-booleans.html). Together they support a practical rule: make distinctions and invariants visible in the type at the boundary where invalid data enters, then keep the valid type easy to use.

## Newtypes prevent category errors

A newtype is a distinct `struct` around an existing value:

```rust
#[derive(Debug, Clone, Copy, PartialEq)]
struct Miles(f64);

#[derive(Debug, Clone, Copy, PartialEq)]
struct Kilometers(f64);

fn marathon_distance(d: Miles) -> bool { d.0 >= 26.2 }
```

`Miles` and `Kilometers` have the same representation but are different compile-time types. A kilometer value cannot accidentally be passed to `marathon_distance`; conversion has to be explicit. This is the point made by both Rust By Example and the Rust Book’s [newtype section](https://doc.rust-lang.org/book/ch20-03-advanced-types.html). A type alias is the wrong tool for this job: `type Miles = f64` only renames `f64`, so miles, kilometers, and unrelated floats remain interchangeable.

The wrapper should expose domain operations, not necessarily the underlying representation. A public tuple field (`pub struct ServicePort(pub u16)`) permits every caller to construct an invalid port and bypass future checks. Prefer a private field and a checked constructor:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServicePort(u16);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidPort;

impl TryFrom<u16> for ServicePort {
    type Error = InvalidPort;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        (value != 0).then_some(Self(value)).ok_or(InvalidPort)
    }
}

impl ServicePort {
    pub const fn get(self) -> u16 { self.0 }
}
```

`TryFrom` is the standard signal for a conversion that can fail; `From` is reserved for conversions that are complete and lossless. For strings, implement `FromStr` so callers can use `"8080".parse::<ServicePort>()`. Both traits return a `Result`, keeping malformed external input out of the trusted domain. The [Rust API Guidelines’ C-VALIDATE rule](https://rust-lang.github.io/api-guidelines/dependability.html#functions-validate-their-arguments-c-validate) prefers static enforcement where possible and otherwise asks APIs to validate dynamically at the boundary. The example deliberately models a service port from `1..=65535`: when binding a socket, many operating systems treat port `0` as a valid request for an ephemeral port, so that API should accept `0` or model the choice explicitly (for example, `BindPort::Ephemeral | BindPort::Explicit(ServicePort)`). A safe `_unchecked` constructor may skip semantic checks only under a documented caller precondition; `unsafe` is justified only when the constructor also relies on a memory-safety invariant that callers must uphold, never merely to avoid a domain check or gain speed.

## Units communicate meaning, but do not prove every property

Wrapping a number in a unit type prevents mixing quantities with different meanings. A `Meters` argument cannot be supplied where `Seconds` is expected, and implementing `Add`, `Mul`, or `Div` can model which operations are physically meaningful. The repository’s units article shows a generic representation that stores a canonical value and uses a type parameter for the unit, allowing values such as meters and millimeters to add after conversion while making length-by-length division produce a scalar rather than another length.

Keep the implementation proportional to the problem. Two wrappers and a few explicit conversions are often enough for application code. A generic dimensional-analysis library, macros for every unit, and implementations of every arithmetic trait are justified when a library serves many units or unit mistakes have serious cost. The type wrapper alone does not prove that a `f64` is finite, nonnegative, or within a physical range: `Meters(f64::NAN)` is still possible if construction is unrestricted. Add validation (`is_finite`, range checks, overflow-aware arithmetic) when that property matters. Do not silently truncate or overflow while converting units; return an error when the conversion cannot represent the result.

## Enums encode a closed choice or state machine

Use an enum when the valid alternatives are a finite, known set and each case needs different behavior or data:

```rust
#[derive(Debug, Clone, Copy)]
enum RotateReason { Full, Timeout }

fn rotate(reason: RotateReason) {
    match reason {
        RotateReason::Full => println!("block was full"),
        RotateReason::Timeout => println!("block timed out"),
    }
}
```

This communicates more than `rotate(true)`, whose meaning is lost at a distant call site. Rust’s exhaustive `match` also makes adding a third reason a compile-time review point. Enum variants can carry the data needed for each state, which often removes illegal combinations represented by several booleans (`connected: bool`, `error: bool`, `retrying: bool`). `Option<T>` remains correct when the only distinction is presence versus absence; introduce a named enum or newtype when “none” has domain meaning such as `Unconfigured`, `Disabled`, or `Unknown`.

An enum is a poor fit for an open-ended value supplied by users or a protocol that may add values independently of this binary. Preserve unknown values with a string/integer newtype or an explicit `Known | Other(...)` design. Use `#[repr(u8)]` or another primitive representation when an ABI or a layout contract requires it, and convert at that boundary. `#[repr]` controls Rust’s memory representation; it does not by itself define a wire format, byte order, versioning, or unknown-value policy. A serializer still needs an explicit schema/codec. Do not rely on the compiler’s default enum layout or transmute an enum to an integer; the [Rust Reference](https://doc.rust-lang.org/reference/items/enumerations.html) says the default representation may use a smaller or otherwise different layout.

## Good and bad patterns

Good domain APIs accept `UserId`, `ServicePort`, `Miles`, or `RotateReason`, construct them through checked or explicit conversions, and expose only operations that preserve their invariant. They validate at trust boundaries, return structured errors for expected bad input, and use exhaustive matches for closed choices. The [compile-time invariant example](https://corrode.dev/blog/compile-time-invariants/) also warns against implementing `DerefMut` blindly: delegating mutable access to an underlying collection can let callers violate a non-empty or sorted invariant.

Bad APIs pass raw `u64`, `u16`, `f64`, or positional booleans everywhere; use aliases when a distinct type is needed; make invariant-bearing fields public; call `unwrap()` on user or network data; or expose unchecked mutation that invalidates the wrapper. These patterns either permit a wrong program to compile or postpone a predictable error until much later. Another bad pattern is wrapper proliferation: a private variable used once, a value that cannot be confused, or an open-ended field does not need a bespoke type. The extra type is worth adding when it prevents a realistic category error, centralizes a rule used in multiple places, documents a public contract, or makes invalid states impossible to represent. Otherwise, ordinary Rust types and a small validation function are clearer.

### Sources from the repository and official Rust docs

- [mre/idiomatic-rust README](https://github.com/mre/idiomatic-rust)
- [Rust API Guidelines: type safety](https://rust-lang.github.io/api-guidelines/type-safety.html)
- [Rust API Guidelines: dependability and validation](https://rust-lang.github.io/api-guidelines/dependability.html)
- [Rust Book: advanced types and newtypes](https://doc.rust-lang.org/book/ch20-03-advanced-types.html)
- [Rust By Example: new type idiom](https://doc.rust-lang.org/rust-by-example/generics/new_types.html)
- [`TryFrom` documentation](https://doc.rust-lang.org/std/convert/trait.TryFrom.html) and [`FromStr`](https://doc.rust-lang.org/std/str/trait.FromStr.html)
- [Rust Reference: enumerations and representation](https://doc.rust-lang.org/reference/items/enumerations.html)
- [Math with distances in Rust](https://www.ferrisellis.com/content/rust-implementing-units-for-types/)
- [Compile-Time Invariants in Rust](https://corrode.dev/blog/compile-time-invariants/)
- [Rust Patterns: Enums Instead Of Booleans](https://blakesmith.me/2019/05/07/rust-patterns-enums-instead-of-booleans.html)
