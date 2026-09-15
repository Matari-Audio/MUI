//! What a box looks like, said in terms of the theme.
//!
//! Nothing here is a pixel colour until [`Fill::paint`] is asked, with a
//! palette and the colour underneath. A tree written once against roles reads
//! correctly in light and dark, on a chip and on the ground.
use crate::color::{Color, Palette};
use mui_layout::Spacing;

/// A colour named by its job.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Role {
    Background,
    Surface,
    Raised,
    Field,
    Layer(i32),
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
            Self::Layer(n) => p.layer(n),
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

#[derive(Clone, Debug, Default, PartialEq)]
pub enum Fill {
    #[default]
    None,
    Role(Role),
    Color(Color),
    Gradient(Gradient),
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
}
impl Paint {
    /// The one colour a thing on top of this paint is judged against.
    pub fn solid(&self) -> Color {
        match self {
            Self::Solid(c) => *c,
            Self::Linear { stops, .. } => {
                stops.first().map_or(Color::oklch(0.5, 0.0, 0.0), |s| s.1)
            }
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
}
