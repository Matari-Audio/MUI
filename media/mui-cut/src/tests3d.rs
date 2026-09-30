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
    assert!(matches!(l.prop("material.transmission"), Some(Anim::Keys(_))));
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
    assert!((g.tint[0] - 0.5).abs() < 1e-5 && g.tint[1] == 1., "{:?}", g.tint);
    // Without them it is opaque, the stage's defaults.
    let plain = three::glb(&glb_with("")).unwrap().parts[0].material;
    assert_eq!((plain.transmission, plain.ior), (0., 1.5));
}
