//! Screen-edge placement for toolkit-owned top-level windows.

/// Monitor edge occupied by a desktop panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum DockEdge {
    Top,
    Bottom,
    Left,
    Right,
}

impl DockEdge {
    pub(crate) fn is_vertical(self) -> bool {
        matches!(self, Self::Left | Self::Right)
    }
}

/// A dock's requested geometry, in compositor logical pixels.
///
/// `height` is the strip thickness: the panel height for [`DockEdge::Top`] and
/// [`DockEdge::Bottom`], the panel width for [`DockEdge::Left`] and
/// [`DockEdge::Right`]. The panel always spans the full monitor extent along its
/// edge. The platform owns the accepted thickness and position. A missing output
/// selects the platform's default monitor. Docking is valid only for detached windows.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct DockOptions {
    pub edge: DockEdge,
    pub height: u32,
    pub output: Option<String>,
}

impl DockOptions {
    /// Requests a full-length panel at an edge, with a bounded logical thickness.
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

    /// Thickness selected by a requested size: its height for horizontal edges,
    /// its width for vertical edges, bounded like [`DockOptions::new`].
    pub(crate) fn thickness(&self, size: crate::dpi::LogicalSize<f64>) -> u32 {
        let requested = if self.edge.is_vertical() { size.width } else { size.height };
        (requested.round() as u32).clamp(100, 1024)
    }
}
