//! Editable text is shaped after layout, against the current field bounds.
use mui_geometry::Point;
use mui_layout::Size;
use mui_text::CaretMap;
use std::{ops::Range, sync::Arc};

/// Editing state attached to a text element. All ranges are UTF-8 bytes.
#[derive(Clone, Debug, PartialEq)]
pub struct EditableText {
    pub value: Arc<str>,
    pub selection: Range<usize>,
    pub caret: usize,
    /// Prefer the previous visual line at a shared soft-wrap boundary.
    pub caret_upstream: bool,
    pub marked: Range<usize>,
    /// Original source range replaced by `marked` in the displayed string.
    pub replacement: Option<Range<usize>>,
    pub multiline: bool,
    pub caret_visible: bool,
    pub scroll: f64,
    pub follow_caret: bool,
    pub insets: [f64; 2],
    /// Previous resolved viewport; resize follows the caret immediately.
    pub previous_viewport: Option<Size>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct TextLineGeometry {
    pub range: Range<usize>,
    pub origin: Point,
    pub height: f64,
    pub carets: Arc<CaretMap>,
}
/// Geometry actually displayed by this frame. Coordinates are field-local;
/// hit testing, selection, native candidate windows and accessibility share it.
#[derive(Clone, Debug, PartialEq)]
pub struct TextGeometry {
    pub text: Arc<str>,
    pub state: EditableText,
    pub lines: Vec<TextLineGeometry>,
    pub caret: Point,
    pub line_height: f64,
    pub scroll: f64,
    pub viewport: Size,
}
impl TextGeometry {
    pub fn source_to_display(&self, byte: usize) -> usize {
        match &self.state.replacement {
            Some(r) if byte >= r.end => byte - r.len() + self.state.marked.len(),
            Some(r) if byte > r.start => r.start,
            _ => byte,
        }
    }
    pub fn display_to_source(&self, byte: usize) -> usize {
        match &self.state.replacement {
            Some(r) if byte >= self.state.marked.end => byte - self.state.marked.len() + r.len(),
            Some(r) if byte >= self.state.marked.start => r.start,
            _ => byte,
        }
        .min(self.state.value.len())
    }
    pub fn row(&self, byte: usize) -> usize {
        if self.state.caret_upstream
            && byte == self.state.caret
            && let Some(row) = self.lines.iter().position(|l| l.range.end == byte)
        {
            return row;
        }
        self.lines
            .iter()
            .rposition(|l| l.range.start <= byte)
            .unwrap_or(0)
    }
    pub fn hit(&self, point: Point) -> usize {
        let line = self.lines.iter().min_by(|a, b| {
            let d = |l: &TextLineGeometry| (point.y - (l.origin.y + l.height / 2.)).abs();
            d(a).total_cmp(&d(b))
        });
        line.map_or(0, |l| {
            self.display_to_source(l.range.start + l.carets.hit(point.x - l.origin.x))
        })
    }
    pub fn visual_move(&self, source_byte: usize, right: bool) -> usize {
        let b = self.source_to_display(source_byte);
        let row = self.row(b);
        let l = &self.lines[row];
        let at = b.saturating_sub(l.range.start).min(l.range.len());
        let next = l.carets.move_visual(at, right);
        if next != at {
            return self.display_to_source(l.range.start + next);
        }
        let neighbor = if right {
            row.checked_add(1).filter(|r| *r < self.lines.len())
        } else {
            row.checked_sub(1)
        };
        neighbor.map_or(source_byte, |r| {
            let l = &self.lines[r];
            let stop = if right {
                l.carets.stops.first()
            } else {
                l.carets.stops.last()
            };
            self.display_to_source(l.range.start + stop.map_or(0, |p| p.0))
        })
    }
    pub fn vertical_move(
        &self,
        source_byte: usize,
        delta: isize,
        goal: Option<f64>,
    ) -> (usize, f64) {
        let b = self.source_to_display(source_byte);
        let row = self.row(b);
        let l = &self.lines[row];
        let x = goal.unwrap_or(
            l.carets
                .x(b.saturating_sub(l.range.start).min(l.range.len())),
        );
        let target =
            (row as isize + delta).clamp(0, self.lines.len().saturating_sub(1) as isize) as usize;
        let l = &self.lines[target];
        (self.display_to_source(l.range.start + l.carets.hit(x)), x)
    }
}
