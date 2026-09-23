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
        planes: vec![Plane::new("l", 80., 45.)
            .scale(0.5)
            .rotate(0., 40. + t as f32 * 60., 0.)
            .depth(30.)
            .edge([1., 0., 0.])],
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
    assert!(stage
        .background("fn background(uv: vec2f, t: f32) -> vec3f { oops }")
        .is_err());
    stage
        .background("fn background(uv: vec2f, t: f32) -> vec3f { return vec3f(uv, 0.); }")
        .unwrap();
}
