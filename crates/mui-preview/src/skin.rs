//! Everything this application decides about how it looks, and nothing else.
//!
//! One file, one `const`. `mui-core` ships the mechanism -- how a surface is
//! derived, where ink is allowed to sit, what a hover does -- and deliberately
//! ships no taste: `Palette::NEUTRAL` leaves every brand role grey, because
//! which colour is *yours* is not a layout library's decision. This is where
//! that decision is made, and the only place in this program where a colour is
//! chosen -- with one deliberate exception, `main.rs`'s debug frame overlay,
//! which is off-palette precisely so it cannot be mistaken for design.
//!
//! The test at the bottom is what keeps that true.

use mui_core::{CornerProfile, Mode, Palette, Pigment, Theme};

/// Two hues and a corner profile. Every other colour the gallery paints --
/// every surface, every ink, every hover, and the entire light theme -- is
/// derived from these.
///
/// A [`Pigment`] is a hue and a chroma with no lightness. Lightness is not a
/// role's identity, it is a consequence of the ground the role is painted on,
/// so [`Mode`] assigns it. That omission is why one declaration serves both
/// themes: see [`skin`].
pub const SKIN: Theme = Theme {
    palette: Palette {
        // A faintly cool grey. Every surface, field and ink is this hue.
        neutral: Pigment::new(264.0, 0.015),
        primary: Pigment::new(242.0, 0.131),
        secondary: Pigment::new(310.0, 0.120),
        tertiary: Pigment::new(190.0, 0.110),
        // One layer's depth, and what the pointer adds. Both always positive:
        // `Mode::sign` decides which way they point.
        step: 0.045,
        hover: 0.11,
        ..Palette::NEUTRAL
    },
    corners: CornerProfile::new(28.0, 32.0),
    ..Theme::DEFAULT
};

/// The gallery's palette, lit whichever way the sidebar checkbox says.
///
/// This is the whole theme switch. There is no light-theme table to keep in
/// step with [`SKIN`], because there is nothing in `SKIN` that a mode could
/// contradict.
pub fn skin(light: bool) -> Palette {
    SKIN.palette
        .with_mode(if light { Mode::Light } else { Mode::Dark })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A theme file that does not describe a usable theme is worse than none:
    /// the failure shows up as an unreadable label three crates away.
    #[test]
    fn the_skin_is_a_valid_theme_in_both_modes() {
        assert!(SKIN.valid());
        for light in [false, true] {
            let p = skin(light);
            assert!(p.valid());
            // The brand roles were actually declared. `Palette::NEUTRAL`
            // leaves them grey, and a grey primary is the shape of a theme
            // file that was half filled in.
            for role in [p.primary(), p.secondary(), p.tertiary()] {
                assert!(role.chroma() > 0.05, "a brand role is still grey");
            }
        }
    }
}
