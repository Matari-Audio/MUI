//! One node of the walk: its outline, paint, surface and children.
use std::sync::Arc;

use mui_geometry::{Bounds, Path, Point, RoundedRect};
use mui_layout::{Frame, Size};

use super::bar::{self, BAR_MARGIN, BAR_STRIP, BAR_THIN, BAR_WIDE};
use super::outline::Contour;
use super::text::Face;
use super::{
    Ancestors, Deferred, Layer, MEMO_AGE, MemoSpan, Painted, ResolvedSurface, SceneError, Text,
    Walk, bounds, empty, snap,
};
use crate::{Color, Content, El, Element, Fill, Mix, ShadowKind, State};

/// A canvas's tagged draws: the surface's hit shapes.
type Hits = Vec<(Arc<str>, Arc<Path>)>;

impl<'a> Walk<'a> {
    pub(super) fn node<'n: 'a>(
        &mut self,
        n: &'n El,
        path: &mut String,
        under: Color,
        ancestors: &Ancestors,
    ) -> Result<(), SceneError> {
        let frame = self.frames[self.i];
        let at = self.i;
        self.i += 1;
        if frame.size.width <= 0.0 || frame.size.height <= 0.0 {
            // A flex share that collapsed to nothing: invisible, and so are
            // its children.
            self.i = at + self.sizes[at];
            return Ok(());
        }
        let memo = match n.payload().extras().memo {
            Some(m) if m.reused && self.splice(m.id, at, frame, path, under, ancestors) => {
                return Ok(());
            }
            Some(m) => Some(self.open(m, at, frame, path, under, ancestors)),
            None => None,
        };
        let key = self.intern(n.key().unwrap_or(path));
        // Before the outline: its cache names the node by it.
        self.key = key.clone();
        let e = n.payload();
        let s = &e.style;
        let mut inner = ancestors.clone();
        inner.cursor = s.cursor.or(ancestors.cursor);
        // A switched-off card switches off what it contains: nothing inside
        // it may be reached while its own frame cannot be.
        inner.disabled = ancestors.disabled || e.disabled;
        if (e.extras().inset_surface.is_some() && !self.regions.contains_key(&at))
            || (e.extras().border_join.is_some() && !self.joined_nodes.contains(&at))
        {
            return Err(mui_geometry::Error::InvalidOptions(
                "material requires a surface-layout owner",
            )
            .into());
        }
        let start = self.paint.len();
        let material = self.material_weld(n, frame, path, under)?;
        let mut contour = match &material {
            Some(m) => Contour {
                changed: true,
                ..Contour::path(m.outline.clone())
            },
            None => self.outline(n, frame, self.i)?,
        };

        // Regions and surfaces are laid out against frames: scene space.
        let world = (e.extras().inside.is_some() || e.extras().surface_padding.is_some())
            .then(|| contour.world());
        let world = world.as_ref().unwrap_or(&contour.path);
        self.partition(n, world, frame, at, (&key, path.as_str()))?;
        if e.extras().surface_padding.is_some() {
            let geometry = self.surface_cache.resolve(
                n,
                (&key, at),
                &self.frames,
                world,
                self.spec,
                self.region_cache,
            )?;
            let origin = geometry.origin;
            self.regions
                .extend(geometry.panels.into_iter().map(|(i, p)| (i, (p, origin))));
            self.joined_nodes.extend(geometry.join_nodes);
            self.surface_joins.insert(at, geometry.joins);
        }
        // Each envelope belongs to exactly one node, visited once.
        let enveloped = match self.region_envelopes.remove(&at) {
            Some(p) => {
                self.mark(Layer::Clip, p, None);
                true
            }
            None => false,
        };
        // A mask composites against what the subtree drew, so the subtree
        // needs a layer of its own even when nothing asked to blend.
        let masked = !s.mask.is_none();
        let blended = match s.layer {
            Some((mix, opacity)) if !(mix == Mix::Normal && opacity == 1.0) => Some((mix, opacity)),
            _ if masked => Some((Mix::Normal, 1.0)),
            _ => None,
        };
        if let Some((mix, opacity)) = blended {
            self.mark(Layer::Blend { mix, opacity }, empty(), None);
        }
        if s.backdrop_blur > 0.0 {
            // In scene space: the renderer samples the backdrop there.
            self.mark(Layer::Backdrop, contour.world(), contour.world_rect());
            if let Some(p) = self.paint.last_mut() {
                p.blur = s.backdrop_blur;
            }
        }
        for sh in s.shadow.iter().filter(|sh| sh.kind == ShadowKind::Drop) {
            self.shadow(sh, &contour, under)?;
        }
        let bg = self.fill(e, material.as_ref(), &contour, under);
        let border_background = self.paint.len();
        let mut bg = self.shells(s, &mut contour, bg)?;
        self.inset_shadows(s, &contour, bg)?;
        let late_stroke = match &s.stroke {
            Some(st) if material.is_none() && e.extras().border_ramp.is_none() => {
                self.stroke(st, e, &contour, bg)?
            }
            _ => None,
        };
        let hits = self.content(e, at, &key, &contour, &mut bg, under)?;
        let content = self.content_size(n, at, frame);
        let shape_bounds = contour.bounds()?.map(|b| b.translated(contour.offset));

        let surface = self.surfaces.len();
        self.at.insert(key.clone(), surface);
        let (semantics, semantic_label_implicit) = match (&e.semantics, &e.content) {
            (Some(semantics), Content::Text(text)) if semantics.label.is_none() => {
                let mut semantics = crate::Semantics::clone(semantics);
                semantics.label = Some(text.clone());
                (Some(semantics), true)
            }
            (semantics, _) => (semantics.as_deref().cloned(), false),
        };
        self.surfaces.push(ResolvedSurface {
            key: key.clone(),
            frame,
            bounds: shape_bounds,
            path: contour.path.clone(),
            rect: contour.rect,
            offset: contour.offset,
            topology_changed: contour.changed,
            cursor: inner.cursor,
            tip: e.extras().tip.clone(),
            focusable: e.focusable,
            captures_wheel: e.captures_wheel,
            tracks_pointer: e.tracks_pointer,
            pointer_states: e
                .states
                .iter()
                .any(|(st, _)| matches!(st, State::Hover | State::Press)),
            disabled: inner.disabled,
            semantics,
            semantic_label_implicit,
            text_value: match &e.content {
                Content::Text(t) => Some(t.clone()),
                _ => None,
            },
            clip: ancestors.clip,
            clip_path: ancestors.clip_paths.clone(),
            parent: ancestors.parent.clone(),
            content,
            hits,
        });
        if n.key()
            .is_some_and(|k| !k.is_empty() && !k.starts_with('/'))
        {
            inner.parent = Some(key.clone());
        }
        // A union is one contour, so its children paint inside it: a square
        // tab's own fill stops at the filleted corner instead of poking past
        // the shared outline.
        let clips =
            n.is_clip() || s.union || e.extras().inside.is_some() || self.regions.contains_key(&at);
        if clips {
            let image = material.as_ref().map(|m| m.image_rect.bounds());
            let image = image.or(shape_bounds);
            self.clip(image, &contour, frame, &mut inner)?;
        }
        self.children(n, at, path, bg, &inner)?;

        // Everything from here on closes this node, whatever its children
        // left the key at.
        self.key = key;
        if let Some(heat) = e
            .scroll_bar_heat
            .filter(|_| n.is_scroll() && !e.scroll_bar_off)
        {
            self.scroll_bars(n, heat, frame, content, bg, &inner)?;
        }
        if clips {
            self.mark(Layer::Unclip, empty(), None);
        }
        if let Some((stroke_path, stroke_rect, fill, width, clipped)) = late_stroke {
            let at = contour.offset;
            if clipped {
                self.mark_at(Layer::Clip, stroke_path.clone(), None, at);
            }
            if let Some(p) = self.push(Layer::Stroke, stroke_path, stroke_rect, &fill, bg) {
                p.width = width;
                p.offset = at;
            }
            if clipped {
                self.mark(Layer::Unclip, empty(), None);
            }
        }
        if let Some(ramp) = &e.extras().border_ramp {
            ramp.validate()?;
            if material.is_some() {
                return Err(SceneError::UnsupportedWeld(
                    "border ramp requires a fixed outline, not material welding",
                ));
            }
            self.border_ramp(n, ramp, at, &contour, bg, border_background)?;
        }
        if let Some(m) = &material {
            // Consume only immediate source plates. Text, canvases, children,
            // clips and semantics keep their own authoring and painter order.
            // The plates painted inside this node, so only its entries are
            // scanned, not everything painted before it.
            let own = self.paint.split_off(start);
            let all = own.len();
            self.paint
                .extend(own.into_iter().filter(|p| !m.consumes(&p.key, p.layer)));
            if self.paint.len() - start != all {
                // What a memo inside painted has been thinned: not copyable.
                for s in self.memos.iter_mut().filter(|s| s.paint.start >= start) {
                    s.closed = false;
                }
            }
        }
        if blended.is_some() {
            if masked
                && let Some(p) =
                    self.push(Layer::Mask, contour.path.clone(), contour.rect, &s.mask, bg)
            {
                p.offset = contour.offset;
            }
            self.mark(Layer::Unblend, empty(), None);
        }
        if enveloped {
            self.mark(Layer::Unclip, empty(), None);
        }
        if let Some(open) = memo {
            self.close(open);
        }
        Ok(())
    }

    /// Whether anything in the node range `at..at + size` takes a region,
    /// an envelope, a join or a ramp anchor from the walk around it.
    fn fed(&self, at: usize, size: usize) -> bool {
        let inside = |k: &usize| (at..at + size).contains(k);
        self.regions.keys().any(inside)
            || self.region_envelopes.keys().any(inside)
            || self.joined_nodes.iter().any(inside)
            || self.ramp_anchors.keys().any(inside)
            || self.surface_joins.keys().any(inside)
    }

    /// Start recording a memo's span; see [`MemoSpan`].
    fn open(
        &mut self,
        m: crate::Memo,
        at: usize,
        frame: Frame,
        path: &str,
        under: Color,
        ancestors: &Ancestors,
    ) -> (usize, usize, usize) {
        let size = self.sizes[at];
        self.memos.push(MemoSpan {
            id: m.id,
            reused: m.reused,
            at,
            size,
            paint: self.paint.len()..self.paint.len(),
            surfaces: self.surfaces.len()..self.surfaces.len(),
            nested: 0,
            origin: Point::new(frame.x, frame.y),
            path: path.to_owned(),
            under,
            base_y: self.base_y,
            ancestors: ancestors.clone(),
            closed: !self.fed(at, size),
            floats: false,
            generation: self.runs.generation,
        });
        (
            self.memos.len() - 1,
            self.deferred.len(),
            self.external_welds.len(),
        )
    }

    fn close(&mut self, (j, deferred, welds): (usize, usize, usize)) {
        let (paint, surfaces, nested) = (self.paint.len(), self.surfaces.len(), self.memos.len());
        let floats = self.deferred.len() != deferred;
        let s = &mut self.memos[j];
        s.paint.end = paint;
        s.surfaces.end = surfaces;
        s.nested = nested - j - 1;
        s.floats = floats;
        s.closed &= !floats && self.external_welds.len() == welds;
    }

    /// Paint a reused memo by copying last resolve's span of it, moved by
    /// however far its origin did. Declines -- and the subtree is walked --
    /// unless every frame in it moved by exactly that, on the device grid,
    /// under the same inheritance, and nothing outside it fed its paint.
    fn splice(
        &mut self,
        id: u64,
        at: usize,
        frame: Frame,
        path: &str,
        under: Color,
        ancestors: &Ancestors,
    ) -> bool {
        let Some(prev) = self.prev else {
            return false;
        };
        let Some(j) = prev.memos.iter().position(|s| s.id == id) else {
            return false;
        };
        let old = &prev.memos[j];
        let spans = &prev.memos[j..=j + old.nested];
        let size = self.sizes[at];
        let oldest = spans.iter().map(|s| s.generation).min().unwrap_or(0);
        let age = self.runs.generation.wrapping_sub(oldest);
        if !old.closed
            || old.size != size
            || old.path != path
            || old.under != under
            || old.base_y != self.base_y
            || !old.ancestors.same(ancestors)
            || age > MEMO_AGE
            || self.fed(at, size)
        {
            return false;
        }
        let d = Point::new(frame.x - old.origin.x, frame.y - old.origin.y);
        let moved = d != Point::ZERO;
        let grid = |v: f64| {
            self.spec
                .device_scale
                .is_none_or(|s| (v * s).fract() == 0.0)
        };
        // A moved span's clips are its own, so they move with it; an
        // ancestor's would not.
        if moved && (ancestors.clip.is_some() || !grid(d.x) || !grid(d.y)) {
            return false;
        }
        let Some(was) = prev.layout.all().get(old.at..old.at + size) else {
            return false;
        };
        let now = &self.frames[at..at + size];
        if !was
            .iter()
            .zip(now)
            .all(|(o, f)| f.x == o.x + d.x && f.y == o.y + d.y && f.size == o.size)
        {
            return false;
        }
        let (paint, surfaces) = (self.paint.len(), self.surfaces.len());
        let mut shift = Shift::new(d);
        self.paint.extend(
            prev.paint[old.paint.clone()]
                .iter()
                .map(|p| shift.painted(p)),
        );
        for s in &prev.surfaces[old.surfaces.clone()] {
            self.at.insert(s.key.clone(), self.surfaces.len());
            self.surfaces.push(shift.surface(s));
        }
        for s in spans {
            let mut s = s.clone();
            s.reused = true;
            s.at = s.at - old.at + at;
            s.paint =
                s.paint.start - old.paint.start + paint..s.paint.end - old.paint.start + paint;
            s.surfaces = s.surfaces.start - old.surfaces.start + surfaces
                ..s.surfaces.end - old.surfaces.start + surfaces;
            s.origin = s.origin + d;
            self.memos.push(s);
        }
        self.age = self.age.max(age);
        self.i = at + size;
        true
    }

    /// The node's text or canvas draws, clipped to its region when it has
    /// one. Text is ink over `under`, so it hands `under` on as the ground.
    fn content(
        &mut self,
        e: &Element,
        at: usize,
        key: &Arc<str>,
        contour: &Contour,
        bg: &mut Color,
        under: Color,
    ) -> Result<Hits, SceneError> {
        let mut hits = Vec::new();
        let shaped = self.regions.contains_key(&at) && !matches!(e.content, Content::None);
        if shaped {
            self.mark_on(Layer::Clip, contour);
        }
        let frame = self.frames[at];
        match &e.content {
            Content::Text(t) => {
                *bg = under;
                self.text(e, t, frame, key, under)?;
            }
            Content::Canvas(c) => {
                // Local to the frame corner, where the canvas draws from.
                let origin = Point::new(frame.x, frame.y);
                let draws = (c.0)(frame.size);
                let generation = self.outlines.generation;
                let paths = match self.outlines.canvases.get_mut(key) {
                    // A plain canvas draws a fresh list every frame, most
                    // often the same shapes: those keep last frame's paths,
                    // wherever it moved, unvalidated, uncopied and equal
                    // downstream by pointer.
                    Some((old, paths, seen))
                        if Arc::ptr_eq(old, &draws)
                            || old.len() == draws.len()
                                && old.iter().zip(draws.iter()).all(|(a, b)| {
                                    Arc::ptr_eq(&a.path, &b.path) || a.path == b.path
                                }) =>
                    {
                        *old = draws.clone();
                        *seen = generation;
                        paths.clone()
                    }
                    _ => {
                        let paths = draws
                            .iter()
                            .map(|draw| {
                                draw.path.validate(100_000)?;
                                Ok(draw.path.clone())
                            })
                            .collect::<Result<Vec<_>, SceneError>>()?;
                        self.outlines
                            .canvases
                            .insert(key.clone(), (draws.clone(), paths.clone(), generation));
                        paths
                    }
                };
                // Hits are local to the surface's offset, the outline's.
                let d = origin - contour.offset;
                for ((k, draw), local) in draws.iter().enumerate().zip(paths) {
                    let d = d + draw.at;
                    if let Some(tag) = &draw.tag {
                        let hit = if d == Point::ZERO {
                            local.clone()
                        } else {
                            let mut p = Path::clone(&local);
                            p.translate(d);
                            Arc::new(p)
                        };
                        hits.push((Arc::clone(tag), hit));
                    }
                    if let Some(p) = self.push(Layer::Draw(k), local, None, &draw.fill, *bg) {
                        p.width = draw.width;
                        p.offset = origin + draw.at;
                    }
                }
            }
            Content::None => {}
        }
        if shaped {
            self.mark(Layer::Unclip, empty(), None);
        }
        Ok(hits)
    }

    /// A label: one run per line, each on its own baseline.
    fn text(
        &mut self,
        e: &Element,
        t: &str,
        frame: Frame,
        key: &Arc<str>,
        under: Color,
    ) -> Result<(), SceneError> {
        let th = self.spec.theme;
        let size = e.text_size.unwrap_or(th.text);
        // Text's own fill is its ink, not a box behind it.
        let ink = if e.style.fill.is_none() {
            Fill::Role(crate::Role::Ink)
        } else {
            e.style.fill.clone()
        };
        let face = Face::of(e, th);
        let lines = self.runs.lines(t, face, frame.size.width, e.lines);
        let fonts = self.runs.fonts_for(face);
        let font_coords = self.runs.coords(&fonts, face);
        let hint = self.runs.settled(key, &font_coords);
        let n = lines.len();
        let base = self.base_y;
        for (li, line) in lines.iter().enumerate() {
            let Some(run) = self.runs.run(line, face)? else {
                break;
            };
            // One line sits centred on ascent+descent, or on the
            // baseline its parent chose; a stack centres the block.
            let dy = match (base, n) {
                (Some(b), _) => b + li as f64 * snap(run.line_height, self.spec.device_scale),
                (_, 1) => {
                    frame.y + (frame.size.height - run.ascent - run.descent) / 2.0 + run.ascent
                }
                _ => {
                    // A snapped line height, so the stack of
                    // baselines is even once the renderer hints each
                    // one to a whole device pixel.
                    let lh = snap(run.line_height, self.spec.device_scale);
                    frame.y
                        + (frame.size.height - n as f64 * lh) / 2.0
                        + run.ascent
                        + li as f64 * lh
                }
            };
            // glifo hints by rounding the device-space baseline per
            // glyph, so an unsnapped stack of fractional line heights
            // rounds to uneven leading. Snap the line, not the glyph.
            let origin = Point::new(
                snap(frame.x, self.spec.device_scale),
                snap(dy, self.spec.device_scale),
            );
            let text = Text {
                fonts: fonts.clone(),
                size: size as f32,
                origin,
                glyphs: run.glyphs.clone(),
                axes: e.axes.clone(),
                font_coords: font_coords.clone(),
                hint,
            };
            if let Some(p) = self.push(Layer::Text, empty(), None, &ink, under) {
                p.text = Some(text);
            }
        }
        Ok(())
    }

    /// A scroll node's in-flow children extent inside its padding,
    /// unscrolled; the frame size otherwise.
    fn content_size(&self, n: &El, at: usize, frame: Frame) -> Size {
        if !n.is_scroll() {
            return frame.size;
        }
        // Only in-flow child boxes belong to this viewport. A nested
        // viewport owns its own overflow; floats do not enlarge the flow.
        let scrolled = n.scroll_offset();
        let pad = n.padding(self.spec.theme.spacing);
        let (mut right, mut bottom) = (frame.x - scrolled[0], frame.y - scrolled[1]);
        let mut child_at = at + 1;
        for child in n.children() {
            if !child.is_float() {
                let f = self.frames[child_at];
                right = right.max(f.right());
                bottom = bottom.max(f.bottom());
            }
            child_at += self.sizes[child_at];
        }
        Size::new(
            (right - frame.x + scrolled[0] + pad.right - pad.left).max(0.0),
            (bottom - frame.y + scrolled[1] + pad.bottom - pad.top).max(0.0),
        )
    }

    /// The overlay scrollbar of a scroll node that overflows, per axis: a
    /// rounded thumb painted over the children inside the node's clip, and
    /// the strip along the far edge it rides in as a surface of its own. The
    /// strip is the pointer target, runtime-owned by its key, and pushed
    /// after the children so it is on top of them in the hit map. Its path
    /// is the strip rather than the thumb, so scrolling does not change the
    /// hit geometry.
    fn scroll_bars(
        &mut self,
        n: &El,
        heat: f64,
        frame: Frame,
        content: Size,
        under: Color,
        inner: &Ancestors,
    ) -> Result<(), SceneError> {
        let key = self.key.clone();
        let offset = n.scroll_offset();
        let heat = heat.clamp(0.0, 1.0);
        let thick = BAR_THIN + (BAR_WIDE - BAR_THIN) * heat;
        let ink = crate::Role::Ink.alpha((0.28 + 0.27 * heat) as f32);
        let frame_at = |x, y, width, height| Frame {
            x,
            y,
            size: Size::new(width, height),
        };
        for vertical in [true, false] {
            let (a, strip) = if vertical {
                let x = frame.right() - BAR_STRIP;
                (1, frame_at(x, frame.y, BAR_STRIP, frame.size.height))
            } else {
                let y = frame.bottom() - BAR_STRIP;
                (0, frame_at(frame.x, y, frame.size.width, BAR_STRIP))
            };
            let along = |f: Frame| {
                if vertical {
                    (f.y, f.size.height)
                } else {
                    (f.x, f.size.width)
                }
            };
            let ((start, len), (_, view)) = (along(strip), along(frame));
            let total = if vertical {
                content.height
            } else {
                content.width
            };
            let Some((at, size)) = bar::thumb(start, len, view, total, offset[a]) else {
                continue;
            };
            // Hugging the far edge, so it thickens inward over the content.
            let thumb = if vertical {
                frame_at(frame.right() - BAR_MARGIN - thick, at, thick, size)
            } else {
                frame_at(at, frame.bottom() - BAR_MARGIN - thick, size, thick)
            };
            let scale = self.spec.device_scale;
            let rr = RoundedRect::new(bounds(thumb, scale), thick / 2.0)?;
            let hit = RoundedRect::new(bounds(strip, scale), 0.0)?;
            let bar_key: Arc<str> = bar::bar_key(&key, vertical).into();
            self.key = bar_key.clone();
            self.push(Layer::Fill, rr.path(), Some(rr), &ink, under);
            self.at.insert(bar_key.clone(), self.surfaces.len());
            self.surfaces.push(ResolvedSurface {
                key: bar_key,
                frame: strip,
                bounds: Some(hit.bounds()),
                path: Arc::new(hit.path()),
                rect: Some(hit),
                offset: Point::ZERO,
                topology_changed: false,
                cursor: None,
                tip: None,
                focusable: false,
                captures_wheel: false,
                tracks_pointer: false,
                // Earns it a place in the hit map without an id.
                pointer_states: true,
                disabled: inner.disabled,
                semantics: None,
                semantic_label_implicit: false,
                text_value: None,
                clip: inner.clip,
                clip_path: inner.clip_paths.clone(),
                parent: None,
                content: strip.size,
                hits: Vec::new(),
            });
        }
        self.key = key;
        Ok(())
    }

    /// Clip the subtree to `b` -- the outline's bounds, or a material's
    /// image -- and hand the descendants that clip both as a rectangle and
    /// as the exact path.
    fn clip(
        &mut self,
        b: Option<Bounds>,
        contour: &Contour,
        frame: Frame,
        inner: &mut Ancestors,
    ) -> Result<(), SceneError> {
        let b = b.unwrap_or_else(|| bounds(frame, self.spec.device_scale));
        let b = inner.clip.map_or(b, |c| {
            Bounds::new(
                b.min.x.max(c.min.x),
                b.min.y.max(c.min.y),
                b.max.x.min(c.max.x),
                b.max.y.min(c.max.y),
            )
        });
        self.mark_on(Layer::Clip, contour);
        inner.clip = Some(b);
        // Every exact outline in one allocation all descendants share.
        // `clip` remains the rectangular fast path used by existing input
        // adapters; rounded or welded corners can now be tested without
        // tessellating during each pointer query. The paths themselves are
        // the ancestors' own outlines: a nested clip adds a pointer each.
        let outer = inner.clip_paths.as_deref().unwrap_or_default();
        inner.clip_paths = Some(
            outer
                .iter()
                .cloned()
                .chain(std::iter::once((contour.path.clone(), contour.offset)))
                .collect(),
        );
        Ok(())
    }

    /// Walk `n`'s children in paint order: carves are spent, sticky ones
    /// paint after their siblings, floats wait for the end of the walk.
    fn children<'n: 'a>(
        &mut self,
        n: &'n El,
        at: usize,
        path: &mut String,
        bg: Color,
        inner: &Ancestors,
    ) -> Result<(), SceneError> {
        let outer_base = self.base_y;
        self.base_y = None;
        let bases = self.baselines(n, at)?;
        // One scratch string for the whole walk: a path is O(depth) bytes and
        // formatting a fresh one per node was the walk's largest single cost.
        let mark = path.len();
        let mut sticky = Vec::new();
        for (j, c) in n.children().iter().enumerate() {
            self.base_y = bases.get(j).and_then(|b| b.map(|(y, _)| y));
            path.truncate(mark);
            super::push_index(path, j);
            if c.payload().carve.is_some() {
                // Already spent: it shaped the outline instead of painting.
                self.i += self.sizes[self.i];
                continue;
            }
            if c.is_sticky() && !c.is_float() {
                // Pinned over the siblings that scroll under it, so it paints
                // after them -- but inside this node's clip, unlike a float.
                sticky.push((self.i, c, path.clone(), self.base_y));
                self.i += self.sizes[self.i];
                continue;
            }
            if c.is_float() {
                self.deferred.push(Deferred {
                    at: self.i,
                    node: c,
                    path: path.clone(),
                    under: bg,
                    ancestors: Ancestors {
                        parent: inner.parent.clone(),
                        cursor: inner.cursor,
                        disabled: inner.disabled,
                        ..Ancestors::default()
                    },
                });
                self.i += self.sizes[self.i];
            } else {
                self.node(c, path, bg, inner)?;
            }
        }
        let end = self.i;
        for (at2, c, mut p, base) in sticky {
            self.i = at2;
            self.base_y = base;
            self.node(c, &mut p, bg, inner)?;
        }
        self.i = end;
        path.truncate(mark);
        self.base_y = outer_base;
        Ok(())
    }

    /// Each direct text child's own centred baseline, then every child
    /// takes the lowest of the ones it shares a line with: a row taller than
    /// its text keeps its labels inside their frames, and a wrapping row gets
    /// one baseline per line instead of one per box. Empty unless `n` is a
    /// `.baseline()` row.
    ///
    /// ponytail: O(n^2) over direct children, which is a handful.
    fn baselines(&mut self, n: &El, at: usize) -> Result<Vec<Option<(f64, Frame)>>, SceneError> {
        let mut bases: Vec<Option<(f64, Frame)>> = Vec::new();
        if !n.payload().baseline {
            return Ok(bases);
        }
        let th = self.spec.theme;
        let mut at2 = at + 1;
        for c in n.children() {
            let f = self.frames[at2];
            at2 += self.sizes[at2];
            let own = match &c.payload().content {
                Content::Text(t) => {
                    let face = Face::of(c.payload(), th);
                    let lines = self.runs.lines(t, face, f.size.width, c.payload().lines);
                    self.runs.run(&lines[0], face)?.map(|r| {
                        let height = if lines.len() == 1 {
                            r.ascent + r.descent
                        } else {
                            lines.len() as f64 * snap(r.line_height, self.spec.device_scale)
                        };
                        f.y + (f.size.height - height) / 2.0 + r.ascent
                    })
                }
                _ => None,
            };
            bases.push(own.map(|b| (b, f)));
        }
        let lines = bases.clone();
        for (b, f) in bases.iter_mut().flatten() {
            *b = lines
                .iter()
                .flatten()
                .filter(|(_, g)| g.y < f.bottom() && f.y < g.bottom())
                .fold(*b, |m, (o, _)| m.max(*o));
        }
        Ok(bases)
    }
}

/// A clip list, and the same list moved.
type Clips = Arc<[super::PlacedPath]>;

/// A copied span moved by `d`: every offset, frame, origin and clip in it;
/// the paths are local and stay the same `Arc`s. Clip lists shared between
/// surfaces are moved once and stay shared.
struct Shift {
    d: Point,
    lists: Vec<(*const [super::PlacedPath], Clips)>,
}
impl Shift {
    fn new(d: Point) -> Self {
        Self {
            d,
            lists: Vec::new(),
        }
    }
    fn painted(&mut self, p: &Painted) -> Painted {
        let mut p = p.clone();
        if self.d != Point::ZERO {
            match &mut p.text {
                Some(t) => t.origin = t.origin + self.d,
                None => p.offset = p.offset + self.d,
            }
        }
        p
    }
    fn surface(&mut self, s: &ResolvedSurface) -> ResolvedSurface {
        let mut s = s.clone();
        let d = self.d;
        if d == Point::ZERO {
            return s;
        }
        s.frame.x += d.x;
        s.frame.y += d.y;
        s.bounds = s.bounds.map(|b| b.translated(d));
        s.offset = s.offset + d;
        s.clip = s.clip.map(|b| b.translated(d));
        if let Some(list) = &s.clip_path {
            let at = Arc::as_ptr(list);
            s.clip_path = Some(
                if let Some((_, moved)) = self.lists.iter().find(|(p, _)| std::ptr::eq(*p, at)) {
                    moved.clone()
                } else {
                    let moved: Clips = list.iter().map(|(p, o)| (p.clone(), *o + d)).collect();
                    self.lists.push((at, moved.clone()));
                    moved
                },
            );
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::Paint;
    use crate::prelude::*;

    /// Paths are local: two same-sized buttons paint one outline `Arc`, each
    /// at its own offset, and a button that only moved keeps it -- its
    /// surface too, so the hit map converts nothing new.
    #[test]
    fn same_shapes_share_one_path_and_a_move_keeps_it() {
        let spec = |gap: f64| {
            SceneSpec::new(
                row((0..2).map(|i| {
                    leaf(40., 20.)
                        .radius(6.)
                        .fill(Role::Primary)
                        .id(format!("b{i}"))
                }))
                .gap(gap)
                .pad(gap),
            )
        };
        let fill = |s: &ResolvedScene, k: &str| {
            s.paint
                .iter()
                .find(|p| p.layer == Layer::Fill && &*p.key == k)
                .map(|p| (p.path.clone(), p.offset))
                .unwrap()
        };
        let mut cache = TextCache::default();
        let first = resolve_scene_with(&spec(4.), &mut cache).unwrap();
        let ((a, at_a), (b, at_b)) = (fill(&first, "b0"), fill(&first, "b1"));
        assert!(Arc::ptr_eq(&a, &b), "one path for both buttons");
        assert_eq!((at_a, at_b), (Point::new(4., 4.), Point::new(48., 4.)));
        let moved = resolve_scene_with(&spec(10.), &mut cache).unwrap();
        let (c, at_c) = fill(&moved, "b1");
        assert!(Arc::ptr_eq(&a, &c), "a move keeps the path");
        assert_eq!(at_c, Point::new(60., 10.));
        let s = moved.surface("b1").unwrap();
        assert!(Arc::ptr_eq(&s.path, &a) && s.offset == at_c);
        let placed = moved.surface("b1").unwrap().placed();
        let first_point = match placed.commands[0] {
            mui_geometry::PathCommand::MoveTo(p) => p,
            ref c => panic!("{c:?}"),
        };
        assert!(first_point.x >= 60. && first_point.y >= 10.);
    }

    /// The fade paints last, source-atop, and only inside the layer the node
    /// opened for it.
    #[test]
    fn a_mask_paints_inside_the_nodes_own_blend_layer() {
        let fade = Gradient::linear(
            180.,
            [(0.7, Role::Surface.alpha(0.)), (1., Role::Surface.into())],
        );
        let row = col![leaf(40., 20.).fill(Role::Primary)]
            .pad(8.)
            .mask(fade)
            .id("list");
        let s = resolve_scene(&SceneSpec::new(row)).unwrap();
        let at = |l: Layer| s.paint.iter().position(|p| p.layer == l).unwrap();
        let blend = at(Layer::Blend {
            mix: Mix::Normal,
            opacity: 1.0,
        });
        assert!(
            blend < at(Layer::Fill),
            "the subtree paints inside the layer"
        );
        assert!(at(Layer::Fill) < at(Layer::Mask));
        assert!(at(Layer::Mask) < at(Layer::Unblend));
        assert!(!s.paint[at(Layer::Mask)].path.commands.is_empty());
    }

    /// A blended node's whole subtree, clip included, sits between the pair.
    #[test]
    fn a_blended_node_is_wrapped_in_a_layer_pair() {
        let dim = column([leaf(10., 10.).fill(Role::Ink).id("kid")])
            .fill(Role::Surface)
            .blend(Mix::Multiply)
            .opacity(0.5)
            .id("dim");
        let root = row([dim, leaf(10., 10.).fill(Role::Surface).id("plain")]).id("root");
        let s = resolve_scene(&SceneSpec::new(root).offered(Size::new(60., 20.))).unwrap();
        let layers: Vec<_> = s.paint.iter().map(|p| (&*p.key, p.layer)).collect();
        let at = |k, l| layers.iter().position(|x| *x == (k, l)).unwrap();
        let open = at(
            "dim",
            Layer::Blend {
                mix: Mix::Multiply,
                opacity: 0.5,
            },
        );
        assert!(open < at("dim", Layer::Fill));
        assert!(at("kid", Layer::Fill) < at("dim", Layer::Unblend));
        assert!(
            !layers
                .iter()
                .any(|(k, l)| *k == "plain" && matches!(l, Layer::Blend { .. } | Layer::Unblend)),
            "{layers:?}"
        );
    }

    #[test]
    fn a_weld_clips_its_children_to_the_shared_contour() {
        // A square tab welded to a rounded body: the tab's own fill must not
        // paint the corner the fillet rounded off, so the weld clips.
        let root = row([
            leaf(36., 60.).fill(Role::Surface).radius(0.).id("tab"),
            leaf(200., 120.).id("body"),
        ])
        .radius(20.)
        .union(Role::Surface)
        .id("weld");
        let s = resolve_scene(&SceneSpec::new(root).offered(Size::new(236., 120.))).unwrap();
        let order: Vec<_> = s.paint.iter().map(|p| (&*p.key, p.layer)).collect();
        let at = |key: &str, layer: Layer| order.iter().position(|x| *x == (key, layer)).unwrap();
        assert!(
            at("weld", Layer::Fill) < at("weld", Layer::Clip)
                && at("weld", Layer::Clip) < at("tab", Layer::Fill)
                && at("tab", Layer::Fill) < at("weld", Layer::Unclip),
            "the tab painted outside the welded contour: {order:?}"
        );
        let clip = &s.paint[at("weld", Layer::Clip)].path;
        let corner = clip.flatten(0.1, 20_000).unwrap().concat();
        assert!(
            !corner.iter().any(|p| p.x.abs() < 1e-6 && p.y.abs() < 1e-6),
            "the clip is the square frame, not the filleted contour"
        );
    }

    #[test]
    fn clip_floats_canvas_cursor_and_content() {
        let list = column([leaf(50., 30.).id("a"), leaf(50., 30.), leaf(50., 30.)])
            .gap(10.)
            .scroll()
            .scrolled(0., 25.)
            .size(60., 60.)
            .cursor(Cursor::Hand)
            .id("list");
        let tip = leaf(10., 10.).fill(Primary).float().id("tip");
        let draw = canvas(|s| {
            vec![Draw::stroke(
                Path::default().move_to(Point::new(0., s.height)).cubic_to(
                    Point::new(s.width / 2., 0.),
                    Point::new(s.width / 2., 0.),
                    Point::new(s.width, s.height),
                ),
                Primary,
                2.,
            )]
        })
        .size(40., 40.)
        .id("curve");
        let root = column([list, tip, draw]).id("root");
        let s = resolve_scene(&SceneSpec::new(root)).unwrap();
        let layers: Vec<_> = s.paint.iter().map(|p| (&*p.key, p.layer)).collect();
        let at = |k: &str, l: Layer| layers.iter().position(|x| *x == (k, l)).unwrap();
        assert!(at("list", Layer::Clip) < at("list", Layer::Unclip));
        assert_eq!(*layers.last().unwrap(), ("tip", Layer::Fill), "{layers:?}");
        assert!(
            s.paint
                .iter()
                .any(|p| &*p.key == "curve" && p.layer == Layer::Draw(0) && p.width == 2.)
        );
        let list = s.surface("list").unwrap();
        assert_eq!(
            list.content,
            Size::new(55., 110.),
            "rows centred at x=5, three rows and two gaps tall"
        );
        assert_eq!(list.clip, None);
        assert_eq!(s.surface("a").unwrap().clip, Some(list.bounds.unwrap()));
        assert_eq!(s.surface("a").unwrap().cursor, Some(Cursor::Hand));
        assert_eq!(s.layout.frame("a").unwrap().y, -25.);
        assert_eq!(s.surfaces().last().map(|s| &*s.key), Some("tip"));
    }

    #[test]
    fn rounded_clip_exposes_cached_path_alongside_rect_bounds() {
        let clip = column([leaf(20., 20.).id("a"), leaf(20., 20.).id("b")])
            .size(40., 40.)
            .radius(10.)
            .clip()
            .id("clip");
        let s = resolve_scene(&SceneSpec::new(clip)).unwrap();
        let parent = s.surface("clip").unwrap();
        let child = s.surface("a").unwrap();
        let sibling = s.surface("b").unwrap();
        assert_eq!(parent.clip, None);
        assert_eq!(child.clip, parent.bounds);
        assert_eq!(sibling.clip, parent.bounds);
        let paths = child.clip_path.as_ref().expect("rounded clip path");
        assert_eq!(paths.len(), 1);
        assert!(
            paths[0]
                .0
                .commands
                .iter()
                .any(|c| matches!(c, mui_geometry::PathCommand::ArcTo(_))),
            "clip path lost its rounded corners"
        );
        assert!(
            Arc::ptr_eq(
                child.clip_path.as_ref().unwrap(),
                sibling.clip_path.as_ref().unwrap()
            ),
            "clip path must remain cached for repeated hit tests"
        );
    }

    #[test]
    fn nested_rounded_clips_keep_every_cached_path() {
        let inner = column([leaf(30., 30.).id("leaf")])
            .size(30., 30.)
            .radius(6.)
            .clip()
            .id("inner");
        let outer = column([inner])
            .size(40., 40.)
            .radius(10.)
            .clip()
            .id("outer");
        let s = resolve_scene(&SceneSpec::new(outer)).unwrap();
        let paths = s.surface("leaf").unwrap().clip_path.as_ref().unwrap();
        assert_eq!(
            paths.len(),
            2,
            "inner and outer clips must both filter hits"
        );
        for (p, k) in paths.iter().zip(["outer", "inner"]) {
            assert!(
                Arc::ptr_eq(&p.0, &s.surface(k).unwrap().path),
                "{k}'s clip is a copy of its outline"
            );
        }
        assert!(paths.iter().all(|p| {
            p.0.commands
                .iter()
                .any(|c| matches!(c, mui_geometry::PathCommand::ArcTo(_)))
        }));
    }

    /// A sticky header paints after the rows that slide under it, and stays
    /// inside the scroll's clip -- a float would escape it.
    #[test]
    fn a_sticky_header_paints_over_its_section_and_keeps_the_clip() {
        let section = column([
            leaf(60., 20.).fill(Role::Surface).id("head").sticky(),
            leaf(60., 60.).id("row"),
        ])
        .id("section");
        let list = column([section]).scroll().size(60., 40.).id("list");
        let s = resolve_scene(&SceneSpec::new(list)).unwrap();
        let order: Vec<_> = s.surfaces().map(|s| &*s.key).collect();
        assert_eq!(
            order,
            ["list", "section", "row", "head"],
            "the header paints last"
        );
        let clip = s.surface("list").unwrap().bounds;
        assert_eq!(s.surface("head").unwrap().clip, clip);
    }

    /// A tagged draw is hit geometry in scene space; an untagged one is
    /// paint and nothing else.
    #[test]
    fn a_tagged_draw_becomes_the_surfaces_hit_shape() {
        let box_ = |w: f64, h: f64| {
            Path::polyline(
                [(0., 0.), (w, 0.), (w, h), (0., h)].map(|(x, y)| Point::new(x, y)),
                true,
            )
        };
        let plot = canvas(move |s| {
            vec![
                Draw::fill(box_(s.width, s.height), Primary),
                Draw::hit(box_(s.width / 2., s.height), "left"),
            ]
        })
        .size(40., 20.)
        .id("plot");
        let root = column([leaf(40., 30.), plot]);
        let s = resolve_scene(&SceneSpec::new(root)).unwrap();
        let surface = s.surface("plot").unwrap();
        let [(tag, path)] = &surface.hits[..] else {
            panic!("one tagged draw, got {:?}", surface.hits.len())
        };
        assert_eq!(&**tag, "left");
        let pts = path.flatten(0.1, 1000).unwrap().concat();
        let top = pts.iter().map(|p| p.y).fold(f64::MAX, f64::min);
        assert_eq!(top + surface.offset.y, 30., "placed in the node's frame");
        assert!(
            !s.paint
                .iter()
                .any(|p| &*p.key == "plot" && p.layer == Layer::Draw(1)),
            "a hit-only draw paints nothing"
        );
    }

    #[test]
    fn a_baseline_row_lines_two_sizes_up_on_the_letters() {
        let root = row([
            text("a").text_size(12.).id("small"),
            text("b").text_size(24.).id("big"),
        ])
        .baseline();
        let mut sp = SceneSpec::new(root);
        sp.font = Some(font());
        let s = resolve_scene(&sp).unwrap();
        let y = |k: &str| {
            s.paint
                .iter()
                .find(|p| &*p.key == k && p.layer == Layer::Text)
                .unwrap()
                .text
                .as_ref()
                .unwrap()
                .origin
                .y
        };
        assert_eq!(y("small"), y("big"), "one baseline, two sizes");
        let mut plain = sp.clone();
        plain.root = row([
            text("a").text_size(12.).id("small"),
            text("b").text_size(24.).id("big"),
        ]);
        let p = resolve_scene(&plain).unwrap();
        let py = |k: &str| {
            p.paint
                .iter()
                .find(|x| &*x.key == k && x.layer == Layer::Text)
                .unwrap()
                .text
                .as_ref()
                .unwrap()
                .origin
                .y
        };
        assert_ne!(py("small"), py("big"), "and centring alone does not");
    }

    /// Every text layer's baseline for a key, in paint order.
    fn baselines(s: &ResolvedScene, k: &str) -> Vec<f64> {
        s.paint
            .iter()
            .filter(|p| &*p.key == k && p.layer == Layer::Text)
            .map(|p| p.text.as_ref().unwrap().origin.y)
            .collect()
    }

    #[test]
    fn a_baseline_row_taller_than_its_text_keeps_the_letters_in_their_frames() {
        let root = row([
            text("Kurv").text_size(22.).id("title"),
            text("v1.0").text_size(11.).id("ver"),
            leaf(80., 40.).id("btn"),
        ])
        .baseline()
        .gap(10.);
        let mut sp = SceneSpec::new(root).offered(Size::new(400., 60.));
        sp.font = Some(font());
        let s = resolve_scene(&sp).unwrap();
        let t = s.layout.frame("title").unwrap();
        let b = baselines(&s, "title")[0];
        assert_eq!(b, baselines(&s, "ver")[0], "one baseline, two sizes");
        assert!(
            b > t.y && b < t.bottom(),
            "baseline {b} outside the title's frame {t:?}"
        );
    }

    #[test]
    fn a_wrapping_baseline_row_gives_every_line_its_own_baseline() {
        let root = row([
            text("alpha").text_size(20.).id("a"),
            text("beta").text_size(11.).id("b"),
            text("gamma").text_size(20.).id("c"),
        ])
        .wrap()
        .baseline()
        .gap(8.);
        let mut sp = SceneSpec::new(root).offered(Size::new(120., 200.));
        sp.font = Some(font());
        let s = resolve_scene(&sp).unwrap();
        let (a, c) = (baselines(&s, "a")[0], baselines(&s, "c")[0]);
        assert_eq!(a, baselines(&s, "b")[0], "line one shares a baseline");
        assert!(a < c, "line two sits below line one: {a} {c}");
        let f = s.layout.frame("c").unwrap();
        assert!(c > f.y && c < f.bottom(), "baseline {c} outside {f:?}");
    }

    #[test]
    fn a_device_scale_puts_every_edge_and_every_baseline_on_the_grid() {
        let long = "wrap ".repeat(40);
        let row = row![
            leaf(0., 20.).grow(1.).id("a"),
            leaf(0., 20.).grow(1.).id("b"),
            leaf(0., 20.).grow(1.).id("c"),
        ];
        let root = column([row, column([text(long).id("p")]).w(120)]);
        let mut sp = SceneSpec::new(root).offered(Size::new(41., 300.)).scale(1.);
        sp.font = Some(font());
        let s = resolve_scene(&sp).unwrap();
        let edges: Vec<[f64; 2]> = ["a", "b", "c"]
            .iter()
            .map(|k| {
                let s = s.surface(k).unwrap();
                let b = s.rect.unwrap().bounds().translated(s.offset);
                [b.min.x, b.max.x]
            })
            .collect();
        for e in edges.iter().flatten() {
            assert_eq!(*e, e.round(), "{edges:?}");
        }
        assert_eq!(edges[0][1], edges[1][0], "no seam between shares");
        assert_eq!(edges[1][1], edges[2][0], "no seam between shares");
        let ys = baselines(&s, "p");
        assert!(ys.len() > 3, "wrapped into {} lines", ys.len());
        let gaps: Vec<f64> = ys.windows(2).map(|w| w[1] - w[0]).collect();
        assert!(
            gaps.windows(2).all(|g| g[0] == g[1]) && ys[0] == ys[0].round(),
            "uneven leading {gaps:?} from {ys:?}"
        );
    }

    /// Fill, clip, mask and surface all hold the node's one outline.
    #[test]
    fn a_filled_clipping_node_shares_one_outline() {
        let card = column([leaf(10., 10.)])
            .fill(Role::Surface)
            .clip()
            .mask(Role::Surface)
            .id("card");
        let s = resolve_scene(&SceneSpec::new(card)).unwrap();
        let outline = &s.surface("card").unwrap().path;
        for layer in [Layer::Fill, Layer::Clip, Layer::Mask] {
            let p = s.paint.iter().find(|p| p.layer == layer).unwrap();
            assert!(Arc::ptr_eq(&p.path, outline), "{layer:?} copied it");
        }
    }

    /// A warm resolve hands every node the key it had last frame.
    #[test]
    fn a_warm_resolve_reuses_every_key() {
        let spec = SceneSpec::new(column([leaf(10., 10.).id("named"), leaf(10., 10.)]));
        let mut text = TextCache::default();
        let cold = resolve_scene_with(&spec, &mut text).unwrap();
        let warm = resolve_scene_with(&spec, &mut text).unwrap();
        assert_eq!(cold.surfaces().count(), 3);
        for (a, b) in cold.surfaces().zip(warm.surfaces()) {
            assert!(Arc::ptr_eq(&a.key, &b.key), "{} was allocated again", a.key);
        }
    }

    #[test]
    fn a_rect_surface_reads_its_bounds_off_the_rect() {
        let s = resolve_scene(&SceneSpec::new(column([leaf(40., 20.).id("k")]))).unwrap();
        let k = s.surface("k").unwrap();
        assert_eq!(k.bounds.unwrap(), k.rect.unwrap().bounds());
    }

    #[test]
    fn a_text_node_keeps_no_fill_layer_and_no_glyph_path() {
        let root = column([text("hi").fill(Role::Primary).id("t")]);
        let mut sp = SceneSpec::new(root);
        sp.font = Some(font());
        let s = resolve_scene(&sp).unwrap();
        assert!(
            !s.paint
                .iter()
                .any(|p| &*p.key == "t" && p.layer == Layer::Fill),
            "a label's fill is its ink, not a box: {:?}",
            s.paint.iter().map(|p| p.layer).collect::<Vec<_>>()
        );
        let ink = s.paint.iter().find(|p| p.layer == Layer::Text).unwrap();
        assert_eq!(ink.paint, Paint::Solid(Theme::default().palette.primary()));
        assert!(ink.text.is_some(), "the glyphs are the ink");
        assert!(
            ink.path.commands.is_empty(),
            "and the outline is not built twice"
        );
    }

    #[test]
    fn a_canvas_closure_may_capture_a_non_send_handle() {
        let seen = std::rc::Rc::new(std::cell::Cell::new(0));
        let c = seen.clone();
        let root = canvas(move |_| {
            c.set(c.get() + 1);
            Vec::new()
        })
        .size(10., 10.);
        resolve_scene(&SceneSpec::new(root)).unwrap();
        assert_eq!(seen.get(), 1);
    }
    /// A cached draw list paints the very same paths every frame, where it
    /// stays and where it moves: only its offset follows it.
    #[test]
    fn a_cached_canvas_reuses_its_placed_paths() {
        let cache = crate::CanvasCache::new();
        let spec = |pad: f64| {
            let draw = crate::canvas_cached(&cache, 1, |s| {
                vec![crate::Draw::fill(
                    Path::polyline(
                        [
                            Point::ZERO,
                            Point::new(s.width, 0.),
                            Point::new(0., s.height),
                        ],
                        true,
                    ),
                    Role::Primary,
                )]
            })
            .size(20., 20.)
            .id("c");
            SceneSpec::new(column([draw]).pad(Spacing::Px(pad)))
        };
        let path = |s: &ResolvedScene| {
            let p = s.paint.iter().find(|p| p.layer == Layer::Draw(0)).unwrap();
            (p.path.clone(), p.placed())
        };
        let mut text = TextCache::default();
        let (a, _) = path(&resolve_scene_with(&spec(4.), &mut text).unwrap());
        let (b, _) = path(&resolve_scene_with(&spec(4.), &mut text).unwrap());
        assert!(Arc::ptr_eq(&a, &b), "a still canvas re-placed its paths");
        let (moved, placed) = path(&resolve_scene_with(&spec(9.), &mut text).unwrap());
        assert!(
            Arc::ptr_eq(&a, &moved),
            "a moved canvas re-placed its paths"
        );
        assert_eq!(
            placed.commands[0],
            mui_geometry::PathCommand::MoveTo(Point::new(9., 9.))
        );
    }

    /// A memoised panel after a lead of width `lead`: a rounded button, a
    /// label and a canvas that counts its draws.
    fn memo_spec(
        lead: f64,
        reused: bool,
        draws: &Arc<std::sync::atomic::AtomicUsize>,
    ) -> SceneSpec {
        static FONT: std::sync::LazyLock<Font> = std::sync::LazyLock::new(super::super::font);
        let count = draws.clone();
        let mut panel = column([
            leaf(40., 20.).fill(Role::Primary).radius(6.).id("m.a"),
            text("kept").id("m.t"),
            canvas(move |s| {
                count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                vec![crate::Draw::fill(
                    Path::polyline([Point::ZERO, Point::new(s.width, s.height)], true),
                    Role::Ink,
                )]
            })
            .size(10., 10.)
            .id("m.c"),
        ])
        .fill(Role::Surface)
        .id("m");
        panel.payload_mut().extras_mut().memo = Some(crate::Memo { id: 7, reused });
        let lead = leaf(lead, 10.).fill(Role::Raised);
        let mut spec =
            SceneSpec::new(row([lead, panel]).align(Align::Start)).offered(Size::new(300., 100.));
        spec.font = Some(FONT.clone());
        spec
    }
    fn draws() -> Arc<std::sync::atomic::AtomicUsize> {
        Arc::default()
    }
    fn count(d: &Arc<std::sync::atomic::AtomicUsize>) -> usize {
        d.load(std::sync::atomic::Ordering::Relaxed)
    }
    fn retained(
        spec: &SceneSpec,
        text: &mut TextCache,
        prev: Option<&ResolvedScene>,
    ) -> ResolvedScene {
        resolve_scene_retained(
            spec,
            text,
            &mut crate::WeldCache::default(),
            &mut |_, _, f| f,
            prev,
        )
        .unwrap()
    }

    /// A reused memo is copied, not walked: its canvas is not drawn again,
    /// and what it paints is what a walk would, down to the same `Arc`s.
    #[test]
    fn a_reused_memo_copies_last_resolves_paint() {
        let d = draws();
        let mut text = TextCache::default();
        let a = retained(&memo_spec(50., false, &d), &mut text, None);
        let b = retained(&memo_spec(50., true, &d), &mut text, Some(&a));
        assert_eq!(count(&d), 1, "the reused canvas was drawn again");
        assert_eq!(a.paint, b.paint);
        let shared = a
            .paint
            .iter()
            .zip(&b.paint)
            .filter(|(x, y)| Arc::ptr_eq(&x.path, &y.path));
        assert_eq!(
            shared.count(),
            a.paint.len(),
            "a copy keeps every path by pointer"
        );
        assert_eq!(b.surface("m.a"), a.surface("m.a"));
    }

    /// Moved by its neighbour, a reused memo is copied translated: paint
    /// and surfaces land exactly where a walk puts them.
    #[test]
    fn a_moved_memo_translates_its_paint_and_surfaces() {
        let d = draws();
        let mut text = TextCache::default();
        let a = retained(&memo_spec(50., false, &d), &mut text, None);
        let b = retained(&memo_spec(70., true, &d), &mut text, Some(&a));
        assert_eq!(count(&d), 1, "a moved memo was walked instead of copied");
        let walked =
            resolve_scene_with(&memo_spec(70., false, &draws()), &mut TextCache::default())
                .unwrap();
        assert_eq!(b.paint, walked.paint);
        for k in ["m", "m.a", "m.t", "m.c"] {
            assert_eq!(b.surface(k), walked.surface(k), "{k}");
        }
        assert_eq!(b.surface("m.a").unwrap().frame.x, 70.);
    }

    /// The caches a copied memo used stay warm while it is copied, and its
    /// walk renews them once they are [`MEMO_AGE`] resolves old.
    #[test]
    fn a_copied_memo_keeps_its_cache_entries() {
        let d = draws();
        let mut text = TextCache::default();
        let mut prev = retained(&memo_spec(50., false, &d), &mut text, None);
        for i in 0..MEMO_AGE + 3 {
            prev = retained(&memo_spec(50., true, &d), &mut text, Some(&prev));
            assert!(
                text.runs.contains_key("kept"),
                "the label's run was swept at {i}"
            );
            assert!(
                !text.outlines.rects.is_empty(),
                "the button's rect was swept at {i}"
            );
        }
        assert_eq!(count(&d), 2, "walked once when first built, once to renew");
    }
}
