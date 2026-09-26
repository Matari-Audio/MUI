//! Wheel scrolling, scrollbar drags and their heats.
use mui_scene::Id;
use super::*;

impl Ui {
    /// How far `id`'s children are scrolled to: the settled offset, which
    /// the drawn one springs toward.
    pub fn scroll(&self, id: impl Into<Id>) -> [f64; 2] {
        let id: Id = id.into();
        let id = id.as_str();
        (self.nodes.get(id).and_then(|n| n.scroll)).map_or([0.0, 0.0], |s| s.map(|s| s.target))
    }

    /// What the new scene says about the retained state: a capture or focus
    /// whose target is gone ends, scroll offsets clamp to their content,
    /// selections of vanished fields drop, and the wheel lands. Returns
    /// whether an offset moved.
    pub(super) fn settle(&mut self, scene: &ResolvedScene, wheel: Vec2) -> bool {
        let live = |id: &str| scene.surface(id).is_some_and(|s| !s.disabled);
        if self.interaction.held().is_some_and(|id| !live(id)) {
            self.cancel();
            self.edits
                .extend(self.cancelled.take().map(|id| (id, Edit::End)));
        }
        if self.focus.as_deref().is_some_and(|id| !live(id)) {
            self.focus = None;
            self.preedit = None;
        }
        let mut animating = false;
        // One whose surface is gone is dropped with it at the commit.
        for (id, n) in &mut self.nodes {
            let (Some(at), Some(surface)) = (&mut n.scroll, scene.surface(id)) else {
                continue;
            };
            let max = [
                surface.content.width - surface.frame.size.width,
                surface.content.height - surface.frame.size.height,
            ];
            for (s, max) in at.iter_mut().zip(max) {
                let next = s.target.clamp(0.0, max.max(0.0));
                animating |= s.target != next;
                s.to(next);
            }
        }
        animating | self.wheel(scene, wheel)
    }

    /// A held scrollbar slides its node: the thumb follows the pointer from
    /// where it was grabbed, and a press on the track beside the thumb
    /// centres the thumb there first. It lands on this frame's tree, like a
    /// widget reading its drag. Returns whether an offset moved.
    pub(super) fn drag_bar(&mut self) -> bool {
        let (Some(held), Some(p)) = (self.interaction.held(), self.pointer.pos) else {
            return false;
        };
        let Some((key, vertical)) = bar::bar_of(held) else {
            return false;
        };
        let Some(scene) = self.scene.as_ref() else {
            return false;
        };
        let (Some(node), Some(strip)) = (scene.surface(key), scene.surface(held)) else {
            return false;
        };
        let along = |f: mui_layout::Frame| {
            if vertical {
                (f.y, f.size.height)
            } else {
                (f.x, f.size.width)
            }
        };
        let ((start, len), (_, view)) = (along(strip.frame), along(node.frame));
        let total = if vertical {
            node.content.height
        } else {
            node.content.width
        };
        let (pos, a) = if vertical { (p.y, 1) } else { (p.x, 0) };
        let pressed = self.interaction.pressed() == Some(held);
        let at = super::node(&mut self.nodes, key)
            .scroll
            .get_or_insert([scroll_spring(); 2]);
        let Some((thumb, size)) = bar::thumb(start, len, view, total, at[a].target) else {
            return false;
        };
        if pressed {
            self.bar_grab = if (thumb..thumb + size).contains(&pos) {
                pos - thumb
            } else {
                size / 2.0
            };
        }
        let next = bar::thumb_offset(start, len, view, total, pos - self.bar_grab);
        // The thumb is under the hand: the offset follows it, no glide.
        let moved = next != at[a].target || next != at[a].value;
        at[a] = at[a].seeded(next);
        moved
    }

    /// Per scroll node that showed a bar last frame, how hot its bar is:
    /// resting, the pointer over the list, or the bar itself under the
    /// pointer or held. Each rides a runtime-owned tween, so it eases.
    pub(super) fn bar_heats(&mut self) -> rustc_hash::FxHashMap<Id, f64> {
        let mut targets = rustc_hash::FxHashMap::default();
        let Some(scene) = self.scene.as_ref() else {
            return targets;
        };
        let (hovered, held) = (self.interaction.hovered(), self.interaction.held());
        for s in scene.surfaces() {
            let Some((key, _)) = bar::bar_of(&s.key) else {
                continue;
            };
            let over =
                |f: mui_layout::Frame| self.pointer.pos.is_some_and(|p| f.contains(p.x, p.y));
            let target = if hovered == Some(&*s.key) || held == Some(&*s.key) {
                1.0
            } else if scene.surface(key).is_some_and(|n| over(n.frame)) {
                0.35
            } else {
                0.0
            };
            let t: &mut f64 = targets.entry(Id::runtime(key)).or_default();
            *t = t.max(target);
        }
        // The heat's tween is `/bar/<key>`, a runtime id built in place.
        let bar = Id::runtime("/bar");
        for (key, target) in &mut targets {
            *target = self.tween(bar.field(key), *target);
        }
        targets
    }

    /// Send the wheel to the innermost scrollable surface under the pointer.
    /// It lands on the next frame's tree, the same frame late a release is.
    pub(super) fn wheel(&mut self, scene: &ResolvedScene, wheel: Vec2) -> bool {
        // A non-finite delta would land in `self.scrolls` for good: `clamp`
        // returns a NaN receiver unchanged, and every later frame would fail
        // validation on the offset.
        if !(wheel.x.is_finite() && wheel.y.is_finite()) || (wheel.x == 0.0 && wheel.y == 0.0) {
            return false;
        }
        let Some(p) = self.pointer.pos else {
            return false;
        };
        for s in scene.surfaces().rev() {
            let f = s.frame;
            if p.x < f.x || p.x > f.right() || p.y < f.y || p.y > f.bottom() {
                continue;
            }
            if s.clip.is_some_and(|clip| {
                p.x < clip.x0 || p.x > clip.x1 || p.y < clip.y0 || p.y > clip.y1
            }) {
                continue;
            }
            // A node that keeps the wheel reads it from `Response::wheel`;
            // nothing it sits in scrolls under it.
            if s.captures_wheel && !s.disabled {
                return false;
            }
            // `content` is the frame size for everything but a scroll node,
            // so an overflow here *is* the "is this scrollable" test.
            let max = [
                (s.content.width - f.size.width).max(0.0),
                (s.content.height - f.size.height).max(0.0),
            ];
            if max[0] <= 0.0 && max[1] <= 0.0 {
                continue;
            }
            // The handoff reads the target, not the drawn offset, so a flick
            // still in flight does not pass the next notch to the parent.
            let at = node(&mut self.nodes, &s.key)
                .scroll
                .get_or_insert([scroll_spring(); 2]);
            let next = [
                (at[0].target + wheel.x).clamp(0.0, max[0]),
                (at[1].target + wheel.y).clamp(0.0, max[1]),
            ];
            if next != at.map(|s| s.target) {
                at[0].to(next[0]);
                at[1].to(next[1]);
                return true;
            }
            // An exhausted or perpendicular nested scroller yields to its parent.
        }
        false
    }
}

/// A scroll offset's glide toward where the wheel put it: short, and
/// critically damped so it never overshoots the end of the content.
pub(super) fn scroll_spring() -> Spring {
    Spring::new(0.12, 1.0)
}
