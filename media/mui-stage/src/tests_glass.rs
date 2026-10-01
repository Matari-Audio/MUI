//! Screen-space reflection and glass. Needs a GPU adapter; with none each
//! test says so and passes.
use super::*;
use mui_scene::prelude::*;

fn stage(w: u32, h: u32) -> Option<Stage> {
    Stage::new(w, h)
        .map_err(|e| eprintln!("no GPU, skipped: {e}"))
        .ok()
}

fn solid(stage: &mut Stage, id: &str, size: f64, c: Color) {
    let root = block(size, size).radius(0.).fill(c);
    let scene = resolve(&SceneSpec::new(root)).expect("resolves");
    stage.layer(id, &scene, Size::new(size, size), 1.).unwrap();
}

/// Left half white, right half black, square.
fn halves(stage: &mut Stage, id: &str, size: f64) {
    let root = row([
        block(size / 2., size)
            .radius(0.)
            .fill(Color::srgb(1., 1., 1.)),
        block(size / 2., size)
            .radius(0.)
            .fill(Color::srgb(0., 0., 0.)),
    ]);
    let scene = resolve(&SceneSpec::new(root)).expect("resolves");
    stage.layer(id, &scene, Size::new(size, size), 16.).unwrap();
}

#[test]
fn a_mirror_floor_shows_the_card_standing_on_it() {
    let Some(mut stage) = stage(320, 240) else {
        return;
    };
    solid(&mut stage, "white", 64., Color::srgb(1., 1., 1.));
    solid(&mut stage, "red", 64., Color::srgb(1., 0., 0.));
    let camera = Camera::front(240., 30.).orbit(0., 18.);
    let shot = |ssr: bool| Shot {
        planes: vec![
            // A polished metal floor at y = -60, and a red card on it.
            Plane::new("white", 900., 900.)
                .rotate(-90., 0., 0.)
                .at(0., -60., 0.)
                .material(Material {
                    metallic: 1.,
                    roughness: 0.02,
                    ..Material::SLAB
                }),
            Plane::new("red", 120., 120.),
        ],
        lights: vec![Light::new(LightKind::Ambient)],
        clear: Some([0.; 3]),
        post: Post::NONE,
        ssr,
        ..Shot::new(camera)
    };
    // Where the card's middle mirrors: its image as far below the floor as
    // it stands above it.
    let [x, y, _] = camera.view_proj(320. / 240.).project([0., -120., 0.]);
    let (px, py) = (
        ((x + 1.) * 0.5 * 320.) as usize,
        ((1. - y) * 0.5 * 240.) as usize,
    );
    let mut at = |ssr| {
        let f = stage.render(0., 0., 1, &|_| shot(ssr)).unwrap();
        f.rgba[(py * 320 + px) * 4..][..3].to_vec()
    };
    let (on, off) = (at(true), at(false));
    assert!(on[0] > 0.8 && on[1] < 0.3, "SSR mirrors the card: {on:?}");
    assert!(
        (off[0] - off[1]).abs() < 0.05,
        "without it the metal mirrors the grey ambient: {off:?}"
    );
}

/// A pane of glass turned `turn` degrees about y in front of a black and
/// white edge 300 units behind it, straight on: the frame's middle row.
struct Pane {
    stage: Stage,
}
impl Pane {
    const W: usize = 400;
    const H: usize = 240;
    const BEHIND: f32 = 300.;
    const TURN: f32 = 40.;
    fn new() -> Option<Self> {
        let mut stage = stage(Self::W as u32, Self::H as u32)?;
        halves(&mut stage, "edge", 64.);
        solid(&mut stage, "clear", 64., Color::srgb(1., 1., 1.));
        Some(Self { stage })
    }
    fn camera() -> Camera {
        Camera::front(Self::H as f32, 30.)
    }
    /// The middle row, red, green and blue, through glass of `m` (none:
    /// no glass).
    fn row(&mut self, m: Option<Material>, sample: Option<u32>) -> [Vec<f32>; 3] {
        let back = Plane::new("edge", 900., 900.).at(0., 0., -Self::BEHIND);
        self.row_over(back, m, sample)
    }
    /// The same over `back` instead of the edge.
    fn row_over(&mut self, back: Plane, m: Option<Material>, sample: Option<u32>) -> [Vec<f32>; 3] {
        let mut planes = vec![back];
        if let Some(m) = m {
            planes.push(
                Plane::new("clear", 240., 200.)
                    .rotate(0., Self::TURN, 0.)
                    .material(m),
            );
        }
        let shot = Shot {
            planes,
            clear: Some([0.; 3]),
            post: Post::NONE,
            sample,
            ..Shot::new(Self::camera())
        };
        let f = self.stage.render(0., 0., 1, &|_| shot.clone()).unwrap();
        let y = Self::H / 2;
        std::array::from_fn(|c| {
            (0..Self::W)
                .map(|x| f.rgba[(y * Self::W + x) * 4 + c])
                .collect()
        })
    }
    /// Where a row crosses half its left plateau, to a fraction of a pixel
    /// (the crossing nearest the middle).
    fn edge(row: &[f32]) -> f32 {
        let hi = row[Self::W / 2 - 60];
        let lo = row[Self::W / 2 + 60].min(hi * 0.2);
        let half = (hi + lo) * 0.5;
        (Self::W / 2 - 60..Self::W / 2 + 60)
            .find(|&x| row[x] >= half && row[x + 1] < half)
            .map(|x| x as f32 + (row[x] - half) / (row[x] - row[x + 1]))
            .expect("an edge")
    }
    /// Pixels the edge moves on screen when seen through a slab `thick`
    /// thick of index `n`: each pixel's ray traced on the CPU through
    /// Snell's law, in and out, to where it meets the edge's plane, and the
    /// column whose ray lands on the edge found by bisection.
    fn expected(n: f32, thick: f32) -> f32 {
        let eye = Self::camera().eye[2];
        let (tan, aspect) = (15f32.to_radians().tan(), Self::W as f32 / Self::H as f32);
        let (s, c) = Self::TURN.to_radians().sin_cos();
        let nrm = [s, 0., c];
        let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
        let refract = |i: [f32; 3], nr: [f32; 3], eta: f32| {
            let ci = -dot(i, nr);
            let k = 1. - eta * eta * (1. - ci * ci);
            std::array::from_fn::<f32, 3, _>(|a| eta * i[a] + (eta * ci - k.sqrt()) * nr[a])
        };
        let land = |col: f32| {
            let x = (col - Self::W as f32 / 2.) / (Self::W as f32 / 2.) * tan * aspect;
            let l = (x * x + 1.).sqrt();
            let d = [x / l, 0., -1. / l];
            let e = [0., 0., eye];
            let hit = -dot(e, nrm) / dot(d, nrm);
            let p: [f32; 3] = std::array::from_fn(|a| e[a] + d[a] * hit);
            let t = refract(d, nrm, 1. / n);
            let path = thick / dot(t, nrm).abs();
            let out: [f32; 3] = std::array::from_fn(|a| p[a] + t[a] * path);
            let o = refract(t, nrm, n);
            out[0] + o[0] * (-Self::BEHIND - out[2]) / o[2]
        };
        let (mut lo, mut hi) = (0., Self::W as f32);
        for _ in 0..40 {
            let mid = 0.5 * (lo + hi);
            if (land(mid) < 0.) == (land(lo) < 0.) {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        lo - Self::W as f32 / 2.
    }
}

fn glass(ior: f32, thickness: f32) -> Material {
    Material {
        transmission: 1.,
        roughness: 0.,
        ior,
        thickness,
        ..Material::SLAB
    }
}

#[test]
fn glass_shifts_what_is_behind_it_by_its_index_and_thickness() {
    let Some(mut pane) = Pane::new() else {
        return;
    };
    let bare = Pane::edge(&pane.row(None, None)[1]);
    let flat = Pane::edge(&pane.row(Some(glass(1., 80.)), None)[1]);
    assert!(
        (flat - bare).abs() < 0.5,
        "an index of 1 bends nothing: {flat} {bare}"
    );
    for (n, thick) in [(1.5, 80.), (1.5, 40.), (1.9, 80.)] {
        let moved = Pane::edge(&pane.row(Some(glass(n, thick)), None)[1]) - bare;
        let want = Pane::expected(n, thick);
        assert!(
            (moved.abs() - want).abs() < 1.,
            "ior {n}, {thick} thick: moved {moved} px, expected {want}"
        );
    }
}

/// A white card hanging in the void, its edge behind the pane: either side
/// of the edge the frame behind lies at another depth (the card, or nothing
/// at all), and the edge still moves by the slab's offset, whichever way.
#[test]
fn an_edge_over_the_void_shifts_by_the_slabs_offset() {
    let Some(mut pane) = Pane::new() else {
        return;
    };
    for side in [-1f32, 1.] {
        let edge = |pane: &mut Pane, m: Option<Material>| {
            let card = Plane::new("clear", 450., 900.).at(side * 225., 0., -Pane::BEHIND);
            let mut row = pane.row_over(card, m, None)[1].clone();
            // The card on the right: its light on the right of the edge.
            if side > 0. {
                row.iter_mut().for_each(|v| *v = 1. - *v);
            }
            Pane::edge(&row)
        };
        let bare = edge(&mut pane, None);
        for (n, thick) in [(1.5, 80.), (1.9, 80.)] {
            let moved = edge(&mut pane, Some(glass(n, thick))) - bare;
            let want = Pane::expected(n, thick);
            assert!(
                (moved.abs() - want).abs() < 1.,
                "card on side {side}, ior {n}, {thick} thick: moved {moved} px, expected {want}"
            );
        }
    }
}

#[test]
fn a_scaled_pane_is_as_thick_as_it_looks() {
    let Some(mut pane) = Pane::new() else {
        return;
    };
    let thick = Pane::edge(&pane.row(Some(glass(1.5, 80.)), None)[1]);
    let row = |pane: &mut Pane, scale: f32| {
        let shot = Shot {
            planes: vec![
                Plane::new("edge", 900., 900.).at(0., 0., -Pane::BEHIND),
                Plane::new("clear", 120., 100.)
                    .scale(scale)
                    .rotate(0., Pane::TURN, 0.)
                    .material(glass(1.5, 40.)),
            ],
            clear: Some([0.; 3]),
            post: Post::NONE,
            ..Shot::new(Pane::camera())
        };
        let f = pane.stage.render(0., 0., 1, &|_| shot.clone()).unwrap();
        let y = Pane::H / 2;
        let g: Vec<f32> = (0..Pane::W)
            .map(|x| f.rgba[(y * Pane::W + x) * 4 + 1])
            .collect();
        Pane::edge(&g)
    };
    let doubled = row(&mut pane, 2.);
    assert!(
        (doubled - thick).abs() < 0.5,
        "40 thick at twice the size is 80: {doubled} {thick}"
    );
}

#[test]
fn dispersion_splits_red_from_blue_at_an_edge() {
    let Some(mut pane) = Pane::new() else {
        return;
    };
    let split = |pane: &mut Pane, k: f32| {
        let [r, _, b] = pane.row(
            Some(Material {
                dispersion: k,
                ..glass(1.5, 160.)
            }),
            None,
        );
        (Pane::edge(&r), Pane::edge(&b))
    };
    let (r, b) = split(&mut pane, 0.);
    assert!((r - b).abs() < 0.2, "no dispersion, one edge: {r} {b}");
    let bare = Pane::edge(&pane.row(None, None)[0]);
    let (r, b) = split(&mut pane, 4.);
    // Blue bends more than red, so it moves further.
    assert!(
        (b - bare).abs() > (r - bare).abs() + 1.5,
        "red at {r}, blue at {b}, unbent {bare}"
    );
}

/// Pixels a row takes to go from 90% to 10% of its left plateau.
fn width(row: &[f32]) -> usize {
    let mid = Pane::W / 2;
    let hi = row[mid - 90];
    let lo = row[mid + 90];
    let span = hi - lo;
    row[mid - 90..mid + 90]
        .iter()
        .filter(|&&v| v < lo + 0.9 * span && v > lo + 0.1 * span)
        .count()
}

#[test]
fn rough_glass_blurs_what_is_behind_it() {
    let Some(mut pane) = Pane::new() else {
        return;
    };
    let rough = |pane: &mut Pane, r: f32, sample| {
        width(
            &pane.row(
                Some(Material {
                    roughness: r,
                    ..glass(1.5, 20.)
                }),
                sample,
            )[1],
        )
    };
    let clear = rough(&mut pane, 0., None);
    let frosted = rough(&mut pane, 0.5, None);
    assert!(clear <= 3, "clear glass keeps the edge sharp: {clear} px");
    assert!(frosted > clear + 8, "frosted: {frosted} px, clear {clear}");
}

#[test]
fn a_beauty_frame_converges_rough_glass() {
    let Some(mut pane) = Pane::new() else {
        return;
    };
    let m = Material {
        roughness: 0.5,
        ..glass(1.5, 20.)
    };
    let one = pane.row(Some(m), Some(0))[1].clone();
    let other = pane.row(Some(m), Some(1))[1].clone();
    assert_ne!(one, other, "each sample takes its own rays");
    // The mean of many is smooth and still blurred.
    let shot = |_| Shot {
        planes: vec![
            Plane::new("edge", 900., 900.).at(0., 0., -Pane::BEHIND),
            Plane::new("clear", 240., 200.)
                .rotate(0., Pane::TURN, 0.)
                .material(m),
        ],
        clear: Some([0.; 3]),
        post: Post::NONE,
        ..Shot::new(Pane::camera())
    };
    let f = pane.stage.beauty(0., 0., 32, &shot).unwrap();
    let y = Pane::H / 2;
    let row: Vec<f32> = (0..Pane::W)
        .map(|x| f.rgba[(y * Pane::W + x) * 4 + 1])
        .collect();
    assert!(width(&row) > 8, "blurred: {}", width(&row));
    // Noise is what a second difference sees; the mean of 32 has far less
    // than one sample.
    let noise = |row: &[f32]| -> f32 {
        row[Pane::W / 2 - 60..Pane::W / 2 + 60]
            .windows(3)
            .map(|w| (w[0] - 2. * w[1] + w[2]).abs())
            .sum()
    };
    assert!(
        noise(&row) < 0.25 * noise(&one),
        "converged: {} vs one sample {}",
        noise(&row),
        noise(&one)
    );
}

/// A clear pane facing a light that shines from the eye: its middle
/// mirrors the light back, a highlight over the black behind it; its
/// corners, turned away from the mirror angle, stay dark.
#[test]
fn a_light_leaves_a_highlight_on_glass() {
    let Some(mut pane) = Pane::new() else {
        return;
    };
    let shot = Shot {
        planes: vec![Plane::new("clear", 240., 200.).material(Material {
            roughness: 0.3,
            ..glass(1.5, 20.)
        })],
        lights: vec![Light {
            direction: [0., 0., -1.],
            ..Light::new(LightKind::Directional)
        }],
        clear: Some([0.; 3]),
        post: Post::NONE,
        ..Shot::new(Pane::camera())
    };
    let f = pane.stage.render(0., 0., 1, &|_| shot.clone()).unwrap();
    let at = |x: usize, y: usize| f.rgba[(y * Pane::W + x) * 4 + 1];
    let (middle, corner) = (
        at(Pane::W / 2, Pane::H / 2),
        at(Pane::W / 2 + 110, Pane::H / 2 + 90),
    );
    assert!(
        middle > corner + 0.15,
        "the highlight {middle}, a corner {corner}"
    );
}

/// Glass behind glass: through a clear pane the red pane behind it shows,
/// as it does beside it, instead of the black the opaque frame has there.
#[test]
fn glass_shows_the_glass_behind_it() {
    let Some(mut pane) = Pane::new() else {
        return;
    };
    solid(&mut pane.stage, "red", 64., Color::srgb(1., 0., 0.));
    let red = Material {
        transmission: 0.5,
        ..glass(1.5, 20.)
    };
    let shot = Shot {
        planes: vec![
            Plane::new("red", 300., 200.)
                .at(0., 0., -100.)
                .material(red),
            // Index 1: nothing bends, so what is behind shows where it is.
            Plane::new("clear", 120., 120.).material(glass(1., 20.)),
        ],
        lights: vec![Light::new(LightKind::Ambient)],
        clear: Some([0.; 3]),
        post: Post::NONE,
        ..Shot::new(Pane::camera())
    };
    let f = pane.stage.render(0., 0., 1, &|_| shot.clone()).unwrap();
    let at = |x: usize| f.rgba[(Pane::H / 2 * Pane::W + x) * 4];
    let (through, beside) = (at(Pane::W / 2), at(Pane::W / 2 + 100));
    assert!(beside > 0.2, "the red pane: {beside}");
    assert!(
        through > beside * 0.8,
        "through the clear pane {through}, beside it {beside}"
    );
}

/// A white label in front of frosted glass over black: the glass does not
/// see it behind itself, so no blurred halo grows round it.
#[test]
fn a_label_in_front_of_frosted_glass_leaves_no_halo() {
    let Some(mut stage) = stage(400, 240) else {
        return;
    };
    solid(&mut stage, "black", 64., Color::srgb(0., 0., 0.));
    solid(&mut stage, "white", 64., Color::srgb(1., 1., 1.));
    let shot = Shot {
        planes: vec![
            Plane::new("black", 900., 900.).at(0., 0., -300.),
            Plane::new("white", 300., 200.).material(Material {
                roughness: 0.6,
                ..glass(1.5, 20.)
            }),
            Plane::new("white", 40., 40.).at(0., 0., 20.),
        ],
        clear: Some([0.; 3]),
        post: Post::NONE,
        ..Shot::new(Pane::camera())
    };
    let f = stage.render(0., 0., 1, &|_| shot.clone()).unwrap();
    let at = |x: usize| f.rgba[(120 * 400 + x) * 4 + 1];
    let [x0, _, _] = Pane::camera()
        .view_proj(400. / 240.)
        .project([20., 0., 20.]);
    let edge = ((x0 + 1.) * 0.5 * 400.) as usize;
    let (near, far) = (at(edge + 8), at(330));
    assert!(at(edge - 4) > 0.9, "the label: {}", at(edge - 4));
    assert!(
        (near - far).abs() < 0.03,
        "beside the label {near}, far from it {far}"
    );
}

/// A sphere `r` across, smooth-shaded, as `Stage::mesh` takes it.
fn sphere(r: f32) -> (Vec<[f32; 6]>, Vec<u32>) {
    let (rings, segs) = (48u32, 96u32);
    let mut v = Vec::new();
    for i in 0..=rings {
        let th = std::f32::consts::PI * i as f32 / rings as f32;
        for j in 0..=segs {
            let ph = std::f32::consts::TAU * j as f32 / segs as f32;
            let n = [th.sin() * ph.cos(), th.cos(), th.sin() * ph.sin()];
            v.push([n[0] * r, n[1] * r, n[2] * r, n[0], n[1], n[2]]);
        }
    }
    let mut idx = Vec::new();
    for i in 0..rings {
        for j in 0..segs {
            let a = i * (segs + 1) + j;
            let b = a + segs + 1;
            idx.extend([a, b, a + 1, a + 1, b, b + 1]);
        }
    }
    (v, idx)
}

/// A polished metal ball over a plain orange floor, seen from just above:
/// its lower half mirrors the floor. A smooth ball's reflection of a plain
/// floor is smooth, out to the clean line where the floor it would see is
/// hidden behind the ball; a march that hits on one pixel and misses on the
/// next speckles it, which the mean squared Laplacian measures.
#[test]
fn a_metal_balls_reflection_is_smooth() {
    const W: usize = 320;
    const H: usize = 240;
    let Some(mut stage) = stage(W as u32, H as u32) else {
        return;
    };
    solid(&mut stage, "orange", 64., Color::srgb(1., 0.4, 0.1));
    let (v, idx) = sphere(60.);
    stage.mesh("ball", &v, &idx);
    let camera = Camera::front(H as f32, 30.).orbit(0., 10.);
    let shot = Shot {
        planes: vec![
            Plane::new("orange", 2000., 2000.)
                .rotate(-90., 0., 0.)
                .at(0., -80., 0.),
        ],
        models: vec![Model {
            mesh: "ball".into(),
            transform: Mat4::IDENTITY,
            color: [1., 1., 1., 1.],
            material: Material {
                metallic: 1.,
                roughness: 0.05,
                ..Material::SLAB
            },
            cast: false,
            receive: false,
            maps: Default::default(),
        }],
        lights: vec![Light {
            color: [0.2; 3],
            ..Light::new(LightKind::Ambient)
        }],
        clear: Some([0.; 3]),
        post: Post::NONE,
        ..Shot::new(camera)
    };
    let one = stage.render(0., 0., 1, &|_| shot.clone()).unwrap();
    // A beauty frame jitters each sample's march; the mean is as smooth.
    let mean = stage.beauty(0., 0., 16, &|_| shot.clone()).unwrap();
    // The ball's lower half, inside its rim.
    let [cx, cy, _] = camera.view_proj(W as f32 / H as f32).project([0., 0., 0.]);
    let (cx, cy) = (
        ((cx + 1.) * 0.5 * W as f32) as usize,
        ((1. - cy) * 0.5 * H as f32) as usize,
    );
    for f in [one, mean] {
        let px = |x: usize, y: usize| f.rgba[(y * W + x) * 4];
        let mean = |c: usize| {
            let n = (cy + 44..cy + 52).flat_map(|y| (cx - 10..cx + 10).map(move |x| (x, y)));
            n.map(|(x, y)| f.rgba[(y * W + x) * 4 + c]).sum::<f32>() / 160.
        };
        let (red, blue) = (mean(0), mean(2));
        assert!(
            red > 2. * blue,
            "the ball mirrors the orange floor: {red} {blue}"
        );
        let (mut e, mut k) = (0f32, 0f32);
        for y in cy + 5..cy + 55 {
            for x in cx - 40..cx + 40 {
                let l = 4. * px(x, y) - px(x - 1, y) - px(x + 1, y) - px(x, y - 1) - px(x, y + 1);
                e += l * l;
                k += 1.;
            }
        }
        // A speckled march measures 9e-5 here, a clean one under 2e-5.
        assert!(e / k < 4e-5, "speckle energy {}", e / k);
    }
}

/// A sunlit sky with half its area clouded, the sun up ahead and right.
fn cloudy(drift: f32) -> Sky {
    Sky {
        sun: [0.4, 0.35, -1.],
        zenith: [0.08, 0.2, 0.55],
        horizon: [0.6, 0.72, 0.85],
        sun_color: [2.6, 2.4, 2.1],
        cover: 0.5,
        drift: [drift, 0.],
    }
}

/// Clear glass `t` transmissive, its rim rounded over `bevel`.
fn clear(t: f32, bevel: f32) -> Material {
    Material {
        transmission: t,
        roughness: 0.,
        thickness: 60.,
        bevel,
        ..Material::SLAB
    }
}

/// Looking 15 degrees up into [`cloudy`], through layer `id` as a pane of
/// `m` turned 40 degrees, or with none: the frame, red, green and blue.
fn sky_frame(stage: &mut Stage, pane: Option<(&str, Material)>, drift: f32) -> Vec<f32> {
    let planes = pane
        .map(|(id, m)| Plane::new(id, 200., 160.).rotate(0., 40., 0.).material(m))
        .into_iter()
        .collect();
    let shot = Shot {
        planes,
        sky: Some(cloudy(drift)),
        clear: Some([0.; 3]),
        post: Post::NONE,
        ..Shot::new(Camera::front(240., 40.).orbit(0., -15.))
    };
    stage.render(0., 0., 1, &|_| shot.clone()).unwrap().rgba
}

#[test]
fn a_sky_has_clouds_that_drift_and_glass_bends_them() {
    let Some(mut stage) = stage(320, 240) else {
        return;
    };
    solid(&mut stage, "clear", 64., Color::srgb(1., 1., 1.));
    solid(&mut stage, "dark", 64., Color::srgb(0.06, 0.06, 0.07));
    let (w, h) = (320, 240);
    let luma = |f: &[f32], x: usize, y: usize| {
        let p = &f[(y * w + x) * 4..];
        0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2]
    };
    let bare = sky_frame(&mut stage, None, 0.);
    // Blue overhead: the gradient is there under the clouds.
    let top = &bare[(4 * w + 4) * 4..];
    assert!(top[2] > top[0], "blue overhead: {:?}", &top[..3]);
    // Clouds, not a gradient and not noise: the upper half's luma spreads
    // widely, yet neighbours are nearly alike (large soft shapes).
    let ls: Vec<f32> = (10..h / 2)
        .flat_map(|y| (0..w).map(move |x| (x, y)))
        .map(|(x, y)| luma(&bare, x, y))
        .collect();
    let mean = ls.iter().sum::<f32>() / ls.len() as f32;
    let sd = (ls.iter().map(|l| (l - mean).powi(2)).sum::<f32>() / ls.len() as f32).sqrt();
    let step = (10..h / 2)
        .flat_map(|y| (0..w - 1).map(move |x| (x, y)))
        .map(|(x, y)| (luma(&bare, x, y) - luma(&bare, x + 1, y)).abs())
        .sum::<f32>()
        / ((h / 2 - 10) * (w - 1)) as f32;
    assert!(sd > 0.05, "clouds vary the sky: sd {sd}");
    assert!(
        step < sd * 0.2,
        "soft shapes, not noise: step {step}, sd {sd}"
    );
    // Drift moves them.
    let moved = sky_frame(&mut stage, None, 0.6);
    let diff = (0..w * h)
        .map(|i| (bare[i * 4] - moved[i * 4]).abs())
        .sum::<f32>()
        / (w * h) as f32;
    assert!(diff > 0.02, "drift moves the clouds: {diff}");
    let all = |f: &[f32]| {
        (0..w * h)
            .map(|i| luma(f, i % w, i / w))
            .collect::<Vec<_>>()
    };
    let mean_diff = |a: &[f32], b: &[f32]| {
        a.iter().zip(b).map(|(a, c)| (a - c).abs()).sum::<f32>() / a.len() as f32
    };
    let b = all(&bare);
    // A flat pane leaves the far sky where it is; a bevelled one bends it
    // round its rim, and its middle shows the clouds, not darkened.
    let flat = all(&sky_frame(&mut stage, Some(("clear", clear(1., 0.))), 0.));
    let round = all(&sky_frame(&mut stage, Some(("clear", clear(1., 40.))), 0.));
    let (bent, moved) = (mean_diff(&b, &round), mean_diff(&b, &flat));
    assert!(
        bent > 2. * moved && bent > 0.004,
        "the rim bends the sky: {bent} against flat {moved}"
    );
    let mid = |f: &[f32]| {
        (h / 2 - 10..h / 2 + 10)
            .map(|y| f[y * w + w / 2])
            .sum::<f32>()
            / 20.
    };
    assert!(
        mid(&round) > 0.6 * mid(&b),
        "the clouds show through: {} against {}",
        mid(&round),
        mid(&b)
    );
    // Printed, a dark layer is clear and a light one stays ink; stained,
    // the dark layer darkens the sky.
    let print = |t| Material {
        print: 1.,
        ..clear(t, 0.)
    };
    let dark_print = all(&sky_frame(&mut stage, Some(("dark", print(1.))), 0.));
    let dark_stain = all(&sky_frame(&mut stage, Some(("dark", clear(1., 0.))), 0.));
    let white_print = all(&sky_frame(&mut stage, Some(("clear", print(1.))), 0.));
    assert!(
        mid(&dark_print) > 0.8 * mid(&b),
        "printed dark is clear: {} against {}",
        mid(&dark_print),
        mid(&b)
    );
    assert!(
        mid(&dark_stain) < 0.3 * mid(&b),
        "stained dark darkens: {}",
        mid(&dark_stain)
    );
    let opaque_white = all(&sky_frame(&mut stage, Some(("clear", clear(0., 0.))), 0.));
    assert!(
        (mid(&white_print) - mid(&opaque_white)).abs() < 0.05,
        "printed light is ink: {} against {}",
        mid(&white_print),
        mid(&opaque_white)
    );
    // Turning to glass starts where opaque ends.
    let faint = all(&sky_frame(&mut stage, Some(("dark", print(0.002))), 0.));
    let opaque = all(&sky_frame(&mut stage, Some(("dark", print(0.))), 0.));
    let pop = b
        .iter()
        .enumerate()
        .map(|(i, _)| (faint[i] - opaque[i]).abs())
        .fold(0f32, f32::max);
    assert!(pop < 0.02, "no pop into glass: {pop}");
}

#[test]
fn glass_bends_the_sky_in_from_beyond_the_frame() {
    let Some(mut stage) = stage(320, 240) else {
        return;
    };
    solid(&mut stage, "clear", 64., Color::srgb(1., 1., 1.));
    // No clouds, the sun low and just out of the frame to the left.
    let sky = Sky {
        sun: [-0.62, 0.08, -0.78],
        zenith: [0.01, 0.02, 0.05],
        horizon: [0.03, 0.035, 0.04],
        sun_color: [1.; 3],
        cover: 0.,
        drift: [0.; 2],
    };
    let camera = Camera::front(240., 40.);
    let render = |stage: &mut Stage, glass: bool| {
        // A domed pane straddling the left edge: its rim bends rays on
        // out past the frame, toward the sun.
        let planes = if glass {
            vec![
                Plane::new("clear", 220., 220.)
                    .at(-170., 0., 0.)
                    .material(clear(1., 110.)),
            ]
        } else {
            vec![]
        };
        let shot = Shot {
            planes,
            sky: Some(sky),
            clear: Some([0.; 3]),
            post: Post::NONE,
            ..Shot::new(camera)
        };
        stage.render(0., 0., 1, &|_| shot.clone()).unwrap().rgba
    };
    let (bare, glass) = (render(&mut stage, false), render(&mut stage, true));
    let w = 320;
    let luma = |f: &[f32], x: usize, y: usize| {
        let p = &f[(y * w + x) * 4..];
        0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2]
    };
    // The brightest the frame's edge gets: all a ray clamped to the frame
    // could find.
    let edge = (0..240).map(|y| luma(&bare, 0, y)).fold(0f32, f32::max);
    let through = (0..240)
        .flat_map(|y| (0..40).map(move |x| (x, y)))
        .map(|(x, y)| luma(&glass, x, y))
        .fold(0f32, f32::max);
    assert!(
        through > edge * 1.3,
        "the sun bent in from beyond: {through} against the edge's {edge}"
    );
}

#[test]
fn only_a_mirror_shows_the_sun_disc_in_the_sky() {
    let Some(mut stage) = stage(320, 240) else {
        return;
    };
    solid(&mut stage, "clear", 64., Color::srgb(1., 1., 1.));
    // The sun behind the camera, so a metal facing it mirrors the disc.
    let sky = Sky {
        sun: [0.08, 0.06, 1.],
        cover: 0.,
        ..cloudy(0.)
    };
    let hottest = |stage: &mut Stage, roughness: f32| {
        let shot = Shot {
            planes: vec![Plane::new("clear", 600., 600.).material(Material {
                metallic: 1.,
                roughness,
                ..Material::SLAB
            })],
            lights: vec![Light::new(LightKind::Ambient)],
            sky: Some(sky),
            clear: Some([0.; 3]),
            post: Post::NONE,
            ssr: false,
            ..Shot::new(Camera::front(240., 30.))
        };
        let f = stage.render(0., 0., 1, &|_| shot.clone()).unwrap();
        f.rgba
            .chunks(4)
            .map(|p| p[0].min(p[1]).min(p[2]))
            .fold(0f32, f32::max)
    };
    let (mirror, rough) = (hottest(&mut stage, 0.02), hottest(&mut stage, 0.5));
    assert!(mirror > 0.99, "a mirror shows the disc: {mirror}");
    assert!(rough < 0.9, "a rough face spreads it: {rough}");
}
