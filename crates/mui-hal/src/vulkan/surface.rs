//! A window surface: `mui-present`'s `VkSwapchain`, created on the first
//! `configure` (it needs the device), with the semaphores that order
//! acquire, render and present.
//!
//! Frame flow: `acquire` pends the acquire semaphore as a wait of the next
//! submission and the image's `UNDEFINED` to `GENERAL` transition; the
//! caller renders and submits; `present` submits the `PRESENT_SRC`
//! transition signalling the image's render semaphore, then presents.

use std::sync::Mutex;

use ash::vk;
use mui_present::PresentError;
use mui_present::linux::vk_swapchain::VkSwapchain;
use raw_window_handle::{RawDisplayHandle, RawWindowHandle};

use super::{Cmd, Device, Error, Instance, Texture, TextureView, err, lock};
use crate::types::{CompositeAlphaMode, TextureFormat};

struct Swap {
    // Before `device`: the chain must go while the device it was made on lives.
    chain: VkSwapchain,
    device: Device,
    textures: Vec<Texture>,
    views: Vec<TextureView>,
    /// Acquire semaphores, one more than images, used round-robin.
    acquire: Vec<vk::Semaphore>,
    /// The present submission that last waited out each acquire semaphore.
    acquire_done: Vec<u64>,
    next: usize,
    /// Render-done semaphores, one per image.
    rendered: Vec<vk::Semaphore>,
    outdated: bool,
    /// Bumped on every rebuild; frames from an older chain are rejected.
    generation: u64,
    /// An acquired image not yet presented. Acquiring again while one is
    /// held (a frame dropped unpresented) forces a rebuild, which releases it.
    held: bool,
}

impl Swap {
    fn semaphores(device: &Device, n: usize) -> Result<Vec<vk::Semaphore>, Error> {
        let d = &device.0.raw;
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            // SAFETY: a plain object creation on a live device.
            match unsafe { d.create_semaphore(&vk::SemaphoreCreateInfo::default(), None) } {
                Ok(s) => out.push(s),
                Err(e) => {
                    for s in out {
                        // SAFETY: just created, never used.
                        unsafe { d.destroy_semaphore(s, None) };
                    }
                    return Err(err("vkCreateSemaphore", e));
                }
            }
        }
        Ok(out)
    }

    /// Brings the chain to rest before its images and semaphores go:
    /// flushes pending work, so an abandoned acquire's wait is consumed,
    /// then waits the device idle under the queue lock, so no other thread
    /// submits meanwhile. Dispatches no callbacks: the surface lock is held.
    fn quiesce(&mut self) -> Result<(), Error> {
        let flushed = self.device.submit_locked(Vec::new());
        let mut q = lock(&self.device.0.queue);
        if flushed.is_err() {
            // Unsubmittable: drop this chain's work instead.
            let (acquire, rendered) = (&self.acquire, &self.rendered);
            q.waits.retain(|s| !acquire.contains(s));
            q.signals.retain(|s| !rendered.contains(s));
            let images: Vec<_> = self.textures.iter().map(|t| t.0.raw).collect();
            q.pending.retain(
                |c| !matches!(c, Cmd::Acquire(t) | Cmd::Release(t) if images.contains(&t.0.raw)),
            );
        }
        self.held = false;
        // SAFETY: a plain wait with no timeout, the queue lock held; presents
        // may still hold the images and semaphores.
        unsafe { self.device.0.raw.device_wait_idle() }.map_err(|e| err("vkDeviceWaitIdle", e))
    }

    /// Rebuilds the per-image state after the chain was (re)configured.
    fn rebuild(&mut self) -> Result<(), Error> {
        // Unique across chains, so a replacement never matches an old frame.
        static GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        self.free_semaphores();
        let format = match self.chain.format() {
            mui_present::SurfaceFormat::Bgra8Unorm => TextureFormat::Bgra8Unorm,
            mui_present::SurfaceFormat::Rgba8Unorm => TextureFormat::Rgba8Unorm,
        };
        let extent = self.chain.extent();
        self.textures = self
            .chain
            .images()
            .iter()
            .map(|&image| Texture::external(&self.device, image, format, extent))
            .collect();
        self.views = self
            .textures
            .iter()
            .map(|t| t.create_view(None))
            .collect::<Result<_, _>>()?;
        let n = self.textures.len();
        self.acquire = Self::semaphores(&self.device, n + 1)?;
        self.acquire_done = vec![0; n + 1];
        self.rendered = Self::semaphores(&self.device, n)?;
        self.next = 0;
        self.outdated = false;
        self.generation = GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Ok(())
    }

    fn free_semaphores(&mut self) {
        for s in self.acquire.drain(..).chain(self.rendered.drain(..)) {
            // SAFETY: callers made the device idle first.
            unsafe { self.device.0.raw.destroy_semaphore(s, None) };
        }
    }
}

impl Drop for Swap {
    fn drop(&mut self) {
        let _ = self.quiesce();
        self.views.clear();
        self.textures.clear();
        self.free_semaphores();
    }
}

/// A window's Vulkan surface.
pub struct Surface {
    instance: Instance,
    display: RawDisplayHandle,
    window: RawWindowHandle,
    swap: Mutex<Option<Swap>>,
}

// SAFETY: the raw handles are only passed to the WSI under the `swap` lock;
// the window owner keeps them alive (see `Instance::create_surface`).
unsafe impl Send for Surface {}
// SAFETY: as above.
unsafe impl Sync for Surface {}

/// An acquired image, to render into then present.
pub struct SurfaceTexture {
    pub texture: Texture,
    pub view: TextureView,
    index: u32,
    slot: usize,
    generation: u64,
    pub suboptimal: bool,
}

/// What acquiring an image came to.
pub enum Acquired {
    Image(SurfaceTexture),
    Outdated,
    Timeout,
    Lost,
    Failed(Error),
}

impl Surface {
    /// # Safety
    /// `display` and `window` must stay live until this surface drops.
    pub unsafe fn new(
        instance: &Instance,
        display: RawDisplayHandle,
        window: RawWindowHandle,
    ) -> Result<Self, Error> {
        let needed = VkSwapchain::required_instance_extensions(display);
        if let Some(missing) = needed.iter().find(|n| !instance.0.extensions.contains(n)) {
            return Err(Error(format!("the instance lacks {missing:?}")));
        }
        Ok(Self {
            instance: instance.clone(),
            display,
            window,
            swap: Mutex::new(None),
        })
    }

    /// (Re)builds the swapchain on `device` at `size`. The device waits
    /// idle first: the old images may not be in flight.
    pub fn configure(
        &self,
        device: &Device,
        size: (u32, u32),
        translucent: bool,
    ) -> Result<(), Error> {
        let mut guard = lock(&self.swap);
        let present = |e: PresentError| Error(format!("swapchain: {e}"));
        match guard.as_mut() {
            Some(swap) if std::sync::Arc::ptr_eq(&swap.device.0, &device.0) => {
                let result = swap.quiesce().and_then(|()| {
                    swap.views.clear();
                    swap.textures.clear();
                    swap.chain.configure(size, translucent).map_err(present)?;
                    swap.rebuild()
                });
                // A half-rebuilt chain is unusable: drop it, so `acquire`
                // reports outdated until a configure succeeds.
                if result.is_err() {
                    *guard = None;
                }
                result
            }
            _ => {
                *guard = None;
                if !device.0.swapchain {
                    return Err(Error("the device lacks VK_KHR_swapchain".into()));
                }
                let adapter = device.adapter();
                // SAFETY: the instance enabled every surface extension it
                // has (checked in `new`), the device enabled
                // VK_KHR_swapchain, and `new`'s caller keeps the window live.
                let chain = unsafe {
                    VkSwapchain::new(
                        &self.instance.0.entry,
                        &self.instance.0.instance,
                        adapter.physical,
                        device.0.raw.clone(),
                        device.family(),
                        self.display,
                        self.window,
                        size,
                        translucent,
                    )
                }
                .map_err(present)?;
                let mut swap = Swap {
                    device: device.clone(),
                    chain,
                    textures: Vec::new(),
                    views: Vec::new(),
                    acquire: Vec::new(),
                    acquire_done: Vec::new(),
                    next: 0,
                    rendered: Vec::new(),
                    outdated: false,
                    generation: 0,
                    held: false,
                };
                swap.rebuild()?;
                *guard = Some(swap);
                Ok(())
            }
        }
    }

    /// Destroys the swapchain (and its `VkSurfaceKHR`); a frame still held
    /// then fails to present. The HAL calls this before the window owner drops.
    pub fn release(&self) {
        *lock(&self.swap) = None;
    }

    /// The composite-alpha mode in effect, once configured.
    pub fn alpha_mode(&self) -> Option<CompositeAlphaMode> {
        use vk::CompositeAlphaFlagsKHR as A;
        lock(&self.swap)
            .as_ref()
            .map(|s| match s.chain.composite_alpha() {
                A::PRE_MULTIPLIED => CompositeAlphaMode::PreMultiplied,
                A::POST_MULTIPLIED => CompositeAlphaMode::PostMultiplied,
                A::INHERIT => CompositeAlphaMode::Inherit,
                _ => CompositeAlphaMode::Opaque,
            })
    }

    /// The swapchain's format, once configured.
    pub fn format(&self) -> Option<TextureFormat> {
        lock(&self.swap)
            .as_ref()
            .and_then(|s| s.textures.first().map(Texture::format))
    }

    /// Wayland: the child subsurface's logical size, for fractional scales.
    pub fn set_logical_size(&self, width: u32, height: u32) {
        if let Some(s) = lock(&self.swap).as_ref() {
            s.chain.set_wayland_logical_size(width, height);
        }
    }

    pub fn acquire(&self) -> Acquired {
        let mut guard = lock(&self.swap);
        let Some(swap) = guard.as_mut() else {
            return Acquired::Outdated;
        };
        if swap.held {
            swap.outdated = true;
        }
        if swap.outdated {
            return Acquired::Outdated;
        }
        let slot = swap.next;
        // The semaphore's last wait must have run before it is signalled again.
        if swap.acquire_done[slot] > swap.device.completed()
            && let Err(e) = swap.device.wait_for(swap.acquire_done[slot])
        {
            return Acquired::Failed(e);
        }
        let semaphore = swap.acquire[slot];
        // SAFETY: `semaphore` is unsignalled with no pending wait (checked
        // above); at most one image is held, so a finite timeout suffices.
        match unsafe { swap.chain.acquire_next_image(semaphore, 2_000_000_000) } {
            Ok(image) => {
                swap.next = (slot + 1) % swap.acquire.len();
                let i = image.index as usize;
                let texture = swap.textures[i].clone();
                {
                    let mut q = lock(&swap.device.0.queue);
                    q.waits.push(semaphore);
                    q.pending.push(Cmd::Acquire(texture.clone()));
                }
                swap.held = true;
                Acquired::Image(SurfaceTexture {
                    view: swap.views[i].clone(),
                    texture,
                    index: image.index,
                    slot,
                    generation: swap.generation,
                    suboptimal: image.suboptimal,
                })
            }
            Err(PresentError::OutOfDate) => {
                swap.outdated = true;
                Acquired::Outdated
            }
            Err(PresentError::SurfaceLost) => Acquired::Lost,
            Err(PresentError::Timeout) => Acquired::Timeout,
            Err(e) => Acquired::Failed(Error(e.to_string())),
        }
    }

    /// Ends `frame`'s rendering and queues it for display. Returns whether
    /// the swapchain should be reconfigured.
    pub fn present(&self, frame: &SurfaceTexture) -> Result<bool, Error> {
        let mut guard = lock(&self.swap);
        let Some(swap) = guard.as_mut() else {
            return Err(Error("the surface was not configured".into()));
        };
        if frame.generation != swap.generation {
            return Err(Error("the frame is from a reconfigured swapchain".into()));
        }
        swap.held = false;
        let rendered = swap.rendered[frame.index as usize];
        {
            let mut q = lock(&swap.device.0.queue);
            q.pending.push(Cmd::Release(frame.texture.clone()));
            q.signals.push(rendered);
        }
        let value = swap.device.submit_locked(Vec::new()).inspect_err(|_| {
            // The image stays acquired: rebuild before reuse.
            swap.outdated = true;
        })?;
        swap.acquire_done[frame.slot] = value;
        let result = swap.device.with_queue(|queue| {
            // SAFETY: the queue is held; the image was acquired and moved to
            // PRESENT_SRC by the submission that signals `rendered`.
            unsafe { swap.chain.present(queue, frame.index, rendered) }
        });
        match result {
            Ok(suboptimal) => Ok(suboptimal || frame.suboptimal),
            Err(PresentError::OutOfDate) => {
                swap.outdated = true;
                Ok(true)
            }
            Err(e) => {
                // The image may still be acquired: rebuild before reuse.
                swap.outdated = true;
                Err(Error(e.to_string()))
            }
        }
    }
}
