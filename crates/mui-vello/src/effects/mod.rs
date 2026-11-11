//! The retained GPU renderer ([`GpuRenderer`], the quad pass behind a
//! present) and the persistent analytic weld effects it samples.
//!
//! A morph update changes one 16-byte uniform lane. Weld materials render
//! into their own textures before the frame and are sampled at their
//! paint-list positions.
//!
//! Device/queue ownership is per renderer. Recreate this object on device loss;
//! do not carry textures or retained frames across devices.
//!
//! `vello-classic` keeps classic Vello's compute renderer as
//! [`VelloRenderer`] for benches and comparisons only.
#[cfg(feature = "vello-classic")]
use std::sync::Arc;

#[cfg(feature = "vello-classic")]
use mui_geometry::Path;

mod pool;
pub(crate) mod renderer;
#[cfg(feature = "vello-classic")]
pub(crate) mod retained;
#[cfg(test)]
mod tests;

pub use pool::{ABSENT_FRAMES, Budget, EffectStats, WeldTextures};
pub use renderer::{Fallback, GpuRenderer};
#[cfg(feature = "vello-classic")]
pub use retained::VelloRenderer;
pub const WELD_SHADER: &str = include_str!("weld.wgsl");
/// The present pass: the retained frame onto the window's view.
pub const PRESENT_SHADER: &str = include_str!("present.wgsl");

/// Run `create` (pipeline creation) one at a time per process on DX12, where
/// concurrent shader compilation from several editors has faulted drivers;
/// elsewhere it just runs.
#[cfg(feature = "vello-classic")]
pub(crate) use vello::with_pipeline_creation as gate;
#[cfg(not(feature = "vello-classic"))]
pub(crate) fn gate<T>(device: &wgpu::Device, create: impl FnOnce() -> T) -> T {
    static DX12: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _serial = (device.adapter_info().backend == wgpu::Backend::Dx12).then(|| {
        DX12.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    });
    create()
}

#[derive(Debug)]
pub enum Error {
    /// Another editor is submitting on the shared classic-Vello renderer
    /// (`vello-classic` only); retry a paced frame.
    Busy,
    Parameters(mui_weld::Error),
    Geometry(mui_geometry::Error),
    Budget(&'static str),
    Unsupported(&'static str),
    Missing(String),
    Device(String),
    Render(String),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Busy => f.write_str("shared GPU renderer busy"),
            Self::Parameters(e) => write!(f, "GPU weld: {e}"),
            Self::Geometry(e) => write!(f, "GPU scene: {e}"),
            Self::Budget(s) => write!(f, "GPU effect budget: {s}"),
            Self::Unsupported(s) => write!(f, "unsupported GPU effect: {s}"),
            Self::Missing(s) => write!(f, "missing external surface: {s}"),
            Self::Device(s) => write!(f, "GPU initialization: {s}"),
            Self::Render(s) => write!(f, "GPU render: {s}"),
        }
    }
}
impl std::error::Error for Error {}
impl From<mui_weld::Error> for Error {
    fn from(e: mui_weld::Error) -> Self {
        Self::Parameters(e)
    }
}
impl From<mui_geometry::Error> for Error {
    fn from(e: mui_geometry::Error) -> Self {
        Self::Geometry(e)
    }
}

/// Each path as a `BezPath`, by the `Arc` it came from: an outline shared
/// by same-sized nodes, or kept by one that only moved, converts once. The
/// entry holds its `Arc`, so the address cannot be reused while it is a key;
/// a path no frame asked for in [`AGE`] frames goes.
#[cfg(feature = "vello-classic")]
#[derive(Default)]
pub(crate) struct Converted {
    map: rustc_hash::FxHashMap<usize, Conversion>,
    frame: u64,
}

#[cfg(feature = "vello-classic")]
struct Conversion {
    /// Held so the pointer key cannot be reused while the entry lives.
    _path: Arc<Path>,
    bez: crate::kurbo::BezPath,
    /// The frame it was last asked for.
    used: u64,
    /// Its last [`Converted::fill`], in the colour that was asked for.
    fill: Option<(vello::peniko::Color, vello::Scene)>,
}

/// Commands from which a plain fill is encoded once and copied after: a
/// glyph run or an icon. A rect costs less to encode than to copy.
#[cfg(feature = "vello-classic")]
pub(crate) const REPLAYED: usize = 16;

/// Frames a conversion outlives its last use; see [`Converted`].
#[cfg(feature = "vello-classic")]
const AGE: u64 = 32;

#[cfg(feature = "vello-classic")]
impl Converted {
    /// Start a frame; every [`AGE`] frames, drop what went unused.
    pub fn tick(&mut self) {
        self.frame += 1;
        if self.frame.is_multiple_of(AGE) {
            let now = self.frame;
            self.map.retain(|_, e| now - e.used <= AGE);
        }
    }
    pub fn get(&mut self, path: &Arc<Path>) -> Result<&crate::kurbo::BezPath, Error> {
        Ok(&self.entry(path)?.bez)
    }

    fn entry(&mut self, path: &Arc<Path>) -> Result<&mut Conversion, Error> {
        use std::collections::hash_map::Entry;
        let e = match self.map.entry(Arc::as_ptr(path) as usize) {
            Entry::Occupied(e) => e.into_mut(),
            Entry::Vacant(v) => {
                let bez = crate::bez_path(path, crate::ARC_TOLERANCE)?;
                v.insert(Conversion {
                    _path: path.clone(),
                    bez,
                    used: 0,
                    fill: None,
                })
            }
        };
        e.used = self.frame;
        Ok(e)
    }

    /// `path` filled with `color` at the origin, encoded once and kept while
    /// the colour stays: an entry that only moved is a copy under its new
    /// transform, the same words a fresh encode would write.
    pub fn fill(
        &mut self,
        path: &Arc<Path>,
        color: vello::peniko::Color,
    ) -> Result<&vello::Scene, Error> {
        let e = self.entry(path)?;
        if e.fill.as_ref().is_none_or(|(c, _)| *c != color) {
            let mut scene = vello::Scene::new();
            let fill = vello::peniko::Fill::NonZero;
            scene.fill(fill, crate::kurbo::Affine::IDENTITY, color, None, &e.bez);
            e.fill = Some((color, scene));
        }
        Ok(&e.fill.as_ref().expect("filled above").1)
    }
}

mod timing;
pub use timing::{GpuTimer, GpuTiming, TimingTicket};
