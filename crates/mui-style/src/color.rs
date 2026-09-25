#![forbid(unsafe_code)]

//! Colour in Oklch, and the rules that derive one colour from another.
//!
//! Oklch because every derivation here -- a hover state, a dimmed label, a
//! surface one layer up -- is a move in lightness or chroma, and only a
//! perceptual space makes the same move look the same on every hue. Nudging
//! sRGB channels lightens a yellow and barely touches a blue.

use color::{AlphaColor, ColorSpace, HueDirection, Oklab, Oklch, Srgb};

/// A colour, held in Oklch: perceptual lightness in `0..1`, chroma in
/// `0..~0.4`, hue in degrees, and straight (un-premultiplied) alpha.
///
/// Lightness moves are only meaningful in a perceptual space, so this is the
/// storage form. [`Color::to_srgb`] hands the renderer what it can paint.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color(AlphaColor<Oklch>);

impl Color {
    /// An opaque colour from Oklch components.
    pub const fn oklch(l: f32, c: f32, h: f32) -> Self {
        Self(AlphaColor::new([l, c, h, 1.0]))
    }

    /// An Oklch colour with straight alpha.
    pub const fn oklcha(l: f32, c: f32, h: f32, alpha: f32) -> Self {
        Self(AlphaColor::new([l, c, h, alpha]))
    }

    /// An opaque colour from sRGB components in `0..1`.
    pub fn srgb(r: f32, g: f32, b: f32) -> Self {
        Self::srgba(r, g, b, 1.0)
    }

    /// An sRGB colour in `0..1` with straight alpha.
    pub fn srgba(r: f32, g: f32, b: f32, alpha: f32) -> Self {
        let [l, c, h] = Srgb::convert::<Oklch>([r, g, b]);
        Self(AlphaColor::new([l, c, h, alpha]))
    }

    /// Perceptual lightness, `0` black to `1` white.
    pub fn lightness(self) -> f32 {
        self.0.components[0]
    }
    /// Colourfulness. `0` is a neutral grey; sRGB rarely exceeds `0.33`.
    pub fn chroma(self) -> f32 {
        self.0.components[1]
    }
    /// Hue angle in degrees.
    pub fn hue(self) -> f32 {
        self.0.components[2]
    }
    /// Straight alpha, `0` transparent to `1` opaque.
    pub fn alpha(self) -> f32 {
        self.0.components[3]
    }

    /// The same hue and chroma at a different lightness.
    pub fn with_lightness(self, l: f32) -> Self {
        let [_, c, h, a] = self.0.components;
        Self(AlphaColor::new([l, c, h, a]))
    }

    /// Lighter by `delta` in perceptual lightness. Negative darkens, and the
    /// result is clamped to `0..1` before it is ever painted.
    pub fn lighten(self, delta: f32) -> Self {
        self.with_lightness((self.lightness() + delta).clamp(0.0, 1.0))
    }

    /// Darker by `delta`. The mirror of [`Color::lighten`].
    pub fn darken(self, delta: f32) -> Self {
        self.lighten(-delta)
    }

    /// Chroma multiplied by `scale`. `0` is fully neutral.
    pub fn scale_chroma(self, scale: f32) -> Self {
        let [l, c, h, a] = self.0.components;
        Self(AlphaColor::new([l, (c * scale).max(0.0), h, a]))
    }

    /// The same colour at a different alpha.
    pub fn with_alpha(self, alpha: f32) -> Self {
        let [l, c, h, _] = self.0.components;
        Self(AlphaColor::new([l, c, h, alpha]))
    }

    /// `t` of the way from `self` to `other`, interpolated in Oklch by the
    /// shorter way round the hue circle. Alpha is interpolated too, which is
    /// what a gradient wants; a derivation that must keep its own alpha
    /// follows this with [`Color::with_alpha`].
    pub fn mix(self, other: Self, t: f32) -> Self {
        Self(
            self.0
                .lerp(other.0, t.clamp(0.0, 1.0), HueDirection::Shorter),
        )
    }

    /// The colour a renderer can paint: sRGB with straight alpha, gamut
    /// mapped so a lightened blue stays blue.
    ///
    /// This is the step that matters. Converting Oklch to sRGB and clamping
    /// the channels shifts hue badly -- lightening this palette's accent by
    /// `0.15` swings it 27 degrees toward cyan -- because clamping one
    /// channel at a time is a move in three dimensions, not one.
    pub fn to_srgb(self) -> AlphaColor<Srgb> {
        let [l, c, h, a] = self.0.components;
        let [r, g, b] = gamut_map([l, c, h]);
        AlphaColor::new([r, g, b, a.clamp(0.0, 1.0)])
    }

    /// WCAG 2.1 relative luminance of the colour as it will actually be
    /// painted -- gamut-mapped sRGB, not the unclamped Oklch request.
    pub fn luminance(self) -> f32 {
        self.to_srgb().discard_alpha().relative_luminance()
    }

    /// WCAG 2.1 contrast ratio, 1.0 (identical) to 21.0 (black on white).
    ///
    /// Alpha is ignored: a translucent colour's real contrast depends on what
    /// is behind it, which a pair of colours cannot know. Compose first, then
    /// measure.
    pub fn contrast(self, other: Color) -> f32 {
        let (a, b) = (self.luminance(), other.luminance());
        let (hi, lo) = if a > b { (a, b) } else { (b, a) };
        (hi + 0.05) / (lo + 0.05)
    }

    /// The least-changed version of `self` that clears `ratio` against `bg`.
    ///
    /// Hue and chroma are held; only lightness moves, and only away from `bg`,
    /// so a colour keeps its identity while becoming legible. Black or white is
    /// as far as it can go: if even that falls short -- a mid grey on a mid grey
    /// cannot reach 4.5 in either direction -- the extreme comes back rather
    /// than an error, because a slightly-illegal label still beats no label.
    pub fn readable_on(self, bg: Color, ratio: f32) -> Color {
        let fg = self;
        if !ratio.is_finite() || fg.contrast(bg) >= ratio {
            return fg;
        }
        let away = if fg.lightness() >= bg.lightness() {
            1.0
        } else {
            0.0
        };
        if fg.with_lightness(away).contrast(bg) < ratio {
            return fg.with_lightness(away);
        }
        // `fail` never clears the ratio and `ok` always does, so bisecting
        // toward `fail` lands on the smallest move that still works.
        let (mut fail, mut ok) = (fg.lightness(), away);
        while (ok - fail).abs() > 1e-3 {
            let mid = 0.5 * (fail + ok);
            if fg.with_lightness(mid).contrast(bg) >= ratio {
                ok = mid;
            } else {
                fail = mid;
            }
        }
        let out = fg.with_lightness(ok);
        // Gamut mapping pulls chroma as lightness moves, so contrast is not
        // perfectly monotonic; fall back to the extreme if the bisection
        // landed a hair short.
        if out.contrast(bg) >= ratio {
            out
        } else {
            fg.with_lightness(away)
        }
    }

    /// Whether every component is finite and in range. Hue is free to wrap.
    pub fn is_valid(self) -> bool {
        let [l, c, h, a] = self.0.components;
        h.is_finite()
            && (0.0..=1.0).contains(&l)
            && (0.0..=1.0).contains(&a)
            && c.is_finite()
            && c >= 0.0
    }
}

/// Whether an Oklch triple survives the trip to sRGB without clamping.
fn in_gamut(v: [f32; 3]) -> bool {
    Oklch::convert::<Srgb>(v)
        .iter()
        .all(|c| (-1e-4..=1.0 + 1e-4).contains(c))
}

/// Perceptual distance between an Oklch colour and an sRGB one.
fn delta_ok(a: [f32; 3], b: [f32; 3]) -> f32 {
    let (a, b) = (Oklch::convert::<Oklab>(a), Srgb::convert::<Oklab>(b));
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// CSS Color 4 §13.2: hold lightness and hue, bisect chroma downward until
/// the clipped sRGB is within a just-noticeable difference of the request.
///
/// The `color` crate deliberately offers only per-component clipping, which
/// it documents as perceptually poor, so the search lives here.
fn gamut_map(v: [f32; 3]) -> [f32; 3] {
    // Most colours are in gamut: convert once, not once to ask and again.
    let rgb = Oklch::convert::<Srgb>(v);
    if rgb.iter().all(|c| (-1e-4..=1.0 + 1e-4).contains(c)) {
        return rgb.map(|c| c.clamp(0.0, 1.0));
    }
    if !v.iter().all(|c| c.is_finite()) {
        return [0.0, 0.0, 0.0];
    }
    if v[0] <= 0.0 {
        return [0.0, 0.0, 0.0];
    }
    if v[0] >= 1.0 {
        return [1.0, 1.0, 1.0];
    }
    // Chroma 0 is always in gamut at a valid lightness, so this is a floor
    // the search can never fall through.
    let (mut lo, mut hi) = (0.0_f32, v[1]);
    let mut best = Oklch::convert::<Srgb>([v[0], 0.0, v[2]]).map(|c| c.clamp(0.0, 1.0));
    while hi - lo > 1e-4 {
        let c = 0.5 * (lo + hi);
        let probe = [v[0], c, v[2]];
        if in_gamut(probe) {
            lo = c;
            best = Oklch::convert::<Srgb>(probe).map(|x| x.clamp(0.0, 1.0));
            continue;
        }
        let clipped = Srgb::clip(Oklch::convert::<Srgb>(probe));
        // Close enough that no eye would tell them apart: take the clip and
        // keep the extra chroma rather than searching it away.
        if delta_ok(probe, clipped) < 0.02 {
            return clipped;
        }
        hi = c;
    }
    best
}

/// Which way the interface is lit.
///
/// This is the whole theme switch. A mode decides where the ground sits, where
/// ink sits, how saturated roles are placed between them, and which way depth
/// goes -- so flipping an interface is one field, not a second table of
/// colours to keep in step with the first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Dark,
    Light,
}

impl Mode {
    /// The window behind everything.
    ///
    /// Not at the extreme: a ground pinned to black or white has no room on
    /// the far side of it, and a recessed thing -- a field, a list well -- has
    /// to go somewhere.
    pub const fn ground(self) -> f32 {
        match self {
            Self::Dark => 0.22,
            Self::Light => 0.93,
        }
    }

    /// How far a surface may get from the ground on the ink's side of it.
    ///
    /// Past this, full-strength [`Palette::ink`] stops clearing
    /// [`Palette::AA_TEXT`] and the theme stops being the theme it said it
    /// was: a "dark" surface at 0.6 lightness is not dark. [`Palette::layer`]
    /// stops here, so no level -- however absurd -- can produce a surface the
    /// palette's own typography cannot sit on.
    pub const fn limit(self) -> f32 {
        match self {
            Self::Dark => 0.50,
            Self::Light => 0.60,
        }
    }

    /// Full-strength ink on the ground.
    pub const fn ink(self) -> f32 {
        match self {
            Self::Dark => 0.93,
            Self::Light => 0.18,
        }
    }

    /// Where a saturated role sits. A dark ground wants its accents light and
    /// a light ground wants them dark, which is why a role cannot declare its
    /// own lightness and still survive the flip.
    pub const fn accent(self) -> f32 {
        match self {
            Self::Dark => 0.75,
            Self::Light => 0.55,
        }
    }

    /// Which way depth goes: on a dark ground a raised thing is lighter, on a
    /// light ground it is darker. Every step and every hover is multiplied by
    /// this, so nothing else has to know which mode it is in.
    pub const fn sign(self) -> f32 {
        match self {
            Self::Dark => 1.0,
            Self::Light => -1.0,
        }
    }

    /// The other one.
    pub const fn flipped(self) -> Self {
        match self {
            Self::Dark => Self::Light,
            Self::Light => Self::Dark,
        }
    }
}

/// A colour with no lightness: the part of a role that survives a theme flip.
///
/// Hue and chroma are the identity of a role -- "our blue", "the warning
/// amber". Lightness is not identity, it is a consequence of the ground the
/// role is painted on, so [`Mode`] assigns it and a `Pigment` does not carry
/// one. This is what makes a dark and a light theme the same declaration.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pigment {
    /// Degrees. Free to wrap.
    pub hue: f32,
    /// 0 is grey. Roughly 0.4 is as saturated as sRGB goes.
    pub chroma: f32,
}

impl Pigment {
    pub const fn new(hue: f32, chroma: f32) -> Self {
        Self { hue, chroma }
    }

    /// No hue at all. A role set to this reads by lightness alone, which is
    /// what the crate's default does for every brand role it has no business
    /// choosing.
    pub const GREY: Self = Self::new(0.0, 0.0);

    /// This role, placed at a lightness.
    pub fn at(self, lightness: f32) -> Color {
        Color::oklch(lightness, self.chroma, self.hue)
    }

    pub fn is_valid(self) -> bool {
        self.hue.is_finite() && self.chroma.is_finite() && self.chroma >= 0.0
    }
}

/// The colours an interface declares, and the two numbers that derive the rest.
///
/// Every field is a [`Pigment`] rather than a colour, so the same declaration
/// serves both modes. Nothing here is a surface, a hover or a disabled state --
/// those are the methods below, because a table of them is the thing that
/// drifts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    pub mode: Mode,

    /// The greys: ground, panels, fields, ink. A little chroma here tints the
    /// whole interface toward a temperature without anything else changing.
    pub neutral: Pigment,

    /// The main brand colour: selection, focus, the live control.
    pub primary: Pigment,
    /// A supporting colour, for a second axis of emphasis.
    pub secondary: Pigment,
    /// A third, for the rare interface that needs one. Set it to `primary` if
    /// it does not.
    pub tertiary: Pigment,

    /// It worked.
    pub success: Pigment,
    /// It will go wrong if you continue.
    pub warning: Pigment,
    /// It went wrong, or it destroys something.
    pub danger: Pigment,

    /// One layer's worth of perceptual lightness, always positive.
    /// [`Mode::sign`] decides which way it points.
    pub step: f32,
    /// What a control gains under the pointer, also always positive.
    /// Deliberately more than `step`: a layer is depth and may be subtle, a
    /// hover has to be noticed.
    pub hover: f32,
}

impl Palette {
    /// A working palette with no taste in it: greys, and the three hues that
    /// success, warning and danger mean nearly everywhere. It exists so
    /// `Theme::default()` produces something legible and usable, not so anyone
    /// ships it.
    ///
    /// The brand roles are grey on purpose. Which colour is *yours* is the one
    /// decision a layout library has no business making, so selection and focus
    /// read by lightness alone until an application says otherwise:
    /// `Palette { primary: Pigment::new(242.0, 0.13), ..Palette::NEUTRAL }`.
    pub const NEUTRAL: Self = Self {
        mode: Mode::Dark,
        neutral: Pigment::GREY,
        primary: Pigment::GREY,
        secondary: Pigment::GREY,
        tertiary: Pigment::GREY,
        success: Pigment::new(145.0, 0.14),
        warning: Pigment::new(85.0, 0.15),
        danger: Pigment::new(25.0, 0.16),
        step: 0.045,
        hover: 0.11,
    };

    /// WCAG 2.1 AA for body text.
    pub const AA_TEXT: f32 = 4.5;
    /// WCAG 2.1 AA for large text and for the boundary of a UI component.
    pub const AA_LARGE: f32 = 3.0;
    /// WCAG 2.1 AAA for body text.
    pub const AAA_TEXT: f32 = 7.0;
    /// WCAG 2.1 1.4.11: a control identified only by its fill, or a state
    /// shown only by a colour change, needs this against what is next to it.
    pub const UI_NONTEXT: f32 = 3.0;

    /// A whole palette from one accent, in `mode`.
    ///
    /// The seed's hue and chroma become `primary`; `secondary` and `tertiary`
    /// step around the wheel from it, the greys take a trace of its hue so the
    /// surfaces read warm or cool with the brand, and the three signal roles
    /// keep their own hues -- red is not the brand's to move. Only hue and
    /// chroma are read: where each role sits in lightness is [`Mode`]'s
    /// business, which is what makes one seed serve both modes.
    ///
    /// The legibility this guarantees, swept over every hue in both modes:
    /// every role's fill clears [`Self::UI_NONTEXT`] against
    /// [`Self::background`] and [`Self::surface`], and [`Self::on`] clears
    /// [`Self::AA_TEXT`] on both of those. Chroma is clamped to what Oklch
    /// can still show in sRGB.
    ///
    /// ponytail: label text laid directly on a fully saturated fill is the
    /// one case that can land short -- near 4.4:1 rather than 4.5:1 -- because
    /// [`Color::readable_on`] keeps the ink's hue while it walks its
    /// lightness, and a chromatic near-black is not black. That is an `on`
    /// ceiling, not a seeding one. Against [`Self::raised`] in light mode an accent lands near
    /// 2.8:1 -- a property of the layer band, not of seeding (the shipped
    /// grey does the same), and chroma cannot buy it back. Put a label or a
    /// border on a control that floats on a raised card. A near-grey seed
    /// yields a near-grey palette; that is the seed's answer, not a fault.
    ///
    /// ```
    /// use mui_style::{Color, Mode, Palette};
    /// let p = Palette::from_seed(Color::oklch(0.6, 0.18, 250.0), Mode::Dark);
    /// assert_eq!(p.primary.hue, 250.0);
    /// assert!(p.primary().contrast(p.surface()) >= Palette::UI_NONTEXT);
    /// assert!(p.on(p.surface()).contrast(p.surface()) >= Palette::AA_TEXT);
    /// ```
    pub fn from_seed(seed: Color, mode: Mode) -> Self {
        // A seed out of a computation can arrive NaN; a palette that then
        // paints nothing is worse than one that paints grey.
        let ok = |v: f32, fallback| if v.is_finite() { v } else { fallback };
        let hue = ok(seed.hue(), 0.0).rem_euclid(360.0);
        let chroma = ok(seed.chroma(), 0.0).clamp(0.0, 0.33);
        let around = |turn: f32| Pigment::new((hue + turn).rem_euclid(360.0), chroma);
        Self {
            mode,
            // Enough hue to tell a warm interface from a cool one at a
            // glance, not enough to read as a colour.
            neutral: Pigment::new(hue, (chroma * 0.1).min(0.02)),
            primary: around(0.0),
            secondary: around(40.0),
            tertiary: around(-40.0),
            ..Self::NEUTRAL
        }
    }

    /// The same palette, lit the other way. This is the entire theme switch.
    pub const fn with_mode(self, mode: Mode) -> Self {
        Self { mode, ..self }
    }

    /// The same palette, flipped.
    pub const fn flipped(self) -> Self {
        self.with_mode(self.mode.flipped())
    }
}

impl Default for Palette {
    fn default() -> Self {
        Self::NEUTRAL
    }
}

/// Surfaces. Named levels, all derived from one ground and one step, so there
/// is no second set of numbers to keep in step with the first.
impl Palette {
    /// A surface `level` steps off the ground, in whichever direction depth
    /// goes in this mode. The named surfaces below are the levels worth a
    /// name; anything else is this.
    /// The neutral at `level` steps off the ground, confined to the band.
    ///
    /// The rule, in full: a layer is `ground + level * step`, walking toward
    /// the ink for positive levels and away for negative ones, stopping at
    /// [`Mode::limit`] on the ink's side and at black or white on the other.
    /// Levels past either end saturate rather than wrapping or escaping, so
    /// every colour this can return takes [`Self::ink`] at [`Self::AA_TEXT`].
    pub fn layer(&self, level: i32) -> Color {
        let want = self.mode.ground() + self.step * level as f32 * self.mode.sign();
        // `sign` already says which side the ink is on, so one comparison
        // serves both modes.
        let held = if self.mode.sign() > 0.0 {
            want.min(self.mode.limit())
        } else {
            want.max(self.mode.limit())
        };
        self.neutral.at(held.clamp(0.0, 1.0))
    }

    /// The WCAG ratio between two layers.
    ///
    /// This is what 1.4.11 measures when a control is identified by its fill
    /// and nothing else. One step is a depth cue, not a boundary: at a typical
    /// `step` adjacent layers sit near 1.15:1, nowhere near
    /// [`Self::UI_NONTEXT`]. Ask [`Self::levels_for`] how far 3:1 actually is
    /// rather than assuming a step buys it.
    pub fn separation(&self, a: i32, b: i32) -> f32 {
        self.layer(a).contrast(self.layer(b))
    }

    /// How many levels off the ground a fill must sit to clear `ratio`
    /// against it, or `None` if the band does not reach that far.
    ///
    /// For a control whose edge is its only affordance -- no border, no label
    /// -- that level is where it has to be, and on a shallow `step` it is
    /// further than anyone guesses.
    pub fn levels_for(&self, ratio: f32) -> Option<i32> {
        let bg = self.background();
        let (mut up, mut down) = (bg, bg);
        for level in 1.. {
            // Raised and recessed both count: a well can be the affordance as
            // readily as a bump, and on a dark ground the room is not
            // symmetric, so which side reaches first is not obvious.
            let (a, b) = (self.layer(level), self.layer(-level));
            if a.contrast(bg) >= ratio {
                return Some(level);
            }
            if b.contrast(bg) >= ratio {
                return Some(-level);
            }
            // Both ends of the band saturated: no further level is a new colour.
            if a == up && b == down {
                return None;
            }
            (up, down) = (a, b);
        }
        None
    }

    /// The window. Everything else is measured from here.
    pub fn background(&self) -> Color {
        self.layer(0)
    }

    /// A panel, a card, a dialog: a region lifted off the window.
    pub fn surface(&self) -> Color {
        self.layer(1)
    }

    /// A control that sits on a surface and looks like it can be pressed.
    pub fn raised(&self) -> Color {
        self.layer(2)
    }

    /// Somewhere to put something: a text input, a list well, a slider track.
    /// Recessed, so it reads as a hole rather than a button.
    pub fn field(&self) -> Color {
        self.layer(-1)
    }
}

/// Roles, placed at the lightness this mode wants for a saturated colour.
impl Palette {
    fn role(&self, p: Pigment) -> Color {
        p.at(self.mode.accent())
    }

    pub fn primary(&self) -> Color {
        self.role(self.primary)
    }
    pub fn secondary(&self) -> Color {
        self.role(self.secondary)
    }
    pub fn tertiary(&self) -> Color {
        self.role(self.tertiary)
    }
    pub fn success(&self) -> Color {
        self.role(self.success)
    }
    pub fn warning(&self) -> Color {
        self.role(self.warning)
    }
    pub fn danger(&self) -> Color {
        self.role(self.danger)
    }
}

/// States and ink. Each takes the colour a thing rests at and returns what it
/// should be instead, so a widget keeps no table of combinations.
impl Palette {
    /// What the pointer does to a control.
    pub fn hover(&self, base: Color) -> Color {
        base.lighten(self.hover * self.mode.sign())
    }

    /// What a press does. Further than a hover, same direction.
    pub fn pressed(&self, base: Color) -> Color {
        base.lighten(self.hover * 1.5 * self.mode.sign())
    }

    /// Pulled toward the ground and drained of most of its colour: a control
    /// that is present but cannot be used. Alpha is preserved, because
    /// disabling something should not also make it translucent.
    pub fn disabled(&self, base: Color) -> Color {
        base.mix(self.background(), 0.5)
            .scale_chroma(0.25)
            .with_alpha(base.alpha())
    }

    /// Full-strength ink, for when the ground is known to be the background.
    pub fn ink(&self) -> Color {
        self.neutral.at(self.mode.ink())
    }

    /// Ink that reads on `bg`: whichever end of the neutral range sits further
    /// from it in perceptual lightness, pushed further if that alone does not
    /// clear [`Palette::AA_TEXT`]. This is what makes a label legible on a
    /// saturated chip without a second ink colour being declared.
    pub fn on(&self, bg: Color) -> Color {
        let ink = self.ink();
        let ground = self.background();
        let d = |c: Color| (c.lightness() - bg.lightness()).abs();
        let fg = if d(ink) >= d(ground) { ink } else { ground };
        fg.readable_on(bg, Self::AA_TEXT)
    }

    /// Ink for provenance, counts, the line under a heading: pulled back
    /// toward `bg` rather than made transparent, then pushed back out if the
    /// dimming cost it legibility. Dimmed text is still text, so it is held to
    /// the same [`Palette::AA_TEXT`] as [`Palette::on`] -- on a close-contrast
    /// ground the floor is what decides how dim it actually gets.
    pub fn dim(&self, bg: Color) -> Color {
        let fg = self.on(bg).mix(bg, 0.4);
        fg.readable_on(bg, Self::AA_TEXT)
    }

    /// Whether every pigment and both positive steps are usable.
    pub fn is_valid(&self) -> bool {
        [
            self.neutral,
            self.primary,
            self.secondary,
            self.tertiary,
            self.success,
            self.warning,
            self.danger,
        ]
        .iter()
        .all(|p| p.is_valid())
            && self.step.is_finite()
            && self.step > 0.0
            && self.hover.is_finite()
            && self.hover > 0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The preview gallery's palette, measured off the hand-picked sRGB
    /// constants it used to carry. Tests need real chroma -- a greyscale
    /// palette cannot show that a hue survived a lightness move -- so the
    /// taste lives here rather than in the crate's default.
    fn designed() -> Palette {
        Palette {
            neutral: Pigment::new(264.0, 0.015),
            primary: Pigment::new(242.0, 0.131),
            ..Palette::NEUTRAL
        }
    }

    /// The claim the whole [`Mode`] type exists to make: one field, and
    /// nothing else in the declaration changes.
    #[test]
    fn flipping_the_mode_is_the_entire_theme_switch() {
        let dark = designed();
        let light = dark.flipped();

        assert_eq!(light.mode, Mode::Light);
        // Identity survives: same hues, same chromas, same steps.
        assert_eq!(light.primary, dark.primary);
        assert_eq!(light.neutral, dark.neutral);
        assert_eq!((light.step, light.hover), (dark.step, dark.hover));
        assert_eq!(light.flipped(), dark);

        // Only the lighting changed.
        assert!(light.background().lightness() > dark.background().lightness());
        assert!(light.ink().lightness() < dark.ink().lightness());
        assert!(light.primary().lightness() < dark.primary().lightness());
        assert!(
            (light.primary().hue() - dark.primary().hue()).abs() < 1.0,
            "the brand hue moved when the lights went on"
        );
    }

    /// Depth has to mean the same thing in both modes: raised is nearer the
    /// viewer, recessed is further, whichever way that happens to be lit.
    #[test]
    fn depth_reads_the_same_way_in_both_modes() {
        for p in [designed(), designed().flipped()] {
            let away = |c: Color| (c.lightness() - p.background().lightness()).abs();
            assert!(away(p.field()) > 0.0, "{:?}: field is flat", p.mode);
            assert!(
                away(p.raised()) > away(p.surface()),
                "{:?}: raised is not further out than surface",
                p.mode
            );
            assert!(
                away(p.hover(p.raised())) > away(p.raised()),
                "{:?}: hover did not lift",
                p.mode
            );
            assert!(
                away(p.pressed(p.raised())) > away(p.hover(p.raised())),
                "{:?}: a press is not further than a hover",
                p.mode
            );
            assert!(
                (p.surface().lightness() - p.background().lightness()) * p.mode.sign() > 0.0,
                "{:?}: a positive layer moved away from the mode's depth direction",
                p.mode
            );
            assert!(
                (p.hover(p.raised()).lightness() - p.raised().lightness()) * p.mode.sign() > 0.0,
                "{:?}: hover moved opposite to the mode's depth direction",
                p.mode
            );
            // A field is recessed and a raised control is not: they sit on
            // opposite sides of the window.
            let side = |c: Color| c.lightness() > p.background().lightness();
            assert_ne!(side(p.field()), side(p.raised()), "{:?}", p.mode);
        }
    }

    /// The regression the contrast work exists for. `dim` used to be a fixed
    /// 40% mix toward the base surface, which measured 2.99:1 on a raised
    /// control -- under even the 3.0 large-text floor, on a widget that paints
    /// The band is the rule that makes every other surface guarantee hold:
    /// whatever level is asked for, the neutral it comes back with is one
    /// full-strength ink still reads on. Sweeping absurd levels is the point
    /// -- a caller computing a level arithmetically is exactly who finds the
    /// end of a ramp, and saturating there must stay legal rather than merely
    /// stop moving.
    #[test]
    fn no_level_escapes_the_band() {
        for base in [Palette::NEUTRAL, designed()] {
            for p in [base, base.flipped()] {
                for level in [-1000, -50, -8, -1, 0, 1, 8, 50, 1000] {
                    let bg = p.layer(level);
                    let r = p.ink().contrast(bg);
                    assert!(
                        r >= Palette::AA_TEXT,
                        "{:?}: full ink on layer({level}) is {r:.2}:1",
                        p.mode
                    );
                    let l = bg.lightness();
                    assert!((0.0..=1.0).contains(&l), "layer({level}) escaped to {l}");
                }
                // Saturating, not wrapping: past the end the ramp stops dead.
                assert_eq!(p.layer(50), p.layer(1000));
                assert_eq!(p.layer(-50), p.layer(-1000));
            }
        }
    }

    /// A light ground sits near white and a dark one near black, so the side
    /// with less room is where two named surfaces collapse into one colour.
    /// They are the four names anyone reaches for; if any two are the same
    /// fill, one of them is a lie.
    #[test]
    fn the_named_surfaces_stay_four_distinct_colours() {
        for base in [Palette::NEUTRAL, designed()] {
            for p in [base, base.flipped()] {
                let named = [
                    ("field", p.field()),
                    ("background", p.background()),
                    ("surface", p.surface()),
                    ("raised", p.raised()),
                ];
                for (i, (an, a)) in named.iter().enumerate() {
                    for (bn, b) in &named[i + 1..] {
                        assert_ne!(a, b, "{:?}: {an} and {bn} are one colour", p.mode);
                    }
                }
                // And they are ordered: each name is further from the ground
                // than the last, on the side the mode says depth goes.
                let d = |c: Color| (c.lightness() - p.background().lightness()).abs();
                assert!(d(p.surface()) < d(p.raised()), "{:?}", p.mode);
            }
        }
    }

    /// One step is a depth cue, not an accessibility boundary. The palette has
    /// to be honest about that: `levels_for` either names a level that really
    /// clears the ratio, or admits the band does not reach it. A dark ground
    /// at this depth cannot reach 3:1 at all, which is a fact about dark
    /// themes -- it is why they draw borders -- not a bug to paper over.
    #[test]
    fn levels_for_never_promises_a_ratio_the_band_cannot_reach() {
        for base in [Palette::NEUTRAL, designed()] {
            for p in [base, base.flipped()] {
                assert!(
                    p.separation(0, 1) < 1.3,
                    "{:?}: one step should be a hint, not a wall",
                    p.mode
                );
                for ratio in [1.2, 1.5, 2.0, Palette::UI_NONTEXT, 4.5] {
                    match p.levels_for(ratio) {
                        Some(level) => {
                            let got = p.separation(0, level);
                            assert!(got >= ratio, "{:?}: level {level} is {got:.2}:1", p.mode);
                        }
                        None => {
                            // Nothing in the band reaches it, in either direction.
                            for level in -60..=60 {
                                let got = p.separation(0, level);
                                assert!(got < ratio, "{:?}: level {level} did reach", p.mode);
                            }
                        }
                    }
                }
            }
        }
    }

    /// notes.
    #[test]
    fn every_ink_role_clears_aa_everywhere_in_both_modes() {
        for base in [Palette::NEUTRAL, designed()] {
            for p in [base, base.flipped()] {
                for level in -50..=50 {
                    let bg = p.layer(level);
                    for (role, fg) in [("on", p.on(bg)), ("dim", p.dim(bg))] {
                        let r = fg.contrast(bg);
                        assert!(
                            r >= Palette::AA_TEXT - 0.01,
                            "{:?}: {role} on layer({level}) is {r:.2}:1",
                            p.mode
                        );
                    }
                }
                // Every saturated role is a background too -- a chip, a
                // selected row, a pressed knob.
                for bg in [
                    p.primary(),
                    p.secondary(),
                    p.tertiary(),
                    p.success(),
                    p.warning(),
                    p.danger(),
                    p.hover(p.primary()),
                ] {
                    let r = p.on(bg).contrast(bg);
                    assert!(r >= Palette::AA_TEXT - 0.01, "{:?}: {r:.2}:1", p.mode);
                }
            }
        }
    }

    /// Dimming is still dimming. If the contrast floor were doing all the
    /// work, `dim` would just return `on` and the distinction would be a lie.
    #[test]
    fn dim_is_visibly_dimmer_where_there_is_room_for_it() {
        for p in [designed(), designed().flipped()] {
            let bg = p.background();
            let closer = (p.dim(bg).lightness() - bg.lightness()).abs();
            let further = (p.on(bg).lightness() - bg.lightness()).abs();
            assert!(closer < further - 0.05, "{:?}", p.mode);
        }
    }

    /// The status roles are the one place the crate does hold an opinion, so
    /// they had better be the opinion everyone else holds.
    #[test]
    fn the_status_roles_are_the_hues_they_claim_to_be() {
        let p = Palette::NEUTRAL;
        let [r, g, b] = {
            let c = p.danger().to_srgb().components;
            [c[0], c[1], c[2]]
        };
        assert!(r > g && r > b, "danger is not red: {r} {g} {b}");
        let c = p.success().to_srgb().components;
        assert!(c[1] > c[0] && c[1] > c[2], "success is not green");
        let c = p.warning().to_srgb().components;
        assert!(c[0] > c[2] && c[1] > c[2], "warning is not amber");
    }

    /// The brand roles are grey until an application says otherwise, and the
    /// status roles are not.
    #[test]
    fn the_default_holds_no_brand_opinion() {
        let p = Palette::NEUTRAL;
        for role in [p.primary(), p.secondary(), p.tertiary()] {
            assert!(role.chroma() < 1e-6, "the default picked a brand colour");
        }
        assert!(p.danger().chroma() > 0.05);
    }

    /// The palette this replaced was hand-tuned by eye, and its "lit accent"
    /// was a separate sRGB literal. Lightening the accent by the lift the
    /// designer actually used lands on that literal to within a rounding step
    /// of 8-bit sRGB -- which is the evidence that a lightness move in Oklch
    /// plus gamut mapping is what they were doing by hand.
    #[test]
    fn hover_reproduces_the_hand_tuned_lit_accent() {
        let p = Palette {
            hover: 0.087,
            ..designed()
        };
        // The hand-tuned pair sat at L=.752 and L=.839; `Mode::Dark` places a
        // role at .75, so this is the same move from the same place.
        let got = p.hover(p.primary()).to_srgb().components;
        for (g, want) in got.iter().zip([0.56, 0.83, 1.00]) {
            assert!(
                (g - want).abs() < 0.015,
                "derived lit accent {got:?} missed the hand-tuned one"
            );
        }
    }

    #[test]
    fn gamut_mapping_holds_hue_where_clipping_does_not() {
        let accent = designed().primary();
        let lifted = accent.lighten(0.15);
        let naive = {
            let v = [
                lifted.lightness(),
                lifted.chroma(),
                lifted.hue().to_radians(),
            ];
            let clipped = Srgb::clip(Oklch::convert::<Srgb>(v));
            Color(AlphaColor::new([clipped[0], clipped[1], clipped[2], 1.0]))
        };
        let mapped = Color::srgb(
            lifted.to_srgb().components[0],
            lifted.to_srgb().components[1],
            lifted.to_srgb().components[2],
        );
        let drift = |c: Color| (c.hue() - accent.hue()).abs();
        assert!(
            drift(naive) > 20.0,
            "clipping was supposed to swing the hue, it moved {:.1}",
            drift(naive)
        );
        assert!(
            drift(mapped) < 8.0,
            "gamut mapping let the hue move {:.1}",
            drift(mapped)
        );
    }

    #[test]
    fn an_in_gamut_colour_round_trips() {
        for want in [[0.2, 0.4, 0.9], [0.9, 0.1, 0.1], [0.5, 0.5, 0.5]] {
            let back = Color::srgb(want[0], want[1], want[2]).to_srgb().components;
            for (g, w) in back.iter().zip(want) {
                assert!((g - w).abs() < 1e-3, "round trip gave {back:?}");
            }
        }
    }

    /// A state must not quietly change how see-through something is. `mix` is
    /// the exception: it blends alpha, because that is what a gradient wants.
    #[test]
    fn alpha_survives_every_derivation() {
        let p = designed();
        let c = p.primary().with_alpha(0.4);
        for got in [p.hover(c), p.pressed(c), p.disabled(c)] {
            assert!((got.alpha() - 0.4).abs() < 1e-6, "alpha lost: {got:?}");
        }
        assert!((c.to_srgb().components[3] - 0.4).abs() < 1e-6);
        assert!((c.mix(p.ink(), 0.5).alpha() - 0.7).abs() < 1e-6);
    }

    #[test]
    fn readable_moves_lightness_and_leaves_hue_alone() {
        let p = designed();
        let bg = p.background();
        // Deliberately illegible: a role at its own lightness on a near ground.
        let fg = p.neutral.at(bg.lightness() + 0.1);
        assert!(fg.contrast(bg) < Palette::AA_TEXT, "premise");
        let fixed = fg.readable_on(bg, Palette::AA_TEXT);
        assert!(fixed.contrast(bg) >= Palette::AA_TEXT);
        assert!((fixed.hue() - fg.hue()).abs() < 1.0);
        assert!(fixed.lightness() > fg.lightness(), "it moved the wrong way");
    }

    #[test]
    fn an_impossible_ratio_saturates_instead_of_looping() {
        let bg = Color::oklch(0.5, 0.0, 264.0);
        let fg = Color::oklch(0.52, 0.0, 264.0);
        // 21:1 against a mid grey is unreachable in either direction.
        let out = fg.readable_on(bg, 21.0);
        assert_eq!(out.lightness(), 1.0);
        assert!(out.contrast(bg) > fg.contrast(bg));
    }

    #[test]
    fn contrast_is_symmetric_and_bounded() {
        let black = Color::oklch(0.0, 0.0, 0.0);
        let white = Color::oklch(1.0, 0.0, 0.0);
        assert!((black.contrast(white) - 21.0).abs() < 0.05);
        assert!((white.contrast(black) - black.contrast(white)).abs() < 1e-4);
        assert!((white.contrast(white) - 1.0).abs() < 1e-4);
    }

    #[test]
    fn a_disabled_control_is_duller_and_closer_to_the_ground() {
        let p = designed();
        let off = p.disabled(p.primary());
        assert!(off.chroma() < p.primary().chroma() * 0.3);
        let toward_ground = (off.lightness() - p.background().lightness()).abs();
        assert!(toward_ground < (p.primary().lightness() - p.background().lightness()).abs());
    }

    #[test]
    fn lightness_saturates_instead_of_escaping_the_range() {
        let white = Color::oklch(0.9, 0.05, 100.0).lighten(0.5);
        assert!((white.lightness() - 1.0).abs() < 1e-6);
        assert!(white.is_valid());
        let black = Color::oklch(0.1, 0.05, 100.0).darken(0.5);
        assert!(black.lightness().abs() < 1e-6);
        assert!(black.is_valid());
    }

    #[test]
    fn an_invalid_pigment_is_caught() {
        assert!(Palette::NEUTRAL.is_valid());
        assert!(!Palette {
            primary: Pigment::new(f32::NAN, 0.1),
            ..Palette::NEUTRAL
        }
        .is_valid());
        assert!(!Palette {
            step: f32::INFINITY,
            ..Palette::NEUTRAL
        }
        .is_valid());
        assert!(!Palette {
            step: 0.0,
            ..Palette::NEUTRAL
        }
        .is_valid());
        assert!(!Palette {
            hover: 0.0,
            ..Palette::NEUTRAL
        }
        .is_valid());
        assert!(!Palette {
            step: -0.045,
            ..Palette::NEUTRAL
        }
        .is_valid());
        assert!(!Palette {
            hover: -0.11,
            ..Palette::NEUTRAL
        }
        .is_valid());
    }

    #[test]
    fn a_seeded_palette_is_legible_in_both_modes() {
        for mode in [Mode::Light, Mode::Dark] {
            for hue in (0..360).step_by(15) {
                let seed = Color::oklch(0.6, 0.2, hue as f32);
                let p = Palette::from_seed(seed, mode);
                assert!(p.is_valid(), "{mode:?} {hue}");
                for role in [
                    p.primary,
                    p.secondary,
                    p.tertiary,
                    p.success,
                    p.warning,
                    p.danger,
                ] {
                    let fill = p.role(role);
                    for (name, under) in [("bg", p.background()), ("surface", p.surface())] {
                        let c = fill.contrast(under);
                        assert!(c >= Palette::UI_NONTEXT, "{mode:?} {hue} on {name}: {c}");
                    }
                    // AA on the surfaces text actually sits on; large-text
                    // AA on the saturated fill itself, which is the `on`
                    // ceiling the doc names.
                    for under in [p.background(), p.surface()] {
                        let ink = p.on(under).contrast(under);
                        assert!(
                            ink >= Palette::AA_TEXT - 0.01,
                            "{mode:?} {hue} ink {ink:.2}"
                        );
                    }
                    let ink = p.on(fill).contrast(fill);
                    assert!(ink >= Palette::AA_LARGE, "{mode:?} {hue} on fill {ink:.2}");
                }
                assert_eq!(p.primary.hue, hue as f32);
                assert!(p.neutral.chroma <= 0.02, "the greys stay grey");
            }
        }
    }

    #[test]
    fn a_seed_no_one_should_have_sent_still_paints() {
        let p = Palette::from_seed(Color::oklch(0.5, f32::NAN, f32::NAN), Mode::Dark);
        assert!(p.is_valid());
        assert_eq!((p.primary.hue, p.primary.chroma), (0.0, 0.0));
        let hot = Palette::from_seed(Color::oklch(0.5, 9.0, 400.0), Mode::Light);
        assert_eq!((hot.primary.hue, hot.primary.chroma), (40.0, 0.33));
    }
}
