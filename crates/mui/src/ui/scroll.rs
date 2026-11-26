//! Wheel scrolling, scrollbar drags and their heats.
use super::*;
use mui_scene::Id;

impl Ui {
    /// A validated tree can be walked without an allocated traversal stack.
    /// Only call outside TREES borrows: payload destructors can re-enter Ui.
    pub(super) fn retire_tree(mut root: El) {
        fn recycle(node: &mut El) {
            node.payload_mut().recycle_declaration_storage();
            for child in node.children_mut() {
                recycle(child);
            }
        }
        recycle(&mut root);
    }

    /// Rejected input can itself be too deep for Rust's recursive field drop.
    /// Detach children before dropping each parent, without cloning payloads.
    pub(super) fn discard_tree(mut root: El) {
        if root.children().is_empty() {
            root.payload_mut().recycle_declaration_storage();
            return;
        }
        let mut pending = vec![root];
        while let Some(mut node) = pending.pop() {
            for child in node.children_mut() {
                pending.push(std::mem::replace(child, mui_scene::block(0.0, 0.0)));
            }
            node.payload_mut().recycle_declaration_storage();
        }
    }

    /// Bound the tree before UI passes recurse into user or expanded memo nodes.
    pub(super) fn validate_tree_budget(
        root: &El,
        limits: mui_layout::Limits,
    ) -> Result<(), SceneError> {
        Self::validate_tree_budget_with(root, limits, |node| node)
    }

    pub(super) fn validate_tree_budget_with<'a>(
        root: &'a El,
        limits: mui_layout::Limits,
        mut expand: impl FnMut(&'a El) -> &'a El,
    ) -> Result<(), SceneError> {
        fn visit<'a>(
            node: &'a El,
            depth: usize,
            left: &mut usize,
            limit: usize,
            expand: &mut impl FnMut(&'a El) -> &'a El,
        ) -> Result<(), SceneError> {
            if depth > limit || *left == 0 {
                return Err(mui_layout::Error::BudgetExceeded.into());
            }
            let node = expand(node);
            *left -= 1;
            if node.children().len() > *left {
                return Err(mui_layout::Error::BudgetExceeded.into());
            }
            for child in node.children().iter().rev() {
                visit(child, depth + 1, left, limit, expand)?;
            }
            Ok(())
        }
        // Check depth before descending, so even rejected input has a bounded
        // call stack. Valid frames need no heap-backed traversal buffer.
        let mut left = limits.nodes;
        visit(root, 0, &mut left, limits.depth, &mut expand)
    }

    /// Read declarations after bounded memo expansion. Only descendants of
    /// a declared scroll root receive its content allowance; the viewport and
    /// unrelated nodes retain the ordinary layout cap.
    pub(super) fn virtual_scroll_extent(
        root: &mut El,
        limits: mui_layout::Limits,
        scale: Option<f64>,
    ) -> Result<f64, SceneError> {
        fn extent_of(node: &El, mut extent: f64, scale: Option<f64>) -> Result<f64, SceneError> {
            if let Some(value) = node.payload().extras().virtual_scroll_extent {
                // Logical extents also reach geometry backends. Leave ample
                // headroom for finite scaled coordinates and arithmetic.
                if !node.is_scroll()
                    || !value.is_finite()
                    || value <= 0.0
                    || value > f32::MAX as f64 / 1024.0
                    || value * scale.unwrap_or(1.0) > f32::MAX as f64 / 1024.0
                {
                    return Err(mui_layout::Error::InvalidValue.into());
                }
                extent = extent.max(value);
            }
            for child in node.children() {
                extent = extent_of(child, extent, scale)?;
            }
            Ok(extent)
        }
        Self::validate_tree_budget(root, limits)?;
        let extent = extent_of(root, limits.extent, scale)?;
        if extent > limits.extent {
            // Ordinary frames need no rare-node allocation or cache-key change.
            // When the pass ceiling is raised, explicitly cap every node so an
            // unrelated intrinsic measurement cannot inherit that ceiling.
            fn cap_tree(node: &mut El, cap: f64) {
                node.set_layout_extent_limit(cap);
                let child_cap = node
                    .payload()
                    .extras()
                    .virtual_scroll_extent
                    .map_or(cap, |value| cap.max(value));
                for child in node.children_mut() {
                    cap_tree(child, child_cap);
                }
            }
            cap_tree(root, limits.extent);
        }
        Ok(extent)
    }

    /// How far `id`'s children are scrolled to: the settled offset, which
    /// the drawn one springs toward.
    pub fn scroll(&self, id: impl Into<Id>) -> [f64; 2] {
        self.saw_input();
        let id: Id = id.into();
        let id = id.as_str();
        (self.nodes.get(id).and_then(|n| n.scroll)).map_or([0.0, 0.0], |s| s.map(|s| s.target))
    }

    /// Set both the drawn and target scroll offsets immediately.
    ///
    /// Returns `false` for non-finite offsets. Negative offsets clamp to zero;
    /// the next frame clamps the upper bound against its new content, allowing
    /// callers to scroll into content inserted during this build. This is useful
    /// for virtual lists, whose constructed rows must match the drawn offset.
    pub fn set_scroll(&mut self, id: impl Into<Id>, offset: [f64; 2]) -> bool {
        self.retained_root = None;
        let id: Id = id.into();
        if !offset.iter().all(|v| v.is_finite()) {
            return false;
        }
        node(&mut self.nodes, id.as_str()).scroll = Some(offset.map(|v| Spring::at(v.max(0.0))));
        self.touched.push(id);
        true
    }

    /// The wheel over `id` last frame, if any: `Some` only while the
    /// pointer is inside `id`'s frame and not clipped away, like
    /// [`Response::wheel`].
    ///
    /// Reading it claims it. Call it every build, not only when it scrolled:
    /// while the tree built alongside the call is on screen, a scroll node
    /// enclosing `id` does not scroll under it. A scroll node *inside* `id`
    /// still scrolls first -- the innermost taker wins -- and `id` reads the
    /// wheel regardless.
    ///
    /// ```
    /// # use mui::Ui;
    /// let mut ui = Ui::default();
    /// let zoom = ui.wheel("lane").map_or(1.0, |w| 1.0 - w.y * 0.01);
    /// # assert_eq!(zoom, 1.0);
    /// ```
    pub fn wheel(&mut self, id: impl Into<Id>) -> Option<Vec2> {
        self.saw_input();
        let id: Id = id.into();
        let wheel = self.get(id.as_str()).wheel;
        self.wheel_claims.push(id.as_str().to_owned());
        (wheel != Vec2::ZERO).then_some(wheel)
    }

    /// What the new scene says about the retained state: a capture or focus
    /// whose target is gone ends, scroll offsets clamp to their content,
    /// selections of vanished fields drop, and the wheel lands. Returns
    /// whether an offset moved.
    pub(super) fn settle(&mut self, scene: &ResolvedScene, wheel: Vec2) -> bool {
        self.retire_clicks(scene);
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
        animating | self.land_wheel(scene, wheel)
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
        let ((_, len), (_, view)) = (along(strip.frame), along(node.frame));
        let start = 0.;
        let p = self
            .capture_pose
            .unwrap_or_else(|| strip.local_pose())
            .local(p);
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
            let over = |s: &mui_scene::ResolvedSurface| {
                self.pointer.pos.is_some_and(|p| self.inside_surface(s, p))
            };
            let target = if hovered == Some(&*s.key) || held == Some(&*s.key) {
                1.0
            } else if scene.surface(key).is_some_and(over) {
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
    pub(super) fn land_wheel(&mut self, scene: &ResolvedScene, wheel: Vec2) -> bool {
        // A non-finite delta would land in `self.scrolls` for good: `clamp`
        // returns a NaN receiver unchanged, and every later frame would fail
        // validation on the offset.
        if !(wheel.x.is_finite() && wheel.y.is_finite()) || (wheel.x == 0.0 && wheel.y == 0.0) {
            return false;
        }
        let Some(p) = self.pointer.pos else {
            return false;
        };
        let shift_x = self.pointer.mods.shift && wheel.x == 0.0 && wheel.y != 0.0;
        let mut saw_vertical = false;
        let mut horizontal_fallback: Option<(Id, f64, f64)> = None;
        for s in scene.surfaces().rev() {
            let f = s.frame;
            if s.disabled || !self.inside_surface(s, p) || !self.owns_wheel(scene, s) {
                continue;
            }
            let local_wheel = s.local_pose().delta(wheel);
            // A node that claimed the wheel with `Ui::wheel` reads raw
            // `self.wheel` (including Shift-fine knob adjustment) in the next
            // tree; nothing it sits in scrolls under it.
            let claimed = self.wheel_claims.iter().any(|c| c == s.key.as_str());
            if claimed {
                return true;
            }
            // `content` is the frame size for everything but a scroll node,
            // so an overflow here *is* the "is this scrollable" test.
            let max = [
                (s.content.width - f.size.width).max(0.0),
                (s.content.height - f.size.height).max(0.0),
            ];
            let effective = if shift_x && local_wheel.x == 0.0 && local_wheel.y != 0.0 {
                Vec2::new(local_wheel.y, 0.0)
            } else {
                local_wheel
            };
            if max[0] > 0.0 || max[1] > 0.0 {
                if max[1] > 0.0 {
                    saw_vertical = true;
                } else if !shift_x
                    && local_wheel.x == 0.0
                    && local_wheel.y != 0.0
                    && horizontal_fallback.is_none()
                {
                    horizontal_fallback = Some((s.key.clone(), max[0], local_wheel.y));
                }
                // The handoff reads the target, not the drawn offset, so a flick
                // still in flight does not pass the next notch to the parent.
                let at = node(&mut self.nodes, &s.key)
                    .scroll
                    .get_or_insert([scroll_spring(); 2]);
                let next = [
                    (at[0].target + effective.x).clamp(0.0, max[0]),
                    (at[1].target + effective.y).clamp(0.0, max[1]),
                ];
                if next != at.map(|s| s.target) {
                    at[0].to(next[0]);
                    at[1].to(next[1]);
                    return true;
                }
            }
            // A node with `.captures_wheel()` keeps any remaining wheel delta
            // from scrolling ancestors underneath it (even when placed directly
            // on a `.scroll()` node whose own offset is already at an edge).
            if s.captures_wheel {
                return true;
            }
            // An exhausted or perpendicular nested scroller yields to its parent.
        }
        // Plain vertical mouse wheel over a horizontal-only scroller with no
        // vertical scroller in its owned ancestor chain scrolls horizontally.
        if !shift_x
            && !saw_vertical
            && let Some((key, max_x, delta_y)) = horizontal_fallback
        {
            let at = node(&mut self.nodes, &key)
                .scroll
                .get_or_insert([scroll_spring(); 2]);
            let next_x = (at[0].target + delta_y).clamp(0.0, max_x);
            if next_x != at[0].target {
                at[0].to(next_x);
                return true;
            }
        }
        false
    }
}

/// A scroll offset's glide toward where the wheel put it: short, and
/// critically damped so it never overshoots the end of the content.
pub(super) fn scroll_spring() -> Spring {
    Spring::new(0.12, 1.0)
}

#[cfg(test)]
mod wheel_routing_tests {
    use super::*;
    use mui_input::{Input, Mods, PointerInput};
    use mui_scene::prelude::*;

    fn wheel_input(x: f64, y: f64, dx: f64, dy: f64, shift: bool) -> Input {
        Input {
            pointer: PointerInput {
                pos: Some(Point::new(x, y)),
                mods: Mods {
                    shift,
                    ..Mods::default()
                },
                ..PointerInput::default()
            },
            wheel: Vec2::new(dx, dy),
            ..Input::default()
        }
    }

    #[test]
    fn scroll_with_captures_wheel_scrolls_itself_and_traps_exhausted_wheel() {
        let mut ui = Ui::default();
        let build = || {
            let inner = col![block(80.0, 200.0)]
                .w(80.0)
                .h(60.0)
                .scroll()
                .captures_wheel()
                .id("inner");
            col![inner, block(100.0, 300.0)]
                .w(100.0)
                .h(100.0)
                .scroll()
                .id("outer")
        };
        ui.frame(build(), None, Input::default(), 0.016).unwrap();
        ui.frame(
            build(),
            None,
            wheel_input(20.0, 20.0, 0.0, 40.0, false),
            0.016,
        )
        .unwrap();
        assert_eq!(ui.scroll("inner"), [0.0, 40.0]);
        assert_eq!(ui.scroll("outer"), [0.0, 0.0]);

        // Even once inner is exhausted at the bottom, captures_wheel keeps outer from scrolling.
        ui.frame(
            build(),
            None,
            wheel_input(20.0, 20.0, 0.0, 500.0, false),
            0.016,
        )
        .unwrap();
        assert_eq!(ui.scroll("inner"), [0.0, 140.0]);
        assert_eq!(ui.scroll("outer"), [0.0, 0.0]);
        ui.frame(
            build(),
            None,
            wheel_input(20.0, 20.0, 0.0, 50.0, false),
            0.016,
        )
        .unwrap();
        assert_eq!(ui.scroll("outer"), [0.0, 0.0]);
    }

    #[test]
    fn disabled_scroller_does_not_scroll_on_wheel() {
        let mut ui = Ui::default();
        let build = || {
            col![block(100.0, 400.0)]
                .w(100.0)
                .h(100.0)
                .scroll()
                .id("bg")
                .disabled()
        };
        ui.frame(build(), None, Input::default(), 0.016).unwrap();
        ui.frame(
            build(),
            None,
            wheel_input(20.0, 20.0, 0.0, 60.0, false),
            0.016,
        )
        .unwrap();
        assert_eq!(ui.scroll("bg"), [0.0, 0.0]);
    }

    #[test]
    fn shift_wheel_scrolls_horizontal_without_breaking_claimed_knob_wheel() {
        let mut ui = Ui::default();
        let mut knob_saw = None;
        let step = |ui: &mut Ui, knob_saw: &mut Option<(Vec2, bool)>, input: Input| {
            let knob = block(30.0, 30.0).id("knob");
            if let Some(w) = ui.wheel("knob") {
                *knob_saw = Some((w, ui.pointer().mods.shift));
            }
            let horiz = row![knob, block(300.0, 40.0)]
                .w(100.0)
                .h(50.0)
                .scroll()
                .id("horiz");
            let root = col![horiz, block(100.0, 300.0)]
                .w(100.0)
                .h(100.0)
                .scroll()
                .id("outer");
            ui.frame(root, None, input, 0.016).unwrap();
        };
        step(&mut ui, &mut knob_saw, Input::default());

        // Shift+Wheel over the horizontal scroller (away from the knob) scrolls horizontally.
        step(
            &mut ui,
            &mut knob_saw,
            wheel_input(70.0, 20.0, 0.0, 45.0, true),
        );
        assert_eq!(ui.scroll("horiz"), [45.0, 0.0]);
        assert_eq!(ui.scroll("outer"), [0.0, 0.0]);

        // Shift+Wheel directly over the knob delivers raw vertical delta + shift to ui.wheel("knob").
        step(
            &mut ui,
            &mut knob_saw,
            wheel_input(15.0, 15.0, 0.0, -24.0, true),
        );
        step(&mut ui, &mut knob_saw, Input::default());
        assert_eq!(knob_saw, Some((Vec2::new(0.0, -24.0), true)));
        assert_eq!(ui.scroll("horiz"), [45.0, 0.0]);
    }

    #[test]
    fn plain_wheel_scrolls_standalone_horizontal_scroller_but_yields_to_vertical_parent() {
        // Standalone horizontal-only scroller: plain vertical wheel scrolls horizontally.
        let mut ui = Ui::default();
        let build_standalone = || {
            row![block(400.0, 40.0)]
                .w(100.0)
                .h(80.0)
                .scroll()
                .id("strip")
        };
        ui.frame(build_standalone(), None, Input::default(), 0.016)
            .unwrap();
        ui.frame(
            build_standalone(),
            None,
            wheel_input(20.0, 20.0, 0.0, 50.0, false),
            0.016,
        )
        .unwrap();
        assert_eq!(ui.scroll("strip"), [50.0, 0.0]);

        // Nested inside a vertical scroller: plain vertical wheel scrolls the vertical parent.
        let mut ui = Ui::default();
        let build_nested = || {
            let strip = row![block(400.0, 40.0)]
                .w(100.0)
                .h(50.0)
                .scroll()
                .id("strip");
            col![strip, block(100.0, 300.0)]
                .w(100.0)
                .h(100.0)
                .scroll()
                .id("outer")
        };
        ui.frame(build_nested(), None, Input::default(), 0.016)
            .unwrap();
        ui.frame(
            build_nested(),
            None,
            wheel_input(20.0, 20.0, 0.0, 50.0, false),
            0.016,
        )
        .unwrap();
        assert_eq!(ui.scroll("strip"), [0.0, 0.0]);
        assert_eq!(ui.scroll("outer"), [0.0, 50.0]);
    }
}
