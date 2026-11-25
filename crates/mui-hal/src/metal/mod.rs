//! Native macOS Metal backend (`objc2-metal` + `objc2-quartz-core`).
//!
//! Commands are recorded as plain data and encoded into an `MTLCommandBuffer`
//! inside `objc2::rc::autoreleasepool` under the queue mutex at submission, so
//! `CommandEncoder` stays `Send` and every submission retains its referenced
//! resources until `MTLCommandBufferStatus::Completed`.

#![expect(
    unsafe_code,
    reason = "the Metal backend bridges Objective-C Metal, QuartzCore, and GCD APIs via objc2"
)]

mod record;
mod resource;
mod surface;

use std::collections::{BTreeSet, VecDeque};
use std::sync::{Arc, Mutex};

// `MTLCreateSystemDefaultDevice` requires linking CoreGraphics on macOS even
// when `wgpu` is not enabled in the crate graph.
#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {}

use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::ProtocolObject;
use objc2_metal::{
    MTLBuffer, MTLCommandBuffer, MTLCommandBufferStatus, MTLCommandQueue,
    MTLCreateSystemDefaultDevice, MTLDevice, MTLLibrary, MTLRenderPipelineState,
    MTLResourceOptions, MTLSamplerState, MTLTexture,
};
use objc2_quartz_core::{CAMetalDrawable, CAMetalLayer};

pub use crate::shared::BackendError as Error;
pub use record::{Cmd, PassCmd, PassRecord};
pub use resource::*;
pub use surface::{Acquired, Surface, SurfaceTexture};

pub(crate) use crate::shared::{
    DeviceLost, ErrorSink, MapCallback, WorkCallback, WorkWaiter, drain_ready_work, lock,
};
use crate::shared::{
    replace_staging_chunk, validate_device_request, validate_write_buffer_args,
    validate_write_texture_args,
};
use crate::types::{
    AdapterInfo, Backend, BufferUsages, DeviceDescriptor, DeviceLostReason, DeviceType, Features,
    Limits,
};

mod sealed {
    pub trait Sealed {}
}

/// Sealed marker for Objective-C Metal protocol objects guaranteed thread-safe
/// across threads by Apple's Metal framework specification.
///
/// # Safety
/// Implementors must be thread-safe for concurrent access across threads per
/// Apple's Metal documentation.
pub(crate) unsafe trait FreeThreaded: sealed::Sealed {}

/// Sealed marker for Objective-C objects safe to move across threads under
/// single ownership or external mutual exclusion (`Send` only, NOT `Sync`).
///
/// # Safety
/// Implementors must be safe to transfer across threads when access is
/// externally synchronized or uniquely owned.
pub(crate) unsafe trait SendMetal: sealed::Sealed {}

macro_rules! impl_free_threaded {
    ($($ty:ty => $doc:literal),* $(,)?) => {
        $(
            impl sealed::Sealed for $ty {}
            // SAFETY: $doc
            unsafe impl FreeThreaded for $ty {}
        )*
    };
}

impl_free_threaded!(
    ProtocolObject<dyn MTLDevice> => "Apple Metal docs: MTLDevice resource and pipeline creation methods are thread-safe.",
    ProtocolObject<dyn MTLCommandQueue> => "Apple Metal docs: MTLCommandQueue is thread-safe for creating command buffers.",
    ProtocolObject<dyn MTLBuffer> => "Apple Metal docs: MTLBuffer handles are thread-safe to retain and reference across command encoders.",
    ProtocolObject<dyn MTLTexture> => "Apple Metal docs: MTLTexture handles are thread-safe to retain and reference across command encoders.",
    ProtocolObject<dyn MTLSamplerState> => "Apple Metal docs: MTLSamplerState is an immutable thread-safe state object.",
    ProtocolObject<dyn MTLLibrary> => "Apple Metal docs: MTLLibrary is thread-safe for function lookup across threads.",
    ProtocolObject<dyn MTLRenderPipelineState> => "Apple Metal docs: MTLRenderPipelineState is an immutable thread-safe state object.",
);

impl sealed::Sealed for ProtocolObject<dyn MTLCommandBuffer> {}
// SAFETY: Apple Metal docs: an `MTLCommandBuffer` is not safe for concurrent
// encoding, but may be transferred across threads once encoded/committed or
// guarded by `DeviceShared::state`'s `Mutex`.
unsafe impl SendMetal for ProtocolObject<dyn MTLCommandBuffer> {}

impl sealed::Sealed for ProtocolObject<dyn CAMetalDrawable> {}
// SAFETY: Apple QuartzCore/Metal docs: a `CAMetalDrawable` is single-owner and
// safe to move across threads to the thread that encodes and presents it.
unsafe impl SendMetal for ProtocolObject<dyn CAMetalDrawable> {}

impl sealed::Sealed for CAMetalLayer {}
// SAFETY: `CAMetalLayer` is held inside `Surface::state` and all layer-hierarchy
// / property mutations (`configure`, `release`) are dispatched onto the AppKit
// main thread via GCD (`run_on_main_sync` / `run_on_main_async`); `nextDrawable()`
// runs under `Surface::layer_io`, which `configure()` also holds while mutating
// layer properties, so the two never overlap (see `surface.rs` module docs).
unsafe impl SendMetal for CAMetalLayer {}

/// Wrapper marking verified thread-safe Objective-C Metal protocol objects `Send + Sync`.
pub(crate) struct SendObj<T: FreeThreaded + ?Sized>(pub Retained<T>);

// SAFETY: `T: FreeThreaded` is sealed and implemented only for Metal protocol
// objects documented thread-safe by Apple.
unsafe impl<T: FreeThreaded + ?Sized> Send for SendObj<T> {}
// SAFETY: as above.
unsafe impl<T: FreeThreaded + ?Sized> Sync for SendObj<T> {}

impl<T: FreeThreaded + ?Sized> Clone for SendObj<T>
where
    Retained<T>: Clone,
{
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T: FreeThreaded + ?Sized> std::ops::Deref for SendObj<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Wrapper marking single-owner or externally mutex-guarded Objective-C objects
/// `Send` without claiming `Sync`.
pub(crate) struct SendOnlyObj<T: SendMetal + ?Sized>(pub Retained<T>);

// SAFETY: `T: SendMetal` is sealed and only held inside a `Mutex` or owned by a
// single value moved across threads; never accessed concurrently without synchronization.
unsafe impl<T: SendMetal + ?Sized> Send for SendOnlyObj<T> {}

impl<T: SendMetal + ?Sized> Clone for SendOnlyObj<T>
where
    Retained<T>: Clone,
{
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T: SendMetal + ?Sized> std::ops::Deref for SendOnlyObj<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// A Metal instance. Cheap to clone.
#[derive(Clone)]
pub struct Instance;

impl Instance {
    pub fn new() -> Result<Self, Error> {
        if MTLCreateSystemDefaultDevice().is_none() {
            return Err(Error::internal("MTLCreateSystemDefaultDevice returned nil"));
        }
        Ok(Self)
    }

    pub fn enumerate_adapters(&self) -> Vec<Adapter> {
        let Some(raw) = MTLCreateSystemDefaultDevice() else {
            return Vec::new();
        };
        let name = raw.name().to_string();
        let registry_id = raw.registryID();
        let device_type = if raw.hasUnifiedMemory() || raw.isLowPower() {
            DeviceType::IntegratedGpu
        } else {
            DeviceType::DiscreteGpu
        };
        let max_buffer_size = raw.maxBufferLength() as u64;
        let info = AdapterInfo {
            name,
            vendor: 0x106B, // Apple PCI vendor ID
            device: (registry_id & 0xFFFF_FFFF) as u32,
            device_type,
            driver: "Metal".into(),
            driver_info: "MSL 2.3".into(),
            backend: Backend::Metal,
        };
        let limits = Limits {
            max_texture_dimension_2d: 16384,
            max_texture_array_layers: 2048,
            max_bind_groups: 4,
            max_uniform_buffer_binding_size: 64 << 10,
            min_uniform_buffer_offset_alignment: 256,
            max_vertex_attributes: 16,
            max_vertex_buffer_array_stride: 2048,
            max_inter_stage_shader_variables: 16,
            max_buffer_size,
        };
        vec![Adapter {
            device: Arc::new(SendObj(raw)),
            info,
            limits,
            features: Features::empty(),
        }]
    }
}

#[derive(Clone)]
pub struct Adapter {
    device: Arc<SendObj<ProtocolObject<dyn MTLDevice>>>,
    info: AdapterInfo,
    limits: Limits,
    features: Features,
}

impl Adapter {
    pub fn get_info(&self) -> AdapterInfo {
        self.info.clone()
    }

    pub fn limits(&self) -> Limits {
        self.limits
    }

    pub fn features(&self) -> Features {
        self.features
    }

    pub fn request_device(&self, desc: &DeviceDescriptor<'_>) -> Result<Device, Error> {
        validate_device_request(
            desc.required_features,
            self.features,
            &desc.required_limits,
            &self.limits,
            "Metal",
        )?;
        let queue = self
            .device
            .newCommandQueue()
            .ok_or_else(|| Error::internal("MTLDevice::newCommandQueue returned nil"))?;
        Ok(Device(Arc::new(DeviceShared {
            raw: self.device.clone(),
            command_queue: SendObj(queue),
            limits: self.limits,
            features: desc.required_features & self.features,
            errors: Arc::new(ErrorSink::new()),
            state: Mutex::new(QueueState {
                staged: Vec::new(),
                staging: Staging::default(),
                inflight: VecDeque::new(),
                map_waiters: Vec::new(),
                work_waiters: Vec::new(),
                submitted: 0,
                completed: 0,
                retired: 0,
                failed_seqs: BTreeSet::new(),
            }),
            lost: DeviceLost::new(),
        })))
    }
}

/// Reusable `StorageModeShared` staging chunks for `write_buffer` / `write_texture`,
/// avoiding per-write `MTLBuffer` allocation.
const STAGING_CHUNK_BYTES: u64 = 1 << 20;
const MAX_RECYCLED_CHUNKS: usize = 8;

#[derive(Default)]
struct Staging {
    /// Active chunk and current 16-byte-aligned write cursor.
    current: Option<(Buffer, u64)>,
    /// Chunks filled since the previous submission.
    used: Vec<Buffer>,
    /// Completed chunks ready for reuse.
    free: Vec<Buffer>,
}

impl Staging {
    /// Returns finished chunks to the free list. Only standard-size chunks are
    /// kept, so retained staging memory is bounded by
    /// `MAX_RECYCLED_CHUNKS * STAGING_CHUNK_BYTES` (8 MiB); oversized one-off
    /// chunks for large uploads are released immediately.
    fn recycle(&mut self, chunks: Vec<Buffer>) {
        for b in chunks {
            if b.size() == STAGING_CHUNK_BYTES && self.free.len() < MAX_RECYCLED_CHUNKS {
                self.free.push(b);
            }
        }
    }

    fn staging_chunk_referenced(chunk: &Buffer, staged: &[Cmd]) -> bool {
        staged.iter().any(|cmd| match cmd {
            Cmd::CopyBuffer { src, .. } | Cmd::BufferToTexture { src, .. } => {
                Arc::ptr_eq(&src.0, &chunk.0)
            }
            _ => false,
        })
    }

    /// Reclaims any unsubmitted chunk that no surviving staged command
    /// references (e.g. after pruning uploads targeting a retired drawable or
    /// on an empty submit).
    fn reclaim_unreferenced(&mut self, staged: &[Cmd]) {
        if let Some((b, used)) = self.current.take() {
            if Self::staging_chunk_referenced(&b, staged) {
                self.current = Some((b, used));
            } else {
                self.recycle(vec![b]);
            }
        }
        let mut kept = Vec::new();
        let mut unreferenced = Vec::new();
        for b in self.used.drain(..) {
            if Self::staging_chunk_referenced(&b, staged) {
                kept.push(b);
            } else {
                unreferenced.push(b);
            }
        }
        self.used = kept;
        self.recycle(unreferenced);
    }

    fn take_for_submission(&mut self) -> Vec<Buffer> {
        let mut chunks = std::mem::take(&mut self.used);
        if let Some((b, _)) = self.current.take() {
            chunks.push(b);
        }
        chunks
    }
}

struct InflightSubmission {
    seq: u64,
    cmd_buf: SendOnlyObj<ProtocolObject<dyn MTLCommandBuffer>>,
    /// Retained until `cmd_buf` finishes so buffers/textures/pipelines stay live.
    _kept: Vec<Cmd>,
    /// Staging chunks consumed by this submission, recycled on completion.
    staging: Vec<Buffer>,
}

struct MapWaiter {
    seq: u64,
    since_seq: u64,
    write_seq: u64,
    callback: MapCallback,
}

struct QueueState {
    staged: Vec<Cmd>,
    staging: Staging,
    inflight: VecDeque<InflightSubmission>,
    map_waiters: Vec<MapWaiter>,
    work_waiters: Vec<WorkWaiter>,
    submitted: u64,
    completed: u64,
    retired: u64,
    failed_seqs: BTreeSet<u64>,
}

pub(crate) struct DeviceShared {
    pub raw: Arc<SendObj<ProtocolObject<dyn MTLDevice>>>,
    pub command_queue: SendObj<ProtocolObject<dyn MTLCommandQueue>>,
    limits: Limits,
    features: Features,
    errors: Arc<ErrorSink>,
    state: Mutex<QueueState>,
    lost: DeviceLost,
}

/// A Metal device handle. Cheap to clone.
#[derive(Clone)]
pub struct Device(pub(crate) Arc<DeviceShared>);

/// Public device owner stored in `DeviceInner::Metal`; dropping it drains work
/// and breaks internal staging reference cycles.
pub struct Owner(pub Device);

impl Drop for Owner {
    fn drop(&mut self) {
        let _ = self.0.poll(true);
        // Break `staged / staging -> Buffer/Texture -> DeviceShared` ownership cycles.
        let mut q = lock(&self.0.0.state);
        q.staged.clear();
        q.staging = Staging::default();
        drop(q);
        self.0.0.errors.clear_uncaptured();
    }
}

impl Device {
    pub(crate) fn errors(&self) -> &Arc<ErrorSink> {
        &self.0.errors
    }

    pub fn limits(&self) -> Limits {
        self.0.limits
    }

    pub fn features(&self) -> Features {
        self.0.features
    }

    pub fn is_lost(&self) -> bool {
        self.0.lost.is_lost()
    }

    pub fn submitted(&self) -> u64 {
        lock(&self.0.state).submitted
    }

    pub fn timestamp_period(&self) -> f32 {
        1.0
    }

    pub fn set_device_lost_callback(
        &self,
        f: impl Fn(DeviceLostReason, String) + Send + Sync + 'static,
    ) {
        self.0.lost.set_callback(f);
    }

    pub(crate) fn notify_lost(&self, reason: DeviceLostReason, message: String) {
        self.0.lost.notify(reason, message);
    }

    pub fn destroy(&self) {
        self.notify_lost(DeviceLostReason::Destroyed, "device destroyed".into());
        let (maps, works) = {
            let mut q = lock(&self.0.state);
            q.staged.clear();
            q.staging = Staging::default();
            q.inflight.clear();
            q.failed_seqs.clear();
            let last = q.submitted;
            q.completed = q.completed.max(last);
            q.retired = q.retired.max(last);
            (
                std::mem::take(&mut q.map_waiters),
                std::mem::take(&mut q.work_waiters),
            )
        };
        for w in maps {
            (w.callback)(Err(crate::BufferAsyncError));
        }
        for w in works {
            (w.callback)();
        }
    }

    /// Sub-allocates `bytes.len()` from the reusable `StorageModeShared` staging
    /// belt under `q`, copying `bytes` into the mapped slice and returning
    /// `(chunk, offset)`.
    fn stage_bytes_locked(&self, q: &mut QueueState, bytes: &[u8]) -> Result<(Buffer, u64), Error> {
        let len = bytes.len() as u64;
        let need = len.max(1).next_multiple_of(16);
        let fits = q
            .staging
            .current
            .as_ref()
            .is_some_and(|(b, used)| used.checked_add(need).is_some_and(|end| end <= b.size()));
        if !fits {
            let Staging {
                current,
                used,
                free,
            } = &mut q.staging;
            replace_staging_chunk::<_, Error>(current, used, || {
                if let Some(i) = free.iter().position(|b| b.size() >= need) {
                    return Ok(free.swap_remove(i));
                }
                let alloc = usize::try_from(need.max(STAGING_CHUNK_BYTES))
                    .map_err(|_| Error::out_of_memory("staging buffer size overflows usize"))?;
                let raw = self
                    .0
                    .raw
                    .newBufferWithLength_options(alloc, MTLResourceOptions::StorageModeShared)
                    .ok_or_else(|| {
                        Error::out_of_memory("MTLDevice::newBufferWithLength returned nil")
                    })?;
                Ok(Buffer::from_raw(
                    self,
                    raw,
                    alloc as u64,
                    BufferUsages::COPY_SRC,
                ))
            })?;
        }
        let Some((chunk, used)) = q.staging.current.as_mut() else {
            return Err(Error::internal("staging chunk unexpectedly absent"));
        };
        let at = *used;
        *used += need;
        if !bytes.is_empty() {
            let raw = chunk
                .0
                .raw
                .as_ref()
                .ok_or_else(|| Error::internal("staging chunk missing MTLBuffer"))?;
            // SAFETY: `chunk` is `StorageModeShared`, exclusively owned by `q.staging`
            // until submitted, and `at + bytes.len() <= chunk.size()`.
            unsafe {
                std::ptr::copy_nonoverlapping(
                    bytes.as_ptr(),
                    raw.contents().as_ptr().cast::<u8>().add(at as usize),
                    bytes.len(),
                );
            }
        }
        Ok((chunk.clone(), at))
    }

    pub fn stage_buffer_write(&self, dst: &Buffer, offset: u64, data: &[u8]) -> Result<(), Error> {
        validate_write_buffer_args(
            dst.belongs_to(&self.0),
            dst.is_valid(),
            dst.usage(),
            dst.size(),
            offset,
            data.len() as u64,
        )?;
        let mut q = lock(&self.0.state);
        if self.is_lost() {
            return Err(Error::internal("Metal device is lost"));
        }
        if dst.is_mapped_or_pending() {
            return Err(Error::validation("cannot write to a mapped buffer"));
        }
        let (staging, src_offset) = self.stage_bytes_locked(&mut q, data)?;
        q.staged.push(Cmd::CopyBuffer {
            src: staging,
            src_offset,
            dst: dst.clone(),
            dst_offset: offset,
            size: data.len() as u64,
        });
        Ok(())
    }

    pub fn stage_texture_write(
        &self,
        dst: &Texture,
        origin: crate::types::Origin3d,
        size: crate::types::Extent3d,
        packed_bytes: &[u8],
    ) -> Result<(), Error> {
        validate_write_texture_args(dst.belongs_to(&self.0), dst.is_valid(), dst.usage())?;
        let mut q = lock(&self.0.state);
        if self.is_lost() {
            return Err(Error::internal("Metal device is lost"));
        }
        if dst.is_retired_swapchain() {
            return Err(Error::validation(
                "cannot write to a retired swapchain image",
            ));
        }
        let (staging, offset) = self.stage_bytes_locked(&mut q, packed_bytes)?;
        q.staged.push(Cmd::BufferToTexture {
            src: staging,
            offset,
            dst: dst.clone(),
            origin,
            size,
        });
        Ok(())
    }

    /// Flushes any already-staged writes targeting `buffer`, then atomically
    /// transitions `buffer` to `Pending(token)` and enqueues `cb` at the
    /// resulting submission sequence under `self.0.state`.
    pub(crate) fn begin_map_for_buffer(
        &self,
        buffer: &Buffer,
        token: u64,
        cb: MapCallback,
    ) -> Result<(), (Error, MapCallback)> {
        let mut cb_opt = Some(cb);
        loop {
            let needs_flush = {
                let mut q = lock(&self.0.state);
                if self.is_lost() {
                    return Err((
                        Error::internal("Metal device is lost"),
                        cb_opt.take().expect("callback present"),
                    ));
                }
                let has_staged = q.staged.iter().any(|cmd| {
                    matches!(cmd, Cmd::CopyBuffer { dst, .. } if Arc::ptr_eq(&dst.0, &buffer.0))
                });
                if has_staged {
                    true
                } else {
                    if !buffer.try_mark_pending(token) {
                        return Err((
                            Error::validation("buffer is already mapped or pending"),
                            cb_opt.take().expect("callback present"),
                        ));
                    }
                    let seq = q.submitted;
                    let since_seq = q.retired;
                    let write_seq = buffer.last_write_seq();
                    q.map_waiters.push(MapWaiter {
                        since_seq,
                        seq,
                        write_seq,
                        callback: cb_opt.take().expect("callback present"),
                    });
                    return Ok(());
                }
            };
            if needs_flush && let Err(e) = self.submit(Vec::new()) {
                return Err((e, cb_opt.take().expect("callback present")));
            }
        }
    }

    /// Drops staged uploads targeting retired swapchain drawables while
    /// preserving all unrelated staged writes and reclaiming unreferenced
    /// staging chunks.
    pub(crate) fn prune_retired_staged(&self) {
        let mut q = lock(&self.0.state);
        q.staged.retain(
            |cmd| !matches!(cmd, Cmd::BufferToTexture { dst, .. } if dst.is_retired_swapchain()),
        );
        let QueueState {
            staged, staging, ..
        } = &mut *q;
        staging.reclaim_unreferenced(staged);
    }

    pub fn on_submitted_work_done(&self, f: WorkCallback) {
        let mut q = lock(&self.0.state);
        let seq = q.submitted;
        q.work_waiters.push(WorkWaiter { seq, callback: f });
    }

    pub fn submit(&self, lists: Vec<Vec<Cmd>>) -> Result<u64, Error> {
        if self.is_lost() {
            return Err(Error::internal("Metal device is lost"));
        }
        for list in &lists {
            for cmd in list {
                record::validate_cmd(&self.0, cmd)?;
            }
        }
        let seq = {
            let mut q = lock(&self.0.state);
            if self.is_lost() {
                return Err(Error::internal("Metal device is lost"));
            }
            q.staged.retain(|cmd| {
                !matches!(cmd, Cmd::BufferToTexture { dst, .. } if dst.is_retired_swapchain())
            });
            for cmd in &q.staged {
                record::validate_cmd(&self.0, cmd)?;
            }
            if q.staged.is_empty() && lists.iter().all(Vec::is_empty) {
                q.staging.reclaim_unreferenced(&[]);
                q.submitted
            } else {
                let staged_len = q.staged.len();
                let mut cmds = std::mem::take(&mut q.staged);
                for mut list in lists {
                    cmds.append(&mut list);
                }
                let encode_res = autoreleasepool(
                    |_| -> Result<Retained<ProtocolObject<dyn MTLCommandBuffer>>, Error> {
                        let cb = self.0.command_queue.commandBuffer().ok_or_else(|| {
                            Error::internal("MTLCommandQueue::commandBuffer returned nil")
                        })?;
                        record::encode_all(&cb, &cmds)?;
                        cb.commit();
                        Ok(cb)
                    },
                );
                let cmd_buf = match encode_res {
                    Ok(cb) => cb,
                    Err(e) => {
                        cmds.truncate(staged_len);
                        q.staged = cmds;
                        return Err(e);
                    }
                };

                let staging_chunks = q.staging.take_for_submission();
                q.submitted += 1;
                let seq = q.submitted;
                for cmd in &cmds {
                    match cmd {
                        Cmd::CopyBuffer { dst, .. } | Cmd::TextureToBuffer { dst, .. } => {
                            dst.record_write_seq(seq);
                        }
                        _ => {}
                    }
                }
                q.inflight.push_back(InflightSubmission {
                    seq,
                    cmd_buf: SendOnlyObj(cmd_buf),
                    _kept: cmds,
                    staging: staging_chunks,
                });
                seq
            }
        };
        let _ = self.poll(false);
        Ok(seq)
    }

    /// Commits any pending staged writes before presenting a drawable so the
    /// drawable reflects every submitted pass in order.
    pub(crate) fn flush_staged_before_present(&self) -> Result<(), Error> {
        let has_staged = !lock(&self.0.state).staged.is_empty();
        if has_staged {
            self.submit(Vec::new())?;
        }
        Ok(())
    }

    pub fn poll(&self, wait: bool) -> Result<(), Error> {
        let mut finished_maps: Vec<(MapCallback, Result<(), crate::BufferAsyncError>)> = Vec::new();
        let finished_work;
        let mut cb_errors = Vec::new();
        let lost = self.is_lost();

        {
            let mut q = lock(&self.0.state);
            if wait {
                while let Some(sub) = q.inflight.pop_front() {
                    sub.cmd_buf.waitUntilCompleted();
                    if sub.cmd_buf.status() == MTLCommandBufferStatus::Error {
                        let msg = sub.cmd_buf.error().map_or_else(
                            || "Metal command buffer failed".into(),
                            |e| e.to_string(),
                        );
                        cb_errors.push(Error::internal(msg));
                        q.failed_seqs.insert(sub.seq);
                    } else {
                        q.completed = q.completed.max(sub.seq);
                    }
                    q.retired = q.retired.max(sub.seq);
                    q.staging.recycle(sub.staging);
                }
            } else {
                while let Some(front) = q.inflight.front() {
                    let status = front.cmd_buf.status();
                    if status == MTLCommandBufferStatus::Completed
                        || status == MTLCommandBufferStatus::Error
                    {
                        let Some(sub) = q.inflight.pop_front() else {
                            break;
                        };
                        if status == MTLCommandBufferStatus::Error {
                            let msg = sub.cmd_buf.error().map_or_else(
                                || "Metal command buffer failed".into(),
                                |e| e.to_string(),
                            );
                            cb_errors.push(Error::internal(msg));
                            q.failed_seqs.insert(sub.seq);
                        } else {
                            q.completed = q.completed.max(sub.seq);
                        }
                        q.retired = q.retired.max(sub.seq);
                        q.staging.recycle(sub.staging);
                    } else {
                        break;
                    }
                }
            }

            let done = if lost { q.submitted } else { q.retired };
            let mut remain_maps = Vec::new();
            let map_waiters = std::mem::take(&mut q.map_waiters);
            for w in map_waiters {
                if w.seq <= done {
                    let depended_on_failure = w.write_seq > 0
                        && (q.failed_seqs.contains(&w.write_seq)
                            || q.failed_seqs
                                .range(w.since_seq.saturating_add(1)..=w.seq)
                                .next()
                                .is_some());
                    let res = if lost || depended_on_failure {
                        Err(crate::BufferAsyncError)
                    } else {
                        Ok(())
                    };
                    finished_maps.push((w.callback, res));
                } else {
                    remain_maps.push(w);
                }
            }
            q.map_waiters = remain_maps;

            let min_keep = q
                .map_waiters
                .iter()
                .map(|w| w.since_seq.min(w.write_seq))
                .min()
                .unwrap_or(q.retired)
                .min(q.retired.saturating_sub(256));
            q.failed_seqs.retain(|&seq| seq >= min_keep);

            finished_work = drain_ready_work(&mut q.work_waiters, done);
        }

        for err in cb_errors {
            self.errors().report(err.into_hal());
        }
        for (cb, res) in finished_maps {
            let map_result = if self.is_lost() {
                Err(crate::BufferAsyncError)
            } else {
                res
            };
            cb(map_result);
        }
        for cb in finished_work {
            cb();
        }
        Ok(())
    }
}
