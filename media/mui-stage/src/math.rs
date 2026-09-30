//! The five matrices a camera and a slab need: column-major, right-handed,
//! clip depth 0..1 as wgpu wants it.
use std::ops::Mul;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mat4(pub [f32; 16]);

impl Mat4 {
    pub const IDENTITY: Self = Self([
        1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
    ]);
    pub fn translate([x, y, z]: [f32; 3]) -> Self {
        let mut m = Self::IDENTITY;
        m.0[12..15].copy_from_slice(&[x, y, z]);
        m
    }
    pub fn scale(s: f32) -> Self {
        let mut m = Self::IDENTITY;
        m.0[0] = s;
        m.0[5] = s;
        m.0[10] = s;
        m
    }
    pub fn rotate_x(a: f32) -> Self {
        let (s, c) = a.sin_cos();
        Self([1., 0., 0., 0., 0., c, s, 0., 0., -s, c, 0., 0., 0., 0., 1.])
    }
    pub fn rotate_y(a: f32) -> Self {
        let (s, c) = a.sin_cos();
        Self([c, 0., -s, 0., 0., 1., 0., 0., s, 0., c, 0., 0., 0., 0., 1.])
    }
    pub fn rotate_z(a: f32) -> Self {
        let (s, c) = a.sin_cos();
        Self([c, s, 0., 0., -s, c, 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.])
    }
    #[rustfmt::skip]
    pub fn perspective(fov_y: f32, aspect: f32, near: f32, far: f32) -> Self {
        let f = 1. / (fov_y * 0.5).tan();
        let r = far / (near - far);
        Self([
            f / aspect, 0., 0., 0.,
            0., f, 0., 0.,
            0., 0., r, -1.,
            0., 0., r * near, 0.,
        ])
    }
    /// An orthographic box looking down -z, `near..far` in front of it
    /// mapped to depth 0..1.
    #[rustfmt::skip]
    pub fn orthographic(half_w: f32, half_h: f32, near: f32, far: f32) -> Self {
        let r = 1. / (near - far);
        Self([
            1. / half_w, 0., 0., 0.,
            0., 1. / half_h, 0., 0.,
            0., 0., r, 0.,
            0., 0., near * r, 1.,
        ])
    }
    pub fn look_at(eye: [f32; 3], target: [f32; 3], up: [f32; 3]) -> Self {
        let norm = |v: [f32; 3]| {
            let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-12);
            v.map(|c| c / l)
        };
        let cross = |a: [f32; 3], b: [f32; 3]| {
            [
                a[1] * b[2] - a[2] * b[1],
                a[2] * b[0] - a[0] * b[2],
                a[0] * b[1] - a[1] * b[0],
            ]
        };
        let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
        let f = norm([target[0] - eye[0], target[1] - eye[1], target[2] - eye[2]]);
        let s = norm(cross(f, up));
        let u = cross(s, f);
        #[rustfmt::skip]
        let m = Self([
            s[0], u[0], -f[0], 0.,
            s[1], u[1], -f[1], 0.,
            s[2], u[2], -f[2], 0.,
            -dot(s, eye), -dot(u, eye), dot(f, eye), 1.,
        ]);
        m
    }
    /// `p` through the matrix, divided by w.
    pub fn project(&self, p: [f32; 3]) -> [f32; 3] {
        let m = &self.0;
        let v = |r: usize| m[r] * p[0] + m[4 + r] * p[1] + m[8 + r] * p[2] + m[12 + r];
        let w = v(3);
        [v(0) / w, v(1) / w, v(2) / w]
    }
}

impl Mul for Mat4 {
    type Output = Self;
    fn mul(self, o: Self) -> Self {
        let mut m = [0.; 16];
        for c in 0..4 {
            for r in 0..4 {
                m[c * 4 + r] = (0..4).map(|k| self.0[k * 4 + r] * o.0[c * 4 + k]).sum();
            }
        }
        Self(m)
    }
}

/// The radical inverse of `i` in `base`: the Halton sequence's value.
pub fn halton(mut i: u32, base: u32) -> f32 {
    let (mut f, mut r) = (1f32, 0f32);
    while i > 0 {
        f /= base as f32;
        r += f * (i % base) as f32;
        i /= base;
    }
    r
}

/// Beauty sample `i`: eight 0..1 numbers from a Halton sequence, one prime
/// base each, the same on every run. In pairs: the pixel offset, the lens,
/// the area lights and the occlusion's turn.
pub fn sample(i: u32) -> [f32; 8] {
    const BASES: [u32; 8] = [2, 3, 5, 7, 11, 13, 17, 19];
    // Index 0 is 0 in every base: start at 1.
    BASES.map(|b| halton(i + 1, b))
}

/// A 0..1 square onto the unit disc, uniformly.
pub fn disc(u: f32, v: f32) -> [f32; 2] {
    let (r, a) = (u.sqrt(), v * std::f32::consts::TAU);
    [r * a.cos(), r * a.sin()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_front_camera_maps_the_layer_edges_to_the_frame_edges() {
        // Camera::front's distance: a 720-tall layer fills a 35-degree view.
        let fov = 35f32.to_radians();
        let z = 360. / (fov * 0.5).tan();
        let vp = Mat4::perspective(fov, 16. / 9., 1., 1e5)
            * Mat4::look_at([0., 0., z], [0.; 3], [0., 1., 0.]);
        let top = vp.project([0., 360., 0.]);
        let right = vp.project([640., 0., 0.]);
        assert!((top[1] - 1.).abs() < 1e-4, "{top:?}");
        assert!((right[0] - 1.).abs() < 1e-4, "{right:?}");
        assert!(top[2] > 0. && top[2] < 1.);
    }

    #[test]
    fn a_punch_in_centres_its_point_and_closes_in() {
        let base = crate::Camera::front(360., 35.);
        let c = base.punch([640., 360.], [160., 90.], 2.);
        // (160, 90) y-down in a 640x360 canvas is (-160, 90) in the world.
        assert_eq!(&c.target[..2], &[-160., 90.]);
        assert!((c.distance() - base.distance() / 2.).abs() < 1e-3);
        let vp = Mat4::perspective(35f32.to_radians(), 16. / 9., 1., 1e5)
            * Mat4::look_at(c.eye, c.target, [0., 1., 0.]);
        let p = vp.project([-160., 90., 0.]);
        assert!(p[0].abs() < 1e-5 && p[1].abs() < 1e-5);
    }

    #[test]
    fn an_orthographic_box_maps_near_and_far_to_zero_and_one() {
        let m = Mat4::orthographic(100., 50., 10., 110.);
        assert_eq!(m.project([100., -50., -10.]), [1., -1., 0.]);
        let far = m.project([0., 0., -110.]);
        assert!((far[2] - 1.).abs() < 1e-6, "{far:?}");
    }

    #[test]
    fn beauty_samples_are_a_fixed_low_discrepancy_sequence() {
        // The same numbers every call: a render is reproducible.
        assert_eq!(sample(5), sample(5));
        assert_eq!(halton(1, 2), 0.5);
        assert_eq!(halton(6, 2), 0.375);
        assert!((halton(4, 3) - 4. / 9.).abs() < 1e-6);
        // Stratified, unlike a random pick: sixteen samples put one pixel
        // offset in each sixteenth across, nine one in each ninth down.
        let mut across = [0; 16];
        let mut down = [0; 9];
        for i in 0..16 {
            across[(sample(i)[0] * 16.) as usize] += 1;
        }
        for i in 0..9 {
            down[(sample(i)[1] * 9.) as usize] += 1;
        }
        assert!(across.iter().all(|&c| c == 1), "{across:?}");
        assert!(down.iter().all(|&c| c == 1), "{down:?}");
        for i in 0..256 {
            assert!(sample(i).iter().all(|v| (0. ..1.).contains(v)));
        }
    }

    #[test]
    fn rotations_turn_the_right_way() {
        // +90 about y takes +x to -z (right-handed).
        let p = Mat4::rotate_y(std::f32::consts::FRAC_PI_2).project([1., 0., 0.]);
        assert!(p[0].abs() < 1e-6 && (p[2] + 1.).abs() < 1e-6, "{p:?}");
    }
}
