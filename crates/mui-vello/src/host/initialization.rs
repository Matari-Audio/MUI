//! Native initialization without window ownership. A caller must pin its image
//! before permitting detached work. On close no join occurs: a running driver
//! call may finish later, drop its unsent result on that thread and exit. Only
//! that transient result may outlive the last editor; the static registry is Weak.
use super::*;
use std::sync::{Weak, mpsc};

static SHARED: Mutex<Weak<Session>> = Mutex::new(Weak::new());

pub(super) struct Session {
    state: Mutex<State>,
    detached: bool,
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
    fn acquire(detached: bool) -> Arc<Self> {
        let mut weak = SHARED
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(session) = weak.upgrade() {
            return session;
        }
        let session = Self::private(detached, Vec::new());
        *weak = Arc::downgrade(&session);
        session
    }
    fn private(detached: bool, excluded: Vec<(String, wgpu::Backend)>) -> Arc<Self> {
        Arc::new(Self {
            detached,
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
        let weak = Arc::downgrade(self);
        drop(state);
        let started = run(self.detached, move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let mut descriptor = instance_descriptor();
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
}
impl GpuInit {
    pub fn new(detached: bool) -> Self {
        if !detached {
            crate::diagnostics::breadcrumb(
                "mui-vello",
                "synchronous_gpu_init",
                "image pin unavailable; CPU first frame precedes synchronous GPU initialization",
            );
        }
        Self {
            session: Session::acquire(detached),
            job: None,
            context: None,
            installed: false,
            retry: Retry::default(),
            excluded: Vec::new(),
            last_failure: None,
            incompatible: true,
        }
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
                        self.session = Session::acquire(self.session.detached);
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
                            alpha_mode: wgpu::CompositeAlphaMode::Opaque,
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
                let _ = sender.send(result); // Disconnected => drops result on this thread.
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
            let config = surface_config(&surface, &gpu.adapter, size, Transparency::Opaque)?;
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
                transparency: Transparency::Opaque,
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
                self.session = Session::private(self.session.detached, self.excluded.clone());
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
    fn closing_drops_a_pending_receiver_without_joining_the_initializer() {
        // The test executable cannot unload. Model an admitted, pinned worker
        // stalled in a driver; it owns only a weak session and result sender.
        let mut init = GpuInit::new(true);
        init.session = Session::private(true, Vec::new());
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
    fn one_rebuild_claim_is_shared_by_all_observers() {
        let session = Session::private(false, Vec::new());
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
        let editor = Session::private(false, Vec::new());
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
        let session = Session::private(false, Vec::new());
        let weak = Arc::downgrade(&session);
        let second = session.clone();
        drop(session);
        assert!(weak.upgrade().is_some());
        drop(second);
        assert!(weak.upgrade().is_none());
    }
}
