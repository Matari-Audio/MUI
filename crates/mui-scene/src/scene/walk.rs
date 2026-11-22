//! One node of the walk: its outline, paint, surface and children.
use std::sync::Arc;

use mui_geometry::kurbo::Affine;
use mui_geometry::{Path, Point, Rect, RoundedRect, Vec2};
use mui_layout::{Frame, Size};

use super::bar::{self, BAR_MARGIN, BAR_STRIP, BAR_THIN, BAR_WIDE};
use super::outline::Contour;
use super::paint::LateStroke;
use super::text::Face;
use super::{
    Ancestors, Deferred, Layer, MEMO_AGE, MemoSpan, Painted, ResolvedSurface, SceneError, Text,
    Walk, bounds, empty, snap,
};
use crate::{Color, Content, El, Element, Fill, Mix, ShadowKind, State};

/// A canvas's tagged draws: the surface's hit shapes.
type Hits = Vec<(Arc<str>, Arc<Path>)>;

/// Whether `v` is a whole number of device pixels at `scale`, give or take
/// float noise: `0.1 * 3.0` px at 10x is 3 pixels, though not exactly.
fn on_grid(v: f64, scale: Option<f64>) -> bool {
    scale.is_none_or(|s| {
        let px = v * s;
        (px - px.round()).abs() <= 1e-6
    })
}

/// A flex share that collapsed to nothing, or a size that is not a number.
fn collapsed(f: Frame) -> bool {
    !(f.size.width > 0.0 && f.size.height > 0.0)
}

/// What painting a node opened, for [`Walk::finish`] to close in reverse
/// once its children are painted.
struct Opened {
    /// The memo span this node records; see [`Walk::open`].
    memo: Option<(usize, usize, usize)>,
    /// The paint index this node's own paint starts at.
    start: usize,
    /// The paint index a border ramp's band paints under.
    border_background: usize,
    /// A region envelope clip is open.
    enveloped: bool,
    /// The whole node, including its own canvas and shadows, is clipped.
    alpha_clips: bool,
    /// A blend layer is open, and whether it ends in a mask.
    blend: Option<bool>,
    /// The children clip is open.
    clips: bool,
    late_stroke: Option<LateStroke>,
}

impl<'a> Walk<'a> {
    /// Paint node `n` and its subtree: enter it, lay out what its material
    /// hands down, paint its own layers, record its surface, walk its
    /// children, then close what it opened.
    pub(super) fn node<'n: 'a>(
        &mut self,
        n: &'n El,
        path: &mut String,
        under: Color,
        ancestors: &Ancestors,
    ) -> Result<(), SceneError> {
        let at = self.i;
        let frame = self.tree.frames[at];
        self.i += 1;
        let e = n.payload();
        let mut pose = ancestors.clone();
        if let Some(radians) = crate::element::rotation_of(n) {
            if !radians.is_finite() {
                return Err(mui_geometry::Error::NonFinite.into());
            }
            let centre = Point::new(
                frame.x + frame.size.width / 2.,
                frame.y + frame.size.height / 2.,
            );
            pose.transform *= Affine::rotate_about(radians % std::f64::consts::TAU, centre);
        }
        self.transform = pose.transform;
        if collapsed(frame) {
            // A flex share that collapsed to nothing: invisible, and so are
            // its children.
            self.i = at + self.tree.sizes[at];
            return Ok(());
        }
        let memo = match n.payload().extras().memo {
            Some(m) if m.reused && self.splice(m.id, at, frame, path, under, &pose) => {
                return Ok(());
            }
            Some(m) => Some(self.open(m, at, frame, path, under, &pose)),
            None => None,
        };
        // ponytail: a tree path past 46 bytes (about 15 levels deep) spills to
        // one allocation per frame; name deep nodes, or intern if one shows up.
        let key = n
            .ident()
            .cloned()
            .unwrap_or_else(|| crate::Id::runtime(path));
        // Before the outline: its cache names the node by it.
        self.key = key.clone();
        let mut inner = pose;
        inner.cursor = e.style.cursor.or(ancestors.cursor);
        // A switched-off card switches off what it contains: nothing inside
        // it may be reached while its own frame cannot be.
        inner.disabled = ancestors.disabled || e.has(Element::DISABLED);
        inner.floating = ancestors.floating || n.is_float();
        self.check_owned(e, at)?;

        let start = self.out.paint.len();
        let material = self.material_weld(n, frame, path, under)?;
        let mut contour = match &material {
            Some(m) => Contour {
                changed: true,
                ..Contour::path(m.outline.clone())
            },
            None => self.outline(n, frame, self.i)?,
        };
        self.plan_below(n, &contour, frame, at, (&key, path))?;
        // Each envelope belongs to exactly one node, visited once.
        let enveloped = match self.plan.region_envelopes.remove(at) {
            Some(p) => {
                self.mark(Layer::Clip, p, None);
                true
            }
            None => false,
        };
        let alpha_clips = e.extras().alpha_mask.as_ref().is_some_and(|m| !m.is_none());
        if alpha_clips {
            self.clip(None, &contour, frame, &mut inner)?;
        }
        let blend = self.open_layers(e, &contour, under)?;
        let bg = self.fill(e, material.as_ref(), &contour, under);
        let border_background = self.out.paint.len();
        let mut bg = self.shells(&e.style, &mut contour, bg)?;
        self.inset_shadows(&e.style, &contour, bg)?;
        let late_stroke = match &e.style.stroke {
            Some(st) if material.is_none() && e.extras().border_ramp.is_none() => {
                self.stroke(st, e, &contour, bg)?
            }
            _ => None,
        };
        let (hits, text_geometry) = self.content(e, at, &key, &contour, &mut bg, under)?;
        let (shape_bounds, content) = self.surface(
            n,
            (&key, at),
            frame,
            &contour,
            (hits, text_geometry),
            (&inner, ancestors),
        )?;
        if n.key().is_some_and(crate::Id::is_named) {
            inner.parent = Some(key.clone());
        }

        // A union is one contour, so its children paint inside it: a square
        // tab's own fill stops at the filleted corner instead of poking past
        // the shared outline.
        let clips = n.is_clip()
            || e.style.union.unwrap_or_default()
            || e.extras().inside.is_some()
            || self.plan.regions.contains(at);
        if clips {
            let image = material.as_ref().map(|m| m.image_rect.bounds());
            let image = image.or(shape_bounds);
            self.clip(image, &contour, frame, &mut inner)?;
        }
        self.children(n, at, path, bg, &inner)?;

        // Everything from here on closes this node, whatever its children
        // left the key at.
        self.key = key;
        self.transform = inner.transform;
        if let Some(heats) = &self.spec.scroll_bars
            && n.is_scroll()
            && !e.has(Element::SCROLL_BAR_OFF)
        {
            let heat = heats.get(self.key.as_str()).copied().unwrap_or(0.0);
            self.scroll_bars(n, heat, frame, content, bg, &inner)?;
        }
        let opened = Opened {
            memo,
            start,
            border_background,
            enveloped,
            alpha_clips,
            blend,
            clips,
            late_stroke,
        };
        self.finish(n, at, &contour, material.as_ref(), bg, opened)
    }

    /// `.inset_surface(..)` and `.join_border(..)` take what a surface-layout
    /// owner above planned for this node; without one they are an error.
    fn check_owned(&self, e: &Element, at: usize) -> Result<(), SceneError> {
        if (e.extras().inset_surface.is_some() && !self.plan.regions.contains(at))
            || (e.extras().border_join.is_some() && !self.plan.joined_nodes.contains(at))
        {
            return Err(mui_geometry::Error::InvalidOptions(
                "material requires a surface-layout owner",
            )
            .into());
        }
        Ok(())
    }

    /// Lay out what `n`'s material hands its descendants -- region shares
    /// and surface panels -- against its outline, for them to pick up from
    /// [`Walk::plan`] when the walk reaches them.
    fn plan_below(
        &mut self,
        n: &El,
        contour: &Contour,
        frame: Frame,
        at: usize,
        (key, path): (&crate::Id, &str),
    ) -> Result<(), SceneError> {
        let x = n.payload().extras();
        // Regions and surfaces are laid out against frames: scene space.
        let world = (x.inside.is_some() || x.surface_padding.is_some()).then(|| contour.world());
        let world = world.as_ref().unwrap_or(&contour.path);
        self.partition(n, world, frame, at, (key, path))?;
        if let Some(padding) = x.surface_padding {
            let geometry = self.caches.surfaces.resolve(
                (n, padding),
                (key, at),
                &self.tree.frames,
                world,
                self.spec,
                self.caches.regions,
            )?;
            let origin = geometry.origin;
            for (i, p) in geometry.panels {
                self.plan.regions.insert(i, (p, origin));
            }
            for i in geometry.join_nodes {
                self.plan.joined_nodes.insert(i, ());
            }
            self.plan.surface_joins.insert(at, geometry.joins);
        }
        Ok(())
    }

    /// Open the layers the node's paint sits in -- a blend group, a backdrop
    /// blur -- and paint its drop shadows under it. Returns whether a blend
    /// layer opened, and if so whether it ends in a mask.
    fn open_layers(
        &mut self,
        e: &Element,
        contour: &Contour,
        under: Color,
    ) -> Result<Option<bool>, SceneError> {
        let s = &e.style;
        // A mask composites against what the subtree drew, so the subtree
        // needs a layer of its own even when nothing asked to blend.
        let masked = [s.mask.as_ref(), e.extras().alpha_mask.as_deref()]
            .into_iter()
            .flatten()
            .any(|m| !m.is_none());
        let blended = match s.layer {
            Some((mix, opacity)) if !(mix == Mix::Normal && opacity == 1.0) => Some((mix, opacity)),
            _ if masked => Some((Mix::Normal, 1.0)),
            _ => None,
        };
        if let Some((mix, opacity)) = blended {
            self.mark(Layer::Blend { mix, opacity }, empty(), None);
        }
        let blur = s.backdrop_blur.unwrap_or_default();
        if blur > 0.0 {
            // In scene space: the renderer samples the backdrop there.
            self.mark(Layer::Backdrop, contour.world(), contour.world_rect());
            if let Some(p) = self.out.paint.last_mut() {
                p.blur = blur;
            }
        }
        for sh in s
            .shadow
            .iter()
            .flatten()
            .filter(|sh| sh.kind == ShadowKind::Drop)
        {
            self.shadow(sh, contour, under)?;
        }
        Ok(blended.map(|_| masked))
    }

    /// Record `n`'s surface: its outline, hit shapes, semantics and
    /// everything input reads back. Returns its shape's bounds in scene
    /// space and its content extent.
    fn surface(
        &mut self,
        n: &El,
        (key, at): (&crate::Id, usize),
        frame: Frame,
        contour: &Contour,
        (hits, text_geometry): (Hits, Option<Arc<crate::TextGeometry>>),
        (inner, ancestors): (&Ancestors, &Ancestors),
    ) -> Result<(Option<Rect>, Size), SceneError> {
        let e = n.payload();
        let content = self.content_size(n, at, frame);
        let bounds = self
            .caches
            .outlines
            .bounds(contour)?
            .map(|b| b + contour.offset.to_vec2());
        let (hit_path, hit_offset) = self.caches.outlines.rotated(
            (key.clone(), 0),
            &contour.path,
            contour.offset,
            inner.transform,
        )?;
        let hits = if inner.transform == Affine::IDENTITY {
            hits
        } else {
            hits.into_iter()
                .enumerate()
                .map(|(index, (tag, path))| {
                    self.caches
                        .outlines
                        .rotated(
                            (key.clone(), index + 1),
                            &path,
                            contour.offset,
                            inner.transform,
                        )
                        .map(|(path, _)| (tag, path))
                })
                .collect::<Result<_, _>>()?
        };
        self.out.at.insert(key.clone(), self.out.surfaces.len());
        let (mut semantics, semantic_label_implicit) = match (&e.semantics, &e.content) {
            (Some(semantics), Content::Text(text)) if semantics.label.is_none() => {
                let mut semantics = crate::Semantics::clone(semantics);
                semantics.label = Some(text.clone());
                (Some(semantics), true)
            }
            (semantics, _) => (semantics.as_deref().cloned(), false),
        };
        if let (Some(g), Some(semantics)) = (&text_geometry, &mut semantics)
            && let crate::A11y::TextInput { carets, .. } = &mut semantics.role
            && g.lines.len() == 1
        {
            let line = &g.lines[0];
            *carets = g
                .state
                .value
                .char_indices()
                .map(|(b, _)| b)
                .chain([g.state.value.len()])
                .map(|b| line.origin.x + line.carets.x(g.source_to_display(b)))
                .collect();
        }
        self.out.surfaces.push(ResolvedSurface {
            key: key.clone(),
            frame,
            transform: inner.transform,
            bounds: bounds.map(|bounds| inner.transform.transform_rect_bbox(bounds)),
            path: hit_path,
            rect: (inner.transform == Affine::IDENTITY)
                .then_some(contour.rect)
                .flatten(),
            offset: hit_offset,
            topology_changed: contour.changed,
            cursor: inner.cursor,
            tip: e.extras().tip.clone(),
            focusable: e.has(Element::FOCUSABLE),
            captures_wheel: e.has(Element::CAPTURES_WHEEL),
            tracks_pointer: e.has(Element::TRACKS_POINTER),
            pointer_states: e
                .states
                .iter()
                .any(|(st, _)| matches!(st, State::Hover | State::Press)),
            disabled: inner.disabled,
            semantics,
            semantic_label_implicit,
            // An editable field draws its own runs, from the start.
            text_align: if e.extras().editable_text.is_some() {
                crate::TextAlign::Start
            } else {
                e.extras().text_align
            },
            description: e.extras().description.clone(),
            value_description: e.extras().value_description.clone(),
            numeric_step: e.extras().numeric_step,
            text_geometry,
            text_value: match &e.content {
                Content::Text(t) => Some(t.clone()),
                _ => None,
            },
            clip: ancestors.clip,
            clip_path: ancestors.clip_paths.clone(),
            clip_rects: ancestors.clip_rects.clone(),
            parent: ancestors.parent.clone(),
            floating: inner.floating,
            content,
            hits,
        });
        Ok((bounds, content))
    }

    /// Close what [`Walk::node`] opened, innermost first, and paint what
    /// goes over the children: the late stroke, the border ramp; then let a
    /// material weld take the plates it fused.
    fn finish(
        &mut self,
        n: &El,
        at: usize,
        contour: &Contour,
        material: Option<&crate::material_weld::MaterialWeld>,
        bg: Color,
        o: Opened,
    ) -> Result<(), SceneError> {
        let e = n.payload();
        if o.clips {
            self.mark(Layer::Unclip, empty(), None);
        }
        if let Some((stroke_path, stroke_rect, fill, width, clipped)) = o.late_stroke {
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
            self.border_ramp(n, ramp, at, contour, bg, o.border_background)?;
        }
        if let Some(m) = material {
            // Consume only immediate source plates. Text, canvases, children,
            // clips and semantics keep their own authoring and painter order.
            // The plates painted inside this node, so only its entries are
            // scanned, not everything painted before it.
            let start = o.start;
            let all = self.out.paint.len() - start;
            self.out
                .paint
                .extract_if(start.., |p| m.consumes(&p.key, p.layer))
                .for_each(drop);
            if self.out.paint.len() - start != all {
                // What a memo inside painted has been thinned: not copyable.
                for s in self
                    .memo
                    .spans
                    .iter_mut()
                    .filter(|s| s.paint.start >= start)
                {
                    s.closed = false;
                }
            }
        }
        if let Some(masked) = o.blend {
            if masked {
                for (layer, fill) in [
                    (Layer::Mask, e.style.mask.as_ref()),
                    (Layer::AlphaMask, e.extras().alpha_mask.as_deref()),
                ] {
                    if let Some(fill) = fill
                        && let Some(p) =
                            self.push(layer, contour.path.clone(), contour.rect, fill, bg)
                    {
                        p.offset = contour.offset;
                    }
                }
            }
            self.mark(Layer::Unblend, empty(), None);
        }
        if o.alpha_clips {
            self.mark(Layer::Unclip, empty(), None);
        }
        if o.enveloped {
            self.mark(Layer::Unclip, empty(), None);
        }
        if let Some(open) = o.memo {
            self.close(open);
        }
        Ok(())
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
        let size = self.tree.sizes[at];
        self.memo.spans.push(MemoSpan {
            id: m.id,
            reused: m.reused,
            at,
            size,
            paint: self.out.paint.len()..self.out.paint.len(),
            surfaces: self.out.surfaces.len()..self.out.surfaces.len(),
            nested: 0,
            origin: Point::new(frame.x, frame.y),
            path: path.to_owned(),
            under,
            base_y: self.base_y,
            ancestors: ancestors.clone(),
            closed: !self.plan.feeds(at..at + size),
            floats: false,
            generation: self.runs.generation,
        });
        (
            self.memo.spans.len() - 1,
            self.out.deferred.len(),
            self.out.external_welds.len(),
        )
    }

    fn close(&mut self, (j, deferred, welds): (usize, usize, usize)) {
        let (paint, surfaces, nested) = (
            self.out.paint.len(),
            self.out.surfaces.len(),
            self.memo.spans.len(),
        );
        let floats = self.out.deferred.len() != deferred;
        let s = &mut self.memo.spans[j];
        s.paint.end = paint;
        s.surfaces.end = surfaces;
        s.nested = nested - j - 1;
        s.floats = floats;
        s.closed &= !floats && self.out.external_welds.len() == welds;
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
        let Some(prev) = self.memo.prev else {
            return false;
        };
        let Some(j) = prev.memos.iter().position(|s| s.id == id) else {
            return false;
        };
        let old = &prev.memos[j];
        let spans = &prev.memos[j..=j + old.nested];
        let size = self.tree.sizes[at];
        let oldest = spans.iter().map(|s| s.generation).min().unwrap_or(0);
        let age = self.runs.generation.wrapping_sub(oldest);
        if !old.closed
            || old.size != size
            || old.path != path
            || old.under != under
            || old.base_y != self.base_y
            || !old.ancestors.same(ancestors)
            || age > MEMO_AGE
            || self.plan.feeds(at..at + size)
        {
            return false;
        }
        let d = Vec2::new(frame.x - old.origin.x, frame.y - old.origin.y);
        let moved = d != Vec2::ZERO;
        let grid = |v: f64| on_grid(v, self.spec.device_scale);
        // A moved span's own clips move with it; its ancestors' stay put
        // (see `Shift`), so content scrolling under a viewport is copied.
        if moved
            && (!grid(d.x)
                || !grid(d.y)
                || prev.paint[old.paint.clone()]
                    .iter()
                    .any(|p| p.transform != Affine::IDENTITY)
                || prev.surfaces[old.surfaces.clone()]
                    .iter()
                    .any(|s| s.transform != Affine::IDENTITY))
        {
            return false;
        }
        let Some(was) = prev.layout.all().get(old.at..old.at + size) else {
            return false;
        };
        let now = &self.tree.frames[at..at + size];
        if !was
            .iter()
            .zip(now)
            .all(|(o, f)| f.x == o.x + d.x && f.y == o.y + d.y && f.size == o.size)
        {
            return false;
        }
        let (paint, surfaces) = (self.out.paint.len(), self.out.surfaces.len());
        let mut shift = Shift::new(d, ancestors);
        self.out.paint.extend(
            prev.paint[old.paint.clone()]
                .iter()
                .map(|p| shift.painted(p)),
        );
        for s in &prev.surfaces[old.surfaces.clone()] {
            self.out.at.insert(s.key.clone(), self.out.surfaces.len());
            self.out.surfaces.push(shift.surface(s));
        }
        for s in spans {
            let mut s = s.clone();
            s.reused = true;
            s.at = s.at - old.at + at;
            s.paint =
                s.paint.start - old.paint.start + paint..s.paint.end - old.paint.start + paint;
            s.surfaces = s.surfaces.start - old.surfaces.start + surfaces
                ..s.surfaces.end - old.surfaces.start + surfaces;
            s.origin += d;
            self.memo.spans.push(s);
        }
        self.memo.age = self.memo.age.max(age);
        self.i = at + size;
        true
    }

    /// The node's text or canvas draws, clipped to its region when it has
    /// one. Text is ink over `under`, so it hands `under` on as the ground.
    fn content(
        &mut self,
        e: &Element,
        at: usize,
        key: &crate::Id,
        contour: &Contour,
        bg: &mut Color,
        under: Color,
    ) -> Result<(Hits, Option<Arc<crate::TextGeometry>>), SceneError> {
        let mut hits = Vec::new();
        let mut text_geometry = None;
        let shaped = self.plan.regions.contains(at) && !matches!(e.content, Content::None);
        if shaped {
            self.mark_on(Layer::Clip, contour);
        }
        let frame = self.tree.frames[at];
        match &e.content {
            Content::Text(t) => {
                if let Some(edit) = &e.extras().editable_text {
                    text_geometry = Some(self.editable_text(e, t, edit, frame, key, *bg)?);
                } else {
                    *bg = under;
                    self.text(e, t, frame, key, under)?;
                }
            }
            Content::Canvas(c) => {
                // Local to the frame corner, where the canvas draws from.
                let origin = Point::new(frame.x, frame.y);
                let draws = c.draw(frame.size);
                let generation = self.caches.outlines.generation;
                let paths = match self.caches.outlines.canvases.get_mut(key) {
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
                            .collect::<Result<Arc<[_]>, SceneError>>()?;
                        self.caches
                            .outlines
                            .canvases
                            .insert(key.clone(), (draws.clone(), paths.clone(), generation));
                        paths
                    }
                };
                // Hits are local to the surface's offset, the outline's.
                let d = origin - contour.offset;
                for ((k, draw), local) in draws.iter().enumerate().zip(paths.iter()) {
                    let d = d + draw.at.to_vec2();
                    if let Some(tag) = &draw.tag {
                        let hit = if d == Vec2::ZERO {
                            local.clone()
                        } else {
                            let mut p = Path::clone(local);
                            p.translate(d);
                            Arc::new(p)
                        };
                        hits.push((Arc::clone(tag), hit));
                    }
                    // A shadow of a rect or circle blurs analytically.
                    let rect = (draw.blur > 0.0)
                        .then(|| mui_geometry::RoundedRect::of(local))
                        .flatten();
                    if let Some(p) = self.push(Layer::Draw(k), local.clone(), rect, &draw.fill, *bg)
                    {
                        p.width = draw.width;
                        p.cap = draw.cap;
                        p.join = draw.join;
                        p.blur = draw.blur;
                        p.offset = origin + draw.at.to_vec2();
                    }
                }
            }
            Content::None => {}
        }
        if shaped {
            self.mark(Layer::Unclip, empty(), None);
        }
        Ok((hits, text_geometry))
    }

    /// A label: one run per line, each on its own baseline.
    fn text(
        &mut self,
        e: &Element,
        t: &str,
        frame: Frame,
        key: &crate::Id,
        under: Color,
    ) -> Result<(), SceneError> {
        let spec = self.spec;
        let th = &spec.theme;
        let size = e.text_px(th);
        // Text's own fill is its ink, not a box behind it.
        let ink = match &e.style.fill {
            Some(f) if !f.is_none() => f.clone(),
            _ => Fill::Role(crate::Role::Ink),
        };
        let face = Face::of(e, th);
        // A stand-in-measured value is one line in the box its stand-in set.
        let room = if e.extras().measure_as.is_some() {
            f64::INFINITY
        } else {
            frame.size.width
        };
        let lines = self.runs.lines(t, face, room, e.lines);
        let fonts = self.runs.fonts_for(face);
        let font_coords = self.runs.coords(&fonts, face);
        // An icon font's Private Use Area glyphs have no stems worth
        // snapping, and each icon size would hold a hinting instance of its own.
        let hint = self.runs.settled(key, &font_coords) && !t.chars().all(private_use);
        let n = lines.len();
        let base = self.base_y;
        // A wrapped paragraph paints slices of its one run, at its metrics.
        let (widths, sliced) = if n > 1 {
            self.runs.line_glyphs(t, face, room, e.lines)
        } else {
            Default::default()
        };
        let share = e.extras().text_align.share();
        let para = if sliced.is_empty() {
            None
        } else {
            self.runs
                .run(t, face)?
                .map(|r| (r.ascent, r.descent, r.line_height))
        };
        for (li, line) in lines.iter().enumerate() {
            let run = match (sliced.get(li), widths.get(li), para) {
                (Some(glyphs), Some(&advance), Some((ascent, descent, line_height))) => LineRun {
                    glyphs: glyphs.clone(),
                    advance,
                    ascent,
                    descent,
                    line_height,
                },
                _ => match self.runs.run(line, face)? {
                    Some(r) => LineRun {
                        glyphs: r.glyphs.clone(),
                        advance: r.advance,
                        ascent: r.ascent,
                        descent: r.descent,
                        line_height: r.line_height,
                    },
                    None => break,
                },
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
            // The box's spare width, shared out by the node's alignment:
            // none for Start, so a start-aligned label stays where it was.
            let dx = if share == 0.0 {
                0.0
            } else {
                (frame.size.width - run.advance) * share
            };
            let origin = Point::new(
                snap(frame.x + dx, self.spec.device_scale),
                snap(dy, self.spec.device_scale),
            );
            let text = Text {
                fonts: fonts.clone(),
                size: size as f32,
                origin,
                glyphs: run.glyphs,
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
                let f = self.tree.frames[child_at];
                right = right.max(f.right());
                bottom = bottom.max(f.bottom());
            }
            child_at += self.tree.sizes[child_at];
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
            let bar_key = crate::Id::runtime(&bar::bar_key(&key, vertical));
            self.key = bar_key.clone();
            self.push(Layer::Fill, rr.path(), Some(rr), &ink, under);
            let hit_path = Arc::new(hit.path());
            let (hit_path, hit_offset) = self.caches.outlines.rotated(
                (bar_key.clone(), 0),
                &hit_path,
                Point::ZERO,
                inner.transform,
            )?;
            self.out.at.insert(bar_key.clone(), self.out.surfaces.len());
            self.out.surfaces.push(ResolvedSurface {
                key: bar_key,
                frame: strip,
                transform: inner.transform,
                bounds: Some(inner.transform.transform_rect_bbox(hit.bounds())),
                path: hit_path,
                rect: (inner.transform == Affine::IDENTITY).then_some(hit),
                offset: hit_offset,
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
                text_align: crate::TextAlign::Start,
                text_value: None,
                description: None,
                value_description: None,
                numeric_step: None,
                text_geometry: None,
                clip: inner.clip,
                clip_path: inner.clip_paths.clone(),
                clip_rects: inner.clip_rects.clone(),
                parent: None,
                floating: inner.floating,
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
        b: Option<Rect>,
        contour: &Contour,
        frame: Frame,
        inner: &mut Ancestors,
    ) -> Result<(), SceneError> {
        let b = self
            .transform
            .transform_rect_bbox(b.unwrap_or_else(|| bounds(frame, self.spec.device_scale)));
        let rects = inner.clip_rects.as_deref().unwrap_or_default();
        inner.clip_rects = Some(rects.iter().copied().chain(std::iter::once(b)).collect());
        self.mark_on(Layer::Clip, contour);
        inner.clip = Some(inner.clip.map_or(b, |c| meet(c, b)));
        // Every exact outline in one allocation all descendants share.
        // `clip` remains the rectangular fast path used by existing input
        // adapters; rounded or welded corners can now be tested without
        // tessellating during each pointer query. The paths themselves are
        // the ancestors' own outlines: a nested clip adds a pointer each.
        let outer = inner.clip_paths.as_deref().unwrap_or_default();
        let own = self.caches.outlines.rotated(
            (self.key.clone(), 0),
            &contour.path,
            contour.offset,
            self.transform,
        )?;
        inner.clip_paths = Some(outer.iter().cloned().chain(std::iter::once(own)).collect());
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
        // Underlays use normal ancestor clipping and surface order, but
        // precede all foreground siblings regardless of declaration order.
        let children_at = self.i;
        let mut at = children_at;
        for (j, child) in n.children().iter().enumerate() {
            if child.is_underlay() {
                self.i = at;
                path.truncate(mark);
                super::push_index(path, j);
                self.node(child, path, bg, inner)?;
            }
            at += self.tree.sizes[at];
        }
        self.i = children_at;
        let mut sticky = Vec::new();
        let mut overlays = Vec::new();
        for (j, c) in n.children().iter().enumerate() {
            self.base_y = bases.get(j).and_then(|b| b.map(|(y, _)| y));
            path.truncate(mark);
            super::push_index(path, j);
            if c.is_underlay() || c.payload().carve.is_some() {
                // Underlays already painted; carves shaped the outline.
                self.i += self.tree.sizes[self.i];
                continue;
            }
            if c.is_sticky() && !c.is_float() {
                // Pinned over the siblings that scroll under it, so it paints
                // after them -- but inside this node's clip, unlike a float.
                sticky.push((self.i, c, path.clone(), self.base_y));
                self.i += self.tree.sizes[self.i];
                continue;
            }
            if c.is_overlay() {
                overlays.push((self.i, c, path.clone()));
                self.i += self.tree.sizes[self.i];
                continue;
            }
            if c.is_float() {
                self.out.deferred.push(Deferred {
                    at: self.i,
                    node: c,
                    path: path.clone(),
                    under: bg,
                    ancestors: Ancestors {
                        transform: inner.transform,
                        parent: inner.parent.clone(),
                        cursor: inner.cursor,
                        disabled: inner.disabled,
                        floating: inner.floating,
                        ..Ancestors::default()
                    },
                });
                self.i += self.tree.sizes[self.i];
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
        for (at2, c, mut p) in overlays {
            self.i = at2;
            self.base_y = None;
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
        if !n.payload().has(Element::BASELINE) {
            return Ok(bases);
        }
        let spec = self.spec;
        let th = &spec.theme;
        let mut at2 = at + 1;
        for c in n.children() {
            let f = self.tree.frames[at2];
            at2 += self.tree.sizes[at2];
            let own = match &c.payload().content {
                Content::Text(t) => {
                    let face = Face::of(c.payload(), th);
                    let lines = self.runs.lines(t, face, f.size.width, c.payload().lines);
                    // Wrapped lines stack at the paragraph's metrics.
                    let first: &str = if lines.len() > 1 { t } else { &lines[0] };
                    self.runs.run(first, face)?.map(|r| {
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
/// One painted line's glyphs and the metrics it stacks at.
struct LineRun {
    glyphs: Arc<[super::TextGlyph]>,
    advance: f64,
    ascent: f64,
    descent: f64,
    line_height: f64,
}

/// Moves a copied span's paint and surfaces by `d`. The clips of
/// `outer`, the span's ancestors, stay where they are: a scroller's
/// viewport does not move with its content.
struct Shift<'o> {
    d: Vec2,
    outer: &'o Ancestors,
    lists: Vec<(*const [super::PlacedPath], Clips)>,
}

/// Two clip rectangles' overlap, as [`Walk::clip`] narrows them.
fn meet(a: Rect, b: Rect) -> Rect {
    Rect::new(
        a.x0.max(b.x0),
        a.y0.max(b.y0),
        a.x1.min(b.x1),
        a.y1.min(b.y1),
    )
}

impl<'o> Shift<'o> {
    fn new(d: Vec2, outer: &'o Ancestors) -> Self {
        Self {
            d,
            outer,
            lists: Vec::new(),
        }
    }
    fn painted(&mut self, p: &Painted) -> Painted {
        let mut p = p.clone();
        if self.d != Vec2::ZERO {
            match &mut p.text {
                Some(t) => t.origin += self.d,
                // A pathless marker (an `Unclip`, a `Blend`) stands nowhere.
                None if p.path.commands.is_empty() => {}
                None => p.offset += self.d,
            }
        }
        p
    }
    fn surface(&mut self, s: &ResolvedSurface) -> ResolvedSurface {
        let mut s = s.clone();
        let d = self.d;
        if d == Vec2::ZERO {
            return s;
        }
        s.frame.x += d.x;
        s.frame.y += d.y;
        s.bounds = s.bounds.map(|b| b + d);
        s.offset += d;
        // The span's own clips are those past its ancestors'; only they move.
        let kept = self.outer.clip_rects.as_ref().map_or(0, |r| r.len());
        let (Some(list), Some(rects)) = (&s.clip_path, &s.clip_rects) else {
            return s;
        };
        if rects.len() <= kept {
            return s;
        }
        let rects: Arc<[Rect]> = rects
            .iter()
            .enumerate()
            .map(|(i, r)| if i < kept { *r } else { *r + d })
            .collect();
        s.clip = rects.iter().copied().reduce(meet);
        s.clip_rects = Some(rects);
        let at = Arc::as_ptr(list);
        s.clip_path = Some(
            if let Some((_, moved)) = self.lists.iter().find(|(p, _)| std::ptr::eq(*p, at)) {
                moved.clone()
            } else {
                let moved: Clips = list
                    .iter()
                    .enumerate()
                    .map(|(i, (p, o))| (p.clone(), if i < kept { *o } else { *o + d }))
                    .collect();
                self.lists.push((at, moved.clone()));
                moved
            },
        );
        s
    }
}

/// A Private Use Area code point: where icon fonts (Phosphor, Material
/// Symbols) put their glyphs.
fn private_use(c: char) -> bool {
    matches!(c, '\u{E000}'..='\u{F8FF}' | '\u{F0000}'..='\u{10FFFF}')
}

#[cfg(test)]
mod tests;
