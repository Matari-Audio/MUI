//! The retained GPU renderer ([`GpuRenderer`], the quad pass behind a
//! present) and the persistent analytic weld effects it samples.
//!
//! A morph update changes one 16-byte uniform lane. Weld materials render
//! into their own textures before the frame and are sampled at their
//! paint-list positions.
//!
//! Device/queue ownership is per renderer. Recreate this object on device loss;
//! do not carry textures or retained frames across devices.

mod pool;
pub(crate) mod renderer;
#[cfg(test)]
mod tests;

pub use pool::{ABSENT_FRAMES, Budget, EffectStats, WeldTextures};
pub use renderer::{Fallback, GpuRenderer};
pub const WELD_SHADER: &str = include_str!("weld.wgsl");
/// The present pass: the retained frame onto the window's view.
pub const PRESENT_SHADER: &str = include_str!("present.wgsl");

/// Run `create` (pipeline creation) one at a time per process on DX12, where
/// concurrent shader compilation from several editors has faulted drivers;
/// elsewhere it just runs.
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

mod timing;
pub use timing::{GpuTimer, GpuTiming, TimingTicket};
