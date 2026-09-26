//! Renderer-independent polygon Boolean operations and adaptive circular fillets.
//!
//! Coordinates are group-local logical units. Apply input affine transforms BEFORE
//! the Boolean operation; radii are measured in the resulting coordinate space.
//! Nothing in this crate knows about egui, wgpu, windowing, input or colors.
//!
//! Supported input: closed polygon exteriors with optional polygon holes. A hole
//! means subtraction from its exterior; it is not an independent negative shape.
//! Native Bezier-curve Boolean operations and distance-based "goo" are not included.
#![forbid(unsafe_code)]

mod bezier;
mod boolean;
pub use bezier::{ARC_TOLERANCE, bez_path, bez_path_into};
pub use kurbo;
mod fillet;
mod math;
mod nesting;
mod offset;
mod regions;
pub use regions::{
    BorderAlign, BorderGeometry, ShapeSplit, SplitAxis, WidthProfile, boolean_paths, border_band,
    border_geometry, boundary_band, union_contours,
};
mod morph;
pub use morph::morph;
mod path;
pub use nesting::{InsetRect, RoundedRect};
pub use offset::{
    InsetShape, OffsetOptions, OffsetShape, boundary_distance, inset_path, offset_path, outset_path,
};

pub use boolean::{
    BooleanOp, GeometryOptions, PlacedShape, Polygon, Ring, RingKind, Topology, boolean,
    boolean_passes, union,
};
pub use fillet::{Corner, CornerStyle, Fillet, RoundedShape, fillet};
pub use math::{Affine, Point, Rect, Vec2, bounds};
pub use path::{Arc, Path, PathCommand};

#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    InvalidOptions(&'static str),
    NonFinite,
    CoordinateLimit,
    DegenerateRing,
    TooManyVertices,
    TooManySegments,
    InvalidPath,
    SelfIntersection,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidOptions(s) => write!(f, "invalid geometry option: {s}"),
            Self::NonFinite => f.write_str("a coordinate, radius or transform is NaN or infinite"),
            Self::CoordinateLimit => {
                f.write_str("coordinates exceed the configured limit; use group-local coordinates")
            }
            Self::DegenerateRing => {
                f.write_str("a polygon ring has fewer than three usable, non-collinear vertices")
            }
            Self::TooManyVertices => f.write_str("the configured vertex limit was exceeded"),
            Self::TooManySegments => f.write_str("path flattening would exceed its segment budget"),
            Self::SelfIntersection => {
                f.write_str("input rings must be simple; crossing or retracing edges were found")
            }
            Self::InvalidPath => f.write_str("invalid or non-closed vector path"),
        }
    }
}
impl std::error::Error for Error {}

#[cfg(test)]
mod tests;
