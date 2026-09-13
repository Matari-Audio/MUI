//! Viewport state is separate from layout: scrolling and transforms never reflow siblings.
use crate::{Frame, Overflow, ResolvedScene, SceneError, Ui};
use mui_geometry::{Affine, Point};
use std::collections::BTreeMap;

/// A content-box clip in original scene coordinates, mapped to the screen by `transform`.
/// Renderers must intersect all ancestor clips, including rotated clips (not just their AABBs).
#[derive(Clone, Debug, PartialEq)]
pub struct Clip {
    pub frame: Frame,
    pub transform: Affine,
}
impl Clip {
    pub fn contains(&self, point: Point) -> bool {
        self.transform
            .inverse()
            .is_some_and(|inverse| inside(self.frame, inverse.apply(point)))
    }
}
#[derive(Clone, Debug)]
pub struct ViewItem {
    /// Apply to the item's original scene-space paths, text origins and layout frames.
    pub transform: Affine,
    /// Clips inherited from ancestors, in outermost-first order.
    pub clips: Vec<Clip>,
    /// This item's content viewport, to be pushed before painting its descendants.
    pub viewport: Option<Clip>,
    /// Resolved positive offset, clamped against the current layout on both axes.
    pub scroll: Point,
}
impl ViewItem {
    pub fn visible_at(&self, point: Point) -> bool {
        point.finite() && self.clips.iter().all(|clip| clip.contains(point))
    }
}
/// UI-thread state keyed by stable, fully scoped item IDs. Offsets use logical pixels.
#[derive(Clone, Debug, Default)]
pub struct ViewState {
    scroll: BTreeMap<String, Point>,
    transforms: BTreeMap<String, Affine>,
}
impl ViewState {
    pub fn set_scroll(&mut self, id: impl Into<String>, offset: Point) -> Result<(), SceneError> {
        if !offset.finite() || offset.x < 0. || offset.y < 0. {
            return Err(SceneError::InvalidModifier(
                "scroll offsets must be finite and nonnegative",
            ));
        }
        let id = id.into();
        if !self.scroll.contains_key(&id) && self.scroll.len() >= 2048 {
            return Err(SceneError::DependencyDepth);
        }
        self.scroll.insert(id, offset);
        Ok(())
    }
    /// Transform around the item's top-left layout origin, inherited by descendants.
    /// Singular transforms are rejected without changing the previous state.
    pub fn set_transform(
        &mut self,
        id: impl Into<String>,
        transform: Affine,
    ) -> Result<(), SceneError> {
        if transform.inverse().is_none() {
            return Err(SceneError::InvalidModifier(
                "view transforms must have a finite inverse",
            ));
        }
        let id = id.into();
        if !self.transforms.contains_key(&id) && self.transforms.len() >= 2048 {
            return Err(SceneError::DependencyDepth);
        }
        self.transforms.insert(id, transform);
        Ok(())
    }
    pub fn remove(&mut self, id: &str) {
        self.scroll.remove(id);
        self.transforms.remove(id);
    }
    pub fn resolve<'a>(
        &self,
        ui: &'a Ui,
        scene: &'a ResolvedScene,
    ) -> Result<View<'a>, SceneError> {
        for id in self.scroll.keys().chain(self.transforms.keys()) {
            if ui.info(id).is_none() {
                return Err(SceneError::MissingLayoutFrame(id.clone()));
            }
        }
        let mut items: BTreeMap<String, ViewItem> = BTreeMap::new();
        for (id, info) in ui.items() {
            let frame = scene
                .layout
                .frame(id)
                .ok_or_else(|| SceneError::MissingLayoutFrame(id.into()))?;
            let content = scene
                .layout
                .content_frame(id)
                .ok_or_else(|| SceneError::MissingLayoutFrame(id.into()))?;
            let mut clips = Vec::new();
            let inherited = if let Some(parent) = info.parent.as_ref() {
                let parent = &items[parent];
                clips.clone_from(&parent.clips);
                clips.extend(parent.viewport.clone());
                Affine::translation(-parent.scroll.x, -parent.scroll.y).then(parent.transform)
            } else {
                Affine::IDENTITY
            };
            let local = self.transforms.get(id).copied().unwrap_or_default();
            let transform = Affine::translation(-frame.x, -frame.y)
                .then(local)
                .then(Affine::translation(frame.x, frame.y))
                .then(inherited);
            if transform.inverse().is_none() {
                return Err(SceneError::InvalidModifier(
                    "composed view transform has no finite inverse",
                ));
            }
            let requested = self.scroll.get(id).copied().unwrap_or(Point::ZERO);
            let limit = scene.layout.scroll_limit(id).unwrap_or_default();
            let scroll = Point::new(requested.x.min(limit.width), requested.y.min(limit.height));
            let viewport = (info.overflow != Overflow::Fit).then_some(Clip {
                frame: content,
                transform,
            });
            items.insert(
                id.into(),
                ViewItem {
                    transform,
                    clips,
                    viewport,
                    scroll,
                },
            );
        }
        // A Boolean merge is one painted surface. It cannot follow divergent member transforms.
        for group in &ui.groups {
            let first = items[&group.members[0]].clone();
            if group
                .members
                .iter()
                .any(|id| items[id].transform != first.transform || items[id].clips != first.clips)
            {
                return Err(SceneError::InvalidModifier(
                    "merged items must share view transforms and clips",
                ));
            }
            items.insert(group.id.clone(), first);
        }
        Ok(View { ui, scene, items })
    }
    /// Apply a screen-space wheel delta to the deepest viewport under the pointer.
    /// Unconsumed movement bubbles to its scroll ancestors; the return value is leftover delta.
    /// Resolve a fresh view after this call before painting or dispatching another event.
    pub fn scroll_at(
        &mut self,
        view: &View<'_>,
        point: Point,
        mut delta: Point,
    ) -> Result<Point, SceneError> {
        if !point.finite() || !delta.finite() {
            return Err(SceneError::InvalidModifier(
                "wheel coordinates must be finite",
            ));
        }
        let mut id = view.hit_item(point, true)?;
        while let Some(current) = id {
            let info = &view.ui.items[current];
            let item = &view.items[current];
            if info.overflow == Overflow::Scroll {
                let inverse = item.transform.inverse().expect("validated view transform");
                let local_delta = vector(inverse, delta);
                let limit = view.scene.layout.scroll_limit(current).unwrap_or_default();
                let next = Point::new(
                    (item.scroll.x + local_delta.x).clamp(0., limit.width),
                    (item.scroll.y + local_delta.y).clamp(0., limit.height),
                );
                delta = delta - vector(item.transform, next - item.scroll);
                self.set_scroll(current, next)?;
            }
            id = info.parent.as_deref();
        }
        Ok(delta)
    }
}
/// A single snapshot shared by painting and picking. Rebuild after layout/state changes.
pub struct View<'a> {
    ui: &'a Ui,
    scene: &'a ResolvedScene,
    items: BTreeMap<String, ViewItem>,
}
impl View<'_> {
    pub fn item(&self, id: &str) -> Option<&ViewItem> {
        self.items.get(id)
    }
    /// Paint order, matching `Ui::items`. Apply each entry's transform and clips when painting.
    pub fn items(&self) -> impl Iterator<Item = (&str, &ViewItem)> {
        self.ui.items().map(|(id, _)| (id, &self.items[id]))
    }
    /// Shape-aware picking in screen coordinates. Painted decorations occlude older siblings;
    /// a noninteractive child bubbles to an interactive ancestor. Disabled targets block clicks.
    /// Extended bridges outside the original frame remain decorative.
    pub fn hover_at(&self, point: Point) -> Result<Option<&str>, SceneError> {
        let mut target = self.hit_item(point, false)?;
        while let Some(current) = target {
            let info = &self.ui.items[current];
            if info.disabled {
                return Ok(None);
            }
            if info.hoverable {
                return Ok(Some(current));
            }
            target = info.parent.as_deref();
        }
        Ok(None)
    }
    fn hit_item(&self, point: Point, viewports: bool) -> Result<Option<&str>, SceneError> {
        if !point.finite() {
            return Ok(None);
        }
        for id in self.ui.order.iter().rev() {
            let info = &self.ui.items[id];
            let item = &self.items[id];
            let viewport = viewports
                && item
                    .viewport
                    .as_ref()
                    .is_some_and(|clip| clip.contains(point));
            if !(viewport
                || info.hoverable
                || info.color.is_some()
                || info.stroke.is_some()
                || info.text.is_some())
            {
                continue;
            }
            if !item.visible_at(point) {
                continue;
            }
            let local = item
                .transform
                .inverse()
                .expect("validated view transform")
                .apply(point);
            let frame = self.scene.layout.frame(id).expect("validated layout frame");
            if !inside(frame, local) {
                continue;
            }
            let surface = self
                .scene
                .surface(id)
                .ok_or_else(|| SceneError::MissingSurface(id.clone()))?;
            // ponytail: flatten per pick; cache flattened regions if pointer profiling warrants it.
            let t = item.transform;
            let scale_bound = t.xx.hypot(t.yx) + t.xy.hypot(t.yy);
            if !surface.path.contains(local, 0.05 / scale_bound, 65_536)? {
                continue;
            }
            return Ok(Some(id));
        }
        Ok(None)
    }
    pub fn tap_at(&self, point: Point) -> Result<Option<&str>, SceneError> {
        Ok(self
            .hover_at(point)?
            .and_then(|id| self.ui.items[id].tap.as_deref()))
    }
}
fn vector(t: Affine, p: Point) -> Point {
    Point::new(t.xx * p.x + t.xy * p.y, t.yx * p.x + t.yy * p.y)
}
fn inside(f: Frame, p: Point) -> bool {
    p.finite() && p.x >= f.x && p.x < f.right() && p.y >= f.y && p.y < f.bottom()
}
