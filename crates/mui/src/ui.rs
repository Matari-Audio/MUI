//! The per-frame runtime: gestures in, animated styles applied, scene out.
use std::collections::BTreeMap;
use std::sync::Arc;

use mui_core::{El, Palette, ResolvedScene, SceneError, SceneSpec, Size, Spring, Theme};
use mui_input::{Hit, Interaction, PointerInput, Response};

/// What one call to [`Ui::frame`] produced.
pub struct Frame<'a> {
    pub scene: &'a ResolvedScene,
    /// A spring is still moving: schedule another frame.
    pub animating: bool,
}

/// Retained state for an immediate tree. Build the tree every frame; the
/// runtime remembers what is hovered, held, and mid-animation.
pub struct Ui {
    pub theme: Theme,
    pub font: Option<Arc<Vec<u8>>>,
    interaction: Interaction,
    hit: Hit,
    scene: Option<ResolvedScene>,
    /// Per key: hover and press springs, 0..1.
    springs: BTreeMap<String, [Spring; 2]>,
}
impl Ui {
    pub fn new(theme: Theme) -> Self {
        Self {
            theme,
            font: None,
            interaction: Interaction::new(),
            hit: Hit::default(),
            scene: None,
            springs: BTreeMap::new(),
        }
    }
    pub fn font(mut self, font: impl Into<Arc<Vec<u8>>>) -> Self {
        self.font = Some(font.into());
        self
    }
    /// Last frame's gesture on `id`. Widgets read this while building the
    /// next tree, so a drag lands one frame late and nobody notices.
    pub fn get(&self, id: &str) -> Response {
        self.interaction.get(id)
    }
    /// Hover and press amounts for `id`, 0..1 and spring-smoothed.
    pub fn state(&self, id: &str) -> (f64, f64) {
        self.springs
            .get(id)
            .map_or((0.0, 0.0), |[h, p]| (h.value, p.value))
    }
    /// Apply a horizontal or vertical drag on `id` to `value` across `range`,
    /// `px` pixels for the full span. Returns whether it changed.
    pub fn drag(
        &self,
        id: &str,
        value: &mut f64,
        range: std::ops::RangeInclusive<f64>,
        px: f64,
        vertical: bool,
    ) -> bool {
        let r = self.get(id);
        if !r.dragged || px <= 0.0 {
            return false;
        }
        let d = if vertical {
            -r.drag_delta.y
        } else {
            r.drag_delta.x
        };
        let next =
            (*value + d / px * (range.end() - range.start())).clamp(*range.start(), *range.end());
        let changed = next != *value;
        *value = next;
        changed
    }

    /// Advance gestures and springs, style the tree by state, resolve it.
    pub fn frame(
        &mut self,
        mut root: El,
        offered: Option<Size>,
        pointer: PointerInput,
        dt: f64,
    ) -> Result<Frame<'_>, SceneError> {
        self.interaction.update(&self.hit, pointer);
        let (hovered, held) = (
            self.interaction.hovered().map(str::to_owned),
            self.interaction.held().map(str::to_owned),
        );
        for (k, [h, p]) in &mut self.springs {
            h.to(f64::from(
                hovered.as_deref() == Some(k) || held.as_deref() == Some(k),
            ));
            p.to(f64::from(held.as_deref() == Some(k)));
        }
        for k in [hovered, held].into_iter().flatten() {
            self.springs
                .entry(k)
                .or_insert_with(|| [Spring::at(0.0), Spring::at(0.0)])
                .iter_mut()
                .for_each(|s| s.to(1.0));
        }
        let mut animating = false;
        for s in self.springs.values_mut().flatten() {
            animating |= s.step(dt);
        }
        self.springs
            .retain(|_, [h, p]| h.value > 0.0 || p.value > 0.0 || !h.settled() || !p.settled());

        let pal = self.theme.palette;
        let springs = &self.springs;
        state(&mut root, &pal, &|k| {
            springs.get(k).map(|[h, p]| (h.value, p.value))
        });

        let mut spec = SceneSpec::new(root).theme(self.theme);
        spec.offered = offered;
        spec.font = self.font.clone();
        let scene = mui_core::resolve_scene(&spec)?;
        // Named nodes are the gesture targets, in z-order. Unnamed ones are
        // decoration.
        let mut hit = Hit::default();
        for k in scene.keys.iter().filter(|k| !k.starts_with('/')) {
            if let Some(s) = scene.surface(k) {
                hit.push(k.clone(), &s.path)?;
            }
        }
        self.hit = hit;
        self.scene = Some(scene);
        Ok(Frame {
            scene: self.scene.as_ref().expect("just set"),
            animating,
        })
    }
}

/// Push hover and press into every named node's fill, proportionally.
fn state(n: &mut El, pal: &Palette, of: &dyn Fn(&str) -> Option<(f64, f64)>) {
    if let Some((h, p)) = n.key().and_then(of) {
        let bg = pal.background();
        let e = n.payload_mut();
        if !e.style.fill.is_none() && (h > 0.0 || p > 0.0) {
            e.style.fill = e.style.fill.map(pal, bg, |c| {
                c.mix(pal.hover(c), h as f32).mix(pal.pressed(c), p as f32)
            });
        }
    }
    for c in n.children_mut() {
        state(c, pal, of);
    }
}
impl std::fmt::Debug for Ui {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ui")
            .field("springs", &self.springs.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mui_core::prelude::*;
    use mui_geometry::Point;

    fn at(x: f64, y: f64, down: bool) -> PointerInput {
        PointerInput {
            pos: Some(Point::new(x, y)),
            primary_down: down,
        }
    }
    #[test]
    fn hover_warms_the_fill_and_a_press_is_reported_next_frame() {
        let mut ui = Ui::new(Theme::DEFAULT);
        let tree = || leaf(40., 40.).fill(Role::Raised).id("b");
        let base = ui
            .frame(tree(), None, PointerInput::default(), 0.016)
            .unwrap()
            .scene
            .paint[0]
            .paint
            .clone();
        ui.frame(tree(), None, at(10., 10., false), 0.016).unwrap();
        let f = ui.frame(tree(), None, at(10., 10., true), 0.016).unwrap();
        assert!(f.animating);
        assert_ne!(f.scene.paint[0].paint, base);
        let _ = ui.frame(tree(), None, at(10., 10., false), 0.016).unwrap();
        assert!(ui.get("b").released);
    }
}
