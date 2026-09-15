# Rust state machines, typestate, and resource guards

The starting point in [`mre/idiomatic-rust`](https://github.com/mre/idiomatic-rust/blob/master/README.md) is useful because it points to both the Rust Design Patterns catalogue and Hoverbear’s [“Pretty State Machine Patterns in Rust”](https://hoverbear.org/blog/rust-state-machine-pattern/). The latter states the central test for a state machine: it has a set of states and defined transitions, and the implementation should make invalid transitions impossible or at least explicit. Rust’s ownership model adds a second dimension: a value can own a resource, and scope can define exactly when that resource is released.

## Pick the state representation from where the state is known

Use an `enum` when the state is data discovered at runtime: protocol messages, user input, a parser, or an event loop. Each variant carries only the data valid for that state, and one exhaustive `match` is a strong correctness check.

The snippets below are illustrative partial APIs; `TcpStream`, `Db`, `Error`, and their operations stand in for domain types supplied by the application or a crate.

```rust
enum Connection {
    Disconnected,
    Connected { socket: TcpStream },
}

impl Connection {
    fn send(&mut self, bytes: &[u8]) -> Result<(), Error> {
        match self {
            Self::Connected { socket } => socket.write_all(bytes).map_err(Error::from),
            Self::Disconnected => Err(Error::NotConnected),
        }
    }
}
```

The bad version is a mutable `state: bool` plus `unwrap`, or an enum transition that panics when called in the wrong state. Hoverbear’s article identifies those costs directly: invalid transitions then fail at runtime and every state-dependent method repeats a match. Returning `Result` is the right runtime contract when an invalid event can legitimately arrive; the caller can retry, reject the message, or close the connection. A panic is appropriate only when the transition represents an internal invariant that safe, trusted code has already promised, not for ordinary network or user input. The Rust Book’s [error-handling chapter](https://doc.rust-lang.org/book/ch09-02-recoverable-errors-with-result.html) describes `Result` as the value that lets the caller decide how to handle a failure and `?` as the concise propagation mechanism.

Use typestate when the state is a property of the *program’s construction path* and an invalid operation is a programming error. Encode each state as a marker type and put only valid methods on the corresponding specialization:

```rust
struct Client<S> { socket: TcpStream, state: S }
struct Disconnected;
struct Connected;

impl Client<Disconnected> {
    fn connect(self) -> Result<Client<Connected>, Error> {
        // handshake can still fail at runtime
        todo!()
    }
}

impl Client<Connected> {
    fn send(&mut self, bytes: &[u8]) -> Result<(), Error> { todo!() }
    fn disconnect(self) -> Client<Disconnected> { todo!() }
}
```

The transition consumes `self`, so the old state cannot be reused. A call to `send` on `Client<Disconnected>` fails to compile, while handshake failure remains a `Result`: types prove the phase after success; I/O proves whether success happened. This is a good fit for builders with required phases, FFI handles, lock ownership, and protocols where “must authenticate before send” has a small, stable graph of transitions. The article shows the same generic-parent shape (`Machine<State>`) and notes its useful properties: valid transitions are compile-time checked, the old value is consumed, and common fields need not be repeated. The [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/type-safety.html) give the broader rule: use deliberate types to convey invariants instead of ambiguous booleans or `Option`s.

Typestate is a bad fit when the number of states is large or data-dependent, callers need to store many phases together, or transitions are driven by untrusted runtime events. It adds public type noise, can require an enum or trait object at a heterogeneous boundary, and does not eliminate runtime failure from the actual operation. A practical hybrid is common: keep a runtime enum for the outer event loop, then return a typed value for a short critical phase whose misuse would be costly.

## RAII makes ownership and scope the cleanup protocol

Rust drops initialized values automatically when their scope ends or a value is overwritten. The [Reference’s destructor rules](https://doc.rust-lang.org/reference/destructors.html) specify that a type’s `Drop::drop` runs before its fields, struct fields are dropped in declaration order, and local variables leave a scope in reverse declaration order. That gives a reliable basis for RAII: acquire a resource in a constructor returning `Result`, store ownership in a guard, and let the guard’s `Drop` release it.

This is why `MutexGuard`, `RwLockWriteGuard`, `RefMut`, file handles, and temporary directory guards are idiomatic. A guard ties access to a lifetime and normally releases it on early return or during panic unwinding. Use `std::mem::drop(guard)` when the resource must be released before the lexical scope ends; the standard library documents this as moving the value into a function so it is dropped immediately ([`mem::drop`](https://doc.rust-lang.org/std/mem/fn.drop.html)). Keep guards short. An async mutex guard is specifically designed to be held across `.await` ([Tokio’s mutex guidance](https://docs.rs/tokio/latest/tokio/sync/struct.Mutex.html)), but the critical section should still be short because other tasks wait; a `std::sync::MutexGuard` across `.await` can be non-`Send` and can deadlock in practice.

RAII is a cleanup convention, not a guarantee that `Drop` always runs. `panic = "abort"` ends a panic without unwinding; [`std::process::exit`](https://doc.rust-lang.org/std/process/fn.exit.html) terminates without running stack destructors; and [`mem::forget`](https://doc.rust-lang.org/std/mem/fn.forget.html) deliberately skips destruction. The `forget` documentation makes the safety boundary explicit: Rust’s safety guarantees do not include a promise that destructors always run, so unsafe code must remain sound if its caller leaks a returned value. Never make memory safety, a lock protocol, or an FFI obligation depend on a destructor eventually executing. Design `Drop` as the normal cleanup path and an important best effort, while treating abort, process termination, and intentional leaks as possible exits.

The bad RAII pattern is to perform a fallible, essential operation only in `Drop`:

```rust
impl Drop for Transaction<'_> {
    fn drop(&mut self) {
        self.db.commit().unwrap(); // wrong: no error can be returned here
    }
}
```

`Drop::drop` returns `()`. The [standard documentation](https://doc.rust-lang.org/std/ops/trait.Drop.html) says implementations should generally avoid panicking because cleanup can run while another panic is unwinding; a second panic can abort the process. `Drop` should therefore do infallible cleanup or best-effort rollback. If completion matters, expose an explicit consuming operation that returns `Result`, and make `Drop` the safety net:

```rust
struct Transaction<'a> {
    db: &'a mut Db,
    committed: bool,
}

impl Transaction<'_> {
    fn commit(mut self) -> Result<(), Error> {
        self.db.commit()?;
        self.committed = true;
        Ok(())
    }
}

impl Drop for Transaction<'_> {
    fn drop(&mut self) {
        if !self.committed {
            self.db.rollback_best_effort();
        }
    }
}
```

The explicit method communicates success and can report a database error; the destructor preserves the rollback invariant on early returns. If rollback itself can fail, record or surface that through an application-specific channel, poison the resource, or design the resource so rollback is local and infallible. Do not pretend `Drop` delivered an error to its caller.

The decision rule is compact: use an enum and `Result` when the environment chooses the next state; use typestate when the API author can statically define legal phases; use RAII guards when lifetime and cleanup are the invariant. Keep `Drop` for cleanup, use explicit methods for operations whose failure the caller must observe, and let ownership consume a state transition when stale values would be dangerous.
