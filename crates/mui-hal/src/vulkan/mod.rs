//! The Vulkan backend: `ash` over Vulkan 1.3 (dynamic rendering, timeline
//! semaphores). Commands are recorded as plain data and only turned into
//! Vulkan commands under the queue lock at submission, so encoders are
//! `Send` without a command pool per thread, and every submission holds
//! what it recorded until its timeline value completes.

#![expect(unsafe_code, reason = "the Vulkan backend is FFI through ash")]

mod record;
mod resource;
mod surface;

use std::collections::VecDeque;
use std::ffi::{CStr, c_char};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use ash::vk;

pub use crate::shared::BackendError as Error;
pub use record::{Cmd, PassCmd, PassRecord};
pub use resource::*;
pub use surface::{Acquired, Surface, SurfaceTexture};

pub(crate) use crate::shared::{LostCallback, MapCallback, WorkCallback as WorkDoneCallback, lock};
use crate::types::{
    AdapterInfo, Backend, DeviceDescriptor, DeviceLostReason, DeviceType, Features, Limits,
};

/// Messages the validation layer reported as errors, across all devices.
pub(crate) static VALIDATION_ERRORS: AtomicUsize = AtomicUsize::new(0);

pub(crate) fn err(what: &str, e: impl std::fmt::Display) -> Error {
    Error::internal(format!("{what}: {e}"))
}

pub(crate) fn vk_err(what: &str, e: vk::Result) -> Error {
    let message = format!("{what}: {e}");
    match e {
        vk::Result::ERROR_OUT_OF_HOST_MEMORY
        | vk::Result::ERROR_OUT_OF_DEVICE_MEMORY
        | vk::Result::ERROR_OUT_OF_POOL_MEMORY => Error::out_of_memory(message),
        _ => Error::internal(message),
    }
}

pub(crate) struct InstanceShared {
    pub entry: ash::Entry,
    pub instance: ash::Instance,
    /// Instance extensions enabled, for surfaces.
    pub extensions: Vec<&'static CStr>,
    debug: Option<(ash::ext::debug_utils::Instance, vk::DebugUtilsMessengerEXT)>,
}

impl Drop for InstanceShared {
    fn drop(&mut self) {
        // SAFETY: every child object holds an Arc to this instance, so none is alive.
        unsafe {
            if let Some((loader, messenger)) = self.debug.take() {
                loader.destroy_debug_utils_messenger(messenger, None);
            }
            self.instance.destroy_instance(None);
        }
    }
}

unsafe extern "system" fn on_message(
    severity: vk::DebugUtilsMessageSeverityFlagsEXT,
    kind: vk::DebugUtilsMessageTypeFlagsEXT,
    data: *const vk::DebugUtilsMessengerCallbackDataEXT<'_>,
    _user: *mut std::ffi::c_void,
) -> vk::Bool32 {
    // SAFETY: the layer passes a valid callback struct for the call's duration.
    let message = unsafe { data.as_ref() }
        .and_then(|d| {
            // SAFETY: `p_message` is a NUL-terminated string or null.
            unsafe { d.message_as_c_str() }
        })
        .map_or_else(String::new, |m| m.to_string_lossy().into_owned());
    // Loader chatter (GENERAL) about other layers is not ours to fail on.
    if severity.contains(vk::DebugUtilsMessageSeverityFlagsEXT::ERROR)
        && kind.contains(vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION)
    {
        VALIDATION_ERRORS.fetch_add(1, Ordering::Relaxed);
        eprintln!("mui-hal vulkan validation: {message}");
    }
    vk::FALSE
}

/// A Vulkan instance. Cheap to clone.
#[derive(Clone)]
pub struct Instance(pub(crate) Arc<InstanceShared>);

impl Instance {
    /// Loads the Vulkan loader and creates an instance with every surface
    /// extension it offers. `validation` turns on the Khronos layer when
    /// installed.
    pub fn new(validation: bool) -> Result<Self, Error> {
        const KHRONOS: &CStr = c"VK_LAYER_KHRONOS_validation";
        // SAFETY: loading the system Vulkan loader runs its initialisers only.
        let entry = unsafe { ash::Entry::load() }.map_err(|e| err("loading libvulkan", e))?;
        // SAFETY: a plain query on a loaded entry.
        let version = unsafe { entry.try_enumerate_instance_version() }
            .map_err(|e| vk_err("vkEnumerateInstanceVersion", e))?
            .unwrap_or(vk::API_VERSION_1_0);
        if version < vk::API_VERSION_1_3 {
            return Err(Error::internal("the Vulkan loader is older than 1.3"));
        }
        // SAFETY: a plain query on a loaded entry.
        let available = unsafe { entry.enumerate_instance_extension_properties(None) }
            .map_err(|e| vk_err("vkEnumerateInstanceExtensionProperties", e))?;
        let has = |name: &CStr| {
            available
                .iter()
                .any(|p| p.extension_name_as_c_str().is_ok_and(|n| n == name))
        };
        let wanted: &[&'static CStr] = &[
            ash::khr::surface::NAME,
            ash::khr::xlib_surface::NAME,
            ash::khr::xcb_surface::NAME,
            ash::khr::wayland_surface::NAME,
            ash::khr::win32_surface::NAME,
        ];
        let mut extensions: Vec<&'static CStr> =
            wanted.iter().copied().filter(|n| has(n)).collect();
        // SAFETY: a plain query on a loaded entry.
        let layers = unsafe { entry.enumerate_instance_layer_properties() }.unwrap_or_default();
        let validation = validation
            && has(ash::ext::debug_utils::NAME)
            && layers
                .iter()
                .any(|l| l.layer_name_as_c_str().is_ok_and(|n| n == KHRONOS));
        if validation {
            extensions.push(ash::ext::debug_utils::NAME);
        }
        let layer_names: Vec<*const c_char> = if validation {
            vec![KHRONOS.as_ptr()]
        } else {
            Vec::new()
        };
        let extension_names: Vec<*const c_char> = extensions.iter().map(|n| n.as_ptr()).collect();
        let app = vk::ApplicationInfo::default()
            .application_name(c"mui")
            .engine_name(c"mui-hal")
            .api_version(vk::API_VERSION_1_3);
        let info = vk::InstanceCreateInfo::default()
            .application_info(&app)
            .enabled_layer_names(&layer_names)
            .enabled_extension_names(&extension_names);
        // SAFETY: `info` and the name arrays it points at outlive the call.
        let instance = unsafe { entry.create_instance(&info, None) }
            .map_err(|e| vk_err("vkCreateInstance", e))?;
        let debug = validation.then(|| {
            let loader = ash::ext::debug_utils::Instance::new(&entry, &instance);
            let info = vk::DebugUtilsMessengerCreateInfoEXT::default()
                .message_severity(
                    vk::DebugUtilsMessageSeverityFlagsEXT::ERROR
                        | vk::DebugUtilsMessageSeverityFlagsEXT::WARNING,
                )
                .message_type(
                    vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION
                        | vk::DebugUtilsMessageTypeFlagsEXT::GENERAL,
                )
                .pfn_user_callback(Some(on_message));
            // SAFETY: the instance enabled VK_EXT_debug_utils above.
            unsafe { loader.create_debug_utils_messenger(&info, None) }
                .ok()
                .map(|m| (loader, m))
        });
        Ok(Self(Arc::new(InstanceShared {
            entry,
            instance,
            extensions,
            debug: debug.flatten(),
        })))
    }

    /// Every physical device that meets MUI's floor: Vulkan 1.3 and a
    /// graphics queue.
    pub fn enumerate_adapters(&self) -> Vec<Adapter> {
        let ash = &self.0.instance;
        // SAFETY: a plain query on a live instance.
        let Ok(devices) = (unsafe { ash.enumerate_physical_devices() }) else {
            return Vec::new();
        };
        devices
            .into_iter()
            .filter_map(|pd| Adapter::new(&self.0, pd))
            .collect()
    }
}

/// A physical device and the queue family MUI renders on.
#[derive(Clone)]
pub struct Adapter {
    instance: Arc<InstanceShared>,
    pub(crate) physical: vk::PhysicalDevice,
    family: u32,
    info: AdapterInfo,
    limits: Limits,
    features: Features,
    timestamp_period: f32,
}

impl Adapter {
    fn new(instance: &Arc<InstanceShared>, physical: vk::PhysicalDevice) -> Option<Self> {
        let ash = &instance.instance;
        let mut driver = vk::PhysicalDeviceDriverProperties::default();
        let mut props2 = vk::PhysicalDeviceProperties2::default().push_next(&mut driver);
        // SAFETY: `physical` came from this instance.
        unsafe { ash.get_physical_device_properties2(physical, &mut props2) };
        let props = props2.properties;
        if props.api_version < vk::API_VERSION_1_3 {
            return None;
        }
        let mut f12 = vk::PhysicalDeviceVulkan12Features::default();
        let mut f13 = vk::PhysicalDeviceVulkan13Features::default();
        let mut f2 = vk::PhysicalDeviceFeatures2::default()
            .push_next(&mut f12)
            .push_next(&mut f13);
        // SAFETY: `physical` came from this instance.
        unsafe { ash.get_physical_device_features2(physical, &mut f2) };
        if f12.timeline_semaphore == vk::FALSE || f13.dynamic_rendering == vk::FALSE {
            return None;
        }
        // SAFETY: `physical` came from this instance.
        let families = unsafe { ash.get_physical_device_queue_family_properties(physical) };
        let (family, props_family) = families
            .iter()
            .enumerate()
            .find(|(_, f)| f.queue_flags.contains(vk::QueueFlags::GRAPHICS))?;
        let l = props.limits;
        let mut features = Features::PIPELINE_CACHE;
        if l.timestamp_compute_and_graphics == vk::TRUE && props_family.timestamp_valid_bits > 0 {
            features |= Features::TIMESTAMP_QUERY | Features::TIMESTAMP_QUERY_INSIDE_ENCODERS;
        }
        let device_type = match props.device_type {
            vk::PhysicalDeviceType::DISCRETE_GPU => DeviceType::DiscreteGpu,
            vk::PhysicalDeviceType::INTEGRATED_GPU => DeviceType::IntegratedGpu,
            vk::PhysicalDeviceType::VIRTUAL_GPU => DeviceType::VirtualGpu,
            vk::PhysicalDeviceType::CPU => DeviceType::Cpu,
            _ => DeviceType::Other,
        };
        let text = |s: Result<&CStr, _>| {
            s.map_or_else(
                |_| String::new(),
                |c: &CStr| c.to_string_lossy().into_owned(),
            )
        };
        Some(Self {
            instance: instance.clone(),
            physical,
            family: family as u32,
            info: AdapterInfo {
                name: text(props.device_name_as_c_str()),
                vendor: props.vendor_id,
                device: props.device_id,
                device_type,
                driver: text(driver.driver_name_as_c_str()),
                driver_info: text(driver.driver_info_as_c_str()),
                backend: Backend::Vulkan,
            },
            limits: Limits {
                max_texture_dimension_2d: l.max_image_dimension2_d,
                max_texture_array_layers: l.max_image_array_layers,
                max_bind_groups: l.max_bound_descriptor_sets,
                max_uniform_buffer_binding_size: u64::from(l.max_uniform_buffer_range),
                min_uniform_buffer_offset_alignment: l.min_uniform_buffer_offset_alignment as u32,
                max_vertex_attributes: l.max_vertex_input_attributes,
                max_vertex_buffer_array_stride: l.max_vertex_input_binding_stride,
                max_inter_stage_shader_variables: l.max_vertex_output_components / 4,
                max_buffer_size: u64::from(u32::MAX),
            },
            features,
            timestamp_period: l.timestamp_period,
        })
    }

    pub fn get_info(&self) -> AdapterInfo {
        self.info.clone()
    }

    pub fn limits(&self) -> Limits {
        self.limits
    }

    pub fn features(&self) -> Features {
        self.features
    }

    /// The queue family index MUI renders and presents on.
    pub fn queue_family(&self) -> u32 {
        self.family
    }

    pub fn request_device(&self, desc: &DeviceDescriptor<'_>) -> Result<Device, Error> {
        if !self.features.contains(desc.required_features) {
            return Err(Error::validation(format!(
                "features {:?} unsupported",
                desc.required_features - self.features
            )));
        }
        if !desc.required_limits.check_limits(&self.limits) {
            return Err(Error::validation("requested limits exceed the adapter's"));
        }
        let ash = &self.instance.instance;
        // SAFETY: `physical` came from this instance.
        let available = unsafe { ash.enumerate_device_extension_properties(self.physical) }
            .map_err(|e| vk_err("vkEnumerateDeviceExtensionProperties", e))?;
        let swapchain = available
            .iter()
            .any(|p| p.extension_name_as_c_str() == Ok(ash::khr::swapchain::NAME));
        let extensions: Vec<*const c_char> = if swapchain {
            vec![ash::khr::swapchain::NAME.as_ptr()]
        } else {
            Vec::new()
        };
        let priorities = [1.0];
        let queues = [vk::DeviceQueueCreateInfo::default()
            .queue_family_index(self.family)
            .queue_priorities(&priorities)];
        let mut f12 = vk::PhysicalDeviceVulkan12Features::default().timeline_semaphore(true);
        let mut f13 = vk::PhysicalDeviceVulkan13Features::default().dynamic_rendering(true);
        let info = vk::DeviceCreateInfo::default()
            .queue_create_infos(&queues)
            .enabled_extension_names(&extensions)
            .push_next(&mut f12)
            .push_next(&mut f13);
        // SAFETY: `info` and everything it points at outlive the call; the
        // features were checked in `Adapter::new`.
        let device = unsafe { ash.create_device(self.physical, &info, None) }
            .map_err(|e| vk_err("vkCreateDevice", e))?;
        // SAFETY: the device was created with one queue in `family`.
        let queue = unsafe { device.get_device_queue(self.family, 0) };
        let mut kind = vk::SemaphoreTypeCreateInfo::default()
            .semaphore_type(vk::SemaphoreType::TIMELINE)
            .initial_value(0);
        // SAFETY: timeline semaphores were enabled above.
        let timeline = unsafe {
            device.create_semaphore(
                &vk::SemaphoreCreateInfo::default().push_next(&mut kind),
                None,
            )
        }
        .map_err(|e| {
            // SAFETY: the device is unused; nothing else owns it yet.
            unsafe { device.destroy_device(None) };
            vk_err("vkCreateSemaphore", e)
        })?;
        // SAFETY: `physical` came from this instance.
        let memory = unsafe { ash.get_physical_device_memory_properties(self.physical) };
        Ok(Device(Arc::new(DeviceShared {
            adapter: self.clone(),
            raw: device,
            swapchain,
            memory,
            timeline,
            queue: Mutex::new(QueueState {
                raw: queue,
                pending: Vec::new(),
                staging: Staging::default(),
                waits: Vec::new(),
                signals: Vec::new(),
                retained_semaphores: Vec::new(),
            }),
            state: Mutex::new(Flight::default()),
            features: desc.required_features,
            limits: desc.required_limits,
            errors: Arc::new(crate::ErrorSink::new()),
            lost: AtomicBool::new(false),
            hardware_lost: AtomicBool::new(false),
            loss: Mutex::new(None),
            on_lost: Mutex::new(None),
        })))
    }
}

/// Uploads written ahead of the next submission, from host-visible chunks.
#[derive(Default)]
pub(crate) struct Staging {
    /// The chunk being filled and its fill level.
    current: Option<(Buffer, u64)>,
    /// Chunks filled since the last submission.
    used: Vec<Buffer>,
    /// Chunks whose submissions completed.
    free: Vec<Buffer>,
}

const CHUNK: u64 = 1 << 20;

impl Staging {
    /// Keeps up to eight completed chunks for reuse.
    fn recycle(&mut self, chunks: Vec<Buffer>) {
        for b in chunks {
            if self.free.len() < 8 {
                self.free.push(b);
            }
        }
    }
}

pub(crate) struct QueueState {
    pub raw: vk::Queue,
    /// Texture initialisations and queue writes, recorded first next submit.
    pub pending: Vec<Cmd>,
    pub staging: Staging,
    /// Binary semaphores the next submission waits on (swapchain acquires).
    pub waits: Vec<vk::Semaphore>,
    /// Binary semaphores the next submission signals (presents wait on them).
    pub signals: Vec<vk::Semaphore>,
    /// Semaphores whose acquire or GPU completion was not established when a
    /// swapchain quiesced; held until device teardown instead of being
    /// destroyed while potentially still in flight.
    pub retained_semaphores: Vec<vk::Semaphore>,
}

/// A command pool and the one primary buffer recorded from it.
type Pool = (vk::CommandPool, vk::CommandBuffer);

struct InFlight {
    value: u64,
    pool: Pool,
    /// What the submission recorded, alive until it completes.
    _keep: Vec<Cmd>,
    staging: Vec<Buffer>,
}

#[derive(Default)]
struct Flight {
    submitted: u64,
    completed: u64,
    in_flight: VecDeque<InFlight>,
    pools: Vec<Pool>,
    maps: Vec<(u64, MapCallback)>,
    work_done: Vec<(u64, WorkDoneCallback)>,
    rejected_maps: Vec<MapCallback>,
}

pub(crate) struct DeviceShared {
    pub adapter: Adapter,
    pub raw: ash::Device,
    pub swapchain: bool,
    memory: vk::PhysicalDeviceMemoryProperties,
    timeline: vk::Semaphore,
    pub queue: Mutex<QueueState>,
    state: Mutex<Flight>,
    features: Features,
    limits: Limits,
    errors: Arc<crate::ErrorSink>,
    lost: AtomicBool,
    /// `true` once the Vulkan driver itself returned `VK_ERROR_DEVICE_LOST`.
    hardware_lost: AtomicBool,
    /// A loss recorded under internal locks, reported by `notify_lost`.
    loss: Mutex<Option<(DeviceLostReason, String)>>,
    on_lost: Mutex<Option<LostCallback>>,
}

impl Drop for DeviceShared {
    fn drop(&mut self) {
        // SAFETY: the last handle is gone; wait for the GPU, then free what
        // the device still owns. In-flight lists hold no `Arc<DeviceShared>`
        // by now, or this would not run.
        unsafe {
            let _ = self.raw.device_wait_idle();
            let state = self
                .state
                .get_mut()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let queue = self
                .queue
                .get_mut()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            queue.pending.clear();
            queue.staging = Staging::default();
            for s in queue.retained_semaphores.drain(..) {
                self.raw.destroy_semaphore(s, None);
            }
            for f in state.in_flight.drain(..) {
                self.raw.destroy_command_pool(f.pool.0, None);
            }
            for (pool, _) in state.pools.drain(..) {
                self.raw.destroy_command_pool(pool, None);
            }
            self.raw.destroy_semaphore(self.timeline, None);
            self.raw.destroy_device(None);
        }
    }
}

/// What the public device and queue handles share. Pending writes, staging
/// and in-flight lists hold resources, and resources hold the device, so
/// dropping the last owner waits for the GPU and clears them.
pub struct Owner(pub Device);

impl Drop for Owner {
    fn drop(&mut self) {
        let d = &(self.0).0;
        // SAFETY: a plain wait on a live device.
        let _ = unsafe { d.raw.device_wait_idle() };
        let mut state = lock(&d.state);
        let maps = std::mem::take(&mut state.maps);
        let rejected_maps = std::mem::take(&mut state.rejected_maps);
        let work_done = std::mem::take(&mut state.work_done);
        for f in state.in_flight.drain(..) {
            // SAFETY: the device is idle, so the pool's work completed.
            unsafe { d.raw.destroy_command_pool(f.pool.0, None) };
        }
        drop(state);
        let mut q = lock(&d.queue);
        q.pending.clear();
        q.staging = Staging::default();
        drop(q);
        // No device is left to complete them.
        for f in maps.into_iter().map(|(_, f)| f).chain(rejected_maps) {
            f(Err(crate::BufferAsyncError));
        }
        for (_, f) in work_done {
            f();
        }
        // Resource captures point back to this device. Release the handler
        // after the final callbacks, with no internal lock held during drop.
        d.errors.clear_uncaptured();
    }
}

/// A logical device with one graphics queue. Cheap to clone; also serves as
/// the queue.
#[derive(Clone)]
pub struct Device(pub(crate) Arc<DeviceShared>);

impl Device {
    pub fn adapter(&self) -> &Adapter {
        &self.0.adapter
    }

    pub fn features(&self) -> Features {
        self.0.features
    }

    pub fn limits(&self) -> Limits {
        self.0.limits
    }

    pub fn raw(&self) -> &ash::Device {
        &self.0.raw
    }

    pub(crate) fn errors(&self) -> &Arc<crate::ErrorSink> {
        &self.0.errors
    }

    #[cfg(test)]
    pub(crate) fn retained_semaphore_count(&self) -> usize {
        lock(&self.0.queue).retained_semaphores.len()
    }

    #[cfg(test)]
    pub(crate) fn retain_semaphore_for_test(&self, semaphore: vk::Semaphore) {
        lock(&self.0.queue).retained_semaphores.push(semaphore);
    }

    pub fn set_device_lost_callback(
        &self,
        f: impl Fn(DeviceLostReason, String) + Send + Sync + 'static,
    ) {
        *lock(&self.0.on_lost) = Some(Box::new(f));
    }

    /// Records the first loss; safe under internal locks. `notify_lost`
    /// reports it once they are released.
    fn lose(&self, reason: DeviceLostReason, why: String) {
        // Under `loss`, so a concurrent `notify_lost` sees the reason with
        // the flag.
        let mut loss = lock(&self.0.loss);
        if !self.0.lost.swap(true, Ordering::AcqRel) {
            *loss = Some((reason, why));
        }
    }

    /// Records a hardware `VK_ERROR_DEVICE_LOST` under internal locks; call
    /// [`Self::notify_lost`] once all queue and surface locks are released.
    pub(crate) fn record_hardware_loss(&self, why: &str) {
        self.0.hardware_lost.store(true, Ordering::Release);
        self.lose(DeviceLostReason::Unknown, why.into());
    }

    /// Reports a recorded loss: fails pending maps, then runs the loss
    /// callback. Call with no internal lock held: callbacks may re-enter.
    pub(crate) fn notify_lost(&self) {
        if !self.is_lost() {
            return;
        }
        let maps = std::mem::take(&mut lock(&self.0.state).maps);
        for (_, f) in maps {
            f(Err(crate::BufferAsyncError));
        }
        let taken = lock(&self.0.loss).take();
        let Some((reason, why)) = taken else {
            return;
        };
        let callback = lock(&self.0.on_lost).take();
        if let Some(f) = callback {
            f(reason, why);
        }
    }

    /// Refuses further work and reports the loss as `Destroyed`.
    pub fn destroy(&self) {
        self.lose(DeviceLostReason::Destroyed, "device destroyed".into());
        self.notify_lost();
    }

    pub fn is_lost(&self) -> bool {
        self.0.lost.load(Ordering::Acquire)
    }

    /// Records `cmd` ahead of the next submission.
    pub(crate) fn pend(&self, cmd: Cmd) {
        lock(&self.0.queue).pending.push(cmd);
    }

    /// `len` bytes of staging memory, written by `fill`, then `copy` given
    /// the chunk and offset to record ahead of the next submission.
    pub(crate) fn stage(
        &self,
        len: u64,
        fill: impl FnOnce(&mut [u8]),
        copy: impl FnOnce(Buffer, u64) -> Cmd,
    ) -> Result<(), Error> {
        let mut q = lock(&self.0.queue);
        self.stage_locked(&mut q, len, fill, copy)
    }

    /// Stages a `Queue::write_buffer` copy under the queue lock, rejecting
    /// writes to mapped/pending buffers atomically against `register_buffer_map`.
    pub(crate) fn stage_buffer_write(
        &self,
        dst: &Buffer,
        offset: u64,
        data: &[u8],
    ) -> Result<(), Error> {
        if !dst.is_valid() {
            return Err(Error::validation("cannot write to an invalid buffer"));
        }
        let len = data.len() as u64;
        if offset.checked_add(len).is_none_or(|end| end > dst.size()) {
            return Err(Error::validation("write_buffer range exceeds buffer size"));
        }
        let mut q = lock(&self.0.queue);
        if dst.is_mapped_or_pending() {
            return Err(Error::validation(
                "cannot write to a buffer while it is mapped",
            ));
        }
        let dst = dst.clone();
        self.stage_locked(
            &mut q,
            len,
            |m| m.copy_from_slice(data),
            |src, src_offset| Cmd::CopyBuffer {
                src,
                src_offset,
                dst,
                dst_offset: offset,
                size: len,
            },
        )
    }

    fn stage_locked(
        &self,
        q: &mut QueueState,
        len: u64,
        fill: impl FnOnce(&mut [u8]),
        copy: impl FnOnce(Buffer, u64) -> Cmd,
    ) -> Result<(), Error> {
        let staging = &mut q.staging;
        let need = len.max(1).next_multiple_of(16);
        let fits = staging
            .current
            .as_ref()
            .is_some_and(|(b, used)| used + need <= b.size());
        if !fits {
            if let Some((b, _)) = staging.current.take() {
                staging.used.push(b);
            }
            let chunk = match staging.free.iter().position(|b| b.size() >= need) {
                Some(i) => staging.free.swap_remove(i),
                None => Buffer::staging(self, need.max(CHUNK))?,
            };
            staging.current = Some((chunk, 0));
        }
        let Some((chunk, used)) = staging.current.as_mut() else {
            unreachable!("a chunk was just made current")
        };
        let at = *used;
        *used += need;
        fill(&mut chunk.mapped_mut()[at as usize..(at + len) as usize]);
        let cmd = copy(chunk.clone(), at);
        q.pending.push(cmd);
        Ok(())
    }

    /// Submits `lists` after the pending writes; returns the submission's
    /// timeline value.
    pub fn submit(&self, lists: Vec<Vec<Cmd>>) -> Result<u64, Error> {
        let result = self.submit_locked(lists);
        self.notify_lost();
        result
    }

    /// A pooled command pool and its buffer, or a new pair.
    fn take_pool(&self, state: &mut Flight) -> Result<Pool, Error> {
        if let Some(pool) = state.pools.pop() {
            return Ok(pool);
        }
        let d = &self.0;
        let info = vk::CommandPoolCreateInfo::default()
            .flags(vk::CommandPoolCreateFlags::TRANSIENT)
            .queue_family_index(d.adapter.family);
        // SAFETY: a plain object creation on a live device.
        let pool = unsafe { d.raw.create_command_pool(&info, None) }
            .map_err(|e| vk_err("vkCreateCommandPool", e))?;
        let info = vk::CommandBufferAllocateInfo::default()
            .command_pool(pool)
            .level(vk::CommandBufferLevel::PRIMARY)
            .command_buffer_count(1);
        // SAFETY: `pool` was just made; nothing else records into it.
        match unsafe { d.raw.allocate_command_buffers(&info) } {
            Ok(cbs) => Ok((pool, cbs[0])),
            Err(e) => {
                // SAFETY: the pool is unused.
                unsafe { d.raw.destroy_command_pool(pool, None) };
                Err(vk_err("vkAllocateCommandBuffers", e))
            }
        }
    }

    /// Resets an idle pool (its buffer too) and returns it to the free list;
    /// a pool that fails to reset is destroyed instead.
    fn recycle_pool(&self, state: &mut Flight, pool: Pool) {
        let raw = &self.0.raw;
        // SAFETY: no pending submission uses the pool.
        match unsafe { raw.reset_command_pool(pool.0, vk::CommandPoolResetFlags::empty()) } {
            Ok(()) => state.pools.push(pool),
            // SAFETY: as above; destroying it frees its buffer.
            Err(_) => unsafe { raw.destroy_command_pool(pool.0, None) },
        }
    }

    /// Submits a bare wait-only batch for `waits` while `q` is already held,
    /// used by `Swap::quiesce` when a full flush fails so WSI acquire signals
    /// are still consumed before `vkDeviceWaitIdle`.
    pub(crate) fn submit_waits_locked(
        &self,
        q: &QueueState,
        waits: &[vk::Semaphore],
    ) -> Result<(), Error> {
        if self.0.hardware_lost.load(Ordering::Acquire) {
            return Err(Error::internal("the device is lost"));
        }
        let wait_stages = vec![vk::PipelineStageFlags::ALL_COMMANDS; waits.len()];
        let submit = vk::SubmitInfo::default()
            .wait_semaphores(waits)
            .wait_dst_stage_mask(&wait_stages);
        // SAFETY: the queue is held through `q`, and `waits` are live binary
        // semaphores with pending WSI acquire signals.
        unsafe { self.0.raw.queue_submit(q.raw, &[submit], vk::Fence::null()) }.map_err(|e| {
            if e == vk::Result::ERROR_DEVICE_LOST {
                self.record_hardware_loss("vkQueueSubmit: device lost");
            }
            vk_err("vkQueueSubmit", e)
        })
    }

    /// `submit` without dispatching loss callbacks: for callers holding
    /// their own locks (surfaces). The next `poll` or `submit` reports.
    pub(crate) fn submit_locked(&self, lists: Vec<Vec<Cmd>>) -> Result<u64, Error> {
        self.submit_with_queue(&mut lock(&self.0.queue), lists)
    }

    fn submit_with_queue(&self, q: &mut QueueState, lists: Vec<Vec<Cmd>>) -> Result<u64, Error> {
        let d = &self.0;
        let mut state = lock(&d.state);
        if self.is_lost() {
            return Err(Error::internal("the device is lost"));
        }
        // Validate every pending and submitted command under both `queue` and
        // `state` locks so map registration (`register_buffer_map`) cannot
        // interleave between validation and advancing `state.submitted`.
        for cmd in q.pending.iter().chain(lists.iter().flatten()) {
            record::validate_cmd(cmd)?;
        }
        let freed = self.retire(&mut state);
        q.staging.recycle(freed);
        let pool = self.take_pool(&mut state)?;
        let cb = pool.1;
        let begin = vk::CommandBufferBeginInfo::default()
            .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
        // SAFETY: `cb`'s pool is reset and held only here.
        if let Err(e) = unsafe { d.raw.begin_command_buffer(cb, &begin) } {
            self.recycle_pool(&mut state, pool);
            return Err(vk_err("vkBeginCommandBuffer", e));
        }
        // Pending writes stay queued until a submission carrying them succeeds.
        let pending = q.pending.len();
        let mut keep = std::mem::take(&mut q.pending);
        for list in lists {
            keep.extend(list);
        }
        let mut recorder = record::Recorder::new(&d.raw, cb);
        for cmd in &keep {
            recorder.record(cmd);
        }
        recorder.finish();
        // SAFETY: recording into `cb` is complete.
        if let Err(e) = unsafe { d.raw.end_command_buffer(cb) } {
            keep.truncate(pending);
            q.pending = keep;
            self.recycle_pool(&mut state, pool);
            return Err(vk_err("vkEndCommandBuffer", e));
        }
        let value = state.submitted + 1;
        let mut staging: Vec<Buffer> = std::mem::take(&mut q.staging.used);
        if let Some((b, _)) = q.staging.current.take() {
            staging.push(b);
        }
        let waits = std::mem::take(&mut q.waits);
        let mut signals = std::mem::take(&mut q.signals);
        let wait_stages = vec![vk::PipelineStageFlags::ALL_COMMANDS; waits.len()];
        let mut wait_values = vec![0; waits.len()];
        let mut signal_values = vec![0; signals.len()];
        signals.push(d.timeline);
        signal_values.push(value);
        wait_values.truncate(waits.len());
        let mut timeline = vk::TimelineSemaphoreSubmitInfo::default()
            .wait_semaphore_values(&wait_values)
            .signal_semaphore_values(&signal_values);
        let cbs = [cb];
        let submit = vk::SubmitInfo::default()
            .wait_semaphores(&waits)
            .wait_dst_stage_mask(&wait_stages)
            .command_buffers(&cbs)
            .signal_semaphores(&signals)
            .push_next(&mut timeline);
        // SAFETY: the queue is held through `q`; every handle `cb` references
        // stays alive in `keep` until the timeline reaches `value`.
        let result = unsafe { d.raw.queue_submit(q.raw, &[submit], vk::Fence::null()) };
        if let Err(e) = result {
            // The submission never started: requeue its pending writes,
            // staging and semaphores for the next one.
            keep.truncate(pending);
            q.pending = keep;
            signals.pop();
            q.waits = waits;
            q.signals = signals;
            q.staging.used = staging;
            self.recycle_pool(&mut state, pool);
            if e == vk::Result::ERROR_DEVICE_LOST {
                self.record_hardware_loss("vkQueueSubmit: device lost");
            }
            return Err(vk_err("vkQueueSubmit", e));
        }
        state.submitted = value;
        state.in_flight.push_back(InFlight {
            value,
            pool,
            _keep: keep,
            staging,
        });
        Ok(value)
    }

    /// The last timeline value the GPU completed, as last seen.
    pub fn completed(&self) -> u64 {
        let mut state = lock(&self.0.state);
        let freed = self.retire(&mut state);
        let completed = state.completed;
        drop(state);
        lock(&self.0.queue).staging.recycle(freed);
        completed
    }

    /// Blocks until the GPU completes timeline `value`.
    pub fn wait_for(&self, value: u64) -> Result<(), Error> {
        let semaphores = [self.0.timeline];
        let values = [value];
        let info = vk::SemaphoreWaitInfo::default()
            .semaphores(&semaphores)
            .values(&values);
        // SAFETY: the timeline semaphore is owned by this device.
        unsafe { self.0.raw.wait_semaphores(&info, 5_000_000_000) }.map_err(|e| {
            if e == vk::Result::ERROR_DEVICE_LOST {
                self.record_hardware_loss("vkWaitSemaphores: device lost");
            }
            vk_err("vkWaitSemaphores", e)
        })
    }

    /// Runs `f` on the raw queue, holding it.
    pub(crate) fn with_queue<R>(&self, f: impl FnOnce(vk::Queue) -> R) -> R {
        let q = lock(&self.0.queue);
        f(q.raw)
    }

    /// The last timeline value submitted.
    pub fn submitted(&self) -> u64 {
        lock(&self.0.state).submitted
    }

    /// Frees what completed submissions held and runs map callbacks whose
    /// work is done. `wait` blocks until everything submitted completes.
    pub fn poll(&self, wait: bool) -> Result<(), Error> {
        let result = self.poll_inner(wait);
        self.notify_lost();
        result
    }

    fn poll_inner(&self, wait: bool) -> Result<(), Error> {
        let d = &self.0;
        let wait_result = if wait && !self.is_lost() {
            let target = lock(&d.state).submitted;
            self.wait_for(target)
        } else {
            Ok(())
        };
        if wait_result.is_err() && !self.is_lost() {
            return wait_result;
        }
        let (ready_maps, rejected_maps, ready_work, freed) = {
            let mut state = lock(&d.state);
            let freed = self.retire(&mut state);
            let completed = state.completed;
            let lost = self.is_lost();
            let (ready_maps, waiting_maps) = std::mem::take(&mut state.maps)
                .into_iter()
                .partition(|(v, _)| *v <= completed);
            state.maps = waiting_maps;
            let (ready_work, waiting_work) = std::mem::take(&mut state.work_done)
                .into_iter()
                .partition(|(v, _)| lost || *v <= completed);
            state.work_done = waiting_work;
            (
                ready_maps,
                std::mem::take(&mut state.rejected_maps),
                ready_work,
                freed,
            )
        };
        // Queue after state, never both: `submit` takes them the other way.
        lock(&d.queue).staging.recycle(freed);
        // Outside the lock: a callback may submit or map again.
        for (_, f) in ready_maps {
            if self.is_lost() {
                f(Err(crate::BufferAsyncError));
            } else {
                f(Ok(()));
            }
        }
        for f in rejected_maps {
            f(Err(crate::BufferAsyncError));
        }
        for (_, f) in ready_work {
            f();
        }
        wait_result
    }

    pub(crate) fn defer_rejected_map(&self, callback: MapCallback) {
        lock(&self.0.state).rejected_maps.push(callback);
    }

    /// Registers a `Queue::on_submitted_work_done` callback; always dispatched
    /// from [`Device::poll`] (never inline during registration, even if lost).
    pub(crate) fn on_submitted_work_done(&self, f: WorkDoneCallback) {
        let mut state = lock(&self.0.state);
        let at = state.submitted;
        state.work_done.push((at, f));
    }

    /// Serializes buffer map registration with `submit_locked` under the same
    /// `queue` -> `state` -> `map_state` lock hierarchy:
    /// - Any staged `Queue::write_buffer` targeting `buffer` in `q.pending` is
    ///   flushed first so the map waits for it (and cannot poison later
    ///   unrelated submissions).
    /// - Checking `is_lost`, transitioning `buffer` to `Pending`, and reading
    ///   `state.submitted` happen atomically under `queue` + `state` locks.
    pub(crate) fn register_buffer_map(
        &self,
        buffer: &Buffer,
        begin_pending: impl Fn(&Buffer) -> Result<u64, crate::Error>,
        make_callback: impl FnOnce(u64) -> MapCallback,
    ) -> Result<(), Option<crate::Error>> {
        let mut q = lock(&self.0.queue);
        if q.pending
            .iter()
            .any(|c| matches!(c, Cmd::CopyBuffer { dst, .. } if dst.is_same(buffer)))
        {
            self.submit_with_queue(&mut q, Vec::new())
                .map_err(|e| Some(e.into_hal()))?;
        }
        let mut state = lock(&self.0.state);
        if self.is_lost() {
            return Err(None);
        }
        let token = begin_pending(buffer).map_err(Some)?;
        let at = state.submitted;
        state.maps.push((at, make_callback(token)));
        Ok(())
    }

    /// Retires completed submissions; returns their staging chunks.
    fn retire(&self, state: &mut Flight) -> Vec<Buffer> {
        let mut freed = Vec::new();
        let d = &self.0;
        // SAFETY: the timeline semaphore is owned by this device.
        match unsafe { d.raw.get_semaphore_counter_value(d.timeline) } {
            Ok(v) => state.completed = v,
            Err(vk::Result::ERROR_DEVICE_LOST) => {
                self.record_hardware_loss("vkGetSemaphoreCounterValue: device lost");
            }
            Err(_) => {}
        }
        while state
            .in_flight
            .front()
            .is_some_and(|f| f.value <= state.completed)
        {
            let Some(f) = state.in_flight.pop_front() else {
                break;
            };
            // The submission that used the pool completed.
            self.recycle_pool(state, f.pool);
            freed.extend(f.staging);
        }
        freed
    }

    /// A memory type index with `want` among `bits`, preferring `prefer` too.
    pub(crate) fn memory_type(
        &self,
        bits: u32,
        want: vk::MemoryPropertyFlags,
        prefer: vk::MemoryPropertyFlags,
    ) -> Option<u32> {
        let m = &self.0.memory;
        let types = &m.memory_types[..m.memory_type_count as usize];
        let find = |flags: vk::MemoryPropertyFlags| {
            types
                .iter()
                .enumerate()
                .find(|(i, t)| bits & (1 << i) != 0 && t.property_flags.contains(flags))
                .map(|(i, _)| i as u32)
        };
        find(want | prefer).or_else(|| find(want))
    }

    pub fn timestamp_period(&self) -> f32 {
        self.0.adapter.timestamp_period
    }

    pub(crate) fn family(&self) -> u32 {
        self.0.adapter.family
    }
}
