//! Resolve opaque paint colors on theme/hover changes, separately from layout.
use crate::{ColorError, Colors, Rgb, SceneError, Ui};
use std::collections::BTreeMap;
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ItemStyle {
    pub fill: Option<Rgb>,
    /// Effective opaque background, including inherited parent color.
    pub background: Rgb,
    pub text: Rgb,
    pub stroke: Option<(Rgb, f64)>,
    pub hovered: bool,
}
#[derive(Debug)]
pub enum StyleError {
    Color(ColorError),
    Scene(SceneError),
}
impl std::fmt::Display for StyleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Color(e) => e.fmt(f),
            Self::Scene(e) => e.fmt(f),
        }
    }
}
impl std::error::Error for StyleError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(match self {
            Self::Color(e) => e,
            Self::Scene(e) => e,
        })
    }
}
impl From<ColorError> for StyleError {
    fn from(e: ColorError) -> Self {
        Self::Color(e)
    }
}
impl From<SceneError> for StyleError {
    fn from(e: SceneError) -> Self {
        Self::Scene(e)
    }
}
impl Ui {
    pub fn resolved_styles(
        &self,
        hovered: Option<&str>,
    ) -> Result<BTreeMap<&str, ItemStyle>, StyleError> {
        self.styles(self.colors(), hovered)
    }
    /// Use colors from this UI's theme. Cache until theme or hover target changes.
    /// Entries include item IDs and merged-outline IDs, ready for the renderer.
    /// Hover on any merged member changes the shared outline consistently.
    pub fn styles<'a>(
        &'a self,
        colors: &Colors,
        hovered: Option<&str>,
    ) -> Result<BTreeMap<&'a str, ItemStyle>, StyleError> {
        let hovered = hovered.filter(|id| self.items.get(*id).is_some_and(|i| i.hoverable));
        let mut result: BTreeMap<&str, ItemStyle> = BTreeMap::new();
        let mut membership = BTreeMap::new();
        for group in &self.groups {
            for id in &group.members {
                membership.insert(id.as_str(), group);
            }
        }
        for id in &self.order {
            let info = &self.items[id];
            let inherited = info
                .parent
                .as_ref()
                .and_then(|p| result.get(p.as_str()))
                .map_or(colors.canvas, |s| s.background);
            let group = membership.get(id.as_str());
            if let Some(style) = group.and_then(|g| result.get(g.id.as_str())).copied() {
                result.insert(id.as_str(), style);
                continue;
            }
            let (paint, backdrop, active) = if let Some(group) = group {
                let backdrop = if &group.owner == id {
                    inherited
                } else {
                    result
                        .get(group.owner.as_str())
                        .map_or(colors.canvas, |s| s.background)
                };
                let active = hovered.is_some_and(|h| group.members.iter().any(|m| m == h));
                (&self.items[&group.members[0]], backdrop, active)
            } else {
                (info, inherited, hovered == Some(id.as_str()))
            };
            let normal = paint.color.map(|c| c.resolve(colors)).transpose()?;
            let mut fill = normal;
            if active {
                let override_color = hovered
                    .and_then(|h| self.items[h].hover_color)
                    .or(paint.hover_color);
                fill = Some(match override_color {
                    Some(c) => c.resolve(colors)?,
                    None => normal
                        .unwrap_or(backdrop)
                        .hovered_with(self.theme().mode, self.theme().hover_shift),
                });
            }
            let background = fill.unwrap_or(backdrop);
            let text = colors
                .text
                .contrast_on(&[background], self.theme().contrast.text)?;
            let stroke = paint
                .stroke
                .map(|(c, w)| {
                    Ok::<_, StyleError>((
                        c.resolve(colors)?
                            .contrast_on(&[background, backdrop], self.theme().contrast.graphics)?,
                        w,
                    ))
                })
                .transpose()?;
            let style = ItemStyle {
                fill,
                background,
                text,
                stroke,
                hovered: active,
            };
            result.insert(id.as_str(), style);
            if let Some(group) = group {
                result.insert(group.id.as_str(), style);
            }
        }
        Ok(result)
    }
}
