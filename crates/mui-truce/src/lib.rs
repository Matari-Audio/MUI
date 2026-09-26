//! MUI editors inside truce plugins.
//!
//! [`MuiEditor`] is truce's `Editor`: a child window under the host's, a
//! wgpu surface, and a `Ui` frame per native event. [`Bridge`] binds widget
//! ids to truce parameters, so a drag is the host's begin/perform/end and
//! host automation is the value the next tree reads. [`window`] is the half
//! that knows no plugin framework.
#![deny(unsafe_code)]
pub mod bridge;
pub use bridge::{Bridge, widget_id};

#[cfg(not(target_arch = "wasm32"))]
mod editor;
#[cfg(not(target_arch = "wasm32"))]
mod platform;
#[cfg(not(target_arch = "wasm32"))]
pub mod window;
#[cfg(not(target_arch = "wasm32"))]
pub use editor::MuiEditor;
