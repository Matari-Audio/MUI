//! Opaque sRGB themes. Contrast is checked after gamut mapping and 8-bit encoding.
//! Alpha, gradients and image backdrops require a separate compositing policy.
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);
impl Rgb {
    pub const BLACK: Self = Self(0, 0, 0);
    pub const WHITE: Self = Self(255, 255, 255);
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self(r, g, b)
    }
    /// OKLCH lightness in 0..=1, nonnegative chroma, and hue in degrees.
    /// Out-of-gamut colors preserve lightness/hue while reducing chroma.
    pub fn from_oklch(l: f64, c: f64, h: f64) -> Result<Self, ColorError> {
        if !l.is_finite()
            || !c.is_finite()
            || !h.is_finite()
            || !(0.0..=1.0).contains(&l)
            || c < 0.0
        {
            return Err(ColorError::InvalidOklch);
        }
        let h = h.rem_euclid(360.0).to_radians();
        Ok(from_lab([l, c * h.cos(), c * h.sin()]))
    }
    pub fn channels(self) -> [u8; 3] {
        [self.0, self.1, self.2]
    }
    pub fn luminance(self) -> f64 {
        let [r, g, b] = self.channels().map(|v| linear(f64::from(v) / 255.));
        0.2126 * r + 0.7152 * g + 0.0722 * b
    }
    pub fn contrast(self, other: Self) -> f64 {
        let (a, b) = (self.luminance(), other.luminance());
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }
    /// Preserve a passing color. Otherwise choose the nearest sampled OKLab tone
    /// satisfying ALL backgrounds. The bounded search includes black/white
    /// fallbacks; it does not claim a globally optimal color or infeasibility proof.
    pub fn contrast_on(self, backgrounds: &[Self], minimum: f64) -> Result<Self, ColorError> {
        if !minimum.is_finite() || !(1.0..=21.0).contains(&minimum) {
            return Err(ColorError::InvalidRatio);
        }
        if backgrounds.is_empty() {
            return Err(ColorError::NoBackground);
        }
        let ratio = |color: Self| {
            backgrounds
                .iter()
                .map(|bg| color.contrast(*bg))
                .fold(f64::INFINITY, f64::min)
        };
        if ratio(self) >= minimum {
            return Ok(self);
        }
        let lab = to_lab(self);
        let mut best = None;
        let mut best_distance = f64::INFINITY;
        let mut attainable = 0_f64;
        for i in 0..=512 {
            let candidate = from_lab([i as f64 / 512., lab[1], lab[2]]);
            let contrast = ratio(candidate);
            attainable = attainable.max(contrast);
            if contrast >= minimum {
                let mapped = to_lab(candidate);
                let distance = (0..3).map(|j| (mapped[j] - lab[j]).powi(2)).sum::<f64>();
                if distance < best_distance {
                    best_distance = distance;
                    best = Some(candidate);
                }
            }
        }
        best.ok_or(ColorError::Unsatisfied {
            minimum,
            best: attainable,
        })
    }
    /// Theme-aware hover tone in OKLab. At an endpoint, move inward instead
    /// of clamping to an unchanged color. Contrast is checked by the style resolver.
    pub fn hovered(self, mode: Mode) -> Self {
        self.hovered_with(mode, 0.06)
    }
    pub(crate) fn hovered_with(self, mode: Mode, amount: f64) -> Self {
        let mut lab = to_lab(self);
        let delta = if mode == Mode::Dark { amount } else { -amount };
        let next = (lab[0] + delta).clamp(0., 1.);
        lab[0] = if (next - lab[0]).abs() < amount / 3. {
            (lab[0] - delta).clamp(0., 1.)
        } else {
            next
        };
        from_lab(lab)
    }
    fn tone(self, lightness: f64, chroma_limit: f64) -> Self {
        let mut lab = to_lab(self);
        let chroma = lab[1].hypot(lab[2]);
        if chroma > chroma_limit {
            lab[1] *= chroma_limit / chroma;
            lab[2] *= chroma_limit / chroma;
        }
        lab[0] = lightness;
        from_lab(lab)
    }
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ColorError {
    InvalidRatio,
    InvalidOklch,
    NoBackground,
    Unsatisfied { minimum: f64, best: f64 },
}
impl fmt::Display for ColorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidOklch => {
                f.write_str("OKLCH requires finite L in 0..1, C >= 0, and hue in degrees")
            }
            Self::InvalidRatio => f.write_str("contrast ratio must be finite and between 1 and 21"),
            Self::NoBackground => f.write_str("contrast needs at least one opaque background"),
            Self::Unsatisfied { minimum, best } => write!(
                f,
                "requested contrast {minimum}:1; best sampled result {best}:1"
            ),
        }
    }
}
impl std::error::Error for ColorError {}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Mode {
    Light,
    #[default]
    Dark,
}
/// Three design primaries, neutral, and success/warning/error/info seeds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Seeds {
    pub primary: [Rgb; 3],
    pub neutral: Rgb,
    pub status: [Rgb; 4],
}
impl Default for Seeds {
    fn default() -> Self {
        Self {
            primary: [Rgb(120, 100, 220), Rgb(40, 160, 140), Rgb(200, 100, 60)],
            neutral: Rgb(128, 128, 128),
            status: [
                Rgb(40, 150, 80),
                Rgb(200, 140, 20),
                Rgb(200, 50, 60),
                Rgb(60, 130, 210),
            ],
        }
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Palette {
    pub light: Seeds,
    pub dark: Option<Seeds>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Accent {
    pub fill: Rgb,
    pub on_fill: Rgb,
    pub soft: Rgb,
    pub on_soft: Rgb,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Colors {
    pub canvas: Rgb,
    pub panel: Rgb,
    pub raised: Rgb,
    pub text: Rgb,
    pub muted: Rgb,
    pub outline: Rgb,
    pub primary: [Accent; 3],
    pub status: [Accent; 4],
}
impl Palette {
    /// Resolve once per palette/mode change; layout and geometry do not depend on colors.
    pub fn resolve(self, mode: Mode) -> Result<Colors, ColorError> {
        let seeds = if mode == Mode::Dark {
            self.dark.unwrap_or(self.light)
        } else {
            self.light
        };
        let (canvas, panel, raised, text, muted, outline, accent, soft) = match mode {
            Mode::Light => (0.97, 0.94, 0.99, 0.20, 0.44, 0.52, 0.52, 0.90),
            Mode::Dark => (0.12, 0.18, 0.24, 0.94, 0.72, 0.62, 0.74, 0.30),
        };
        let canvas = seeds.neutral.tone(canvas, 0.025);
        let panel = seeds.neutral.tone(panel, 0.025);
        let raised = seeds.neutral.tone(raised, 0.025);
        let backgrounds = [canvas, panel, raised];
        let text = seeds
            .neutral
            .tone(text, 0.02)
            .contrast_on(&backgrounds, 4.5)?;
        let muted = seeds
            .neutral
            .tone(muted, 0.02)
            .contrast_on(&backgrounds, 4.5)?;
        let outline = seeds
            .neutral
            .tone(outline, 0.02)
            .contrast_on(&backgrounds, 3.)?;
        let make = |seed: Rgb| -> Result<Accent, ColorError> {
            let fill = seed.tone(accent, 0.25);
            let soft = seed.tone(soft, 0.08);
            Ok(Accent {
                fill,
                soft,
                on_fill: text.contrast_on(&[fill], 4.5)?,
                on_soft: text.contrast_on(&[soft], 4.5)?,
            })
        };
        Ok(Colors {
            canvas,
            panel,
            raised,
            text,
            muted,
            outline,
            primary: [
                make(seeds.primary[0])?,
                make(seeds.primary[1])?,
                make(seeds.primary[2])?,
            ],
            status: [
                make(seeds.status[0])?,
                make(seeds.status[1])?,
                make(seeds.status[2])?,
                make(seeds.status[3])?,
            ],
        })
    }
}
fn linear(v: f64) -> f64 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}
fn encoded(v: f64) -> u8 {
    let v = v.clamp(0., 1.);
    let v = if v <= 0.0031308 {
        12.92 * v
    } else {
        1.055 * v.powf(1. / 2.4) - 0.055
    };
    (v * 255.).round() as u8
}
fn to_lab(color: Rgb) -> [f64; 3] {
    let [r, g, b] = color.channels().map(|v| linear(f64::from(v) / 255.));
    let l = (0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b).cbrt();
    let m = (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b).cbrt();
    let s = (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b).cbrt();
    [
        0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
        1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
        0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s,
    ]
}
fn linear_from_lab([l, a, b]: [f64; 3]) -> [f64; 3] {
    let x = (l + 0.3963377774 * a + 0.2158037573 * b).powi(3);
    let y = (l - 0.1055613458 * a - 0.0638541728 * b).powi(3);
    let z = (l - 0.0894841775 * a - 1.2914855480 * b).powi(3);
    [
        4.0767416621 * x - 3.3077115913 * y + 0.2309699292 * z,
        -1.2684380046 * x + 2.6097574011 * y - 0.3413193965 * z,
        -0.0041960863 * x - 0.7034186147 * y + 1.7076147010 * z,
    ]
}
fn from_lab(lab: [f64; 3]) -> Rgb {
    if lab[0] <= 0. {
        return Rgb::BLACK;
    }
    if lab[0] >= 1. {
        return Rgb::WHITE;
    }
    let inside = |rgb: [f64; 3]| rgb.iter().all(|v| (0.0..=1.0).contains(v));
    let mut rgb = linear_from_lab(lab);
    if !inside(rgb) {
        // Chroma reduction at fixed lightness and hue, rather than RGB clipping.
        let (mut lo, mut hi) = (0., 1.);
        for _ in 0..20 {
            let mid = (lo + hi) * 0.5;
            let candidate = linear_from_lab([lab[0], lab[1] * mid, lab[2] * mid]);
            if inside(candidate) {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        rgb = linear_from_lab([lab[0], lab[1] * lo, lab[2] * lo]);
    }
    let [r, g, b] = rgb.map(encoded);
    Rgb(r, g, b)
}
