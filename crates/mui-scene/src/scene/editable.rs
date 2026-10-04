use super::text::Face;
use super::{Layer, SceneError, Text, Walk, empty, snap};
use crate::{Color, EditableText, Element, Fill, TextGeometry, TextLineGeometry};
use mui_geometry::{Path, Point, Rect};
use mui_layout::{Frame, Size};
use std::sync::Arc;

fn box_path(rect: Rect) -> Path {
    Path::polyline(
        [
            (rect.x0, rect.y0),
            (rect.x1, rect.y0),
            (rect.x1, rect.y1),
            (rect.x0, rect.y1),
        ]
        .map(|(x, y)| Point::new(x, y)),
        true,
    )
}
impl Walk<'_> {
    pub(super) fn editable_text(
        &mut self,
        e: &Element,
        text: &str,
        edit: &EditableText,
        frame: Frame,
        key: &crate::Id,
        under: Color,
    ) -> Result<Arc<TextGeometry>, SceneError> {
        if !edit.scroll.is_finite() || edit.insets.iter().any(|v| !v.is_finite() || *v < 0.) {
            return Err(mui_geometry::Error::NonFinite.into());
        }
        let face = Face::of(e, &self.spec.theme);
        let [px, py] = edit.insets;
        let width = (frame.size.width - 2. * px).max(0.01);
        let height = (frame.size.height - 2. * py).max(0.);
        let metrics = self.runs.run("M", face)?.cloned();
        let lh = snap(
            metrics.as_ref().map_or(face.size * 1.25, |r| r.line_height),
            self.spec.device_scale,
        );
        let ascent = metrics.as_ref().map_or(face.size, |r| r.ascent);
        let ranges = if edit.multiline {
            self.runs.line_ranges(text, face, width)
        } else {
            vec![0..text.len()]
        };
        let row = if edit.caret_upstream {
            ranges.iter().position(|r| r.end == edit.caret)
        } else {
            None
        }
        .or_else(|| ranges.iter().rposition(|r| r.start <= edit.caret))
        .unwrap_or(0);
        let mut scroll = edit.scroll.max(0.);
        let viewport_size = Size::new(width, height);
        if edit.multiline && (edit.follow_caret || edit.previous_viewport != Some(viewport_size)) {
            let top = row as f64 * lh;
            scroll = scroll.min(top).max(top + lh - height);
        }
        scroll = scroll.clamp(0., (ranges.len() as f64 * lh - height).max(0.));
        let fonts = self.runs.fonts_for(face);
        let coords = self.runs.coords(&fonts, face);
        let hint = self.runs.settled(key, &coords);
        let mut lines = Vec::with_capacity(ranges.len());
        for (i, range) in ranges.into_iter().enumerate() {
            let map = self.runs.caret_map(&text[range.clone()], face)?;
            let shift = if edit.multiline {
                0.
            } else {
                (map.x(edit.caret.min(text.len())) - width + 2.).max(0.)
            };
            let y = if edit.multiline {
                i as f64 * lh - scroll
            } else {
                (height - lh) / 2.
            };
            lines.push(TextLineGeometry {
                range,
                origin: Point::new(px - shift, py + y),
                height: lh,
                carets: map,
            });
        }
        let caret_line = &lines[row];
        let caret = Point::new(
            caret_line.origin.x
                + caret_line.carets.x(edit
                    .caret
                    .saturating_sub(caret_line.range.start)
                    .min(caret_line.range.len())),
            caret_line.origin.y,
        );
        let geometry = Arc::new(TextGeometry {
            text: text.into(),
            state: edit.clone(),
            lines,
            caret,
            line_height: lh,
            scroll,
            viewport: viewport_size,
        });
        let viewport = Rect::new(
            frame.x + px,
            frame.y + py,
            frame.right() - px,
            frame.bottom() - py,
        );
        self.mark(Layer::Clip, Arc::new(box_path(viewport)), None);
        let mut index = 0;
        for line in &geometry.lines {
            if line.origin.y + lh < py || line.origin.y > py + height {
                continue;
            }
            let range = &line.range;
            let baseline = Point::new(
                snap(frame.x + line.origin.x, self.spec.device_scale),
                snap(frame.y + line.origin.y + ascent, self.spec.device_scale),
            );
            let run = self.runs.run(&text[range.clone()], face)?.cloned();
            let bands = line.carets.selection_spans(
                edit.selection.start.saturating_sub(range.start)
                    ..edit
                        .selection
                        .end
                        .saturating_sub(range.start)
                        .min(range.len()),
            );
            for band in &bands {
                let rect = Rect::new(
                    frame.x + line.origin.x + band.start,
                    frame.y + line.origin.y,
                    frame.x + line.origin.x + band.end,
                    frame.y + line.origin.y + lh,
                );
                self.push(
                    Layer::Draw(index),
                    box_path(rect),
                    None,
                    &Fill::Role(crate::Role::Primary),
                    under,
                );
                index += 1;
            }
            let native = run.map(|run| Text {
                fonts: fonts.clone(),
                size: face.size as f32,
                origin: baseline,
                glyphs: run.glyphs,
                axes: e.axes.clone(),
                font_coords: coords.clone(),
                hint,
            });
            if let Some(native) = native {
                if let Some(p) = self.push(
                    Layer::Text,
                    empty(),
                    None,
                    &Fill::Role(crate::Role::Ink),
                    under,
                ) {
                    p.text = Some(native.clone());
                }
                // Reuse the whole shaped run under each selected band: no
                // substring reshaping, lost kerning or broken ligatures.
                for band in &bands {
                    let rect = Rect::new(
                        frame.x + line.origin.x + band.start,
                        frame.y + line.origin.y,
                        frame.x + line.origin.x + band.end,
                        frame.y + line.origin.y + lh,
                    );
                    self.mark(Layer::Clip, Arc::new(box_path(rect)), None);
                    let primary = self.spec.theme.palette.primary();
                    if let Some(p) = self.push(
                        Layer::Text,
                        empty(),
                        None,
                        &Fill::Role(crate::Role::Ink),
                        primary,
                    ) {
                        p.text = Some(native.clone());
                    }
                    self.mark(Layer::Unclip, empty(), None);
                }
            }
            let start = edit.marked.start.max(range.start);
            let end = edit.marked.end.min(range.end);
            if start < end {
                for band in line
                    .carets
                    .selection_spans(start - range.start..end - range.start)
                {
                    let rect = Rect::new(
                        frame.x + line.origin.x + band.start,
                        frame.y + line.origin.y + lh - 2.,
                        frame.x + line.origin.x + band.end,
                        frame.y + line.origin.y + lh,
                    );
                    self.push(
                        Layer::Draw(index),
                        box_path(rect),
                        None,
                        &Fill::Role(crate::Role::Ink),
                        under,
                    );
                    index += 1;
                }
            }
        }
        if edit.caret_visible {
            let rect = Rect::new(
                frame.x + caret.x,
                frame.y + caret.y,
                frame.x + caret.x + 2.,
                frame.y + caret.y + lh,
            );
            self.push(
                Layer::Draw(index),
                box_path(rect),
                None,
                &Fill::Role(crate::Role::Ink),
                under,
            );
        }
        self.mark(Layer::Unclip, empty(), None);
        Ok(geometry)
    }
}
