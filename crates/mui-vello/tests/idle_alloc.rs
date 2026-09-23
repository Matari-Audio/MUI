//! An idle frame on the tiled path plans and commits its damage without
//! touching the allocator. Its own binary: the counting allocator is global.
#![cfg(feature = "gpu-effects")]

use mui_scene::prelude::*;
use mui_vello::effects::damage::{tiles, DamagePlan, DamageTracker};
use mui_vello::kurbo::Affine;
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

struct Counting;

thread_local! {
    // `const` and destructor-free, so reading it never allocates.
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.with(|n| n.set(n.get() + 1));
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        ALLOCATIONS.with(|n| n.set(n.get() + 1));
        unsafe { System.realloc(ptr, layout, size) }
    }
}

#[global_allocator]
static COUNTING: Counting = Counting;

fn allocations(f: impl FnOnce()) -> usize {
    let before = ALLOCATIONS.with(Cell::get);
    f();
    ALLOCATIONS.with(Cell::get) - before
}

/// The tracker used to clone the whole paint list on every commit and build
/// fresh flag and index vectors on every plan, idle frames included.
#[test]
fn an_idle_frame_plans_and_commits_without_allocating() {
    let root = column((0..20).map(|i| {
        leaf(40., 20.)
            .fill(Role::Primary)
            .stroke(Role::Ink)
            .id(format!("l{i}"))
    }))
    .id("root");
    let scene = resolve_scene(&SceneSpec::new(root).offered(Size::new(200., 600.))).unwrap();
    let grid = tiles([512, 512], 128);
    let mut tracker = DamageTracker::default();
    let mut plan = DamagePlan::default();
    // The first frame fills the buffers.
    tracker.plan(&scene, Affine::IDENTITY, &grid, &mut plan);
    tracker.commit(&scene, Affine::IDENTITY);
    let n = allocations(|| {
        tracker.plan(&scene, Affine::IDENTITY, &grid, &mut plan);
        tracker.commit(&scene, Affine::IDENTITY);
    });
    assert!(plan.dirty.is_empty());
    assert_eq!(n, 0, "an idle frame allocated {n} times");
}
