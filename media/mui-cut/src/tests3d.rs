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
           {"id":"look","kind":"camera","x":10,"y":20,"z":-300,"look_at":"card","distance":100,"ry":40,
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
    // Looking at a layer it stands at its own x/y/z, the path carrying it
    // 50 px across; distance and ry do not move it.
    let c = eval(&p, s, 1.5).view.unwrap().camera;
    assert!(close(c.target, [300., 200., 50.]), "{c:?}");
    assert!(close(c.eye, [60., 20., -300.]), "{c:?}");
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
    assert!(part.material.metallic > 0.5 && part.color[0] > part.color[2]);
    assert!(mesh.max[1] > mesh.min[1]);
    // Its triangles wind counter-clockwise about their normals, as glTF
    // says: Blender shades a clockwise one from the inside (black in Cycles).
    let v = |i: u32| part.vertices[i as usize];
    let sub = |a: [f32; 6], b: [f32; 6]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let outward = part
        .indices
        .chunks(3)
        .filter(|t| {
            let (a, b, c) = (v(t[0]), v(t[1]), v(t[2]));
            let (e, f) = (sub(b, a), sub(c, a));
            let n = [
                e[1] * f[2] - e[2] * f[1],
                e[2] * f[0] - e[0] * f[2],
                e[0] * f[1] - e[1] * f[0],
            ];
            n[0] * (a[3] + b[3] + c[3]) + n[1] * (a[4] + b[4] + c[4]) + n[2] * (a[5] + b[5] + c[5])
                > 0.
        })
        .count();
    assert_eq!(outward, part.indices.len() / 3);
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

/// Beauty is the mean of jittered samples, and the preview's running mean
/// of them lands where the export's does.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_refined_preview_is_the_beauty_export() {
    let mut p = Project::load(STAGE3D).unwrap();
    p.size = [480, 270];
    let f = eval(&p, &p.scenes[0], 3.);
    let Some(mut g) = offline(&p, Engine::Classic) else {
        return;
    };
    let plain = frame(&mut g, &[f.clone(), f.clone()]);
    g.beauty = true;
    let export = frame(&mut g, &vec![f.clone(); 16]);
    let preview = match g.push_refined(&f, 16).unwrap() {
        Some(px) => px,
        None => g.finish().unwrap().pop().unwrap(),
    };
    let off = |a: &[u8], b: &[u8], by: u8| {
        a.iter()
            .zip(b)
            .filter(|(a, b)| a.abs_diff(**b) > by)
            .count()
    };
    assert!(
        off(&plain, &export, 8) > plain.len() / 100,
        "beauty changes the frame"
    );
    assert!(
        off(&preview, &export, 3) < export.len() / 1000,
        "{} channels off",
        off(&preview, &export, 3)
    );
}

#[test]
fn the_orbit_preview_swings_about_the_target_and_keeps_zero_as_is() {
    let cam = Cam {
        eye: [0.0, 0.0, -1000.0],
        target: [0.0, 0.0, 0.0],
        fov: 40.0,
        roll: 0.0,
        focus: 0.0,
        aperture: 0.0,
    };
    assert!(close(cam.orbit(0.0, 0.0, 1.0).eye, cam.eye));
    // A quarter turn right puts the eye on the left, looking +x.
    assert!(close(cam.orbit(90.0, 0.0, 1.0).eye, [-1000.0, 0.0, 0.0]));
    // Pitching down lifts the eye (y up is negative in project space).
    let up = cam.orbit(0.0, 30.0, 2.0).eye;
    assert!(up[1] < 0.0 && (up.iter().map(|v| v * v).sum::<f64>().sqrt() - 2000.0).abs() < 1e-6);
}

/// Where the 3D pass cannot run, the editor draws flat with a notice but an
/// export stops instead of writing the wrong picture.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn an_export_refuses_to_draw_a_3d_shot_flat() {
    let p = scene3d(r#"{"id":"a","kind":"rect","x":320,"y":180}"#);
    let Some(mut g) = offline(&p, Engine::Classic) else {
        return;
    };
    g.canvas.three_d = false;
    let f = eval(&p, &p.scenes[0], 0.);
    assert!(g.push(&[f]).is_err());
    assert!(g.canvas.notice().contains("flat"));
}

/// A 3D scene's effect stack runs on the 3D pass's output, on both
/// engines and under motion blur: levels with no saturation greys a red
/// card the 3D pass drew.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_3d_scene_runs_its_effects_on_the_3d_pass() {
    let card = r##"{"id":"card","kind":"rect","x":320,"y":180,"width":300,"height":200,
        "ry":20,"fill":"#e03020"}"##;
    let plain = scene3d(card);
    let mut fx = plain.clone();
    fx.scenes[0].effects =
        serde_json::from_str(r#"[{"type": "levels", "saturation": 0}]"#).unwrap();
    let centre = (180 * 640 + 320) * 4;
    for engine in [Engine::Classic, Engine::Sparse] {
        let Some(mut g) = offline(&plain, engine) else {
            return;
        };
        let subs = |p: &Project| [0., 0.01].map(|t| eval(p, &p.scenes[0], t));
        let a = frame(&mut g, &subs(&plain));
        let b = frame(&mut g, &subs(&fx));
        assert!(g.canvas.notice().is_empty(), "{}", g.canvas.notice());
        let [r, gr, bl] = [0, 1, 2].map(|i| i32::from(a[centre + i]));
        assert!(
            r > gr + 80 && r > bl + 80,
            "{engine:?}: the card is red: {r} {gr} {bl}"
        );
        let [r, gr, bl] = [0, 1, 2].map(|i| i32::from(b[centre + i]));
        assert!(
            (r - gr).abs() <= 3 && (gr - bl).abs() <= 3 && r > 20,
            "{engine:?}: the effect greyed it: {r} {gr} {bl}"
        );
    }
}

/// An overlay layer is drawn flat after the 3D pass and the scene's
/// effects: a green caption over a red card greyed by levels stays green,
/// in plain frames, beauty samples and over a Blender plate, and a tilted
/// one is not foreshortened. Blender's description leaves it out.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn an_overlay_draws_flat_over_the_3d_pass_and_its_effects() {
    let layers = |overlay: bool| {
        format!(
            r##"{{"id":"card","kind":"rect","x":320,"y":180,"width":300,"height":200,
                "ry":20,"fill":"#e03020"}},
               {{"id":"cap","kind":"rect","x":320,"y":180,"width":120,"height":60,
                "ry":60,"fill":"#20d040","overlay":{overlay}}}"##
        )
    };
    let mut p = scene3d(&layers(true));
    p.scenes[0].effects = serde_json::from_str(r#"[{"type": "levels", "saturation": 0}]"#).unwrap();
    assert!(
        Project::load(&p.to_json()).unwrap() == p,
        "overlay round-trips"
    );
    let mut under = p.clone();
    under.scenes[0].layers[1].overlay = false;
    let at = |x: usize, y: usize| (y * 640 + x) * 4;
    let rgb = |px: &[u8], i: usize| [0, 1, 2].map(|k| i32::from(px[i + k]));
    let green = |c: [i32; 3]| c[1] > c[0] + 80 && c[1] > c[2] + 80;
    let grey = |c: [i32; 3]| (c[0] - c[1]).abs() <= 3 && (c[1] - c[2]).abs() <= 3 && c[0] > 20;
    // `ry` 60 would squeeze it to half its width in 3D: flat, it spans
    // x 260..380.
    let edge = at(372, 180);
    for engine in [Engine::Classic, Engine::Sparse] {
        let Some(mut g) = offline(&p, engine) else {
            return;
        };
        let f = eval(&p, &p.scenes[0], 0.);
        for beauty in [false, true] {
            g.beauty = beauty;
            let px = frame(&mut g, &vec![f.clone(); if beauty { 4 } else { 1 }]);
            assert!(g.canvas.notice().is_empty(), "{}", g.canvas.notice());
            assert!(
                green(rgb(&px, at(320, 180))),
                "{engine:?} {beauty}: {:?}",
                rgb(&px, at(320, 180))
            );
            assert!(
                green(rgb(&px, edge)),
                "{engine:?} {beauty}: flat {:?}",
                rgb(&px, edge)
            );
            assert!(
                grey(rgb(&px, at(200, 180))),
                "{engine:?} {beauty}: card {:?}",
                rgb(&px, at(200, 180))
            );
        }
        g.beauty = false;
        // Not an overlay, the caption is a slab the effect greys.
        let px = frame(&mut g, &[eval(&under, &under.scenes[0], 0.)]);
        assert!(
            grey(rgb(&px, at(320, 180))),
            "{engine:?}: {:?}",
            rgb(&px, at(320, 180))
        );
        // Over a Blender frame: a blue plate, greyed, the caption on it.
        let plate: Vec<u8> = [40u8, 60, 220, 255].repeat(640 * 360);
        let px = match g.push_plate(&f, &plate).unwrap() {
            Some(px) => px,
            None => g.finish().unwrap().pop().unwrap(),
        };
        assert!(
            green(rgb(&px, at(320, 180))),
            "{engine:?} plate: {:?}",
            rgb(&px, at(320, 180))
        );
        assert!(
            grey(rgb(&px, at(40, 40))),
            "{engine:?} plate: {:?}",
            rgb(&px, at(40, 40))
        );
    }
    let o = crate::blender::Options::new(None, None, 1, p.size).unwrap();
    let (desc, _) = crate::blender::describe(
        &p,
        &p.scenes[0],
        &[0.],
        &Assets::default(),
        std::path::Path::new("."),
        &o,
    )
    .unwrap();
    let ids: Vec<&str> = desc.layers.iter().map(|l| l.id.as_str()).collect();
    assert_eq!(ids, ["card"], "Blender leaves the overlay to mui-cut");
    assert!(
        p.scenes[0].composites_over_blender() && !under.scenes[0].effects.is_empty(),
        "mui-cut composites over Blender's frames"
    );
    under.scenes[0].effects.clear();
    assert!(!under.scenes[0].composites_over_blender());
    p.scenes[0].effects.clear();
    assert!(
        p.scenes[0].composites_over_blender(),
        "an overlay alone still needs the GPU pass over Blender"
    );
}

/// A layer's own effects run in 3D, on its slab: levels greys a red card
/// and a blur spreads it past its edge, onto room the slab grows for it,
/// in plain frames and beauty samples. The others stay sharp.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_layers_effects_run_on_its_slab_with_room_to_spread() {
    let p = scene3d(
        r##"{"id":"card","kind":"rect","x":200,"y":180,"width":200,"height":160,
            "fill":"#e03020","effects":[{"type":"levels","saturation":0},
            {"type":"blur","radius":6}]},
           {"id":"plain","kind":"rect","x":480,"y":180,"width":120,"height":120,
            "fill":"#e03020"}"##,
    );
    let at = |x: usize, y: usize| (y * 640 + x) * 4;
    let rgb = |px: &[u8], i: usize| [0, 1, 2].map(|k| i32::from(px[i + k]));
    for engine in [Engine::Classic, Engine::Sparse] {
        let Some(mut g) = offline(&p, engine) else {
            return;
        };
        let f = eval(&p, &p.scenes[0], 0.);
        for beauty in [false, true] {
            g.beauty = beauty;
            let px = frame(&mut g, &vec![f.clone(); if beauty { 4 } else { 1 }]);
            assert!(g.canvas.notice().is_empty(), "{}", g.canvas.notice());
            let [r, gr, b] = rgb(&px, at(200, 180));
            assert!(
                (r - gr).abs() <= 3 && (gr - b).abs() <= 3 && r > 20,
                "{engine:?} {beauty}: levels greyed the card: {r} {gr} {b}"
            );
            let bg = rgb(&px, at(630, 10))[0];
            // The card ends at x 300: blurred, it fades out past it.
            let (inside, edge, out) = (
                rgb(&px, at(290, 180))[0],
                rgb(&px, at(300, 180))[0],
                rgb(&px, at(308, 180))[0],
            );
            assert!(
                inside > edge && edge > out && out > bg + 6,
                "{engine:?} {beauty}: blur spreads: {inside} {edge} {out}"
            );
            // The plain card's edge (x 540) stays sharp and red.
            let [r, gr, _] = rgb(&px, at(536, 180));
            assert!(r > gr + 80, "{engine:?} {beauty}: plain card {r} {gr}");
            let past = rgb(&px, at(546, 180))[0];
            assert!(past <= bg + 2, "{engine:?} {beauty}: sharp: {past} on {bg}");
        }
    }
}

#[test]
fn a_material_round_trips_keys_binds_and_fits_the_schema() {
    let layer = |roughness: &str| {
        format!(
            r##"{{"size":[640,360],"fps":30,{{VARS}}"scenes":[{{"name":"a","duration":2,"mode":"3d",
            "layers":[{{"id":"pane","kind":"rect","extrude":8,"material":{{"metallic":0.1,
            "roughness":{roughness},"transmission":[{{"t":0,"v":0}},{{"t":1,"v":1}}],
            "ior":1.45,"thickness":12,"dispersion":0.4,"tint":"#e0f0ff"}}}}]}}]}}"##
        )
    };
    let bound = layer(r#"{"var":"rough"}"#).replace(
        "{VARS}",
        r#""variables":{"rough":{"type":"number","value":0.3}},"#,
    );
    let plain = layer("0.3").replace("{VARS}", "");
    let schema = Project::json_schema();
    let v = jsonschema::validator_for(&schema).unwrap();
    for src in [&bound, &plain] {
        let doc: serde_json::Value = serde_json::from_str(src).unwrap();
        let errs: Vec<String> = v.iter_errors(&doc).map(|e| e.to_string()).collect();
        assert!(errs.is_empty(), "{errs:?}");
    }
    let p = Project::load(&bound).unwrap();
    let again = Project::load(&Project::load(&plain).unwrap().to_json()).unwrap();
    assert_eq!(again.scenes, Project::load(&plain).unwrap().scenes);
    assert!(again.to_json().contains("\"material\""));
    let l = &p.scenes[0].layers[0];
    assert!(matches!(
        l.prop("material.transmission"),
        Some(Anim::Keys(_))
    ));
    assert!(l.props_in(true).iter().any(|(n, _)| n == "material.tint"));
    let m = eval(&p, &p.scenes[0], 1.).layers[0].space.material.unwrap();
    assert_eq!(m.roughness, Some(0.3), "bound");
    assert_eq!(m.transmission, Some(1.), "keyed");
    assert!(m.glass());
    let stage = m.over(mui_stage::Material::SLAB);
    assert_eq!(
        (stage.ior, stage.thickness, stage.dispersion),
        (1.45, 12., 0.4)
    );
    assert!(stage.tint[0] < stage.tint[2] && stage.tint[2] > 0.99);
    // Left out is the slab's; a misspelling is an error that says where.
    let bare = three::Surface::default().over(mui_stage::Material::SLAB);
    assert_eq!(bare, mui_stage::Material::SLAB);
    let e = Project::load(&plain.replace("\"ior\"", "\"iorr\"")).unwrap_err();
    assert!(e.contains("material") && e.contains("iorr"), "{e}");
    // Checks clean: a 3D layer's material is not an ignored property.
    let issues = check::check(&bound, &mut Renderer::new(64, 36), &|_| true);
    assert!(
        !issues.iter().any(|i| i.path.contains("material")),
        "{issues:?}"
    );
}

/// A one-triangle glTF binary whose material uses `extensions`.
fn glb_with(extensions: &str) -> Vec<u8> {
    let json = format!(
        r#"{{"asset":{{"version":"2.0"}},"scene":0,"scenes":[{{"nodes":[0]}}],
        "nodes":[{{"mesh":0}}],"meshes":[{{"primitives":[{{"attributes":{{"POSITION":0}},"material":0}}]}}],
        "materials":[{{"pbrMetallicRoughness":{{"metallicFactor":0,"roughnessFactor":0.1}},
        "extensions":{{{extensions}}}}}],
        "accessors":[{{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[1,1,0]}}],
        "bufferViews":[{{"buffer":0,"byteLength":36}}],"buffers":[{{"byteLength":36}}]}}"#
    );
    let mut j = json.into_bytes();
    j.resize(j.len().next_multiple_of(4), b' ');
    let bin: Vec<u8> = [0f32, 0., 0., 1., 0., 0., 0., 1., 0.]
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    let mut out = Vec::new();
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&((12 + 8 + j.len() + 8 + bin.len()) as u32).to_le_bytes());
    out.extend_from_slice(&(j.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&j);
    out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    out.extend_from_slice(b"BIN\0");
    out.extend_from_slice(&bin);
    out
}

#[test]
fn a_gltf_glass_material_reads_its_extensions() {
    let m = three::glb(&glb_with(
        r#""KHR_materials_transmission":{"transmissionFactor":1},
        "KHR_materials_ior":{"ior":1.7},
        "KHR_materials_volume":{"thicknessFactor":2,"attenuationDistance":4,"attenuationColor":[0.25,1,1]},
        "KHR_materials_dispersion":{"dispersion":0.5}"#,
    ))
    .unwrap();
    let g = m.parts[0].material;
    assert_eq!((g.transmission, g.ior, g.thickness), (1., 1.7, 2.));
    assert_eq!((g.dispersion, g.roughness), (0.5, 0.1));
    // Half the attenuation distance keeps the square root of its colour.
    assert!(
        (g.tint[0] - 0.5).abs() < 1e-5 && g.tint[1] == 1.,
        "{:?}",
        g.tint
    );
    // Without them it is opaque, the stage's defaults.
    let plain = three::glb(&glb_with("")).unwrap().parts[0].material;
    assert_eq!((plain.transmission, plain.ior), (0., 1.5));
}

/// A solid `w` x `h` PNG of `rgba`.
#[cfg(not(target_arch = "wasm32"))]
fn solid_png(w: u32, h: u32, rgba: [u8; 4]) -> Vec<u8> {
    let px: Vec<u8> = (0..w * h).flat_map(|_| rgba).collect();
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.write_header().unwrap().write_image_data(&px).unwrap();
    out
}

/// A 3D plugin layer whose one part is dark (opaque or see-through) over a
/// dark backdrop, exploded, extruded and lit as the KURV example is:
/// nothing behind the part may show through lighter than the part is.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn nothing_lighter_shows_behind_a_dark_exploded_part() {
    for (alpha, name) in [(255u8, "opaque"), (140, "translucent")] {
        let p = scene3d(
            r##"{"id":"key","kind":"light","fill":"#f2f2f2","rx":48,"ry":-35,"softness":2},
               {"id":"fill","kind":"light","type":"ambient","fill":"#c8c8d0","intensity":0.45},
               {"id":"syn","kind":"plugin","source":{"bin":"x"},"x":320,"y":180,"scale":2,
                "ry":20,"rx":8,"explode":0.5,"backdrop":0.35,"extrude":4}"##,
        );
        let key = eval(&p, &p.scenes[0], 0.).layers[2]
            .plugin
            .clone()
            .unwrap()
            .state;
        let cap = serde_json::json!({"width": 200, "height": 100,
        "parts": [{"path": "a", "id": "a", "frame": [10, 10, 180, 80]}],
        "layers": [
            {"group": "background", "rect": [0, 0, 200, 100], "src": "img/bg.png"},
            {"group": "a", "rect": [10, 10, 180, 80], "src": "img/a.png"},
        ]});
        let Some(mut g) = offline(&p, Engine::Classic) else {
            return;
        };
        g.assets
            .add_asset(
                &format!("{}/{key}.json", plugin::CACHE),
                cap.to_string().as_bytes(),
            )
            .unwrap();
        g.assets
            .add_asset(
                &format!("{}/img/bg.png", plugin::CACHE),
                &solid_png(200, 100, [18, 18, 20, 255]),
            )
            .unwrap();
        g.assets
            .add_asset(
                &format!("{}/img/a.png", plugin::CACHE),
                &solid_png(180, 80, [40, 40, 46, alpha]),
            )
            .unwrap();
        let px = frame(&mut g, &[eval(&p, &p.scenes[0], 0.)]);
        // Lit, the part reads a little lighter than its 40; the backdrop
        // is darker still. A plate, a slab body or a light edge would not.
        let lightest = px
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| c[0].max(c[1]).max(c[2]))
            .max();
        assert!(lightest < Some(72), "{name}: {lightest:?}");
    }
}

/// `examples/toy.glb`: a cube with base-colour, normal and
/// metallic-roughness PNGs turning once about y over two seconds, and
/// beside it a two-joint skinned bar whose upper joint bends 60 degrees
/// and back. Tiny, built here so the example's model has a source.
fn toy_glb() -> Vec<u8> {
    use serde_json::{Value, json};
    #[derive(Default)]
    struct Gb {
        bin: Vec<u8>,
        views: Vec<Value>,
        accessors: Vec<Value>,
    }
    impl Gb {
        fn view(&mut self, bytes: &[u8]) -> usize {
            while !self.bin.len().is_multiple_of(4) {
                self.bin.push(0);
            }
            self.views.push(
                json!({"buffer": 0, "byteOffset": self.bin.len(), "byteLength": bytes.len()}),
            );
            self.bin.extend_from_slice(bytes);
            self.views.len() - 1
        }
        fn acc(&mut self, bytes: &[u8], comp: u32, count: usize, ty: &str) -> usize {
            let v = self.view(bytes);
            self.accessors
                .push(json!({"bufferView": v, "componentType": comp, "count": count, "type": ty}));
            self.accessors.len() - 1
        }
        fn shorts(&mut self, data: &[u16]) -> usize {
            let b: Vec<u8> = data.iter().flat_map(|i| i.to_le_bytes()).collect();
            self.acc(&b, 5123, data.len(), "SCALAR")
        }
        fn floats(&mut self, data: &[f32], ty: &str, n: usize, bounds: bool) -> usize {
            let b: Vec<u8> = data.iter().flat_map(|f| f.to_le_bytes()).collect();
            let a = self.acc(&b, 5126, data.len() / n, ty);
            if bounds {
                let (lo, hi): (Vec<f32>, Vec<f32>) = (0..n)
                    .map(|k| {
                        let c = data.iter().skip(k).step_by(n);
                        (
                            c.clone().fold(f32::MAX, |a, &b| a.min(b)),
                            c.fold(f32::MIN, |a, &b| a.max(b)),
                        )
                    })
                    .unzip();
                self.accessors[a]["min"] = json!(lo);
                self.accessors[a]["max"] = json!(hi);
            }
            a
        }
    }
    let png = |w: u32, h: u32, px: Vec<u8>| {
        let mut out = Vec::new();
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(png::ColorType::Rgba);
        enc.write_header().unwrap().write_image_data(&px).unwrap();
        out
    };
    let checker = |n: u32, a: [u8; 4], b: [u8; 4]| {
        let px = (0..n * n)
            .flat_map(|i| {
                if (i % n + i / n).is_multiple_of(2) {
                    a
                } else {
                    b
                }
            })
            .collect();
        png(n, n, px)
    };
    let mut gb = Gb::default();
    // The cube: four corners per face, wound counter-clockwise outward.
    let (mut pos, mut nor, mut uv, mut idx) = (vec![], vec![], vec![], vec![]);
    for n in [
        [1., 0., 0.],
        [-1., 0., 0.],
        [0., 1., 0.],
        [0., -1., 0.],
        [0., 0., 1.],
        [0., 0., -1.],
    ] {
        let v: [f32; 3] = if n[1] == 0. {
            [0., 1., 0.]
        } else {
            [0., 0., -n[1]]
        };
        let u = [
            v[1] * n[2] - v[2] * n[1],
            v[2] * n[0] - v[0] * n[2],
            v[0] * n[1] - v[1] * n[0],
        ];
        let base = (pos.len() / 3) as u16;
        for (su, sv) in [(-1., -1.), (1., -1.), (1., 1.), (-1., 1.)] {
            for k in 0..3 {
                pos.push(0.5 * (n[k] + su * u[k] + sv * v[k]));
            }
            nor.extend(n);
            uv.extend([(su + 1.) / 2., (1. - sv) / 2.]);
        }
        idx.extend([0, 1, 2, 0, 2, 3].map(|i| base + i));
    }
    let cube_pos = gb.floats(&pos, "VEC3", 3, true);
    let cube_nor = gb.floats(&nor, "VEC3", 3, false);
    let cube_uv = gb.floats(&uv, "VEC2", 2, false);
    let cube_idx = gb.shorts(&idx);
    // The bar: square rings at y -0.5, 0.5, 1.5, x about 1.5, bound to
    // joint 0 at the foot, half and half at the knee, joint 1 at the top.
    let (mut pos, mut nor, mut joints, mut weights, mut idx) =
        (vec![], vec![], vec![], vec![], vec![]);
    for (ring, y) in [-0.5f32, 0.5, 1.5].into_iter().enumerate() {
        for [dx, dz] in [[1., 1.], [1., -1.], [-1., -1.], [-1., 1.]] {
            pos.extend([1.5 + 0.15 * dx, y, 0.15 * dz]);
            let l = std::f32::consts::FRAC_1_SQRT_2;
            nor.extend([dx * l, 0., dz * l]);
            joints.extend([0u8, 1, 0, 0]);
            let w = 0.5 * ring as f32;
            weights.extend([1. - w, w, 0., 0.]);
        }
    }
    for ring in 0..2u16 {
        for side in 0..4u16 {
            let (a, b) = (ring * 4 + side, ring * 4 + (side + 1) % 4);
            idx.extend([a, a + 4, b, b, a + 4, b + 4]);
        }
    }
    let bar_pos = gb.floats(&pos, "VEC3", 3, true);
    let bar_nor = gb.floats(&nor, "VEC3", 3, false);
    let bar_w = gb.floats(&weights, "VEC4", 4, false);
    let bar_j = gb.acc(&joints, 5121, joints.len() / 4, "VEC4");
    let bar_idx = gb.shorts(&idx);
    let ibm: Vec<f32> = [[-1.5, 0.5, 0.], [-1.5, -0.5, 0.]]
        .iter()
        .flat_map(|t| {
            [
                1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., t[0], t[1], t[2], 1.,
            ]
        })
        .collect();
    let ibm = gb.floats(&ibm, "MAT4", 16, false);
    // Keys: the cube a quarter turn every half second, the knee 60 degrees
    // at one second.
    let times: Vec<f32> = vec![0., 0.5, 1., 1.5, 2.];
    let turn: Vec<f32> = (0..5)
        .flat_map(|i| {
            let h = i as f32 * std::f32::consts::FRAC_PI_4;
            [0., h.sin(), 0., h.cos()]
        })
        .collect();
    let knee_t: Vec<f32> = vec![0., 1., 2.];
    let h = 30f32.to_radians();
    let knee: Vec<f32> = vec![0., 0., 0., 1., 0., 0., h.sin(), h.cos(), 0., 0., 0., 1.];
    let t5 = gb.floats(&times, "SCALAR", 1, true);
    let turn = gb.floats(&turn, "VEC4", 4, false);
    let t3 = gb.floats(&knee_t, "SCALAR", 1, true);
    let knee = gb.floats(&knee, "VEC4", 4, false);
    // Maps: orange and cream checks, a bumpy normal checker, and shiny
    // metal squares on rough plastic.
    let pngs = [
        checker(8, [255, 140, 30, 255], [250, 240, 220, 255]),
        checker(4, [128, 128, 255, 255], [190, 128, 220, 255]),
        checker(2, [0, 200, 0, 255], [0, 60, 255, 255]),
    ];
    let images: Vec<Value> = pngs
        .iter()
        .map(|p| json!({"bufferView": gb.view(p), "mimeType": "image/png"}))
        .collect();
    let Gb {
        mut bin,
        views,
        accessors,
    } = gb;
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
    let doc = json!({
        "asset": {"version": "2.0", "generator": "mui-cut tests3d::toy_glb"},
        "scene": 0,
        "scenes": [{"nodes": [0, 1, 2]}],
        "nodes": [
            {"name": "cube", "mesh": 0},
            {"name": "bar", "mesh": 1, "skin": 0},
            {"name": "foot", "translation": [1.5, -0.5, 0.], "children": [3]},
            {"name": "knee", "translation": [0., 1., 0.]}
        ],
        "skins": [{"joints": [2, 3], "inverseBindMatrices": ibm}],
        "meshes": [
            {"primitives": [{"attributes": {"POSITION": cube_pos, "NORMAL": cube_nor,
                "TEXCOORD_0": cube_uv}, "indices": cube_idx, "material": 0}]},
            {"primitives": [{"attributes": {"POSITION": bar_pos, "NORMAL": bar_nor,
                "JOINTS_0": bar_j, "WEIGHTS_0": bar_w}, "indices": bar_idx, "material": 1}]}
        ],
        "materials": [
            {"pbrMetallicRoughness": {"baseColorTexture": {"index": 0},
                "metallicRoughnessTexture": {"index": 2}}, "normalTexture": {"index": 1}},
            {"pbrMetallicRoughness": {"baseColorFactor": [0.1, 0.6, 0.55, 1.],
                "metallicFactor": 0., "roughnessFactor": 0.5}}
        ],
        "textures": [{"source": 0, "sampler": 0}, {"source": 1, "sampler": 0}, {"source": 2, "sampler": 0}],
        "samplers": [{"magFilter": 9728, "minFilter": 9728}],
        "images": images,
        "animations": [{"name": "toy", "samplers": [
            {"input": t5, "output": turn, "interpolation": "LINEAR"},
            {"input": t3, "output": knee, "interpolation": "LINEAR"}
        ], "channels": [
            {"sampler": 0, "target": {"node": 0, "path": "rotation"}},
            {"sampler": 1, "target": {"node": 3, "path": "rotation"}}
        ]}],
        "accessors": accessors,
        "bufferViews": views,
        "buffers": [{"byteLength": bin.len()}]
    });
    let mut text = serde_json::to_vec(&doc).unwrap();
    while !text.len().is_multiple_of(4) {
        text.push(b' ');
    }
    let total = 12 + 8 + text.len() + 8 + bin.len();
    let mut out = Vec::with_capacity(total);
    out.extend(b"glTF");
    out.extend(2u32.to_le_bytes());
    out.extend((total as u32).to_le_bytes());
    out.extend((text.len() as u32).to_le_bytes());
    out.extend(b"JSON");
    out.extend(text);
    out.extend((bin.len() as u32).to_le_bytes());
    out.extend(b"BIN\0");
    out.extend(bin);
    out
}

/// The example's model is what [`toy_glb`] builds (`UPDATE_GOLDEN=1`
/// rewrites it): its maps, nodes, animation and skin all read, and it
/// poses by time.
#[test]
fn a_gltf_reads_its_maps_animation_and_skin() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/toy.glb");
    let built = toy_glb();
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(path, &built).unwrap();
    }
    assert!(
        std::fs::read(path).unwrap() == built,
        "examples/toy.glb is stale: UPDATE_GOLDEN=1"
    );
    let mesh = three::glb(&built).unwrap();
    assert_eq!(mesh.parts.len(), 2);
    let (cube, bar) = (&mesh.parts[0], &mesh.parts[1]);
    assert_eq!(cube.maps, [Some(0), Some(1), Some(2)]);
    assert_eq!(cube.uvs.len(), cube.vertices.len());
    assert_eq!(mesh.images[0].size, [8, 8]);
    assert_eq!(&mesh.images[0].rgba[..4], &[255, 140, 30, 255]);
    assert!(bar.skin.is_some() && bar.maps == [None; 3]);
    assert!(mesh.animated());
    // A quarter turn about y at half a second takes +x to -z.
    let at = |t: Option<f64>, p: [f32; 3]| mesh.place(cube, &mesh.pose(t)).project(p);
    let close = |a: [f32; 3], b: [f32; 3]| (0..3).all(|i| (a[i] - b[i]).abs() < 1e-4);
    assert!(close(at(Some(0.5), [0.5, 0., 0.]), [0., 0., -0.5]));
    assert!(close(at(Some(0.25), [0.5, 0., 0.]), {
        let h = std::f32::consts::FRAC_PI_8;
        [0.5 * (2. * h).cos(), 0., -0.5 * (2. * h).sin()]
    }));
    assert!(close(at(None, [0.5, 0., 0.]), [0.5, 0., 0.]), "rest");
    assert!(
        close(at(Some(9.), [0.5, 0., 0.]), [0.5, 0., 0.]),
        "held at the end"
    );
    // The knee bends the top ring 60 degrees about (1.5, 0.5): the foot
    // stays, the top swings left.
    let bent = mesh.skinned(bar, &mesh.pose(Some(1.))).unwrap();
    let rest = mesh.skinned(bar, &mesh.pose(None)).unwrap();
    for i in 0..4 {
        assert!(close(
            bent[i][..3].try_into().unwrap(),
            rest[i][..3].try_into().unwrap()
        ));
    }
    let top: [f32; 3] = std::array::from_fn(|k| (8..12).map(|i| bent[i][k]).sum::<f32>() / 4.);
    let (s, c) = 60f32.to_radians().sin_cos();
    assert!(close(top, [1.5 - s, 0.5 + c, 0.]), "{top:?}");
    // Rest bounds: the cube and the straight bar.
    assert!(close(mesh.min, [-0.5, -0.5, -0.5]) && close(mesh.max, [1.65, 1.5, 0.5]));
    // A model layer's `time` offsets the scene's.
    let p = scene3d(r#"{"id":"m","kind":"model","path":"toy.glb","time":0.5}"#);
    assert_eq!(eval(&p, &p.scenes[0], 1.).layers[0].time, 1.5);
}

/// A model draws its maps and moves with its animation on the GPU.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_gltf_model_shows_its_maps_and_animates() {
    let p = scene3d(
        r#"{"id":"m","kind":"model","path":"toy.glb","x":320,"y":180,"height":240},
           {"id":"key","kind":"light","rx":-30,"ry":20},
           {"id":"fill","kind":"light","type":"ambient","intensity":0.6}"#,
    );
    for engine in [Engine::Classic, Engine::Sparse] {
        let Some(mut g) = offline(&p, engine) else {
            return;
        };
        g.assets.add_asset("toy.glb", &toy_glb()).unwrap();
        let at = |g: &mut Offline, t: f64| frame(g, &[eval(&p, &p.scenes[0], t)]);
        let (a, b) = (at(&mut g, 0.), at(&mut g, 0.3));
        assert!(g.canvas.notice().is_empty(), "{}", g.canvas.notice());
        // Orange checks: red well over blue.
        let orange = a
            .chunks(4)
            .filter(|p| i32::from(p[0]) > i32::from(p[2]) + 90 && p[1] > 40)
            .count();
        assert!(orange > 500, "{engine:?}: {orange} orange pixels");
        let moved = a
            .iter()
            .zip(&b)
            .filter(|(x, y)| x.abs_diff(**y) > 30)
            .count();
        assert!(moved > 2000, "{engine:?}: {moved} channels moved");
    }
}

/// A scene's `sky`: it fits the schema, puts the sun where `elevation` and
/// `azimuth` say, drifts its clouds with `wind`, and is what the camera
/// sees behind the layers instead of the background colour.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_sky_is_seen_behind_a_3d_scene() {
    let src = r##"{"size":[320,180],"fps":30,"scenes":[{"name":"a","duration":2,"mode":"3d",
        "background":"#000000","layers":[{"id":"cam","kind":"camera","rx":-20}],
        "sky":{"elevation":[{"t":0,"v":90},{"t":1,"v":30}],"azimuth":90,"cover":0.5,"wind":0.25}}]}"##;
    let doc: serde_json::Value = serde_json::from_str(src).unwrap();
    let schema = Project::json_schema();
    let v = jsonschema::validator_for(&schema).unwrap();
    let errs: Vec<String> = v.iter_errors(&doc).map(|e| e.to_string()).collect();
    assert!(errs.is_empty(), "{errs:?}");
    let p = Project::load(src).unwrap();
    assert_eq!(Project::load(&p.to_json()).unwrap().scenes, p.scenes);
    let sky = |t: f64| eval(&p, &p.scenes[0], t).view.unwrap().sky.unwrap();
    let up = sky(0.).sun;
    assert!(up[1] > 0.999, "straight up at 90: {up:?}");
    // 30 degrees up, 90 right of straight into the scene: world +x.
    let s = sky(1.);
    assert!((s.sun[0] - 0.866).abs() < 1e-3 && (s.sun[1] - 0.5).abs() < 1e-3);
    assert!(s.sun[2].abs() < 1e-3);
    assert_eq!(s.drift, [0.25, 0.]);
    let Some(mut g) = offline(&p, Engine::Classic) else {
        return;
    };
    let px = frame(&mut g, &[eval(&p, &p.scenes[0], 1.)]);
    // Looking up into it: blue sky and cloud, not the black background.
    let at = |x: usize, y: usize| &px[(y * 320 + x) * 4..][..3];
    let top = at(160, 10);
    assert!(top[2] > 60 && top[2] >= top[0], "sky up top: {top:?}");
    let lit = (0..320).map(|x| at(x, 60)[1]).collect::<Vec<_>>();
    let (lo, hi) = (lit.iter().min().unwrap(), lit.iter().max().unwrap());
    assert!(hi - lo > 20, "clouds across it: {lo}..{hi}");
}
