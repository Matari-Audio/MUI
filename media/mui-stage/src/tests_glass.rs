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
        let mut planes = vec![Plane::new("edge", 900., 900.).at(0., 0., -Self::BEHIND)];
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
