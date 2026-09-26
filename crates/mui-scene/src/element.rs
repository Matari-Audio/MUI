//! The payload a layout node carries, and the words that build a tree.
//!
//! ```
//! use mui_scene::prelude::*;
//! use mui_material::Material;
//! let card = col([text("Cutoff"), text("1.2 kHz").fill(Role::Dim)])
//!     .gap(S)
//!     .pad(M)
//!     .fill(Role::Raised)
//!     .shell(4.0, Role::Field);
//! assert_eq!(card.children().len(), 2);
//! ```
use crate::{Cursor, Elevation, Fill, Fit, Image, Mix, Radius, Shadow, Stroke, Style};
use mui_geometry::CornerStyle;
use mui_geometry::{Path, Point};
use mui_layout::{Id, Node, Px, Size, Spacing};
use mui_motion::Spring;
use mui_text::{Axes, Weight};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

/// One stroke or fill a canvas hands back, in the canvas's own pixels.
///
/// A draw with a [`tag`](Draw::tag) is also the canvas node's hit shape:
/// see [`Draw::hit`].
#[derive(Clone, Debug, PartialEq)]
pub struct Draw {
    /// Shared, so a canvas that keeps its shapes hands the walk the same
    /// `Arc` and the walk never copies one.
    pub path: Arc<Path>,
    pub fill: Fill,
    /// Stroke width; `0` fills.
    pub width: f64,
    /// Names this shape as a hit target of the canvas node. The runtime
    /// reports it as the node's own gesture, tagged with this name.
    pub tag: Option<Arc<str>>,
    /// Where the path's origin sits in the canvas: a shape drawn once and
    /// placed, like a cached run of glyphs, moves without a copy.
    pub at: Point,
}
impl Draw {
    pub fn fill(path: impl Into<Arc<Path>>, fill: impl Into<Fill>) -> Self {
        Self {
            path: path.into(),
            fill: fill.into(),
            width: 0.0,
            tag: None,
            at: Point::ZERO,
        }
    }
    pub fn stroke(path: impl Into<Arc<Path>>, fill: impl Into<Fill>, width: f64) -> Self {
        Self {
            path: path.into(),
            fill: fill.into(),
            width,
            tag: None,
            at: Point::ZERO,
        }
    }
    /// `image` stretched over the rectangle at `(x, y)`, `w` by `h`: a logo,
    /// a screenshot, or pixels read back from another GPU pipeline. Built at
    /// `w * scale` by `h * scale` pixels it lands one image pixel per device
    /// pixel. The GPU renderer keeps one atlas upload per `rgba` buffer, so
    /// hand it the same `Arc` while the pixels stay the same.
    pub fn image(x: f64, y: f64, w: f64, h: f64, image: Arc<Image>) -> Self {
        let corners = [(x, y), (x + w, y), (x + w, y + h), (x, y + h)];
        Self::fill(
            Path::polyline(corners.map(|(x, y)| Point::new(x, y)), true),
            Fill::Image(image, Fit::Fill),
        )
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
    pub fn hit(path: impl Into<Arc<Path>>, tag: impl Into<Arc<str>>) -> Self {
        Self {
            path: path.into(),
            fill: Fill::None,
            width: 0.0,
            tag: Some(tag.into()),
            at: Point::ZERO,
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
    /// let ring = Draw::fill(Path::polyline(tri, true), Role::Primary).tag("ring");
    /// assert_eq!(ring.tag.as_deref(), Some("ring"));
    /// ```
    pub fn tag(mut self, tag: impl Into<Arc<str>>) -> Self {
        self.tag = Some(tag.into());
        self
    }
    /// Place the path's origin at `p` in the canvas.
    pub fn at(mut self, p: Point) -> Self {
        self.at = p;
        self
    }
}

/// Custom drawing: called with the node's size every frame, in the walk.
///
/// The tree's closures ([`Canvas`], [`StateStyle`], [`Outline`]) are not
/// `Send`: a tree is built and resolved on one thread every frame, and a
/// closure may capture an `Rc` or a `Cell`. Nothing that outlives the frame
/// (the [`Resolver`](crate::Resolver)'s caches, `mui::Ui`) keeps a closure,
/// only what it drew, so those stay `Send` without asking it of the tree.
///
/// Equality is identity: two are equal when they are the same `Arc`. A
/// rebuilt closure compares unequal even when it draws the same thing, so
/// `==` on a tree can say "changed" when nothing did, never the reverse.
#[derive(Clone)]
pub struct Canvas(pub Arc<dyn Fn(Size) -> Arc<[Draw]>>);
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

/// One caller-owned memo for custom drawing.
///
/// The key must cover everything the drawing closure reads. Reusing a key at
/// the same size deliberately reuses the old immutable draw list.
type CacheSlot<K> = Rc<RefCell<Option<(K, Size, Arc<[Draw]>)>>>;
pub struct CanvasCache<K>(CacheSlot<K>);
impl<K> CanvasCache<K> {
    pub fn new() -> Self {
        Self(Rc::new(RefCell::new(None)))
    }
}
impl<K> Clone for CanvasCache<K> {
    fn clone(&self) -> Self {
        Self(Rc::clone(&self.0))
    }
}
impl<K> Default for CanvasCache<K> {
    fn default() -> Self {
        Self::new()
    }
}

/// An interaction state a node can declare its look for, beside the resting
/// one. See [`Styled::on`].
///
/// ```
/// use mui_scene::prelude::*;
/// let tab = block(64., 28.).fill(Role::Field).on(State::Focus, |s| s.stroke(Role::Ink)).id("tab");
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
/// let lift = StateStyle(Arc::new(|s: Style| s.fill(Role::Primary)));
/// assert_eq!(lift.0(Style::default()).fill, Some(Fill::Role(Role::Primary)));
/// ```
///
/// Not `Send`, and equality is identity; see [`Canvas`].
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
/// `mui_material::Material::union` is the third boolean, and the only one that reads every
/// child at once, so it stays a flag on the parent's style.
///
/// ```
/// use mui_scene::prelude::*;
/// # use mui_scene::Carve;
/// use mui_material::Material;
/// let ring = stack![].square(64.).pill().fill(Role::Primary).cut(block(40., 40.).pill());
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
    Text(Arc<str>),
    Canvas(Canvas),
}

/// What a surface means to a screen reader, beyond where it is.
#[derive(Clone, Debug, PartialEq)]
pub enum A11y {
    Button,
    Slider {
        value: f64,
        min: f64,
        max: f64,
    },
    Toggle {
        on: bool,
    },
    /// A one-line field. `selection` is the anchor and the caret, in
    /// characters of `value`, equal when nothing is selected. `carets` is
    /// the x of a caret before each character and after the last, in the
    /// field's own space -- one more entry than `value` has characters --
    /// or empty when the field did not measure them.
    TextInput {
        value: Arc<str>,
        selection: (usize, usize),
        carets: Vec<f64>,
    },
    Label,
    Group,
    Scroll,
    /// A picture: a logo, a screenshot, an icon that means something. Its
    /// `.named(..)` is the alt text; without one it is decoration and goes
    /// unnamed, as an `<img alt="">` does.
    Image,
}

/// A role and the name read out with it. A node with none is an unnamed
/// group; a control with no label is named by its id.
#[derive(Clone, Debug, PartialEq)]
pub struct Semantics {
    pub role: A11y,
    pub label: Option<Arc<str>>,
}
impl Semantics {
    pub fn new(role: A11y) -> Self {
        Self { role, label: None }
    }
}

/// A node's own local outline. It is used for painting, clipping and input, not
/// merely drawn on top of a rectangular hit target. Its callback is evaluated at
/// the layout size; it must return closed, consistently wound contours.
///
/// Not `Send`, like [`Canvas`] and [`StateStyle`]: see [`Canvas`].
/// Equality is identity; see [`Canvas`].
#[derive(Clone)]
pub struct Outline(pub Arc<dyn Fn(Size) -> Path>);
impl std::fmt::Debug for Outline {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Outline(..)")
    }
}
impl PartialEq for Outline {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Element {
    pub style: Style,
    pub content: Content,
    /// Text size in pixels; `None` is the [`text_role`](Element::text_role)'s,
    /// or the theme's.
    pub text_size: Option<f64>,
    /// Which of the theme's [`TypeScale`](crate::TypeScale) sizes the text
    /// takes when no `text_size` is set. See [`title`](crate::title).
    pub text_role: Option<TextRole>,
    /// Variable-font axis positions the glyphs are drawn at: `wght`, `FILL`,
    /// whatever the face declares. Empty is the face's default instance.
    /// See [`Styled::text_weight`] and [`Styled::text_axis`].
    pub axes: Axes,
    /// A face for this node alone, tried before the scene's font and its
    /// fallbacks: an icon font on an icon. See [`Styled::font`].
    pub font: Option<mui_text::Font>,
    /// Cap a wrapped label at this many lines. See [`Styled::lines`].
    pub lines: Option<usize>,
    /// What this node means: the role and name mui-access reports.
    pub semantics: Option<Box<Semantics>>,
    /// Looks declared for interaction states, applied in order by the
    /// runtime before the tree is resolved. See [`Styled::on`].
    pub states: Vec<(State, StateStyle)>,
    /// Set on a child pushed by `Material::cut` or `Material::keep` (mui-material): the
    /// child shapes its parent's outline instead of painting itself.
    pub carve: Option<Carve>,
    pub bend: f64,
    pub border_align: crate::BorderAlign,
    /// The on/off switches, one bit each: [`Element::FOCUSABLE`] ..
    /// [`Element::SCROLL_BAR_OFF`]. Read with [`Element::has`].
    pub flags: u16,
    /// Everything most nodes never set, boxed on first write so every
    /// builder call moves a small node. Read it through [`Element::extras`].
    pub extras: Option<Box<Extras>>,
}

/// The rarely set half of an [`Element`].
#[derive(Clone, Debug, PartialEq)]
pub struct Extras {
    /// A string this text node is at least as wide as, whatever it currently
    /// says. See [`Styled::reserve`].
    pub reserve: Option<String>,
    /// Shown after the pointer rests on the node.
    pub tip: Option<Arc<str>>,
    /// The spring this node's paint chases when its declared style changes.
    /// Only meaningful on a node with an id: the runtime has nothing to
    /// compare an anonymous node against. See [`Styled::animate_with`].
    pub transition: Option<Spring>,
    /// The spring this node's solved *frame* chases: position and size
    /// glide instead of jumping when the layout moves it. See
    /// [`Styled::animate_layout`].
    pub layout_transition: Option<Spring>,
    /// Where the node comes in from on its first frame, and fades out to
    /// after it leaves the tree. See [`Styled::appear`].
    pub appear: Option<Appear>,
    /// What this node *is*, whatever it is named this frame. See
    /// [`Styled::identity`].
    pub identity: Option<u64>,
    /// Shape identity: when it changes, the outline morphs from the old
    /// shape to the new one. See [`Styled::morph`].
    pub morph: Option<u64>,
    /// Material weld: the plates' paint blended into one baked or GPU
    /// image. `None` paints every child itself. See [`Styled::weld`].
    pub welding: Option<crate::Weld>,
    /// Custom local shape; geometry is validated before publication.
    pub outline: Option<Outline>,
    pub border_ramp: Option<crate::BorderRamp>,
    pub inside: Option<Spacing>,
    /// Own the corner policy and clearances of marked descendant surfaces.
    pub surface_padding: Option<Spacing>,
    /// Empty uses this node's frame; otherwise union the named footprints.
    pub inset_surface: Option<Vec<Id>>,
    /// Join this frame to the nearest horizontal edge of the named body.
    pub border_join: Option<Id>,
    /// Set by the runtime on the root of a memoised subtree (`Ui::memo`).
    pub memo: Option<Memo>,
}

/// The root of a memoised subtree. A resolve records where each memo's
/// paint and surfaces land; a `reused` one -- the very subtree last frame
/// resolved, handed back unchanged -- is painted by copying those.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Memo {
    pub id: u64,
    pub reused: bool,
}
impl Extras {
    const NONE: Self = Self {
        reserve: None,
        tip: None,
        transition: None,
        layout_transition: None,
        appear: None,
        identity: None,
        morph: None,
        welding: None,
        outline: None,
        border_ramp: None,
        inside: None,
        surface_padding: None,
        inset_surface: None,
        border_join: None,
        memo: None,
    };
}
impl Default for Extras {
    fn default() -> Self {
        Self::NONE
    }
}
impl Element {
    /// Takes keyboard focus on click and on Tab.
    pub const FOCUSABLE: u16 = 1 << 0;
    /// The wheel over this node is its own: an enclosing `.scroll()` does
    /// not slide. See [`Styled::captures_wheel`].
    pub const CAPTURES_WHEEL: u16 = 1 << 1;
    /// Reads the raw pointer while building, so a move over it is never
    /// inert. See [`Styled::tracks_pointer`].
    pub const TRACKS_POINTER: u16 = 1 << 2;
    /// Switched off: no hit testing, no focus, and the look declared for
    /// [`State::Disabled`]. Inherited by the subtree. See
    /// [`Styled::disabled`].
    pub const DISABLED: u16 = 1 << 3;
    /// A row whose text children share one baseline. See [`Styled::baseline`].
    pub const BASELINE: u16 = 1 << 4;
    /// Square every child's corners when resolved. See
    /// `mui_material::Material::segmented`.
    pub const SEGMENTED: u16 = 1 << 5;
    /// Exclude this immediate child from its parent's weld, not from layout.
    pub const WELD_EXCLUDED: u16 = 1 << 6;
    /// A `.scroll()` node paints no overlay scrollbar. See
    /// [`Styled::no_scrollbar`].
    pub const SCROLL_BAR_OFF: u16 = 1 << 7;
    /// An [`icon`]: with no `.font(f)` of its own it draws in
    /// [`Theme::icon_font`](crate::Theme::icon_font).
    pub const ICON: u16 = 1 << 8;

    /// The face this node shapes with, before the scene's fonts: its own, or
    /// the theme's icon font for an icon.
    pub(crate) fn face_font<'a>(&'a self, theme: &'a crate::Theme) -> Option<&'a mui_text::Font> {
        self.font.as_ref().or_else(|| {
            if self.has(Self::ICON) { theme.icon_font.as_ref() } else { None }
        })
    }

    /// Whether the switch `flag` is on: `e.has(Element::FOCUSABLE)`.
    pub fn has(&self, flag: u16) -> bool {
        self.flags & flag != 0
    }
    /// Turn the switch `flag` on or off.
    pub fn set(&mut self, flag: u16, on: bool) {
        if on {
            self.flags |= flag;
        } else {
            self.flags &= !flag;
        }
    }
    /// The rare fields, all unset when none was ever written.
    pub fn extras(&self) -> &Extras {
        // A promoted constant, not a `static`: no `Sync` asked of the
        // closures an `Extras` can hold.
        self.extras.as_deref().unwrap_or(&Extras::NONE)
    }
    pub fn extras_mut(&mut self) -> &mut Extras {
        self.extras.get_or_insert_default()
    }
}

/// How a node enters: it always fades in from transparent, and starts from
/// here. On leaving it fades out where it last stood.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Appear {
    Fade,
    /// From this fraction of its size, about its centre.
    Scale(f64),
    /// From this far away, in logical units.
    Slide(f64, f64),
}

/// Which [`TypeScale`](crate::TypeScale) size a text node takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextRole {
    Title,
    Body,
    Caption,
}
impl Element {
    /// The text size this node resolves to under `th`: its own
    /// `text_size`, else its role's, else the theme's.
    pub fn text_px(&self, th: &crate::Theme) -> f64 {
        let t = th.type_scale;
        self.text_size.unwrap_or(match self.text_role {
            Some(TextRole::Title) => t.title,
            Some(TextRole::Body) => t.body,
            Some(TextRole::Caption) => t.caption,
            None => th.text,
        })
    }
}

/// A styled layout node: the type every constructor here returns.
pub type El = Node<Element>;

/// A box of a known size: an icon cell, a swatch, a spacer.
pub fn block(width: impl Into<mui_layout::Len>, height: impl Into<mui_layout::Len>) -> El {
    Node::block(width, height)
}
/// An empty, growing block: pushes its siblings apart.
pub fn spacer() -> El {
    Node::block(0.0, 0.0).grow(1.0)
}
pub fn row(children: impl IntoIterator<Item = El>) -> El {
    Node::row(children)
}
pub fn col(children: impl IntoIterator<Item = El>) -> El {
    Node::col(children)
}
/// Children sharing one box, painted in order.
pub fn stack(children: impl IntoIterator<Item = El>) -> El {
    Node::stack(children)
}
pub fn grid(cols: usize, children: impl IntoIterator<Item = El>) -> El {
    Node::grid(cols, children)
}
/// Shows the first candidate that fits the room on offer, widest first.
/// See [`Node::fits`] and the `fits!` macro.
///
/// ```
/// use mui_scene::prelude::*;
/// let bar = fits([text("Save changes"), text("Save"), block(8., 8.)]);
/// assert_eq!(bar.children().len(), 3);
/// ```
pub fn fits(candidates: impl IntoIterator<Item = El>) -> El {
    Node::fits(candidates)
}
/// A label, measured from the scene's font. Ink defaults to whatever reads on
/// the nearest painted ancestor; `.fill(..)` overrides it.
pub fn text(s: impl Into<Arc<str>>) -> El {
    Node::content().with(Element {
        content: Content::Text(s.into()),
        ..Element::default()
    })
}
/// One symbol from an icon font, drawn as text at the text size so `opsz`
/// tracks it. `symbol` is the font's codepoint for it -- for Material
/// Symbols, `mui_symbols::sym::HOME` is one, and `mui_symbols::codepoint`
/// looks up a name that arrives at run time. The face is the theme's
/// [`icon_font`](crate::Theme::icon_font); `.font(f)` overrides it. Style it
/// like a label: `.text_size(24)`, `.icon_fill(1.)`, `.text_weight(..)`,
/// `.grade(..)`, `.fill(..)` for ink.
///
/// ```
/// use mui_scene::prelude::*;
/// # let font = Font::new(epaint_default_fonts::HACK_REGULAR).unwrap();
/// let theme = Theme { icon_font: Some(font.clone()), ..Theme::DEFAULT };
/// let home = icon(mui_symbols::sym::HOME).text_size(24).icon_fill(1.).id("home");
/// assert_eq!(home.payload().axes.get("FILL"), Some(1.));
/// let scene = resolve(&SceneSpec::new(home).theme(theme)).unwrap();
/// assert!(scene.surface("home").unwrap().text.is_some(), "shaped in the theme's icon font");
/// ```
pub fn icon(symbol: char) -> El {
    let mut el = text(symbol.encode_utf8(&mut [0; 4]) as &str);
    el.payload_mut().set(Element::ICON, true);
    el
}
/// Your own paths, painted inside the node's frame. Sized like any
/// container: give it `.size(..)`, `.aspect(..)` or let it stretch.
pub fn canvas(f: impl Fn(Size) -> Vec<Draw> + 'static) -> El {
    Node::stack([]).with(Element {
        content: Content::Canvas(Canvas(Arc::new(move |size| f(size).into()))),
        ..Element::default()
    })
}
/// A [`canvas`] whose draw list is rebuilt only when `key` or its size changes.
pub fn canvas_keyed<K: Clone + PartialEq + 'static>(
    cache: &CanvasCache<K>,
    key: K,
    f: impl Fn(Size) -> Vec<Draw> + 'static,
) -> El {
    let cache = cache.clone();
    Node::stack([]).with(Element {
        content: Content::Canvas(Canvas(Arc::new(move |size| {
            if let Some((old_key, old_size, draws)) = cache.0.borrow().as_ref()
                && *old_key == key
                && *old_size == size
            {
                return Arc::clone(draws);
            }
            let draws: Arc<[Draw]> = f(size).into();
            *cache.0.borrow_mut() = Some((key.clone(), size, Arc::clone(&draws)));
            draws
        }))),
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
/// let bare = Style::default().fill(Role::Raised).radius(12.);
/// let mut node = block(80., 24.).preset(bare);
/// assert_eq!(node.style_mut().radius, Some(Radius::Px(12.)));
/// ```
pub trait Paints: Sized {
    fn style_mut(&mut self) -> &mut Style;

    /// Clear the stroke now. Apply after presets that should not restore it.
    fn no_stroke(mut self) -> Self {
        self.style_mut().stroke = None;
        self
    }
    /// Clear the fill now, without disabling input or the border.
    fn no_fill(mut self) -> Self {
        self.style_mut().fill = Some(Fill::None);
        self
    }

    fn fill(mut self, f: impl Into<Fill>) -> Self {
        self.style_mut().fill = Some(f.into());
        self
    }
    fn stroke(mut self, f: impl Into<Fill>) -> Self {
        let s = self.style_mut();
        s.stroke = Some(Stroke {
            fill: Some(f.into()),
            width: s.stroke.as_ref().and_then(|s| s.width),
        });
        self
    }
    fn stroke_width(mut self, w: impl Px) -> Self {
        let w = w.px();
        match &mut self.style_mut().stroke {
            Some(s) => s.width = Some(w),
            none => {
                *none = Some(Stroke {
                    fill: None,
                    width: Some(w),
                });
            }
        }
        self
    }
    fn radius(mut self, r: impl Into<Radius>) -> Self {
        self.style_mut().radius = Some(r.into());
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
    /// let mut card = block(80., 48.).radius(16.).corners(CornerStyle::Squircle);
    /// assert_eq!(card.style_mut().corners, Some(CornerStyle::Squircle));
    /// ```
    fn corners(mut self, c: CornerStyle) -> Self {
        self.style_mut().corners = Some(c);
        self
    }
    /// Add a shadow. Shadows stack, so a tight contact and a wide ambient
    /// are two calls; [`Paints::shadows`] replaces the list instead.
    ///
    /// ```
    /// use mui_scene::prelude::*;
    /// let mut el = block(80., 24.).shadow(Shadow::soft(2.)).shadow(Shadow::soft(12.));
    /// assert_eq!(el.style_mut().shadow.as_ref().map(Vec::len), Some(2));
    /// ```
    fn shadow(mut self, s: Shadow) -> Self {
        self.style_mut().shadow.get_or_insert_default().push(s);
        self
    }
    /// Replace the whole shadow list.
    ///
    /// ```
    /// use mui_scene::prelude::*;
    /// let mut el = block(80., 24.).shadow(Shadow::soft(12.)).shadows([]);
    /// assert_eq!(el.style_mut().shadow, Some(vec![]));
    /// ```
    fn shadows(mut self, s: impl IntoIterator<Item = Shadow>) -> Self {
        self.style_mut().shadow = Some(s.into_iter().collect());
        self
    }
    /// The theme's shadow list for this step off the surface.
    ///
    /// ```
    /// use mui_scene::prelude::*;
    /// let mut el = block(80., 24.).elevation(Elevation::Floating);
    /// assert_eq!(el.style_mut().shadow.as_deref(), Some(Elevation::Floating.shadows()));
    /// ```
    fn elevation(self, e: Elevation) -> Self {
        self.shadows(e.shadows().iter().cloned())
    }
    /// Composite this node's whole subtree through `m`.
    ///
    /// Keeps whatever opacity was set; see [`Paints::opacity`].
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
    /// let mut el = block(10., 10.).blend(Mix::Multiply).opacity(0.5);
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
    /// let fade = Gradient::linear(180., [(0.8, Role::Surface.alpha(0.)), (1., Role::Surface.into())]);
    /// let mut list = col!["one", "two"].scroll().mask(fade);
    /// assert!(!list.style_mut().mask.is_none());
    /// ```
    fn mask(mut self, f: impl Into<Fill>) -> Self {
        self.style_mut().mask = Some(f.into());
        self
    }
    /// Before this node paints, blur whatever was painted behind it, clipped
    /// to its outline: the frosted dim under a modal. `radius` is the
    /// Gaussian's standard deviation in logical pixels, as CSS `blur()`.
    ///
    /// The renderer paints everything before this node a second time
    /// through a blur filter, so it costs about one more paint of the
    /// scene under it -- once per encode, not per presented frame. A canvas
    /// without filter layers skips it and shows the node's own fill alone,
    /// so pair it with a translucent fill that reads on its own.
    ///
    /// ```
    /// use mui_scene::prelude::*;
    /// let scrim = Color::oklcha(0., 0., 0., 0.6);
    /// let mut dim = stack![text("Save?")].fill(scrim).backdrop_blur(8.);
    /// assert_eq!(dim.style_mut().backdrop_blur, Some(8.));
    /// ```
    fn backdrop_blur(mut self, radius: impl Px) -> Self {
        self.style_mut().backdrop_blur = Some(radius.px());
        self
    }
    /// Merge a prepared style *over* this one: `.preset(card())`. Every
    /// field the preset states wins; the rest of the chain survives. Both
    /// are moved, never copied: pass `card.clone()` to keep one.
    ///
    /// ```
    /// use mui_scene::prelude::*;
    /// # use mui_scene::Style;
    /// let card = Style::default().radius(12.);
    /// let mut el = block(80., 24.).fill(Role::Primary).preset(card);
    /// assert_eq!(el.style_mut().radius, Some(Radius::Px(12.)));
    /// assert_eq!(el.style_mut().fill, Some(Fill::Role(Role::Primary)));
    /// ```
    fn preset(mut self, s: Style) -> Self {
        let slot = self.style_mut();
        *slot = std::mem::take(slot).over(s);
        self
    }
    /// Merge a prepared style *under* this one: a default the rest of the
    /// chain, before or after, is free to override.
    ///
    /// ```
    /// use mui_scene::prelude::*;
    /// # use mui_scene::Style;
    /// let card = Style::default().fill(Role::Raised);
    /// let mut el = block(80., 24.).fill(Role::Danger).base(card);
    /// assert_eq!(el.style_mut().fill, Some(Fill::Role(Role::Danger)));
    /// ```
    fn base(mut self, s: Style) -> Self {
        let slot = self.style_mut();
        *slot = s.over(std::mem::take(slot));
        self
    }
    /// Pointer shape over this node and, unless they say otherwise, its
    /// children.
    fn cursor(mut self, c: Cursor) -> Self {
        self.style_mut().cursor = Some(c);
        self
    }
    /// Apply `f` only when `cond`: `.when(selected, |e| e.fill(Primary))`.
    /// `.when(true, outlined)` hands the node to a reusable run of builders.
    fn when(self, cond: bool, f: impl FnOnce(Self) -> Self) -> Self {
        if cond { f(self) } else { self }
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
/// let save = block(64., 28.).a11y(A11y::Button).named("Save").tip("Write it out").id("save");
/// assert!(save.payload().extras().tip.is_some());
/// ```
pub trait Styled: Paints {
    fn element_mut(&mut self) -> &mut Element;

    /// Material-weld immediate non-floating, non-excluded plate children:
    /// their fills and borders blend into one image across the seams.
    /// `Weld::default()` blends both, `Weld::shape()` keeps the borders,
    /// `Weld::borders()` keeps the fills, `.morph(p)` and `.quality(q)` set
    /// the progress and the raster budget, and `Weld::off()` removes the
    /// weld. The backend is the host's choice: see
    /// [`SceneSpec::weld_backend`](crate::SceneSpec::weld_backend). For a
    /// shared vector outline that leaves each child's paint alone, see
    /// `mui_material::Material::union`.
    ///
    /// ```
    /// use mui_scene::prelude::*;
    /// let pair = row![block(20., 20.), block(20., 20.)].weld(Weld::shape().morph(0.5));
    /// assert!(pair.payload().extras().welding.is_some());
    /// assert!(pair.weld(Weld::off()).payload().extras().welding.is_none());
    /// ```
    fn weld(mut self, weld: crate::Weld) -> Self {
        let on = !weld.is_off();
        match &mut self.element_mut().extras {
            Some(x) if !on => x.welding = None,
            _ if !on => {}
            x => x.get_or_insert_default().welding = Some(weld),
        }
        self
    }
    /// Keep this child independent from its immediate parent's weld. Text and
    /// floating children are excluded automatically; layout is never removed.
    fn unwelded(mut self) -> Self {
        self.element_mut().set(Element::WELD_EXCLUDED, true);
        self
    }
    /// A custom closed shape in local logical units. Use opposite contour
    /// winding for holes. The shape becomes paint, clip and hit geometry.
    fn outline(mut self, shape: impl Fn(Size) -> Path + 'static) -> Self {
        self.element_mut().extras_mut().outline = Some(Outline(Arc::new(shape)));
        self
    }

    fn text_size(mut self, px: impl Px) -> Self {
        self.element_mut().text_size = Some(px.px());
        self
    }
    /// How heavy this label's glyphs are, as a position on the font's `wght`
    /// axis. Inherited by nothing: a weight is a property of the run, so a
    /// row of labels states it per label.
    ///
    /// ```
    /// use mui_scene::prelude::*;
    /// let heading = text("Oscillator").text_weight(Weight::BOLD);
    /// assert_eq!(heading.payload().axes.get("wght"), Some(700.));
    /// ```
    fn text_weight(self, w: Weight) -> Self {
        self.text_axis("wght", f32::from(w.value()))
    }
    /// Any variable-font axis by tag: `FILL`, `GRAD`, `wdth`, `slnt`. A tag
    /// the face lacks does nothing; a value outside its range is clamped.
    /// `opsz` follows the text size on its own unless set here.
    ///
    /// Animate it with a spring and set it every frame -- an axis left out
    /// snaps back to its default, it does not hold.
    ///
    /// ```
    /// use mui_scene::prelude::*;
    /// let star = text("\u{E838}").text_axis("FILL", 1.0);
    /// assert_eq!(star.payload().axes.get("FILL"), Some(1.0));
    /// ```
    fn text_axis(mut self, tag: &str, value: f32) -> Self {
        self.element_mut().axes.set(tag, value);
        self
    }
    /// A face for this node, tried before the scene's font. The scene's font
    /// and fallbacks still cover any glyph it lacks.
    fn font(mut self, font: mui_text::Font) -> Self {
        self.element_mut().font = Some(font);
        self
    }
    /// Material Symbols `FILL`, 0 outlined to 1 filled. Tween it for the
    /// morph. Sugar for [`Styled::text_axis`].
    fn icon_fill(self, fill: f32) -> Self {
        self.text_axis("FILL", fill)
    }
    /// Material Symbols `GRAD`, -50 to 200: weight without width, for
    /// emphasis or a light-on-dark correction. Sugar for [`Styled::text_axis`].
    fn grade(self, grade: f32) -> Self {
        self.text_axis("GRAD", grade)
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
    /// let wide = resolve(&spec).unwrap().surface("gain").unwrap().frame.size.width;
    /// let bare = SceneSpec::new(row![text("0.0 dB").id("gain")]);
    /// let bare = resolve(&bare).unwrap().surface("gain").unwrap().frame.size.width;
    /// assert!(wide > bare);
    /// ```
    fn reserve(mut self, s: impl Into<String>) -> Self {
        self.element_mut().extras_mut().reserve = Some(s.into());
        self
    }
    /// Declare what this node looks like while hovered, pressed or focused,
    /// beside what it looks like at rest. `f` is handed the resting style,
    /// so it edits rather than replaces, and a later `.fill(..)` is still
    /// what the state derives from.
    ///
    /// The runtime applies these while building the frame, to any node: an
    /// unnamed one that declares Hover or Press becomes a pointer target by
    /// its tree path, where other unnamed surfaces are decoration and never
    /// hit. Pair with [`Styled::animate`] to cross rather than cut.
    ///
    /// ```
    /// use mui_scene::prelude::*;
    /// let mut el = block(80., 24.).fill(Role::Field).on(State::Hover, |s| s.radius(4.)).id("b");
    /// assert_eq!(el.element_mut().states.len(), 1);
    /// ```
    fn on(mut self, state: State, f: impl Fn(Style) -> Style + 'static) -> Self {
        self.element_mut()
            .states
            .push((state, StateStyle(Arc::new(f))));
        self
    }
    fn tip(mut self, s: impl Into<Arc<str>>) -> Self {
        self.element_mut().extras_mut().tip = Some(s.into());
        self
    }
    /// What this node is, for accessibility: `.a11y(A11y::Button)`. Only a
    /// node with an id becomes a surface, so only one is ever reported.
    fn a11y(mut self, k: A11y) -> Self {
        let e = self.element_mut();
        match &mut e.semantics {
            Some(s) => s.role = k,
            none => *none = Some(Box::new(Semantics::new(k))),
        }
        self
    }
    /// The name read out with the role; the id otherwise.
    fn named(mut self, name: impl Into<Arc<str>>) -> Self {
        let e = self.element_mut();
        let s = e
            .semantics
            .get_or_insert_with(|| Box::new(Semantics::new(A11y::Group)));
        s.label = Some(name.into());
        self
    }
    fn focusable(mut self) -> Self {
        self.element_mut().set(Element::FOCUSABLE, true);
        self
    }
    /// Keep the wheel for this node: a timeline that zooms on the wheel
    /// inside a scrolling column reads it from `Response::wheel`, and the
    /// column does not scroll under it. Without this, every node under the
    /// pointer sees the wheel *and* the innermost scroller slides.
    fn captures_wheel(mut self) -> Self {
        self.element_mut().set(Element::CAPTURES_WHEEL, true);
        self
    }
    /// This node's tree reads the raw pointer (`Ui::local`, the host's own
    /// input), not just its hover: a hover readout, a crosshair. A move over
    /// it is never `Ui::inert`, so the host frames it.
    fn tracks_pointer(mut self) -> Self {
        self.element_mut().set(Element::TRACKS_POINTER, true);
        self
    }
    /// Switch this node -- and everything under it -- off: it drops out of
    /// hit testing and out of Tab, and it paints whatever it declared for
    /// [`State::Disabled`]. A greyed control that still drags is worse than
    /// no grey at all, so the look and the gate are one call. A condition
    /// goes through [`Paints::when`]: `.when(!module.enabled, Styled::disabled)`.
    ///
    /// ```
    /// use mui_scene::prelude::*;
    /// let bypassed = block(64., 28.)
    ///     .fill(Role::Primary)
    ///     .on(State::Disabled, |s| s.fill(Role::Ink.alpha(0.2)))
    ///     .disabled()
    ///     .id("osc-2");
    /// assert!(bypassed.payload().has(mui_scene::Element::DISABLED));
    /// ```
    fn disabled(mut self) -> Self {
        self.element_mut().set(Element::DISABLED, true);
        self
    }
    /// Spring this node's paint toward whatever it is next declared to be,
    /// instead of cutting. The runtime keys the springs by the node's id, or
    /// by its tree path when it has none.
    ///
    /// Paint only -- fill colour, stroke width, `Px` radius, text size,
    /// shadow blur, `Px` shell depths. Sizes, gaps, padding and layout
    /// frames are **not** transitioned: they are solved fresh every frame,
    /// and springing them would fight the layout rather than decorate it.
    /// Animate a position by springing the value you feed the tree instead
    /// (see `Ui::tween`). [`Styled::animate`] is this with the default spring.
    fn animate_with(mut self, s: Spring) -> Self {
        self.element_mut().extras_mut().transition = Some(s);
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
        self.element_mut().set(Element::BASELINE, true);
        self
    }
    /// Cap a wrapping label at `n` lines; the last one ends in an ellipsis.
    fn lines(mut self, n: usize) -> Self {
        self.element_mut().lines = Some(n.max(1));
        self
    }
    /// Hide the overlay scrollbar a `.scroll()` node shows while it
    /// overflows. On by default: a thin `Ink` thumb over the far edge of the
    /// viewport that thickens under the pointer and drags. The `Ui` runtime
    /// owns it, so a scene resolved without one paints none. Switch it off
    /// where the list draws its own position, or where a `.mask()` fade
    /// already says there is more.
    fn no_scrollbar(mut self) -> Self {
        self.element_mut().set(Element::SCROLL_BAR_OFF, true);
        self
    }
    /// [`Styled::animate_with`] with the default spring.
    fn animate(self) -> Self {
        self.animate_with(Spring::DEFAULT)
    }
    /// Spring the node's solved frame -- where layout put it and how big --
    /// instead of cutting when the layout changes: a panel opening, a toggle
    /// knob changing sides, a rack slot reordered. The solve itself is
    /// untouched; only the frame this node is painted, clipped and hit at
    /// chases it, so nothing fights the layout.
    ///
    /// Children ride along: a child that animates too springs relative to
    /// this node, one that does not simply moves with it. The node's own
    /// shape is rebuilt at the in-between frame, so a rounded panel grows as
    /// a rounded panel rather than a stretched bitmap. Key by id (or tree
    /// path): renaming the node restarts it.
    ///
    /// ```
    /// use mui_scene::prelude::*;
    /// let knob = block(16., 16.).pill().animate_layout();
    /// assert!(knob.payload().extras().layout_transition.is_some());
    /// ```
    fn animate_layout(self) -> Self {
        self.animate_layout_with(Spring::DEFAULT)
    }
    /// [`Styled::animate_layout`] with your own spring.
    fn animate_layout_with(mut self, s: Spring) -> Self {
        self.element_mut().extras_mut().layout_transition = Some(s);
        self
    }
    /// Come in from `from` and fade in on the first frame the node exists;
    /// fade out where it stood on the frame it is gone. Implies
    /// [`Styled::animate`] and [`Styled::animate_layout`] with the default
    /// spring for whichever it has not set.
    ///
    /// The exit needs an id: the runtime keeps the node's last paint under
    /// that name for as long as it takes to fade.
    ///
    /// ```
    /// use mui_scene::prelude::*;
    /// let toast = text("Saved").appear(Appear::Slide(0., 12.)).id("toast");
    /// assert!(toast.payload().extras().transition.is_some());
    /// ```
    fn appear(mut self, from: Appear) -> Self {
        let e = self.element_mut().extras_mut();
        e.appear = Some(from);
        e.transition.get_or_insert(Spring::DEFAULT);
        e.layout_transition.get_or_insert(Spring::DEFAULT);
        self
    }
    /// What this node is, independent of its name: a rack slot's module,
    /// not its position. When a node with the same identity comes back under
    /// another name -- `osc/3` became `osc/1` because the rack was reordered
    /// -- the runtime carries everything it kept under the old name to the
    /// new one: springs, a layout glide (so the slot *moves* to its new place
    /// instead of both slots cross-fading), focus, scroll offsets, a text
    /// selection, and a drag in flight. Names composed under it
    /// (`osc/3/gain`) and unnamed descendants follow too.
    ///
    /// ```
    /// use mui_scene::prelude::*;
    /// let module_uid = 7u64;
    /// let slot = block(80., 40.).animate_layout().identity(module_uid).id("osc/3");
    /// assert!(slot.payload().extras().identity.is_some());
    /// ```
    fn identity(mut self, what: impl std::hash::Hash) -> Self {
        use std::hash::{BuildHasher, BuildHasherDefault, DefaultHasher};
        self.element_mut().extras_mut().identity =
            Some(BuildHasherDefault::<DefaultHasher>::default().hash_one(what));
        self
    }
    /// Name this node's shape. When the name changes -- a play glyph becomes
    /// a pause, a pill becomes a circle, a tab becomes a panel -- the outline
    /// morphs from the old shape into the new one over the node's
    /// transition spring ([`Spring::DEFAULT`] without one) instead of
    /// cutting. The same name frame to frame follows the node's shape
    /// directly, so springs on its radius or frame are left alone.
    ///
    /// ```
    /// use mui_scene::prelude::*;
    /// let playing = true;
    /// let icon = block(24., 24.).morph(if playing { "pause" } else { "play" }).id("transport");
    /// assert!(icon.payload().extras().morph.is_some());
    /// ```
    fn morph(mut self, shape: impl std::hash::Hash) -> Self {
        use std::hash::{BuildHasher, BuildHasherDefault, DefaultHasher};
        self.element_mut().extras_mut().morph =
            Some(BuildHasherDefault::<DefaultHasher>::default().hash_one(shape));
        self
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
    use crate::{Content, Element, Style};
    use std::cell::Cell;
    use std::rc::Rc;
    use std::sync::Arc;

    fn card() -> Style {
        Style::default().fill(Role::Raised).radius(12.)
    }

    #[test]
    fn cached_canvas_reuses_only_the_same_key_and_size() {
        let cache = CanvasCache::new();
        let calls = Rc::new(Cell::new(0));
        let make = |key| {
            let calls = Rc::clone(&calls);
            canvas_keyed(&cache, key, move |size| {
                calls.set(calls.get() + 1);
                vec![Draw::fill(
                    Path::polyline(
                        [
                            Point::ZERO,
                            Point::new(size.width, 0.),
                            Point::new(size.width, size.height),
                            Point::new(0., size.height),
                        ],
                        true,
                    ),
                    Role::Primary,
                )]
            })
        };
        let size = Size::new(80., 40.);
        let first = make(7);
        let Content::Canvas(first) = &first.payload().content else {
            panic!("a canvas")
        };
        let a = (first.0)(size);
        let b = (first.0)(size);
        assert!(Arc::ptr_eq(&a, &b));
        assert_eq!(calls.get(), 1);

        let changed = make(8);
        let Content::Canvas(changed) = &changed.payload().content else {
            panic!("a canvas")
        };
        let _ = (changed.0)(size);
        let _ = (changed.0)(Size::new(81., 40.));
        assert_eq!(calls.get(), 3);
    }

    /// Every builder call moves the node by value, so its size is the price
    /// of every `.fill(..)`: rare fields live in `Extras` and `Rare` boxes.
    #[test]
    fn nodes_stay_small() {
        use std::mem::size_of;
        assert!(size_of::<Element>() <= 336, "{}", size_of::<Element>());
        assert!(size_of::<El>() <= 664, "{}", size_of::<El>());
    }

    /// The whole merge rule: per field, the side that states something wins,
    /// and which side that is depends only on which method was called.
    #[test]
    fn preset_wins_per_field_and_base_loses_per_field() {
        let mut over = block(10., 10.)
            .fill(Role::Primary)
            .stroke(Role::Ink)
            .preset(card());
        let s = over.style_mut();
        assert_eq!((s.fill.clone(), s.radius), (card().fill, card().radius));
        assert!(s.stroke.is_some(), "a field the preset left unset survives");

        let mut under = block(10., 10.).fill(Role::Primary).base(card());
        let s = under.style_mut();
        assert_eq!(
            (s.fill.clone(), s.radius),
            (Some(Role::Primary.into()), card().radius)
        );
    }

    /// One slot per concept: a second spelling of the same thing replaces
    /// the first, and a different thing does not.
    #[test]
    fn radius_and_stroke_keep_one_slot_each() {
        assert_eq!(
            block(10., 10.).pill().radius(8.).style_mut().radius,
            Some(Radius::Px(8.))
        );
        assert_eq!(
            block(10., 10.).radius(8.).pill().style_mut().radius,
            Some(Radius::Pill)
        );
        let mut el = block(10., 10.).stroke_width(2.).stroke(Role::Ink);
        let stroke = el.style_mut().stroke.clone().expect("set");
        assert_eq!(
            (stroke.fill, stroke.width),
            (Some(Role::Ink.into()), Some(2.))
        );
        // A width over a bordered base keeps the base's colour.
        let bordered = Style {
            stroke: Some(crate::Stroke {
                fill: Some(Role::Danger.into()),
                width: Some(1.),
            }),
            ..Style::default()
        };
        let mut el = block(10., 10.).base(bordered.clone()).stroke_width(2.);
        let stroke = el.style_mut().stroke.clone().expect("set");
        assert_eq!(
            (stroke.fill, stroke.width),
            (Some(Role::Danger.into()), Some(2.))
        );
        let mut el = block(10., 10.).stroke_width(2.).base(bordered);
        let stroke = el.style_mut().stroke.clone().expect("set");
        assert_eq!(
            (stroke.fill, stroke.width),
            (Some(Role::Danger.into()), Some(2.))
        );
    }

    /// A segmented strip says it once, on the container: the children go
    /// square when the scene resolves, so one pushed after `.segmented()`
    /// is squared too, and the strip's own corner rounds the two ends.
    #[test]
    fn segmented_squares_children_pushed_after_it() {
        let strip = row![block(60., 28.).radius(8.).id("a")]
            .radius(12.)
            .segmented()
            .push(block(60., 28.).radius(8.).id("b"))
            .id("strip");
        assert!(strip.is_clip(), "the ends are rounded by the clip");
        let s = resolve(&SceneSpec::new(strip)).unwrap();
        let r = |k: &str| s.surface(k).unwrap().rect.unwrap().radius();
        assert_eq!((r("a"), r("b"), r("strip")), (0., 0., 12.));
    }

    /// A role at an alpha is still the role: it tracks the palette, and it
    /// is not the literal colour a light-mode eyedropper would give.
    #[test]
    fn a_faded_role_resolves_through_the_palette() {
        let p = crate::Palette::NEUTRAL;
        let under = p.surface();
        let Some(crate::Paint::Solid(c)) = Role::Ink.alpha(0.12).paint(&p, under) else {
            panic!("a faded role paints solid");
        };
        assert!((c.alpha() - 0.12).abs() < 1e-6);
        assert_eq!(c.with_alpha(1.0), p.on(under));
    }
}
