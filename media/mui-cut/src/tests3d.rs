//! 3D scenes: the camera's maths, lights, models, and that 2D is untouched.
use super::*;
use crate::three::{aim, front_distance, stage_camera};

const STAGE3D: &str = include_str!("../examples/stage3d.cut.json");
const DEMO: &str = include_str!("../examples/demo.cut.json");

fn scene3d(layers: &str) -> Project {
    Project::load(&format!(
        r#"{{"size":[640,360],"fps":30,"scenes":[{{"name":"a","duration":2,"mode":"3d",
            "layers":[{layers}]}}]}}"#
    ))
    .unwrap()
}

fn close(a: [f64; 3], b: [f64; 3]) -> bool {
    (0..3).all(|i| (a[i] - b[i]).abs() < 1e-6)
}

/// A project point through the stage camera, in project pixels.
fn to_px(p: &Project, cam: &Cam, at: [f64; 3]) -> [f64; 2] {
    let [w, h] = p.size.map(f64::from);
    let vp = stage_camera(p.size, cam).view_proj((w / h) as f32);
    let q = vp.project(three::world(p.size, at));
    [
        (f64::from(q[0]) + 1.) * 0.5 * w,
        (1. - f64::from(q[1])) * 0.5 * h,
    ]
}

#[test]
fn the_default_camera_sees_the_z0_plane_as_the_2d_frame() {
    let p = scene3d(r#"{"id":"r","kind":"rect"}"#);
    let f = eval(&p, &p.scenes[0], 0.);
    let cam = &f.view.as_ref().unwrap().camera;
    assert!(close(cam.eye, [320., 180., -front_distance(360., 40.)]));
    for pt in [[0., 0.], [640., 360.], [100., 250.], [320., 180.]] {
        let got = to_px(&p, cam, [pt[0], pt[1], 0.]);
        assert!(
            (got[0] - pt[0]).abs() < 0.01 && (got[1] - pt[1]).abs() < 0.01,
            "{pt:?} -> {got:?}"
        );
    }
    // Deeper is smaller, toward the centre.
    let far = to_px(&p, cam, [0., 0., 500.]);
    assert!(far[0] > 0. && far[1] > 0. && far[0] < 320.);
}

#[test]
fn the_camera_orbits_dollies_rolls_and_looks_at_a_layer() {
    let p = scene3d(
        r#"{"id":"cam","kind":"camera","x":100,"y":50,"z":20,"ry":90,"distance":400},
           {"id":"up","kind":"camera","x":0,"y":0,"rx":30,"distance":200,"dolly":0.5,
            "aperture":8,"focus":10,"opacity":[{"t":0,"v":0,"interp":"hold"},{"t":1,"v":1}]},
           {"id":"card","kind":"rect","x":300,"y":200,"z":50},
           {"id":"look","kind":"camera","look_at":"card","distance":100,
            "path":"M 0 0 L 100 0","path_offset":0.5,"opacity":[{"t":0,"v":0,"interp":"hold"},{"t":1.5,"v":1}]}"#,
    );
    let s = &p.scenes[0];
    // Yaw 90: the camera turned right, so it sits left of its target.
    let c = eval(&p, s, 0.).view.unwrap().camera;
    assert!(close(c.target, [100., 50., 20.]), "{c:?}");
    assert!(close(c.eye, [-300., 50., 20.]), "{c:?}");
    assert_eq!((c.focus, c.aperture), (0., 0.), "no aperture, no focus");
    // A later camera takes over once it shows; pitched 30 it looks down
    // from above, dollied halfway in.
    let c = eval(&p, s, 1.).view.unwrap().camera;
    let back = aim(30., 0.);
    assert!(close(c.eye, back.map(|v| -v * 100.)), "{c:?}");
    assert!(c.eye[1] < 0., "above its target");
    assert!(
        (c.focus - 110.).abs() < 1e-9,
        "focus past the target: {}",
        c.focus
    );
    // Looking at a layer while the path carries the eye 50 px across.
    let c = eval(&p, s, 1.5).view.unwrap().camera;
    assert!(close(c.target, [300., 200., 50.]), "{c:?}");
    assert!(close(c.eye, [350., 200., -50.]), "{c:?}");
    // What it looks at lands mid-frame, rolled or not.
    for roll in [0., 30.] {
        let cam = Cam { roll, ..c.clone() };
        let mid = to_px(&p, &cam, c.target);
        assert!(
            (mid[0] - 320.).abs() < 1e-3 && (mid[1] - 180.).abs() < 1e-3,
            "{mid:?}"
        );
    }
    // Rolled clockwise, a point right of centre moves down on screen.
    let right = [c.target[0] + 20., c.target[1], c.target[2]];
    let rolled = to_px(
        &p,
        &Cam {
            roll: 30.,
            ..c.clone()
        },
        right,
    );
    assert!(rolled[1] > 180. + 1., "{rolled:?}");
    let bad = r#"{"size":[64,64],"fps":30,"scenes":[{"name":"a","duration":1,
        "layers":[{"id":"c","kind":"camera","look_at":"nobody"}]}]}"#;
    assert!(Project::load(bad).is_err());
}

#[test]
fn lights_aim_by_pitch_and_yaw_and_fade_with_opacity() {
    let p = scene3d(
        r##"{"id":"sun","kind":"light","rx":90,"fill":"#ff0000","intensity":2,"opacity":0.5},
           {"id":"spot","kind":"light","type":"spot","ry":90,"cast_shadows":false}"##,
    );
    let v = eval(&p, &p.scenes[0], 0.).view.unwrap();
    assert_eq!(v.lights.len(), 2);
    assert!(close(v.lights[0].direction, [0., 1., 0.]), "straight down");
    assert_eq!(v.lights[0].intensity, 1.);
    assert!(close(v.lights[1].direction, [1., 0., 0.]), "to the right");
    assert!(!v.lights[1].shadows);
}

#[test]
fn a_2d_scene_evaluates_and_saves_exactly_as_before() {
    let p = Project::load(DEMO).unwrap();
    assert_eq!(p.to_json(), DEMO);
    for s in &p.scenes {
        let f = eval(&p, s, 1.);
        assert!(f.view.is_none());
        let json = serde_json::to_string(&f).unwrap();
        assert!(!json.contains("\"space\"") && !json.contains("\"view\""));
        for l in &s.layers {
            assert_eq!(
                l.props().len() + 6,
                l.props_in(true).len(),
                "3D lists six more"
            );
            assert!(l.props().iter().all(|(n, _)| n != "z"));
        }
    }
}

#[test]
fn the_stage_example_loads_round_trips_and_its_model_parses() {
    let p = Project::load(STAGE3D).unwrap();
    assert_eq!(p.to_json(), STAGE3D);
    let mesh = three::glb(include_bytes!("../examples/knot.glb")).unwrap();
    assert_eq!(mesh.parts.len(), 1);
    let part = &mesh.parts[0];
    assert!(part.indices.len() > 1000 && part.indices.len().is_multiple_of(3));
    assert!(part.metallic > 0.5 && part.color[0] > part.color[2]);
    assert!(mesh.max[1] > mesh.min[1]);
    assert!(three::glb(b"not a model").is_err());
}

#[cfg(not(target_arch = "wasm32"))]
fn offline(p: &Project, engine: Engine) -> Option<Offline> {
    let mut g = Offline::new(p.size, engine)
        .map_err(|e| eprintln!("skipped: no GPU ({e})"))
        .ok()?;
    g.assets
        .add_asset("knot.glb", include_bytes!("../examples/knot.glb"))
        .unwrap();
    Some(g)
}

#[cfg(not(target_arch = "wasm32"))]
fn frame(g: &mut Offline, subs: &[Frame]) -> Vec<u8> {
    match g.push(subs).unwrap() {
        Some(px) => px,
        None => g.finish().unwrap().pop().unwrap(),
    }
}

/// Flat layers at z 0 under the default camera and no lights are the 2D
/// frame, give or take the 3D pass's antialiasing and texture filtering.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn an_unturned_unlit_3d_scene_draws_like_its_2d_self() {
    let mut p = Project::load(DEMO).unwrap();
    p.size = [640, 360];
    for mut s in p.scenes.clone() {
        let flat = eval(&p, &s, 1.);
        s.mode = Mode::ThreeD;
        let deep = eval(&p, &s, 1.);
        for engine in [Engine::Classic, Engine::Sparse] {
            let Some(mut g) = offline(&p, engine) else {
                return;
            };
            let a = frame(&mut g, std::slice::from_ref(&flat));
            let b = frame(&mut g, std::slice::from_ref(&deep));
            let off = a
                .iter()
                .zip(&b)
                .filter(|(a, b)| a.abs_diff(**b) > 24)
                .count();
            assert!(
                off < a.len() / 50,
                "{engine:?} scene {}: {off} channels differ",
                s.name
            );
        }
    }
}

/// The example (shadow maps, contact shadow, a model, depth of field)
/// renders the same pixels twice, on both engines, with motion blur.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn shadowed_3d_frames_are_deterministic_and_drawn() {
    let mut p = Project::load(STAGE3D).unwrap();
    p.size = [480, 270];
    let s = &p.scenes[0];
    let subs: Vec<Frame> = (0..3)
        .map(|k| eval(&p, s, 3. + f64::from(k) * 0.01))
        .collect();
    let mut engines = Vec::new();
    for engine in [Engine::Classic, Engine::Sparse] {
        let Some(mut g) = offline(&p, engine) else {
            return;
        };
        let a = frame(&mut g, &subs);
        let b = frame(&mut g, &subs);
        assert_eq!(a, b, "{engine:?}");
        let bg = p.scenes[0].background.0;
        let drawn = a
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|px| px[..3].iter().zip(&bg).any(|(a, b)| a.abs_diff(*b) > 30))
            .count();
        assert!(drawn > a.len() / 4 / 10, "{engine:?}: {drawn} pixels drawn");
        engines.push(a);
    }
    // The engines only paint the atlas: the 3D pass is the same.
    let off = engines[0]
        .iter()
        .zip(&engines[1])
        .filter(|(a, b)| a.abs_diff(**b) > 8)
        .count();
    assert!(off < engines[0].len() / 200, "{off} channels differ");
}
