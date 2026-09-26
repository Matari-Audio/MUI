//! Compare uniform contour offsets against the former variable-width sweep.
use mui_geometry::*;
use std::{hint::black_box, time::Instant};

fn main() {
    let o = OffsetOptions::default();
    // Give the reference enough temporary vertices to finish a rounded border.
    let g = GeometryOptions {
        max_vertices: 65_536,
        ..Default::default()
    };
    let outline = RoundedRect::new(Bounds::new(0., 0., 400., 260.), 30.)
        .unwrap()
        .path();
    let old = || {
        let sweep = union_contours(
            &boundary_band(&outline, WidthProfile::uniform(2.), o).unwrap(),
            o,
            g,
        )
        .unwrap();
        let band = boolean_paths(&sweep, &outline, BooleanOp::Intersection, o, g).unwrap();
        let interior = boolean_paths(&outline, &band, BooleanOp::Difference, o, g).unwrap();
        BorderGeometry { band, interior }
    };
    let new = || {
        border_geometry(
            &outline,
            WidthProfile::uniform(2.),
            BorderAlign::Inside,
            o,
            g,
        )
        .unwrap()
    };
    for _ in 0..5 {
        black_box(old());
        black_box(new());
    }
    let mut before = Vec::new();
    let mut after = Vec::new();
    for _ in 0..51 {
        let start = Instant::now();
        black_box(old());
        before.push(start.elapsed().as_secs_f64());
        let start = Instant::now();
        black_box(new());
        after.push(start.elapsed().as_secs_f64());
    }
    before.sort_by(f64::total_cmp);
    after.sort_by(f64::total_cmp);
    println!(
        "uniform rounded border: sweep {:.3} ms, offsets {:.3} ms, {:.2}x; median of 51 warmed samples",
        before[25] * 1000.,
        after[25] * 1000.,
        before[25] / after[25]
    );
}
