//! The payload a layout node carries, and the words that build a tree.
//!
//! ```
//! use mui_scene::prelude::*;
//! let card = column([text("Cutoff"), text("1.2 kHz").fill(Role::Dim)])
//!     .gap(S)
//!     .pad(M)
//!     .fill(Role::Raised)
//!     .shell(4.0, Role::Field);
//! assert_eq!(card.children().len(), 2);
//! ```
use crate::{Cursor, Elevation, Fill, Mix, Radius, Shadow, Stroke, Style};
use mui_geometry::CornerStyle;
use mui_geometry::Path;
use mui_layout::{Node, Size, Spacing};
use mui_motion::Spring;
use mui_text::Weight;
use std::sync::Arc;

/// One stroke or fill a canvas hands back, in the canvas's own pixels.
///
/// A draw with a [`tag`](Draw::tag) is also the canvas node's hit shape:
/// see [`Draw::hit`].
#[derive(Clone, Debug, PartialEq)]
pub struct Draw {
    pub path: Path,
    pub fill: Fill,
    /// Stroke width; `0` fills.
    pub width: f64,
    /// Names this shape as a hit target of the canvas node. The runtime
    /// reports it as the node's own gesture, tagged with this name.
    pub tag: Option<Arc<str>>,
}
impl Draw {
    pub fn fill(path: Path, fill: impl Into<Fill>) -> Self {
        Self {
            path,
            fill: fill.into(),
            width: 0.0,
            tag: None,
        }
    }
    pub fn stroke(path: Path, fill: impl Into<Fill>, width: f64) -> Self {
        Self {
            path,
            fill: fill.into(),
            width,
            tag: None,
        }
    }
    /// Geometry that responds but paints nothing: the fat target around a
    /// hairline, or a knot's grab radius.
    ///
    /// ```
    /// use mui_scene::prelude::*;
    /// # use mui_scene::Draw;
    /// let square = [(12., 12.), (28., 12.), (28., 28.), (12., 28.)];
    /// let grab = Draw::hit(Path::polyline(square.map(|(x, y)| Point::new(x, y)), true), "knot-0");
    /// assert!(grab.fill.is_none());
    /// ```
    pub fn hit(path: Path, tag: impl Into<Arc<str>>) -> Self {
        Self {
            path,
            fill: Fill::None,
            width: 0.0,
            tag: Some(tag.into()),
        }
    }
    /// Name this drawn shape, so the pointer inside it -- and nowhere else
    /// in the node's frame -- is a gesture on the canvas. Read the name back
    /// with `Ui::tag`.
    ///
    /// A stroke is hit-tested by its path's *fill*, not its outline: tag a
    /// closed shape, or add a [`Draw::hit`] beside the stroke.
    ///
    /// ```
    /// use mui_scene::prelude::*;
    /// # use mui_scene::Draw;
    /// let tri = [(0., 0.), (20., 0.), (10., 16.)].map(|(x, y)| Point::new(x, y));
    /// let ring = Draw::fill(Path::polyline(tri, true), Primary).tag("ring");
    /// assert_eq!(ring.tag.as_deref(), Some("ring"));
    /// ```
    pub fn tag(mut self, tag: impl Into<Arc<str>>) -> Self {
        self.tag = Some(tag.into());
        self
    }
}

/// Custom drawing: called with the node's size every frame, in the walk.
#[derive(Clone)]
pub struct Canvas(pub Arc<dyn Fn(Size) -> Vec<Draw>>);
impl std::fmt::Debug for Canvas {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Canvas(..)")
    }
}
impl PartialEq for Canvas {
    fn eq(&self, o: &Self) -> bool {
        Arc::ptr_eq(&self.0, &o.0)
    }
}

/// An interaction state a node can declare its look for, beside the resting
/// one. See [`Styled::on`].
///
/// ```
/// use mui_scene::prelude::*;
/// let tab = leaf(64., 28.).fill(Field).on(State::Focus, |s| s.stroke(Ink)).id("tab");
/// assert_eq!(tab.payload().states.len(), 1);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Hover,
    Press,
    Focus,
    /// Switched off: see [`Styled::disabled`]. Unlike the other three it is
    /// declared by the tree rather than discovered by the runtime, and the
    /// node it is on responds to nothing.
    Disabled,
}

/// What a node looks like in one [`State`]: its resting style in, the style
/// to paint out. [`Styled::on`] wraps the closure in one; two of them are
/// equal only when they are the same closure.
///
/// ```
/// use mui_scene::prelude::*;
/// # use mui_scene::StateStyle;
/// # use std::sync::Arc;
/// let lift = StateStyle(Arc::new(|s: Style| s.fill(Primary)));
/// assert_eq!(lift.0(Style::default()).fill, Fill::Role(Role::Primary));
/// ```
#[derive(Clone)]
pub struct StateStyle(pub Arc<dyn Fn(Style) -> Style>);
impl std::fmt::Debug for StateStyle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("StateStyle(..)")
    }
}
impl PartialEq for StateStyle {
    fn eq(&self, o: &Self) -> bool {
        Arc::ptr_eq(&self.0, &o.0)
    }
}

/// What a child does to its parent's outline instead of painting itself.
/// `.weld` is the third of these, and the only one that reads every child at
/// once, so it stays a flag on the parent's style.
///
/// ```
/// use mui_scene::prelude::*;
/// # use mui_scene::Carve;
/// let ring = stack![].square(64.).pill().fill(Primary).cut(leaf(40., 40.).pill());
/// assert_eq!(ring.children()[0].payload().carve, Some(Carve::Cut));
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Carve {
    /// Boolean difference: the child's shape is taken out of the parent's.
    Cut,
    /// Boolean intersection: only the overlap survives.
    Keep,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub enum Content {
    #[default]
    None,
    Text(String),
    Canvas(Canvas),
}

/// What a surface means to a screen reader, beyond where it is.
#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Button,
    Slider { value: f64, min: f64, max: f64 },
    Toggle { on: bool },
    TextInput { value: String },
    Label,
    Group,
    Scroll,
}

/// A role and the name read out with it. A node with none is a group named
/// by its own id.
#[derive(Clone, Debug, PartialEq)]
pub struct Semantics {
    pub role: Kind,
    pub label: Option<String>,
}
impl Semantics {
    pub fn new(role: Kind) -> Self {
        Self { role, label: None }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Element {
    pub style: Style,
    pub content: Content,
    /// Text size in pixels; `None` is the theme's.
    pub text_size: Option<f64>,
    /// How heavy the glyphs are drawn. See [`Styled::text_weight`].
    pub weight: Weight,
    /// A string this text node is at least as wide as, whatever it currently
    /// says. See [`Styled::reserve`].
    pub reserve: Option<String>,
    /// Shown after the pointer rests on the node.
    pub tip: Option<String>,
    /// Takes keyboard focus on click and on Tab.
    pub focusable: bool,
    /// Switched off: no hit testing, no focus, and the look declared for
    /// [`State::Disabled`]. Inherited by the subtree. See
    /// [`Styled::disabled`].
    pub disabled: bool,
    /// A row whose text children share one baseline. See [`Styled::baseline`].
    pub baseline: bool,
    /// Cap a wrapped label at this many lines. See [`Styled::lines`].
    pub lines: Option<usize>,
    /// What this node means: the role and name mui-access reports.
    pub semantics: Option<Semantics>,
    /// Looks declared for interaction states, applied in order by the
    /// runtime before the tree is resolved. See [`Styled::on`].
    pub states: Vec<(State, StateStyle)>,
    /// The spring this node's paint chases when its declared style changes.
    /// Only meaningful on a node with an id: the runtime has nothing to
    /// compare an anonymous node against. See [`Styled::transition`].
    pub transition: Option<Spring>,
    /// Set on a child pushed by [`Sugar::cut`](crate::Sugar::cut) or
    /// [`Sugar::keep`](crate::Sugar::keep): the child shapes its parent's
    /// outline instead of painting itself.
    pub carve: Option<Carve>,
}

/// A styled layout node: the type every constructor here returns.
pub type El = Node<Element>;

pub fn leaf(width: f64, height: f64) -> El {
    Node::leaf(width, height)
}
/// An empty, growing leaf: pushes its siblings apart.
pub fn spacer() -> El {
    Node::leaf(0.0, 0.0).grow(1.0)
}
pub fn row(children: impl IntoIterator<Item = El>) -> El {
    Node::row(children)
}
pub fn column(children: impl IntoIterator<Item = El>) -> El {
    Node::column(children)
}
pub fn overlay(children: impl IntoIterator<Item = El>) -> El {
    Node::overlay(children)
}
pub fn grid(cols: usize, children: impl IntoIterator<Item = El>) -> El {
    Node::grid(cols, children)
}
/// Shows the first candidate that fits the room on offer, widest first.
/// See [`Node::fits`] and the `fits!` macro.
///
/// ```
/// use mui_scene::prelude::*;
/// let bar = fits([text("Save changes"), text("Save"), leaf(8., 8.)]);
/// assert_eq!(bar.children().len(), 3);
/// ```
pub fn fits(candidates: impl IntoIterator<Item = El>) -> El {
    Node::fits(candidates)
}
/// A label, measured from the scene's font. Ink defaults to whatever reads on
/// the nearest painted ancestor; `.fill(..)` overrides it.
pub fn text(s: impl Into<String>) -> El {
    Node::content().with(Element {
        content: Content::Text(s.into()),
        ..Element::default()
    })
}
/// Your own paths, painted inside the node's frame. Sized like any
/// container: give it `.size(..)`, `.aspect(..)` or let it stretch.
pub fn canvas(f: impl Fn(Size) -> Vec<Draw> + 'static) -> El {
    Node::overlay([]).with(Element {
        content: Content::Canvas(Canvas(Arc::new(f))),
        ..Element::default()
    })
}
/// What the `row!`/`col!` macros accept: an `El`, or a string for a label.
pub trait IntoEl {
    fn into_el(self) -> El;
}
impl IntoEl for El {
    fn into_el(self) -> El {
        self
    }
}
impl IntoEl for &str {
    fn into_el(self) -> El {
        text(self)
    }
}
impl IntoEl for String {
    fn into_el(self) -> El {
        text(self)
    }
}

/// The paint builders, on an `El` or on a bare [`Style`]: one trait, so
/// `.fill(..)` chains after `.gap(..)` in either order, and a state closure
/// says `|s| s.fill(..)` with the same words the tree used.
///
/// ```
/// use mui_scene::prelude::*;
/// let bare = Style::default().fill(Raised).radius(12.);
/// let mut node = leaf(80., 24.).preset(&bare);
/// assert_eq!(node.style_mut().radius, Radius::Px(12.));
/// ```
pub trait Paints: Sized {
    fn style_mut(&mut self) -> &mut Style;

    fn fill(mut self, f: impl Into<Fill>) -> Self {
        self.style_mut().fill = f.into();
        self
    }
    fn stroke(mut self, f: impl Into<Fill>) -> Self {
        let s = self.style_mut();
        s.stroke = Some(Stroke {
            fill: f.into(),
            width: s.stroke.as_ref().and_then(|s| s.width),
        });
        self
    }
    fn stroke_width(mut self, w: f64) -> Self {
        match &mut self.style_mut().stroke {
            Some(s) => s.width = Some(w),
            none => {
                *none = Some(Stroke {
                    fill: Fill::None,
                    width: Some(w),
                })
            }
        }
        self
    }
    fn radius(mut self, r: impl Into<Radius>) -> Self {
        self.style_mut().radius = r.into();
        self
    }
    fn pill(self) -> Self {
        self.radius(Radius::Pill)
    }
    /// The curve this node's corners turn through, separately from how big
    /// they are: a continuous superellipse instead of a circular arc.
    ///
    /// It flows through whatever the node's outline turns out to be -- a
    /// welded union, its shells, its stroke -- because it restyles the arcs
    /// the outline is made of.
    ///
    /// ```
    /// use mui_scene::prelude::*;
    /// let mut card = leaf(80., 48.).radius(16.).corners(CornerStyle::Squircle);
    /// assert_eq!(card.style_mut().corners, CornerStyle::Squircle);
    /// ```
    fn corners(mut self, c: CornerStyle) -> Self {
        self.style_mut().corners = c;
        self
    }
    /// Add a shadow. Shadows stack, so a tight contact and a wide ambient
    /// are two calls; [`Paints::shadows`] replaces the list instead.
    ///
    /// ```
    /// use mui_scene::prelude::*;
    /// let mut el = leaf(80., 24.).shadow(Shadow::soft(2.)).shadow(Shadow::soft(12.));
    /// assert_eq!(el.style_mut().shadow.len(), 2);
    /// ```
    fn shadow(mut self, s: Shadow) -> Self {
        self.style_mut().shadow.push(s);
        self
    }
    /// Replace the whole shadow list.
    ///
    /// ```
    /// use mui_scene::prelude::*;
    /// let mut el = leaf(80., 24.).shadow(Shadow::soft(12.)).shadows([]);
    /// assert!(el.style_mut().shadow.is_empty());
    /// ```
    fn shadows(mut self, s: impl IntoIterator<Item = Shadow>) -> Self {
        self.style_mut().shadow = s.into_iter().collect();
        self
    }
    /// The theme's shadow list for this step off the surface.
    ///
    /// ```
    /// use mui_scene::prelude::*;
    /// let mut el = leaf(80., 24.).elevation(Elevation::Floating);
    /// assert_eq!(el.style_mut().shadow.len(), 2);
    /// ```
    fn elevation(self, e: Elevation) -> Self {
        self.shadows(e.shadows())
    }
    /// A ring `d` inside the previous outline, painted `f`. Stack them for
    /// constant-thickness nesting.
    fn shell(mut self, d: impl Into<Spacing>, f: impl Into<Fill>) -> Self {
        self.style_mut().shells.push((d.into(), f.into()));
        self
    }
    /// Composite this node's whole subtree through `m`.
    ///
    /// Keeps whatever opacity was set; see [`Styled::opacity`].
    fn blend(mut self, m: Mix) -> Self {
        let l = self.style_mut().layer.get_or_insert((Mix::Normal, 1.0));
        l.0 = m;
        self
    }
    /// Composite this node's whole subtree at `o` alpha, keeping whatever
    /// blend mode was set.
    ///
    /// ```
    /// use mui_scene::prelude::*;
    /// let mut el = leaf(10., 10.).blend(Mix::Multiply).opacity(0.5);
    /// assert_eq!(el.style_mut().layer, Some((Mix::Multiply, 0.5)));
    /// ```
    fn opacity(mut self, o: f32) -> Self {
        let l = self.style_mut().layer.get_or_insert((Mix::Normal, 1.0));
        l.1 = o;
        self
    }
    /// Paint `f` over everything this node and its subtree drew, and only
    /// where they drew it: one source-atop layer, no mask buffer, both
    /// backends.
    ///
    /// A ramp from the surface colour to transparent is the fade at the
    /// clipped edge of a scroll; a ramp between two roles over a label is a
    /// gradient-tinted glyph. What it cannot do is cut alpha out of the
    /// node -- source-atop paints *onto* the shape, it does not erase it.
    ///
    /// ```
    /// use mui_scene::prelude::*;
    /// # use mui_scene::Fill;
    /// let fade = Gradient::linear(180., [(0.8, Surface.alpha(0.)), (1., Surface.into())]);
    /// let mut list = col!["one", "two"].scroll().mask(fade);
    /// assert!(!list.style_mut().mask.is_none());
    /// ```
    fn mask(mut self, f: impl Into<Fill>) -> Self {
        self.style_mut().mask = f.into();
        self
    }
    /// Paint the union of the children's frames as one filleted shape.
    fn weld(mut self, f: impl Into<Fill>) -> Self {
        let s = self.style_mut();
        s.weld = true;
        s.fill = f.into();
        self
    }
    /// Merge a prepared style *over* this one: `.preset(&card())`. Every
    /// field the preset states wins; the rest of the chain survives.
    ///
    /// ```
    /// use mui_scene::prelude::*;
    /// # use mui_scene::Style;
    /// let card = Style { radius: Radius::Px(12.), ..Style::default() };
    /// let mut el = leaf(80., 24.).fill(Primary).preset(&card);
    /// assert_eq!(el.style_mut().radius, Radius::Px(12.));
    /// assert_eq!(el.style_mut().fill, Fill::Role(Role::Primary));
    /// ```
    fn preset(mut self, s: &Style) -> Self {
        let slot = self.style_mut();
        *slot = slot.over(s);
        self
    }
    /// Merge a prepared style *under* this one: a default the rest of the
    /// chain, before or after, is free to override.
    ///
    /// ```
    /// use mui_scene::prelude::*;
    /// # use mui_scene::Style;
    /// let card = Style { fill: Role::Raised.into(), ..Style::default() };
    /// let mut el = leaf(80., 24.).fill(Role::Danger).base(&card);
    /// assert_eq!(el.style_mut().fill, Fill::Role(Role::Danger));
    /// ```
    fn base(mut self, s: &Style) -> Self {
        let slot = self.style_mut();
        *slot = s.over(slot);
        self
    }
    /// Hand the node to `f`: a reusable run of builders, without a trait.
    ///
    /// ```
    /// use mui_scene::prelude::*;
    /// let outlined = |e: El| e.stroke(Ink).radius(8.);
    /// let mut el = leaf(80., 24.).apply(outlined);
    /// assert_eq!(el.style_mut().radius, Radius::Px(8.));
    /// ```
    fn apply(self, f: impl FnOnce(Self) -> Self) -> Self {
        f(self)
    }
    /// Pointer shape over this node and, unless they say otherwise, its
    /// children.
    fn cursor(mut self, c: Cursor) -> Self {
        self.style_mut().cursor = Some(c);
        self
    }
    /// Apply `f` only when `cond`: `.when(selected, |e| e.fill(Primary))`.
    fn when(self, cond: bool, f: impl FnOnce(Self) -> Self) -> Self {
        if cond {
            f(self)
        } else {
            self
        }
    }
}
impl Paints for Style {
    fn style_mut(&mut self) -> &mut Style {
        self
    }
}

/// What a node is, beyond its paint: text, tips, semantics, motion and
/// the looks it declares for the states it can be in. Only an `El` has
/// these -- a bare [`Style`] is paint and nothing else.
///
/// ```
/// use mui_scene::prelude::*;
/// let save = leaf(64., 28.).role(Kind::Button).label("Save").tip("Write it out").id("save");
/// assert!(save.payload().tip.is_some());
/// ```
pub trait Styled: Paints {
    fn element_mut(&mut self) -> &mut Element;

    fn text_size(mut self, px: f64) -> Self {
        self.element_mut().text_size = Some(px);
        self
    }
    /// How heavy this label's glyphs are, as a position on the font's `wght`
    /// axis. Inherited by nothing: a weight is a property of the run, so a
    /// row of labels states it per label.
    ///
    /// ```
    /// use mui_scene::prelude::*;
    /// let heading = text("Oscillator").text_weight(Weight::BOLD);
    /// assert_eq!(heading.payload().weight, Weight::BOLD);
    /// ```
    fn text_weight(mut self, w: Weight) -> Self {
        self.element_mut().weight = w;
        self
    }
    /// Measure this text node as if it said `s`, whenever `s` is the wider
    /// of the two: a readout keeps its box while its value changes, so the
    /// row beside it does not shuffle every frame.
    ///
    /// Give it the widest string the node can ever show -- `"-88.8 dB"`, not
    /// the value now. It sets a floor, never a ceiling: a longer string than
    /// reserved still measures at its own width.
    ///
    /// ```
    /// use mui_scene::prelude::*;
    /// let spec = SceneSpec::new(row![text("0.0 dB").reserve("-88.8 dB").id("gain")]);
    /// let wide = resolve_scene(&spec).unwrap().surface("gain").unwrap().frame.size.width;
    /// let bare = SceneSpec::new(row![text("0.0 dB").id("gain")]);
    /// let bare = resolve_scene(&bare).unwrap().surface("gain").unwrap().frame.size.width;
    /// assert!(wide > bare);
    /// ```
    fn reserve(mut self, s: impl Into<String>) -> Self {
        self.element_mut().reserve = Some(s.into());
        self
    }
    /// Declare what this node looks like while hovered, pressed or focused,
    /// beside what it looks like at rest. `f` is handed the resting style,
    /// so it edits rather than replaces, and a later `.fill(..)` is still
    /// what the state derives from.
    ///
    /// Only a node with an id has a state to read; the runtime applies these
    /// while building the frame. Pair with [`Styled::animate`] to cross
    /// rather than cut.
    ///
    /// ```
    /// use mui_scene::prelude::*;
    /// let mut el = leaf(80., 24.).fill(Field).on(State::Hover, |s| s.radius(4.)).id("b");
    /// assert_eq!(el.element_mut().states.len(), 1);
    /// ```
    fn on(mut self, state: State, f: impl Fn(Style) -> Style + 'static) -> Self {
        self.element_mut()
            .states
            .push((state, StateStyle(Arc::new(f))));
        self
    }
    fn tip(mut self, s: impl Into<String>) -> Self {
        self.element_mut().tip = Some(s.into());
        self
    }
    /// What this node is, for accessibility: `.role(Kind::Button)`. Only a
    /// node with an id becomes a surface, so only one is ever reported.
    fn role(mut self, k: Kind) -> Self {
        let e = self.element_mut();
        match &mut e.semantics {
            Some(s) => s.role = k,
            none => *none = Some(Semantics::new(k)),
        }
        self
    }
    /// The name read out with the role; the id otherwise.
    fn label(mut self, name: impl Into<String>) -> Self {
        let e = self.element_mut();
        let s = e
            .semantics
            .get_or_insert_with(|| Semantics::new(Kind::Group));
        s.label = Some(name.into());
        self
    }
    fn focusable(mut self) -> Self {
        self.element_mut().focusable = true;
        self
    }
    /// Switch this node -- and everything under it -- off: it drops out of
    /// hit testing and out of Tab, and it paints whatever it declared for
    /// [`State::Disabled`]. A greyed control that still drags is worse than
    /// no grey at all, so the look and the gate are one call.
    ///
    /// Takes the flag rather than being a marker, because the caller almost
    /// always has one already (`.disabled(!module.enabled)`).
    ///
    /// ```
    /// use mui_scene::prelude::*;
    /// let bypassed = leaf(64., 28.)
    ///     .fill(Primary)
    ///     .on(State::Disabled, |s| s.fill(Ink.alpha(0.2)))
    ///     .disabled(true)
    ///     .id("osc-2");
    /// assert!(bypassed.payload().disabled);
    /// ```
    fn disabled(mut self, on: bool) -> Self {
        self.element_mut().disabled = on;
        self
    }
    /// Spring this node's paint toward whatever it is next declared to be,
    /// instead of cutting. Needs an id: the runtime keys the springs by it.
    ///
    /// Paint only -- fill colour, stroke width, `Px` radius, text size,
    /// shadow blur, `Px` shell depths. Sizes, gaps, padding and layout
    /// frames are **not** transitioned: they are solved fresh every frame,
    /// and springing them would fight the layout rather than decorate it.
    /// Animate a position by springing the value you feed the tree instead
    /// (see `Ui::tween`).
    fn transition(mut self, s: Spring) -> Self {
        self.element_mut().transition = Some(s);
        self
    }
    /// Sit this container's text children on one baseline instead of
    /// centring each in its own frame, so a 12 px label and a 24 px readout
    /// line up on the letters rather than on the boxes.
    ///
    /// ponytail: the row's height is still the tallest child's, so a big
    /// ascent can push a shifted line past the frame; give the row a height
    /// if that shows.
    fn baseline(mut self) -> Self {
        self.element_mut().baseline = true;
        self
    }
    /// Cap a wrapping label at `n` lines; the last one ends in an ellipsis.
    fn lines(mut self, n: usize) -> Self {
        self.element_mut().lines = Some(n.max(1));
        self
    }
    /// [`Styled::transition`] with the default spring.
    fn animate(self) -> Self {
        self.transition(Spring::DEFAULT)
    }
}
impl Paints for El {
    fn style_mut(&mut self) -> &mut Style {
        &mut self.payload_mut().style
    }
}
impl Styled for El {
    fn element_mut(&mut self) -> &mut Element {
        self.payload_mut()
    }
}

#[cfg(test)]
mod tests {
    use crate::prelude::*;
    use crate::Style;

    fn card() -> Style {
        Style {
            fill: Raised.into(),
            radius: Radius::Px(12.),
            ..Style::default()
        }
    }

    /// The whole merge rule: per field, the side that states something wins,
    /// and which side that is depends only on which method was called.
    #[test]
    fn preset_wins_per_field_and_base_loses_per_field() {
        let mut over = leaf(10., 10.).fill(Primary).stroke(Ink).preset(&card());
        let s = over.style_mut();
        assert_eq!((s.fill.clone(), s.radius), (card().fill, card().radius));
        assert!(s.stroke.is_some(), "a field the preset left unset survives");

        let mut under = leaf(10., 10.).fill(Primary).base(&card());
        let s = under.style_mut();
        assert_eq!((s.fill.clone(), s.radius), (Primary.into(), card().radius));
    }

    /// One slot per concept: a second spelling of the same thing replaces
    /// the first, and a different thing does not.
    #[test]
    fn radius_and_stroke_keep_one_slot_each() {
        assert_eq!(
            leaf(10., 10.).pill().radius(8.).style_mut().radius,
            Radius::Px(8.)
        );
        assert_eq!(
            leaf(10., 10.).radius(8.).pill().style_mut().radius,
            Radius::Pill
        );
        let mut el = leaf(10., 10.).stroke_width(2.).stroke(Ink);
        let stroke = el.style_mut().stroke.clone().expect("set");
        assert_eq!((stroke.fill, stroke.width), (Ink.into(), Some(2.)));
    }

    /// A joined strip says it once, on the container: the children go
    /// square and its own corner is what rounds the two ends.
    #[test]
    fn join_squares_every_inner_corner_and_keeps_the_strips_own() {
        let mut strip = row![leaf(60., 28.), leaf(60., 28.), leaf(60., 28.)]
            .radius(12.)
            .join();
        assert_eq!(strip.style_mut().radius, Radius::Px(12.));
        assert!(strip.is_clip(), "the ends are rounded by the clip");
        for c in strip.children_mut() {
            assert_eq!(c.style_mut().radius, Radius::Px(0.));
        }
    }

    /// A role at an alpha is still the role: it tracks the palette, and it
    /// is not the literal colour a light-mode eyedropper would give.
    #[test]
    fn a_faded_role_resolves_through_the_palette() {
        let p = crate::Palette::NEUTRAL;
        let under = p.surface();
        let Some(crate::Paint::Solid(c)) = Ink.alpha(0.12).paint(&p, under) else {
            panic!("a faded role paints solid");
        };
        assert!((c.alpha() - 0.12).abs() < 1e-6);
        assert_eq!(c.with_alpha(1.0), p.on(under));
    }
}
