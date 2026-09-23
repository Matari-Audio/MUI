//! Emitting one paint entry, and a node's shadows.
use mui_geometry::{Bounds, Path, Point, RoundedRect};

use super::{Layer, Painted, SceneError, Walk};
use crate::{Color, Fill, Shadow, ShadowKind};

impl Walk<'_> {
    /// One shadow of a node, offset and spread off the node's own outline.
    ///
    /// A rounded rect blurs analytically, so that is what the entry carries
    /// whenever the outline is one. A welded outline is not, and becomes one
    /// blurred rect per welded child instead.
    pub(super) fn shadow(
        &mut self,
        sh: &Shadow,
        outline: &Path,
        rect: Option<RoundedRect>,
        welds: &[RoundedRect],
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
        let rects: Vec<RoundedRect> = match rect {
            Some(r) => vec![r],
            None if !welds.is_empty() => welds.to_vec(),
            // ponytail: no analytic rect and no welds -- the shape travels
            // as a path, and the spread with it is dropped.
            None => {
                if let Some(p) = self.push(
                    Layer::Shadow(sh.kind),
                    outline.rigid_transform(d, 0.0)?,
                    None,
                    &sh.fill,
                    under,
                ) {
                    p.blur = sh.blur;
                }
                return Ok(());
            }
        };
        for r in rects {
            let r = moved(r)?;
            if let Some(p) = self.push(Layer::Shadow(sh.kind), r.path(), Some(r), &sh.fill, under) {
                p.blur = sh.blur;
            }
        }
        Ok(())
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
            .weld(Role::Surface)
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
            .weld(Role::Surface)
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
