//! Screen-edge placement for toolkit-owned top-level windows.

/// Monitor edge occupied by a desktop panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum DockEdge {
    Top,
    Bottom,
}

/// A dock's requested geometry, in compositor logical pixels.
///
/// The platform owns the accepted width and position. A missing output selects
/// the platform's default monitor. Docking is valid only for detached windows.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct DockOptions {
    pub edge: DockEdge,
    pub height: u32,
    pub output: Option<String>,
}

impl DockOptions {
    /// Requests a full-width panel at an edge, with a bounded logical height.
    #[must_use]
    pub fn new(edge: DockEdge, height: u32) -> Self {
        Self { edge, height: height.clamp(100, 1024), output: None }
    }

    /// Selects a named output instead of the platform's default.
    #[must_use]
    pub fn with_output(mut self, output: Option<String>) -> Self {
        self.output = output.filter(|name| !name.is_empty());
        self
    }
}
