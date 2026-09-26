//! A ~1000-node stress tree: nested 8 deep, grids, wrapping rows, paragraphs.
//! Times `resolve_scene_with` against the layout solve alone, at three shapes.
use std::alloc::{GlobalAlloc, Layout as AllocLayout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

static ALLOCS: AtomicUsize = AtomicUsize::new(0);
static COUNTER: AtomicUsize = AtomicUsize::new(0);
static BYTES: AtomicUsize = AtomicUsize::new(0);
struct Counting;
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: AllocLayout) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(l.size(), Ordering::Relaxed);
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: AllocLayout) {
        unsafe { System.dealloc(p, l) }
    }
}
#[global_allocator]
static A: Counting = Counting;

use mui_scene::prelude::*;
use mui_scene::{Limits, TextCache, resolve_scene_with};

const PARA: &str = "A compact CSS-like DSL where everything aligns automatically and nothing is placed absolutely.";

fn count(n: &El) -> usize {
    1 + n.children().iter().map(count).sum::<usize>()
}

fn cell(i: usize) -> El {
    column([
        text(format!("p{i}")).text_size(11.0),
        leaf(12.0, 12.0).fill(Role::Field).radius(3.0),
    ])
    .gap(4.0)
    .pad(4.0)
    .fill(Role::Raised)
    .id(format!("c{}", COUNTER.fetch_add(1, Ordering::Relaxed)))
}

fn block(depth: usize, i: usize) -> El {
    if depth == 0 {
        return row([cell(i), cell(i + 1), text(PARA).lines(3).shrink(1.0)])
            .gap(6.0)
            .wrap();
    }
    column([
        row((0..3).map(|k| cell(i * 7 + k))).gap(6.0).wrap(),
        grid(3, (0..6).map(|k| cell(i * 11 + k))).gap(4.0),
        block(depth - 1, i + 1),
    ])
    .gap(6.0)
    .pad(4.0)
    .fill(Role::Surface)
}

fn tree() -> El {
    column((0..4).map(|i| block(7, i)))
        .gap(8.0)
        .pad(8.0)
        .fill(Role::Background)
        .scroll()
}

/// A lower bound on "has the layout input changed": hash the publicly
/// reachable layout fields of every node. The real key would also cover
/// width/height/basis/grow/shrink/align/min/max/aspect/order/span.
fn hash_tree(n: &El, h: &mut std::collections::hash_map::DefaultHasher) {
    use std::hash::{Hash, Hasher};
    n.key().hash(h);
    n.children().len().hash(h);
    let p = n.padding(mui_scene::SpacingScale::DEFAULT);
    for v in [
        p.left,
        p.top,
        p.right,
        p.bottom,
        n.scroll_offset()[0],
        n.scroll_offset()[1],
    ] {
        v.to_bits().hash(h);
    }
    (n.is_scroll(), n.is_clip(), n.is_float(), n.vertical()).hash(h);
    let _ = h.finish();
    for c in n.children() {
        hash_tree(c, h);
    }
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

fn main() {
    println!("nodes: {}", count(&tree()));
    println!(
        "size_of Painted {} ResolvedSurface {}",
        std::mem::size_of::<mui_scene::Painted>(),
        std::mem::size_of::<mui_scene::ResolvedSurface>()
    );
    let font = Font::new(epaint_default_fonts::HACK_REGULAR).unwrap();
    for (w, h) in [(1280.0, 800.0), (240.0, 2400.0), (2000.0, 300.0)] {
        let mut spec = SceneSpec::new(tree())
            .offered(Size::new(w, h))
            .font(font.clone());
        spec.limits = Limits {
            nodes: 100_000,
            depth: 256,
            ..Limits::default()
        };
        let mut cache = TextCache::default();
        let t = Instant::now();
        let first = resolve_scene_with(&spec, &mut cache);
        let cold = t.elapsed().as_secs_f64() * 1e3;
        match &first {
            Ok(s) => println!("  paint {} keys {}", s.paint.len(), s.surfaces().count()),
            Err(e) => println!("  error: {e}"),
        }
        for _ in 0..5 {
            let _ = resolve_scene_with(&spec, &mut cache);
        }
        let (a0, b0) = (
            ALLOCS.load(Ordering::Relaxed),
            BYTES.load(Ordering::Relaxed),
        );
        let _ = resolve_scene_with(&spec, &mut cache);
        println!(
            "  one warm resolve: {} allocations, {} KiB",
            ALLOCS.load(Ordering::Relaxed) - a0,
            (BYTES.load(Ordering::Relaxed) - b0) / 1024
        );
        let (a1, b1) = (
            ALLOCS.load(Ordering::Relaxed),
            BYTES.load(Ordering::Relaxed),
        );
        let _ = mui_layout::resolve_with(
            &spec.root,
            spec.offered,
            spec.limits,
            spec.theme.spacing,
            |_e, _room| Size::new(40.0, 14.0),
        );
        println!(
            "  one bare solve:   {} allocations, {} KiB",
            ALLOCS.load(Ordering::Relaxed) - a1,
            (BYTES.load(Ordering::Relaxed) - b1) / 1024
        );
        let hashes: Vec<f64> = (0..30)
            .map(|_| {
                let t = Instant::now();
                let mut h = std::collections::hash_map::DefaultHasher::new();
                hash_tree(&spec.root, &mut h);
                let ms = t.elapsed().as_secs_f64() * 1e3;
                std::hint::black_box(std::hash::Hasher::finish(&h));
                ms
            })
            .collect();
        println!("  partial tree hash: {:.3} ms", median(hashes));
        let full: Vec<f64> = (0..30)
            .map(|_| {
                let t = Instant::now();
                let r = resolve_scene_with(&spec, &mut cache);
                let ms = t.elapsed().as_secs_f64() * 1e3;
                std::hint::black_box(r.is_ok());
                ms
            })
            .collect();
        // Layout solve alone, with a constant measurer (no shaping).
        let solve: Vec<f64> = (0..30)
            .map(|_| {
                let t = Instant::now();
                let l = mui_layout::resolve_with(
                    &spec.root,
                    spec.offered,
                    spec.limits,
                    spec.theme.spacing,
                    |_e, _room| Size::new(40.0, 14.0),
                );
                let ms = t.elapsed().as_secs_f64() * 1e3;
                std::hint::black_box(l.map(|l| l.all().len()).unwrap_or(0));
                ms
            })
            .collect();
        println!(
            "{w}x{h}: cold {cold:.3} ms | warm resolve {:.3} ms | bare solve {:.3} ms",
            median(full),
            median(solve)
        );
    }
}
