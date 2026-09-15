//! The payload a layout node carries, and the words that build a tree.
//!
//! ```
//! use mui_core::styled::prelude::*;
//! let card = column([text("Cutoff"), text("1.2 kHz").fill(Role::Dim)])
//!     .gap(S)
//!     .pad(M)
//!     .fill(Role::Raised)
//!     .shell(4.0, Role::Field);
//! assert_eq!(card.children().len(), 2);
//! ```
use super::style::{Fill, Radius, Shadow, Stroke, Style};
use mui_layout::generic::Node;
use mui_layout::Spacing;

#[derive(Clone, Debug, Default, PartialEq)]
pub enum Content {
    #[default]
    None,
    Text(String),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Element {
    pub style: Style,
    pub content: Content,
    /// Text size in pixels; `None` is the theme's.
    pub text_size: Option<f64>,
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
}
impl Styled for El {
    fn style_mut(&mut self) -> &mut Style {
        &mut self.payload_mut().style
    }
    fn element_mut(&mut self) -> &mut Element {
        self.payload_mut()
    }
}
