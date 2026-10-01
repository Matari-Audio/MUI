//! Needs a GPU adapter; with none (a bare CI box) each test says so and passes.
use super::*;
use mui_scene::prelude::*;

fn stage(w: u32, h: u32) -> Option<Stage> {
    Stage::new(w, h)
        .map_err(|e| eprintln!("no GPU, skipped: {e}"))
        .ok()
}

/// Left half white, right half black, `size` logical.
fn halves(size: Size) -> ResolvedScene {
    let root = row([
        block(size.width / 2., size.height).fill(Color::srgb(1., 1., 1.)),
        block(size.width / 2., size.height).fill(Color::srgb(0., 0., 0.)),
    ]);
    resolve(&SceneSpec::new(root)).expect("resolves")
}

#[test]
fn a_flat_front_plane_is_the_2d_render() {
    let Some(mut stage) = stage(320, 180) else {
        return;
    };
    let size = Size::new(160., 90.);
    stage.layer("l", &halves(size), size, 2.).unwrap();
    let f = stage
        .render(0., 0., 1, &|_| Shot {
            planes: vec![Plane::new("l", 160., 90.)],
            post: Post::NONE,
            ..Shot::new(Camera::front(90., 30.))
        })
        .unwrap();
    let px = |x: usize, y: usize| f.rgba[(y * 320 + x) * 4];
    // Two output pixels per unit: the seam sits on column 160.
    assert!(
        px(158, 90) > 0.99 && px(161, 90) < 0.01,
        "{} {}",
        px(158, 90),
        px(161, 90)
    );
    // Mid-edge, clear of the theme's rounded corners.
    assert!(
        px(80, 0) > 0.99 && px(240, 179) < 0.01 && px(0, 90) > 0.99,
        "fills the frame edge to edge: {} {}",
        px(80, 0),
        px(240, 179)
    );
}

#[test]
fn a_turned_slab_shows_its_wall_and_blur_is_deterministic() {
    let Some(mut stage) = stage(160, 90) else {
        return;
    };
    let size = Size::new(80., 45.);
    stage.layer("l", &halves(size), size, 1.).unwrap();
    let shot = |t: f64| Shot {
        planes: vec![
            Plane::new("l", 80., 45.)
                .scale(0.5)
                .rotate(0., 40. + t as f32 * 60., 0.)
                .depth(30.)
                .edge([1., 0., 0.]),
        ],
        post: Post::NONE,
        ..Shot::new(Camera::front(45., 30.))
    };
    let a = stage.render(0.5, 0.1, 6, &shot).unwrap();
    let b = stage.render(0.5, 0.1, 6, &shot).unwrap();
    assert_eq!(a.rgba, b.rgba);
    let red = a
        .rgba
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|p| p[0] > 0.3 && p[1] < 0.1 && p[2] < 0.1)
        .count();
    assert!(
        red > 20,
        "the red wall shows at a 40+ degree turn: {red} px"
    );
}

#[test]
fn a_background_that_does_not_compile_is_an_error_not_a_panic() {
    let Some(mut stage) = stage(8, 8) else { return };
    assert!(
        stage
            .background("fn background(uv: vec2f, t: f32) -> vec3f { oops }")
            .is_err()
    );
    stage
        .background("fn background(uv: vec2f, t: f32) -> vec3f { return vec3f(uv, 0.); }")
        .unwrap();
}

/// `n` one-unit stripes, white and black, a square `n` units a side.
fn stripes(n: usize) -> ResolvedScene {
    let root = row((0..n).map(|i| {
        block(1., n as f64).radius(0.).fill(if i % 2 == 0 {
            Color::srgb(1., 1., 1.)
        } else {
            Color::srgb(0., 0., 0.)
        })
    }));
    resolve(&SceneSpec::new(root)).expect("resolves")
}

#[test]
fn far_fine_detail_averages_to_its_light_not_to_moire() {
    let Some(mut stage) = stage(64, 64) else {
        return;
    };
    stage
        .layer("s", &stripes(256), Size::new(256., 256.), 2.)
        .unwrap();
    // 256 stripes squeezed into ~32 pixels: 8 stripes a pixel.
    let f = stage
        .render(0., 0., 1, &|_| Shot {
            planes: vec![Plane::new("s", 256., 256.).scale(0.125)],
            post: Post::NONE,
            ..Shot::new(Camera::front(64., 30.))
        })
        .unwrap();
    let row: Vec<f32> = (20..44).map(|x| f.rgba[(32 * 64 + x) * 4]).collect();
    // Half the light is sRGB 0.735, not the byte mean 0.5, and flat: no beats.
    for v in &row {
        assert!((v - 0.735).abs() < 0.04, "{row:?}");
    }
}

#[test]
fn the_floor_mirrors_a_slab_standing_on_it() {
    let Some(mut stage) = stage(96, 96) else {
        return;
    };
    let red = resolve(&SceneSpec::new(
        block(40., 40.).radius(0.).fill(Color::srgb(1., 0., 0.)),
    ))
    .unwrap();
    stage.layer("r", &red, Size::new(40., 40.), 1.).unwrap();
    let shot = |reflect: f32| {
        move |_| Shot {
            planes: vec![Plane::new("r", 40., 40.).at(0., 20., 0.)],
            floor: Some(Floor {
                reflect,
                color: [0., 0., 0.],
                ..Floor::at(0.)
            }),
            post: Post::NONE,
            ..Shot::new(Camera::front(96., 30.).orbit(0., 12.))
        }
    };
    let reds = |f: &Frame| {
        // Below the frame's middle: under the floor line, where only the
        // reflection can be red.
        (60..96)
            .flat_map(|y| (0..96).map(move |x| (y * 96 + x) * 4))
            .filter(|&i| f.rgba[i] > 0.2 && f.rgba[i + 1] < 0.1)
            .count()
    };
    let with = stage.render(0., 0., 1, &shot(1.)).unwrap();
    let without = stage.render(0., 0., 1, &shot(0.)).unwrap();
    assert!(
        reds(&with) > 50 && reds(&without) == 0,
        "{} {}",
        reds(&with),
        reds(&without)
    );
}

#[test]
fn depth_of_field_blurs_what_is_off_focus_only() {
    let Some(mut stage) = stage(160, 90) else {
        return;
    };
    let size = Size::new(80., 45.);
    stage.layer("l", &halves(size), size, 2.).unwrap();
    let mut seam = |focus: f32| {
        let f = stage
            .render(0., 0., 1, &|_| {
                let cam = Camera::front(45., 30.);
                Shot {
                    planes: vec![Plane::new("l", 80., 45.)],
                    post: Post {
                        focus: cam.distance() * focus,
                        aperture: 40.,
                        max_blur: 12.,
                        ..Post::NONE
                    },
                    ..Shot::new(cam)
                }
            })
            .unwrap();
        f.rgba[(45 * 160 + 76) * 4]
    };
    assert!(seam(1.) > 0.99, "in focus: sharp");
    assert!(seam(0.3) < 0.97, "focused far in front: the seam blurs");
}

#[test]
fn text_extrudes_from_its_own_glyphs() {
    let Some(mut stage) = stage(160, 90) else {
        return;
    };
    let font = Font::new(epaint_default_fonts::HACK_REGULAR).unwrap();
    let plane = stage
        .text_layer("t", &[font], "MUI", 40., Color::srgb(1., 1., 1.), 4., 2.)
        .unwrap();
    let contours = plane
        .outline
        .as_ref()
        .unwrap()
        .flatten(0.5, 1 << 16)
        .unwrap();
    assert!(contours.len() >= 3, "one contour a letter at least");
    let [w, h] = plane.size;
    let f = stage
        .render(0., 0., 1, &|_| Shot {
            planes: vec![
                plane
                    .clone()
                    .depth(12.)
                    .rotate(0., 35., 0.)
                    .edge([0., 0., 1.]),
            ],
            post: Post::NONE,
            ..Shot::new(Camera::front(h * 1.4, 30.))
        })
        .unwrap();
    let white = f
        .rgba
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|p| p[0] > 0.9 && p[2] > 0.9)
        .count();
    let wall = f
        .rgba
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|p| p[2] > 0.2 && p[0] < 0.1)
        .count();
    assert!(
        white > 100 && wall > 20,
        "faces {white}, walls {wall} ({w}x{h})"
    );
}

#[test]
fn layers_of_different_sizes_share_one_renderer() {
    let Some(mut stage) = stage(320, 180) else {
        return;
    };
    let red = resolve(&SceneSpec::new(
        block(40., 40.).radius(0.).fill(Color::srgb(1., 0., 0.)),
    ))
    .unwrap();
    let size = Size::new(160., 90.);
    // Small then large, then small again: each keeps its own pixels.
    stage.layer("r", &red, Size::new(40., 40.), 1.).unwrap();
    stage.layer("l", &halves(size), size, 2.).unwrap();
    stage.layer("r", &red, Size::new(40., 40.), 1.).unwrap();
    let front = |id: &'static str| {
        move |_| Shot {
            planes: vec![Plane::new(id, 160., 90.)],
            post: Post::NONE,
            ..Shot::new(Camera::front(90., 30.))
        }
    };
    let r = stage.render(0., 0., 1, &front("r")).unwrap();
    let l = stage.render(0., 0., 1, &front("l")).unwrap();
    let at = |f: &Frame, x: usize, y: usize| -> [f32; 3] {
        let i = (y * 320 + x) * 4;
        [f.rgba[i], f.rgba[i + 1], f.rgba[i + 2]]
    };
    assert!(
        at(&r, 160, 90)[0] > 0.9 && at(&r, 160, 90)[1] < 0.1,
        "{:?}",
        at(&r, 160, 90)
    );
    assert!(
        at(&l, 80, 90)[1] > 0.99 && at(&l, 240, 90)[0] < 0.01,
        "{:?}",
        at(&l, 80, 90)
    );
}

#[test]
fn a_lost_device_is_an_error_not_a_panic() {
    let Some(mut stage) = stage(8, 8) else {
        return;
    };
    stage.device.destroy();
    let size = Size::new(4., 4.);
    assert!(matches!(
        stage.layer("l", &halves(size), size, 1.),
        Err(Error::DeviceLost)
    ));
}

#[test]
fn a_lit_card_casts_a_shadow_on_the_floor_the_same_every_time() {
    let Some(mut stage) = stage(160, 90) else {
        return;
    };
    let size = Size::new(80., 45.);
    stage.layer("l", &halves(size), size, 1.).unwrap();
    let shot = |shadows: bool, contact: f32| {
        let mut floor = Floor::at(-40.);
        floor.color = [0.5; 3];
        floor.reflect = 0.;
        floor.contact = contact;
        let mut sun = Light::new(LightKind::Directional);
        sun.direction = [0.6, -1., -0.3];
        sun.shadows = shadows;
        let mut ambient = Light::new(LightKind::Ambient);
        ambient.color = [0.2; 3];
        Shot {
            planes: vec![Plane::new("l", 80., 45.).at(0., -15., 0.).depth(4.)],
            lights: vec![ambient, sun],
            floor: Some(floor),
            clear: Some([0.; 3]),
            post: Post::NONE,
            ..Shot::new(Camera::front(90., 30.).orbit(0., 25.))
        }
    };
    let render = |stage: &mut Stage, shadows, contact| {
        stage
            .render(0., 0., 1, &|_| shot(shadows, contact))
            .unwrap()
            .rgba
    };
    let lit = render(&mut stage, false, 0.);
    let a = render(&mut stage, true, 0.);
    let b = render(&mut stage, true, 0.);
    assert_eq!(a, b, "a shadow map renders the same every time");
    let darker = |x: &[f32]| {
        x.as_chunks::<4>()
            .0
            .iter()
            .zip(lit.as_chunks::<4>().0)
            .filter(|(s, l)| l[1] - s[1] > 0.1)
            .count()
    };
    assert!(
        darker(&a) > 40,
        "the card shadows the floor: {}",
        darker(&a)
    );
    let contact = render(&mut stage, false, 1.);
    assert!(
        darker(&contact) > 40,
        "contact shadow: {}",
        darker(&contact)
    );
}

/// A model's base-colour map shows over its UVs, and a normal map tilts
/// its light.
#[test]
fn a_models_maps_colour_and_bend_its_surface() {
    let Some(mut stage) = stage(64, 64) else {
        return;
    };
    let v = |x: f32, y: f32| [x, y, 0., 0., 0., 1.];
    stage.mesh_uv(
        "quad",
        &[v(-30., -30.), v(30., -30.), v(30., 30.), v(-30., 30.)],
        &[[0., 1.], [1., 1.], [1., 0.], [0., 0.]],
        &[0, 1, 2, 0, 2, 3],
    );
    // Left red, right green (two texels each, so a repeating filter does
    // not wrap one into the other); a normal leaning 45 degrees along +u.
    let rg: Vec<u8> = [
        [255, 0, 0, 255],
        [255, 0, 0, 255],
        [0, 255, 0, 255],
        [0, 255, 0, 255],
    ]
    .concat();
    stage.texture("rg", &rg, [4, 1], true);
    stage.texture("lean", &[218, 128, 218, 255], [1, 1], false);
    let shot = |maps: [Option<String>; 3]| Shot {
        models: vec![Model {
            mesh: "quad".into(),
            transform: Mat4::IDENTITY,
            color: [1.; 4],
            material: Material {
                metallic: 0.,
                roughness: 1.,
                ..Material::SLAB
            },
            cast: false,
            receive: false,
            maps,
        }],
        lights: vec![Light {
            direction: [0., 0., -1.],
            ..Light::new(LightKind::Directional)
        }],
        clear: Some([0.; 3]),
        post: Post::NONE,
        ..Shot::new(Camera::front(64., 30.))
    };
    let mut px = |maps: [Option<String>; 3], x: usize| {
        let f = stage.render(0., 0., 1, &|_| shot(maps.clone())).unwrap();
        <[f32; 4]>::try_from(&f.rgba[(32 * 64 + x) * 4..][..4]).unwrap()
    };
    let (left, right) = (
        px([Some("rg".into()), None, None], 18),
        px([Some("rg".into()), None, None], 46),
    );
    assert!(left[0] > 0.8 && left[1] < 0.1, "left {left:?}");
    assert!(right[1] > 0.8 && right[0] < 0.1, "right {right:?}");
    let flat = px(Default::default(), 32)[0];
    let leant = px([None, Some("lean".into()), None], 32)[0];
    assert!(
        // cos 45 = 0.707 of the light, 0.858 once sRGB-encoded.
        (leant - 0.858 * flat).abs() < 0.03,
        "normal map: {leant} of {flat}"
    );
}

#[test]
fn a_model_draws_and_lights_from_its_mesh() {
    let Some(mut stage) = stage(64, 64) else {
        return;
    };
    // One triangle facing the viewer.
    let n = [0., 0., 1.];
    let v = |x: f32, y: f32| [x, y, 0., n[0], n[1], n[2]];
    stage.mesh(
        "tri",
        &[v(-20., -20.), v(20., -20.), v(0., 20.)],
        &[0, 1, 2],
    );
    let shot = |lights: Vec<Light>| Shot {
        models: vec![Model {
            mesh: "tri".into(),
            transform: Mat4::IDENTITY,
            color: [1., 0., 0., 1.],
            material: Material {
                metallic: 0.,
                roughness: 1.,
                ..Material::SLAB
            },
            cast: true,
            receive: true,
            maps: Default::default(),
        }],
        lights,
        clear: Some([0.; 3]),
        post: Post::NONE,
        ..Shot::new(Camera::front(64., 30.))
    };
    let mut key = Light::new(LightKind::Directional);
    key.direction = [0., 0., -1.];
    let f = stage.render(0., 0., 1, &|_| shot(vec![key])).unwrap();
    let centre = &f.rgba[(32 * 64 + 32) * 4..][..4];
    assert!(centre[0] > 0.9 && centre[1] < 0.05, "{centre:?}");
    let dark = stage.render(0., 0., 1, &|_| shot(vec![Light::new(LightKind::Ambient)]));
    assert!(dark.is_ok());
    assert!(matches!(
        stage.render(0., 0., 1, &|_| Shot {
            models: vec![Model {
                mesh: "nope".into(),
                ..shot(vec![]).models[0].clone()
            }],
            ..shot(vec![])
        }),
        Err(Error::MissingMesh(_))
    ));
}

#[test]
fn the_irradiance_of_a_constant_environment_is_that_constant() {
    let grey = EnvImage::from_fn(64, 32, |_| [0.7, 0.2, 1.5]);
    let sh = env::sh9(&grey);
    for n in [
        [0., 1., 0.],
        [0., -1., 0.],
        [1., 0., 0.],
        [0., 0., -1.],
        [0.6, 0.48, -0.64],
    ] {
        let e = env::irradiance(&sh, n);
        for (got, want) in e.into_iter().zip([0.7, 0.2, 1.5]) {
            assert!((got - want).abs() < want * 0.01, "{n:?}: {e:?}");
        }
    }
    // A sky lit from above only: the ground-facing side gets none of it,
    // the top all but the band limit.
    let sky = EnvImage::from_fn(64, 32, |d| [f32::from(u8::from(d[1] > 0.)); 3]);
    let sh = env::sh9(&sky);
    let (up, down) = (
        env::irradiance(&sh, [0., 1., 0.]),
        env::irradiance(&sh, [0., -1., 0.]),
    );
    assert!(up[0] > 0.9 && down[0] < 0.1, "{up:?} {down:?}");
}

#[test]
fn roughness_maps_linearly_onto_the_mip_chain() {
    let last = (env::LEVELS - 1) as f32;
    assert_eq!(env::mip_of_roughness(0.), 0.);
    assert_eq!(env::mip_of_roughness(1.), last);
    assert_eq!(env::mip_of_roughness(7.), last, "clamped");
    for m in 0..env::LEVELS {
        assert!((env::mip_of_roughness(env::roughness_of_mip(m)) - m as f32).abs() < 1e-6);
    }
    // Directions and texel coordinates round-trip, +z at u = 3/4.
    for d in [[0.6, 0.48, -0.64], [0., 0.8, 0.6], [-1., 0., 0.]] {
        let back = env::dir_of(env::uv_of(d));
        assert!(
            (0..3).all(|i| (back[i] - d[i]).abs() < 1e-5),
            "{d:?} {back:?}"
        );
    }
    assert!((env::uv_of([0., 0., 1.])[0] - 0.75).abs() < 1e-6);
    // The LUT: a smooth mirror seen head on reflects F0 whole; a rough
    // one at grazing loses most of it.
    let lut = env::brdf_lut();
    let n = env::LUT as usize;
    let [a, b] = lut[n - 1];
    assert!(a + b > 0.9 && a + b < 1.05, "{a} {b}");
    let [a, b] = lut[n * n - 1];
    assert!(a + b < 0.5, "{a} {b}");
}

/// An environment that is bright only to the viewer's +z side, whose
/// light a flat mirror facing the viewer throws back.
fn front_light(d: [f32; 3]) -> [f32; 3] {
    [if d[2] > 0.5 { 1. } else { 0.02 }; 3]
}

fn quad(stage: &mut Stage) {
    let n = [0., 0., 1.];
    let v = |x: f32, y: f32| [x, y, 0., n[0], n[1], n[2]];
    stage.mesh(
        "quad",
        &[v(-30., -30.), v(30., -30.), v(30., 30.), v(-30., 30.)],
        &[0, 1, 2, 0, 2, 3],
    );
}

#[test]
fn a_metal_lit_only_by_the_environment_mirrors_it() {
    let Some(mut stage) = stage(64, 64) else {
        return;
    };
    quad(&mut stage);
    stage.environment("front", &EnvImage::from_fn(128, 64, front_light));
    let shot = |env: Option<Environment>| Shot {
        models: vec![Model {
            mesh: "quad".into(),
            transform: Mat4::IDENTITY,
            color: [1., 0.8, 0.4, 1.],
            material: Material {
                metallic: 1.,
                roughness: 0.2,
                ..Material::SLAB
            },
            cast: false,
            receive: false,
            maps: Default::default(),
        }],
        // Ambient only, so without the environment the metal is lit.
        lights: vec![Light {
            color: [0.1; 3],
            ..Light::new(LightKind::Ambient)
        }],
        environment: env,
        clear: Some([0.; 3]),
        post: Post::NONE,
        ..Shot::new(Camera::front(64., 30.))
    };
    let mut centre = |env: Option<Environment>| {
        let f = stage.render(0., 0., 1, &|_| shot(env.clone())).unwrap();
        f.rgba[(32 * 64 + 32) * 4..][..3].to_vec()
    };
    let bare = centre(None);
    let env = Environment {
        map: "front".into(),
        ..Environment::studio()
    };
    let lit = centre(Some(env.clone()));
    let turned = centre(Some(Environment {
        rotation: 180.,
        ..env.clone()
    }));
    // It mirrors the bright side, tinted by its colour, and turning the
    // environment away takes it back.
    assert!(
        lit[0] > bare[0] + 0.3 && lit[0] > lit[2] + 0.1,
        "{lit:?} vs {bare:?}"
    );
    assert!(turned[0] < lit[0] - 0.3, "{turned:?} vs {lit:?}");
    // Intensity scales it; the studio needs no upload.
    let dim = centre(Some(Environment {
        intensity: 0.25,
        ..env
    }));
    assert!(dim[0] < lit[0] - 0.1, "{dim:?}");
    assert!(
        stage
            .render(0., 0., 1, &|_| shot(Some(Environment::studio())))
            .is_ok()
    );
    assert!(matches!(
        stage.render(0., 0., 1, &|_| shot(Some(Environment {
            map: "nope".into(),
            ..Environment::studio()
        }))),
        Err(Error::MissingEnvironment(_))
    ));
}

/// A 40-unit cube standing on the floor at the origin.
fn cube(stage: &mut Stage) {
    let mut v = Vec::new();
    let mut idx = Vec::new();
    for axis in 0..3 {
        for side in [-1f32, 1.] {
            let mut n = [0.; 3];
            n[axis] = side;
            let (a, b) = ((axis + 1) % 3, (axis + 2) % 3);
            let base = v.len() as u32;
            for (s, t) in [(-1., -1.), (1., -1.), (1., 1.), (-1., 1.)] {
                let mut p = [0f32; 3];
                p[axis] = side * 20.;
                p[a] = s * 20.;
                p[b] = t * 20.;
                p[1] += 20.;
                v.push([p[0], p[1], p[2], n[0], n[1], n[2]]);
            }
            idx.extend([0, 1, 2, 0, 2, 3].map(|k| base + k));
        }
    }
    stage.mesh("cube", &v, &idx);
}

#[test]
fn ambient_occlusion_darkens_a_contact_crease_not_an_open_floor() {
    let (w, h) = (160u32, 120u32);
    let Some(mut stage) = stage(w, h) else {
        return;
    };
    cube(&mut stage);
    let camera = Camera {
        eye: [60., 140., 220.],
        target: [0., 10., 0.],
        fov: 40.,
        roll: 0.,
    };
    let shot = |ao: Option<Ao>, reflect: f32| {
        let mut floor = Floor::at(0.);
        floor.color = [0.5; 3];
        floor.reflect = reflect;
        Shot {
            models: vec![Model {
                mesh: "cube".into(),
                transform: Mat4::IDENTITY,
                color: [0.5, 0.5, 0.5, 1.],
                material: Material {
                    metallic: 0.,
                    roughness: 1.,
                    ..Material::SLAB
                },
                cast: false,
                receive: false,
                maps: Default::default(),
            }],
            lights: vec![Light::new(LightKind::Ambient)],
            floor: Some(floor),
            clear: Some([0.; 3]),
            ao,
            post: Post::NONE,
            ..Shot::new(camera)
        }
    };
    let render = |stage: &mut Stage, ao, reflect| {
        stage
            .render(0., 0., 1, &|_| shot(ao, reflect))
            .unwrap()
            .rgba
    };
    let luma = |stage: &mut Stage, ao| render(stage, ao, 0.);
    let plain = luma(&mut stage, None);
    let ao = Some(Ao {
        strength: 1.,
        radius: 40.,
    });
    let a = luma(&mut stage, ao);
    assert_eq!(a, luma(&mut stage, ao), "the same every time");
    let vp = camera.view_proj(w as f32 / h as f32);
    let px = |p: [f32; 3]| {
        let q = vp.project(p);
        let (x, y) = (
            ((q[0] + 1.) * 0.5 * w as f32) as usize,
            ((1. - q[1]) * 0.5 * h as f32) as usize,
        );
        (y * w as usize + x) * 4
    };
    // On the floor just in front of the cube's foot, and well clear of it.
    let crease = px([0., 0., 23.]);
    let open = px([-70., 0., 60.]);
    assert!(
        plain[crease] > 0.2 && (plain[crease] - plain[open]).abs() < 0.05,
        "flat light: {} {}",
        plain[crease],
        plain[open]
    );
    assert!(
        a[crease] < plain[crease] * 0.9,
        "the crease darkens: {} -> {}",
        plain[crease],
        a[crease]
    );
    assert!(
        (a[open] - plain[open]).abs() < plain[open] * 0.03,
        "the open floor does not: {} -> {}",
        plain[open],
        a[open]
    );
    // A mirror image lies below the floor; seen as depth it would be a pit
    // whose rim the floor around it is occluded by. It darkens no more
    // than a matte floor does.
    let dark = |plain: &[f32], a: &[f32]| -> f32 { plain.iter().zip(a).map(|(p, a)| p - a).sum() };
    let matte = dark(&plain, &a);
    let shiny = dark(
        &render(&mut stage, None, 0.35),
        &render(&mut stage, ao, 0.35),
    );
    assert!(shiny < matte * 1.015, "matte {matte}, reflective {shiny}");
}

/// Every mip level of an `Rgba16Float` texture, as linear RGBA.
fn read_levels(stage: &Stage, tex: &wgpu::Texture) -> Vec<(u32, u32, Vec<[f32; 4]>)> {
    (0..tex.mip_level_count())
        .map(|m| {
            let (w, h) = ((tex.width() >> m).max(1), (tex.height() >> m).max(1));
            let row = (w * 8).next_multiple_of(256);
            let buf = stage.device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: u64::from(row * h),
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            let mut enc = stage
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            enc.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo {
                    texture: tex,
                    mip_level: m,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyBufferInfo {
                    buffer: &buf,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(row),
                        rows_per_image: None,
                    },
                },
                wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
            );
            stage.queue.submit([enc.finish()]);
            buf.slice(..).map_async(wgpu::MapMode::Read, |_| {});
            stage
                .device
                .poll(wgpu::PollType::wait_indefinitely())
                .unwrap();
            let bytes = buf.slice(..).get_mapped_range().unwrap();
            let px = bytes
                .chunks_exact(row as usize)
                .flat_map(|r| r[..w as usize * 8].as_chunks::<8>().0.to_vec())
                .map(|p| {
                    std::array::from_fn(|i| {
                        half::f16::from_le_bytes([p[2 * i], p[2 * i + 1]]).to_f32()
                    })
                })
                .collect();
            (w, h, px)
        })
        .collect()
}

#[test]
fn the_gpu_prefilter_matches_the_cpu_reference() {
    let Some(stage) = stage(8, 8) else { return };
    // The studio, and a small hot sun over a sky: the hard case for a
    // filtered-importance-sampled convolution.
    let sun = EnvImage::from_fn(1024, 512, |d| {
        let hot = d[0] * 0.6 + d[1] * 0.8 > 0.995;
        [if hot { 40. } else { 0.1 + 0.3 * d[1].max(0.) }; 3]
    });
    for img in [EnvImage::studio(), sun] {
        let cpu = env::prefilter(&img);
        let (tex, sh) = stage.prefilter(&img);
        assert_eq!(sh, cpu.sh);
        let gpu = read_levels(&stage, &tex);
        assert_eq!(gpu.len(), cpu.levels.len());
        for (m, ((w, h, g), c)) in gpu.iter().zip(&cpu.levels).enumerate() {
            assert_eq!((*w, *h), (c.width, c.height), "level {m}");
            // Half floats, and the GPU's filtering weights: within a few
            // percent of the level's own brightness scale.
            let scale = c.rgb.iter().map(|p| p[0]).fold(0f32, f32::max).max(1e-3);
            let (mut worst, mut sum) = (0f32, 0f32);
            for (a, b) in g.iter().zip(&c.rgb) {
                let e = (0..3).map(|i| (a[i] - b[i]).abs()).fold(0., f32::max) / scale;
                worst = worst.max(e);
                sum += e;
            }
            let mean = sum / c.rgb.len() as f32;
            assert!(
                worst < 0.05 && mean < 0.005,
                "level {m}: worst {worst}, mean {mean}"
            );
        }
    }
}

/// White, opaque, `size` logical.
fn white(size: Size) -> ResolvedScene {
    let root = block(size.width, size.height)
        .radius(0.)
        .fill(Color::srgb(1., 1., 1.));
    resolve(&SceneSpec::new(root)).expect("resolves")
}

/// Pixels of a row whose value lies strictly between its two plateaus.
fn ramp(row: &[f32]) -> usize {
    let (lo, hi) = row
        .iter()
        .fold((f32::MAX, f32::MIN), |(a, b), &v| (a.min(v), b.max(v)));
    let k = (hi - lo) * 0.1;
    row.iter().filter(|&&v| v > lo + k && v < hi - k).count()
}

#[test]
fn a_beauty_penumbra_widens_with_the_light_softness() {
    let Some(mut stage) = stage(128, 128) else {
        return;
    };
    stage
        .layer("l", &white(Size::new(64., 64.)), Size::new(64., 64.), 1.)
        .unwrap();
    // A card 100 units over the floor covering x < 0, a sun leaning +x:
    // the shadow's edge falls at x = 30, under the camera looking down.
    let mut penumbra = |softness: f32| {
        let f = stage
            .beauty(0., 0., 32, &|_| {
                let mut floor = Floor::at(0.);
                floor.color = [0.5; 3];
                floor.reflect = 0.;
                let mut sun = Light::new(LightKind::Directional);
                sun.direction = [0.3, -1., 0.];
                sun.softness = softness;
                Shot {
                    planes: vec![
                        Plane::new("l", 400., 400.)
                            .rotate(-90., 0., 0.)
                            .at(-200., 100., 0.),
                    ],
                    lights: vec![sun],
                    floor: Some(floor),
                    clear: Some([0.; 3]),
                    post: Post::NONE,
                    ..Shot::new(Camera {
                        eye: [30., 300., 0.],
                        target: [30., 0., 0.],
                        fov: 30.,
                        roll: 0.,
                    })
                }
            })
            .unwrap();
        let row: Vec<f32> = (48..128).map(|x| f.rgba[(64 * 128 + x) * 4 + 1]).collect();
        ramp(&row)
    };
    let (hard, soft) = (penumbra(0.5), penumbra(6.));
    // A 9-degree sun 100 units up: ~16 units, ~13 pixels of penumbra, 10%
    // to 90% of it about half that.
    assert!(hard <= 3, "a small sun is nearly hard: {hard} px");
    assert!(soft >= 6 && soft >= 3 * hard, "{hard} px -> {soft} px");
}

#[test]
fn a_thin_lens_is_sharp_at_its_focus_and_blurs_off_it() {
    let Some(mut stage) = stage(160, 90) else {
        return;
    };
    let size = Size::new(80., 45.);
    stage.layer("l", &halves(size), size, 2.).unwrap();
    let mut seam = |focus: f32| {
        let f = stage
            .beauty(0., 0., 64, &|_| {
                let cam = Camera::front(45., 30.);
                Shot {
                    planes: vec![Plane::new("l", 80., 45.)],
                    post: Post {
                        focus: cam.distance() * focus,
                        aperture: 8.,
                        max_blur: 8.,
                        ..Post::NONE
                    },
                    ..Shot::new(cam)
                }
            })
            .unwrap();
        let row: Vec<f32> = (40..120).map(|x| f.rgba[(45 * 160 + x) * 4]).collect();
        ramp(&row)
    };
    let sharp = seam(1.);
    // Focused at half the distance: a blur radius of 8 * 0.5 / 1 = 4 px.
    let blurred = seam(0.5);
    assert!(sharp <= 2, "in focus: {sharp} px of ramp");
    assert!((4..=10).contains(&blurred), "off focus: {blurred} px");
}

#[test]
fn beauty_samples_antialias_an_edge_past_msaa() {
    let Some(mut stage) = stage(96, 96) else {
        return;
    };
    let size = Size::new(48., 48.);
    stage.layer("l", &white(size), size, 1.).unwrap();
    let shot = |_| Shot {
        planes: vec![Plane::new("l", 48., 48.).rotate(0., 0., 12.)],
        clear: Some([0.; 3]),
        post: Post::NONE,
        ..Shot::new(Camera::front(96., 30.))
    };
    let levels = |f: Frame| {
        let mut v: Vec<u8> = f
            .rgba8()
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| p[0])
            .filter(|&v| v > 4 && v < 251)
            .collect();
        v.sort_unstable();
        v.dedup();
        v.len()
    };
    let msaa = levels(stage.render(0., 0., 1, &shot).unwrap());
    let beauty = levels(stage.beauty(0., 0., 16, &shot).unwrap());
    assert!(msaa <= 3, "4x MSAA alone: {msaa} edge levels");
    assert!(beauty >= 8, "16 jittered samples: {beauty} edge levels");
    // And the same every time.
    let a = stage.beauty(0., 0., 16, &shot).unwrap().rgba;
    assert_eq!(a, stage.beauty(0., 0., 16, &shot).unwrap().rgba);
}

#[test]
fn a_slabs_walls_are_built_once_while_its_size_depth_and_outline_hold() {
    let Some(mut stage) = stage(160, 90) else {
        return;
    };
    let size = Size::new(80., 45.);
    stage.layer("l", &halves(size), size, 1.).unwrap();
    // A fresh outline every subframe, as mui-cut makes one: equal content
    // is the same walls.
    let shot = |depth: f32| {
        move |t: f64| {
            let ring = mui_geometry::Path::polyline(
                [(0., 0.), (80., 0.), (80., 45.), (0., 45.)]
                    .map(|(x, y)| mui_geometry::Point::new(x, y)),
                true,
            );
            Shot {
                planes: vec![
                    Plane::new("l", 80., 45.)
                        .rotate(0., 30. + t as f32 * 40., 0.)
                        .depth(depth)
                        .outline(Arc::new(ring)),
                ],
                post: Post::NONE,
                ..Shot::new(Camera::front(45., 30.))
            }
        }
    };
    stage.render(0.5, 0.1, 4, &shot(20.)).unwrap();
    stage.render(0.6, 0.1, 4, &shot(20.)).unwrap();
    assert_eq!(stage.walls_built, 1, "eight subframes, one set of walls");
    stage.render(0.7, 0.1, 2, &shot(30.)).unwrap();
    assert_eq!(stage.walls_built, 2, "a new depth builds new walls");
    assert_eq!(stage.walls.len(), 1, "the old ones are dropped");
}

#[test]
fn a_point_light_casts_soft_shadows_from_its_cube() {
    let Some(mut stage) = stage(128, 128) else {
        return;
    };
    stage
        .layer("l", &white(Size::new(64., 64.)), Size::new(64., 64.), 1.)
        .unwrap();
    // A lamp 200 units up at x = -60, a card covering x < 0 halfway down:
    // the shadow's edge falls on the floor at x = 60, which the camera
    // looking down sees right of the card.
    let shot = |shadows: bool, softness: f32| {
        let mut floor = Floor::at(0.);
        floor.color = [0.5; 3];
        floor.reflect = 0.;
        let mut lamp = Light::new(LightKind::Point);
        lamp.position = [-60., 200., 0.];
        lamp.shadows = shadows;
        lamp.softness = softness;
        Shot {
            planes: vec![
                Plane::new("l", 400., 400.)
                    .rotate(-90., 0., 0.)
                    .at(-200., 100., 0.),
            ],
            lights: vec![lamp],
            floor: Some(floor),
            clear: Some([0.; 3]),
            post: Post::NONE,
            ..Shot::new(Camera {
                eye: [0., 400., 0.],
                target: [0., 0., 0.],
                fov: 30.,
                roll: 0.,
            })
        }
    };
    let row =
        |f: &Frame| -> Vec<f32> { (0..128).map(|x| f.rgba[(64 * 128 + x) * 4 + 1]).collect() };
    let lit = stage.render(0., 0., 1, &|_| shot(false, 1.)).unwrap();
    let a = stage.render(0., 0., 1, &|_| shot(true, 1.)).unwrap();
    let b = stage.render(0., 0., 1, &|_| shot(true, 1.)).unwrap();
    assert_eq!(a.rgba, b.rgba, "the cube renders the same every time");
    let (lit, a) = (row(&lit), row(&a));
    // Left of the edge (x 60, pixel 100) is shadowed, right of it lit as
    // with no shadow at all.
    assert!(a[80] < lit[80] * 0.2, "shadowed: {} of {}", a[80], lit[80]);
    assert!(
        (a[120] - lit[120]).abs() < lit[120] * 0.05,
        "lit: {} of {}",
        a[120],
        lit[120]
    );
    // Beauty moves the lamp across its area: a wider light, a wider edge.
    let mut penumbra = |softness: f32| {
        let f = stage.beauty(0., 0., 32, &|_| shot(true, softness)).unwrap();
        ramp(&row(&f)[70..128])
    };
    let (hard, soft) = (penumbra(0.5), penumbra(6.));
    assert!(hard <= 4, "a small lamp is nearly hard: {hard} px");
    assert!(soft >= 6 && soft >= 2 * hard, "{hard} px -> {soft} px");
}

#[test]
fn a_shot_draws_more_than_64_planes() {
    // A plugin exploded four levels deep is ~90 slabs; the cap used to be 64
    // and silently dropped the rest.
    let Some(mut stage) = stage(100, 100) else {
        return;
    };
    let white = block(10., 10.).radius(0.).fill(Color::srgb(1., 1., 1.));
    let white = resolve(&SceneSpec::new(white)).expect("resolves");
    stage.layer("w", &white, Size::new(10., 10.), 1.).unwrap();
    let f = stage
        .render(0., 0., 1, &|_| Shot {
            planes: (0..100)
                .map(|i| {
                    let mut p = Plane::new("w", 10., 10.);
                    p.position = [(i % 10) as f32 * 10. - 45., 45. - (i / 10) as f32 * 10., 0.];
                    p
                })
                .collect(),
            post: Post::NONE,
            ..Shot::new(Camera::front(100., 30.))
        })
        .unwrap();
    // The last plane, bottom right.
    let px = f.rgba[(95 * 100 + 95) * 4];
    assert!(px > 0.9, "plane 100 is drawn: {px}");
}

#[test]
fn coplanar_planes_keep_their_order_under_a_tilted_camera() {
    // Sibling parts of a plugin share a plane: the one submitted later is
    // painted later in 2D and must stay on top when the camera tilts, not
    // lose to whichever centre happens to be nearer.
    let Some(mut stage) = stage(100, 100) else {
        return;
    };
    for (name, rgb) in [("g", (0., 1., 0.)), ("r", (1., 0., 0.))] {
        let b = block(10., 10.)
            .radius(0.)
            .fill(Color::srgb(rgb.0, rgb.1, rgb.2));
        let b = resolve(&SceneSpec::new(b)).expect("resolves");
        stage.layer(name, &b, Size::new(10., 10.), 1.).unwrap();
    }
    let f = stage
        .render(0., 0., 1, &|_| Shot {
            // Green first and nearer to the camera below, red second and
            // farther; they overlap about the origin.
            planes: vec![
                Plane::new("g", 60., 40.).at(0., 15., 0.),
                Plane::new("r", 60., 40.).at(0., -15., 0.),
            ],
            post: Post::NONE,
            ..Shot::new(Camera::front(100., 30.).orbit(0., -50.))
        })
        .unwrap();
    let px = &f.rgba[(50 * 100 + 50) * 4..][..3];
    assert!(px[0] > px[1], "the later plane is on top: {px:?}");
}

#[test]
fn a_fading_face_fades_its_shadow() {
    // A backdrop fading out cast its whole shadow down to half opacity and
    // none below: the floor under it jumped a stop in one frame. Across a
    // beauty frame's samples the shadow now fades with the face.
    let Some(mut stage) = stage(100, 100) else {
        return;
    };
    let white = block(10., 10.).radius(0.).fill(Color::srgb(1., 1., 1.));
    let white = resolve(&SceneSpec::new(white)).expect("resolves");
    stage.layer("w", &white, Size::new(10., 10.), 1.).unwrap();
    let mut key = Light::new(LightKind::Directional);
    key.direction = [1., 0., -1.];
    key.softness = 0.;
    // The wall at x 0..20 in the shade of a card at x -30..-10, 30 in front.
    let mut shade = |card: Option<f32>| {
        let f = stage
            .beauty(0., 0., 16, &|_| {
                let mut planes = vec![Plane::new("w", 100., 100.)];
                if let Some(o) = card {
                    planes.push(Plane::new("w", 20., 20.).at(-20., 0., 30.).opacity(o));
                }
                Shot {
                    planes,
                    lights: vec![key],
                    post: Post::NONE,
                    ..Shot::new(Camera::front(100., 30.))
                }
            })
            .unwrap();
        f.rgba[(50 * 100 + 60) * 4]
    };
    let (lit, dark, half) = (shade(None), shade(Some(1.)), shade(Some(0.4)));
    assert!(lit - dark > 0.1, "the card casts a shadow: {lit} {dark}");
    let f = (lit - half) / (lit - dark);
    assert!(
        (0.2..0.6).contains(&f),
        "a 0.4 card casts {f} of its shadow"
    );
}
