//! A node's own paint: fill, shells, shadows, stroke and border ramp, and
//! the one place an entry is pushed.
use mui_geometry::{inset_path, BooleanOp, Bounds, Path, Point, RoundedRect};

use super::outline::Contour;
use super::{find, Layer, Painted, SceneError, Walk};
use crate::material_weld::MaterialWeld;
use crate::regions::Operation;
use crate::{BorderRamp, Color, Content, El, Element, Fill, Paint, Radius, Shadow, ShadowKind};
use crate::{Stroke, Style};

/// What a structural entry paints: nothing a renderer looks at.
const CLEAR: Color = Color::oklcha(0.0, 0.0, 0.0, 0.0);

/// A stroke held back until the node's children have painted: path, rect,
/// paint and width.
pub(super) type LateStroke = (Path, Option<RoundedRect>, Fill, f64);

/// The colour a painted entry leaves for what paints on it.
fn solid(p: Option<&mut Painted>, or: Color) -> Color {
    p.map_or(or, |p| p.paint.solid())
}

impl Walk<'_> {
    /// The node's fill -- or its material weld's image -- and the colour it
    /// leaves for what paints on it.
    pub(super) fn fill(
        &mut self,
        e: &Element,
        material: Option<&MaterialWeld>,
        contour: &Contour,
        under: Color,
    ) -> Color {
        match material {
            Some(MaterialWeld {
                external: Some(external),
                image_rect,
                ..
            }) => {
                self.external_welds
                    .insert(self.key.clone(), external.clone());
                self.paint.push(Painted {
                    key: self.key.clone(),
                    layer: Layer::External,
                    path: image_rect.path(),
                    paint: Paint::Solid(under),
                    rect: Some(*image_rect),
                    width: 0.0,
                    blur: 0.0,
                    text: None,
                });
                under
            }
            Some(m) => solid(
                self.push(
                    Layer::Fill,
                    m.image_rect.path(),
                    Some(m.image_rect),
                    &m.image_fill,
                    under,
                ),
                under,
            ),
            // Text's own fill is its ink, not a box behind it: never pushed,
            // though its colour still grounds the shells as before.
            None if matches!(e.content, Content::Text(_)) => e
                .style
                .fill
                .paint(&self.spec.theme.palette, under)
                .map_or(under, |p| p.solid()),
            // ponytail: `Painted` owns its path, so a filled node copies its
            // outline once. `Arc<Path>` there is the upgrade -- an API break
            // for every renderer.
            None => solid(
                self.push(
                    Layer::Fill,
                    contour.path.clone(),
                    contour.rect,
                    &e.style.fill,
                    under,
                ),
                under,
            ),
        }
    }

    /// Every shell, each a parallel inset of the one before; returns the
    /// colour the innermost one leaves.
    pub(super) fn shells(
        &mut self,
        s: &Style,
        contour: &mut Contour,
        mut bg: Color,
    ) -> Result<Color, SceneError> {
        // The shell before this one: an analytic rect insets as one, and
        // only a path shell needs the previous path kept.
        let (mut cur, mut cur_rect) = (None::<Path>, contour.rect);
        for (i, (d, f)) in s.shells.iter().enumerate() {
            let d = d.resolve(self.spec.theme.spacing);
            if !(d.is_finite() && d >= 0.0) {
                return Err(SceneError::InvalidRadius);
            }
            let shell = match cur_rect {
                Some(rr) => {
                    let i2 = rr.inset(d)?;
                    contour.changed |= i2.corner_collapsed;
                    let Some(child) = i2.shape else { break };
                    cur_rect = Some(child);
                    child.path()
                }
                None => {
                    let i2 =
                        inset_path(cur.as_ref().unwrap_or(&contour.path), d, self.spec.offsets)?;
                    contour.changed |= i2.counts_changed;
                    cur = Some(i2.path.clone());
                    i2.path
                }
            };
            bg = solid(self.push(Layer::Shell(i), shell, cur_rect, f, bg), bg);
        }
        Ok(bg)
    }

    /// Inset shadows, inside the shape and over everything it has painted so
    /// far: the inverse blur is opaque *outside* its rectangle, so the
    /// outline is what keeps it in the box.
    pub(super) fn inset_shadows(
        &mut self,
        s: &Style,
        contour: &Contour,
        bg: Color,
    ) -> Result<(), SceneError> {
        if !s.shadow.iter().any(|sh| sh.kind == ShadowKind::Inset) {
            return Ok(());
        }
        self.mark(Layer::Clip, contour.path.clone(), contour.rect);
        for sh in s.shadow.iter().filter(|sh| sh.kind == ShadowKind::Inset) {
            self.shadow(sh, contour, bg)?;
        }
        self.mark(Layer::Unclip, Path::default(), None);
        Ok(())
    }

    /// One shadow of a node, offset and spread off the node's own outline.
    ///
    /// A rounded rect blurs analytically, so that is what the entry carries
    /// whenever the outline is one. A welded outline is not, and becomes one
    /// blurred rect per welded child instead.
    pub(super) fn shadow(
        &mut self,
        sh: &Shadow,
        contour: &Contour,
        under: Color,
    ) -> Result<(), SceneError> {
        let d = Point::new(sh.dx, sh.dy);
        // CSS spread: the drop grows, the inset shrinks, and the radius
        // follows so the corner keeps its shape.
        let grow = match sh.kind {
            ShadowKind::Drop => sh.spread,
            ShadowKind::Inset => -sh.spread,
        };
        let moved = |r: RoundedRect| {
            let b = r.bounds();
            RoundedRect::new(
                Bounds::new(
                    b.min.x + d.x - grow,
                    b.min.y + d.y - grow,
                    b.max.x + d.x + grow,
                    b.max.y + d.y + grow,
                ),
                (r.radius() + grow).max(0.0),
            )
        };
        // ponytail: a welded shadow is the union of the children's blurs,
        // not the blur of the union -- each child rect keeps the convex
        // radius, so the seams are rounded where the welded outline is
        // straight or concave, and overlapping children over-composite
        // there. A blur filter layer is the upgrade.
        let rects = match &contour.rect {
            Some(r) => std::slice::from_ref(r),
            None if !contour.shadow_rects.is_empty() => &contour.shadow_rects[..],
            // ponytail: no analytic rect and no welds -- the shape travels
            // as a path, and the spread with it is dropped.
            None => {
                if let Some(p) = self.push(
                    Layer::Shadow(sh.kind),
                    contour.path.rigid_transform(d, 0.0)?,
                    None,
                    &sh.fill,
                    under,
                ) {
                    p.blur = sh.blur;
                }
                return Ok(());
            }
        };
        for &r in rects {
            let r = moved(r)?;
            if let Some(p) = self.push(Layer::Shadow(sh.kind), r.path(), Some(r), &sh.fill, under) {
                p.blur = sh.blur;
            }
        }
        Ok(())
    }

    /// The node's stroke. Painted now, or handed back when it has to paint
    /// after the children.
    ///
    /// A union parent is one continuous outline, but its children paint
    /// after the parent. Keep the stroke until the subtree is complete so a
    /// child fill cannot erase the shared outer border. Ordinary nodes
    /// retain the historical ordering (stroke before their content).
    pub(super) fn stroke(
        &mut self,
        st: &Stroke,
        e: &Element,
        contour: &Contour,
        bg: Color,
    ) -> Result<Option<LateStroke>, SceneError> {
        let w = st.width.unwrap_or(self.spec.theme.stroke_width);
        if !(w.is_finite() && w >= 0.0) {
            return Err(SceneError::InvalidRadius);
        }
        if let (crate::BorderAlign::Inside, Some(rr)) = (e.border_align, contour.rect) {
            let Some(rr) = rr.inset(w / 2.)?.shape else {
                return Ok(Some((contour.path.clone(), None, st.fill.clone(), 0.)));
            };
            if e.style.union {
                return Ok(Some((rr.path(), Some(rr), st.fill.clone(), w)));
            }
            if let Some(p) = self.push(Layer::Stroke, rr.path(), Some(rr), &st.fill, bg) {
                p.width = w;
            }
            return Ok(None);
        }
        let band = mui_geometry::border_geometry(
            &contour.path,
            mui_geometry::WidthProfile::uniform(w),
            e.border_align,
            self.spec.offsets,
            self.spec.geometry,
        )?
        .band;
        Ok(Some((band, None, st.fill.clone(), 0.)))
    }

    /// A border ramp's band, painted after the children. One with tabs moves
    /// to just after the node's fill, under its shells and everything else.
    pub(super) fn border_ramp(
        &mut self,
        n: &El,
        ramp: &BorderRamp,
        at: usize,
        contour: &Contour,
        bg: Color,
        border_background: usize,
    ) -> Result<(), SceneError> {
        let th = self.spec.theme;
        let named_frame = |id: &mui_layout::Id| -> Result<_, SceneError> {
            if let Some(frame) = self.ramp_frames.get(&(at, id.clone())) {
                return Ok(*frame);
            }
            let index = find(n, id.as_str(), at, &self.sizes).ok_or(
                mui_geometry::Error::InvalidOptions("border ramp descendant missing"),
            )?;
            Ok(self.frames[index])
        };
        let anchor = match self.ramp_anchors.get(&at) {
            Some(frame) => *frame,
            None => match &ramp.anchor {
                None => self.frames[at],
                Some(id) => named_frame(id)?,
            },
        };
        if anchor.size.width <= 0.0 {
            return Err(
                mui_geometry::Error::InvalidOptions("border ramp anchor has no width").into(),
            );
        }
        let mut sweep = ramp.clone();
        if ramp.align == crate::BorderAlign::Center {
            sweep.from.1 *= 0.5;
            sweep.to.1 *= 0.5;
        }
        let mut band = self.borders.band(
            &self.key,
            &contour.path,
            &sweep,
            anchor,
            0.1 / self.spec.device_scale.unwrap_or(1.0),
        )?;
        // Extend the same material into the tabs and their concave shoulders.
        // The final welded outline supplies all corners through the clip.
        let shoulder = match n.payload().style.radius {
            Radius::Pair(_, concave) => concave,
            Radius::Scale(k) => th.corners.concave * k,
            _ => th.corners.concave,
        };
        crate::border_ramp::decorate(&mut band, ramp, anchor, shoulder, named_frame)?;
        if ramp.align == crate::BorderAlign::Outside {
            let merged = self.cached_region((at, 7), Operation::Sweep(band))?;
            band = mui_geometry::boolean_paths(
                &merged,
                &contour.path,
                BooleanOp::Difference,
                self.spec.offsets,
                self.spec.geometry,
            )?;
        }
        let Some(bounds) = Bounds::from_points(band.flatten(0.1, 250_000)?.concat()) else {
            return Ok(());
        };
        let start = self.paint.len();
        if ramp.align == crate::BorderAlign::Inside {
            self.mark(Layer::Clip, contour.path.clone(), contour.rect);
        }
        self.push(Layer::Stroke, band, None, &ramp.fill(anchor, bounds), bg);
        if ramp.align == crate::BorderAlign::Inside {
            self.mark(Layer::Unclip, Path::default(), None);
        }
        if !ramp.tabs.is_empty() {
            let paint: Vec<_> = self.paint.drain(start..).collect();
            self.paint
                .splice(border_background..border_background, paint);
        }
        Ok(())
    }

    /// A structural entry -- a clip, a blend, or the one closing it -- whose
    /// paint is meaningless.
    pub(super) fn mark(&mut self, layer: Layer, path: Path, rect: Option<RoundedRect>) {
        self.paint.push(Painted {
            key: self.key.clone(),
            layer,
            path,
            paint: Paint::Solid(CLEAR),
            rect,
            width: 0.0,
            blur: 0.0,
            text: None,
        });
    }

    pub(super) fn push(
        &mut self,
        layer: Layer,
        path: Path,
        rect: Option<RoundedRect>,
        fill: &Fill,
        under: Color,
    ) -> Option<&mut Painted> {
        let paint = fill.paint(&self.spec.theme.palette, under)?;
        self.paint.push(Painted {
            key: self.key.clone(),
            layer,
            path,
            paint,
            rect,
            width: 0.0,
            blur: 0.0,
            text: None,
        });
        self.paint.last_mut()
    }
}

#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::prelude::*;
    use crate::{Paint, ShadowKind};

    /// An inset shadow paints over the fill and inside the outline, which
    /// is the whole difference from a drop shadow: same call, opposite side.
    #[test]
    fn an_inset_shadow_paints_over_the_fill_and_clipped_to_the_outline() {
        let root = leaf(40., 40.)
            .radius(8.)
            .fill(Role::Surface)
            .shadow(Shadow::soft(6.))
            .shadow(Shadow::inset(4.))
            .id("box");
        let s = resolve_scene(&SceneSpec::new(root).offered(Size::new(40., 40.))).unwrap();
        let at = |l: Layer| s.paint.iter().position(|p| p.layer == l).expect("layer");
        let (drop, fill) = (at(Layer::Shadow(ShadowKind::Drop)), at(Layer::Fill));
        let inset = at(Layer::Shadow(ShadowKind::Inset));
        assert!(drop < fill, "the drop shadow is over the fill");
        assert!(fill < at(Layer::Clip) && at(Layer::Clip) < inset);
        assert!(
            inset < at(Layer::Unclip),
            "the inset shadow escapes the box"
        );
        assert_eq!(s.paint[inset].blur, 4.);
    }

    /// A welded outline has no analytic rounded rect, so its shadow is one
    /// blurred rect per welded child instead of a single dropped entry.
    #[test]
    fn a_welded_shadow_is_one_blurred_rect_per_child() {
        let root = row([leaf(20., 20.).id("a"), leaf(20., 40.).id("b")])
            .union(Role::Surface)
            .shadow(Shadow::soft(12.))
            .id("weld");
        let s = resolve_scene(&SceneSpec::new(root).offered(Size::new(40., 40.))).unwrap();
        let sh: Vec<_> = s
            .paint
            .iter()
            .filter(|p| matches!(p.layer, Layer::Shadow(_)))
            .collect();
        assert_eq!(sh.len(), 2, "one blurred rect per welded child");
        for (p, k) in sh.iter().zip(["a", "b"]) {
            assert_eq!(p.blur, 12.);
            let r = p.rect.expect("a rect the renderer can blur").bounds();
            let child = s.surface(k).unwrap().rect.unwrap().bounds();
            assert!((r.min.x - child.min.x).abs() < 1e-9);
            assert!((r.min.y - child.min.y - Shadow::soft(12.).dy).abs() < 1e-9);
        }
    }

    #[test]
    fn shells_are_parallel_and_paint_in_z_order() {
        let s = resolve_scene(&welded_tab()).unwrap();
        let tab = s.surface("tab").unwrap().rect.unwrap();
        let shell = s.paint.iter().find(|p| p.layer == Layer::Shell(0)).unwrap();
        let r = shell.rect.unwrap();
        assert!((tab.radius() - r.radius() - 12.).abs() < 1e-9);
        assert!((r.bounds().min.x - tab.bounds().min.x - 12.).abs() < 1e-9);
        let layers: Vec<_> = s.paint.iter().map(|p| (&*p.key, p.layer)).collect();
        assert_eq!(
            layers,
            [
                ("root", Layer::Fill),
                ("root", Layer::Clip),
                ("tab", Layer::Shell(0)),
                ("root", Layer::Unclip)
            ]
        );
        assert!(
            s.surface("/0/1").is_some(),
            "unnamed nodes are keyed by path"
        );
    }

    #[test]
    fn welded_stroke_paints_after_child_fills() {
        let root = row([leaf(20., 20.).fill(Role::Primary).id("child")])
            .union(Role::Surface)
            .stroke(Role::Ink)
            .stroke_width(2.)
            .id("weld");
        let s = resolve_scene(&SceneSpec::new(root).offered(Size::new(20., 20.))).unwrap();
        let order: Vec<_> = s.paint.iter().map(|p| (&*p.key, p.layer)).collect();
        let at = |key: &str, layer: Layer| order.iter().position(|x| *x == (key, layer)).unwrap();
        assert!(
            at("weld", Layer::Fill) < at("child", Layer::Fill)
                && at("child", Layer::Fill) < at("weld", Layer::Stroke),
            "welded border was painted under its child: {order:?}"
        );
    }

    #[test]
    fn a_shell_on_a_weld_follows_the_concave_outline() {
        let mut sp = welded_tab();
        sp.root = sp.root.shell(6., Role::Field);
        let r = resolve_scene(&sp).unwrap();
        let outer = r.surface("root").unwrap();
        assert!(outer.rect.is_none());
        let inner = &r
            .paint
            .iter()
            .find(|p| p.layer == Layer::Shell(0))
            .unwrap()
            .path;
        let oc = outer.path.flatten(0.1, 20_000).unwrap();
        let ic = inner.flatten(0.1, 20_000).unwrap();
        let mut min = f64::INFINITY;
        for p in ic.iter().flatten().step_by(7) {
            min = min.min(mui_geometry::boundary_distance(*p, &oc));
        }
        assert!((min - 6.0).abs() < 0.35, "measured inset={min}");
    }

    #[test]
    fn roles_resolve_against_the_palette_and_ink_reads_on_its_ground() {
        let root = column([text("hi").id("t")]).fill(Role::Primary).id("card");
        let mut sp = SceneSpec::new(root);
        sp.font = Some(Font::new(epaint_default_fonts::HACK_REGULAR).unwrap());
        let s = resolve_scene(&sp).unwrap();
        let th = Theme::default();
        let card = &s.paint[0];
        assert_eq!(card.paint, Paint::Solid(th.palette.primary()));
        let ink = s.paint.iter().find(|p| p.layer == Layer::Text).unwrap();
        assert_eq!(ink.paint, Paint::Solid(th.palette.on(th.palette.primary())));
        assert!(s.layout.frame("t").unwrap().size.width > 10.);
    }

    #[test]
    fn a_stroke_paints_inside_the_frame_it_was_given() {
        let root = overlay([leaf(20., 20.).radius(0.).stroke(Role::Ink).id("k")]);
        let s = resolve_scene(&SceneSpec::new(root).offered(Size::new(20., 20.))).unwrap();
        let st = s.paint.iter().find(|p| p.layer == Layer::Stroke).unwrap();
        let b = Bounds::from_points(st.path.flatten(0.01, 100_000).unwrap().concat()).unwrap();
        let w = st.width / 2.0;
        let f = s.layout.frame("k").unwrap();
        // The painted band is the path grown by half the width; inside means
        // that band is exactly the frame.
        assert!((b.min.x - w - f.x).abs() < 1e-9, "{b:?} {f:?} {w}");
        assert!((b.max.y + w - f.bottom()).abs() < 1e-9, "{b:?} {f:?} {w}");
    }
}
