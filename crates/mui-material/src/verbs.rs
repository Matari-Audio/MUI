// Also compiled into mui-scene's own unit tests (`#[path]` there), whose
// fixtures use these verbs: a dev-dependency on this crate would hand them a
// second, incompatible copy of mui-scene's types. So: names come from the
// parent module only, never from `mui_scene::` or `crate::` directly.
use super::{BorderRamp, Carve, El, Element, Fill, Id, Spacing, Styled};

/// Outline, surface and border-material verbs on an [`El`].
pub trait Material: Styled {
    /// Paint the union of the children's outlines as one filleted vector
    /// shape. Only the outline is shared: each child keeps its own paint.
    /// Shells, strokes, shadows, clips and `.inside(..)` follow the union.
    /// To blend the children's paint across the seam instead, see
    /// [`Styled::weld`](mui_scene::Styled::weld).
    fn union(mut self, f: impl Into<Fill>) -> Self {
        let s = self.style_mut();
        s.union = Some(true);
        s.fill = Some(f.into());
        self
    }
    /// A ring `d` inside the previous outline, painted `f`. Stack them for
    /// constant-thickness nesting.
    fn shell(mut self, d: impl Into<Spacing>, f: impl Into<Fill>) -> Self {
        self.style_mut()
            .shells
            .get_or_insert_default()
            .push((d.into(), f.into()));
        self
    }
    /// Transform width and color on this node's single, fixed inside border.
    /// Supports ordinary, custom and [`Paints::union`] contours on every
    /// renderer.
    fn border_ramp(mut self, ramp: BorderRamp) -> Self {
        self.style_mut().stroke = None;
        self.element_mut().extras_mut().border_ramp = Some(ramp);
        self
    }
    /// Derive marked surfaces from this container's final contour and border.
    /// Layout remains intrinsic; only material outlines are derived. Nested
    /// owners start a new scope. Rounding belongs here, never on each panel.
    fn surface_layout(mut self, padding: impl Into<Spacing>) -> Self {
        self.element_mut().extras_mut().surface_padding = Some(padding.into());
        self
    }
    /// A recessed material using this node's footprint and the owner's corners.
    fn inset_surface(mut self) -> Self {
        self.element_mut().extras_mut().inset_surface = Some(Vec::new());
        self
    }
    /// One recessed material made from several named layout footprints.
    /// Use a background sibling when controls occupy holes in the material.
    fn inset_surface_of(mut self, members: impl IntoIterator<Item = impl Into<Id>>) -> Self {
        self.element_mut().extras_mut().inset_surface =
            Some(members.into_iter().map(Into::into).collect());
        self
    }
    /// Extend the owner's border material into this frame. The named body
    /// supplies the attachment edge, including when several bodies share a rim.
    /// This node retains its ordinary layout and interaction rectangle.
    fn join_border(mut self, body: impl Into<Id>) -> Self {
        self.element_mut().extras_mut().border_join = Some(body.into());
        self
    }
    /// Butt these children into one control: the gap closes, every child's
    /// corners go square, and the container clips them to its own corner.
    /// The strip's outer corners keep the radius; every seam inside it is
    /// square. daisyUI's `join`, on whichever axis the container already
    /// runs. The squaring happens when the scene resolves, so a child pushed
    /// after this call is squared too.
    ///
    /// ```
    /// use mui_material::prelude::*;
    /// let strip = row![block(60., 28.), block(60., 28.)].radius(Corner::Field).segmented();
    /// assert!(strip.payload().has(mui_scene::Element::SEGMENTED) && strip.is_clip());
    /// ```
    fn segmented(self) -> Self;
    /// Takes `el`'s shape out of this node's outline: boolean difference.
    /// The child is placed like any floating overlay child, so `.center()`,
    /// `.w(..)` and the rest position the hole, and then it is never
    /// painted. The shell, the border and the clip all follow the result,
    /// exactly as they follow a [`union`](Material::union). On a block,
    /// the block becomes a stack of its own size to hold the hole.
    ///
    /// ```
    /// use mui_material::prelude::*;
    /// let ring = block(64., 64.).pill().fill(Role::Primary).cut(block(40., 40.).pill());
    /// assert!(ring.children()[0].payload().carve.is_some());
    /// ```
    fn cut(self, el: El) -> Self;
    /// Keeps only what `el` overlaps: boolean intersection. See [`cut`](Material::cut).
    ///
    /// ```
    /// use mui_material::prelude::*;
    /// let half = stack![].square(64.).fill(Role::Primary).keep(block(32., 64.));
    /// assert!(half.children()[0].payload().carve.is_some());
    /// ```
    fn keep(self, el: El) -> Self;
}

impl Material for El {
    fn segmented(mut self) -> Self {
        // The container's own outline is what rounds the two ends: a clip,
        // not four per-corner radii the rest of the system would have to
        // learn.
        self.payload_mut().set(Element::SEGMENTED, true);
        self.gap(0.).clip()
    }
    fn cut(self, el: El) -> Self {
        self.push(carved(el, Carve::Cut))
    }
    fn keep(self, el: El) -> Self {
        self.push(carved(el, Carve::Keep))
    }
}

// A carve rides along as a floating child so layout sizes and places it for
// free; `push` turns a block into a stack to hold it.
fn carved(el: El, how: Carve) -> El {
    let mut el = el.float();
    el.payload_mut().carve = Some(how);
    el
}
