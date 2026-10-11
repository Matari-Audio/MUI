//! Desktop-selected Linux file dialogs without a renderer or GUI toolkit.
//!
//! Enable `xdg-portal` on Linux. Callers retain their native Windows/macOS
//! implementation and cancel editor-owned jobs before destroying the parent.

use std::path::PathBuf;

/// A copied portal identity, never a borrowed native window/display pointer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Parent {
    X11(u32),
    /// An already exported xdg-foreign handle. Its GUI owner keeps the export
    /// alive until the request finishes or is cancelled.
    Wayland(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DialogKind {
    OpenFile { multiple: bool },
    SaveFile { file_name: Option<String> },
    PickFolder { multiple: bool },
}

/// File extensions without a leading dot, e.g. `wav`, `kurvy`, or `*`.
/// An empty extension list offers no filter; these are not MIME types.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DialogFilter {
    pub name: String,
    pub extensions: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct DialogRequest {
    pub parent: Option<Parent>,
    pub title: String,
    pub kind: DialogKind,
    pub directory: Option<PathBuf>,
    pub filters: Vec<DialogFilter>,
}

/// `Ok(None)` is cancellation; transport/backend/invalid-response errors are
/// `Err`. The caller reads or writes the selected paths itself.
pub type DialogResult = Result<Option<Vec<PathBuf>>, String>;

#[cfg(all(target_os = "linux", feature = "xdg-portal"))]
mod portal;
#[cfg(all(target_os = "linux", feature = "xdg-portal"))]
pub use portal::{CancelHandle, DialogJob, DialogService};
