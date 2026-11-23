//! Observable presentation state for MUI windows and editors.

/// High-level presentation pipeline currently delivering frames to a window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PresentationMode {
    /// GPU renders directly into a native window swapchain with no CPU readback.
    DirectGpu,
    /// GPU renders offscreen and transfers damaged bitmaps to the window thread.
    ReadbackGpu,
    /// CPU renders at 1:1 physical pixel resolution and presents directly.
    #[default]
    CpuDirect,
    /// CPU renders within the pixel budget and scales up to fill the window.
    CpuScaled,
}

/// Concrete OS presentation mechanism backing a surface.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PresentationBackend {
    /// Windows DirectComposition visual + flip-model DXGI swapchain (D3D12).
    WinDirectComposition,
    /// Windows Vulkan `VkSwapchainKHR` on the `HWND` (when D3D12 is unavailable).
    WinVkSwapchain,
    /// Windows retained `CreateDIBSection` painted inside `WM_PAINT`.
    WinDibWmPaint,
    /// macOS `CAMetalLayer` GPU drawable presentation.
    MacMetalLayer,
    /// macOS `IOSurface` ring attached to a dedicated `CALayer.contents`.
    MacIoSurface,
    /// Linux Vulkan `VkSwapchainKHR` (X11 or dedicated Wayland child subsurface).
    LinuxVkSwapchain,
    /// Linux direct X11 MIT-SHM shared-memory blit via `x11rb`.
    LinuxX11Shm,
    /// Linux direct X11 `PutImage` fallback via `x11rb`.
    LinuxX11PutImage,
    /// Linux Wayland `wl_shm` ring on a dedicated child `wl_subsurface`.
    LinuxWaylandShm,
    /// Headless or offscreen target with no window-system swapchain.
    #[default]
    Offscreen,
}

impl PresentationBackend {
    /// Returns `true` when this backend presents GPU-rendered swapchain frames.
    #[must_use]
    pub const fn is_gpu(self) -> bool {
        matches!(
            self,
            Self::WinDirectComposition
                | Self::WinVkSwapchain
                | Self::MacMetalLayer
                | Self::LinuxVkSwapchain
        )
    }

    /// The window-system mechanism presenting frames rendered with `api` on
    /// this OS; [`Self::Offscreen`] for software or an API the OS cannot present.
    #[must_use]
    pub const fn for_gpu(api: GraphicsApi) -> Self {
        match api {
            // MoltenVK draws into the view's CAMetalLayer too.
            GraphicsApi::Metal | GraphicsApi::Vulkan if cfg!(target_os = "macos") => {
                Self::MacMetalLayer
            }
            GraphicsApi::D3D12 if cfg!(target_os = "windows") => Self::WinDirectComposition,
            GraphicsApi::Vulkan if cfg!(target_os = "windows") => Self::WinVkSwapchain,
            GraphicsApi::Vulkan if cfg!(target_os = "linux") => Self::LinuxVkSwapchain,
            _ => Self::Offscreen,
        }
    }

    /// Returns `true` when this backend presents CPU-uploaded pixel buffers.
    #[must_use]
    pub const fn is_cpu(self) -> bool {
        matches!(
            self,
            Self::WinDibWmPaint
                | Self::MacIoSurface
                | Self::LinuxX11Shm
                | Self::LinuxX11PutImage
                | Self::LinuxWaylandShm
        )
    }
}

/// Graphics API that rendered the last frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum GraphicsApi {
    /// CPU rasterization.
    #[default]
    Software,
    /// Vulkan.
    Vulkan,
    /// Direct3D 12.
    D3D12,
    /// Metal.
    Metal,
}

/// Why a window presents CPU frames instead of GPU ones.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FallbackReason {
    /// Software mode was forced by the user toggle or environment.
    ForceCpu,
    /// The crash sentinel quarantined the driver of every usable adapter.
    DriverSentinelTripped,
    /// No adapter could present to this window.
    AdapterUnavailable,
    /// GPU initialization failed for another reason.
    InitializationFailed,
    /// The GPU device was lost.
    DeviceLost,
    /// The window's GPU surface was lost and could not be replaced.
    SurfaceLost,
    /// A GPU frame failed.
    RenderFailed,
}

/// Observable snapshot of a window's active presentation pipeline.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PresentationState {
    /// Active presentation mode.
    pub mode: PresentationMode,
    /// Active platform presentation backend.
    pub backend: PresentationBackend,
    /// Graphics API that rendered the last frame.
    pub api: GraphicsApi,
    /// Why GPU frames are not presented, while on CPU frames after a GPU
    /// attempt (or forced software); `None` on GPU frames.
    pub fallback: Option<FallbackReason>,
    /// Native window surface dimensions in physical pixels.
    pub surface_size: (u32, u32),
    /// Backing raster dimensions in pixels (`== surface_size` unless scaled).
    pub render_size: (u32, u32),
    /// Total frames successfully presented on this window.
    pub frames_presented: u64,
    /// Whether the active surface composites per-pixel alpha with the OS desktop.
    pub translucent: bool,
    /// Whether software presentation was explicitly forced via env or user toggle.
    pub software_forced: bool,
    /// Whether a per-driver crash sentinel triggered automatic CPU fallback.
    pub driver_sentinel_tripped: bool,
    /// Human-readable reason for the current mode or last fallback.
    pub reason: String,
}

impl PresentationState {
    /// Records a successfully presented frame and updates dimensions and mode.
    pub fn record_frame(
        &mut self,
        mode: PresentationMode,
        backend: PresentationBackend,
        surface_size: (u32, u32),
        render_size: (u32, u32),
        reason: impl Into<String>,
    ) {
        self.mode = mode;
        self.backend = backend;
        self.surface_size = surface_size;
        self.render_size = render_size;
        self.frames_presented = self.frames_presented.saturating_add(1);
        self.reason = reason.into();
    }

    /// Records why the window presents CPU frames; `None` once GPU frames
    /// present. Keeps `driver_sentinel_tripped` in step with the reason.
    pub fn record_fallback(&mut self, reason: Option<FallbackReason>) {
        self.fallback = reason;
        self.driver_sentinel_tripped = reason == Some(FallbackReason::DriverSentinelTripped);
    }

    /// Returns `true` when the last presented frame used a CPU software path.
    #[must_use]
    pub const fn is_software(&self) -> bool {
        matches!(
            self.mode,
            PresentationMode::CpuDirect | PresentationMode::CpuScaled
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presentation_state_tracks_transitions_and_counts() {
        let mut state = PresentationState::default();
        assert!(state.is_software());
        assert_eq!(state.frames_presented, 0);

        state.record_frame(
            PresentationMode::CpuDirect,
            PresentationBackend::LinuxX11Shm,
            (800, 600),
            (800, 600),
            "CPU startup",
        );
        assert!(state.is_software());
        assert!(state.backend.is_cpu());
        assert!(!state.backend.is_gpu());
        assert_eq!(state.frames_presented, 1);

        state.record_frame(
            PresentationMode::DirectGpu,
            PresentationBackend::LinuxVkSwapchain,
            (800, 600),
            (800, 600),
            "GPU ready",
        );
        assert!(!state.is_software());
        assert!(state.backend.is_gpu());
        assert_eq!(state.frames_presented, 2);
    }

    #[test]
    fn fallback_reason_drives_the_sentinel_flag() {
        let mut state = PresentationState::default();
        state.record_fallback(Some(FallbackReason::DriverSentinelTripped));
        assert!(state.driver_sentinel_tripped);
        state.record_fallback(None);
        assert!(!state.driver_sentinel_tripped);
        assert_eq!(state.fallback, None);
        state.record_fallback(Some(FallbackReason::AdapterUnavailable));
        assert!(!state.driver_sentinel_tripped);
    }

    #[test]
    fn gpu_backend_follows_the_selected_api() {
        assert_eq!(
            PresentationBackend::for_gpu(GraphicsApi::Software),
            PresentationBackend::Offscreen
        );
        #[cfg(target_os = "linux")]
        {
            assert_eq!(
                PresentationBackend::for_gpu(GraphicsApi::Vulkan),
                PresentationBackend::LinuxVkSwapchain
            );
            assert_eq!(
                PresentationBackend::for_gpu(GraphicsApi::D3D12),
                PresentationBackend::Offscreen
            );
        }
    }
}
