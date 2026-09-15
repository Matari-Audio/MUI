# Unsafe Rust and FFI: small, auditable proofs at the boundary

The [idiomatic-rust repository](https://github.com/mre/idiomatic-rust) points to Canonical's [Rust best practices](https://canonical.github.io/rust-best-practices/), the [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/), and Immunant's [Refactoring Rust Transpiled from C](https://immunant.com/blog/2020/09/transpiled_c_safety/). Their shared rule is to isolate unavoidable unsafe code, state checkable invariants, and express ownership and failure in Rust's types. Merely changing `*mut T` spelling leaves a C-style unsafe translation.

## What “correct” means

`unsafe` is a proof obligation, not a performance or “trust me” annotation. The [Rust Reference's unsafe chapter](https://doc.rust-lang.org/reference/unsafe-keyword.html) says an unsafe function defines conditions callers must satisfy, while an unsafe block asserts that its operations' conditions have been discharged. Canonical's [unsafe discipline](https://canonical.github.io/rust-best-practices/unsafe-discipline.html) therefore recommends minimizing the scope and documenting each precondition with `// SAFETY:`. A small block beside a checked conversion is auditable; a 100-line `unsafe fn` hides which state makes each dereference valid.

For an FFI declaration, the declaration itself is a proof claim. In the 2024 edition, write `unsafe extern "C"` and make each item `unsafe` unless it is safe for every possible input. The [Reference](https://doc.rust-lang.org/reference/items/external-blocks.html) makes the author responsible for matching the foreign signature: argument and return types, ABI, calling convention, variadic rules, and statics. A declaration that gets one of these wrong can cause undefined behavior before a wrapper gets a chance to validate anything.

Use `#[repr(C)]` on structs, unions, and enums exchanged with C; ordinary Rust layout is not a C contract. The [Reference's layout guarantees](https://doc.rust-lang.org/reference/type-layout.html) also explain a common trap: a C enum can hold an arbitrary integer, whereas a Rust fieldless enum only permits its declared discriminants. Model C enums with a target-appropriate integer or a validated newtype when unknown values are possible. `repr(C)` fixes layout, not pointer validity, ownership, thread safety, or lifetimes.

## The good wrapper pattern

Keep raw declarations private; expose a safe API that checks results and owns resources with `Drop`:

```rust
#[repr(C)]
struct CThing { _private: [u8; 0], _marker: std::marker::PhantomData<(*mut u8, std::marker::PhantomPinned)> }

unsafe extern "C" {
    fn thing_new(size: usize) -> *mut CThing;
    fn thing_free(p: *mut CThing);
}

pub struct Thing(std::ptr::NonNull<CThing>);

impl Thing {
    pub fn new(size: usize) -> Option<Self> {
        // SAFETY: the assumed foreign contract accepts every size, requires
        // no prior initialization, and returns an owned handle or null.
        let p = unsafe { thing_new(size) };
        std::ptr::NonNull::new(p).map(Self)
    }
}

impl Drop for Thing {
    fn drop(&mut self) {
        // SAFETY: this pointer came from thing_new and is freed exactly once here.
        unsafe { thing_free(self.0.as_ptr()) }
    }
}
```

The opaque type prevents accidentally passing a `Bar*` to a `foo(Thing*)` API. The marker makes the pointee `!Send`, `!Sync`, and `!Unpin`, but `NonNull<CThing>` does not propagate `!Unpin`; if the *handle* must not move, make `Thing` itself `!Unpin` (for example with `PhantomPinned`) and expose the appropriate pinned API. The [Rustonomicon's FFI chapter](https://doc.rust-lang.org/nomicon/ffi.html) recommends this shape, `CString`/`CStr`, and destructors for foreign resources. Pair allocators and deallocators: never turn C `malloc` into `Box::from_raw`, or free a Rust `Box` with C `free`.

For borrowed buffers, return a slice only after proving every condition required by [`slice::from_raw_parts`](https://doc.rust-lang.org/std/slice/fn.from_raw_parts.html): pointer is non-null and aligned, points to initialized `T`s in one allocation, `len * size_of::<T>()` fits in `isize`, and no mutation or conflicting access occurs for the borrow's lifetime. Even a zero-length slice still requires a non-null aligned pointer; branch on a C null pointer first. If C can mutate or free the memory, copy into a Rust-owned `Vec` or keep the result as a raw handle and synchronize it. Do not hold a Rust reference across a call that may reallocate, mutate, or destroy the object.

Callbacks need the same lifetime discipline. Pass a stable context pointer (usually a pinned or boxed object), use the exact `extern "C" fn` ABI, and unregister the callback before dropping the context. If C can call concurrently, synchronize access and only implement `Send`/`Sync` after proving the library's thread contract. Raw pointers are deliberately neither `Send` nor `Sync`; an `unsafe impl` that guesses is unsound. A callback should catch or encode Rust failures rather than letting a panic cross an uncontrolled boundary.

## What goes wrong

This looks plausible but is undefined behavior:

```rust
// Bad: C supplied arbitrary bytes; this assumes alignment, initialization,
// a complete u32, and a still-live allocation.
unsafe { *(bytes as *const u8 as *const u32) }
```

Use a checked byte-length conversion and `read_unaligned`, or copy bytes into an aligned `u32`. The [pointer documentation](https://doc.rust-lang.org/std/ptr/index.html) requires validity for the size of the access; most operations also require alignment. It also describes pointer provenance: a pointer is more than an integer address and may only access its allocation. Converting a pointer to `usize`, doing arithmetic, and casting back can lose provenance. Prefer pointer offsets from a live base, or strict-provenance APIs such as `with_addr`/`map_addr`. `wrapping_offset` does not grant permission to dereference elsewhere.

Other bad patterns are retaining a `Vec` pointer across growth, creating a reference from a nullable/dangling C pointer, assuming `&'static` because C “usually” keeps data alive, or exposing C mutable globals without synchronization. Immunant's C2Rust refactoring replaces an owning allocation pointer with `Vec` and a movable raw view with an index, avoiding a self-referential struct. Map C null returns to `Option`/`Result`, not fabricated references.

## ABI and unwinding choices

Use the platform's correct ABI (`"C"`, `"system"`, or another documented convention), and use `Option<extern "C" fn(...)>` for nullable C function pointers rather than transmuting null into a function pointer; the nullable-pointer optimization is guaranteed by the Nomicon. For an exported Rust function, distinguish checks that are actually possible (null, integer overflow, known range) from caller obligations that a raw `ptr`/`len` pair cannot prove (live allocation, provenance, initialization, and aliasing). Make those obligations explicit in the C contract and, when exposing a Rust API, use an `unsafe fn` for them; otherwise redesign around an owned copy or an opaque handle. Return an error code or out-parameter status, and wrap fallible Rust code in `catch_unwind` if the C caller must survive it.

The non-unwinding `"C"` ABI must not be used to let C++ exceptions enter Rust. If intentional Rust/C++ unwinding is part of the design, both declarations and definitions need `"C-unwind"` and compatible runtimes. The [Reference's panic rules](https://doc.rust-lang.org/reference/panic.html) and the Nomicon note that a foreign exception crossing a non-unwinding boundary is undefined behavior. [`catch_unwind`](https://doc.rust-lang.org/stable/std/panic/fn.catch_unwind.html) catches unwinding Rust panics only; aborting panics are not caught. With an appropriate `"C-unwind"` ABI, catching a foreign exception has unspecified behavior: the process may abort after destructors run, or `catch_unwind` may return an opaque `Err`. Most libraries should keep the boundary non-unwinding and convert errors explicitly.

## Limits and unresolved semantics

Rust's pointer provenance and aliasing model is still being specified. The [standard pointer documentation](https://doc.rust-lang.org/std/ptr/index.html) explicitly says precise validity rules are not determined, while the [Unsafe Code Guidelines glossary](https://rust-lang.github.io/unsafe-code-guidelines/glossary.html) describes provenance and validity as active specification work. C's ABI details, enum representation, allocator behavior, effective type, and C++ exception runtime are likewise target/toolchain facts, not inferred from Rust types. Treat these as audit inputs: inspect headers and compiler flags, test each supported target, use bindgen/cbindgen where appropriate, and run sanitizers/Miri for the subset they understand. The durable idiom is conservative: preserve allocation provenance, avoid references unless their lifetime and aliasing are proved, keep unsafe blocks tiny, and make ownership transitions one-way and visible.
