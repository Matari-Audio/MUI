//! One node of the walk: its outline, paint, surface and children.
use std::fmt::Write as _;
use std::sync::Arc;

use mui_geometry::{Bounds, Path, Point};
use mui_layout::{Frame, Size};

use super::outline::Contour;
use super::text::Face;
use super::{bounds, snap, Ancestors, Deferred, Layer, ResolvedSurface, SceneError, Text, Walk};
use crate::{Color, Content, El, Element, Fill, Mix, ShadowKind};

/// A canvas's tagged draws: the surface's hit shapes.
type Hits = Vec<(Arc<str>, Path)>;

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
        // ponytail: one `Arc<str>` per node per frame, cloned four times
        // instead of four heap copies; interning across frames is the upgrade.
        let key: Arc<str> = n.key().map_or_else(|| Arc::from(path.as_str()), Arc::from);
        let e = n.payload();
        let s = &e.style;
        let mut inner = ancestors.clone();
        inner.cursor = s.cursor.or(ancestors.cursor);
        // A switched-off card switches off what it contains: nothing inside
        // it may be reached while its own frame cannot be.
        inner.disabled = ancestors.disabled || e.disabled;
        if (e.inset_surface.is_some() && !self.regions.contains_key(&at))
            || (e.border_join.is_some() && !self.joined_nodes.contains(&at))
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

        self.partition(n, &contour.path, frame, at)?;
        if e.surface_padding.is_some() {
            let geometry =
                self.surface_cache
                    .resolve(n, at, &self.frames, &contour.path, self.spec)?;
            self.regions.extend(geometry.panels);
            self.joined_nodes.extend(geometry.join_nodes);
            self.surface_joins.insert(at, geometry.joins);
        }
        self.key = key.clone();
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
            self.mark(Layer::Blend { mix, opacity }, Path::default(), None);
        }
        for sh in s.shadow.iter().filter(|sh| sh.kind == ShadowKind::Drop) {
            self.shadow(sh, &contour, under)?;
        }
        let bg = self.fill(e, material.as_ref(), &contour, under);
        let border_background = self.paint.len();
        let mut bg = self.shells(s, &mut contour, bg)?;
        self.inset_shadows(s, &contour, bg)?;
        let late_stroke = match &s.stroke {
            Some(st) if material.is_none() && e.border_ramp.is_none() => {
                self.stroke(st, e, &contour, bg)?
            }
            _ => None,
        };
        let hits = self.content(e, at, &key, &contour, &mut bg, under)?;
        let content = self.content_size(n, at, frame);

        let surface = self.surfaces.len();
        self.at.insert(key.clone(), surface);
        let (semantics, semantic_label_implicit) = match (&e.semantics, &e.content) {
            (Some(semantics), Content::Text(text)) if semantics.label.is_none() => {
                let mut semantics = semantics.clone();
                semantics.label = Some(text.clone());
                (Some(semantics), true)
            }
            (semantics, _) => (semantics.clone(), false),
        };
        self.surfaces.push(ResolvedSurface {
            key: key.clone(),
            frame,
            bounds: match contour.rect {
                // A rounded rectangle already knows its bounds; only a
                // welded outline has to be flattened to find them.
                Some(r) => Some(r.bounds()),
                None => Bounds::from_points(contour.path.flatten(0.5, 100_000)?.concat()),
            },
            // Filled in at the end of this node, from the outline itself.
            path: Path::default(),
            rect: contour.rect,
            topology_changed: contour.changed,
            cursor: inner.cursor,
            tip: e.tip.clone(),
            focusable: e.focusable,
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
        let clips = n.is_clip() || s.union || e.inside.is_some() || self.regions.contains_key(&at);
        if clips {
            let image = material.as_ref().map(|m| m.image_rect.bounds());
            self.clip(image, &contour, frame, &mut inner)?;
        }
        self.children(n, at, path, bg, &inner)?;

        // Everything from here on closes this node, whatever its children
        // left the key at.
        self.key = key;
        if clips {
            self.mark(Layer::Unclip, Path::default(), None);
        }
        if let Some((stroke_path, stroke_rect, fill, width)) = late_stroke {
            if let Some(p) = self.push(Layer::Stroke, stroke_path, stroke_rect, &fill, bg) {
                p.width = width;
            }
        }
        if let Some(ramp) = &e.border_ramp {
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
            self.paint
                .extend(own.into_iter().filter(|p| !m.consumes(&p.key, p.layer)));
        }
        if blended.is_some() {
            if masked {
                self.push(Layer::Mask, contour.path.clone(), contour.rect, &s.mask, bg);
            }
            self.mark(Layer::Unblend, Path::default(), None);
        }
        if enveloped {
            self.mark(Layer::Unclip, Path::default(), None);
        }
        // The outline's last use, so the surface takes it instead of a copy.
        self.surfaces[surface].path = contour.path;
        Ok(())
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
            self.mark(Layer::Clip, contour.path.clone(), contour.rect);
        }
        let frame = self.frames[at];
        match &e.content {
            Content::Text(t) => {
                *bg = under;
                self.text(e, t, frame, key, under)?;
            }
            Content::Canvas(c) => {
                let origin = Point::new(frame.x, frame.y);
                for (k, d) in (c.0)(frame.size).iter().enumerate() {
                    let moved = d.path.rigid_transform(origin, 0.0)?;
                    if let Some(tag) = &d.tag {
                        hits.push((Arc::clone(tag), moved.clone()));
                    }
                    if let Some(p) = self.push(Layer::Draw(k), moved, None, &d.fill, *bg) {
                        p.width = d.width;
                    }
                }
            }
            Content::None => {}
        }
        if shaped {
            self.mark(Layer::Unclip, Path::default(), None);
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
        let coords = font_coords
            .first()
            .cloned()
            .unwrap_or_else(|| Arc::from(&[][..]));
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
                coords: coords.clone(),
                font_coords: font_coords.clone(),
                hint,
            };
            if let Some(p) = self.push(Layer::Text, Path::default(), None, &ink, under) {
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

    /// Clip the subtree to the node's outline, and hand the descendants
    /// that clip both as a rectangle and as the exact path.
    fn clip(
        &mut self,
        image: Option<Bounds>,
        contour: &Contour,
        frame: Frame,
        inner: &mut Ancestors,
    ) -> Result<(), SceneError> {
        let b = match image {
            Some(b) => b,
            None => Bounds::from_points(contour.path.flatten(0.5, 100_000)?.concat())
                .unwrap_or_else(|| bounds(frame, self.spec.device_scale)),
        };
        let b = inner.clip.map_or(b, |c| {
            Bounds::new(
                b.min.x.max(c.min.x),
                b.min.y.max(c.min.y),
                b.max.x.min(c.max.x),
                b.max.y.min(c.max.y),
            )
        });
        self.mark(Layer::Clip, contour.path.clone(), contour.rect);
        inner.clip = Some(b);
        // Keep every exact outline in one shared allocation for all
        // descendants. `clip` remains the rectangular fast path used by
        // existing input adapters; rounded or welded corners can now be
        // tested without tessellating during each pointer query.
        // ponytail: a nested clip copies its clipping ancestors' paths,
        // O(clip depth) per clipping node; `Arc<[Arc<Path>]>` is the
        // upgrade, and it changes mui-input's `&[Path]` clip API too.
        let mut paths = inner
            .clip_paths
            .as_deref()
            .map_or_else(Vec::new, |paths| paths.to_vec());
        paths.push(contour.path.clone());
        inner.clip_paths = Some(Arc::from(paths.into_boxed_slice()));
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
            let _ = write!(path, "/{j}");
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

#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::prelude::*;
    use crate::Paint;

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
        assert!(s
            .paint
            .iter()
            .any(|p| &*p.key == "curve" && p.layer == Layer::Draw(0) && p.width == 2.));
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
        assert!(paths.iter().all(|p| {
            p.commands
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
        assert_eq!(top, 30., "moved into the node's frame");
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
                let b = s.surface(k).unwrap().rect.unwrap().bounds();
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
}
