# Collections, arenas, IDs, and parallel performance in idiomatic Rust

The source repository describes idiomatic Rust as the concise, conventional way to express a task in Rust, rather than translating habits from another language ([`idiomatic-rust` README](https://github.com/mre/idiomatic-rust#readme)). For collection-heavy code, that convention is primarily about making ownership and data-shape explicit. Performance follows from that choice, but “fast” is only correct when it preserves the collection’s invariants and the program’s intended semantics.

## Start with the standard collection that matches the access pattern

`Vec<T>` is a contiguous growable array, so it is usually the best default for sequential data, stacks, arena storage, and hot read-mostly traversal ([`Vec` documentation](https://doc.rust-lang.org/std/vec/struct.Vec.html)). Contiguity improves locality, indexing is constant-time, and `push` is amortized efficient. If the number of elements is known or has a credible upper bound, `Vec::with_capacity` or `reserve` avoids avoidable reallocations and copies. Do not reserve a huge guess: it trades a few possible reallocations for permanent memory pressure, and `Vec` does not automatically shrink after removals.

Use `HashMap` for key lookup with no ordering requirement, `BTreeMap` when sorted iteration, range queries, or minimum/maximum keys are part of the contract, and `VecDeque` for efficient operations at both ends. The standard collection guide gives the useful complexity comparison: vector indexing is O(1), hash-map lookup is expected O(1), and B-tree lookup is O(log n); a `Vec` generally wins ties because it avoids pointer-heavy structure and has better locality ([standard collections guide](https://doc.rust-lang.org/std/collections/)). An ordered result is a correctness requirement, not a cosmetic performance choice.

When a map operation depends on whether a key is present, use its `entry` API so the key is searched once:

```rust
use std::collections::HashMap;

fn add_edge(graph: &mut HashMap<usize, Vec<usize>>, from: usize, to: usize) {
    graph.entry(from).or_default().push(to);
}
```

The common bad pattern is `contains_key` followed by `insert`, then a second `get_mut`; it performs duplicate lookup work and makes the “present versus absent” logic easier to get wrong. Cloning a key or value merely to satisfy that pattern also creates allocation and semantic cost. Move values when ownership is transferring, borrow with `&T`/`&[T]` when it is not, and use `iter`, `iter_mut`, `into_iter`, `extend`, or `collect` to state whether a collection is borrowed, mutated, or consumed. Iterators are lazy and avoid a temporary allocation until the code explicitly collects ([collection iterator and entry guidance](https://doc.rust-lang.org/std/collections/)).

## Arena trees and graphs: IDs make mutation tractable

The repository’s linked article presents the key arena pattern: store nodes in one `Vec`, and store relationships as numeric IDs instead of references ([Idiomatic tree and graph structures](https://rust-leipzig.github.io/architecture/2016/12/20/idiomatic-trees-in-rust/)). A minimal shape is:

```rust
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct NodeId(usize); // keep the field private

struct Node<T> {
    value: T,
    children: Vec<NodeId>,
}

struct Arena<T> {
    nodes: Vec<Node<T>>,
}

impl<T> Arena<T> {
    fn get(&self, id: NodeId) -> Option<&Node<T>> {
        self.nodes.get(id.0)
    }
}
```

This is a good pattern when nodes are created and traversed dynamically, when a tree or graph needs mutation, or when lifetimes would otherwise make a self-referential structure awkward. It avoids `Rc<RefCell<...>>`, runtime borrow checks, and one allocation per node; child `Vec`s can still allocate independently, so the layout is not literally one allocation for the whole graph. The arena’s contiguous node storage also makes traversal and parallel read access straightforward. The `indextree` crate packages this design with checked mutation, parallel iteration, and a generation counter for removed slots ([`indextree` documentation](https://docs.rs/indextree/latest/indextree/)).

The bad pattern is a node containing `Rc<RefCell<Box<Node>>>` for every parent and child. It can be valid for genuinely shared, independently owned objects, but it introduces reference-count updates, runtime borrow failures, and cycle/leak hazards; replacing `Rc` with `Arc<Mutex<_>>` adds atomic operations and lock contention. Use that design when shared ownership is the domain model and nodes must outlive the arena. Do not use it just to silence a borrow-checker error.

An ID is a handle, not a proof of validity. A raw `usize` can be out of bounds, can accidentally be used with the wrong arena, and becomes stale if a slot is removed and reused. Even a generational ID detects reuse within its intended arena; it does not by itself prove that the ID came from this particular arena. Keep its constructor private, keep arena provenance in the API or add an arena identity when needed, expose checked access returning `Option` or `Result`, update both sides of parent/child or edge relationships in one method, and decide whether IDs remain stable across deletion. For arbitrary removal and reuse, use a generational ID (index plus generation), a tombstone policy, or a crate such as `indextree`/`slotmap`. Panicking indexing is reasonable only for an internal invariant already established by construction; public or input-derived IDs should be checked.

## Cloning and allocation are ownership decisions

`Vec::clone` allocates a new vector and invokes `Clone` for every element ([`Vec::clone` docs](https://doc.rust-lang.org/std/vec/struct.Vec.html#impl-Clone-for-Vec%3CT%2C%20A%3E)). That is a new buffer, but the element’s own semantics still apply: cloning `Arc<T>` increments a reference count and shares `T`, while cloning `String` copies its bytes. The same principle applies to maps and arena payloads. Cloning a small `NodeId` is cheap, but cloning an arena duplicates every node and invokes each payload’s `Clone`. A good API returns `&Node`, `&mut Node`, slices, or iterators when the caller only needs access; it consumes a collection with `into_iter` when ownership should move; and it uses `Cow` or an explicit copy-on-write boundary when most callers can borrow. `clone_from` can reuse an existing destination allocation when replacement is unavoidable. A bad pattern is repeated `collect::<Vec<_>>()` in a traversal merely to iterate once, or cloning a whole graph for a read-only helper.

## Parallel traversal needs independent work

Rayon’s current API turns many sequential iterator chains into `par_iter` chains and guarantees data-race-free execution ([Rayon crate docs](https://docs.rs/rayon/latest/rayon/)). Its scheduler can use work stealing and decide at runtime whether tasks run concurrently, but it does not promise that every workload benefits or that a fixed threshold is chosen optimally. The linked Rayon article demonstrates the practical rule: split disjoint `&mut` slices or immutable arena reads, and use a sequential base case for small chunks because task scheduling overhead can exceed useful work ([Rayon article](https://smallcultfollowing.com/babysteps/blog/2015/12/18/rayon-data-parallelism-in-rust/)). For an arena, `arena.nodes.par_iter()` is a good fit for independent read-only analysis; parallel mutation should produce per-item or per-thread results and merge them, or use carefully disjoint mutable partitions. A shared `Mutex<HashMap<...>>` inside `par_iter` is often a bad pattern: it serializes the hot path and hides ordering/race semantics. Prefer `map`, `fold`, `reduce`, or `par_extend`, and preserve deterministic ordering when the output contract requires it.

## Profile the real workload

Complexity tables are a filter, not a benchmark. Compare representative graph sizes, deletion rates, key distributions, payload sizes, and thread counts. `cargo bench` runs optimized benchmarks; stable projects can use Criterion, which records statistical distributions and detects regressions ([Cargo benchmark command](https://doc.rust-lang.org/cargo/commands/cargo-bench.html), [Criterion documentation](https://bheisler.github.io/criterion.rs/book/)). Measure allocation count and peak memory when allocation is the suspected cost, then verify that a change preserves ID validity, ordering, duplicate-edge policy, and traversal results. Choose `Vec`, maps, arenas, and Rayon because their semantics fit first; retain a measured optimization only when its workload evidence beats the simpler design.
