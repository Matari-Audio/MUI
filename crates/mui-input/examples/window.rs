//! A live MUI window with no egui in the dependency graph.
//!
//!     cargo run -p mui-input --example window
//!
//! Hover a control and it lightens; press and it takes the accent; click and it
//! latches. Drag anywhere on the panel to move the whole scene -- keep dragging
//! past the edge of the window and it still tracks, because a press captures
//! its target.
//!
//! Three crates and no toolkit: `mui-layout` places the frames, `mui-core`
//! merges and fillets them, `mui-input` decides what the pointer means, and
//! `vello_hybrid` draws. winit and wgpu are the example's own, standing in for
//! the plugin wrapper that owns the window in a real host.

use std::collections::BTreeSet;
use std::sync::Arc;

use mui_core::dsl::column;
use mui_core::{
    resolve_scene, CornerProfile, CornerRule, FrameRadius, ResolvedScene, SceneSpec, Spacing,
    SurfaceSpec, Theme,
};
use mui_geometry::Point;
use mui_input::{Hit, Interaction, PointerInput};
use mui_layout::{Align, Node, Size};
use vello_common::kurbo::Affine;
use vello_common::peniko::color::AlphaColor;
use vello_common::peniko::Fill;
use vello_hybrid::{RenderSize, RenderTargetConfig, Renderer, Resources, Scene, TextureBindings};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

const CONTROLS: [&str; 3] = ["plus", "phase", "warp"];
/// Back to front. Hit testing walks this in reverse, so the controls sitting on
/// the pill win over the panel underneath them.
const PAINT_ORDER: [&str; 5] = ["outer", "pill-shell", "plus", "phase", "warp"];

fn spec() -> SceneSpec {
    let controls = column(
        "controls",
        CONTROLS.map(|id| Node::leaf(id, Size::new(28.0, 28.0))),
    )
    .gap(10.0)
    .align(Align::Center);

    let tab = column(
        "tab-frame",
        [column("pill-frame", [controls]).padding(10.0)],
    )
    .padding(12.0);

    let root = column(
        "root",
        [tab, Node::leaf("panel-frame", Size::new(420.0, 180.0))],
    )
    .align(Align::Start);

    let mut scene = SceneSpec::new(root)
        .theme(Theme {
            corners: CornerProfile::new(28.0, 32.0),
            ..Theme::default()
        })
        .surface(SurfaceSpec::frame("panel", "panel-frame").radius(FrameRadius::Global))
        .surface(SurfaceSpec::frame("tab", "tab-frame").radius(FrameRadius::Global))
        .surface(SurfaceSpec::merge("outer", ["panel", "tab"]).corners(CornerRule::Global))
        .surface(SurfaceSpec::inset("pill-shell", "tab", Spacing::px(12.0)));
    for id in CONTROLS {
        scene = scene.surface(SurfaceSpec::frame(id, id).radius(FrameRadius::Absolute(9.0)));
    }
    scene
}

type Rgba = AlphaColor<vello_common::peniko::color::Srgb>;

const PANEL: Rgba = AlphaColor::new([0.13, 0.14, 0.17, 1.0]);
const WELL: Rgba = AlphaColor::new([0.09, 0.10, 0.12, 1.0]);
const IDLE: Rgba = AlphaColor::new([0.27, 0.29, 0.34, 1.0]);
const HOVER: Rgba = AlphaColor::new([0.40, 0.43, 0.50, 1.0]);
const ACCENT: Rgba = AlphaColor::new([0.35, 0.72, 0.98, 1.0]);

struct App {
    scene: ResolvedScene,
    hit: Hit,
    input: Interaction,
    latched: BTreeSet<String>,
    /// Scene origin, moved by dragging the panel. Pointer positions are taken
    /// back through this before hit testing.
    origin: Point,
    pointer: PointerInput,
    /// `MUI_TRACE=1` prints what the pointer resolves to each frame. The one
    /// thing worth seeing when a control stops responding.
    trace: bool,
    gpu: Option<Gpu>,
}

struct Gpu {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    renderer: Renderer,
    resources: Resources,
    vello: Scene,
}

impl App {
    fn new() -> Self {
        let scene = resolve_scene(&spec()).expect("scene resolves");
        let mut hit = Hit::default();
        for id in PAINT_ORDER {
            let surface = scene.surface(id).unwrap_or_else(|| panic!("no {id}"));
            hit.push(id, &surface.path).expect("hit geometry");
        }
        Self {
            scene,
            hit,
            input: Interaction::new(),
            latched: BTreeSet::new(),
            origin: Point::new(48., 48.),
            pointer: PointerInput::default(),
            trace: std::env::var_os("MUI_TRACE").is_some(),
            gpu: None,
        }
    }

    /// One interaction frame. Separate from drawing so it stays testable and so
    /// a missed redraw can never swallow a click.
    fn tick(&mut self) {
        let local = PointerInput {
            pos: self.pointer.pos.map(|p| p - self.origin),
            primary_down: self.pointer.primary_down,
        };
        self.input.update(&self.hit, local);
        if self.trace {
            eprintln!("local={:?} hovered={:?}", local.pos, self.input.hovered());
        }

        for id in CONTROLS {
            // `remove` reports whether it was there, so the toggle is one call.
            if self.input.get(id).clicked && !self.latched.remove(id) {
                self.latched.insert(id.to_owned());
            }
        }
        // Dragging the body moves the scene. The control surfaces sit on top,
        // so a press that starts on one never reaches here.
        let body = self.input.get("outer");
        if body.dragged {
            self.origin = self.origin + body.drag_delta;
        }
    }

    fn ink(&self, id: &str) -> Rgba {
        let r = self.input.get(id);
        match id {
            "outer" => PANEL,
            "pill-shell" => WELL,
            _ if r.held => ACCENT,
            _ if self.latched.contains(id) => ACCENT,
            _ if r.hovered => HOVER,
            _ => IDLE,
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gpu.is_some() {
            return; // Resumed can fire more than once; the surface is still good.
        }
        let attrs = Window::default_attributes()
            .with_title("MUI -- layout, merge, vello, no egui")
            .with_inner_size(winit::dpi::LogicalSize::new(640, 380));
        let window = Arc::new(event_loop.create_window(attrs).expect("window"));
        // The GL backend needs the display handle to reach EGL/GLX. KURV's
        // Windows path prefers GL, so hand it over rather than hope for
        // Vulkan.
        let display = Box::new(event_loop.owned_display_handle());
        self.gpu = Some(pollster::block_on(Gpu::new(window, display)));
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(gpu) = &mut self.gpu {
                    gpu.resize(size.width, size.height);
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.pointer.pos = Some(Point::new(position.x, position.y));
            }
            WindowEvent::CursorLeft { .. } => self.pointer.pos = None,
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => self.pointer.primary_down = state == ElementState::Pressed,
            WindowEvent::RedrawRequested => {
                self.tick();
                let inks: Vec<(&str, Rgba)> =
                    PAINT_ORDER.iter().map(|id| (*id, self.ink(id))).collect();
                if let Some(gpu) = &mut self.gpu {
                    gpu.draw(&self.scene, &inks, self.origin);
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _: &ActiveEventLoop) {
        if let Some(gpu) = &self.gpu {
            gpu.window.request_redraw();
        }
    }
}

impl Gpu {
    async fn new(window: Arc<Window>, display: Box<winit::event_loop::OwnedDisplayHandle>) -> Self {
        let instance = wgpu::Instance::new(
            wgpu::InstanceDescriptor::new_with_display_handle_from_env(display),
        );
        let surface = instance.create_surface(window.clone()).expect("surface");
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await
            .expect("adapter");
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await
            .expect("device");

        let size = window.inner_size();
        let caps = surface.get_capabilities(&adapter);
        // Vello writes sRGB values, so an _Srgb surface would encode them a
        // second time. Take the linear sibling of the surface's own preferred
        // format -- picking the first non-sRGB entry instead lands on
        // Rgba16Unorm here, which needs a device feature we never asked for.
        let preferred = caps.formats[0];
        let linear = preferred.remove_srgb_suffix();
        let format = if caps.formats.contains(&linear) {
            linear
        } else {
            preferred
        };
        let config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .expect("surface config");
        let config = wgpu::SurfaceConfiguration { format, ..config };
        surface.configure(&device, &config);

        let (renderer, resources) = Renderer::new(
            &device,
            &RenderTargetConfig {
                format,
                width: config.width,
                height: config.height,
            },
        );
        Self {
            vello: Scene::new(config.width as u16, config.height as u16),
            window,
            surface,
            device,
            queue,
            config,
            renderer,
            resources,
        }
    }

    fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 || (width, height) == (self.config.width, self.config.height) {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        self.vello.reset_and_resize(width as u16, height as u16);
        // The pipelines are keyed on the target size; rebuild rather than
        // guess at which of them survive.
        let (renderer, resources) = Renderer::new(
            &self.device,
            &RenderTargetConfig {
                format: self.config.format,
                width,
                height,
            },
        );
        self.renderer = renderer;
        self.resources = resources;
    }

    fn draw(&mut self, scene: &ResolvedScene, inks: &[(&str, Rgba)], origin: Point) {
        self.vello.reset();
        self.vello.set_fill_rule(Fill::EvenOdd); // Agree with mui-tessellate.
        self.vello
            .set_transform(Affine::translate((origin.x, origin.y)));
        for (id, ink) in inks {
            let Some(surface) = scene.surface(id) else {
                continue;
            };
            let Ok(path) = mui_vello::bez_path(&surface.path, mui_vello::ARC_TOLERANCE) else {
                continue;
            };
            self.vello.set_paint(*ink);
            self.vello.fill_path(&path);
        }

        use wgpu::CurrentSurfaceTexture as Acquired;
        let frame = match self.surface.get_current_texture() {
            Acquired::Success(f) | Acquired::Suboptimal(f) => f,
            // Outdated and Lost both want the swapchain rebuilt; the size has
            // not changed, so reconfigure with what we already have.
            Acquired::Outdated | Acquired::Lost => {
                self.surface.configure(&self.device, &self.config);
                return;
            }
            // Timeout, Occluded, Validation: skip the frame and try again.
            _ => return,
        };
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(self.config.format),
            ..Default::default()
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        if self
            .renderer
            .render(
                &self.vello,
                &mut self.resources,
                &self.device,
                &self.queue,
                &mut encoder,
                &RenderSize {
                    width: self.config.width,
                    height: self.config.height,
                },
                &view,
                &TextureBindings::new(),
            )
            .is_ok()
        {
            self.queue.submit([encoder.finish()]);
            frame.present();
        }
    }
}

fn main() {
    let event_loop = EventLoop::new().expect("event loop");
    event_loop.set_control_flow(ControlFlow::Wait);
    event_loop.run_app(&mut App::new()).expect("run");
}
