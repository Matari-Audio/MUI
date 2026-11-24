//! A window surface: `mui-present`'s `VkSwapchain`, created on the first
//! `configure` (it needs the device), with the semaphores that order
//! acquire, render and present.
//!
//! Frame flow: `acquire` pends the acquire semaphore as a wait of the next
//! submission and the image's `UNDEFINED` to `GENERAL` transition; the
//! caller renders and submits; `present` submits the `PRESENT_SRC`
//! transition signalling the image's render semaphore, then presents.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use ash::vk;
use mui_present::PresentError;
use mui_present::linux::vk_swapchain::VkSwapchain;
use raw_window_handle::{RawDisplayHandle, RawWindowHandle};

use super::{Cmd, Device, Error, Instance, Texture, TextureView, lock, vk_err};
use crate::types::{CompositeAlphaMode, TextureFormat};

/// A render-complete semaphore owned by a swapchain image slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RenderSemaphore {
    raw: vk::Semaphore,
    /// `true` once submitted to an unfenced `vkQueuePresentKHR`: `vkDeviceWaitIdle`
    /// does not prove the presentation engine finished waiting on it, so it must
    /// be retained until device teardown rather than destroyed on reconfigure.
    presented: bool,
}

struct Swap {
    // Before `device`: the chain must go while the device it was made on lives.
    chain: VkSwapchain,
    device: Device,
    textures: Vec<Texture>,
    views: Vec<TextureView>,
    /// Shared liveness flag embedded in every swapchain `Texture` of the
    /// current generation; flipped to `false` under the device queue lock
    /// during `quiesce` so concurrent or subsequent submissions targeting old
    /// swapchain images are rejected before touching Vulkan.
    live: Arc<AtomicBool>,
    /// Acquire semaphores, one more than images, used round-robin.
    acquire: Vec<vk::Semaphore>,
    /// The present submission that last waited out each acquire semaphore.
    acquire_done: Vec<u64>,
    next: usize,
    /// Render-done semaphores, one per image.
    rendered: Vec<RenderSemaphore>,
    outdated: bool,
    /// Globally unique per rebuilt swapchain; frames from an older or replaced
    /// chain are rejected.
    generation: u64,
    /// Globally unique token of the single currently acquired, unpresented
    /// frame. Acquiring again while one is outstanding forces a rebuild;
    /// presenting requires the exact matching token.
    outstanding: Option<u64>,
}

/// Decides which semaphores from a quiescing swapchain cannot be safely
/// destroyed yet and moves them into `retained`.
///
/// - When `idle_ok` is `false`, `vkDeviceWaitIdle` failed so GPU completion is
///   unknown for every semaphore on the swapchain: retain all of them.
/// - When `idle_ok` is `true`:
///   - If `waits_submitted` is `false`, retain `unwaited` acquire semaphores
///     whose WSI acquire signal was never consumed by a queue submit.
///   - Retain every `rendered` semaphore with `presented == true` because
///     `vkDeviceWaitIdle` does not prove an unfenced `vkQueuePresentKHR` has
///     finished waiting on it.
fn retain_unverified_semaphores(
    acquire: &mut Vec<vk::Semaphore>,
    rendered: &mut Vec<RenderSemaphore>,
    unwaited: &[vk::Semaphore],
    waits_submitted: bool,
    idle_ok: bool,
    retained: &mut Vec<vk::Semaphore>,
) {
    if idle_ok {
        if !waits_submitted && !unwaited.is_empty() {
            acquire.retain(|s| !unwaited.contains(s));
            retained.extend_from_slice(unwaited);
        }
        rendered.retain(|r| {
            if r.presented {
                retained.push(r.raw);
                false
            } else {
                true
            }
        });
    } else {
        retained.append(acquire);
        retained.extend(rendered.drain(..).map(|r| r.raw));
    }
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
                    return Err(vk_err("vkCreateSemaphore", e));
                }
            }
        }
        Ok(out)
    }

    /// Brings the chain to rest before its images and semaphores go:
    /// flushes pending work so an abandoned acquire's wait is consumed,
    /// marks the swapchain's images retired under the queue lock, then waits
    /// the device idle under the queue lock so no other thread submits
    /// meanwhile. Unverified acquire semaphores (on failed flush/wait submit),
    /// any presented render-complete semaphores, and all semaphores on failed
    /// `device_wait_idle` are retained on the device instead of destroyed
    /// prematurely. Dispatches no callbacks: the surface lock may be held.
    fn quiesce(&mut self) -> Result<(), Error> {
        let flushed = self.device.submit_locked(Vec::new());
        let mut q = lock(&self.device.0.queue);
        // Retire this swapchain's images under the queue lock: every
        // `submit_locked` checks `live` under this same lock before recording,
        // and every submission already made finishes in `device_wait_idle`.
        self.live.store(false, Ordering::Release);
        self.outdated = true;
        self.outstanding = None;
        let mut unwaited = Vec::new();
        q.waits.retain(|s| {
            if self.acquire.contains(s) {
                unwaited.push(*s);
                false
            } else {
                true
            }
        });
        let rendered = &self.rendered;
        q.signals.retain(|s| !rendered.iter().any(|r| r.raw == *s));
        let images: Vec<_> = self.textures.iter().map(|t| t.0.raw).collect();
        q.pending.retain(
            |c| !matches!(c, Cmd::Acquire(t) | Cmd::Release(t) if images.contains(&t.0.raw)),
        );
        // If the normal flush failed (e.g. an invalid unrelated command in
        // `q.pending` or software `Device::destroy`) and left acquire waits
        // unconsumed, try a bare wait-only submit on the queue to establish
        // acquire completion before waiting idle.
        let waits_submitted = if unwaited.is_empty() {
            true
        } else if flushed.is_err() {
            self.device.submit_waits_locked(&q, &unwaited).is_ok()
        } else {
            true
        };
        // SAFETY: a plain wait with no timeout, the queue lock held; presents
        // may still hold the images and semaphores.
        let idle = unsafe { self.device.0.raw.device_wait_idle() }.map_err(|e| {
            if e == vk::Result::ERROR_DEVICE_LOST {
                self.device
                    .record_hardware_loss("vkDeviceWaitIdle: device lost");
            }
            vk_err("vkDeviceWaitIdle", e)
        });
        retain_unverified_semaphores(
            &mut self.acquire,
            &mut self.rendered,
            &unwaited,
            waits_submitted,
            idle.is_ok(),
            &mut q.retained_semaphores,
        );
        idle
    }

    /// Rebuilds the per-image state after the chain was (re)configured.
    fn rebuild(&mut self) -> Result<(), Error> {
        // Unique across chains, so a replacement never matches an old frame.
        static GENERATION: AtomicU64 = AtomicU64::new(1);
        self.free_semaphores();
        let format = match self.chain.format() {
            mui_present::SurfaceFormat::Bgra8Unorm => TextureFormat::Bgra8Unorm,
            mui_present::SurfaceFormat::Rgba8Unorm => TextureFormat::Rgba8Unorm,
        };
        let extent = self.chain.extent();
        let live = Arc::new(AtomicBool::new(true));
        self.live = live.clone();
        self.textures = self
            .chain
            .images()
            .iter()
            .map(|&image| Texture::external(&self.device, image, format, extent, live.clone()))
            .collect();
        self.views = self
            .textures
            .iter()
            .map(|t| t.create_view(None))
            .collect::<Result<_, _>>()?;
        let n = self.textures.len();
        self.acquire = Self::semaphores(&self.device, n + 1)?;
        self.acquire_done = vec![0; n + 1];
        self.rendered = Self::semaphores(&self.device, n)?
            .into_iter()
            .map(|raw| RenderSemaphore {
                raw,
                presented: false,
            })
            .collect();
        self.next = 0;
        self.outdated = false;
        self.outstanding = None;
        self.generation = GENERATION.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    fn free_semaphores(&mut self) {
        for s in self
            .acquire
            .drain(..)
            .chain(self.rendered.drain(..).map(|r| r.raw))
        {
            // SAFETY: callers established GPU/acquire completion first; any
            // semaphore whose completion could not be established (including
            // presented render-complete semaphores) was already moved out to
            // `DeviceShared::queue.retained_semaphores`.
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
    token: u64,
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
            return Err(Error::internal(format!("the instance lacks {missing:?}")));
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
        let mut notify_devices = Vec::new();
        let result = (|| {
            let mut guard = lock(&self.swap);
            let present = |e: PresentError| Error::validation(format!("swapchain: {e}"));
            match guard.as_mut() {
                Some(swap) if Arc::ptr_eq(&swap.device.0, &device.0) => {
                    notify_devices.push(device.clone());
                    swap.quiesce()?;
                    swap.views.clear();
                    swap.textures.clear();
                    let result = swap
                        .chain
                        .configure(size, translucent)
                        .map_err(present)
                        .and_then(|()| swap.rebuild());
                    // A half-rebuilt chain is unusable: drop it, so `acquire`
                    // reports outdated until a configure succeeds.
                    if result.is_err() {
                        *guard = None;
                    }
                    result
                }
                _ => {
                    if let Some(old) = guard.as_mut() {
                        notify_devices.push(old.device.clone());
                        if let Err(e) = old.quiesce()
                            && !old.device.0.hardware_lost.load(Ordering::Acquire)
                        {
                            return Err(e);
                        }
                        *guard = None;
                    }
                    if device.0.swapchain {
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
                        .map_err(present);
                        match chain {
                            Ok(chain) => {
                                let mut swap = Swap {
                                    device: device.clone(),
                                    chain,
                                    textures: Vec::new(),
                                    views: Vec::new(),
                                    live: Arc::new(AtomicBool::new(false)),
                                    acquire: Vec::new(),
                                    acquire_done: Vec::new(),
                                    next: 0,
                                    rendered: Vec::new(),
                                    outdated: false,
                                    generation: 0,
                                    outstanding: None,
                                };
                                let built = swap.rebuild();
                                if built.is_ok() {
                                    *guard = Some(swap);
                                } else {
                                    notify_devices.push(device.clone());
                                }
                                built
                            }
                            Err(e) => Err(e),
                        }
                    } else {
                        Err(Error::validation("the device lacks VK_KHR_swapchain"))
                    }
                }
            }
        })();
        for dev in notify_devices {
            dev.notify_lost();
        }
        result
    }

    /// Destroys the swapchain (and its `VkSurfaceKHR`); a frame still held
    /// then fails to present. The HAL calls this before the window owner drops.
    pub fn release(&self) {
        let device = {
            let mut guard = lock(&self.swap);
            guard.take().map(|old| {
                let device = old.device.clone();
                drop(old);
                device
            })
        };
        if let Some(device) = device {
            device.notify_lost();
        }
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
        let (device, outcome) = {
            let mut guard = lock(&self.swap);
            let Some(swap) = guard.as_mut() else {
                return Acquired::Outdated;
            };
            (swap.device.clone(), Self::acquire_locked(swap))
        };
        device.notify_lost();
        outcome
    }

    fn acquire_locked(swap: &mut Swap) -> Acquired {
        static ACQUISITION: AtomicU64 = AtomicU64::new(1);
        if swap.outstanding.is_some() {
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
                let token = ACQUISITION.fetch_add(1, Ordering::Relaxed);
                swap.outstanding = Some(token);
                Acquired::Image(SurfaceTexture {
                    view: swap.views[i].clone(),
                    texture,
                    index: image.index,
                    slot,
                    generation: swap.generation,
                    token,
                    suboptimal: image.suboptimal,
                })
            }
            Err(PresentError::OutOfDate) => {
                swap.outdated = true;
                Acquired::Outdated
            }
            Err(PresentError::SurfaceLost) => Acquired::Lost,
            Err(PresentError::DeviceLost) => {
                swap.outdated = true;
                swap.device
                    .record_hardware_loss("vkAcquireNextImageKHR: device lost");
                Acquired::Failed(Error::internal(PresentError::DeviceLost.to_string()))
            }
            Err(PresentError::Timeout) => Acquired::Timeout,
            Err(e) => Acquired::Failed(Error::internal(e.to_string())),
        }
    }

    /// Consumes `frame`, ends its rendering, and queues it for display.
    /// Returns whether the swapchain should be reconfigured.
    #[expect(
        clippy::needless_pass_by_value,
        reason = "consumes the acquired frame so double-present is a compile error"
    )]
    pub fn present(&self, frame: SurfaceTexture) -> Result<bool, Error> {
        let (notify_device, outcome) = {
            let mut guard = lock(&self.swap);
            let Some(swap) = guard.as_mut() else {
                return Err(Error::validation("the surface was not configured"));
            };
            if frame.generation != swap.generation {
                return Err(Error::validation(
                    "the frame is from a reconfigured swapchain",
                ));
            }
            if swap.outstanding != Some(frame.token) {
                return Err(Error::validation(
                    "the frame is no longer the active swapchain acquisition",
                ));
            }
            swap.outstanding = None;
            let notify_device = swap.device.clone();
            let idx = frame.index as usize;
            let rendered = swap.rendered[idx].raw;
            let release_texture = swap.textures[idx].clone();
            {
                let mut q = lock(&swap.device.0.queue);
                q.pending.push(Cmd::Release(release_texture));
                q.signals.push(rendered);
            }
            let outcome = match swap.device.submit_locked(Vec::new()) {
                Err(e) => {
                    // The image stays acquired: rebuild before reuse.
                    swap.outdated = true;
                    Err(e)
                }
                Ok(value) => {
                    swap.acquire_done[frame.slot] = value;
                    // Mark `rendered` as presented before `vkQueuePresentKHR`
                    // so `quiesce` retains it until device teardown (since
                    // `vkDeviceWaitIdle` does not prove unfenced present completion).
                    swap.rendered[idx].presented = true;
                    let result = swap.device.with_queue(|queue| {
                        // SAFETY: the queue is held; the image was acquired and moved to
                        // PRESENT_SRC by the submission that signals `rendered`.
                        unsafe { swap.chain.present(queue, frame.index, rendered) }
                    });
                    if matches!(result, Err(PresentError::DeviceLost)) {
                        swap.device
                            .record_hardware_loss("vkQueuePresentKHR: device lost");
                    }
                    match result {
                        Ok(suboptimal) => Ok(suboptimal || frame.suboptimal),
                        Err(PresentError::OutOfDate) => {
                            swap.outdated = true;
                            Ok(true)
                        }
                        Err(e) => {
                            // The image may still be acquired: rebuild before reuse.
                            swap.outdated = true;
                            Err(Error::internal(e.to_string()))
                        }
                    }
                }
            };
            (notify_device, outcome)
        };
        notify_device.notify_lost();
        outcome
    }
}

#[cfg(test)]
mod tests {
    use ash::vk::Handle;

    use super::*;

    fn sem(n: u64) -> vk::Semaphore {
        vk::Semaphore::from_raw(n)
    }

    fn rsem(n: u64, presented: bool) -> RenderSemaphore {
        RenderSemaphore {
            raw: sem(n),
            presented,
        }
    }

    #[test]
    fn retain_keeps_none_when_waits_submitted_and_idle_ok() {
        let mut acquire = vec![sem(1), sem(2), sem(3)];
        let mut rendered = vec![rsem(10, false), rsem(11, false)];
        let mut retained = Vec::new();
        retain_unverified_semaphores(&mut acquire, &mut rendered, &[], true, true, &mut retained);
        assert!(retained.is_empty());
        assert_eq!(acquire, vec![sem(1), sem(2), sem(3)]);
        assert_eq!(rendered, vec![rsem(10, false), rsem(11, false)]);
    }

    #[test]
    fn retain_holds_unwaited_acquire_when_flush_submit_fails() {
        let mut acquire = vec![sem(1), sem(2), sem(3)];
        let mut rendered = vec![rsem(10, false), rsem(11, false)];
        let mut retained = Vec::new();
        retain_unverified_semaphores(
            &mut acquire,
            &mut rendered,
            &[sem(2)],
            false,
            true,
            &mut retained,
        );
        assert_eq!(retained, vec![sem(2)]);
        assert_eq!(acquire, vec![sem(1), sem(3)]);
        assert_eq!(rendered, vec![rsem(10, false), rsem(11, false)]);
    }

    #[test]
    fn retain_holds_presented_rendered_semaphores_even_when_idle_ok() {
        let mut acquire = vec![sem(1), sem(2)];
        let mut rendered = vec![rsem(10, true), rsem(11, false)];
        let mut retained = Vec::new();
        retain_unverified_semaphores(&mut acquire, &mut rendered, &[], true, true, &mut retained);
        assert_eq!(retained, vec![sem(10)]);
        assert_eq!(acquire, vec![sem(1), sem(2)]);
        assert_eq!(rendered, vec![rsem(11, false)]);
    }

    #[test]
    fn retain_holds_all_semaphores_when_device_wait_idle_fails() {
        let mut acquire = vec![sem(1), sem(2)];
        let mut rendered = vec![rsem(10, false)];
        let mut retained = Vec::new();
        retain_unverified_semaphores(
            &mut acquire,
            &mut rendered,
            &[sem(1)],
            true,
            false,
            &mut retained,
        );
        assert_eq!(retained, vec![sem(1), sem(2), sem(10)]);
        assert!(acquire.is_empty());
        assert!(rendered.is_empty());
    }
}
