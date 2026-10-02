//! Procedural motion: instancing, effectors, jitter, group stagger and
//! behaviours.
use super::*;

fn scene_of(layers: &str) -> Project {
    Project::load(&format!(
        r##"{{"size":[400,200],"fps":30,"scenes":[{{"name":"a","duration":2,"background":"#000000","layers":[{layers}]}}]}}"##
    ))
    .unwrap()
}

fn px(p: &Project, t: f64) -> Vec<u8> {
    let mut r = Renderer::new(400, 200);
    r.draw(&eval(p, &p.scenes[0], t)).unwrap().0
}

fn at(px: &[u8], x: usize, y: usize) -> [u8; 4] {
    px[(y * 400 + x) * 4..][..4].try_into().unwrap()
}

#[test]
fn a_duplicator_instances_a_group_with_its_children_and_hides_it() {
    // A group of two squares (a red one at its pivot, a green one 20 px
    // right) instanced three times in a row 100 px apart round (200, 100).
    let layers = |extra: &str| {
        format!(
            r##"{{"id":"g","kind":"group","x":30,"y":30,"rotation":45}},
            {{"id":"a","kind":"rect","parent":"g","width":10,"height":10,"fill":"#ff0000"}},
            {{"id":"b","kind":"rect","parent":"g","x":20,"width":10,"height":10,"fill":"#00ff00"}},
            {{"id":"d","kind":"duplicator","source":"g","layout":"linear","count":3,"spacing_x":100,"x":200,"y":100{extra}}}"##
        )
    };
    let p = scene_of(&layers(""));
    let f = eval(&p, &p.scenes[0], 0.);
    let d = f.layers.iter().find(|l| l.id == "d").unwrap();
    // The source's own place (and its turn) is each copy's: 3 copies x 2 rects.
    let ids: Vec<&str> = d.comp.iter().map(|k| k.id.as_str()).collect();
    assert_eq!(ids, ["d/0/a", "d/0/b", "d/1/a", "d/1/b", "d/2/a", "d/2/b"]);
    let b1 = &d.comp[3];
    assert!((b1.x - 220.).abs() < 1e-9 && (b1.y - 100.).abs() < 1e-9 && b1.rotation.abs() < 1e-9);
    let px0 = px(&p, 0.);
    for x in [100, 200, 300] {
        assert_eq!(at(&px0, x, 100), [255, 0, 0, 255], "a red square at {x}");
        assert_eq!(
            at(&px0, x + 20, 100),
            [0, 255, 0, 255],
            "a green square right of {x}"
        );
    }
    // The source itself is hidden (and not "never visible" in check).
    assert_eq!(at(&px0, 30, 30), [0, 0, 0, 255]);
    let doc = format!(
        r#"{{"size":[400,200],"fps":30,"scenes":[{{"name":"a","duration":2,"layers":[{}]}}]}}"#,
        layers("")
    );
    let issues = check::check(&doc, &mut Renderer::new(400, 200), &|_| true);
    assert!(
        !issues.iter().any(|i| i.code == "never_visible"),
        "{issues:?}"
    );
    // `show_source` keeps it.
    let p = scene_of(&layers(r#","show_source":true"#));
    assert_ne!(at(&px(&p, 0.), 30, 30), [0, 0, 0, 255]);
    // A duplicator cannot instance a layer it sits under.
    let e = Project::load(
        r#"{"size":[400,200],"fps":30,"scenes":[{"name":"a","duration":2,"layers":[
            {"id":"g","kind":"group"},
            {"id":"d","kind":"duplicator","parent":"g","source":"g"}]}]}"#,
    )
    .unwrap_err();
    assert!(e.contains("cannot instance itself"), "{e}");
    let e = Project::load(
        r#"{"size":[400,200],"fps":30,"scenes":[{"name":"a","duration":2,"layers":[
            {"id":"d","kind":"duplicator","source":"nope"}]}]}"#,
    )
    .unwrap_err();
    assert!(e.contains("no layer `nope`"), "{e}");
}

#[test]
fn instanced_copies_take_their_animators_and_nest() {
    // The second half of the copies moves down 50 px: the animator reads
    // each copy's index, as with plain copies.
    let p = scene_of(
        r##"{"id":"s","kind":"ellipse","width":10,"height":10,"fill":"#ffffff"},
            {"id":"d","kind":"duplicator","source":"s","layout":"linear","count":4,"spacing_x":50,"x":200,"y":50,
             "animators":[{"start":0.5,"y":50}]},
            {"id":"e","kind":"duplicator","source":"d","layout":"linear","count":2,"spacing_x":0,"spacing_y":0,"x":0,"y":0}"##,
    );
    let f = eval(&p, &p.scenes[0], 0.);
    let e = f.layers.iter().find(|l| l.id == "e").unwrap();
    // `e` copies `d` (with its four instances) twice; `d` itself is hidden.
    assert_eq!(e.comp.len(), 2);
    assert_eq!(e.comp[0].comp.len(), 4);
    let ys: Vec<f64> = e.comp[0].comp.iter().map(|k| k.y.round()).collect();
    assert_eq!(ys, [0., 0., 50., 50.]);
    assert!(
        f.layers
            .iter()
            .find(|l| l.id == "d")
            .unwrap()
            .comp
            .is_empty()
    );
}

#[test]
fn jitter_is_seeded_bounded_and_recolours_instances() {
    let row = |seed: u32| {
        scene_of(&format!(
            r##"{{"id":"d","kind":"duplicator","layout":"linear","count":40,"spacing_x":0,"x":200,"y":100,"fill":"#ff0000",
                "animators":[{{"seed":{seed},"jitter_x":10,"jitter_rotation":30,"jitter_scale":0.5,"jitter_opacity":1,"jitter_hue":120}}]}}"##
        ))
    };
    let fx = |p: &Project| p.scenes[0].layers[0].at(0.).fx;
    let (a, b) = (fx(&row(1)), fx(&row(2)));
    // The same every time, another shuffle with another seed.
    assert_eq!(a, fx(&row(1)));
    assert_ne!(a, b);
    for f in &a {
        assert!(f.x.abs() <= 10. && f.rotation.abs() <= 30.);
        assert!((0.5..=1.5).contains(&f.scale) && (0. ..=1.).contains(&f.opacity));
        assert_eq!(f.y, 0.);
    }
    // Spread both ways, and hues round red both ways (red stays the max
    // or turns towards green or blue).
    assert!(a.iter().any(|f| f.x < -5.) && a.iter().any(|f| f.x > 5.));
    assert!(a.iter().any(|f| f.fill.0[1] > 100) && a.iter().any(|f| f.fill.0[2] > 100));
    // An instanced copy is recoloured from its own fill.
    let p = scene_of(
        r##"{"id":"s","kind":"rect","width":20,"height":20,"fill":"#00ff00"},
            {"id":"d","kind":"duplicator","source":"s","count":1,"x":200,"y":100,
             "animators":[{"jitter_hue":120,"seed":3}]}"##,
    );
    let d = &eval(&p, &p.scenes[0], 0.).layers[1];
    let f = d.comp[0].fill.0;
    assert_ne!(f, [0, 255, 0, 255]);
    // Turned, not faded: still fully saturated and opaque.
    assert_eq!(
        (f[..3].iter().max(), f[..3].iter().min(), f[3]),
        (Some(&255), Some(&0), 255)
    );
    assert_eq!(at(&px(&p, 0.), 200, 100), f);
}

#[test]
fn an_effector_weighs_copies_by_place_and_sweeps() {
    // A row of 9 copies 40 px apart (-160..160); a sphere effector of
    // radius 50 (hard edge) sweeping from x -160 to 160 over 2 s lifts
    // the copies it covers by 100 px.
    let p = scene_of(
        r##"{"id":"d","kind":"duplicator","layout":"linear","count":9,"spacing_x":40,"x":200,"y":150,"width":20,"height":20,
            "animators":[{"y":-100,"falloff":{"x":[{"t":0,"v":-160,"interp":"linear"},{"t":2,"v":160}],"radius":50,"softness":0}}]}"##,
    );
    let ys = |t: f64| -> Vec<f64> {
        p.scenes[0].layers[0]
            .at(t)
            .fx
            .iter()
            .map(|f| f.y.round())
            .collect()
    };
    assert_eq!(ys(0.), [-100., -100., 0., 0., 0., 0., 0., 0., 0.]);
    assert_eq!(ys(1.), [0., 0., 0., -100., -100., -100., 0., 0., 0.]);
    // Drawn: at 1 s the middle copy is lifted, the end ones are not.
    let px1 = px(&p, 1.);
    assert_eq!(at(&px1, 200, 50), [255; 4]);
    assert_eq!(at(&px1, 200, 150), [0, 0, 0, 255]);
    assert_eq!(at(&px1, 40, 150), [255; 4]);
    // Soft edges fade; invert swaps; a box and a wall; and the range
    // selector still applies (only the first half here).
    let fx = |falloff: &str, range: &str| -> Vec<f64> {
        let p = scene_of(&format!(
            r##"{{"id":"d","kind":"duplicator","layout":"linear","count":9,"spacing_x":40,
                "animators":[{{"y":-100{range},"falloff":{falloff}}}]}}"##
        ));
        p.scenes[0].layers[0]
            .at(0.)
            .fx
            .iter()
            .map(|f| -f.y.round())
            .collect()
    };
    let soft = fx(r#"{"radius":40,"softness":80}"#, "");
    assert_eq!(soft[4], 100.);
    assert!(soft[2] > 0. && soft[2] < 100. && soft[1] == 0.);
    assert_eq!(
        fx(r#"{"radius":50,"softness":0,"invert":true}"#, ""),
        [100., 100., 100., 0., 0., 0., 100., 100., 100.]
    );
    assert_eq!(
        fx(r#"{"shape":"linear","radius":0,"softness":0}"#, ""),
        [100., 100., 100., 100., 100., 0., 0., 0., 0.]
    );
    assert_eq!(
        fx(
            r#"{"shape":"box","radius":40,"softness":0}"#,
            r#","end":0.5"#
        ),
        [0., 0., 0., 100., 50., 0., 0., 0., 0.]
    );
}

#[test]
fn distance_order_ripples_and_stagger_ease_spreads_delays() {
    // A 3x3 grid, 1 s stagger, ranked by distance from the middle: the
    // centre goes first, the four edges together, then the four corners.
    let delays = |extra: &str| -> Vec<f64> {
        let p = scene_of(&format!(
            r##"{{"id":"d","kind":"duplicator","count":9,"columns":3,"spacing_x":10,"spacing_y":10,
                "animators":[{{"stagger":1{extra},"y":[{{"t":0,"v":0,"interp":"linear"}},{{"t":100,"v":100}}]}}]}}"##
        ));
        // y = t - delay at t = 50: each copy's delay.
        let fx = p.scenes[0].layers[0].at(50.).fx;
        fx.iter()
            .map(|f| ((50. - f.y) * 1e6).round() / 1e6)
            .collect()
    };
    let d = 8. / 2f64.sqrt(); // an edge: distance 10 of the corners' 10 sqrt 2, times 8
    let d = (d * 1e6).round() / 1e6;
    assert_eq!(
        delays(r#","order":"distance""#),
        [8., d, 8., d, 0., d, 8., d, 8.]
    );
    // From an effector's centre instead: the top-left copy first.
    let tl = delays(r#","order":"distance","falloff":{"x":-10,"y":-10,"radius":1000}"#);
    assert_eq!((tl[0], tl[8]), (0., 8.));
    // Eased: the last rank still waits 8 s, the middle ones less (`in`).
    let lin = delays("");
    let eased = delays(r#","stagger_ease":"in""#);
    assert_eq!((eased[0], eased[8]), (0., 8.));
    assert_eq!(lin[4], 4.);
    assert_eq!(eased[4], 2.);
}

#[test]
fn a_group_staggers_its_children_one_after_another() {
    // Three squares under a group with a cascade: each child rises in on
    // its own clock, 0.5 s behind the one before.
    let p = scene_of(
        r##"{"id":"g","kind":"group","x":200,"y":100,
             "animators":[{"stagger":0.5,"y":60,"opacity":0,
                           "amount":[{"t":0,"v":1,"interp":"linear"},{"t":0.5,"v":0}]}]},
            {"id":"a","kind":"rect","parent":"g","x":-100,"width":20,"height":20},
            {"id":"b","kind":"rect","parent":"g","width":20,"height":20},
            {"id":"c","kind":"rect","parent":"g","x":100,"width":20,"height":20}"##,
    );
    let f = eval(&p, &p.scenes[0], 0.5);
    let y = |id: &str| f.layers.iter().find(|l| l.id == id).unwrap().y;
    let o = |id: &str| f.layers.iter().find(|l| l.id == id).unwrap().opacity;
    // At 0.5 s: `a` has landed, `b` just starts, `c` waits.
    assert_eq!((y("a"), o("a")), (100., 1.));
    assert_eq!((y("b"), o("b")), (160., 0.));
    assert_eq!((y("c"), o("c")), (160., 0.));
    let f = eval(&p, &p.scenes[0], 0.75);
    let y = |id: &str| f.layers.iter().find(|l| l.id == id).unwrap().y;
    assert!(y("b") > 100. && y("b") < 160. && y("c") == 160.);
    // Drawn: at 0.5 s only `a` shows.
    let px = px(&p, 0.5);
    assert_eq!(at(&px, 100, 100), [255; 4]);
    assert_eq!(at(&px, 200, 160), [0, 0, 0, 255]);
    // The group's animator props are listed, so they key in the editor.
    let props: Vec<String> = p.scenes[0].layers[0]
        .props()
        .into_iter()
        .map(|(n, _)| n)
        .collect();
    assert!(
        props.contains(&"animators.0.stagger".to_owned()),
        "{props:?}"
    );
}

#[test]
fn the_group_presets_build_and_round_trip() {
    for name in ["cascade_children", "ripple"] {
        let a = Animator::preset(name, 0.2, 0.6).unwrap();
        let back: Animator = serde_json::from_value(serde_json::to_value(&a).unwrap()).unwrap();
        assert_eq!(a, back);
    }
    assert_eq!(
        Animator::preset("ripple", 0., 1.).unwrap().order,
        Order::Distance
    );
}

#[test]
fn behaviours_wiggle_and_oscillate_on_top_of_keys() {
    let p = scene_of(
        r##"{"id":"r","kind":"rect","width":20,"height":20,
             "x":[{"t":0,"v":100,"interp":"linear"},{"t":2,"v":300}],"y":100,
             "behaviours":[{"prop":"y","kind":"oscillate","amount":50,"freq":1,"phase":0.25},
                           {"prop":"rotation","amount":30,"freq":4,"seed":7},
                           {"prop":"opacity","kind":"oscillate","amount":2}]}"##,
    );
    let l = &p.scenes[0].layers[0];
    // Oscillate: a sine from `phase`, on top of y's value; x keeps its keys.
    let d = l.at(0.);
    assert!((d.y - 150.).abs() < 1e-9 && d.x == 100.);
    assert!((l.at(0.5).y - 50.).abs() < 1e-9);
    assert_eq!(l.at(1.).x, 200.);
    // Opacity stays in 0..1.
    assert!((0..60).all(|i| (0. ..=1.).contains(&l.at(f64::from(i) / 30.).opacity)));
    // Wiggle: bounded, smooth frame to frame, not still, the same every run.
    let rot: Vec<f64> = (0..120)
        .map(|i| l.at(f64::from(i) / 60.).rotation)
        .collect();
    assert!(rot.iter().all(|r| r.abs() <= 30.));
    assert!(rot.windows(2).all(|w| (w[1] - w[0]).abs() < 6.), "{rot:?}");
    assert!(rot.iter().any(|r| r.abs() > 5.));
    assert_eq!(
        rot,
        (0..120)
            .map(|i| l.at(f64::from(i) / 60.).rotation)
            .collect::<Vec<_>>()
    );
    // Another property with the same seed wiggles another way.
    let other = scene_of(
        r#"{"id":"r","kind":"rect","behaviours":[{"prop":"x","amount":30,"freq":4,"seed":7}]}"#,
    );
    let x: Vec<f64> = (0..120)
        .map(|i| other.scenes[0].layers[0].at(f64::from(i) / 60.).x)
        .collect();
    assert_ne!(x, rot);
    // Drawn where the behaviour puts it: at 0.5 s y is 50.
    assert_eq!(at(&px(&p, 0.5), 200, 50)[3], 255);
    // A property it cannot move is refused; its numbers are keyable props.
    let e = Project::load(
        r#"{"size":[400,200],"fps":30,"scenes":[{"name":"a","duration":2,"layers":[
            {"id":"r","kind":"rect","behaviours":[{"prop":"fill"}]}]}]}"#,
    )
    .unwrap_err();
    assert!(
        e.contains("behaviours[0].prop") && e.contains("`fill` is not one of"),
        "{e}"
    );
    assert!(l.prop("behaviours.1.amount").is_some());
}

#[test]
fn the_procedural_example_round_trips_and_checks_clean() {
    let src = include_str!("../examples/procedural.cut.json");
    let p = Project::load(src).unwrap();
    assert_eq!(p.to_json(), src);
    let issues = check::check(src, &mut Renderer::new(320, 180), &|_| true);
    assert!(issues.is_empty(), "{issues:?}");
    // The array is drawn as 75 copies of the tile's two layers.
    let f = eval(&p, &p.scenes[0], 2.);
    assert_eq!(
        f.layers
            .iter()
            .find(|l| l.id == "array")
            .unwrap()
            .comp
            .len(),
        150
    );
}
