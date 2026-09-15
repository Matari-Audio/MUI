//! The payload a layout node carries, and the words that build a tree.
//!
//! ```
//! use mui_core::prelude::*;
//! let card = column([text("Cutoff"), text("1.2 kHz").fill(Role::Dim)])
//!     .gap(S)
//!     .pad(M)
//!     .fill(Role::Raised)
//!     .shell(4.0, Role::Field);
//! assert_eq!(card.children().len(), 2);
//! ```
use crate::motion::Spring;
use crate::style::{Cursor, Fill, Radius, Shadow, Stroke, Style};
use mui_geometry::Path;
use mui_layout::{Node, Size, Spacing};
use std::sync::Arc;

/// One stroke or fill a canvas hands back, in the canvas's own pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct Draw {
    pub path: Path,
    pub fill: Fill,
    /// Stroke width; `0` fills.
    pub width: f64,
}
impl Draw {
    pub fn fill(path: Path, fill: impl Into<Fill>) -> Self {
        Self {
            path,
            fill: fill.into(),
            width: 0.0,
        }
    }
    pub fn stroke(path: Path, fill: impl Into<Fill>, width: f64) -> Self {
        Self {
            path,
            fill: fill.into(),
            width,
        }
    }
}

/// Custom drawing: called with the node's size every frame, in the walk.
#[derive(Clone)]
pub struct Canvas(pub Arc<dyn Fn(Size) -> Vec<Draw> + Send + Sync>);
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

#[derive(Clone, Debug, Default, PartialEq)]
pub enum Content {
    #[default]
    None,
    Text(String),
    Canvas(Canvas),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Element {
    pub style: Style,
    pub content: Content,
    /// Text size in pixels; `None` is the theme's.
    pub text_size: Option<f64>,
    /// Shown after the pointer rests on the node.
    pub tip: Option<String>,
    /// Takes keyboard focus on click and on Tab.
    pub focusable: bool,
    /// A row whose text children share one baseline. See [`Styled::baseline`].
    pub baseline: bool,
    /// Cap a wrapped label at this many lines. See [`Styled::lines`].
    pub lines: Option<usize>,
    /// The spring this node's paint chases when its declared style changes.
    /// Only meaningful on a node with an id: the runtime has nothing to
    /// compare an anonymous node against. See [`Styled::transition`].
    pub transition: Option<Spring>,
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
pub fn canvas(f: impl Fn(Size) -> Vec<Draw> + Send + Sync + 'static) -> El {
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

/// Paint builders on any `El`. One trait, so `.fill(..)` chains after
/// `.gap(..)` in either order.
pub trait Styled: Sized {
    fn style_mut(&mut self) -> &mut Style;
    fn element_mut(&mut self) -> &mut Element;

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
    fn shadow(mut self, s: Shadow) -> Self {
        self.style_mut().shadow = Some(s);
        self
    }
    /// A ring `d` inside the previous outline, painted `f`. Stack them for
    /// constant-thickness nesting.
    fn shell(mut self, d: impl Into<Spacing>, f: impl Into<Fill>) -> Self {
        self.style_mut().shells.push((d.into(), f.into()));
        self
    }
    /// Paint the union of the children's frames as one filleted shape.
    fn weld(mut self, f: impl Into<Fill>) -> Self {
        let s = self.style_mut();
        s.weld = true;
        s.fill = f.into();
        self
    }
    fn text_size(mut self, px: f64) -> Self {
        self.element_mut().text_size = Some(px);
        self
    }
    /// Copy a prepared style: `.styled(&CARD)`.
    fn styled(mut self, s: &Style) -> Self {
        *self.style_mut() = s.clone();
        self
    }
    /// Pointer shape over this node and, unless they say otherwise, its
    /// children.
    fn cursor(mut self, c: Cursor) -> Self {
        self.style_mut().cursor = Some(c);
        self
    }
    fn tip(mut self, s: impl Into<String>) -> Self {
        self.element_mut().tip = Some(s.into());
        self
    }
    fn focusable(mut self) -> Self {
        self.element_mut().focusable = true;
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
    /// Apply `f` only when `cond`: `.when(selected, |e| e.fill(Primary))`.
    fn when(self, cond: bool, f: impl FnOnce(Self) -> Self) -> Self {
        if cond {
            f(self)
        } else {
            self
        }
    }
}
impl Styled for El {
    fn style_mut(&mut self) -> &mut Style {
        &mut self.payload_mut().style
    }
    fn element_mut(&mut self) -> &mut Element {
        self.payload_mut()
    }
}
