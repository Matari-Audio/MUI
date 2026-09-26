//! What a panel of fifty widgets costs the allocator per frame. Its own
//! binary: the counting allocator is global.
use mui::prelude::*;
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

struct Counting;

thread_local! {
    // `const` and destructor-free, so reading it never allocates.
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

// SAFETY: every method forwards to `System` with the caller's arguments
// unchanged; the counter never allocates, so the GlobalAlloc contract is
// System's.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.with(|n| n.set(n.get() + 1));
        // SAFETY: the caller upholds `GlobalAlloc::alloc`'s contract; forwarded as is.
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: the caller upholds `GlobalAlloc::dealloc`'s contract; forwarded as is.
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        ALLOCATIONS.with(|n| n.set(n.get() + 1));
        // SAFETY: the caller upholds `GlobalAlloc::realloc`'s contract; forwarded as is.
        unsafe { System.realloc(ptr, layout, size) }
    }
}

#[global_allocator]
static COUNTING: Counting = Counting;

fn allocations<T>(f: impl FnOnce() -> T) -> (usize, T) {
    let before = ALLOCATIONS.with(Cell::get);
    let t = f();
    (ALLOCATIONS.with(Cell::get) - before, t)
}

/// Twenty buttons, ten toggles, ten sliders and ten knobs, ids composed
/// without the heap.
fn panel(ui: &mut Ui, v: &mut [f64; 20], on: &mut [bool; 10]) -> El {
    let root = Id::of("w");
    let mut rows = Vec::new();
    for i in 0..10 {
        let a = root.slot(i);
        let (b0, _) = button(ui, &*a.field("a"), "Go");
        let (b1, _) = button(ui, &*a.field("b"), "Stop");
        let (t, _) = toggle(ui, &*a.field("t"), &mut on[i]);
        let (s, _) = slider(ui, &*a.field("s"), "Gain", &mut v[i], 0.0..=1.0);
        let (k, _) = knob(ui, &*a.field("k"), "Cut", &mut v[10 + i], 0.0..=1.0);
        rows.push(row![b0, b1, t, s, k]);
    }
    col(rows)
}

/// A widget built with the same id every frame allocates nothing for the id:
/// the one allocation left is the boxed build closure.
#[test]
fn a_widget_id_costs_the_allocator_nothing() {
    let mut ui = Ui::new(Theme::DEFAULT);
    let id = Id::of("rack").slot(3).field("go");
    let (n, _) = allocations(|| button(&mut ui, &*id, ""));
    assert_eq!(n, 1, "only the build closure is boxed");
}

/// `cargo test -p mui --test frame_alloc -- --nocapture --ignored`: the
/// allocations per frame of a fifty-widget panel, building and framing.
#[test]
#[ignore = "a measurement, not a check"]
fn fifty_widgets_per_frame() {
    const N: usize = 20;
    let mut ui =
        Ui::new(Theme::DEFAULT).font(Font::new(epaint_default_fonts::HACK_REGULAR).unwrap());
    let (mut v, mut on) = ([0.5; 20], [false; 10]);
    let size = Some(Size::new(1200., 800.));
    for _ in 0..3 {
        let tree = panel(&mut ui, &mut v, &mut on);
        ui.frame(tree, size, PointerInput::default(), 0.016)
            .unwrap();
    }
    let (mut build, mut frame) = (0, 0);
    for _ in 0..N {
        let (b, tree) = allocations(|| panel(&mut ui, &mut v, &mut on));
        let (f, _) = allocations(|| {
            ui.frame(tree, size, PointerInput::default(), 0.016)
                .map(|_| ())
                .unwrap();
        });
        build += b;
        frame += f;
    }
    println!(
        "per frame: build {} + frame {} = {}",
        build / N,
        frame / N,
        (build + frame) / N
    );
}
