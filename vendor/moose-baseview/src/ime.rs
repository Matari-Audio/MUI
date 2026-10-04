//! Native text input shared contract. Platform implementations own their native resources.
use crate::dpi::{PhysicalPosition, PhysicalSize};
use std::ops::Range;

#[derive(Clone, Debug, PartialEq)]
pub enum Ime {
    Enabled,
    Preedit {
        text: String,
        cursor: Option<(usize, usize)>,
    },
    Commit(String),
    Disabled,
    /// Replace this UTF-8 byte range before the next preedit or commit.
    Selection(Range<usize>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ImeConfiguration {
    /// Stable focused input identity; changing it cancels the previous composition.
    pub id: String,
    pub position: PhysicalPosition<f64>,
    pub size: PhysicalSize<f64>,
    pub text: String,
    pub selection: Range<usize>,
    /// Marked range in `text`, in UTF-8 bytes.
    pub marked: Option<Range<usize>>,
}

impl ImeConfiguration {
    /// Reject invalid geometry and selections rather than passing them to native APIs.
    pub(crate) fn valid(&self) -> bool {
        self.position.x.is_finite()
            && self.position.y.is_finite()
            && self.size.width.is_finite()
            && self.size.height.is_finite()
            && self.size.width >= 0.0
            && self.size.height >= 0.0
            && self.selection.start <= self.selection.end
            && self.text.is_char_boundary(self.selection.start)
            && self.text.is_char_boundary(self.selection.end)
            && self.marked.as_ref().is_none_or(|r| {
                r.start <= r.end
                    && self.text.is_char_boundary(r.start)
                    && self.text.is_char_boundary(r.end)
            })
    }
}

#[cfg(any(target_os = "linux", test))]
pub(crate) fn char_to_byte(text: &str, index: usize) -> usize {
    text.char_indices().nth(index).map_or(text.len(), |(i, _)| i)
}

#[cfg(any(target_os = "windows", target_os = "macos", test))]
pub(crate) fn utf16_to_byte(text: &str, index: usize) -> usize {
    let mut units = 0;
    for (byte, ch) in text.char_indices() {
        if units + ch.len_utf16() > index {
            return byte;
        }
        units += ch.len_utf16();
    }
    text.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_offsets_respect_surrogates_and_utf8() {
        let s = "a😀é";
        assert_eq!((char_to_byte(s, 2), utf16_to_byte(s, 3)), (5, 5));
        assert_eq!(utf16_to_byte(s, 2), 1);
        let mut config = ImeConfiguration {
            id: "field".into(),
            position: PhysicalPosition::new(3., 4.),
            size: PhysicalSize::new(1., 15.),
            text: s.into(),
            selection: 1..5,
            marked: None,
        };
        assert!(config.valid());
        assert_eq!(Some(config.clone()), Some(config.clone()), "unchanged config compares equal");
        config.selection = 2..5;
        assert!(!config.valid());
        config.selection = 1..5;
        config.position.x = f64::NAN;
        assert!(!config.valid());
    }
}
