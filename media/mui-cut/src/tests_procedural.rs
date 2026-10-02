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
