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
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    renderer: Renderer,
    resources: Resources,
    vello: Scene,
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
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await
            .expect("adapter");
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await
            .expect("device");

        let size = window.inner_size();
        let (width, height) = target_size(size.width, size.height).unwrap_or((1, 1));
        let caps = surface.get_capabilities(&adapter);
        // Vello writes sRGB values, so an _Srgb surface would encode them a
        // second time. Take the linear sibling of the surface's own preferred
        // format -- picking the first non-sRGB entry instead lands on
        // Rgba16Unorm here, which needs a device feature we never asked for.
        let preferred = caps.formats[0];
        let linear = preferred.remove_srgb_suffix();
        let format = if caps.formats.contains(&linear) {
            linear
        } else {
            preferred
        };
        let config = surface
            .get_default_config(&adapter, width, height)
            .expect("surface config");
        let config = wgpu::SurfaceConfiguration { format, ..config };
        surface.configure(&device, &config);

        let (renderer, resources) = Renderer::new(
            &device,
            &RenderTargetConfig {
                format,
                width,
                height,
            },
        );
        Self {
            vello: Scene::new(width as u16, height as u16),
            window,
            surface,
            device,
            queue,
            config,
            renderer,
            resources,
        }
    }

    pub fn window(&self) -> &Window {
        &self.window
    }

    /// The surface size in physical pixels. The only size in this program.
    pub fn size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
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
        if (width, height) == (self.config.width, self.config.height) {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        self.vello.reset_and_resize(width as u16, height as u16);
    }

    /// Clear the scene and hand it over. Returning the `Scene` rather than
    /// exposing it as a field is what makes forgetting the reset impossible.
    pub fn begin(&mut self) -> &mut Scene {
        self.vello.reset();
        &mut self.vello
    }

    /// Render and present what [`Gpu::begin`] handed out.
    ///
    /// Notifies the compositor immediately before presenting, which is what
    /// keeps a Wayland frame callback from stalling the next redraw.
    pub fn present(&mut self) {
        use wgpu::CurrentSurfaceTexture as Acquired;
        let frame = match self.surface.get_current_texture() {
            Acquired::Success(f) | Acquired::Suboptimal(f) => f,
            // Outdated and Lost both want the swapchain rebuilt; the size has
            // not changed, so reconfigure with what we already have.
            Acquired::Outdated | Acquired::Lost => {
                self.surface.configure(&self.device, &self.config);
                return;
            }
            // Timeout, Occluded, Validation: skip the frame and try again.
            _ => return,
        };
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(self.config.format),
            ..Default::default()
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        if self
            .renderer
            .render(
                &self.vello,
                &mut self.resources,
                &self.device,
                &self.queue,
                &mut encoder,
                &RenderSize {
                    width: self.config.width,
                    height: self.config.height,
                },
                &view,
                &TextureBindings::new(),
            )
            .is_ok()
        {
            self.queue.submit([encoder.finish()]);
            self.window.pre_present_notify();
            frame.present();
        }
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
