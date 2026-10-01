//! Glass traced through the slabs (`Shot::trace`). Needs a GPU adapter;
//! with none each test says so and passes.
use super::*;
use mui_scene::prelude::*;

const W: usize = 400;
const H: usize = 240;
const TURN: f32 = 40.;
/// What a traced plane's face shows: the left or right half of the atlas.
const LEFT: [f32; 4] = [0., 0., 0.5, 1.];
const RIGHT: [f32; 4] = [0.5, 0., 1., 1.];

/// A stage with one layer, "atlas", as mui-cut draws every plane from:
/// `left` (or white over black halves, the edge) beside clear white.
fn stage(left: Option<Color>) -> Option<Stage> {
    let mut stage = Stage::new(W as u32, H as u32)
        .map_err(|e| eprintln!("no GPU, skipped: {e}"))
        .ok()?;
    let white = Color::srgb(1., 1., 1.);
    let left = match left {
        Some(c) => vec![block(64., 64.).radius(0.).fill(c)],
        None => vec![
            block(32., 64.).radius(0.).fill(white),
            block(32., 64.).radius(0.).fill(Color::srgb(0., 0., 0.)),
        ],
    };
    let root = row(left
        .into_iter()
        .chain([block(64., 64.).radius(0.).fill(white)]));
    let scene = resolve(&SceneSpec::new(root)).expect("resolves");
    stage
        .layer("atlas", &scene, Size::new(128., 64.), 16.)
        .unwrap();
    Some(stage)
}

fn camera() -> Camera {
    Camera::front(H as f32, 30.)
}

fn glass(ior: f32) -> Material {
    Material {
        transmission: 1.,
        roughness: 0.,
        ior,
        ..Material::SLAB
    }
}

fn frame(stage: &mut Stage, planes: Vec<Plane>, trace: bool) -> Vec<f32> {
    let shot = Shot {
        planes,
        clear: Some([0.; 3]),
        post: Post::NONE,
        trace,
        ..Shot::new(camera())
    };
    stage.render(0., 0., 1, &|_| shot.clone()).unwrap().rgba
}

/// The middle row's channel `c`.
fn middle_row(f: &[f32], c: usize) -> Vec<f32> {
    (0..W).map(|x| f[((H / 2) * W + x) * 4 + c]).collect()
}

/// Where a row crosses half its left plateau, to a fraction of a pixel.
fn edge(row: &[f32]) -> f32 {
    let hi = row[W / 2 - 90];
    let lo = row[W / 2 + 90].min(hi * 0.2);
    let half = (hi + lo) * 0.5;
    (W / 2 - 90..W / 2 + 90)
        .find(|&x| row[x] >= half && row[x + 1] < half)
        .map(|x| x as f32 + (row[x] - half) / (row[x] - row[x + 1]))
        .expect("an edge")
}

/// Pixels the edge at depth `behind` moves when seen through slabs of
/// index `n`, `thick` deep, turned `TURN` about y, whose faces lie
/// `fronts` along their normal: each column's ray traced on the CPU
/// through Snell's law at every face, the one landing on the edge found
/// by bisection.
fn expected(n: f32, thick: f32, fronts: &[f32], behind: f32) -> f32 {
    let eye = camera().eye[2];
    let (tan, aspect) = (15f32.to_radians().tan(), W as f32 / H as f32);
    let (s, c) = TURN.to_radians().sin_cos();
    let nrm = [s, 0., c];
    let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let refract = |i: [f32; 3], eta: f32| {
        let ci = -dot(i, nrm);
        let k = 1. - eta * eta * (1. - ci * ci);
        std::array::from_fn::<f32, 3, _>(|a| eta * i[a] + (eta * ci - k.sqrt()) * nrm[a])
    };
    let go = |p: [f32; 3], d: [f32; 3], plane: f32| {
        let t = (plane - dot(p, nrm)) / dot(d, nrm);
        std::array::from_fn::<f32, 3, _>(|a| p[a] + d[a] * t)
    };
    let land = |col: f32| {
        let x = (col - W as f32 / 2.) / (W as f32 / 2.) * tan * aspect;
        let l = (x * x + 1.).sqrt();
        let (mut p, mut d) = ([0., 0., eye], [x / l, 0., -1. / l]);
        for &f in fronts {
            p = go(p, d, f);
            let inside = refract(d, 1. / n);
            p = go(p, inside, f - thick);
            d = refract(inside, n);
        }
        p[0] + d[0] * (behind - p[2]) / d[2]
    };
    let (mut lo, mut hi) = (0., W as f32);
    for _ in 0..40 {
        let mid = 0.5 * (lo + hi);
        if (land(mid) < 0.) == (land(lo) < 0.) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    lo - W as f32 / 2.
}

/// Two 80-deep slabs one behind the other: through both the edge moves
/// as a ray refracted by the front one and then by the back one lands,
/// not as the back pane looks from the eye.
#[test]
fn traced_glass_behind_glass_bends_along_the_front_panes_ray() {
    let Some(mut stage) = stage(None) else {
        return;
    };
    const BEHIND: f32 = -600.;
    let back = Plane::new("atlas", 1800., 900.).uv(LEFT).at(0., 0., BEHIND);
    let pane = |z: f32| {
        Plane::new("atlas", 400., 200.)
            .uv(RIGHT)
            .depth(80.)
            .rotate(0., TURN, 0.)
            .at(0., 0., z)
            .material(glass(1.5))
    };
    let bare = edge(&middle_row(&frame(&mut stage, vec![back.clone()], true), 1));
    let second = -250.;
    let fronts = [0., second * TURN.to_radians().cos()];
    for count in [1, 2] {
        let mut planes = vec![back.clone(), pane(0.)];
        if count == 2 {
            planes.push(pane(second));
        }
        let moved = edge(&middle_row(&frame(&mut stage, planes, true), 1)) - bare;
        let want = expected(1.5, 80., &fronts[..count], BEHIND);
        assert!(
            (moved.abs() - want).abs() < 1.,
            "{count} pane(s): moved {moved} px, expected {want}"
        );
    }
}

/// A red card behind the camera, facing a pane of dense glass: traced, the
/// pane mirrors it; on screen it is nowhere to be found.
#[test]
fn traced_glass_mirrors_what_is_off_screen() {
    let Some(mut stage) = stage(Some(Color::srgb(1., 0., 0.))) else {
        return;
    };
    let eye = camera().eye[2];
    let planes = vec![
        Plane::new("atlas", 3000., 3000.)
            .uv(LEFT)
            .rotate(0., 180., 0.)
            .at(0., 0., eye + 300.),
        Plane::new("atlas", 400., 200.)
            .uv(RIGHT)
            .material(Material {
                thickness: 20.,
                ..glass(2.5)
            }),
    ];
    let traced = frame(&mut stage, planes.clone(), true);
    let screen = frame(&mut stage, planes, false);
    let at = |f: &[f32]| f[((H / 2) * W + W / 2) * 4..][..3].to_vec();
    let (t, s) = (at(&traced), at(&screen));
    assert!(t[0] > 0.3 && t[1] < 0.05, "traced: the red card, {t:?}");
    assert!(s[0] < 0.05, "on screen: nothing to mirror, {s:?}");
}

/// Frosted, dispersive glass at one ray per pixel: two frames are the same
/// to the bit, and where what is behind is flat, so is the glass.
#[test]
fn traced_glass_is_noiseless_and_repeatable() {
    let Some(mut stage) = stage(None) else {
        return;
    };
    let planes = vec![
        Plane::new("atlas", 1800., 900.).uv(LEFT).at(0., 0., -300.),
        Plane::new("atlas", 400., 200.)
            .uv(RIGHT)
            .depth(20.)
            .rotate(0., TURN, 0.)
            .material(Material {
                roughness: 0.4,
                dispersion: 2.,
                ..glass(1.5)
            }),
    ];
    let a = frame(&mut stage, planes.clone(), true);
    let b = frame(&mut stage, planes, true);
    assert!(a == b, "two traced frames differ");
    let row = middle_row(&a, 1);
    let flat = &row[W / 2 - 90..W / 2 - 50];
    let bumps: f32 = flat
        .windows(3)
        .map(|w| (w[0] - 2. * w[1] + w[2]).abs())
        .sum();
    assert!(flat[0] > 0.2, "white seen through the glass: {}", flat[0]);
    assert!(bumps < 1e-3, "noise over a flat white: {bumps}");
}
