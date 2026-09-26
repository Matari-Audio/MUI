//! Glyph outlines as MUI geometry.
//!
//! A glyph here is not a texture — it is a [`mui_geometry::Path`] in the same
//! coordinate space as every other surface, so it flattens, tessellates and
//! (once contour winding is classified) composes with the Boolean surface
//! system like any other shape. [`text_run`] lays a whole string out the same
//! way, as one path; there is still no atlas anywhere.
//!
//! Variable-font axes are an argument rather than a font variant: the outline
//! is re-derived at whatever axis position is asked for, so Material Symbols
//! morph from unfilled to filled as `FILL` 0 -> 1. The geometry path here has
//! no cache at all; `mui-vello` rasterises the same outlines through a glyph
//! cache keyed on the normalized axis coordinates, one entry per position.
#![forbid(unsafe_code)]

mod caret;
mod error;
mod font;
mod lines;
mod outline;
mod shape;

pub use caret::{caret_positions, caret_x, hit_index};
pub use error::Error;
pub use font::{Axes, Axis, AxisInfo, Font, Weight, axes, normalized_coords};
pub use lines::{Line, break_lines, break_lines_from_advances, char_advances, min_content_width};
pub use outline::glyph_path;
pub use shape::{Glyph, TextRun, shape_run, text_run};

#[cfg(test)]
mod test_fonts {
    use crate::Font;

    pub(crate) fn hack() -> Font {
        Font::new(epaint_default_fonts::HACK_REGULAR).unwrap()
    }
    pub(crate) fn inter() -> Font {
        Font::new(ttf_inter::REGULAR).unwrap()
    }
    pub(crate) fn emoji() -> Font {
        Font::new(epaint_default_fonts::NOTO_EMOJI_REGULAR).unwrap()
    }
    pub(crate) fn symbols() -> Font {
        Font::new(MATERIAL_SYMBOLS).unwrap()
    }

    /// Three Material Symbols glyphs (home, favorite, settings), fvar/avar/
    /// gvar/HVAR and GSUB FeatureVariations kept. Regenerate with
    /// `pyftsubset MaterialSymbolsOutlined[FILL,GRAD,opsz,wght].ttf
    /// --unicodes=U+E88A,U+E87D,U+E8B8 --layout-features='*' --no-hinting`.
    pub(crate) const MATERIAL_SYMBOLS: &[u8] =
        include_bytes!("../fonts/MaterialSymbolsOutlined-subset.ttf");
}
