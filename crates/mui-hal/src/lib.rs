//! MUI's owned, render-only GPU HAL.
//!
//! One API, named and shaped after the subset of wgpu MUI's renderer uses,
//! so the renderer ports by path. Handles are cheap clones that dispatch
//! over backends:
//!
//! - Vulkan (`ash`, Linux): Vulkan 1.3 dynamic rendering, SPIR-V from
//!   `mui-shaders`, presentation through `mui-present`'s `VkSwapchain`.
//! - Metal (`objc2-metal`, macOS): MSL 2.3 from `mui-shaders`, presentation
//!   through `CAMetalLayer`.
//! - D3D11 (`windows`, Windows): HLSL SM 5.0 / precompiled DXBC from
//!   `mui-shaders`, presentation through `mui-present`'s flip-model DXGI 1.2
//!   swapchain (`CreateSwapChainForHwnd` for opaque, DirectComposition for
//!   translucent).
//! - wgpu (feature `wgpu`): passthrough for web, media and wgpu-sharing plugins, and interim
//!   default on macOS and Windows while OS-CI parity is pending. It also wraps
//!   a device a host made itself (`Device::from_wgpu`).
//!
//! An [`Instance`] opens the native backends it can and asks wgpu for the
//! rest; `MUI_HAL_BACKEND=wgpu` sends everything through wgpu (A/B runs), and
//! `MUI_HAL_BACKEND=metal` / `MUI_HAL_BACKEND=d3d11` / `MUI_HAL_BACKEND=native`
//! selects the native backend on macOS/Windows even when `wgpu` is enabled.
//!
//! Object creation and submission failures on native backends are reported to
//! per-thread error scopes (or the device's uncaptured error handler) like
//! wgpu, returning invalid placeholder handles that fail cleanly if used later.

#![deny(unsafe_code)]
// wgpu-core's nesting overflows the default when proving `DeviceShared: Send`.
#![recursion_limit = "256"]
// On single-backend builds (wasm with wgpu only, or native targets without wgpu)
// enum matches degenerate to one variant.
#![cfg_attr(
    not(all(
        any(target_os = "linux", target_os = "macos", target_os = "windows"),
        feature = "wgpu"
    )),
    allow(
        irrefutable_let_patterns,
        unused_mut,
        unused_variables,
        clippy::needless_return,
        clippy::unnecessary_find_map,
        reason = "single-backend build"
    )
)]

pub(crate) mod binding;
#[cfg(target_os = "windows")]
pub mod d3d11;
#[cfg(target_os = "macos")]
pub mod metal;
pub(crate) mod shared;
mod types;
#[cfg(target_os = "linux")]
pub mod vulkan;
#[cfg(feature = "wgpu")]
mod wgpu_backend;

use std::future::Future;
use std::ops::{Bound as RangeBound, Range, RangeBounds};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

pub use mui_shaders::ShaderId;
pub use types::*;
#[cfg(feature = "wgpu")]
pub use wgpu;

#[cfg(target_os = "windows")]
use d3d11 as d3d;
#[cfg(target_os = "macos")]
use metal as mtl;
#[cfg(target_os = "linux")]
use vulkan as vk;
#[cfg(feature = "wgpu")]
use wgpu_backend as wg;

#[cfg(not(any(
    target_os = "linux",
    target_os = "macos",
    target_os = "windows",
    feature = "wgpu"
)))]
compile_error!("mui-hal has no backend for this target: enable its `wgpu` feature");

/// Device-level errors: what an error scope or the uncaptured handler sees.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    OutOfMemory { source: String },
    Validation { description: String },
    Internal { description: String },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OutOfMemory { source } => write!(f, "out of memory: {source}"),
            Self::Validation { description } => write!(f, "validation error: {description}"),
            Self::Internal { description } => write!(f, "internal error: {description}"),
        }
    }
}

impl std::error::Error for Error {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequestDeviceError(pub String);

impl std::fmt::Display for RequestDeviceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "requesting a device failed: {}", self.0)
    }
}

impl std::error::Error for RequestDeviceError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequestAdapterError(pub String);

impl std::fmt::Display for RequestAdapterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "no adapter: {}", self.0)
    }
}

impl std::error::Error for RequestAdapterError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BufferAsyncError;

impl std::fmt::Display for BufferAsyncError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("buffer mapping failed")
    }
}

impl std::error::Error for BufferAsyncError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PollError(pub String);

impl std::fmt::Display for PollError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "polling the device failed: {}", self.0)
    }
}

impl std::error::Error for PollError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreateSurfaceError(pub String);

impl std::fmt::Display for CreateSurfaceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "creating a surface failed: {}", self.0)
    }
}

impl std::error::Error for CreateSurfaceError {}

/// Validation-layer errors reported so far (Vulkan with
/// `MUI_HAL_VALIDATION=1`); tests assert it stays zero.
pub fn validation_errors() -> usize {
    #[cfg(target_os = "linux")]
    {
        vk::VALIDATION_ERRORS.load(std::sync::atomic::Ordering::Relaxed)
    }
    #[cfg(not(target_os = "linux"))]
    {
        0
    }
}

#[cfg(all(test, target_os = "linux"))]
use shared::lock;
use shared::{ErrorSink, Scope, UncapturedHandler};

/// A future that is either ready or wgpu's (which may not be `Send`).
pub struct Pending<T>(PendingInner<T>);

enum PendingInner<T> {
    Ready(Option<T>),
    #[cfg(feature = "wgpu")]
    Boxed(Pin<Box<dyn Future<Output = T>>>),
}

impl<T> Pending<T> {
    fn ready(v: T) -> Self {
        Self(PendingInner::Ready(Some(v)))
    }
}

impl<T: Unpin> Future for Pending<T> {
    type Output = T;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<T> {
        match &mut self.0 {
            PendingInner::Ready(v) => {
                let _ = cx;
                Poll::Ready(v.take().expect("polled after completion"))
            }
            #[cfg(feature = "wgpu")]
            PendingInner::Boxed(f) => f.as_mut().poll(cx),
        }
    }
}

bitflags::bitflags! {
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
    pub struct InstanceFlags: u32 {
        const DEBUG = 1 << 0;
        const VALIDATION = 1 << 1;
    }
}

#[derive(Clone, Debug)]
pub struct InstanceDescriptor {
    pub backends: Backends,
    pub flags: InstanceFlags,
    /// Open native backends where they exist. `false` routes everything
    /// through wgpu (with the `wgpu` feature).
    pub native: bool,
    /// Options for the wgpu backends (DX12 presentation and compiler).
    #[cfg(feature = "wgpu")]
    pub backend_options: wgpu::BackendOptions,
}

impl InstanceDescriptor {
    /// The backends `WGPU_BACKEND` lists (as wgpu reads it), else every one;
    /// validation when `MUI_HAL_VALIDATION=1`; everything through wgpu when
    /// `MUI_HAL_BACKEND=wgpu` or with the `wgpu-only` feature. On macOS and
    /// Windows when `wgpu` is compiled in, `wgpu` stays default until OS-CI
    /// parity passes, while `MUI_HAL_BACKEND=metal|d3d11|native` (or
    /// `WGPU_BACKEND=d3d11`) opts into the native backend.
    pub fn from_env() -> Self {
        let validation = std::env::var_os("MUI_HAL_VALIDATION").is_some_and(|v| v == "1");
        let env_backend = std::env::var("MUI_HAL_BACKEND").ok();
        let backends = std::env::var("WGPU_BACKEND")
            .map_or_else(|_| Backends::all(), |v| Backends::from_comma_list(&v));
        let native = binding::resolve_native_selection(
            env_backend.as_deref(),
            backends,
            cfg!(feature = "wgpu"),
            cfg!(feature = "wgpu-only"),
            cfg!(target_os = "linux"),
        );
        Self {
            backends,
            flags: if validation {
                InstanceFlags::VALIDATION | InstanceFlags::DEBUG
            } else {
                InstanceFlags::empty()
            },
            native,
            #[cfg(feature = "wgpu")]
            backend_options: wgpu::BackendOptions::from_env_or_default(),
        }
    }

    /// wgpu's name for [`InstanceDescriptor::from_env`].
    pub fn new_without_display_handle_from_env() -> Self {
        Self::from_env()
    }
}

impl Default for InstanceDescriptor {
    fn default() -> Self {
        Self::from_env()
    }
}

/// The backends native to this build (no wgpu).
pub fn native_backends() -> Backends {
    if cfg!(target_os = "linux") {
        Backends::VULKAN
    } else if cfg!(target_os = "macos") {
        Backends::METAL
    } else if cfg!(target_os = "windows") {
        Backends::D3D11
    } else {
        Backends::empty()
    }
}

#[derive(Clone)]
enum InstanceInner {
    #[cfg(target_os = "linux")]
    Vulkan(Result<vk::Instance, vk::Error>),
    #[cfg(target_os = "macos")]
    Metal(Result<mtl::Instance, mtl::Error>),
    #[cfg(target_os = "windows")]
    D3d11(Result<d3d::Instance, d3d::Error>),
    #[cfg(feature = "wgpu")]
    Wgpu(wgpu::Instance, Backends),
}

#[derive(Clone)]
pub struct Instance {
    inner: Vec<InstanceInner>,
}

/// What [`Instance::request_adapter`] looks for.
#[derive(Clone, Copy, Debug, Default)]
pub struct RequestAdapterOptions<'s, 'w> {
    pub power_preference: PowerPreference,
    /// Only a software adapter: none here (MUI renders on the CPU instead).
    pub force_fallback_adapter: bool,
    /// Prefer adapters that can present to this surface.
    pub compatible_surface: Option<&'s Surface<'w>>,
}

impl Instance {
    /// The adapter `options` prefers: a discrete GPU for high performance,
    /// an integrated one for low power, else the first offered.
    pub fn request_adapter(
        &self,
        options: &RequestAdapterOptions<'_, '_>,
    ) -> Pending<Result<Adapter, RequestAdapterError>> {
        let preference = options.power_preference;
        let fallback = options.force_fallback_adapter;
        // A surface presents only through adapters of its own backend.
        // ponytail: backend match only; per-adapter presentation support
        // needs the VkSurfaceKHR the swapchain makes at `configure`.
        let native_surface = options.compatible_surface.map(Surface::is_native);
        // A wgpu surface: wgpu's own selection checks surface support
        // before ranking.
        #[cfg(feature = "wgpu")]
        if let (
            Some(Surface {
                inner: SurfaceInner::Wgpu(surface),
                ..
            }),
            Some(instance),
        ) = (options.compatible_surface, self.as_wgpu())
        {
            let found = instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: match preference {
                    PowerPreference::None => wgpu::PowerPreference::None,
                    PowerPreference::LowPower => wgpu::PowerPreference::LowPower,
                    PowerPreference::HighPerformance => wgpu::PowerPreference::HighPerformance,
                },
                force_fallback_adapter: fallback,
                compatible_surface: Some(surface),
                ..Default::default()
            });
            return Pending(PendingInner::Boxed(Box::pin(async move {
                found
                    .await
                    .map(|a| Adapter(AdapterInner::Wgpu(a)))
                    .map_err(|e| RequestAdapterError(e.to_string()))
            })));
        }
        let found = self.enumerate_adapters(Backends::all());
        let pick = move |adapters: Vec<Adapter>| {
            let rank = |a: &Adapter| match (preference, a.get_info().device_type) {
                (_, DeviceType::Cpu) if fallback => 0,
                (_, DeviceType::Cpu) => 9,
                (PowerPreference::HighPerformance, DeviceType::DiscreteGpu)
                | (PowerPreference::LowPower, DeviceType::IntegratedGpu) => 1,
                _ => 2,
            };
            adapters
                .into_iter()
                .filter(|a| !fallback || a.get_info().device_type == DeviceType::Cpu)
                .filter(|a| native_surface.is_none_or(|n| n == a.is_native()))
                .min_by_key(rank)
                .ok_or_else(|| RequestAdapterError("no adapter matches the request".into()))
        };
        match found.0 {
            PendingInner::Ready(v) => Pending::ready(pick(v.unwrap_or_default())),
            #[cfg(feature = "wgpu")]
            PendingInner::Boxed(f) => {
                Pending(PendingInner::Boxed(Box::pin(async move { pick(f.await) })))
            }
        }
    }

    #[cfg_attr(
        not(feature = "wgpu"),
        expect(
            clippy::needless_pass_by_value,
            reason = "wgpu's signature; it consumes"
        )
    )]
    pub fn new(desc: InstanceDescriptor) -> Self {
        let mut inner = Vec::new();
        let mut rest = desc.backends;
        #[cfg(target_os = "linux")]
        if desc.native && !cfg!(feature = "wgpu-only") && desc.backends.contains(Backends::VULKAN) {
            let vulkan = vk::Instance::new(desc.flags.contains(InstanceFlags::VALIDATION));
            if vulkan.is_ok() {
                rest -= Backends::VULKAN;
            }
            inner.push(InstanceInner::Vulkan(vulkan));
        }
        #[cfg(target_os = "macos")]
        if desc.native && !cfg!(feature = "wgpu-only") && desc.backends.contains(Backends::METAL) {
            let metal = mtl::Instance::new();
            if metal.is_ok() {
                rest -= Backends::METAL;
            }
            inner.push(InstanceInner::Metal(metal));
        }
        #[cfg(target_os = "windows")]
        if desc.native && !cfg!(feature = "wgpu-only") && desc.backends.contains(Backends::D3D11) {
            let d3d11 = d3d::Instance::new(desc.flags.contains(InstanceFlags::VALIDATION));
            if d3d11.is_ok() {
                rest -= Backends::D3D11;
            }
            inner.push(InstanceInner::D3d11(d3d11));
        }
        #[cfg(feature = "wgpu")]
        if !rest.is_empty() {
            let mut d = wgpu::InstanceDescriptor::new_without_display_handle_from_env();
            d.backends &= wg::backends(rest);
            d.flags = if desc.flags.contains(InstanceFlags::VALIDATION) {
                wgpu::InstanceFlags::VALIDATION | wgpu::InstanceFlags::DEBUG
            } else {
                wgpu::InstanceFlags::VALIDATION_INDIRECT_CALL
            };
            d.backend_options = desc.backend_options;
            inner.push(InstanceInner::Wgpu(wgpu::Instance::new(d), rest));
        }
        let _ = rest;
        Self { inner }
    }

    /// The backends this build can open: native ones plus, with the `wgpu`
    /// feature, those wgpu was compiled with.
    pub fn enabled_backend_features() -> Backends {
        #[cfg_attr(not(feature = "wgpu"), expect(unused_mut, reason = "wgpu adds to it"))]
        let mut backends = native_backends();
        #[cfg(feature = "wgpu")]
        {
            backends |= wg::from_backends(wgpu::Instance::enabled_backend_features());
        }
        backends
    }

    /// An instance over a wgpu instance a host made.
    #[cfg(feature = "wgpu")]
    pub fn from_wgpu(instance: wgpu::Instance) -> Self {
        Self {
            inner: vec![InstanceInner::Wgpu(instance, Backends::all())],
        }
    }

    /// The wgpu instance in use, if any.
    #[cfg(feature = "wgpu")]
    pub fn as_wgpu(&self) -> Option<&wgpu::Instance> {
        self.inner.iter().find_map(|i| match i {
            InstanceInner::Wgpu(w, _) => Some(w),
            #[cfg(target_os = "linux")]
            InstanceInner::Vulkan(_) => None,
            #[cfg(target_os = "macos")]
            InstanceInner::Metal(_) => None,
            #[cfg(target_os = "windows")]
            InstanceInner::D3d11(_) => None,
        })
    }

    /// Why a requested native backend could not start, if one could not.
    pub fn backend_errors(&self) -> Vec<String> {
        self.inner
            .iter()
            .filter_map(|i| match i {
                #[cfg(target_os = "linux")]
                InstanceInner::Vulkan(r) => r.as_ref().err().map(ToString::to_string),
                #[cfg(target_os = "macos")]
                InstanceInner::Metal(r) => r.as_ref().err().map(ToString::to_string),
                #[cfg(target_os = "windows")]
                InstanceInner::D3d11(r) => r.as_ref().err().map(ToString::to_string),
                #[cfg(feature = "wgpu")]
                InstanceInner::Wgpu(..) => None,
            })
            .collect()
    }

    pub fn enumerate_adapters(&self, backends: Backends) -> Pending<Vec<Adapter>> {
        let mut out = Vec::new();
        for i in &self.inner {
            match i {
                #[cfg(target_os = "linux")]
                InstanceInner::Vulkan(Ok(instance)) if backends.contains(Backends::VULKAN) => {
                    out.extend(
                        instance
                            .enumerate_adapters()
                            .into_iter()
                            .map(|a| Adapter(AdapterInner::Vulkan(a))),
                    );
                }
                #[cfg(target_os = "linux")]
                InstanceInner::Vulkan(_) => {}
                #[cfg(target_os = "macos")]
                InstanceInner::Metal(Ok(instance)) if backends.contains(Backends::METAL) => {
                    out.extend(
                        instance
                            .enumerate_adapters()
                            .into_iter()
                            .map(|a| Adapter(AdapterInner::Metal(a))),
                    );
                }
                #[cfg(target_os = "macos")]
                InstanceInner::Metal(_) => {}
                #[cfg(target_os = "windows")]
                InstanceInner::D3d11(Ok(instance)) if backends.contains(Backends::D3D11) => {
                    out.extend(
                        instance
                            .enumerate_adapters()
                            .into_iter()
                            .map(|a| Adapter(AdapterInner::D3d11(a))),
                    );
                }
                #[cfg(target_os = "windows")]
                InstanceInner::D3d11(_) => {}
                #[cfg(feature = "wgpu")]
                InstanceInner::Wgpu(w, covered) => {
                    let wanted = wg::backends(backends & *covered);
                    if wanted.is_empty() {
                        continue;
                    }
                    let found = w.enumerate_adapters(wanted);
                    let native = out;
                    return Pending(PendingInner::Boxed(Box::pin(async move {
                        let mut all = native;
                        all.extend(
                            found
                                .await
                                .into_iter()
                                .map(|a| Adapter(AdapterInner::Wgpu(a))),
                        );
                        all
                    })));
                }
            }
        }
        Pending::ready(out)
    }

    /// A surface on `target`'s window, kept alive inside the surface.
    pub fn create_surface<'w>(
        &self,
        target: impl raw_window_handle::HasWindowHandle
        + raw_window_handle::HasDisplayHandle
        + Send
        + Sync
        + 'w,
    ) -> Result<Surface<'w>, CreateSurfaceError> {
        #[cfg(target_os = "linux")]
        if let Some(instance) = self.vulkan() {
            let handle = |e: raw_window_handle::HandleError| CreateSurfaceError(e.to_string());
            let window = target.window_handle().map_err(handle)?.as_raw();
            let display = target.display_handle().map_err(handle)?.as_raw();
            // SAFETY: `target` owns the window and lives inside the surface,
            // which drops its swapchain first.
            #[expect(unsafe_code, reason = "raw handles kept alive by `target`")]
            let surface = unsafe { vk::Surface::new(instance, display, window) }
                .map_err(|e| CreateSurfaceError(e.to_string()))?;
            return Ok(Surface {
                inner: SurfaceInner::Vulkan(Arc::new(surface)),
                _not_sync: std::marker::PhantomData,
                _target: Some(Box::new(target)),
            });
        }
        #[cfg(target_os = "macos")]
        if self.metal().is_some() {
            let handle = |e: raw_window_handle::HandleError| CreateSurfaceError(e.to_string());
            let window = target.window_handle().map_err(handle)?.as_raw();
            let display = target.display_handle().map_err(handle)?.as_raw();
            // SAFETY: `target` owns the NSView and lives inside the surface,
            // which detaches its CAMetalLayer before `_target` drops.
            #[expect(unsafe_code, reason = "raw handles kept alive by `target`")]
            let surface = unsafe { mtl::Surface::new(display, window) }
                .map_err(|e| CreateSurfaceError(e.to_string()))?;
            return Ok(Surface {
                inner: SurfaceInner::Metal(Arc::new(surface)),
                _not_sync: std::marker::PhantomData,
                _target: Some(Box::new(target)),
            });
        }
        #[cfg(target_os = "windows")]
        if self.d3d11().is_some() {
            let handle = |e: raw_window_handle::HandleError| CreateSurfaceError(e.to_string());
            let window = target.window_handle().map_err(handle)?.as_raw();
            let display = target.display_handle().map_err(handle)?.as_raw();
            // SAFETY: `target` owns the HWND and lives inside the surface,
            // which releases its DXGI swapchain presenter before `_target` drops.
            #[expect(unsafe_code, reason = "raw handles kept alive by `target`")]
            let surface = unsafe { d3d::Surface::new(display, window) }
                .map_err(|e| CreateSurfaceError(e.to_string()))?;
            return Ok(Surface {
                inner: SurfaceInner::D3d11(Arc::new(surface)),
                _not_sync: std::marker::PhantomData,
                _target: Some(Box::new(target)),
            });
        }
        #[cfg(feature = "wgpu")]
        if let Some(instance) = self.as_wgpu() {
            let surface = instance
                .create_surface(wgpu::SurfaceTarget::from(target))
                .map_err(|e| CreateSurfaceError(e.to_string()))?;
            return Ok(Surface::from_wgpu(surface));
        }
        let _ = target;
        Err(CreateSurfaceError(
            "no backend can present to this window".into(),
        ))
    }

    /// The Vulkan instance, when the Vulkan backend started.
    #[cfg(target_os = "linux")]
    pub fn vulkan(&self) -> Option<&vk::Instance> {
        self.inner.iter().find_map(|i| match i {
            InstanceInner::Vulkan(Ok(v)) => Some(v),
            _ => None,
        })
    }

    /// The Metal instance, when the Metal backend started.
    #[cfg(target_os = "macos")]
    pub fn metal(&self) -> Option<&mtl::Instance> {
        self.inner.iter().find_map(|i| match i {
            InstanceInner::Metal(Ok(m)) => Some(m),
            _ => None,
        })
    }

    /// The D3D11 instance, when the D3D11 backend started.
    #[cfg(target_os = "windows")]
    pub fn d3d11(&self) -> Option<&d3d::Instance> {
        self.inner.iter().find_map(|i| match i {
            InstanceInner::D3d11(Ok(d)) => Some(d),
            _ => None,
        })
    }
}

#[derive(Clone)]
enum AdapterInner {
    #[cfg(target_os = "linux")]
    Vulkan(vk::Adapter),
    #[cfg(target_os = "macos")]
    Metal(mtl::Adapter),
    #[cfg(target_os = "windows")]
    D3d11(d3d::Adapter),
    #[cfg(feature = "wgpu")]
    Wgpu(wgpu::Adapter),
}

#[derive(Clone)]
pub struct Adapter(AdapterInner);

impl std::fmt::Debug for Adapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Adapter")
            .field(&self.get_info().name)
            .finish()
    }
}

impl Adapter {
    pub fn get_info(&self) -> AdapterInfo {
        match &self.0 {
            #[cfg(target_os = "linux")]
            AdapterInner::Vulkan(a) => a.get_info(),
            #[cfg(target_os = "macos")]
            AdapterInner::Metal(a) => a.get_info(),
            #[cfg(target_os = "windows")]
            AdapterInner::D3d11(a) => a.get_info(),
            #[cfg(feature = "wgpu")]
            AdapterInner::Wgpu(a) => wg::adapter_info(&a.get_info()),
        }
    }

    pub fn limits(&self) -> Limits {
        match &self.0 {
            #[cfg(target_os = "linux")]
            AdapterInner::Vulkan(a) => a.limits(),
            #[cfg(target_os = "macos")]
            AdapterInner::Metal(a) => a.limits(),
            #[cfg(target_os = "windows")]
            AdapterInner::D3d11(a) => a.limits(),
            #[cfg(feature = "wgpu")]
            AdapterInner::Wgpu(a) => wg::limits(&a.limits()),
        }
    }

    pub fn features(&self) -> Features {
        match &self.0 {
            #[cfg(target_os = "linux")]
            AdapterInner::Vulkan(a) => a.features(),
            #[cfg(target_os = "macos")]
            AdapterInner::Metal(a) => a.features(),
            #[cfg(target_os = "windows")]
            AdapterInner::D3d11(a) => a.features(),
            #[cfg(feature = "wgpu")]
            AdapterInner::Wgpu(a) => wg::features(a.features()),
        }
    }

    /// The usages `format` allows on this adapter.
    pub fn get_texture_format_features(&self, format: TextureFormat) -> TextureFormatFeatures {
        match &self.0 {
            // Vulkan 1.3, Metal, and D3D11 make every format MUI names mandatory
            // for sampling, copies and (bar Rgba32Float) colour attachments.
            #[cfg(target_os = "linux")]
            AdapterInner::Vulkan(_) => TextureFormatFeatures {
                allowed_usages: match format {
                    TextureFormat::Rgba32Float => {
                        TextureUsages::all() - TextureUsages::RENDER_ATTACHMENT
                    }
                    _ => TextureUsages::all(),
                },
            },
            #[cfg(target_os = "macos")]
            AdapterInner::Metal(_) => TextureFormatFeatures {
                allowed_usages: match format {
                    TextureFormat::Rgba32Float => {
                        TextureUsages::all() - TextureUsages::RENDER_ATTACHMENT
                    }
                    _ => TextureUsages::all(),
                },
            },
            #[cfg(target_os = "windows")]
            AdapterInner::D3d11(_) => TextureFormatFeatures {
                allowed_usages: match format {
                    TextureFormat::Rgba32Float => {
                        TextureUsages::all() - TextureUsages::RENDER_ATTACHMENT
                    }
                    _ => TextureUsages::all(),
                },
            },
            #[cfg(feature = "wgpu")]
            AdapterInner::Wgpu(a) => TextureFormatFeatures {
                allowed_usages: wg::from_texture_usages(
                    a.get_texture_format_features(wg::format(format))
                        .allowed_usages,
                ),
            },
        }
    }

    pub fn request_device(
        &self,
        desc: &DeviceDescriptor<'_>,
    ) -> Pending<Result<(Device, Queue), RequestDeviceError>> {
        match &self.0 {
            #[cfg(target_os = "linux")]
            AdapterInner::Vulkan(a) => Pending::ready(
                a.request_device(desc)
                    .map(|d| Device::new(DeviceInner::Vulkan(vk::Owner(d)), a.get_info()))
                    .map_err(|e| RequestDeviceError(e.to_string())),
            ),
            #[cfg(target_os = "macos")]
            AdapterInner::Metal(a) => Pending::ready(
                a.request_device(desc)
                    .map(|d| Device::new(DeviceInner::Metal(mtl::Owner(d)), a.get_info()))
                    .map_err(|e| RequestDeviceError(e.to_string())),
            ),
            #[cfg(target_os = "windows")]
            AdapterInner::D3d11(a) => Pending::ready(
                a.request_device(desc)
                    .map(|d| Device::new(DeviceInner::D3d11(d3d::Owner(d)), a.get_info()))
                    .map_err(|e| RequestDeviceError(e.to_string())),
            ),
            #[cfg(feature = "wgpu")]
            AdapterInner::Wgpu(a) => {
                let info = wg::adapter_info(&a.get_info());
                let request = a.request_device(&wgpu::DeviceDescriptor {
                    label: desc.label,
                    required_features: wg::to_features(desc.required_features),
                    required_limits: wg::to_limits(&desc.required_limits),
                    ..Default::default()
                });
                Pending(PendingInner::Boxed(Box::pin(async move {
                    request
                        .await
                        .map(|(d, q)| Device::new(DeviceInner::Wgpu(d, q), info))
                        .map_err(|e| RequestDeviceError(e.to_string()))
                })))
            }
        }
    }

    /// Whether this is a native (not wgpu) adapter. A wgpu adapter can report
    /// a native API in [`AdapterInfo::backend`]; this cannot be fooled.
    #[must_use]
    pub fn is_native(&self) -> bool {
        match &self.0 {
            #[cfg(target_os = "linux")]
            AdapterInner::Vulkan(_) => true,
            #[cfg(target_os = "macos")]
            AdapterInner::Metal(_) => true,
            #[cfg(target_os = "windows")]
            AdapterInner::D3d11(_) => true,
            #[cfg(feature = "wgpu")]
            AdapterInner::Wgpu(_) => false,
        }
    }

    #[cfg(target_os = "linux")]
    pub fn as_vulkan(&self) -> Option<&vk::Adapter> {
        match &self.0 {
            AdapterInner::Vulkan(a) => Some(a),
            #[cfg(feature = "wgpu")]
            AdapterInner::Wgpu(_) => None,
        }
    }

    #[cfg(target_os = "macos")]
    pub fn as_metal(&self) -> Option<&mtl::Adapter> {
        match &self.0 {
            AdapterInner::Metal(a) => Some(a),
            #[cfg(feature = "wgpu")]
            AdapterInner::Wgpu(_) => None,
        }
    }

    #[cfg(target_os = "windows")]
    pub fn as_d3d11(&self) -> Option<&d3d::Adapter> {
        match &self.0 {
            AdapterInner::D3d11(a) => Some(a),
            #[cfg(feature = "wgpu")]
            AdapterInner::Wgpu(_) => None,
        }
    }

    #[cfg(feature = "wgpu")]
    pub fn as_wgpu(&self) -> Option<&wgpu::Adapter> {
        match &self.0 {
            AdapterInner::Wgpu(a) => Some(a),
            #[cfg(target_os = "linux")]
            AdapterInner::Vulkan(_) => None,
            #[cfg(target_os = "macos")]
            AdapterInner::Metal(_) => None,
            #[cfg(target_os = "windows")]
            AdapterInner::D3d11(_) => None,
        }
    }
}

enum DeviceInner {
    #[cfg(target_os = "linux")]
    Vulkan(vk::Owner),
    #[cfg(target_os = "macos")]
    Metal(mtl::Owner),
    #[cfg(target_os = "windows")]
    D3d11(d3d::Owner),
    #[cfg(feature = "wgpu")]
    Wgpu(wgpu::Device, wgpu::Queue),
}

struct DeviceShared {
    inner: DeviceInner,
    info: AdapterInfo,
    errors: Arc<ErrorSink>,
}

/// A device. Cheap to clone; equal when it is the same device.
#[derive(Clone)]
pub struct Device(Arc<DeviceShared>);

impl PartialEq for Device {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for Device {}

impl std::fmt::Debug for Device {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Device")
            .field("adapter", &self.0.info.name)
            .field("backend", &self.0.info.backend)
            .finish()
    }
}

/// A device's queue: the same object as its [`Device`], seen as a queue.
#[derive(Clone)]
pub struct Queue(Arc<DeviceShared>);

impl std::fmt::Debug for Queue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Queue").finish_non_exhaustive()
    }
}

/// A pushed native scope; it leaves the stack when popped or dropped.
struct NativeScope {
    errors: Arc<ErrorSink>,
    id: u64,
}

impl NativeScope {
    fn take(&self) -> Option<Scope> {
        self.errors.take_scope(self.id)
    }
}

impl Drop for NativeScope {
    fn drop(&mut self) {
        self.take();
    }
}

/// An error scope pushed by [`Device::push_error_scope`].
#[must_use = "pop the scope to read what it caught"]
pub struct ErrorScopeGuard(ScopeInner);

enum ScopeInner {
    Native(NativeScope),
    #[cfg(feature = "wgpu")]
    Wgpu(wgpu::ErrorScopeGuard),
}

impl ErrorScopeGuard {
    pub fn pop(self) -> Pending<Option<Error>> {
        match self.0 {
            ScopeInner::Native(scope) => Pending::ready(scope.take().and_then(|s| s.caught)),
            #[cfg(feature = "wgpu")]
            ScopeInner::Wgpu(g) => {
                let popped = g.pop();
                Pending(PendingInner::Boxed(Box::pin(async move {
                    popped.await.map(wg::error)
                })))
            }
        }
    }
}

impl Device {
    fn new(inner: DeviceInner, info: AdapterInfo) -> (Self, Queue) {
        let errors = match &inner {
            #[cfg(target_os = "linux")]
            DeviceInner::Vulkan(o) => o.0.errors().clone(),
            #[cfg(target_os = "macos")]
            DeviceInner::Metal(o) => o.0.errors().clone(),
            #[cfg(target_os = "windows")]
            DeviceInner::D3d11(o) => o.0.errors().clone(),
            #[cfg(feature = "wgpu")]
            DeviceInner::Wgpu(..) => Arc::new(ErrorSink::new()),
        };
        let shared = Arc::new(DeviceShared {
            inner,
            info,
            errors,
        });
        (Self(shared.clone()), Queue(shared))
    }

    /// A device over a wgpu device and queue a host made.
    #[cfg(feature = "wgpu")]
    pub fn from_wgpu(device: wgpu::Device, queue: wgpu::Queue) -> (Self, Queue) {
        let info = wg::adapter_info(&device.adapter_info());
        Self::new(DeviceInner::Wgpu(device, queue), info)
    }

    /// The wgpu device and queue, on the wgpu backend.
    #[cfg(feature = "wgpu")]
    pub fn as_wgpu(&self) -> Option<(&wgpu::Device, &wgpu::Queue)> {
        match &self.0.inner {
            DeviceInner::Wgpu(d, q) => Some((d, q)),
            #[cfg(target_os = "linux")]
            DeviceInner::Vulkan(_) => None,
            #[cfg(target_os = "macos")]
            DeviceInner::Metal(_) => None,
            #[cfg(target_os = "windows")]
            DeviceInner::D3d11(_) => None,
        }
    }

    /// The Vulkan device, on the Vulkan backend.
    #[cfg(target_os = "linux")]
    pub fn as_vulkan(&self) -> Option<&vk::Device> {
        match &self.0.inner {
            DeviceInner::Vulkan(o) => Some(&o.0),
            #[cfg(feature = "wgpu")]
            DeviceInner::Wgpu(..) => None,
        }
    }

    /// The Metal device, on the Metal backend.
    #[cfg(target_os = "macos")]
    pub fn as_metal(&self) -> Option<&mtl::Device> {
        match &self.0.inner {
            DeviceInner::Metal(o) => Some(&o.0),
            #[cfg(feature = "wgpu")]
            DeviceInner::Wgpu(..) => None,
        }
    }

    /// The D3D11 device, on the D3D11 backend.
    #[cfg(target_os = "windows")]
    pub fn as_d3d11(&self) -> Option<&d3d::Device> {
        match &self.0.inner {
            DeviceInner::D3d11(o) => Some(&o.0),
            #[cfg(feature = "wgpu")]
            DeviceInner::Wgpu(..) => None,
        }
    }

    pub fn adapter_info(&self) -> AdapterInfo {
        self.0.info.clone()
    }

    pub fn features(&self) -> Features {
        match &self.0.inner {
            #[cfg(target_os = "linux")]
            DeviceInner::Vulkan(o) => o.0.features(),
            #[cfg(target_os = "macos")]
            DeviceInner::Metal(o) => o.0.features(),
            #[cfg(target_os = "windows")]
            DeviceInner::D3d11(o) => o.0.features(),
            #[cfg(feature = "wgpu")]
            DeviceInner::Wgpu(d, _) => wg::features(d.features()),
        }
    }

    pub fn limits(&self) -> Limits {
        match &self.0.inner {
            #[cfg(target_os = "linux")]
            DeviceInner::Vulkan(o) => o.0.limits(),
            #[cfg(target_os = "macos")]
            DeviceInner::Metal(o) => o.0.limits(),
            #[cfg(target_os = "windows")]
            DeviceInner::D3d11(o) => o.0.limits(),
            #[cfg(feature = "wgpu")]
            DeviceInner::Wgpu(d, _) => wg::limits(&d.limits()),
        }
    }

    /// Reports `e` to the innermost native scope that filters it, else the
    /// uncaptured handler.
    #[cfg_attr(
        not(any(target_os = "linux", target_os = "macos", target_os = "windows")),
        expect(dead_code, reason = "native only")
    )]
    fn report(&self, e: Error) {
        self.0.errors.report(e);
    }

    pub fn push_error_scope(&self, filter: ErrorFilter) -> ErrorScopeGuard {
        #[cfg(feature = "wgpu")]
        if let DeviceInner::Wgpu(d, _) = &self.0.inner {
            return ErrorScopeGuard(ScopeInner::Wgpu(
                d.push_error_scope(wg::error_filter(filter)),
            ));
        }
        let id = self.0.errors.push_scope(filter);
        ErrorScopeGuard(ScopeInner::Native(NativeScope {
            errors: self.0.errors.clone(),
            id,
        }))
    }

    pub fn on_uncaptured_error(&self, handler: UncapturedHandler) {
        #[cfg(feature = "wgpu")]
        if let DeviceInner::Wgpu(d, _) = &self.0.inner {
            let h = handler.clone();
            d.on_uncaptured_error(Arc::new(move |e| h(wg::error(e))));
        }
        self.0.errors.set_uncaptured(handler);
    }

    pub fn set_device_lost_callback(
        &self,
        f: impl Fn(DeviceLostReason, String) + Send + Sync + 'static,
    ) {
        match &self.0.inner {
            #[cfg(target_os = "linux")]
            DeviceInner::Vulkan(o) => o.0.set_device_lost_callback(f),
            #[cfg(target_os = "macos")]
            DeviceInner::Metal(o) => o.0.set_device_lost_callback(f),
            #[cfg(target_os = "windows")]
            DeviceInner::D3d11(o) => o.0.set_device_lost_callback(f),
            #[cfg(feature = "wgpu")]
            DeviceInner::Wgpu(d, _) => d.set_device_lost_callback(move |reason, message| {
                let reason = match reason {
                    wgpu::DeviceLostReason::Destroyed => DeviceLostReason::Destroyed,
                    _ => DeviceLostReason::Unknown,
                };
                f(reason, message);
            }),
        }
    }

    /// Frees what finished work held and runs map callbacks whose work
    /// completed; `Wait` blocks for everything submitted.
    pub fn poll(&self, poll: PollType) -> Result<(), PollError> {
        match &self.0.inner {
            #[cfg(target_os = "linux")]
            DeviceInner::Vulkan(o) => {
                o.0.poll(poll == PollType::Wait)
                    .map_err(|e| PollError(e.to_string()))
            }
            #[cfg(target_os = "macos")]
            DeviceInner::Metal(o) => {
                o.0.poll(poll == PollType::Wait)
                    .map_err(|e| PollError(e.to_string()))
            }
            #[cfg(target_os = "windows")]
            DeviceInner::D3d11(o) => {
                o.0.poll(poll == PollType::Wait)
                    .map_err(|e| PollError(e.to_string()))
            }
            #[cfg(feature = "wgpu")]
            DeviceInner::Wgpu(d, _) => d
                .poll(match poll {
                    PollType::Poll => wgpu::PollType::Poll,
                    PollType::Wait => wgpu::PollType::wait_indefinitely(),
                })
                .map(drop)
                .map_err(|e| PollError(e.to_string())),
        }
    }

    /// Marks the device lost: it reports so and refuses further work.
    pub fn destroy(&self) {
        let _ = self.poll(PollType::Wait);
        match &self.0.inner {
            #[cfg(target_os = "linux")]
            DeviceInner::Vulkan(o) => o.0.destroy(),
            #[cfg(target_os = "macos")]
            DeviceInner::Metal(o) => o.0.destroy(),
            #[cfg(target_os = "windows")]
            DeviceInner::D3d11(o) => o.0.destroy(),
            #[cfg(feature = "wgpu")]
            DeviceInner::Wgpu(d, _) => d.destroy(),
        }
    }

    /// Unwraps a native creation result, reporting any error to per-thread
    /// error scopes (or the uncaptured handler) and returning `invalid`.
    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
    fn or_invalid<T>(&self, r: Result<T, shared::BackendError>, invalid: impl FnOnce() -> T) -> T {
        match r {
            Ok(v) => v,
            Err(e) => {
                self.report(e.into_hal());
                invalid()
            }
        }
    }

    pub fn create_buffer(&self, desc: &BufferDescriptor<'_>) -> Buffer {
        assert!(
            !desc.mapped_at_creation,
            "mui-hal maps only through map_async"
        );
        Buffer(match &self.0.inner {
            #[cfg(target_os = "linux")]
            DeviceInner::Vulkan(o) => {
                BufferInner::Vulkan(self.or_invalid(vk::Buffer::new(&o.0, desc), || {
                    vk::Buffer::invalid(&o.0, desc)
                }))
            }
            #[cfg(target_os = "macos")]
            DeviceInner::Metal(o) => {
                BufferInner::Metal(self.or_invalid(mtl::Buffer::new(&o.0, desc), || {
                    mtl::Buffer::invalid(&o.0, desc)
                }))
            }
            #[cfg(target_os = "windows")]
            DeviceInner::D3d11(o) => {
                BufferInner::D3d11(self.or_invalid(d3d::Buffer::new(&o.0, desc), || {
                    d3d::Buffer::invalid(&o.0, desc)
                }))
            }
            #[cfg(feature = "wgpu")]
            DeviceInner::Wgpu(d, _) => {
                BufferInner::Wgpu(d.create_buffer(&wgpu::BufferDescriptor {
                    label: desc.label,
                    size: desc.size,
                    usage: wg::buffer_usages(desc.usage),
                    mapped_at_creation: false,
                }))
            }
        })
    }

    pub fn create_texture(&self, desc: &TextureDescriptor<'_>) -> Texture {
        Texture(match &self.0.inner {
            #[cfg(target_os = "linux")]
            DeviceInner::Vulkan(o) => {
                TextureInner::Vulkan(self.or_invalid(vk::Texture::new(&o.0, desc), || {
                    vk::Texture::invalid(&o.0, desc)
                }))
            }
            #[cfg(target_os = "macos")]
            DeviceInner::Metal(o) => {
                TextureInner::Metal(self.or_invalid(mtl::Texture::new(&o.0, desc), || {
                    mtl::Texture::invalid(&o.0, desc)
                }))
            }
            #[cfg(target_os = "windows")]
            DeviceInner::D3d11(o) => {
                TextureInner::D3d11(self.or_invalid(d3d::Texture::new(&o.0, desc), || {
                    d3d::Texture::invalid(&o.0, desc)
                }))
            }
            #[cfg(feature = "wgpu")]
            DeviceInner::Wgpu(d, _) => {
                let formats: Vec<_> = desc.view_formats.iter().copied().map(wg::format).collect();
                TextureInner::Wgpu(d.create_texture(&wgpu::TextureDescriptor {
                    label: desc.label,
                    size: wg::extent(desc.size),
                    mip_level_count: desc.mip_level_count,
                    sample_count: desc.sample_count,
                    dimension: wgpu::TextureDimension::D2,
                    format: wg::format(desc.format),
                    usage: wg::texture_usages(desc.usage),
                    view_formats: &formats,
                }))
            }
        })
    }

    pub fn create_sampler(&self, desc: &SamplerDescriptor<'_>) -> Sampler {
        Sampler(match &self.0.inner {
            #[cfg(target_os = "linux")]
            DeviceInner::Vulkan(o) => SamplerInner::Vulkan(
                self.or_invalid(vk::Sampler::new(&o.0, desc), || vk::Sampler::invalid(&o.0)),
            ),
            #[cfg(target_os = "macos")]
            DeviceInner::Metal(o) => {
                SamplerInner::Metal(self.or_invalid(mtl::Sampler::new(&o.0, desc), || {
                    mtl::Sampler::invalid(&o.0)
                }))
            }
            #[cfg(target_os = "windows")]
            DeviceInner::D3d11(o) => {
                SamplerInner::D3d11(self.or_invalid(d3d::Sampler::new(&o.0, desc), || {
                    d3d::Sampler::invalid(&o.0)
                }))
            }
            #[cfg(feature = "wgpu")]
            DeviceInner::Wgpu(d, _) => {
                SamplerInner::Wgpu(d.create_sampler(&wgpu::SamplerDescriptor {
                    label: desc.label,
                    mag_filter: wg::filter(desc.mag_filter),
                    min_filter: wg::filter(desc.min_filter),
                    ..Default::default()
                }))
            }
        })
    }

    pub fn create_bind_group_layout(
        &self,
        desc: &BindGroupLayoutDescriptor<'_>,
    ) -> BindGroupLayout {
        BindGroupLayout(match &self.0.inner {
            #[cfg(target_os = "linux")]
            DeviceInner::Vulkan(o) => LayoutInner::Vulkan(
                self.or_invalid(vk::BindGroupLayout::new(&o.0, desc.entries), || {
                    vk::BindGroupLayout::invalid(&o.0)
                }),
            ),
            #[cfg(target_os = "macos")]
            DeviceInner::Metal(o) => LayoutInner::Metal(
                self.or_invalid(mtl::BindGroupLayout::new(&o.0, desc.entries), || {
                    mtl::BindGroupLayout::invalid(&o.0)
                }),
            ),
            #[cfg(target_os = "windows")]
            DeviceInner::D3d11(o) => LayoutInner::D3d11(
                self.or_invalid(d3d::BindGroupLayout::new(&o.0, desc.entries), || {
                    d3d::BindGroupLayout::invalid(&o.0)
                }),
            ),
            #[cfg(feature = "wgpu")]
            DeviceInner::Wgpu(d, _) => {
                let entries: Vec<_> = desc.entries.iter().map(wg::layout_entry).collect();
                LayoutInner::Wgpu(
                    d.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                        label: desc.label,
                        entries: &entries,
                    }),
                )
            }
        })
    }

    pub fn create_bind_group(&self, desc: &BindGroupDescriptor<'_>) -> BindGroup {
        BindGroup(match &self.0.inner {
            #[cfg(target_os = "linux")]
            DeviceInner::Vulkan(o) => {
                let layout = desc.layout.vk();
                let entries = desc
                    .entries
                    .iter()
                    .map(|e| {
                        let bound = match &e.resource {
                            BindingResource::Buffer(b) => vk::Bound::Buffer {
                                buffer: b.buffer.vk().clone(),
                                offset: b.offset,
                                size: b.size.map(std::num::NonZeroU64::get),
                            },
                            BindingResource::TextureView(v) => vk::Bound::View(v.vk().clone()),
                            BindingResource::Sampler(s) => vk::Bound::Sampler(s.vk().clone()),
                        };
                        (e.binding, bound)
                    })
                    .collect();
                GroupInner::Vulkan(
                    self.or_invalid(vk::BindGroup::new(&o.0, layout, entries), || {
                        vk::BindGroup::invalid(layout)
                    }),
                )
            }
            #[cfg(target_os = "macos")]
            DeviceInner::Metal(o) => {
                let layout = desc.layout.mtl();
                let entries = desc
                    .entries
                    .iter()
                    .map(|e| {
                        let bound = match &e.resource {
                            BindingResource::Buffer(b) => mtl::Bound::Buffer {
                                buffer: b.buffer.mtl().clone(),
                                offset: b.offset,
                                size: b.size.map(std::num::NonZeroU64::get),
                            },
                            BindingResource::TextureView(v) => mtl::Bound::View(v.mtl().clone()),
                            BindingResource::Sampler(s) => mtl::Bound::Sampler(s.mtl().clone()),
                        };
                        (e.binding, bound)
                    })
                    .collect();
                GroupInner::Metal(
                    self.or_invalid(mtl::BindGroup::new(&o.0, layout, entries), || {
                        mtl::BindGroup::invalid(layout)
                    }),
                )
            }
            #[cfg(target_os = "windows")]
            DeviceInner::D3d11(o) => {
                let layout = desc.layout.d3d();
                let entries = desc
                    .entries
                    .iter()
                    .map(|e| {
                        let bound = match &e.resource {
                            BindingResource::Buffer(b) => d3d::Bound::Buffer {
                                buffer: b.buffer.d3d().clone(),
                                offset: b.offset,
                                size: b.size.map(std::num::NonZeroU64::get),
                            },
                            BindingResource::TextureView(v) => d3d::Bound::View(v.d3d().clone()),
                            BindingResource::Sampler(s) => d3d::Bound::Sampler(s.d3d().clone()),
                        };
                        (e.binding, bound)
                    })
                    .collect();
                GroupInner::D3d11(
                    self.or_invalid(d3d::BindGroup::new(&o.0, layout, entries), || {
                        d3d::BindGroup::invalid(layout)
                    }),
                )
            }
            #[cfg(feature = "wgpu")]
            DeviceInner::Wgpu(d, _) => {
                let entries: Vec<_> = desc
                    .entries
                    .iter()
                    .map(|e| wgpu::BindGroupEntry {
                        binding: e.binding,
                        resource: match &e.resource {
                            BindingResource::Buffer(b) => {
                                wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                                    buffer: b.buffer.wg(),
                                    offset: b.offset,
                                    size: b.size,
                                })
                            }
                            BindingResource::TextureView(v) => {
                                wgpu::BindingResource::TextureView(v.wg())
                            }
                            BindingResource::Sampler(s) => wgpu::BindingResource::Sampler(s.wg()),
                        },
                    })
                    .collect();
                GroupInner::Wgpu(d.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: desc.label,
                    layout: desc.layout.wg(),
                    entries: &entries,
                }))
            }
        })
    }

    pub fn create_pipeline_layout(&self, desc: &PipelineLayoutDescriptor<'_>) -> PipelineLayout {
        PipelineLayout(match &self.0.inner {
            #[cfg(target_os = "linux")]
            DeviceInner::Vulkan(o) => {
                let sets: Option<Vec<vk::BindGroupLayout>> = desc
                    .bind_group_layouts
                    .iter()
                    .map(|l| l.map(|g| g.vk().clone()))
                    .collect();
                if let Some(sets) = sets {
                    PipelineLayoutInner::Vulkan(
                        self.or_invalid(vk::PipelineLayout::new(&o.0, &sets), || {
                            vk::PipelineLayout::invalid(&o.0)
                        }),
                    )
                } else {
                    self.report(Error::Validation {
                        description: "mui-hal pipeline layouts must bind every group".into(),
                    });
                    PipelineLayoutInner::Vulkan(vk::PipelineLayout::invalid(&o.0))
                }
            }
            #[cfg(target_os = "macos")]
            DeviceInner::Metal(o) => {
                let sets: Option<Vec<mtl::BindGroupLayout>> = desc
                    .bind_group_layouts
                    .iter()
                    .map(|l| l.map(|g| g.mtl().clone()))
                    .collect();
                if let Some(sets) = sets {
                    PipelineLayoutInner::Metal(
                        self.or_invalid(mtl::PipelineLayout::new(&o.0, &sets), || {
                            mtl::PipelineLayout::invalid(&o.0)
                        }),
                    )
                } else {
                    self.report(Error::Validation {
                        description: "mui-hal pipeline layouts must bind every group".into(),
                    });
                    PipelineLayoutInner::Metal(mtl::PipelineLayout::invalid(&o.0))
                }
            }
            #[cfg(target_os = "windows")]
            DeviceInner::D3d11(o) => {
                let sets: Option<Vec<d3d::BindGroupLayout>> = desc
                    .bind_group_layouts
                    .iter()
                    .map(|l| l.map(|g| g.d3d().clone()))
                    .collect();
                if let Some(sets) = sets {
                    PipelineLayoutInner::D3d11(
                        self.or_invalid(d3d::PipelineLayout::new(&o.0, &sets), || {
                            d3d::PipelineLayout::invalid(&o.0)
                        }),
                    )
                } else {
                    self.report(Error::Validation {
                        description: "mui-hal pipeline layouts must bind every group".into(),
                    });
                    PipelineLayoutInner::D3d11(d3d::PipelineLayout::invalid(&o.0))
                }
            }
            #[cfg(feature = "wgpu")]
            DeviceInner::Wgpu(d, _) => {
                let sets: Vec<Option<&wgpu::BindGroupLayout>> = desc
                    .bind_group_layouts
                    .iter()
                    .map(|l| l.map(BindGroupLayout::wg))
                    .collect();
                PipelineLayoutInner::Wgpu(d.create_pipeline_layout(
                    &wgpu::PipelineLayoutDescriptor {
                        label: desc.label,
                        bind_group_layouts: &sets,
                        immediate_size: 0,
                    },
                ))
            }
        })
    }

    pub fn create_shader_module(&self, desc: ShaderModuleDescriptor<'_>) -> ShaderModule {
        let ShaderSource::Mui(id) = desc.source;
        let inner = match &self.0.inner {
            #[cfg(target_os = "linux")]
            DeviceInner::Vulkan(o) => {
                ModuleInner::Vulkan(self.or_invalid(vk::ShaderModule::new(&o.0, id), || {
                    vk::ShaderModule::invalid(&o.0)
                }))
            }
            #[cfg(target_os = "macos")]
            DeviceInner::Metal(o) => {
                ModuleInner::Metal(self.or_invalid(mtl::ShaderModule::new(&o.0, id), || {
                    mtl::ShaderModule::invalid(&o.0, id)
                }))
            }
            #[cfg(target_os = "windows")]
            DeviceInner::D3d11(o) => {
                ModuleInner::D3d11(self.or_invalid(d3d::ShaderModule::new(&o.0, id), || {
                    d3d::ShaderModule::invalid(&o.0, id)
                }))
            }
            #[cfg(feature = "wgpu")]
            DeviceInner::Wgpu(d, _) => {
                ModuleInner::Wgpu(d.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: desc.label,
                    source: wgpu::ShaderSource::Wgsl(wg::shader_source(id).into()),
                }))
            }
        };
        ShaderModule { id, inner }
    }

    pub fn create_render_pipeline(&self, desc: &RenderPipelineDescriptor<'_>) -> RenderPipeline {
        let buffers: Vec<VertexBufferLayout<'_>> =
            desc.vertex.buffers.iter().flatten().cloned().collect();
        match &self.0.inner {
            #[cfg(target_os = "linux")]
            DeviceInner::Vulkan(o) => {
                let fail = |e: Error| {
                    self.report(e);
                    RenderPipeline(PipelineInner::Vulkan(vk::RenderPipeline::invalid(
                        &o.0,
                        &vk::PipelineLayout::invalid(&o.0),
                    )))
                };
                let Some(fragment) = desc.fragment.as_ref() else {
                    return fail(Error::Validation {
                        description: "mui-hal pipelines require a fragment stage".into(),
                    });
                };
                let [Some(target)] = fragment.targets else {
                    return fail(Error::Validation {
                        description: "mui-hal pipelines require exactly one colour target".into(),
                    });
                };
                let Some(layout) = desc.layout else {
                    return fail(Error::Validation {
                        description: "mui-hal pipelines require an explicit layout".into(),
                    });
                };
                let resolve_entry =
                    |module: &ShaderModule, name: Option<&str>, stage| -> Result<String, Error> {
                        let entries = module.id.artifact().entry_points;
                        match name {
                            Some(n) => {
                                if entries.iter().any(|e| e.stage == stage && e.name == n) {
                                    Ok(n.to_owned())
                                } else {
                                    Err(Error::Validation {
                                        description: format!(
                                            "shader {:?} has no {stage:?} entry point named `{n}`",
                                            module.id
                                        ),
                                    })
                                }
                            }
                            None => entries
                                .iter()
                                .find(|e| e.stage == stage)
                                .map(|e| e.name.to_owned())
                                .ok_or_else(|| Error::Validation {
                                    description: format!(
                                        "shader {:?} has no {stage:?} entry point",
                                        module.id
                                    ),
                                }),
                        }
                    };
                let vs = match resolve_entry(
                    desc.vertex.module,
                    desc.vertex.entry_point,
                    mui_shaders::Stage::Vertex,
                ) {
                    Ok(s) => s,
                    Err(e) => return fail(e),
                };
                let fs = match resolve_entry(
                    fragment.module,
                    fragment.entry_point,
                    mui_shaders::Stage::Fragment,
                ) {
                    Ok(s) => s,
                    Err(e) => return fail(e),
                };
                let pipe = self.or_invalid(
                    vk::RenderPipeline::new(
                        &o.0,
                        &vk::PipelineDesc {
                            layout: layout.vk(),
                            vertex: (desc.vertex.module.vk(), &vs),
                            fragment: (fragment.module.vk(), &fs),
                            buffers: &buffers,
                            topology: desc.primitive.topology,
                            target: *target,
                            cache: desc.cache.map(PipelineCache::vk),
                        },
                    ),
                    || vk::RenderPipeline::invalid(&o.0, layout.vk()),
                );
                RenderPipeline(PipelineInner::Vulkan(pipe))
            }
            #[cfg(target_os = "macos")]
            DeviceInner::Metal(o) => {
                let fail = |e: Error| {
                    self.report(e);
                    RenderPipeline(PipelineInner::Metal(mtl::RenderPipeline::invalid(&o.0)))
                };
                let Some(fragment) = desc.fragment.as_ref() else {
                    return fail(Error::Validation {
                        description: "mui-hal pipelines require a fragment stage".into(),
                    });
                };
                let [Some(target)] = fragment.targets else {
                    return fail(Error::Validation {
                        description: "mui-hal pipelines require exactly one colour target".into(),
                    });
                };
                let Some(layout) = desc.layout else {
                    return fail(Error::Validation {
                        description: "mui-hal pipelines require an explicit layout".into(),
                    });
                };
                let pipe = self.or_invalid(
                    mtl::RenderPipeline::new(
                        &o.0,
                        &mtl::PipelineDesc {
                            layout: layout.mtl(),
                            vertex: (desc.vertex.module.mtl(), desc.vertex.entry_point),
                            fragment: (fragment.module.mtl(), fragment.entry_point),
                            buffers: &buffers,
                            topology: desc.primitive.topology,
                            target: *target,
                        },
                    ),
                    || mtl::RenderPipeline::invalid(&o.0),
                );
                RenderPipeline(PipelineInner::Metal(pipe))
            }
            #[cfg(target_os = "windows")]
            DeviceInner::D3d11(o) => {
                let fail = |e: Error| {
                    self.report(e);
                    RenderPipeline(PipelineInner::D3d11(d3d::RenderPipeline::invalid(&o.0)))
                };
                let Some(fragment) = desc.fragment.as_ref() else {
                    return fail(Error::Validation {
                        description: "mui-hal pipelines require a fragment stage".into(),
                    });
                };
                let [Some(target)] = fragment.targets else {
                    return fail(Error::Validation {
                        description: "mui-hal pipelines require exactly one colour target".into(),
                    });
                };
                let Some(layout) = desc.layout else {
                    return fail(Error::Validation {
                        description: "mui-hal pipelines require an explicit layout".into(),
                    });
                };
                let pipe = self.or_invalid(
                    d3d::RenderPipeline::new(
                        &o.0,
                        &d3d::PipelineDesc {
                            layout: layout.d3d(),
                            vertex: (desc.vertex.module.d3d(), desc.vertex.entry_point),
                            fragment: (fragment.module.d3d(), fragment.entry_point),
                            buffers: &buffers,
                            topology: desc.primitive.topology,
                            target: *target,
                        },
                    ),
                    || d3d::RenderPipeline::invalid(&o.0),
                );
                RenderPipeline(PipelineInner::D3d11(pipe))
            }
            #[cfg(feature = "wgpu")]
            DeviceInner::Wgpu(d, _) => {
                let fragment = desc
                    .fragment
                    .as_ref()
                    .expect("mui-hal pipelines have a fragment stage");
                let [Some(target)] = fragment.targets else {
                    panic!("mui-hal pipelines have exactly one colour target");
                };
                let entry = |module: &ShaderModule, name: Option<&str>, stage| {
                    name.map_or_else(
                        || {
                            module
                                .id
                                .artifact()
                                .entry_points
                                .iter()
                                .find(|e| e.stage == stage)
                                .map(|e| e.name.to_owned())
                                .expect("the shader has an entry point for the stage")
                        },
                        str::to_owned,
                    )
                };
                let vs = entry(
                    desc.vertex.module,
                    desc.vertex.entry_point,
                    mui_shaders::Stage::Vertex,
                );
                let fs = entry(
                    fragment.module,
                    fragment.entry_point,
                    mui_shaders::Stage::Fragment,
                );
                let layout = desc.layout.expect("mui-hal pipelines name their layout");
                let attributes: Vec<Vec<wgpu::VertexAttribute>> = buffers
                    .iter()
                    .map(|b| b.attributes.iter().map(wg::attribute).collect())
                    .collect();
                let layouts: Vec<_> = buffers
                    .iter()
                    .zip(&attributes)
                    .map(|(b, a)| {
                        Some(wgpu::VertexBufferLayout {
                            array_stride: b.array_stride,
                            step_mode: wg::step_mode(b.step_mode),
                            attributes: a,
                        })
                    })
                    .collect();
                let targets = [Some(wg::target(target))];
                RenderPipeline(PipelineInner::Wgpu(d.create_render_pipeline(
                    &wgpu::RenderPipelineDescriptor {
                        label: desc.label,
                        layout: Some(layout.wg()),
                        vertex: wgpu::VertexState {
                            module: desc.vertex.module.wg(),
                            entry_point: Some(&vs),
                            compilation_options: wgpu::PipelineCompilationOptions::default(),
                            buffers: &layouts,
                        },
                        primitive: wgpu::PrimitiveState {
                            topology: wg::topology(desc.primitive.topology),
                            ..Default::default()
                        },
                        depth_stencil: None,
                        multisample: wgpu::MultisampleState::default(),
                        fragment: Some(wgpu::FragmentState {
                            module: fragment.module.wg(),
                            entry_point: Some(&fs),
                            compilation_options: wgpu::PipelineCompilationOptions::default(),
                            targets: &targets,
                        }),
                        multiview_mask: None,
                        cache: desc.cache.map(PipelineCache::wg),
                    },
                )))
            }
        }
    }

    pub fn create_command_encoder(&self, desc: &CommandEncoderDescriptor<'_>) -> CommandEncoder {
        let _ = desc;
        CommandEncoder(match &self.0.inner {
            #[cfg(target_os = "linux")]
            DeviceInner::Vulkan(_) => EncoderInner::Vulkan(Vec::new()),
            #[cfg(target_os = "macos")]
            DeviceInner::Metal(_) => EncoderInner::Metal(Vec::new()),
            #[cfg(target_os = "windows")]
            DeviceInner::D3d11(_) => EncoderInner::D3d11(Vec::new()),
            #[cfg(feature = "wgpu")]
            DeviceInner::Wgpu(d, _) => EncoderInner::Wgpu(
                d.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: desc.label }),
            ),
        })
    }

    pub fn create_query_set(&self, desc: &QuerySetDescriptor<'_>) -> QuerySet {
        QuerySet(match &self.0.inner {
            #[cfg(target_os = "linux")]
            DeviceInner::Vulkan(o) => {
                QueryInner::Vulkan(self.or_invalid(vk::QuerySet::new(&o.0, desc), || {
                    vk::QuerySet::invalid(&o.0)
                }))
            }
            #[cfg(target_os = "macos")]
            DeviceInner::Metal(o) => {
                QueryInner::Metal(self.or_invalid(mtl::QuerySet::new(&o.0, desc), || {
                    mtl::QuerySet::invalid(&o.0)
                }))
            }
            #[cfg(target_os = "windows")]
            DeviceInner::D3d11(o) => {
                QueryInner::D3d11(self.or_invalid(d3d::QuerySet::new(&o.0, desc), || {
                    d3d::QuerySet::invalid(&o.0)
                }))
            }
            #[cfg(feature = "wgpu")]
            DeviceInner::Wgpu(d, _) => {
                QueryInner::Wgpu(d.create_query_set(&wgpu::QuerySetDescriptor {
                    label: desc.label,
                    ty: wgpu::QueryType::Timestamp,
                    count: desc.count,
                }))
            }
        })
    }

    /// A pipeline cache primed with `desc.data` when the driver accepts it.
    ///
    /// # Safety
    /// `desc.data` must come from [`PipelineCache::get_data`] on this
    /// adapter; the driver checks its header but trusts the rest.
    #[expect(unsafe_code, reason = "mirrors wgpu's unsafe persisted-cache API")]
    pub unsafe fn create_pipeline_cache(
        &self,
        desc: &PipelineCacheDescriptor<'_>,
    ) -> PipelineCache {
        PipelineCache(match &self.0.inner {
            #[cfg(target_os = "linux")]
            DeviceInner::Vulkan(o) => CacheInner::Vulkan(
                self.or_invalid(vk::PipelineCache::new(&o.0, desc.data), || {
                    vk::PipelineCache::invalid(&o.0)
                }),
            ),
            #[cfg(target_os = "macos")]
            DeviceInner::Metal(o) => CacheInner::Metal(
                self.or_invalid(mtl::PipelineCache::new(&o.0, desc.data), || {
                    mtl::PipelineCache::invalid(&o.0)
                }),
            ),
            #[cfg(target_os = "windows")]
            DeviceInner::D3d11(o) => CacheInner::D3d11(
                self.or_invalid(d3d::PipelineCache::new(&o.0, desc.data), || {
                    d3d::PipelineCache::invalid(&o.0)
                }),
            ),
            #[cfg(feature = "wgpu")]
            // SAFETY: forwarded from this function's contract.
            DeviceInner::Wgpu(d, _) => CacheInner::Wgpu(unsafe {
                d.create_pipeline_cache(&wgpu::PipelineCacheDescriptor {
                    label: desc.label,
                    data: desc.data,
                    fallback: desc.fallback,
                })
            }),
        })
    }
}

/// wgpu's `util` names.
pub mod util {
    pub use crate::pipeline_cache_key;
}

/// A key naming `info`'s pipeline-cache blobs, or `None` where caches are
/// not kept.
pub fn pipeline_cache_key(info: &AdapterInfo) -> Option<String> {
    match info.backend {
        Backend::Vulkan => Some(format!(
            "mui-hal-vulkan-{:x}-{:x}-{}",
            info.vendor,
            info.device,
            info.driver_info
                .replace(|c: char| !c.is_ascii_alphanumeric(), "_")
        )),
        _ => None,
    }
}

impl Queue {
    #[cfg(target_os = "linux")]
    fn vk(&self) -> Option<&vk::Device> {
        match &self.0.inner {
            DeviceInner::Vulkan(o) => Some(&o.0),
            #[cfg(feature = "wgpu")]
            DeviceInner::Wgpu(..) => None,
        }
    }

    #[cfg(target_os = "macos")]
    fn mtl(&self) -> Option<&mtl::Device> {
        match &self.0.inner {
            DeviceInner::Metal(o) => Some(&o.0),
            #[cfg(feature = "wgpu")]
            DeviceInner::Wgpu(..) => None,
        }
    }

    #[cfg(target_os = "windows")]
    fn d3d(&self) -> Option<&d3d::Device> {
        match &self.0.inner {
            DeviceInner::D3d11(o) => Some(&o.0),
            #[cfg(feature = "wgpu")]
            DeviceInner::Wgpu(..) => None,
        }
    }

    #[cfg(feature = "wgpu")]
    fn wg(&self) -> Option<&wgpu::Queue> {
        match &self.0.inner {
            DeviceInner::Wgpu(_, q) => Some(q),
            #[cfg(target_os = "linux")]
            DeviceInner::Vulkan(_) => None,
            #[cfg(target_os = "macos")]
            DeviceInner::Metal(_) => None,
            #[cfg(target_os = "windows")]
            DeviceInner::D3d11(_) => None,
        }
    }

    /// Writes `data` into `buffer` at `offset` before the next submission.
    pub fn write_buffer(&self, buffer: &Buffer, offset: u64, data: &[u8]) {
        if data.is_empty() {
            return;
        }
        #[cfg(feature = "wgpu")]
        if let Some(q) = self.wg() {
            q.write_buffer(buffer.wg(), offset, data);
            return;
        }
        #[cfg(target_os = "linux")]
        if let Some(d) = self.vk()
            && let Err(e) = d.stage_buffer_write(buffer.vk(), offset, data)
        {
            self.0.errors.report(e.into_hal());
        }
        #[cfg(target_os = "macos")]
        if let Some(d) = self.mtl()
            && let Err(e) = d.stage_buffer_write(buffer.mtl(), offset, data)
        {
            self.0.errors.report(e.into_hal());
        }
        #[cfg(target_os = "windows")]
        if let Some(d) = self.d3d()
            && let Err(e) = d.stage_buffer_write(buffer.d3d(), offset, data)
        {
            self.0.errors.report(e.into_hal());
        }
    }

    /// Writes texels into `dest` before the next submission.
    pub fn write_texture(
        &self,
        dest: TexelCopyTextureInfo<'_>,
        data: &[u8],
        layout: TexelCopyBufferLayout,
        size: Extent3d,
    ) {
        #[cfg(feature = "wgpu")]
        if let Some(q) = self.wg() {
            q.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: dest.texture.wg(),
                    mip_level: dest.mip_level,
                    origin: wg::origin(dest.origin),
                    aspect: wgpu::TextureAspect::All,
                },
                data,
                wg::buffer_layout(layout),
                wg::extent(size),
            );
            return;
        }
        #[cfg(target_os = "linux")]
        if let Some(d) = self.vk() {
            if dest.mip_level != 0 {
                self.0.errors.report(Error::Validation {
                    description: "native textures have one mip level".into(),
                });
                return;
            }
            let dst = dest.texture.vk().clone();
            if dst.is_retired_swapchain() {
                self.0.errors.report(Error::Validation {
                    description: "cannot write to a retired swapchain image".into(),
                });
                return;
            }
            if !dst.is_valid() {
                self.0.errors.report(Error::Validation {
                    description: "cannot write to an invalid texture".into(),
                });
                return;
            }
            // Native swapchain images are render attachments only. Reject
            // uploads before they can queue a copy to an image that may retire.
            if !dst.usage().contains(TextureUsages::COPY_DST) {
                self.0.errors.report(Error::Validation {
                    description: "write_texture requires COPY_DST usage".into(),
                });
                return;
            }
            let block = dst.format().block_size() as usize;
            let row = size.width as usize * block;
            let bytes_per_row = layout.bytes_per_row.map_or(row, |b| b as usize);
            let rows_per_image = layout.rows_per_image.unwrap_or(size.height) as usize;
            let layers = size.depth_or_array_layers.max(1) as usize;
            let len = row * size.height as usize * layers;
            if len == 0 {
                return;
            }
            let origin = dest.origin;
            if let Err(e) = d.stage(
                len as u64,
                |m| {
                    let mut at = 0;
                    for z in 0..layers {
                        for y in 0..size.height as usize {
                            let from =
                                layout.offset as usize + (z * rows_per_image + y) * bytes_per_row;
                            m[at..at + row].copy_from_slice(&data[from..from + row]);
                            at += row;
                        }
                    }
                },
                |src, offset| vk::Cmd::BufferToTexture {
                    src,
                    offset,
                    dst,
                    origin,
                    size,
                },
            ) {
                self.0.errors.report(e.into_hal());
            }
        }
        #[cfg(target_os = "macos")]
        if let Some(d) = self.mtl() {
            let dst = dest.texture.mtl();
            let target = shared::TextureWriteTarget {
                belongs: dst.belongs_to(&d.0),
                valid: dst.is_valid(),
                usage: dst.usage(),
                dims: (dst.width(), dst.height(), dst.layers()),
                block_size: dst.format().block_size(),
            };
            match shared::pack_texture_write_bytes(
                &target,
                dest.mip_level,
                dest.origin,
                size,
                layout,
                data,
            ) {
                Ok(Some(packed)) => {
                    if let Err(e) = d.stage_texture_write(dst, dest.origin, size, &packed) {
                        self.0.errors.report(e.into_hal());
                    }
                }
                Ok(None) => {}
                Err(e) => self.0.errors.report(e.into_hal()),
            }
        }
        #[cfg(target_os = "windows")]
        if let Some(d) = self.d3d() {
            let dst = dest.texture.d3d();
            let target = shared::TextureWriteTarget {
                belongs: dst.belongs_to(&d.0),
                valid: dst.is_valid(),
                usage: dst.usage(),
                dims: (dst.width(), dst.height(), dst.layers()),
                block_size: dst.format().block_size(),
            };
            match shared::pack_texture_write_bytes(
                &target,
                dest.mip_level,
                dest.origin,
                size,
                layout,
                data,
            ) {
                Ok(Some(packed)) => {
                    if let Err(e) = d.stage_texture_write(dst, dest.origin, size, packed) {
                        self.0.errors.report(e.into_hal());
                    }
                }
                Ok(None) => {}
                Err(e) => self.0.errors.report(e.into_hal()),
            }
        }
        let _ = (data, layout, size, dest);
    }

    pub fn submit(&self, buffers: impl IntoIterator<Item = CommandBuffer>) -> SubmissionIndex {
        match &self.0.inner {
            #[cfg(target_os = "linux")]
            DeviceInner::Vulkan(o) => {
                let lists = buffers
                    .into_iter()
                    .map(|b| match b.0 {
                        EncoderInner::Vulkan(cmds) => cmds,
                        #[cfg(feature = "wgpu")]
                        EncoderInner::Wgpu(_) => panic!("a wgpu encoder on a Vulkan queue"),
                    })
                    .collect();
                match o.0.submit(lists) {
                    Ok(v) => SubmissionIndex(v),
                    Err(e) => {
                        self.0.errors.report(e.into_hal());
                        SubmissionIndex(o.0.submitted())
                    }
                }
            }
            #[cfg(target_os = "macos")]
            DeviceInner::Metal(o) => {
                let lists = buffers
                    .into_iter()
                    .map(|b| match b.0 {
                        EncoderInner::Metal(cmds) => cmds,
                        #[cfg(feature = "wgpu")]
                        EncoderInner::Wgpu(_) => panic!("a wgpu encoder on a Metal queue"),
                    })
                    .collect();
                match o.0.submit(lists) {
                    Ok(v) => SubmissionIndex(v),
                    Err(e) => {
                        self.0.errors.report(e.into_hal());
                        SubmissionIndex(o.0.submitted())
                    }
                }
            }
            #[cfg(target_os = "windows")]
            DeviceInner::D3d11(o) => {
                let lists = buffers
                    .into_iter()
                    .map(|b| match b.0 {
                        EncoderInner::D3d11(cmds) => cmds,
                        #[cfg(feature = "wgpu")]
                        EncoderInner::Wgpu(_) => panic!("a wgpu encoder on a D3D11 queue"),
                    })
                    .collect();
                match o.0.submit(lists) {
                    Ok(v) => SubmissionIndex(v),
                    Err(e) => {
                        self.0.errors.report(e.into_hal());
                        SubmissionIndex(o.0.submitted())
                    }
                }
            }
            #[cfg(feature = "wgpu")]
            DeviceInner::Wgpu(_, q) => {
                q.submit(buffers.into_iter().map(|b| match b.0 {
                    EncoderInner::Wgpu(e) => e.finish(),
                    #[cfg(target_os = "linux")]
                    EncoderInner::Vulkan(_) => panic!("a Vulkan encoder on a wgpu queue"),
                    #[cfg(target_os = "macos")]
                    EncoderInner::Metal(_) => panic!("a Metal encoder on a wgpu queue"),
                    #[cfg(target_os = "windows")]
                    EncoderInner::D3d11(_) => panic!("a D3D11 encoder on a wgpu queue"),
                }));
                SubmissionIndex(0)
            }
        }
    }

    /// Ends `frame`'s rendering and shows it.
    pub fn present(&self, frame: SurfaceTexture) {
        match frame.inner {
            #[cfg(target_os = "linux")]
            SurfaceTextureInner::Vulkan(surface, f) => {
                if let Err(e) = surface.present(f) {
                    self.0.errors.report(e.into_hal());
                }
            }
            #[cfg(target_os = "macos")]
            SurfaceTextureInner::Metal(surface, f) => {
                if let Err(e) = surface.present(&f) {
                    self.0.errors.report(e.into_hal());
                }
            }
            #[cfg(target_os = "windows")]
            SurfaceTextureInner::D3d11(surface, f) => {
                if let Err(e) = surface.present(f) {
                    self.0.errors.report(e.into_hal());
                }
            }
            #[cfg(feature = "wgpu")]
            SurfaceTextureInner::Wgpu(t) => {
                if let Some(q) = self.wg() {
                    q.present(t);
                }
            }
        }
    }

    /// Calls `f` (from [`Device::poll`]) once everything submitted so far
    /// completes.
    pub fn on_submitted_work_done(&self, f: impl FnOnce() + Send + 'static) {
        match &self.0.inner {
            #[cfg(target_os = "linux")]
            DeviceInner::Vulkan(o) => o.0.on_submitted_work_done(Box::new(f)),
            #[cfg(target_os = "macos")]
            DeviceInner::Metal(o) => o.0.on_submitted_work_done(Box::new(f)),
            #[cfg(target_os = "windows")]
            DeviceInner::D3d11(o) => o.0.on_submitted_work_done(Box::new(f)),
            #[cfg(feature = "wgpu")]
            DeviceInner::Wgpu(_, q) => q.on_submitted_work_done(f),
        }
    }

    /// Nanoseconds per timestamp tick.
    pub fn get_timestamp_period(&self) -> f32 {
        match &self.0.inner {
            #[cfg(target_os = "linux")]
            DeviceInner::Vulkan(o) => o.0.timestamp_period(),
            #[cfg(target_os = "macos")]
            DeviceInner::Metal(o) => o.0.timestamp_period(),
            #[cfg(target_os = "windows")]
            DeviceInner::D3d11(o) => o.0.timestamp_period(),
            #[cfg(feature = "wgpu")]
            DeviceInner::Wgpu(_, q) => q.get_timestamp_period(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SubmissionIndex(pub u64);

/// Declares a handle: an enum over backends wrapped in a public struct,
/// with `vk()`/`mtl()`/`d3d()`/`wg()` accessors.
macro_rules! handle {
    ($name:ident, $inner:ident, $vk:ident, $wg:ident) => {
        #[derive(Clone)]
        enum $inner {
            #[cfg(target_os = "linux")]
            Vulkan(vk::$vk),
            #[cfg(target_os = "macos")]
            Metal(mtl::$vk),
            #[cfg(target_os = "windows")]
            D3d11(d3d::$vk),
            #[cfg(feature = "wgpu")]
            Wgpu(wgpu::$wg),
        }

        #[derive(Clone)]
        pub struct $name($inner);

        impl std::fmt::Debug for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.debug_struct(stringify!($name)).finish_non_exhaustive()
            }
        }

        #[expect(
            clippy::allow_attributes,
            reason = "which accessors go unused varies by handle"
        )]
        #[allow(dead_code, reason = "not every handle uses both accessors")]
        impl $name {
            #[cfg(target_os = "linux")]
            #[track_caller]
            fn vk(&self) -> &vk::$vk {
                match &self.0 {
                    $inner::Vulkan(x) => x,
                    #[cfg(feature = "wgpu")]
                    $inner::Wgpu(_) => panic!(concat!("a wgpu ", stringify!($name), " on Vulkan")),
                }
            }

            #[cfg(target_os = "macos")]
            #[track_caller]
            fn mtl(&self) -> &mtl::$vk {
                match &self.0 {
                    $inner::Metal(x) => x,
                    #[cfg(feature = "wgpu")]
                    $inner::Wgpu(_) => panic!(concat!("a wgpu ", stringify!($name), " on Metal")),
                }
            }

            #[cfg(target_os = "windows")]
            #[track_caller]
            fn d3d(&self) -> &d3d::$vk {
                match &self.0 {
                    $inner::D3d11(x) => x,
                    #[cfg(feature = "wgpu")]
                    $inner::Wgpu(_) => panic!(concat!("a wgpu ", stringify!($name), " on D3D11")),
                }
            }

            #[cfg(feature = "wgpu")]
            #[track_caller]
            fn wg(&self) -> &wgpu::$wg {
                match &self.0 {
                    $inner::Wgpu(x) => x,
                    #[cfg(target_os = "linux")]
                    $inner::Vulkan(_) => {
                        panic!(concat!("a Vulkan ", stringify!($name), " on wgpu"))
                    }
                    #[cfg(target_os = "macos")]
                    $inner::Metal(_) => {
                        panic!(concat!("a Metal ", stringify!($name), " on wgpu"))
                    }
                    #[cfg(target_os = "windows")]
                    $inner::D3d11(_) => {
                        panic!(concat!("a D3D11 ", stringify!($name), " on wgpu"))
                    }
                }
            }

            /// The wgpu object, on the wgpu backend.
            #[cfg(feature = "wgpu")]
            pub fn as_wgpu(&self) -> Option<&wgpu::$wg> {
                match &self.0 {
                    $inner::Wgpu(x) => Some(x),
                    #[cfg(target_os = "linux")]
                    $inner::Vulkan(_) => None,
                    #[cfg(target_os = "macos")]
                    $inner::Metal(_) => None,
                    #[cfg(target_os = "windows")]
                    $inner::D3d11(_) => None,
                }
            }

            /// Wraps a wgpu object a host made.
            #[cfg(feature = "wgpu")]
            pub fn from_wgpu(x: wgpu::$wg) -> Self {
                Self($inner::Wgpu(x))
            }
        }
    };
}

handle!(Buffer, BufferInner, Buffer, Buffer);
handle!(Texture, TextureInner, Texture, Texture);
handle!(TextureView, ViewInner, TextureView, TextureView);
handle!(Sampler, SamplerInner, Sampler, Sampler);
handle!(
    BindGroupLayout,
    LayoutInner,
    BindGroupLayout,
    BindGroupLayout
);
handle!(BindGroup, GroupInner, BindGroup, BindGroup);
handle!(
    PipelineLayout,
    PipelineLayoutInner,
    PipelineLayout,
    PipelineLayout
);
handle!(
    RenderPipeline,
    PipelineInner,
    RenderPipeline,
    RenderPipeline
);
handle!(QuerySet, QueryInner, QuerySet, QuerySet);
handle!(PipelineCache, CacheInner, PipelineCache, PipelineCache);

impl Buffer {
    pub fn size(&self) -> u64 {
        match &self.0 {
            #[cfg(target_os = "linux")]
            BufferInner::Vulkan(b) => b.size(),
            #[cfg(target_os = "macos")]
            BufferInner::Metal(b) => b.size(),
            #[cfg(target_os = "windows")]
            BufferInner::D3d11(b) => b.size(),
            #[cfg(feature = "wgpu")]
            BufferInner::Wgpu(b) => b.size(),
        }
    }

    pub fn usage(&self) -> BufferUsages {
        match &self.0 {
            #[cfg(target_os = "linux")]
            BufferInner::Vulkan(b) => b.usage(),
            #[cfg(target_os = "macos")]
            BufferInner::Metal(b) => b.usage(),
            #[cfg(target_os = "windows")]
            BufferInner::D3d11(b) => b.usage(),
            #[cfg(feature = "wgpu")]
            BufferInner::Wgpu(b) => wg::from_buffer_usages(b.usage()),
        }
    }

    pub fn as_entire_binding(&self) -> BindingResource<'_> {
        BindingResource::Buffer(BufferBinding {
            buffer: self,
            offset: 0,
            size: None,
        })
    }

    pub fn slice(&self, range: impl RangeBounds<u64>) -> BufferSlice<'_> {
        let start = match range.start_bound() {
            RangeBound::Included(&s) => s,
            RangeBound::Excluded(&s) => s + 1,
            RangeBound::Unbounded => 0,
        };
        let end = match range.end_bound() {
            RangeBound::Included(&e) => e + 1,
            RangeBound::Excluded(&e) => e,
            RangeBound::Unbounded => self.size(),
        };
        BufferSlice {
            buffer: self,
            offset: start,
            size: end - start,
        }
    }

    pub fn unmap(&self) {
        match &self.0 {
            #[cfg(target_os = "linux")]
            BufferInner::Vulkan(b) => b.unmap(),
            #[cfg(target_os = "macos")]
            BufferInner::Metal(b) => b.unmap(),
            #[cfg(target_os = "windows")]
            BufferInner::D3d11(b) => b.unmap(),
            #[cfg(feature = "wgpu")]
            BufferInner::Wgpu(b) => b.unmap(),
        }
    }

    pub fn destroy(&self) {
        match &self.0 {
            #[cfg(target_os = "linux")]
            BufferInner::Vulkan(b) => b.unmap(),
            #[cfg(target_os = "macos")]
            BufferInner::Metal(b) => b.unmap(),
            #[cfg(target_os = "windows")]
            BufferInner::D3d11(b) => b.unmap(),
            #[cfg(feature = "wgpu")]
            BufferInner::Wgpu(b) => b.destroy(),
        }
    }
}

#[derive(Clone, Copy)]
pub struct BufferSlice<'a> {
    buffer: &'a Buffer,
    offset: u64,
    size: u64,
}

impl<'a> BufferSlice<'a> {
    #[cfg(feature = "wgpu")]
    fn wg(&self) -> wgpu::BufferSlice<'a> {
        self.buffer.wg().slice(self.offset..self.offset + self.size)
    }

    /// Calls `callback` (from [`Device::poll`]) once the work submitted so
    /// far completes; then [`BufferSlice::get_mapped_range`] reads it.
    pub fn map_async(
        &self,
        mode: MapMode,
        callback: impl FnOnce(Result<(), BufferAsyncError>) + Send + 'static,
    ) {
        let _ = mode;
        match &self.buffer.0 {
            #[cfg(target_os = "linux")]
            BufferInner::Vulkan(b) => b.map_async(self.offset, self.size, Box::new(callback)),
            #[cfg(target_os = "macos")]
            BufferInner::Metal(b) => b.map_async(self.offset, self.size, Box::new(callback)),
            #[cfg(target_os = "windows")]
            BufferInner::D3d11(b) => b.map_async(self.offset, self.size, Box::new(callback)),
            #[cfg(feature = "wgpu")]
            BufferInner::Wgpu(_) => self.wg().map_async(wgpu::MapMode::Read, move |r| {
                callback(r.map_err(|_| BufferAsyncError));
            }),
        }
    }

    pub fn get_mapped_range(&self) -> Result<BufferView<'a>, BufferAsyncError> {
        match &self.buffer.0 {
            #[cfg(target_os = "linux")]
            BufferInner::Vulkan(b) => b
                .copy_mapped_range(self.offset, self.size)
                .map(|bytes| BufferView(ViewBytes::Owned(bytes), std::marker::PhantomData)),
            #[cfg(target_os = "macos")]
            BufferInner::Metal(b) => b
                .copy_mapped_range(self.offset, self.size)
                .map(|bytes| BufferView(ViewBytes::Owned(bytes), std::marker::PhantomData)),
            #[cfg(target_os = "windows")]
            BufferInner::D3d11(b) => b
                .copy_mapped_range(self.offset, self.size)
                .map(|bytes| BufferView(ViewBytes::Owned(bytes), std::marker::PhantomData)),
            #[cfg(feature = "wgpu")]
            BufferInner::Wgpu(_) => self
                .wg()
                .get_mapped_range()
                .map(|v| BufferView(ViewBytes::Wgpu(v), std::marker::PhantomData))
                .map_err(|_| BufferAsyncError),
        }
    }
}

/// A mapped range, readable while it lives.
pub struct BufferView<'a>(ViewBytes, std::marker::PhantomData<&'a [u8]>);

enum ViewBytes {
    #[cfg_attr(
        not(any(target_os = "linux", target_os = "macos", target_os = "windows")),
        expect(dead_code, reason = "native only")
    )]
    Owned(Vec<u8>),
    #[cfg(feature = "wgpu")]
    Wgpu(wgpu::BufferView),
}

/// Ends the mapping's borrow: [`Buffer::unmap`] needs every view dropped.
impl Drop for BufferView<'_> {
    fn drop(&mut self) {}
}

impl std::ops::Deref for BufferView<'_> {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        match &self.0 {
            ViewBytes::Owned(s) => s,
            #[cfg(feature = "wgpu")]
            ViewBytes::Wgpu(v) => v,
        }
    }
}

impl PartialEq for TextureView {
    fn eq(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            #[cfg(target_os = "linux")]
            (ViewInner::Vulkan(a), ViewInner::Vulkan(b)) => Arc::ptr_eq(&a.0, &b.0),
            #[cfg(target_os = "macos")]
            (ViewInner::Metal(a), ViewInner::Metal(b)) => Arc::ptr_eq(&a.0, &b.0),
            #[cfg(target_os = "windows")]
            (ViewInner::D3d11(a), ViewInner::D3d11(b)) => Arc::ptr_eq(&a.0, &b.0),
            #[cfg(feature = "wgpu")]
            (ViewInner::Wgpu(a), ViewInner::Wgpu(b)) => a == b,
            #[cfg(all(
                any(target_os = "linux", target_os = "macos", target_os = "windows"),
                feature = "wgpu"
            ))]
            _ => false,
        }
    }
}

impl PartialEq for Texture {
    fn eq(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            #[cfg(target_os = "linux")]
            (TextureInner::Vulkan(a), TextureInner::Vulkan(b)) => Arc::ptr_eq(&a.0, &b.0),
            #[cfg(target_os = "macos")]
            (TextureInner::Metal(a), TextureInner::Metal(b)) => Arc::ptr_eq(&a.0, &b.0),
            #[cfg(target_os = "windows")]
            (TextureInner::D3d11(a), TextureInner::D3d11(b)) => Arc::ptr_eq(&a.0, &b.0),
            #[cfg(feature = "wgpu")]
            (TextureInner::Wgpu(a), TextureInner::Wgpu(b)) => a == b,
            #[cfg(all(
                any(target_os = "linux", target_os = "macos", target_os = "windows"),
                feature = "wgpu"
            ))]
            _ => false,
        }
    }
}

impl Texture {
    pub fn create_view(&self, desc: &TextureViewDescriptor<'_>) -> TextureView {
        TextureView(match &self.0 {
            #[cfg(target_os = "linux")]
            TextureInner::Vulkan(t) => ViewInner::Vulkan(match t.create_view(desc.dimension) {
                Ok(v) => v,
                Err(e) => {
                    t.device().errors().report(e.into_hal());
                    vk::TextureView::invalid(t.clone())
                }
            }),
            #[cfg(target_os = "macos")]
            TextureInner::Metal(t) => ViewInner::Metal(match t.create_view(desc.dimension) {
                Ok(v) => v,
                Err(e) => {
                    t.device().errors().report(e.into_hal());
                    mtl::TextureView::invalid(t.clone())
                }
            }),
            #[cfg(target_os = "windows")]
            TextureInner::D3d11(t) => ViewInner::D3d11(match t.create_view(desc.dimension) {
                Ok(v) => v,
                Err(e) => {
                    t.device().errors().report(e.into_hal());
                    d3d::TextureView::invalid(t.clone())
                }
            }),
            #[cfg(feature = "wgpu")]
            TextureInner::Wgpu(t) => ViewInner::Wgpu(t.create_view(&wgpu::TextureViewDescriptor {
                label: desc.label,
                dimension: desc.dimension.map(wg::view_dimension),
                ..Default::default()
            })),
        })
    }

    pub fn size(&self) -> Extent3d {
        Extent3d {
            width: self.width(),
            height: self.height(),
            depth_or_array_layers: self.depth_or_array_layers(),
        }
    }

    pub fn width(&self) -> u32 {
        match &self.0 {
            #[cfg(target_os = "linux")]
            TextureInner::Vulkan(t) => t.width(),
            #[cfg(target_os = "macos")]
            TextureInner::Metal(t) => t.width(),
            #[cfg(target_os = "windows")]
            TextureInner::D3d11(t) => t.width(),
            #[cfg(feature = "wgpu")]
            TextureInner::Wgpu(t) => t.width(),
        }
    }

    pub fn height(&self) -> u32 {
        match &self.0 {
            #[cfg(target_os = "linux")]
            TextureInner::Vulkan(t) => t.height(),
            #[cfg(target_os = "macos")]
            TextureInner::Metal(t) => t.height(),
            #[cfg(target_os = "windows")]
            TextureInner::D3d11(t) => t.height(),
            #[cfg(feature = "wgpu")]
            TextureInner::Wgpu(t) => t.height(),
        }
    }

    pub fn depth_or_array_layers(&self) -> u32 {
        match &self.0 {
            #[cfg(target_os = "linux")]
            TextureInner::Vulkan(t) => t.layers(),
            #[cfg(target_os = "macos")]
            TextureInner::Metal(t) => t.layers(),
            #[cfg(target_os = "windows")]
            TextureInner::D3d11(t) => t.layers(),
            #[cfg(feature = "wgpu")]
            TextureInner::Wgpu(t) => t.depth_or_array_layers(),
        }
    }

    pub fn format(&self) -> TextureFormat {
        match &self.0 {
            #[cfg(target_os = "linux")]
            TextureInner::Vulkan(t) => t.format(),
            #[cfg(target_os = "macos")]
            TextureInner::Metal(t) => t.format(),
            #[cfg(target_os = "windows")]
            TextureInner::D3d11(t) => t.format(),
            #[cfg(feature = "wgpu")]
            TextureInner::Wgpu(t) => {
                wg::from_format(t.format()).expect("a texture mui-hal can name")
            }
        }
    }

    pub fn usage(&self) -> TextureUsages {
        match &self.0 {
            #[cfg(target_os = "linux")]
            TextureInner::Vulkan(t) => t.usage(),
            #[cfg(target_os = "macos")]
            TextureInner::Metal(t) => t.usage(),
            #[cfg(target_os = "windows")]
            TextureInner::D3d11(t) => t.usage(),
            #[cfg(feature = "wgpu")]
            TextureInner::Wgpu(t) => wg::from_texture_usages(t.usage()),
        }
    }

    pub fn as_image_copy(&self) -> TexelCopyTextureInfo<'_> {
        TexelCopyTextureInfo {
            texture: self,
            mip_level: 0,
            origin: Origin3d::ZERO,
            aspect: TextureAspect::All,
        }
    }

    pub fn destroy(&self) {
        #[cfg(feature = "wgpu")]
        if let TextureInner::Wgpu(t) = &self.0 {
            t.destroy();
        }
    }
}

impl PipelineCache {
    pub fn get_data(&self) -> Option<Vec<u8>> {
        match &self.0 {
            #[cfg(target_os = "linux")]
            CacheInner::Vulkan(c) => c.get_data(),
            #[cfg(target_os = "macos")]
            CacheInner::Metal(c) => c.get_data(),
            #[cfg(target_os = "windows")]
            CacheInner::D3d11(c) => c.get_data(),
            #[cfg(feature = "wgpu")]
            CacheInner::Wgpu(c) => c.get_data(),
        }
    }
}

#[derive(Clone)]
enum ModuleInner {
    #[cfg(target_os = "linux")]
    Vulkan(vk::ShaderModule),
    #[cfg(target_os = "macos")]
    Metal(mtl::ShaderModule),
    #[cfg(target_os = "windows")]
    D3d11(d3d::ShaderModule),
    #[cfg(feature = "wgpu")]
    Wgpu(wgpu::ShaderModule),
}

#[derive(Clone)]
pub struct ShaderModule {
    id: ShaderId,
    inner: ModuleInner,
}

impl std::fmt::Debug for ShaderModule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShaderModule")
            .field("id", &self.id)
            .finish()
    }
}

impl ShaderModule {
    #[cfg(target_os = "linux")]
    fn vk(&self) -> &vk::ShaderModule {
        match &self.inner {
            ModuleInner::Vulkan(m) => m,
            #[cfg(feature = "wgpu")]
            ModuleInner::Wgpu(_) => panic!("a wgpu shader module on Vulkan"),
        }
    }

    #[cfg(target_os = "macos")]
    fn mtl(&self) -> &mtl::ShaderModule {
        match &self.inner {
            ModuleInner::Metal(m) => m,
            #[cfg(feature = "wgpu")]
            ModuleInner::Wgpu(_) => panic!("a wgpu shader module on Metal"),
        }
    }

    #[cfg(target_os = "windows")]
    fn d3d(&self) -> &d3d::ShaderModule {
        match &self.inner {
            ModuleInner::D3d11(m) => m,
            #[cfg(feature = "wgpu")]
            ModuleInner::Wgpu(_) => panic!("a wgpu shader module on D3D11"),
        }
    }

    #[cfg(feature = "wgpu")]
    fn wg(&self) -> &wgpu::ShaderModule {
        match &self.inner {
            ModuleInner::Wgpu(m) => m,
            #[cfg(target_os = "linux")]
            ModuleInner::Vulkan(_) => panic!("a Vulkan shader module on wgpu"),
            #[cfg(target_os = "macos")]
            ModuleInner::Metal(_) => panic!("a Metal shader module on wgpu"),
            #[cfg(target_os = "windows")]
            ModuleInner::D3d11(_) => panic!("a D3D11 shader module on wgpu"),
        }
    }
}

pub struct BufferBinding<'a> {
    pub buffer: &'a Buffer,
    pub offset: u64,
    pub size: Option<std::num::NonZeroU64>,
}

pub enum BindingResource<'a> {
    Buffer(BufferBinding<'a>),
    TextureView(&'a TextureView),
    Sampler(&'a Sampler),
}

pub struct BindGroupEntry<'a> {
    pub binding: u32,
    pub resource: BindingResource<'a>,
}

pub struct BindGroupDescriptor<'a> {
    pub label: Option<&'a str>,
    pub layout: &'a BindGroupLayout,
    pub entries: &'a [BindGroupEntry<'a>],
}

pub struct PipelineLayoutDescriptor<'a> {
    pub label: Option<&'a str>,
    pub bind_group_layouts: &'a [Option<&'a BindGroupLayout>],
    /// Push-constant bytes; MUI uses none.
    pub immediate_size: u32,
}

/// Which shader: MUI's are named, never passed as source.
#[derive(Clone, Copy, Debug)]
pub enum ShaderSource {
    Mui(ShaderId),
}

#[derive(Clone, Copy, Debug)]
pub struct ShaderModuleDescriptor<'a> {
    pub label: Option<&'a str>,
    pub source: ShaderSource,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct PipelineCompilationOptions;

pub struct VertexState<'a> {
    pub module: &'a ShaderModule,
    pub entry_point: Option<&'a str>,
    pub compilation_options: PipelineCompilationOptions,
    pub buffers: &'a [Option<VertexBufferLayout<'a>>],
}

pub struct FragmentState<'a> {
    pub module: &'a ShaderModule,
    pub entry_point: Option<&'a str>,
    pub compilation_options: PipelineCompilationOptions,
    pub targets: &'a [Option<ColorTargetState>],
}

/// No depth or stencil in MUI: always `None`.
#[derive(Clone, Copy, Debug)]
pub enum DepthStencilState {}

pub struct RenderPipelineDescriptor<'a> {
    pub label: Option<&'a str>,
    pub layout: Option<&'a PipelineLayout>,
    pub vertex: VertexState<'a>,
    pub primitive: PrimitiveState,
    pub depth_stencil: Option<DepthStencilState>,
    pub multisample: MultisampleState,
    pub fragment: Option<FragmentState<'a>>,
    pub multiview_mask: Option<std::num::NonZeroU32>,
    pub cache: Option<&'a PipelineCache>,
}

#[derive(Clone, Debug, Default)]
pub struct CommandEncoderDescriptor<'a> {
    pub label: Option<&'a str>,
}

pub struct PipelineCacheDescriptor<'a> {
    pub label: Option<&'a str>,
    pub data: Option<&'a [u8]>,
    /// Vulkan always falls back to an empty cache when the driver refuses.
    pub fallback: bool,
}

#[derive(Clone, Copy)]
pub struct TexelCopyTextureInfo<'a> {
    pub texture: &'a Texture,
    pub mip_level: u32,
    pub origin: Origin3d,
    pub aspect: TextureAspect,
}

#[cfg(target_os = "linux")]
impl TexelCopyTextureInfo<'_> {
    /// Native textures have one mip level.
    fn single_mip(&self) -> &vk::Texture {
        assert_eq!(self.mip_level, 0, "native textures have one mip level");
        self.texture.vk()
    }
}

#[derive(Clone, Copy)]
pub struct TexelCopyBufferInfo<'a> {
    pub buffer: &'a Buffer,
    pub layout: TexelCopyBufferLayout,
}

pub struct RenderPassColorAttachment<'a> {
    pub view: &'a TextureView,
    pub depth_slice: Option<u32>,
    pub resolve_target: Option<&'a TextureView>,
    pub ops: Operations<Color>,
}

pub struct RenderPassTimestampWrites<'a> {
    pub query_set: &'a QuerySet,
    pub beginning_of_pass_write_index: Option<u32>,
    pub end_of_pass_write_index: Option<u32>,
}

/// No depth attachment in MUI: always `None`.
#[derive(Clone, Copy, Debug)]
pub enum RenderPassDepthStencilAttachment {}

pub struct RenderPassDescriptor<'a> {
    pub label: Option<&'a str>,
    pub color_attachments: &'a [Option<RenderPassColorAttachment<'a>>],
    pub depth_stencil_attachment: Option<RenderPassDepthStencilAttachment>,
    pub timestamp_writes: Option<RenderPassTimestampWrites<'a>>,
    pub occlusion_query_set: Option<&'a QuerySet>,
    pub multiview_mask: Option<std::num::NonZeroU32>,
}

enum EncoderInner {
    #[cfg(target_os = "linux")]
    Vulkan(Vec<vk::Cmd>),
    #[cfg(target_os = "macos")]
    Metal(Vec<mtl::Cmd>),
    #[cfg(target_os = "windows")]
    D3d11(Vec<d3d::Cmd>),
    #[cfg(feature = "wgpu")]
    Wgpu(wgpu::CommandEncoder),
}

pub struct CommandEncoder(EncoderInner);

/// A finished encoder, ready to submit.
pub struct CommandBuffer(EncoderInner);

impl CommandEncoder {
    pub fn finish(self) -> CommandBuffer {
        CommandBuffer(self.0)
    }

    pub fn begin_render_pass<'e>(&'e mut self, desc: &RenderPassDescriptor<'_>) -> RenderPass<'e> {
        let [Some(attachment)] = desc.color_attachments else {
            panic!("mui-hal passes have exactly one colour attachment");
        };
        let inner = match &mut self.0 {
            #[cfg(target_os = "linux")]
            EncoderInner::Vulkan(_) => PassInner::Vulkan(vk::PassRecord {
                target: attachment.view.vk().clone(),
                clear: match attachment.ops.load {
                    LoadOp::Clear(c) => Some([c.r as f32, c.g as f32, c.b as f32, c.a as f32]),
                    LoadOp::Load => None,
                },
                timestamps: desc.timestamp_writes.as_ref().map(|t| {
                    (
                        t.query_set.vk().clone(),
                        t.beginning_of_pass_write_index,
                        t.end_of_pass_write_index,
                    )
                }),
                cmds: Vec::new(),
            }),
            #[cfg(target_os = "macos")]
            EncoderInner::Metal(_) => PassInner::Metal(mtl::PassRecord {
                target: attachment.view.mtl().clone(),
                clear: match attachment.ops.load {
                    LoadOp::Clear(c) => Some([c.r as f32, c.g as f32, c.b as f32, c.a as f32]),
                    LoadOp::Load => None,
                },
                timestamps: desc.timestamp_writes.as_ref().map(|t| {
                    (
                        t.query_set.mtl().clone(),
                        t.beginning_of_pass_write_index,
                        t.end_of_pass_write_index,
                    )
                }),
                cmds: Vec::new(),
            }),
            #[cfg(target_os = "windows")]
            EncoderInner::D3d11(_) => PassInner::D3d11(d3d::PassRecord {
                target: attachment.view.d3d().clone(),
                clear: match attachment.ops.load {
                    LoadOp::Clear(c) => Some([c.r as f32, c.g as f32, c.b as f32, c.a as f32]),
                    LoadOp::Load => None,
                },
                timestamps: desc.timestamp_writes.as_ref().map(|t| {
                    (
                        t.query_set.d3d().clone(),
                        t.beginning_of_pass_write_index,
                        t.end_of_pass_write_index,
                    )
                }),
                cmds: Vec::new(),
            }),
            #[cfg(feature = "wgpu")]
            EncoderInner::Wgpu(e) => {
                let pass = e.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: desc.label,
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: attachment.view.wg(),
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: match attachment.ops.load {
                                LoadOp::Clear(c) => wgpu::LoadOp::Clear(wg::color(c)),
                                LoadOp::Load => wgpu::LoadOp::Load,
                            },
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: desc.timestamp_writes.as_ref().map(|t| {
                        wgpu::RenderPassTimestampWrites {
                            query_set: t.query_set.wg(),
                            beginning_of_pass_write_index: t.beginning_of_pass_write_index,
                            end_of_pass_write_index: t.end_of_pass_write_index,
                        }
                    }),
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                PassInner::Wgpu(Box::new(pass.forget_lifetime()))
            }
        };
        RenderPass {
            encoder: self,
            inner: Some(inner),
        }
    }

    pub fn copy_buffer_to_buffer(
        &mut self,
        src: &Buffer,
        src_offset: u64,
        dst: &Buffer,
        dst_offset: u64,
        size: impl Into<Option<u64>>,
    ) {
        let size = size.into();
        match &mut self.0 {
            #[cfg(target_os = "linux")]
            EncoderInner::Vulkan(c) => c.push(vk::Cmd::CopyBuffer {
                src: src.vk().clone(),
                src_offset,
                dst: dst.vk().clone(),
                dst_offset,
                size: size.unwrap_or_else(|| src.size().saturating_sub(src_offset)),
            }),
            #[cfg(target_os = "macos")]
            EncoderInner::Metal(c) => c.push(mtl::Cmd::CopyBuffer {
                src: src.mtl().clone(),
                src_offset,
                dst: dst.mtl().clone(),
                dst_offset,
                size: size.unwrap_or_else(|| src.size().saturating_sub(src_offset)),
            }),
            #[cfg(target_os = "windows")]
            EncoderInner::D3d11(c) => c.push(d3d::Cmd::CopyBuffer {
                src: src.d3d().clone(),
                src_offset,
                dst: dst.d3d().clone(),
                dst_offset,
                size: size.unwrap_or_else(|| src.size().saturating_sub(src_offset)),
            }),
            #[cfg(feature = "wgpu")]
            EncoderInner::Wgpu(e) => {
                e.copy_buffer_to_buffer(src.wg(), src_offset, dst.wg(), dst_offset, size);
            }
        }
    }

    pub fn copy_texture_to_texture(
        &mut self,
        src: TexelCopyTextureInfo<'_>,
        dst: TexelCopyTextureInfo<'_>,
        size: Extent3d,
    ) {
        match &mut self.0 {
            #[cfg(target_os = "linux")]
            EncoderInner::Vulkan(c) => c.push(vk::Cmd::TextureToTexture {
                src: src.single_mip().clone(),
                src_origin: src.origin,
                dst: dst.single_mip().clone(),
                dst_origin: dst.origin,
                size,
            }),
            #[cfg(target_os = "macos")]
            EncoderInner::Metal(c) => c.push(mtl::Cmd::TextureToTexture {
                src: src.texture.mtl().clone(),
                src_mip: src.mip_level,
                src_origin: src.origin,
                dst: dst.texture.mtl().clone(),
                dst_mip: dst.mip_level,
                dst_origin: dst.origin,
                size,
            }),
            #[cfg(target_os = "windows")]
            EncoderInner::D3d11(c) => c.push(d3d::Cmd::TextureToTexture {
                src: src.texture.d3d().clone(),
                src_mip: src.mip_level,
                src_origin: src.origin,
                dst: dst.texture.d3d().clone(),
                dst_mip: dst.mip_level,
                dst_origin: dst.origin,
                size,
            }),
            #[cfg(feature = "wgpu")]
            EncoderInner::Wgpu(e) => e.copy_texture_to_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: src.texture.wg(),
                    mip_level: src.mip_level,
                    origin: wg::origin(src.origin),
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyTextureInfo {
                    texture: dst.texture.wg(),
                    mip_level: dst.mip_level,
                    origin: wg::origin(dst.origin),
                    aspect: wgpu::TextureAspect::All,
                },
                wg::extent(size),
            ),
        }
    }

    pub fn copy_texture_to_buffer(
        &mut self,
        src: TexelCopyTextureInfo<'_>,
        dst: TexelCopyBufferInfo<'_>,
        size: Extent3d,
    ) {
        match &mut self.0 {
            #[cfg(target_os = "linux")]
            EncoderInner::Vulkan(c) => {
                let block = src.texture.format().block_size();
                c.push(vk::Cmd::TextureToBuffer {
                    src: src.single_mip().clone(),
                    origin: src.origin,
                    dst: dst.buffer.vk().clone(),
                    offset: dst.layout.offset,
                    row_length: dst.layout.bytes_per_row.map_or(size.width, |b| b / block),
                    image_height: dst.layout.rows_per_image.unwrap_or(size.height),
                    size,
                });
            }
            #[cfg(target_os = "macos")]
            EncoderInner::Metal(c) => {
                let block = src.texture.format().block_size();
                c.push(mtl::Cmd::TextureToBuffer {
                    src: src.texture.mtl().clone(),
                    mip_level: src.mip_level,
                    origin: src.origin,
                    dst: dst.buffer.mtl().clone(),
                    offset: dst.layout.offset,
                    bytes_per_row: dst.layout.bytes_per_row.unwrap_or(size.width * block),
                    rows_per_image: dst.layout.rows_per_image.unwrap_or(size.height),
                    size,
                });
            }
            #[cfg(target_os = "windows")]
            EncoderInner::D3d11(c) => {
                let block = src.texture.format().block_size();
                c.push(d3d::Cmd::TextureToBuffer {
                    src: src.texture.d3d().clone(),
                    mip_level: src.mip_level,
                    origin: src.origin,
                    dst: dst.buffer.d3d().clone(),
                    offset: dst.layout.offset,
                    bytes_per_row: dst.layout.bytes_per_row.unwrap_or(size.width * block),
                    rows_per_image: dst.layout.rows_per_image.unwrap_or(size.height),
                    size,
                });
            }
            #[cfg(feature = "wgpu")]
            EncoderInner::Wgpu(e) => e.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo {
                    texture: src.texture.wg(),
                    mip_level: src.mip_level,
                    origin: wg::origin(src.origin),
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyBufferInfo {
                    buffer: dst.buffer.wg(),
                    layout: wg::buffer_layout(dst.layout),
                },
                wg::extent(size),
            ),
        }
    }

    pub fn write_timestamp(&mut self, set: &QuerySet, index: u32) {
        match &mut self.0 {
            #[cfg(target_os = "linux")]
            EncoderInner::Vulkan(c) => c.push(vk::Cmd::Timestamp(set.vk().clone(), index)),
            #[cfg(target_os = "macos")]
            EncoderInner::Metal(c) => c.push(mtl::Cmd::Timestamp(set.mtl().clone(), index)),
            #[cfg(target_os = "windows")]
            EncoderInner::D3d11(c) => c.push(d3d::Cmd::Timestamp(set.d3d().clone(), index)),
            #[cfg(feature = "wgpu")]
            EncoderInner::Wgpu(e) => e.write_timestamp(set.wg(), index),
        }
    }

    pub fn resolve_query_set(
        &mut self,
        set: &QuerySet,
        range: Range<u32>,
        dst: &Buffer,
        offset: u64,
    ) {
        match &mut self.0 {
            #[cfg(target_os = "linux")]
            EncoderInner::Vulkan(c) => c.push(vk::Cmd::Resolve {
                set: set.vk().clone(),
                first: range.start,
                count: range.len() as u32,
                dst: dst.vk().clone(),
                offset,
            }),
            #[cfg(target_os = "macos")]
            EncoderInner::Metal(c) => c.push(mtl::Cmd::Resolve {
                set: set.mtl().clone(),
                first: range.start,
                count: range.len() as u32,
                dst: dst.mtl().clone(),
                offset,
            }),
            #[cfg(target_os = "windows")]
            EncoderInner::D3d11(c) => c.push(d3d::Cmd::Resolve {
                set: set.d3d().clone(),
                first: range.start,
                count: range.len() as u32,
                dst: dst.d3d().clone(),
                offset,
            }),
            #[cfg(feature = "wgpu")]
            EncoderInner::Wgpu(e) => e.resolve_query_set(set.wg(), range, dst.wg(), offset),
        }
    }
}

enum PassInner {
    #[cfg(target_os = "linux")]
    Vulkan(vk::PassRecord),
    #[cfg(target_os = "macos")]
    Metal(mtl::PassRecord),
    #[cfg(target_os = "windows")]
    D3d11(d3d::PassRecord),
    #[cfg(feature = "wgpu")]
    Wgpu(Box<wgpu::RenderPass<'static>>),
}

/// A render pass recording into its encoder; ends when dropped.
pub struct RenderPass<'e> {
    encoder: &'e mut CommandEncoder,
    inner: Option<PassInner>,
}

impl Drop for RenderPass<'_> {
    fn drop(&mut self) {
        match (self.inner.take(), &mut self.encoder.0) {
            #[cfg(target_os = "linux")]
            (Some(PassInner::Vulkan(record)), EncoderInner::Vulkan(c)) => {
                c.push(vk::Cmd::Pass(record));
            }
            #[cfg(target_os = "macos")]
            (Some(PassInner::Metal(record)), EncoderInner::Metal(c)) => {
                c.push(mtl::Cmd::Pass(record));
            }
            #[cfg(target_os = "windows")]
            (Some(PassInner::D3d11(record)), EncoderInner::D3d11(c)) => {
                c.push(d3d::Cmd::Pass(record));
            }
            // wgpu's pass ends itself when dropped.
            _ => {}
        }
    }
}

impl RenderPass<'_> {
    pub fn set_pipeline(&mut self, pipeline: &RenderPipeline) {
        match self.inner.as_mut() {
            #[cfg(target_os = "linux")]
            Some(PassInner::Vulkan(r)) => r.cmds.push(vk::PassCmd::Pipeline(pipeline.vk().clone())),
            #[cfg(target_os = "macos")]
            Some(PassInner::Metal(r)) => {
                r.cmds.push(mtl::PassCmd::Pipeline(pipeline.mtl().clone()));
            }
            #[cfg(target_os = "windows")]
            Some(PassInner::D3d11(r)) => {
                r.cmds.push(d3d::PassCmd::Pipeline(pipeline.d3d().clone()));
            }
            #[cfg(feature = "wgpu")]
            Some(PassInner::Wgpu(p)) => p.set_pipeline(pipeline.wg()),
            None => {}
        }
    }

    pub fn set_bind_group<'g>(
        &mut self,
        index: u32,
        group: impl Into<Option<&'g BindGroup>>,
        offsets: &[u32],
    ) {
        let Some(group) = group.into() else { return };
        match self.inner.as_mut() {
            #[cfg(target_os = "linux")]
            Some(PassInner::Vulkan(r)) => r.cmds.push(vk::PassCmd::Group(
                index,
                group.vk().clone(),
                offsets.to_vec(),
            )),
            #[cfg(target_os = "macos")]
            Some(PassInner::Metal(r)) => r.cmds.push(mtl::PassCmd::Group(
                index,
                group.mtl().clone(),
                offsets.to_vec(),
            )),
            #[cfg(target_os = "windows")]
            Some(PassInner::D3d11(r)) => r.cmds.push(d3d::PassCmd::Group(
                index,
                group.d3d().clone(),
                offsets.to_vec(),
            )),
            #[cfg(feature = "wgpu")]
            Some(PassInner::Wgpu(p)) => p.set_bind_group(index, group.wg(), offsets),
            None => {}
        }
    }

    pub fn set_vertex_buffer(&mut self, slot: u32, slice: BufferSlice<'_>) {
        match self.inner.as_mut() {
            #[cfg(target_os = "linux")]
            Some(PassInner::Vulkan(r)) => r.cmds.push(vk::PassCmd::Vertex(
                slot,
                slice.buffer.vk().clone(),
                slice.offset,
            )),
            #[cfg(target_os = "macos")]
            Some(PassInner::Metal(r)) => r.cmds.push(mtl::PassCmd::Vertex(
                slot,
                slice.buffer.mtl().clone(),
                slice.offset,
            )),
            #[cfg(target_os = "windows")]
            Some(PassInner::D3d11(r)) => r.cmds.push(d3d::PassCmd::Vertex(
                slot,
                slice.buffer.d3d().clone(),
                slice.offset,
            )),
            #[cfg(feature = "wgpu")]
            Some(PassInner::Wgpu(p)) => p.set_vertex_buffer(slot, slice.wg()),
            None => {}
        }
    }

    pub fn set_viewport(&mut self, x: f32, y: f32, w: f32, h: f32, min_depth: f32, max_depth: f32) {
        match self.inner.as_mut() {
            #[cfg(target_os = "linux")]
            Some(PassInner::Vulkan(r)) => r
                .cmds
                .push(vk::PassCmd::Viewport([x, y, w, h, min_depth, max_depth])),
            #[cfg(target_os = "macos")]
            Some(PassInner::Metal(r)) => r
                .cmds
                .push(mtl::PassCmd::Viewport([x, y, w, h, min_depth, max_depth])),
            #[cfg(target_os = "windows")]
            Some(PassInner::D3d11(r)) => r
                .cmds
                .push(d3d::PassCmd::Viewport([x, y, w, h, min_depth, max_depth])),
            #[cfg(feature = "wgpu")]
            Some(PassInner::Wgpu(p)) => p.set_viewport(x, y, w, h, min_depth, max_depth),
            None => {}
        }
    }

    pub fn set_scissor_rect(&mut self, x: u32, y: u32, w: u32, h: u32) {
        match self.inner.as_mut() {
            #[cfg(target_os = "linux")]
            Some(PassInner::Vulkan(r)) => r.cmds.push(vk::PassCmd::Scissor([x, y, w, h])),
            #[cfg(target_os = "macos")]
            Some(PassInner::Metal(r)) => r.cmds.push(mtl::PassCmd::Scissor([x, y, w, h])),
            #[cfg(target_os = "windows")]
            Some(PassInner::D3d11(r)) => r.cmds.push(d3d::PassCmd::Scissor([x, y, w, h])),
            #[cfg(feature = "wgpu")]
            Some(PassInner::Wgpu(p)) => p.set_scissor_rect(x, y, w, h),
            None => {}
        }
    }

    pub fn draw(&mut self, vertices: Range<u32>, instances: Range<u32>) {
        match self.inner.as_mut() {
            #[cfg(target_os = "linux")]
            Some(PassInner::Vulkan(r)) => r.cmds.push(vk::PassCmd::Draw {
                vertices,
                instances,
            }),
            #[cfg(target_os = "macos")]
            Some(PassInner::Metal(r)) => r.cmds.push(mtl::PassCmd::Draw {
                vertices,
                instances,
            }),
            #[cfg(target_os = "windows")]
            Some(PassInner::D3d11(r)) => r.cmds.push(d3d::PassCmd::Draw {
                vertices,
                instances,
            }),
            #[cfg(feature = "wgpu")]
            Some(PassInner::Wgpu(p)) => p.draw(vertices, instances),
            None => {}
        }
    }
}

/// Anything kept alive for its drop alone.
trait Keep: Send + Sync {}
impl<T: Send + Sync> Keep for T {}

enum SurfaceInner<'w> {
    #[cfg(target_os = "linux")]
    Vulkan(Arc<vk::Surface>),
    #[cfg(target_os = "macos")]
    Metal(Arc<mtl::Surface>),
    #[cfg(target_os = "windows")]
    D3d11(Arc<d3d::Surface>),
    #[cfg(feature = "wgpu")]
    Wgpu(wgpu::Surface<'w>),
    #[cfg(not(feature = "wgpu"))]
    #[expect(dead_code, reason = "carries 'w without wgpu")]
    Never(std::marker::PhantomData<&'w ()>, std::convert::Infallible),
}

/// A window's surface, holding the window target it was made from.
///
/// `Surface` is `Send` (so a window thread can hand a created surface to a
/// render worker) and intentionally `!Sync` to prevent concurrent cross-thread
/// surface reconfiguration or presentation races.
pub struct Surface<'w> {
    inner: SurfaceInner<'w>,
    _not_sync: std::marker::PhantomData<std::cell::Cell<()>>,
    // Declared last: drops after the swapchain.
    _target: Option<Box<dyn Keep + 'w>>,
}

// wgpu's types are `!Send` on wasm32 without `fragile-send-sync-non-atomic-wasm`.
#[cfg(not(all(target_arch = "wasm32", feature = "wgpu")))]
const _: () = {
    const fn assert_send<T: Send>() {}
    assert_send::<Surface<'static>>();
    assert_send::<Device>();
};

impl Drop for Surface<'_> {
    fn drop(&mut self) {
        // Frames share the native surface; end its swapchain while the window
        // owner (`_target`) still lives.
        #[cfg(target_os = "linux")]
        if let SurfaceInner::Vulkan(s) = &self.inner {
            s.release();
        }
        #[cfg(target_os = "macos")]
        if let SurfaceInner::Metal(s) = &self.inner {
            s.release();
        }
        #[cfg(target_os = "windows")]
        if let SurfaceInner::D3d11(s) = &self.inner {
            s.release();
        }
    }
}

impl std::fmt::Debug for Surface<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Surface").finish_non_exhaustive()
    }
}

impl<'w> Surface<'w> {
    /// Wraps a wgpu surface a host made.
    #[cfg(feature = "wgpu")]
    pub fn from_wgpu(surface: wgpu::Surface<'w>) -> Self {
        Self {
            inner: SurfaceInner::Wgpu(surface),
            _not_sync: std::marker::PhantomData,
            _target: None,
        }
    }

    /// What the surface offers. On native backends the swapchain settles format
    /// and alpha when configured ([`Surface::alpha_mode`]); this lists what it
    /// may pick.
    /// Whether this is a native (not wgpu) surface.
    fn is_native(&self) -> bool {
        match &self.inner {
            #[cfg(target_os = "linux")]
            SurfaceInner::Vulkan(_) => true,
            #[cfg(target_os = "macos")]
            SurfaceInner::Metal(_) => true,
            #[cfg(target_os = "windows")]
            SurfaceInner::D3d11(_) => true,
            #[cfg(feature = "wgpu")]
            SurfaceInner::Wgpu(_) => false,
            #[cfg(not(feature = "wgpu"))]
            SurfaceInner::Never(_, n) => match *n {},
        }
    }

    pub fn get_capabilities(&self, adapter: &Adapter) -> SurfaceCapabilities {
        match &self.inner {
            #[cfg(target_os = "linux")]
            SurfaceInner::Vulkan(_) if !adapter.is_native() => SurfaceCapabilities {
                formats: Vec::new(),
                present_modes: Vec::new(),
                alpha_modes: Vec::new(),
                usages: TextureUsages::empty(),
            },
            #[cfg(target_os = "linux")]
            SurfaceInner::Vulkan(_) => SurfaceCapabilities {
                formats: vec![TextureFormat::Bgra8Unorm, TextureFormat::Rgba8Unorm],
                present_modes: vec![PresentMode::Mailbox, PresentMode::Fifo],
                alpha_modes: vec![
                    CompositeAlphaMode::Opaque,
                    CompositeAlphaMode::PreMultiplied,
                    CompositeAlphaMode::PostMultiplied,
                ],
                usages: TextureUsages::RENDER_ATTACHMENT,
            },
            #[cfg(target_os = "macos")]
            SurfaceInner::Metal(_) if !adapter.is_native() => SurfaceCapabilities {
                formats: Vec::new(),
                present_modes: Vec::new(),
                alpha_modes: Vec::new(),
                usages: TextureUsages::empty(),
            },
            #[cfg(target_os = "macos")]
            SurfaceInner::Metal(_) => SurfaceCapabilities {
                formats: vec![
                    TextureFormat::Bgra8Unorm,
                    TextureFormat::Bgra8UnormSrgb,
                    TextureFormat::Rgba16Float,
                ],
                present_modes: vec![PresentMode::Fifo, PresentMode::Immediate],
                alpha_modes: vec![
                    CompositeAlphaMode::Opaque,
                    CompositeAlphaMode::PreMultiplied,
                ],
                usages: TextureUsages::RENDER_ATTACHMENT
                    | TextureUsages::COPY_SRC
                    | TextureUsages::COPY_DST
                    | TextureUsages::TEXTURE_BINDING,
            },
            #[cfg(target_os = "windows")]
            SurfaceInner::D3d11(_) if !adapter.is_native() => SurfaceCapabilities {
                formats: Vec::new(),
                present_modes: Vec::new(),
                alpha_modes: Vec::new(),
                usages: TextureUsages::empty(),
            },
            #[cfg(target_os = "windows")]
            SurfaceInner::D3d11(_) => SurfaceCapabilities {
                formats: vec![TextureFormat::Bgra8Unorm, TextureFormat::Rgba8Unorm],
                present_modes: vec![
                    PresentMode::Fifo,
                    PresentMode::Mailbox,
                    PresentMode::Immediate,
                ],
                alpha_modes: vec![
                    CompositeAlphaMode::Opaque,
                    CompositeAlphaMode::PreMultiplied,
                ],
                usages: TextureUsages::RENDER_ATTACHMENT,
            },
            #[cfg(feature = "wgpu")]
            SurfaceInner::Wgpu(s) => {
                let Some(a) = adapter.as_wgpu() else {
                    return SurfaceCapabilities {
                        formats: Vec::new(),
                        present_modes: Vec::new(),
                        alpha_modes: Vec::new(),
                        usages: TextureUsages::empty(),
                    };
                };
                let caps = s.get_capabilities(a);
                SurfaceCapabilities {
                    formats: caps
                        .formats
                        .into_iter()
                        .filter_map(wg::from_format)
                        .collect(),
                    present_modes: caps
                        .present_modes
                        .into_iter()
                        .map(wg::from_present_mode)
                        .collect(),
                    alpha_modes: caps
                        .alpha_modes
                        .into_iter()
                        .map(wg::from_alpha_mode)
                        .collect(),
                    usages: wg::from_texture_usages(caps.usages),
                }
            }
            #[cfg(not(feature = "wgpu"))]
            SurfaceInner::Never(_, n) => {
                let _ = adapter;
                match *n {}
            }
        }
    }

    /// A configuration at `width` x `height` in the first format and alpha
    /// mode the surface offers, or `None` when it offers none.
    pub fn get_default_config(
        &self,
        adapter: &Adapter,
        width: u32,
        height: u32,
    ) -> Option<SurfaceConfiguration> {
        let caps = self.get_capabilities(adapter);
        Some(SurfaceConfiguration {
            usage: TextureUsages::RENDER_ATTACHMENT,
            format: *caps.formats.first()?,
            width,
            height,
            present_mode: PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: *caps.alpha_modes.first()?,
            view_formats: Vec::new(),
        })
    }

    /// Builds the swapchain for `config`. A failure goes to `device`'s
    /// error scopes as a validation error.
    pub fn configure(&self, device: &Device, config: &SurfaceConfiguration) {
        match (&self.inner, &device.0.inner) {
            #[cfg(target_os = "linux")]
            (SurfaceInner::Vulkan(s), DeviceInner::Vulkan(o)) => {
                let translucent = matches!(
                    config.alpha_mode,
                    CompositeAlphaMode::PreMultiplied | CompositeAlphaMode::PostMultiplied
                );
                if let Err(e) = s.configure(&o.0, (config.width, config.height), translucent) {
                    device.report(Error::Validation {
                        description: e.to_string(),
                    });
                }
            }
            #[cfg(target_os = "macos")]
            (SurfaceInner::Metal(s), DeviceInner::Metal(o)) => {
                if let Err(e) = s.configure(&o.0, config) {
                    device.report(e.into_hal());
                }
            }
            #[cfg(target_os = "windows")]
            (SurfaceInner::D3d11(s), DeviceInner::D3d11(o)) => {
                if let Err(e) = s.configure(&o.0, config) {
                    device.report(e.into_hal());
                }
            }
            #[cfg(feature = "wgpu")]
            (SurfaceInner::Wgpu(s), DeviceInner::Wgpu(d, _)) => {
                s.configure(d, &wg::surface_config(config));
            }
            #[cfg(all(
                any(target_os = "linux", target_os = "macos", target_os = "windows"),
                feature = "wgpu"
            ))]
            _ => device.report(Error::Validation {
                description: "the surface and device are on different backends".into(),
            }),
            #[cfg(not(feature = "wgpu"))]
            (SurfaceInner::Never(_, n), _) => match *n {},
        }
    }

    /// The alpha mode the configured swapchain composites with (native), or
    /// `None` where the configuration's own is in effect.
    pub fn alpha_mode(&self) -> Option<CompositeAlphaMode> {
        match &self.inner {
            #[cfg(target_os = "linux")]
            SurfaceInner::Vulkan(s) => s.alpha_mode(),
            #[cfg(target_os = "macos")]
            SurfaceInner::Metal(s) => s.alpha_mode(),
            #[cfg(target_os = "windows")]
            SurfaceInner::D3d11(s) => s.alpha_mode(),
            #[cfg(feature = "wgpu")]
            SurfaceInner::Wgpu(_) => None,
            #[cfg(not(feature = "wgpu"))]
            SurfaceInner::Never(_, n) => match *n {},
        }
    }

    /// The configured swapchain's format (native), or `None` where the
    /// configuration's own is in effect.
    pub fn format(&self) -> Option<TextureFormat> {
        match &self.inner {
            #[cfg(target_os = "linux")]
            SurfaceInner::Vulkan(s) => s.format(),
            #[cfg(target_os = "macos")]
            SurfaceInner::Metal(s) => s.format(),
            #[cfg(target_os = "windows")]
            SurfaceInner::D3d11(s) => s.format(),
            #[cfg(feature = "wgpu")]
            SurfaceInner::Wgpu(_) => None,
            #[cfg(not(feature = "wgpu"))]
            SurfaceInner::Never(_, n) => match *n {},
        }
    }

    /// Wayland: the surface's logical size, for fractional scaling.
    pub fn set_logical_size(&self, width: u32, height: u32) {
        match &self.inner {
            #[cfg(target_os = "linux")]
            SurfaceInner::Vulkan(s) => s.set_logical_size(width, height),
            #[cfg(target_os = "macos")]
            SurfaceInner::Metal(_) => {
                let _ = (width, height);
            }
            #[cfg(target_os = "windows")]
            SurfaceInner::D3d11(_) => {
                let _ = (width, height);
            }
            #[cfg(feature = "wgpu")]
            SurfaceInner::Wgpu(_) => {}
            #[cfg(not(feature = "wgpu"))]
            SurfaceInner::Never(_, n) => match *n {},
        }
    }

    pub fn get_current_texture(&self) -> CurrentSurfaceTexture {
        match &self.inner {
            #[cfg(target_os = "linux")]
            SurfaceInner::Vulkan(s) => match s.acquire() {
                vk::Acquired::Image(f) => {
                    let suboptimal = f.suboptimal;
                    let frame = SurfaceTexture {
                        texture: Texture(TextureInner::Vulkan(f.texture.clone())),
                        view: TextureView(ViewInner::Vulkan(f.view.clone())),
                        inner: SurfaceTextureInner::Vulkan(s.clone(), f),
                    };
                    if suboptimal {
                        CurrentSurfaceTexture::Suboptimal(frame)
                    } else {
                        CurrentSurfaceTexture::Success(frame)
                    }
                }
                vk::Acquired::Outdated => CurrentSurfaceTexture::Outdated,
                vk::Acquired::Timeout => CurrentSurfaceTexture::Timeout,
                vk::Acquired::Lost => CurrentSurfaceTexture::Lost,
                vk::Acquired::Failed(e) => {
                    eprintln!("mui-hal: acquiring a swapchain image failed: {e}");
                    CurrentSurfaceTexture::Lost
                }
            },
            #[cfg(target_os = "macos")]
            SurfaceInner::Metal(s) => match s.acquire() {
                mtl::Acquired::Image(f) => CurrentSurfaceTexture::Success(SurfaceTexture {
                    texture: Texture(TextureInner::Metal(f.texture.clone())),
                    view: TextureView(ViewInner::Metal(f.view.clone())),
                    inner: SurfaceTextureInner::Metal(s.clone(), f),
                }),
                mtl::Acquired::Outdated => CurrentSurfaceTexture::Outdated,
                mtl::Acquired::Timeout => CurrentSurfaceTexture::Timeout,
                mtl::Acquired::Lost => CurrentSurfaceTexture::Lost,
                mtl::Acquired::Failed(e) => {
                    eprintln!("mui-hal: acquiring a Metal drawable failed: {e}");
                    CurrentSurfaceTexture::Lost
                }
            },
            #[cfg(target_os = "windows")]
            SurfaceInner::D3d11(s) => match s.acquire() {
                d3d::Acquired::Image(f) => CurrentSurfaceTexture::Success(SurfaceTexture {
                    texture: Texture(TextureInner::D3d11(f.texture.clone())),
                    view: TextureView(ViewInner::D3d11(f.view.clone())),
                    inner: SurfaceTextureInner::D3d11(s.clone(), f),
                }),
                d3d::Acquired::Outdated => CurrentSurfaceTexture::Outdated,
                d3d::Acquired::Occluded => CurrentSurfaceTexture::Occluded,
                d3d::Acquired::Lost => CurrentSurfaceTexture::Lost,
                d3d::Acquired::Failed(e) => {
                    eprintln!("mui-hal: acquiring a D3D11 back buffer failed: {e}");
                    CurrentSurfaceTexture::Lost
                }
            },
            #[cfg(feature = "wgpu")]
            SurfaceInner::Wgpu(s) => {
                let wrap = |t: wgpu::SurfaceTexture| SurfaceTexture {
                    texture: Texture(TextureInner::Wgpu(t.texture.clone())),
                    view: TextureView(ViewInner::Wgpu(
                        t.texture
                            .create_view(&wgpu::TextureViewDescriptor::default()),
                    )),
                    inner: SurfaceTextureInner::Wgpu(t),
                };
                match s.get_current_texture() {
                    wgpu::CurrentSurfaceTexture::Success(t) => {
                        CurrentSurfaceTexture::Success(wrap(t))
                    }
                    wgpu::CurrentSurfaceTexture::Suboptimal(t) => {
                        CurrentSurfaceTexture::Suboptimal(wrap(t))
                    }
                    wgpu::CurrentSurfaceTexture::Timeout => CurrentSurfaceTexture::Timeout,
                    wgpu::CurrentSurfaceTexture::Occluded => CurrentSurfaceTexture::Occluded,
                    wgpu::CurrentSurfaceTexture::Outdated => CurrentSurfaceTexture::Outdated,
                    wgpu::CurrentSurfaceTexture::Lost => CurrentSurfaceTexture::Lost,
                    wgpu::CurrentSurfaceTexture::Validation => CurrentSurfaceTexture::Validation,
                }
            }
            #[cfg(not(feature = "wgpu"))]
            SurfaceInner::Never(_, n) => match *n {},
        }
    }
}

enum SurfaceTextureInner {
    #[cfg(target_os = "linux")]
    Vulkan(Arc<vk::Surface>, vk::SurfaceTexture),
    #[cfg(target_os = "macos")]
    Metal(Arc<mtl::Surface>, mtl::SurfaceTexture),
    #[cfg(target_os = "windows")]
    D3d11(Arc<d3d::Surface>, d3d::SurfaceTexture),
    #[cfg(feature = "wgpu")]
    Wgpu(wgpu::SurfaceTexture),
}

/// An acquired surface image: render into `texture`, then
/// [`Queue::present`] it.
pub struct SurfaceTexture {
    pub texture: Texture,
    /// A view of `texture`, made with the swapchain.
    pub view: TextureView,
    inner: SurfaceTextureInner,
}

impl std::fmt::Debug for SurfaceTexture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SurfaceTexture").finish_non_exhaustive()
    }
}

/// What [`Surface::get_current_texture`] came to.
pub enum CurrentSurfaceTexture {
    Success(SurfaceTexture),
    Suboptimal(SurfaceTexture),
    Outdated,
    Occluded,
    Timeout,
    Lost,
    Validation,
}

#[cfg(all(test, target_os = "linux"))]
mod tests;
