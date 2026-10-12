//! Native interactive window move/resize for top-level (floating) windows.
//!
//! Parented plugin embeds cannot use these protocols — the host owns the
//! embed rect. Callers should fall back to host `request_resize` there.

use std::fmt;

/// Edge or corner used to start an interactive resize.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ResizeEdge {
    North,
    South,
    East,
    West,
    NorthEast,
    NorthWest,
    SouthEast,
    SouthWest,
}

impl ResizeEdge {
    /// EWMH `_NET_WM_MOVERESIZE` direction values.
    #[cfg(target_os = "linux")]
    pub(crate) const fn x11_moveresize_direction(self) -> u32 {
        match self {
            Self::NorthWest => 0,
            Self::North => 1,
            Self::NorthEast => 2,
            Self::East => 3,
            Self::SouthEast => 4,
            Self::South => 5,
            Self::SouthWest => 6,
            Self::West => 7,
        }
    }

    #[cfg(target_os = "macos")]
    pub(crate) const fn affects_width(self) -> bool {
        !matches!(self, Self::North | Self::South)
    }

    #[cfg(target_os = "macos")]
    pub(crate) const fn affects_height(self) -> bool {
        !matches!(self, Self::East | Self::West)
    }

    #[cfg(target_os = "macos")]
    pub(crate) const fn grows_from_west(self) -> bool {
        matches!(self, Self::West | Self::NorthWest | Self::SouthWest)
    }

    #[cfg(target_os = "macos")]
    pub(crate) const fn grows_from_south(self) -> bool {
        matches!(self, Self::South | Self::SouthEast | Self::SouthWest)
    }
}

/// Why a native drag-resize / drag-move could not start.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum DragResizeError {
    /// Window is a host-parented child; the WM/OS will not resize it.
    NotTopLevel,
    /// Platform has no interactive resize-start API (caller may DIY).
    NotSupported,
    /// Missing pointer state needed by the compositor (e.g. Wayland serial).
    NoActivePointer,
    /// Underlying OS call failed.
    Os(String),
}

impl fmt::Display for DragResizeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotTopLevel => write!(f, "native drag-resize requires a top-level window"),
            Self::NotSupported => write!(f, "native drag-resize is not supported on this platform"),
            Self::NoActivePointer => write!(f, "native drag-resize needs an active pointer press"),
            Self::Os(msg) => write!(f, "native drag-resize failed: {msg}"),
        }
    }
}

impl std::error::Error for DragResizeError {}
