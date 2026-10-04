//! Native text APIs use UTF-16 code units; MUI text geometry uses UTF-8 bytes.
use std::ops::Range;

/// Clamp an offset to a scalar boundary at or before the requested byte.
pub fn byte_to_utf16(text: &str, byte: usize) -> usize {
    text.char_indices()
        .take_while(|(b, _)| *b < byte.min(text.len()))
        .filter(|(b, c)| b + c.len_utf8() <= byte)
        .map(|(_, c)| c.len_utf16())
        .sum()
}
/// Clamp an offset inside a surrogate pair to the scalar's leading boundary.
pub fn utf16_to_byte(text: &str, offset: usize) -> usize {
    let mut units = 0;
    for (byte, c) in text.char_indices() {
        if units + c.len_utf16() > offset {
            return byte;
        }
        units += c.len_utf16();
    }
    text.len()
}
/// Expand a non-empty native range to include any partially covered scalar.
pub fn utf16_range_to_bytes(text: &str, range: Range<usize>) -> Range<usize> {
    let start = utf16_to_byte(text, range.start);
    let mut end = utf16_to_byte(text, range.end.max(range.start));
    if range.end > range.start && byte_to_utf16(text, end) < range.end && end < text.len() {
        end += text[end..].chars().next().map_or(0, char::len_utf8);
    }
    start..end
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_ranges_never_slice_surrogates_or_utf8_scalars() {
        let s = "a😀e\u{301}אב";
        for (b, _) in s.char_indices().chain([(s.len(), '\0')]) {
            assert_eq!(utf16_to_byte(s, byte_to_utf16(s, b)), b);
        }
        assert_eq!(utf16_to_byte(s, 2), 1);
        assert_eq!(utf16_range_to_bytes(s, 2..3), 1..5);
        assert_eq!(utf16_range_to_bytes(s, 2..2), 1..1);
        assert_eq!(byte_to_utf16(s, 3), 1);
    }
}
