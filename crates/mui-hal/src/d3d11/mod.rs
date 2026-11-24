//! Native Windows Direct3D 11 backend (`windows` COM bindings + `mui-present`
//! DirectComposition swapchain).
//!
//! Commands are recorded as plain data and executed on the immediate
//! `ID3D11DeviceContext` under the device's context mutex at submission, so
//! `CommandEncoder` stays `Send` and every submission retains its resources.

#![expect(
    unsafe_code,
    reason = "the D3D11 backend calls Win32 Direct3D11, DXGI, and D3DCompiler COM APIs"
)]

mod record;
mod resource;
mod surface;

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE_UNKNOWN, D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_11_1,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_CREATE_DEVICE_DEBUG, D3D11_FEATURE_D3D11_OPTIONS,
    D3D11_FEATURE_DATA_D3D11_OPTIONS, D3D11_QUERY_DESC, D3D11_QUERY_EVENT, D3D11_SDK_VERSION,
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11DeviceContext1, ID3D11Query,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, IDXGIAdapter, IDXGIAdapter1, IDXGIFactory1,
};
use windows::core::Interface;

pub use record::{Cmd, PassCmd, PassRecord};
pub use resource::*;
pub use surface::{Acquired, Surface, SurfaceTexture};

use crate::types::{
    AdapterInfo, Backend, BufferUsages, DeviceDescriptor, DeviceLostReason, DeviceType, Extent3d,
    Features, Limits, Origin3d, TextureUsages,
};

pub(crate) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// A D3D11 error's HAL category.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ErrorKind {
    Validation,
    OutOfMemory,
    Internal,
}

/// A D3D11 failure, already formatted.
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

pub(crate) fn win_err(what: &str, e: &windows::core::Error) -> Error {
    let hr = e.code().0 as u32;
    let msg = format!("{what}: {e} (0x{hr:08x})");
    // E_OUTOFMEMORY = 0x8007000E
    if hr == 0x8007_000E {
        Error::out_of_memory(msg)
    } else {
        Error::internal(msg)
    }
}

pub(crate) type MapCallback = Box<dyn FnOnce(Result<(), crate::BufferAsyncError>) + Send>;
pub(crate) type WorkCallback = Box<dyn FnOnce() + Send>;
type LostCallback = Box<dyn Fn(DeviceLostReason, String) + Send + Sync>;

/// Wrapper marking free-threaded D3D11/DXGI COM interfaces `Send + Sync`.
pub(crate) struct ComSend<T>(pub T);

// SAFETY: `ID3D11Device`, `ID3D11Buffer`, `ID3D11Texture2D`, `ID3D11*View`,
// `ID3D11*Shader`, `ID3D11*State`, `ID3D11Query`, `IDXGIFactory1`, and
// `IDXGIAdapter1` are free-threaded in D3D11/DXGI.
unsafe impl<T> Send for ComSend<T> {}
// SAFETY: as above.
unsafe impl<T> Sync for ComSend<T> {}

impl<T: Clone> Clone for ComSend<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T> std::ops::Deref for ComSend<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Wrapper marking `!Sync` D3D11 immediate-context COM interfaces
/// (`ID3D11DeviceContext`, `ID3D11DeviceContext1`) `Send` only; always held
/// inside `Mutex<ContextState>`.
pub(crate) struct ComSendOnly<T>(pub T);

// SAFETY: `ID3D11DeviceContext` / `ID3D11DeviceContext1` are externally
// synchronized by `DeviceShared::state`'s `Mutex` and never shared without it.
unsafe impl<T> Send for ComSendOnly<T> {}

impl<T> std::ops::Deref for ComSendOnly<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// A DXGI / D3D11 instance. Cheap to clone.
#[derive(Clone)]
pub struct Instance {
    factory: Arc<ComSend<IDXGIFactory1>>,
    validation: bool,
}

impl Instance {
    pub fn new(validation: bool) -> Result<Self, Error> {
        // SAFETY: standard DXGI factory creation.
        let factory: IDXGIFactory1 =
            unsafe { CreateDXGIFactory1() }.map_err(|e| win_err("CreateDXGIFactory1", &e))?;
        Ok(Self {
            factory: Arc::new(ComSend(factory)),
            validation,
        })
    }

    pub fn enumerate_adapters(&self) -> Vec<Adapter> {
        let mut out = Vec::new();
        let mut idx = 0u32;
        loop {
            // SAFETY: enumerating adapters from a live `IDXGIFactory1`.
            let Ok(adapter) = (unsafe { self.factory.EnumAdapters1(idx) }) else {
                break;
            };
            idx += 1;
            // SAFETY: querying adapter description on a live `IDXGIAdapter1`.
            let Ok(desc) = (unsafe { adapter.GetDesc1() }) else {
                continue;
            };
            let end = desc
                .Description
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(desc.Description.len());
            let name = String::from_utf16_lossy(&desc.Description[..end]);
            let is_sw = (desc.Flags & (DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32)) != 0;
            let device_type = if is_sw {
                DeviceType::Cpu
            } else if desc.DedicatedVideoMemory > 0 {
                DeviceType::DiscreteGpu
            } else {
                DeviceType::IntegratedGpu
            };
            let info = AdapterInfo {
                name,
                vendor: desc.VendorId,
                device: desc.DeviceId,
                device_type,
                driver: "D3D11".into(),
                driver_info: "SM 5.0".into(),
                backend: Backend::D3d11,
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
                max_buffer_size: 256 << 20,
            };
            out.push(Adapter {
                raw: Arc::new(ComSend(adapter)),
                info,
                limits,
                features: Features::empty(),
                validation: self.validation,
            });
        }
        out
    }
}

#[derive(Clone)]
pub struct Adapter {
    raw: Arc<ComSend<IDXGIAdapter1>>,
    info: AdapterInfo,
    limits: Limits,
    features: Features,
    validation: bool,
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
                "D3D11 device lacks required features {missing:?}"
            )));
        }
        if !desc.required_limits.check_limits(&self.limits) {
            return Err(Error::validation(
                "D3D11 adapter does not satisfy required limits",
            ));
        }
        let adapter_base: IDXGIAdapter = self
            .raw
            .cast()
            .map_err(|e| win_err("QueryInterface(IDXGIAdapter)", &e))?;
        let levels = [D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0];
        let base_flags = D3D11_CREATE_DEVICE_BGRA_SUPPORT;
        let try_create = |flags| -> Result<(ID3D11Device, ID3D11DeviceContext), Error> {
            let mut dev = None;
            let mut ctx = None;
            // SAFETY: passing a non-null `IDXGIAdapter` requires `D3D_DRIVER_TYPE_UNKNOWN`.
            unsafe {
                D3D11CreateDevice(
                    &adapter_base,
                    D3D_DRIVER_TYPE_UNKNOWN,
                    HMODULE::default(),
                    flags,
                    Some(&levels),
                    D3D11_SDK_VERSION,
                    Some(&mut dev),
                    None,
                    Some(&mut ctx),
                )
            }
            .map_err(|e| win_err("D3D11CreateDevice", &e))?;
            let dev =
                dev.ok_or_else(|| Error::internal("D3D11CreateDevice returned null device"))?;
            let ctx =
                ctx.ok_or_else(|| Error::internal("D3D11CreateDevice returned null context"))?;
            Ok((dev, ctx))
        };

        let (raw_device, raw_context) = if self.validation {
            try_create(base_flags | D3D11_CREATE_DEVICE_DEBUG)
                .or_else(|_| try_create(base_flags))?
        } else {
            try_create(base_flags)?
        };

        let context1: Option<ID3D11DeviceContext1> = raw_context.cast().ok();
        let mut opts = D3D11_FEATURE_DATA_D3D11_OPTIONS::default();
        // SAFETY: `opts` matches `D3D11_FEATURE_D3D11_OPTIONS`.
        let cb_offsetting = context1.is_some()
            && unsafe {
                raw_device.CheckFeatureSupport(
                    D3D11_FEATURE_D3D11_OPTIONS,
                    (&mut opts as *mut D3D11_FEATURE_DATA_D3D11_OPTIONS).cast::<c_void>(),
                    std::mem::size_of::<D3D11_FEATURE_DATA_D3D11_OPTIONS>() as u32,
                )
            }
            .is_ok()
            && opts.ConstantBufferOffsetting.as_bool();

        Ok(Device(Arc::new(DeviceShared {
            raw: ComSend(raw_device),
            limits: self.limits,
            features: desc.required_features & self.features,
            errors: Arc::new(crate::ErrorSink::new()),
            cb_offsetting,
            state: Mutex::new(ContextState {
                context: ComSendOnly(raw_context),
                context1: context1.map(ComSendOnly),
                staged: Vec::new(),
                inflight: Vec::new(),
                map_waiters: Vec::new(),
                work_waiters: Vec::new(),
                submitted: 0,
                completed: 0,
            }),
            lost: AtomicBool::new(false),
            on_lost: Mutex::new(None),
        })))
    }
}

struct InflightEvent {
    seq: u64,
    query: ComSend<ID3D11Query>,
    _kept: Vec<Cmd>,
}

struct MapWaiter {
    seq: u64,
    callback: MapCallback,
}

struct WorkWaiter {
    seq: u64,
    callback: WorkCallback,
}

pub(crate) struct ContextState {
    pub context: ComSendOnly<ID3D11DeviceContext>,
    pub context1: Option<ComSendOnly<ID3D11DeviceContext1>>,
    staged: Vec<Cmd>,
    inflight: Vec<InflightEvent>,
    map_waiters: Vec<MapWaiter>,
    work_waiters: Vec<WorkWaiter>,
    submitted: u64,
    completed: u64,
}

pub(crate) struct DeviceShared {
    pub raw: ComSend<ID3D11Device>,
    limits: Limits,
    features: Features,
    errors: Arc<crate::ErrorSink>,
    pub cb_offsetting: bool,
    pub state: Mutex<ContextState>,
    lost: AtomicBool,
    on_lost: Mutex<Option<LostCallback>>,
}

#[derive(Clone)]
pub struct Device(pub(crate) Arc<DeviceShared>);

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
            let mut st = lock(&self.0.state);
            st.staged.clear();
            st.inflight.clear();
            let last = st.submitted;
            st.completed = st.completed.max(last);
            (
                std::mem::take(&mut st.map_waiters),
                std::mem::take(&mut st.work_waiters),
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
                "cannot write to a buffer from another device",
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
        let mut st = lock(&self.0.state);
        if self.is_lost() {
            return Err(Error::internal("D3D11 device is lost"));
        }
        if dst.is_mapped_or_pending() {
            return Err(Error::validation("cannot write to a mapped buffer"));
        }
        st.staged.push(Cmd::WriteBuffer {
            dst: dst.clone(),
            offset,
            data: data.to_vec(),
        });
        Ok(())
    }

    pub fn stage_texture_write(
        &self,
        dst: &Texture,
        origin: Origin3d,
        size: Extent3d,
        packed_bytes: Vec<u8>,
    ) -> Result<(), Error> {
        if !dst.belongs_to(&self.0) {
            return Err(Error::validation(
                "cannot write to a texture from another device",
            ));
        }
        if !dst.is_valid() {
            return Err(Error::validation("cannot write to an invalid texture"));
        }
        if !dst.usage().contains(TextureUsages::COPY_DST) {
            return Err(Error::validation(
                "write_texture destination lacks COPY_DST",
            ));
        }
        let mut st = lock(&self.0.state);
        if self.is_lost() {
            return Err(Error::internal("D3D11 device is lost"));
        }
        if dst.is_retired_swapchain() {
            return Err(Error::validation(
                "cannot write to a retired swapchain image",
            ));
        }
        st.staged.push(Cmd::WriteTexture {
            dst: dst.clone(),
            origin,
            size,
            data: packed_bytes,
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
                let mut st = lock(&self.0.state);
                if self.is_lost() {
                    return Err((
                        Error::internal("D3D11 device is lost"),
                        cb_opt.take().expect("callback present"),
                    ));
                }
                let has_staged = st.staged.iter().any(|cmd| {
                    matches!(cmd, Cmd::WriteBuffer { dst, .. } if Arc::ptr_eq(&dst.0, &buffer.0))
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
                    let seq = st.submitted;
                    st.map_waiters.push(MapWaiter {
                        seq,
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

    /// Drops staged uploads targeting retired swapchain back buffers while
    /// preserving all unrelated staged writes.
    pub(crate) fn prune_retired_staged(&self) {
        let mut st = lock(&self.0.state);
        st.staged.retain(
            |cmd| !matches!(cmd, Cmd::WriteTexture { dst, .. } if dst.is_retired_swapchain()),
        );
    }

    pub fn on_submitted_work_done(&self, f: WorkCallback) {
        let mut st = lock(&self.0.state);
        let seq = st.submitted;
        st.work_waiters.push(WorkWaiter { seq, callback: f });
    }

    pub fn submit(&self, lists: Vec<Vec<Cmd>>) -> Result<u64, Error> {
        if self.is_lost() {
            return Err(Error::internal("D3D11 device is lost"));
        }
        // Validate caller command lists before touching `st.staged` so a failing
        // command buffer never discards earlier valid `queue.write_*` writes.
        for list in &lists {
            for cmd in list {
                record::validate_cmd(&self.0, cmd)?;
            }
        }

        let submit_res = {
            let mut st = lock(&self.0.state);
            if self.is_lost() {
                return Err(Error::internal("D3D11 device is lost"));
            }
            // Drop stale uploads to swapchain images retired by reconfigure so
            // they cannot permanently wedge subsequent valid submissions.
            st.staged.retain(
                |cmd| !matches!(cmd, Cmd::WriteTexture { dst, .. } if dst.is_retired_swapchain()),
            );
            for cmd in &st.staged {
                record::validate_cmd(&self.0, cmd)?;
            }
            if st.staged.is_empty() && lists.iter().all(Vec::is_empty) {
                Ok(st.submitted)
            } else {
                // Allocate the completion event query BEFORE taking `st.staged`
                // or executing any GPU commands so query-creation failure leaves
                // state untouched and partial execution always stays tracked.
                let qdesc = D3D11_QUERY_DESC {
                    Query: D3D11_QUERY_EVENT,
                    MiscFlags: 0,
                };
                let mut query = None;
                // SAFETY: valid event query descriptor on live device.
                unsafe { self.0.raw.CreateQuery(&qdesc, Some(&mut query)) }
                    .map_err(|e| win_err("ID3D11Device::CreateQuery(EVENT)", &e))?;
                let query = query.ok_or_else(|| Error::internal("CreateQuery returned null"))?;

                let mut cmds = std::mem::take(&mut st.staged);
                for mut list in lists {
                    cmds.append(&mut list);
                }

                let exec_res = record::execute_all(
                    &self.0.raw,
                    &st.context,
                    st.context1.as_deref(),
                    self.0.cb_offsetting,
                    &cmds,
                );

                // Always signal the completion query and retain `cmds` even if
                // `execute_all` failed partway through so any already-issued GPU
                // prefix keeps its resources alive and is never replayed.
                // SAFETY: ends the event query on the immediate context and flushes.
                unsafe {
                    st.context.End(&query);
                    st.context.Flush();
                }

                st.submitted += 1;
                let seq = st.submitted;
                st.inflight.push(InflightEvent {
                    seq,
                    query: ComSend(query),
                    _kept: cmds,
                });
                exec_res.map(|()| seq)
            }
        };
        // Non-blocking reap so completed back-buffer/staging refs do not leak
        // when callers do not call `device.poll()`.
        let _ = self.poll(false);
        submit_res
    }

    /// Unbinds OM render targets and flushes the immediate context so DXGI
    /// `ResizeBuffers` sees zero lingering references to swapchain back buffers.
    pub(crate) fn clear_render_targets_and_flush(&self) {
        let st = lock(&self.0.state);
        // SAFETY: unbinding render targets and flushing immediate context.
        unsafe {
            st.context.OMSetRenderTargets(None, None);
            st.context.Flush();
        }
    }

    pub(crate) fn flush_staged_before_present(&self) -> Result<(), Error> {
        let has_staged = !lock(&self.0.state).staged.is_empty();
        if has_staged {
            self.submit(Vec::new())?;
        }
        Ok(())
    }

    pub fn poll(&self, wait: bool) -> Result<(), Error> {
        let mut finished_maps = Vec::new();
        let mut finished_work = Vec::new();

        // Check device removal on Windows.
        // SAFETY: plain status query on live D3D11 device.
        if let Err(e) = unsafe { self.0.raw.GetDeviceRemovedReason() } {
            self.notify_lost(
                DeviceLostReason::Unknown,
                format!("ID3D11Device removed: {e}"),
            );
        }
        let mut lost = self.is_lost();
        let mut get_data_lost: Option<String> = None;

        {
            let mut st = lock(&self.0.state);
            let inflight = std::mem::take(&mut st.inflight);
            let mut remaining = Vec::new();
            for ev in inflight {
                if lost {
                    st.completed = st.completed.max(ev.seq);
                    continue;
                }
                let mut done_flag = 0u32;
                let mut ready = false;
                loop {
                    // SAFETY: polling a live `D3D11_QUERY_EVENT`.
                    // Note: `windows` maps both `S_OK` (0) and `S_FALSE` (1) to `Ok(())`;
                    // only `S_OK` writes `done_flag = 1` (`TRUE`).
                    let res = unsafe {
                        st.context.GetData(
                            &ev.query.0,
                            Some((&mut done_flag as *mut u32).cast::<c_void>()),
                            std::mem::size_of::<u32>() as u32,
                            0,
                        )
                    };
                    match res {
                        Ok(()) if done_flag != 0 => {
                            ready = true;
                            break;
                        }
                        Ok(()) => {
                            if !wait {
                                break;
                            }
                            std::thread::yield_now();
                        }
                        Err(e) => {
                            get_data_lost.get_or_insert_with(|| {
                                format!("ID3D11DeviceContext::GetData: {e}")
                            });
                            lost = true;
                            ready = true;
                            break;
                        }
                    }
                }
                if ready {
                    st.completed = st.completed.max(ev.seq);
                } else {
                    remaining.push(ev);
                }
            }
            st.inflight = remaining;

            let done = if lost { st.submitted } else { st.completed };
            let mut remain_maps = Vec::new();
            for w in st.map_waiters.drain(..) {
                if w.seq <= done {
                    finished_maps.push(w.callback);
                } else {
                    remain_maps.push(w);
                }
            }
            st.map_waiters = remain_maps;

            let mut remain_work = Vec::new();
            for w in st.work_waiters.drain(..) {
                if w.seq <= done {
                    finished_work.push(w.callback);
                } else {
                    remain_work.push(w);
                }
            }
            st.work_waiters = remain_work;
        }

        if let Some(msg) = get_data_lost {
            self.notify_lost(DeviceLostReason::Unknown, msg);
        }

        for cb in finished_maps {
            let map_result = if lost || self.is_lost() {
                Err(crate::BufferAsyncError)
            } else {
                Ok(())
            };
            cb(map_result);
        }
        for cb in finished_work {
            cb();
        }
        Ok(())
    }
}
