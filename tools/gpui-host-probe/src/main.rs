//! Standalone native GPUI presentation of complete Vello CPU scenes.
//! This probes a full-frame pixel bridge; it does not embed a plugin window.

#[allow(dead_code, unexpected_cfgs)]
#[path = "../../../crates/mui-preview/src/scenes.rs"]
mod scenes;

#[allow(dead_code)]
mod material_fixture {
    include!("../../../crates/mui-material/tests/material_welding.rs");

    pub fn specimen() -> El {
        tree(Weld::all())
    }
}

struct Material;
impl scenes::PreviewScene for Material {
    fn name(&self) -> &'static str {
        "Baked material regression fixture"
    }
    fn about(&self) -> &'static str {
        "The exact material_welding.rs Weld::all plate fixture."
    }
    fn specimen(&mut self, _: &mut Ui) -> mui::prelude::El {
        material_fixture::specimen()
    }
}

use anyhow::{Result, anyhow};
use gpui::{
    App, Bounds, Context, FocusHandle, KeyDownEvent, Modifiers, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, ObjectFit, RenderImage, Window, WindowBounds, WindowOptions, div,
    img, prelude::*, px, size,
};
use mui::prelude::{Button, Input, Key, KeyPress, Mods, Point, PointerInput, Ui};
use mui_vello::{kurbo::Affine, software::Renderer};
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    fs::File,
    hash::{Hash, Hasher},
    io::Write,
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};

type Journal = Rc<RefCell<Option<File>>>;

fn record(journal: &Journal, event: serde_json::Value) -> Result<()> {
    eprintln!("{event}");
    if let Some(file) = journal.borrow_mut().as_mut() {
        writeln!(file, "{event}")?;
        file.flush()?;
    }
    Ok(())
}

#[derive(Default)]
struct TestRun {
    step: u8,
    callbacks: u32,
    checksum: u64,
    extent: (u32, u32),
    capture_requested: Option<u8>,
    captures: u8,
}

struct Probe {
    scene: Box<dyn scenes::PreviewScene>,
    ui: Ui,
    raster: Renderer,
    image: Option<Arc<RenderImage>>,
    pointer: PointerInput,
    pending: VecDeque<Input>,
    focus: FocusHandle,
    last: Instant,
    started: Instant,
    frames: u64,
    test: Option<TestRun>,
    journal: Journal,
    completed: Rc<Cell<bool>>,
    failure: Rc<RefCell<Option<String>>>,
    capture: Option<std::path::PathBuf>,
}

fn checksum(pixels: &[u8]) -> u64 {
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    pixels.hash(&mut hash);
    hash.finish()
}

fn mods(value: Modifiers) -> Mods {
    Mods {
        shift: value.shift,
        ctrl: value.control,
        alt: value.alt,
        cmd: value.platform,
    }
}

fn button(value: MouseButton) -> Option<Button> {
    match value {
        MouseButton::Left => Some(Button::Primary),
        MouseButton::Right => Some(Button::Secondary),
        MouseButton::Middle => Some(Button::Middle),
        MouseButton::Navigate(_) => None,
    }
}

fn render_image(pixels: &[u8], width: u32, height: u32) -> Result<Arc<RenderImage>> {
    anyhow::ensure!(
        width > 0
            && height > 0
            && (u64::from(width) * u64::from(height)).checked_mul(4) == Some(pixels.len() as u64),
        "CPU pixels do not match the native image extent"
    );
    let mut pixels = pixels.to_vec();
    for pixel in pixels.chunks_exact_mut(4) {
        // GPUI expects straight BGRA; Vello CPU supplies premultiplied RGBA.
        gpui::swap_rgba_pa_to_bgra(pixel);
    }
    let buffer = image::RgbaImage::from_raw(width, height, pixels)
        .ok_or_else(|| anyhow!("CPU pixels do not match the native image extent"))?;
    Ok(Arc::new(RenderImage::new(smallvec::smallvec![
        image::Frame::new(buffer)
    ])))
}

fn rss_kib() -> Option<u64> {
    std::fs::read_to_string("/proc/self/status")
        .ok()?
        .lines()
        .find_map(|line| line.strip_prefix("VmRSS:"))?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

impl Probe {
    fn advance_test(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Result<()> {
        let Some(mut test) = self.test.take() else {
            return Ok(());
        };
        test.callbacks += 1;
        anyhow::ensure!(
            test.callbacks <= 1800,
            "native resize/callback progression stalled"
        );
        if let Some(directory) = &self.capture {
            if test.step != 4 || self.raster.size() != test.extent {
                let request = directory.join(format!("capture-{}.json", test.step));
                let ack = request.with_extension("ack");
                if test.capture_requested != Some(test.step) {
                    if ack.exists() {
                        std::fs::remove_file(&ack)?;
                    }
                    let (width, height) = self.raster.size();
                    let reference = format!("reference-{}.png", test.step);
                    // Compare opaque pixels: the CPU reference retains premultiplied alpha.
                    image::save_buffer_with_format(
                        directory.join(&reference),
                        self.raster.pixels(),
                        width,
                        height,
                        image::ColorType::Rgba8,
                        image::ImageFormat::Png,
                    )?;
                    let event = serde_json::json!({
                        "case":test.step, "scene":self.scene.name(), "stage":[0,0,width,height],
                        "reference":reference, "reference_dimensions":[width,height],
                        "reference_alpha":"premultiplied",
                    });
                    let pending = request.with_extension("tmp");
                    std::fs::write(&pending, serde_json::to_vec(&event)?)?;
                    std::fs::rename(pending, request)?;
                    test.capture_requested = Some(test.step);
                }
                if !ack.exists() {
                    self.test = Some(test);
                    cx.notify();
                    return Ok(());
                }
                test.captures += 1;
                record(
                    &self.journal,
                    serde_json::json!({"event":"native_capture_ack", "case":test.step}),
                )?;
            }
        }
        match test.step {
            0 => {
                self.scene = Box::new(scenes::Gestures::default());
                test.step = 1;
            }
            1 => {
                test.checksum = checksum(self.raster.pixels());
                let frame = self
                    .ui
                    .scene()
                    .and_then(|s| s.layout.frame("g-cutoff"))
                    .ok_or_else(|| anyhow!("gesture fixture has no cutoff target"))?;
                let (x, y) = frame.center();
                for (y, down) in [(y, false), (y, true), (y - 24., true), (y - 24., false)] {
                    self.pointer.pos = Some(Point::new(x, y));
                    self.pointer.buttons = self.pointer.buttons.set(Button::Primary, down);
                    self.pending.push_back(self.pointer.into());
                }
                test.step = 2;
            }
            2 => {
                anyhow::ensure!(
                    checksum(self.raster.pixels()) != test.checksum,
                    "scripted knob gesture did not change CPU pixels"
                );
                record(
                    &self.journal,
                    serde_json::json!({"event":"scripted_input_changed_image", "frame":self.frames}),
                )?;
                self.scene = Box::new(scenes::Images::new());
                test.step = 3;
            }
            3 => {
                test.extent = self.raster.size();
                let viewport = window.viewport_size();
                window.resize(size(
                    px(f32::from(viewport.width) * 0.75),
                    px(f32::from(viewport.height) * 0.75),
                ));
                test.step = 4;
            }
            4 if self.raster.size() != test.extent => {
                record(
                    &self.journal,
                    serde_json::json!({
                        "event":"test_complete", "completed":true,
                        "changed_images":self.frames, "native_frame_callbacks":test.callbacks,
                    "resize_from":test.extent, "resize_to":self.raster.size(),
                    "native_capture_acknowledgements":test.captures,
                        "native_gpu_submission_verified":false,
                        "timing_scope":"CPU preparation and native frame scheduling, not GPU completion or scanout",
                    }),
                )?;
                self.completed.set(true);
                cx.quit();
                return Ok(());
            }
            4 => {}
            _ => return Err(anyhow!("invalid test phase")),
        }
        self.test = Some(test);
        cx.notify();
        Ok(())
    }

    fn pointer_event(
        &mut self,
        position: gpui::Point<gpui::Pixels>,
        modifiers: Modifiers,
        pressed: Option<(MouseButton, bool)>,
        cx: &mut Context<Self>,
    ) {
        self.pointer.pos = Some(Point::new(
            f64::from(f32::from(position.x)),
            f64::from(f32::from(position.y)),
        ));
        self.pointer.mods = mods(modifiers);
        if let Some((native, down)) = pressed {
            if let Some(button) = button(native) {
                self.pointer.buttons = self.pointer.buttons.set(button, down);
            }
        }
        self.pending.push_back(self.pointer.into());
        cx.notify();
    }

    fn frame(&mut self, window: &mut Window) -> Result<Arc<RenderImage>> {
        let viewport = window.viewport_size();
        let scale = f64::from(window.scale_factor());
        let logical = mui::prelude::Size::new(
            f64::from(f32::from(viewport.width)),
            f64::from(f32::from(viewport.height)),
        );
        // Renderer::resize validates zero, u16 limits and the 64 MiB target budget.
        let extent = (
            (logical.width * scale).ceil() as u32,
            (logical.height * scale).ceil() as u32,
        );
        self.raster.resize(extent).map_err(|e| anyhow!(e))?;
        self.ui.set_scale(Some(scale));
        let started = Instant::now();
        let dt = self.last.elapsed().as_secs_f64();
        self.last = Instant::now();
        while let Some(input) = self.pending.pop_front() {
            let root = self.scene.specimen(&mut self.ui);
            self.ui.frame(root, Some(logical), input, 0.)?;
        }
        // MUI widgets consume the preceding frame's response when rebuilding.
        let root = self.scene.specimen(&mut self.ui);
        let frame = self.ui.frame(root, Some(logical), self.pointer, dt)?;
        let animate = frame.animating || !frame.edits.is_empty();
        let paint_ops = frame.scene.paint.len();
        let resolve_ms = started.elapsed().as_secs_f64() * 1000.;
        let started = Instant::now();
        let changed = self
            .raster
            .render(frame.scene, Affine::scale(scale))
            .map_err(|e| anyhow!(e))?;
        let raster_ms = started.elapsed().as_secs_f64() * 1000.;
        if animate {
            window.request_animation_frame();
        }
        if changed || self.image.is_none() {
            let started = Instant::now();
            let image = render_image(self.raster.pixels(), extent.0, extent.1)?;
            if let Some(previous) = self.image.take() {
                window.drop_image(previous)?;
            }
            self.image = Some(image);
            self.frames += 1;
            record(
                &self.journal,
                serde_json::json!({
                    "event": "cpu_image_ready", "frame": self.frames,
                    "scene": self.scene.name(), "extent": [extent.0, extent.1],
                    "scale": scale, "paint_ops": paint_ops,
                    "resolve_ms": resolve_ms, "raster_ms": raster_ms,
                    "image_prepare_ms": started.elapsed().as_secs_f64() * 1000.,
                    "image_payload_bytes": self.raster.pixels().len(),
                    "rss_kib": rss_kib(), "since_start_ms": self.started.elapsed().as_secs_f64() * 1000.,
                    "gpu_specs": window.gpu_specs().map(|gpu| format!("{gpu:?}")),
                }),
            )?;
            let ready = Instant::now();
            let frame_id = self.frames;
            let journal = self.journal.clone();
            window.on_next_frame(move |_, _| {
                if let Err(error) = record(
                    &journal,
                    serde_json::json!({
                        "event": "next_frame_callback", "frame": frame_id,
                        "elapsed_ms": ready.elapsed().as_secs_f64() * 1000.,
                    }),
                ) {
                    eprintln!("GPUI probe journal failed: {error:#}");
                }
            });
        }
        // The native window may redraw cached pixels without rerasterizing or reallocating.
        self.image
            .clone()
            .ok_or_else(|| anyhow!("missing CPU frame"))
    }
}

impl Render for Probe {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = match self.frame(window) {
            Ok(image) => img(image)
                .size_full()
                .object_fit(ObjectFit::Fill)
                .into_any_element(),
            Err(error) => {
                eprintln!("GPUI host probe refused frame: {error:#}");
                if self.test.is_some() {
                    *self.failure.borrow_mut() = Some(format!("frame refused: {error:#}"));
                    cx.quit();
                }
                div()
                    .child(format!("Frame refused: {error:#}"))
                    .into_any_element()
            }
        };
        if self.test.is_some() {
            let entity = cx.entity().downgrade();
            window.on_next_frame(move |window, cx| {
                if let Err(error) = entity.update(cx, |probe, cx| {
                    if let Err(error) = probe.advance_test(window, cx) {
                        *probe.failure.borrow_mut() = Some(format!("{error:#}"));
                        let _ = record(&probe.journal, serde_json::json!({"event":"test_failed", "error":format!("{error:#}")}));
                        cx.quit();
                    }
                }) { eprintln!("GPUI probe entity closed during test: {error:#}"); }
            });
        }
        let mut root = div()
            .id("mui-pixels")
            .track_focus(&self.focus)
            .size_full()
            .child(content)
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                this.pointer_event(event.position, event.modifiers, None, cx);
            }))
            .capture_any_mouse_down(cx.listener(|this, event: &MouseDownEvent, window, cx| {
                this.focus.focus(window, cx);
                this.pointer_event(
                    event.position,
                    event.modifiers,
                    Some((event.button, true)),
                    cx,
                );
            }))
            .capture_any_mouse_up(cx.listener(|this, event: &MouseUpEvent, _, cx| {
                this.pointer_event(
                    event.position,
                    event.modifiers,
                    Some((event.button, false)),
                    cx,
                );
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if let Some(key) = Key::from_name(&event.keystroke.key) {
                    this.pending.push_back(Input {
                        pointer: this.pointer,
                        keys: vec![KeyPress {
                            key,
                            mods: mods(event.keystroke.modifiers),
                        }],
                        ..Input::default()
                    });
                    cx.notify();
                }
            }));
        for native in [MouseButton::Left, MouseButton::Right, MouseButton::Middle] {
            root = root.on_mouse_up_out(
                native,
                cx.listener(|this, event: &MouseUpEvent, _, cx| {
                    this.pointer_event(
                        event.position,
                        event.modifiers,
                        Some((event.button, false)),
                        cx,
                    );
                }),
            );
        }
        root
    }
}

fn main() -> Result<()> {
    let started = Instant::now();
    let argument = std::env::args().nth(1);
    let test_ui = argument
        .as_deref()
        .is_some_and(|v| v == "--test-ui" || v.starts_with("--test-ui="));
    let selected = argument
        .as_deref()
        .map(|v| v.strip_prefix("--test-ui=").unwrap_or(v));
    let scene: Box<dyn scenes::PreviewScene> = match selected {
        Some("--test-ui") => Box::new(scenes::Editor),
        None | Some("editor") => Box::new(scenes::Editor),
        Some("effects") => Box::new(scenes::Effects),
        Some("material") => Box::new(Material),
        Some("images") => Box::new(scenes::Images::new()),
        Some("gestures") => Box::new(scenes::Gestures::default()),
        Some(other) => {
            return Err(anyhow!(
                "unknown fixture {other}; use editor/effects/material/images/gestures"
            ));
        }
    };
    let journal = Rc::new(RefCell::new(
        if let Some(directory) = std::env::var_os("MUI_TEST_RESULTS") {
            std::fs::create_dir_all(&directory)?;
            Some(File::create(
                std::path::PathBuf::from(directory).join("gpui-events.jsonl"),
            )?)
        } else {
            None
        },
    ));
    record(
        &journal,
        serde_json::json!({
            "event":"run_begin", "test_ui":test_ui, "initial_scene":scene.name(),
            "os":std::env::consts::OS, "arch":std::env::consts::ARCH,
            "gpui_revision":"f1a10a5227a331e86bdb006301e1090a21d33e7a", "embedded":false,
        }),
    )?;
    let font = mui::prelude::Font::new(epaint_default_fonts::HACK_REGULAR)?;
    let ui = Ui::default().font(font);
    let raster = Renderer::new((1, 1)).map_err(|e| anyhow!(e))?;
    let launch_error = Rc::new(RefCell::new(None));
    let error = launch_error.clone();
    let completed = Rc::new(Cell::new(false));
    let finished = completed.clone();
    let final_journal = journal.clone();
    let capture = if std::env::var_os("MUI_TEST_CAPTURE").is_some_and(|v| v == "1") {
        Some(
            std::env::var_os("MUI_TEST_RESULTS")
                .map(std::path::PathBuf::from)
                .ok_or_else(|| anyhow!("MUI_TEST_CAPTURE requires MUI_TEST_RESULTS"))?,
        )
    } else {
        None
    };
    gpui_platform::application().run(move |cx: &mut App| {
        if test_ui {
            let timer = cx.background_executor().timer(Duration::from_secs(30));
            let completed = completed.clone();
            let failure = launch_error.clone();
            let journal = journal.clone();
            cx.spawn(async move |cx| {
                timer.await;
                if !completed.get() {
                    *failure.borrow_mut() = Some("native test exceeded 30 seconds".into());
                    let _ = record(&journal, serde_json::json!({"event":"test_failed", "error":"native test exceeded 30 seconds"}));
                    let _ = cx.update(|cx| cx.quit());
                }
            }).detach();
        }
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        let bounds = Bounds::centered(None, size(px(1200.), px(800.)), cx);
        if let Err(error) = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |window, cx| {
                if test_ui { window.set_window_title("MUI tester"); }
                cx.new(|cx| {
                    let focus = cx.focus_handle();
                    focus.focus(window, cx);
                    Probe {
                        scene,
                        ui,
                        raster,
                        image: None,
                        pointer: PointerInput::default(),
                        pending: VecDeque::new(),
                        focus,
                        last: Instant::now(),
                        started,
                        frames: 0,
                        test: test_ui.then(TestRun::default),
                        journal,
                        completed,
                        failure: launch_error.clone(),
                        capture,
                    }
                })
            },
        ) {
            *launch_error.borrow_mut() =
                Some(format!("GPUI could not open native window: {error:#}"));
            cx.quit();
        }
    });
    if test_ui && !finished.get() && error.borrow().is_none() {
        *error.borrow_mut() = Some("native application exited before test completion".into());
    }
    record(
        &final_journal,
        serde_json::json!({
            "event":"run_end", "completed":!test_ui || finished.get(),
            "error":error.borrow().as_deref(), "native_application_returned":true,
            "native_gpu_submission_verified":false,
        }),
    )?;
    if let Some(error) = error.borrow_mut().take() {
        return Err(anyhow!(error));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_image_preserves_alpha_and_channel_order() {
        let pixels = [128, 64, 32, 128, 0, 0, 0, 0, 1, 2, 3, 255];
        let image = render_image(&pixels, 3, 1).unwrap();
        assert_eq!(
            image.as_bytes(0).unwrap(),
            [63, 127, 254, 128, 0, 0, 0, 0, 3, 2, 1, 255]
        );
    }

    #[test]
    fn cpu_image_refuses_mismatched_extents() {
        for (width, height, pixels) in [
            (2, 1, &[1, 2, 3, 255][..]),
            (1, 1, &[1, 2, 3, 255, 4, 5, 6, 255][..]),
            (0, 1, &[][..]),
            (u32::MAX, u32::MAX, &[][..]),
        ] {
            assert!(render_image(pixels, width, height).is_err());
        }
    }
}
