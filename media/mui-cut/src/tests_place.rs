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
