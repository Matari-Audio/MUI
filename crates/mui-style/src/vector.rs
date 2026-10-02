//! Immutable vector artwork, independent of its file format and device scale.
use kurbo::{Affine, BezPath, Stroke};
use peniko::{BlendMode, Brush, Fill};
use std::sync::Arc;

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
        })
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
    }
}
