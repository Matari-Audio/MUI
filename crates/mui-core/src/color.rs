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
    pub fn valid(self) -> bool {
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
    if in_gamut(v) {
        return Oklch::convert::<Srgb>(v).map(|c| c.clamp(0.0, 1.0));
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

/// The colours an interface declares by hand. Everything else -- a hover
/// state, a disabled control, a label that has to read on a coloured chip --
/// is derived from these by the methods below.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    /// The ground a panel sits at. Layers step off this.
    pub surface: Color,
    /// The one saturated colour: selection, focus, the live control.
    pub accent: Color,
    /// Ink on `surface`.
    pub ink: Color,
    /// Failure.
    pub error: Color,
    /// One layer's worth of perceptual lightness. Positive for a dark theme,
    /// where depth reads lighter; negative for a light theme, where it reads
    /// darker. Flipping its sign, and `hover` with it, flips the interface.
    pub step: f32,
    /// What a control gains under the pointer. Deliberately more than `step`:
    /// a layer is depth and may be subtle, a hover has to be noticed. The
    /// default is the mean of the two hover lifts the hand-tuned palette this
    /// replaced used -- `+0.128` on a grey, `+0.087` on the accent.
    pub hover: f32,
}

impl Palette {
    /// The preview gallery's palette, measured off the hand-picked sRGB
    /// constants it used to carry.
    pub const DARK: Self = Self {
        surface: Color::oklch(0.260, 0.015, 264.0),
        accent: Color::oklch(0.752, 0.131, 242.0),
        ink: Color::oklch(0.922, 0.015, 264.0),
        error: Color::oklch(0.633, 0.164, 23.0),
        step: 0.045,
        hover: 0.11,
    };
}

impl Palette {
    /// WCAG 2.1 AA for body text.
    pub const AA_TEXT: f32 = 4.5;
    /// WCAG 2.1 AA for large text and for the boundary of a UI component.
    pub const AA_LARGE: f32 = 3.0;
    /// WCAG 2.1 AAA for body text.
    pub const AAA_TEXT: f32 = 7.0;
}

impl Default for Palette {
    fn default() -> Self {
        Self::DARK
    }
}

impl Palette {
    /// A surface `level` layers off the ground. `0` is the ground itself;
    /// positive lifts toward the viewer (a panel over a backdrop, a control
    /// over a panel), negative sinks (a well, a slider track).
    pub fn layer(&self, level: i32) -> Color {
        self.surface.lighten(self.step * level as f32)
    }

    /// The pointer is over it.
    pub fn hover(&self, base: Color) -> Color {
        base.lighten(self.hover)
    }

    /// The pointer is down on it: half again past hover, so a press always
    /// reads as more than a hover.
    pub fn pressed(&self, base: Color) -> Color {
        base.lighten(self.hover * 1.5)
    }

    /// It cannot be used: half-way back to the ground, and mostly neutral.
    pub fn disabled(&self, base: Color) -> Color {
        base.mix(self.surface, 0.5)
            .scale_chroma(0.25)
            .with_alpha(base.alpha())
    }

    /// Ink that reads on `bg`: whichever of `ink` and `surface` sits further
    /// from it in perceptual lightness, pushed further if that alone does not
    /// clear [`Palette::AA_TEXT`]. This is what makes a label legible on the
    /// accent without a second ink colour being declared.
    pub fn on(&self, bg: Color) -> Color {
        let d = |c: Color| (c.lightness() - bg.lightness()).abs();
        let fg = if d(self.ink) >= d(self.surface) {
            self.ink
        } else {
            self.surface
        };
        fg.readable_on(bg, Self::AA_TEXT)
    }

    /// Ink for provenance, counts, the line under a heading: pulled back
    /// toward `bg` rather than made transparent, then pushed back out if the
    /// dimming cost it legibility. Dimmed text is still text, so it is held to
    /// the same [`Palette::AA_TEXT`] as [`Palette::on`] -- on a light-enough
    /// ground the contrast floor is what decides how dim it actually gets.
    pub fn dim(&self, bg: Color) -> Color {
        let fg = self.on(bg).mix(bg, 0.4).with_alpha(self.ink.alpha());
        fg.readable_on(bg, Self::AA_TEXT)
    }

    /// Whether every colour and the step are usable.
    pub fn valid(&self) -> bool {
        self.surface.valid()
            && self.accent.valid()
            && self.ink.valid()
            && self.error.valid()
            && self.step.is_finite()
            && self.hover.is_finite()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The palette this replaced was hand-tuned by eye, and its "lit accent"
    /// was a separate sRGB literal. Lightening the accent by the lift the
    /// designer actually used lands on that literal to within a rounding step
    /// of 8-bit sRGB -- which is the evidence that a lightness move in Oklch
    /// plus gamut mapping is what they were doing by hand.
    ///
    /// `Palette::DARK` uses a slightly larger lift than this, because it has
    /// to serve greys as well, so it is set here rather than taken from the
    /// default.
    /// A light theme is the dark one with two colours swapped and both steps
    /// negated. Nothing else is restated, which is the claim these tests check.
    fn light() -> Palette {
        Palette {
            surface: Color::oklch(0.97, 0.005, 264.0),
            ink: Color::oklch(0.20, 0.010, 264.0),
            step: -0.045,
            hover: -0.11,
            ..Palette::DARK
        }
    }

    #[test]
    fn hover_reproduces_the_hand_tuned_lit_accent() {
        let p = Palette {
            hover: 0.087,
            ..Palette::DARK
        };
        let got = p.hover(p.accent).to_srgb().components;
        let want = [0.56, 0.83, 1.00];
        for (g, w) in got.iter().zip(want) {
            assert!(
                (g - w).abs() < 0.015,
                "derived lit accent {got:?} is not the hand-tuned {want:?}"
            );
        }
    }

    /// The whole reason for the bisection. Clamping sRGB channels swings the
    /// accent toward cyan; holding hue costs chroma instead.
    #[test]
    fn gamut_mapping_holds_hue_where_clipping_does_not() {
        let accent = Palette::default().accent;
        let want = accent.lighten(0.15);
        let naive = Srgb::clip(Oklch::convert::<Srgb>([
            want.lightness(),
            want.chroma(),
            want.hue(),
        ]));
        let naive_hue = Srgb::convert::<Oklch>(naive)[2];
        let mapped_hue =
            Srgb::convert::<Oklch>(want.to_srgb().components[..3].try_into().unwrap())[2];
        assert!(
            (naive_hue - want.hue()).abs() > 20.0,
            "clipping was expected to shift hue badly, got {naive_hue}"
        );
        assert!(
            (mapped_hue - want.hue()).abs() < 8.0,
            "gamut mapping drifted to {mapped_hue} from {}",
            want.hue()
        );
    }

    #[test]
    fn an_in_gamut_colour_round_trips() {
        let c = Color::srgb(0.35, 0.72, 0.98);
        let back = c.to_srgb().components;
        for (g, w) in back.iter().zip([0.35, 0.72, 0.98, 1.0]) {
            assert!((g - w).abs() < 1e-3, "round trip gave {back:?}");
        }
    }

    #[test]
    fn alpha_survives_every_derivation() {
        let p = Palette::default();
        let c = p.accent.with_alpha(0.4);
        for got in [p.hover(c), p.pressed(c), p.disabled(c)] {
            assert!((got.alpha() - 0.4).abs() < 1e-6, "alpha lost: {got:?}");
        }
        assert!((c.to_srgb().components[3] - 0.4).abs() < 1e-6);
        // A mix is a gradient stop, not a state, so it does blend alpha.
        assert!((c.mix(p.ink, 0.5).alpha() - 0.7).abs() < 1e-6);
    }

    #[test]
    fn layers_climb_on_a_dark_theme_and_sink_on_a_light_one() {
        let dark = Palette::default();
        assert!(dark.layer(-1).lightness() < dark.layer(0).lightness());
        assert!(dark.layer(0).lightness() < dark.layer(3).lightness());

        let light = light();
        assert!(light.layer(-1).lightness() > light.layer(0).lightness());
        assert!(light.hover(light.layer(3)).lightness() < light.layer(3).lightness());
    }

    /// The regression this whole section exists for. `ink_dim` used to be a
    /// fixed 40% mix toward the base surface, which measured 2.99:1 on a
    /// raised control -- under even the 3.0 large-text floor, on a widget that
    /// paints notes.
    #[test]
    fn every_ink_role_clears_aa_on_every_layer() {
        for p in [Palette::DARK, light()] {
            for level in -2..=4 {
                let bg = p.layer(level);
                for (role, fg) in [("on", p.on(bg)), ("dim", p.dim(bg))] {
                    let r = fg.contrast(bg);
                    assert!(
                        r >= Palette::AA_TEXT - 0.01,
                        "{role} on layer({level}) is {r:.2}:1"
                    );
                }
            }
            // The accent is a background too -- a selected row, a pressed knob.
            for bg in [p.accent, p.hover(p.accent), p.error] {
                assert!(p.on(bg).contrast(bg) >= Palette::AA_TEXT - 0.01);
            }
        }
    }

    /// Dimming is still dimming. If the floor were doing all the work, `dim`
    /// would just return `on` and the distinction would be a lie.
    #[test]
    fn dim_is_visibly_dimmer_where_there_is_room_for_it() {
        let p = Palette::DARK;
        let bg = p.layer(0);
        assert!(p.dim(bg).lightness() < p.on(bg).lightness() - 0.05);
    }

    #[test]
    fn readable_moves_lightness_and_leaves_hue_alone() {
        let p = Palette::DARK;
        let bg = p.layer(0);
        // Deliberately illegible: the error red measures 4.16:1 on the ground.
        let fixed = p.error.readable_on(bg, Palette::AA_TEXT);
        assert!(p.error.contrast(bg) < Palette::AA_TEXT, "premise");
        assert!(fixed.contrast(bg) >= Palette::AA_TEXT);
        assert!(
            (fixed.hue() - p.error.hue()).abs() < 1.0,
            "{} vs {}",
            fixed.hue(),
            p.error.hue()
        );
        // And no further than it had to go.
        assert!(fixed.lightness() < p.error.lightness() + 0.1);
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

    /// One `on` rule has to serve both a dark ground and a light accent.
    #[test]
    fn ink_flips_to_stay_legible() {
        let p = Palette::default();
        assert_eq!(p.on(p.surface), p.ink, "dark ground wants the light ink");
        assert_eq!(
            p.on(p.accent),
            p.surface,
            "the accent is lighter than the ground, so ink has to invert"
        );
    }

    #[test]
    fn a_disabled_control_is_duller_and_closer_to_the_ground() {
        let p = Palette::default();
        let off = p.disabled(p.accent);
        assert!(off.chroma() < p.accent.chroma() * 0.3);
        let toward_ground = (off.lightness() - p.surface.lightness()).abs();
        assert!(toward_ground < (p.accent.lightness() - p.surface.lightness()).abs());
    }

    #[test]
    fn lightness_saturates_instead_of_escaping_the_range() {
        let white = Color::oklch(0.9, 0.02, 264.0).lighten(5.0);
        assert!((white.lightness() - 1.0).abs() < 1e-6);
        assert!(white.valid());
        let black = Color::oklch(0.1, 0.02, 264.0).darken(5.0);
        assert!(black.lightness().abs() < 1e-6);
        assert!(black.valid());
    }
}
