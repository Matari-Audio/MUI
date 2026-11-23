//! Owned Linux Vulkan swapchain (`ash` `VK_KHR_swapchain`) over Xlib, XCB, or
//! a dedicated fresh Wayland child `wl_subsurface`.
//!
//! Never reuses a `wl_surface` that had `wl_shm` attached: on Wayland,
//! [`VkSwapchain`] constructs its own [`WaylandSubsurface`] and tears it down
//! cleanly on drop.

use ash::{khr, vk};
use raw_window_handle::{RawDisplayHandle, RawWindowHandle};

use super::wayland::WaylandSubsurface;
use crate::{PresentError, PresentMode, SurfaceFormat};

/// Result of acquiring the next presentable swapchain image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AcquiredImage {
    /// Index into [`VkSwapchain::images`] and [`VkSwapchain::image_views`].
    pub index: u32,
    /// `true` when the swapchain still presents but should be reconfigured after present.
    pub suboptimal: bool,
}

struct SwapchainImages {
    loader: khr::swapchain::Device,
    device: ash::Device,
    handle: vk::SwapchainKHR,
    images: Vec<vk::Image>,
    views: Vec<vk::ImageView>,
}

impl Drop for SwapchainImages {
    #[expect(unsafe_code, reason = "release owned Vulkan swapchain resources")]
    fn drop(&mut self) {
        // SAFETY: the owner retires all GPU work before releasing these resources.
        unsafe {
            for view in self.views.drain(..) {
                self.device.destroy_image_view(view, None);
            }
            self.loader.destroy_swapchain(self.handle, None);
        }
    }
}

/// Owned Vulkan WSI surface + swapchain for Linux X11 and Wayland windows.
pub struct VkSwapchain {
    surface_loader: khr::surface::Instance,
    swapchain_loader: khr::swapchain::Device,
    device: ash::Device,
    physical_device: vk::PhysicalDevice,
    surface: vk::SurfaceKHR,
    swapchain: Option<SwapchainImages>,
    vk_format: vk::Format,
    format: SurfaceFormat,
    extent: (u32, u32),
    translucent: bool,
    composite_alpha: vk::CompositeAlphaFlagsKHR,
    /// Dedicated child `wl_subsurface` kept alive until after `vkDestroySurfaceKHR`.
    wayland_subsurface: Option<WaylandSubsurface>,
}

impl VkSwapchain {
    /// Required Vulkan instance extensions for the given Linux display handle.
    #[must_use]
    pub fn required_instance_extensions(
        display: RawDisplayHandle,
    ) -> &'static [&'static std::ffi::CStr] {
        match display {
            RawDisplayHandle::Wayland(_) => &[khr::surface::NAME, khr::wayland_surface::NAME],
            RawDisplayHandle::Xcb(_) => &[khr::surface::NAME, khr::xcb_surface::NAME],
            _ => &[khr::surface::NAME, khr::xlib_surface::NAME],
        }
    }

    /// Creates a Vulkan WSI surface and configures an initial swapchain.
    ///
    /// # Safety
    /// - `entry`, `instance`, `physical_device`, and `device` must be valid Vulkan handles.
    /// - `instance` must have every extension [`Self::required_instance_extensions`]
    ///   returns for `display` enabled, and `device` must have `VK_KHR_swapchain`
    ///   enabled.
    /// - `display` and `window` must remain live on their owning window until
    ///   this [`VkSwapchain`] is dropped.
    /// - All GPU work referencing swapchain images must finish before calling
    ///   [`Self::configure`] or dropping [`VkSwapchain`].
    ///
    /// # Errors
    /// Returns [`PresentError`] when the handle is unsupported, the adapter
    /// cannot present to the surface, or swapchain creation fails.
    #[expect(
        unsafe_code,
        reason = "Vulkan WSI surface and swapchain creation via ash"
    )]
    #[expect(
        clippy::too_many_arguments,
        reason = "explicit Vulkan WSI handles and initial config"
    )]
    pub unsafe fn new(
        entry: &ash::Entry,
        instance: &ash::Instance,
        physical_device: vk::PhysicalDevice,
        device: ash::Device,
        queue_family: u32,
        display: RawDisplayHandle,
        window: RawWindowHandle,
        size: (u32, u32),
        want_translucent: bool,
    ) -> Result<Self, PresentError> {
        let surface_loader = khr::surface::Instance::new(entry, instance);
        let swapchain_loader = khr::swapchain::Device::new(instance, &device);

        let (surface, wayland_subsurface) = match (display, window) {
            (RawDisplayHandle::Xlib(d), RawWindowHandle::Xlib(w)) => {
                let dpy = d
                    .display
                    .ok_or(PresentError::UnsupportedHandle("null Xlib Display*"))?;
                let loader = khr::xlib_surface::Instance::new(entry, instance);
                let info = vk::XlibSurfaceCreateInfoKHR::default()
                    .dpy(dpy.as_ptr().cast())
                    .window(w.window);
                // SAFETY: caller upholds live Xlib display and window handles.
                let s = unsafe { loader.create_xlib_surface(&info, None) }.map_err(|e| {
                    PresentError::Platform(format!("vkCreateXlibSurfaceKHR failed: {e}"))
                })?;
                (s, None)
            }
            (RawDisplayHandle::Xcb(d), RawWindowHandle::Xcb(w)) => {
                let conn = d
                    .connection
                    .ok_or(PresentError::UnsupportedHandle("null XCB connection"))?;
                let loader = khr::xcb_surface::Instance::new(entry, instance);
                let info = vk::XcbSurfaceCreateInfoKHR::default()
                    .connection(conn.as_ptr().cast())
                    .window(w.window.get());
                // SAFETY: caller upholds live XCB connection and window handles.
                let s = unsafe { loader.create_xcb_surface(&info, None) }.map_err(|e| {
                    PresentError::Platform(format!("vkCreateXcbSurfaceKHR failed: {e}"))
                })?;
                (s, None)
            }
            (RawDisplayHandle::Wayland(d), RawWindowHandle::Wayland(w)) => {
                // Dedicated fresh child wl_subsurface: never shares a wl_surface with wl_shm.
                // SAFETY: caller upholds live parent Wayland display and surface.
                let child =
                    unsafe { WaylandSubsurface::new(d.display.as_ptr(), w.surface.as_ptr())? };
                let loader = khr::wayland_surface::Instance::new(entry, instance);
                let info = vk::WaylandSurfaceCreateInfoKHR::default()
                    .display(child.display_ptr().cast())
                    .surface(child.surface_ptr().cast());
                // SAFETY: `child` owns the fresh wl_surface and outlives `s`.
                let s = unsafe { loader.create_wayland_surface(&info, None) }.map_err(|e| {
                    PresentError::Platform(format!("vkCreateWaylandSurfaceKHR failed: {e}"))
                })?;
                (s, Some(child))
            }
            _ => {
                return Err(PresentError::UnsupportedHandle(
                    "expected Xlib, Xcb, or Wayland handles for Linux VkSwapchain",
                ));
            }
        };

        let mut this = Self {
            surface_loader,
            swapchain_loader,
            device,
            physical_device,
            surface,
            swapchain: None,
            vk_format: vk::Format::B8G8R8A8_UNORM,
            format: SurfaceFormat::Bgra8Unorm,
            extent: (0, 0),
            translucent: false,
            composite_alpha: vk::CompositeAlphaFlagsKHR::OPAQUE,
            wayland_subsurface,
        };

        // SAFETY: the caller supplies a valid physical device and queue family.
        let supported = unsafe {
            this.surface_loader.get_physical_device_surface_support(
                physical_device,
                queue_family,
                surface,
            )
        }
        .map_err(|e| PresentError::Platform(format!("surface support query failed: {e}")))?;
        if !supported {
            return Err(PresentError::Platform(
                "selected Vulkan queue family does not support presentation to this surface".into(),
            ));
        }
        this.configure(size, want_translucent)?;
        Ok(this)
    }

    /// Rebuilds the swapchain for `size` physical pixels and `want_translucent`.
    ///
    /// # Errors
    /// Returns [`PresentError`] when the surface offers no non-sRGB UNORM format
    /// or swapchain creation fails.
    #[expect(
        unsafe_code,
        reason = "Vulkan swapchain and image view creation/destruction"
    )]
    pub fn configure(
        &mut self,
        size: (u32, u32),
        want_translucent: bool,
    ) -> Result<(), PresentError> {
        if size.0 == 0 || size.1 == 0 {
            return Err(PresentError::InvalidSize(size));
        }
        // SAFETY: `physical_device` and `surface` are live Vulkan handles.
        let caps = unsafe {
            self.surface_loader
                .get_physical_device_surface_capabilities(self.physical_device, self.surface)
        }
        .map_err(|e| PresentError::Platform(format!("surface capabilities failed: {e}")))?;
        // SAFETY: same as above.
        let formats = unsafe {
            self.surface_loader
                .get_physical_device_surface_formats(self.physical_device, self.surface)
        }
        .map_err(|e| PresentError::Platform(format!("surface formats failed: {e}")))?;
        // SAFETY: same as above.
        let present_modes = unsafe {
            self.surface_loader
                .get_physical_device_surface_present_modes(self.physical_device, self.surface)
        }
        .map_err(|e| PresentError::Platform(format!("surface present modes failed: {e}")))?;

        let (vk_format, color_space, format) = formats
            .iter()
            .find_map(|sf| match sf.format {
                vk::Format::B8G8R8A8_UNORM => {
                    Some((sf.format, sf.color_space, SurfaceFormat::Bgra8Unorm))
                }
                vk::Format::R8G8B8A8_UNORM => {
                    Some((sf.format, sf.color_space, SurfaceFormat::Rgba8Unorm))
                }
                _ => None,
            })
            .ok_or_else(|| {
                PresentError::Platform("surface offers no non-sRGB UNORM format".into())
            })?;

        let offered_modes: Vec<PresentMode> = present_modes
            .iter()
            .filter_map(|&m| match m {
                vk::PresentModeKHR::MAILBOX => Some(PresentMode::Mailbox),
                vk::PresentModeKHR::FIFO => Some(PresentMode::Fifo),
                vk::PresentModeKHR::FIFO_RELAXED => Some(PresentMode::FifoRelaxed),
                vk::PresentModeKHR::IMMEDIATE => Some(PresentMode::Immediate),
                _ => None,
            })
            .collect();
        let vk_present_mode = match PresentMode::choose(&offered_modes) {
            PresentMode::Mailbox => vk::PresentModeKHR::MAILBOX,
            PresentMode::Fifo => vk::PresentModeKHR::FIFO,
            PresentMode::FifoRelaxed => vk::PresentModeKHR::FIFO_RELAXED,
            PresentMode::Immediate => vk::PresentModeKHR::IMMEDIATE,
        };

        let (composite_alpha, translucent) =
            choose_composite_alpha(caps.supported_composite_alpha, want_translucent);

        let extent = if caps.current_extent.width == u32::MAX {
            vk::Extent2D {
                width: size
                    .0
                    .clamp(caps.min_image_extent.width, caps.max_image_extent.width),
                height: size
                    .1
                    .clamp(caps.min_image_extent.height, caps.max_image_extent.height),
            }
        } else {
            caps.current_extent
        };
        if extent.width == 0 || extent.height == 0 {
            return Err(PresentError::InvalidSize((extent.width, extent.height)));
        }

        let mut image_count = caps.min_image_count.max(2);
        if caps.max_image_count > 0 {
            image_count = image_count.min(caps.max_image_count);
        }

        // Vulkan retires oldSwapchain even when vkCreateSwapchainKHR fails.
        let old = self.swapchain.take();
        self.extent = (0, 0);
        self.translucent = false;
        let create_info = vk::SwapchainCreateInfoKHR::default()
            .surface(self.surface)
            .min_image_count(image_count)
            .image_format(vk_format)
            .image_color_space(color_space)
            .image_extent(extent)
            .image_array_layers(1)
            .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT)
            .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
            .pre_transform(caps.current_transform)
            .composite_alpha(composite_alpha)
            .present_mode(vk_present_mode)
            .clipped(true)
            .old_swapchain(old.as_ref().map_or(vk::SwapchainKHR::null(), |s| s.handle));

        // SAFETY: all fields were validated against caps, formats, and present modes.
        let handle = unsafe { self.swapchain_loader.create_swapchain(&create_info, None) }
            .map_err(|e| PresentError::Platform(format!("vkCreateSwapchainKHR failed: {e}")))?;
        drop(old);

        let mut replacement = SwapchainImages {
            loader: self.swapchain_loader.clone(),
            device: self.device.clone(),
            handle,
            images: Vec::new(),
            views: Vec::new(),
        };
        // SAFETY: the replacement owns this live handle through every error path.
        replacement.images = unsafe { replacement.loader.get_swapchain_images(handle) }
            .map_err(|e| PresentError::Platform(format!("get_swapchain_images failed: {e}")))?;
        replacement.views.reserve(replacement.images.len());
        for &image in &replacement.images {
            let view_info = vk::ImageViewCreateInfo::default()
                .image(image)
                .view_type(vk::ImageViewType::TYPE_2D)
                .format(vk_format)
                .subresource_range(vk::ImageSubresourceRange {
                    aspect_mask: vk::ImageAspectFlags::COLOR,
                    base_mip_level: 0,
                    level_count: 1,
                    base_array_layer: 0,
                    layer_count: 1,
                });
            // SAFETY: `image` belongs to `new_swapchain` with matching `vk_format`.
            let view = unsafe { self.device.create_image_view(&view_info, None) }
                .map_err(|e| PresentError::Platform(format!("create_image_view failed: {e}")))?;
            replacement.views.push(view);
        }

        self.swapchain = Some(replacement);
        self.vk_format = vk_format;
        self.format = format;
        self.extent = (extent.width, extent.height);
        self.translucent = translucent;
        self.composite_alpha = composite_alpha;
        Ok(())
    }

    /// Acquires the next swapchain image index, signaling `semaphore`.
    ///
    /// # Errors
    /// Returns [`PresentError::OutOfDate`] or [`PresentError::SurfaceLost`] on
    /// WSI status changes.
    ///
    /// # Safety
    /// `semaphore` must be a live, unsignaled binary semaphore on this device
    /// with no pending operations. The caller must synchronize access to both
    /// the semaphore and swapchain and obey Vulkan's image acquisition limits.
    /// `timeout_ns == u64::MAX` is allowed only while the acquire is guaranteed
    /// to make forward progress: at most `images().len()` minus the surface's
    /// `minImageCount` images currently acquired. Pass a finite timeout otherwise.
    #[expect(unsafe_code, reason = "Vulkan vkAcquireNextImageKHR")]
    pub unsafe fn acquire_next_image(
        &self,
        semaphore: vk::Semaphore,
        timeout_ns: u64,
    ) -> Result<AcquiredImage, PresentError> {
        let swapchain = self.swapchain.as_ref().ok_or(PresentError::OutOfDate)?;
        // SAFETY: the swapchain is owned and the caller validates the semaphore.
        match unsafe {
            self.swapchain_loader.acquire_next_image(
                swapchain.handle,
                timeout_ns,
                semaphore,
                vk::Fence::null(),
            )
        } {
            Ok((index, suboptimal)) => Ok(AcquiredImage { index, suboptimal }),
            Err(vk::Result::TIMEOUT | vk::Result::NOT_READY) => Err(PresentError::Timeout),
            Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => Err(PresentError::OutOfDate),
            Err(vk::Result::ERROR_SURFACE_LOST_KHR) => Err(PresentError::SurfaceLost),
            Err(e) => Err(PresentError::Platform(format!(
                "vkAcquireNextImageKHR failed: {e}"
            ))),
        }
    }

    /// Presents swapchain image `index` after waiting on `wait_semaphore`.
    ///
    /// Returns `Ok(true)` if the swapchain reported `SUBOPTIMAL_KHR` and should
    /// be reconfigured before the next frame.
    ///
    /// # Errors
    /// Returns [`PresentError::OutOfDate`] or [`PresentError::SurfaceLost`] on
    /// WSI status changes.
    ///
    /// # Safety
    /// `queue` must be live on this device and support presentation to this
    /// surface. `index` must identify an acquired image in present layout.
    /// `wait_semaphore` must be a live binary semaphore signaled by rendering.
    /// The caller must synchronize access to the queue and swapchain and keep
    /// all resources alive until the presentation operations complete.
    #[expect(unsafe_code, reason = "Vulkan vkQueuePresentKHR")]
    pub unsafe fn present(
        &self,
        queue: vk::Queue,
        index: u32,
        wait_semaphore: vk::Semaphore,
    ) -> Result<bool, PresentError> {
        let swapchain = self.swapchain.as_ref().ok_or(PresentError::OutOfDate)?;
        let swapchains = [swapchain.handle];
        let indices = [index];
        let waits = [wait_semaphore];
        let info = vk::PresentInfoKHR::default()
            .wait_semaphores(&waits)
            .swapchains(&swapchains)
            .image_indices(&indices);
        // SAFETY: `queue`, `swapchains`, and `waits` are valid Vulkan handles.
        match unsafe { self.swapchain_loader.queue_present(queue, &info) } {
            Ok(suboptimal) => Ok(suboptimal),
            Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => Err(PresentError::OutOfDate),
            Err(vk::Result::ERROR_SURFACE_LOST_KHR) => Err(PresentError::SurfaceLost),
            Err(e) => Err(PresentError::Platform(format!(
                "vkQueuePresentKHR failed: {e}"
            ))),
        }
    }

    /// Updates the Wayland child subsurface logical destination size when on Wayland.
    pub fn set_wayland_logical_size(&self, logical_width: u32, logical_height: u32) {
        if let Some(sub) = &self.wayland_subsurface {
            sub.set_logical_size(logical_width, logical_height);
        }
    }

    /// Swapchain images for layout transitions.
    #[must_use]
    pub fn images(&self) -> &[vk::Image] {
        self.swapchain.as_ref().map_or(&[], |s| &s.images)
    }

    /// Swapchain image views for render pass attachments.
    #[must_use]
    pub fn image_views(&self) -> &[vk::ImageView] {
        self.swapchain.as_ref().map_or(&[], |s| &s.views)
    }

    /// Active Vulkan pixel format.
    #[must_use]
    pub const fn vk_format(&self) -> vk::Format {
        self.vk_format
    }

    /// Active portable surface format.
    #[must_use]
    pub const fn format(&self) -> SurfaceFormat {
        self.format
    }

    /// Configured swapchain dimensions in physical pixels.
    #[must_use]
    pub const fn extent(&self) -> (u32, u32) {
        self.extent
    }

    /// The composite-alpha mode the swapchain was created with.
    #[must_use]
    pub const fn composite_alpha(&self) -> vk::CompositeAlphaFlagsKHR {
        self.composite_alpha
    }

    /// Whether the swapchain composites alpha with the desktop compositor.
    #[must_use]
    pub const fn translucent(&self) -> bool {
        self.translucent
    }
}

/// Picks an advertised composite-alpha mode: a blending one when translucency
/// is wanted, else `OPAQUE`, else `INHERIT`, else whichever blending mode the
/// surface offers. The flag reports whether the compositor blends alpha, so
/// an opaque caller must then write alpha = 1.
fn choose_composite_alpha(
    supported: vk::CompositeAlphaFlagsKHR,
    want_translucent: bool,
) -> (vk::CompositeAlphaFlagsKHR, bool) {
    use vk::CompositeAlphaFlagsKHR as A;
    let blending = [A::PRE_MULTIPLIED, A::POST_MULTIPLIED];
    let order: &[A] = if want_translucent {
        &[A::PRE_MULTIPLIED, A::POST_MULTIPLIED, A::OPAQUE, A::INHERIT]
    } else {
        &[A::OPAQUE, A::INHERIT, A::PRE_MULTIPLIED, A::POST_MULTIPLIED]
    };
    // Vulkan guarantees at least one bit; INHERIT is a last resort otherwise.
    let mode = order
        .iter()
        .copied()
        .find(|&m| supported.contains(m))
        .unwrap_or(A::INHERIT);
    (mode, blending.contains(&mode))
}

impl Drop for VkSwapchain {
    #[expect(unsafe_code, reason = "ordered Vulkan swapchain and surface teardown")]
    fn drop(&mut self) {
        drop(self.swapchain.take());
        if self.surface != vk::SurfaceKHR::null() {
            // SAFETY: swapchain is destroyed; surface drops before Wayland child subsurface.
            unsafe {
                self.surface_loader.destroy_surface(self.surface, None);
            }
            self.surface = vk::SurfaceKHR::null();
        }
        // Wayland child subsurface drops last, after VkSurfaceKHR is gone.
        self.wayland_subsurface.take();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vk::CompositeAlphaFlagsKHR as A;

    #[test]
    fn composite_alpha_always_picks_an_advertised_bit() {
        assert_eq!(
            choose_composite_alpha(A::OPAQUE | A::PRE_MULTIPLIED, true),
            (A::PRE_MULTIPLIED, true)
        );
        assert_eq!(
            choose_composite_alpha(A::OPAQUE | A::PRE_MULTIPLIED, false),
            (A::OPAQUE, false)
        );
        assert_eq!(
            choose_composite_alpha(A::INHERIT, true),
            (A::INHERIT, false)
        );
        // Only a blending mode advertised: use it and report blending.
        assert_eq!(
            choose_composite_alpha(A::PRE_MULTIPLIED, false),
            (A::PRE_MULTIPLIED, true)
        );
        assert_eq!(
            choose_composite_alpha(A::POST_MULTIPLIED, false),
            (A::POST_MULTIPLIED, true)
        );
    }
}
