//! MUI preview gallery: a dev-only host that puts a [`scenes::PreviewScene`]
//! on screen through the same `mui::Ui` a plugin would use.
//!
//! The whole window, sidebar included, is one styled tree resolved once a
//! frame, hosted directly on our owned `moose-baseview` + `mui-baseview`
//! standalone window layer (X11, native Wayland `xdg_toplevel`, Win32, macOS
//! Cocoa) with direct AccessKit accessibility and zero `winit`.
//!
//! Run: `cargo run -p mui-preview`
#![forbid(unsafe_code)]

mod headless;
mod host;
mod scenes;
mod skin;
mod tester;

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use host::Gpu;
use mui::Clipboard as _;
use mui::geometry::{Point, Vec2};
use mui::host::NativeKey;
use mui::prelude::*;
use mui::vello::kurbo::{Affine, Rect, Shape as _, Stroke};
use mui_baseview::baseview::dpi::{PhysicalPosition, PhysicalSize};
use mui_baseview::baseview::{
    DamageArea, Event, EventStatus, FrameDemand, HandlerError, ImeConfiguration, MouseCursor,
    MouseEvent, ScrollDelta, WindowContext, WindowEvent, WindowHandler, WindowSize,
};
use mui_baseview::native::{ime_event, key_event, mods, mouse_button, native_cursor};
use mui_baseview::standalone::StandaloneAccessibility;
use scenes::PreviewScene;

/// Material Symbols, the playground's 52-icon subset (`save`, `lock`, ...).
static ICONS: std::sync::LazyLock<Font> = std::sync::LazyLock::new(|| {
    Font::new(&include_bytes!("../../mui-playground/fonts/MaterialSymbolsOutlined-ui.ttf")[..])
        .expect("the bundled icon font parses")
});

const SIDEBAR: f64 = 240.0;
/// How often `MUI_PREVIEW_THEME` is stat'd, and how many frames the title bar
/// averages before it says anything.
const POLL: Duration = Duration::from_millis(500);
const TITLE_EVERY: u32 = 30;
/// The inspector's own label, in pixels. Not the theme's text size: this is
/// developer chrome and must not move when the theme does.
const LABEL: f64 = 12.0;
/// One wheel line in logical pixels. baseview reports lines, MUI scrolls pixels.
const LINE: f64 = 40.0;

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
            "corners_selector" => t.corners.selector = v,
            "corners_field" => t.corners.field = v,
            "corners_box" => t.corners.box_ = v,
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
    canvas: &mut (impl mui::vello::Canvas + ?Sized),
    scene: &mui::scene::ResolvedScene,
    xf: Affine,
    palette: &Palette,
    font: &Font,
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
        canvas.set_stroke(Stroke::new(if mui::layout::Id::is_named(&s.key) {
            1.0
        } else {
            0.5
        }));
        canvas.stroke_path(&outline(s.frame));
    }
    // `keys` is tree order, which is z-order, so the last frame containing the
    // pointer is the one on top.
    let Some(s) = pointer.and_then(|p| {
        scene.surfaces().rev().find(|s| {
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
    let fonts = std::slice::from_ref(font);
    let Ok(run) = mui_text::shape_run(fonts, &label, LABEL, &[]) else {
        return;
    };
    canvas.glyphs(&mui::scene::Text {
        fonts: fonts.into(),
        size: LABEL as f32,
        origin: Point::new(SIDEBAR + 12.0, height - 12.0),
        glyphs: run
            .glyphs
            .iter()
            .map(|g| mui::scene::TextGlyph {
                id: g.id,
                x: g.x as f32,
                y: g.y as f32,
                font: g.font,
            })
            .collect(),
        axes: Axes::default(),
        hint: true,
        font_coords: vec![Arc::from(&[][..])].into(),
    });
}

struct App {
    tester: Option<tester::Tester>,
    frame_error: Option<String>,
    ui: Ui,
    /// The gallery's own font, kept so the inspector can set its own labels
    /// without going through the scene.
    font: Font,
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
    /// Pointer samples since the last frame, in physical pixels.
    events: Vec<PointerInput>,
    pointer: PointerInput,
    /// Everything non-pointer collected since the last frame: it rides on the
    /// last pointer sample of the batch.
    wheel: Vec2,
    keys: Vec<KeyPress>,
    text: String,
    ime: Vec<Ime>,
    /// Where the focused field wants the candidate window.
    ime_area: Option<(Point, mui::scene::Size)>,
    applied_ime: Option<Option<ImeConfiguration>>,
    applied_cursor: Option<MouseCursor>,
    mods: Mods,
    cursor: Cursor,
    typed: Option<char>,
    /// In-process clipboard mirror (also synced with the OS clipboard when a
    /// native window is attached).
    clipboard: String,
    os_clipboard: mui_baseview::Clipboard,
    last: Instant,
    repaint_at: Option<Instant>,
    paint_retry_at: Option<Instant>,
    dirty: bool,
    animating: bool,
    visible: bool,
    gpu: Option<Gpu>,
    access: Option<StandaloneAccessibility>,
}

impl App {
    fn new() -> Self {
        let font = Font::new(epaint_default_fonts::HACK_REGULAR).expect("bundled Hack parses");
        Self {
            tester: None,
            frame_error: None,
            ui: {
                let ui = Ui::new(skin::SKIN).font(font.clone()).fallback_font(
                    Font::new(epaint_default_fonts::NOTO_EMOJI_REGULAR)
                        .expect("bundled Noto Emoji parses"),
                );
                ui.gpu_welding()
            },
            font,
            // `MUI_PREVIEW_SCENE=Scroll` opens on a scene by name, for a
            // screenshot with no clicking.
            selected: std::env::var("MUI_PREVIEW_SCENE")
                .ok()
                .and_then(|n| scenes::all().iter().position(|s| s.name() == n))
                .unwrap_or(0),
            scenes: scenes::all(),
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
            wheel: Vec2::new(0.0, 0.0),
            keys: Vec::new(),
            text: String::new(),
            ime: Vec::new(),
            ime_area: None,
            applied_ime: None,
            applied_cursor: None,
            mods: Mods::default(),
            cursor: Cursor::Arrow,
            typed: None,
            clipboard: String::new(),
            os_clipboard: mui_baseview::Clipboard::default(),
            last: Instant::now(),
            repaint_at: None,
            paint_retry_at: None,
            dirty: true,
            animating: false,
            visible: true,
            gpu: None,
            access: None,
        }
    }

    /// One pointer sample. `button` is the one whose state changed, if any;
    /// the modifiers ride on every sample, because a gesture reads them long
    /// after the key that set them was pressed.
    fn queue(&mut self, pos: Option<Option<Point>>, button: Option<(Button, bool)>) {
        self.pointer = PointerInput {
            pos: pos.unwrap_or(self.pointer.pos),
            buttons: match button {
                Some((b, down)) => self.pointer.buttons.set(b, down),
                None => self.pointer.buttons,
            },
            mods: self.mods,
        };
        self.events.push(self.pointer);
    }

    /// Is any focusable surface focused? Escape belongs to the UI when one is.
    fn any_focus(&self) -> bool {
        self.ui
            .scene()
            .is_some_and(|s| s.surfaces().any(|s| self.ui.focused(&s.key)))
    }

    fn cancel(&mut self) {
        self.events.clear();
        self.keys.clear();
        self.text.clear();
        self.ime.clear();
        self.pointer = PointerInput::default();
        self.ui.cancel();
    }

    /// The window as one tree: sidebar, then a stage the specimen centres in.
    fn tree(&mut self, w: f64, h: f64) -> El {
        let ui = &mut self.ui;
        // A theme file, once one has parsed, replaces the compiled-in skin
        // wholesale; the mode is still the sidebar's to say.
        let mut theme = self.theme.clone().unwrap_or(skin::SKIN);
        // The playground's Material Symbols subset, so `icon(sym::..)` in a
        // scene draws its glyph, unless a theme file brings its own face.
        theme.icon_font.get_or_insert_with(|| ICONS.clone());
        ui.set_theme(Theme {
            palette: match &self.theme {
                Some(t) => t
                    .palette
                    .with_mode(if self.light { Mode::Light } else { Mode::Dark }),
                None => skin::skin(self.light),
            },
            ..theme
        });
        if let Some(c) = self.typed.take() {
            self.scenes[self.selected].key(c);
        }
        for i in 0..self.scenes.len() {
            if ui.get(format!("scene-{i}")).clicked {
                ui.set_tips(mui::TipPolicy::default());
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
        // The list grows with the gallery, so it takes the slack and scrolls;
        // the switches below it stay put instead of being squeezed to nothing.
        side.push(
            col((0..self.scenes.len()).map(|i| {
                let on = i == self.selected;
                let item = button(ui, format!("scene-{i}"), self.scenes[i].name()).el;
                item.variant(if on { Variant::Solid } else { Variant::Soft })
                    .size(S)
                    .el()
                    .radius(Corner::Field)
                    .w(pct(100.))
            }))
            .gap(S)
            .scroll()
            .grow(1.0)
            .id("scene-list"),
        );
        let scene = &mut self.scenes[self.selected];
        let mut switch = |label: &str, id: &str, v: &mut bool| {
            row([
                text(label).fill(Role::Dim),
                spacer(),
                toggle(ui, id, label, v).el.into_el(),
            ])
            .align(Align::Center)
        };
        side.push(switch("light", "light", &mut self.light));
        side.push(switch("frames", "frames", &mut self.frames));
        side.extend(scene.controls(ui));
        let sidebar = col(side).gap(S).pad(M).w(SIDEBAR).fill(Role::Surface);
        let specimen = scene.specimen(ui).centered_at(self.pan.x, self.pan.y);
        // Panning is unbounded by design -- drag the specimen wherever -- so
        // the stage clips it instead; without this it paints over the sidebar,
        // which is drawn first.
        let stage = stack([specimen])
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
        let dt = now.duration_since(self.last).as_secs_f64();
        self.last = now;
        let clipboard = if self.gpu.is_some() {
            self.os_clipboard
                .get()
                .unwrap_or_else(|| self.clipboard.clone())
        } else {
            self.clipboard.clone()
        };
        input.clipboard = Some(clipboard);
        self.ui.set_scale(Some(scale));
        if let Some(gpu) = &mut self.gpu {
            gpu.update();
            self.ui
                .set_gpu_welding_available(gpu.rendering_mode() == "gpu");
        }
        let root = self.tree(w, h);
        let (animating, cursor) = match self.ui.frame(root, Some(Size::new(w, h)), input, dt) {
            // Destructured first: `f` borrows `self.ui`, and handing the
            // copy to the scene needs `self` back.
            Ok(f) => {
                let (animating, cursor, copied) = (f.animating, f.cursor, f.clipboard.clone());
                self.ime_area = f.ime;
                self.repaint_at = f.repaint_after.and_then(|delay| now.checked_add(delay));
                if let Some(s) = copied {
                    self.scenes[self.selected].clipboard(&s);
                    if self.gpu.is_some() {
                        self.os_clipboard.set(&s);
                    }
                    self.clipboard = s;
                }
                (animating, cursor)
            }
            Err(e) => {
                eprintln!("frame: {e}");
                self.frame_error = Some(e.to_string());
                (false, Cursor::Arrow)
            }
        };
        self.cursor = cursor;
        animating
    }

    /// Allow an input method while a field is focused, and keep the
    /// candidate window under its caret. Only calls baseview on change so an
    /// active composition is never dropped.
    fn apply_ime(&mut self, scale: f64) {
        let Some(gpu) = self.gpu.as_ref() else {
            return;
        };
        let config = self.ime_area.and_then(|(at, size)| {
            let state = self.ui.text_input_state()?;
            Some(ImeConfiguration {
                id: state.id,
                position: PhysicalPosition::new(at.x * scale, at.y * scale),
                size: PhysicalSize::new(size.width * scale, size.height * scale),
                text: state.text,
                selection: state.selection,
                marked: state.marked,
            })
        });
        if self.applied_ime.as_ref() != Some(&config) {
            gpu.window().set_ime_configuration(config.clone());
            self.applied_ime = Some(config);
        }
    }

    fn replay(&mut self, size: (u32, u32), scale: f64) -> bool {
        let rest = Input {
            wheel: std::mem::take(&mut self.wheel),
            keys: std::mem::take(&mut self.keys),
            text: std::mem::take(&mut self.text),
            ime: std::mem::take(&mut self.ime),
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
            gpu.set_title(&format!(
                "mui preview \u{2014} {:.1} ms resolve, {:.1} ms paint, {fps:.0} fps",
                self.resolve_s / n * 1e3,
                self.paint_s / n * 1e3
            ));
        }
        (self.resolve_s, self.paint_s, self.counted) = (0.0, 0.0, 0);
        self.counted_at = Instant::now();
    }

    /// Hand the screen reader this frame's tree. Nothing is built when
    /// nothing is listening.
    fn publish(&mut self) {
        if let Some(access) = self.access.as_mut() {
            access.publish(&self.ui);
        }
    }

    fn draw(&mut self) -> Result<Option<mui::vello::effects::EffectStats>, String> {
        let Some(gpu) = &mut self.gpu else {
            return Ok(None);
        };
        let scale = gpu.scale_factor();
        let height = f64::from(gpu.size().1) / scale;
        let Some(scene) = self.ui.scene() else {
            return Ok(None);
        };
        let xf = Affine::scale(scale);
        let extra = self.scenes[self.selected].overlay();
        let wants_overlay = self.frames || extra.is_some();
        let draw_extra = |canvas: &mut dyn mui::vello::Canvas| {
            if let Some((key, path)) = extra
                && let (Some(s), Ok(bez)) = (
                    scene.surface(key),
                    mui::vello::bez_path(&path, mui::vello::ARC_TOLERANCE),
                )
            {
                canvas.set_transform(xf * Affine::translate((s.frame.x, s.frame.y)));
                canvas.set_paint(
                    self.ui
                        .theme()
                        .palette
                        .on(self.ui.theme().palette.raised())
                        .to_srgb()
                        .into(),
                );
                canvas.fill_path(&bez);
            }
            if self.frames {
                let pointer = self
                    .pointer
                    .pos
                    .map(|p| Point::new(p.x / scale, p.y / scale));
                inspect(
                    canvas,
                    scene,
                    xf,
                    &self.ui.theme().palette,
                    &self.font,
                    pointer,
                    height,
                );
            }
        };
        if wants_overlay {
            gpu.present_with_overlay(scene, xf, draw_extra)
        } else {
            gpu.present(scene, xf)
        }
    }
}

struct PreviewHandler {
    app: RefCell<App>,
    cx: WindowContext,
    outcome: Arc<Mutex<Result<(), String>>>,
    finalized: Cell<bool>,
}

impl PreviewHandler {
    fn finish_and_teardown(&self) {
        let Ok(mut app) = self.app.try_borrow_mut() else {
            return;
        };
        if self.finalized.replace(true) {
            return;
        }
        // Drop native accessibility and GPU surfaces before the window closes.
        drop(app.access.take());
        drop(app.gpu.take());
        let result = if let Some(error) = app.frame_error.take() {
            Err(error)
        } else {
            app.tester.as_mut().map_or(Ok(()), tester::Tester::finish)
        };
        *self
            .outcome
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = result;
    }
}

impl Drop for PreviewHandler {
    fn drop(&mut self) {
        self.finish_and_teardown();
    }
}

impl WindowHandler for PreviewHandler {
    fn frame_demand(&self) -> FrameDemand {
        let Ok(app) = self.app.try_borrow() else {
            return FrameDemand::Continuous;
        };
        if !app.visible {
            return FrameDemand::Idle;
        }
        let now = Instant::now();
        if let Some(tester) = &app.tester {
            if tester.waiting_capture() {
                return app.repaint_at.map_or(
                    FrameDemand::At(now + Duration::from_millis(25)),
                    FrameDemand::At,
                );
            }
            return FrameDemand::At(now);
        }
        if app.animating {
            return FrameDemand::Continuous;
        }
        if app.dirty
            || app
                .access
                .as_ref()
                .is_some_and(StandaloneAccessibility::wants_tree)
        {
            return FrameDemand::At(now);
        }
        let theme = app
            .theme_path
            .as_ref()
            .and_then(|_| app.theme_at.checked_add(POLL));
        let renderer = app.gpu.as_ref().and_then(|gpu| gpu.next_wake(now));
        [app.repaint_at, app.paint_retry_at, theme, renderer]
            .into_iter()
            .flatten()
            .min()
            .map_or(FrameDemand::Idle, FrameDemand::At)
    }

    fn on_frame(&self) -> Result<(), HandlerError> {
        let mut app = self.app.borrow_mut();
        if let Some(access) = app.access.as_mut() {
            access.update_bounds(&self.cx);
        }
        let mut a11y_dirty = false;
        if let Some(mut access) = app.access.take() {
            if access.apply(&mut app.ui) || access.wants_tree() {
                a11y_dirty = true;
            }
            app.access = Some(access);
        }
        if a11y_dirty {
            app.dirty = true;
        }
        if app.reload() {
            app.dirty = true;
        }
        if let Some(mut tester) = app.tester.take() {
            let resize = tester.draw(&mut app, &self.cx);
            drop(app);
            if let Some((logical, event)) = resize {
                // AppKit can call resized synchronously during the resize.
                tester.start_case(&self.cx, logical, &event);
            }
            self.app.borrow_mut().tester = Some(tester);
            return Ok(());
        }
        if !app.visible {
            return Ok(());
        }
        let now = Instant::now();
        if app.repaint_at.is_some_and(|at| at <= now) {
            app.repaint_at = None;
            app.dirty = true;
        }
        if let Some(gpu) = &mut app.gpu {
            gpu.update();
        }
        let Some(gpu) = &app.gpu else {
            return Ok(());
        };
        let (size, scale) = (gpu.size(), gpu.scale_factor());
        app.dirty = false;
        let start = Instant::now();
        let animating = app.replay(size, scale);
        app.animating = animating;
        let resolved = Instant::now();
        if app.paint_retry_at.is_none_or(|at| now >= at) {
            app.paint_retry_at = match app.draw() {
                Ok(_) => None,
                Err(error) => {
                    eprintln!("Preview paint: {error}");
                    if let Some(gpu) = &mut app.gpu {
                        gpu.invalidate();
                    }
                    Some(Instant::now() + Duration::from_millis(100))
                }
            };
        }
        app.title(
            resolved.duration_since(start).as_secs_f64(),
            resolved.elapsed().as_secs_f64(),
        );
        app.publish();
        let cursor = native_cursor(app.cursor);
        if app.applied_cursor != Some(cursor) {
            let _ = self.cx.set_mouse_cursor(cursor);
            app.applied_cursor = Some(cursor);
        }
        app.apply_ime(scale);
        if animating {
            self.cx.request_redraw();
        }
        Ok(())
    }

    fn damage(&self, _: DamageArea<'_>) {
        if let Ok(mut app) = self.app.try_borrow_mut() {
            if let Some(gpu) = &mut app.gpu {
                gpu.invalidate();
            }
            app.dirty = true;
        }
    }

    fn resized(&self, size: WindowSize) -> Result<(), HandlerError> {
        let mut app = self.app.borrow_mut();
        app.visible = size.physical.width > 0 && size.physical.height > 0;
        if let Some(gpu) = &mut app.gpu {
            gpu.resize(size.physical.width, size.physical.height);
        }
        app.dirty = true;
        self.cx.request_redraw();
        Ok(())
    }

    fn on_event(&self, event: Event) -> EventStatus {
        let mut app = self.app.borrow_mut();
        match event {
            Event::Window(WindowEvent::WillClose) => {
                drop(app);
                self.finish_and_teardown();
                return EventStatus::Captured;
            }
            Event::Window(WindowEvent::Focused) => {
                if let Some(access) = app.access.as_mut() {
                    access.focus(true);
                }
            }
            Event::Window(WindowEvent::Unfocused) => {
                if let Some(access) = app.access.as_mut() {
                    access.focus(false);
                }
                app.cancel();
            }
            Event::Window(WindowEvent::RedrawRequested) => {
                if let Some(gpu) = &mut app.gpu {
                    gpu.invalidate();
                }
            }
            Event::Window(WindowEvent::ScaleFactorChanged(_)) => {
                let size = self.cx.size().physical;
                if let Some(gpu) = &mut app.gpu {
                    gpu.resize(size.width, size.height);
                }
            }
            Event::Mouse(MouseEvent::CursorMoved {
                position,
                modifiers,
            }) => {
                app.mods = mods(modifiers);
                app.queue(Some(Some(Point::new(position.x, position.y))), None);
            }
            Event::Mouse(MouseEvent::CursorLeft) => {
                app.queue(Some(None), None);
            }
            Event::Mouse(MouseEvent::ButtonPressed { button, modifiers }) => {
                app.mods = mods(modifiers);
                if let Some(b) = mouse_button(button) {
                    app.queue(None, Some((b, true)));
                } else {
                    return EventStatus::Ignored;
                }
            }
            Event::Mouse(MouseEvent::ButtonReleased { button, modifiers }) => {
                app.mods = mods(modifiers);
                if let Some(b) = mouse_button(button) {
                    app.queue(None, Some((b, false)));
                } else {
                    return EventStatus::Ignored;
                }
            }
            Event::Mouse(MouseEvent::WheelScrolled { delta, modifiers }) => {
                app.mods = mods(modifiers);
                app.pointer.mods = app.mods;
                // baseview points the wheel at the viewer; a scroll offset
                // points at the content, so both axes flip.
                let d = match delta {
                    ScrollDelta::Lines { x, y } => {
                        Point::new(f64::from(x) * -LINE, f64::from(y) * -LINE)
                    }
                    ScrollDelta::Pixels { x, y } => Point::new(-f64::from(x), -f64::from(y)),
                };
                app.wheel = Vec2::new(app.wheel.x + d.x, app.wheel.y + d.y);
            }
            Event::Ime(e) => {
                app.ime.push(ime_event(&e));
            }
            Event::Keyboard(key) => {
                let translated = key_event(&key);
                app.mods = translated.mods;
                app.pointer.mods = app.mods;
                if translated.down {
                    // The inspector is the host's, not the UI's: it never
                    // reaches a widget.
                    if format!("{:?}", key.key) == "Named(F12)" {
                        app.frames = !app.frames;
                    } else {
                        match translated.key {
                            NativeKey::Named(k) => {
                                // Escape is the UI's while it has a focus to drop;
                                // only an idle Escape closes the gallery.
                                if k == mui::prelude::Key::Escape && !app.any_focus() {
                                    self.cx.request_close();
                                    return EventStatus::Captured;
                                }
                                app.keys.push(KeyPress {
                                    key: k,
                                    mods: translated.mods,
                                });
                            }
                            NativeKey::Text(ref s)
                                if translated.mods.ctrl || translated.mods.cmd =>
                            {
                                app.keys.extend(s.chars().map(|c| KeyPress {
                                    key: mui::prelude::Key::Char(c),
                                    mods: translated.mods,
                                }));
                            }
                            NativeKey::Text(ref s) => {
                                app.text.extend(s.chars().filter(|c| !c.is_control()));
                                app.typed = s.chars().find(|c| !c.is_control());
                            }
                            NativeKey::Modifier(_) | NativeKey::Other => {}
                        }
                    }
                }
            }
            _ => return EventStatus::Ignored,
        }
        app.dirty = true;
        self.cx.request_redraw();
        EventStatus::Captured
    }
}

fn main() -> Result<(), String> {
    // Agent tooling: no window, no GPU. See `mui-preview --help`.
    let args: Vec<String> = std::env::args().skip(1).collect();
    if mui_headless::wants(&args) {
        std::process::exit(mui_headless::main(headless::all()));
    }
    let tester = std::env::args()
        .any(|arg| arg == "--test-ui")
        .then(tester::Tester::new)
        .transpose()?;
    let outcome = Arc::new(Mutex::new(Ok(())));
    let run_outcome = Arc::clone(&outcome);
    let settings = mui_baseview::standalone::settings("MUI preview", (1000, 680));
    mui_baseview::standalone::run_window(settings, move |cx| {
        let mut app = App::new();
        app.tester = tester;
        app.gpu = Some(Gpu::try_new(cx.clone()).map_err(std::io::Error::other)?);
        app.access = StandaloneAccessibility::attach(&cx);
        Ok(PreviewHandler {
            app: RefCell::new(app),
            cx,
            outcome: run_outcome,
            finalized: Cell::new(false),
        })
    })?;
    outcome
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: (u32, u32) = (1000, 680);

    fn at(x: f64, y: f64, down: bool) -> PointerInput {
        PointerInput {
            pos: Some(Point::new(x, y)),
            buttons: Buttons::default().set(Button::Primary, down),
            ..PointerInput::default()
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
    fn changing_scenes_restores_default_tips() {
        let mut app = App::new();
        app.tick(SIZE, 1.0, PointerInput::default());
        app.ui.set_tips(mui::TipPolicy::Off);
        click(&mut app, "scene-1");
        assert_eq!(app.ui.tips(), mui::TipPolicy::default());
    }

    /// The widgets describe themselves, so the tree a screen reader gets has
    /// real roles with names, not a pile of groups.
    #[test]
    fn the_gallery_reports_a_named_button_and_slider() {
        use mui_access::accesskit::Role;
        let mut app = App::new();
        app.selected = app
            .scenes
            .iter()
            .position(|s| s.name() == "Widgets")
            .unwrap();
        app.tick(SIZE, 1.0, PointerInput::default());
        let u = mui_access::tree_update(app.ui.scene().unwrap(), None, 1.0);
        let named = |role| {
            u.nodes
                .iter()
                .any(|(_, n)| n.role() == role && n.label().is_some_and(|l| !l.is_empty()))
        };
        assert!(named(Role::Button), "no named button");
        assert!(named(Role::Slider), "no named slider");
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
            let stage_at = scene.surfaces().position(|s| &*s.key == "stage").unwrap();
            let f = scene.surfaces().nth(stage_at + 1).unwrap().frame;
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
        app.selected = app
            .scenes
            .iter()
            .position(|s| s.name() == "Select")
            .expect("the Select scene");
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

        // The whole lane is the slider: its value is what moves.
        let gain = |app: &App| {
            let s = app.ui.scene().unwrap().surface("gain").unwrap();
            match s.semantics.as_ref().map(|s| &s.role) {
                Some(mui::scene::A11y::Slider { value, .. }) => *value,
                _ => panic!("gain is a slider"),
            }
        };
        let (g, v) = (centre(&app, "gain"), gain(&app));
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
        assert!(gain(&app) > v, "the value did not follow the hand");
    }

    /// The whole new input path in one go: a wheel event reaching the scroll
    /// scene through `Input`, and a press-drag-release reaching the drag scene
    /// as a drop.
    #[test]
    fn the_wheel_scrolls_and_a_drag_between_pills_swaps_them() {
        let mut app = App::new();
        let pick = |app: &App, name| {
            app.scenes
                .iter()
                .position(|s| s.name() == name)
                .expect(name)
        };
        app.selected = pick(&app, "Scroll");
        app.tick(SIZE, 1.0, PointerInput::default());
        let c = centre(&app, "scroll");
        app.tick(
            SIZE,
            1.0,
            Input {
                pointer: at(c.x, c.y, false),
                wheel: Vec2::new(0.0, 120.0),
                ..Input::default()
            },
        );
        assert!(app.ui.scroll("scroll")[1] > 0.0, "the wheel moved nothing");

        app.selected = pick(&app, "Drag");
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
