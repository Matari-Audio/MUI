//! Renderer-independent Parley paragraphs, measured and finalized against MUI content bounds.
#![forbid(unsafe_code)]

use mui_core::{ResolvedScene, SceneError, Ui};
use mui_layout::{Available, Error, Frame, MeasureInput, Measurement, Size};
pub use parley;
use parley::{Alignment, AlignmentOptions, FontContext, Layout, LayoutContext, StyleProperty};
use std::collections::BTreeMap;

/// Logical pixel typography. DPI scaling belongs to the renderer, not this layout.
#[derive(Clone, Debug)]
pub struct TextStyle {
    /// Font family list, e.g. `"Inter, sans-serif"`.
    pub family: String,
    pub weight: f32,
    pub size: f32,
    pub line_height: f32,
    pub alignment: Alignment,
}
impl Default for TextStyle {
    fn default() -> Self {
        Self {
            family: "sans-serif".into(),
            weight: 400.,
            size: 14.,
            line_height: 20.,
            alignment: Alignment::Start,
        }
    }
}
impl TextStyle {
    fn validate(&self) -> Result<(), SceneError> {
        if self.family.trim().is_empty()
            || self.family.len() > 4096
            || !self.weight.is_finite()
            || !(1. ..=1000.).contains(&self.weight)
            || !self.size.is_finite()
            || self.size <= 0.
            || self.size > 4096.
            || !self.line_height.is_finite()
            || self.line_height <= 0.
            || self.line_height > 16384.
        {
            return Err(SceneError::InvalidModifier(
                "invalid text family, weight, size or line height",
            ));
        }
        Ok(())
    }
}

/// Owns reusable font and shaping contexts. Keep one per UI thread or editor.
#[derive(Default)]
pub struct TextSystem {
    fonts: FontContext,
    context: LayoutContext<()>,
}
impl TextSystem {
    /// Register bundled font bytes; return the families recognized by Parley.
    pub fn register_font(
        &mut self,
        bytes: Vec<u8>,
    ) -> Result<Vec<parley::fontique::FamilyId>, SceneError> {
        let families = self.fonts.collection.register_fonts(
            parley::fontique::Blob::new(std::sync::Arc::new(bytes)),
            None,
        );
        if families.is_empty() {
            return Err(SceneError::InvalidModifier(
                "font contains no supported faces",
            ));
        }
        Ok(families.into_iter().map(|(id, _)| id).collect())
    }
    /// Register bundled fonts here. System font discovery requires `system-fonts`.
    pub fn fonts_mut(&mut self) -> &mut FontContext {
        &mut self.fonts
    }

    pub fn resolve(&mut self, ui: &Ui, style: &TextStyle) -> Result<TextScene, SceneError> {
        self.resolve_with(ui, |_, _| style.clone())
    }

    /// Select typography once per text item. All state is published only on success.
    pub fn resolve_with(
        &mut self,
        ui: &Ui,
        mut style: impl FnMut(&str, &str) -> TextStyle,
    ) -> Result<TextScene, SceneError> {
        let mut paragraphs = BTreeMap::new();
        for (id, info) in ui.items() {
            let Some(text) = info.text.as_deref() else {
                continue;
            };
            if text.len() > 1_048_576 {
                return Err(SceneError::InvalidModifier("text exceeds 1 MiB budget"));
            }
            let style = style(id, text);
            style.validate()?;
            let mut builder = self.context.ranged_builder(&mut self.fonts, text, 1., true);
            builder.push_default(StyleProperty::FontFamily(parley::FontFamily::Source(
                style.family.as_str().into(),
            )));
            builder.push_default(StyleProperty::FontWeight(parley::FontWeight::new(
                style.weight,
            )));
            builder.push_default(StyleProperty::FontSize(style.size));
            builder.push_default(StyleProperty::LineHeight(parley::LineHeight::Absolute(
                style.line_height,
            )));
            let layout = builder.build(text);
            paragraphs.insert(
                id.to_owned(),
                Paragraph {
                    layout,
                    alignment: style.alignment,
                    frame: Frame {
                        x: 0.,
                        y: 0.,
                        size: Size::default(),
                    },
                },
            );
        }
        let scene = ui.resolve_with_baseline(|id, _, input| {
            paragraphs
                .get_mut(id)
                .ok_or_else(|| Error::MissingMeasurement(id.into()))?
                .measure(input)
        })?;
        // Measurement callbacks are speculative and may be skipped for fixed-size leaves.
        // Always reflow at the FINAL content width before handing glyphs to a renderer.
        for (id, paragraph) in &mut paragraphs {
            paragraph.frame = scene
                .layout
                .content_frame(id)
                .ok_or_else(|| SceneError::MissingLayoutFrame(id.clone()))?;
            paragraph
                .layout
                .break_all_lines(Some(paragraph.frame.size.width as f32));
            paragraph
                .layout
                .align(paragraph.alignment, AlignmentOptions::default());
        }
        Ok(TextScene { scene, paragraphs })
    }
}

/// Resolved geometry and the matching, final-width glyph layouts.
/// Keep these together; re-resolve after changing content, typography, width or theme spacing.
pub struct TextScene {
    pub scene: ResolvedScene,
    paragraphs: BTreeMap<String, Paragraph>,
}
impl TextScene {
    pub fn paragraph(&self, id: &str) -> Option<&Paragraph> {
        self.paragraphs.get(id)
    }
    pub fn paragraphs(&self) -> impl Iterator<Item = (&str, &Paragraph)> {
        self.paragraphs.iter().map(|(id, p)| (id.as_str(), p))
    }
}
pub struct Paragraph {
    layout: Layout<()>,
    alignment: Alignment,
    frame: Frame,
}
impl Paragraph {
    /// Glyph coordinates are relative to this content frame's origin.
    pub fn frame(&self) -> Frame {
        self.frame
    }
    pub fn layout(&self) -> &Layout<()> {
        &self.layout
    }
    fn measure(&mut self, input: MeasureInput) -> Result<Measurement, Error> {
        let widths = self.layout.calculate_content_widths();
        let width = input.known.width.unwrap_or_else(|| match input.width {
            Available::MinContent => f64::from(widths.min),
            Available::MaxContent => f64::from(widths.max),
            Available::Definite(limit) => {
                limit.min(f64::from(widths.max)).max(f64::from(widths.min))
            }
        });
        if !width.is_finite() || !(0. ..=1e7).contains(&width) {
            return Err(Error::InvalidValue);
        }
        self.layout.break_all_lines(Some(width as f32));
        Ok(Measurement {
            baseline: self
                .layout
                .lines()
                .next()
                .map(|line| f64::from(line.metrics().baseline)),
            size: Size::new(
                width,
                input
                    .known
                    .height
                    .unwrap_or(f64::from(self.layout.height())),
            ),
        })
    }
}
