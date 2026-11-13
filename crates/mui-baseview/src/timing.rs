//! Per-frame stage timings, on with `MUI_FRAME_TIMES=1`: every 60 samples
//! of a stage, its p50, p99 and max go to stderr.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::LazyLock;
use std::time::Instant;

const EVERY: usize = 60;

static ON: LazyLock<bool> = LazyLock::new(|| std::env::var_os("MUI_FRAME_TIMES").is_some());

thread_local! {
    static SEEN: RefCell<HashMap<&'static str, Vec<f64>>> = RefCell::new(HashMap::new());
}

/// `f`, timed in milliseconds as `stage` when timings are on.
pub(crate) fn time<T>(stage: &'static str, f: impl FnOnce() -> T) -> T {
    if !*ON {
        return f();
    }
    let t = Instant::now();
    let out = f();
    note(stage, t.elapsed().as_secs_f64() * 1e3);
    out
}

/// One sample of `stage` (milliseconds, or any unit the stage names).
pub(crate) fn note(stage: &'static str, value: f64) {
    if !*ON {
        return;
    }
    SEEN.with_borrow_mut(|seen| {
        let all = seen.entry(stage).or_default();
        all.push(value);
        if all.len() >= EVERY {
            all.sort_by(f64::total_cmp);
            let at = |q: f64| {
                all.get(((all.len() - 1) as f64 * q) as usize)
                    .copied()
                    .unwrap_or_default()
            };
            eprintln!(
                "mui-frame-times {stage}: p50 {:.2}, p99 {:.2}, max {:.2}",
                at(0.5),
                at(0.99),
                at(1.)
            );
            all.clear();
        }
    });
}
