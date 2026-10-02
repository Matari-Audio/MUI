use super::*;
use crate::motion;
use mui_vello::kurbo::Shape as _;

const DEMO: &str = include_str!("../examples/demo.cut.json");

fn key(t: f64, v: f64, interp: Interp) -> Key<f64> {
    Key {
        t,
        v,
        interp,
        in_: None,
        out: None,
    }
}

#[test]
fn keys_are_hit_exactly_and_hold_and_linear_behave() {
    let a = Anim::Keys(vec![
        key(0., 10., Interp::Linear),
        key(1., 20., Interp::Hold),
        key(2., 0., Interp::Bezier),
        key(3., 5., Interp::Hold),
    ]);
    assert_eq!(a.at(-1.), 10.);
    assert_eq!(a.at(0.), 10.);
    assert_eq!(a.at(0.5), 15.);
    assert_eq!(a.at(1.), 20.);
    assert_eq!(a.at(1.99), 20.);
    assert_eq!(a.at(2.), 0.);
    assert_eq!(a.at(3.), 5.);
    assert_eq!(a.at(9.), 5.);
}

#[test]
fn bezier_is_monotone_in_time_and_lands_on_its_keys() {
    // Handles dragged far past the segment in time are clamped, so the curve
    // still has one value per time and increasing keys with flat handles
    // never go backwards.
    for (out, inn) in [
        (None, None),
        (Some([0.9, 0.]), Some([-0.9, 0.])),
        (Some([5., 0.]), Some([-5., 0.])),
        (Some([0., 0.]), Some([0., 0.])),
    ] {
        let a = Anim::Keys(vec![
            Key {
                out,
                ..key(1., 100., Interp::Bezier)
            },
            Key {
                in_: inn,
                ..key(2., 300., Interp::Hold)
            },
        ]);
        assert_eq!(a.at(1.), 100.);
        assert_eq!(a.at(2.), 300.);
        let mut last = f64::MIN;
        for i in 0..=1000 {
            let v = a.at(1. + f64::from(i) / 1000.);
            assert!(v >= last - 1e-9, "{out:?} {inn:?} at {i}: {v} < {last}");
            last = v;
        }
    }
    // Default handles are an ease: slow at both ends, symmetric.
    let ease = Anim::Keys(vec![key(0., 0., Interp::Bezier), key(1., 1., Interp::Hold)]);
    assert!(ease.at(0.1) < 0.1 && ease.at(0.9) > 0.9);
    assert!((ease.at(0.5) - 0.5).abs() < 1e-9);
    // Value handles overshoot: an `out` pointing up past the next key.
    let over = Anim::Keys(vec![
        Key {
            out: Some([0.5, 3.]),
            ..key(0., 0., Interp::Bezier)
        },
        key(1., 1., Interp::Hold),
    ]);
    assert!((0..100).any(|i| over.at(f64::from(i) / 100.) > 1.));
}

#[test]
fn colours_parse_print_and_tween() {
    let c = Rgba::try_from("#7c6cff40".to_owned()).unwrap();
    assert_eq!(c.0, [0x7c, 0x6c, 0xff, 0x40]);
    assert_eq!(String::from(c), "#7c6cff40");
    assert_eq!(String::from(Rgba([1, 2, 3, 255])), "#010203");
    assert!(Rgba::try_from("#12345".to_owned()).is_err());
    assert!(Rgba::try_from("#gg0000".to_owned()).is_err());
    let fade = Anim::Keys(vec![
        Key {
            t: 0.,
            v: Rgba([0, 0, 0, 255]),
            interp: Interp::Linear,
            in_: None,
            out: None,
        },
        Key {
            t: 1.,
            v: Rgba([200, 100, 0, 255]),
            interp: Interp::Hold,
            in_: None,
            out: None,
        },
    ]);
    assert_eq!(fade.at(0.5), Rgba([100, 50, 0, 255]));
}

#[test]
fn the_demo_loads_evaluates_and_round_trips_byte_for_byte() {
    let p = Project::load(DEMO).unwrap();
    assert_eq!(p.scenes.len(), 3);
    // The saved form is the hand-written form: an editor save of an
    // untouched file is not a diff.
    assert_eq!(p.to_json(), DEMO);
    assert_eq!(Project::load(&p.to_json()).unwrap(), p);
    let shapes = p.scene("shapes").unwrap();
    let f = eval(&p, shapes, 1.5);
    let card = f.layers.iter().find(|l| l.id == "card").unwrap();
    assert_eq!((card.x, card.rotation, card.radius), (1040., 180., 90.));
    // Seekable: the same time gives the same frame whatever came before.
    assert_eq!(eval(&p, shapes, 0.7), eval(&p, shapes, 0.7));
}

#[test]
fn load_sorts_keys_and_rejects_bad_projects() {
    let p = Project::load(
        r#"{"size":[64,64],"fps":30,"scenes":[{"name":"a","duration":1,"layers":[
            {"id":"r","kind":"rect","x":[{"t":1,"v":10,"interp":"hold"},{"t":0,"v":0,"interp":"linear"}]}]}]}"#,
    )
    .unwrap();
    let l = &p.scenes[0].layers[0];
    assert_eq!(l.x.at(0.5), 5.);
    assert_eq!(l.width.at(0.), 100., "left out is the default");
    for bad in [
        r#"{"size":[0,64],"fps":30,"scenes":[]}"#,
        r#"{"size":[64,64],"fps":0,"scenes":[]}"#,
        r#"{"size":[64,64],"fps":30,"scenes":[{"name":"a","duration":0}]}"#,
        r#"{"size":[64,64],"fps":30,"scenes":[{"name":"a","duration":1,"layers":[{"id":"r","kind":"rect"},{"id":"r","kind":"rect"}]}]}"#,
        r#"{"size":[64,64],"fps":30,"scenes":[{"name":"a","duration":1,"layers":[{"id":"r","kind":"rect","x":[]}]}]}"#,
        r#"{"size":[64,64],"fps":30,"scenes":[{"name":"a","duration":1,"layers":[{"id":"r","kind":"blob"}]}]}"#,
    ] {
        assert!(Project::load(bad).is_err(), "accepted {bad}");
    }
}

#[test]
fn the_renderer_draws_layers_where_eval_puts_them() {
    let p = Project::load(DEMO).unwrap();
    let shapes = p.scene("shapes").unwrap();
    let mut r = Renderer::new(320, 180);
    let (px, quads) = r.draw(&eval(&p, shapes, 0.)).unwrap();
    assert_eq!(px.len(), 320 * 180 * 4);
    // The card starts at x 240, y 300 of 1280x720 and is 180 square, unrotated.
    let card = quads.iter().find(|q| q.id == "card").unwrap();
    assert_eq!(card.pts[0], [150., 210.]);
    assert_eq!(card.pts[2], [330., 390.]);
    // Its centre pixel, at a quarter scale, is the card's purple.
    let at = |x: usize, y: usize| &px[(y * 320 + x) * 4..][..4];
    assert_eq!(at(60, 75), [0x7c, 0x6c, 0xff, 255]);
    assert_eq!(at(5, 5), [0x12, 0x13, 0x1a, 255], "background");
    // Text is measured: the label's quad is wider than it is tall.
    let label = quads.iter().find(|q| q.id == "label").unwrap();
    assert!(label.pts[1][0] - label.pts[0][0] > 3. * (label.pts[3][1] - label.pts[0][1]));
    // A later time draws something else.
    let (later, _) = r.draw(&eval(&p, shapes, 1.)).unwrap();
    assert_ne!(px, later);
}

/// A 16x8 PNG, left half red, right half half-transparent blue.
#[cfg(not(target_arch = "wasm32"))]
fn test_png() -> Vec<u8> {
    let px: Vec<u8> = (0..8 * 16)
        .flat_map(|i| {
            if i % 16 < 8 {
                [255, 0, 0, 255]
            } else {
                [0, 0, 255, 128]
            }
        })
        .collect();
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, 16, 8);
    enc.set_color(png::ColorType::Rgba);
    enc.write_header().unwrap().write_image_data(&px).unwrap();
    out
}

/// Every GPU engine (ring readback, float shutter) matches the CPU path,
/// frame for frame and in order, on every demo scene (shapes and text) plus
/// an image layer. Skips on a machine with no GPU adapter.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn gpu_frames_match_the_cpu_in_order() {
    let mut p = Project::load(DEMO).unwrap();
    p.scenes[0].layers.push(
        serde_json::from_str(
            r#"{"id": "pic", "kind": "image", "path": "t.png", "x": 900.0, "y": 500.0,
                "width": 320.0, "height": 160.0, "radius": 20.0, "rotation": 10.0}"#,
        )
        .unwrap(),
    );
    let png = test_png();
    // Five frames per scene through a ring of three, four subframes each.
    let frames: Vec<Vec<Frame>> = p
        .scenes
        .iter()
        .flat_map(|s| {
            let p = &p;
            (0..5).map(move |i| {
                (0..4)
                    .map(|k| eval(p, s, 0.5 + f64::from(i) * 0.1 + f64::from(k) * 0.02))
                    .collect()
            })
        })
        .collect();
    let mut cpu = Renderer::new(320, 180);
    cpu.add_asset("t.png", &png).unwrap();
    let want: Vec<Vec<u8>> = frames
        .iter()
        .map(|subs| shutter(&mut cpu, &mut Vec::new(), subs).unwrap())
        .collect();
    let mut engines = Vec::new();
    for engine in [Engine::Classic, Engine::Sparse] {
        let mut gpu = match Offline::new([320, 180], engine) {
            Ok(g) => g,
            Err(e) => return eprintln!("skipped: no GPU ({e})"),
        };
        gpu.assets.add_png("t.png", &png).unwrap();
        let mut got = Vec::new();
        for subs in &frames {
            got.extend(gpu.push(subs).unwrap());
        }
        got.extend(gpu.finish().unwrap());
        assert_eq!(got.len(), frames.len());
        // Antialiasing (text, image filtering) differs a little between
        // CPU and GPU at edges; the frames must not.
        for (i, (g, c)) in got.iter().zip(&want).enumerate() {
            let off = differing(g, c);
            assert!(
                off < c.len() / 100,
                "{engine:?} frame {i}: {off} channels differ from the CPU"
            );
        }
        engines.push(got);
    }
    // vello_gpu is a drop-in for classic Vello.
    for (i, (s, c)) in engines[1].iter().zip(&engines[0]).enumerate() {
        let off = differing(s, c);
        assert!(
            off < c.len() / 1000,
            "frame {i}: {off} channels differ from classic"
        );
    }
}

/// Channels further apart than 8 of 255.
#[cfg(not(target_arch = "wasm32"))]
fn differing(a: &[u8], b: &[u8]) -> usize {
    assert_eq!(a.len(), b.len());
    a.iter().zip(b).filter(|(a, b)| a.abs_diff(**b) > 8).count()
}

/// The frame-parallel CPU pool hands frames back in push order, the same
/// pixels one renderer draws alone.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn the_cpu_pool_keeps_frames_in_order() {
    let p = Project::load(DEMO).unwrap();
    let s = p.scene("shapes").unwrap();
    let frames: Vec<Vec<Frame>> = (0..12)
        .map(|i| {
            (0..1 + i % 3)
                .map(|k| eval(&p, s, f64::from(i) * 0.2 + f64::from(k) * 0.01))
                .collect()
        })
        .collect();
    let mut one = Renderer::new(160, 90);
    let want: Vec<Vec<u8>> = frames
        .iter()
        .map(|subs| shutter(&mut one, &mut Vec::new(), subs).unwrap())
        .collect();
    let mut pool = CpuPool::new(160, 90, 4, 0, &Assets::default());
    let mut got = Vec::new();
    for subs in &frames {
        got.extend(pool.push(subs.clone()).unwrap());
    }
    got.extend(pool.finish().unwrap());
    // Each worker's glyph atlas warms up on its own, so text may differ by a
    // hair; a frame out of order differs by whole shapes.
    assert_eq!(got.len(), want.len());
    for (i, (g, w)) in got.iter().zip(&want).enumerate() {
        let off = differing(g, w);
        assert!(off < w.len() / 500, "pool frame {i}: {off} channels differ");
    }
    // A multithreaded rasteriser draws the same frame.
    let mut mt = Renderer::with_threads(160, 90, 3);
    let off = differing(
        &shutter(&mut mt, &mut Vec::new(), &frames[4]).unwrap(),
        &want[4],
    );
    assert!(off < want[4].len() / 500, "{off} channels differ");
}

const SHOWCASE: &str = include_str!("../examples/showcase.cut.json");

fn one_layer(layer: &str) -> Project {
    Project::load(&format!(
        r#"{{"size":[400,200],"fps":30,"scenes":[{{"name":"a","duration":2,"layers":[{layer}]}}]}}"#
    ))
    .unwrap()
}

#[test]
fn the_showcase_loads_round_trips_and_seeks() {
    let p = Project::load(SHOWCASE).unwrap();
    assert_eq!(p.to_json(), SHOWCASE);
    for s in &p.scenes {
        assert_eq!(eval(&p, s, 1.3), eval(&p, s, 1.3));
    }
}

#[test]
fn nested_properties_are_checked_listed_and_sampled() {
    // An empty key list or bad path data is refused wherever it sits.
    for bad in [
        r#"{"id":"t","kind":"text","text":"a","animators":[{"offset":[]}]}"#,
        r#"{"id":"p","kind":"path","d":"M 0 0 Q"}"#,
        r#"{"id":"d","kind":"duplicator","layout":"path","along":"nonsense"}"#,
        r#"{"id":"d","kind":"path","d":"M0 0","deformers":[{"kind":"melt"}]}"#,
    ] {
        let json = format!(
            r#"{{"size":[64,64],"fps":30,"scenes":[{{"name":"a","duration":1,"layers":[{bad}]}}]}}"#
        );
        assert!(Project::load(&json).is_err(), "accepted {bad}");
    }
    let p = one_layer(
        r#"{"id":"t","kind":"text","text":"ab","animators":[{"offset":[{"t":1,"v":1},{"t":0,"v":0,"interp":"linear"}]}],
            "deformers":[{"kind":"wave"}]}"#,
    );
    let l = &p.scenes[0].layers[0];
    let names: Vec<String> = l.props().into_iter().map(|(n, _)| n).collect();
    for n in [
        "font_size",
        "tracking",
        "animators.0.offset",
        "animators.0.fill",
        "deformers.0.amplitude",
        "trim_end",
    ] {
        assert!(names.iter().any(|m| m == n), "{n} missing from {names:?}");
    }
    assert!(
        !names.iter().any(|m| m == "width" || m == "count"),
        "text has no width or count"
    );
    // Nested keys were sorted on load, like top-level ones.
    assert_eq!(l.prop("animators.0.offset").unwrap().at(0.5), 0.5);
}

#[test]
fn text_units_split_by_char_word_and_line() {
    let s = "ab cd\nef";
    assert_eq!(text_units(s, Unit::Char), [0, 1, 2, 3, 4, 5, 6]);
    assert_eq!(text_units(s, Unit::Word), [0, 0, 0, 1, 1, 2, 2]);
    assert_eq!(text_units(s, Unit::Line), [0, 0, 0, 0, 0, 1, 1]);
}

#[test]
fn selectors_step_stagger_and_shuffle_deterministically() {
    let fx = |a: &Animator, n: usize, t: f64| {
        motion::apply(std::slice::from_ref(a), n, Rgba([255; 4]), t, |_| {
            (0..n).collect()
        })
    };
    // Typewriter: halfway through, the first half shows and the rest waits.
    let tw = Animator::preset("typewriter", 0., 1.).unwrap();
    let o: Vec<f64> = fx(&tw, 10, 0.5).iter().map(|f| f.opacity).collect();
    assert_eq!(o, [1., 1., 1., 1., 1., 0., 0., 0., 0., 0.]);
    // Cascade: each glyph runs the same curve a stagger after the last.
    let c = Animator::preset("cascade", 0., 0.5).unwrap();
    let at = fx(&c, 4, 0.3);
    assert!(
        at[0].y < at[1].y && at[1].y < at[3].y,
        "later glyphs are further back"
    );
    assert!((fx(&c, 4, 0.3 + 0.04)[1].y - at[0].y).abs() < 1e-9);
    assert_eq!(fx(&c, 4, 5.)[3].y, 0.);
    // A seeded shuffle: a permutation, the same every time, another per seed.
    let rank = |seed| {
        let a = Animator {
            order: Order::Random,
            seed,
            stagger: Anim::Value(1.),
            amount: Anim::Keys(vec![key(0., 1., Interp::Hold), key(0.5, 0., Interp::Hold)]),
            x: Anim::Value(1.),
            ..Animator::default()
        };
        // At t = k + 0.75 exactly the units ranked above k are still selected.
        (0..8)
            .map(|u| {
                (0..8)
                    .filter(|&k| fx(&a, 8, f64::from(k) + 0.75)[u].x > 0.)
                    .count()
            })
            .collect::<Vec<_>>()
    };
    let r = rank(1);
    let mut sorted = r.clone();
    sorted.sort_unstable();
    assert_eq!(sorted, (0..8).collect::<Vec<_>>());
    assert_eq!(r, rank(1));
    assert_ne!(r, rank(2));
    // Falloff shapes: a ramp across all units rises, a triangle peaks mid.
    let ramp = Animator {
        shape: Falloff::RampUp,
        x: Anim::Value(1.),
        ..Animator::default()
    };
    let xs: Vec<f64> = fx(&ramp, 4, 0.).iter().map(|f| f.x).collect();
    assert_eq!(xs, [0.125, 0.375, 0.625, 0.875]);
}

#[test]
fn trim_keeps_the_asked_part_of_each_contour() {
    let piece = || vector::Piece {
        path: vector::parse("M 0 0 L 100 0"),
        fill: None,
        stroke: None,
    };
    let bounds = |p: &[vector::Piece]| {
        let b = p[0].path.bounding_box();
        (b.x0.round(), b.x1.round())
    };
    let mut p = [piece()];
    vector::trim(&mut p, [0.25, 0.75, 0.]);
    assert_eq!(bounds(&p), (25., 75.));
    // Past the end it wraps: 0.8..1 and 0..0.1, two open contours.
    let mut p = [piece()];
    vector::trim(&mut p, [0.3, 0.6, 0.5]);
    let moves = p[0]
        .path
        .elements()
        .iter()
        .filter(|e| matches!(e, mui_vello::kurbo::PathEl::MoveTo(_)))
        .count();
    assert_eq!(moves, 2);
    assert_eq!(bounds(&p), (0., 100.));
    let mut p = [piece()];
    vector::trim(&mut p, [0.5, 0.5, 0.]);
    assert!(
        p[0].path.elements().is_empty(),
        "an empty range draws nothing"
    );
}

#[test]
fn deformers_are_deterministic_and_neutral_at_zero() {
    let run = |d: Deform| {
        let mut p = [vector::Piece {
            path: vector::parse("M -100 0 L 100 0"),
            fill: None,
            stroke: None,
        }];
        vector::deform(&mut p, &[d]);
        p[0].path.elements().to_vec()
    };
    let flat = run(Deform::Bend {
        angle: 0.,
        length: 100.,
    });
    assert!(flat.len() > 40, "subdivided so a straight edge can bend");
    let still = |els: &[mui_vello::kurbo::PathEl]| {
        els.iter().all(|e| match e {
            mui_vello::kurbo::PathEl::MoveTo(p) | mui_vello::kurbo::PathEl::LineTo(p) => {
                p.y.abs() < 1e-9
            }
            _ => true,
        })
    };
    assert!(still(&flat));
    let noise = |seed| {
        run(Deform::Noise {
            amount: 10.,
            frequency: 0.013,
            phase: 0.3,
            seed,
        })
    };
    assert_eq!(noise(4), noise(4));
    assert_ne!(noise(4), noise(5));
    assert!(!still(&noise(4)));
    assert!(!still(&run(Deform::Wave {
        amplitude: 5.,
        wavelength: 50.,
        phase: 0.1
    })));
    let bent = run(Deform::Bend {
        angle: 90.,
        length: 100.,
    });
    assert!(!still(&bent));
}

#[test]
fn duplicators_lay_copies_out_and_stagger_them() {
    let p = one_layer(
        r#"{"id":"d","kind":"duplicator","x":200,"y":100,"width":10,"height":10,"count":6,"columns":3,"spacing_x":40,"spacing_y":20}"#,
    );
    let d = p.scenes[0].layers[0].at(0.);
    assert_eq!((d.count, d.fx.len()), (6, 6));
    let pieces = vector::duplicator(&d, Shape::Rect, "", Layout::Grid, "", false);
    let centres: Vec<(f64, f64)> = pieces
        .iter()
        .map(|p| {
            let b = p.path.bounding_box().center();
            (b.x.round(), b.y.round())
        })
        .collect();
    assert_eq!(
        centres,
        [
            (-40., -10.),
            (0., -10.),
            (40., -10.),
            (-40., 10.),
            (0., 10.),
            (40., 10.)
        ]
    );
    // Radial starts at twelve o'clock; a path layout spaces copies by length.
    let ring = vector::duplicator(&d, Shape::Ellipse, "", Layout::Radial, "", true);
    let top = ring[0].path.bounding_box().center();
    assert!((top.x.abs() < 1e-6) && (top.y + 200.).abs() < 1e-6);
    let along = vector::duplicator(&d, Shape::Rect, "", Layout::Path, "M 0 0 L 100 0", false);
    let xs: Vec<f64> = along
        .iter()
        .map(|p| p.path.bounding_box().center().x.round())
        .collect();
    assert_eq!(xs, [0., 20., 40., 60., 80., 100.]);
    // The drawn copies are where the layout put them, in the project.
    let mut r = Renderer::new(400, 200);
    let (px, quads) = r.draw(&eval(&p, &p.scenes[0], 0.)).unwrap();
    assert_eq!(quads[0].pts[0], [155., 85.]);
    let at = |x: usize, y: usize| &px[(y * 400 + x) * 4..][..4];
    assert_eq!(at(160, 90), [255; 4], "a copy");
    assert_eq!(at(180, 90), [16, 16, 20, 255], "between copies");
}

#[test]
fn text_is_multiline_and_its_glyphs_move() {
    let p = one_layer(
        r#"{"id":"t","kind":"text","text":"Hi\nthere","x":200,"y":100,"font_size":40,
            "animators":[{"ease":"step","start":0.45,"y":30}]}"#,
    );
    let d = p.scenes[0].layers[0].at(0.);
    assert_eq!(d.fx.len(), 7);
    assert_eq!(
        d.fx.iter().map(|f| f.y).collect::<Vec<_>>(),
        [0., 0., 0., 30., 30., 30., 30.]
    );
    let r = Renderer::new(400, 200);
    let two = r.assets.layers(&eval(&p, &p.scenes[0], 0.)).unwrap();
    let q = &two.quads[0].pts;
    assert!(q[2][1] - q[0][1] > 80., "two lines of 40 px text: {q:?}");
    // Without the animator the block is shorter: the second line moved down.
    let flat =
        one_layer(r#"{"id":"t","kind":"text","text":"Hi\nthere","x":200,"y":100,"font_size":40}"#);
    let q0 = r
        .assets
        .layers(&eval(&flat, &flat.scenes[0], 0.))
        .unwrap()
        .quads[0]
        .pts;
    assert!(q[2][1] - q[0][1] > q0[2][1] - q0[0][1] + 25.);
}

#[test]
fn svg_and_lottie_layers_draw_their_files() {
    let p = Project::load(SHOWCASE).unwrap();
    let s = p.scene("import").unwrap();
    let mut r = Renderer::new(1280, 720);
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/");
    for f in ["mark.svg", "spin.json"] {
        r.add_asset(f, &std::fs::read(format!("{dir}{f}")).unwrap())
            .unwrap();
    }
    let (px, quads) = r.draw(&eval(&p, s, 1.)).unwrap();
    let at = |x: usize, y: usize| px[(y * 1280 + x) * 4..][..4].to_vec();
    // The SVG's yellow triangle, and the Lottie's pink dot, at their layers.
    assert_eq!(at(330, 330), [0xff, 0xcf, 0x5c, 255]);
    assert_eq!(at(760, 360), [0xff, 0x5c, 0x8a, 255]);
    // The Lottie plays: its frame at another time differs.
    let (later, _) = r.draw(&eval(&p, s, 1.5)).unwrap();
    assert_ne!(px, later);
    let spin = quads.iter().find(|q| q.id == "spin").unwrap();
    assert!(spin.pts[1][0] - spin.pts[0][0] > 100.);
    // Time maps to the file's frames: `time` + t * speed, clamped without loop.
    let slow = s.layers.iter().find(|l| l.id == "spin-slow").unwrap();
    assert_eq!(slow.at(1.).time, 1.);
    assert!(r.add_asset("bad.json", b"{").is_err());
    assert!(r.add_asset("bad.svg", b"<nope").is_err());
}

const FX: &str = include_str!("../examples/effects.cut.json");

#[test]
fn effects_load_default_clamp_and_round_trip() {
    let p = Project::load(FX).unwrap();
    assert_eq!(p.to_json(), FX);
    assert!(p.has_effects() && !Project::load(DEMO).unwrap().has_effects());
    let s = p.scene("broadcast").unwrap();
    let f = eval(&p, s, 0.75);
    let card = f.layers.iter().find(|l| l.id == "card").unwrap();
    // A keyed parameter evaluates like any property.
    assert_eq!(card.effects[0].values["radius"], fx::Val::Num(7.));
    // Left-out parameters are the schema's defaults.
    assert_eq!(f.effects[2].values["size"], fx::Val::Num(1.5));
    assert_eq!(f.seed, 22, "0.75 s at 30 fps is frame 22");
    // Subframes of one output frame share the seed.
    assert_eq!(eval(&p, s, 22. / 30. + 0.5 / 30. * 0.99).seed, 22);
    let clamped = Project::load(
        r#"{"size":[64,64],"fps":30,"scenes":[{"name":"a","duration":1,"effects":[{"type":"blur","radius":1000}]}]}"#,
    )
    .unwrap();
    let f = eval(&clamped, &clamped.scenes[0], 0.);
    assert_eq!(f.effects[0].values["radius"], fx::Val::Num(64.));
    for bad in [
        r#"{"type":"sparkle"}"#,
        r##"{"type":"blur","radius":"#ff0000"}"##,
        r#"{"type":"blur","sigma":2}"#,
        r#"{"type":"levels","tint":3}"#,
        r#"{"type":"blur","radius":[]}"#,
    ] {
        let json = format!(
            r#"{{"size":[64,64],"fps":30,"scenes":[{{"name":"a","duration":1,"layers":[{{"id":"r","kind":"rect","effects":[{bad}]}}]}}]}}"#
        );
        assert!(Project::load(&json).is_err(), "accepted {bad}");
    }
    // Every shader's Params can be packed: colours first, then numbers.
    for d in fx::EFFECTS {
        let first_num = d
            .params
            .iter()
            .position(|p| matches!(p.ty, fx::Ty::Num { .. }));
        assert!(
            d.params
                .iter()
                .skip(first_num.unwrap_or(d.params.len()))
                .all(|p| matches!(p.ty, fx::Ty::Num { .. })),
            "{}: colours must come first",
            d.name
        );
    }
}

/// A 160x90 frame: black, a white 40-pixel square in the middle carrying
/// `layer` effects, and `scene` effects, at `t`.
#[cfg(not(target_arch = "wasm32"))]
fn square(layer: &str, scene: &str, t: f64) -> (Project, Frame) {
    let json = format!(
        r##"{{"size":[160,90],"fps":30,"scenes":[{{"name":"a","duration":2,"background":"#000000",
        "layers":[{{"id":"sq","kind":"rect","x":80,"y":45,"width":40,"height":40,"effects":[{layer}]}}],
        "effects":[{scene}]}}]}}"##
    );
    let p = Project::load(&json).unwrap();
    let f = eval(&p, &p.scenes[0], t);
    (p, f)
}

/// Straight RGBA of each output frame (a list of subframes) on the GPU, on
/// both engines: they must agree (edges aside), so effects hold on each.
#[cfg(not(target_arch = "wasm32"))]
fn gpu_frames(frames: &[Vec<Frame>]) -> Option<Vec<Vec<u8>>> {
    let run = |engine| {
        let mut g = match Offline::new([160, 90], engine) {
            Ok(g) => g,
            Err(e) => {
                eprintln!("skipped: no GPU ({e})");
                return None;
            }
        };
        let mut out = Vec::new();
        for subs in frames {
            out.extend(g.push(subs).unwrap());
        }
        out.extend(g.finish().unwrap());
        Some(out)
    };
    let classic = run(Engine::Classic)?;
    let sparse = run(Engine::Sparse)?;
    for (c, s) in classic.iter().zip(&sparse) {
        let off = c.iter().zip(s).filter(|(a, b)| a.abs_diff(**b) > 8).count();
        assert!(
            off < c.len() / 100,
            "the engines disagree on {off} channels"
        );
    }
    Some(classic)
}

#[cfg(not(target_arch = "wasm32"))]
fn px(img: &[u8], x: usize, y: usize) -> [u8; 4] {
    img[(y * 160 + x) * 4..][..4].try_into().unwrap()
}

#[cfg(not(target_arch = "wasm32"))]
fn near(a: [u8; 4], b: [u8; 4], tol: u8) -> bool {
    a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= tol)
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn effects_draw_what_they_say() {
    let one = |layer: &str, scene: &str| {
        let (_, f) = square(layer, scene, 0.5);
        gpu_frames(&[vec![f]]).map(|mut v| v.remove(0))
    };
    let Some(plain) = one("", "") else { return };
    assert_eq!(px(&plain, 80, 45), [255; 4]);
    assert_eq!(px(&plain, 10, 10), [0, 0, 0, 255]);

    // Levels: a full red tint by luminance turns white red, black stays.
    let red = one(
        "",
        r##"{"type":"levels","tint":"#ff0000","tint_amount":1}"##,
    )
    .unwrap();
    assert!(
        near(px(&red, 80, 45), [255, 0, 0, 255], 2),
        "{:?}",
        px(&red, 80, 45)
    );
    assert_eq!(px(&red, 10, 10), [0, 0, 0, 255]);

    // Blur on the layer: the middle stays white, the edge goes half way,
    // far outside stays clear.
    let blur = one(r#"{"type":"blur","radius":4}"#, "").unwrap();
    assert!(near(px(&blur, 80, 45), [255; 4], 1));
    let edge = px(&blur, 60, 45)[0];
    assert!((90..170).contains(&edge), "edge {edge}");
    assert_eq!(px(&blur, 40, 45), [0, 0, 0, 255]);

    // Chromatic aberration: red magnified and blue shrunk about the centre,
    // so a red fringe just outside both edges and no blue just inside.
    let ca = one("", r#"{"type":"chromatic","amount":8}"#).unwrap();
    for x in [58, 101] {
        let p = px(&ca, x, 45);
        assert!(p[0] > 150 && p[1] < 10 && p[2] < 10, "outside {x}: {p:?}");
    }
    for x in [61, 98] {
        let p = px(&ca, x, 45);
        assert!(p[0] > 250 && p[1] > 250 && p[2] < 100, "inside {x}: {p:?}");
    }

    // CRT: the corners fall off the curved screen, scanlines darken rows.
    let crt = one(
        "",
        r#"{"type":"crt","curvature":0.3,"scanlines":0.8,"line":4,"vignette":0}"#,
    )
    .unwrap();
    assert_eq!(px(&crt, 0, 0)[3], 0);
    let rows: Vec<u8> = (35..55).map(|y| px(&crt, 80, y)[0]).collect();
    let (lo, hi) = (*rows.iter().min().unwrap(), *rows.iter().max().unwrap());
    assert!(hi > 200 && lo < 120, "scanlines {rows:?}");

    // Plasma fills the square's shape and nothing else, and is not flat.
    let pl = one(r#"{"type":"plasma"}"#, "").unwrap();
    assert_eq!(px(&pl, 10, 10), [0, 0, 0, 255]);
    let inside: std::collections::HashSet<[u8; 4]> =
        (62..98).step_by(5).map(|x| px(&pl, x, 45)).collect();
    assert!(inside.len() > 3, "{inside:?}");
    assert!(
        inside
            .iter()
            .all(|p| *p != [255; 4] && *p != [0, 0, 0, 255])
    );

    // Displacement moves pixels, and the same frame displaces the same way.
    let d = one(r#"{"type":"displace","amount":6,"scale":10}"#, "").unwrap();
    assert_ne!(d, plain);
    assert_eq!(
        d,
        one(r#"{"type":"displace","amount":6,"scale":10}"#, "").unwrap()
    );

    let db = one(r#"{"type":"directional_blur","length":20,"angle":90}"#, "").unwrap();
    assert!(near(px(&db, 80, 45), [255; 4], 1));
    assert!(
        px(&db, 80, 30)[0] < 250 && px(&db, 80, 30)[0] > 10,
        "smeared down"
    );
    assert_eq!(px(&db, 50, 45), [0, 0, 0, 255], "not across");
}

/// One frame of a 160x90 project: `layers` over `background`, at 0.5 s.
#[cfg(not(target_arch = "wasm32"))]
fn frame_of(background: &str, layers: &str) -> Frame {
    let json = format!(
        r##"{{"size":[160,90],"fps":30,"scenes":[{{"name":"a","duration":2,"background":"{background}","layers":[{layers}]}}]}}"##
    );
    let p = Project::load(&json).unwrap();
    eval(&p, &p.scenes[0], 0.5)
}

/// Glow: each mode lights where it says, the falloff is smooth (no steps
/// from 8-bit levels), and the radius keys without jumps.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn glow_lights_where_its_mode_says() {
    let one = |layer: &str, scene: &str| {
        let (_, f) = square(layer, scene, 0.5);
        gpu_frames(&[vec![f]]).map(|mut v| v.remove(0))
    };
    let Some(plain) = one("", "") else { return };

    // Outer, tinted red: a red haze just outside, fading out; the square
    // itself white; far off, black.
    let outer = one(
        r##"{"type":"glow","mode":"outer","color":"#ff0000","tint":1,"radius":8}"##,
        "",
    )
    .unwrap();
    let (a, b) = (px(&outer, 56, 45), px(&outer, 50, 45));
    assert!(a[0] > 60 && a[1] < a[0] / 4 && a[2] < a[0] / 4, "{a:?}");
    assert!(b[0] < a[0] && b[0] > 2, "fading: {b:?}");
    assert_eq!(px(&outer, 80, 45), [255; 4]);
    assert_eq!(px(&outer, 5, 5), [0, 0, 0, 255]);

    // Inner: red just inside the edges, the middle still white, outside
    // untouched.
    let inner = one(
        r##"{"type":"glow","mode":"inner","color":"#ff0000","tint":1,"radius":4}"##,
        "",
    )
    .unwrap();
    let e = px(&inner, 61, 45);
    assert!(e[0] > 200 && e[1] < 200, "edge {e:?}");
    assert!(
        near(px(&inner, 80, 45), [255; 4], 8),
        "{:?}",
        px(&inner, 80, 45)
    );
    assert_eq!(px(&inner, 56, 45), [0, 0, 0, 255]);

    // Bloom over the scene: the white square lights the black round it,
    // falling off smoothly; nothing passes a threshold of 1 with no knee.
    let bloom = one(
        "",
        r#"{"type":"glow","threshold":0.5,"radius":24,"intensity":1.5}"#,
    )
    .unwrap();
    let row: Vec<i32> = (101..150)
        .map(|x| i32::from(px(&bloom, x, 45)[0]))
        .collect();
    assert!(row[3] > 20, "lit round it: {row:?}");
    for w in row.windows(2) {
        assert!(w[1] <= w[0] + 2 && w[0] - w[1] <= 12, "no steps: {row:?}");
    }
    let dark = one("", r#"{"type":"glow","threshold":1,"knee":0}"#).unwrap();
    assert!(
        dark.iter().zip(&plain).all(|(a, b)| a.abs_diff(*b) <= 1),
        "a threshold of 1 blooms nothing"
    );

    // Neon: a halo outside, a white core.
    let neon = one(r#"{"type":"glow","mode":"neon","radius":16}"#, "").unwrap();
    assert!(px(&neon, 54, 45)[0] > 20, "{:?}", px(&neon, 54, 45));
    assert_eq!(px(&neon, 80, 45), [255; 4]);

    // A keyed radius grows smoothly past a pyramid level (16 px here).
    let at = |r: f64| {
        let o = one(
            &format!(r#"{{"type":"glow","mode":"outer","radius":{r}}}"#),
            "",
        )
        .unwrap();
        i32::from(px(&o, 52, 45)[0])
    };
    let (lo, hi) = (at(15.8), at(16.2));
    assert!((lo - hi).abs() <= 4, "{lo} then {hi}");
}

#[test]
fn effect_modes_and_backdrops_are_checked() {
    let load = |layer: &str, scene: &str| {
        Project::load(&format!(
            r#"{{"size":[64,64],"fps":30,"scenes":[{{"name":"a","duration":1,"layers":[{{"id":"r","kind":"rect","effects":[{layer}]}}],"effects":[{scene}]}}]}}"#
        ))
    };
    assert!(load(r#"{"type":"glow","mode":"neon"}"#, "").is_ok());
    assert!(load(r#"{"type":"glow","mode":"sparkle"}"#, "").is_err());
    assert!(load(r#"{"type":"blur","mode":"outer"}"#, "").is_err());
    let e = load("", r#"{"type":"light_wrap"}"#).unwrap_err();
    assert!(e.contains("goes on a layer"), "{e}");
    // Left out, the mode is the first; it survives a save.
    let p = load(r#"{"type":"glow"},{"type":"glow","mode":"inner"}"#, "").unwrap();
    let f = eval(&p, &p.scenes[0], 0.);
    let modes: Vec<_> = f.layers[0].effects.iter().map(|e| e.mode).collect();
    assert_eq!(modes, [Some("bloom"), Some("inner")]);
    assert!(p.to_json().contains(r#""mode": "inner""#));
}

/// Light wrap: a dark layer over a bright backdrop picks the light up at
/// its edges, not in its middle.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn light_wrap_lights_the_edges_from_behind() {
    let sq = |fx: &str| {
        frame_of(
            "#ffffff",
            &format!(
                r##"{{"id":"sq","kind":"rect","x":80,"y":45,"width":40,"height":40,"fill":"#000000","effects":[{fx}]}}"##
            ),
        )
    };
    let Some(out) = gpu_frames(&[
        vec![sq("")],
        vec![sq(r#"{"type":"light_wrap","radius":3}"#)],
    ]) else {
        return;
    };
    let (plain, wrap) = (&out[0], &out[1]);
    assert_eq!(px(plain, 61, 45), [0, 0, 0, 255]);
    let (edge, mid) = (px(wrap, 61, 45)[0], px(wrap, 80, 45)[0]);
    assert!(edge > 60, "edge {edge}");
    assert!(mid < 20 && edge > mid + 50, "middle {mid}");
    assert_eq!(px(wrap, 10, 10), [255; 4], "the backdrop is untouched");
}

/// Grain is deterministic: the same frame is the same pixels in two renders,
/// a new frame is new grain, and a motion-blurred frame's subframes share
/// it, so blur accumulates the effect instead of averaging it away.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn grain_is_seeded_by_frame_and_survives_motion_blur() {
    let grain = r#"{"type":"grain","amount":0.3}"#;
    let at = |t: f64| square("", grain, t).1;
    let Some(a) = gpu_frames(&[vec![at(0.5)], vec![at(0.5)], vec![at(0.6)]]) else {
        return;
    };
    assert_eq!(a[0], a[1], "two renders of one frame");
    assert_ne!(a[0], a[2], "the next frame has new grain");
    assert_ne!(a[0], {
        let (_, f) = square("", "", 0.5);
        gpu_frames(&[vec![f]]).unwrap().remove(0)
    });
    // Four subframes inside frame 15 (0.5 s): the same grain as no blur.
    let subs: Vec<Frame> = (0..4).map(|k| at(0.5 + f64::from(k) * 0.004)).collect();
    let blurred = gpu_frames(&[subs]).unwrap().remove(0);
    let off = blurred
        .iter()
        .zip(&a[0])
        .filter(|(x, y)| x.abs_diff(**y) > 2)
        .count();
    assert_eq!(off, 0, "{off} channels differ");

    // A moving square under a red tint: the blur smears red, per subframe.
    let moving = |t: f64| {
        let json = r##"{"size":[160,90],"fps":30,"scenes":[{"name":"a","duration":2,"background":"#000000",
            "layers":[{"id":"sq","kind":"rect","x":[{"t":0,"v":40,"interp":"linear"},{"t":1,"v":120}],"y":45,"width":20,"height":20,
            "effects":[{"type":"levels","tint":"#ff0000","tint_amount":1}]}]}]}"##;
        let p = Project::load(json).unwrap();
        eval(&p, &p.scenes[0], t)
    };
    let subs: Vec<Frame> = (0..8).map(|k| moving(0.5 + f64::from(k) * 0.05)).collect();
    let smear = gpu_frames(&[subs]).unwrap().remove(0);
    // The square sweeps x 70..118 over the shutter: a red smear, densest in
    // the middle, fading to its ends, nothing white.
    let row: Vec<[u8; 4]> = (60..128).map(|x| px(&smear, x, 45)).collect();
    assert!(row.iter().all(|p| p[1] < 5 && p[2] < 5), "only red");
    let (edge, mid) = (px(&smear, 72, 45)[0], px(&smear, 94, 45)[0]);
    assert!(
        mid > 180 && (30..mid - 40).contains(&edge),
        "edge {edge} mid {mid}"
    );
}

/// The GPU's 4:2:0 planes are the CPU reference's, from the same pixels,
/// within a step of rounding (the GPU converts before rounding to bytes).
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn gpu_yuv_matches_the_reference() {
    use crate::yuv::{Yuv, from_rgba};
    let p = Project::load(DEMO).unwrap();
    let s = p.scene("shapes").unwrap();
    // 162 wide: rows that are not a multiple of 4 bytes get padded and cut.
    let size = [162, 90];
    let subs: Vec<Frame> = (0..3)
        .map(|k| eval(&p, s, 0.6 + f64::from(k) * 0.02))
        .collect();
    let run = |yuv: Option<Yuv>| -> Option<Vec<u8>> {
        let mut g = match yuv.map_or_else(
            || Offline::new(size, Engine::Sparse),
            |y| Offline::with_yuv(size, Engine::Sparse, y),
        ) {
            Ok(g) => g,
            Err(e) => {
                eprintln!("skipped: no GPU ({e})");
                return None;
            }
        };
        let mut out: Vec<Vec<u8>> = g.push(&subs).unwrap().into_iter().collect();
        out.extend(g.finish().unwrap());
        out.pop()
    };
    let Some(rgba) = run(None) else { return };
    for yuv in [Yuv::Nv12, Yuv::P010] {
        let gpu = run(Some(yuv)).unwrap();
        let want = from_rgba(&rgba, size, yuv);
        assert_eq!(gpu.len(), yuv.frame_bytes(size));
        assert_eq!(gpu.len(), want.len());
        // Compare samples: bytes for NV12, 10-bit values for P010, where
        // one 8-bit step of the RGBA reference is four.
        let (got, exp, tol): (Vec<i32>, Vec<i32>, i32) = match yuv {
            Yuv::Nv12 => (
                gpu.iter().map(|&v| i32::from(v)).collect(),
                want.iter().map(|&v| i32::from(v)).collect(),
                1,
            ),
            Yuv::P010 => {
                let s = |b: &[u8]| {
                    b.chunks(2)
                        .map(|c| i32::from(u16::from_le_bytes([c[0], c[1]]) >> 6))
                        .collect()
                };
                (s(&gpu), s(&want), 5)
            }
        };
        let worst = got
            .iter()
            .zip(&exp)
            .map(|(a, b)| (a - b).abs())
            .max()
            .unwrap();
        assert!(worst <= tol, "{yuv:?}: off by {worst}");
    }
    // Known values: BT.709 limited range.
    let white = from_rgba(&[255; 16], [2, 2], Yuv::Nv12);
    assert_eq!(white, [235, 235, 235, 235, 128, 128]);
    let red = from_rgba(&[255, 0, 0, 255].repeat(4), [2, 2], Yuv::Nv12);
    assert_eq!(red, [63, 63, 63, 63, 102, 240]);
}

#[test]
fn render_settings_load_check_and_layer() {
    let p = Project::load(
        r#"{"size":[64,64],"fps":30,"scenes":[],"render":{"codec":"h265","crf":20,"mb":4}}"#,
    )
    .unwrap();
    let file = p.render.clone().unwrap();
    let flags = Render {
        crf: Some(12),
        container: Some("mkv".into()),
        ..Render::default()
    };
    let r = file.with(&flags);
    assert_eq!(
        (r.codec.as_deref(), r.crf, r.mb, r.container.as_deref()),
        (Some("h265"), Some(12), Some(4), Some("mkv"))
    );
    assert!(Project::load(&p.to_json()).is_ok(), "round trips");
    for bad in [
        r#"{"codec":"vp9"}"#,
        r#"{"pix_fmt":"rgb24"}"#,
        r#"{"container":"avi"}"#,
    ] {
        let json = format!(r#"{{"size":[64,64],"fps":30,"scenes":[],"render":{bad}}}"#);
        assert!(Project::load(&json).is_err(), "accepted {bad}");
    }
}

#[test]
fn ray_traced_glass_is_asked_for_by_name_with_a_sample_count() {
    let load = |render: &str| {
        Project::load(&format!(
            r#"{{"size":[64,64],"fps":30,"scenes":[],"render":{render}}}"#
        ))
    };
    let rt = |render: &str| load(render).unwrap().render.unwrap().rt_glass();
    assert_eq!(rt(r#"{"glass":"rt"}"#), Some(0));
    assert_eq!(rt(r#"{"glass":"rt","glass_samples":64}"#), Some(0));
    assert_eq!(rt(r#"{"glass":"rt-path"}"#), Some(16));
    assert_eq!(rt(r#"{"glass":"rt-path","glass_samples":64}"#), Some(64));
    assert_eq!(rt(r#"{"glass":"raster","glass_samples":64}"#), None);
    // Traced glass needs no ray queries: it is not `rt`.
    assert_eq!(rt(r#"{"glass":"trace"}"#), None);
    let trace = load(r#"{"glass":"trace"}"#).unwrap().render.unwrap();
    assert_eq!(trace.glass, Some(Glass::Trace));
    assert_eq!(rt("{}"), None);
    // A flag over the file: `--glass rt-path` turns it on, the file's count stays.
    let file = load(r#"{"glass_samples":8}"#).unwrap().render.unwrap();
    let flags = Render {
        glass: Some(Glass::RtPath),
        ..Render::default()
    };
    assert_eq!(file.with(&flags).rt_glass(), Some(8));
    for bad in [
        r#"{"glass":"cycles"}"#,
        r#"{"glass_samples":0}"#,
        r#"{"glass_samples":5000}"#,
    ] {
        assert!(load(bad).is_err(), "accepted {bad}");
    }
}

const VARIANTS: &str = include_str!("../examples/variants.cut.json");

fn drawn<'a>(f: &'a Frame, id: &str) -> &'a Drawn {
    f.layers.iter().find(|d| d.id == id).unwrap()
}

#[test]
fn variables_bind_and_variants_resolve() {
    let p = Project::load(VARIANTS).unwrap();
    // The defaults: dark, 1920x1080.
    let f = eval(&p, &p.scenes[0], 1.5);
    assert_eq!(f.size, [1920, 1080]);
    assert_eq!(f.background, Rgba::try_from("#0e0f14".to_owned()).unwrap());
    let card = drawn(&f, "card");
    assert_eq!(
        (card.x, card.y, card.width, card.radius),
        (960., 540., 1152., 32.)
    );
    assert!(matches!(&drawn(&f, "title").kind, Kind::Text { text, .. } if text == "Ship it"));
    assert_eq!(drawn(&f, "badge").opacity, 1.);
    assert_eq!(drawn(&f, "badge").x, 1780.);
    // A keyframe value binds too: W * -0.3 at t = 0.
    assert_eq!(drawn(&eval(&p, &p.scenes[0], 0.), "card").x, -576.);

    let v = p.variant("light-tall").unwrap();
    let f = eval(&v, &v.scenes[0], 1.5);
    assert_eq!(f.size, [1080, 1920]);
    assert_eq!(f.background, Rgba::try_from("#f4f1ea".to_owned()).unwrap());
    let card = drawn(&f, "card");
    assert_eq!((card.x, card.y, card.width), (540., 960., 648.));
    assert_eq!(card.fill, Rgba::try_from("#e4572e".to_owned()).unwrap());
    let title = drawn(&f, "title");
    // The override replaces the binding outright.
    assert_eq!(title.font_size, 96.);
    assert!(matches!(&title.kind, Kind::Text { text, .. } if text == "Ship it, tall"));
    assert_eq!(drawn(&f, "badge").opacity, 0.);
    assert!(p.variant("nope").is_err());

    // Saving keeps the bindings, the variants and the key order.
    assert_eq!(p.to_json(), VARIANTS);
    assert_eq!(v.to_json(), VARIANTS);
}

#[test]
fn variables_bind_duplicator_animator_and_deformer_params() {
    // A number binds anywhere a number goes, even deep in a stack.
    let json = r#"{"size": [64, 64], "fps": 30,
        "variables": {"n": {"type": "number", "value": 3}, "gap": {"type": "number", "value": 10}},
        "variants": [{"name": "big", "vars": {"n": 6, "gap": 20}}],
        "scenes": [{"name": "a", "duration": 1, "layers": [
            {"id": "row", "kind": "duplicator", "count": {"var": "n"},
             "spacing_x": {"var": "gap"}, "spacing_y": {"var": "gap", "mul": 2}},
            {"id": "word", "kind": "text", "text": "hi",
             "animators": [{"stagger": {"var": "gap", "mul": 0.01}, "x": {"var": "gap"}}],
             "deformers": [{"kind": "twist", "angle": {"var": "gap"}, "radius": {"var": "n", "mul": 10}}]}
        ]}]}"#;
    let check = |p: &Project, n: usize, gap: f64| {
        let f = eval(p, &p.scenes[0], 0.5);
        let row = drawn(&f, "row");
        assert_eq!((row.count, row.spacing), (n, [gap, gap * 2.]));
        let word = &p.scenes[0].layers[1];
        assert!((word.animators[0].stagger.at(0.) - gap * 0.01).abs() < 1e-12);
        assert_eq!(word.animators[0].x.at(0.), gap);
        assert_eq!(
            drawn(&f, "word").deformers,
            vec![Deform::Twist {
                angle: gap,
                radius: n as f64 * 10.
            }]
        );
    };
    let p = Project::load(json).unwrap();
    check(&p, 3, 10.);
    check(&p.variant("big").unwrap(), 6, 20.);
}

#[test]
fn variables_reject_what_does_not_fit() {
    let with = |vars: &str, variants: &str, bg: &str| {
        let json = format!(
            r#"{{"size": [64, 64], "fps": 30, "variables": {vars}, "variants": {variants},
                "scenes": [{{"name": "a", "duration": 1, "background": {bg}, "layers": []}}]}}"#
        );
        Project::load(&json)
    };
    let theme = r#"{"theme": {"type": "enum", "options": ["dark", "light"], "value": "dark"}}"#;
    let bg = r##"{"var": "theme", "map": {"dark": "#000000", "light": "#ffffff"}}"##;
    assert!(with(theme, "[]", bg).is_ok());
    // An enum value outside its options, in a variant.
    let e = with(theme, r#"[{"name": "x", "vars": {"theme": "blue"}}]"#, bg).unwrap_err();
    assert!(e.contains("not one of"), "{e}");
    // A variant setting an undeclared variable.
    assert!(with(theme, r#"[{"name": "x", "vars": {"nope": 1}}]"#, bg).is_err());
    // A binding to nothing, and a map missing a value.
    assert!(with(theme, "[]", r#"{"var": "nope"}"#).is_err());
    assert!(
        with(
            theme,
            "[]",
            r##"{"var": "theme", "map": {"light": "#ffffff"}}"##
        )
        .is_err()
    );
    // A colour variable holding no colour; an override that hits nothing.
    assert!(
        with(
            r#"{"c": {"type": "color", "value": 3}}"#,
            "[]",
            r##""#000000""##
        )
        .is_err()
    );
    assert!(
        with(
            theme,
            r#"[{"name": "x", "overrides": {"a/ghost": {"x": 1}}}]"#,
            bg
        )
        .is_err()
    );
    // A bound number that resolves to text fails the document's own check.
    let s = r#"{"s": {"type": "string", "value": "hi"}}"#;
    let json = format!(
        r#"{{"size": [64, 64], "fps": 30, "variables": {s},
            "scenes": [{{"name": "a", "duration": {{"var": "s"}}, "layers": []}}]}}"#
    );
    assert!(Project::load(&json).is_err());
}

const PLUGIN: &str = r#"{"id":"syn","kind":"plugin","source":{"bin":"adapter"},
    "x":200,"y":100,
    "params":[{"id":"filter","field":"cutoff","value":[{"t":0.5,"v":0.2,"interp":"linear"},{"t":1.0,"v":0.8,"interp":"hold"}]}],
    "pointer_x":[{"t":0,"v":-1,"interp":"hold"},{"t":1.5,"v":10,"interp":"hold"}],
    "pointer_y":[{"t":0,"v":-1,"interp":"hold"},{"t":1.5,"v":20,"interp":"hold"}]}"#;

#[test]
fn plugin_state_keys_follow_what_the_adapter_was_told() {
    let p = one_layer(PLUGIN);
    let l = &p.scenes[0].layers[0];
    let steps = l.plugin_track(p.fps, p.sample_rate, 60);
    // Frame 0 sets the parameter; it moves on frames 16..=30 (0.5 s to 1 s,
    // linear) and the pointer arrives on frame 45. Nothing else is a step.
    let frames: Vec<usize> = steps.iter().map(|s| s.frame).collect();
    assert_eq!(
        frames,
        [0].into_iter()
            .chain(16..=30)
            .chain([45])
            .collect::<Vec<_>>()
    );
    assert_eq!(
        steps[0].commands,
        [serde_json::json!({"op": "set", "id": "filter", "field": "cutoff", "value": 0.2})]
    );
    assert_eq!(steps.last().unwrap().commands[0]["kind"], "pointer");
    // A preset loads on frame 0, before the layer's own values.
    let preset = one_layer(&PLUGIN.replace("\"x\":200", "\"preset\":\"a.preset\",\"x\":200"));
    assert_eq!(
        preset.scenes[0].layers[0].plugin_track(30., 48_000, 0)[0].commands,
        [
            serde_json::json!({"op": "preset", "path": "a.preset"}),
            serde_json::json!({"op": "set", "id": "filter", "field": "cutoff", "value": 0.2}),
        ]
    );
    // Deterministic: the same document gives the same keys, all distinct.
    assert_eq!(
        steps,
        one_layer(PLUGIN).scenes[0].layers[0].plugin_track(30., 48_000, 60)
    );
    let keys: std::collections::HashSet<_> = steps.iter().map(|s| &s.key).collect();
    assert_eq!(keys.len(), steps.len());
    // A key hashes the source too: another adapter is another capture.
    let other = one_layer(&PLUGIN.replace("\"adapter\"", "\"other\""));
    assert_ne!(
        other.scenes[0].layers[0].plugin_track(30., 48_000, 0)[0].key,
        steps[0].key
    );
    // Coming back to a value is still a new state: the history differs.
    let back = one_layer(&PLUGIN.replace(
        r#"{"t":1.0,"v":0.8,"interp":"hold"}"#,
        r#"{"t":1.0,"v":0.8,"interp":"linear"},{"t":1.5,"v":0.2,"interp":"hold"}"#,
    ));
    let back = back.scenes[0].layers[0].plugin_track(30., 48_000, 60);
    assert_eq!(back[0].key, steps[0].key);
    assert_ne!(back.last().unwrap().key, steps[0].key);
}

#[test]
fn plugin_eval_is_pure_and_reads_the_frame_grid() {
    let p = one_layer(PLUGIN);
    let s = &p.scenes[0];
    let state = |t: f64| eval(&p, s, t).layers[0].plugin.clone().unwrap().state;
    let forward: Vec<String> = (0..60).map(|f| state(f as f64 / 30.)).collect();
    let backward: Vec<String> = (0..60).rev().map(|f| state(f as f64 / 30.)).collect();
    assert_eq!(forward, backward.into_iter().rev().collect::<Vec<_>>());
    // Between grid frames the state is the frame's; a static stretch shares one.
    assert_eq!(state(0.52), state(0.5));
    assert_eq!(state(0.1), state(0.4));
    assert_ne!(state(0.5), state(0.6));
    let steps = s.layers[0].plugin_track(30., 48_000, 59);
    assert_eq!(state(1.9), steps.last().unwrap().key);
    // Other kinds have no plugin state.
    let rect = one_layer(r#"{"id":"r","kind":"rect"}"#);
    assert!(eval(&rect, &rect.scenes[0], 0.).layers[0].plugin.is_none());
}

#[test]
fn explode_pulls_parts_away_from_the_centre() {
    assert_eq!(plugin::explode([100., 100.], [150., 80.], 0.), [0., 0., 0.]);
    assert_eq!(
        plugin::explode([100., 100.], [150., 80.], 0.5),
        [25., -10., 0.5 * plugin::EXPLODE_DEPTH]
    );
    // A part on the centre stays put in the plane.
    assert_eq!(plugin::explode([10., 10.], [10., 10.], 1.)[..2], [0., 0.]);
}

#[test]
fn plugin_layers_are_checked_and_list_their_part_tracks() {
    for bad in [
        r#"{"id":"p","kind":"plugin","source":{}}"#,
        r#"{"id":"p","kind":"plugin","source":{"bin":"a","example":"b"}}"#,
        r#"{"id":"p","kind":"plugin","source":{"cargo":"C.toml"}}"#,
        r#"{"id":"p","kind":"plugin","source":{"bin":"a"},"parts":{"a//b":{}}}"#,
    ] {
        let json = format!(
            r#"{{"size":[400,200],"fps":30,"scenes":[{{"name":"a","duration":2,"layers":[{bad}]}}]}}"#
        );
        assert!(Project::load(&json).is_err(), "accepted {bad}");
    }
    let p = one_layer(
        r#"{"id":"p","kind":"plugin","source":{"cargo":"C.toml","example":"s"},
            "params":[{"id":1,"field":"level","value":0.5}],
            "parts":{"osc":{"x":[{"t":0,"v":0},{"t":1,"v":50}],"highlight":1}}}"#,
    );
    let l = &p.scenes[0].layers[0];
    let names: Vec<String> = l.props().into_iter().map(|(n, _)| n).collect();
    for n in [
        "explode",
        "backdrop",
        "pointer_x",
        "params.0.value",
        "parts.osc.x",
        "parts.osc.highlight",
    ] {
        assert!(names.iter().any(|m| m == n), "{n} not in {names:?}");
    }
    assert!(!names.iter().any(|m| m == "stroke" || m == "width"));
    assert!((l.prop("parts.osc.x").unwrap().at(0.5) - 25.).abs() < 1e-6);
    // A save keeps it as written.
    assert_eq!(Project::load(&p.to_json()).unwrap(), p);
    // Surface ids may have dots (`tl.audio`); `/` still nests.
    let p = one_layer(
        r#"{"id":"p","kind":"plugin","source":{"bin":"a"},"parts":{"tl.audio/tl.audio.plot":{"z":-60}}}"#,
    );
    assert!(
        p.scenes[0].layers[0]
            .prop("parts.tl.audio/tl.audio.plot.z")
            .is_some()
    );
}

/// A capture of a 200x100 UI: a background and two parts, `a` left, `b` right.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn capture_assets(p: &Project) -> Assets {
    let key = eval(p, &p.scenes[0], 0.).layers[0]
        .plugin
        .clone()
        .unwrap()
        .state;
    let cap = serde_json::json!({"width": 200, "height": 100, "layers": [
        {"group": "background", "rect": [0, 0, 200, 100], "src": "img/bg.png"},
        {"group": "a", "rect": [20, 20, 40, 20], "src": "img/a.png"},
        {"group": "b", "rect": [140, 60, 40, 20], "src": "img/a.png"},
    ]});
    let mut a = Assets::default();
    a.add_asset(
        &format!("{}/{key}.json", plugin::CACHE),
        cap.to_string().as_bytes(),
    )
    .unwrap();
    for img in ["img/bg.png", "img/a.png"] {
        a.add_asset(&format!("{}/{img}", plugin::CACHE), &test_png())
            .unwrap();
    }
    a
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn captured_parts_become_movable_layers() {
    let doc = |extra: &str| {
        one_layer(&format!(
            r#"{{"id":"syn","kind":"plugin","source":{{"bin":"x"}},"x":200,"y":100{extra}}}"#
        ))
    };
    let at = |p: &Project, a: &Assets| a.layers(&eval(p, &p.scenes[0], 0.)).unwrap();
    let plain = doc("");
    let assets = capture_assets(&plain);
    let l = at(&plain, &assets);
    // The UI is centred on the layer: its quad, then one quad per part.
    assert_eq!(l.quads[0].pts[0], [100., 50.]);
    let ids: Vec<&str> = l.parts.iter().map(|q| q.id.as_str()).collect();
    assert_eq!(ids, ["syn#a", "syn#b"]);
    assert_eq!(l.parts[0].pts[0], [120., 70.]);
    assert_eq!(l.scenes.len(), 3, "background and two parts");
    // Exploded, `a` (centre 40,30 of a 200x100 UI centred on 100,50) moves
    // by its offset from the centre; its own track adds on top.
    let moved = doc(r#","explode":1,"parts":{"a":{"x":5,"highlight":1}}"#);
    let assets = capture_assets(&moved);
    let l = at(&moved, &assets);
    assert_eq!(l.parts[0].pts[0], [120. - 60. + 5., 70. - 20.]);
    assert_eq!(l.parts[1].pts[0], [240. + 60., 110. + 20.]);
    assert_eq!(l.scenes.len(), 4, "the highlight is one more tree");
    // No capture yet: the plugin draws a placeholder and no parts.
    let l = Assets::default()
        .layers(&eval(&plain, &plain.scenes[0], 0.))
        .unwrap();
    assert!(l.parts.is_empty());
    assert_eq!(l.scenes.len(), 1);
    // And the pixels: a part's red lands where its quad says.
    let mut r = Renderer::new(400, 200);
    r.assets = capture_assets(&plain);
    let (px, quads) = r.draw(&eval(&plain, &plain.scenes[0], 0.)).unwrap();
    assert!(quads.iter().any(|q| q.id == "syn#b"));
    let red = |x: usize, y: usize| px[(y * 400 + x) * 4] > 200 && px[(y * 400 + x) * 4 + 2] < 60;
    assert!(red(125, 80), "part a's left half is red");
    assert!(red(100, 50), "a fragment keeps its corners");
}

/// A part's `material` keys over its layer's, field by field, so a plugin
/// turns to glass part by part; and it lists, keys and fits the schema.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_part_keys_its_own_glass_over_its_layers_material() {
    let src = r#"{"size":[400,200],"fps":30,"scenes":[{"name":"a","duration":2,"mode":"3d",
        "layers":[{"id":"syn","kind":"plugin","source":{"bin":"x"},"x":200,"y":100,
        "material":{"roughness":0.1,"print":1,"bevel":6},
        "parts":{"a":{"material":{"transmission":[{"t":0,"v":0,"interp":"linear"},{"t":1,"v":1}]}}}}]}]}"#;
    let doc: serde_json::Value = serde_json::from_str(src).unwrap();
    let schema = Project::json_schema();
    let v = jsonschema::validator_for(&schema).unwrap();
    let errs: Vec<String> = v.iter_errors(&doc).map(|e| e.to_string()).collect();
    assert!(errs.is_empty(), "{errs:?}");
    let p = Project::load(src).unwrap();
    let l = &p.scenes[0].layers[0];
    assert!(
        l.props_in(true)
            .iter()
            .any(|(n, _)| n == "parts.a.material.transmission"),
        "a part's material is keyable by path"
    );
    let assets = capture_assets(&p);
    let at = |t: f64| {
        let d = &eval(&p, &p.scenes[0], t).layers[0];
        assets
            .slabs(d)
            .into_iter()
            .map(|s| (s.id.clone(), s.space.material))
            .collect::<Vec<_>>()
    };
    let half = at(0.5);
    let get = |id: &str| half.iter().find(|(i, _)| i == id).unwrap().1.unwrap();
    let (a, b) = (get("syn#a"), get("syn#b"));
    assert!((a.transmission.unwrap() - 0.5).abs() < 1e-9, "keyed: {a:?}");
    assert_eq!(
        (a.roughness, a.print, a.bevel),
        (Some(0.1), Some(1.), Some(6.)),
        "the layer's under it"
    );
    assert_eq!(
        (b.transmission, b.roughness),
        (None, Some(0.1)),
        "b keeps the layer's"
    );
    let m = a.over(mui_stage::Material::SLAB);
    assert_eq!((m.transmission, m.print, m.bevel), (0.5, 1., 6.));
}

/// In a 3D scene every part is its own slab: where the 2D drawing puts it
/// (turns and all), `explode` towards the viewer plus its own `z`.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn plugin_parts_are_slabs_at_their_depth_in_3d() {
    let doc = |extra: &str| {
        one_layer(&format!(
            r#"{{"id":"syn","kind":"plugin","source":{{"bin":"x"}},"x":200,"y":100,
                "z":30,"scale":2,"rotation":90{extra}}}"#
        ))
    };
    let centre = |q: &Quad| {
        let [a, _, c, _] = q.pts;
        [(a[0] + c[0]) / 2., (a[1] + c[1]) / 2.]
    };
    let p = doc(r#","explode":0.5,"parts":{"a":{"x":5,"z":40,"rotation":10,"highlight":1}}"#);
    let assets = capture_assets(&p);
    let d = &eval(&p, &p.scenes[0], 0.).layers[0];
    let flat = assets.layers(&eval(&p, &p.scenes[0], 0.)).unwrap();
    let slabs = assets.slabs(d);
    let ids: Vec<&str> = slabs.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(
        ids,
        ["syn", "syn#a", "syn#a", "syn#b"],
        "backdrop, a's plate, a, b"
    );
    let a = &slabs[2];
    assert!(a.plugin.is_none() && matches!(a.kind, Kind::Image { .. }));
    // Across the screen, exactly where the 2D drawing puts it.
    let want = centre(&flat.parts[0]);
    assert!(
        (a.x - want[0]).abs() < 1e-3 && (a.y - want[1]).abs() < 1e-3,
        "{:?} vs {want:?}",
        [a.x, a.y]
    );
    assert_eq!((a.scale, a.rotation), (2., 100.));
    // In depth: half of EXPLODE_DEPTH towards the viewer, 40 back (and the
    // pixel every part stands proud), in the layer's (doubled) pixels, from
    // the layer's own z.
    let z = 30. - (0.5 * plugin::EXPLODE_DEPTH - 40. + 1.) * 2.;
    assert!((a.space.z - z).abs() < 1e-3, "{} vs {z}", a.space.z);
    assert!(
        slabs[1].space.z > a.space.z,
        "the highlight plate is behind"
    );
    assert!(
        (slabs[0].space.z - 30.).abs() < 1e-3,
        "the backdrop stays put"
    );
    // Other layers pass through; a plugin with no capture is its placeholder.
    let rect = one_layer(r#"{"id":"r","kind":"rect"}"#);
    let r = &eval(&rect, &rect.scenes[0], 0.).layers[0];
    assert_eq!(assets.slabs(r), std::slice::from_ref(r));
    let none = Assets::default().slabs(d);
    assert_eq!(none.len(), 1);
    assert!(matches!(none[0].kind, Kind::Rect));
}

/// And mui-stage draws them: an unexploded plugin in a 3D scene looks like
/// its 2D self (it drew nothing before it was split into slabs).
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_plugin_in_a_3d_scene_draws_like_its_2d_self() {
    let mut p = one_layer(r#"{"id":"syn","kind":"plugin","source":{"bin":"x"},"x":200,"y":100}"#);
    let flat = eval(&p, &p.scenes[0], 0.);
    p.scenes[0].mode = Mode::ThreeD;
    let deep = eval(&p, &p.scenes[0], 0.);
    let mut g = match Offline::new(p.size, Engine::Classic) {
        Ok(g) => g,
        Err(e) => return eprintln!("skipped: no GPU ({e})"),
    };
    g.assets = capture_assets(&p);
    let mut shot = |f: &Frame| match g.push(std::slice::from_ref(f)).unwrap() {
        Some(px) => px,
        None => g.finish().unwrap().pop().unwrap(),
    };
    let (a, b) = (shot(&flat), shot(&deep));
    // Opaque red where the 2D frame has it (the 3D pass blends the test
    // image's translucent half in linear light, so that half differs).
    let red: fn(&[u8]) -> Vec<bool> = |px| {
        px.as_chunks::<4>()
            .0
            .iter()
            .map(|c| c[0] > 200 && c[2] < 60)
            .collect()
    };
    let (ra, rb) = (red(&a), red(&b));
    assert!(ra.iter().filter(|r| **r).count() > 5_000);
    let off = ra.iter().zip(&rb).filter(|(a, b)| a != b).count();
    assert!(off < ra.len() / 100, "{off} pixels differ");
}

/// A capture of a 200x100 UI as a two-level tree: panel `a` (its frame
/// 20,20 80x40) holding control `a/k` (30,30 20x20), and panel `b`. The
/// control has a free image, wider than its clipped one.
#[cfg(not(target_arch = "wasm32"))]
fn tree_assets(p: &Project) -> Assets {
    let key = eval(p, &p.scenes[0], 0.).layers[0]
        .plugin
        .clone()
        .unwrap()
        .state;
    let cap = serde_json::json!({"width": 200, "height": 100,
    "parts": [
        {"path": "a", "id": "a", "frame": [20, 20, 80, 40]},
        {"path": "b", "id": "b", "frame": [140, 60, 40, 20]},
        {"path": "a/k", "id": "k", "parent": "a", "frame": [30, 30, 20, 20]},
    ],
    "layers": [
        {"group": "background", "rect": [0, 0, 200, 100], "src": "img/bg.png"},
        {"group": "a", "rect": [20, 20, 80, 40], "src": "img/a.png"},
        {"group": "a/k", "rect": [30, 30, 20, 20], "src": "img/a.png",
         "free": {"src": "img/free.png", "rect": [26, 30, 28, 20]}},
        {"group": "b", "rect": [140, 60, 40, 20], "src": "img/a.png"},
    ]});
    let mut a = Assets::default();
    a.add_asset(
        &format!("{}/{key}.json", plugin::CACHE),
        cap.to_string().as_bytes(),
    )
    .unwrap();
    for img in ["img/bg.png", "img/a.png", "img/free.png"] {
        a.add_asset(&format!("{}/{img}", plugin::CACHE), &test_png())
            .unwrap();
    }
    a
}

#[cfg(not(target_arch = "wasm32"))]
fn tree_doc(extra: &str) -> Project {
    one_layer(&format!(
        r#"{{"id":"syn","kind":"plugin","source":{{"bin":"x"}},"x":200,"y":100{extra}}}"#
    ))
}

/// `explode_levels` limits how deep explode reaches: at 1 a control rides
/// with its panel, at 2 it also moves away from the panel's centre; its
/// own tracks, keyed by path, move it in its panel.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn explode_levels_separate_panels_then_their_controls() {
    let quads = |extra: &str| {
        let p = tree_doc(extra);
        let a = tree_assets(&p);
        let l = a.layers(&eval(&p, &p.scenes[0], 0.)).unwrap();
        let at = |id: &str| l.parts.iter().find(|q| q.id == id).unwrap().pts[0];
        (
            at("syn#a"),
            at("syn#a/k"),
            l.parts.iter().map(|q| q.id.clone()).collect::<Vec<_>>(),
        )
    };
    // Parents first, children after: a child wins a hit.
    let (a0, k0, ids) = quads("");
    assert_eq!(ids, ["syn#a", "syn#b", "syn#a/k"]);
    // The UI (200x100) is centred on 200,100: its origin is 100,50.
    assert_eq!((a0, k0), ([120., 70.], [130., 80.]));
    // One level: `a` (centre 60,40) moves by its offset from the UI's
    // centre (100,50), and `k` rides with it.
    let (a1, k1, _) = quads(r#","explode":1"#);
    assert_eq!(a1, [120. - 40., 70. - 10.]);
    assert_eq!([k1[0] - a1[0], k1[1] - a1[1]], [10., 10.]);
    // Two: `k` (centre 40,40) also moves off `a`'s centre (60,40).
    let (a2, k2, _) = quads(r#","explode":1,"explode_levels":2"#);
    assert_eq!(a2, a1);
    assert_eq!([k2[0] - a2[0], k2[1] - a2[1]], [10. - 20., 10.]);
    // A control's own track, by path, adds on top, in its panel.
    let (_, k3, _) = quads(r#","explode":1,"parts":{"a/k":{"y":7}}"#);
    assert_eq!(k3, [k1[0], k1[1] + 7.]);
    // Staggered: the second level runs behind the first.
    let p = tree_doc(
        r#","explode_levels":2,"explode_stagger":0.5,
        "explode":[{"t":0,"v":0,"interp":"linear"},{"t":1,"v":1,"interp":"linear"}]"#,
    );
    let e = eval(&p, &p.scenes[0], 0.75).layers[0]
        .plugin
        .clone()
        .unwrap()
        .explode;
    assert!(
        (e[0] - 0.75).abs() < 1e-9 && (e[1] - 0.25).abs() < 1e-9,
        "{e:?}"
    );
}

/// A control keyed or exploded deeper than the panels asks the adapter
/// for that many levels of parts; panels alone ask nothing new (so their
/// captures keep their keys).
#[test]
fn deeper_parts_ask_the_adapter_for_more_levels() {
    let first = |extra: &str| {
        let p = one_layer(&format!(
            r#"{{"id":"p","kind":"plugin","source":{{"bin":"a"}}{extra}}}"#
        ));
        p.scenes[0].layers[0].plugin_track(30., 48_000, 0)[0]
            .commands
            .clone()
    };
    assert!(first("").is_empty());
    assert!(first(r#","parts":{"osc":{"x":1}}"#).is_empty());
    let depth = |extra: &str| first(extra)[0]["depth"].clone();
    assert_eq!(depth(r#","explode_levels":2"#), 2);
    assert_eq!(depth(r#","parts":{"osc/osc-shape":{"x":1}}"#), 2);
    // Surface ids have slashes of their own: a deep path must not ask for
    // more levels than the adapter serves (1..8), or the capture fails.
    assert_eq!(
        depth(r#","parts":{"group-frame/0/osc/0/panel/wave/osc/0/wave":{"x":1}}"#),
        8
    );
    assert_eq!(
        first(r#","explode_levels":3,"select":["a"]"#)[0]["ids"],
        serde_json::json!(["a"])
    );
    for bad in [
        r#","explode_levels":0"#,
        r#","explode_levels":9"#,
        r#","explode_stagger":-1"#,
        r#","parts":{"a//b":{}}"#,
        r#","parts":{"/a":{}}"#,
    ] {
        let json = format!(
            r#"{{"size":[400,200],"fps":30,"scenes":[{{"name":"a","duration":2,"layers":[{{"id":"p","kind":"plugin","source":{{"bin":"a"}}{bad}}}]}}]}}"#
        );
        assert!(Project::load(&json).is_err(), "accepted {bad}");
    }
}

/// At rest a control draws its clipped image (the UI as it is); moved, its
/// free one, uncut by the panel it left.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_moved_control_draws_free_of_its_panels_clip() {
    let images = |extra: &str| {
        let p = tree_doc(extra);
        let a = tree_assets(&p);
        let d = &eval(&p, &p.scenes[0], 0.).layers[0];
        let slabs: Vec<String> = a
            .slabs(d)
            .into_iter()
            .filter(|s| s.id == "syn#a/k")
            .filter_map(|s| match s.kind {
                Kind::Image { path } => Some(path),
                _ => None,
            })
            .collect();
        let widths: Vec<f64> = a
            .slabs(d)
            .iter()
            .filter(|s| s.id == "syn#a/k")
            .map(|s| s.width)
            .collect();
        (slabs, widths)
    };
    let (rest, w) = images("");
    assert_eq!(rest, [format!("{}/img/a.png", plugin::CACHE)]);
    assert_eq!(w, [20.]);
    let (moved, w) = images(r#","explode":0.5,"explode_levels":2"#);
    assert_eq!(moved, [format!("{}/img/free.png", plugin::CACHE)]);
    assert_eq!(w, [28.]);
    // Its panel moving takes it out of the UI too.
    assert_eq!(images(r#","parts":{"a":{"x":3}}"#).0, moved);
}

/// In 3D each level stacks in front of the one above it.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn each_level_stacks_deeper_in_3d() {
    let p = tree_doc(r#","explode":0.5,"explode_levels":2"#);
    let a = tree_assets(&p);
    let slabs = a.slabs(&eval(&p, &p.scenes[0], 0.).layers[0]);
    let z = |id: &str| slabs.iter().find(|s| s.id == id).unwrap().space.z;
    // Larger z is farther: the backdrop, then the panel, then its control.
    let step = 0.5 * plugin::EXPLODE_DEPTH + 1.;
    assert!((z("syn") - 0.).abs() < 1e-6);
    assert!((z("syn#a") + step).abs() < 1e-6, "{}", z("syn#a"));
    assert!((z("syn#a/k") + 2. * step).abs() < 1e-6, "{}", z("syn#a/k"));
}

/// Notes land on their own samples at the project's rate: an end before a
/// start on the same sample, a length never below a sample, and each
/// frame's advance carries exactly the notes in its span.
#[test]
fn notes_are_scheduled_to_sample_offsets() {
    let notes = [
        Note {
            t: 0.5,
            dur: 0.25,
            pitch: 60,
            vel: 100,
        },
        Note {
            t: 0.75,
            dur: 1e-9,
            pitch: 60,
            vel: 90,
        },
    ];
    let ev = plugin::note_events(&notes, 44_100);
    let at: Vec<(u64, &str)> = ev
        .iter()
        .map(|(s, v)| (*s, v["op"].as_str().unwrap()))
        .collect();
    assert_eq!(
        at,
        [
            (22_050, "note_on"),
            (33_075, "note_off"),
            (33_075, "note_on"),
            (33_076, "note_off")
        ]
    );
    assert_eq!(ev[0].1["at"], 22_050);
    let p = one_layer(
        r#"{"id":"syn","kind":"plugin","source":{"bin":"adapter"},"notes":[{"t":0.5,"dur":0.25,"pitch":60}]}"#,
    );
    let steps = p.scenes[0].layers[0].plugin_track(30., 48_000, 30);
    // Every frame after the first advances to its own sample.
    assert_eq!(steps.len(), 31);
    assert_eq!(
        steps[15].commands[0],
        serde_json::json!({"op": "advance", "to": 24_000, "notes": []})
    );
    // The note at 0.5 s is sample 24000: in frame 16's span [24000, 25600).
    assert_eq!(steps[16].commands[0]["to"], 25_600);
    assert_eq!(steps[16].commands[0]["notes"][0]["at"], 24_000);
}

/// A capture state, and so a render segment, names the notes played: other
/// notes are other states and other frames.
#[test]
fn segment_keys_change_with_the_notes() {
    let with = |pitch: u8| {
        one_layer(&format!(
            r#"{{"id":"syn","kind":"plugin","source":{{"bin":"adapter"}},"notes":[{{"t":0.2,"dur":0.5,"pitch":{pitch}}}]}}"#
        ))
    };
    let (a, b) = (with(60), with(62));
    let frame = |p: &Project| serde_json::to_string(&eval(p, &p.scenes[0], 1.)).unwrap();
    assert_ne!(frame(&a), frame(&b));
    // Before the notes differ in anything played, the state is shared.
    let state = |p: &Project, t| {
        eval(p, &p.scenes[0], t).layers[0]
            .plugin
            .clone()
            .unwrap()
            .state
    };
    assert_eq!(state(&a, 0.1), state(&b, 0.1));
    assert_ne!(state(&a, 0.3), state(&b, 0.3));
}

/// A keyed view size is sent as `view` input when it changes, in whole
/// pixels; 0 is the plugin's own size and sends nothing at the start.
#[test]
fn a_keyed_view_size_is_sent_when_it_changes() {
    let p = one_layer(
        r#"{"id":"syn","kind":"plugin","source":{"bin":"adapter"},
        "view_width":[{"t":0,"v":0,"interp":"hold"},{"t":1,"v":900.4,"interp":"hold"}],
        "view_height":600}"#,
    );
    let steps = p.scenes[0].layers[0].plugin_track(30., 48_000, 45);
    assert_eq!(steps.iter().map(|s| s.frame).collect::<Vec<_>>(), [0, 30]);
    assert!(steps[0].commands.is_empty());
    assert_eq!(
        steps[1].commands,
        [serde_json::json!({"op": "input", "kind": "view", "width": 900.0, "height": 600.0})]
    );
}

/// Glass `texture`: its patterns key, list by path, fit the schema, merge
/// a part's over its layer's, and reach the stage.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_glass_texture_keys_and_reaches_the_stage() {
    let src = r#"{"size":[400,200],"fps":30,"scenes":[{"name":"a","duration":2,"mode":"3d",
        "layers":[{"id":"syn","kind":"plugin","source":{"bin":"x"},"x":200,"y":100,
        "material":{"transmission":1,"texture":{"ribbed":{"strength":[{"t":0,"v":0,"interp":"linear"},{"t":1,"v":1}],"scale":30}}},
        "parts":{"a":{"material":{"texture":{"hammered":{"strength":0.5}}}}}}]}]}"#;
    let doc: serde_json::Value = serde_json::from_str(src).unwrap();
    let schema = Project::json_schema();
    let v = jsonschema::validator_for(&schema).unwrap();
    let errs: Vec<String> = v.iter_errors(&doc).map(|e| e.to_string()).collect();
    assert!(errs.is_empty(), "{errs:?}");
    let p = Project::load(src).unwrap();
    let l = &p.scenes[0].layers[0];
    let props: Vec<String> = l.props_in(true).into_iter().map(|(n, _)| n).collect();
    for path in [
        "material.texture.ribbed.strength",
        "material.texture.ribbed.scale",
        "parts.a.material.texture.hammered.strength",
    ] {
        assert!(props.iter().any(|n| n == path), "{path} in {props:?}");
    }
    let m = l.material.as_ref().unwrap().at(0.25);
    assert_eq!(m.ribbed, Some([0.25, 30.]), "keyed");
    let assets = capture_assets(&p);
    let d = &eval(&p, &p.scenes[0], 0.5).layers[0];
    let slabs = assets.slabs(d);
    let get = |id: &str| {
        let s = slabs.iter().find(|s| s.id == id).unwrap();
        s.space.material.unwrap().over(mui_stage::Material::SLAB)
    };
    let (a, b) = (get("syn#a"), get("syn#b"));
    assert_eq!(
        (
            a.ribbed.strength,
            a.ribbed.scale,
            a.hammered.strength,
            a.hammered.scale
        ),
        (0.5, 30., 0.5, 24.),
        "a's dimples over the layer's reeds, at the default size"
    );
    assert_eq!(
        (b.ribbed.strength, b.hammered.strength),
        (0.5, 0.),
        "b has the layer's"
    );
}

/// On the GPU a comp is one layer: its effects run over its scene's
/// layers together (the per-layer offscreen pass), and its scene's
/// background stays out.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn comp_effects_run_over_the_whole_scene() {
    let p = Project::load(
        r##"{"size":[160,90],"fps":30,"scenes":[
            {"name":"a","duration":1,"background":"#000000","layers":[
              {"id":"c","kind":"comp","scene":"b","x":80,"y":45,
               "effects":[{"type":"levels","tint":"#ff0000","tint_amount":1}]}]},
            {"name":"b","duration":1,"background":"#00ff00","layers":[
              {"id":"l","kind":"rect","x":70,"y":45,"width":20,"height":20},
              {"id":"r","kind":"rect","x":90,"y":45,"width":20,"height":20}]}]}"##,
    )
    .unwrap();
    let f = eval(&p, &p.scenes[0], 0.);
    let Some(img) = gpu_frames(&[vec![f]]).map(|mut v| v.remove(0)) else {
        return;
    };
    for x in [70, 90] {
        assert!(
            near(px(&img, x, 45), [255, 0, 0, 255], 2),
            "{:?}",
            px(&img, x, 45)
        );
    }
    assert_eq!(px(&img, 10, 10), [0, 0, 0, 255]);
}
