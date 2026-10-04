//! Virtual collections built from ordinary MUI scroll, focus and semantic nodes.
//!
//! Retain [`ListState`] in the view, update its keys only when the model changes,
//! and construct only the rows requested by [`uniform_list`] or [`variable_list`].
//! Variable rows report their actual height during construction; offscreen rows
//! use the estimate until visited, or heights supplied with [`ListState::set_height`].
//! A changed offscreen row must notify the height index explicitly.
//!
//! GPUI's `elements/{uniform_list,list}.rs` at a84689073d296dfd39987bc7dd478e43ef76d83a
//! was reviewed (gpui is Apache-2.0). Its layout/application callbacks and SumTree
//! depend on GPUI's runtime. This implementation uses MUI's existing scroll tree
//! and a standard Fenwick height index; no GPUI source is copied.

use super::Response;
use crate::Ui;
use mui_input::Key;
use mui_scene::{A11y, El, Id, Styled, block, col};
use rustc_hash::FxHashMap;
use std::{collections::VecDeque, ops::Range};

/// Where to put an item when scrolling from code.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ScrollTo {
    Start,
    Center,
    End,
    /// Move only enough to expose an item outside the viewport.
    #[default]
    Nearest,
}

/// A finite, fixed viewport in logical pixels. Resize it with the view.
#[derive(Clone, Copy, Debug)]
pub struct ListOptions {
    pub width: f64,
    pub height: f64,
    /// Extra logical pixels built above and below the viewport.
    pub overscan: f64,
    /// Arrow, Home/End, PageUp/Down and Enter/Space handling on row/list focus.
    /// Keys in a row's child control remain that control's responsibility.
    pub keyboard: bool,
}

impl ListOptions {
    pub fn new(width: f64, height: f64) -> Self {
        Self {
            width,
            height,
            overscan: 64.0,
            keyboard: true,
        }
    }
    fn validate(self) {
        assert!(
            self.width.is_finite() && self.width > 0.0,
            "list width must be finite and positive"
        );
        assert!(
            self.height.is_finite() && self.height > 0.0,
            "list height must be finite and positive"
        );
        assert!(
            self.overscan.is_finite() && self.overscan >= 0.0,
            "list overscan must be finite and nonnegative"
        );
    }
}

/// A visible row's stable identity. Use `id.field("control")` for child widgets;
/// display indexes change on reorder, model keys and these IDs do not.
#[derive(Clone, Debug)]
pub struct ListItem {
    pub index: usize,
    pub key: u64,
    pub id: Id,
    /// The indexed height, or the estimate before first measurement.
    pub height: f64,
    pub active: bool,
}

/// A variable row with its actual logical height, computed only when visible.
/// The row element is clipped to this height. Recompute it after width changes.
#[must_use]
pub struct ListRow {
    pub el: El,
    pub height: f64,
}
impl ListRow {
    pub fn new(el: El, height: f64) -> Self {
        assert!(
            valid_height(height),
            "row height must be finite and positive"
        );
        Self { el, height }
    }
}

/// Collection interaction and real construction counts for this build.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ListEvent {
    pub active: Option<u64>,
    pub activated: Option<u64>,
    /// Intersecting the viewport, without overscan.
    pub visible: Range<usize>,
    /// The contiguous range passed to the row constructor, including overscan.
    pub rendered: Range<usize>,
    pub rows_built: usize,
}

#[derive(Clone, Debug)]
enum Heights {
    Uniform(f64),
    Variable { values: Vec<f64>, tree: Vec<f64> },
}

impl Heights {
    fn variable(values: Vec<f64>) -> Self {
        let mut tree = vec![0.0; values.len() + 1];
        // Linear construction; individual height changes update logarithmically.
        for (i, value) in values.iter().enumerate() {
            let j = i + 1;
            tree[j] += value;
            let parent = j + j.isolate_lowest_one();
            if parent < tree.len() {
                tree[parent] += tree[j];
            }
        }
        Self::Variable { values, tree }
    }
    fn height(&self, index: usize) -> f64 {
        match self {
            Self::Uniform(h) => *h,
            Self::Variable { values, .. } => values[index],
        }
    }
    fn prefix(&self, mut count: usize) -> f64 {
        match self {
            Self::Uniform(h) => count as f64 * h,
            Self::Variable { tree, .. } => {
                let mut sum = 0.0;
                while count > 0 {
                    sum += tree[count];
                    count &= count - 1;
                }
                sum
            }
        }
    }
    fn set(&mut self, index: usize, height: f64) {
        if let Self::Variable { values, tree } = self {
            let delta = height - values[index];
            values[index] = height;
            let mut i = index + 1;
            while i < tree.len() {
                tree[i] += delta;
                i += i.isolate_lowest_one();
            }
        }
    }
    /// Index containing `offset`; exact row boundaries belong to the next row.
    fn index_at(&self, offset: f64, count: usize) -> usize {
        if offset <= 0.0 || count == 0 {
            return 0;
        }
        match self {
            Self::Uniform(h) => ((offset / h).floor() as usize).min(count),
            Self::Variable { tree, .. } => {
                let mut index = 0;
                let mut sum = 0.0;
                let mut bit = count.isolate_highest_one();
                while bit > 0 {
                    let next = index + bit;
                    if next <= count && sum + tree[next] <= offset {
                        sum += tree[next];
                        index = next;
                    }
                    bit >>= 1;
                }
                index
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Anchor {
    key: u64,
    index: usize,
    within: f64,
}
#[derive(Clone, Copy, Debug)]
struct Pending {
    key: u64,
    align: ScrollTo,
    focus: bool,
}

/// Retained keyed collection state. Initialization and `set_items` are O(n).
/// Unchanged builds touch only visible/overscan rows, range lookup and individual
/// variable height changes are O(log n). Uniform range lookup is O(1).
///
/// Keys must be unique permanent model IDs. Removing/reordering/inserting keys
/// preserves the top visible key and its offset within the row. If that key is
/// removed, the row now at its old position becomes the anchor. Focused controls
/// that leave the built range yield focus to the list; their local widget state
/// follows MUI's normal lifetime (removed nodes are retired).
#[derive(Clone, Debug)]
pub struct ListState {
    keys: Vec<u64>,
    indexes: FxHashMap<u64, usize>,
    heights: Heights,
    estimate: f64,
    offset: f64,
    native_offset: f64,
    dirty_offset: bool,
    pending: Option<Pending>,
    active: Option<u64>,
}

impl ListState {
    pub fn uniform(keys: impl IntoIterator<Item = u64>, height: f64) -> Self {
        Self::new(keys.into_iter().collect(), height, true)
    }
    /// Convenience for append-only/index-keyed data. For editable collections
    /// use permanent IDs with `uniform` rather than changing display indexes.
    pub fn uniform_count(count: usize, height: f64) -> Self {
        Self::uniform((0..count).map(|i| i as u64), height)
    }
    pub fn variable(keys: impl IntoIterator<Item = u64>, estimated_height: f64) -> Self {
        Self::new(keys.into_iter().collect(), estimated_height, false)
    }
    fn new(keys: Vec<u64>, height: f64, uniform: bool) -> Self {
        assert!(
            valid_height(height) && (keys.len() as f64 * height).is_finite(),
            "invalid list height"
        );
        let indexes = index_keys(&keys);
        let heights = if uniform {
            Heights::Uniform(height)
        } else {
            Heights::variable(vec![height; keys.len()])
        };
        Self {
            keys,
            indexes,
            heights,
            estimate: height,
            offset: 0.0,
            native_offset: 0.0,
            dirty_offset: false,
            pending: None,
            active: None,
        }
    }
    pub fn len(&self) -> usize {
        self.keys.len()
    }
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }
    pub fn key(&self, index: usize) -> Option<u64> {
        self.keys.get(index).copied()
    }
    pub fn index(&self, key: u64) -> Option<usize> {
        self.indexes.get(&key).copied()
    }
    pub fn height(&self, index: usize) -> Option<f64> {
        (index < self.len()).then(|| self.heights.height(index))
    }
    pub fn total_height(&self) -> f64 {
        self.heights.prefix(self.len())
    }
    pub fn scroll_offset(&self) -> f64 {
        self.offset
    }
    pub fn active(&self) -> Option<u64> {
        self.active
    }

    /// Replace the model order, retaining measured heights by key. Only call
    /// when the data changes, never as part of each frame's row construction.
    // ponytail: O(n) model edits; use an order-statistic tree if frequent large
    // splices become a measured bottleneck.
    pub fn set_items(&mut self, keys: impl IntoIterator<Item = u64>) {
        let keys: Vec<_> = keys.into_iter().collect();
        let indexes = index_keys(&keys);
        let anchor = self.anchor();
        let heights = match self.heights {
            Heights::Uniform(h) => Heights::Uniform(h),
            Heights::Variable { .. } => Heights::variable(
                keys.iter()
                    .map(|key| {
                        self.index(*key)
                            .map_or(self.estimate, |i| self.heights.height(i))
                    })
                    .collect(),
            ),
        };
        assert!(
            heights.prefix(keys.len()).is_finite(),
            "list height overflow"
        );
        self.keys = keys;
        self.indexes = indexes;
        self.heights = heights;
        self.active = self.active.filter(|k| self.indexes.contains_key(k));
        self.restore(anchor);
    }
    /// Insert/remove a range of model keys. Row construction remains lazy;
    /// rebuilding the vector and height index costs O(n) only on this edit.
    pub fn splice(&mut self, range: Range<usize>, replacement: impl IntoIterator<Item = u64>) {
        assert!(
            range.start <= range.end && range.end <= self.len(),
            "invalid list splice"
        );
        let mut keys = self.keys.clone();
        keys.splice(range, replacement);
        self.set_items(keys);
    }
    /// Update a variable row, preserving the scroll anchor. Invalid indexes,
    /// non-finite/non-positive heights and uniform states return `false`.
    pub fn set_height(&mut self, index: usize, height: f64) -> bool {
        if index >= self.len()
            || !valid_height(height)
            || !matches!(self.heights, Heights::Variable { .. })
        {
            return false;
        }
        let old = self.heights.height(index);
        if old == height {
            return true;
        }
        if !(self.total_height() - old + height).is_finite() {
            return false;
        }
        let anchor = self.anchor();
        self.heights.set(index, height);
        self.restore(anchor);
        true
    }
    /// Change all uniform row heights, preserving the top key and pixel offset.
    pub fn set_uniform_height(&mut self, height: f64) -> bool {
        if !matches!(self.heights, Heights::Uniform(_))
            || !valid_height(height)
            || !(self.len() as f64 * height).is_finite()
        {
            return false;
        }
        let anchor = self.anchor();
        self.heights = Heights::Uniform(height);
        self.estimate = height;
        self.restore(anchor);
        true
    }
    /// Schedule scrolling for the next widget build. The key is captured now,
    /// so a reorder before the next build still targets the same model item.
    pub fn scroll_to_item(&mut self, index: usize, align: ScrollTo) -> bool {
        self.schedule(index, align, false)
    }
    /// Bring an offscreen item into view before giving its row keyboard focus.
    pub fn focus_item(&mut self, index: usize) -> bool {
        self.schedule(index, ScrollTo::Nearest, true)
    }
    fn schedule(&mut self, index: usize, align: ScrollTo, focus: bool) -> bool {
        let Some(key) = self.key(index) else {
            return false;
        };
        self.pending = Some(Pending { key, align, focus });
        true
    }
    fn anchor(&self) -> Option<Anchor> {
        let i = self
            .heights
            .index_at(self.offset, self.len())
            .min(self.len().saturating_sub(1));
        self.key(i).map(|key| Anchor {
            key,
            index: i,
            within: self.offset - self.heights.prefix(i),
        })
    }
    fn restore(&mut self, anchor: Option<Anchor>) {
        self.offset = anchor.filter(|_| !self.is_empty()).map_or(0.0, |a| {
            let i = self.index(a.key).unwrap_or(a.index.min(self.len() - 1));
            self.heights.prefix(i)
                + a.within
                    .max(0.0)
                    .min(self.heights.height(i) * (1.0 - f64::EPSILON))
        });
        self.dirty_offset = true;
    }
    fn align_item(&mut self, index: usize, height: f64, align: ScrollTo) {
        let top = self.heights.prefix(index);
        let bottom = top + self.heights.height(index);
        self.offset = match align {
            ScrollTo::Start => top,
            ScrollTo::Center => (top + bottom - height) * 0.5,
            ScrollTo::End => bottom - height,
            ScrollTo::Nearest if top < self.offset => top,
            ScrollTo::Nearest if bottom > self.offset + height => {
                // An oversized row starts at its top instead of hiding its header.
                if bottom - top > height {
                    top
                } else {
                    bottom - height
                }
            }
            ScrollTo::Nearest => self.offset,
        };
        self.clamp(height);
    }
    fn clamp(&mut self, height: f64) {
        self.offset = self
            .offset
            .clamp(0.0, (self.total_height() - height).max(0.0));
    }
    fn visible(&self, height: f64) -> Range<usize> {
        let start = self.heights.index_at(self.offset, self.len());
        let end_y = self.offset + height;
        let i = self.heights.index_at(end_y, self.len());
        let end = if i < self.len() && self.heights.prefix(i) < end_y {
            i + 1
        } else {
            i
        };
        start..end
    }
}

fn valid_height(height: f64) -> bool {
    height.is_finite() && height > 0.0
}
fn index_keys(keys: &[u64]) -> FxHashMap<u64, usize> {
    let mut indexes = FxHashMap::default();
    indexes.reserve(keys.len());
    for (i, key) in keys.iter().enumerate() {
        assert!(
            indexes.insert(*key, i).is_none(),
            "duplicate list key {key}"
        );
    }
    indexes
}

/// Construct only uniform rows intersecting the viewport plus overscan. Rows
/// must have the height used in `ListState::uniform`; excess content is clipped.
/// Native wheel scrolling and scrollbar dragging take effect on the next build,
/// exactly like other MUI widgets reading the previous frame's interactions.
pub fn uniform_list(
    ui: &mut Ui,
    id: impl Into<Id>,
    state: &mut ListState,
    options: ListOptions,
    mut render: impl FnMut(&mut Ui, ListItem) -> El,
) -> Response<ListEvent> {
    assert!(
        matches!(state.heights, Heights::Uniform(_)),
        "uniform_list requires uniform state"
    );
    build(ui, id.into(), state, options, |ui, item| {
        let height = item.height;
        ListRow::new(render(ui, item), height)
    })
}

/// Construct variable rows lazily, reporting each row's exact height with
/// `ListRow`. The height index updates immediately and preserves the top key;
/// the loop continues until the measured rows fill the viewport plus overscan.
/// Offscreen height changes use `state.set_height`. No offscreen constructor or
/// layout is invoked to measure unknown rows.
pub fn variable_list(
    ui: &mut Ui,
    id: impl Into<Id>,
    state: &mut ListState,
    options: ListOptions,
    render: impl FnMut(&mut Ui, ListItem) -> ListRow,
) -> Response<ListEvent> {
    assert!(
        matches!(state.heights, Heights::Variable { .. }),
        "variable_list requires variable state"
    );
    build(ui, id.into(), state, options, render)
}

fn row_focus(focus: &str, row_parent: &Id) -> Option<(u64, bool)> {
    let suffix = focus.strip_prefix(row_parent.as_str())?.strip_prefix('/')?;
    let (key, child) = suffix
        .split_once('/')
        .map_or((suffix, false), |(key, _)| (key, true));
    Some((key.parse().ok()?, child))
}

fn build(
    ui: &mut Ui,
    id: Id,
    state: &mut ListState,
    options: ListOptions,
    mut render: impl FnMut(&mut Ui, ListItem) -> ListRow,
) -> Response<ListEvent> {
    options.validate();
    let native = ui.scroll(&id)[1];
    if state.dirty_offset {
        // Model edits may arrive after the previous frame landed a wheel or
        // scrollbar move. Preserve that native delta as well as the anchor.
        state.offset += native - state.native_offset;
    } else {
        state.offset = native;
    }
    state.clamp(options.height);
    let row_parent = id.field("item");
    let focused_row = ui
        .focus_key()
        .and_then(|focus| row_focus(focus, &row_parent));
    let reveal_focus = focused_row
        .filter(|(key, _)| state.index(*key).is_some() && state.active != Some(*key))
        .map(|(key, _)| Pending {
            key,
            align: ScrollTo::Nearest,
            focus: false,
        });
    if let Some((key, _)) = focused_row.filter(|(key, _)| state.index(*key).is_some()) {
        state.active = Some(key);
    }
    let mut focus_target = None;
    let mut activated = None;
    if options.keyboard
        && (ui.focused(&id) || focused_row.is_some_and(|(_, child)| !child))
        && !state.is_empty()
    {
        let keys = ui.keys(ui.focus_key().unwrap_or(id.as_str())).to_vec();
        for press in keys {
            // Modified keys belong to shortcuts and host-level navigation.
            if press.mods.ctrl || press.mods.cmd || press.mods.alt {
                continue;
            }
            let i = state
                .active
                .and_then(|key| state.index(key))
                .unwrap_or_else(|| {
                    state
                        .heights
                        .index_at(state.offset, state.len())
                        .min(state.len() - 1)
                });
            let next = match press.key {
                Key::Down => Some((i + 1).min(state.len() - 1)),
                Key::Up => Some(i.saturating_sub(1)),
                Key::Home => Some(0),
                Key::End => Some(state.len() - 1),
                Key::PageDown => Some(
                    state
                        .heights
                        .index_at(state.heights.prefix(i) + options.height, state.len())
                        .min(state.len() - 1),
                ),
                Key::PageUp => Some(state.heights.index_at(
                    (state.heights.prefix(i) - options.height).max(0.0),
                    state.len(),
                )),
                Key::Enter | Key::Space => {
                    activated = state.key(i);
                    None
                }
                _ => None,
            };
            if let Some(next) = next {
                state.active = state.key(next);
                state.align_item(next, options.height, ScrollTo::Nearest);
                focus_target = state.key(next);
            }
        }
    }
    let mut requested = state.pending.take().or(reveal_focus);
    if requested.is_none() {
        requested = focus_target.map(|key| Pending {
            key,
            align: ScrollTo::Nearest,
            focus: true,
        });
    }
    // Measure a requested destination before aligning to it. An estimate of a
    // tall preceding row must not keep an offscreen focus request unconstructed.
    let mut prepared = None;
    if let Some(pending) = requested
        && let Some(index) = state.index(pending.key)
    {
        if pending.focus {
            state.active = Some(pending.key);
            focus_target = Some(pending.key);
        }
        prepared = Some((
            index,
            build_row(ui, &row_parent, state, index, options.width, &mut render),
        ));
        state.align_item(index, options.height, pending.align);
    }
    let mut start = state
        .heights
        .index_at((state.offset - options.overscan).max(0.0), state.len());
    let mut rows = VecDeque::new();
    let mut i = start;
    let mut y = state.heights.prefix(start);
    let mut rows_built = usize::from(prepared.is_some());
    let destination = requested.and_then(|p| state.index(p.key));
    while i < state.len()
        && (y < state.offset + options.height + options.overscan
            || destination.is_some_and(|target| i <= target))
    {
        let row = if prepared.as_ref().is_some_and(|(index, _)| *index == i) {
            prepared.take().expect("prepared row").1
        } else {
            rows_built += 1;
            build_row(ui, &row_parent, state, i, options.width, &mut render)
        };
        rows.push_back(row);
        y += state.heights.height(i);
        i += 1;
    }
    if let Some(pending) = requested
        && let Some(index) = state.index(pending.key)
    {
        state.align_item(index, options.height, pending.align);
    }
    while i < state.len() && y < state.offset + options.height + options.overscan {
        rows_built += 1;
        rows.push_back(build_row(
            ui,
            &row_parent,
            state,
            i,
            options.width,
            &mut render,
        ));
        y += state.heights.height(i);
        i += 1;
    }
    state.clamp(options.height);
    // Tail measurements may shrink the scroll extent. Construct newly exposed
    // leading rows immediately rather than showing a blank strip for a frame.
    while start > 0 && state.heights.prefix(start) > (state.offset - options.overscan).max(0.0) {
        start -= 1;
        let row = if prepared.as_ref().is_some_and(|(index, _)| *index == start) {
            prepared.take().expect("prepared row").1
        } else {
            rows_built += 1;
            build_row(ui, &row_parent, state, start, options.width, &mut render)
        };
        rows.push_front(row);
        state.clamp(options.height);
    }
    if let Some(key) = focus_target
        && let Some(index) = state.index(key)
        && (start..i).contains(&index)
    {
        ui.focus(row_parent.entity(key));
    }
    if focus_target.is_none()
        && let Some((key, _)) = focused_row
        && !state
            .index(key)
            .is_some_and(|index| (start..i).contains(&index))
    {
        ui.focus(&id);
    }
    let top = state.heights.prefix(start);
    let bottom = (state.total_height() - state.heights.prefix(i)).max(0.0);
    let mut children = Vec::with_capacity(rows.len() + 2);
    children.push(block(options.width, top).shrink(0.0));
    children.extend(rows);
    children.push(block(options.width, bottom).shrink(0.0));
    // Match the constructed range to the rendered offset; springing from an
    // old position would expose rows that intentionally were never constructed.
    ui.set_scroll(&id, [0.0, state.offset]);
    state.native_offset = state.offset;
    state.dirty_offset = false;
    let mut el = col(children)
        .size(options.width, options.height)
        .shrink(0.0)
        .scroll()
        .id(id)
        .focusable()
        .a11y(A11y::Scroll);
    el.payload_mut().extras_mut().virtual_scroll_extent =
        Some(state.total_height().max(options.width).max(options.height));
    Response {
        el,
        changed: ListEvent {
            active: state.active,
            activated,
            visible: state.visible(options.height),
            rendered: start..i,
            rows_built,
        },
    }
}

fn build_row(
    ui: &mut Ui,
    parent: &Id,
    state: &mut ListState,
    index: usize,
    width: f64,
    render: &mut impl FnMut(&mut Ui, ListItem) -> ListRow,
) -> El {
    let key = state.keys[index];
    let id = parent.entity(key);
    if ui.get(&id).pressed {
        state.active = Some(key);
    }
    let row = render(
        ui,
        ListItem {
            index,
            key,
            id: id.clone(),
            height: state.heights.height(index),
            active: state.active == Some(key),
        },
    );
    assert!(
        valid_height(row.height),
        "row height must be finite and positive"
    );
    if matches!(state.heights, Heights::Variable { .. }) {
        assert!(state.set_height(index, row.height), "list height overflow");
    }
    col([row.el])
        .size(width, row.height)
        .shrink(0.0)
        .clip()
        .id(id)
        .focusable()
        .a11y(A11y::Group)
        .named(format!("Item {} of {}", index + 1, state.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indexed_height_updates_and_pixel_queries_match_a_linear_reference() {
        let mut values = (0..257).map(|i| (i % 7 + 1) as f64).collect::<Vec<_>>();
        let mut heights = Heights::variable(values.clone());
        for step in 0..1024 {
            let i = step * 73 % values.len();
            values[i] = (step % 19 + 1) as f64;
            heights.set(i, values[i]);
            let mut prefix = 0.0;
            for (j, height) in values.iter().enumerate() {
                assert_eq!(heights.prefix(j), prefix);
                assert_eq!(heights.index_at(prefix, values.len()), j);
                assert_eq!(heights.index_at(prefix + height * 0.5, values.len()), j);
                prefix += height;
            }
            assert_eq!(heights.prefix(values.len()), prefix);
            assert_eq!(heights.index_at(prefix, values.len()), values.len());
        }
    }
}
