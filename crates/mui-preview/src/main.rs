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

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use host::Gpu;
use mui::geometry::Point;
use mui::prelude::*;
use mui::vello::kurbo::{Affine, Rect, Shape as _, Stroke};
use mui::vello::Canvas as _;
use scenes::PreviewScene;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key as WinitKey, NamedKey};
use winit::window::{CursorIcon, Window, WindowId};

const SIDEBAR: f64 = 240.0;
/// How often `MUI_PREVIEW_THEME` is stat'd, and how many frames the title bar
/// averages before it says anything.
const POLL: Duration = Duration::from_millis(500);
const TITLE_EVERY: u32 = 30;
/// The inspector's own label, in pixels. Not the theme's text size: this is
/// developer chrome and must not move when the theme does.
const LABEL: f64 = 12.0;
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

/// A theme file: `key = value` a line, `#` starts a comment, everything
/// unstated stays [`skin::SKIN`]'s. Returns what failed to parse so the caller
/// can say so once per reload rather than once per frame.
///
/// Eight numbers is the whole surface -- two hues, two chromas, the layer and
/// hover deltas, and the two corner radii -- because those are the knobs you
/// actually turn while looking at the window. Anything else is a recompile.
fn parse_theme(src: &str) -> (Theme, Vec<String>) {
    let mut t = skin::SKIN;
    let mut bad = Vec::new();
    for (n, line) in src.lines().enumerate() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            bad.push(format!("line {}: not `key = value`", n + 1));
            continue;
        };
        let Ok(v) = value.trim().parse::<f64>() else {
            bad.push(format!(
                "line {}: `{}` is not a number",
                n + 1,
                value.trim()
            ));
            continue;
        };
        let f = v as f32;
        match key.trim() {
            "neutral_hue" => t.palette.neutral.hue = f,
            "neutral_chroma" => t.palette.neutral.chroma = f,
            "primary_hue" => t.palette.primary.hue = f,
            "primary_chroma" => t.palette.primary.chroma = f,
            "step" => t.palette.step = f,
            "hover" => t.palette.hover = f,
            "corners_convex" => t.corners.convex = v,
            "corners_concave" => t.corners.concave = v,
            k => bad.push(format!("line {}: unknown key `{k}`", n + 1)),
        }
    }
    // A theme that cannot derive a legible surface would fail every frame from
    // inside `Ui::frame`, three crates from the file that caused it.
    if !t.is_valid() {
        bad.push("not a usable theme; keeping the compiled-in skin".into());
        t = skin::SKIN;
    }
    (t, bad)
}

/// The F12 overlay: every surface frame outlined in translucent primary, and
/// the one under the pointer picked out solid with its key and frame.
///
/// Keys a scene actually wrote draw at 1 px and the tree-path keys (`/0/2`)
/// nobody named draw at 0.5, so a scene's ids stand out of its scaffolding.
fn inspect(
    canvas: &mut impl mui::vello::Canvas,
    scene: &mui::core::ResolvedScene,
    xf: Affine,
    palette: &Palette,
    font: &Arc<[u8]>,
    pointer: Option<Point>,
    height: f64,
) {
    fn outline(f: mui::layout::Frame) -> mui::vello::kurbo::BezPath {
        Rect::new(f.x, f.y, f.right(), f.bottom()).to_path(0.1)
    }
    let primary = palette.primary().to_srgb();
    canvas.set_transform(xf);
    canvas.set_paint(primary.with_alpha(0.45).into());
    for s in scene.surfaces() {
        canvas.set_stroke(Stroke::new(if s.key.starts_with('/') { 0.5 } else { 1.0 }));
        canvas.stroke_path(&outline(s.frame));
    }
    // `keys` is tree order, which is z-order, so the last frame containing the
    // pointer is the one on top. ponytail: rectangles, not the rounded
    // outlines `mui_input::Hit` tests -- close enough to point at a widget.
    let Some(s) = pointer.and_then(|p| {
        scene
            .keys
            .iter()
            .rev()
            .filter_map(|k| scene.surface(k))
            .find(|s| {
                let f = s.frame;
                (f.x..=f.right()).contains(&p.x) && (f.y..=f.bottom()).contains(&p.y)
            })
    }) else {
        return;
    };
    canvas.set_paint(primary.into());
    canvas.set_stroke(Stroke::new(2.0));
    canvas.stroke_path(&outline(s.frame));
    let f = s.frame;
    let label = format!(
        "{}  {:.0} {:.0} {:.0} {:.0}",
        s.key, f.x, f.y, f.size.width, f.size.height
    );
    let Ok(run) = mui_text::text_run(font, &label, LABEL, &[], mui::vello::ARC_TOLERANCE) else {
        return;
    };
    canvas.glyphs(&mui::core::Text {
        font: font.clone(),
        size: LABEL as f32,
        origin: Point::new(SIDEBAR + 12.0, height - 12.0),
        glyphs: run.glyphs.iter().map(|&(id, x)| (id, x as f32)).collect(),
    });
}

struct App {
    ui: Ui,
    /// The gallery's own font, kept so the inspector can set its own labels
    /// without going through the scene.
    font: Arc<[u8]>,
    scenes: Vec<Box<dyn PreviewScene>>,
    selected: usize,
    /// Where the specimen was dragged to, relative to centred.
    pan: Point,
    light: bool,
    /// The F12 inspector, which the sidebar switch also flips.
    frames: bool,
    /// `MUI_PREVIEW_THEME`, the mtime of the last read, and what parsed. The
    /// outer `Option` means never read, so a missing file still reports once.
    theme_path: Option<PathBuf>,
    theme_mtime: Option<Option<SystemTime>>,
    theme_at: Instant,
    theme: Option<Theme>,
    /// Frame cost since the last title update.
    resolve_s: f64,
    paint_s: f64,
    counted: u32,
    counted_at: Instant,
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
    /// The preview is its own clipboard: a copy comes back on
    /// `Frame::clipboard` and rides back in on the next `Input`. ponytail:
    /// no OS clipboard, add `arboard` here if cross-app paste matters.
    clipboard: String,
    last: Instant,
    gpu: Option<Gpu>,
    /// Arc-to-cubic conversions reused across frames; a still gallery
    /// re-encodes without reconverting a single path.
    paths: mui::vello::PathCache,
}

impl App {
    fn new() -> Self {
        let font: Arc<[u8]> = Arc::from(epaint_default_fonts::HACK_REGULAR);
        Self {
            ui: Ui::new(skin::SKIN).font(font.clone()),
            font,
            scenes: scenes::all(),
            selected: 0,
            pan: Point::new(0.0, 0.0),
            light: false,
            frames: false,
            theme_path: std::env::var_os("MUI_PREVIEW_THEME").map(PathBuf::from),
            theme_mtime: None,
            theme_at: Instant::now(),
            theme: None,
            resolve_s: 0.0,
            paint_s: 0.0,
            counted: 0,
            counted_at: Instant::now(),
            events: Vec::new(),
            pointer: PointerInput::default(),
            wheel: Point::new(0.0, 0.0),
            keys: Vec::new(),
            text: String::new(),
            mods: Mods::default(),
            cursor: Cursor::Arrow,
            typed: None,
            clipboard: String::new(),
            last: Instant::now(),
            gpu: None,
            paths: mui::vello::PathCache::new(),
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
        // A theme file, once one has parsed, replaces the compiled-in skin
        // wholesale; the mode is still the sidebar's to say.
        let theme = self.theme.unwrap_or(skin::SKIN);
        ui.theme = Theme {
            palette: match self.theme {
                Some(t) => t
                    .palette
                    .with_mode(if self.light { Mode::Light } else { Mode::Dark }),
                None => skin::skin(self.light),
            },
            ..theme
        };
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
        // Panning is unbounded by design -- drag the specimen wherever -- so
        // the stage clips it instead; without this it paints over the sidebar,
        // which is drawn first.
        let stage = overlay([specimen])
            .grow(1.0)
            .clip()
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
        input.clipboard = Some(self.clipboard.clone());
        self.ui.scale = Some(scale);
        let root = self.tree(w, h);
        let (animating, cursor) = match self.ui.frame(root, Some(Size::new(w, h)), input, dt) {
            // Destructured first: `f` borrows `self.ui`, and handing the
            // copy to the scene needs `self` back.
            Ok(f) => {
                let (animating, cursor, copied) = (f.animating, f.cursor, f.clipboard.clone());
                if let Some(s) = copied {
                    self.scenes[self.selected].clipboard(&s);
                    self.clipboard = s;
                }
                (animating, cursor)
            }
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

    /// Re-read the theme file when its mtime moved; `true` when the theme
    /// changed. `std::fs` and `std::time` are the preview's privilege: it is
    /// the native binary, and this is a dev aid, not something a plugin ships.
    fn reload(&mut self) -> bool {
        let Some(path) = self.theme_path.clone() else {
            return false;
        };
        if self.theme_at.elapsed() < POLL {
            return false;
        }
        self.theme_at = Instant::now();
        let mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
        if self.theme_mtime == Some(mtime) {
            return false;
        }
        self.theme_mtime = Some(mtime);
        match std::fs::read_to_string(&path) {
            Ok(src) => {
                let (theme, bad) = parse_theme(&src);
                for b in bad {
                    eprintln!("{}: {b}", path.display());
                }
                self.theme = Some(theme);
                true
            }
            Err(e) => {
                eprintln!("{}: {e}", path.display());
                false
            }
        }
    }

    /// The frame cost, averaged over [`TITLE_EVERY`] frames, in the title bar:
    /// the one readout that costs no pixels and no scene.
    fn title(&mut self, resolve: f64, paint: f64) {
        self.resolve_s += resolve;
        self.paint_s += paint;
        self.counted += 1;
        if self.counted < TITLE_EVERY {
            return;
        }
        let n = f64::from(self.counted);
        let fps = n / self.counted_at.elapsed().as_secs_f64().max(1e-9);
        if let Some(gpu) = &self.gpu {
            gpu.window().set_title(&format!(
                "mui preview \u{2014} {:.1} ms resolve, {:.1} ms paint, {fps:.0} fps",
                self.resolve_s / n * 1e3,
                self.paint_s / n * 1e3
            ));
        }
        (self.resolve_s, self.paint_s, self.counted) = (0.0, 0.0, 0);
        self.counted_at = Instant::now();
    }

    fn draw(&mut self) {
        let Some(gpu) = &mut self.gpu else { return };
        let scale = gpu.window().scale_factor();
        let height = f64::from(gpu.size().1) / scale;
        let Some(scene) = self.ui.scene() else { return };
        let mut canvas = gpu.begin();
        let xf = Affine::scale(scale);
        if let Err(e) = mui::vello::paint_cached(&mut canvas, scene, xf, &mut self.paths) {
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
            let pointer = self
                .pointer
                .pos
                .map(|p| Point::new(p.x / scale, p.y / scale));
            inspect(
                &mut canvas,
                scene,
                xf,
                &self.ui.theme.palette,
                &self.font,
                pointer,
                height,
            );
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

    /// The theme file is the only thing that changes with no event behind it,
    /// so it is the only reason this loop ever wakes on a timer.
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.theme_path.is_none() {
            return;
        }
        if self.reload() {
            if let Some(gpu) = &self.gpu {
                gpu.window().request_redraw();
            }
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + POLL));
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
                    // The inspector is the host's, not the UI's: it never
                    // reaches a widget.
                    WinitKey::Named(NamedKey::F12) => self.frames = !self.frames,
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
                let start = Instant::now();
                let animating = self.replay(size, scale);
                let resolved = Instant::now();
                self.draw();
                self.title(
                    resolved.duration_since(start).as_secs_f64(),
                    resolved.elapsed().as_secs_f64(),
                );
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
            let stage_at = scene.keys.iter().position(|k| &**k == "stage").unwrap();
            let f = scene.surface(&scene.keys[stage_at + 1]).unwrap().frame;
            assert!(
                f.x >= stage.x && f.right() <= stage.right(),
                "scene {i} leaves the stage"
            );
        }
    }

    /// The preview is the clipboard: a copy inside the window has to come
    /// back out of `Frame::clipboard` or paste has nothing to paste.
    #[test]
    fn a_copy_in_the_select_scene_lands_in_the_preview_clipboard() {
        let mut app = App::new();
        app.selected = app.scenes.len() - 1;
        app.tick(SIZE, 1.0, PointerInput::default());
        click(&mut app, "sel-field");
        let ctrl = |c: char| KeyPress {
            key: mui::prelude::Key::Char(c),
            mods: Mods {
                ctrl: true,
                ..Mods::default()
            },
        };
        app.keys = vec![ctrl('a'), ctrl('c')];
        app.replay(SIZE, 1.0);
        // `set_clipboard` is answered by the frame after the one that asked.
        app.tick(SIZE, 1.0, PointerInput::default());
        assert_eq!(app.clipboard, "select me");
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
    fn a_theme_file_is_a_few_numbers_and_says_what_it_could_not_read() {
        let (t, bad) = parse_theme("primary_hue = 12.5\nnonsense\nstep=0.08 # a comment\n");
        assert_eq!(t.palette.primary.hue, 12.5);
        assert_eq!(t.palette.step, 0.08);
        // Everything unstated is still the compiled-in skin.
        assert_eq!(t.palette.neutral.hue, skin::SKIN.palette.neutral.hue);
        assert_eq!(bad.len(), 1, "{bad:?}");
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
