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

/// The GPU path (ring readback, float shutter) matches the CPU path, frame
/// for frame and in order. Skips on a machine with no GPU adapter.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn gpu_frames_match_the_cpu_in_order() {
    let p = Project::load(DEMO).unwrap();
    let s = p.scene("shapes").unwrap();
    let mut gpu = match Offline::new([320, 180]) {
        Ok(g) => g,
        Err(e) => return eprintln!("skipped: no GPU ({e})"),
    };
    let mut cpu = Renderer::new(320, 180);
    // Five frames through a ring of three, four subframes each, mid-motion.
    let frames: Vec<Vec<Frame>> = (0..5)
        .map(|i| {
            (0..4)
                .map(|k| eval(&p, s, 0.5 + f64::from(i) * 0.1 + f64::from(k) * 0.02))
                .collect()
        })
        .collect();
    let mut got = Vec::new();
    for subs in &frames {
        got.extend(gpu.push(subs).unwrap());
    }
    got.extend(gpu.finish().unwrap());
    assert_eq!(got.len(), frames.len());
    for (subs, g) in frames.iter().zip(&got) {
        let mut acc = vec![0.; 320 * 180 * 4];
        for f in subs {
            mui_reel::accumulate(&mut acc, &cpu.draw(f).unwrap().0);
        }
        let c = mui_reel::resolve(&acc, subs.len());
        let off = g
            .iter()
            .zip(&c)
            .filter(|(a, b)| a.abs_diff(**b) > 8)
            .count();
        // Antialiasing differs a little at edges; the frames must not.
        assert!(off < c.len() / 500, "{off} channels differ");
    }
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
