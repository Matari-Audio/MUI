//! The window, the device, and the one Vello `Scene` everything draws into.
//!
//! Split out of `main.rs` because it is the half of this binary that a plugin
//! wrapper would own instead. It is deliberately not a crate: the only other
//! consumer anyone can name is KURV, and KURV's window comes from its plugin
//! host, not from winit.
//!
//! Everything here is in **physical pixels**. The surface, the scene and the
//! pointer are all the same space, so the gallery needs no conversion at all
//! between what it hit-tests and what it draws.

use std::sync::Arc;

use crate::device::OnDevice;
use vello_hybrid::{RenderSize, RenderTargetConfig, Renderer, Resources, Scene, TextureBindings};
use winit::window::Window;

/// The surface size clamped into what everything downstream can actually hold,
/// or `None` when there is nothing to draw into.
///
/// [`Scene`] is `u16`-dimensioned, so a plain `as u16` wraps a wide window to a
/// narrow scene silently. A minimised window reports 0x0, and configuring a
/// zero-sized surface is undefined rather than merely empty.
pub fn target_size(width: u32, height: u32) -> Option<(u32, u32)> {
    (width > 0 && height > 0).then(|| (width.min(u16::MAX as u32), height.min(u16::MAX as u32)))
}

pub struct Gpu {
    instance: wgpu::Instance,
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    gpu: OnDevice<Device>,
    vello: Scene,
}

/// Everything that lives on one device: rebuilt whole when it is lost. The
/// `Cache` too -- its atlas ids name slots in the dead renderer's atlas.
struct Device {
    config: wgpu::SurfaceConfiguration,
    renderer: Renderer,
    resources: Resources,
    cache: mui::vello::Cache,
}

impl Gpu {
    /// The display handle comes from the event loop rather than from the window
    /// so the GL backend can reach EGL/GLX. The surface is then created
    /// *without* a display handle, because handing wgpu a second one is how the
    /// GL path ends up with two EGL displays that disagree.
    ///
    /// Backend order is `WGPU_BACKEND`'s business, not ours -- a dev gallery has
    /// no opinion, and the plugin wrapper that does have one is not this code.
    pub async fn new(
        window: Arc<Window>,
        display: Box<winit::event_loop::OwnedDisplayHandle>,
    ) -> Self {
        let instance = wgpu::Instance::new(
            wgpu::InstanceDescriptor::new_with_display_handle_from_env(display),
        );
        let surface = instance
            .create_surface(wgpu::SurfaceTarget::from_window_without_display(
                window.clone(),
            ))
            .expect("surface");
        let gpu = OnDevice::open(&instance, Some(&surface), build(&surface, &window))
            .expect("initialize the gallery's GPU");
        let (width, height) = (gpu.state.config.width, gpu.state.config.height);
        Self {
            vello: Scene::new(width as u16, height as u16),
            instance,
            window,
            surface,
            gpu,
        }
    }

    pub fn window(&self) -> &Window {
        &self.window
    }

    /// The surface size in physical pixels. The only size in this program.
    pub fn size(&self) -> (u32, u32) {
        (self.gpu.state.config.width, self.gpu.state.config.height)
    }

    /// Reconfigure for a new surface size.
    ///
    /// The renderer is *not* rebuilt. [`Renderer::render`] takes a [`RenderSize`]
    /// per call, so the pipelines outlive every resize; rebuilding them was a
    /// guess, and it cost a pipeline compile on every drag of a window edge.
    /// The `Scene` does have to be resized -- it clips to its own dimensions,
    /// not to the render size.
    pub fn resize(&mut self, width: u32, height: u32) {
        let Some((width, height)) = target_size(width, height) else {
            return;
        };
        if (width, height) == self.size() {
            return;
        }
        let config = &mut self.gpu.state.config;
        config.width = width;
        config.height = height;
        self.surface.configure(&self.gpu.device, config);
        self.vello.reset_and_resize(width as u16, height as u16);
    }

    /// Clear the scene and hand it over as a `Canvas`. Returning it rather
    /// than exposing it as a field is what makes forgetting the reset
    /// impossible; the `Resources` ride along so a glyph run reuses Vello's
    /// hinted-outline cache rather than re-hinting every frame.
    pub fn begin(&mut self) -> mui::vello::Gpu<'_> {
        self.vello.reset();
        let OnDevice {
            device,
            queue,
            state,
            ..
        } = &mut self.gpu;
        mui::vello::Gpu {
            scene: &mut self.vello,
            resources: &mut state.resources,
            cache: &mut state.cache,
            atlas: Some(mui::vello::Atlas {
                renderer: &mut state.renderer,
                device,
                queue,
            }),
        }
    }

    /// Render and present what [`Gpu::begin`] handed out.
    ///
    /// Notifies the compositor immediately before presenting, which is what
    /// keeps a Wayland frame callback from stalling the next redraw.
    pub fn present(&mut self) {
        // What `begin` encoded may name atlas ids of the dead device; the
        // redraw re-encodes it against the new one.
        match self.gpu.recover(
            &self.instance,
            Some(&self.surface),
            build(&self.surface, &self.window),
        ) {
            Ok(false) => {}
            Ok(true) => return self.window.request_redraw(),
            Err(e) => return eprintln!("GPU device lost and not rebuilt: {e}"),
        }
        let (
            device,
            queue,
            Device {
                config,
                renderer,
                resources,
                ..
            },
        ) = (&self.gpu.device, &self.gpu.queue, &mut self.gpu.state);
        use wgpu::CurrentSurfaceTexture as Acquired;
        let frame = match self.surface.get_current_texture() {
            Acquired::Success(f) | Acquired::Suboptimal(f) => f,
            // Outdated means the swapchain needs rebuilding; the size has not
            // changed, so reconfigure with what we already have. Under
            // ControlFlow::Wait nothing else will ask for the frame we just
            // dropped, so ask here or the window stays stale until the next
            // input -- which is how a keyboard-driven resize left it blank.
            Acquired::Outdated => {
                self.surface.configure(device, config);
                self.window.request_redraw();
                return;
            }
            Acquired::Timeout => {
                self.window.request_redraw();
                return;
            }
            // Lost is not recoverable by reconfiguring -- wgpu wants the
            // surface, and possibly the device, rebuilt. A gallery is not worth
            // that machinery, but silence would look like a hang.
            Acquired::Lost => {
                eprintln!("surface lost; restart the gallery");
                return;
            }
            // Occluded (minimised) and Validation: skip the frame. Asking for a
            // redraw here would spin at 100% while minimised; `Occluded(false)`
            // re-arms the loop on its own.
            _ => return,
        };
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(config.format),
            ..Default::default()
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        match renderer.render(
            &self.vello,
            resources,
            device,
            queue,
            &mut encoder,
            &RenderSize {
                width: config.width,
                height: config.height,
            },
            &view,
            &TextureBindings::new(),
        ) {
            Ok(()) => {
                queue.submit([encoder.finish()]);
                self.window.pre_present_notify();
                frame.present();
            }
            // A black window in the binary whose whole job is showing you what
            // broke is the one failure that must not be silent.
            Err(e) => eprintln!("vello: {e}"),
        }
    }
}

/// The renderer and surface configuration for `surface` on a device: the
/// first frame and every device loss both come through here.
fn build<'a>(
    surface: &'a wgpu::Surface<'static>,
    window: &'a Window,
) -> impl FnOnce(&wgpu::Adapter, &wgpu::Device, &wgpu::Queue) -> Result<Device, String> + 'a {
    move |adapter, device, _| {
        let size = window.inner_size();
        let (width, height) = target_size(size.width, size.height).unwrap_or((1, 1));
        let caps = surface.get_capabilities(adapter);
        // Vello writes sRGB values, so an _Srgb surface would encode them a
        // second time. Take the linear sibling of the surface's own preferred
        // format -- picking the first non-sRGB entry instead lands on
        // Rgba16Unorm here, which needs a device feature we never asked for.
        let preferred = *caps.formats.first().ok_or("surface has no formats")?;
        let linear = preferred.remove_srgb_suffix();
        let format = if caps.formats.contains(&linear) {
            linear
        } else {
            preferred
        };
        let config = surface
            .get_default_config(adapter, width, height)
            .ok_or("surface config")?;
        let config = wgpu::SurfaceConfiguration { format, ..config };
        surface.configure(device, &config);
        let (renderer, resources) = Renderer::new(
            device,
            &RenderTargetConfig {
                format,
                width,
                height,
            },
        );
        Ok(Device {
            config,
            renderer,
            resources,
            cache: Default::default(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two ways a resize actually breaks: the `as u16` wrap, and
    /// configuring a minimised window.
    #[test]
    fn a_target_size_never_wraps() {
        assert_eq!(target_size(0, 720), None);
        assert_eq!(target_size(800, 0), None);
        assert_eq!(target_size(1, 1), Some((1, 1)));
        assert_eq!(target_size(70_000, 600), Some((65_535, 600)));
    }
}
