//! How a MUI value gets from where it is to where it was just told to be.
//!
//! Two independent things, both pure maths over `f64`/`f32` and `std` alone:
//! a [`Spring`], the second-order chase every animated paint channel is
//! stepped by, [`Keys`], keyframes that land on a time, and [`curve`], normalized single-valued cubic Beziers for
//! response shapers and their editors.
//!
//! Nothing here knows about time sources, frames or trees: a caller owns the
//! state and hands in a `dt`. It is not the runtime that drives the springs
//! (that is `mui`), not the element DSL (`mui-scene`), and not a renderer.
#![forbid(unsafe_code)]

pub mod curve;
mod keys;
mod spring;

pub use keys::{Ease, Keys};
pub use spring::Spring;
