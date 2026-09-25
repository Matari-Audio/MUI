//! What `Ui::frame` costs on a rebuilt editor tree, per kind of frame: nothing
//! changed, the pointer crossing widgets, a resize, and a tooltip coming and
//! going. Median of 60, allocations per frame.
//!
//! ```text
//! cargo run -p mui --profile perf --example frame_bench
//! ```
use std::alloc::{GlobalAlloc, Layout as AllocLayout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use mui::prelude::*;

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

const TRACKS: usize = 16;

fn editor(ui: &mut Ui, values: &mut [f64]) -> El {
    let mut v = values.iter_mut();
    let tracks: Vec<El> = (0..TRACKS)
        .map(|t| {
            let knobs: Vec<El> = (0..4)
                .map(|k| {
                    knob(ui, format!("t{t}/k{k}"), "Gain", v.next().unwrap(), 0.0..=1.0)
                        .0
                        .el()
                })
                .collect();
            let fader = slider(ui, format!("t{t}/f"), "Level", v.next().unwrap(), 0.0..=1.0)
                .0
                .el();
            row([
                column([
                    text(format!("Track {t}"))
                        .id(format!("t{t}/name"))
                        .tip("Double-click to rename"),
                    text("A compact note that wraps when the track column gets narrow.")
                        .shrink(1.0),
                ])
                .gap(4.0)
                .grow(1.0),
                row(knobs).gap(8.0),
                fader.width(220.0),
            ])
            .gap(12.0)
            .pad(8.0)
            .fill(Role::Raised)
        })
        .collect();
    column(tracks).gap(6.0).pad(10.0).fill(Role::Background)
}

fn main() {
    let font = Font::new(epaint_default_fonts::HACK_REGULAR).unwrap();
    let mut ui = Ui::new(Theme::DEFAULT).font(font);
    let mut values = vec![0.5; TRACKS * 5];
    let size = |w: f64| Some(Size::new(w, 900.0));
    let away = PointerInput {
        pos: Some(Point::new(2.0, 2.0)),
        ..Default::default()
    };
    let mut frame = |ui: &mut Ui, w: f64, p: PointerInput, dt: f64| {
        let root = editor(ui, &mut values);
        std::hint::black_box(ui.frame(root, size(w), p, dt).unwrap().scene.paint.len());
    };
    frame(&mut ui, 1200.0, away, 0.016);
    let at = |ui: &Ui, id: &str| {
        let f = ui.scene().unwrap().layout.frame(id).unwrap();
        let (x, y) = f.center();
        PointerInput {
            pos: Some(Point::new(x, y)),
            ..Default::default()
        }
    };
    let over: Vec<PointerInput> = (0..TRACKS)
        .map(|t| at(&ui, &format!("t{t}/k1")))
        .collect();
    let name = at(&ui, "t3/name");
    let mut run = |name: &str, f: &mut dyn FnMut(&mut Ui, usize)| {
        for i in 0..10 {
            f(&mut ui, i);
        }
        let a = ALLOCS.load(Ordering::Relaxed);
        f(&mut ui, 10);
        let allocs = ALLOCS.load(Ordering::Relaxed) - a;
        let mut t: Vec<f64> = (11..71)
            .map(|i| {
                let s = Instant::now();
                f(&mut ui, i);
                s.elapsed().as_secs_f64() * 1e3
            })
            .collect();
        t.sort_by(f64::total_cmp);
        println!("{name:>8}: {:.3} ms, {allocs} allocations", t[t.len() / 2]);
    };
    run("steady", &mut |ui, _| frame(ui, 1200.0, away, 0.016));
    run("hover", &mut |ui, i| frame(ui, 1200.0, over[i % TRACKS], 0.016));
    run("resize", &mut |ui, i| {
        frame(ui, 900.0 + (i % 50) as f64 * 6.0, away, 0.016)
    });
    // The pointer rests on a name until its tip shows, then leaves: a tip
    // comes or goes on two frames in three.
    let mut tips = 0;
    run("tooltip", &mut |ui, i| {
        let p = if i % 3 == 0 { away } else { name };
        frame(ui, 1200.0, p, 0.6);
        tips += ui.scene().unwrap().layout.frame("/tip").is_some() as usize;
    });
    assert!(tips > 20, "the tip showed {tips} times");
    println!("{:?}", ui.layout_stats());
}
