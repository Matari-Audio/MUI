//! BT.709 limited-range 4:2:0 on the CPU: the reference the GPU's
//! `yuv.wgsl` is tested against, and what the CPU renderer hands ffmpeg.

/// The planes' layout: NV12 (8-bit) or P010 (10 bits in 16).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Yuv {
    Nv12,
    P010,
}

impl Yuv {
    /// ffmpeg's name for it.
    pub fn pix_fmt(self) -> &'static str {
        match self {
            Self::Nv12 => "nv12",
            Self::P010 => "p010le",
        }
    }
    /// Bytes of one frame, planes packed tight.
    pub fn frame_bytes(self, [w, h]: [u32; 2]) -> usize {
        let px = w as usize * h as usize * 3 / 2;
        match self {
            Self::Nv12 => px,
            Self::P010 => px * 2,
        }
    }
}

const KR: f32 = 0.2126;
const KB: f32 = 0.0722;

/// Straight sRGB RGBA bytes (even sides) to `yuv`, planes packed tight.
pub fn from_rgba(rgba: &[u8], [w, h]: [u32; 2], yuv: Yuv) -> Vec<u8> {
    let (w, h) = (w as usize, h as usize);
    let c = |x: usize, y: usize| -> [f32; 3] {
        let p = &rgba[(y * w + x) * 4..];
        [p[0], p[1], p[2]].map(|v| f32::from(v) / 255.)
    };
    let l = |p: [f32; 3]| KR * p[0] + (1. - KR - KB) * p[1] + KB * p[2];
    let mut ys = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            ys.push((16. + 219. * l(c(x, y))) / 255.);
        }
    }
    let mut uvs = Vec::with_capacity(w * h / 2);
    for y in (0..h).step_by(2) {
        for x in (0..w).step_by(2) {
            let (a, b, d, e) = (c(x, y), c(x + 1, y), c(x, y + 1), c(x + 1, y + 1));
            let s: [f32; 3] = std::array::from_fn(|i| (a[i] + b[i] + d[i] + e[i]) / 4.);
            let l = l(s);
            uvs.push((128. + 224. * (s[2] - l) / 1.8556) / 255.);
            uvs.push((128. + 224. * (s[0] - l) / 1.5748) / 255.);
        }
    }
    let all = ys.into_iter().chain(uvs);
    match yuv {
        Yuv::Nv12 => all
            .map(|v| (v * 255. + 0.5).clamp(0., 255.) as u8)
            .collect(),
        Yuv::P010 => all
            .flat_map(|v| (((v * 1023. + 0.5).clamp(0., 1023.) as u16) << 6).to_le_bytes())
            .collect(),
    }
}
