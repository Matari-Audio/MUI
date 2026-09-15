# Idiomatic async Rust: tasks, threads, cancellation, and flow control

This note follows the async/concurrency links in [`mre/idiomatic-rust`](https://github.com/mre/idiomatic-rust), especially [“Why choose async/await over threads?”](https://notgull.net/why-not-threads/), [“Cancelling async Rust”](https://sunshowers.io/posts/cancelling-async-rust/), and [“Async Rust can be a pleasure to work with (without `Send + Sync + 'static`)”](https://emschwartz.me/async-rust-can-be-a-pleasure-to-work-with-without-send-sync-static/). The [Rust Async Book](https://rust-lang.github.io/async-book/) and current [Tokio documentation](https://tokio.rs/tokio/tutorial/) provide the normative API details. The practical test for “idiomatic” here is correctness under ownership, cancellation, load, and shutdown—not merely a short-looking function.

## Start with the execution model

An `async fn` returns a future, and a Rust future is passive: it makes no progress until an executor polls it. `.await` may yield the current task, and dropping a not-yet-complete future stops further polling of that future; effects already handed to an external system are governed by that system. This is why `let a = fetch_a().await; let b = fetch_b().await;` is sequential, while `tokio::join!(fetch_a(), fetch_b())` runs both futures concurrently in one task. `tokio::spawn` creates a separate runtime-managed task, which may run in parallel on another worker thread. The [Async Book’s comparison](https://rust-lang.github.io/async-book/01_getting_started/02_why_async.html) describes async as cheap user-space tasks and threads as OS-scheduled units with more memory and switching overhead.

Choose async when there are many mostly-waiting operations (sockets, timers, database requests), or when composing timeout, race, stream, and bounded-queue behavior is central. Choose ordinary threads when there are only a few activities, existing synchronous/blocking code is the main asset, thread priority or OS integration matters, or work is CPU-bound. Threads are preemptive, so one accidentally long computation does not prevent another thread from running. Async scheduling is cooperative: a task that performs blocking I/O or a long CPU loop without yielding can stall every task on that executor thread. The models are complementary; a server commonly uses async for network orchestration and threads or a CPU pool for computation.

Async is not automatically faster. The linked [threads comparison](https://notgull.net/why-not-threads/) explicitly cautions that threaded code can win for CPU-bound work and that async’s strongest argument is its composability for large I/O concurrency. Benchmark the actual workload rather than treating “async” as a performance annotation.

## Ownership, `Send`, and `'static`

The bounds at a spawn boundary describe the executor, not async Rust itself. [`std::thread::spawn`](https://doc.rust-lang.org/std/thread/fn.spawn.html) requires a `Send + 'static` closure and return value because the new thread may outlive its caller and values must cross threads. Tokio’s [`spawn`](https://tokio.rs/tokio/tutorial/spawning) similarly requires `Future + Send + 'static`: a multithreaded work-stealing runtime can move a suspended task to another worker, so state retained across `.await` must be movable. `'static` means “contains no borrowed data that would become invalid,” not “leaks forever.” The idiomatic pattern is to move owned state into the task, keep the `JoinHandle`, and inspect its `Result` so panics and task cancellation are visible:

```rust
let handle = tokio::spawn(async move { process(request).await });
let response = handle.await??;
```

Do not plaster `Send + Sync + 'static` onto every library function just because one caller uses Tokio. A function that returns a future can often borrow normally; the bound belongs where a task is detached or allowed to migrate. `Sync` means a type can be safely shared through references between threads; it is a different property from `Send`, and futures usually need `Send` at a multithreaded spawn boundary, not `Sync` by default.

When borrowing is the desired ownership model, use structured concurrency: `join!`, `try_join!`, a scoped task facility, or an executor that guarantees children finish before the scope ends. Standard threads have this explicitly in [`thread::scope`](https://doc.rust-lang.org/std/thread/fn.scope.html), whose children may borrow non-`'static` data and are joined before the scope returns. The linked structured-concurrency article explains the same benefit for async: child lifetime, error propagation, and cleanup stay attached to the parent. A single-threaded or thread-per-core runtime can also support `!Send` tasks, but that is an architectural choice. `Rc`, `RefCell`, or a non-`Send` GUI handle is correct only when the task is guaranteed to remain on its owning thread; switching later to Tokio’s multithreaded `spawn` should then fail loudly at the boundary.

## Cancellation is part of the API contract

`tokio::select!` returns when one branch completes and drops all losing futures. This makes it powerful for timeouts and shutdown, but every branch must be cancellation-safe in its use context. Tokio documents `mpsc::Receiver::recv`, socket `accept`, ordinary `read`/`write`, and stream `next` as cancellation-safe. It documents `read_exact`, `read_to_end`, `write_all`, and queued lock/semaphore acquisition as not cancellation-safe. Cancellation safety is local; cancellation correctness is the system property that matters. A dropped future can be locally valid yet still lose a message or leave a protocol invariant broken.

Bad pattern—timing out a send can silently lose the value:

```rust
tokio::select! {
    result = tx.send(message) => { result?; Ok(()) },
    _ = tokio::time::sleep(deadline) => Ok(()), // `message` is dropped
}
```

The snippets are schematic: `process`, `make_message`, and the surrounding `Result` type come from the application, and Tokio features/runtime setup must be enabled by the crate.

The [Tokio MPSC documentation](https://docs.rs/tokio/latest/tokio/sync/mpsc/struct.Sender.html) and the cancellation article recommend reserving capacity first, then performing the infallible send through a permit. Generate the message after capacity is secured if generation is expensive or has side effects:

```rust
let permit = tokio::time::timeout(deadline, tx.reserve()).await??;
permit.send(make_message());
```

Dropping `reserve()` still loses that sender’s FIFO queue position, so if ordering/fairness matters, pin one reserve future and repeatedly poll `&mut` that same future across `select!` iterations. Likewise, do not take a value out of a mutex-protected state, await, and assume the value will always be put back: cancellation can leave the shared state in an invalid `None` or half-updated state. Prefer an actor task that owns the resource, or make each awaited operation preserve a valid invariant. If partial I/O is expected, use an API that records progress (for example a cursor with `write_all_buf`) rather than canceling a `write_all` whose partial result is unknown.

Use a task when work must finish independently of a caller’s future, such as committing an audit record after a client disconnects. A Tokio task is runtime-driven; dropping its `JoinHandle` detaches it, while `abort` requests cancellation. That independence is useful, but it transfers shutdown responsibility to the owner: retain handles or use a task group, signal shutdown cooperatively, and await completion. A future directly awaited by a request handler will be canceled when the handler is dropped, which is correct only when abandoning the operation is semantically acceptable. `select!`’s default fairness is randomized; `biased;` saves a small cost but makes poll order—and starvation prevention—the caller’s responsibility.

Threads do not have a safe general equivalent of dropping a future. Killing a thread can strand a lock or interrupt an arbitrary invariant. Use a channel, an atomic flag, or a cancellation token checked at defined points, then join the thread. Dropping a standard `JoinHandle` detaches the thread, so do that only when process-lifetime background work is intentional.

## Backpressure and blocking boundaries

Bound concurrency and queues deliberately. [`tokio::sync::mpsc::channel`](https://docs.rs/tokio/latest/tokio/sync/mpsc/fn.channel.html) suspends a sender when its capacity is full; this is backpressure. Tokio’s [channel tutorial](https://tokio.rs/tokio/tutorial/channels) warns that unbounded queues eventually consume all available memory. Pick capacity from a memory/load budget, decide whether a full queue should wait, reject, shed, or coalesce work, and make shutdown observable through channel closure. A common good pattern is one task owning a socket, connection, or mutable state while other tasks send typed commands over a bounded channel; it avoids holding a mutex guard across I/O and gives one place to enforce ordering.

For synchronous workers, [`std::sync::mpsc::sync_channel`](https://doc.rust-lang.org/std/sync/mpsc/fn.sync_channel.html) supplies equivalent bounded behavior by blocking the sending thread. Do not call blocking `recv`, filesystem, compression, or CPU-heavy code on an async worker: it blocks unrelated tasks. Tokio’s [`spawn_blocking`](https://docs.rs/tokio/latest/tokio/task/fn.spawn_blocking.html) is for bounded blocking work that eventually finishes. Its closure is `Send + 'static`; once started it cannot be aborted, and many CPU jobs should be limited with a semaphore or delegated to a pool such as Rayon. Long-lived blocking loops belong on a dedicated `std::thread`, with explicit cooperative shutdown.

The resulting rule is simple but not simplistic: keep ownership close to the work, make every cancellation point intentional, make queues finite, and let the selected runtime determine the bounds. Async is a good fit when cooperative waiting and composition buy something tangible; a thread or scoped thread is the better idiom when blocking, CPU parallelism, or straightforward lifetime control is the real problem.

Sources accessed 2026-09-13: repository README and linked articles above; [Rust Async Book: concurrency](https://rust-lang.github.io/async-book/part-guide/concurrency.html), [Tokio spawning](https://tokio.rs/tokio/tutorial/spawning), [`select!`](https://docs.rs/tokio/latest/tokio/macro.select.html), Tokio channels, and Tokio `spawn_blocking`; Rust standard-library `thread::spawn`, `thread::scope`, and `sync_channel` documentation.
