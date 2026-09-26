//! Median CPU bake time for three representative welds.
//! `cargo run -p mui-weld --release --example bake_timing`
use mui_weld::{Brush, Color, Geometry, Point, Quality, Rect, Request, Source, Weld, bake};
use std::time::Instant;

fn rect(x: f64, c: Color, width: f64) -> Source {
    Source {
        shape: Geometry::RoundedRect {
            bounds: Rect::new(x, 0.0, x + 96.0, 96.0),
            radius: 20.0,
        },
        fill: Some(Brush::Solid(c)),
        border: Some(Brush::Solid(Color::srgb(1.0, 1.0, 1.0, 1.0))),
        width,
    }
}

fn main() {
    let (red, blue) = (
        Color::srgb(1.0, 0.0, 0.0, 1.0),
        Color::srgb(0.0, 0.0, 1.0, 1.0),
    );
    let star: Vec<Point> = (0..40)
        .map(|i| {
            let (a, r) = (
                f64::from(i) * std::f64::consts::TAU / 40.0,
                [60.0, 30.0][i as usize % 2],
            );
            Point::new(150.0 + r * a.cos(), 50.0 + r * a.sin())
        })
        .collect();
    let cases = [
        (
            "blend, 2 rects, 2x",
            vec![rect(0.0, red, 4.0), rect(104.0, blue, 8.0)],
            Weld::all(),
        ),
        (
            "crisp, 3 rects, 2x",
            vec![
                rect(0.0, red, 4.0),
                rect(80.0, blue, 8.0),
                rect(160.0, red, 2.0),
            ],
            Weld::crisp(),
        ),
        (
            "blend, rect + 40-gon, 2x",
            vec![
                rect(0.0, red, 4.0),
                Source {
                    shape: Geometry::Contours(vec![star]),
                    fill: Some(Brush::Solid(blue)),
                    border: None,
                    width: 0.0,
                },
            ],
            Weld::all(),
        ),
    ];
    for (name, sources, weld) in cases {
        let r = Request {
            sources,
            weld,
            quality: Quality::at_scale(2.0),
        };
        let mut times: Vec<f64> = (0..15)
            .map(|_| {
                let t = Instant::now();
                std::hint::black_box(bake(&r).unwrap());
                t.elapsed().as_secs_f64() * 1e3
            })
            .collect();
        times.sort_by(f64::total_cmp);
        let b = bake(&r).unwrap();
        let sum = b.rgba.iter().map(|&v| u64::from(v)).sum::<u64>();
        let pts = b.contours.iter().map(Vec::len).sum::<usize>();
        println!(
            "{name}: {}x{} median {:.3} ms (rgba sum {sum}, {pts} contour points)",
            b.width, b.height, times[7]
        );
    }
}
