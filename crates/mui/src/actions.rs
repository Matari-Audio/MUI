//! Platform accessibility requests, independent of pointer hit testing.

/// A host asks for an operation on an existing, enabled semantic control.
/// Queue with `Ui::request_action`, then build the normal widget tree and frame.
#[derive(Clone, Debug, PartialEq)]
pub enum SemanticAction {
    Focus { id: String },
    Activate { id: String },
    /// Value in the control's semantic units, not an implicitly normalized value.
    SetValue { id: String, value: f64 },
}
impl SemanticAction {
    pub fn focus(id: impl Into<String>) -> Self {
        Self::Focus { id: id.into() }
    }
    pub fn activate(id: impl Into<String>) -> Self {
        Self::Activate { id: id.into() }
    }
    pub fn set_value(id: impl Into<String>, value: f64) -> Self {
        Self::SetValue { id: id.into(), value }
    }
    pub(crate) fn id(&self) -> &str {
        match self {
            Self::Focus { id } | Self::Activate { id } | Self::SetValue { id, .. } => id,
        }
    }
}
