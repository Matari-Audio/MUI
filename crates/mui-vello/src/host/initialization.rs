//! Native initialization without window ownership. A caller must pin its image
//! before permitting detached work. On close no join occurs: a running driver
//! call may finish later, drop its unsent result on that thread and exit. The
//! admitted session and its pipelines live until process exit. The caller
//! pins the image first, including all code used by device callbacks.
use super::*;
use std::sync::mpsc;

// DX12 HWND and DirectComposition surfaces require different instance options.
// Other backends can share one compute context regardless of surface alpha.
static SHARED: Mutex<[Option<Arc<Session>>; 2]> = Mutex::new([None, None]);

fn session_slot(transparency: Transparency) -> usize {
    usize::from(cfg!(windows) && transparency == Transparency::Translucent)
}

fn session_descriptor(transparency: Transparency) -> wgpu::InstanceDescriptor {
    let mut descriptor = instance_descriptor();
    if transparency == Transparency::Translucent {
        descriptor.backend_options.dx12.presentation_system =
            wgpu::Dx12SwapchainKind::DxgiFromVisual;
    }
    descriptor
}

pub(super) struct Session {
    state: Mutex<State>,
    detached: bool,
    transparency: Transparency,
    excluded: Vec<(String, wgpu::Backend)>,
}
struct State {
    generation: u64,
    building: bool,
    context: Option<Arc<Context>>,
    failure: Option<(String, FailureClass)>,
    retry: Retry,
}
impl State {
    fn begin_generation(&mut self) -> Result<u64, HostError> {
        if self.building {
            return Err(HostError::Unavailable {
                message: "generation already building".into(),
                class: FailureClass::Transient,
            });
        }
        self.building = true;
        Ok(self.generation.wrapping_add(1))
    }
}
struct Context {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    errors: Arc<DeviceErrors>,
    diagnostics: GpuDiagnostics,
    vello: crate::effects::retained::SharedVello,
    generation: u64,
}

fn run(detached: bool, work: impl FnOnce() + Send + 'static) -> bool {
    if detached {
        // If spawning fails the closure is dropped, not silently detached. The
        // next poll observes a disconnected result and enters backoff.
        std::thread::Builder::new()
            .name("mui-gpu-init".into())
            .spawn(work)
            .is_ok()
    } else {
        work();
        true
    }
}

impl Session {
    fn acquire(detached: bool, transparency: Transparency) -> Arc<Self> {
        // A caller without an image pin cannot inherit another caller's worker
        // permission. Its initialization must remain on its owning thread.
        if !detached {
            return Self::private(detached, transparency, Vec::new());
        }
        let mut shared = SHARED
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(session) = &shared[session_slot(transparency)] {
            return session.clone();
        }
        let session = Self::private(detached, transparency, Vec::new());
        shared[session_slot(transparency)] = Some(session.clone());
        session
    }
    fn private(
        detached: bool,
        transparency: Transparency,
        excluded: Vec<(String, wgpu::Backend)>,
    ) -> Arc<Self> {
        Arc::new(Self {
            detached,
            transparency,
            excluded,
            state: Mutex::new(State {
                generation: 0,
                building: false,
                context: None,
                failure: None,
                retry: Retry::default(),
            }),
        })
    }
    fn context(self: &Arc<Self>) -> Result<Option<Arc<Context>>, HostError> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(context) = &state.context {
            if !context.errors.lost.load(Ordering::Acquire) {
                return Ok(Some(context.clone()));
            }
            state.context = None;
            state.retry.fail(Instant::now(), FailureClass::Transient);
        }
        if state.building || !state.retry.ready(Instant::now()) {
            if let Some((message, class)) = &state.failure {
                return Err(HostError::Unavailable {
                    message: message.clone(),
                    class: *class,
                });
            }
            return Ok(None);
        }
        let generation = state.begin_generation()?;
        state.failure = None;
        let excluded = self.excluded.clone();
        let transparency = self.transparency;
        let weak = Arc::downgrade(self);
        drop(state);
        let started = run(self.detached, move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let mut descriptor = session_descriptor(transparency);
                // Vello cannot render through GL. Vulkan WSI accepts each window's
                // own X connection; no process-wide borrowed display is retained.
                descriptor.backends &= wgpu::Backends::PRIMARY;
                if descriptor.backends.is_empty() {
                    return Err(HostError::Unavailable {
                        message: "no compute-capable native backend enabled".into(),
                        class: FailureClass::Permanent,
                    });
                }
                let instance = wgpu::Instance::new(descriptor);
                let gpu = OnDevice::open_filtered(
                    &instance,
                    None,
                    (1, 1),
                    Transparency::Opaque,
                    &excluded,
                )?;
                Ok(Arc::new(Context {
                    instance,
                    adapter: gpu.adapter,
                    device: gpu.device,
                    queue: gpu.queue,
                    errors: gpu.errors,
                    diagnostics: gpu.diagnostics,
                    vello: gpu.renderer.shared_vello(),
                    generation,
                }))
            }))
            .unwrap_or_else(|payload| {
                Err(HostError::Unavailable {
                    message: format!(
                        "GPU initializer panic: {}",
                        crate::diagnostics::panic_message(&*payload)
                    ),
                    class: FailureClass::Permanent,
                })
            });
            // Never keep the session/editor alive across a native driver call.
            // If its last owner closed, result drops here on the initializer.
            if let Some(session) = weak.upgrade() {
                let mut state = session
                    .state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                state.building = false;
                match result {
                    Ok(context) => {
                        state.generation = generation;
                        state.context = Some(context);
                        state.retry.reset();
                    }
                    Err(error) => {
                        let class = error.failure_class();
                        state.failure = Some((error.to_string(), class));
                        state.retry.fail(Instant::now(), class);
                    }
                }
            }
        });
        if !started {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state.building = false;
            state.retry.fail(Instant::now(), FailureClass::Transient);
            state.failure = Some((
                "cannot spawn GPU initializer".into(),
                FailureClass::Transient,
            ));
        }
        Ok(None)
    }
}

/// A bounded publication gate used only by native integration fixtures.
/// No native window, surface or callback is retained by this gate.
#[cfg(any(test, feature = "test-support"))]
#[doc(hidden)]
#[derive(Clone, Default)]
pub struct InitGate {
    state: Arc<(Mutex<GateState>, std::sync::Condvar)>,
}
#[cfg(any(test, feature = "test-support"))]
#[derive(Default)]
struct GateState {
    entered: bool,
    released: bool,
    rejected: Option<bool>,
}
#[cfg(any(test, feature = "test-support"))]
impl InitGate {
    /// Wait at most `timeout` for an actual per-window GPU result to be ready.
    pub fn wait_entered(&self, timeout: std::time::Duration) -> bool {
        let (lock, ready) = &*self.state;
        let state = lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (state, _) = ready
            .wait_timeout_while(state, timeout, |s| !s.entered)
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.entered
    }
    /// Allow the worker to publish or drop its result after the editor closes.
    pub fn release(&self) {
        let (lock, ready) = &*self.state;
        lock.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .released = true;
        ready.notify_all();
    }
    /// Observe that publication finished with the editor's receiver gone.
    pub fn wait_receiver_dropped(&self, timeout: std::time::Duration) -> bool {
        let (lock, ready) = &*self.state;
        let state = lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (state, _) = ready
            .wait_timeout_while(state, timeout, |s| s.rejected.is_none())
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.rejected == Some(true)
    }
    fn pause(&self) {
        let (lock, ready) = &*self.state;
        let mut state = lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.entered = true;
        ready.notify_all();
        // A broken fixture cannot strand an initializer indefinitely.
        drop(
            ready
                .wait_timeout_while(state, std::time::Duration::from_secs(10), |s| !s.released)
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
    }
    fn finished(&self, rejected: bool) {
        let (lock, ready) = &*self.state;
        lock.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .rejected = Some(rejected);
        ready.notify_all();
    }
}

/// One editor's asynchronous initializer. Keep this alive while the editor is
/// open, including during CPU recovery; all editors then adopt one generation.
/// `detached` must be the successful result of the host's image-pin operation.
pub struct GpuInit {
    session: Arc<Session>,
    job: Option<mpsc::Receiver<Result<OnDevice, HostError>>>,
    context: Option<Arc<Context>>,
    installed: bool,
    retry: Retry,
    excluded: Vec<(String, wgpu::Backend)>,
    last_failure: Option<String>,
    incompatible: bool,
    transparency: Transparency,
    #[cfg(any(test, feature = "test-support"))]
    completion_gate: Option<InitGate>,
}
impl GpuInit {
    /// Initialize an opaque window after its first CPU frame has presented.
    pub fn new(detached: bool) -> Self {
        Self::new_with_transparency(detached, Transparency::Opaque)
    }

    /// Initialize a native window while preserving its alpha presentation mode.
    ///
    /// Create this only after a successful CPU present. `detached` must be the
    /// host's successful image pin result; false keeps heavy work synchronous.
    /// Native surfaces are created only by [`Self::poll`] on the window thread.
    /// DX12 translucent windows use DirectComposition. Compatible windows share
    /// device/pipelines; alpha capability is still checked for each surface.
    pub fn new_with_transparency(detached: bool, transparency: Transparency) -> Self {
        if !detached {
            crate::diagnostics::breadcrumb(
                "mui-vello",
                "synchronous_gpu_init",
                "image pin unavailable; CPU first frame precedes synchronous GPU initialization",
            );
        }
        Self {
            session: Session::acquire(detached, transparency),
            job: None,
            context: None,
            installed: false,
            retry: Retry::default(),
            excluded: Vec::new(),
            last_failure: None,
            incompatible: true,
            transparency,
            #[cfg(any(test, feature = "test-support"))]
            completion_gate: None,
        }
    }
    /// Hold result publication for a native-close integration fixture.
    /// Panics if detached work was not admitted by a successful image pin.
    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn set_completion_gate_for_testing(&mut self, gate: InitGate) {
        assert!(
            self.session.detached,
            "completion gate requires a pinned worker"
        );
        self.completion_gate = Some(gate);
    }
    pub fn pending(&self) -> bool {
        !self.installed && self.retry.ready(Instant::now())
    }
    pub fn deadline(&self) -> Option<Instant> {
        self.retry.deadline()
    }
    /// Called when the window abandons its GPU. No surface crosses this boundary.
    pub fn recover(&mut self, class: FailureClass) {
        self.installed = false;
        self.job = None;
        self.context = None;
        self.retry.fail(Instant::now(), class);
    }
    fn failure(&mut self, error: &HostError) {
        let message = error.to_string();
        if self.last_failure.as_ref() != Some(&message) {
            crate::diagnostics::error("mui-vello", "cpu_presenter", &message);
            self.last_failure = Some(message);
        }
        self.retry.fail(Instant::now(), error.failure_class());
    }
    /// Poll without waiting. Only `create` and configure touch the native view,
    /// on the caller's window thread, after all heavy compute pipelines are ready.
    /// The caller must hold its native callback/ownership lease across this call
    /// and drop the returned Host before native destruction. An image pin only
    /// preserves code; it does not preserve an HWND, AppKit view or X11 drawable.
    /// No native handle, surface or factory is ever captured by detached work.
    pub fn poll(
        &mut self,
        size: (u32, u32),
        mut create: impl FnMut(&wgpu::Instance) -> Result<wgpu::Surface<'static>, String>,
    ) -> Option<Result<Host, HostError>> {
        if self.installed
            || target_size(size.0, size.1).is_none()
            || !self.retry.ready(Instant::now())
        {
            return None;
        }
        if self.job.is_none() {
            let context = match self.session.context() {
                Ok(Some(context)) => context,
                Ok(None) => return None,
                Err(error) => {
                    // Exhausting private surface candidates is not proof that
                    // the original view can never configure (mapping/DPI may lag).
                    let error = if self.excluded.is_empty() || self.incompatible {
                        error
                    } else {
                        self.excluded.clear();
                        self.session = Session::acquire(self.session.detached, self.transparency);
                        HostError::Unavailable {
                            message: error.to_string(),
                            class: FailureClass::Transient,
                        }
                    };
                    self.failure(&error);
                    return Some(Err(error));
                }
            };
            let (sender, receiver) = mpsc::sync_channel(1);
            self.context = Some(context.clone());
            self.job = Some(receiver);
            let transparency = self.transparency;
            #[cfg(any(test, feature = "test-support"))]
            let completion_gate = self.completion_gate.clone();
            let _ = run(self.session.detached, move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let limit = context.device.limits().max_texture_dimension_2d;
                    let (width, height) = (size.0.min(limit), size.1.min(limit));
                    let renderer = pollster::block_on(GpuRenderer::new_shared(
                        &context.device,
                        &context.queue,
                        wgpu::TextureFormat::Bgra8Unorm,
                        [width, height],
                        Budget::default(),
                        Some(context.vello.clone()),
                    ))
                    .map_err(HostError::Render)?;
                    Ok(OnDevice {
                        adapter: context.adapter.clone(),
                        device: context.device.clone(),
                        queue: context.queue.clone(),
                        errors: context.errors.clone(),
                        diagnostics: context.diagnostics.clone(),
                        size: (width, height),
                        renderer,
                        config: wgpu::SurfaceConfiguration {
                            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                            format: wgpu::TextureFormat::Bgra8Unorm,
                            width,
                            height,
                            present_mode: wgpu::PresentMode::AutoNoVsync,
                            desired_maximum_frame_latency: 1,
                            alpha_mode: alpha_mode(
                                &[
                                    wgpu::CompositeAlphaMode::Opaque,
                                    wgpu::CompositeAlphaMode::PreMultiplied,
                                ],
                                transparency,
                            ),
                            view_formats: Vec::new(),
                            color_space: wgpu::SurfaceColorSpace::Auto,
                        },
                    })
                }))
                .unwrap_or_else(|payload| {
                    Err(HostError::Unavailable {
                        message: format!(
                            "window GPU resources: {}",
                            crate::diagnostics::panic_message(&*payload)
                        ),
                        class: FailureClass::Permanent,
                    })
                });
                #[cfg(any(test, feature = "test-support"))]
                if let Some(gate) = &completion_gate {
                    gate.pause();
                }
                // Disconnected => drops GPU-only resources here. No native
                // handle, surface or factory crossed the worker boundary.
                #[cfg(any(test, feature = "test-support"))]
                let rejected = sender.send(result).is_err();
                #[cfg(not(any(test, feature = "test-support")))]
                let _ = sender.send(result);
                #[cfg(any(test, feature = "test-support"))]
                if let Some(gate) = completion_gate {
                    gate.finished(rejected);
                }
            });
        }
        let result = match self.job.as_ref()?.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return None,
            Err(mpsc::TryRecvError::Disconnected) => Err(HostError::Unavailable {
                message: "GPU initializer disconnected".into(),
                class: FailureClass::Transient,
            }),
        };
        self.job = None;
        let context = self.context.take()?;
        let mut surface_attempted = false;
        let result = result.and_then(|mut gpu| {
            if gpu.lost() {
                return Err(HostError::Unavailable {
                    message: "device lost during initialization".into(),
                    class: FailureClass::Transient,
                });
            }
            let surface = create(&context.instance).map_err(HostError::Configuration)?;
            surface_attempted = true;
            let config = surface_config(&surface, &gpu.adapter, size, self.transparency)?;
            configure(&surface, &gpu.device, &config).map_err(HostError::Configuration)?;
            gpu.renderer.set_surface_format(config.format);
            gpu.renderer
                .resize([config.width, config.height])
                .map_err(HostError::Render)?;
            gpu.size = (config.width, config.height);
            gpu.config = config;
            Ok(Host {
                instance: context.instance.clone(),
                shared: Some(self.session.clone()),
                surface: Some(surface),
                gpu,
                wanted: target_size(size.0, size.1),
                retry: Retry::default(),
                generation: context.generation,
                transparency: self.transparency,
                first_frame: true,
            })
        });
        let result = result.map_err(|error| {
            // Capability rejection can try another private adapter; temporary
            // configure failures can return to the original after backoff.
            if surface_attempted
                && matches!(error, HostError::Configuration(_) | HostError::Surface(_))
            {
                self.incompatible &= matches!(error, HostError::Surface(_));
                let info = context.adapter.get_info();
                self.excluded.push((info.name, info.backend));
                self.session = Session::private(
                    self.session.detached,
                    self.transparency,
                    self.excluded.clone(),
                );
                HostError::Unavailable {
                    message: error.to_string(),
                    class: FailureClass::Transient,
                }
            } else {
                error
            }
        });
        match &result {
            Ok(_) => {
                self.installed = true;
                self.retry.reset();
                self.last_failure = None;
            }
            Err(error) => {
                self.failure(error);
            }
        }
        Some(result)
    }
}

pub(super) fn surface_config(
    surface: &wgpu::Surface<'_>,
    adapter: &wgpu::Adapter,
    size: (u32, u32),
    transparency: Transparency,
) -> Result<wgpu::SurfaceConfiguration, HostError> {
    let limit = adapter.limits().max_texture_dimension_2d;
    let (width, height) = (size.0.min(limit).max(1), size.1.min(limit).max(1));
    let caps = surface.get_capabilities(adapter);
    let format = surface_format(&caps.formats).ok_or({
        HostError::Surface("adapter cannot present a non-sRGB UNORM frame to this window")
    })?;
    let config = surface
        .get_default_config(adapter, width, height)
        .ok_or(HostError::Surface("adapter cannot configure this window"))?;
    Ok(wgpu::SurfaceConfiguration {
        format,
        alpha_mode: alpha_mode(&caps.alpha_modes, transparency),
        present_mode: present_mode(&caps.present_modes),
        desired_maximum_frame_latency: 1,
        ..config
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pinned_session_survives_the_last_editor_closing() {
        let init = GpuInit::new(true);
        let weak = Arc::downgrade(&init.session);
        drop(init);
        let reopened = GpuInit::new(true);
        assert!(Arc::ptr_eq(&weak.upgrade().unwrap(), &reopened.session));
    }
    #[test]
    fn closing_drops_a_pending_receiver_without_joining_the_initializer() {
        // The test executable cannot unload. Model an admitted, pinned worker
        // stalled in a driver; it owns only a weak session and result sender.
        let mut init = GpuInit::new(true);
        init.session = Session::private(true, Transparency::Opaque, Vec::new());
        let weak = Arc::downgrade(&init.session);
        let (result, receiver) = mpsc::sync_channel(1);
        let (entered, entered_rx) = mpsc::sync_channel(1);
        let (release, release_rx) = mpsc::sync_channel(1);
        let (done, done_rx) = mpsc::sync_channel(1);
        init.job = Some(receiver);
        assert!(run(true, move || {
            entered.send(()).unwrap();
            release_rx
                .recv_timeout(std::time::Duration::from_secs(2))
                .unwrap();
            assert!(weak.upgrade().is_none());
            assert!(
                result
                    .send(Err(HostError::Unavailable {
                        message: "cancelled fixture".into(),
                        class: FailureClass::Transient,
                    }))
                    .is_err()
            );
            done.send(()).unwrap();
        }));
        entered_rx
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap();
        let started = Instant::now();
        drop(init);
        assert!(started.elapsed() < std::time::Duration::from_millis(100));
        release.send(()).unwrap();
        done_rx
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap();
    }

    #[test]
    fn unpinned_editor_cannot_inherit_detached_work_from_a_pinned_editor() {
        let pinned = GpuInit::new(true);
        let unpinned = GpuInit::new(false);
        assert!(pinned.session.detached);
        assert!(!unpinned.session.detached);
        assert!(!Arc::ptr_eq(&pinned.session, &unpinned.session));
    }

    #[test]
    fn transparent_initializer_preserves_alpha_and_windows_presentation_contract() {
        let opaque = GpuInit::new(true);
        let transparent = GpuInit::new_with_transparency(true, Transparency::Translucent);
        assert_eq!(transparent.transparency, Transparency::Translucent);
        assert_eq!(
            Arc::ptr_eq(&opaque.session, &transparent.session),
            !cfg!(windows)
        );
        assert_eq!(
            session_descriptor(Transparency::Translucent)
                .backend_options
                .dx12
                .presentation_system,
            wgpu::Dx12SwapchainKind::DxgiFromVisual
        );
        assert_eq!(
            session_slot(Transparency::Translucent),
            usize::from(cfg!(windows))
        );
    }

    #[test]
    #[ignore = "requires a native compute adapter"]
    fn close_before_gpu_result_publication_never_creates_a_native_surface() {
        let gate = InitGate::default();
        let mut init = GpuInit::new(true);
        init.session = Session::private(true, Transparency::Opaque, Vec::new());
        init.set_completion_gate_for_testing(gate.clone());
        let stop = Instant::now() + std::time::Duration::from_secs(10);
        while !gate.wait_entered(std::time::Duration::from_millis(1)) {
            assert!(
                Instant::now() < stop,
                "GPU fixture did not reach publication"
            );
            assert!(
                init.poll((32, 32), |_| panic!(
                    "native factory before gated completion"
                ))
                .is_none()
            );
        }
        let began = Instant::now();
        drop(init);
        assert!(began.elapsed() < std::time::Duration::from_millis(100));
        gate.release();
        assert!(gate.wait_receiver_dropped(std::time::Duration::from_secs(2)));
    }

    #[test]
    fn one_rebuild_claim_is_shared_by_all_observers() {
        let session = Session::private(false, Transparency::Opaque, Vec::new());
        let observer = session.clone();
        assert_eq!(session.state.lock().unwrap().begin_generation().unwrap(), 1);
        assert!(observer.state.lock().unwrap().begin_generation().is_err());
        let mut state = session.state.lock().unwrap();
        state.generation = 1;
        state.building = false;
        assert_eq!(state.begin_generation().unwrap(), 2);
    }

    #[test]
    #[ignore = "requires native compute adapter"]
    fn editors_adopt_one_device_and_rebuilt_generation() {
        let editor = Session::private(false, Transparency::Opaque, Vec::new());
        let second = editor.clone();
        assert!(editor.context().unwrap().is_none());
        let context = editor.context().unwrap().unwrap();
        let adopted = second.context().unwrap().unwrap();
        assert!(Arc::ptr_eq(&context, &adopted));
        context.device.destroy();
        let _ = context.device.poll(wgpu::PollType::Poll);
        assert!(editor.context().unwrap().is_none());
        editor.state.lock().unwrap().retry.reset();
        assert!(second.context().unwrap().is_none());
        let rebuilt = editor.context().unwrap().unwrap();
        assert_eq!(rebuilt.generation, context.generation + 1);
        assert_eq!(second.context().unwrap().unwrap().device, rebuilt.device);
        // wgpu Device equality compares local IDs, reused by another Instance.
        // Context identity/generation prove replacement instead.
        assert!(!Arc::ptr_eq(&rebuilt, &context));
        let weak = Arc::downgrade(&rebuilt);
        drop(rebuilt);
        drop(context);
        drop(adopted);
        drop(second);
        drop(editor);
        assert!(weak.upgrade().is_none());
    }

    #[test]
    fn weak_registry_does_not_own_the_last_editor() {
        let session = Session::private(false, Transparency::Opaque, Vec::new());
        let weak = Arc::downgrade(&session);
        let second = session.clone();
        drop(session);
        assert!(weak.upgrade().is_some());
        drop(second);
        assert!(weak.upgrade().is_none());
    }
}
