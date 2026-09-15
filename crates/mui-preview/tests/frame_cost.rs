//! Where a frame's time actually goes, measured rather than guessed.
//!
//! `#[ignore]` because it is a measurement, not an assertion.
//!
//! ```text
//! cargo test -p mui-preview --release --test frame_cost -- --ignored --nocapture
//! ```
use std::time::Instant;

#[path = "../../mui-demo/src/generated.rs"]
mod generated;

fn ms(f: impl Fn()) -> f64 {
    let n = 20;
    let t = Instant::now();
    for _ in 0..n {
        f();
    }
    t.elapsed().as_secs_f64() * 1000. / n as f64
}

#[test]
#[ignore = "a measurement"]
fn what_a_frame_costs() {
    let font = epaint_default_fonts::HACK_REGULAR;
    let tol = mui_vello::ARC_TOLERANCE;

    // One sidebar's worth of labels, re-shaped from the font every frame.
    let labels = [
        "MUI preview",
        "mui-layout places, mui-core",
        "merges, vello draws",
        "Pill tab + panel",
        "Constant thickness nes..",
        "Segmented row",
        "Glyph axes",
        "fill surfaces",
        "layout frames",
        "Rebuild",
        "2 surfaces, 147 segments, 88",
        "re-solves",
        "A variable-font outline",
        "rebuilt as MUI geometry at",
        "every axis position. Drag an",
        "glyph",
        "size  281.77",
        "no variation axes",
    ];
    println!(
        "sidebar text_run x{}   {:8.3} ms",
        labels.len(),
        ms(|| {
            for l in labels {
                let _ = mui_text::text_run(font, l, 13., &[], tol).unwrap();
            }
        })
    );
    println!(
        "  one 13px label       {:8.3} ms",
        ms(|| {
            let _ = mui_text::text_run(font, "Constant thickness nes..", 13., &[], tol).unwrap();
        })
    );

    // The chrome's paths, converted to Beziers every frame.
    let rect = mui_geometry::RoundedRect::new(
        mui_geometry::Bounds {
            min: mui_geometry::Point::new(0., 0.),
            max: mui_geometry::Point::new(208., 24.),
        },
        5.,
    )
    .unwrap()
    .path();
    println!(
        "  bez_path x40         {:8.3} ms",
        ms(|| {
            for _ in 0..40 {
                let _ = mui_vello::bez_path(&rect, tol).unwrap();
            }
        })
    );

    // The half that is supposed to be expensive: re-solving the specimen.
    println!(
        "glyph_path @280px      {:8.3} ms",
        ms(|| {
            let _ = mui_text::glyph_path(font, 'a', 280., &[], 0.05).unwrap();
        })
    );

    // The other half of a frame: vello's CPU-side strip generation.
    let glyph = mui_text::glyph_path(font, 'a', 280., &[], 0.05).unwrap();
    let bez = mui_vello::bez_path(&glyph, tol).unwrap();
    println!(
        "vello fill+stroke 1600x1000 {:6.3} ms",
        ms(|| {
            let mut scene = vello_hybrid::Scene::new(1600, 1000);
            scene.set_paint(vello_common::peniko::color::palette::css::WHITE);
            scene.fill_path(&bez);
            scene.stroke_path(&bez);
        })
    );
}

#[test]
#[ignore = "a measurement"]
fn scene_resolution_costs() {
    use mui_core::{resolve_scene, Spacing, SurfaceSpec};
    use std::hint::black_box;

    let basic = generated::generated_scene();
    let offset =
        basic
            .clone()
            .surface(SurfaceSpec::inset("merged-inset", "outer", Spacing::px(4.)));
    for (name, spec) in [("pill resolve", basic), ("merged parallel inset", offset)] {
        for _ in 0..20 {
            black_box(resolve_scene(black_box(&spec)).unwrap());
        }
        let mut samples = Vec::with_capacity(200);
        for _ in 0..200 {
            let start = Instant::now();
            let scene = black_box(resolve_scene(black_box(&spec)).unwrap());
            samples.push(start.elapsed().as_secs_f64() * 1000.);
            assert!(scene.surface("outer").unwrap().basis.vertex_count() >= 6);
        }
        samples.sort_by(f64::total_cmp);
        println!(
            "{name}: median {:.3} ms, p95 {:.3} ms (200 samples)",
            samples[100], samples[189]
        );
    }
}
