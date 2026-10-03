//! The sky's light by direction, on the CPU: the physical atmosphere
//! (single-scattering Rayleigh, Mie and ozone, after Nishita 1993, with the
//! earth's shadow for twilight) and the artist's gradient, without clouds
//! or the sun's disc. The same maths as `atmosphere` and `sky_seen` in
//! stage.wgsl, which the tests hold to this; it bakes the sky that lights a
//! shot ([`Sky::light`]) and gives the sun's colour through the air.
//!
//! Lengths are kilometres, directions the stage's world (y up).
use crate::{Atmosphere, Sky};

const EARTH: f32 = 6360.;
const TOP: f32 = 6460.;
const RAYLEIGH: [f32; 3] = [5.802e-3, 13.558e-3, 33.1e-3];
const MIE: f32 = 3.996e-3;
const MIE_EXT: f32 = 4.4e-3;
const OZONE: [f32; 3] = [0.650e-3, 1.881e-3, 0.085e-3];
const H_RAYLEIGH: f32 = 8.;
const H_MIE: f32 = 1.2;
const MIE_G: f32 = 0.8;
/// Samples along the view ray and toward the sun.
pub const VIEW_STEPS: u32 = 16;
pub const SUN_STEPS: u32 = 6;
/// The sun's irradiance at intensity 1: lights a white face square to it
/// to about 1, and leaves a clear noon sky round 0.2 to 0.8.
pub const SUN: f32 = 3.6;
/// The sun's angular radius, radians.
pub const SUN_RADIUS: f32 = 0.004_67;
/// ponytail: single scattering leaves twilight too dark; this isotropic
/// share of the light scattered once more stands in for the multiple
/// scattering Hillaire's LUT would compute.
const MULTI: f32 = 1.;
/// A clear day's sky, zenith and horizon, as [`Sky::auto_exposure`]
/// meters it: the level it leaves alone.
const DAY: f32 = 0.16;

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn scale(a: [f32; 3], k: f32) -> [f32; 3] {
    a.map(|v| v * k)
}
fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0., 1.);
    t * t * (3. - 2. * t)
}

/// Where a ray from `o` along unit `d` crosses the sphere of radius `r`
/// about the earth's centre: near and far, or none.
fn sphere(o: [f32; 3], d: [f32; 3], r: f32) -> Option<(f32, f32)> {
    let b = dot(o, d);
    let c = dot(o, o) - r * r;
    let h = b * b - c;
    (h >= 0.).then(|| (-b - h.sqrt(), -b + h.sqrt()))
}

/// Rayleigh, Mie and ozone density at height `h` km.
fn density(h: f32) -> [f32; 3] {
    [
        (-h / H_RAYLEIGH).exp(),
        (-h / H_MIE).exp(),
        (1. - (h - 25.).abs() / 15.).max(0.),
    ]
}

/// Extinction per km of air of `density`.
fn extinction(a: &Atmosphere, [r, m, o]: [f32; 3]) -> [f32; 3] {
    let mie = MIE_EXT * a.dust() * m;
    std::array::from_fn(|c| RAYLEIGH[c] * r + mie + OZONE[c] * a.ozone * o)
}

/// Optical depth from `p` to the top of the air along `d`; `None` where
/// the earth is in the way.
fn depth_to_sun(a: &Atmosphere, p: [f32; 3], d: [f32; 3]) -> Option<[f32; 3]> {
    if sphere(p, d, EARTH).is_some_and(|(t0, _)| t0 > 0.) {
        return None;
    }
    let (_, far) = sphere(p, d, TOP)?;
    let ds = far / SUN_STEPS as f32;
    let mut od = [0.; 3];
    for i in 0..SUN_STEPS {
        let q = add(p, scale(d, (i as f32 + 0.5) * ds));
        let h = dot(q, q).sqrt() - EARTH;
        od = add(od, scale(extinction(a, density(h)), ds));
    }
    Some(od)
}

fn eye(a: &Atmosphere) -> [f32; 3] {
    [0., EARTH + (a.altitude / 1000.).clamp(0.001, 50.), 0.]
}

/// What is left of sunlight through the air from the eye toward `sun`:
/// white overhead, reddened low, black once the sun has set.
pub fn transmittance(a: &Atmosphere, sun: [f32; 3]) -> [f32; 3] {
    depth_to_sun(a, eye(a), norm(sun)).map_or([0.; 3], |od| od.map(|v| (-v).exp()))
}

/// The sky's radiance along `d` (linear RGB), its sun along `sun`, without
/// the sun's disc: light the air scatters toward the eye, and below the
/// horizon the ground lit through it.
pub fn atmosphere(a: &Atmosphere, sun: [f32; 3], d: [f32; 3]) -> [f32; 3] {
    let (sun, d) = (norm(sun), norm(d));
    let o = eye(a);
    let Some((_, mut far)) = sphere(o, d, TOP) else {
        return [0.; 3];
    };
    let ground = sphere(o, d, EARTH).filter(|&(t0, _)| t0 > 0.);
    if let Some((t0, _)) = ground {
        far = t0;
    }
    let (phase_r, phase_m) = phases(dot(d, sun));
    let mie_s = MIE * a.dust();
    let mut od = [0f32; 3];
    let mut sum = [0f32; 3];
    // Steps growing with the square of their count: short near the eye,
    // where the air is dense and a long ray's light mostly comes from.
    let n2 = (VIEW_STEPS * VIEW_STEPS) as f32;
    for i in 0..VIEW_STEPS {
        let (t0, t1) = (
            (i * i) as f32 / n2 * far,
            ((i + 1) * (i + 1)) as f32 / n2 * far,
        );
        let ds = t1 - t0;
        let p = add(o, scale(d, 0.5 * (t0 + t1)));
        let r = dot(p, p).sqrt();
        let dens = density(r - EARTH);
        let step = scale(extinction(a, dens), ds);
        let mid = add(od, scale(step, 0.5));
        od = add(od, step);
        let scatter_r = scale(RAYLEIGH, dens[0]);
        let scatter_m = mie_s * dens[1];
        if let Some(ol) = depth_to_sun(a, p, sun) {
            for c in 0..3 {
                let t = (-(mid[c] + ol[c])).exp();
                sum[c] += t * (scatter_r[c] * phase_r + scatter_m * phase_m) * ds;
            }
        }
        let lit = multiple(a, dot(p, sun) / r);
        for c in 0..3 {
            let t = (-mid[c]).exp();
            sum[c] += t * (scatter_r[c] + scatter_m) * ds * lit;
        }
    }
    if let Some((t0, _)) = ground {
        // The ground: Lambertian, lit by the sun through the air.
        let p = add(o, scale(d, t0));
        let n = scale(p, 1. / EARTH);
        let lit = depth_to_sun(a, p, sun).map_or([0.; 3], |ol| ol.map(|v| (-v).exp()));
        let (ns, k) = (dot(n, sun), a.ground_albedo / std::f32::consts::PI);
        // The sky's own light on it, a little, on through twilight.
        let sky = 0.15 * smoothstep(-0.3, 0.2, ns);
        for c in 0..3 {
            sum[c] += (-od[c]).exp() * (lit[c] * ns.max(0.) + sky) * k;
        }
    }
    scale(sum, SUN * a.intensity)
}

/// Rayleigh's and Mie's phase at `mu`, the cosine between the view and
/// the sun.
fn phases(mu: f32) -> (f32, f32) {
    let g2 = MIE_G * MIE_G;
    (
        3. / (16. * std::f32::consts::PI) * (1. + mu * mu),
        3. / (8. * std::f32::consts::PI) * (1. - g2) * (1. + mu * mu)
            / ((2. + g2) * (1. + g2 - 2. * MIE_G * mu).max(1e-4).powf(1.5)),
    )
}

/// Light scattered more than once, per unit of scattering, isotropic, at a
/// point whose sun is `up` (the cosine from its zenith): as bright as the
/// sun is high there, and on through twilight, when the air still high
/// in the sun lights the air below it.
fn multiple(a: &Atmosphere, up: f32) -> f32 {
    let lit = smoothstep(-0.3, 0.2, up) * (0.3 + 0.7 * up.max(0.)) * (1. + a.ground_albedo);
    lit * MULTI / (4. * std::f32::consts::PI)
}

/// Aerial perspective: the air between the eye and a surface `km` off
/// along unit `d`, its sun along `sun`, as one layer at the eye's height
/// (a shot's distances are short beside the air's scale heights): what
/// it leaves of the surface's light, and the light it scatters in. Far
/// enough, a surface takes the sky's colour at the horizon.
pub fn aerial(a: &Atmosphere, sun: [f32; 3], d: [f32; 3], km: f32) -> ([f32; 3], [f32; 3]) {
    let (sun, d) = (norm(sun), norm(d));
    let dens = density(eye(a)[1] - EARTH);
    let ext = extinction(a, dens);
    let (pr, pm) = phases(dot(d, sun));
    let e = transmittance(a, sun);
    let ms = multiple(a, sun[1]);
    let sm = MIE * a.dust() * dens[1];
    let k = SUN * a.intensity;
    let tr = ext.map(|v| (-v * km.max(0.)).exp());
    let add = std::array::from_fn(|c| {
        let sr = RAYLEIGH[c] * dens[0];
        let j = (sr * pr + sm * pm) * e[c] + (sr + sm) * ms;
        j * k * (1. - tr[c]) / ext[c].max(1e-9)
    });
    (tr, add)
}

/// The colour [`Sky::physical`] gives sunlight: its irradiance through the
/// air over π, the light a white face square to it reflects.
pub fn sunlight(a: &Atmosphere, sun: [f32; 3]) -> [f32; 3] {
    scale(
        transmittance(a, sun),
        SUN * a.intensity / std::f32::consts::PI,
    )
}

fn norm(v: [f32; 3]) -> [f32; 3] {
    let l = dot(v, v).sqrt();
    if l > 0. {
        scale(v, 1. / l)
    } else {
        [0., 1., 0.]
    }
}

/// The sky's radiance along `d` without clouds or the sun's disc: the
/// physical atmosphere or the gradient, as `sky_seen` draws it.
pub fn radiance(k: &Sky, d: [f32; 3]) -> [f32; 3] {
    let d = norm(d);
    if let Some(a) = &k.atmosphere {
        return atmosphere(a, k.sun, d);
    }
    let sun = norm(k.sun);
    let up = d[1];
    let h = 1. - up.abs().clamp(0., 1.);
    let mu = dot(d, sun).max(0.);
    let mut c: [f32; 3] = std::array::from_fn(|i| {
        let a = k.zenith[i] + (k.horizon[i] - k.zenith[i]) * h.powi(6);
        if up < 0. {
            let low = k.horizon[i] * 0.55 + k.zenith[i] * 0.15;
            k.horizon[i] + (low - k.horizon[i]) * smoothstep(0., 0.5, -up)
        } else {
            a
        }
    });
    let glow = 0.06 * mu.powi(4) + 0.12 * mu.powi(16) * h + 0.35 * mu.powi(64);
    c = add(c, scale(k.sun_color, glow));
    c
}

/// The sky as it lights a shot: [`radiance`] by direction, `width` by
/// half that, with the clouds' cover dimming and greying it.
/// ponytail: cloud cover as a uniform overcast blend, not the clouds'
/// shapes; bake `sky_seen` on the GPU if a shot needs their light to move.
pub fn bake(k: &Sky, width: u32) -> crate::EnvImage {
    let grey = add(
        scale(add(k.zenith, k.horizon), 0.35),
        scale(k.sun_color, 0.12),
    );
    let cover = k.cover.clamp(0., 1.) * 0.7;
    crate::EnvImage::from_fn(width, width / 2, |d| {
        let c = radiance(k, d);
        std::array::from_fn(|i| c[i] + (grey[i] - c[i]) * cover * smoothstep(-0.1, 0.2, d[1]))
    })
}

impl Atmosphere {
    /// Its Mie (dust and droplet) density: turbidity 1 is pure air, 2 a
    /// clear day, as Blender's dust density 1.
    pub fn dust(&self) -> f32 {
        (self.turbidity - 1.).max(0.)
    }
    /// As the shaders' `sky4` takes it: dust, ozone, the eye's height in
    /// km, the ground's albedo.
    pub fn uniform(&self) -> [f32; 4] {
        [
            self.dust(),
            self.ozone.max(0.),
            (self.altitude / 1000.).clamp(0.001, 50.),
            self.ground_albedo.clamp(0., 1.),
        ]
    }
}

impl Sky {
    /// As the shaders' `sky0`..`sky5` take it.
    pub fn uniform(&self) -> [f32; 24] {
        let mut g = [0.; 24];
        g[..3].copy_from_slice(&norm(self.sun));
        g[3] = 1.;
        g[4..7].copy_from_slice(&self.zenith);
        g[7] = self.cover.clamp(0., 1.);
        g[8..11].copy_from_slice(&self.horizon);
        g[11] = self.drift[0];
        g[12..15].copy_from_slice(&self.sun_color);
        g[15] = self.drift[1];
        if let Some(a) = &self.atmosphere {
            g[3] = 2.;
            g[16..20].copy_from_slice(&a.uniform());
            g[20] = SUN * a.intensity;
            g[21] = self.aerial.max(0.);
        }
        g[22] = self.density.max(0.);
        g[23] = self.cloud_altitude.clamp(0.1, 20.);
        g
    }

    /// The exposure that keeps a physical sky readable as the sun goes
    /// down, as a camera's meter would: 1 by day, rising (by less than the
    /// light falls, so dusk still reads as dusk) to 64 deep in twilight.
    /// The gradient's colours are the artist's: 1.
    pub fn auto_exposure(&self) -> f32 {
        let Some(a) = &self.atmosphere else {
            return 1.;
        };
        let luma = |c: [f32; 3]| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
        let k = a.intensity.max(1e-6);
        let seen = (luma(self.zenith) + luma(self.horizon)) * 0.5 / k;
        (DAY / seen.max(1e-9)).powf(0.8).clamp(1., 64.)
    }

    /// A physical sky: `air` scattering sunlight from `sun`, its
    /// zenith, horizon and sun colours (what the clouds are lit by) its own.
    pub fn physical(sun: [f32; 3], air: Atmosphere, cover: f32, drift: [f32; 2]) -> Self {
        let s = norm(sun);
        // The horizon a quarter turn round from the sun, a little up.
        let side = norm([-s[2], 0.05, s[0]]);
        let side = if s[0] == 0. && s[2] == 0. {
            [1., 0.05, 0.]
        } else {
            side
        };
        Self {
            sun: s,
            zenith: atmosphere(&air, s, [0., 1., 0.]),
            horizon: atmosphere(&air, s, side),
            sun_color: sunlight(&air, s),
            cover,
            drift,
            aerial: 1.,
            atmosphere: Some(air),
            light: true,
            ..Self::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clear() -> Atmosphere {
        Atmosphere::default()
    }
    fn sun_at(deg: f32) -> [f32; 3] {
        let e = deg.to_radians();
        [0., e.sin(), -e.cos()]
    }
    fn luma(c: [f32; 3]) -> f32 {
        0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
    }

    #[test]
    fn a_clear_noon_sky_is_blue_overhead_and_paler_at_the_horizon() {
        let a = clear();
        let sun = sun_at(60.);
        let zenith = atmosphere(&a, sun, [0., 1., 0.]);
        let horizon = atmosphere(&a, sun, [0., 0.02, 1.]);
        assert!(
            zenith[2] > 1.5 * zenith[0] && zenith[2] > zenith[1],
            "blue zenith {zenith:?}"
        );
        assert!(
            luma(horizon) > luma(zenith),
            "brighter horizon {horizon:?} {zenith:?}"
        );
        // Whiter: the horizon's red is nearer its blue than the zenith's.
        assert!(horizon[0] / horizon[2] > zenith[0] / zenith[2]);
        // Exposure-friendly at intensity 1: no daylight sky clips.
        for c in [zenith, horizon] {
            assert!(c.iter().all(|v| (0.03..1.).contains(v)), "{c:?}");
        }
        // Sunlight at noon is near white and lights a white face to ~1.
        let s = sunlight(&a, sun);
        assert!(s.iter().all(|v| (0.6..1.3).contains(v)), "{s:?}");
    }

    #[test]
    fn the_sun_reddens_as_it_sets_and_twilight_glows_after() {
        let a = clear();
        let t = |deg| transmittance(&a, sun_at(deg));
        let (noon, low) = (t(60.), t(2.));
        assert!(
            low[0] / low[2] > 3. * noon[0] / noon[2],
            "reddened {low:?} {noon:?}"
        );
        assert_eq!(t(-3.), [0.; 3], "set: no direct sun");
        // At sunset the sky toward the sun is orange; overhead still blue.
        let sun = sun_at(1.);
        let toward = atmosphere(&a, sun, [0., 0.03, -1.]);
        assert!(toward[0] > toward[2], "orange toward the sun {toward:?}");
        // Twilight: the sun 4 degrees down still lights the sky, dimmer
        // than day, brightest over where it set.
        let sun = sun_at(-4.);
        let toward = atmosphere(&a, sun, [0., 0.05, -1.]);
        let away = atmosphere(&a, sun, [0., 0.05, 1.]);
        let day = atmosphere(&a, sun_at(30.), [0., 0.05, 1.]);
        assert!(
            luma(toward) > 1e-3 && luma(toward) < 0.3 * luma(day),
            "{toward:?}"
        );
        assert!(luma(toward) > luma(away), "{toward:?} {away:?}");
    }

    #[test]
    fn haze_whitens_and_ozone_blues_the_sky() {
        let sun = sun_at(30.);
        let d = [1., 0.15, 0.];
        let ratio = |a: &Atmosphere| {
            let c = atmosphere(a, sun, d);
            c[0] / c[2]
        };
        let hazy = Atmosphere {
            turbidity: 8.,
            ..clear()
        };
        assert!(ratio(&hazy) > ratio(&clear()) * 1.2);
        let no_ozone = Atmosphere {
            ozone: 0.,
            ..clear()
        };
        // Ozone takes red and green out of a low sun's long path.
        let low = sun_at(-2.);
        let z = |a: &Atmosphere| atmosphere(a, low, [0., 1., 0.]);
        assert!(z(&clear())[2] / z(&clear())[0] > z(&no_ozone)[2] / z(&no_ozone)[0]);
    }

    fn srgb(c: f32) -> f32 {
        if c <= 0.003_130_8 {
            c * 12.92
        } else {
            1.055 * c.powf(1. / 2.4) - 0.055
        }
    }

    /// The stage draws the physical sky `atmosphere` computes: the middle
    /// of a narrow view each way matches it, so the WGSL is this model.
    #[test]
    fn the_stage_draws_the_physical_sky_this_model_computes() {
        use crate::{Camera, Post, Shot, Stage};
        let Ok(mut stage) = Stage::new(16, 16) else {
            eprintln!("no GPU, skipped");
            return;
        };
        let air = Atmosphere {
            turbidity: 3.,
            altitude: 400.,
            ..clear()
        };
        for (sun, d) in [
            (sun_at(35.), [0.2, 0.5, 0.84]),
            (sun_at(35.), [0.3, 0.97, 0.1]),
            (sun_at(3.), [0.05, 0.04, -1.]),
            (sun_at(3.), [0.7, 0.1, 0.7]),
            (sun_at(-3.), [0., 0.08, -1.]),
            (sun_at(20.), [0.6, -0.15, 0.78]),
        ] {
            let d = norm(d);
            let sky = Sky::physical(sun, air, 0., [0.; 2]);
            let shot = Shot {
                sky: Some(sky),
                post: Post::NONE,
                ..Shot::new(Camera {
                    eye: [0.; 3],
                    target: d,
                    fov: 2.,
                    roll: 0.,
                })
            };
            let f = stage.render(0., 0., 1, &|_| shot.clone()).unwrap();
            let px = &f.rgba[(8 * 16 + 8) * 4..][..3];
            let want = atmosphere(&air, sun, d).map(|v| srgb(v).min(1.));
            for c in 0..3 {
                assert!(
                    (px[c] - want[c]).abs() < 0.02 + 0.03 * want[c],
                    "sun {sun:?} along {d:?}: drew {px:?}, model {want:?}"
                );
            }
        }
    }

    /// A white card facing the camera, lit only by a physical sky behind
    /// the camera: its light is the sky's (blue without the sun's lamp),
    /// dims through twilight, and without `light` the card is unlit.
    #[test]
    fn a_sky_lights_the_shot_as_an_environment_does() {
        use crate::{Camera, Plane, Post, Shot, Stage};
        use mui_scene::prelude::*;
        let Ok(mut stage) = Stage::new(32, 32) else {
            eprintln!("no GPU, skipped");
            return;
        };
        let root = block(16., 16.).radius(0.).fill(Color::srgb(1., 1., 1.));
        let scene = resolve(&SceneSpec::new(root)).expect("resolves");
        stage
            .layer("white", &scene, Size::new(16., 16.), 1.)
            .unwrap();
        let mut card = |deg: f32, light: bool| {
            let e = deg.to_radians();
            let sky = Sky {
                light,
                ..Sky::physical([0., e.sin(), e.cos()], clear(), 0., [0.; 2])
            };
            let shot = Shot {
                planes: vec![Plane::new("white", 400., 400.)],
                sky: Some(sky),
                post: Post::NONE,
                ..Shot::new(Camera::front(100., 30.))
            };
            let f = stage.render(0., 0., 1, &|_| shot.clone()).unwrap();
            let p = &f.rgba[(16 * 32 + 16) * 4..][..3];
            [p[0], p[1], p[2]]
        };
        let day = card(40., true);
        assert!(
            day[2] > day[0] && luma(day) > 0.15 && luma(day) < 0.9,
            "{day:?}"
        );
        let dusk = card(-7., true);
        assert!(luma(dusk) < 0.5 * luma(day), "{dusk:?} against {day:?}");
        let unlit = card(40., false);
        assert!(unlit.iter().all(|v| *v > 0.99), "unlit: {unlit:?}");
    }

    #[test]
    fn aerial_perspective_hazes_far_surfaces_toward_a_bluish_sky() {
        let a = clear();
        let (sun, d) = (sun_at(50.), [0., 0., 1.]);
        let (t0, l0) = aerial(&a, sun, d, 0.);
        assert!(t0.iter().all(|v| (*v - 1.).abs() < 1e-6) && l0 == [0.; 3]);
        let (near, far) = (aerial(&a, sun, d, 2.), aerial(&a, sun, d, 60.));
        // Blue goes first and comes back as the sky's.
        assert!(far.0[2] < far.0[0] && far.1[2] > far.1[0], "{far:?}");
        assert!(luma(far.1) > luma(near.1) && luma(far.0) < luma(near.0));
        // Very far, a surface is about the horizon's brightness.
        let (_, add) = aerial(&a, sun, d, 1000.);
        let horizon = atmosphere(&a, sun, [0., 0.01, 1.]);
        let r = luma(add) / luma(horizon);
        assert!((0.4..2.5).contains(&r), "{add:?} against {horizon:?}");
    }

    #[test]
    fn auto_exposure_leaves_the_day_alone_and_opens_up_for_twilight() {
        let at = |deg| Sky::physical(sun_at(deg), clear(), 0., [0.; 2]).auto_exposure();
        assert_eq!(at(60.), 1.);
        assert_eq!(at(30.), 1.);
        let mut last = 1.;
        for deg in [10., 4., 1., -2., -4., -6.] {
            let e = at(deg);
            assert!(e >= last, "{deg}: {e} after {last}");
            last = e;
        }
        assert!((4. ..=64.).contains(&at(-4.)), "{}", at(-4.));
        // Intensity is the artist's: the meter does not undo it.
        let dim = Sky::physical(sun_at(-4.), Atmosphere { intensity: 0.5, ..clear() }, 0., [0.; 2]);
        assert_eq!(dim.auto_exposure(), at(-4.));
        assert_eq!(Sky::default().auto_exposure(), 1.);
    }

    /// A white card `dist` units off, unlit (the sky lights nothing), seen
    /// through `aerial`: the stage draws what [`aerial`] computes.
    #[test]
    fn the_stage_hazes_a_far_card_as_the_model_does() {
        use crate::{Camera, Plane, Post, Shot, Stage};
        use mui_scene::prelude::*;
        let Ok(mut stage) = Stage::new(16, 16) else {
            eprintln!("no GPU, skipped");
            return;
        };
        let root = block(16., 16.).radius(0.).fill(Color::srgb(1., 1., 1.));
        let scene = resolve(&SceneSpec::new(root)).expect("resolves");
        stage.layer("white", &scene, Size::new(16., 16.), 1.).unwrap();
        let air = Atmosphere { turbidity: 3., ..clear() };
        for (sun, dist, k) in [(sun_at(20.), 4000., 10.), (sun_at(3.), 3000., 25.), (sun_at(-3.), 4000., 20.)] {
            let sky = Sky {
                light: false,
                aerial: k,
                ..Sky::physical(sun, air, 0., [0.; 2])
            };
            let shot = Shot {
                planes: vec![Plane::new("white", dist, dist)],
                sky: Some(sky),
                post: Post::NONE,
                ..Shot::new(Camera {
                    eye: [0., 0., dist],
                    target: [0.; 3],
                    fov: 30.,
                    roll: 0.,
                })
            };
            let f = stage.render(0., 0., 1, &|_| shot.clone()).unwrap();
            let px = &f.rgba[(8 * 16 + 8) * 4..][..3];
            let (tr, add) = aerial(&air, sun, [0., 0., -1.], dist * k / 1000.);
            let want: [f32; 3] = std::array::from_fn(|c| srgb(tr[c] + add[c]).min(1.));
            for c in 0..3 {
                assert!(
                    (px[c] - want[c]).abs() < 0.02 + 0.03 * want[c],
                    "sun {sun:?} at {dist}: drew {px:?}, model {want:?}"
                );
            }
        }
    }

    /// Looking up into a clouded physical sky: the clouds are there, the
    /// same at the same time, moved by their drift, and thicker clouds
    /// darker underneath away from the sun.
    #[test]
    fn the_clouds_are_deterministic_drift_and_darken_as_they_thicken() {
        use crate::{Camera, Post, Shot, Stage};
        let Ok(mut stage) = Stage::new(48, 48) else {
            eprintln!("no GPU, skipped");
            return;
        };
        let mut frame = |cover: f32, density: f32, drift: f32| {
            let sky = Sky {
                density,
                ..Sky::physical(sun_at(35.), clear(), cover, [drift, 0.])
            };
            let shot = Shot {
                sky: Some(sky),
                post: Post::NONE,
                // Up and away from the sun.
                ..Shot::new(Camera {
                    eye: [0.; 3],
                    target: [0., 0.8, 0.6],
                    fov: 60.,
                    roll: 0.,
                })
            };
            stage.render(0., 0., 1, &|_| shot.clone()).unwrap().rgba
        };
        let mean = |f: &[f32]| f.chunks(4).map(|p| luma([p[0], p[1], p[2]])).sum::<f32>() / (f.len() / 4) as f32;
        let diff = |a: &[f32], b: &[f32]| a.iter().zip(b).map(|(x, y)| (x - y).abs()).sum::<f32>() / a.len() as f32;
        let clear_sky = frame(0., 1., 0.);
        let cloudy = frame(0.7, 1., 0.);
        assert!(diff(&clear_sky, &cloudy) > 0.02, "clouds show");
        assert_eq!(frame(0.7, 1., 0.), cloudy, "the same time, the same clouds");
        assert!(diff(&frame(0.7, 1., 0.5), &cloudy) > 0.01, "drift moves them");
        let (thin, thick) = (mean(&frame(0.7, 0.4, 0.)), mean(&frame(0.7, 3., 0.)));
        assert!(thick < thin, "dark bellies: {thick} against {thin}");
    }
}
