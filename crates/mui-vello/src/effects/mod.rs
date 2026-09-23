//! Persistent analytic effects and retained Vello Hybrid encoding.
//!
//! No CPU pixel bake or readback exists in the presentation path. A morph update
//! changes one 16-byte uniform lane. Effects are rendered BEFORE the Vello pass
//! in the same command encoder and sampled at their actual paint-list positions.
//!
//! Device/queue ownership is per renderer. Recreate this object on device loss;
//! do not carry texture IDs, bindings or retained encodings across devices.
use std::sync::Arc;

use mui_geometry::Path;

pub mod damage;
mod pool;
mod retained;
mod tiled;
pub use tiled::{TileStats, TiledEffects};
#[cfg(test)]
mod tests;

pub use pool::{Budget, EffectStats, WeldTextures, ABSENT_FRAMES};
pub use retained::HybridEffects;
pub const WELD_SHADER: &str = include_str!("weld.wgsl");

#[derive(Debug)]
pub enum Error {
    Parameters(mui_weld::Error),
    Geometry(mui_geometry::Error),
    Budget(&'static str),
    Unsupported(&'static str),
    Missing(String),
    Device(String),
    Render(String),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parameters(e) => write!(f, "GPU weld: {e}"),
            Self::Geometry(e) => write!(f, "GPU scene: {e}"),
            Self::Budget(s) => write!(f, "GPU effect budget: {s}"),
            Self::Unsupported(s) => write!(f, "unsupported GPU effect: {s}"),
            Self::Missing(s) => write!(f, "missing external surface: {s}"),
            Self::Device(s) => write!(f, "GPU initialization: {s}"),
            Self::Render(s) => write!(f, "GPU render: {s}"),
        }
    }
}
impl std::error::Error for Error {}
impl From<mui_weld::Error> for Error {
    fn from(e: mui_weld::Error) -> Self {
        Self::Parameters(e)
    }
}
impl From<mui_geometry::Error> for Error {
    fn from(e: mui_geometry::Error) -> Self {
        Self::Geometry(e)
    }
}

/// Each paint entry's path as a `BezPath`, kept across frames beside the
/// path it came from: an entry whose path did not change -- the same `Arc`,
/// or an equal path -- is not converted again. Index `i` is paint entry `i`.
#[derive(Default)]
pub(crate) struct Converted(Vec<(Arc<Path>, crate::kurbo::BezPath)>);
impl Converted {
    pub fn resize(&mut self, len: usize) {
        self.0.resize_with(len, Default::default);
    }
    pub fn get(&mut self, i: usize, path: &Arc<Path>) -> Result<&crate::kurbo::BezPath, Error> {
        let (source, bez) = &mut self.0[i];
        if !Arc::ptr_eq(source, path) {
            if **source != **path {
                if let Err(e) = crate::bez_path_into(path, crate::ARC_TOLERANCE, bez) {
                    // Leave a pair that still matches: the empty path, empty.
                    *source = Arc::default();
                    bez.truncate(0);
                    return Err(e.into());
                }
            }
            *source = Arc::clone(path);
        }
        Ok(bez)
    }
}

mod timing;
pub use timing::{GpuTimer, GpuTiming, TimingTicket};
