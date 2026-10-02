//! Immutable vector artwork, independent of its file format and device scale.
use crate::Image;
use kurbo::{Affine, BezPath, Stroke};
use peniko::{BlendMode, Brush, Fill};
use std::sync::Arc;

/// Device-resolution intermediate for effects that require raster compositing.
/// Implementations retain their vector source and bound/cache requested sizes.
/// The returned image uses the same straight RGBA contract as all MUI images.
pub trait RasterSource: std::fmt::Debug + Send + Sync {
    /// Never decode, rasterize or wait for a worker here. A miss schedules
    /// preparation and returns None; paint omits that pending intermediate.
    fn prepared_image(&self, width: u32, height: u32) -> Result<Option<Image>, String>;
    /// Preparation-worker boundary; true when image/error readiness changed.
    fn prepare_requested(&self) -> Result<bool, String> {
        Ok(false)
    }
    fn set_waker(&self, _wake: Arc<dyn Fn() + Send + Sync>) {}
    fn wake_pending(&self) -> bool {
        false
    }
    /// Changes when a prepared image or terminal error is installed.
    fn revision(&self) -> u64 {
        0
    }
    fn retained_bytes(&self) -> usize;
}

#[derive(Clone, Debug)]
pub enum VectorCommand {
    Fill {
        path: BezPath,
        transform: Affine,
        brush: Brush,
        brush_transform: Affine,
        rule: Fill,
    },
    Stroke {
        path: BezPath,
        transform: Affine,
        brush: Brush,
        brush_transform: Affine,
        stroke: Stroke,
    },
    PushLayer {
        path: BezPath,
        transform: Affine,
        blend: BlendMode,
        alpha: f32,
    },
    PopLayer,
}

#[derive(Clone, Debug)]
pub struct Vector {
    pub width: f64,
    pub height: f64,
    commands: Arc<[VectorCommand]>,
    raster: Option<Arc<dyn RasterSource>>,
}
impl Vector {
    /// Reject invalid extents and unbalanced layers before a paint walk.
    pub fn new(width: f64, height: f64, commands: Vec<VectorCommand>) -> Option<Self> {
        if ![width, height].into_iter().all(|n| n.is_finite() && n > 0.) {
            return None;
        }
        let mut layers = 0usize;
        for command in &commands {
            match command {
                VectorCommand::PushLayer { .. } => layers += 1,
                VectorCommand::PopLayer => layers = layers.checked_sub(1)?,
                VectorCommand::Fill {
                    brush: Brush::Image(_),
                    ..
                }
                | VectorCommand::Stroke {
                    brush: Brush::Image(_),
                    ..
                } => return None,
                _ => {}
            }
        }
        (layers == 0).then(|| Self {
            width,
            height,
            commands: commands.into(),
            raster: None,
        })
    }
    /// Retain a vector source with effects requiring a device-sized intermediate.
    pub fn filtered(width: f64, height: f64, source: Arc<dyn RasterSource>) -> Option<Self> {
        let mut vector = Self::new(width, height, Vec::new())?;
        vector.raster = Some(source);
        Some(vector)
    }
    pub fn raster_source(&self) -> Option<&Arc<dyn RasterSource>> {
        self.raster.as_ref()
    }
    pub fn commands(&self) -> &[VectorCommand] {
        &self.commands
    }
}
// The same retained command buffer is the same artwork. Never scan paths during
// the renderer's changed-scene check.
impl PartialEq for Vector {
    fn eq(&self, other: &Self) -> bool {
        self.width == other.width
            && self.height == other.height
            && Arc::ptr_eq(&self.commands, &other.commands)
            && match (&self.raster, &other.raster) {
                (None, None) => true,
                (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                _ => false,
            }
    }
}
