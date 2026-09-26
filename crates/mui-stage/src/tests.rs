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
        leaf(size.width / 2., size.height).fill(Color::srgb(1., 1., 1.)),
        leaf(size.width / 2., size.height).fill(Color::srgb(0., 0., 0.)),
    ]);
    resolve_scene(&SceneSpec::new(root)).expect("resolves")
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
        leaf(1., n as f64).radius(0.).fill(if i % 2 == 0 {
            Color::srgb(1., 1., 1.)
        } else {
            Color::srgb(0., 0., 0.)
        })
    }));
    resolve_scene(&SceneSpec::new(root)).expect("resolves")
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
    let red = resolve_scene(&SceneSpec::new(
        leaf(40., 40.).radius(0.).fill(Color::srgb(1., 0., 0.)),
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
