use super::*;
use taffy::prelude::TaffyMaxContent;
use taffy::{
    prelude as t,
    style_helpers::{auto, fr, length, line, percent, span},
};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Constraints {
    pub width: Option<f64>,
    pub height: Option<f64>,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Available {
    Definite(f64),
    MinContent,
    MaxContent,
}
#[derive(Clone, Copy, Debug)]
pub struct MeasureInput {
    pub known: Constraints,
    pub width: Available,
    pub height: Available,
}
/// Content size and first baseline, measured from the top of the content box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Measurement {
    pub size: Size,
    pub baseline: Option<f64>,
}
impl From<Size> for Measurement {
    fn from(size: Size) -> Self {
        Self {
            size,
            baseline: None,
        }
    }
}
struct Entry<'a> {
    node: &'a Node,
    key: String,
    id: t::NodeId,
    children: Vec<usize>,
    padding: Insets,
    gap: f64,
}
struct Builder<'a> {
    tree: t::TaffyTree<usize>,
    entries: Vec<Entry<'a>>,
    keys: BTreeMap<String, ()>,
    limits: Limits,
    spacing: &'a SpacingScale,
}
impl From<taffy::TaffyError> for Error {
    fn from(e: taffy::TaffyError) -> Self {
        Self::Backend(e.to_string())
    }
}
fn alignment(a: Align) -> t::AlignItems {
    match a {
        Align::Start => t::AlignItems::START,
        Align::Center => t::AlignItems::CENTER,
        Align::End => t::AlignItems::END,
        Align::Stretch => t::AlignItems::STRETCH,
        Align::Baseline => t::AlignItems::BASELINE,
    }
}
fn justification(j: Justify) -> t::JustifyContent {
    match j {
        Justify::Start => t::JustifyContent::START,
        Justify::Center => t::JustifyContent::CENTER,
        Justify::End => t::JustifyContent::END,
        Justify::SpaceBetween => t::JustifyContent::SPACE_BETWEEN,
        Justify::SpaceEvenly => t::JustifyContent::SPACE_EVENLY,
        Justify::SpaceAround => t::JustifyContent::SPACE_AROUND,
    }
}
fn dimension(v: Option<Sizing>) -> t::Dimension {
    match v {
        Some(Sizing::Fill) => percent(1.),
        Some(Sizing::Fixed(v)) => length(v as f32),
        _ => auto(),
    }
}
fn horizontal(v: Horizontal) -> Align {
    match v {
        Horizontal::Left => Align::Start,
        Horizontal::Center => Align::Center,
        Horizontal::Right => Align::End,
        Horizontal::Stretch => Align::Stretch,
    }
}
fn vertical(v: Vertical) -> Align {
    match v {
        Vertical::Top => Align::Start,
        Vertical::Middle => Align::Center,
        Vertical::Bottom => Align::End,
        Vertical::Stretch => Align::Stretch,
    }
}
fn aligned_distribution(v: Align) -> t::JustifyContent {
    match v {
        Align::Start | Align::Baseline => t::JustifyContent::START,
        Align::Center => t::JustifyContent::CENTER,
        Align::End => t::JustifyContent::END,
        Align::Stretch => t::JustifyContent::STRETCH,
    }
}
fn apply_position(style: &mut t::Style, node: &Node, column: bool) {
    if let Some((x, y)) = node.physical {
        style.justify_content = Some(aligned_distribution(if column {
            vertical(y)
        } else {
            horizontal(x)
        }));
        style.align_items = Some(alignment(if column { horizontal(x) } else { vertical(y) }));
    }
    if let Some(pack) = node.pack_override {
        style.justify_content = Some(justification(pack));
    }
}
fn track(v: Track) -> taffy::style::GridTemplateComponent<String> {
    match v {
        Track::Hug => auto(),
        Track::Fixed(v) => length(v as f32),
        Track::Fraction(v) => fr(v as f32),
    }
}
impl<'a> Builder<'a> {
    fn add(
        &mut self,
        node: &'a Node,
        scope: &str,
        path: &str,
        depth: usize,
    ) -> Result<usize, Error> {
        if depth > self.limits.depth || self.entries.len() >= self.limits.nodes {
            return Err(Error::BudgetExceeded);
        }
        // Count before recursion, including containers, without introducing placeholder IDs.
        if self.keys.len() >= self.limits.nodes {
            return Err(Error::BudgetExceeded);
        }
        let scope = if let Some(local) = &node.scope {
            if local.is_empty() || local.contains('/') || local.starts_with('@') {
                return Err(Error::InvalidValue);
            }
            format!("{scope}{local}/")
        } else {
            scope.to_owned()
        };
        if node.key.contains('/') || node.key.starts_with('@') {
            return Err(Error::InvalidValue);
        }
        let key = format!(
            "{scope}{}",
            if node.key.is_empty() {
                format!("@{path}")
            } else {
                node.key.clone()
            }
        );
        if self.keys.insert(key.clone(), ()).is_some() {
            return Err(Error::DuplicateKey(key));
        }
        let extent = self.limits.extent;
        let shrink = node
            .shrink
            .unwrap_or(if matches!(node.kind, Kind::Measured) {
                1.
            } else {
                0.
            });
        let values = [node.grow, shrink];
        if !node.minimum.valid(extent)
            || node.maximum.is_some_and(|s| !s.valid(extent))
            || values
                .iter()
                .any(|v| !v.is_finite() || *v < 0. || *v > extent)
            || [node.width, node.height]
                .into_iter()
                .flatten()
                .any(|s| matches!(s,Sizing::Fixed(v) if !v.is_finite()||v<0.||v>extent))
        {
            return Err(Error::InvalidValue);
        }
        if let Some(max) = node.maximum {
            if max.width < node.minimum.width || max.height < node.minimum.height {
                return Err(Error::InvalidValue);
            }
        }
        if node.grid.is_some_and(|n| n == 0 || n > 256)
            || (node.grid.is_some() && node.axis != Axis::Row)
            || (node.grid.is_none() && (node.columns.is_some() || node.rows.is_some()))
            || node
                .cell
                .is_some_and(|(c, r)| c == 0 || r == 0 || c > 256 || r > 256)
            || [node.span.0, node.span.1]
                .into_iter()
                .any(|v| v == 0 || v > 256)
        {
            return Err(Error::InvalidValue);
        }
        for tracks in [&node.columns, &node.rows].into_iter().flatten() {
            if tracks.is_empty()
                || tracks.len() > 256
                || tracks.iter().any(|v| match v {
                    Track::Hug => false,
                    Track::Fixed(v) => !v.is_finite() || *v < 0. || *v > extent,
                    Track::Fraction(v) => !v.is_finite() || *v <= 0. || *v > extent,
                })
            {
                return Err(Error::InvalidValue);
            }
        }
        if matches!(node.kind, Kind::Overlay(_)) && node.pack_override.is_some() {
            return Err(Error::InvalidValue);
        }
        if depth == 0 && (node.cell.is_some() || node.span != (1, 1) || node.place.is_some()) {
            return Err(Error::InvalidValue);
        }
        if node.wrap
            && (!matches!(node.kind, Kind::Stack(_))
                || node.axis == Axis::Auto
                || node.grid.is_some()
                || matches!(node.kind, Kind::Overlay(_)))
        {
            return Err(Error::InvalidValue);
        }
        let mut padding = [0.; 4];
        for (out, value) in padding.iter_mut().zip(node.padding) {
            *out = value.resolve(self.spacing).ok_or(Error::InvalidValue)?;
        }
        let padding = Insets {
            left: padding[0],
            right: padding[1],
            top: padding[2],
            bottom: padding[3],
        };
        let gap = node.gap.resolve(self.spacing).ok_or(Error::InvalidValue)?;
        if !padding.valid(extent) || gap > extent {
            return Err(Error::InvalidValue);
        }
        let mut min = node.minimum;
        if let Kind::Leaf(size) = node.kind {
            if !size.valid(extent) {
                return Err(Error::InvalidValue);
            }
            if shrink == 0. {
                min.width = min.width.max(size.width + padding.horizontal());
                min.height = min.height.max(size.height + padding.vertical());
            }
        }
        if node
            .maximum
            .is_some_and(|max| min.width > max.width || min.height > max.height)
        {
            return Err(Error::InsufficientSpace(key));
        }
        let mut style = t::Style {
            size: t::Size {
                width: dimension(node.width),
                height: dimension(node.height),
            },
            min_size: t::Size {
                width: length(min.width as f32),
                height: length(min.height as f32),
            },
            max_size: node.maximum.map_or(
                t::Size {
                    width: auto(),
                    height: auto(),
                },
                |s| t::Size {
                    width: length(s.width as f32),
                    height: length(s.height as f32),
                },
            ),
            padding: t::Rect {
                left: length(padding.left as f32),
                right: length(padding.right as f32),
                top: length(padding.top as f32),
                bottom: length(padding.bottom as f32),
            },
            gap: t::Size {
                width: length(gap as f32),
                height: length(gap as f32),
            },
            flex_direction: if node.axis == Axis::Column {
                t::FlexDirection::Column
            } else {
                t::FlexDirection::Row
            },
            overflow: taffy::geometry::Point {
                x: if node.overflow == Overflow::Fit {
                    taffy::style::Overflow::Visible
                } else {
                    taffy::style::Overflow::Hidden
                },
                y: if node.overflow == Overflow::Fit {
                    taffy::style::Overflow::Visible
                } else {
                    taffy::style::Overflow::Hidden
                },
            },
            flex_grow: node.grow as f32,
            flex_shrink: shrink as f32,
            flex_wrap: if node.wrap {
                t::FlexWrap::Wrap
            } else {
                t::FlexWrap::NoWrap
            },
            align_items: Some(alignment(node.align)),
            justify_content: Some(justification(node.justify)),
            ..Default::default()
        };
        apply_position(&mut style, node, node.axis == Axis::Column);
        style.align_self = node.align_self.map(alignment);
        if let Some((x, y)) = node.place {
            style.justify_self = Some(alignment(horizontal(x)));
            style.align_self = Some(alignment(vertical(y)));
        }
        if let Some((c, r)) = node.cell {
            style.grid_column = t::Line {
                start: line(c as i16),
                end: span(node.span.0 as u16),
            };
            style.grid_row = t::Line {
                start: line(r as i16),
                end: span(node.span.1 as u16),
            };
        } else {
            style.grid_column = t::Line {
                start: auto(),
                end: span(node.span.0 as u16),
            };
            style.grid_row = t::Line {
                start: auto(),
                end: span(node.span.1 as u16),
            };
        }
        if let Some(n) = node.grid {
            style.display = t::Display::Grid;
            style.grid_template_columns = node.columns.as_ref().map_or_else(
                || vec![fr(1.); n],
                |v| v.iter().copied().map(track).collect(),
            );
            if let Some(rows) = &node.rows {
                style.grid_template_rows = rows.iter().copied().map(track).collect();
            }
            style.justify_items = Some(alignment(
                node.physical.map_or(node.align, |(x, _)| horizontal(x)),
            ));
            style.align_items = Some(alignment(
                node.physical.map_or(node.align, |(_, y)| vertical(y)),
            ));
            style.align_content = Some(node.physical.map_or(t::AlignContent::START, |(_, y)| {
                aligned_distribution(vertical(y))
            }));
        }
        let overlay = matches!(node.kind, Kind::Overlay(_));
        if overlay {
            style.display = t::Display::Grid;
            style.justify_items = Some(alignment(node.align));
            style.align_items = Some(if matches!(node.align, Align::Stretch | Align::Baseline) {
                alignment(node.align)
            } else {
                alignment(match node.justify {
                    Justify::Center => Align::Center,
                    Justify::End => Align::End,
                    _ => Align::Start,
                })
            });
            if let Some((x, y)) = node.physical {
                style.justify_items = Some(alignment(horizontal(x)));
                style.align_items = Some(alignment(vertical(y)));
            }
            style.justify_content = Some(t::JustifyContent::STRETCH);
            style.align_content = Some(t::AlignContent::STRETCH);
        }
        if node.overflow == Overflow::Scroll {
            // Oversized centered/end-aligned content must remain reachable from offset zero.
            for align in [&mut style.align_items, &mut style.justify_items]
                .into_iter()
                .flatten()
            {
                align.safety = taffy::style::AlignmentSafety::Safe;
            }
            for align in [&mut style.justify_content, &mut style.align_content]
                .into_iter()
                .flatten()
            {
                align.safety = taffy::style::AlignmentSafety::Safe;
            }
        }
        let mut children = Vec::new();
        for (i, child) in node.children().iter().enumerate() {
            if node.grid.is_none()
                && (child.cell.is_some() || child.span != (1, 1) || child.place.is_some())
            {
                return Err(Error::InvalidValue);
            }
            children.push(self.add(child, &scope, &format!("{path}.{i}"), depth + 1)?);
        }
        let ids: Vec<_> = children.iter().map(|i| self.entries[*i].id).collect();
        if node.overflow == Overflow::Scroll {
            for id in &ids {
                let mut child_style = self.tree.style(*id)?.clone();
                for align in [&mut child_style.align_self, &mut child_style.justify_self]
                    .into_iter()
                    .flatten()
                {
                    align.safety = taffy::style::AlignmentSafety::Safe;
                }
                self.tree.set_style(*id, child_style)?;
            }
        }
        if overlay {
            for id in &ids {
                let mut child_style = self.tree.style(*id)?.clone();
                child_style.grid_row = t::Line {
                    start: line(1),
                    end: line(2),
                };
                child_style.grid_column = t::Line {
                    start: line(1),
                    end: line(2),
                };
                self.tree.set_style(*id, child_style)?;
            }
        }
        let index = self.entries.len();
        let id = if node.children().is_empty() {
            self.tree.new_leaf_with_context(style, index)?
        } else {
            self.tree.new_with_children(style, &ids)?
        };
        self.entries.push(Entry {
            node,
            key,
            id,
            children,
            padding,
            gap,
        });
        Ok(index)
    }
    fn compute(
        &mut self,
        root: usize,
        available: t::Size<t::AvailableSpace>,
        measure: &mut impl FnMut(&str, MeasureInput) -> Result<Measurement, Error>,
    ) -> Result<(), Error> {
        let mut failure = None;
        let entries = &self.entries;
        let extent = self.limits.extent;
        let needs_baseline = entries
            .iter()
            .any(|e| e.node.align == Align::Baseline || e.node.align_self == Some(Align::Baseline));
        self.tree.compute_layout_with_measure(
            entries[root].id,
            available,
            |inputs, _, context, style| {
                let mut output = taffy::compute_leaf_layout(
                    inputs,
                    style,
                    |_, _| 0.,
                    |known, available| {
                        let Some(index) = context.as_deref() else {
                            return t::Size::ZERO;
                        };
                        let entry = &entries[*index];
                        let result = match entry.node.kind {
                            Kind::Leaf(s) => Ok(s.into()),
                            Kind::Measured => measure(
                                &entry.key,
                                MeasureInput {
                                    known: Constraints {
                                        width: known.width.map(f64::from),
                                        height: known.height.map(f64::from),
                                    },
                                    width: convert_available(available.width),
                                    height: convert_available(available.height),
                                },
                            ),
                            _ => Ok(Size::default().into()),
                        };
                        match result {
                            Ok(s) if valid_measurement(s, extent) => t::Size {
                                width: s.size.width as f32,
                                height: s.size.height as f32,
                            },
                            Ok(_) => {
                                failure = Some(Error::InvalidValue);
                                t::Size::ZERO
                            }
                            Err(e) => {
                                failure = Some(e);
                                t::Size::ZERO
                            }
                        }
                    },
                );
                // Fixed-size leaves may skip intrinsic measurement. Obtain font metrics
                // at the actual content size before Taffy aligns this leaf with siblings.
                if let Some(index) = context.as_deref() {
                    let entry = &entries[*index];
                    if needs_baseline && matches!(entry.node.kind, Kind::Measured) {
                        let width =
                            (f64::from(output.size.width) - entry.padding.horizontal()).max(0.);
                        let height =
                            (f64::from(output.size.height) - entry.padding.vertical()).max(0.);
                        match measure(
                            &entry.key,
                            MeasureInput {
                                known: Constraints {
                                    width: Some(width),
                                    height: Some(height),
                                },
                                width: Available::Definite(width),
                                height: Available::Definite(height),
                            },
                        ) {
                            Ok(m) if valid_measurement(m, extent) => {
                                output.baselines.first =
                                    m.baseline.map(|b| (b + entry.padding.top) as f32);
                            }
                            Ok(_) => failure = Some(Error::InvalidValue),
                            Err(e) => failure = Some(e),
                        }
                    }
                }
                output
            },
        )?;
        if let Some(e) = failure {
            return Err(e);
        }
        Ok(())
    }
    fn collect(
        &self,
        i: usize,
        origin: [f64; 2],
        frames: &mut BTreeMap<String, Frame>,
        content_frames: &mut BTreeMap<String, Frame>,
    ) -> Result<(), Error> {
        let e = &self.entries[i];
        let l = self.tree.layout(e.id)?;
        let frame = Frame {
            x: origin[0] + f64::from(l.location.x),
            y: origin[1] + f64::from(l.location.y),
            size: Size::new(l.size.width.into(), l.size.height.into()),
        };
        if !frame.size.valid(self.limits.extent) || !frame.x.is_finite() || !frame.y.is_finite() {
            return Err(Error::BudgetExceeded);
        }
        let slack = 1e-5 + frame.size.width.max(frame.size.height) * f64::from(f32::EPSILON) * 8.;
        if e.node.maximum.is_some_and(|m| {
            frame.size.width > m.width + slack || frame.size.height > m.height + slack
        }) {
            return Err(Error::InsufficientSpace(e.key.clone()));
        }
        for &child in &e.children {
            let c = self.tree.layout(self.entries[child].id)?;
            if e.node.overflow == Overflow::Fit
                && (f64::from(c.location.x) < e.padding.left - slack
                    || f64::from(c.location.y) < e.padding.top - slack
                    || f64::from(c.location.x + c.size.width)
                        > frame.size.width - e.padding.right + slack
                    || f64::from(c.location.y + c.size.height)
                        > frame.size.height - e.padding.bottom + slack)
            {
                return Err(Error::InsufficientSpace(e.key.clone()));
            }
            self.collect(child, [frame.x, frame.y], frames, content_frames)?;
        }
        let content = Frame {
            x: frame.x + e.padding.left,
            y: frame.y + e.padding.top,
            size: Size::new(
                (frame.size.width - e.padding.horizontal()).max(0.),
                (frame.size.height - e.padding.vertical()).max(0.),
            ),
        };
        content_frames.insert(e.key.clone(), content);
        frames.insert(e.key.clone(), frame);
        Ok(())
    }
}
fn convert_available(v: t::AvailableSpace) -> Available {
    match v {
        t::AvailableSpace::Definite(v) => Available::Definite(v.into()),
        t::AvailableSpace::MinContent => Available::MinContent,
        t::AvailableSpace::MaxContent => Available::MaxContent,
    }
}

pub fn resolve(root: &Node, offered: Option<Size>, limits: Limits) -> Result<Layout, Error> {
    resolve_with_spacing(root, offered, limits, &SpacingScale::default())
}
pub fn resolve_with_spacing(
    root: &Node,
    offered: Option<Size>,
    limits: Limits,
    spacing: &SpacingScale,
) -> Result<Layout, Error> {
    let constraints = offered.map_or(Constraints::default(), |s| Constraints {
        width: Some(s.width),
        height: Some(s.height),
    });
    resolve_measured(root, constraints, limits, spacing, |key, _| {
        Err(Error::MissingMeasurement(key.into()))
    })
}
/// Resolve with independent parent constraints and a width-aware leaf measurer.
/// Auto flows prefer a single row at max-content width and fall back to a column.
/// Each flow can switch once per resolution; no previous-frame state or oscillation.
pub fn resolve_measured(
    root: &Node,
    constraints: Constraints,
    limits: Limits,
    spacing: &SpacingScale,
    mut measure: impl FnMut(&str, MeasureInput) -> Result<Size, Error>,
) -> Result<Layout, Error> {
    resolve_measured_with_baseline(root, constraints, limits, spacing, |id, input| {
        measure(id, input).map(Into::into)
    })
}
fn valid_measurement(m: Measurement, extent: f64) -> bool {
    m.size.valid(extent)
        && m.baseline
            .is_none_or(|b| b.is_finite() && b >= 0. && b <= extent)
}
/// Resolve with first-baseline font metrics. The size-only API remains supported.
pub fn resolve_measured_with_baseline(
    root: &Node,
    constraints: Constraints,
    limits: Limits,
    spacing: &SpacingScale,
    mut measure: impl FnMut(&str, MeasureInput) -> Result<Measurement, Error>,
) -> Result<Layout, Error> {
    if !limits.extent.is_finite()
        || limits.extent <= 0.
        || limits.extent > 1e7
        || limits.nodes == 0
        || limits.depth > 128
        || !spacing.valid()
        || [constraints.width, constraints.height]
            .into_iter()
            .flatten()
            .any(|v| !v.is_finite() || v < 0. || v > limits.extent)
    {
        return Err(Error::InvalidValue);
    }
    let mut builder = Builder {
        tree: t::TaffyTree::new(),
        entries: Vec::new(),
        keys: BTreeMap::new(),
        limits,
        spacing,
    };
    builder.tree.disable_rounding();
    let root_index = builder.add(root, "", "0", 0)?;
    let id = builder.entries[root_index].id;
    let original = builder.tree.style(id)?.clone();
    // Intrinsic preferred widths: root parent constraints do not contaminate this pass.
    if builder
        .entries
        .iter()
        .any(|e| matches!(e.node.kind, Kind::Stack(_)) && e.node.axis == Axis::Auto)
    {
        builder.compute(root_index, t::Size::MAX_CONTENT, &mut measure)?;
    }
    let mut adaptive = Vec::new();
    for e in &builder.entries {
        if matches!(e.node.kind, Kind::Stack(_)) && e.node.axis == Axis::Auto {
            let required = e
                .children
                .iter()
                .map(|i| {
                    builder
                        .tree
                        .layout(builder.entries[*i].id)
                        .map(|l| f64::from(l.size.width))
                })
                .collect::<Result<Vec<_>, _>>()?
                .iter()
                .sum::<f64>()
                + e.gap * e.children.len().saturating_sub(1) as f64
                + e.padding.horizontal();
            adaptive.push((e.id, required));
        }
    }
    let mut style = original;
    for (dim, value, setting) in [
        (&mut style.size.width, constraints.width, root.width),
        (&mut style.size.height, constraints.height, root.height),
    ] {
        if let Some(v) = value {
            if setting.is_none() || setting == Some(Sizing::Fill) {
                *dim = length(v as f32);
            }
        }
    }
    builder.tree.set_style(id, style)?;
    let available = t::Size {
        width: constraints
            .width
            .map_or(t::AvailableSpace::MaxContent, |v| {
                t::AvailableSpace::Definite(v as f32)
            }),
        height: constraints
            .height
            .map_or(t::AvailableSpace::MaxContent, |v| {
                t::AvailableSpace::Definite(v as f32)
            }),
    };
    loop {
        builder.compute(root_index, available, &mut measure)?;
        let mut changed = false;
        // Entries are postorder: settle ancestors before measuring descendants again.
        for &(id, required) in adaptive.iter().rev() {
            if builder.tree.style(id)?.flex_direction == t::FlexDirection::Row
                && f64::from(builder.tree.layout(id)?.size.width) + 1e-4 < required
            {
                let mut style = builder.tree.style(id)?.clone();
                style.flex_direction = t::FlexDirection::Column;
                let node = builder.entries.iter().find(|e| e.id == id).unwrap().node;
                apply_position(&mut style, node, true);
                builder.tree.set_style(id, style)?;
                changed = true;
                break;
            }
        }
        if !changed {
            break;
        }
    }
    let mut frames = BTreeMap::new();
    let mut content_frames = BTreeMap::new();
    builder.collect(root_index, [0., 0.], &mut frames, &mut content_frames)?;
    let size = frames[&builder.entries[root_index].key].size;
    // Hard minimums may cause Taffy to exceed the offered size; surface that conflict.
    if constraints
        .width
        .is_some_and(|v| root.width != Some(Sizing::Hug) && size.width > v + 1e-4)
        || constraints
            .height
            .is_some_and(|v| root.height != Some(Sizing::Hug) && size.height > v + 1e-4)
    {
        return Err(Error::InsufficientSpace(root.key.clone()));
    }
    let mut scroll_limits = BTreeMap::new();
    for entry in &builder.entries {
        let viewport = content_frames[&entry.key];
        let mut limit = Size::default();
        if entry.node.overflow == Overflow::Scroll {
            for child in &entry.children {
                let frame = frames[&builder.entries[*child].key];
                limit.width = limit.width.max(frame.right() - viewport.right());
                limit.height = limit.height.max(frame.bottom() - viewport.bottom());
            }
        }
        scroll_limits.insert(entry.key.clone(), limit);
    }
    Ok(Layout {
        size,
        frames,
        content_frames,
        scroll_limits,
    })
}
