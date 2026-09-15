# Idiomatic Rust: traits, generics, `impl Trait`, and `dyn`

The source repository describes itself as a peer-reviewed collection of resources for clean Rust, not as a language specification. Its linked Rust API Guidelines make the same distinction: the recommendations are important interoperability considerations, but are not mandates. For this topic the most relevant recommendations are C-GENERIC (minimize assumptions with generics), C-OBJECT (keep potentially useful trait objects object-safe), C-STRUCT-BOUNDS (avoid unnecessary bounds on data structures), C-SEALED (seal traits when downstream implementations would prevent evolution), and C-NEWTYPE-HIDE (hide representation behind a stable type). Sources: [idiomatic-rust](https://github.com/mre/idiomatic-rust), [API Guidelines checklist](https://rust-lang.github.io/api-guidelines/checklist.html).

## Start with the smallest contract

A generic function should state what it actually needs. If the operation only iterates, this is more correct and reusable:

```rust
fn total(values: impl IntoIterator<Item = i64>) -> i64 {
    values.into_iter().sum()
}
```

It accepts arrays, vectors, ranges, and custom iterators without forcing callers to expose a particular container. `&Vec<i64>` is a bad pattern: it assumes storage instead of behavior and rejects types that can provide a slice or iterator. A bound such as `T: Clone + Debug + Send + Sync + 'static` is also bad when the body only needs `T: Display`; every extra bound narrows callers and becomes part of the API contract. Use a `where` clause when several bounds would obscure the signature. Rust’s Reference explains that bounds grant access to the associated items and methods promised by the trait; `?Sized` is the specific escape hatch for the implicit `Sized` bound on type parameters and associated types. See [trait and lifetime bounds](https://doc.rust-lang.org/reference/trait-bounds.html).

Generics normally mean static dispatch: the compiler monomorphizes each used concrete type, enabling direct calls, inlining, and layout without an extra pointer. That is a correctness and performance win when the set of types is known at compile time, but it can increase code size and compile time. A public generic wrapper can call a private non-generic implementation when monomorphization has become a measured problem; do not “de-genericize” speculatively. The API Guidelines explicitly list generic reusability and static optimization as benefits, while warning about code size, homogeneous collections, and verbose signatures ([C-GENERIC](https://rust-lang.github.io/api-guidelines/flexibility.html#c-generic)).

## Associated types express one canonical output

Use an associated type when an implementation has one logically fixed output for a given implementing type:

```rust
trait Parser {
    type Output;
    fn parse(&self, input: &str) -> Result<Self::Output, ParseError>;
}
```

This makes `Parser::Output` part of the implementation’s contract and avoids repeating a type parameter at every use. A generic trait parameter is better when one type can implement the relationship more than once with different outputs, for example `trait Convert<T> { type Error; fn convert(T) -> ... }`. The Reference defines associated types as aliases associated with an implementation and supports generic associated types (GATs) for lifetime-dependent outputs. A bad pattern is introducing a generic parameter merely to encode a one-to-one relationship, or exposing a giant concrete iterator type that leaks implementation details. Use `impl Iterator` or a named newtype when the output should be abstract and evolvable.

## `impl Trait` has two different jobs

In an argument, `impl Trait` is anonymous generic syntax:

```rust
fn log_all(messages: impl IntoIterator<Item = String>) { /* ... */ }
```

The caller supplies a type satisfying the bound, but cannot explicitly select a generic argument as with `fn log_all<I: IntoIterator<...>>(...)`. Changing between these forms can therefore break callers that use turbofish syntax. In a return position, `impl Trait` is an opaque, concrete type chosen by the function:

```rust
fn numbers() -> impl Iterator<Item = u32> {
    0..10
}
```

Every return path must resolve to the same concrete type. This is good for closures and iterator chains: callers see only the useful capability, while the library avoids a heap allocation and vtable. It is incorrect to return two unrelated iterator types from `if` branches under one `impl Iterator`; combine them with a common adapter, an enum, or `Box<dyn Iterator>` when the choice is genuinely dynamic. `impl Trait` also cannot be a struct field or general type alias. See the [Rust Reference on impl Trait](https://doc.rust-lang.org/reference/types/impl-trait.html) and the API Guidelines’ warning that public opaque returns trade concise signatures for limited expressibility ([C-NEWTYPE-HIDE](https://rust-lang.github.io/api-guidelines/future-proofing.html#c-newtype-hide)).

## Choose `dyn` for runtime heterogeneity

Use a trait object when the program must hold different concrete implementations together or select behavior at runtime:

```rust
trait Handler {
    fn handle(&self, request: &Request) -> Response;
}

fn dispatch_all(handlers: &[Box<dyn Handler>], request: &Request) -> Vec<Response> {
    handlers.iter().map(|h| h.handle(request)).collect()
}
```

`dyn Handler` is dynamically sized and is used behind a pointer such as `&dyn Handler` or `Box<dyn Handler>`. The pointer carries a data pointer and vtable; calls use dynamic dispatch, which adds indirection and usually blocks inlining. That cost is justified for plugin registries, heterogeneous collections, dependency injection boundaries, or a small number of runtime-selected calls. It is a bad pattern to box every value “for abstraction” when a generic parameter or enum provides a closed, statically known set.

Modern documentation calls the relevant property *dyn compatibility* (formerly object safety). A dyn-compatible trait has no `Self: Sized` supertrait, no associated constants, no generic associated types, and dispatchable methods must have an allowed receiver (`&self`, `&mut self`, `Box<Self>`, `Rc<Self>`, `Arc<Self>`, or permitted `Pin` forms), no type parameters, no extra `Self` in arguments or returns, and no `async fn` or return-position `impl Trait`. A method that is useful only for concrete implementers can be excluded with `where Self: Sized`:

```rust
trait Handler {
    fn handle(&self, request: &Request) -> Response;
    fn clone_handler(&self) -> Self where Self: Sized;
}
```

The trait remains usable as `dyn Handler`, but `clone_handler` cannot be called through the object. Do not force dyn compatibility if the trait’s central value is generic methods or type-level outputs; split a dyn-facing trait from extension functionality instead. Sources: [Rust Reference dyn compatibility](https://doc.rust-lang.org/reference/items/traits.html#dyn-compatibility), [trait objects](https://doc.rust-lang.org/reference/types/trait-object.html), and [The Book’s static/dynamic dispatch discussion](https://doc.rust-lang.org/book/ch18-02-trait-objects.html).

## Evolve public traits deliberately

An ordinary public trait is open to downstream implementations; adding a required method is consequently a breaking change. If only the defining crate should implement it, use a private supertrait and document that it is sealed:

```rust
mod private { pub trait Sealed {} }
pub trait FastHash: private::Sealed { fn hash(&self) -> u64; }
```

The API Guidelines say sealing preserves the ability to add methods or change undocumented internals, while removing or changing documented public methods remains breaking ([C-SEALED](https://rust-lang.github.io/api-guidelines/future-proofing.html#c-sealed)). Conversely, sealing an extension point that users reasonably need is a bad pattern because it blocks legitimate implementations.

Finally, keep derivable bounds off generic structs:

```rust
#[derive(Clone, Debug)]
struct Cache<T> { value: T }
```

Writing `struct Cache<T: Clone + Debug>` unnecessarily makes the type itself unusable with otherwise valid `T`s and makes later bound changes breaking. Put bounds on the methods that need them. Exceptions include a required associated-type projection, `?Sized`, or a `Drop` implementation whose bounds Rust requires on the struct. This is the practical test for a “correct” pattern: every bound should correspond to an operation or invariant, every dispatch choice should follow the runtime/static requirement, and every public abstraction should promise no more representation than clients need.
