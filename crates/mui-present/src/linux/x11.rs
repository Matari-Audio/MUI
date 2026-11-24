//! Direct X11 CPU presentation via `x11rb` MIT-SHM with automatic `PutImage` fallback.
//!
//! Opens a private pure-Rust X11 connection (zero `libX11` or `libxcb` dynamic
//! link) to the host window's display. MIT-SHM frames rotate through
//! [`SHM_SLOTS`] segments: every `ShmPutImage` asks for a `ShmCompletion`
//! event, and a slot is written again only after the server reported it done
//! (or a round trip proved every earlier request was processed). Each SysV
//! segment is marked `IPC_RMID` as soon as the server attached it, and every
//! error path before that removes and detaches it, so no path leaks a kernel
//! SHM ID.
//!
//! Each frame ends with one round trip, so a request the server rejected (for
//! example after the host destroyed the window) fails that frame instead of
//! counting as presented. Drawables whose pixel layout is not 32-bit
//! native-endian x8r8g8b8 are rejected at construction.

use std::ptr;

use x11rb::connection::{Connection, RequestConnection};
use x11rb::protocol::Event;
use x11rb::protocol::shm::{self, ConnectionExt as ShmExt};
use x11rb::protocol::xproto::{self, ConnectionExt as XprotoExt, CreateGCAux, ImageFormat};
use x11rb::rust_connection::RustConnection;

use crate::{DamageRect, PresentError, PresentationBackend, blit_scaled_0rgb};

/// MIT-SHM buffers in flight: one the server may be reading, one to write.
const SHM_SLOTS: usize = 2;

fn platform(what: &str, error: impl std::fmt::Display) -> PresentError {
    PresentError::Platform(format!("x11 {what} failed: {error}"))
}

/// A SysV segment id, removed (`IPC_RMID`) when dropped.
struct SegmentId(libc::c_int);

impl Drop for SegmentId {
    #[expect(unsafe_code, reason = "SysV shmctl(IPC_RMID)")]
    fn drop(&mut self) {
        // SAFETY: the id came from shmget and is removed exactly once.
        unsafe {
            libc::shmctl(self.0, libc::IPC_RMID, ptr::null_mut());
        }
    }
}

/// This process's attachment of a SysV segment, detached when dropped.
struct Mapping(*mut libc::c_void);

impl Drop for Mapping {
    #[expect(unsafe_code, reason = "SysV shmdt")]
    fn drop(&mut self) {
        // SAFETY: the address came from shmat and is detached exactly once.
        unsafe {
            libc::shmdt(self.0);
        }
    }
}

struct ShmBuffer {
    seg: shm::Seg,
    map: Mapping,
    len_u32: usize,
    size: (u32, u32),
    /// `ShmPutImage` requests the server has not reported complete.
    pending: u32,
}

impl ShmBuffer {
    #[expect(unsafe_code, reason = "POSIX SysV shared memory for X11 MIT-SHM")]
    fn create(conn: &RustConnection, size: (u32, u32)) -> Result<Self, PresentError> {
        let len_u32 = (size.0 as usize)
            .checked_mul(size.1 as usize)
            .ok_or(PresentError::InvalidSize(size))?;
        let byte_len = len_u32
            .checked_mul(4)
            .ok_or(PresentError::InvalidSize(size))?;

        // SAFETY: IPC_PRIVATE creates a fresh private shared-memory segment.
        let shmid = unsafe { libc::shmget(libc::IPC_PRIVATE, byte_len, libc::IPC_CREAT | 0o600) };
        if shmid < 0 {
            return Err(platform("shmget", std::io::Error::last_os_error()));
        }
        let id = SegmentId(shmid);
        // SAFETY: shmid was just returned by shmget; a null address lets the kernel choose.
        let addr = unsafe { libc::shmat(shmid, ptr::null(), 0) };
        if addr == (-1isize as *mut libc::c_void) || addr.is_null() {
            return Err(platform("shmat", std::io::Error::last_os_error()));
        }
        let map = Mapping(addr);
        let seg = conn.generate_id().map_err(|e| platform("generate_id", e))?;
        conn.shm_attach(seg, shmid as u32, false)
            .map_err(|e| platform("shm_attach send", e))?
            .check()
            .map_err(|e| platform("shm_attach", e))?;
        // Both attachments exist: the kernel frees the segment once the last
        // one goes, even if this process dies abruptly.
        drop(id);
        Ok(Self {
            seg,
            map,
            len_u32,
            size,
            pending: 0,
        })
    }

    #[expect(unsafe_code, reason = "exclusive view of our private MIT-SHM segment")]
    fn as_mut_slice(&mut self) -> &mut [u32] {
        // SAFETY: the mapping holds `len_u32` 4-byte words, is owned by `self`,
        // and the caller only writes an idle slot the server is not reading.
        unsafe { std::slice::from_raw_parts_mut(self.map.0.cast::<u32>(), self.len_u32) }
    }
}

/// `true` when a `depth`/`visual` drawable takes this presenter's pixels
/// unconverted: 32 bits per pixel in native byte order through an x8r8g8b8
/// `TrueColor` visual (in practice depth 24 or 32).
fn native_pixel_layout(setup: &xproto::Setup, depth: u8, visual: xproto::Visualid) -> bool {
    let native_order = if cfg!(target_endian = "little") {
        xproto::ImageOrder::LSB_FIRST
    } else {
        xproto::ImageOrder::MSB_FIRST
    };
    setup.image_byte_order == native_order
        && setup
            .pixmap_formats
            .iter()
            .any(|f| f.depth == depth && f.bits_per_pixel == 32)
        && setup
            .roots
            .iter()
            .flat_map(|screen| &screen.allowed_depths)
            .filter(|d| d.depth == depth)
            .flat_map(|d| &d.visuals)
            .any(|v| {
                v.visual_id == visual
                    && v.class == xproto::VisualClass::TRUE_COLOR
                    && (v.red_mask, v.green_mask, v.blue_mask) == (0x00ff_0000, 0xff00, 0xff)
            })
}

/// `PutImage` tiles of `rect` whose payload fits in `max_bytes`: whole rows
/// when a row fits, otherwise row segments of at most `max_bytes / 4` pixels.
fn put_image_tiles(rect: DamageRect, max_bytes: usize) -> impl Iterator<Item = DamageRect> {
    let max_px = (max_bytes / 4).max(1);
    let tile_w = (rect.width as usize).clamp(1, max_px) as u32;
    let tile_h = (max_px / tile_w as usize).clamp(1, rect.height.max(1) as usize) as u32;
    let (x_end, y_end) = (rect.x + rect.width, rect.y + rect.height);
    (rect.y..y_end).step_by(tile_h as usize).flat_map(move |y| {
        (rect.x..x_end)
            .step_by(tile_w as usize)
            .map(move |x| DamageRect {
                x,
                y,
                width: tile_w.min(x_end - x),
                height: tile_h.min(y_end - y),
            })
    })
}

/// Direct X11 CPU presenter (`x11rb` MIT-SHM with `PutImage` fallback).
pub struct X11Presenter {
    conn: RustConnection,
    window: xproto::Window,
    gc: xproto::Gcontext,
    depth: u8,
    opaque_mask: u32,
    shm_supported: bool,
    slots: [Option<ShmBuffer>; SHM_SLOTS],
    /// Slot written by the frame being built.
    active: usize,
    fallback: Vec<u32>,
    fallback_size: (u32, u32),
}

impl X11Presenter {
    /// Creates a direct X11 presenter targeting window `window` on `display`
    /// (an X11 display name such as `":0"`; `None` uses `$DISPLAY`).
    ///
    /// # Errors
    /// Returns [`PresentError`] if the display cannot be reached, the window
    /// cannot be queried, its pixel layout is not 32-bit native-endian
    /// x8r8g8b8, or a graphics context cannot be created.
    pub fn new(window: u32, display: Option<&str>) -> Result<Self, PresentError> {
        let (conn, _) = RustConnection::connect(display).map_err(|e| platform("connect", e))?;
        let geom = conn
            .get_geometry(window)
            .map_err(|e| platform("get_geometry send", e))?
            .reply()
            .map_err(|e| platform("get_geometry", e))?;
        let visual = conn
            .get_window_attributes(window)
            .map_err(|e| platform("get_window_attributes send", e))?
            .reply()
            .map_err(|e| platform("get_window_attributes", e))?
            .visual;
        let depth = geom.depth;
        if !native_pixel_layout(conn.setup(), depth, visual) {
            return Err(PresentError::Platform(format!(
                "x11 drawable layout unsupported (depth {depth}, visual {visual:#x}): \
                 needs 32-bit native-endian x8r8g8b8"
            )));
        }
        // Depth-32 ARGB visuals interpret the top byte as alpha under compositors;
        // depth-24 ignores it. Setting 0xff00_0000 keeps CPU frames opaque on both.
        let opaque_mask = if depth == 32 { 0xff00_0000 } else { 0 };
        let gc = conn.generate_id().map_err(|e| platform("gc id", e))?;
        conn.create_gc(gc, window, &CreateGCAux::new().graphics_exposures(0))
            .map_err(|e| platform("create_gc", e))?;
        let shm_supported = conn
            .shm_query_version()
            .ok()
            .and_then(|c| c.reply().ok())
            .is_some_and(|r| r.major_version >= 1);
        let _ = conn.flush();
        Ok(Self {
            conn,
            window,
            gc,
            depth,
            opaque_mask,
            shm_supported,
            slots: [None, None],
            active: 0,
            fallback: Vec::new(),
            fallback_size: (0, 0),
        })
    }

    /// Reports whether MIT-SHM or `PutImage` is currently active.
    #[must_use]
    pub fn backend(&self) -> PresentationBackend {
        if self.shm_supported {
            PresentationBackend::LinuxX11Shm
        } else {
            PresentationBackend::LinuxX11PutImage
        }
    }

    /// Retires slots whose `ShmPutImage` requests the server reported done and
    /// fails on any queued X11 request error (dropped void cookies report there).
    fn drain_completions(&mut self) -> Result<(), PresentError> {
        while let Some(event) = self
            .conn
            .poll_for_event()
            .map_err(|e| platform("poll_for_event", e))?
        {
            match event {
                Event::Error(error) => {
                    return Err(PresentError::Platform(format!(
                        "x11 request failed: {error:?}"
                    )));
                }
                Event::ShmCompletion(done) => {
                    if let Some(slot) = self
                        .slots
                        .iter_mut()
                        .flatten()
                        .find(|slot| slot.seg == done.shmseg)
                    {
                        slot.pending = slot.pending.saturating_sub(1);
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Ends a frame: flushes, then one round trip so every request error of
    /// the frame is reported before it counts as presented.
    fn finish_frame(&mut self) -> Result<(), PresentError> {
        self.conn.flush().map_err(|e| platform("flush", e))?;
        self.sync()?;
        self.drain_completions()
    }

    /// A round trip: every earlier request, every `ShmPutImage` included,
    /// has been processed once its reply arrives.
    fn sync(&self) -> Result<(), PresentError> {
        self.conn
            .get_input_focus()
            .map_err(|e| platform("sync send", e))?
            .reply()
            .map(drop)
            .map_err(|e| platform("sync", e))
    }

    fn destroy_slot(&mut self, index: usize) {
        if let Some(old) = self.slots[index].take() {
            // An idle slot: the server no longer reads it.
            let _ = self.conn.shm_detach(old.seg);
            let _ = self.conn.flush();
        }
    }

    /// Picks an idle SHM slot sized `size` for the next frame. `false`: MIT-SHM
    /// is unavailable and the `PutImage` buffer is ready instead.
    fn ensure_buffer(&mut self, size: (u32, u32)) -> Result<bool, PresentError> {
        if self.shm_supported {
            self.drain_completions()?;
            let idle = (0..SHM_SLOTS)
                .map(|offset| (self.active + 1 + offset) % SHM_SLOTS)
                .find(|&i| self.slots[i].as_ref().is_none_or(|slot| slot.pending == 0));
            let index = if let Some(index) = idle {
                index
            } else {
                // Every slot is still being read: fence on a round trip.
                self.sync()?;
                self.drain_completions()?;
                for slot in self.slots.iter_mut().flatten() {
                    slot.pending = 0;
                }
                (self.active + 1) % SHM_SLOTS
            };
            if self.slots[index]
                .as_ref()
                .is_none_or(|slot| slot.size != size)
            {
                self.destroy_slot(index);
                if let Ok(buffer) = ShmBuffer::create(&self.conn, size) {
                    self.slots[index] = Some(buffer);
                } else {
                    // Remote X11 or restricted container: PutImage from now on.
                    self.sync()?;
                    for i in 0..SHM_SLOTS {
                        self.destroy_slot(i);
                    }
                    self.shm_supported = false;
                }
            }
            if self.shm_supported {
                self.active = index;
                return Ok(true);
            }
        }
        if self.fallback_size != size {
            self.fallback
                .resize(size.0 as usize * size.1 as usize, self.opaque_mask);
            self.fallback_size = size;
        }
        Ok(false)
    }

    fn target(&mut self, use_shm: bool) -> &mut [u32] {
        if use_shm {
            self.slots[self.active]
                .as_mut()
                .expect("idle shm slot selected")
                .as_mut_slice()
        } else {
            &mut self.fallback
        }
    }

    /// Uploads and presents one dirty rectangle of `0x00RRGGBB` pixels.
    pub fn present_patch(
        &mut self,
        rect: DamageRect,
        pixels_0rgb: &[u32],
        surface_size: (u32, u32),
    ) -> Result<(), PresentError> {
        let use_shm = self.ensure_buffer(surface_size)?;
        let opaque = self.opaque_mask;
        let dst = self.target(use_shm);
        let stride = surface_size.0 as usize;
        let w = rect.width as usize;
        for row in 0..rect.height as usize {
            let dst_start = (rect.y as usize + row) * stride + rect.x as usize;
            let src_start = row * w;
            for (out, &px) in dst[dst_start..dst_start + w]
                .iter_mut()
                .zip(&pixels_0rgb[src_start..src_start + w])
            {
                *out = opaque | px;
            }
        }
        self.submit_rects(surface_size, &[rect], use_shm)
    }

    /// Uploads and presents a full-surface `0x00RRGGBB` frame with explicit damage rects.
    pub fn present_full_with_damage(
        &mut self,
        pixels_0rgb: &[u32],
        surface_size: (u32, u32),
        damage: &[DamageRect],
    ) -> Result<(), PresentError> {
        let use_shm = self.ensure_buffer(surface_size)?;
        let opaque = self.opaque_mask;
        let rects: Vec<DamageRect> = if damage.is_empty() {
            vec![DamageRect::from_array([
                0,
                0,
                surface_size.0,
                surface_size.1,
            ])]
        } else {
            damage.iter().copied().filter(|r| !r.is_empty()).collect()
        };
        let dst = self.target(use_shm);
        let stride = surface_size.0 as usize;
        for &r in &rects {
            let w = r.width as usize;
            for row in r.y as usize..(r.y + r.height) as usize {
                let start = row * stride + r.x as usize;
                for (out, &px) in dst[start..start + w]
                    .iter_mut()
                    .zip(&pixels_0rgb[start..start + w])
                {
                    *out = opaque | px;
                }
            }
        }
        self.submit_rects(surface_size, &rects, use_shm)
    }

    /// Presents a budgeted `src_size` bitmap scaled to fill `dst_size`.
    pub fn present_scaled(
        &mut self,
        src_size: (u32, u32),
        pixels_0rgb: &[u32],
        dst_size: (u32, u32),
    ) -> Result<(), PresentError> {
        let use_shm = self.ensure_buffer(dst_size)?;
        let opaque = self.opaque_mask;
        let dst = self.target(use_shm);
        blit_scaled_0rgb(
            pixels_0rgb,
            src_size,
            dst,
            dst_size,
            dst_size.0 as usize,
            opaque,
        );
        self.submit_rects(
            dst_size,
            &[DamageRect::from_array([0, 0, dst_size.0, dst_size.1])],
            use_shm,
        )
    }

    fn submit_rects(
        &mut self,
        surface_size: (u32, u32),
        rects: &[DamageRect],
        use_shm: bool,
    ) -> Result<(), PresentError> {
        let coord = |v: u32| u16::try_from(v).map_err(|_| PresentError::InvalidPatch);
        if use_shm {
            let total_w = u16::try_from(surface_size.0)
                .map_err(|_| PresentError::InvalidSize(surface_size))?;
            let total_h = u16::try_from(surface_size.1)
                .map_err(|_| PresentError::InvalidSize(surface_size))?;
            let seg = self.slots[self.active]
                .as_ref()
                .expect("shm slot active")
                .seg;
            for &r in rects.iter().filter(|r| !r.is_empty()) {
                let (x, y) = (coord(r.x)?, coord(r.y)?);
                self.conn
                    .shm_put_image(
                        self.window,
                        self.gc,
                        total_w,
                        total_h,
                        x,
                        y,
                        coord(r.width)?,
                        coord(r.height)?,
                        x as i16,
                        y as i16,
                        self.depth,
                        ImageFormat::Z_PIXMAP.into(),
                        // A ShmCompletion event reports the server done reading.
                        true,
                        seg,
                        0,
                    )
                    .map_err(|e| platform("shm_put_image", e))?;
                if let Some(slot) = self.slots[self.active].as_mut() {
                    slot.pending += 1;
                }
            }
            return self.finish_frame();
        }

        // PutImage fallback: every request stays under the maximum request length.
        let max_bytes = self
            .conn
            .maximum_request_bytes()
            .saturating_sub(256)
            .max(4096);
        let stride = surface_size.0 as usize;
        let mut bytes = Vec::new();
        for &r in rects.iter().filter(|r| !r.is_empty()) {
            for tile in put_image_tiles(r, max_bytes) {
                bytes.clear();
                bytes.reserve(tile.width as usize * tile.height as usize * 4);
                for row in tile.y..tile.y + tile.height {
                    let start = row as usize * stride + tile.x as usize;
                    for &px in &self.fallback[start..start + tile.width as usize] {
                        bytes.extend_from_slice(&px.to_ne_bytes());
                    }
                }
                self.conn
                    .put_image(
                        ImageFormat::Z_PIXMAP,
                        self.window,
                        self.gc,
                        coord(tile.width)?,
                        coord(tile.height)?,
                        i16::try_from(tile.x).map_err(|_| PresentError::InvalidPatch)?,
                        i16::try_from(tile.y).map_err(|_| PresentError::InvalidPatch)?,
                        0,
                        self.depth,
                        &bytes,
                    )
                    .map_err(|e| platform("put_image", e))?;
            }
        }
        self.finish_frame()
    }
}

impl Drop for X11Presenter {
    fn drop(&mut self) {
        for slot in self.slots.iter().flatten() {
            let _ = self.conn.shm_detach(slot.seg);
        }
        let _ = self.conn.free_gc(self.gc);
        // The server has stopped reading every segment before ours detach.
        let _ = self.sync();
        self.slots = [None, None];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn covers(rect: DamageRect, max_bytes: usize) -> Vec<DamageRect> {
        let tiles: Vec<_> = put_image_tiles(rect, max_bytes).collect();
        let area: u64 = tiles
            .iter()
            .map(|t| u64::from(t.width) * u64::from(t.height))
            .sum();
        assert_eq!(
            area,
            u64::from(rect.width) * u64::from(rect.height),
            "{rect:?}"
        );
        for t in &tiles {
            assert!(t.width as usize * t.height as usize * 4 <= max_bytes.max(4));
            assert!(t.x >= rect.x && t.x + t.width <= rect.x + rect.width);
            assert!(t.y >= rect.y && t.y + t.height <= rect.y + rect.height);
        }
        tiles
    }

    #[test]
    fn put_image_tiles_split_rows_then_columns() {
        // Rows fit: whole-row bands of 10 rows.
        let tiles = covers(DamageRect::from_array([5, 7, 100, 100]), 4000);
        assert_eq!(tiles.len(), 10);
        assert!(tiles.iter().all(|t| t.width == 100 && t.height == 10));
        // One row exceeds the request: 4096-pixel segments, one row each.
        let tiles = covers(DamageRect::from_array([3, 0, 10_000, 3]), 4096 * 4);
        assert_eq!(tiles.len(), 9);
        assert_eq!(tiles[2], DamageRect::from_array([8195, 0, 1808, 1]));
        // Degenerate request sizes still make progress one pixel at a time.
        assert_eq!(covers(DamageRect::from_array([0, 0, 2, 2]), 1).len(), 4);
    }

    /// Serialize Xvfb tests within this process so concurrent `Xvfb -displayfd`
    /// launches and process-wide `/proc/sysvipc/shm` (`own_segments()`) counts
    /// cannot race against each other.
    static XVFB_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// A private Xvfb server, terminated cleanly on drop.
    struct Xvfb(std::process::Child, String);

    impl Xvfb {
        fn start_and_connect(depth: u8) -> Option<(Self, RustConnection, usize)> {
            use std::io::{BufRead, BufReader, ErrorKind};

            const ATTEMPTS: usize = 5;
            let mut last_err = String::new();
            for attempt in 0..ATTEMPTS {
                let mut child = match std::process::Command::new("Xvfb")
                    .args([
                        "-displayfd",
                        "1",
                        "-screen",
                        "0",
                        &format!("320x240x{depth}"),
                        "-nolisten",
                        "tcp",
                    ])
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::null())
                    .spawn()
                {
                    Ok(child) => child,
                    Err(err) if err.kind() == ErrorKind::NotFound => return None,
                    Err(err) => {
                        last_err = format!("spawn Xvfb: {err}");
                        continue;
                    }
                };
                let mut line = String::new();
                let read_ok = child
                    .stdout
                    .take()
                    .and_then(|out| BufReader::new(out).read_line(&mut line).ok())
                    .is_some();
                let display_num = line.trim().parse::<u32>().ok();
                let server = Self(child, format!(":{}", line.trim()));
                if !read_ok || display_num.is_none() {
                    last_err = format!("Xvfb wrote invalid displayfd {:?}", line.trim());
                    drop(server);
                    std::thread::sleep(std::time::Duration::from_millis(20));
                    continue;
                }
                match RustConnection::connect(Some(&server.1)) {
                    Ok((conn, screen)) => return Some((server, conn, screen)),
                    Err(err) => {
                        last_err = format!(
                            "connect to Xvfb {} (attempt {}): {err}",
                            server.1,
                            attempt + 1
                        );
                        drop(server);
                        std::thread::sleep(std::time::Duration::from_millis(20));
                    }
                }
            }
            panic!("failed to start and connect to Xvfb after {ATTEMPTS} attempts: {last_err}");
        }
    }

    impl Drop for Xvfb {
        fn drop(&mut self) {
            // Prefer SIGTERM so Xvfb unlinks its /tmp/.X*-lock and
            // /tmp/.X11-unix/X* socket instead of leaving stale sockets behind
            // for subsequent displayfd allocations.
            let _ = std::process::Command::new("kill")
                .args(["-TERM", &self.0.id().to_string()])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
            for _ in 0..20 {
                if matches!(self.0.try_wait(), Ok(Some(_))) {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    /// SysV segments created by this process that still exist.
    fn own_segments() -> usize {
        let pid = std::process::id().to_string();
        std::fs::read_to_string("/proc/sysvipc/shm")
            .unwrap_or_default()
            .lines()
            .skip(1)
            .filter(|line| line.split_whitespace().nth(4) == Some(pid.as_str()))
            .count()
    }

    fn pixel(conn: &RustConnection, window: u32, x: i16, y: i16) -> u32 {
        let image = conn
            .get_image(ImageFormat::Z_PIXMAP, window, x, y, 1, 1, !0)
            .unwrap()
            .reply()
            .unwrap();
        u32::from_ne_bytes(image.data[..4].try_into().unwrap()) & 0x00ff_ffff
    }

    /// A mapped 64x48 child of the root window, created and synced on `conn`.
    fn test_window(conn: &RustConnection, screen: usize) -> u32 {
        let root = conn.setup().roots[screen].root;
        let window = conn.generate_id().unwrap();
        conn.create_window(
            x11rb::COPY_DEPTH_FROM_PARENT,
            window,
            root,
            0,
            0,
            64,
            48,
            0,
            xproto::WindowClass::INPUT_OUTPUT,
            0,
            &xproto::CreateWindowAux::new(),
        )
        .unwrap();
        conn.map_window(window).unwrap();
        conn.get_input_focus().unwrap().reply().unwrap();
        window
    }

    #[test]
    fn xvfb_rejects_foreign_layouts_and_reports_request_errors() {
        let _xvfb_guard = XVFB_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        {
            let Some((server, conn, screen)) = Xvfb::start_and_connect(16) else {
                eprintln!("skipped: Xvfb is not available");
                return;
            };
            let window = test_window(&conn, screen);
            assert!(
                X11Presenter::new(window, Some(&server.1)).is_err(),
                "depth-16 drawable must be rejected"
            );
        }

        let Some((server, conn, screen)) = Xvfb::start_and_connect(24) else {
            return;
        };
        let window = test_window(&conn, screen);
        let mut presenter = X11Presenter::new(window, Some(&server.1)).unwrap();
        let pixels = vec![0; 64 * 48];
        presenter
            .present_full_with_damage(&pixels, (64, 48), &[])
            .unwrap();
        conn.destroy_window(window).unwrap();
        conn.get_input_focus().unwrap().reply().unwrap();
        for use_shm in [true, false] {
            presenter.shm_supported = use_shm;
            assert!(
                presenter
                    .present_full_with_damage(&pixels, (64, 48), &[])
                    .is_err(),
                "rejected request must fail the frame (shm: {use_shm})"
            );
        }
    }

    #[test]
    fn xvfb_shm_slots_rotate_without_tearing_or_leaking() {
        let _xvfb_guard = XVFB_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some((server, conn, screen)) = Xvfb::start_and_connect(24) else {
            eprintln!("skipped: Xvfb is not available");
            return;
        };
        let window = test_window(&conn, screen);

        let before = own_segments();
        let mut presenter = X11Presenter::new(window, Some(&server.1)).unwrap();
        assert_eq!(presenter.backend(), PresentationBackend::LinuxX11Shm);
        let size = (64, 48);
        for frame in 0..8u32 {
            let color = (0x0010_2030 * (frame + 1)) & 0x00ff_ffff;
            let pixels = vec![color; 64 * 48];
            presenter
                .present_full_with_damage(&pixels, size, &[])
                .unwrap();
            presenter.sync().unwrap();
            assert_eq!(pixel(&conn, window, 63, 47), color, "frame {frame}");
            // A patch lands in the other slot and shows only its rectangle.
            presenter
                .present_patch(DamageRect::from_array([0, 0, 1, 1]), &[0x00ab_cdef], size)
                .unwrap();
            presenter.sync().unwrap();
            assert_eq!(pixel(&conn, window, 0, 0), 0x00ab_cdef);
            assert_eq!(pixel(&conn, window, 63, 47), color);
        }
        assert!(
            presenter.slots.iter().all(Option::is_some),
            "both slots used"
        );
        assert_eq!(own_segments(), before + SHM_SLOTS);
        // Every slot busy: the next frame fences instead of overwriting.
        for slot in presenter.slots.iter_mut().flatten() {
            slot.pending = 1;
        }
        presenter
            .present_full_with_damage(&vec![0x0011_2233; 64 * 48], size, &[])
            .unwrap();
        presenter.sync().unwrap();
        assert_eq!(pixel(&conn, window, 10, 10), 0x0011_2233);
        // A resize replaces both segments and frees the old ones.
        for _ in 0..2 {
            presenter
                .present_full_with_damage(&vec![0; 32 * 24], (32, 24), &[])
                .unwrap();
        }
        presenter.sync().unwrap();
        assert_eq!(own_segments(), before + SHM_SLOTS);
        drop(presenter);
        conn.get_input_focus().unwrap().reply().unwrap();
        assert_eq!(own_segments(), before, "every segment is released");

        // PutImage fallback through 2-D tiles.
        let mut presenter = X11Presenter::new(window, Some(&server.1)).unwrap();
        presenter.shm_supported = false;
        let pixels: Vec<u32> = (0..64 * 48).collect();
        presenter
            .present_full_with_damage(&pixels, size, &[])
            .unwrap();
        presenter.sync().unwrap();
        assert_eq!(pixel(&conn, window, 63, 47), 64 * 48 - 1);
        assert_eq!(pixel(&conn, window, 5, 1), 69);
    }
}
