use crate::dpi::{PhysicalPosition, PhysicalSize};

/// External damage reported by the native window before its next draw.
#[non_exhaustive]
#[derive(Debug, Copy, Clone)]
pub enum DamageArea<'a> {
    FullWindow,
    Rect(DamageRect),
    Rects(&'a [DamageRect]),
}

/// A damaged rectangle in physical client-area pixels.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct DamageRect {
    position: PhysicalPosition<u32>,
    size: PhysicalSize<u32>,
}

impl DamageRect {
    pub const fn new(position: PhysicalPosition<u32>, size: PhysicalSize<u32>) -> Self {
        Self { position, size }
    }

    pub fn position(&self) -> PhysicalPosition<u32> {
        self.position
    }

    pub fn size(&self) -> PhysicalSize<u32> {
        self.size
    }
}
