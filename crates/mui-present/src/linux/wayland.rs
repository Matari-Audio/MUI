//! Direct Wayland `wl_shm` presentation and isolated child `wl_subsurface` helper.
//!
//! **Protocol invariant:** Never hot-swap `wl_shm` and Vulkan on the same
//! `wl_surface`. Both [`WaylandShmPresenter`] and [`super::vk_swapchain::VkSwapchain`]
//! allocate their own fresh child `wl_surface` + `wl_subsurface` attached to the
//! host window's parent `wl_surface` (with an empty `wl_region` input mask so all
//! pointer/touch input passes through to the parent window). When switching
//! between `wl_shm` and Vulkan, the old child subsurface is destroyed completely
//! and a brand-new child subsurface is created for the new backend.

use std::ffi::c_void;
use std::os::fd::{AsFd, FromRawFd, OwnedFd};
use std::ptr;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use wayland_backend::sys::client::Backend;
use wayland_client::backend::ObjectId;
use wayland_client::protocol::{
    wl_buffer, wl_compositor, wl_registry, wl_shm, wl_shm_pool, wl_subcompositor, wl_subsurface,
    wl_surface,
};
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle};
use wayland_protocols::wp::viewporter::client::{wp_viewport, wp_viewporter};

use crate::{DamageRect, PresentError, blit_scaled_0rgb};

/// Globals discovered on our private Wayland event queue.
#[derive(Default)]
struct WlGlobals {
    compositor: Option<wl_compositor::WlCompositor>,
    subcompositor: Option<wl_subcompositor::WlSubcompositor>,
    shm: Option<wl_shm::WlShm>,
    viewporter: Option<wp_viewporter::WpViewporter>,
}

impl Dispatch<wl_registry::WlRegistry, ()> for WlGlobals {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        {
            match interface.as_str() {
                "wl_compositor" if state.compositor.is_none() => {
                    state.compositor = Some(registry.bind(name, version.min(6), qh, ()));
                }
                "wl_subcompositor" if state.subcompositor.is_none() => {
                    state.subcompositor = Some(registry.bind(name, 1, qh, ()));
                }
                "wl_shm" if state.shm.is_none() => {
                    state.shm = Some(registry.bind(name, 1, qh, ()));
                }
                "wp_viewporter" if state.viewporter.is_none() => {
                    state.viewporter = Some(registry.bind(name, 1, qh, ()));
                }
                _ => {}
            }
        }
    }
}

impl Dispatch<wl_compositor::WlCompositor, ()> for WlGlobals {
    fn event(
        _: &mut Self,
        _: &wl_compositor::WlCompositor,
        _: wl_compositor::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_subcompositor::WlSubcompositor, ()> for WlGlobals {
    fn event(
        _: &mut Self,
        _: &wl_subcompositor::WlSubcompositor,
        _: wl_subcompositor::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_surface::WlSurface, ()> for WlGlobals {
    fn event(
        _: &mut Self,
        _: &wl_surface::WlSurface,
        _: wl_surface::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_subsurface::WlSubsurface, ()> for WlGlobals {
    fn event(
        _: &mut Self,
        _: &wl_subsurface::WlSubsurface,
        _: wl_subsurface::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_shm::WlShm, ()> for WlGlobals {
    fn event(
        _: &mut Self,
        _: &wl_shm::WlShm,
        _: wl_shm::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_shm_pool::WlShmPool, ()> for WlGlobals {
    fn event(
        _: &mut Self,
        _: &wl_shm_pool::WlShmPool,
        _: wl_shm_pool::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_buffer::WlBuffer, Arc<AtomicBool>> for WlGlobals {
    fn event(
        _: &mut Self,
        _: &wl_buffer::WlBuffer,
        event: wl_buffer::Event,
        busy: &Arc<AtomicBool>,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if matches!(event, wl_buffer::Event::Release) {
            busy.store(false, Ordering::Release);
        }
    }
}

impl Dispatch<wayland_client::protocol::wl_region::WlRegion, ()> for WlGlobals {
    fn event(
        _: &mut Self,
        _: &wayland_client::protocol::wl_region::WlRegion,
        _: wayland_client::protocol::wl_region::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wp_viewporter::WpViewporter, ()> for WlGlobals {
    fn event(
        _: &mut Self,
        _: &wp_viewporter::WpViewporter,
        _: wp_viewporter::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wp_viewport::WpViewport, ()> for WlGlobals {
    fn event(
        _: &mut Self,
        _: &wp_viewport::WpViewport,
        _: wp_viewport::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

/// Allocates an anonymous shared-memory file descriptor via `memfd_create`
/// (with `/dev/shm` unlinked fallback for older kernels/sandboxes).
#[expect(
    unsafe_code,
    reason = "Linux memfd_create / shm_open + ftruncate for wl_shm"
)]
fn create_shm_fd(byte_len: usize) -> Result<OwnedFd, PresentError> {
    let name = c"mui-present-shm";
    // SAFETY: valid static NUL-terminated name and standard MFD_CLOEXEC flag.
    let mut fd = unsafe { libc::memfd_create(name.as_ptr(), libc::MFD_CLOEXEC) };
    if fd < 0 {
        let fallback = format!(
            "/mui-present-{}-{}\0",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        );
        // SAFETY: fallback is NUL-terminated; immediately unlinked so no file lingers.
        fd = unsafe {
            let f = libc::shm_open(
                fallback.as_ptr().cast::<libc::c_char>(),
                libc::O_RDWR | libc::O_CREAT | libc::O_EXCL | libc::O_CLOEXEC,
                0o600,
            );
            if f >= 0 {
                libc::shm_unlink(fallback.as_ptr().cast::<libc::c_char>());
            }
            f
        };
    }
    if fd < 0 {
        return Err(PresentError::Platform(format!(
            "failed to create wl_shm fd: {}",
            std::io::Error::last_os_error()
        )));
    }
    // SAFETY: `fd` is a newly created owned file descriptor.
    let owned = unsafe { OwnedFd::from_raw_fd(fd) };
    // SAFETY: truncating our own freshly opened shm fd to `byte_len`.
    let rc = unsafe { libc::ftruncate(fd, byte_len as libc::off_t) };
    if rc != 0 {
        return Err(PresentError::Platform(format!(
            "ftruncate({byte_len}) failed: {}",
            std::io::Error::last_os_error()
        )));
    }
    Ok(owned)
}

struct MappedShm {
    _fd: OwnedFd,
    ptr: *mut u32,
    byte_len: usize,
    pool: wl_shm_pool::WlShmPool,
    buffer: wl_buffer::WlBuffer,
    busy: Arc<AtomicBool>,
    size: (u32, u32),
}

// SAFETY: the mapping is exclusively owned; only `&mut self` writes through it.
#[expect(unsafe_code, reason = "a raw mmap pointer owned by its buffer")]
unsafe impl Send for MappedShm {}
// SAFETY: `&MappedShm` exposes no access to the mapped pixels.
#[expect(unsafe_code, reason = "a raw mmap pointer owned by its buffer")]
unsafe impl Sync for MappedShm {}

impl MappedShm {
    #[expect(unsafe_code, reason = "mmap anonymous wl_shm backing memory")]
    fn new(
        shm: &wl_shm::WlShm,
        qh: &QueueHandle<WlGlobals>,
        size: (u32, u32),
        format: wl_shm::Format,
    ) -> Result<Self, PresentError> {
        let len_u32 = (size.0 as usize)
            .checked_mul(size.1 as usize)
            .ok_or(PresentError::InvalidSize(size))?;
        let byte_len = len_u32
            .checked_mul(4)
            .ok_or(PresentError::InvalidSize(size))?;
        let stride =
            i32::try_from(size.0.saturating_mul(4)).map_err(|_| PresentError::InvalidSize(size))?;
        let width = i32::try_from(size.0).map_err(|_| PresentError::InvalidSize(size))?;
        let height = i32::try_from(size.1).map_err(|_| PresentError::InvalidSize(size))?;
        let pool_size = i32::try_from(byte_len).map_err(|_| PresentError::InvalidSize(size))?;

        let fd = create_shm_fd(byte_len)?;
        // SAFETY: mapping `byte_len` bytes of our freshly sized anonymous shm fd.
        let addr = unsafe {
            libc::mmap(
                ptr::null_mut(),
                byte_len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                std::os::fd::AsRawFd::as_raw_fd(&fd),
                0,
            )
        };
        if addr == libc::MAP_FAILED || addr.is_null() {
            return Err(PresentError::Platform(format!(
                "mmap({byte_len}) failed: {}",
                std::io::Error::last_os_error()
            )));
        }

        let pool = shm.create_pool(fd.as_fd(), pool_size, qh, ());
        let busy = Arc::new(AtomicBool::new(false));
        let buffer = pool.create_buffer(0, width, height, stride, format, qh, Arc::clone(&busy));

        Ok(Self {
            _fd: fd,
            ptr: addr.cast::<u32>(),
            byte_len,
            pool,
            buffer,
            busy,
            size,
        })
    }

    #[expect(
        unsafe_code,
        reason = "exclusive mutable view of our mapped wl_shm slot"
    )]
    fn as_mut_slice(&mut self) -> &mut [u32] {
        // SAFETY: `self.ptr` points to `self.byte_len / 4` mapped u32 words.
        unsafe { std::slice::from_raw_parts_mut(self.ptr, self.byte_len / 4) }
    }
}

impl Drop for MappedShm {
    #[expect(unsafe_code, reason = "munmap wl_shm backing memory on drop")]
    fn drop(&mut self) {
        self.buffer.destroy();
        self.pool.destroy();
        // SAFETY: `self.ptr` and `self.byte_len` match the `mmap` call in `new`.
        unsafe {
            libc::munmap(self.ptr.cast::<libc::c_void>(), self.byte_len);
        }
    }
}

/// Dedicated child `wl_surface` + `wl_subsurface` for isolating Vulkan WSI
/// from `wl_shm` on the host window's parent `wl_surface`.
pub struct WaylandSubsurface {
    conn: Connection,
    _queue: EventQueue<WlGlobals>,
    globals: WlGlobals,
    parent: wl_surface::WlSurface,
    surface: wl_surface::WlSurface,
    subsurface: wl_subsurface::WlSubsurface,
    viewport: Option<wp_viewport::WpViewport>,
    anchor: Option<MappedShm>,
}

impl WaylandSubsurface {
    /// Creates a fresh child `wl_surface` + `wl_subsurface` attached to `parent_surface`.
    ///
    /// # Safety
    /// `display_ptr` must be a live `wl_display*` and `parent_surface_ptr` a live
    /// `wl_surface*` on that display that outlives the returned [`WaylandSubsurface`].
    ///
    /// # Errors
    /// Returns [`PresentError`] if required Wayland globals (`wl_compositor`,
    /// `wl_subcompositor`) are missing or proxy creation fails.
    #[expect(
        unsafe_code,
        reason = "borrows foreign wl_display and parent wl_surface pointers"
    )]
    pub unsafe fn new(
        display_ptr: *mut c_void,
        parent_surface_ptr: *mut c_void,
    ) -> Result<Self, PresentError> {
        if display_ptr.is_null() || parent_surface_ptr.is_null() {
            return Err(PresentError::UnsupportedHandle(
                "null Wayland display or surface pointer",
            ));
        }
        // SAFETY: caller guarantees live wl_display* outliving this subsurface.
        let backend = unsafe { Backend::from_foreign_display(display_ptr.cast()) };
        let conn = Connection::from_backend(backend);
        let mut queue = conn.new_event_queue::<WlGlobals>();
        let qh = queue.handle();
        let mut globals = WlGlobals::default();
        let display = conn.display();
        let registry = display.get_registry(&qh, ());
        queue
            .roundtrip(&mut globals)
            .map_err(|e| PresentError::Platform(format!("wayland roundtrip failed: {e}")))?;
        drop(registry);

        let compositor = globals
            .compositor
            .as_ref()
            .ok_or_else(|| PresentError::Platform("compositor lacks wl_compositor".into()))?;
        let subcompositor = globals
            .subcompositor
            .as_ref()
            .ok_or_else(|| PresentError::Platform("compositor lacks wl_subcompositor".into()))?;

        // SAFETY: caller guarantees live wl_surface* proxy on `conn`.
        let parent_id = unsafe {
            ObjectId::from_ptr(
                wl_surface::WlSurface::interface(),
                parent_surface_ptr.cast(),
            )
        }
        .map_err(|e| PresentError::Platform(format!("invalid parent wl_surface: {e}")))?;
        let parent = wl_surface::WlSurface::from_id(&conn, parent_id)
            .map_err(|e| PresentError::Platform(format!("parent wl_surface::from_id: {e}")))?;

        let surface = compositor.create_surface(&qh, ());
        let subsurface = subcompositor.get_subsurface(&surface, &parent, &qh, ());
        subsurface.set_position(0, 0);
        subsurface.set_desync();

        // Empty input region so pointer/touch events pass through to the parent window.
        let empty = compositor.create_region(&qh, ());
        surface.set_input_region(Some(&empty));
        empty.destroy();

        let viewport = globals
            .viewporter
            .as_ref()
            .map(|vp| vp.get_viewport(&surface, &qh, ()));

        let anchor = if let Some(shm) = globals.shm.as_ref() {
            MappedShm::new(shm, &qh, (1, 1), wl_shm::Format::Argb8888).ok()
        } else {
            None
        };
        if let Some(anchor) = &anchor {
            // Map an unmapped xdg_toplevel parent with a 1x1 transparent shm anchor
            // (parent never touches Vulkan; Vulkan renders strictly into `surface`).
            parent.attach(Some(&anchor.buffer), 0, 0);
            parent.commit();
        }
        let _ = conn.flush();

        Ok(Self {
            conn,
            _queue: queue,
            globals,
            parent,
            surface,
            subsurface,
            viewport,
            anchor,
        })
    }

    /// Raw `wl_display*` pointer for Vulkan `VkWaylandSurfaceCreateInfoKHR`.
    #[must_use]
    pub fn display_ptr(&self) -> *mut c_void {
        self.conn.backend().display_ptr().cast()
    }

    /// Raw child `wl_surface*` pointer (never shared with `wl_shm`).
    #[must_use]
    pub fn surface_ptr(&self) -> *mut c_void {
        self.surface.id().as_ptr().cast()
    }

    /// Updates the child subsurface logical destination via `wp_viewport` when available.
    pub fn set_logical_size(&self, logical_width: u32, logical_height: u32) {
        if let Some(vp) = &self.viewport
            && logical_width > 0
            && logical_height > 0
        {
            vp.set_destination(logical_width as i32, logical_height as i32);
            self.surface.commit();
            let _ = self.conn.flush();
        }
    }
}

impl Drop for WaylandSubsurface {
    fn drop(&mut self) {
        if let Some(vp) = self.viewport.take() {
            vp.destroy();
        }
        self.surface.attach(None, 0, 0);
        self.surface.commit();
        self.subsurface.destroy();
        self.surface.destroy();
        self.anchor.take();
        if let Some(vp) = self.globals.viewporter.take() {
            vp.destroy();
        }
        self.globals.shm.take();
        if let Some(sub) = self.globals.subcompositor.take() {
            sub.destroy();
        }
        let _ = &self.parent;
        let _ = self.conn.flush();
    }
}

/// Direct Wayland `wl_shm` CPU presenter backed by a double-buffered anonymous
/// shared-memory ring on a dedicated child `wl_subsurface`.
pub struct WaylandShmPresenter {
    conn: Connection,
    queue: EventQueue<WlGlobals>,
    qh: QueueHandle<WlGlobals>,
    globals: WlGlobals,
    parent: wl_surface::WlSurface,
    surface: wl_surface::WlSurface,
    subsurface: wl_subsurface::WlSubsurface,
    viewport: Option<wp_viewport::WpViewport>,
    /// 1x1 `wl_shm` anchor attached to `parent` so `xdg_toplevel` maps immediately
    /// while all real pixel buffers live strictly on the child `wl_subsurface`.
    parent_anchor: Option<MappedShm>,
    parent_mapped: bool,
    /// Double-buffered SHM ring for tear-free presentation without blocking.
    ring: [Option<MappedShm>; 2],
    next_slot: usize,
    retained: Vec<u32>,
    retained_size: (u32, u32),
    scale_factor: f64,
    surface_version: u32,
}

impl WaylandShmPresenter {
    /// Creates a direct Wayland `wl_shm` presenter attached as a child subsurface
    /// of `parent_surface_ptr`.
    ///
    /// # Safety
    /// `display_ptr` must be a live `wl_display*` and `parent_surface_ptr` a live
    /// `wl_surface*` on that display that outlives this presenter.
    ///
    /// # Errors
    /// Returns [`PresentError`] when Wayland globals are missing or creation fails.
    #[expect(
        unsafe_code,
        reason = "borrows foreign wl_display and parent wl_surface pointers"
    )]
    pub unsafe fn new(
        display_ptr: *mut c_void,
        parent_surface_ptr: *mut c_void,
    ) -> Result<Self, PresentError> {
        if display_ptr.is_null() || parent_surface_ptr.is_null() {
            return Err(PresentError::UnsupportedHandle(
                "null Wayland display or surface pointer",
            ));
        }
        // SAFETY: caller upholds display lifetime.
        let backend = unsafe { Backend::from_foreign_display(display_ptr.cast()) };
        let conn = Connection::from_backend(backend);
        let mut queue = conn.new_event_queue::<WlGlobals>();
        let qh = queue.handle();
        let mut globals = WlGlobals::default();
        let registry = conn.display().get_registry(&qh, ());
        queue
            .roundtrip(&mut globals)
            .map_err(|e| PresentError::Platform(format!("wayland roundtrip failed: {e}")))?;
        drop(registry);

        let compositor = globals
            .compositor
            .as_ref()
            .ok_or_else(|| PresentError::Platform("compositor lacks wl_compositor".into()))?;
        let subcompositor = globals
            .subcompositor
            .as_ref()
            .ok_or_else(|| PresentError::Platform("compositor lacks wl_subcompositor".into()))?;
        let shm = globals
            .shm
            .as_ref()
            .ok_or_else(|| PresentError::Platform("compositor lacks wl_shm".into()))?;

        // SAFETY: caller upholds parent wl_surface lifetime on this connection.
        let parent_id = unsafe {
            ObjectId::from_ptr(
                wl_surface::WlSurface::interface(),
                parent_surface_ptr.cast(),
            )
        }
        .map_err(|e| PresentError::Platform(format!("invalid parent wl_surface: {e}")))?;
        let parent = wl_surface::WlSurface::from_id(&conn, parent_id)
            .map_err(|e| PresentError::Platform(format!("parent wl_surface::from_id: {e}")))?;

        let surface = compositor.create_surface(&qh, ());
        let surface_version = surface.version();
        let subsurface = subcompositor.get_subsurface(&surface, &parent, &qh, ());
        subsurface.set_position(0, 0);
        subsurface.set_desync();

        // Empty input region keeps all pointer/touch focus on the parent window.
        let empty = compositor.create_region(&qh, ());
        surface.set_input_region(Some(&empty));
        empty.destroy();

        let viewport = globals
            .viewporter
            .as_ref()
            .map(|vp| vp.get_viewport(&surface, &qh, ()));

        let parent_anchor = MappedShm::new(shm, &qh, (1, 1), wl_shm::Format::Argb8888).ok();

        Ok(Self {
            conn,
            queue,
            qh,
            globals,
            parent,
            surface,
            subsurface,
            viewport,
            parent_anchor,
            parent_mapped: false,
            ring: [None, None],
            next_slot: 0,
            retained: Vec::new(),
            retained_size: (0, 0),
            scale_factor: 1.0,
            surface_version,
        })
    }

    /// Sets the window scale factor for `wp_viewport` logical sizing.
    pub fn set_scale_factor(&mut self, scale: f64) {
        if scale.is_finite() && scale > 0.0 {
            self.scale_factor = scale;
        }
    }

    fn acquire_slot(&mut self, buf_size: (u32, u32)) -> Result<usize, PresentError> {
        let _ = self.queue.dispatch_pending(&mut self.globals);
        let shm = self
            .globals
            .shm
            .as_ref()
            .ok_or_else(|| PresentError::Platform("wl_shm unavailable".into()))?;
        // Prefer an idle slot whose dimensions already match.
        let idx = if let Some(i) = self.ring.iter().position(|slot| {
            slot.as_ref()
                .is_some_and(|b| !b.busy.load(Ordering::Acquire))
        }) {
            i
        } else {
            let i = self.next_slot;
            self.next_slot = (self.next_slot + 1) % self.ring.len();
            i
        };
        let needs_alloc = self.ring[idx]
            .as_ref()
            .is_none_or(|b| b.size != buf_size || b.busy.load(Ordering::Acquire));
        if needs_alloc {
            self.ring[idx] = Some(MappedShm::new(
                shm,
                &self.qh,
                buf_size,
                wl_shm::Format::Xrgb8888,
            )?);
        }
        Ok(idx)
    }

    fn commit_slot(
        &mut self,
        idx: usize,
        dst_physical: (u32, u32),
        damage: &[DamageRect],
    ) -> Result<(), PresentError> {
        let slot = self.ring[idx].as_mut().expect("acquired shm slot");
        slot.busy.store(true, Ordering::Release);
        let mut buffer_scale = 1;
        if let Some(vp) = &self.viewport {
            let logical_w = (f64::from(dst_physical.0) / self.scale_factor)
                .round()
                .max(1.0) as i32;
            let logical_h = (f64::from(dst_physical.1) / self.scale_factor)
                .round()
                .max(1.0) as i32;
            vp.set_destination(logical_w, logical_h);
        } else if self.surface_version >= 3 {
            // No viewport: an integer scale maps the physical buffer onto the
            // logical size. Fractional scales stay 1:1 (oversized) here; every
            // compositor offering fractional scaling also offers wp_viewporter.
            buffer_scale = integer_buffer_scale(self.scale_factor, dst_physical);
            self.surface.set_buffer_scale(buffer_scale as i32);
        }
        self.surface.attach(Some(&slot.buffer), 0, 0);
        for &r in damage {
            if r.is_empty() {
                continue;
            }
            if self.surface_version >= 4 {
                self.surface
                    .damage_buffer(r.x as i32, r.y as i32, r.width as i32, r.height as i32);
            } else {
                // Surface-local coordinates: buffer pixels divided by the scale.
                let (x, y) = (r.x / buffer_scale, r.y / buffer_scale);
                let w = (r.x + r.width).div_ceil(buffer_scale) - x;
                let h = (r.y + r.height).div_ceil(buffer_scale) - y;
                self.surface.damage(x as i32, y as i32, w as i32, h as i32);
            }
        }
        self.surface.commit();
        if !self.parent_mapped {
            if let Some(anchor) = &self.parent_anchor {
                self.parent.attach(Some(&anchor.buffer), 0, 0);
            }
            self.parent.commit();
            self.parent_mapped = true;
        }
        self.conn
            .flush()
            .map_err(|e| PresentError::Platform(format!("wayland flush failed: {e}")))?;
        Ok(())
    }

    /// Uploads and presents one dirty rectangle of `0x00RRGGBB` pixels.
    ///
    /// A partial patch needs the 1:1 baseline of an earlier full frame at
    /// `surface_size`; without one (first frame, resize, or after a scaled
    /// frame) it fails so the caller repaints the full surface.
    pub fn present_patch(
        &mut self,
        rect: DamageRect,
        pixels_0rgb: &[u32],
        surface_size: (u32, u32),
    ) -> Result<(), PresentError> {
        if self.retained_size != surface_size {
            if rect != DamageRect::from_array([0, 0, surface_size.0, surface_size.1]) {
                return Err(PresentError::Platform(
                    "wayland patch has no full-frame baseline".into(),
                ));
            }
            self.retained
                .resize(surface_size.0 as usize * surface_size.1 as usize, 0);
            self.retained_size = surface_size;
        }
        let stride = surface_size.0 as usize;
        let w = rect.width as usize;
        for row in 0..rect.height as usize {
            let dst = (rect.y as usize + row) * stride + rect.x as usize;
            let src = row * w;
            self.retained[dst..dst + w].copy_from_slice(&pixels_0rgb[src..src + w]);
        }
        let idx = self.acquire_slot(surface_size)?;
        let dst = self.ring[idx]
            .as_mut()
            .expect("acquired shm slot")
            .as_mut_slice();
        dst.copy_from_slice(&self.retained);
        self.commit_slot(idx, surface_size, &[rect])
    }

    /// Uploads and presents a full-surface `0x00RRGGBB` frame with explicit damage rects.
    pub fn present_full_with_damage(
        &mut self,
        pixels_0rgb: &[u32],
        surface_size: (u32, u32),
        damage: &[DamageRect],
    ) -> Result<(), PresentError> {
        if self.retained_size != surface_size {
            self.retained
                .resize(surface_size.0 as usize * surface_size.1 as usize, 0);
            self.retained_size = surface_size;
        }
        self.retained.copy_from_slice(pixels_0rgb);
        let idx = self.acquire_slot(surface_size)?;
        let dst = self.ring[idx]
            .as_mut()
            .expect("acquired shm slot")
            .as_mut_slice();
        dst.copy_from_slice(pixels_0rgb);
        let full = [DamageRect::from_array([
            0,
            0,
            surface_size.0,
            surface_size.1,
        ])];
        let rects = if damage.is_empty() { &full[..] } else { damage };
        self.commit_slot(idx, surface_size, rects)
    }

    /// Presents a budgeted `src_size` bitmap scaled to fill `dst_size`.
    pub fn present_scaled(
        &mut self,
        src_size: (u32, u32),
        pixels_0rgb: &[u32],
        dst_size: (u32, u32),
    ) -> Result<(), PresentError> {
        // When wp_viewporter is available, upload only the budgeted `src_size`
        // buffer and let the Wayland compositor hardware-scale it to `dst_size`.
        if self.viewport.is_some() {
            let idx = self.acquire_slot(src_size)?;
            // The commit is queued from here on: the 1:1 baseline is stale.
            self.retained_size = (0, 0);
            let dst = self.ring[idx]
                .as_mut()
                .expect("acquired shm slot")
                .as_mut_slice();
            dst.copy_from_slice(pixels_0rgb);
            return self.commit_slot(
                idx,
                dst_size,
                &[DamageRect::from_array([0, 0, src_size.0, src_size.1])],
            );
        }
        let idx = self.acquire_slot(dst_size)?;
        self.retained_size = (0, 0);
        let dst = self.ring[idx]
            .as_mut()
            .expect("acquired shm slot")
            .as_mut_slice();
        blit_scaled_0rgb(pixels_0rgb, src_size, dst, dst_size, dst_size.0 as usize, 0);
        self.commit_slot(
            idx,
            dst_size,
            &[DamageRect::from_array([0, 0, dst_size.0, dst_size.1])],
        )
    }
}

/// The `wl_surface` buffer scale for `scale` without a viewport: the scale
/// itself when it is a whole number dividing both buffer dimensions (the
/// protocol requires that), else 1.
fn integer_buffer_scale(scale: f64, buffer: (u32, u32)) -> u32 {
    let whole = scale.round();
    if (scale - whole).abs() > 1e-6 || !(1.0..=16.0).contains(&whole) {
        return 1;
    }
    let s = whole as u32;
    if buffer.0.is_multiple_of(s) && buffer.1.is_multiple_of(s) {
        s
    } else {
        1
    }
}

impl Drop for WaylandShmPresenter {
    fn drop(&mut self) {
        if let Some(vp) = self.viewport.take() {
            vp.destroy();
        }
        self.surface.attach(None, 0, 0);
        self.surface.commit();
        self.subsurface.destroy();
        self.surface.destroy();
        self.ring = [None, None];
        self.parent_anchor.take();
        if let Some(vp) = self.globals.viewporter.take() {
            vp.destroy();
        }
        self.globals.shm.take();
        if let Some(sub) = self.globals.subcompositor.take() {
            sub.destroy();
        }
        let _ = self.conn.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buffer_scale_is_a_whole_divisor_or_one() {
        assert_eq!(integer_buffer_scale(2.0, (1600, 1200)), 2);
        assert_eq!(integer_buffer_scale(1.0, (801, 601)), 1);
        assert_eq!(integer_buffer_scale(1.5, (1200, 900)), 1);
        assert_eq!(integer_buffer_scale(2.0, (1601, 1200)), 1);
        assert_eq!(integer_buffer_scale(3.0, (900, 600)), 3);
    }
}
