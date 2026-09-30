use super::*;

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
