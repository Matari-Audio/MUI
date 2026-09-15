# Idiomatic Rust: iterators, loops, and fallible pipelines

The linked repository defines idiomatic code as the concise, convenient, common way to express a task in the language, rather than importing habits from another language. Its iterator resources point to two ideas that belong together: use Rust's iterator protocol and adapters to state the data transformation, while choosing a plain loop when control flow or side effects are the real subject. “Idiomatic” is therefore a correctness and communication decision, not a contest to produce the longest method chain. See [mre/idiomatic-rust](https://github.com/mre/idiomatic-rust), especially its links to [Iteration patterns for Result & Option](https://xion.io/post/code/rust-iter-patterns.html) and [Effectively Using Iterators in Rust](https://hermanradtke.com/2015/06/22/effectively-using-iterators-in-rust.html/).

## The protocol and its ownership choices

`Iterator` requires `next(&mut self) -> Option<Self::Item>`. `None` means the iterator has no next item; a `for` loop repeatedly calls this protocol for us. The Rust Book explains that this removes manual indexing and boundary bookkeeping, and that adapters are lazy: `map` or `filter` creates a pipeline but executes nothing until a consumer such as `collect`, `sum`, `for_each`, `find`, `any`, or `fold` advances it ([Rust Book, “Processing a Series of Items with Iterators”](https://doc.rust-lang.org/book/ch13-02-iterators.html); [Iterator trait](https://doc.rust-lang.org/std/iter/trait.Iterator.html)).

The initial iterator determines ownership. `collection.iter()` borrows and yields `&T`; `collection.iter_mut()` borrows mutably and yields `&mut T`; `collection.into_iter()` consumes the collection and yields owned `T`. This choice is correctness, not style: use `iter()` when the caller still needs the collection, `into_iter()` when the pipeline should own or move its values, and `iter_mut()` when in-place mutation is the operation. An unnecessary `clone()` often signals that the wrong iterator flavor was selected. A good example is `names.iter().map(|name| name.len()).sum()`: the closure works for `Vec<String>` and `Vec<&str>`, computes from borrowed strings, and leaves `names` available. A bad example is `names.clone().into_iter()` merely to avoid deciding whether the function should borrow or consume. When the receiver is a reference, `(&names).into_iter()` also iterates by reference; spelling the receiver makes that choice clear. Array `IntoIterator` behavior changed with the 2021 edition, so qualify the receiver in edition-sensitive code.

## Pipelines versus loops

For a straight transformation, a small pipeline says what happens in data order:

```rust
let odds_squared: Vec<_> = (1..100)
    .filter(|n| n % 2 != 0)
    .map(|n| n * n)
    .collect();
```

This is a good pattern because each adapter has one readable job, the final `collect` makes the required work explicit, and the compiler can keep the traversal as one lazy pipeline. By contrast, an index loop such as `for i in 0..items.len() { if items[i].valid() { out.push(items[i].value()); } }` repeats indexing and exposes boundary details that the collection already knows. Iterating over `items.iter()` or `items.into_iter()` makes the ownership and element operation visible and works for non-indexable iterators too.

The method chain becomes a bad pattern when it hides decisions. A chain of nested closures that mutates several accumulators, handles multiple error cases, and conditionally breaks is harder to review than a `for` loop with named variables. Use a loop when the operation is primarily imperative: several `continue`/`break` paths, a mutable state machine, ordered side effects, or error messages that need the current item and index. A loop is still idiomatic Rust; every `for` loop is built on `IntoIterator`. Keep adapters for transformations and predicates, and keep control flow where it can be read directly.

Do not put work in a bare adapter and expect it to run:

```rust
items.iter().map(|item| send(item)); // bad: the Map is never consumed
items.iter().for_each(|item| send(item)); // explicit side-effect consumer
```

If the side effect has meaningful failure, `try_for_each` or a loop returning `Result` communicates short-circuiting better than a discarded `map` value. `inspect` is useful for temporary observation, but a production pipeline whose meaning depends on hidden logging inside `map` is difficult to reason about.

## Option: filter, flatten, and search without losing intent

`Option` has a one-or-zero-item iterator. For a sequence of optional values where absent values should be skipped, `filter_map` combines selection and transformation:

```rust
let ids: Vec<_> = records.iter().filter_map(|record| record.id).collect();
```

That is clearer than `map(|r| r.id).filter(|id| id.is_some()).map(|id| id.unwrap())`, and it avoids a panic. Use `find_map` when the first successful conversion is the result; it short-circuits instead of collecting everything. `flatten` is appropriate when the nested structure itself is the point. The standard library documents `Option`'s `IntoIterator` implementation and analogous `Result` behavior ([Option](https://doc.rust-lang.org/std/option/enum.Option.html); [Result](https://doc.rust-lang.org/stable/std/result/enum.Result.html)).

The crucial exception is that skipping `None` is only correct if absence is expected and uninteresting. `records.iter().filter_map(|r| r.parse_id().ok()).collect()` is a bad validation pattern: it turns malformed IDs into an apparently successful, incomplete list. Keep the failure visible by returning `Result` instead.

## Result: fail fast or report everything deliberately

For fallible transformations, map each item to `Result<T, E>` and let the target collection encode the policy:

```rust
let parsed: Result<Vec<u32>, _> = input.iter()
    .map(|text| text.parse::<u32>())
    .collect();
```

`Result` implements `FromIterator`: collection stops at the first `Err` and returns it; if all items succeed, it returns a collection of the `Ok` values. The official documentation explicitly states that no further elements are taken after the first error ([`Result`'s `FromIterator` implementation](https://doc.rust-lang.org/core/result/enum.Result.html#impl-FromIterator%3CResult%3CA%2C%20E%3E%3E-for-Result%3CV%2C%20E%3E)). This is concise and correct for all-or-nothing parsing, validation, or loading where the first error is sufficient. It also preserves laziness: later inputs are not parsed after failure. “All-or-nothing” describes the returned value, not a transaction: if the mapping closure writes a file or mutates shared state before a later item fails, those earlier side effects remain. Validate first and commit second, or implement explicit rollback, when atomic external effects are required.

The corresponding `Option<Vec<T>>` collection is `None` if any input is `None`; use it when one missing prerequisite invalidates the whole result. Do not use either collection trick when the requirement is “process every item and report every error.” In that case, write the policy explicitly with a loop that pushes successes and errors into separate collections, or use `partition` followed by deliberate conversion. `filter_map(Result::ok)` is especially dangerous because it silently discards errors and can make partial data look complete.

Use `map` for changing a successful value, `map_err` for adding or converting error context, and `and_then` when the next operation can fail. `?` inside a small helper or loop is often easier to read than deeply nested `and_then` calls. The right short-circuit consumer depends on the question: `any`/`all` for predicates, `find`/`find_map` for the first match, `try_for_each` for fallible side effects, and `try_fold` for fallible accumulation. State the return type of `collect` when inference would obscure the policy; `Result<Vec<_>, E>` tells a reviewer immediately that one failure aborts the collection.

## Review checklist

Ask four questions: what is borrowed or moved; where is the consuming operation; is skipping or short-circuiting intentional; and is the chain easier to understand than a loop? Prefer one-pass lazy adapters for pure, linear transformations. Split a long chain into named stages or switch to a loop when closure bodies contain substantial control flow. Preserve errors unless the API explicitly promises best-effort behavior, and use `unwrap` only for a documented invariant that cannot be represented more accurately in the type. These rules make iterator code concise while keeping its ownership, evaluation timing, and failure policy visible.

## Sources

- [mre/idiomatic-rust README](https://github.com/mre/idiomatic-rust)
- [Rust Book: Processing a Series of Items with Iterators](https://doc.rust-lang.org/book/ch13-02-iterators.html)
- [`std::iter::Iterator`](https://doc.rust-lang.org/std/iter/trait.Iterator.html)
- [`std::option::Option`](https://doc.rust-lang.org/std/option/enum.Option.html)
- [`std::result::Result`](https://doc.rust-lang.org/stable/std/result/enum.Result.html)
- [Xion: Iteration patterns for Result & Option](https://xion.io/post/code/rust-iter-patterns.html)
- [Herman Radtke: Effectively Using Iterators in Rust](https://hermanradtke.com/2015/06/22/effectively-using-iterators-in-rust.html/)
