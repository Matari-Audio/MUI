//! Warm resolves of an editor-like tree of welded, stroked, shelled panels:
//! standing still, sliding a pixel per frame, and from cold caches. Prints
//! the median time and the Boolean passes each frame ran.
use mui_scene::prelude::*;
use mui_scene::{Resolver};
use std::time::Instant;

fn panel(i: usize) -> El {
    let tab = col([block(20., 20.)
        .pill()
        .fill(Role::Primary)
        .id(format!("k{i}"))])
    .pad(8.)
    .id(format!("tab{i}"))
    .shell(4., Role::Raised);
    let body = row((0..4).map(|j| {
        block(40., 24.)
            .radius(6.)
            .fill(Role::Field)
            .stroke(Role::Dim)
            .id(format!("b{i}.{j}"))
    }))
    .gap(6.)
    .pad(10.)
    .id(format!("body{i}"));
    col([tab, body])
        .align(Align::Start)
        .union(Role::Surface)
        .stroke(Role::Dim)
        .stroke_width(1.5)
        .shell(3., Role::Raised)
        .id(format!("panel{i}"))
}

fn tree(shift: f64) -> El {
    col((0..24).map(panel))
        .gap(8.)
        .pad(Spacing::Px(8. + shift))
        .fill(Role::Background)
}

fn main() {
    let font = Font::new(epaint_default_fonts::HACK_REGULAR).unwrap();
    let spec = |shift| {
        SceneSpec::new(tree(shift))
            .offered(Size::new(1280. + 2. * shift, 4000. + 2. * shift))
            .font(font.clone())
    };
    let mut cache = Resolver::default();
    let still = spec(0.);
    cache.resolve(&still).unwrap();
    let mut run = |label: &str, specs: &mut dyn FnMut(usize) -> SceneSpec, cold: bool| {
        let mut times = Vec::new();
        let mut passes = 0;
        for k in 0..41 {
            let s = specs(k);
            if cold {
                cache = Resolver::default();
            }
            let before = mui_geometry::boolean_passes();
            let t = Instant::now();
            std::hint::black_box(cache.resolve(&s).unwrap());
            times.push(t.elapsed().as_secs_f64() * 1e3);
            passes += mui_geometry::boolean_passes() - before;
        }
        times.sort_by(f64::total_cmp);
        println!(
            "{label}: median {:.3} ms, {:.1} boolean passes/frame",
            times[times.len() / 2],
            passes as f64 / 41.
        );
    };
    run("steady", &mut |_| still.clone(), false);
    run("moving", &mut |k| spec(1. + k as f64), false);
    // Every weld reshapes: what a miss costs.
    run("cold", &mut |_| still.clone(), true);
}
