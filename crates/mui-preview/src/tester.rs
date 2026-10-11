//! Automated gallery traversal over the real window / GPU path.
use super::*;
use serde_json::{Value, json};
use std::fs::{self, File};
use std::io::Write;

const SIZES: [(u32, u32); 3] = [(1000, 680), (700, 900), (1280, 400)];
const STEPS: usize = 5;

pub(super) struct Tester {
    directory: PathBuf,
    journal: File,
    case: usize,
    step: usize,
    started: bool,
    deadline: Instant,
    presented: usize,
    capture: bool,
    waiting_capture: bool,
    error: Option<String>,
    wanted: winit::dpi::PhysicalSize<u32>,
}

impl Tester {
    pub fn new() -> Result<Self, String> {
        let directory = std::env::var_os("MUI_TEST_RESULTS")
            .map_or_else(|| PathBuf::from("ui-results"), PathBuf::from);
        fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
        let journal = File::create(directory.join("events.jsonl")).map_err(|e| e.to_string())?;
        let mut tester = Self {
            directory,
            journal,
            case: 0,
            step: 0,
            started: false,
            deadline: Instant::now() + Duration::from_secs(60),
            presented: 0,
            capture: std::env::var_os("MUI_TEST_CAPTURE").is_some_and(|v| v == "1"),
            waiting_capture: false,
            error: None,
            wanted: winit::dpi::PhysicalSize::new(0, 0),
        };
        tester.record(&json!({"event":"run_begin", "schema":1,
            "os":std::env::consts::OS, "arch":std::env::consts::ARCH,
            "revision":option_env!("MUI_BUILD_REVISION").unwrap_or("unknown"),
            "backend":std::env::var("WGPU_BACKEND").unwrap_or_default(),
            "scenes":scenes::all().iter().map(|s| s.name()).collect::<Vec<_>>(),
            "variants_per_scene":SIZES.len()*2, "steps_per_case":STEPS}))?;
        Ok(tester)
    }

    fn record(&mut self, value: &Value) -> Result<(), String> {
        serde_json::to_writer(&mut self.journal, value).map_err(|e| e.to_string())?;
        writeln!(self.journal).map_err(|e| e.to_string())?;
        // Keep the last stage through a native fault; exclude this from timings.
        self.journal.sync_data().map_err(|e| e.to_string())
    }

    fn fail(&mut self, event_loop: &ActiveEventLoop, error: String) {
        let _ = self.record(&json!({"event":"failed", "case":self.case,
            "step":self.step,"message":error}));
        eprintln!("MUI tester failed: {error}");
        self.error = Some(error);
        event_loop.exit();
    }

    pub fn draw(&mut self, app: &mut App, event_loop: &ActiveEventLoop) {
        if let Err(error) = self.frame(app, event_loop) {
            self.fail(event_loop, error);
        } else {
            // Ui::frame updates its own wake deadline; traversal needs a tick
            // even when the current scene is idle or waiting for a capture.
            app.repaint_at = Some(Instant::now() + Duration::from_millis(25));
        }
    }

    fn frame(&mut self, app: &mut App, event_loop: &ActiveEventLoop) -> Result<(), String> {
        let expected = app.scenes.len() * SIZES.len() * 2;
        if self.case == expected {
            event_loop.exit();
            return Ok(());
        }
        if Instant::now() > self.deadline {
            return Err(format!(
                "case {} step {} did not complete before its deadline; wanted {:?}, actual {:?}",
                self.case,
                self.step,
                self.wanted,
                app.gpu.as_ref().map(|gpu| gpu.window().inner_size())
            ));
        }
        app.repaint_at = Some(Instant::now() + Duration::from_millis(25));
        let scene_index = self.case / (SIZES.len() * 2);
        let variant = self.case % (SIZES.len() * 2);
        let size = SIZES[variant / 2];
        if !self.started {
            app.cancel();
            app.ui.blur();
            app.selected = scene_index;
            app.light = variant % 2 == 1;
            app.frames = variant / 2 == 2;
            app.pan = Point::new(0., 0.);
            let gpu = app
                .gpu
                .as_ref()
                .ok_or("native GPU initialization is required")?;
            gpu.window().set_title("MUI tester");
            // Hosted desktops can be smaller than a portrait specimen. Fit
            // within the monitor so OS work-area clamping cannot stall traversal.
            let fit = gpu.window().current_monitor().map_or(1., |monitor| {
                let screen = monitor.size();
                let scale = gpu.window().scale_factor();
                ((f64::from(screen.width) / scale - 160.).max(64.) / f64::from(size.0))
                    .min((f64::from(screen.height) / scale - 160.).max(64.) / f64::from(size.1))
                    .min(1.)
            });
            let size = winit::dpi::LogicalSize::new(
                (f64::from(size.0) * fit).floor().max(1.) as u32,
                (f64::from(size.1) * fit).floor().max(1.) as u32,
            );
            // Synchronous platforms return the size actually accepted by the OS.
            self.wanted = gpu
                .window()
                .request_inner_size(size)
                .unwrap_or_else(|| size.to_physical(gpu.window().scale_factor()));
            self.deadline = Instant::now() + Duration::from_secs(60);
            self.presented = 0;
            self.started = true;
            let logical = self.wanted.to_logical::<f64>(gpu.window().scale_factor());
            let requested = SIZES[variant / 2];
            self.record(&json!({"event":"case_begin", "case":self.case,
                "scene":app.scenes[scene_index].name(), "light":app.light,
                "requested_logical_size":[requested.0,requested.1], "logical_size":[logical.width,logical.height], "overlay":app.frames,
                "rendering_mode":gpu.rendering_mode(), "gpu_error":gpu.gpu_error(),
                "gpu":gpu.diagnostics().map(ToString::to_string)}))?;
            return Ok(());
        }
        if self.waiting_capture {
            let ack = self.directory.join(format!("capture-{}.ack", self.case));
            if !ack.is_file() {
                return Ok(());
            }
            fs::remove_file(ack).map_err(|e| e.to_string())?;
            self.waiting_capture = false;
            return self.complete_case();
        }
        let gpu = app.gpu.as_mut().ok_or("window GPU disappeared")?;
        let scale = gpu.window().scale_factor();
        let actual = gpu.window().inner_size();
        if actual != self.wanted {
            return Ok(());
        }
        gpu.try_resize(actual.width, actual.height)?;
        let physical = (actual.width, actual.height);
        let target = app.ui.scene().and_then(|scene| {
            let stage = scene.surface("stage")?.frame;
            scene
                .surfaces()
                .find(|s| s.focusable && !s.disabled && s.frame.x >= stage.x)
                .map(|s| {
                    (
                        s.key.to_string(),
                        Point::new(
                            s.frame.x + s.frame.size.width / 2.,
                            s.frame.y + s.frame.size.height / 2.,
                        ),
                    )
                })
        });
        if let Some((_, point)) = &target {
            let point = Point::new(
                (point.x + if self.step == 2 { 12. } else { 0. }) * scale,
                point.y * scale,
            );
            app.queue(
                Some(Some(point)),
                Some((Button::Primary, self.step == 1 || self.step == 2)),
            );
        }
        if self.step == 3 {
            app.wheel = Vec2::new(0., 24.);
            app.keys.push(KeyPress {
                key: mui::prelude::Key::Tab,
                mods: Mods::default(),
            });
            app.text.push_str("MUI");
            app.typed = Some('A');
        }
        app.frame_error = None;
        self.record(&json!({"event":"frame_begin", "case":self.case,
            "step":self.step,"target":target.as_ref().map(|t| &t.0)}))?;
        let start = Instant::now();
        app.replay(physical, scale);
        let resolve_ms = start.elapsed().as_secs_f64() * 1000.;
        if let Some(error) = app.frame_error.take() {
            return Err(error);
        }
        let start = Instant::now();
        let stats = app.draw()?;
        let submit_ms = start.elapsed().as_secs_f64() * 1000.;
        let current = app.gpu.as_ref().is_some_and(Gpu::frame_was_current);
        if stats.is_none() && !current {
            return Ok(());
        }
        self.presented += usize::from(stats.is_some());
        let scene = app.ui.scene().ok_or("frame resolved no scene")?;
        if scene.paint.len() <= 3 {
            return Err("gallery scene painted no specimen".into());
        }
        self.record(&json!({"event":"frame", "case":self.case, "step":self.step,
            "resolve_ms":resolve_ms,"submit_ms":submit_ms,"presented":stats.is_some(),
            "rendering_mode":app.gpu.as_ref().map(Gpu::rendering_mode),
            "current":current,"physical_size":[physical.0,physical.1],"scale":scale,
            "paint_items":scene.paint.len(),"surfaces":scene.surfaces().count(),
            "rendered_pixels":stats.map(|s|s.rendered_pixels),
            "texture_allocations":stats.map(|s|s.texture_allocations),
            "resident_texture_bytes":stats.map(|s|s.resident_texture_bytes),
            "peak_texture_bytes":stats.map(|s|s.peak_texture_bytes),
            "effect_draws":stats.map(|s|s.effect_draws)}))?;
        let stage = scene.surface("stage").map(|surface| surface.frame);
        app.publish();
        self.step += 1;
        if self.step == STEPS {
            if self.presented == 0 {
                return Err("case never submitted a frame to its native surface".into());
            }
            if self.capture {
                let stage = stage.ok_or("missing specimen stage")?;
                let request = json!({"case":self.case,"scene":app.scenes[scene_index].name(),
                    "stage":[stage.x*scale,stage.y*scale,stage.size.width*scale,stage.size.height*scale]});
                let pending = self.directory.join(format!("capture-{}.tmp", self.case));
                fs::write(
                    &pending,
                    serde_json::to_vec(&request).map_err(|e| e.to_string())?,
                )
                .map_err(|e| e.to_string())?;
                fs::rename(
                    pending,
                    self.directory.join(format!("capture-{}.json", self.case)),
                )
                .map_err(|e| e.to_string())?;
                self.waiting_capture = true;
            } else {
                self.complete_case()?;
            }
        }
        Ok(())
    }

    fn complete_case(&mut self) -> Result<(), String> {
        self.record(&json!({"event":"case_end","case":self.case,"presented":self.presented}))?;
        self.case += 1;
        self.step = 0;
        self.started = false;
        Ok(())
    }

    pub fn finish(&mut self) -> Result<(), String> {
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        let expected = scenes::all().len() * SIZES.len() * 2;
        if self.case != expected {
            return Err(format!(
                "only {} of {expected} gallery cases completed",
                self.case
            ));
        }
        self.record(&json!({"event":"run_end","completed":self.case,"expected":expected}))
    }
}
