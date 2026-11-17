//! One frame: commands and input in, state reconciled, the tree resolved,
//! the new scene committed.
use super::*;

impl Ui {
    /// Advance gestures and springs, style the tree by state, resolve it.
    ///
    /// The phases run in this order, each reading what the one before left:
    /// commands and input in, capture reconciled against last frame's hit
    /// map, focus and keys, the tip, the state and motion sweep over the new
    /// tree, the resolve, then what the new scene says about the old state,
    /// and finally the commit.
    pub fn frame(
        &mut self,
        root: El,
        offered: Option<Size>,
        input: impl Into<Input>,
        dt: f64,
    ) -> Result<Frame<'_>, SceneError> {
        if !(dt.is_finite() && dt >= 0.0 && (self.time + dt).is_finite()) {
            Self::discard_tree(root);
            return Err(SceneError::InvalidFrameDelta);
        }
        let input = input.into();
        let memo = root
            .payload()
            .extras()
            .memo
            .as_ref()
            .map(|m| (m.id, m.reused));
        let previous_root = self.retained_root.take();
        self.retained_frame = false;
        let quiet = input.pointer == self.pointer
            && input.pointer.buttons.is_empty()
            && input.wheel == Vec2::ZERO
            && input.keys.is_empty()
            && input.text.is_empty()
            && input.ime.is_empty()
            && input.trail.is_empty()
            && input.clipboard.is_none()
            && self.focus.is_none()
            && self.interaction.held().is_none()
            && self.interaction.hovered().is_none()
            && self.tagged.is_none()
            && self.actions.is_empty()
            && self.edits.is_empty()
            && self.delivered.is_empty()
            && self.cancelled.is_none()
            && self.ime_caret.is_none();
        if quiet
            && memo.is_some_and(|(id, reused)| {
                reused
                    && previous_root == Some((id, offered))
                    && self.declared_as_delivered(&root, id)
            })
            && let Some(scene) = self.scene.as_ref().map(Arc::clone)
        {
            let previous_blink = self.blink();
            self.time += dt;
            self.advance_caller_wake();
            self.play_until = self.time;
            self.issued = 0;
            // This root owns every retained subtree. No new build ran, so
            // consume liveness without discarding nested retained declarations.
            self.kept.values_mut().for_each(|k| k.live = false);
            self.retained_root = previous_root;
            self.retained_frame = true;
            // The issued placeholder is not the retained declaration tree.
            // Return its rare-field box even on the no-resolve fast path.
            Self::discard_tree(root);
            return Ok(self.commit(scene, None, None, previous_blink, false, true));
        }
        if let Err(error) = Self::validate_tree_budget(&root, self.layout_limits)
            .and_then(|()| self.validate_expanded_tree_budget(&root))
        {
            Self::discard_tree(root);
            return Err(error);
        }
        self.resolved = false;
        self.bracket_commands();
        let Input {
            pointer,
            wheel,
            keys,
            text,
            clipboard,
            ime,
            // The view's, read in its build; the `Ui` hit-tests the newest.
            trail: _,
        } = input;
        let previous_pointer = std::mem::replace(&mut self.pointer, pointer);
        let was = previous_pointer.buttons;
        let was_buttons = was;
        // A non-finite delta reaches no widget, as it reaches no scroller.
        self.wheel = if wheel.x.is_finite() && wheel.y.is_finite() {
            wheel
        } else {
            Vec2::ZERO
        };
        // A paste key with no text handed in reads the host's clipboard, if
        // it gave one: nothing here reads it speculatively.
        let paste_key = keys
            .iter()
            .any(|k| matches!(k.key, Key::Char('v' | 'V')) && (k.mods.ctrl || k.mods.cmd));
        self.pasted = match (clipboard, self.board.as_mut()) {
            (None, Some(b)) if paste_key => b.get(),
            (c, _) => c,
        };
        let previous_blink = self.blink();
        // Plays were read while building this tree, before advancing the clock.
        // Even a frame crossing the last key owes the tree drawing its endpoint.
        let playing = self.time < self.play_until;
        self.time += dt;
        self.advance_caller_wake();
        if self.wheel != Vec2::ZERO {
            self.wheel_turned = Some(self.time);
        }
        // Consume this build's request. An unread/removed play must not keep an
        // otherwise idle UI awake until its old deadline.
        self.play_until = self.time;
        // A release is read by the *next* tree, so that frame must come even
        // when nothing is moving.
        // Inline ids: no heap copy for a key under 47 bytes.
        let before = [
            self.interaction.hovered().map(Id::runtime),
            self.interaction.held().map(Id::runtime),
            self.tagged.as_ref().map(|t| Id::runtime(&t.0)),
        ];
        // The kept memo subtrees go back in first: everything below reads
        // the whole tree.
        let mut root = root;
        let mut memos = self.splice(&mut root);
        if let Err(error) = Self::validate_tree_budget(&root, self.layout_limits) {
            Self::discard_tree(root);
            return Err(error);
        }
        let captured = self.reconcile();
        // Reduced motion does not poll a stationary gesture. Capture edges and
        // moved drag samples still owe the next tree its up-to-date response.
        let capture_wake = if self.reduced_motion() {
            before[1].as_deref() != self.interaction.held()
                || (captured
                    && (previous_pointer.pos != self.pointer.pos
                        || previous_pointer.buttons != self.pointer.buttons
                        || !keys.is_empty()
                        || !text.is_empty()
                        || !ime.is_empty()
                        || self.wheel != Vec2::ZERO
                        || self.pasted.is_some()))
        } else {
            captured
        };
        let mut animating = capture_wake | playing;
        // The tree just built read last frame's hover: a new target is owed
        // the tree that knows it, even with no spring to carry it there.
        animating |= self.interaction.hovered() != before[0].as_deref();
        animating |= self.drag_bar();
        self.follow_identities(&root);
        animating |= self.hover_springs(&root, dt);
        self.intake_focus(was, keys, text, ime);
        // Bar heats tween first: one that starts moving now stirs its node.
        let heats = self.bar_heats();
        self.retain_auto(&mut root, &before);
        let hovered = self.interaction.hovered().map(Id::runtime);
        let (tip, tip_pending) = self.tip_due(hovered.as_deref(), dt);
        animating |= tip_pending;
        if tip.is_some() && !root.is_container() {
            // The tip is about to wrap this leaf: everything moves down one.
            memos.iter_mut().for_each(|(p, _)| p.insert(0, 0));
        }
        let mut root = Self::wrap_tip(root, tip.as_ref());
        if let Err(error) = Self::validate_tree_budget(&root, self.layout_limits) {
            Self::discard_tree(root);
            return Err(error);
        }
        let (moving, shaped) = self.sweep(&mut root, dt);
        animating |= moving;
        let (mut scene, glided, root) = self.resolve(root, offered, dt, heats)?;
        let root = self.capture(root, &memos);
        self.keep_previous(root);
        animating |= glided;
        animating |= self.after_motion(&mut scene, shaped, dt);
        self.wheel_capture = self.wheel_capture_for(&scene);
        animating |= self.settle(&scene, wheel);
        // Keys and typed text are read by the next build, like a release:
        // a host that sleeps between events owes it that frame.
        animating |= !self.keys.is_empty() || !self.typed.is_empty();
        self.wheel_claims.clear();
        self.heat(&scene, &before, self.pointer.buttons != was_buttons);
        if !animating
            && self.focus.is_none()
            && self.interaction.held().is_none()
            && self.interaction.hovered().is_none()
            && self.tagged.is_none()
        {
            self.retained_root = memo.map(|(id, _)| (id, offered));
        }
        Ok(self.commit(
            Arc::new(scene),
            hovered.as_deref(),
            tip,
            previous_blink,
            animating,
            false,
        ))
    }

    /// The caller has now built its tree and consumed these commands. Drain
    /// before resolution so a layout error cannot replay an activation.
    /// Gesture edges stay queued in `edits` until delivered by a frame or
    /// `close`.
    fn bracket_commands(&mut self) {
        let active = self.interaction.held().map(str::to_owned);
        let mut atomic = BTreeSet::new();
        // So are the keys the focused control just read: an activation or a
        // step is one edit, bracketed like the semantic action it mirrors.
        let keyed = self
            .focus
            .as_deref()
            .filter(|id| keyed_edit(self.scene.as_deref(), id, &self.keys))
            .map(str::to_owned);
        let ids = std::mem::take(&mut self.actions)
            .into_iter()
            .map(|action| action.id().to_owned())
            .chain(keyed);
        for id in ids {
            if active.as_deref() != Some(id.as_str()) && atomic.insert(id.clone()) {
                self.edits.push((id.clone(), Edit::Begin));
                self.edits.push((id, Edit::End));
            }
        }
    }

    /// Run this frame's pointer against last frame's hit map: drop a capture
    /// or focus whose target was switched off, move the capture, queue its
    /// edges and latch the tagged shape under a new press. Returns whether a
    /// capture was held coming in, which owes the next tree its release.
    fn reconcile(&mut self) -> bool {
        // Switched off mid-gesture: the hit map stopped reporting it when it
        // resolved disabled, and a drag must not outlive its target. The
        // cancel still owes the host an `Edit::End`.
        let off = |scene: Option<&ResolvedScene>, k: &str| {
            scene.and_then(|s| s.surface(k)).is_some_and(|s| s.disabled)
        };
        if self
            .interaction
            .held()
            .is_some_and(|k| off(self.scene.as_deref(), k))
        {
            self.cancel();
        }
        // The same for a focus it held when it was switched off: Tab already
        // steps over it, and a key stream into a dead node is worse.
        if self
            .focus
            .as_deref()
            .is_some_and(|k| off(self.scene.as_deref(), k))
        {
            self.focus = None;
        }
        let prev_held = self.interaction.held().map(str::to_owned);
        let last_scene = self.scene.as_deref();
        self.interaction
            .update_with(&self.hit, self.pointer, |key, tag, p| {
                if tag.is_some() {
                    return None;
                }
                last_scene.and_then(|s| s.external_contains(key, p))
            });
        // A payload outlives its gesture by exactly the one frame the drop is
        // reported in -- the frame the target's tree reads it from.
        if self.interaction.held().is_none() && self.interaction.dropped().is_none() {
            self.drag = None;
        }
        let held = self.interaction.held().map(str::to_owned);
        if prev_held != held {
            self.capture_pose = held.as_deref().and_then(|key| {
                self.scene
                    .as_ref()?
                    .surface(key)
                    .map(mui_scene::ResolvedSurface::local_pose)
            });
        }
        // A gesture is exactly the span a target is captured for, so the two
        // edges are the two ends of that capture -- plus the one a `cancel`
        // stole before this frame could see it.
        // Extend rather than assign: an edge computed for a frame that then
        // failed to resolve stays queued for the next one that does.
        self.edits
            .extend(self.cancelled.take().map(|k| (k, Edit::End)));
        if prev_held != held {
            self.edits.extend(prev_held.clone().map(|k| (k, Edit::End)));
            self.edits.extend(held.clone().map(|k| (k, Edit::Begin)));
        }
        // Last frame's hit map and this frame's pointer, exactly as the
        // interaction above: a new capture latches the shape under the press.
        if held.is_none() || prev_held != held {
            self.tagged = self.pointer.pos.and_then(|p| {
                let (id, tag) = self.hit.at_tagged_with(p, |key, tag, p| {
                    if tag.is_some() {
                        return None;
                    }
                    self.scene
                        .as_ref()
                        .and_then(|s| s.external_contains(key, p))
                })?;
                Some((id.to_owned(), tag?.to_owned()))
            });
        }
        prev_held.is_some()
    }

    /// Carry state kept under a name to the node's new name when a node with
    /// the same identity comes back renamed or moved. Runs after this frame's
    /// pointer has been matched against last frame's names, so a drag that
    /// caused the reorder keeps its capture under the new name.
    fn follow_identities(&mut self, root: &El) {
        // Child indices down to here: a path is spelled out only for a node
        // that has an identity, and most trees have none.
        fn visit(n: &El, at: &mut Vec<usize>, out: &mut HashMap<u64, (Option<String>, String)>) {
            if let Some(id) = n.payload().extras().identity {
                let mut path = String::new();
                at.iter().for_each(|&j| push_index(&mut path, j));
                out.insert(id, (n.key().map(str::to_owned), path));
            }
            for (j, c) in n.children().iter().enumerate() {
                at.push(j);
                visit(c, at, out);
                at.pop();
            }
        }
        let mut now = HashMap::new();
        visit(root, &mut Vec::new(), &mut now);
        let mut moves: Vec<(String, String)> = Vec::new();
        for (id, (key, path)) in &now {
            let Some((was_key, was_path)) = self.identities.get(id) else {
                continue;
            };
            if let (Some(a), Some(b)) = (was_key, key)
                && a != b
            {
                moves.push((a.clone(), b.clone()));
            }
            if was_path != path {
                moves.push((was_path.clone(), path.clone()));
            }
        }
        self.identities = now;
        if moves.is_empty() {
            return;
        }
        // The deepest match wins: a renamed slot inside a renamed rack.
        moves.sort_by_key(|m| std::cmp::Reverse(m.0.len()));
        let to = |k: &str| -> Option<String> {
            moves.iter().find_map(|(a, b)| {
                if k == a {
                    return Some(b.clone());
                }
                // A name prefixes the names composed under it, a path the
                // paths below it; "osc/30" is not under "osc/3".
                let rest = k.strip_prefix(a.as_str())?;
                rest.starts_with('/').then(|| format!("{b}{rest}"))
            })
        };
        // A moved entry wins over one already under its new name.
        // All leave before any lands, so a swap is a swap.
        let renamed: Vec<_> = (self.nodes.keys())
            .filter_map(|k| Some((k.clone(), to(k)?)))
            .collect();
        let moved: Vec<_> = (renamed.into_iter())
            .filter_map(|(was, now)| Some((mui_scene::Id::runtime(&now), self.nodes.remove(&was)?)))
            .collect();
        self.nodes.extend(moved);
        let saved = self.focus_traps.iter_mut().map(|(_, f)| f);
        for k in [&mut self.focus, &mut self.double].into_iter().chain(saved) {
            if let Some(new) = k.as_deref().and_then(to) {
                *k = Some(new);
            }
        }
        for k in [
            self.hover.as_mut().map(|h| &mut h.0),
            self.tagged.as_mut().map(|t| &mut t.0),
            self.drag.as_mut().map(|d| &mut d.0),
            self.last_click.as_mut().map(|p| &mut p.id),
            self.pending_click.as_mut().map(|p| &mut p.id),
        ]
        .into_iter()
        .flatten()
        {
            if let Some(new) = to(k) {
                *k = new;
            }
        }
        self.interaction.rename(&to);
    }

    /// Resolve the styled tree, and rebuild the hit map when the hit
    /// geometry changed.
    fn resolve(
        &mut self,
        root: El,
        offered: Option<Size>,
        dt: f64,
        heats: rustc_hash::FxHashMap<Id, f64>,
    ) -> Result<(ResolvedScene, bool, El), SceneError> {
        let mut spec = SceneSpec::new(root).theme(self.theme.clone());
        spec.limits = self.layout_limits;
        spec.limits.extent = Self::virtual_scroll_extent(&mut spec.root, spec.limits, self.scale)?;
        spec.offered = offered;
        spec.font = self.font.clone();
        spec.fallback_fonts = self.fallback_fonts.clone();
        spec.device_scale = self.scale;
        spec.weld_backend = if self.gpu_welding_available {
            self.weld_backend
        } else {
            mui_scene::WeldBackend::Reference
        };
        spec.scroll_bars = Some(heats);
        let mut glided = false;
        let reduced = self.reduced_motion();
        let nodes = &mut self.nodes;
        let scene = self.resolver.resolve_after(
            &spec,
            &mut |key, e, target| {
                let spring = e.extras().layout_transition.unwrap_or(Spring::DEFAULT);
                let t = [target.x, target.y, target.size.width, target.size.height];
                let (seen, s) = node(nodes, key)
                    .glide
                    .get_or_insert_with(|| (false, enter(spring, t, e.extras().appear)));
                *seen = true;
                for (s, t) in s.iter_mut().zip(t) {
                    s.to(t);
                    glided |= step_spring(s, dt, reduced);
                }
                mui_scene::Frame {
                    x: s[0].value,
                    y: s[1].value,
                    size: Size::new(s[2].value, s[3].value),
                }
            },
            self.scene.as_deref(),
        )?;
        let found = self.resolver.take_found_fallbacks();
        if !found.is_empty() {
            self.invalidate_retained();
            self.fallback_fonts.extend(found);
            self.runs.set_fallbacks(self.fallback_fonts.clone());
            self.repaint_in(0.0);
        }
        if self
            .scene
            .as_ref()
            .is_none_or(|previous| !same_hit_geometry(previous, &scene))
        {
            // Named nodes are the gesture targets, in z-order, and so is an
            // unnamed one that declared a hover or press look, by its tree
            // path: it routes exactly as it would with an id. Every other
            // unnamed node, the root included, is decoration the pointer
            // passes through. A target clipped away does not respond.
            //
            // ponytail: a capture an unnamed node takes on the frame a tip
            // wraps or unwraps the root keeps its old path; it releases
            // normally but reports no click. Key positional targets by
            // something stabler than the path if that ever shows.
            // The last map's conversions stay: a moved outline is the same
            // `Arc`, so only new shapes convert.
            let mut hit = std::mem::take(&mut self.hit);
            hit.clear();
            for s in scene
                .surfaces()
                .filter(|s| (named(&s.key) || s.pointer_states) && !s.disabled)
            {
                let key = || s.key.clone();
                if s.hits.is_empty() {
                    hit.push_placed(key(), None, &s.path, s.offset, s.clip, s.clip_paths())?;
                }
                // A canvas that named its draws is hit by those shapes instead
                // of by its frame, so a ring responds in the ring, not its hole.
                for (tag, path) in &s.hits {
                    let tag = Some(tag.clone());
                    hit.push_placed(key(), tag, path, s.offset, s.clip, s.clip_paths())?;
                }
            }
            self.hit = hit;
        }
        // Equivalent contours may arrive in a fresh clip-list Arc even when
        // the gesture map is reused. Register only this frame's list keys.
        self.hit
            .prepare_clips(scene.surfaces().filter_map(|s| s.clip_paths()))?;
        Ok((scene, glided, spec.root))
    }

    fn wheel_capture_for(&self, scene: &ResolvedScene) -> Option<Id> {
        if self.wheel == Vec2::ZERO {
            return None;
        }
        let pointer = self.pointer.pos?;
        scene
            .surfaces()
            .rev()
            .find(|s| {
                (s.captures_wheel || self.popup_wheel_scopes.contains(&s.key))
                    && !s.disabled
                    && self.inside_surface(s, pointer)
            })
            .map(|s| s.key.clone())
    }

    pub(super) fn owns_wheel(
        &self,
        scene: &ResolvedScene,
        surface: &mui_scene::ResolvedSurface,
    ) -> bool {
        let Some(owner) = &self.wheel_capture else {
            return true;
        };
        let mut current = Some(surface);
        while let Some(s) = current {
            if s.key == *owner {
                return true;
            }
            current = s.parent.as_ref().and_then(|id| scene.surface(id.as_str()));
        }
        false
    }

    /// Logical frame containment in the node's own axes, plus the same
    /// cached exact ancestor clips used by gesture hit testing.
    pub(super) fn inside_surface(&self, s: &mui_scene::ResolvedSurface, p: Point) -> bool {
        let local = s.local(p);
        local.x >= 0.
            && local.x <= s.frame.size.width
            && local.y >= 0.
            && local.y <= s.frame.size.height
            && self.hit.inside_clips(p, s.clip, s.clip_paths())
    }

    /// Keep the scene, hand out the edges, and say what the host should do
    /// next: the cursor, the caret area, where the tip landed, and whether
    /// to schedule another frame.
    fn commit(
        &mut self,
        scene: Arc<ResolvedScene>,
        hovered: Option<&str>,
        tip: Option<(Arc<str>, String)>,
        previous_blink: bool,
        mut animating: bool,
        retained_root: bool,
    ) -> Frame<'_> {
        let held = self.interaction.held();
        let cursor = held
            .or(hovered)
            .and_then(|k| scene.surface(k).and_then(|s| s.cursor))
            .unwrap_or(Cursor::Arrow);
        let cursor = match cursor {
            Cursor::Grab if held.is_some() => Cursor::Grabbing,
            c => c,
        };

        // The caret area a field asked for, moved into the scene's space.
        let ime = self.ime_caret.take().and_then(|(id, at, h)| {
            let s = scene.surface(&id)?;
            let (at, h) = s
                .text_geometry
                .as_ref()
                .map_or((at, h), |g| (g.caret, g.line_height));
            let f = s.frame;
            let rect = s.transform.transform_rect_bbox(mui_geometry::Rect::new(
                f.x + at.x,
                f.y + at.y,
                f.x + at.x + 1.,
                f.y + at.y + h,
            ));
            Some((
                Point::new(rect.x0, rect.y0),
                Size::new(rect.width(), rect.height()),
            ))
        });
        // Where the pin actually put it, so a host placing its own tooltip
        // window agrees with the one in the scene.
        let tip = tip.and_then(|(t, _)| {
            let f = scene.surface(TIP_KEY)?.frame;
            Some((t.to_string(), Point::new(f.x, f.y)))
        });
        // A reused memo's nodes were not visited, and are still there.
        self.nodes.retain(|k, n| {
            n.prune(
                || retained_root || scene.memos_at(k).any(|(_, reused)| reused),
                scene.surface(k).is_some(),
            )
        });
        self.delivered = std::mem::take(&mut self.edits);
        self.popup_wheel_scopes
            .retain(|id| scene.surface(id.as_str()).is_some());
        if let Some(old) = self.scene.replace(scene)
            && let Ok(old) = Arc::try_unwrap(old)
        {
            // A host retaining the previous frame still owns its buffers.
            // Recycling is an allocation optimization, never a scene clone.
            self.resolver.recycle(old);
        }
        // A field that gets focus back builds its IME caret next frame:
        // ask for that frame now rather than when input next arrives.
        animating |= self.settle_focus_trap();
        let repaint_after = self.repaint_after();
        // Widgets read the clock while constructing the tree, before this frame
        // advances it. One catch-up frame makes the blink edge visible now,
        // rather than one whole half-period late. It then sleeps again.
        animating |= previous_blink != self.blink()
            && self
                .focus
                .as_deref()
                .and_then(|key| self.scene.as_deref()?.surface(key))
                .is_some_and(|surface| {
                    !surface.disabled
                        && matches!(
                            surface.semantics.as_ref().map(|sem| &sem.role),
                            Some(A11y::TextInput { .. })
                        )
                });
        self.resolved = true;
        Frame {
            scene: self.scene.as_deref().expect("just set"),
            animating,
            repaint_after,
            tip,
            cursor,
            edits: &self.delivered,
            clipboard: match (self.copied.take(), self.board.as_mut()) {
                (Some(s), Some(b)) => {
                    b.set(&s);
                    None
                }
                (s, _) => s,
            },
            ime,
        }
    }
}
