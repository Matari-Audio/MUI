//! What a box looks like, said in terms of the theme.
//!
//! Nothing here is a pixel colour until [`Fill::paint`] is asked, with a
//! palette and the colour underneath. A tree written once against roles reads
//! correctly in light and dark, on a chip and on the ground.
use crate::{Color, Corner, Palette};
use mui_geometry::CornerStyle;
use mui_geometry::Spacing;
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
    /// This role at `a` alpha: a hairline that still tracks the theme.
    ///
    /// ```
    /// use mui_style::{Role::*, *};
    /// let hairline = Ink.alpha(0.12);
    /// let p = Palette::NEUTRAL;
    /// let under = p.surface();
    /// assert_eq!(hairline.paint(&p, under), Fill::from(p.on(under).with_alpha(0.12)).paint(&p, under));
    /// ```
    pub fn alpha(self, a: f32) -> Fill {
        Fill::Faded(self, a)
    }
}

/// Where a gradient's ramp runs across the box it fills.
///
/// ```
/// use mui_style::{Role::*, *};
/// let arc = Gradient::conic(-135., [(0., Primary), (1., Field)]);
/// assert_eq!(arc.kind, GradientKind::Conic { angle: -135. });
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GradientKind {
    /// CSS-style angle in degrees: 180 runs top to bottom.
    Linear { angle: f64 },
    /// `center` in unit box coordinates -- `(0.5, 0.5)` is the middle --
    /// and `radius` as a fraction of the box's longer side.
    Radial { center: (f64, f64), radius: f64 },
    /// A sweep clockwise from `angle` degrees about the box's centre: the
    /// knob arc and the ring meter, with no geometry at all.
    Conic { angle: f64 },
}

/// A ramp of [`Fill`] stops, positioned by [`GradientKind`].
///
/// Stops resolve and interpolate one at a time, so a hover tint or a spring
/// walks the ramp and keeps its shape. That holds **within one kind only**:
/// two gradients of different kinds have no correspondence between their
/// geometries, so a kind change cuts rather than tweens.
///
/// ```
/// use mui_style::{Role::*, *};
/// let ramp = Gradient::linear(90., [(0., Primary), (1., Surface)]);
/// assert_eq!(ramp.stops.len(), 2);
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct Gradient {
    pub kind: GradientKind,
    pub stops: Vec<(f32, Fill)>,
}
impl Gradient {
    /// A ramp along `angle`, CSS-style: 180 runs top to bottom.
    ///
    /// ```
    /// use mui_style::{Role::*, *};
    /// let sky = Gradient::linear(180., [(0., Raised), (1., Surface)]);
    /// assert_eq!(sky.stops.len(), 2);
    /// ```
    pub fn linear<F: Into<Fill>>(angle: f64, stops: impl IntoIterator<Item = (f32, F)>) -> Self {
        Self::new(GradientKind::Linear { angle }, stops)
    }
    /// A ramp out of `center` (unit box coordinates) to `radius` of the
    /// box's longer side: an LED, a glow, a specular highlight.
    ///
    /// ```
    /// use mui_style::{Role::*, *};
    /// let led = Gradient::radial((0.3, 0.3), 0.6, [(0., Raised), (1., Surface)]);
    /// assert_eq!(led.stops.len(), 2);
    /// ```
    pub fn radial<F: Into<Fill>>(
        center: (f64, f64),
        radius: f64,
        stops: impl IntoIterator<Item = (f32, F)>,
    ) -> Self {
        Self::new(GradientKind::Radial { center, radius }, stops)
    }
    /// A sweep from `angle`, clockwise about the centre: the knob arc.
    ///
    /// ```
    /// use mui_style::{Role::*, *};
    /// let arc = Gradient::conic(-135., [(0., Primary), (0.7, Primary), (0.7, Field)]);
    /// assert_eq!(arc.stops.len(), 3);
    /// ```
    pub fn conic<F: Into<Fill>>(angle: f64, stops: impl IntoIterator<Item = (f32, F)>) -> Self {
        Self::new(GradientKind::Conic { angle }, stops)
    }
    /// Two stops, top to bottom.
    ///
    /// ```
    /// use mui_style::{Role::*, *};
    /// let card = Gradient::vertical(Raised, Surface);
    /// assert_eq!(card.stops.len(), 2);
    /// ```
    pub fn vertical<F: Into<Fill>>(top: F, bottom: F) -> Self {
        Self::linear(180.0, [(0.0, top), (1.0, bottom)])
    }
    fn new<F: Into<Fill>>(kind: GradientKind, stops: impl IntoIterator<Item = (f32, F)>) -> Self {
        Self {
            kind,
            stops: stops.into_iter().map(|(t, f)| (t, f.into())).collect(),
        }
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
    /// A GPU texture the host registered with its renderer under this key,
    /// in place of `rgba`: pixels that never leave the GPU. See
    /// [`Image::texture`].
    pub texture: Option<u64>,
}
impl Image {
    /// Pixels that live in a GPU texture the host keeps up to date and hands
    /// the renderer under `key` (`GpuRenderer::set_texture`). A renderer with
    /// no such texture, or none at all (the CPU one), paints the fill's
    /// fallback.
    pub fn texture(key: u64, width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            rgba: Arc::from([]),
            texture: Some(key),
        }
    }
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
            texture: None,
        })
    }
}
/// Comparing pixels would make an equality check cost a frame; two images
/// are the same image when they are the same buffer.
impl PartialEq for Image {
    fn eq(&self, o: &Self) -> bool {
        self.width == o.width
            && self.height == o.height
            && self.texture == o.texture
            && Arc::ptr_eq(&self.rgba, &o.rgba)
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
    /// A role resolved, then taken to this alpha. See [`Role::alpha`].
    Faded(Role, f32),
    Color(Color),
    /// Boxed: a ramp is rare, and inline it would triple every fill.
    Gradient(Box<Gradient>),
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
        Self::Gradient(Box::new(g))
    }
}

/// A fill with every role looked up: what a renderer is handed.
#[derive(Clone, Debug, PartialEq)]
pub enum Paint {
    Solid(Color),
    /// A resolved ramp: the same geometry the [`Gradient`] asked for, with
    /// every stop looked up.
    Gradient {
        kind: GradientKind,
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
            Self::Gradient { stops, .. } => {
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
            Self::Faded(r, a) => Paint::Solid(r.color(p, under).with_alpha(*a)),
            Self::Color(c) => Paint::Solid(*c),
            Self::Image(image, fit) => Paint::Image {
                image: image.clone(),
                fit: *fit,
            },
            Self::Gradient(g) => Paint::Gradient {
                kind: g.kind,
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
            Some(Paint::Gradient { kind, stops }) => Gradient {
                kind,
                stops: stops
                    .into_iter()
                    .map(|(t, c)| (t, Fill::Color(f(c))))
                    .collect(),
            }
            .into(),
        }
    }
}

/// Convex corner radius of a box.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Radius {
    /// The theme's box radius: what a panel, a card or a dialog rounds by.
    #[default]
    Theme,
    Px(f64),
    /// Independent convex and concave radii; (0, 0) defers all rounding.
    Pair(f64, f64),
    /// One of the theme's radii, named by what it rounds. See [`Corner`].
    Token(Corner),
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
impl From<(f64, f64)> for Radius {
    fn from((convex, concave): (f64, f64)) -> Self {
        Self::Pair(convex, concave)
    }
}
/// `.radius(Corner::Field)`: the theme says how much.
impl From<Corner> for Radius {
    fn from(c: Corner) -> Self {
        Self::Token(c)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Stroke {
    pub fill: Fill,
    /// `None` takes the theme's stroke width.
    pub width: Option<f64>,
}

/// Whether a shadow falls outside the shape or inside it.
///
/// An inset shadow is painted clipped to the node's own outline, after its
/// fill and shells. Its `blur` is the feather it fades over, so a zero-blur
/// inset shadow paints nothing -- use a small one for a crisp edge.
///
/// ```
/// use mui_style::{Role::*, *};
/// assert_eq!(Shadow::inset(3.).kind, ShadowKind::Inset);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ShadowKind {
    #[default]
    Drop,
    Inset,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Shadow {
    pub blur: f64,
    pub dx: f64,
    pub dy: f64,
    /// Grow the shadow's rectangle before blurring -- shrink it, for an
    /// inset one. CSS `box-shadow`'s fourth length.
    pub spread: f64,
    pub kind: ShadowKind,
    pub fill: Fill,
}
impl Shadow {
    /// A soft drop below the box, a quarter-strength black.
    ///
    /// ```
    /// use mui_style::{Role::*, *};
    /// assert_eq!(Shadow::soft(12.).dy, 6.);
    /// ```
    pub fn soft(blur: f64) -> Self {
        Self {
            blur,
            dx: 0.0,
            dy: blur / 2.0,
            spread: 0.0,
            kind: ShadowKind::Drop,
            fill: Fill::Color(Color::oklcha(0.0, 0.0, 0.0, 0.25)),
        }
    }
    /// The same shadow cast inward from the box's own edge: a recess, a
    /// floor under a translucent panel.
    ///
    /// ```
    /// use mui_style::{Role::*, *};
    /// let recess = Shadow::inset(4.);
    /// assert_eq!((recess.kind, recess.dy), (ShadowKind::Inset, 2.));
    /// ```
    pub fn inset(blur: f64) -> Self {
        Self {
            kind: ShadowKind::Inset,
            ..Self::soft(blur)
        }
    }
}

/// How far off the surface a node reads, as the shadow list that says so.
///
/// Two shadows -- a tight contact and a wide ambient -- are the whole
/// difference between a default box and a designed one, and nobody should
/// hand-tune four numbers per node to get them.
///
/// ```
/// use mui_style::{Role::*, *};
/// assert_eq!(Elevation::Raised.shadows().len(), 2);
/// assert!(Elevation::Flat.shadows().is_empty());
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Elevation {
    /// On the surface: no shadow at all.
    #[default]
    Flat,
    /// A card or a control lifted off the panel.
    Raised,
    /// A menu, a dialog, a drag ghost: off the panel entirely.
    Floating,
}
impl Elevation {
    /// The contact and ambient pair this step is made of.
    ///
    /// ```
    /// use mui_style::{Role::*, *};
    /// let [contact, ambient] = &Elevation::Floating.shadows()[..] else { panic!() };
    /// assert!(ambient.blur > contact.blur);
    /// ```
    pub fn shadows(self) -> Vec<Shadow> {
        // Black at an alpha, not a role: a shadow is the absence of light on
        // whatever is under it, and tinting it with the palette reads as a
        // second, wrong-coloured panel.
        let cast = |blur: f64, dy: f64, a: f32| Shadow {
            blur,
            dy,
            fill: Fill::Color(Color::oklcha(0.0, 0.0, 0.0, a)),
            ..Shadow::soft(blur)
        };
        match self {
            Self::Flat => Vec::new(),
            Self::Raised => vec![cast(2.0, 1.0, 0.30), cast(8.0, 4.0, 0.18)],
            Self::Floating => vec![cast(4.0, 2.0, 0.34), cast(24.0, 12.0, 0.26)],
        }
    }
}

/// Everything a node says about its own paint. Layers, back to front: drop
/// shadows, fill, shells (each a constant-thickness inset of the last),
/// inset shadows, stroke, then the node's text.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Style {
    pub fill: Fill,
    pub stroke: Option<Stroke>,
    pub radius: Radius,
    /// The curve every corner turns through: circular, or a continuous
    /// superellipse. See `Paints::corners` in `mui-scene`.
    pub corners: CornerStyle,
    /// Back to front: every [`ShadowKind::Drop`] under the fill, every
    /// [`ShadowKind::Inset`] over the shells.
    pub shadow: Vec<Shadow>,
    pub shells: Vec<(Spacing, Fill)>,
    /// Outline is the union of the children's outlines, filleted, instead
    /// of this node's own rectangle: a tab joined to its panel. See
    /// `Paints::union` in `mui-scene`.
    pub union: bool,
    /// Pointer shape over the node; inherited by children that set none.
    pub cursor: Option<Cursor>,
    /// Blend mode and opacity for this node's whole subtree, as a
    /// compositing layer. `None` paints straight onto what is under it.
    pub layer: Option<(Mix, f32)>,
    /// Painted over everything this node and its children drew, and only
    /// where they drew: source-atop, in the node's outline. See
    /// `Paints::mask` in `mui-scene`.
    pub mask: Fill,
    /// Blur what was painted before this node, inside its outline, by this
    /// standard deviation in logical pixels; `0` leaves it sharp. See
    /// `Paints::backdrop_blur` in `mui-scene`.
    pub backdrop_blur: f64,
}

impl Style {
    /// `other` merged over `self`, per field: every field `other` states
    /// wins, every field it leaves at its default is kept from `self`. Last
    /// write wins, one field at a time -- there is no cascade and no
    /// specificity.
    ///
    /// A field's default *is* its "unset": `Fill::None` paints nothing,
    /// `Radius::Theme` takes the theme's, `None` and `[]` say nothing. The
    /// exception is `union`, which has no third state and so only ever turns
    /// on.
    ///
    /// ```
    /// use mui_style::{Role::*, *};
    /// let card = Style { radius: Radius::Px(12.), ..Style::default() };
    /// let mine = Style { fill: Primary.into(), ..Style::default() };
    /// let both = mine.clone().over(card.clone());
    /// assert_eq!(both.fill, mine.fill);   // card states no fill
    /// assert_eq!(both.radius, card.radius);
    /// ```
    pub fn over(self, other: Style) -> Style {
        Style {
            fill: if other.fill.is_none() {
                self.fill
            } else {
                other.fill
            },
            stroke: other.stroke.or(self.stroke),
            radius: if other.radius == Radius::Theme {
                self.radius
            } else {
                other.radius
            },
            corners: if other.corners == CornerStyle::Round {
                self.corners
            } else {
                other.corners
            },
            shadow: if other.shadow.is_empty() {
                self.shadow
            } else {
                other.shadow
            },
            shells: if other.shells.is_empty() {
                self.shells
            } else {
                other.shells
            },
            union: self.union || other.union,
            cursor: other.cursor.or(self.cursor),
            layer: other.layer.or(self.layer),
            mask: if other.mask.is_none() {
                self.mask
            } else {
                other.mask
            },
            backdrop_blur: if other.backdrop_blur > 0.0 {
                other.backdrop_blur
            } else {
                self.backdrop_blur
            },
        }
    }
}

/// How a blended layer's colour combines with what is under it.
///
/// Mirrors `peniko::Mix` variant for variant: `mui-scene` has no renderer
/// dependency, and `mui-vello` maps the two with an exhaustive match.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mix {
    #[default]
    Normal,
    Multiply,
    Screen,
    Overlay,
    Darken,
    Lighten,
    ColorDodge,
    ColorBurn,
    HardLight,
    SoftLight,
    Difference,
    Exclusion,
    Hue,
    Saturation,
    Color,
    Luminosity,
}
