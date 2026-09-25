//! The gallery's device-loss path. Everything built on a device
//! -- pipelines, atlases, weld textures, retained encodings -- dies with it,
//! so a host keeps all of that in `T` and [`OnDevice::recover`] rebuilds the
//! lot on a fresh device. The window and surface survive; `T` does not.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub struct OnDevice<T> {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub state: T,
    /// Raised by wgpu's device-lost callback, on whatever thread wgpu calls it.
    lost: Arc<AtomicBool>,
}

impl<T> OnDevice<T> {
    /// An adapter able to present to `surface` (any adapter without one), a
    /// device on it, and `build`'s state for that device.
    pub fn open(
        instance: &wgpu::Instance,
        surface: Option<&wgpu::Surface<'_>>,
        build: impl FnOnce(&wgpu::Adapter, &wgpu::Device, &wgpu::Queue) -> Result<T, String>,
    ) -> Result<Self, String> {
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: surface,
            ..Default::default()
        }))
        .map_err(|e| e.to_string())?;
        eprintln!("MUI GPU adapter: {:?}", adapter.get_info());
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
                .map_err(|e| e.to_string())?;
        let lost = Arc::new(AtomicBool::new(false));
        let flag = lost.clone();
        device.set_device_lost_callback(move |_, _| flag.store(true, Ordering::Release));
        let state = build(&adapter, &device, &queue)?;
        Ok(Self {
            device,
            queue,
            state,
            lost,
        })
    }

    /// `true` when the device was lost and everything on it was rebuilt: the
    /// caller skips this frame and asks for another. `build` is the one
    /// [`OnDevice::open`] took.
    pub fn recover(
        &mut self,
        instance: &wgpu::Instance,
        surface: Option<&wgpu::Surface<'_>>,
        build: impl FnOnce(&wgpu::Adapter, &wgpu::Device, &wgpu::Queue) -> Result<T, String>,
    ) -> Result<bool, String> {
        if !self.lost.load(Ordering::Acquire) {
            return Ok(false);
        }
        *self = Self::open(instance, surface, build)?;
        eprintln!("MUI GPU device lost; renderer rebuilt");
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mui::prelude::*;
    use mui::vello::effects::{Budget, GpuRenderer};
    use mui::vello::kurbo::Affine;

    /// A real loss, not a flag flipped by hand: `Device::destroy` fires the
    /// lost callback, and the renderer rebuilt on the new device renders.
    #[test]
    fn a_destroyed_device_is_replaced_and_renders_again() {
        let instance = wgpu::Instance::default();
        let build = |_: &wgpu::Adapter, d: &wgpu::Device, q: &wgpu::Queue| {
            let format = wgpu::TextureFormat::Rgba8Unorm;
            let budget = Budget::default();
            pollster::block_on(GpuRenderer::new(d, q, format, [16, 16], budget))
                .map_err(|e| e.to_string())
        };
        let mut gpu = match OnDevice::open(&instance, None, build) {
            Ok(gpu) => gpu,
            Err(e) => return eprintln!("SKIPPED: no wgpu device ({e})"),
        };
        assert!(!gpu.recover(&instance, None, build).unwrap());
        let old = gpu.device.clone();
        old.destroy();
        let _ = old.poll(wgpu::PollType::Poll);
        assert!(gpu.recover(&instance, None, build).unwrap(), "loss unseen");
        assert_ne!(gpu.device, old);
        let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: 16,
                height: 16,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let scope = gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let spec = SceneSpec::new(leaf(16., 16.).fill(Role::Primary)).offered(Size::new(16., 16.));
        let scene = resolve_scene(&spec).unwrap();
        let view = target.create_view(&Default::default());
        gpu.state.render(&scene, Affine::IDENTITY, &view).unwrap();
        assert!(pollster::block_on(scope.pop()).is_none());
        assert!(!gpu.recover(&instance, None, build).unwrap());
    }
}
