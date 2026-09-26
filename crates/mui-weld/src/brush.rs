use crate::field::Checked;
use crate::{Error, Point, Rect};
use std::sync::Arc;

/// Straight linear-light RGB. Compositing uses premultiplied linear RGB;
/// material/gradient interpolation uses premultiplied Oklab, not hue angles.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Color(pub [f64; 4]);
impl Color {
    pub const TRANSPARENT: Self = Self([0.0; 4]);
    pub fn srgb(r: f64, g: f64, b: f64, alpha: f64) -> Self {
        let linear = |x: f64| {
            if x <= 0.04045 {
                x / 12.92
            } else {
                ((x + 0.055) / 1.055).powf(2.4)
            }
        };
        Self([linear(r), linear(g), linear(b), alpha])
    }
    pub fn rgba8(self) -> [u8; 4] {
        let encoded = |v: f64| {
            let v = v.clamp(0.0, 1.0);
            if v <= 0.0031308 {
                12.92 * v
            } else {
                1.055 * v.powf(1.0 / 2.4) - 0.055
            }
        };
        if self.0[3] <= 0.0 {
            return [0; 4];
        }
        [
            (255.0 * encoded(self.0[0]) + 0.5) as u8,
            (255.0 * encoded(self.0[1]) + 0.5) as u8,
            (255.0 * encoded(self.0[2]) + 0.5) as u8,
            (255.0 * self.0[3].clamp(0.0, 1.0) + 0.5) as u8,
        ]
    }
    pub(crate) fn validate(self) -> Result<(), Error> {
        if self.0.iter().any(|x| !x.is_finite())
            || self.0[..3].iter().any(|x| !(-0.001..=1.001).contains(x))
            || !(0.0..=1.0).contains(&self.0[3])
        {
            return Err(Error::Invalid("colour"));
        }
        Ok(())
    }
    pub(crate) fn premul(self) -> Premul {
        let [r, g, b, a] = self.0;
        Premul([r * a, g * a, b * a, a])
    }
    fn lab(self) -> [f64; 3] {
        let [r, g, b, _] = self.0;
        let l = (0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b).cbrt();
        let m = (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b).cbrt();
        let s = (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b).cbrt();
        [
            0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
            1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
            0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s,
        ]
    }
    fn from_lab([l, a, b]: [f64; 3], alpha: f64) -> Self {
        let ll = (l + 0.3963377774 * a + 0.2158037573 * b).powi(3);
        let mm = (l - 0.1055613458 * a - 0.0638541728 * b).powi(3);
        let ss = (l - 0.0894841775 * a - 1.2914855480 * b).powi(3);
        Self([
            4.0767416621 * ll - 3.3077115913 * mm + 0.2309699292 * ss,
            -1.2684380046 * ll + 2.6097574011 * mm - 0.3413193965 * ss,
            -0.0041960863 * ll - 0.7034186147 * mm + 1.7076147010 * ss,
            alpha,
        ])
    }
    /// Weights must be non-negative and sum to one. Transparent colours cannot
    /// inject hue; missing paint is explicitly transparent, not black paint.
    pub(crate) fn weighted(values: impl IntoIterator<Item = (Self, f64)>) -> Self {
        let (mut lab, mut alpha) = ([0.0; 3], 0.0);
        for (c, w) in values {
            let wa = w * c.0[3];
            let v = c.lab();
            for j in 0..3 {
                lab[j] += v[j] * wa;
            }
            alpha += wa;
        }
        if alpha <= 1e-15 {
            return Self::TRANSPARENT;
        }
        Self::from_lab(lab.map(|x| x / alpha), alpha.clamp(0.0, 1.0))
    }
}

/// From any `color` space (a peniko colour is `AlphaColor<Srgb>`, mui-style
/// hands out `to_srgb()`): through sRGB, linearised in f64 as [`Color::srgb`].
impl<CS: color::ColorSpace> From<color::AlphaColor<CS>> for Color {
    fn from(c: color::AlphaColor<CS>) -> Self {
        let [r, g, b, a] = c.convert::<color::Srgb>().components.map(f64::from);
        Self::srgb(r, g, b, a)
    }
}
impl<CS: color::ColorSpace> From<Color> for color::AlphaColor<CS> {
    fn from(c: Color) -> Self {
        color::AlphaColor::<color::LinearSrgb>::new(c.0.map(|v| v as f32)).convert()
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Premul(pub(crate) [f64; 4]);
impl Premul {
    pub(crate) fn scale(self, x: f64) -> Self {
        Self(self.0.map(|v| v * x))
    }
    pub(crate) fn over(self, bottom: Self) -> Self {
        Self(std::array::from_fn(|j| {
            self.0[j] + bottom.0[j] * (1.0 - self.0[3])
        }))
    }
    pub(crate) fn mix(self, other: Self, t: f64) -> Self {
        Self(std::array::from_fn(|j| {
            self.0[j] * (1.0 - t) + other.0[j] * t
        }))
    }
    pub(crate) fn straight(self) -> Color {
        let a = self.0[3];
        if a <= 1e-15 {
            Color::TRANSPARENT
        } else {
            Color([self.0[0] / a, self.0[1] / a, self.0[2] / a, a])
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Stop {
    pub at: f64,
    pub color: Color,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageFit {
    Fill,
    Cover,
    Contain,
}
#[derive(Clone, Debug)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Arc<[u8]>,
}
impl PartialEq for Image {
    fn eq(&self, b: &Self) -> bool {
        self.width == b.width && self.height == b.height && Arc::ptr_eq(&self.rgba, &b.rgba)
    }
}
#[derive(Clone, Debug, PartialEq)]
pub enum Brush {
    Solid(Color),
    Linear {
        from: Point,
        to: Point,
        stops: Vec<Stop>,
    },
    Radial {
        center: Point,
        radius: f64,
        stops: Vec<Stop>,
    },
    Conic {
        center: Point,
        angle: f64,
        stops: Vec<Stop>,
    },
    Image {
        image: Image,
        bounds: Rect,
        fit: ImageFit,
    },
}
impl Brush {
    pub(crate) fn validate(&self) -> Result<(), Error> {
        let stops = match self {
            Self::Solid(c) => return c.validate(),
            Self::Linear { from, to, stops } => {
                from.validate()?;
                to.validate()?;
                if from == to {
                    return Err(Error::Invalid("zero-length gradient"));
                }
                stops
            }
            Self::Radial {
                center,
                radius,
                stops,
            } => {
                center.validate()?;
                if !radius.is_finite() || *radius <= 0.0 || *radius > 1e7 {
                    return Err(Error::Invalid("gradient radius"));
                }
                stops
            }
            Self::Conic {
                center,
                angle,
                stops,
            } => {
                center.validate()?;
                if !angle.is_finite() {
                    return Err(Error::Invalid("gradient angle"));
                }
                stops
            }
            Self::Image { image, bounds, .. } => {
                bounds.validate()?;
                let bytes = (image.width as usize)
                    .checked_mul(image.height as usize)
                    .and_then(|n| n.checked_mul(4));
                return if image.width == 0 || image.height == 0 || bytes != Some(image.rgba.len()) {
                    Err(Error::Invalid("image dimensions"))
                } else {
                    Ok(())
                };
            }
        };
        if stops.is_empty() || stops.len() > 256 {
            return Err(Error::Invalid("gradient stop count"));
        }
        let mut previous = f64::NEG_INFINITY;
        for s in stops {
            if !s.at.is_finite() || s.at < previous || !(0.0..=1.0).contains(&s.at) {
                return Err(Error::Invalid("gradient stops must be ordered in 0..=1"));
            }
            s.color.validate()?;
            previous = s.at;
        }
        Ok(())
    }
    /// Conservative retained heap payload, counting shared image buffers in full.
    pub(crate) fn retained_bytes(&self) -> usize {
        match self {
            Self::Solid(_) => 0,
            Self::Linear { stops, .. } | Self::Radial { stops, .. } | Self::Conic { stops, .. } => {
                stops.capacity().saturating_mul(std::mem::size_of::<Stop>())
            }
            Self::Image { image, .. } => image
                .rgba
                .len()
                .saturating_add(2 * std::mem::size_of::<usize>()),
        }
    }
    pub fn sample(&self, p: Point) -> Color {
        match self {
            Self::Solid(c) => *c,
            Self::Linear { from, to, stops } => {
                let v = *to - *from;
                ramp(stops, (p - *from).dot(v) / v.dot(v))
            }
            Self::Radial {
                center,
                radius,
                stops,
            } => ramp(stops, (p - *center).length() / radius),
            Self::Conic {
                center,
                angle,
                stops,
            } => {
                let d = p - *center;
                // CSS angles: zero points up, positive turns clockwise.
                let a = d.y.atan2(d.x) + std::f64::consts::FRAC_PI_2 - angle.to_radians();
                ramp(
                    stops,
                    a.rem_euclid(std::f64::consts::TAU) / std::f64::consts::TAU,
                )
            }
            Self::Image { image, bounds, fit } => image_sample(image, *bounds, *fit, p),
        }
    }
}
fn ramp(stops: &[Stop], t: f64) -> Color {
    let Some(first) = stops.first() else {
        return Color::TRANSPARENT;
    };
    if t < first.at {
        return first.color;
    }
    // Upper bound means the later colour owns an exact hard stop.
    let right = stops.partition_point(|s| s.at <= t);
    if right >= stops.len() {
        return stops[stops.len() - 1].color;
    }
    if right == 0 {
        return first.color;
    }
    let (a, b) = (&stops[right - 1], &stops[right]);
    let u = (t - a.at) / (b.at - a.at);
    Color::weighted([(a.color, 1.0 - u), (b.color, u)])
}
fn image_sample(image: &Image, b: Rect, fit: ImageFit, p: Point) -> Color {
    // Requests are validated before evaluation; this public sampler is also inert
    // for malformed user-created images rather than indexing outside the buffer.
    if image.width == 0 || image.height == 0 || b.validate().is_err() || p.validate().is_err() {
        return Color::TRANSPARENT;
    }
    let (iw, ih) = (f64::from(image.width), f64::from(image.height));
    let (mut w, mut h) = (b.width(), b.height());
    if fit != ImageFit::Fill {
        let k = if fit == ImageFit::Cover {
            (w / iw).max(h / ih)
        } else {
            (w / iw).min(h / ih)
        };
        w = iw * k;
        h = ih * k;
    }
    let (u, v) = (
        (p.x - (b.x0 + (b.width() - w) / 2.0)) / w,
        (p.y - (b.y0 + (b.height() - h) / 2.0)) / h,
    );
    if fit == ImageFit::Contain && (!(0.0..=1.0).contains(&u) || !(0.0..=1.0).contains(&v)) {
        return Color::TRANSPARENT;
    }
    let (x, y) = (
        (u * iw - 0.5).clamp(0.0, iw - 1.0),
        (v * ih - 0.5).clamp(0.0, ih - 1.0),
    );
    let (ix, iy) = (x.floor() as u32, y.floor() as u32);
    let read = |xx: u32, yy: u32| {
        let offset = (yy as usize)
            .checked_mul(image.width as usize)
            .and_then(|n| n.checked_add(xx as usize))
            .and_then(|n| n.checked_mul(4));
        let Some(bytes) =
            offset.and_then(|i| i.checked_add(4).and_then(|end| image.rgba.get(i..end)))
        else {
            return Premul::default();
        };
        Color::srgb(
            bytes[0] as f64 / 255.0,
            bytes[1] as f64 / 255.0,
            bytes[2] as f64 / 255.0,
            bytes[3] as f64 / 255.0,
        )
        .premul()
    };
    let nx = ix.saturating_add(1).min(image.width - 1);
    let ny = iy.saturating_add(1).min(image.height - 1);
    read(ix, iy)
        .mix(read(nx, iy), x - f64::from(ix))
        .mix(
            read(ix, ny).mix(read(nx, ny), x - f64::from(ix)),
            y - f64::from(iy),
        )
        .straight()
}
