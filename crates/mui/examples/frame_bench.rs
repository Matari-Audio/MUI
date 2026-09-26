//! What `Ui::frame` costs on a rebuilt editor-sized tree (~2000 nodes: a
//! toolbar, track headers with knobs, faders and hover-styled buttons, a
//! waveform canvas and a lane of clips per track), per kind of frame: nothing
//! changed, the pointer crossing widgets, a resize, and a tooltip coming and
//! going. Only `Ui::frame` is timed, in thread CPU time, not the tree build.
//! Median and min of 60,
//! allocations per frame.
//!
//! ```text
//! cargo run -p mui --profile perf --example frame_bench
//! ```
use std::alloc::{GlobalAlloc, Layout as AllocLayout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use mui::geometry::Path;
use mui::prelude::*;

static ALLOCS: AtomicUsize = AtomicUsize::new(0);
struct Counting;
// SAFETY: every method forwards to `System` with the caller's arguments
// unchanged; the counter never allocates, so the GlobalAlloc contract is
// System's.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: AllocLayout) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: the caller upholds `GlobalAlloc::alloc`'s contract; forwarded as is.
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: AllocLayout) {
        // SAFETY: the caller upholds `GlobalAlloc::dealloc`'s contract; forwarded as is.
        unsafe { System.dealloc(p, l) }
    }
}
#[global_allocator]
static A: Counting = Counting;

/// This thread's CPU time in ms: the machine is shared, the wall clock is not
/// ours.
fn cpu_ms() -> f64 {
    let mut t = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `t` is a live, writable timespec for the call's duration.
    unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut t) };
    t.tv_sec as f64 * 1e3 + t.tv_nsec as f64 * 1e-6
}

const TRACKS: usize = 16;
const CLIPS: usize = 40;

/// A status-bar style button: a label on a rounded fill that lights and
/// borders on hover.
fn button(id: String, label: &str) -> El {
    let (hover, press) = (
        Color::oklcha(0.4, 0.02, 250.0, 1.0),
        Color::oklcha(0.3, 0.02, 250.0, 1.0),
    );
    row([text(label).text_size(12.0).fill(Role::Ink)])
        .pad((8.0, 3.0))
        .h(22.0)
        .align(Align::Center)
        .fill(Role::Raised)
        .radius(5.0)
        .on(State::Hover, move |s| {
            s.fill(hover).stroke(Role::Ink.alpha(0.3)).stroke_width(1.0)
        })
        .on(State::Press, move |s| s.fill(press))
        .a11y(A11y::Button)
        .named(label)
        .focusable()
        .tip("A button")
        .id(id)
}

fn wave(t: usize) -> El {
    canvas(move |s| {
        let pts = (0..64).map(|i| {
            let x = s.width * i as f64 / 63.0;
            let y = s.height * (0.5 + 0.4 * ((i + t) as f64 * 0.3).sin());
            Point::new(x, y)
        });
        vec![Draw::stroke(Path::polyline(pts, false), Role::Primary, 1.0)]
    })
    .w(160.0)
    .h(40.0)
    .id(format!("t{t}/wave"))
}

fn editor(ui: &mut Ui, values: &mut [f64]) -> El {
    let mut v = values.iter_mut();
    let toolbar = row((0..24).map(|i| button(format!("tool{i}"), "Tool")))
        .gap(4.0)
        .wrap();
    let ruler = row((0..60).map(|i| text(format!("{i}")).text_size(10.0).w(20.0))).gap(4.0);
    let tracks = (0..TRACKS).map(|t| {
        let knobs: Vec<El> = (0..4)
            .map(|k| {
                knob(
                    ui,
                    format!("t{t}/k{k}"),
                    "Gain",
                    v.next().unwrap(),
                    0.0..=1.0,
                )
                .el
                .into_el()
            })
            .collect();
        let fader = slider(ui, format!("t{t}/f"), "Level", v.next().unwrap(), 0.0..=1.0)
            .el
            .into_el();
        let head = col([
            text(format!("Track {t}"))
                .id(format!("t{t}/name"))
                .tip("Double-click to rename"),
            row(["M", "S", "R", "Fx", "Arm"].map(|b| button(format!("t{t}/{b}"), b))).gap(2.0),
            row(knobs).gap(8.0),
            fader.w(200.0),
        ])
        .gap(4.0)
        .w(260.0);
        let clips = row((0..CLIPS).map(|c| {
            stack![text(format!("Clip {c}")).text_size(10.0)]
                .w(48.0)
                .h(40.0)
                .fill(Role::Primary.alpha(0.4))
                .stroke(Role::Ink.alpha(0.2))
                .radius(4.0)
                .on(State::Hover, |s| s.stroke(Role::Ink.alpha(0.6)))
                .id(format!("t{t}/c{c}"))
        }))
        .gap(2.0)
        .clip()
        .grow(1.0);
        row([head, wave(t), clips])
            .gap(8.0)
            .pad(6.0)
            .fill(Role::Raised)
            .radius(6.0)
    });
    col([toolbar, ruler].into_iter().chain(tracks))
        .gap(4.0)
        .pad(8.0)
        .fill(Role::Background)
}

fn count(n: &El) -> usize {
    1 + n.children().iter().map(count).sum::<usize>()
}

fn main() {
    let font = Font::new(epaint_default_fonts::HACK_REGULAR).unwrap();
    let mut ui = Ui::new(Theme::DEFAULT).font(font);
    let mut values = vec![0.5; TRACKS * 5];
    let size = |w: f64| Some(Size::new(w, 1400.0));
    let away = PointerInput {
        pos: Some(Point::new(2.0, 2.0)),
        ..Default::default()
    };
    let mut nodes = 0;
    let mut frame = |ui: &mut Ui, w: f64, p: PointerInput, dt: f64| -> f64 {
        let root = editor(ui, &mut values);
        nodes = count(&root);
        let s = cpu_ms();
        std::hint::black_box(ui.frame(root, size(w), p, dt).unwrap().scene.paint.len());
        cpu_ms() - s
    };
    frame(&mut ui, 1800.0, away, 0.016);
    let at = |ui: &Ui, id: &str| {
        let f = ui.scene().unwrap().layout.frame(id).unwrap();
        let (x, y) = f.center();
        PointerInput {
            pos: Some(Point::new(x, y)),
            ..Default::default()
        }
    };
    // Across the clips and buttons of every track, the way a pointer
    // sweeps the editor.
    let over: Vec<PointerInput> = (0..TRACKS)
        .flat_map(|t| {
            [
                format!("t{t}/c{}", t % CLIPS),
                format!("t{t}/S"),
                format!("t{t}/k1"),
            ]
        })
        .map(|id| at(&ui, &id))
        .collect();
    let name = at(&ui, "t3/name");
    // `ONLY=<name>` runs one case long enough to profile.
    let only = std::env::var("ONLY").ok();
    let mut run = |name: &str, f: &mut dyn FnMut(&mut Ui, usize) -> f64| {
        match only.as_deref() {
            Some(o) if o == name => (0..3000).for_each(|i| {
                f(&mut ui, i);
            }),
            Some(_) => return,
            None => {}
        }
        for i in 0..10 {
            f(&mut ui, i);
        }
        let a = ALLOCS.load(Ordering::Relaxed);
        f(&mut ui, 10);
        let allocs = ALLOCS.load(Ordering::Relaxed) - a;
        let mut t: Vec<f64> = (11..71).map(|i| f(&mut ui, i)).collect();
        t.sort_by(f64::total_cmp);
        println!(
            "{name:>8}: {:.3} ms median, {:.3} min, {allocs} allocations",
            t[t.len() / 2],
            t[0]
        );
    };
    run("steady", &mut |ui, _| frame(ui, 1800.0, away, 0.016));
    run("hover", &mut |ui, i| {
        frame(ui, 1800.0, over[i % over.len()], 0.016)
    });
    run("resize", &mut |ui, i| {
        frame(ui, 1500.0 + (i % 50) as f64 * 6.0, away, 0.016)
    });
    // The pointer rests on a name until its tip shows, then leaves: a tip
    // comes or goes on two frames in three.
    let mut tips = 0;
    run("tooltip", &mut |ui, i| {
        let p = if i % 3 == 0 { away } else { name };
        let ms = frame(ui, 1800.0, p, 0.6);
        tips += ui.scene().unwrap().layout.frame("/tip").is_some() as usize;
        ms
    });
    assert!(only.is_some() || tips > 20, "the tip showed {tips} times");
    println!("{nodes} nodes, {:?}", ui.layout_stats());
}
