//! Sources, component layers, parenting, reset and taking 3D flat.
use super::*;
use crate::place::{flatten, reparent};
use crate::tests::capture_assets;

fn scene(mode: &str, layers: &str) -> Project {
    Project::load(&format!(
        r#"{{"size":[400,200],"fps":30,"scenes":[{{"name":"a","duration":2,"mode":"{mode}","layers":[{layers}]}}]}}"#
    ))
    .unwrap()
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

fn world(p: &Project, id: &str, t: f64) -> Drawn {
    eval(p, &p.scenes[0], t)
        .layers
        .into_iter()
        .find(|l| l.id == id)
        .unwrap()
}

/// A child's transform goes through its parent's (and its parent's):
/// position turned and scaled, rotation and scale added up, depth scaled,
/// opacity multiplied, whatever order the layers paint in.
#[test]
fn children_inherit_their_parents_transform() {
    let p = scene(
        "3d",
        r#"{"id":"kid","kind":"rect","parent":"mid","x":10,"rotation":5,"scale":1.5,"opacity":0.5,"z":3},
           {"id":"mid","kind":"rect","parent":"top","x":0,"y":0},
           {"id":"top","kind":"rect","x":100,"y":50,"rotation":90,"scale":2,"opacity":0.5,"z":10}"#,
    );
    let k = world(&p, "kid", 0.);
    assert!(near(k.x, 100.) && near(k.y, 70.), "{} {}", k.x, k.y);
    assert!(near(k.rotation, 95.) && near(k.scale, 3.) && near(k.opacity, 0.25));
    assert!(near(k.space.z, 16.), "{}", k.space.z);
    // The layer's own tracks stay local: an unparented copy is untouched.
    let alone = scene("2d", r#"{"id":"kid","kind":"rect","x":10}"#);
    assert!(near(world(&alone, "kid", 0.).x, 10.));
}

/// Parenting (and unparenting) keeps a layer where it is on screen, the
/// way Cavalry and After Effects do, by rewriting its local keys.
#[test]
fn reparenting_keeps_the_world_position() {
    let p = scene(
        "2d",
        r#"{"id":"top","kind":"rect","x":300,"y":120,"rotation":30,"scale":1.5},
           {"id":"kid","kind":"rect",
            "x":[{"t":0,"v":50},{"t":1,"v":150}],
            "y":[{"t":0,"v":40},{"t":1,"v":90}],
            "rotation":10,"scale":[{"t":0,"v":1},{"t":1,"v":2}]}"#,
    );
    let before: Vec<Drawn> = [0., 0.5, 1.].map(|t| world(&p, "kid", t)).into();
    let kid = reparent(&p, &p.scenes[0], "kid", Some("top"), 0.5).unwrap();
    assert_eq!(kid.parent, "top");
    assert!(!near(kid.x.at(0.), 50.), "the local track was rewritten");
    let mut q = p.clone();
    q.scenes[0].layers[1] = kid;
    let q = Project::load(&q.to_json()).unwrap();
    for (t, b) in [0., 0.5, 1.].into_iter().zip(&before) {
        let a = world(&q, "kid", t);
        assert!(
            near(a.x, b.x)
                && near(a.y, b.y)
                && near(a.rotation, b.rotation)
                && near(a.scale, b.scale),
            "at {t}: {:?} vs {:?}",
            [a.x, a.y, a.rotation, a.scale],
            [b.x, b.y, b.rotation, b.scale]
        );
    }
    // And back out: the original tracks, to rounding.
    let out = reparent(&q, &q.scenes[0], "kid", None, 0.5).unwrap();
    assert!(out.parent.is_empty());
    assert!(near(out.x.at(1.), 150.) && near(out.y.at(0.), 40.) && near(out.scale.at(1.), 2.));
}

/// A parent chain that comes back round is an error when loading (so
/// `check` reports it), and so is a parent that is not there.
#[test]
fn parent_cycles_and_missing_parents_are_errors() {
    let bad = |layers: &str| {
        Project::load(&format!(
            r#"{{"size":[64,64],"fps":30,"scenes":[{{"name":"s","duration":1,"layers":[{layers}]}}]}}"#
        ))
        .unwrap_err()
    };
    let e = bad(r#"{"id":"a","kind":"rect","parent":"b"},{"id":"b","kind":"rect","parent":"a"}"#);
    assert!(
        e.starts_with("scenes[0].layers[0].parent:") && e.contains("cycle (a -> b -> a)"),
        "{e}"
    );
    let e = bad(r#"{"id":"a","kind":"rect","parent":"a"}"#);
    assert!(e.contains("cycle"), "{e}");
    let e = bad(r#"{"id":"a","kind":"rect","parent":"ghost"}"#);
    assert!(e.contains("no layer `ghost`"), "{e}");
    let issues = check::check(
        r#"{"size":[64,64],"fps":30,"scenes":[{"name":"s","duration":1,"layers":[
            {"id":"a","kind":"rect","parent":"b"},{"id":"b","kind":"rect","parent":"a"}]}]}"#,
        &mut Renderer::new(64, 64),
        &|_| true,
    );
    assert!(
        issues
            .iter()
            .any(|i| i.severity == check::Severity::Error && i.message.contains("cycle")),
        "{issues:?}"
    );
    // Reparenting refuses to close a loop.
    let p = scene(
        "2d",
        r#"{"id":"a","kind":"rect"},{"id":"b","kind":"rect","parent":"a"}"#,
    );
    let e = reparent(&p, &p.scenes[0], "a", Some("b"), 0.).unwrap_err();
    assert!(e.contains("cycle"), "{e}");
    assert!(reparent(&p, &p.scenes[0], "a", Some("a"), 0.).is_err());
}

/// `sources` round-trips through a save, is checked, and the panel's list
/// adds what layers use without importing, once each.
#[test]
fn sources_round_trip_and_list_what_layers_use() {
    let json = r##"{
  "size": [400, 200],
  "fps": 30.0,
  "sources": [
    {
      "id": "synth",
      "kind": "plugin",
      "source": {
        "cargo": "../Cargo.toml",
        "example": "synth"
      }
    },
    {
      "id": "logo",
      "name": "Logo",
      "kind": "svg",
      "path": "mark.svg"
    }
  ],
  "scenes": [
    {
      "name": "a",
      "duration": 2.0,
      "background": "#101014",
      "layers": [
        {
          "id": "l",
          "kind": "svg",
          "path": "mark.svg"
        },
        {
          "id": "pic",
          "kind": "image",
          "path": "img/card.png"
        },
        {
          "id": "pic2",
          "kind": "image",
          "path": "img/card.png"
        }
      ]
    }
  ]
}
"##;
    let p = Project::load(json).unwrap();
    assert_eq!(p.to_json(), json);
    let all = p.all_sources();
    let ids: Vec<&str> = all.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, ["synth", "logo", "card.png"]);
    assert_eq!(all[2].path(), Some("img/card.png"));
    assert_eq!(
        all[0].state(),
        Some(plugin::home(&Source {
            cargo: "../Cargo.toml".into(),
            example: "synth".into(),
            ..Source::default()
        }))
    );
    for bad in [
        r#"[{"id":"x","kind":"svg","path":"a.svg"},{"id":"x","kind":"svg","path":"b.svg"}]"#,
        r#"[{"id":"","kind":"svg","path":"a.svg"}]"#,
        r#"[{"id":"x","kind":"svg","path":""}]"#,
        r#"[{"id":"x","kind":"plugin","source":{}}]"#,
    ] {
        let doc = format!(r#"{{"size":[64,64],"fps":30,"sources":{bad},"scenes":[]}}"#);
        assert!(Project::load(&doc).unwrap_err().starts_with("sources["));
    }
}

/// The Sources panel's capture of a plugin is the one a bare layer two
/// levels deep starts in, and a component layer showing a control asks
/// for that depth too: its part is captured.
#[test]
fn a_plugin_layer_starts_in_its_sources_home_state() {
    let home = plugin::home(&Source {
        bin: "x".into(),
        ..Source::default()
    });
    for l in [
        r#"{"id":"syn","kind":"plugin","source":{"bin":"x"},"explode_levels":2}"#,
        r#"{"id":"syn","kind":"plugin","source":{"bin":"x"},"show":["osc/knob"]}"#,
    ] {
        let p = scene("2d", l);
        assert_eq!(world(&p, "syn", 0.).plugin.unwrap().state, home, "{l}");
    }
    let p = scene("2d", r#"{"id":"syn","kind":"plugin","source":{"bin":"x"}}"#);
    assert_ne!(
        world(&p, "syn", 0.).plugin.unwrap().state,
        home,
        "one level"
    );
}

/// A component layer (`show`) draws its parts alone, where the whole UI
/// puts them, and its outline is theirs.
#[test]
fn a_component_layer_draws_only_its_parts() {
    let p = scene(
        "2d",
        r#"{"id":"syn","kind":"plugin","source":{"bin":"x"},"x":200,"y":100,"show":["a"]}"#,
    );
    let assets = capture_assets(&p);
    let l = assets.layers(&eval(&p, &p.scenes[0], 0.)).unwrap();
    assert_eq!(l.scenes.len(), 1, "part a alone: no backdrop, no b");
    let ids: Vec<&str> = l.parts.iter().map(|q| q.id.as_str()).collect();
    assert_eq!(ids, ["syn#a"]);
    // The UI (200x100) is centred on 200,100; a is at 20,20 in it.
    assert_eq!(l.quads[0].pts[0], [120., 70.]);
    assert_eq!(l.quads[0].pts[2], [160., 90.]);
    assert_eq!(l.parts[0].pts[0], [120., 70.]);
    let slabs = assets.slabs(&eval(&p, &p.scenes[0], 0.).layers[0]);
    let ids: Vec<&str> = slabs.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, ["syn#a"], "in 3D too");
    // Nested parts come with their ancestor.
    let show = ["panel".to_owned()];
    assert!(plugin::shows(&show, "panel") && plugin::shows(&show, "panel/knob/cap"));
    assert!(!plugin::shows(&show, "panelx") && !plugin::shows(&show, "other"));
    assert!(plugin::shows(&[], "anything"));
}

/// A home capture's parts as the Sources panel nests them: the
/// capture's tree, with real names, each part's own rects and thumb.
#[test]
fn a_home_captures_parts_make_a_tree() {
    let cap: Capture = serde_json::from_value(serde_json::json!({
        "width": 100, "height": 50,
        "layers": [
            {"group": "background", "rect": [0, 0, 100, 50], "src": "bg.png"},
            {"group": "osc", "rect": [0, 0, 30, 10], "src": "osc.png"},
            {"group": "osc/knob", "rect": [5, 0, 10, 10], "src": "knob.png"},
            {"group": "env", "rect": [40, 0, 10, 10], "src": "env.png"},
        ],
        "parts": [
            {"path": "osc", "id": "osc", "frame": [0, 0, 30, 10]},
            {"path": "osc/knob", "id": "knob", "parent": "osc", "frame": [5, 0, 10, 10]},
            {"path": "env", "id": "env", "frame": [40, 0, 10, 10]},
        ],
    }))
    .unwrap();
    let t = plugin::home_tree(&cap, "k");
    let ids: Vec<&str> = t
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["osc", "env"]);
    let knob = &t[0]["children"][0];
    assert_eq!(
        (knob["id"].as_str(), knob["surface"].as_str()),
        (Some("osc/knob"), Some("knob"))
    );
    assert_eq!(knob["thumb"], format!("{}/knob.png", plugin::CACHE));
    assert_eq!(knob["level"], 2);
}

/// Reset to default clears a layer's transform keys and a plugin's part
/// offsets and explode (the captured layout), and leaves what it shows.
#[test]
fn reset_puts_a_layer_back_where_it_was_placed() {
    let p = scene(
        "2d",
        r#"{"id":"syn","kind":"plugin","source":{"bin":"x"},
            "x":[{"t":0,"v":10},{"t":1,"v":90}],"y":7,"rotation":20,"scale":3,"opacity":0.2,
            "explode":[{"t":0,"v":0},{"t":1,"v":1}],
            "params":[{"id":"f","field":"cutoff","value":0.4}],
            "parts":{"a":{"x":30}}},
           {"id":"kid","kind":"rect","parent":"syn","x":40,"y":[{"t":0,"v":1},{"t":1,"v":2}]}"#,
    );
    let mut l = p.scenes[0].layers[0].clone();
    l.reset(p.size);
    assert_eq!(
        (l.x.at(0.5), l.y.at(0.5)),
        (200., 100.),
        "the frame's middle"
    );
    assert_eq!(
        (
            l.rotation.at(0.),
            l.scale.at(0.),
            l.opacity.at(0.),
            l.explode.at(1.)
        ),
        (0., 1., 1., 0.)
    );
    let Kind::Plugin { parts, params, .. } = &l.kind else {
        unreachable!()
    };
    assert!(parts.is_empty() && params.len() == 1);
    let mut kid = p.scenes[0].layers[1].clone();
    kid.reset(p.size);
    assert_eq!((kid.x.at(0.), kid.y.at(1.)), (0., 0.), "on its parent");
    assert_eq!(kid.parent, "syn");
}

/// Taking a 3D scene flat keeps each layer where the camera showed it:
/// with the default camera the z = 0 plane is the frame, and a layer twice
/// as far is half the size, halfway to the middle.
#[test]
fn flattening_a_3d_scene_keeps_what_the_camera_shows() {
    let d = three::front_distance(200., 40.);
    let p = scene(
        "3d",
        &format!(
            r#"{{"id":"near","kind":"rect","x":100,"y":50,"rotation":10,"scale":2}},
               {{"id":"far","kind":"rect","x":[{{"t":0,"v":100}},{{"t":1,"v":300}}],"y":50,"z":{d},"ry":30}}"#
        ),
    );
    let flat = flatten(&p, &p.scenes[0], 0.);
    assert_eq!(flat.mode, Mode::TwoD);
    let [near_, far] = [&flat.layers[0], &flat.layers[1]];
    let close = |a: f64, b: f64| (a - b).abs() < 0.01;
    assert!(close(near_.x.at(0.), 100.) && close(near_.y.at(0.), 50.));
    assert!(close(near_.rotation.at(0.), 10.) && close(near_.scale.at(0.), 2.));
    // Twice as far from the eye: half size, half way to the centre (200,100).
    assert!(
        close(far.x.at(0.), 150.) && close(far.x.at(1.), 250.),
        "{:?}",
        far.x
    );
    assert!(close(far.y.at(0.), 75.) && close(far.scale.at(0.), 0.5));
    assert_eq!((far.z.at(0.), far.ry.at(0.)), (0., 0.));
    // A 2D scene comes back as it was.
    let two = scene("2d", r#"{"id":"r","kind":"rect","x":5}"#);
    assert_eq!(flatten(&two, &two.scenes[0], 0.), two.scenes[0]);
}

/// Parenting carries into 3D renders: the stage's slabs and the Blender
/// scene of a child are those of the same layer placed at its world
/// transform with no parent.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn parented_layers_render_in_3d_and_blender_where_they_sit() {
    let dad = r##"{"id":"dad","kind":"rect","x":300,"y":50,"z":40,"rotation":30,"scale":2,"opacity":0.5,"fill":"#ff0000"}"##;
    let a = scene(
        "3d",
        &format!(
            r##"{dad},{{"id":"kid","kind":"rect","parent":"dad","x":10,"y":5,"z":3,"width":20,"height":10,"fill":"#00ff00"}}"##
        ),
    );
    let w = world(&a, "kid", 0.);
    let b = scene(
        "3d",
        &format!(
            r##"{dad},{{"id":"kid","kind":"rect","x":{},"y":{},"z":{},"rotation":{},"scale":{},"opacity":{},"width":20,"height":10,"fill":"#00ff00"}}"##,
            w.x, w.y, w.space.z, w.rotation, w.scale, w.opacity
        ),
    );
    assert!(
        w.x != 10. && w.space.z != 3.,
        "the child moved with its parent"
    );
    let assets = Assets::default();
    let slab = |p: &Project| {
        let f = eval(p, &p.scenes[0], 0.);
        format!("{:?}", assets.slabs(&f.layers[1]))
    };
    assert_eq!(slab(&a), slab(&b), "stage slabs");
    let o = crate::blender::Options::new(None, None, 1, [400, 200]).unwrap();
    let desc = |p: &Project| {
        let (d, _) = crate::blender::describe(
            p,
            &p.scenes[0],
            &[0.],
            &assets,
            std::path::Path::new("."),
            &o,
        )
        .unwrap();
        serde_json::to_string(&d.frames).unwrap()
    };
    assert_eq!(desc(&a), desc(&b), "Blender objects");
}

/// Two placements agree to `tol` in every entry.
fn same(a: [f32; 16], b: [f32; 16], tol: f32) -> bool {
    a.iter().zip(&b).all(|(x, y)| (x - y).abs() <= tol)
}

/// A layer's pose as mui-stage and Blender place it: [`three::pose`] with
/// its scale (the rects here are centred, so no other offset).
fn posed(size: [u32; 2], d: &Drawn) -> mui_stage::Mat4 {
    three::pose(size, d, mui_stage::Mat4::scale(d.scale as f32))
}

/// In 3D a child rides its parent's whole transform, tilt and anchor
/// included: its slab (the stage's quad and plane) and its Blender object
/// sit where the parent's slab matrix carries its local one.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_child_rides_its_tilted_parent_in_the_stage_and_blender() {
    use mui_stage::{Mat4, Plane};
    let p = scene(
        "3d",
        r#"{"id":"dad","kind":"rect","x":300,"y":50,"z":40,"rotation":30,"rx":25,"ry":-40,"scale":2,"anchor_z":6},
           {"id":"kid","kind":"rect","parent":"dad","x":10,"y":5,"z":3,"rotation":15,"rx":10,"ry":20,"scale":0.5,"width":20,"height":10}"#,
    );
    let size = p.size;
    let f = eval(&p, &p.scenes[0], 0.);
    let (dad, kid) = (&f.layers[0], &f.layers[1]);
    let deg = |v: f64| v.to_radians() as f32;
    // The parent's space as its slab is posed, its face 6 px in front of
    // its pivot; the child's local pose in it (stage axes: y up, z out).
    let parent = three::pose(size, dad, Mat4::scale(2.) * Mat4::translate([0., 0., 6.]));
    let local = Mat4::translate([10., -5., -3.])
        * Mat4::rotate_y(deg(20.))
        * Mat4::rotate_x(deg(-10.))
        * Mat4::rotate_z(deg(-15.))
        * Mat4::scale(0.5);
    let want = (parent * local).0;
    assert!(
        kid.space.rx != 10. && kid.space.ry != 20.,
        "the tilt is inherited"
    );
    assert!(same(posed(size, kid).0, want, 1e-3), "the stage's quad");
    let mut plane = Plane::new("atlas", 20., 10.)
        .scale(kid.scale as f32)
        .rotate(
            -kid.space.rx as f32,
            kid.space.ry as f32,
            -kid.rotation as f32,
        );
    plane.position = three::world(size, [kid.x, kid.y, kid.space.z]);
    assert!(same(plane.model().0, want, 1e-3), "the stage's slab");
    let o = crate::blender::Options::new(None, None, 1, size).unwrap();
    let (d, _) = crate::blender::describe(
        &p,
        &p.scenes[0],
        &[0.],
        &Assets::default(),
        std::path::Path::new("."),
        &o,
    )
    .unwrap();
    let m = d.frames[0][0].layers[1].m;
    let b = crate::blender::placed(Mat4(want));
    assert!(same(m, b, 1e-3), "Blender: {m:?} vs {b:?}");
}

/// Reparenting between tilted parents in 3D keeps the whole world matrix
/// (turns included) at every key, and coming back gives the local tracks
/// back.
#[test]
fn reparenting_between_tilted_parents_keeps_the_world_matrix() {
    let p = scene(
        "3d",
        r#"{"id":"dad","kind":"rect","x":300,"y":50,"z":40,"rotation":30,"rx":25,"ry":-40,"scale":2,"anchor_z":6},
           {"id":"mom","kind":"rect","x":100,"y":150,"z":-20,"rotation":-70,"rx":-35,"ry":15,"scale":0.8},
           {"id":"kid","kind":"rect","parent":"dad","width":20,"height":10,
            "x":[{"t":0,"v":10,"interp":"linear"},{"t":2,"v":40}],"y":5,"z":3,
            "rx":[{"t":0,"v":0},{"t":2,"v":60}],"ry":20,"rotation":[{"t":0.5,"v":15},{"t":1.5,"v":-45}],
            "scale":0.5}"#,
    );
    // Every turn key's time (the position moves linearly, so is exact
    // throughout).
    let times = [0., 0.5, 1.5, 2.];
    let pose_at = |p: &Project, t: f64| posed(p.size, &world(p, "kid", t)).0;
    let before: Vec<_> = times.iter().map(|&t| pose_at(&p, t)).collect();
    let step = |p: &Project, to: Option<&str>| {
        let l = reparent(p, &p.scenes[0], "kid", to, 0.7).unwrap();
        let mut q = p.clone();
        q.scenes[0].layers[2] = l;
        Project::load(&q.to_json()).unwrap()
    };
    let mut q = p.clone();
    for to in [Some("mom"), None, Some("dad")] {
        q = step(&q, to);
        for (&t, b) in times.iter().zip(&before) {
            let a = pose_at(&q, t);
            assert!(same(a, *b, 1e-3), "under {to:?} at {t}: {a:?} vs {b:?}");
        }
    }
    let (a, b) = (&q.scenes[0].layers[2], &p.scenes[0].layers[2]);
    for t in times {
        for (x, y) in [
            (&a.x, &b.x),
            (&a.rx, &b.rx),
            (&a.ry, &b.ry),
            (&a.rotation, &b.rotation),
        ] {
            assert!(
                near(x.at(t), y.at(t)) || (x.at(t) - y.at(t)).abs() < 1e-6,
                "at {t}"
            );
        }
    }
}

/// Under a turned parent x and y mix, so a layer keyed at different times
/// on each gets keyed at all of them: every key stays where it was.
#[test]
fn reparenting_under_a_turned_parent_unions_the_key_times() {
    let p = scene(
        "2d",
        r#"{"id":"top","kind":"rect","x":200,"y":100,"rotation":90,"scale":1.5},
           {"id":"kid","kind":"rect",
            "x":[{"t":0,"v":50},{"t":1,"v":150,"interp":"linear"}],
            "y":[{"t":0.5,"v":40,"interp":"linear"},{"t":2,"v":90}]}"#,
    );
    let times = [0., 0.5, 1., 2.];
    let before: Vec<Drawn> = times.iter().map(|&t| world(&p, "kid", t)).collect();
    let kid = reparent(&p, &p.scenes[0], "kid", Some("top"), 0.3).unwrap();
    let keyed = |a: &Anim<f64>| match a {
        Anim::Keys(k) => k.iter().map(|k| k.t).collect::<Vec<_>>(),
        Anim::Value(_) => Vec::new(),
    };
    assert_eq!(keyed(&kid.x), times, "x keyed at every time");
    assert_eq!(keyed(&kid.y), times, "y keyed at every time");
    let mut q = p.clone();
    q.scenes[0].layers[1] = kid;
    for (&t, b) in times.iter().zip(&before) {
        let a = world(&q, "kid", t);
        assert!(
            near(a.x, b.x) && near(a.y, b.y),
            "at {t}: {:?} vs {:?}",
            [a.x, a.y],
            [b.x, b.y]
        );
    }
}

/// A layer bound to variables reparents in every variant: each keeps its
/// world position, untouched bindings stay, and a variant whose result
/// differs from the file's gets its own override.
#[test]
fn reparenting_a_var_bound_layer_keeps_every_variant_in_place() {
    let text = r#"{"size":[400,200],"fps":30,
      "variables":{"gap":{"type":"number","value":10}},
      "variants":[{"name":"wide","size":[800,200]},{"name":"far","vars":{"gap":40}}],
      "scenes":[{"name":"a","duration":2,"layers":[
        {"id":"top","kind":"rect","x":{"var":"W","mul":0.25},"y":100,"rotation":90,"scale":2},
        {"id":"kid","kind":"rect","x":{"var":"W","mul":0.5},"y":{"var":"gap","add":50},
         "width":{"var":"gap","mul":2},"height":10}]}]}"#;
    let root: serde_json::Value = serde_json::from_str(text).unwrap();
    let p = Project::load(text).unwrap();
    let variants = [None, Some("wide"), Some("far")];
    let get = |p: &Project, v: Option<&str>| match v {
        Some(n) => p.variant(n).unwrap(),
        None => p.clone(),
    };
    let before: Vec<Drawn> = variants
        .iter()
        .map(|&v| world(&get(&p, v), "kid", 0.))
        .collect();
    let out = crate::place::rewrite(&root, 0, &|p, s| {
        let l = reparent(p, s, "kid", Some("top"), 0.)?;
        let mut s = s.clone();
        s.layers[1] = l;
        Ok(s)
    })
    .unwrap();
    let kid = &out["scenes"][0]["layers"][1];
    assert_eq!(kid["parent"], "top");
    assert_eq!(kid["width"]["var"], "gap", "an untouched binding stays");
    assert!(
        out["variants"][0]["overrides"]["a/kid"].is_object(),
        "{}",
        out["variants"]
    );
    let q = Project::load(&out.to_string()).unwrap();
    for (&v, b) in variants.iter().zip(&before) {
        let a = world(&get(&q, v), "kid", 0.);
        assert!(
            near(a.x, b.x)
                && near(a.y, b.y)
                && near(a.rotation, b.rotation)
                && near(a.scale, b.scale),
            "{v:?}: {:?} vs {:?}",
            [a.x, a.y, a.rotation, a.scale],
            [b.x, b.y, b.rotation, b.scale]
        );
    }
    // Reset and taking flat run through the same rewrite.
    let reset = crate::place::rewrite(&out, 0, &|p, s| {
        let mut s = s.clone();
        s.layers[1].reset(p.size);
        Ok(s)
    })
    .unwrap();
    let q = Project::load(&reset.to_string()).unwrap();
    for v in variants {
        let a = world(&get(&q, v), "kid", 0.);
        assert!(
            near(a.x, get(&q, v).size[0] as f64 / 4.) && near(a.y, 100.),
            "{v:?} on its parent"
        );
    }
}

/// A font file is a source; a text layer that picks it by id draws its
/// glyphs, on the canvas and in the textures Blender and the stage use.
#[test]
fn a_font_source_draws_its_own_glyphs() {
    const ICONS: &[u8] =
        include_bytes!("../../../crates/mui-text/fonts/MaterialSymbolsOutlined-subset.ttf");
    let p = Project::load(
        r##"{"size":[64,64],"fps":30,
          "sources":[{"id":"icons","kind":"font","path":"fonts/icons.ttf"}],
          "scenes":[{"name":"a","duration":1,"layers":[
            {"id":"t","kind":"text","text":"","font":"icons","x":32,"y":32,"font_size":40,"fill":"#ffffff"}]}]}"##,
    )
    .unwrap();
    let listed: Vec<_> = p.all_sources().into_iter().map(|m| m.id).collect();
    assert_eq!(
        listed,
        ["icons"],
        "the layer's font is the source, not another"
    );
    let f = eval(&p, &p.scenes[0], 0.);
    assert!(
        matches!(&f.layers[0].kind, Kind::Text { font, .. } if font == "fonts/icons.ttf"),
        "the id resolves to the file"
    );
    let ink = |px: &[u8]| px.chunks(4).filter(|c| c[0] > 128).count();
    let mut r = Renderer::new(64, 64);
    let (inter, _) = r.draw(&f).unwrap();
    r.add_asset("fonts/icons.ttf", ICONS).unwrap();
    let (icons, _) = r.draw(&f).unwrap();
    assert!(ink(&inter) > 20 && ink(&icons) > 20, "both draw");
    assert_ne!(inter, icons, "the font's own glyph, not Inter's");
    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut a = Assets::default();
        let (plain, ..) = a.paint(&f.layers[0], 1.).unwrap();
        a.add_asset("fonts/icons.ttf", ICONS).unwrap();
        let (font, ..) = a.paint(&f.layers[0], 1.).unwrap();
        assert_ne!(plain, font, "Blender's texture");
    }
}

/// Under a tilted, scaled glass parent a child keeps its own material,
/// in the stage's frame and in Blender's; only its pose comes from above.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_child_keeps_its_own_material_under_a_tilted_parent() {
    let p = scene(
        "3d",
        r##"{"id":"dad","kind":"rect","x":200,"y":100,"rx":25,"ry":-40,"scale":2,"extrude":6,
             "width":80,"height":60,"material":{"transmission":1,"ior":1.7}},
            {"id":"kid","kind":"rect","parent":"dad","x":10,"y":5,"z":3,"extrude":4,
             "width":20,"height":10,"material":{"metallic":1,"roughness":0.2}}"##,
    );
    let (dad, kid) = (world(&p, "dad", 0.), world(&p, "kid", 0.));
    assert!(kid.space.rx != 0. && kid.scale == 2., "posed by its parent");
    let own = kid.space.material.unwrap();
    assert_eq!(
        (own.metallic, own.roughness, own.transmission),
        (Some(1.), Some(0.2), None),
        "not the parent's glass"
    );
    assert_eq!(dad.space.material.unwrap().ior, Some(1.7));
    let o = crate::blender::Options::new(None, None, 1, p.size).unwrap();
    let (d, _) = crate::blender::describe(
        &p,
        &p.scenes[0],
        &[0.],
        &Assets::default(),
        std::path::Path::new("."),
        &o,
    )
    .unwrap();
    let mat = |id: &str| {
        let i = d.layers.iter().position(|l| l.id == id).unwrap();
        d.frames[0][0].layers[i].mat.clone()
    };
    let (m, top) = (mat("kid"), mat("dad"));
    assert_eq!((m.metallic, m.transmission), (Some(1.), Some(0.)));
    // Object units: the object's scale (the parent's 2) makes it 8 px.
    assert_eq!(m.thickness, Some(4.));
    assert_eq!((top.transmission, top.ior), (Some(1.), Some(1.7)));
}

/// The centre pixel of `p`'s first scene at `t`, drawn 40x20 on the CPU.
fn centre_px(p: &Project, t: f64) -> [u8; 4] {
    let mut r = Renderer::new(40, 20);
    let (px, _) = r.draw(&eval(p, &p.scenes[0], t)).unwrap();
    px[(10 * 40 + 20) * 4..][..4].try_into().unwrap()
}

/// A group draws nothing; its children move and fade with it, through
/// nested groups, and a group faded out takes its whole subtree.
#[test]
fn groups_draw_nothing_and_carry_their_subtree() {
    let layers = |opacity: f64| {
        format!(
            r##"{{"id":"outer","kind":"group","x":200,"y":100,"opacity":{opacity}}},
               {{"id":"inner","kind":"group","parent":"outer","scale":2,"opacity":0.5}},
               {{"id":"dot","kind":"rect","parent":"inner","x":5,"fill":"#ff0000"}}"##
        )
    };
    let p = scene("2d", &layers(1.));
    let dot = world(&p, "dot", 0.);
    assert!(near(dot.x, 210.) && near(dot.y, 100.) && near(dot.scale, 2.));
    assert!(near(dot.opacity, 0.5), "opacity multiplies down the tree");
    let red = centre_px(&p, 0.);
    assert!(red[0] > 100 && red[1] < 20, "the child draws: {red:?}");
    let hidden = scene("2d", &layers(0.));
    assert_eq!(world(&hidden, "dot", 0.).opacity, 0.);
    assert_eq!(
        centre_px(&hidden, 0.),
        [16, 16, 20, 255],
        "the subtree is gone"
    );
    // Alone, a group (white fill and 100 square by default) draws nothing.
    let empty = scene("2d", r#"{"id":"g","kind":"group","x":200,"y":100}"#);
    assert_eq!(centre_px(&empty, 0.), [16, 16, 20, 255]);
    assert_eq!(
        empty.scenes[0].layers[0]
            .props()
            .iter()
            .map(|(n, _)| n.as_str())
            .collect::<Vec<_>>(),
        ["x", "y", "scale", "rotation", "opacity"]
    );
    // In 3D it is no slab, for the stage or Blender.
    let three = scene("3d", r#"{"id":"g","kind":"group","x":200,"y":100}"#);
    let f = eval(&three, &three.scenes[0], 0.);
    assert!(Assets::default().slabs(&f.layers[0]).is_empty());
}

/// `start`/`end` and `hidden` switch a layer and its subtree off; keys
/// stay in scene time while what plays starts at `start`.
#[test]
fn layers_show_between_start_and_end() {
    let p = scene(
        "2d",
        r##"{"id":"g","kind":"group","start":1,"end":1.5,"x":200,"y":100},
            {"id":"dot","kind":"rect","parent":"g","fill":"#ff0000",
             "x":[{"t":0,"v":0,"interp":"linear"},{"t":2,"v":20}]},
            {"id":"anim","kind":"lottie","path":"a.json","start":0.5,"time":2},
            {"id":"off","kind":"rect","hidden":true}"##,
    );
    let on = |id: &str, t: f64| world(&p, id, t).opacity > 0.;
    assert!(!on("dot", 0.99) && on("dot", 1.) && on("dot", 1.49) && !on("dot", 1.5));
    assert!(!on("g", 0.5) && !on("off", 1.));
    assert!(near(world(&p, "dot", 1.).x, 210.), "keys in scene time");
    assert_eq!(centre_px(&p, 0.5), [16, 16, 20, 255]);
    assert!(centre_px(&p, 1.2)[0] > 200);
    assert!(
        near(world(&p, "anim", 1.).time, 2.5),
        "plays from its start"
    );
    // Saved as written; refused back to front.
    let back = Project::load(&p.to_json()).unwrap();
    assert_eq!(back, p);
    assert!(p.to_json().contains(r#""start": 1.0,"#));
    for bad in [r#""start":2,"end":1"#, r#""start":1,"end":1"#] {
        let json = format!(
            r#"{{"size":[400,200],"fps":30,"scenes":[{{"name":"a","duration":2,"layers":[{{"id":"r","kind":"rect",{bad}}}]}}]}}"#
        );
        assert!(Project::load(&json).unwrap_err().contains("end"), "{bad}");
    }
}

/// Markers are data: they load and save one a line, as written.
#[test]
fn markers_round_trip() {
    let json = r#"{"size":[400,200],"fps":30,"scenes":[{"name":"a","duration":2,
        "markers":[{"t":1.5,"name":"drop"},{"t":0.25}]}]}"#;
    let p = Project::load(json).unwrap();
    let m = &p.scenes[0].markers;
    assert_eq!(
        (m[0].t, m[0].name.as_str(), m[1].name.as_str()),
        (1.5, "drop", "")
    );
    let saved = p.to_json();
    assert!(
        saved.contains(r#"{ "t": 1.5, "name": "drop" },"#),
        "{saved}"
    );
    assert_eq!(Project::load(&saved).unwrap().to_json(), saved);
}
