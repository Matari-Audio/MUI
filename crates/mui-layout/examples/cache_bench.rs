//! What the layout cache costs against a bare solve: an unchanged tree, one
//! changed label, and a resize every frame. Median of 50, allocations per call.
use std::alloc::{GlobalAlloc, Layout as AllocLayout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use mui_layout::{
    LayoutCache, Limits, Node, Size, SpacingScale, resolve_cached_with, resolve_with,
};

static ALLOCS: AtomicUsize = AtomicUsize::new(0);
struct Counting;
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: AllocLayout) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: AllocLayout) {
        unsafe { System.dealloc(p, l) }
    }
}
#[global_allocator]
static A: Counting = Counting;

type N = Node<String>;
const PARA: &str = "A compact CSS-like DSL where everything aligns automatically.";

fn text(s: impl Into<String>) -> N {
    Node::content().with(s.into())
}
fn cell(i: usize) -> N {
    N::column([text(format!("p{i}")), N::leaf(12., 12.)])
        .gap(4.)
        .pad(4.)
        .id(format!("c{i}"))
}
fn block(depth: usize, i: usize) -> N {
    if depth == 0 {
        return N::row([cell(i * 100), cell(i * 100 + 1), text(PARA).shrink(1.)])
            .gap(6.)
            .wrap();
    }
    N::column([
        N::row((0..3).map(|k| cell(i * 1000 + depth * 10 + k)))
            .gap(6.)
            .wrap(),
        N::grid(3, (0..6).map(|k| cell(i * 1000 + depth * 10 + 3 + k))).gap(4.),
        block(depth - 1, i),
    ])
    .gap(6.)
    .pad(4.)
}
fn tree() -> N {
    N::column((0..4).map(|i| block(7, i + 1)))
        .gap(8.)
        .pad(8.)
        .scroll()
}
// The measurer callback hands over `&P`, and `P` here is `String`.
#[allow(clippy::ptr_arg)]
fn metric(s: &String, room: Option<f64>) -> Size {
    let full = s.len() as f64 * 6.;
    let w = room.unwrap_or(full).max(6.);
    Size::new(full.min(w), (full / w).ceil().max(1.) * 14.)
}
fn key(s: &String, out: &mut Vec<u8>) {
    out.extend_from_slice(s.as_bytes());
}
const LIMITS: Limits = Limits {
    nodes: 100_000,
    depth: 256,
    extent: 1e6,
};

/// `ONLY=<name>` runs one case long enough to profile.
fn run(name: &str, mut f: impl FnMut(usize)) {
    match std::env::var("ONLY") {
        Ok(only) if only == name => (0..5000).for_each(&mut f),
        Ok(_) => return,
        Err(_) => {}
    }
    for i in 0..5 {
        f(i);
    }
    let a = ALLOCS.load(Ordering::Relaxed);
    f(5);
    let allocs = ALLOCS.load(Ordering::Relaxed) - a;
    let mut t: Vec<f64> = (6..56)
        .map(|i| {
            let s = Instant::now();
            f(i);
            s.elapsed().as_secs_f64() * 1e3
        })
        .collect();
    t.sort_by(f64::total_cmp);
    println!("{name:>10}: {:.3} ms, {allocs} allocations", t[t.len() / 2]);
}

fn main() {
    let mut n = tree();
    let at = |w: f64| Some(Size::new(w, 800.));
    run("bare", |_| {
        std::hint::black_box(
            resolve_with(&n, at(1280.), LIMITS, SpacingScale::DEFAULT, metric).unwrap(),
        );
    });
    let mut c = LayoutCache::default();
    run("warm", |_| {
        std::hint::black_box(
            resolve_cached_with(
                &n,
                at(1280.),
                LIMITS,
                SpacingScale::DEFAULT,
                &mut c,
                key,
                metric,
            )
            .unwrap(),
        );
    });
    println!("{:?}", c.stats());
    run("resize", |i| {
        let w = 900. + (i % 40) as f64 * 7.;
        let l = resolve_cached_with(
            &n,
            at(w),
            LIMITS,
            SpacingScale::DEFAULT,
            &mut c,
            key,
            metric,
        )
        .unwrap();
        debug_assert_eq!(
            l,
            resolve_with(&n, at(w), LIMITS, SpacingScale::DEFAULT, metric).unwrap()
        );
        std::hint::black_box(l);
    });
    run("bare resize", |i| {
        let w = 900. + (i % 40) as f64 * 7.;
        std::hint::black_box(
            resolve_with(&n, at(w), LIMITS, SpacingScale::DEFAULT, metric).unwrap(),
        );
    });
    run("one label", |i| {
        // Deep in the last block: the path from it to the root changes.
        let mut p = &mut n;
        while !p.children().is_empty() {
            let last = p.children().len() - 1;
            p = &mut p.children_mut()[last];
        }
        *p.payload_mut() = if i % 2 == 0 {
            "short".into()
        } else {
            PARA.into()
        };
        let l = resolve_cached_with(
            &n,
            at(1280.),
            LIMITS,
            SpacingScale::DEFAULT,
            &mut c,
            key,
            metric,
        )
        .unwrap();
        debug_assert_eq!(
            l,
            resolve_with(&n, at(1280.), LIMITS, SpacingScale::DEFAULT, metric).unwrap()
        );
        std::hint::black_box(l);
    });
    println!("{:?}", c.stats());
}
