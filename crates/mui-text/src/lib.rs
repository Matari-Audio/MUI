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
#[cfg(feature = "system-fonts")]
mod discovery;
mod error;
mod font;
mod lines;
mod offsets;
mod outline;
mod shape;

pub use caret::{CaretCluster, CaretMap, caret_positions, caret_x, hit_index};
#[cfg(feature = "system-fonts")]
pub use discovery::{Family, FontDatabase, FontQuery, FontSelection, FontStretch, FontStyle};
pub use error::Error;
pub use font::{Axes, Axis, AxisInfo, Font, Weight, axes, normalized_coords};
pub use lines::{Line, break_lines, break_lines_from_advances, char_advances, min_content_width};
pub use offsets::{byte_to_utf16, utf16_range_to_bytes, utf16_to_byte};
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

    /// Wrap two supplied test fonts as a TTC, relocating table-directory
    /// offsets to the shared file. Checksums aren't consumed by either parser.
    pub(crate) fn collection() -> Vec<u8> {
        let mut bytes = b"ttcf\0\x01\0\0\0\0\0\x02".to_vec();
        bytes.extend_from_slice(&[0; 8]);
        for (index, source) in [epaint_default_fonts::HACK_REGULAR, ttf_inter::REGULAR]
            .into_iter()
            .enumerate()
        {
            while bytes.len() % 4 != 0 {
                bytes.push(0);
            }
            let start = bytes.len();
            bytes[12 + index * 4..16 + index * 4].copy_from_slice(&(start as u32).to_be_bytes());
            bytes.extend_from_slice(source);
            let count = u16::from_be_bytes([source[4], source[5]]) as usize;
            for table in 0..count {
                let at = start + 12 + table * 16 + 8;
                let offset = u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap());
                bytes[at..at + 4].copy_from_slice(&(offset + start as u32).to_be_bytes());
            }
        }
        bytes
    }

    /// Three Material Symbols glyphs (home, favorite, settings), fvar/avar/
    /// gvar/HVAR and GSUB FeatureVariations kept. Regenerate with
    /// `pyftsubset MaterialSymbolsOutlined[FILL,GRAD,opsz,wght].ttf
    /// --unicodes=U+E88A,U+E87D,U+E8B8 --layout-features='*' --no-hinting`.
    pub(crate) const MATERIAL_SYMBOLS: &[u8] =
        include_bytes!("../fonts/MaterialSymbolsOutlined-subset.ttf");
}
