//! MUI preview gallery: a dev-only host that puts a [`scenes::PreviewScene`]
//! on screen through the same `mui::Ui` a plugin would use.
//!
//! The whole window, sidebar included, is one styled tree resolved once a
//! frame. winit and wgpu are this binary's own, standing in for the plugin
//! wrapper that owns the window in a real host.
//!
//! Run: `cargo run -p mui-preview`
#![forbid(unsafe_code)]

mod host;
mod scenes;
mod skin;

use std::sync::Arc;
use std::time::Instant;

use host::Gpu;
use mui::geometry::Point;
use mui::prelude::*;
use mui::vello::kurbo::{Affine, Rect, Shape as _, Stroke};
use mui::vello::peniko::color::AlphaColor;
use mui::vello::Canvas as _;
use scenes::PreviewScene;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key as WinitKey, NamedKey};
use winit::window::{CursorIcon, Window, WindowId};

/// The layout-frame overlay is a debug aid: off-palette so it reads against
/// any theme.
const FRAME: AlphaColor<mui::vello::peniko::color::Srgb> =
    AlphaColor::new([0.95, 0.45, 0.75, 0.60]);
const SIDEBAR: f64 = 240.0;
/// One wheel line in logical pixels. winit reports lines, MUI scrolls pixels.
const LINE: f64 = 40.0;

/// What the hovered surface asks for, in winit's vocabulary.
fn icon(c: Cursor) -> CursorIcon {
    match c {
        Cursor::Arrow => CursorIcon::Default,
        Cursor::Hand => CursorIcon::Pointer,
        Cursor::Grab => CursorIcon::Grab,
        Cursor::Grabbing => CursorIcon::Grabbing,
        Cursor::Text => CursorIcon::Text,
        Cursor::ResizeH => CursorIcon::EwResize,
        Cursor::ResizeV => CursorIcon::NsResize,
        Cursor::Crosshair => CursorIcon::Crosshair,
        Cursor::Forbidden => CursorIcon::NotAllowed,
    }
}

/// The named keys MUI has a word for; everything else is the host's business.
fn named(k: NamedKey) -> Option<mui::prelude::Key> {
    use mui::prelude::Key as K;
    Some(match k {
        NamedKey::Enter => K::Enter,
        NamedKey::Escape => K::Escape,
        NamedKey::Tab => K::Tab,
        NamedKey::Backspace => K::Backspace,
        NamedKey::Delete => K::Delete,
        NamedKey::ArrowLeft => K::Left,
        NamedKey::ArrowRight => K::Right,
        NamedKey::ArrowUp => K::Up,
        NamedKey::ArrowDown => K::Down,
        NamedKey::Home => K::Home,
        NamedKey::End => K::End,
        _ => return None,
    })
}

struct App {
    ui: Ui,
    scenes: Vec<Box<dyn PreviewScene>>,
    selected: usize,
    /// Where the specimen was dragged to, relative to centred.
    pan: Point,
    light: bool,
    frames: bool,
    /// Pointer samples since the last frame, in physical pixels. winit
    /// dispatches every queued event and then one redraw, so a press and
    /// release in one batch must both be seen to make a click.
    events: Vec<PointerInput>,
    pointer: PointerInput,
    /// Everything non-pointer collected since the last frame: it rides on the
    /// last pointer sample of the batch.
    wheel: Point,
    keys: Vec<KeyPress>,
    text: String,
    mods: Mods,
    cursor: Cursor,
    typed: Option<char>,
    last: Instant,
    gpu: Option<Gpu>,
}

impl App {
    fn new() -> Self {
        let font = Arc::new(epaint_default_fonts::HACK_REGULAR.to_vec());
        Self {
            ui: Ui::new(skin::SKIN).font(font),
            scenes: scenes::all(),
            selected: 0,
            pan: Point::new(0.0, 0.0),
            light: false,
            frames: false,
            events: Vec::new(),
            pointer: PointerInput::default(),
            wheel: Point::new(0.0, 0.0),
            keys: Vec::new(),
            text: String::new(),
            mods: Mods::default(),
            cursor: Cursor::Arrow,
            typed: None,
            last: Instant::now(),
            gpu: None,
        }
    }

    fn queue(&mut self, pos: Option<Option<Point>>, down: Option<bool>) {
        self.pointer = PointerInput {
            pos: pos.unwrap_or(self.pointer.pos),
            primary_down: down.unwrap_or(self.pointer.primary_down),
        };
        self.events.push(self.pointer);
    }

    /// Is any focusable surface focused? Escape belongs to the UI when one is.
    fn any_focus(&self) -> bool {
        self.ui
            .scene()
            .is_some_and(|s| s.keys.iter().any(|k| self.ui.focused(k)))
    }

    fn cancel(&mut self) {
        self.events.clear();
        self.keys.clear();
        self.text.clear();
        self.pointer = PointerInput::default();
        self.ui.cancel();
    }

    /// The window as one tree: sidebar, then a stage the specimen centres in.
    fn tree(&mut self, w: f64, h: f64) -> El {
        let ui = &mut self.ui;
        ui.theme.palette = skin::skin(self.light);
        if let Some(c) = self.typed.take() {
            self.scenes[self.selected].key(c);
        }
        for i in 0..self.scenes.len() {
            if ui.get(&format!("scene-{i}")).clicked {
                self.selected = i;
                self.pan = Point::new(0.0, 0.0);
            }
        }
        let d = ui.get("stage").drag_delta;
        self.pan = Point::new(self.pan.x + d.x, self.pan.y + d.y);

        let scene = &mut self.scenes[self.selected];
        let mut side = vec![
            text("MUI preview").text_size(16.0),
            text(scene.about()).fill(Role::Dim),
        ];
        side.extend((0..self.scenes.len()).map(|i| {
            let on = i == self.selected;
            row([text(self.scenes[i].name())])
                .pad_xy(10.0, 6.0)
                .radius(8.0)
                .fill(if on { Role::Primary } else { Role::Raised })
                .id(format!("scene-{i}"))
        }));
        let scene = &mut self.scenes[self.selected];
        let switch = |label: &str, id: &str, v: &mut bool| {
            row([text(label).fill(Role::Dim), spacer(), toggle(ui, id, v)]).align(Align::Center)
        };
        side.push(switch("light", "light", &mut self.light));
        side.push(switch("frames", "frames", &mut self.frames));
        side.extend(scene.controls(ui));
        let sidebar = column(side)
            .gap(S)
            .pad(M)
            .width(SIDEBAR)
            .fill(Role::Surface);
        let specimen = scene
            .specimen(ui)
            .anchor(Align::Center, Align::Center)
            .offset(self.pan.x, self.pan.y);
        let stage = overlay([specimen])
            .grow(1.0)
            .fill(Role::Background)
            .id("stage");
        row([sidebar, stage]).size(w, h)
    }

    /// One interaction frame at the window's logical size. Returns whether a
    /// spring is still moving.
    fn tick(&mut self, (pw, ph): (u32, u32), scale: f64, input: impl Into<Input>) -> bool {
        let (w, h) = (f64::from(pw) / scale, f64::from(ph) / scale);
        let mut input = input.into();
        input.pointer.pos = input
            .pointer
            .pos
            .map(|p| Point::new(p.x / scale, p.y / scale));
        let now = Instant::now();
        let dt = now.duration_since(self.last).as_secs_f64().min(0.1);
        self.last = now;
        let root = self.tree(w, h);
        let (animating, cursor) = match self.ui.frame(root, Some(Size::new(w, h)), input, dt) {
            Ok(f) => (f.animating, f.cursor),
            Err(e) => {
                eprintln!("frame: {e}");
                (false, Cursor::Arrow)
            }
        };
        self.cursor = cursor;
        animating
    }

    fn replay(&mut self, size: (u32, u32), scale: f64) -> bool {
        let rest = Input {
            wheel: std::mem::take(&mut self.wheel),
            keys: std::mem::take(&mut self.keys),
            text: std::mem::take(&mut self.text),
            ..Input::default()
        };
        let events = std::mem::take(&mut self.events);
        let Some(last) = events.len().checked_sub(1) else {
            return self.tick(
                size,
                scale,
                Input {
                    pointer: self.pointer,
                    ..rest
                },
            );
        };
        events.into_iter().enumerate().fold(false, |a, (i, p)| {
            let input = if i == last {
                Input {
                    pointer: p,
                    ..rest.clone()
                }
            } else {
                Input::from(p)
            };
            self.tick(size, scale, input) | a
        })
    }

    fn draw(&mut self) {
        let Some(gpu) = &mut self.gpu else { return };
        let scale = gpu.window().scale_factor();
        let Some(scene) = self.ui.scene() else { return };
        let mut canvas = gpu.begin();
        let xf = Affine::scale(scale);
        if let Err(e) = mui::vello::paint(&mut canvas, scene, xf) {
            eprintln!("paint: {e}");
        }
        if let Some((key, path)) = self.scenes[self.selected].overlay() {
            if let (Some(s), Ok(bez)) = (
                scene.surface(key),
                mui::vello::bez_path(&path, mui::vello::ARC_TOLERANCE),
            ) {
                canvas.set_transform(xf * Affine::translate((s.frame.x, s.frame.y)));
                canvas.set_paint(
                    self.ui
                        .theme
                        .palette
                        .on(self.ui.theme.palette.raised())
                        .to_srgb()
                        .into(),
                );
                canvas.fill_path(&bez);
            }
        }
        if self.frames {
            canvas.set_transform(xf);
            canvas.set_paint(FRAME.into());
            canvas.set_stroke(Stroke::new(1.0));
            for f in scene.layout.all() {
                canvas.stroke_path(&Rect::new(f.x, f.y, f.right(), f.bottom()).to_path(0.1));
            }
        }
        gpu.present();
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gpu.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("MUI preview")
            .with_inner_size(winit::dpi::LogicalSize::new(1000, 680));
        let window = Arc::new(event_loop.create_window(attrs).expect("window"));
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
                self.queue(Some(Some(Point::new(position.x, position.y))), None)
            }
            WindowEvent::CursorLeft { .. } => self.queue(Some(None), None),
            WindowEvent::Focused(false) => self.cancel(),
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
                self.queue(None, Some(state == ElementState::Pressed));
            }
            WindowEvent::ModifiersChanged(m) => {
                let s = m.state();
                self.mods = Mods {
                    shift: s.shift_key(),
                    ctrl: s.control_key(),
                    alt: s.alt_key(),
                    cmd: s.super_key(),
                };
            }
            WindowEvent::MouseWheel { delta, .. } => {
                // winit points the wheel at the viewer; a scroll offset points
                // at the content, so both axes flip.
                let d = match delta {
                    MouseScrollDelta::LineDelta(x, y) => {
                        Point::new(f64::from(x) * -LINE, f64::from(y) * -LINE)
                    }
                    MouseScrollDelta::PixelDelta(p) => Point::new(-p.x, -p.y),
                };
                self.wheel = Point::new(self.wheel.x + d.x, self.wheel.y + d.y);
            }
            WindowEvent::KeyboardInput { ref event, .. } if event.state.is_pressed() => {
                let mods = self.mods;
                match &event.logical_key {
                    WinitKey::Named(n) => {
                        if let Some(key) = named(*n) {
                            // Escape is the UI's while it has a focus to drop;
                            // only an idle Escape closes the gallery.
                            if key == mui::prelude::Key::Escape && !self.any_focus() {
                                event_loop.exit();
                                return;
                            }
                            self.keys.push(KeyPress { key, mods });
                        }
                    }
                    // A shortcut reaches widgets as keys; plain typing reaches
                    // them as text below. Sending both would type every
                    // character twice.
                    WinitKey::Character(s) if mods.ctrl || mods.cmd => {
                        self.keys.extend(s.chars().map(|c| KeyPress {
                            key: mui::prelude::Key::Char(c),
                            mods,
                        }))
                    }
                    _ => {}
                }
                if !mods.ctrl && !mods.cmd {
                    if let Some(t) = event.text.as_ref() {
                        self.text.extend(t.chars().filter(|c| !c.is_control()));
                        self.typed = t.chars().find(|c| !c.is_control());
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                let Some(gpu) = &self.gpu else { return };
                let (size, scale) = (gpu.size(), gpu.window().scale_factor());
                let animating = self.replay(size, scale);
                self.draw();
                if let Some(gpu) = &self.gpu {
                    gpu.window().set_cursor(icon(self.cursor));
                }
                if animating {
                    if let Some(gpu) = &self.gpu {
                        gpu.window().request_redraw();
                    }
                }
                return;
            }
            _ => {}
        }
        if let Some(gpu) = &self.gpu {
            gpu.window().request_redraw();
        }
    }
}

fn main() {
    let event_loop = EventLoop::new().expect("event loop");
    event_loop.set_control_flow(ControlFlow::Wait);
    event_loop.run_app(&mut App::new()).expect("run");
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: (u32, u32) = (1000, 680);

    fn at(x: f64, y: f64, down: bool) -> PointerInput {
        PointerInput {
            pos: Some(Point::new(x, y)),
            primary_down: down,
        }
    }
    fn centre(app: &App, key: &str) -> Point {
        let f = app.ui.scene().unwrap().surface(key).unwrap().frame;
        Point::new(f.x + f.size.width / 2.0, f.y + f.size.height / 2.0)
    }
    fn click(app: &mut App, key: &str) {
        let c = centre(app, key);
        // The release lands in a frame whose tree was built before it; the
        // frame after reads it.
        for down in [false, true, false, false] {
            app.tick(SIZE, 1.0, at(c.x, c.y, down));
        }
    }

    #[test]
    fn every_scene_resolves_and_the_stage_centres_its_specimen() {
        let mut app = App::new();
        for i in 0..app.scenes.len() {
            app.selected = i;
            app.tick(SIZE, 1.0, PointerInput::default());
            let scene = app.ui.scene().unwrap();
            assert!(scene.paint.len() > 3, "scene {i} paints nothing");
            let stage = scene.surface("stage").unwrap().frame;
            let stage_at = scene.keys.iter().position(|k| k == "stage").unwrap();
            let f = scene.surface(&scene.keys[stage_at + 1]).unwrap().frame;
            assert!(
                f.x >= stage.x && f.right() <= stage.right(),
                "scene {i} leaves the stage"
            );
        }
    }

    #[test]
    fn the_scene_list_selects_and_a_typed_key_reaches_the_glyph_scene() {
        let mut app = App::new();
        app.tick(SIZE, 1.0, PointerInput::default());
        click(&mut app, "scene-4");
        assert_eq!(app.selected, 4);
        app.typed = Some('g');
        app.tick(SIZE, 1.0, PointerInput::default());
        assert!(app.scenes[4].overlay().is_some());
    }

    #[test]
    fn a_drag_on_the_stage_pans_and_a_drag_on_a_slider_changes_it() {
        let mut app = App::new();
        app.selected = 3;
        app.tick(SIZE, 1.0, PointerInput::default());
        let before = centre(&app, "widgets");
        let stage = centre(&app, "stage");
        let (x, y) = (stage.x, stage.y - 250.0);
        for (dx, down) in [
            (0.0, false),
            (0.0, true),
            (30.0, true),
            (60.0, true),
            (60.0, false),
            (60.0, false),
        ] {
            app.tick(SIZE, 1.0, at(x + dx, y, down));
        }
        let after = centre(&app, "widgets");
        assert!(
            (after.x - before.x - 60.0).abs() < 1e-6,
            "pan {} vs {}",
            after.x,
            before.x
        );

        let g = centre(&app, "gain");
        for (dx, down) in [
            (0.0, false),
            (0.0, true),
            (40.0, true),
            (80.0, true),
            (80.0, false),
            (80.0, false),
        ] {
            app.tick(SIZE, 1.0, at(g.x + dx, g.y, down));
        }
        assert!(
            centre(&app, "gain").x > g.x + 30.0,
            "thumb did not follow the hand"
        );
    }

    /// The whole new input path in one go: a wheel event reaching the scroll
    /// scene through `Input`, and a press-drag-release reaching the drag scene
    /// as a drop.
    #[test]
    fn the_wheel_scrolls_and_a_drag_between_pills_swaps_them() {
        let mut app = App::new();
        app.selected = 5;
        app.tick(SIZE, 1.0, PointerInput::default());
        let c = centre(&app, "scroll");
        app.tick(
            SIZE,
            1.0,
            Input {
                pointer: at(c.x, c.y, false),
                wheel: Point::new(0.0, 120.0),
                ..Input::default()
            },
        );
        assert!(app.ui.scroll("scroll")[1] > 0.0, "the wheel moved nothing");

        app.selected = 9;
        app.tick(SIZE, 1.0, PointerInput::default());
        let width = |app: &App, key| {
            app.ui
                .scene()
                .unwrap()
                .surface(key)
                .unwrap()
                .frame
                .size
                .width
        };
        // "Osc" against "Filter": the short pill grows when the labels trade.
        let before = width(&app, "pill-0");
        let (a, b) = (centre(&app, "pill-0"), centre(&app, "pill-1"));
        for (p, down) in [(a, false), (a, true), (b, true), (b, false), (b, false)] {
            app.tick(SIZE, 1.0, at(p.x, p.y, down));
        }
        assert!(
            width(&app, "pill-0") > before + 10.0,
            "the drop did not swap the labels"
        );
    }

    #[test]
    fn the_light_toggle_relights_the_palette() {
        let mut app = App::new();
        app.tick(SIZE, 1.0, PointerInput::default());
        let dark = app.ui.scene().unwrap().paint[0].paint.clone();
        click(&mut app, "light");
        app.tick(SIZE, 1.0, PointerInput::default());
        assert!(app.light);
        assert_ne!(app.ui.scene().unwrap().paint[0].paint, dark);
    }
}
