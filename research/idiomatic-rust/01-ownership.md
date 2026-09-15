# Ownership, borrowing, and lifetimes in this MUI repository

Research date: 2026-09-13. Scope: the ownership and lifetime advice linked by [mre/idiomatic-rust](https://github.com/mre/idiomatic-rust), checked against the [Rust Book](https://doc.rust-lang.org/stable/book/ch04-00-understanding-ownership.html), [Rust Reference lifetime-elision rules](https://doc.rust-lang.org/stable/reference/lifetime-elision.html), and the current repository code.

Rust’s useful split is simple: the owner decides when a value is dropped; a borrow temporarily accesses that value without taking ownership. Assigning or passing a non-`Copy` value moves it, while `&T` permits shared reads and `&mut T` permits one exclusive reader/writer. This gives the compiler enough information to reject use-after-free, dangling references, and conflicting mutation before running the program. A function should therefore borrow when it only needs a temporary view, take ownership when it must store or transform the value independently, and return an owned value when the result must outlive local inputs.

The repository already shows this boundary clearly in [`App`](</mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-preview/src/main.rs:157>): it owns `Vec<Box<dyn PreviewScene>>`; each `Box` owns one heap-allocated scene while dynamic dispatch permits different scene implementations in one vector. `App` also has `font: Arc<Vec<u8>>`, and passes `font.clone()` to `Chrome::new`: cloning the `Arc` shares the allocation and increments a counter instead of copying the font bytes. Use `Arc` when shared ownership must cross an ownership boundary (and possibly threads); `Rc` is the cheaper single-threaded alternative when thread-safety is not required. Neither pointer removes the need to reason about mutability, cycles, or the lifetime of resources.

`Baked::build(scenes[0].as_ref(), &font)` is a good transient-borrow pattern: `App` remains the owner of the scene and font while `build` receives only the access it needs. Likewise, [`Hit::push`](</mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-input/src/lib.rs:53>) borrows the source `Path`, converts it immediately into an owned renderer path, and stores an owned `String` from `id`. No borrow of the caller escapes into `Hit`, so the type needs no lifetime parameter. This is often the cleanest API for caches, registries, and render data: borrow at the input boundary, own the data that must survive the call.

## Good and bad API shapes

```rust
// Bad: the caller gives up ownership for a read-only calculation.
fn name_len(name: String) -> usize {
    name.len()
}

// Good: accepts String, &str, and other string-like callers without moving data.
fn name_len(name: &str) -> usize {
    name.len()
}
```

The first version forces a `String` move and makes the caller clone or rebuild it if it needs the value afterward. The second expresses the actual requirement and leaves ownership unchanged. Taking `String` is still correct when the function stores it, must mutate and return it, or intentionally consumes it. Do not clone merely to silence a move error: first ask whether the callee can borrow, whether a scoped expression can finish the borrow, or whether ownership should be transferred deliberately.

Lifetimes exist on every reference, but they are usually inferred. The [current Reference rules](https://doc.rust-lang.org/stable/reference/lifetime-elision.html) assign each elided input its own lifetime; with one input lifetime, an elided output borrows from that input, and a method output normally borrows from `self`. Thus this explicit annotation adds noise without adding information:

```rust
// Usually needlessly explicit.
fn trim<'input>(value: &'input str) -> &'input str { value.trim() }

// Idiomatic equivalent under lifetime elision.
fn trim(value: &str) -> &str { value.trim() }
```

That is the central advice of [Don't Worry About Lifetimes](https://corrode.dev/blog/lifetimes/) (published 2024-05-29, updated 2025-03-27): do not add annotations speculatively. Add them when the compiler requires a relationship, when a profiled hot path benefits from avoiding allocation, or when an explicit relationship documents a complex API. A lifetime is not a duration to tune; it is a constraint saying which owner a borrow is tied to.

Explicit lifetimes become necessary when multiple inputs could source the output. The classic example is:

```rust
fn longest<'input>(left: &'input str, right: &'input str) -> &'input str {
    if left.len() >= right.len() { left } else { right }
}
```

`'input` says the returned slice is valid only for the common region in which both inputs are valid; it does not extend either string’s lifetime. A bad version would return `&str` with two elided inputs: the compiler cannot infer whether the result comes from `left` or `right` without analyzing the function body. When a type borrows from multiple owners, name the origins when that improves reviewability. [Naming Your Lifetimes](https://www.possiblerust.com/pattern/naming-your-lifetimes) demonstrates `AuthorView<'art, 'auth>` for article and author providers and recommends semantic names such as `'config`, `'arena`, or `'de` when short `'a`/`'b` names would obscure which owner supplies which field. Keep one obvious lifetime short and elided; add names when there are genuinely distinct sources or a long-lived arena/config owner.

## Solving borrow errors without fighting the model

The useful repair is usually to shorten a borrow or change the ownership boundary. This keeps the original allocation and makes the sequence explicit:

```rust
let name = String::from(" Herman ");
let trimmed_len = { name.trim().len() }; // borrow ends with the block
let owned_name = name;                   // move is now valid
```

A helper such as `fn len_of_trimmed(name: &str) -> usize` works for a reusable operation. If a closure captures `name` from its environment, it may retain that borrow in the closure value; passing `&name` as a closure parameter gives the borrow a narrower call lifetime. Clone only when an independent owned copy is the intended result. If a mutable borrow and immutable borrow overlap, split the operation into phases or use disjoint fields; `&mut` exclusivity is the safety guarantee, not an arbitrary obstacle.

Returning a reference to a local is always wrong and is rejected:

```rust
// Bad: `local` is dropped on return.
fn greeting() -> &str {
    let local = String::from("hello");
    &local
}

// Good: transfer ownership to the caller.
fn greeting() -> String { String::from("hello") }
```

Avoid using `'static` as an escape hatch. It means the reference is valid for the whole program, which is appropriate for string literals and genuinely global data, but usually signals that the function should return owned data, use an `Arc`, or redesign its storage. The corrode article explicitly presents `Rc`/`Arc` as alternatives when shared ownership is preferable to threading borrowed lifetimes through a design.

One dated caveat matters for the linked [2015 “cannot move out of” article](https://hermanradtke.com/2015/06/09/strategies-for-solving-cannot-move-out-of-borrowing-errors-in-rust.html): its scope-based advice remains useful, but modern Rust has non-lexical lifetimes enabled by default since Rust 1.63 ([official announcement](https://blog.rust-lang.org/2022/08/05/nll-by-default/)). A borrow often ends at its last use before the end of the enclosing block, so first try the smallest reorder or expression scope. If that still fails, extract a helper or explicitly create a scope. The invariant has not changed: no move or mutation may occur while a live reference still needs the old value.

The practical rule for this repo is: own scenes, cached render paths, and bytes that outlive one call; borrow geometry and fonts while computing; use `&mut` only for the state being changed; add a lifetime parameter only when a returned or stored reference must document a real relationship; and prefer `Arc`/owned handles when several components must share data without coupling their APIs to one borrow owner.
