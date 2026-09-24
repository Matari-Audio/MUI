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
    fn rotations_turn_the_right_way() {
        // +90 about y takes +x to -z (right-handed).
        let p = Mat4::rotate_y(std::f32::consts::FRAC_PI_2).project([1., 0., 0.]);
        assert!(p[0].abs() < 1e-6 && (p[2] + 1.).abs() < 1e-6, "{p:?}");
    }
}
