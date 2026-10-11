//! Image-based light: an equirectangular HDR environment, prefiltered once
//! on the CPU when it is uploaded (the same numbers native and on the web).
//! Specular is split-sum: a mip chain whose level `m` is the environment
//! convolved with a GGX lobe of roughness `m / (LEVELS - 1)`, and a BRDF LUT
//! of the scale and bias applied to F0. Diffuse is the environment's
//! irradiance as nine spherical harmonics.
//!
//! Directions are the stage's world (y up, z toward the viewer). A texel's
//! `u` runs with `atan2(z, x)` from 0.5 at +x and `v` from the zenith (0)
//! to the nadir (1): Blender's equirectangular mapping, once its z-up axes
//! are turned to the stage's, so one image lights both the same way.
use std::f32::consts::PI;

/// Mip levels of the specular chain: level 0 is a mirror, the last is
/// roughness 1.
pub const LEVELS: u32 = 6;
/// Width of level 0 (the chain's height is half its width).
pub const WIDTH: u32 = 512;
/// Side of the BRDF LUT.
pub const LUT: u32 = 32;
const SAMPLES: u32 = 64;

/// A linear-light equirectangular environment, top row the zenith.
#[derive(Clone, Debug, PartialEq)]
pub struct EnvImage {
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<[f32; 3]>,
}

/// The mip level that holds roughness `r`: linear, 0 a mirror.
pub fn mip_of_roughness(r: f32) -> f32 {
    r.clamp(0., 1.) * (LEVELS - 1) as f32
}

/// The roughness mip level `m` was convolved with.
pub fn roughness_of_mip(m: u32) -> f32 {
    m.min(LEVELS - 1) as f32 / (LEVELS - 1) as f32
}

/// A unit direction's texel coordinates, 0..1.
pub fn uv_of(d: [f32; 3]) -> [f32; 2] {
    [
        0.5 + d[2].atan2(d[0]) / (2. * PI),
        0.5 - d[1].clamp(-1., 1.).asin() / PI,
    ]
}

/// The unit direction at texel coordinates `uv`.
pub fn dir_of([u, v]: [f32; 2]) -> [f32; 3] {
    let (phi, el) = ((u - 0.5) * 2. * PI, (0.5 - v) * PI);
    [el.cos() * phi.cos(), el.sin(), el.cos() * phi.sin()]
}

impl EnvImage {
    /// `width`x`height` texels of `f(direction)`.
    pub fn from_fn(width: u32, height: u32, f: impl Fn([f32; 3]) -> [f32; 3]) -> Self {
        let rgb = (0..height)
            .flat_map(|y| (0..width).map(move |x| (x, y)))
            .map(|(x, y)| {
                f(dir_of([
                    (x as f32 + 0.5) / width as f32,
                    (y as f32 + 0.5) / height as f32,
                ]))
            })
            .collect();
        Self { width, height, rgb }
    }

    /// The built-in neutral studio: a graphite room, lighter overhead, a
    /// broad soft key up front left, a fill right and a strip behind, so a
    /// metal has something to mirror. Grey, and dim enough that nothing
    /// blooms.
    pub fn studio() -> Self {
        Self::from_fn(WIDTH, WIDTH / 2, studio)
    }

    /// Bilinear, wrapping round in `u`.
    pub fn sample(&self, [u, v]: [f32; 2]) -> [f32; 3] {
        let (w, h) = (self.width as i64, self.height as i64);
        let x = u * self.width as f32 - 0.5;
        let y = (v * self.height as f32 - 0.5).clamp(0., (h - 1) as f32);
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let at = |x: i64, y: i64| self.rgb[(y.clamp(0, h - 1) * w + x.rem_euclid(w)) as usize];
        let (x0, y0) = (x0 as i64, y0 as i64);
        let [a, b, c, d] = [
            at(x0, y0),
            at(x0 + 1, y0),
            at(x0, y0 + 1),
            at(x0 + 1, y0 + 1),
        ];
        std::array::from_fn(|i| {
            (a[i] * (1. - fx) + b[i] * fx) * (1. - fy) + (c[i] * (1. - fx) + d[i] * fx) * fy
        })
    }

    /// Half the size, each texel the mean of four.
    pub(crate) fn half(&self) -> Self {
        let (w, h) = ((self.width / 2).max(1), (self.height / 2).max(1));
        let at = |x: u32, y: u32| {
            self.rgb[(y.min(self.height - 1) * self.width + x.min(self.width - 1)) as usize]
        };
        let rgb = (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .map(|(x, y)| {
                let q = [
                    at(2 * x, 2 * y),
                    at(2 * x + 1, 2 * y),
                    at(2 * x, 2 * y + 1),
                    at(2 * x + 1, 2 * y + 1),
                ];
                std::array::from_fn(|i| q.iter().map(|p| p[i]).sum::<f32>() * 0.25)
            })
            .collect();
        Self {
            width: w,
            height: h,
            rgb,
        }
    }

    /// This image as the chain's level 0: halved while it is larger, then
    /// resampled to exactly [`WIDTH`] by half that.
    pub(crate) fn base(&self) -> Self {
        if (self.width, self.height) == (WIDTH, WIDTH / 2) {
            return self.clone();
        }
        let mut img = self.clone();
        while img.width >= 2 * WIDTH {
            img = img.half();
        }
        Self::from_fn(WIDTH, WIDTH / 2, |d| img.sample(uv_of(d)))
    }

    /// A Radiance `.hdr` (RGBE, flat or run-length encoded, `-Y H +X W`).
    pub fn from_hdr(bytes: &[u8]) -> Result<Self, String> {
        let mut pos = 0;
        let mut line = || -> Result<&[u8], String> {
            let end = bytes[pos..]
                .iter()
                .position(|&b| b == b'\n')
                .ok_or("hdr: header ends early")?;
            let l = &bytes[pos..pos + end];
            pos += end + 1;
            Ok(l)
        };
        if !line()?.starts_with(b"#?") {
            return Err("hdr: not a Radiance file".into());
        }
        while !line()?.is_empty() {}
        let res = String::from_utf8_lossy(line()?).into_owned();
        let f: Vec<&str> = res.split_whitespace().collect();
        let (height, width) = match f[..] {
            ["-Y", h, "+X", w] => (h.parse::<u32>(), w.parse::<u32>()),
            _ => return Err(format!("hdr: layout `{res}` (only -Y H +X W)")),
        };
        let (width, height) = (
            width.map_err(|e| e.to_string())?,
            height.map_err(|e| e.to_string())?,
        );
        if width == 0 || height == 0 || u64::from(width) * u64::from(height) > 1 << 26 {
            return Err(format!("hdr: {width}x{height}"));
        }
        let mut data = &bytes[pos..];
        let mut rgbe = vec![[0u8; 4]; (width * height) as usize];
        let short = || "hdr: pixel data ends early".to_owned();
        for row in rgbe.chunks_mut(width as usize) {
            let rle = (8..32768).contains(&width)
                && data.len() >= 4
                && data[0] == 2
                && data[1] == 2
                && usize::from(data[2]) << 8 | usize::from(data[3]) == width as usize;
            if !rle {
                for px in row.iter_mut() {
                    *px = data.get(..4).ok_or_else(short)?.try_into().expect("4");
                    data = &data[4..];
                }
                continue;
            }
            data = &data[4..];
            for c in 0..4 {
                let mut x = 0;
                while x < row.len() {
                    let &n = data.first().ok_or_else(short)?;
                    if n > 128 {
                        let run = usize::from(n - 128);
                        let &v = data.get(1).ok_or_else(short)?;
                        for px in row.get_mut(x..x + run).ok_or_else(short)? {
                            px[c] = v;
                        }
                        (x, data) = (x + run, &data[2..]);
                    } else {
                        let n = usize::from(n.max(1));
                        let src = data.get(1..=n).ok_or_else(short)?;
                        for (px, &v) in row.get_mut(x..x + n).ok_or_else(short)?.iter_mut().zip(src)
                        {
                            px[c] = v;
                        }
                        (x, data) = (x + n, &data[n + 1..]);
                    }
                }
            }
        }
        let rgb = rgbe
            .iter()
            .map(|p| {
                if p[3] == 0 {
                    return [0.; 3];
                }
                let k = 2f32.powi(i32::from(p[3]) - 136);
                [0, 1, 2].map(|i| (f32::from(p[i]) + 0.5) * k)
            })
            .collect();
        Ok(Self { width, height, rgb })
    }

    /// An OpenEXR file's first RGB(A) layer.
    pub fn from_exr(bytes: &[u8]) -> Result<Self, String> {
        use exr::prelude::*;
        let img = read()
            .no_deep_data()
            .largest_resolution_level()
            .rgba_channels(
                |size, _| EnvImage {
                    width: size.width() as u32,
                    height: size.height() as u32,
                    rgb: vec![[0.; 3]; size.area()],
                },
                |img: &mut EnvImage, p: Vec2<usize>, (r, g, b, _a): (f32, f32, f32, f32)| {
                    img.rgb[p.y() * img.width as usize + p.x()] = [r, g, b];
                },
            )
            .first_valid_layer()
            .all_attributes()
            .from_buffered(std::io::Cursor::new(bytes))
            .map_err(|e| format!("exr: {e}"))?;
        Ok(img.layer_data.channel_data.pixels)
    }

    /// `.hdr` or `.exr` by the file name's extension.
    pub fn decode(path: &str, bytes: &[u8]) -> Result<Self, String> {
        match path
            .rsplit('.')
            .next()
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("hdr") => Self::from_hdr(bytes),
            Some("exr") => Self::from_exr(bytes),
            _ => Err(format!("{path}: an environment is a .hdr or .exr")),
        }
    }

    /// As an uncompressed-enough (ZIP) RGB OpenEXR file.
    pub fn to_exr(&self) -> Result<Vec<u8>, String> {
        use exr::prelude::*;
        let w = self.width as usize;
        let channels = SpecificChannels::rgb(|p: Vec2<usize>| {
            let [r, g, b] = self.rgb[p.y() * w + p.x()];
            (r, g, b)
        });
        let layer = Layer::new(
            (w, self.height as usize),
            LayerAttributes::default(),
            Encoding::SMALL_LOSSLESS,
            channels,
        );
        let mut out = std::io::Cursor::new(Vec::new());
        Image::from_layer(layer)
            .write()
            .to_buffered(&mut out)
            .map_err(|e| format!("exr: {e}"))?;
        Ok(out.into_inner())
    }
}

fn smooth(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0., 1.);
    t * t * (3. - 2. * t)
}

/// The studio's radiance toward `d`.
fn studio(d: [f32; 3]) -> [f32; 3] {
    let y = d[1];
    let room = if y < 0. {
        0.05 + 0.04 * smooth(-0.35, 0., y)
    } else {
        0.09 + 0.23 * smooth(0., 1., y)
    };
    let [u, v] = uv_of(d);
    // A soft-edged panel centred at (u, v) texel coordinates.
    let panel = |cu: f32, cv: f32, hu: f32, hv: f32, k: f32| {
        let du = (u - cu + 0.5).rem_euclid(1.) - 0.5;
        k * (1. - smooth(hu * 0.7, hu, du.abs())) * (1. - smooth(hv * 0.7, hv, (v - cv).abs()))
    };
    // u 0.75 faces +z (toward the camera), 0.25 faces -z (behind the set).
    let lamps = panel(0.66, 0.26, 0.09, 0.09, 2.6)
        + panel(0.88, 0.36, 0.05, 0.08, 1.1)
        + panel(0.25, 0.3, 0.16, 0.025, 1.6);
    [room + lamps; 3]
}

/// Level `m` of the chain from the source pyramid: GGX-filtered with the
/// level's roughness, reading the pyramid at each sample's footprint
/// (filtered importance sampling) so a few samples do not sparkle.
#[cfg(test)]
fn convolve(pyramid: &[EnvImage], m: u32) -> EnvImage {
    let rough = roughness_of_mip(m);
    let a = (rough * rough).max(1e-4);
    let base = &pyramid[0];
    let texel = 4. * PI / (base.width * base.height) as f32;
    let samples: Vec<([f32; 3], f32, f32)> = (0..SAMPLES)
        .map(|i| {
            // A GGX half vector about +z, its pdf toward the light (N = V).
            let (e1, e2) = ((i as f32 + 0.5) / SAMPLES as f32, radical_inverse(i));
            let phi = 2. * PI * e2;
            let cos = ((1. - e1) / (1. + (a * a - 1.) * e1)).sqrt();
            let sin = (1. - cos * cos).max(0.).sqrt();
            let h = [sin * phi.cos(), sin * phi.sin(), cos];
            let dd = (cos * cos * (a * a - 1.) + 1.).max(1e-7);
            let ggx = a * a / (PI * dd * dd);
            let pdf = ggx / 4.;
            let lod = (0.5 * (1. / (SAMPLES as f32 * pdf) / texel).log2() + 1.).max(0.);
            // L = 2 (V.H) H - V with V = N = +z.
            let l = [2. * cos * h[0], 2. * cos * h[1], 2. * cos * cos - 1.];
            (l, l[2], lod)
        })
        .filter(|s| s.1 > 0.)
        .collect();
    let at = |lod: f32, d: [f32; 3]| {
        let top = pyramid.len() - 1;
        let lo = (lod.floor() as usize).min(top);
        let hi = (lo + 1).min(top);
        let f = (lod - lo as f32).clamp(0., 1.);
        let (p, q) = (pyramid[lo].sample(uv_of(d)), pyramid[hi].sample(uv_of(d)));
        std::array::from_fn::<f32, 3, _>(|i| p[i] * (1. - f) + q[i] * f)
    };
    let w = WIDTH >> m;
    EnvImage::from_fn(w, w / 2, |n| {
        // A frame about n.
        let up = if n[1].abs() < 0.999 {
            [0., 1., 0.]
        } else {
            [1., 0., 0.]
        };
        let t = normalize(cross(up, n));
        let b = cross(n, t);
        let (mut sum, mut weight) = ([0f32; 3], 0.);
        for &(l, ndl, lod) in &samples {
            let d = std::array::from_fn(|i| t[i] * l[0] + b[i] * l[1] + n[i] * l[2]);
            let c = at(lod, d);
            for i in 0..3 {
                sum[i] += c[i] * ndl;
            }
            weight += ndl;
        }
        sum.map(|s| s / weight)
    })
}

fn radical_inverse(i: u32) -> f32 {
    i.reverse_bits() as f32 * 2.328_306_4e-10
}
#[cfg(test)]
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
#[cfg(test)]
fn normalize(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-12);
    v.map(|c| c / l)
}

/// The nine real SH basis functions at unit `d`.
fn basis([x, y, z]: [f32; 3]) -> [f32; 9] {
    [
        0.282_095,
        0.488_603 * y,
        0.488_603 * z,
        0.488_603 * x,
        1.092_548 * x * y,
        1.092_548 * y * z,
        0.315_392 * (3. * z * z - 1.),
        1.092_548 * x * z,
        0.546_274 * (x * x - y * y),
    ]
}

/// The environment's irradiance as nine SH coefficients, each already
/// times its band's cosine-lobe factor over π, so [`irradiance`] is what a
/// white Lambertian surface reflects.
pub fn sh9(img: &EnvImage) -> [[f32; 3]; 9] {
    let mut sh = [[0f32; 3]; 9];
    let (w, h) = (img.width as f32, img.height as f32);
    for y in 0..img.height {
        let v = (y as f32 + 0.5) / h;
        // A texel's solid angle: its share of 2π across by π down, times
        // cos(elevation).
        let da = (2. * PI / w) * (PI / h) * ((0.5 - v) * PI).cos();
        for x in 0..img.width {
            let d = dir_of([(x as f32 + 0.5) / w, v]);
            let c = img.rgb[(y * img.width + x) as usize];
            for (k, b) in basis(d).into_iter().enumerate() {
                for i in 0..3 {
                    sh[k][i] += c[i] * b * da;
                }
            }
        }
    }
    let band = [1., 2. / 3., 2. / 3., 2. / 3., 0.25, 0.25, 0.25, 0.25, 0.25];
    for (k, s) in sh.iter_mut().enumerate() {
        *s = s.map(|c| c * band[k]);
    }
    sh
}

/// Diffuse light off a white surface facing unit `n`, from [`sh9`].
pub fn irradiance(sh: &[[f32; 3]; 9], n: [f32; 3]) -> [f32; 3] {
    let b = basis(n);
    std::array::from_fn(|i| (0..9).map(|k| sh[k][i] * b[k]).sum::<f32>().max(0.))
}

/// The split-sum BRDF LUT, [`LUT`] square: `x` is N.V, `y` roughness; each
/// texel the scale and bias on F0 (GGX, height-correlated Smith with the
/// IBL `k`).
pub fn brdf_lut() -> Vec<[f32; 2]> {
    let mut out = Vec::with_capacity((LUT * LUT) as usize);
    for y in 0..LUT {
        let rough = (y as f32 + 0.5) / LUT as f32;
        let a = rough * rough;
        let k = a / 2.;
        for x in 0..LUT {
            let nv = ((x as f32 + 0.5) / LUT as f32).max(1e-3);
            let v = [(1. - nv * nv).sqrt(), 0., nv];
            let (mut sa, mut sb) = (0f32, 0f32);
            for i in 0..SAMPLES {
                let (e1, e2) = ((i as f32 + 0.5) / SAMPLES as f32, radical_inverse(i));
                let phi = 2. * PI * e2;
                let cos = ((1. - e1) / (1. + (a * a - 1.) * e1)).sqrt();
                let sin = (1. - cos * cos).max(0.).sqrt();
                let h = [sin * phi.cos(), sin * phi.sin(), cos];
                let vh = v[0] * h[0] + v[1] * h[1] + v[2] * h[2];
                let l = std::array::from_fn::<f32, 3, _>(|i| 2. * vh * h[i] - v[i]);
                let (nl, nh) = (l[2].max(0.), h[2].max(0.));
                if nl <= 0. {
                    continue;
                }
                let g1 = |c: f32| c / (c * (1. - k) + k);
                let g = g1(nv) * g1(nl);
                let vis = g * vh.max(0.) / (nh * nv).max(1e-6);
                let fc = (1. - vh.max(0.)).powi(5);
                sa += (1. - fc) * vis;
                sb += fc * vis;
            }
            out.push([sa / SAMPLES as f32, sb / SAMPLES as f32]);
        }
    }
    out
}

/// The specular chain, level 0 first, and the irradiance SH.
#[cfg(test)]
pub(crate) struct Prefiltered {
    pub levels: Vec<EnvImage>,
    pub sh: [[f32; 3]; 9],
}

/// The irradiance SH from a chain's level 0, as [`prefilter`] finds it.
pub(crate) fn sh_of(base: &EnvImage) -> [[f32; 3]; 9] {
    sh9(&base.half().half().half())
}

/// The CPU reference for the stage's GPU prefilter (`env.wgsl`).
#[cfg(test)]
pub(crate) fn prefilter(img: &EnvImage) -> Prefiltered {
    let mut pyramid = vec![img.base()];
    while pyramid.last().expect("one").width > 4 {
        let next = pyramid.last().expect("one").half();
        pyramid.push(next);
    }
    let sh = sh9(&pyramid[3]);
    let mut levels = vec![pyramid[0].clone()];
    levels.extend((1..LEVELS).map(|m| convolve(&pyramid, m)));
    Prefiltered { levels, sh }
}
