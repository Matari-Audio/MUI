//! MUI's non-real-time control/document contract for Truce plugins.
//! Truce owns parameter metadata, atomics, host transport and the state envelope.
pub mod document;
pub mod parameter;
pub use document::{Document, EditorState, Error, Module, Route, Target};
pub use parameter::{Automation, Edit, Parameter};
