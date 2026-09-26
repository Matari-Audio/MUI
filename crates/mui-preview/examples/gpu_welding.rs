//! Native shader lab using MUI's real scene lowering and Vello composition.
//! Space: animate; arrows: morph; 1/2/3: all/body/border; R: reset placement.
//! Drag anywhere to move the second plate. Morph ticks reuse the resolved tree.
use mui::prelude::*;
use mui::vello::kurbo::Affine;
use std::{sync::Arc, time::Instant};
use winit::{
    application::ApplicationHandler,
    event::{ElementState, MouseButton, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{Key as WinitKey, NamedKey},
    window::{Window, WindowId},
};
#[expect(
    dead_code,
    reason = "the shared gallery host has debug-overlay methods this lab never calls"
)]
#[path = "../src/host.rs"]
mod host;

struct Lab {
    gpu: Option<host::Gpu>,
    ui: Ui,
    dirty: bool,
    visible: bool,
    morph: f64,
    policy: Weld,
    offset: (f64, f64),
    pointer: Option<(f64, f64)>,
    down: bool,
    crisp: bool,
    animate: bool,
    clock: Instant,
    resolves: u64,
}
impl Lab {
    fn new() -> Self {
        Self {
            gpu: None,
            ui: Ui::new(Theme::DEFAULT)
                .gpu_welding()
                .font(Font::new(epaint_default_fonts::HACK_REGULAR).unwrap()),
            dirty: true,
            visible: true,
            morph: 1.,
            policy: Weld::all(),
            offset: (-60., 20.),
            pointer: None,
            down: false,
            crisp: true,
            animate: false,
            clock: Instant::now(),
            resolves: 0,
        }
    }
    fn tree(&self) -> El {
        let a = leaf(150., 110.)
            .fill(Color::oklcha(0.68, 0.22, 25., 1.))
            .stroke(Warning)
            .stroke_width(2.)
            .radius(22.)
            .id("left");
        let b = leaf(150., 110.)
            .fill(Color::oklcha(0.65, 0.2, 270., 0.85))
            .stroke(Ink)
            .stroke_width(4.)
            .radius(35.)
            .offset(self.offset.0, self.offset.1)
            .id("right");
        col![
            text(
                "C: crisp/organic / Space: animate / arrows: morph / 1,2,3: channels / drag: move"
            ),
            row![a, b]
                .gap(25.)
                .gpu_weld(
                    self.policy
                        .reach(if self.crisp { 0. } else { 45. })
                        .blend(95.)
                        .morph(self.morph)
                )
                .id("join")
        ]
        .gap(L)
        .center()
        .full()
        .fill(Background)
    }
    fn draw(&mut self) {
        if !self.visible {
            return;
        }
        let Some(gpu) = self.gpu.as_ref() else {
            return;
        };
        let (pw, ph) = gpu.size();
        let scale = gpu.window().scale_factor();
        if self.animate {
            self.morph = 0.5 + 0.5 * (self.clock.elapsed().as_secs_f64() * 1.7).sin();
        }
        let mut layout_work = mui::layout::LayoutStats::default();
        if self.dirty {
            self.ui.scale = Some(scale);
            let root = self.tree();
            if let Err(e) = self.ui.frame(
                root,
                Some(Size::new(f64::from(pw) / scale, f64::from(ph) / scale)),
                Input::default(),
                0.,
            ) {
                eprintln!("scene: {e}");
                return;
            }
            self.resolves += 1;
            self.dirty = false;
            layout_work = self.ui.layout_stats();
        }
        if let Err(e) = self.ui.set_weld_morph("join", self.morph) {
            eprintln!("morph: {e}");
            return;
        }
        let Some(scene) = self.ui.scene() else {
            return;
        };
        let gpu = self.gpu.as_mut().expect("initialized above");
        match gpu.present(scene,Affine::scale(scale)) {
            Ok(Some(s))=>gpu.window().set_title(&format!("MUI {} | morph {:.2} | measure/arrange {}/{} | uniforms/boundary {}/{} B | effect {} | encodes {}",
                if self.crisp {"crisp"} else {"organic"},self.morph,layout_work.measured_nodes,layout_work.arranged_nodes,
                s.uniform_upload_bytes,s.boundary_upload_bytes,s.effect_draws,s.encoded_scenes)),
            Ok(None)=>{},Err(e)=>eprintln!("render: {e}"),
        }
        if self.animate {
            gpu.window().request_redraw();
        }
    }
}
impl ApplicationHandler for Lab {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gpu.is_some() {
            return;
        }
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title("MUI native weld")
                        .with_inner_size(winit::dpi::LogicalSize::new(960., 640.)),
                )
                .expect("window"),
        );
        let gpu = host::Gpu::new(window, Box::new(event_loop.owned_display_handle()));
        gpu.window().request_redraw();
        self.gpu = Some(gpu);
    }
    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
                return;
            }
            WindowEvent::RedrawRequested => {
                self.draw();
                return;
            }
            WindowEvent::Resized(size) => {
                self.visible = size.width > 0 && size.height > 0;
                self.dirty = true;
                if let Some(g) = &mut self.gpu {
                    g.resize(size.width, size.height);
                }
            }
            WindowEvent::ScaleFactorChanged { .. } => self.dirty = true,
            WindowEvent::Occluded(hidden) => {
                self.visible = !hidden;
                if hidden {
                    self.down = false;
                }
            }
            WindowEvent::Focused(false) => self.down = false,
            WindowEvent::MouseInput {
                button: MouseButton::Left,
                state,
                ..
            } => self.down = state == ElementState::Pressed,
            WindowEvent::CursorMoved { position, .. } => {
                let scale = self.gpu.as_ref().map_or(1., |g| g.window().scale_factor());
                let p = (position.x / scale, position.y / scale);
                if self.down
                    && let Some(old) = self.pointer
                {
                    self.offset.0 = (self.offset.0 + p.0 - old.0).clamp(-300., 300.);
                    self.offset.1 = (self.offset.1 + p.1 - old.1).clamp(-160., 160.);
                    self.dirty = true;
                }
                self.pointer = Some(p);
                if !self.down {
                    return;
                }
            }
            WindowEvent::KeyboardInput { event, .. } if event.state.is_pressed() => {
                match event.logical_key {
                    WinitKey::Named(NamedKey::Space) => self.animate = !self.animate,
                    WinitKey::Named(NamedKey::ArrowLeft) => {
                        self.animate = false;
                        self.morph = (self.morph - 0.05).max(0.);
                    }
                    WinitKey::Named(NamedKey::ArrowRight) => {
                        self.animate = false;
                        self.morph = (self.morph + 0.05).min(1.);
                    }
                    WinitKey::Character(ref s) => match s.as_str() {
                        "1" => {
                            self.policy = Weld::all();
                            self.dirty = true;
                        }
                        "2" => {
                            self.policy = Weld::shape();
                            self.dirty = true;
                        }
                        "3" => {
                            self.policy = Weld::borders();
                            self.dirty = true;
                        }
                        "c" | "C" => {
                            self.crisp = !self.crisp;
                            self.dirty = true;
                        }
                        "r" | "R" => {
                            self.offset = (-60., 20.);
                            self.dirty = true;
                        }
                        _ => return,
                    },
                    _ => return,
                }
            }
            _ => return,
        }
        if self.visible
            && let Some(g) = &self.gpu
        {
            g.window().request_redraw();
        }
    }
}
fn main() {
    let events = EventLoop::new().expect("event loop");
    events.set_control_flow(ControlFlow::Wait);
    events.run_app(&mut Lab::new()).expect("run lab");
}
