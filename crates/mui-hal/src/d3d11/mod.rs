//! Native Windows Direct3D 11 backend (`windows` COM bindings + `mui-present`
//! DXGI 1.2 flip-model swapchain / DirectComposition).
//!
//! Commands are recorded as plain data and executed on the immediate
//! `ID3D11DeviceContext` under `DeviceShared::state` at submission, so
//! `CommandEncoder` stays `Send` and every submission retains its resources.
//!
//! **Lock ordering invariant:** Always acquire `Surface::state` BEFORE
//! `DeviceShared::state`. Never acquire `Surface::state` while holding
//! `DeviceShared::state`. Every DXGI swapchain operation (`new`, `resize`,
//! `back_buffer`, `present`, teardown) acquires `DeviceShared::state` so DXGI's
//! internal use of the immediate context can never race with `Queue::submit`.

#![expect(
    unsafe_code,
    reason = "the D3D11 backend calls Win32 Direct3D11, DXGI, and D3DCompiler COM APIs"
)]

mod record;
mod resource;
mod surface;

use std::ffi::c_void;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE_UNKNOWN, D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_11_1,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_CREATE_DEVICE_DEBUG, D3D11_FEATURE_D3D11_OPTIONS,
    D3D11_FEATURE_DATA_D3D11_OPTIONS, D3D11_QUERY_DESC, D3D11_QUERY_EVENT, D3D11_SDK_VERSION,
    D3D11CreateDevice, ID3D11BlendState, ID3D11Buffer, ID3D11DepthStencilState, ID3D11Device,
    ID3D11DeviceContext, ID3D11DeviceContext1, ID3D11InputLayout, ID3D11PixelShader, ID3D11Query,
    ID3D11RasterizerState, ID3D11RenderTargetView, ID3D11SamplerState, ID3D11ShaderResourceView,
    ID3D11Texture2D, ID3D11VertexShader,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, IDXGIAdapter, IDXGIAdapter1, IDXGIFactory1,
};
use windows::core::Interface;

pub use crate::shared::BackendError as Error;
pub use record::{Cmd, PassCmd, PassRecord};
pub use resource::*;
pub use surface::{Acquired, Surface, SurfaceTexture};

pub(crate) use crate::shared::{
    DeviceLost, ErrorSink, MapCallback, WorkCallback, WorkWaiter, drain_ready_work, lock,
};
use crate::shared::{
    validate_device_request, validate_write_buffer_args, validate_write_texture_args,
};
use crate::types::{
    AdapterInfo, Backend, DeviceDescriptor, DeviceLostReason, DeviceType, Extent3d, Features,
    Limits, Origin3d,
};

/// Upper bound on `Device::poll(true)` event-query waits so a GPU TDR or device
/// removal can never permanently deadlock the caller while holding the mutex.
const POLL_WAIT_TIMEOUT: Duration = Duration::from_secs(5);
/// Number of fast `yield_now()` polls before backing off to 1 ms sleeps.
const POLL_YIELD_SPINS: u32 = 64;

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

mod sealed {
    pub trait Sealed {}
}

/// Sealed marker for D3D11/DXGI COM interfaces guaranteed free-threaded by
/// Microsoft Learn (safe to send and share concurrently across threads).
///
/// # Safety
/// Implementors must be free-threaded COM interfaces per Microsoft Learn's
/// Direct3D 11 / DXGI threading documentation.
pub(crate) unsafe trait FreeThreaded: sealed::Sealed {}

/// Sealed marker for COM interfaces safe to move across threads under external
/// mutual exclusion (`Send` only, NOT `Sync`).
///
/// # Safety
/// Implementors must be apartment-neutral COM interfaces safe to transfer
/// across threads when every call is externally serialized.
pub(crate) unsafe trait SendCom: sealed::Sealed {}

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
    IDXGIFactory1 => "Microsoft Learn DXGI threading: IDXGIFactory1 adapter enumeration is free-threaded.",
    IDXGIAdapter1 => "Microsoft Learn DXGI threading: IDXGIAdapter1 descriptor queries are free-threaded.",
    ID3D11Device => "Microsoft Learn D3D11 threading: ID3D11Device creation/query methods are free-threaded unless D3D11_CREATE_DEVICE_SINGLETHREADED is set.",
    ID3D11Buffer => "Microsoft Learn D3D11 threading: ID3D11Buffer resource handles are immutable free-threaded device children.",
    ID3D11Texture2D => "Microsoft Learn D3D11 threading: ID3D11Texture2D resource handles are immutable free-threaded device children.",
    ID3D11ShaderResourceView => "Microsoft Learn D3D11 threading: ID3D11ShaderResourceView is an immutable free-threaded device child.",
    ID3D11RenderTargetView => "Microsoft Learn D3D11 threading: ID3D11RenderTargetView is an immutable free-threaded device child.",
    ID3D11SamplerState => "Microsoft Learn D3D11 threading: ID3D11SamplerState is an immutable free-threaded state object.",
    ID3D11VertexShader => "Microsoft Learn D3D11 threading: ID3D11VertexShader is an immutable free-threaded shader object.",
    ID3D11PixelShader => "Microsoft Learn D3D11 threading: ID3D11PixelShader is an immutable free-threaded shader object.",
    ID3D11InputLayout => "Microsoft Learn D3D11 threading: ID3D11InputLayout is an immutable free-threaded device child.",
    ID3D11RasterizerState => "Microsoft Learn D3D11 threading: ID3D11RasterizerState is an immutable free-threaded state object.",
    ID3D11BlendState => "Microsoft Learn D3D11 threading: ID3D11BlendState is an immutable free-threaded state object.",
    ID3D11DepthStencilState => "Microsoft Learn D3D11 threading: ID3D11DepthStencilState is an immutable free-threaded state object.",
    ID3D11Query => "Microsoft Learn D3D11 threading: ID3D11Query handles are free-threaded; context End/GetData calls are serialized under DeviceShared::state.",
);

impl sealed::Sealed for ID3D11DeviceContext {}
// SAFETY: Microsoft Learn D3D11 threading: ID3D11DeviceContext is NOT free-threaded
// (`!Sync`), but is apartment-neutral and safe to transfer across threads when
// every call is serialized under `DeviceShared::state`'s `Mutex`.
unsafe impl SendCom for ID3D11DeviceContext {}

impl sealed::Sealed for ID3D11DeviceContext1 {}
// SAFETY: same as `ID3D11DeviceContext`; serialized under `DeviceShared::state`.
unsafe impl SendCom for ID3D11DeviceContext1 {}

/// Wrapper marking verified free-threaded D3D11/DXGI COM interfaces `Send + Sync`.
pub(crate) struct ComSend<T: FreeThreaded>(pub T);

// SAFETY: `T: FreeThreaded` is sealed and implemented only for COM interfaces
// verified free-threaded per Microsoft Learn.
unsafe impl<T: FreeThreaded> Send for ComSend<T> {}
// SAFETY: as above.
unsafe impl<T: FreeThreaded> Sync for ComSend<T> {}

impl<T: FreeThreaded + Clone> Clone for ComSend<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T: FreeThreaded> std::ops::Deref for ComSend<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Wrapper marking `!Sync` D3D11 immediate-context COM interfaces `Send` only;
/// always held inside `Mutex<ContextState>`.
pub(crate) struct ComSendOnly<T: SendCom>(pub T);

// SAFETY: `T: SendCom` is sealed and implemented only for COM interfaces safe
// to transfer across threads under external mutual exclusion (`Mutex<ContextState>`).
unsafe impl<T: SendCom> Send for ComSendOnly<T> {}

impl<T: SendCom> std::ops::Deref for ComSendOnly<T> {
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
        validate_device_request(
            desc.required_features,
            self.features,
            &desc.required_limits,
            &self.limits,
            "D3D11",
        )?;
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
        let has_opts = context1.is_some()
            && unsafe {
                raw_device.CheckFeatureSupport(
                    D3D11_FEATURE_D3D11_OPTIONS,
                    (&mut opts as *mut D3D11_FEATURE_DATA_D3D11_OPTIONS).cast::<c_void>(),
                    std::mem::size_of::<D3D11_FEATURE_DATA_D3D11_OPTIONS>() as u32,
                )
            }
            .is_ok();
        let cb_offsetting = has_opts && opts.ConstantBufferOffsetting.as_bool();

        Ok(Device(Arc::new(DeviceShared {
            raw: ComSend(raw_device),
            limits: self.limits,
            features: desc.required_features & self.features,
            errors: Arc::new(ErrorSink::new()),
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
            lost: DeviceLost::new(),
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

impl ContextState {
    /// Unbinds OM render targets and flushes the immediate context while the
    /// caller already holds `DeviceShared::state`.
    pub(crate) fn clear_render_targets_and_flush(&self) {
        // SAFETY: immediate context is exclusively locked by `&self`.
        unsafe {
            self.context.OMSetRenderTargets(None, None);
            self.context.Flush();
        }
    }

    /// Flushes the immediate context while the caller holds `DeviceShared::state`.
    pub(crate) fn flush(&self) {
        // SAFETY: immediate context is exclusively locked by `&self`.
        unsafe { self.context.Flush() }
    }
}

pub(crate) struct DeviceShared {
    pub raw: ComSend<ID3D11Device>,
    limits: Limits,
    features: Features,
    errors: Arc<ErrorSink>,
    pub cb_offsetting: bool,
    pub state: Mutex<ContextState>,
    lost: DeviceLost,
}

#[derive(Clone)]
pub struct Device(pub(crate) Arc<DeviceShared>);

pub struct Owner(pub Device);

impl Drop for Owner {
    fn drop(&mut self) {
        let _ = self.0.poll(true);
        // Break `staged -> Texture/Buffer -> DeviceShared -> staged` ownership cycles.
        lock(&self.0.0.state).staged.clear();
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

    /// Queries `ID3D11Device::GetDeviceRemovedReason` and triggers loss
    /// notification if hardware removal/reset occurred.
    pub(crate) fn check_device_removed(&self) -> bool {
        // SAFETY: plain free-threaded status query on live `ID3D11Device`.
        if let Err(e) = unsafe { self.0.raw.GetDeviceRemovedReason() } {
            self.notify_lost(
                DeviceLostReason::Unknown,
                format!("ID3D11Device removed: {e}"),
            );
            return true;
        }
        self.is_lost()
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
        validate_write_buffer_args(
            dst.belongs_to(&self.0),
            dst.is_valid(),
            dst.usage(),
            dst.size(),
            offset,
            data.len() as u64,
        )?;
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
        validate_write_texture_args(dst.belongs_to(&self.0), dst.is_valid(), dst.usage())?;
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
        let submit_res = {
            let mut st = lock(&self.0.state);
            if self.is_lost() {
                return Err(Error::internal("D3D11 device is lost"));
            }
            st.staged.retain(
                |cmd| !matches!(cmd, Cmd::WriteTexture { dst, .. } if dst.is_retired_swapchain()),
            );
            // Validate under the context lock: swapchain retirement is published
            // under it too (see `surface.rs`), so no back buffer can retire between
            // validation and execution.
            for cmd in st.staged.iter().chain(lists.iter().flatten()) {
                record::validate_cmd(&self.0, cmd)?;
            }
            if st.staged.is_empty() && lists.iter().all(Vec::is_empty) {
                Ok(st.submitted)
            } else {
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
        let _ = self.poll(false);
        submit_res
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
        let finished_work;

        let mut lost = self.check_device_removed();
        let mut poll_lost_msg: Option<String> = None;

        {
            let mut st = lock(&self.0.state);
            lost |= self.is_lost();
            let inflight = std::mem::take(&mut st.inflight);
            let mut remaining = Vec::new();
            let deadline = wait.then(|| Instant::now() + POLL_WAIT_TIMEOUT);

            for ev in inflight {
                if lost {
                    st.completed = st.completed.max(ev.seq);
                    continue;
                }
                let mut done_flag = 0u32;
                let mut ready = false;
                let mut spins = 0u32;
                loop {
                    // SAFETY: polling a live `D3D11_QUERY_EVENT` under `DeviceShared::state`.
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
                            // Check device removal inside the wait loop so TDR
                            // breaks out immediately instead of spinning forever.
                            // SAFETY: plain free-threaded status query on live `ID3D11Device`.
                            if let Err(e) = unsafe { self.0.raw.GetDeviceRemovedReason() } {
                                poll_lost_msg.get_or_insert_with(|| {
                                    format!("ID3D11Device removed during poll: {e}")
                                });
                                lost = true;
                                ready = true;
                                break;
                            }
                            // `destroy()` (or another poll) may set HAL loss while we
                            // hold the lock; it already notified, so just stop waiting.
                            if self.is_lost() {
                                lost = true;
                                ready = true;
                                break;
                            }
                            if deadline.is_some_and(|d| Instant::now() >= d) {
                                poll_lost_msg.get_or_insert_with(|| {
                                    format!(
                                        "D3D11 poll(wait) timed out after {}s (TDR or stalled GPU)",
                                        POLL_WAIT_TIMEOUT.as_secs()
                                    )
                                });
                                lost = true;
                                ready = true;
                                break;
                            }
                            if spins < POLL_YIELD_SPINS {
                                spins += 1;
                                std::thread::yield_now();
                            } else {
                                std::thread::sleep(Duration::from_millis(1));
                            }
                        }
                        Err(e) => {
                            poll_lost_msg.get_or_insert_with(|| {
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
            // Fold in loss published concurrently (e.g. `destroy()`), before any
            // waiter is told its work completed successfully.
            lost |= self.is_lost();

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
            finished_work = drain_ready_work(&mut st.work_waiters, done);
            // Publish loss while `st` is still held: `completed` was advanced past
            // events that never signalled, so the flag must be visible before any
            // other thread can read that `completed` and reuse "finished" memory.
            if poll_lost_msg.is_some() && !self.0.lost.mark() {
                poll_lost_msg = None;
            }
        }

        // A blocking wait that ended in loss/timeout did not complete the work: say so,
        // after the loss callback and waiter cleanup below have run.
        // Derived from the loss outcome, not from who owns the loss callback: loss seen at
        // entry or already reported by another thread still fails a blocking wait.
        let wait_err = (wait && lost).then(|| {
            Error::internal(
                poll_lost_msg
                    .clone()
                    .unwrap_or_else(|| "D3D11 device lost before the wait completed".into()),
            )
        });
        if let Some(msg) = poll_lost_msg {
            self.0.lost.fire(DeviceLostReason::Unknown, msg);
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
        wait_err.map_or(Ok(()), Err)
    }
}
