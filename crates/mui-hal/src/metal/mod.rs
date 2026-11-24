//! Native macOS Metal backend (`objc2-metal` + `objc2-quartz-core`).
//!
//! Commands are recorded as plain data and encoded into an `MTLCommandBuffer`
//! inside `objc2::rc::autoreleasepool` under the queue mutex at submission, so
//! `CommandEncoder` stays `Send` and every submission retains its referenced
//! resources until `MTLCommandBufferStatus::Completed`.

#![expect(
    unsafe_code,
    reason = "the Metal backend bridges Objective-C Metal and QuartzCore protocols via objc2"
)]

mod record;
mod resource;
mod surface;

use std::collections::{BTreeSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

// `MTLCreateSystemDefaultDevice` requires linking CoreGraphics on macOS even
// when `wgpu` is not enabled in the crate graph.
#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {}

use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::ProtocolObject;
use objc2_metal::{
    MTLBuffer as _, MTLCommandBuffer, MTLCommandBufferStatus, MTLCommandQueue,
    MTLCreateSystemDefaultDevice, MTLDevice, MTLResourceOptions,
};

pub use record::{Cmd, PassCmd, PassRecord};
pub use resource::*;
pub use surface::{Acquired, Surface, SurfaceTexture};

use crate::types::{
    AdapterInfo, Backend, BufferUsages, DeviceDescriptor, DeviceLostReason, DeviceType, Features,
    Limits,
};

/// Locks `m`, recovering across poison so internal HAL state stays consistent.
pub(crate) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Wrapper marking thread-safe Objective-C Metal protocol objects `Send + Sync`.
///
/// Apple's Metal specification guarantees that `MTLDevice`, `MTLCommandQueue`,
/// `MTLBuffer`, `MTLTexture`, `MTLSamplerState`, `MTLLibrary`, and
/// `MTLRenderPipelineState` are thread-safe to retain and reference across threads.
pub(crate) struct SendObj<T: ?Sized>(pub Retained<T>);

// SAFETY: only used for thread-safe Metal protocol objects documented above.
unsafe impl<T: ?Sized> Send for SendObj<T> {}
// SAFETY: as above.
unsafe impl<T: ?Sized> Sync for SendObj<T> {}

impl<T> Clone for SendObj<T>
where
    Retained<T>: Clone,
{
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T: ?Sized> std::ops::Deref for SendObj<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Wrapper marking single-owner or externally mutex-guarded Objective-C objects
/// (`MTLCommandBuffer`, `CAMetalLayer`, `CAMetalDrawable`) `Send` without
/// claiming `Sync`.
pub(crate) struct SendOnlyObj<T: ?Sized>(pub Retained<T>);

// SAFETY: held exclusively inside a `Mutex` or owned by a single value moved
// across threads; never accessed concurrently without external synchronization.
unsafe impl<T: ?Sized> Send for SendOnlyObj<T> {}

impl<T> Clone for SendOnlyObj<T>
where
    Retained<T>: Clone,
{
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T: ?Sized> std::ops::Deref for SendOnlyObj<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// A Metal error's HAL category.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ErrorKind {
    Validation,
    OutOfMemory,
    Internal,
}

/// A Metal failure, already formatted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    kind: ErrorKind,
    message: String,
}

impl Error {
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

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

pub(crate) type MapCallback = Box<dyn FnOnce(Result<(), crate::BufferAsyncError>) + Send>;
pub(crate) type WorkCallback = Box<dyn FnOnce() + Send>;
type LostCallback = Box<dyn Fn(DeviceLostReason, String) + Send + Sync>;

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
        let missing = desc.required_features - self.features;
        if !missing.is_empty() {
            return Err(Error::validation(format!(
                "Metal device lacks required features {missing:?}"
            )));
        }
        if !desc.required_limits.check_limits(&self.limits) {
            return Err(Error::validation(
                "Metal adapter does not satisfy required limits",
            ));
        }
        let queue = self
            .device
            .newCommandQueue()
            .ok_or_else(|| Error::internal("MTLDevice::newCommandQueue returned nil"))?;
        Ok(Device(Arc::new(DeviceShared {
            raw: self.device.clone(),
            command_queue: SendObj(queue),
            limits: self.limits,
            features: desc.required_features & self.features,
            errors: Arc::new(crate::ErrorSink::new()),
            state: Mutex::new(QueueState {
                staged: Vec::new(),
                inflight: VecDeque::new(),
                map_waiters: Vec::new(),
                work_waiters: Vec::new(),
                submitted: 0,
                completed: 0,
                retired: 0,
                failed_seqs: BTreeSet::new(),
            }),
            lost: AtomicBool::new(false),
            on_lost: Mutex::new(None),
        })))
    }
}

struct InflightSubmission {
    seq: u64,
    cmd_buf: SendOnlyObj<ProtocolObject<dyn MTLCommandBuffer>>,
    /// Retained until `cmd_buf` finishes so buffers/textures/pipelines stay live.
    _kept: Vec<Cmd>,
}

struct MapWaiter {
    seq: u64,
    since_seq: u64,
    write_seq: u64,
    callback: MapCallback,
}

struct WorkWaiter {
    seq: u64,
    callback: WorkCallback,
}

struct QueueState {
    staged: Vec<Cmd>,
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
    errors: Arc<crate::ErrorSink>,
    state: Mutex<QueueState>,
    lost: AtomicBool,
    on_lost: Mutex<Option<LostCallback>>,
}

/// A Metal device handle. Cheap to clone.
#[derive(Clone)]
pub struct Device(pub(crate) Arc<DeviceShared>);

/// Public device owner stored in `DeviceInner::Metal`; dropping it destroys the
/// device cleanly once no other clones remain.
pub struct Owner(pub Device);

impl Drop for Owner {
    fn drop(&mut self) {
        let _ = self.0.poll(true);
        // Break `staged -> Texture/Buffer -> DeviceShared -> staged` ownership cycles.
        lock(&self.0.0.state).staged.clear();
    }
}

impl Device {
    pub(crate) fn errors(&self) -> &Arc<crate::ErrorSink> {
        &self.0.errors
    }

    pub fn limits(&self) -> Limits {
        self.0.limits
    }

    pub fn features(&self) -> Features {
        self.0.features
    }

    pub fn is_lost(&self) -> bool {
        self.0.lost.load(Ordering::Acquire)
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
        *lock(&self.0.on_lost) = Some(Box::new(f));
    }

    pub(crate) fn notify_lost(&self, reason: DeviceLostReason, message: String) {
        if !self.0.lost.swap(true, Ordering::AcqRel) {
            let cb = lock(&self.0.on_lost).take();
            if let Some(cb) = cb {
                cb(reason, message);
            }
        }
    }

    pub fn destroy(&self) {
        self.notify_lost(DeviceLostReason::Destroyed, "device destroyed".into());
        let (maps, works) = {
            let mut q = lock(&self.0.state);
            q.staged.clear();
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

    pub fn stage_buffer_write(&self, dst: &Buffer, offset: u64, data: &[u8]) -> Result<(), Error> {
        if !dst.belongs_to(&self.0) {
            return Err(Error::validation(
                "write_buffer destination belongs to a different device",
            ));
        }
        if !dst.is_valid() {
            return Err(Error::validation("cannot write to an invalid buffer"));
        }
        if !dst.usage().contains(BufferUsages::COPY_DST) {
            return Err(Error::validation("write_buffer destination lacks COPY_DST"));
        }
        let end = offset
            .checked_add(data.len() as u64)
            .ok_or_else(|| Error::validation("write_buffer range overflows u64"))?;
        if end > dst.size() {
            return Err(Error::validation("write_buffer exceeds buffer size"));
        }
        if self.is_lost() {
            return Err(Error::internal("Metal device is lost"));
        }
        let staging = self.make_staging_buffer(data)?;
        let mut q = lock(&self.0.state);
        if self.is_lost() {
            return Err(Error::internal("Metal device is lost"));
        }
        if dst.is_mapped_or_pending() {
            return Err(Error::validation("cannot write to a mapped buffer"));
        }
        q.staged.push(Cmd::CopyBuffer {
            src: staging,
            src_offset: 0,
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
        if !dst.belongs_to(&self.0) {
            return Err(Error::validation(
                "write_texture destination belongs to a different device",
            ));
        }
        if !dst.is_valid() {
            return Err(Error::validation("cannot write to an invalid texture"));
        }
        if !dst.usage().contains(crate::types::TextureUsages::COPY_DST) {
            return Err(Error::validation(
                "write_texture destination lacks COPY_DST",
            ));
        }
        if self.is_lost() {
            return Err(Error::internal("Metal device is lost"));
        }
        let staging = self.make_staging_buffer(packed_bytes)?;
        let mut q = lock(&self.0.state);
        if self.is_lost() {
            return Err(Error::internal("Metal device is lost"));
        }
        if dst.is_retired_swapchain() {
            return Err(Error::validation(
                "cannot write to a retired swapchain image",
            ));
        }
        q.staged.push(Cmd::BufferToTexture {
            src: staging,
            offset: 0,
            dst: dst.clone(),
            origin,
            size,
        });
        Ok(())
    }

    fn make_staging_buffer(&self, bytes: &[u8]) -> Result<Buffer, Error> {
        let len = bytes.len().max(4);
        let raw = self
            .0
            .raw
            .newBufferWithLength_options(len, MTLResourceOptions::StorageModeShared)
            .ok_or_else(|| Error::out_of_memory("MTLDevice::newBufferWithLength returned nil"))?;
        // SAFETY: `raw` is a newly created `StorageModeShared` buffer with length >= `bytes.len()`.
        unsafe {
            std::ptr::copy_nonoverlapping(
                bytes.as_ptr(),
                raw.contents().as_ptr().cast::<u8>(),
                bytes.len(),
            );
        }
        Ok(Buffer::from_raw(
            self,
            raw,
            len as u64,
            BufferUsages::COPY_SRC,
        ))
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
    /// preserving all unrelated staged writes.
    pub(crate) fn prune_retired_staged(&self) {
        let mut q = lock(&self.0.state);
        q.staged.retain(
            |cmd| !matches!(cmd, Cmd::BufferToTexture { dst, .. } if dst.is_retired_swapchain()),
        );
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
        // Validate caller command lists before touching `q.staged` so a bad
        // command buffer never discards earlier valid `queue.write_*` calls.
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
            // Drop stale uploads to drawables retired by a surface reconfigure
            // so they can never wedge subsequent valid submissions.
            q.staged.retain(|cmd| {
                !matches!(cmd, Cmd::BufferToTexture { dst, .. } if dst.is_retired_swapchain())
            });
            for cmd in &q.staged {
                record::validate_cmd(&self.0, cmd)?;
            }
            if q.staged.is_empty() && lists.iter().all(Vec::is_empty) {
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
                        // Restore accepted staged writes in original order when
                        // encoding or buffer creation fails before `commit()`.
                        cmds.truncate(staged_len);
                        q.staged = cmds;
                        return Err(e);
                    }
                };

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
                });
                seq
            }
        };
        // Non-blocking reap so completed drawables and staging buffers are freed
        // promptly even when callers do not call `device.poll()`.
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
        let mut finished_work = Vec::new();
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
                }
            } else {
                while let Some(front) = q.inflight.front() {
                    let status = front.cmd_buf.status();
                    if status == MTLCommandBufferStatus::Completed
                        || status == MTLCommandBufferStatus::Error
                    {
                        let sub = q.inflight.pop_front().expect("checked front");
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

            // Keep `failed_seqs` bounded once older than active waiters and recent writes.
            let min_keep = q
                .map_waiters
                .iter()
                .map(|w| w.since_seq.min(w.write_seq))
                .min()
                .unwrap_or(q.retired)
                .min(q.retired.saturating_sub(256));
            q.failed_seqs.retain(|&seq| seq >= min_keep);

            let mut remain_work = Vec::new();
            for w in q.work_waiters.drain(..) {
                if w.seq <= done {
                    finished_work.push(w.callback);
                } else {
                    remain_work.push(w);
                }
            }
            q.work_waiters = remain_work;
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
