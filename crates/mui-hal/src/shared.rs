//! Shared internal HAL primitives and portable validation helpers reused
//! across native backends (`vulkan`, `metal`, `d3d11`).
//!
//! Compiled and unit-tested on every host so validation semantics stay uniform
//! across all backends without duplicating boilerplate.

#![allow(
    dead_code,
    reason = "shared across cfg-gated Vulkan, Metal, and D3D11 backends and unit-tested on all hosts"
)]

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use crate::types::{
    BufferUsages, DeviceLostReason, ErrorFilter, Extent3d, Features, Limits, Origin3d,
    TexelCopyBufferLayout, TextureDescriptor, TextureFormat, TextureUsages,
};

/// Locks `m`, recovering across poison so internal HAL state stays consistent.
pub(crate) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// A native backend error's HAL category.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ErrorKind {
    Validation,
    OutOfMemory,
    Internal,
}

/// A native backend failure, already formatted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackendError {
    kind: ErrorKind,
    message: String,
}

impl BackendError {
    pub(crate) fn validation(message: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::Validation,
            message: message.into(),
        }
    }

    pub(crate) fn out_of_memory(message: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::OutOfMemory,
            message: message.into(),
        }
    }

    pub(crate) fn internal(message: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::Internal,
            message: message.into(),
        }
    }

    pub(crate) fn into_hal(self) -> crate::Error {
        match self.kind {
            ErrorKind::Validation => crate::Error::Validation {
                description: self.message,
            },
            ErrorKind::OutOfMemory => crate::Error::OutOfMemory {
                source: self.message,
            },
            ErrorKind::Internal => crate::Error::Internal {
                description: self.message,
            },
        }
    }
}

impl std::fmt::Display for BackendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for BackendError {}

pub(crate) type MapCallback = Box<dyn FnOnce(Result<(), crate::BufferAsyncError>) + Send>;
pub(crate) type WorkCallback = Box<dyn FnOnce() + Send>;
pub(crate) type LostCallback = Box<dyn Fn(DeviceLostReason, String) + Send + Sync>;
pub(crate) type UncapturedHandler = Arc<dyn Fn(crate::Error) + Send + Sync>;

/// A native error scope. Like wgpu's, scopes are per thread: an error is
/// caught by the innermost matching scope its own thread pushed.
pub(crate) struct Scope {
    pub id: u64,
    pub thread: std::thread::ThreadId,
    pub filter: ErrorFilter,
    pub caught: Option<crate::Error>,
}

/// Per-device native error scopes and uncaptured handler, shared with native
/// resources without forming a reference cycle back to `DeviceShared`.
pub(crate) struct ErrorSink {
    scopes: Mutex<Vec<Scope>>,
    uncaptured: Mutex<Option<UncapturedHandler>>,
}

impl ErrorSink {
    pub(crate) fn new() -> Self {
        Self {
            scopes: Mutex::new(Vec::new()),
            uncaptured: Mutex::new(None),
        }
    }

    /// Reports `e` to the innermost native scope that filters it, else the
    /// uncaptured handler.
    pub(crate) fn report(&self, e: crate::Error) {
        let filter = match &e {
            crate::Error::OutOfMemory { .. } => ErrorFilter::OutOfMemory,
            crate::Error::Validation { .. } => ErrorFilter::Validation,
            crate::Error::Internal { .. } => ErrorFilter::Internal,
        };
        {
            let mut scopes = lock(&self.scopes);
            let thread = std::thread::current().id();
            if let Some(scope) = scopes
                .iter_mut()
                .rev()
                .find(|s| s.thread == thread && s.filter == filter)
            {
                scope.caught.get_or_insert(e);
                return;
            }
        }
        let handler = lock(&self.uncaptured).clone();
        match handler {
            Some(h) => h(e),
            None => eprintln!("mui-hal: uncaptured GPU error: {e}"),
        }
    }

    pub(crate) fn push_scope(&self, filter: ErrorFilter) -> u64 {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        lock(&self.scopes).push(Scope {
            id,
            thread: std::thread::current().id(),
            filter,
            caught: None,
        });
        id
    }

    pub(crate) fn take_scope(&self, id: u64) -> Option<Scope> {
        let mut scopes = lock(&self.scopes);
        let at = scopes.iter().position(|s| s.id == id)?;
        Some(scopes.remove(at))
    }

    pub(crate) fn set_uncaptured(&self, handler: UncapturedHandler) {
        let prev = lock(&self.uncaptured).replace(handler);
        drop(prev);
    }

    /// Releases any stored uncaptured handler outside internal locks (breaks
    /// capture cycles on device teardown).
    pub(crate) fn clear_uncaptured(&self) {
        let handler = lock(&self.uncaptured).take();
        drop(handler);
    }
}

/// Shared device-lost flag + one-shot callback state.
pub(crate) struct DeviceLost {
    lost: AtomicBool,
    on_lost: Mutex<Option<LostCallback>>,
}

impl DeviceLost {
    pub(crate) fn new() -> Self {
        Self {
            lost: AtomicBool::new(false),
            on_lost: Mutex::new(None),
        }
    }

    pub(crate) fn is_lost(&self) -> bool {
        self.lost.load(Ordering::Acquire)
    }

    pub(crate) fn set_callback(
        &self,
        f: impl Fn(DeviceLostReason, String) + Send + Sync + 'static,
    ) {
        let prev = lock(&self.on_lost).replace(Box::new(f));
        drop(prev);
    }

    /// Marks the device lost once and fires any registered callback outside the
    /// callback mutex.
    pub(crate) fn notify(&self, reason: DeviceLostReason, message: String) {
        if self.mark() {
            self.fire(reason, message);
        }
    }

    /// Publishes the lost flag without running the callback; returns `true` on
    /// the first transition. Callers that advance fence state under a device
    /// mutex call this *before* releasing it, so no thread can observe
    /// `completed` advanced past unfinished GPU work while `is_lost()` is false,
    /// then call [`Self::fire`] once the mutex is released.
    pub(crate) fn mark(&self) -> bool {
        !self.lost.swap(true, Ordering::AcqRel)
    }

    /// Runs the registered callback at most once. Call only after [`Self::mark`]
    /// returned `true`, and never while holding a device mutex.
    pub(crate) fn fire(&self, reason: DeviceLostReason, message: String) {
        let cb = lock(&self.on_lost).take();
        if let Some(cb) = cb {
            cb(reason, message);
        }
    }
}

/// Pending `on_submitted_work_done` waiter.
pub(crate) struct WorkWaiter {
    pub seq: u64,
    pub callback: WorkCallback,
}

/// Splits `waiters`, returning callbacks whose `seq <= done`.
pub(crate) fn drain_ready_work(waiters: &mut Vec<WorkWaiter>, done: u64) -> Vec<WorkCallback> {
    let mut ready = Vec::new();
    let mut remain = Vec::new();
    for w in waiters.drain(..) {
        if w.seq <= done {
            ready.push(w.callback);
        } else {
            remain.push(w);
        }
    }
    *waiters = remain;
    ready
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MapState {
    Unmapped,
    Pending(u64),
    Mapped,
}

/// Thread-safe buffer mapping state machine shared by Metal and D3D11 buffers.
pub(crate) struct BufferMapState {
    state: Mutex<MapState>,
    next_token: AtomicU64,
}

impl BufferMapState {
    pub(crate) fn new() -> Self {
        Self {
            state: Mutex::new(MapState::Unmapped),
            next_token: AtomicU64::new(1),
        }
    }

    pub(crate) fn is_mapped_or_pending(&self) -> bool {
        *lock(&self.state) != MapState::Unmapped
    }

    pub(crate) fn next_token(&self) -> u64 {
        self.next_token.fetch_add(1, Ordering::Relaxed)
    }

    pub(crate) fn try_mark_pending(&self, token: u64) -> bool {
        let mut state = lock(&self.state);
        if *state != MapState::Unmapped {
            return false;
        }
        *state = MapState::Pending(token);
        true
    }

    pub(crate) fn complete_pending(
        &self,
        token: u64,
        res: Result<(), crate::BufferAsyncError>,
    ) -> Result<(), crate::BufferAsyncError> {
        let mut state = lock(&self.state);
        if *state == MapState::Pending(token) {
            *state = if res.is_ok() {
                MapState::Mapped
            } else {
                MapState::Unmapped
            };
            res
        } else {
            Err(crate::BufferAsyncError)
        }
    }

    /// Acquires the mapping mutex iff the buffer is currently [`MapState::Mapped`],
    /// keeping it locked so `unmap()` cannot race mid-read.
    pub(crate) fn lock_mapped(&self) -> Result<MutexGuard<'_, MapState>, crate::BufferAsyncError> {
        let state = lock(&self.state);
        if *state != MapState::Mapped {
            return Err(crate::BufferAsyncError);
        }
        Ok(state)
    }

    pub(crate) fn unmap(&self) {
        *lock(&self.state) = MapState::Unmapped;
    }
}

// --- Shared validation helpers ---

pub(crate) fn validate_device_request(
    required_features: Features,
    supported_features: Features,
    required_limits: &Limits,
    supported_limits: &Limits,
    backend: &str,
) -> Result<(), BackendError> {
    let missing = required_features - supported_features;
    if !missing.is_empty() {
        return Err(BackendError::validation(format!(
            "{backend} device lacks required features {missing:?}"
        )));
    }
    if !required_limits.check_limits(supported_limits) {
        return Err(BackendError::validation(format!(
            "{backend} adapter does not satisfy required limits"
        )));
    }
    Ok(())
}

pub(crate) fn validate_buffer_size(
    size: u64,
    max_size: u64,
    backend: &str,
) -> Result<(), BackendError> {
    if size > max_size {
        return Err(BackendError::validation(format!(
            "buffer size {size} exceeds {backend} max_buffer_size {max_size}"
        )));
    }
    Ok(())
}

pub(crate) fn validate_texture_desc(
    desc: &TextureDescriptor<'_>,
    backend: &str,
) -> Result<(u32, u32), BackendError> {
    if desc.size.width == 0 || desc.size.height == 0 {
        return Err(BackendError::validation(
            "texture dimensions must be non-zero",
        ));
    }
    if desc.mip_level_count > 1 {
        return Err(BackendError::validation(format!(
            "native {backend} textures support only one mip level (mip_level_count == 1)"
        )));
    }
    Ok((
        desc.size.depth_or_array_layers.max(1),
        desc.sample_count.max(1),
    ))
}

pub(crate) fn validate_buffer_range(
    offset: u64,
    size: u64,
    buf_size: u64,
    max_end: u64,
    what: &str,
) -> Result<u64, BackendError> {
    let end = offset
        .checked_add(size)
        .ok_or_else(|| BackendError::validation(format!("{what} buffer range overflows u64")))?;
    if end > buf_size {
        return Err(BackendError::validation(format!(
            "{what} buffer range {offset}..{end} exceeds buffer size {buf_size}"
        )));
    }
    if end > max_end {
        return Err(BackendError::validation(format!(
            "{what} buffer offset exceeds backend address limit"
        )));
    }
    Ok(end)
}

pub(crate) fn validate_texture_region(
    origin: Origin3d,
    size: Extent3d,
    width: u32,
    height: u32,
    layers: u32,
) -> Result<(), BackendError> {
    let req_layers = size.depth_or_array_layers.max(1);
    if origin.x.checked_add(size.width).is_none_or(|x| x > width)
        || origin.y.checked_add(size.height).is_none_or(|y| y > height)
        || origin.z.checked_add(req_layers).is_none_or(|z| z > layers)
    {
        return Err(BackendError::validation(
            "texture copy region exceeds bounds",
        ));
    }
    Ok(())
}

pub(crate) fn validate_write_buffer_args(
    belongs: bool,
    valid: bool,
    usage: BufferUsages,
    buf_size: u64,
    offset: u64,
    data_len: u64,
) -> Result<(), BackendError> {
    if !belongs {
        return Err(BackendError::validation(
            "cannot write to a buffer from another device",
        ));
    }
    if !valid {
        return Err(BackendError::validation(
            "cannot write to an invalid buffer",
        ));
    }
    if !usage.contains(BufferUsages::COPY_DST) {
        return Err(BackendError::validation(
            "write_buffer destination lacks COPY_DST",
        ));
    }
    let end = offset
        .checked_add(data_len)
        .ok_or_else(|| BackendError::validation("write_buffer range overflows u64"))?;
    if end > buf_size {
        return Err(BackendError::validation("write_buffer exceeds buffer size"));
    }
    Ok(())
}

/// Makes `next()` the active staging chunk (cursor 0). The old chunk moves to
/// `used` (its staged writes stay valid) only once the replacement exists, so
/// a failed allocation keeps the active chunk and its cursor.
pub(crate) fn replace_staging_chunk<B, E>(
    current: &mut Option<(B, u64)>,
    used: &mut Vec<B>,
    next: impl FnOnce() -> Result<B, E>,
) -> Result<(), E> {
    let chunk = next()?;
    if let Some((old, _)) = current.replace((chunk, 0)) {
        used.push(old);
    }
    Ok(())
}

pub(crate) fn validate_write_texture_args(
    belongs: bool,
    valid: bool,
    usage: TextureUsages,
) -> Result<(), BackendError> {
    if !belongs {
        return Err(BackendError::validation(
            "cannot write to a texture from another device",
        ));
    }
    if !valid {
        return Err(BackendError::validation(
            "cannot write to an invalid texture",
        ));
    }
    if !usage.contains(TextureUsages::COPY_DST) {
        return Err(BackendError::validation(
            "write_texture destination lacks COPY_DST",
        ));
    }
    Ok(())
}

pub(crate) fn validate_submit_texture(
    belongs: bool,
    retired: bool,
    valid: bool,
    kind: &str,
) -> Result<(), BackendError> {
    if !belongs {
        return Err(BackendError::validation(format!(
            "cannot submit commands referencing a {kind} from another device"
        )));
    }
    if retired {
        return Err(BackendError::validation(
            "cannot submit commands targeting a retired swapchain image",
        ));
    }
    if !valid {
        return Err(BackendError::validation(format!(
            "cannot submit commands referencing an invalid {kind}"
        )));
    }
    Ok(())
}

pub(crate) fn validate_submit_buffer(
    belongs: bool,
    valid: bool,
    mapped_or_pending: Option<bool>,
) -> Result<(), BackendError> {
    if !belongs {
        return Err(BackendError::validation(
            "cannot submit commands referencing a buffer from another device",
        ));
    }
    if !valid {
        return Err(BackendError::validation(
            "cannot submit commands referencing an invalid buffer",
        ));
    }
    if mapped_or_pending == Some(true) {
        return Err(BackendError::validation(
            "cannot write to a buffer while it is mapped",
        ));
    }
    Ok(())
}

/// Validates `TextureToBuffer` layout/region and returns total byte span in the
/// destination buffer.
#[expect(
    clippy::too_many_arguments,
    reason = "mirrors validated TextureToBuffer fields"
)]
pub(crate) fn validate_texture_to_buffer_layout(
    origin: Origin3d,
    size: Extent3d,
    tex_dims: (u32, u32, u32),
    mip_level: u32,
    mip_count: u32,
    sample_count: u32,
    tex_usage: TextureUsages,
    block_size: u32,
    dst_usage: BufferUsages,
    dst_size: u64,
    offset: u64,
    bytes_per_row: u32,
    rows_per_image: u32,
    max_end: u64,
) -> Result<(), BackendError> {
    if mip_level >= mip_count {
        return Err(BackendError::validation(format!(
            "texture_to_buffer mip_level {mip_level} exceeds mip_level_count {mip_count}"
        )));
    }
    if sample_count > 1 {
        return Err(BackendError::validation(
            "cannot copy multisampled texture to buffer without resolving first",
        ));
    }
    if !tex_usage.contains(TextureUsages::COPY_SRC) {
        return Err(BackendError::validation(
            "texture_to_buffer source lacks COPY_SRC",
        ));
    }
    if !dst_usage.contains(BufferUsages::COPY_DST) {
        return Err(BackendError::validation(
            "texture_to_buffer destination lacks COPY_DST",
        ));
    }
    if size.width == 0 || size.height == 0 {
        return Ok(());
    }
    validate_texture_region(origin, size, tex_dims.0, tex_dims.1, tex_dims.2)?;
    let row_bytes = u64::from(size.width)
        .checked_mul(u64::from(block_size))
        .ok_or_else(|| BackendError::validation("texture_to_buffer row bytes overflow"))?;
    let bpr = u64::from(bytes_per_row);
    let rpi = u64::from(rows_per_image);
    let layers = u64::from(size.depth_or_array_layers.max(1));
    if bpr < row_bytes {
        return Err(BackendError::validation(
            "texture_to_buffer bytes_per_row is smaller than row byte width",
        ));
    }
    if layers > 1 && rpi < u64::from(size.height) {
        return Err(BackendError::validation(
            "texture_to_buffer rows_per_image is smaller than copy height",
        ));
    }
    let bpi = bpr
        .checked_mul(rpi)
        .ok_or_else(|| BackendError::validation("texture_to_buffer bytes_per_image overflows"))?;
    let last_slice_off = (layers - 1)
        .checked_mul(bpi)
        .ok_or_else(|| BackendError::validation("texture_to_buffer slice offset overflows"))?;
    let last_row_off = u64::from(size.height - 1)
        .checked_mul(bpr)
        .ok_or_else(|| BackendError::validation("texture_to_buffer row offset overflows"))?;
    let total = last_slice_off
        .checked_add(last_row_off)
        .and_then(|v| v.checked_add(row_bytes))
        .ok_or_else(|| BackendError::validation("texture_to_buffer byte range overflows"))?;
    validate_buffer_range(offset, total, dst_size, max_end, "texture_to_buffer")?;
    Ok(())
}

#[expect(
    clippy::too_many_arguments,
    reason = "mirrors validated TextureToTexture fields"
)]
pub(crate) fn validate_texture_to_texture(
    same_texture: bool,
    src_dims: (u32, u32, u32),
    src_mip: u32,
    src_mip_count: u32,
    src_samples: u32,
    src_format: TextureFormat,
    src_usage: TextureUsages,
    src_origin: Origin3d,
    dst_dims: (u32, u32, u32),
    dst_mip: u32,
    dst_mip_count: u32,
    dst_samples: u32,
    dst_format: TextureFormat,
    dst_usage: TextureUsages,
    dst_origin: Origin3d,
    size: Extent3d,
) -> Result<(), BackendError> {
    if src_mip >= src_mip_count || dst_mip >= dst_mip_count {
        return Err(BackendError::validation(
            "texture_to_texture mip level exceeds texture mip_level_count",
        ));
    }
    if !src_usage.contains(TextureUsages::COPY_SRC) {
        return Err(BackendError::validation(
            "texture_to_texture source lacks COPY_SRC",
        ));
    }
    if !dst_usage.contains(TextureUsages::COPY_DST) {
        return Err(BackendError::validation(
            "texture_to_texture destination lacks COPY_DST",
        ));
    }
    if src_format != dst_format {
        return Err(BackendError::validation(
            "texture_to_texture formats must match",
        ));
    }
    if src_samples != dst_samples {
        return Err(BackendError::validation(
            "texture_to_texture sample counts must match",
        ));
    }
    if size.width == 0 || size.height == 0 {
        return Ok(());
    }
    // WebGPU rule, and D3D11 `CopySubresourceRegion` requires distinct
    // subresources: a copy within one texture must not share a mip+layer.
    let layers = size.depth_or_array_layers.max(1);
    if same_texture
        && src_mip == dst_mip
        && src_origin.z < dst_origin.z.saturating_add(layers)
        && dst_origin.z < src_origin.z.saturating_add(layers)
    {
        return Err(BackendError::validation(
            "texture_to_texture source and destination share a subresource",
        ));
    }
    validate_texture_region(src_origin, size, src_dims.0, src_dims.1, src_dims.2)?;
    validate_texture_region(dst_origin, size, dst_dims.0, dst_dims.1, dst_dims.2)?;
    if src_samples > 1
        && (src_origin.x != 0
            || src_origin.y != 0
            || dst_origin.x != 0
            || dst_origin.y != 0
            || size.width != src_dims.0
            || size.height != src_dims.1
            || size.width != dst_dims.0
            || size.height != dst_dims.1)
    {
        return Err(BackendError::validation(
            "multisampled texture_to_texture requires whole-subresource copies with matching dimensions and zero x/y origins",
        ));
    }
    Ok(())
}

/// The `Queue::write_texture` destination facts the packer validates first.
pub(crate) struct TextureWriteTarget {
    pub belongs: bool,
    pub valid: bool,
    pub usage: TextureUsages,
    pub dims: (u32, u32, u32),
    pub block_size: u32,
}

/// Validates the destination, then packs texel rows from `Queue::write_texture`
/// into a tightly packed contiguous byte buffer for Metal / D3D11 upload. The
/// destination is checked before any allocation, so a foreign or invalid
/// texture reports `Validation`, never the packer's `OutOfMemory`.
pub(crate) fn pack_texture_write_bytes(
    dst: &TextureWriteTarget,
    mip_level: u32,
    origin: Origin3d,
    size: Extent3d,
    layout: TexelCopyBufferLayout,
    data: &[u8],
) -> Result<Option<Vec<u8>>, BackendError> {
    validate_write_texture_args(dst.belongs, dst.valid, dst.usage)?;
    let (tex_dims, block_size) = (dst.dims, dst.block_size);
    if mip_level != 0 {
        return Err(BackendError::validation(
            "native textures have one mip level",
        ));
    }
    if size.width == 0 || size.height == 0 {
        return Ok(None);
    }
    validate_texture_region(origin, size, tex_dims.0, tex_dims.1, tex_dims.2)?;
    let row = (size.width as usize)
        .checked_mul(block_size as usize)
        .ok_or_else(|| BackendError::validation("write_texture row stride overflows usize"))?;
    let bytes_per_row = layout.bytes_per_row.map_or(row, |b| b as usize);
    let rows_per_image = layout.rows_per_image.unwrap_or(size.height) as usize;
    if bytes_per_row < row || rows_per_image < size.height as usize {
        return Err(BackendError::validation(
            "write_texture layout bytes_per_row or rows_per_image is too small",
        ));
    }
    let layers = size.depth_or_array_layers.max(1) as usize;
    let len = row
        .checked_mul(size.height as usize)
        .and_then(|n| n.checked_mul(layers))
        .ok_or_else(|| BackendError::validation("write_texture byte length overflows usize"))?;
    let base_off = usize::try_from(layout.offset)
        .map_err(|_| BackendError::validation("write_texture source offset overflows usize"))?;
    let last_end = (layers - 1)
        .checked_mul(rows_per_image)
        .and_then(|r| r.checked_add(size.height as usize - 1))
        .and_then(|r| r.checked_mul(bytes_per_row))
        .and_then(|off| base_off.checked_add(off))
        .and_then(|from| from.checked_add(row))
        .ok_or_else(|| BackendError::validation("write_texture source offset overflows usize"))?;
    if last_end > data.len() {
        return Err(BackendError::validation(
            "write_texture source slice out of bounds",
        ));
    }
    // A large upload already holds its source; a second copy must fail as an error
    // in the queue's error scope, not abort the host.
    let mut packed = Vec::new();
    packed
        .try_reserve_exact(len)
        .map_err(|_| BackendError::out_of_memory("write_texture: cannot allocate packed upload"))?;
    for z in 0..layers {
        for y in 0..size.height as usize {
            let from = z
                .checked_mul(rows_per_image)
                .and_then(|r| r.checked_add(y))
                .and_then(|r| r.checked_mul(bytes_per_row))
                .and_then(|off| base_off.checked_add(off));
            let end = from.and_then(|f| f.checked_add(row));
            let (Some(from), Some(end)) = (from, end) else {
                return Err(BackendError::validation(
                    "write_texture source offset overflows usize",
                ));
            };
            if end > data.len() {
                return Err(BackendError::validation(
                    "write_texture source slice out of bounds",
                ));
            }
            packed.extend_from_slice(&data[from..end]);
        }
    }
    Ok(Some(packed))
}

pub(crate) fn resolve_dynamic_offset(
    dynamic_bindings: &[u32],
    binding: u32,
    dynamic_offsets: &[u32],
    base_offset: u64,
) -> Result<u64, BackendError> {
    let dyn_off = dynamic_bindings
        .binary_search(&binding)
        .ok()
        .and_then(|idx| dynamic_offsets.get(idx))
        .copied()
        .unwrap_or(0);
    base_offset
        .checked_add(u64::from(dyn_off))
        .ok_or_else(|| BackendError::validation("uniform buffer offset overflows u64"))
}

pub(crate) fn validate_map_async(
    valid: bool,
    usage: BufferUsages,
    buf_size: u64,
    offset: u64,
    size: u64,
) -> Result<(), crate::Error> {
    if !valid || !usage.contains(BufferUsages::MAP_READ) {
        return Err(crate::Error::Validation {
            description: "map_async requires a valid MAP_READ buffer".into(),
        });
    }
    if offset.checked_add(size).is_none_or(|end| end > buf_size) {
        return Err(crate::Error::Validation {
            description: "map_async slice exceeds buffer bounds".into(),
        });
    }
    Ok(())
}

pub(crate) fn validate_bind_group_buffer(
    belongs: bool,
    valid: bool,
    usage: BufferUsages,
    buf_size: u64,
    offset: u64,
    size: Option<u64>,
) -> Result<(), BackendError> {
    if !belongs {
        return Err(BackendError::validation(
            "bind group references a buffer from another device",
        ));
    }
    if !valid {
        return Err(BackendError::validation(
            "bind group references an invalid buffer",
        ));
    }
    if !usage.contains(BufferUsages::UNIFORM) {
        return Err(BackendError::validation(
            "bind group buffer lacks UNIFORM usage",
        ));
    }
    let end = offset
        .checked_add(size.unwrap_or(0))
        .ok_or_else(|| BackendError::validation("bind group buffer range overflows u64"))?;
    if end > buf_size {
        return Err(BackendError::validation(
            "bind group buffer range exceeds buffer size",
        ));
    }
    Ok(())
}

pub(crate) fn validate_bind_group_view(
    belongs: bool,
    retired: bool,
    valid: bool,
    has_texture_binding: bool,
) -> Result<(), BackendError> {
    if !belongs {
        return Err(BackendError::validation(
            "bind group references a texture view from another device",
        ));
    }
    if retired {
        return Err(BackendError::validation(
            "bind group references a retired swapchain image",
        ));
    }
    if !valid {
        return Err(BackendError::validation(
            "bind group references an invalid texture view",
        ));
    }
    if !has_texture_binding {
        return Err(BackendError::validation(
            "bind group texture view lacks TEXTURE_BINDING",
        ));
    }
    Ok(())
}

pub(crate) fn validate_bind_group_sampler(belongs: bool, valid: bool) -> Result<(), BackendError> {
    if !belongs {
        return Err(BackendError::validation(
            "bind group references a sampler from another device",
        ));
    }
    if !valid {
        return Err(BackendError::validation(
            "bind group references an invalid sampler",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn writable(dims: (u32, u32, u32), block_size: u32) -> TextureWriteTarget {
        TextureWriteTarget {
            belongs: true,
            valid: true,
            usage: TextureUsages::COPY_DST,
            dims,
            block_size,
        }
    }

    #[test]
    fn pack_texture_write_bytes_validates_destination_before_packing() {
        // A full 8192x8192 RGBA8 source: packing it would copy 256 MiB, so a
        // foreign, invalid or non-COPY_DST destination must be rejected first.
        let data = vec![0u8; 8192 * 8192 * 4];
        let size = Extent3d {
            width: 8192,
            height: 8192,
            depth_or_array_layers: 1,
        };
        let cases = [
            (false, true, TextureUsages::COPY_DST, "another device"),
            (true, false, TextureUsages::COPY_DST, "invalid texture"),
            (true, true, TextureUsages::TEXTURE_BINDING, "COPY_DST"),
        ];
        for (belongs, valid, usage, needle) in cases {
            let dst = TextureWriteTarget {
                belongs,
                valid,
                usage,
                ..writable((8192, 8192, 1), 4)
            };
            let err = pack_texture_write_bytes(
                &dst,
                0,
                Origin3d::ZERO,
                size,
                TexelCopyBufferLayout::default(),
                &data,
            )
            .unwrap_err();
            assert!(
                err.kind == ErrorKind::Validation && err.message.contains(needle),
                "{needle}: {err:?}"
            );
        }
    }

    #[test]
    fn buffer_map_state_machine_enforces_token_and_exclusivity() {
        let m = BufferMapState::new();
        assert!(!m.is_mapped_or_pending());
        assert!(m.lock_mapped().is_err());

        let t1 = m.next_token();
        assert!(m.try_mark_pending(t1));
        assert!(m.is_mapped_or_pending());
        assert!(!m.try_mark_pending(m.next_token()));
        // Wrong token fails without leaving Mapped.
        assert!(m.complete_pending(t1 + 99, Ok(())).is_err());
        assert!(m.complete_pending(t1, Ok(())).is_ok());
        assert!(m.lock_mapped().is_ok());
        m.unmap();
        assert!(!m.is_mapped_or_pending());
        assert!(m.lock_mapped().is_err());
    }

    #[test]
    fn device_lost_mark_publishes_before_the_callback_runs_once() {
        use std::sync::atomic::AtomicUsize;
        let lost = Arc::new(DeviceLost::new());
        let calls = Arc::new(AtomicUsize::new(0));
        let (l, c) = (lost.clone(), calls.clone());
        lost.set_callback(move |_, _| {
            // The flag is already visible when the callback runs.
            assert!(l.is_lost());
            c.fetch_add(1, Ordering::SeqCst);
        });
        assert!(lost.mark());
        assert!(lost.is_lost());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        // A racing `notify` after `mark` must not fire (or double-fire) it.
        lost.notify(DeviceLostReason::Unknown, "race".into());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(!lost.mark());
        lost.fire(DeviceLostReason::Unknown, "poll".into());
        lost.fire(DeviceLostReason::Unknown, "again".into());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn pack_texture_write_bytes_packs_strided_rows_and_rejects_bad_inputs() {
        // 2x2 RGBA8 with bytes_per_row = 12 (4 bytes padding per row)
        let src = [
            1, 2, 3, 4, 5, 6, 7, 8, 99, 99, 99, 99, 9, 10, 11, 12, 13, 14, 15, 16, 99, 99, 99, 99,
        ];
        let packed = pack_texture_write_bytes(
            &writable((2, 2, 1), 4),
            0,
            Origin3d::ZERO,
            Extent3d {
                width: 2,
                height: 2,
                depth_or_array_layers: 1,
            },
            TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(12),
                rows_per_image: Some(2),
            },
            &src,
        )
        .unwrap()
        .unwrap();
        assert_eq!(packed, (1u8..=16).collect::<Vec<_>>());

        // Mip > 0 rejected.
        assert!(
            pack_texture_write_bytes(
                &writable((2, 2, 1), 4),
                1,
                Origin3d::ZERO,
                Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1
                },
                TexelCopyBufferLayout::default(),
                &src
            )
            .is_err()
        );
        // Out of bounds rejected.
        assert!(
            pack_texture_write_bytes(
                &writable((2, 2, 1), 4),
                0,
                Origin3d { x: 2, y: 0, z: 0 },
                Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1
                },
                TexelCopyBufferLayout::default(),
                &src
            )
            .is_err()
        );
    }

    #[test]
    fn device_lost_fires_callback_at_most_once() {
        let dl = DeviceLost::new();
        let count = Arc::new(AtomicU64::new(0));
        let c = count.clone();
        dl.set_callback(move |reason, _| {
            assert_eq!(reason, DeviceLostReason::Unknown);
            c.fetch_add(1, Ordering::SeqCst);
        });
        assert!(!dl.is_lost());
        dl.notify(DeviceLostReason::Unknown, "first".into());
        dl.notify(DeviceLostReason::Destroyed, "second".into());
        assert!(dl.is_lost());
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn pack_texture_write_bytes_rejects_short_source_before_allocating() {
        // 8192x8192 RGBA8 would allocate 256 MiB if bounds were checked after allocation.
        let tiny = [0u8; 4];
        let res = pack_texture_write_bytes(
            &writable((8192, 8192, 1), 4),
            0,
            Origin3d::ZERO,
            Extent3d {
                width: 8192,
                height: 8192,
                depth_or_array_layers: 1,
            },
            TexelCopyBufferLayout::default(),
            &tiny,
        );
        assert!(res.is_err());
    }

    #[test]
    fn replacing_callbacks_drops_previous_handler_outside_mutex() {
        struct DropUncaptured(Arc<ErrorSink>);
        impl Drop for DropUncaptured {
            fn drop(&mut self) {
                self.0.report(crate::Error::Internal {
                    description: "from drop".into(),
                });
            }
        }
        struct DropLost(Arc<DeviceLost>);
        impl Drop for DropLost {
            fn drop(&mut self) {
                self.0.set_callback(|_, _| {});
            }
        }

        let sink = Arc::new(ErrorSink::new());
        let guard = DropUncaptured(sink.clone());
        sink.set_uncaptured(Arc::new(move |_| {
            let _ = &guard;
        }));
        let seen = Arc::new(AtomicU64::new(0));
        let seen_clone = seen.clone();
        sink.set_uncaptured(Arc::new(move |_| {
            seen_clone.fetch_add(1, Ordering::SeqCst);
        }));
        assert_eq!(seen.load(Ordering::SeqCst), 1);

        let dl = Arc::new(DeviceLost::new());
        let lost_guard = DropLost(dl.clone());
        dl.set_callback(move |_, _| {
            let _ = &lost_guard;
        });
        dl.set_callback(|_, _| {});
    }

    #[test]
    fn replace_staging_chunk_keeps_active_chunk_when_allocation_fails() {
        let mut current = Some(("a", 48));
        let mut used = Vec::new();
        assert_eq!(
            replace_staging_chunk(&mut current, &mut used, || Err("oom")),
            Err("oom")
        );
        assert_eq!((current, used.len()), (Some(("a", 48)), 0));
        replace_staging_chunk::<_, ()>(&mut current, &mut used, || Ok("b")).unwrap();
        assert_eq!((current, used), (Some(("b", 0)), vec!["a"]));
    }

    #[test]
    fn validate_texture_to_texture_rejects_one_shared_subresource() {
        let copy = |same, src_mip, src_z, dst_mip, dst_z| {
            validate_texture_to_texture(
                same,
                (8, 8, 4),
                src_mip,
                2,
                1,
                TextureFormat::Rgba8Unorm,
                TextureUsages::COPY_SRC | TextureUsages::COPY_DST,
                Origin3d {
                    x: 0,
                    y: 0,
                    z: src_z,
                },
                (8, 8, 4),
                dst_mip,
                2,
                1,
                TextureFormat::Rgba8Unorm,
                TextureUsages::COPY_SRC | TextureUsages::COPY_DST,
                Origin3d {
                    x: 4,
                    y: 4,
                    z: dst_z,
                },
                Extent3d {
                    width: 4,
                    height: 4,
                    depth_or_array_layers: 2,
                },
            )
        };
        // Same texture, mip and overlapping layers: rejected even without texel overlap.
        assert!(copy(true, 0, 0, 0, 1).is_err());
        // Different mip, disjoint layers, or another texture stay valid.
        assert!(copy(true, 0, 0, 1, 0).is_ok());
        assert!(copy(true, 0, 0, 0, 2).is_ok());
        assert!(copy(false, 0, 0, 0, 0).is_ok());
    }

    #[test]
    fn validate_texture_to_texture_enforces_whole_subresource_for_msaa() {
        let check = |src_dims, dst_dims, samples, src_origin, dst_origin, size| {
            validate_texture_to_texture(
                false,
                src_dims,
                0,
                1,
                samples,
                TextureFormat::Rgba8Unorm,
                TextureUsages::COPY_SRC,
                src_origin,
                dst_dims,
                0,
                1,
                samples,
                TextureFormat::Rgba8Unorm,
                TextureUsages::COPY_DST,
                dst_origin,
                size,
            )
        };

        // Whole-subresource MSAA copy at zero x/y origins (including array slice selection) succeeds.
        assert!(
            check(
                (64, 64, 2),
                (64, 64, 2),
                4,
                Origin3d { x: 0, y: 0, z: 1 },
                Origin3d { x: 0, y: 0, z: 0 },
                Extent3d {
                    width: 64,
                    height: 64,
                    depth_or_array_layers: 1,
                },
            )
            .is_ok()
        );

        // Partial MSAA copy size is rejected.
        assert!(
            check(
                (64, 64, 1),
                (64, 64, 1),
                4,
                Origin3d::ZERO,
                Origin3d::ZERO,
                Extent3d {
                    width: 16,
                    height: 16,
                    depth_or_array_layers: 1,
                },
            )
            .is_err()
        );

        // Mismatched destination dimensions for MSAA copy are rejected.
        assert!(
            check(
                (64, 64, 1),
                (128, 128, 1),
                4,
                Origin3d::ZERO,
                Origin3d::ZERO,
                Extent3d {
                    width: 64,
                    height: 64,
                    depth_or_array_layers: 1,
                },
            )
            .is_err()
        );

        // Non-zero source or destination x/y origins for MSAA copy are rejected.
        assert!(
            check(
                (64, 64, 1),
                (64, 64, 1),
                4,
                Origin3d { x: 8, y: 0, z: 0 },
                Origin3d::ZERO,
                Extent3d {
                    width: 56,
                    height: 64,
                    depth_or_array_layers: 1,
                },
            )
            .is_err()
        );
        assert!(
            check(
                (64, 64, 1),
                (64, 64, 1),
                4,
                Origin3d::ZERO,
                Origin3d { x: 0, y: 8, z: 0 },
                Extent3d {
                    width: 64,
                    height: 56,
                    depth_or_array_layers: 1,
                },
            )
            .is_err()
        );

        // Single-sampled partial copies with non-zero origins still succeed.
        assert!(
            check(
                (64, 64, 1),
                (64, 64, 1),
                1,
                Origin3d { x: 8, y: 4, z: 0 },
                Origin3d { x: 16, y: 12, z: 0 },
                Extent3d {
                    width: 16,
                    height: 16,
                    depth_or_array_layers: 1,
                },
            )
            .is_ok()
        );
    }
}
