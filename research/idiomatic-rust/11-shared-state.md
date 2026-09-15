# Shared state in idiomatic Rust: ownership, locks, and atomics

The `idiomatic-rust` collection points readers to Mara Bos’s *Rust Atomics and Locks* and to Brooks’s “Sneaky Deadlock With `if let` Blocks,” which together give the right theme: choose the smallest synchronization mechanism that expresses the invariant, and make guard lifetimes visible. Rust’s `Send` and `Sync` bounds then make many bad choices compile-time errors, but they do not prove that a lock order is deadlock-free or that an atomic protocol is logically correct.

## Choose ownership before synchronization

If one thread owns the state, pass `&mut T` or move `T`; this is simpler and faster than shared mutability. For read-only data shared by threads, `Arc<T>` is appropriate. `Rc<T>` is the cheaper, single-threaded reference-counted pointer: its count is non-atomic and it is explicitly `!Send` and `!Sync`. Use it for DAGs, GUI state, or other one-thread object graphs; pair it with `RefCell<T>` only when that graph needs mutation through aliases. A `Weak<T>` edge is the usual way to break parent/child cycles, because strong cycles keep allocations alive forever.

`Arc<T>` uses atomic reference-count operations, so it costs more than `Rc<T>`. It does not make the payload thread-safe: `Arc<T>` is `Send + Sync` only when `T` is. Thus `Arc<RefCell<T>>` cannot satisfy a cross-thread `Send`/`Sync` bound because `RefCell` performs non-atomic runtime borrow tracking. It can exist in one thread, but `Rc<RefCell<T>>` is usually the clearer and cheaper choice there. For mutable cross-thread state, the usual forms are `Arc<Mutex<T>>`, `Arc<RwLock<T>>`, or `Arc<AtomicUsize>`/another suitable atomic. Prefer `Arc::clone(&shared)` at handoff sites; it makes the ownership operation visually distinct from cloning the payload.

Good single-threaded interior mutability:

```rust
use std::cell::{Cell, RefCell};

let visits = Cell::new(0_u32);       // Copy/set/replace; no references out
visits.set(visits.get() + 1);

let names = RefCell::new(Vec::new());
names.borrow_mut().push("Ada");     // dynamic borrow checked at runtime
```

`Cell<T>` is ideal for small `Copy` values or whole-value replacement. Through `&Cell<T>` it cannot hand out `&T`/`&mut T`; `get_mut(&mut self)` can return `&mut T` when the caller already has exclusive access. It is not `Sync`. `RefCell<T>` can expose borrowed references, but `borrow`/`borrow_mut` panic on an overlapping incompatible borrow; use `try_borrow` or `try_borrow_mut` when a panic is not an acceptable failure mode. A plain mutable reference is still the best pattern when the API can obtain one. Do not use `RefCell` to hide an ownership design problem.

## Mutexes, reader-writer locks, and guard lifetimes

`Mutex<T>` gives one exclusive guard. `RwLock<T>` permits concurrent readers and one writer, so it fits demonstrably read-heavy data with short reads. A `Mutex` is often the better default: it has fewer states, no reader/writer upgrade issue, and avoids depending on platform-specific fairness. The standard `RwLock` does not promise writer or reader priority. Neither lock is reentrant; locking the same mutex from its owning thread may panic or deadlock, and upgrading a read lock to a write lock can deadlock.

Keep critical sections short and let the guard’s lifetime document the section. Extract a copy or owned result, then drop the guard before calling external code or acquiring another lock:

```rust
let snapshot = { shared.lock().unwrap().status.clone() };
send_to_callback(snapshot); // no lock held during arbitrary code
```

This is bad when the callback can call back into `shared`:

```rust
let guard = shared.lock().unwrap();
send_to_callback(guard.status.clone()); // callback can block forever
```

The compiler tracks the guard’s borrow lifetime, but it cannot infer your global lock-order policy. Establish one order for multiple locks, or redesign to avoid holding one lock while acquiring another. The linked deadlock article shows a particularly sneaky case: in Rust 2021 and earlier, a guard created in an `if let` scrutinee can live through the `else` arm. This can deadlock:

```rust
if let Some(value) = *map.read().unwrap() {
    println!("{value}");
} else {
    *map.write().unwrap() = Some(5); // read guard may still be alive
}
```

Copy the condition in a separate statement or use an explicit block. If another thread can initialize the value between those statements, recheck while holding the write lock:

```rust
let initialized = { map.read().unwrap().is_some() };
if !initialized {
    let mut value = map.write().unwrap();
    value.get_or_insert(5);
}
```

The explicit read scope prevents the deadlock, while `get_or_insert` under the exclusive lock prevents a racing initializer from being overwritten. If the write is cheap, taking the write lock once and calling `get_or_insert` is simpler.

Edition 2024 shortens `if let` scrutinee temporaries before entering `else`, removing this specific lifetime trap, but explicit scopes remain clearer and portable across editions. Migration can change destructor timing; review `if_let_rescope` warnings. A `match` preserves the older extended-scrutinee behavior, so it is not automatically the deadlock fix.

Lock poisoning is an invariant signal, not a lock-recovery algorithm. If a thread panics while holding a `Mutex`, later `lock()` calls return `PoisonError`; `unwrap()` deliberately propagates the panic. Recover with `into_inner()` only after validating or replacing the protected state, then call `clear_poison()` once it is known-good. Poisoning is advisory and detection is not guaranteed in every panic path, so correctness must not rely on it. `RwLock` poisoning generally occurs only for a panic while holding the exclusive write lock, not for a reader panic.

## Atomics and `Send`/`Sync`

Atomics provide indivisible operations on selected primitive types. They are the right tool for an independent counter, flag, or state word; they are not a drop-in replacement for a multi-field invariant. `Relaxed` provides atomicity only for the touched value and does not publish other memory. A `Release` store paired with an `Acquire` load publishes preceding writes when the acquire reads the value written by that release (or its release sequence); `SeqCst` adds one total order and is easier to reason about when a protocol genuinely needs it. Choose orderings from a stated happens-before argument, then test the protocol under contention; choosing `Relaxed` merely because it is fast is a correctness bug.

`Send` means ownership may move to another thread. `Sync` means `&T` may be shared across threads (`T: Sync` exactly when `&T: Send`). They are unsafe marker traits, automatically derived from fields. Unsafe manual implementations therefore require proving every access and destruction path; an incorrect implementation can invalidate safe code. `std::sync::MutexGuard` is intentionally `!Send` in the standard API for portability: on POSIX it must be dropped on the thread that acquired the lock. This statement is specific to that guard type; another lock implementation may choose different guard-transfer semantics and must uphold its own unlock requirements. Lifetimes on returned references describe relationships and cannot extend an underlying lock, `RefCell`, or stack value; return owned data when the guard must be dropped.

## Sources

- [idiomatic-rust README](https://github.com/mre/idiomatic-rust) (links to the two non-official resources below)
- [Rust Atomics and Locks](https://mara.nl/atomics/)
- [Sneaky `if let` deadlock](https://brooksblog.bearblog.dev/rusts-sneaky-deadlock-with-if-let-blocks/)
- [Rc](https://doc.rust-lang.org/std/rc/struct.Rc.html), [Arc](https://doc.rust-lang.org/std/sync/struct.Arc.html), [Cell](https://doc.rust-lang.org/std/cell/struct.Cell.html), and [RefCell](https://doc.rust-lang.org/std/cell/struct.RefCell.html)
- [Mutex](https://doc.rust-lang.org/std/sync/struct.Mutex.html), [RwLock](https://doc.rust-lang.org/std/sync/struct.RwLock.html), and [poison recovery](https://doc.rust-lang.org/std/sync/struct.Mutex.html#method.clear_poison)
- [Atomic types](https://doc.rust-lang.org/std/sync/atomic/index.html) and [memory orderings](https://doc.rust-lang.org/std/sync/atomic/enum.Ordering.html)
- [Send and Sync](https://doc.rust-lang.org/book/ch16-04-extensible-concurrency-sync-and-send.html), [Rustonomicon details](https://doc.rust-lang.org/nomicon/send-and-sync.html), and [2024 `if let` temporary scope](https://doc.rust-lang.org/edition-guide/rust-2024/temporary-if-let-scope.html)
