use super::*;
use crate::field::{pixel, smooth_min};
use std::sync::Arc;

fn source(x: f64, c: Color, width: f64) -> Source {
    Source {
        shape: Geometry::RoundedRect {
            bounds: Rect::new(x, 0.0, x + 24.0, 24.0),
            radius: 5.0,
        },
        fill: Some(Brush::Solid(c)),
        border: Some(Brush::Solid(Color::srgb(1.0, 1.0, 1.0, 1.0))),
        width,
    }
}
fn red() -> Color {
    Color::srgb(1.0, 0.0, 0.0, 1.0)
}
fn blue() -> Color {
    Color::srgb(0.0, 0.0, 1.0, 1.0)
}
fn request() -> Request {
    Request {
        sources: vec![source(0.0, red(), 1.0), source(26.0, blue(), 4.0)],
        weld: Weld::all(),
        quality: Quality::default(),
    }
}
fn close(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-8, "{a} != {b}");
}

#[test]
fn omitted_options_mean_all_not_shape_only() {
    assert_eq!(Weld::default(), Weld::all());
    assert_eq!(Weld::shape().border, Channel::Keep);
    assert_eq!(Weld::borders().fill, Channel::Keep);
    assert_ne!(Channel::Keep, Channel::Omit);
}
#[test]
fn quadratic_union_matches_two_input_formula() {
    for a in -10..10 {
        for b in -10..10 {
            let (a, b, k) = (a as f64 / 3.0, b as f64 / 3.0, 4.0);
            let h = (0.5 + 0.5 * (b - a) / k).clamp(0.0, 1.0);
            close(
                smooth_min(&[a, b], k).0,
                b + (a - b) * h - k * h * (1.0 - h),
            );
        }
    }
}
#[test]
fn all_permutations_have_the_same_union() {
    let d = [-3.0, -2.0, 1.0];
    let expected = smooth_min(&d, 8.0).0;
    for p in [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ] {
        close(smooth_min(&p.map(|i| d[i]), 8.0).0, expected);
    }
}
#[test]
fn weights_are_nonnegative_sum_to_one_and_have_compact_support() {
    let (_, w) = smooth_min(&[-2.0, -1.0, 100.0], 4.0);
    close(w[..3].iter().sum(), 1.0);
    assert_eq!(w[2], 0.0);
    assert!(w.iter().all(|v| *v >= 0.0));
}
#[test]
fn adding_a_far_source_does_not_change_the_seam() {
    close(
        smooth_min(&[-2.0, 1.0], 4.0).0,
        smooth_min(&[-2.0, 1.0, 1e6], 4.0).0,
    );
}
#[test]
fn hard_union_is_idempotent_and_zero_radius_is_finite() {
    close(smooth_min(&[2.0, 2.0], 0.0).0, 2.0);
    let s = sample_field(
        &request().sources,
        Point::new(25.0, 12.0),
        Weld::all().reach(0.0),
    )
    .unwrap();
    assert!(s.distance > 0.0);
}
#[test]
fn morph_grows_a_real_bridge_and_does_not_move_layout_sources() {
    let r = request();
    let p = Point::new(25.0, 12.0);
    assert!(
        sample_field(&r.sources, p, r.weld.morph(0.0))
            .unwrap()
            .distance
            > 0.0
    );
    assert!(
        sample_field(&r.sources, p, r.weld.morph(1.0))
            .unwrap()
            .distance
            < 0.0
    );
    let mut old = f64::INFINITY;
    for i in 0..=100 {
        let d = sample_field(&r.sources, p, r.weld.morph(i as f64 / 100.0))
            .unwrap()
            .distance;
        assert!(d <= old + 1e-10);
        old = d;
    }
}
#[test]
fn transparent_blue_cannot_tint_opaque_red() {
    let c = Color::weighted([(red(), 0.5), (Color::srgb(0.0, 0.0, 1.0, 0.0), 0.5)]);
    close(c.0[3], 0.5);
    assert_eq!(c.rgba8(), [255, 0, 0, 128]);
}
#[test]
fn colour_weights_follow_shape_order_not_array_index() {
    let r = request();
    let p = Point::new(25.0, 12.0);
    let a = pixel(&r.sources, p, r.weld, 1.0);
    let mut sources = r.sources.clone();
    sources.reverse();
    let b = pixel(&sources, p, r.weld, 1.0);
    for i in 0..4 {
        close(a.0[i], b.0[i]);
    }
}
#[test]
fn original_endpoint_preserves_separate_border_colours() {
    let mut r = request();
    r.sources[0].border = Some(Brush::Solid(blue()));
    let c = pixel(&r.sources, Point::new(0.25, 12.0), r.weld.morph(0.0), 0.1).rgba8();
    assert_eq!(c, [0, 0, 255, 255]);
}
#[test]
fn shape_only_keeps_an_internal_border() {
    let mut r = request();
    r.sources[1] = source(20.0, red(), 1.0);
    r.sources[0].border = Some(Brush::Solid(blue()));
    let p = Point::new(23.5, 12.0);
    let kept = pixel(&r.sources, p, Weld::shape(), 0.1).rgba8();
    let merged = pixel(&r.sources, p, Weld::all(), 0.1).rgba8();
    assert!(kept[2] > 200);
    assert!(merged[0] > 200 && merged[2] < 10);
}
#[test]
fn border_omit_really_removes_it() {
    let r = request();
    let p = Point::new(0.25, 12.0);
    let c = pixel(
        &r.sources,
        p,
        r.weld.border(Channel::Omit).reach(0.0).blend(0.0),
        0.1,
    )
    .rgba8();
    assert_eq!(c, [255, 0, 0, 255]);
}
#[test]
fn hole_winding_is_not_filled_in_by_sampling() {
    let outer = vec![
        Point::new(0.0, 0.0),
        Point::new(20.0, 0.0),
        Point::new(20.0, 20.0),
        Point::new(0.0, 20.0),
    ];
    let hole = vec![
        Point::new(5.0, 5.0),
        Point::new(5.0, 15.0),
        Point::new(15.0, 15.0),
        Point::new(15.0, 5.0),
    ];
    let shape = Geometry::Contours(vec![outer, hole]);
    assert!(shape.distance(Point::new(10.0, 10.0)) > 0.0);
    assert!(shape.distance(Point::new(2.0, 10.0)) < 0.0);
    let r = Request {
        sources: vec![Source {
            shape,
            fill: Some(Brush::Solid(red())),
            border: None,
            width: 0.0,
        }],
        weld: Weld::all().reach(0.0),
        quality: Quality::default(),
    };
    let b = bake(&r).unwrap();
    assert_eq!(b.contours.len(), 2);
    let area = |p: &[Point]| {
        p.iter()
            .zip(p.iter().cycle().skip(1))
            .take(p.len())
            .map(|(a, b)| a.x * b.y - a.y * b.x)
            .sum::<f64>()
    };
    assert!(area(&b.contours[0]) * area(&b.contours[1]) < 0.0);
}
#[test]
fn rejects_nonfinite_options_and_unbounded_rasters_before_allocation() {
    for value in [f64::NAN, f64::INFINITY, -1.0, 2.0] {
        let mut r = request();
        r.weld.progress = value;
        assert!(bake(&r).is_err());
    }
    let mut r = request();
    r.quality.max_pixels = 4;
    assert_eq!(bake(&r).unwrap_err(), Error::Budget);
    r.quality = Quality::default();
    r.sources[0].width = f64::NAN;
    assert!(bake(&r).is_err());
}
#[test]
fn half_covered_border_does_not_double_antialias_alpha() {
    let r = request();
    let c = pixel(
        &r.sources[..1],
        Point::new(0.0, 12.0),
        r.weld.morph(0.0),
        1.0,
    );
    close(c.0[3], 0.5);
}
#[test]
fn cache_reuses_pixels_and_invalidates_material_changes() {
    let mut c = WeldCache::default();
    let mut r = request();
    let a = c.get(&r).unwrap();
    let b = c.get(&r).unwrap();
    assert!(Arc::ptr_eq(&a, &b));
    r.sources[0].width = 7.0;
    let b = c.get(&r).unwrap();
    assert!(!Arc::ptr_eq(&a, &b));
    assert_eq!(c.stats(), (1, 2));
}
#[test]
fn tiny_cache_never_retains_an_oversize_bake() {
    let mut c = WeldCache::with_limit(1);
    let _ = c.get(&request()).unwrap();
    assert_eq!(c.bytes(), 0);
}
#[test]
fn more_than_two_materials_are_supported() {
    let mut r = request();
    r.sources
        .push(source(12.0, Color::srgb(0.0, 1.0, 0.0, 1.0), 2.0));
    let s = sample_field(&r.sources, Point::new(24.0, 12.0), r.weld).unwrap();
    assert_eq!(s.weights().len(), 3);
    close(s.weights().iter().sum(), 1.0);
    assert!(bake(&r).is_ok());
}
#[test]
fn fractional_device_scale_rebakes_instead_of_upscaling() {
    let mut r = request();
    let a = bake(&r).unwrap();
    r.quality.scale = 1.5;
    let b = bake(&r).unwrap();
    assert!(b.width > a.width && b.height > a.height);
}
#[test]
fn gradient_hard_stops_use_the_later_colour() {
    let g = Brush::Linear {
        from: Point::new(0.0, 0.0),
        to: Point::new(1.0, 0.0),
        stops: vec![
            Stop {
                at: 0.0,
                color: red(),
            },
            Stop {
                at: 0.5,
                color: red(),
            },
            Stop {
                at: 0.5,
                color: blue(),
            },
            Stop {
                at: 1.0,
                color: blue(),
            },
        ],
    };
    assert_eq!(g.sample(Point::new(0.5, 0.0)).rgba8(), [0, 0, 255, 255]);
}

#[test]
fn application_scalar_uses_the_same_spatial_weights() {
    let r = request();
    let s = sample_field(&r.sources, Point::new(25.0, 12.0), r.weld).unwrap();
    close(s.scalar(&[2.0, 8.0]).unwrap(), 5.0);
    assert!(s.scalar(&[2.0]).is_err());
    assert!(s.scalar(&[f64::NAN, 8.0]).is_err());
}
#[test]
fn malformed_image_sampling_is_inert_instead_of_indexing_outside_it() {
    let brush = Brush::Image {
        image: Image {
            width: u32::MAX,
            height: u32::MAX,
            rgba: Arc::from([0u8; 4]),
        },
        bounds: Rect::new(0.0, 0.0, 1.0, 1.0),
        fit: ImageFit::Fill,
    };
    assert_eq!(brush.sample(Point::new(1.0, 1.0)).rgba8(), [0, 0, 0, 0]);
}

include!("reference_vectors.rs");

#[test]
fn first_nonzero_morph_frame_cannot_pop_overlapping_antialias_edges() {
    let sources = [source(0.0, red(), 1.0), source(0.0, blue(), 1.0)];
    let at = Point::new(0.0, 12.0);
    let zero = pixel(&sources, at, Weld::all().morph(0.0), 1.0);
    let tiny = pixel(&sources, at, Weld::all().morph(1e-6), 1.0);
    close(zero.0[3], 0.75);
    for i in 0..4 {
        assert!((zero.0[i] - tiny.0[i]).abs() < 1e-7);
    }
}
