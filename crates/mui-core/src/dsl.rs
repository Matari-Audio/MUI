//! Concise Rust authoring helpers. They intentionally hide solver types.
use crate::{Align, Insets, Justify, Node, Size};

pub fn leaf(id: impl Into<String>, width: f64, height: f64) -> Node {
    Node::leaf(id, Size::new(width, height))
}
pub fn row(id: impl Into<String>, children: impl IntoIterator<Item = Node>) -> Node {
    Node::row(id, children)
}
pub fn column(id: impl Into<String>, children: impl IntoIterator<Item = Node>) -> Node {
    Node::column(id, children)
}
pub fn stack(id: impl Into<String>, children: impl IntoIterator<Item = Node>) -> Node {
    Node::overlay(id, children)
}

pub trait NodeExt: Sized {
    fn pad(self, v: f64) -> Self;
    fn pad_xy(self, x: f64, y: f64) -> Self;
    fn inset(self, i: Insets) -> Self;
    fn gap_by(self, v: f64) -> Self;
    fn center(self) -> Self;
    fn stretch(self) -> Self;
    fn justify(self, j: Justify) -> Self;
    fn min(self, w: f64, h: f64) -> Self;
    fn max(self, w: f64, h: f64) -> Self;
    fn grow_by(self, w: f64) -> Self;
}
impl NodeExt for Node {
    fn pad(self, v: f64) -> Self {
        self.padding(v)
    }
    fn pad_xy(self, x: f64, y: f64) -> Self {
        self.padding_xy(x, y)
    }
    fn inset(self, i: Insets) -> Self {
        self.insets(i)
    }
    fn gap_by(self, v: f64) -> Self {
        self.gap(v)
    }
    fn center(self) -> Self {
        self.align(Align::Center).justify(Justify::Center)
    }
    fn stretch(self) -> Self {
        self.align(Align::Stretch)
    }
    fn justify(self, j: Justify) -> Self {
        Node::justify(self, j)
    }
    fn min(self, w: f64, h: f64) -> Self {
        self.min_size(Size::new(w, h))
    }
    fn max(self, w: f64, h: f64) -> Self {
        self.max_size(Size::new(w, h))
    }
    fn grow_by(self, w: f64) -> Self {
        self.grow(w)
    }
}

/// Compact container; `.axis(Axis::Auto)` chooses row or column.
pub fn flow(children: impl IntoIterator<Item = Node>) -> Node {
    Node::flow(children)
}
