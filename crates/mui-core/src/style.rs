//! What a box looks like, said in terms of the theme.
//!
//! Nothing here is a pixel colour until [`Fill::paint`] is asked, with a
//! palette and the colour underneath. A tree written once against roles reads
//! correctly in light and dark, on a chip and on the ground.
use crate::{Color, Palette};
use mui_layout::Spacing;
use std::sync::Arc;

/// The pointer's shape over a node.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Cursor {
    #[default]
    Arrow,
    Hand,
    Grab,
    Grabbing,
    Text,
    ResizeH,
    ResizeV,
    Crosshair,
    Forbidden,
}

/// A colour named by its job.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Role {
    Background,
    Surface,
    Raised,
    Field,
    /// An elevation step above `Surface`; negative sinks.
    Level(i32),
    Primary,
    Secondary,
    Tertiary,
    Success,
    Warning,
    Danger,
    /// Full-strength ink that reads on whatever it sits on.
    Ink,
    /// Quieter ink, still legible.
    Dim,
}
impl Role {
    pub fn color(self, p: &Palette, under: Color) -> Color {
        match self {
            Self::Background => p.background(),
            Self::Surface => p.surface(),
            Self::Raised => p.raised(),
            Self::Field => p.field(),
            Self::Level(n) => p.layer(n),
            Self::Primary => p.primary(),
            Self::Secondary => p.secondary(),
            Self::Tertiary => p.tertiary(),
            Self::Success => p.success(),
            Self::Warning => p.warning(),
            Self::Danger => p.danger(),
            Self::Ink => p.on(under),
            Self::Dim => p.dim(under),
        }
    }
}

/// A linear gradient, CSS-style: `angle` 180 runs top to bottom.
#[derive(Clone, Debug, PartialEq)]
pub struct Gradient {
    pub angle: f64,
    pub stops: Vec<(f32, Fill)>,
}
impl Gradient {
    pub fn linear<F: Into<Fill>>(angle: f64, stops: impl IntoIterator<Item = (f32, F)>) -> Self {
        Self {
            angle,
            stops: stops.into_iter().map(|(t, f)| (t, f.into())).collect(),
        }
    }
    pub fn vertical<F: Into<Fill>>(top: F, bottom: F) -> Self {
        Self::linear(180.0, [(0.0, top), (1.0, bottom)])
    }
}

/// Straight (not premultiplied) 8-bit RGBA, row-major, no padding: what
/// every decoder hands back. Decoding is the host's job -- this crate takes
/// no image dependency -- so hand `Image::rgba` the bytes a PNG, JPEG or
/// texture readback produced. The renderer premultiplies once and keeps the
/// result keyed on the `Arc`, so cloning an `Image` around is free.
#[derive(Clone, Debug)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Arc<[u8]>,
}
impl Image {
    /// `None` unless `rgba` is exactly `width * height * 4` bytes.
    pub fn rgba(width: u32, height: u32, rgba: impl Into<Arc<[u8]>>) -> Option<Self> {
        let rgba = rgba.into();
        let want = (width as usize)
            .checked_mul(height as usize)?
            .checked_mul(4)?;
        (want > 0 && rgba.len() == want).then_some(Self {
            width,
            height,
            rgba,
        })
    }
}
/// Comparing pixels would make an equality check cost a frame; two images
/// are the same image when they are the same buffer.
impl PartialEq for Image {
    fn eq(&self, o: &Self) -> bool {
        self.width == o.width && self.height == o.height && Arc::ptr_eq(&self.rgba, &o.rgba)
    }
}

/// How an image is mapped onto the box it fills.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Fit {
    /// Fill the box, crop the overflowing side. The photo default.
    #[default]
    Cover,
    /// Fit inside the box, letterboxed: the whole image is visible.
    Contain,
    /// Stretch to the box, aspect ratio be damned.
    Fill,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub enum Fill {
    #[default]
    None,
    Role(Role),
    Color(Color),
    Gradient(Gradient),
    Image(Arc<Image>, Fit),
}
impl From<Role> for Fill {
    fn from(r: Role) -> Self {
        Self::Role(r)
    }
}
impl From<Color> for Fill {
    fn from(c: Color) -> Self {
        Self::Color(c)
    }
}
/// An image fills its box by covering it, like a CSS background.
impl From<Arc<Image>> for Fill {
    fn from(i: Arc<Image>) -> Self {
        Self::Image(i, Fit::Cover)
    }
}
impl From<Gradient> for Fill {
    fn from(g: Gradient) -> Self {
        Self::Gradient(g)
    }
}

/// A fill with every role looked up: what a renderer is handed.
#[derive(Clone, Debug, PartialEq)]
pub enum Paint {
    Solid(Color),
    Linear {
        angle: f64,
        stops: Vec<(f32, Color)>,
    },
    /// The image fills the `Painted`'s outline; its extent is the outline's
    /// own bounds, so no extra geometry travels with the paint.
    Image {
        image: Arc<Image>,
        fit: Fit,
    },
}
impl Paint {
    /// The one colour a thing on top of this paint is judged against.
    pub fn solid(&self) -> Color {
        match self {
            Self::Solid(c) => *c,
            Self::Linear { stops, .. } => {
                stops.first().map_or(Color::oklch(0.5, 0.0, 0.0), |s| s.1)
            }
            // Ink over a photo is a designer's problem, not a palette's; mid
            // grey is the honest guess and keeps contrast checks running.
            Self::Image { .. } => Color::oklch(0.5, 0.0, 0.0),
        }
    }
}

impl Fill {
    pub fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }
    pub fn paint(&self, p: &Palette, under: Color) -> Option<Paint> {
        Some(match self {
            Self::None => return None,
            Self::Role(r) => Paint::Solid(r.color(p, under)),
            Self::Color(c) => Paint::Solid(*c),
            Self::Image(image, fit) => Paint::Image {
                image: image.clone(),
                fit: *fit,
            },
            Self::Gradient(g) => Paint::Linear {
                angle: g.angle,
                stops: g
                    .stops
                    .iter()
                    .map(|(t, f)| (*t, f.paint(p, under).map_or(under, |p| p.solid())))
                    .collect(),
            },
        })
    }
    /// Resolve, then push every colour through `f`: hover and press states
    /// without a second table of colours.
    pub fn map(&self, p: &Palette, under: Color, f: impl Fn(Color) -> Color) -> Fill {
        match self.paint(p, under) {
            None => Fill::None,
            Some(Paint::Solid(c)) => Fill::Color(f(c)),
            // Pixels are not a role: a hover tint has nothing to map here.
            Some(Paint::Image { image, fit }) => Fill::Image(image, fit),
            Some(Paint::Linear { angle, stops }) => Fill::Gradient(Gradient {
                angle,
                stops: stops
                    .into_iter()
                    .map(|(t, c)| (t, Fill::Color(f(c))))
                    .collect(),
            }),
        }
    }
}

/// Convex corner radius of a box.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Radius {
    #[default]
    Theme,
    Px(f64),
    /// Multiple of the theme radius.
    Scale(f64),
    /// Half the short side, whatever that turns out to be.
    Pill,
}
impl From<f64> for Radius {
    fn from(v: f64) -> Self {
        Self::Px(v)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Stroke {
    pub fill: Fill,
    /// `None` takes the theme's stroke width.
    pub width: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Shadow {
    pub blur: f64,
    pub dx: f64,
    pub dy: f64,
    pub fill: Fill,
}
impl Shadow {
    /// A soft drop below the box, a quarter-strength black.
    pub fn soft(blur: f64) -> Self {
        Self {
            blur,
            dx: 0.0,
            dy: blur / 2.0,
            fill: Fill::Color(Color::oklcha(0.0, 0.0, 0.0, 0.25)),
        }
    }
}

/// Everything a node says about its own paint. Layers, back to front:
/// shadow, fill, shells (each a constant-thickness inset of the last),
/// stroke, then the node's text.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Style {
    pub fill: Fill,
    pub stroke: Option<Stroke>,
    pub radius: Radius,
    pub shadow: Option<Shadow>,
    pub shells: Vec<(Spacing, Fill)>,
    /// Outline is the union of the children's frames, filleted, instead of
    /// this node's own rectangle: a tab welded to its panel.
    pub weld: bool,
    /// Pointer shape over the node; inherited by children that set none.
    pub cursor: Option<Cursor>,
}
