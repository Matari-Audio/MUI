//! Material and surface verbs for MUI's element tree.
//!
//! The data these verbs write (`Style::shells`, `Extras::border_ramp`,
//! `Extras::surface_padding`, ..) lives in `mui-scene`, and so does the code
//! that resolves it: surfaces, regions, border ramps and material welds read
//! and write the scene walk's frames, paint list and per-node caches at a
//! dozen points mid-walk, so they stay beside it. This crate owns the
//! vocabulary: the [`Material`] extension trait, re-exported by the `mui`
//! prelude and by [`prelude`] here, plus paint-only capture of a resolved
//! scene ([`Capture`], [`resize_capture`]), which needs nothing from the walk.
#![forbid(unsafe_code)]

use mui_scene::{BorderRamp, Carve, El, Fill, Id, Spacing, Styled};

mod capture;
mod verbs;
pub use capture::{Capture, CaptureError, CaptureLayer, resize_capture};
pub use verbs::Material;

pub mod prelude {
    pub use crate::{Capture, Material};
    pub use mui_scene::prelude::*;
}
