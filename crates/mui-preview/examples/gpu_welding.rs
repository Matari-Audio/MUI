//! Native shader lab using MUI's real scene lowering and 4-pass SDF quad /
//! analytic weld renderer, hosted directly on our owned `moose-baseview` /
//! `mui-baseview` standalone window layer (zero `winit`).
//! Space: animate; arrows: morph; 1/2/3: all/body/border; C: crisp/organic;
//! R: reset placement; drag anywhere to move the second plate.
use std::cell::RefCell;
use std::time::Instant;

use mui::host::NativeKey;
use mui::prelude::*;
use mui::vello::kurbo::Affine;
use mui_baseview::baseview::{
    DamageArea, Event, EventStatus, FrameDemand, HandlerError, MouseButton, MouseEvent,
    WindowContext, WindowEvent, WindowHandler, WindowSize,
};
use mui_baseview::native::key_event;

#[expect(dead_code, reason = "shared with the mui-preview binary")]
#[path = "../src/host.rs"]
mod host;

struct Lab {
    gpu: Option<host::Gpu>,
    cx: WindowContext,
    ui: Ui,
    dirty: bool,
    gpu_welding_available: Option<bool>,
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
    fn new(cx: WindowContext) -> Result<Self, String> {
        let gpu = host::Gpu::try_new(cx.clone())?;
        Ok(Self {
            gpu: Some(gpu),
            cx,
            ui: Ui::default()
                .gpu_welding()
                .font(Font::new(epaint_default_fonts::HACK_REGULAR).unwrap()),
            dirty: true,
            gpu_welding_available: None,
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
        })
    }

    fn tree(&self) -> El {
        let a = block(150., 110.)
            .fill(Color::oklcha(0.68, 0.22, 25., 1.))
            .stroke(Role::Warning)
            .stroke_width(2.)
            .radius(22.)
            .id("left");
        let b = block(150., 110.)
            .fill(Color::oklcha(0.65, 0.2, 270., 0.85))
            .stroke(Role::Ink)
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
                .weld(
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
        .fill(Role::Background)
    }

    fn draw(&mut self) {
        if !self.visible {
            return;
        }
        let Some(gpu) = self.gpu.as_mut() else {
            return;
        };
        gpu.update();
        let software = gpu.rendering_mode() == "cpu";
        if self.gpu_welding_available != Some(!software) {
            self.gpu_welding_available = Some(!software);
            self.ui.set_gpu_welding_available(!software);
            self.dirty = true;
        }
        let (pw, ph) = gpu.size();
        let scale = gpu.scale_factor();
        if self.animate {
            self.morph = 0.5 + 0.5 * (self.clock.elapsed().as_secs_f64() * 1.7).sin();
        }
        let mut layout_work = mui::layout::LayoutStats::default();
        if self.dirty {
            self.ui.set_scale(Some(scale));
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
        match gpu.present(scene, Affine::scale(scale)) {
            Ok(Some(s)) => gpu.set_title(&format!(
                "MUI {} | morph {:.2} | measure/arrange {}/{} | uniforms/boundary {}/{} B | effect {} | encodes {}",
                if self.crisp { "crisp" } else { "organic" },
                self.morph,
                layout_work.measured_nodes,
                layout_work.arranged_nodes,
                s.uniform_upload_bytes,
                s.boundary_upload_bytes,
                s.effect_draws,
                s.encoded_scenes
            )),
            Ok(None) => {}
            Err(e) => {
                self.dirty = true;
                eprintln!("render: {e}");
            }
        }
    }
}

struct LabHandler(RefCell<Lab>);

impl WindowHandler for LabHandler {
    fn frame_demand(&self) -> FrameDemand {
        let Ok(lab) = self.0.try_borrow() else {
            return FrameDemand::Continuous;
        };
        if !lab.visible {
            return FrameDemand::Idle;
        }
        if lab.animate {
            return FrameDemand::Continuous;
        }
        if lab.dirty {
            return FrameDemand::At(Instant::now());
        }
        lab.gpu
            .as_ref()
            .and_then(|gpu| gpu.next_wake(Instant::now()))
            .map_or(FrameDemand::Idle, FrameDemand::At)
    }

    fn on_frame(&self) -> Result<(), HandlerError> {
        let mut lab = self.0.borrow_mut();
        lab.draw();
        Ok(())
    }

    fn damage(&self, _: DamageArea<'_>) {
        if let Ok(mut lab) = self.0.try_borrow_mut() {
            if let Some(gpu) = &mut lab.gpu {
                gpu.invalidate();
            }
            lab.dirty = true;
        }
    }

    fn resized(&self, size: WindowSize) -> Result<(), HandlerError> {
        let mut lab = self.0.borrow_mut();
        lab.visible = size.physical.width > 0 && size.physical.height > 0;
        lab.dirty = true;
        if let Some(gpu) = &mut lab.gpu {
            gpu.resize(size.physical.width, size.physical.height);
        }
        lab.cx.request_redraw();
        Ok(())
    }

    fn on_event(&self, event: Event) -> EventStatus {
        let mut lab = self.0.borrow_mut();
        match event {
            Event::Window(WindowEvent::WillClose) => {
                lab.gpu.take();
                return EventStatus::Captured;
            }
            Event::Window(WindowEvent::RedrawRequested) => {
                if let Some(gpu) = &mut lab.gpu {
                    gpu.invalidate();
                }
                lab.dirty = true;
            }
            Event::Window(WindowEvent::ScaleFactorChanged(_)) => {
                lab.dirty = true;
            }
            Event::Window(WindowEvent::Unfocused)
            | Event::Mouse(MouseEvent::ButtonReleased {
                button: MouseButton::Left,
                ..
            }) => {
                lab.down = false;
            }
            Event::Mouse(MouseEvent::ButtonPressed {
                button: MouseButton::Left,
                ..
            }) => {
                lab.down = true;
            }
            Event::Mouse(MouseEvent::CursorMoved { position, .. }) => {
                let scale = lab.cx.scale_factor();
                let p = (position.x / scale, position.y / scale);
                if lab.down
                    && let Some(old) = lab.pointer
                {
                    lab.offset.0 = (lab.offset.0 + p.0 - old.0).clamp(-300., 300.);
                    lab.offset.1 = (lab.offset.1 + p.1 - old.1).clamp(-160., 160.);
                    lab.dirty = true;
                }
                lab.pointer = Some(p);
                if !lab.down {
                    return EventStatus::Captured;
                }
            }
            Event::Keyboard(key) => {
                let translated = key_event(&key);
                if !translated.down {
                    return EventStatus::Ignored;
                }
                match translated.key {
                    NativeKey::Named(Key::Escape) => {
                        lab.cx.request_close();
                        return EventStatus::Captured;
                    }
                    NativeKey::Text(ref s) if s == " " => {
                        lab.animate = !lab.animate;
                    }
                    NativeKey::Named(Key::Left) => {
                        lab.animate = false;
                        lab.morph = (lab.morph - 0.05).max(0.);
                    }
                    NativeKey::Named(Key::Right) => {
                        lab.animate = false;
                        lab.morph = (lab.morph + 0.05).min(1.);
                    }
                    NativeKey::Text(ref s) => match s.as_str() {
                        "1" => {
                            lab.policy = Weld::all();
                            lab.dirty = true;
                        }
                        "2" => {
                            lab.policy = Weld::shape();
                            lab.dirty = true;
                        }
                        "3" => {
                            lab.policy = Weld::borders();
                            lab.dirty = true;
                        }
                        "c" | "C" => {
                            lab.crisp = !lab.crisp;
                            lab.dirty = true;
                        }
                        "r" | "R" => {
                            lab.offset = (-60., 20.);
                            lab.dirty = true;
                        }
                        _ => return EventStatus::Ignored,
                    },
                    _ => return EventStatus::Ignored,
                }
            }
            _ => return EventStatus::Ignored,
        }
        if lab.visible {
            lab.cx.request_redraw();
        }
        EventStatus::Captured
    }
}

fn main() {
    let settings = mui_baseview::standalone::settings("MUI native weld", (960, 640));
    mui_baseview::standalone::run_window(settings, |cx| {
        Lab::new(cx)
            .map(|lab| LabHandler(RefCell::new(lab)))
            .map_err(|error| HandlerError::from(std::io::Error::other(error)))
    })
    .expect("run lab");
}
