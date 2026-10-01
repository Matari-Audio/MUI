//! The browser's handle on the engine: the web editor keeps the project as
//! JSON, hands every edit to [`Cut::load`], and draws the viewport with
//! [`GpuView`] on WebGPU or [`Cut::render`] on the CPU -- the same evaluator
//! and renderers the CLI uses.
use wasm_bindgen::prelude::*;

use crate::render::Assets;
use crate::{Engine, GpuCanvas, Project, Renderer, Shutter, eval, subframes};

thread_local! {
    static PANIC: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}

/// The message of the last panic ("" when none): wasm only throws
/// `unreachable`, and a panicked object refuses every later call.
#[wasm_bindgen(js_name = lastPanic)]
pub fn last_panic() -> String {
    PANIC.with_borrow(Clone::clone)
}

#[wasm_bindgen(start)]
fn start() {
    std::panic::set_hook(Box::new(|info| PANIC.set(info.to_string())));
}

fn load(json: &str, variant: Option<String>) -> Result<Project, String> {
    let p = Project::load(json)?;
    match variant.filter(|v| !v.is_empty()) {
        Some(v) => p.variant(&v),
        None => Ok(p),
    }
}

/// [`crate::place::rewrite`] on project text.
fn rewrite(
    doc: &str,
    scene: usize,
    op: &dyn Fn(&Project, &crate::Scene) -> Result<crate::Scene, String>,
) -> Result<String, String> {
    let root = serde_json::from_str(doc).map_err(|e| e.to_string())?;
    let out = crate::place::rewrite(&root, scene, op)?;
    serde_json::to_string(&out).map_err(|e| e.to_string())
}

/// `s` with its layer of `l`'s id replaced by `l`.
fn with(s: &crate::Scene, l: crate::Layer) -> crate::Scene {
    let mut s = s.clone();
    if let Some(o) = s.layers.iter_mut().find(|o| o.id == l.id) {
        *o = l;
    }
    s
}

#[wasm_bindgen]
pub struct Cut {
    project: Option<Project>,
    renderer: Renderer,
    quads: String,
}

impl Default for Cut {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen]
impl Cut {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            project: None,
            renderer: Renderer::new(1, 1),
            quads: "[]".into(),
        }
    }
    /// Parse and keep `json`, as `variant` makes it if one is named. On
    /// error the last good project stays loaded.
    pub fn load(&mut self, json: &str, variant: Option<String>) -> Result<(), String> {
        self.project = Some(load(json, variant)?);
        Ok(())
    }
    /// The project as loaded, every binding resolved: the size, fps and
    /// durations the editor lays out with.
    pub fn resolved(&self) -> String {
        self.project
            .as_ref()
            .and_then(|p| serde_json::to_string(p).ok())
            .unwrap_or_default()
    }
    /// The project as a save writes it.
    pub fn canonical(&self) -> String {
        self.project
            .as_ref()
            .map(Project::to_json)
            .unwrap_or_default()
    }
    /// A file a layer names (PNG, SVG or Lottie JSON), by its path.
    pub fn add_asset(&mut self, path: &str, bytes: &[u8]) -> Result<(), String> {
        self.renderer.add_asset(path, bytes)
    }
    /// Scene `scene` at `t`, `w` by `h` straight RGBA. The layers' quads, in
    /// project pixels, are kept for [`Cut::quads`].
    pub fn render(&mut self, scene: usize, t: f64, w: u16, h: u16) -> Result<Vec<u8>, String> {
        let p = self.project.as_ref().ok_or("no project loaded")?;
        let s = p.scenes.get(scene).ok_or("no such scene")?;
        if self.renderer.size() != (w, h) {
            let assets = std::mem::take(&mut self.renderer.assets);
            self.renderer = Renderer::new(w, h);
            self.renderer.assets = assets;
        }
        let (px, quads) = self.renderer.draw(&eval(p, s, t))?;
        self.quads = serde_json::to_string(&quads).map_err(|e| e.to_string())?;
        Ok(px)
    }
    /// Why the CPU view draws `scene` differently, or empty: it has no 3D
    /// pass, so 3D scenes draw flat.
    pub fn notice(&self, scene: usize) -> String {
        match self.project.as_ref().and_then(|p| p.scenes.get(scene)) {
            Some(s) if s.mode == crate::Mode::ThreeD => {
                "3D scenes draw flat on the CPU renderer".into()
            }
            _ => String::new(),
        }
    }
    /// The effect schema as JSON: `[{name, about, passes, params: [{name,
    /// default, min?, max?}]}]`, a string default being a colour.
    pub fn effects() -> String {
        serde_json::to_string(crate::fx::EFFECTS).unwrap_or_default()
    }
    /// JSON `[{id, pts: [[x, y] x4]}]` from the last render.
    pub fn quads(&self) -> String {
        self.quads.clone()
    }
    /// `n` values of one numeric property across `[t0, t1]`, for the graph
    /// editor: the curve it draws is the evaluator's own.
    pub fn sample(
        &self,
        scene: usize,
        layer: &str,
        prop: &str,
        t0: f64,
        t1: f64,
        n: usize,
    ) -> Vec<f64> {
        let Some(a) = self
            .project
            .as_ref()
            .and_then(|p| p.scenes.get(scene))
            .and_then(|s| s.layers.iter().find(|l| l.id == layer))
            .and_then(|l| l.prop(prop))
        else {
            return Vec::new();
        };
        let n = n.max(2);
        (0..n)
            .map(|i| a.at(t0 + (t1 - t0) * i as f64 / (n - 1) as f64))
            .collect()
    }
    /// Every keyable property of one layer at `t`, as JSON
    /// `[{"p": path, "v": number or "#rrggbb"}]`: the inspector's rows,
    /// the timeline's and the graph's choices.
    pub fn props(&self, scene: usize, layer: &str, t: f64) -> String {
        let Some((s, l)) = self
            .project
            .as_ref()
            .and_then(|p| p.scenes.get(scene))
            .and_then(|s| Some((s, s.layers.iter().find(|l| l.id == layer)?)))
        else {
            return "[]".into();
        };
        let rows: Vec<serde_json::Value> = l
            .props_in(s.mode == crate::Mode::ThreeD)
            .into_iter()
            .map(|(p, a)| match a {
                crate::Prop::Num(a) => serde_json::json!({ "p": p, "v": a.at(t) }),
                crate::Prop::Color(a) => serde_json::json!({ "p": p, "v": String::from(a.at(t)) }),
            })
            .collect();
        serde_json::to_string(&rows).unwrap_or_default()
    }
    /// A preset animator (`typewriter`, `cascade`, `pop`) keyed from `t0`
    /// over `dur` seconds, as JSON; empty for an unknown name.
    pub fn preset(&self, name: &str, t0: f64, dur: f64) -> String {
        crate::Animator::preset(name, t0, dur)
            .and_then(|a| serde_json::to_string(&a).ok())
            .unwrap_or_default()
    }
    /// Every plugin capture the project shows, as asset paths
    /// (`.cut-cache/<state>.json`), JSON: what the editor fetches (after
    /// `mui-cut serve` has captured them) and hands the viewport.
    pub fn plugin_states(&self) -> String {
        let Some(p) = &self.project else {
            return "[]".into();
        };
        let mut paths: Vec<String> = p
            .scenes
            .iter()
            .flat_map(|s| {
                let last = crate::plugin::frame_at(s.duration, p.fps);
                s.layers
                    .iter()
                    .flat_map(move |l| l.plugin_track(p.fps, p.sample_rate, last))
            })
            .map(|s| s.key)
            .chain(
                p.all_sources()
                    .iter()
                    .filter_map(crate::sources::Media::state),
            )
            .map(|k| format!("{}/{k}.json", crate::plugin::CACHE))
            .collect();
        paths.sort();
        paths.dedup();
        serde_json::to_string(&paths).unwrap_or_default()
    }
    /// Interact in 3D ([`crate::pick::pick`]): the plugin layer and the
    /// point of its UI under project point `(x, y)` of `scene` at `t`, seen
    /// from the orbit preview's camera when `orbit` is `[yaw, pitch,
    /// zoom]` (empty: the scene camera), as JSON `{layer, slab, ui}`; `""`
    /// when none. `only` (empty: any) keeps a drag on the slab it started on.
    pub fn pick(&self, scene: usize, t: f64, x: f64, y: f64, orbit: &[f64], only: &str) -> String {
        let Some(s) = self
            .project
            .as_ref()
            .and_then(|p| Some((p, p.scenes.get(scene)?)))
        else {
            return String::new();
        };
        let mut f = eval(s.0, s.1, t);
        if let (Some(v), [yaw, pitch, zoom]) = (f.view.as_mut(), orbit) {
            v.camera = v.camera.orbit(*yaw, *pitch, *zoom);
        }
        crate::pick::pick(
            &self.renderer.assets,
            &f,
            x,
            y,
            (!only.is_empty()).then_some(only),
        )
        .and_then(|h| serde_json::to_string(&h).ok())
        .unwrap_or_default()
    }
    /// A plugin layer's parts at `t` as [`tree_json`](crate::plugin::tree_json),
    /// once its capture (`.cut-cache/<state>.json`) has been added; `""`
    /// before, or for another kind of layer.
    pub fn plugin_parts(&self, scene: usize, t: f64, layer: &str) -> String {
        let Some(p) = &self.project else {
            return String::new();
        };
        let Some(s) = p.scenes.get(scene) else {
            return String::new();
        };
        eval(p, s, t)
            .layers
            .into_iter()
            .find(|l| l.id == layer)
            .and_then(|l| l.plugin)
            .and_then(|at| {
                let cap = self.renderer.assets.capture(&at.state)?;
                Some(crate::plugin::tree_json(cap, &at).to_string())
            })
            .unwrap_or_default()
    }
    /// Every source (see [`Project::all_sources`]) as JSON, a plugin's
    /// with `state`: the manifest path of its fresh capture, which holds
    /// its parts.
    pub fn sources(&self) -> String {
        let Some(p) = &self.project else {
            return "[]".into();
        };
        let rows: Vec<serde_json::Value> = p
            .all_sources()
            .iter()
            .map(|m| {
                let mut v = serde_json::to_value(m).unwrap_or_default();
                if let Some(k) = m.state() {
                    v["state"] = format!("{}/{k}.json", crate::plugin::CACHE).into();
                }
                v
            })
            .collect();
        serde_json::to_string(&rows).unwrap_or_default()
    }
    /// A plugin source's parts as [`crate::plugin::home_tree`], from its
    /// home capture (`state`, as [`Cut::sources`] names it) once added:
    /// the nodes `cutParts` has, nothing moved; `"[]"` before.
    pub fn source_parts(&self, state: &str) -> String {
        let key = state
            .trim_start_matches(crate::plugin::CACHE)
            .trim_start_matches('/')
            .trim_end_matches(".json");
        self.renderer.assets.capture(key).map_or_else(
            || "[]".into(),
            |c| crate::plugin::home_tree(c, key).to_string(),
        )
    }
    /// Layer `id` of scene `scene` of the project text `doc` parented to
    /// `parent` ("" detaches it), kept where it is on screen at `t` in
    /// every variant: the new project text ([`crate::place::rewrite`]).
    pub fn reparent(
        &self,
        doc: &str,
        scene: usize,
        id: &str,
        parent: &str,
        t: f64,
    ) -> Result<String, String> {
        rewrite(doc, scene, &|p, s| {
            let l = crate::place::reparent(p, s, id, Some(parent), t)?;
            Ok(with(s, l))
        })
    }
    /// Layer `id` of scene `scene` of `doc` back to its default layout in
    /// every variant: the new project text.
    pub fn reset(&self, doc: &str, scene: usize, id: &str) -> Result<String, String> {
        rewrite(doc, scene, &|p, s| {
            let mut l = s
                .layers
                .iter()
                .find(|l| l.id == id)
                .ok_or("no such layer")?
                .clone();
            l.reset(p.size);
            Ok(with(s, l))
        })
    }
    /// Scene `scene` of `doc` taken flat, as its 3D shot shows it at `t`
    /// in every variant (see [`crate::place::flatten`]): the new project
    /// text.
    pub fn flatten(&self, doc: &str, scene: usize, t: f64) -> Result<String, String> {
        rewrite(doc, scene, &|p, s| Ok(crate::place::flatten(p, s, t)))
    }
    /// The evaluated frame as JSON: what the inspector shows at the playhead.
    pub fn frame(&self, scene: usize, t: f64) -> String {
        self.project
            .as_ref()
            .and_then(|p| Some(eval(p, p.scenes.get(scene)?, t)))
            .and_then(|f| serde_json::to_string(&f).ok())
            .unwrap_or_default()
    }
}

/// The viewport on the GPU: MUI's Vello renderers drawing straight into an
/// `OffscreenCanvas` (the editor's worker owns it), on WebGPU or WebGL2.
#[wasm_bindgen]
pub struct GpuView {
    canvas: GpuCanvas,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    project: Option<Project>,
    assets: Assets,
    adapter: String,
    /// The orbit preview's yaw, pitch and zoom; never saved.
    orbit: Option<[f64; 3]>,
    /// For exports with motion blur, made on the first one.
    shutter: Option<Shutter>,
}

#[wasm_bindgen]
impl GpuView {
    /// `api` is `webgpu` or `webgl2`, `engine` `classic` or `gpu`
    /// (vello_gpu); WebGL2 has no compute shaders, so it always gets
    /// vello_gpu. Fails when there is no such adapter or device; the canvas
    /// then holds that API's context, so probe before calling.
    pub async fn create(
        canvas: web_sys::OffscreenCanvas,
        api: &str,
        engine: &str,
    ) -> Result<GpuView, String> {
        let (backends, engine) = match (api, engine) {
            ("webgl2", _) => (wgpu::Backends::GL, Engine::Sparse),
            ("webgpu", "gpu") => (wgpu::Backends::BROWSER_WEBGPU, Engine::Sparse),
            ("webgpu", "classic") => (wgpu::Backends::BROWSER_WEBGPU, Engine::Classic),
            _ => return Err(format!("no GPU view for {api} with {engine}")),
        };
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let (w, h) = (canvas.width().max(1), canvas.height().max(1));
        // A WebGL adapter comes from the canvas's context, so the surface
        // goes first.
        let surface = instance
            .create_surface(wgpu::SurfaceTarget::OffscreenCanvas(canvas))
            .map_err(|e| e.to_string())?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await
            .map_err(|e| format!("no {api} adapter: {e}"))?;
        let required_limits = if backends == wgpu::Backends::GL {
            wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter.limits())
        } else {
            wgpu::Limits::default()
        };
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                required_limits,
                ..Default::default()
            })
            .await
            .map_err(|e| e.to_string())?;
        let caps = surface.get_capabilities(&adapter);
        let format =
            mui_vello::host::surface_format(&caps.formats).ok_or("no non-sRGB canvas format")?;
        let config = wgpu::SurfaceConfiguration {
            format,
            view_formats: Vec::new(),
            ..surface
                .get_default_config(&adapter, w, h)
                .ok_or("canvas not supported by the adapter")?
        };
        surface.configure(&device, &config);
        let mut canvas = GpuCanvas::new(&device, &queue, format, [w, h], engine).await?;
        // WebGL2 lacks the storage and depth-array features the 3D pass
        // needs; 3D scenes draw flat there with a notice.
        canvas.three_d = api != "webgl2";
        Ok(Self {
            canvas,
            surface,
            config,
            project: None,
            assets: Assets::default(),
            adapter: adapter.get_info().name,
            orbit: None,
            shutter: None,
        })
    }
    /// `classic` or `vello_gpu`.
    pub fn engine(&self) -> String {
        self.canvas.engine().name().into()
    }
    pub fn adapter(&self) -> String {
        self.adapter.clone()
    }
    pub fn load(&mut self, json: &str, variant: Option<String>) -> Result<(), String> {
        self.project = Some(load(json, variant)?);
        Ok(())
    }
    /// Why the last frame drew differently from the export, or empty.
    pub fn notice(&self) -> String {
        self.canvas.notice().into()
    }
    /// Look at 3D scenes from the shot camera swung `yaw` and `pitch`
    /// degrees about its target, `zoom` times as far; the project is
    /// untouched.
    pub fn set_orbit(&mut self, yaw: f64, pitch: f64, zoom: f64) {
        self.orbit = Some([yaw, pitch, zoom]);
    }
    pub fn clear_orbit(&mut self) {
        self.orbit = None;
    }
    pub fn add_asset(&mut self, path: &str, bytes: &[u8]) -> Result<(), String> {
        self.assets.add_asset(path, bytes)
    }
    /// An export frame: scene `scene`'s output frame at `t`, `mb` subframes
    /// averaged by the shutter (as `mui-cut render --mb`), presented at the
    /// canvas's size for a `VideoFrame` to take.
    pub fn draw_frame(&mut self, scene: usize, t: f64, mb: usize) -> Result<(), String> {
        use wgpu::CurrentSurfaceTexture as Acquired;
        let p = self.project.as_ref().ok_or("no project loaded")?;
        let s = p.scenes.get(scene).ok_or("no such scene")?;
        let size = [self.config.width, self.config.height];
        if self.canvas.size() != size {
            self.canvas.resize(size)?;
        }
        if self.shutter.as_ref().is_none_or(|sh| sh.size() != size) {
            let f = self.config.format;
            self.shutter = Some(Shutter::new(&self.canvas.device, size, f, f));
        }
        let shutter = self.shutter.as_ref().expect("made above");
        shutter.expose(
            &mut self.canvas,
            &self.assets,
            &subframes(p, s, t, mb),
            false,
        )?;
        let frame = match self.surface.get_current_texture() {
            Acquired::Success(f) | Acquired::Suboptimal(f) => f,
            other => return Err(format!("no canvas texture: {other:?}")),
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut enc = self
            .canvas
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        shutter.resolve(&mut enc, &view);
        self.canvas.queue.submit([enc.finish()]);
        self.canvas.queue.present(frame);
        Ok(())
    }
    /// Scene `scene` at `t` presented at `w` by `h`; the layers' quads as
    /// JSON `[{id, pts}]` in project pixels.
    /// With `sample`, a 3D frame is beauty sample `sample` folded into the
    /// mean of the ones before it (0 starts afresh): the paused viewport
    /// refines while the editor keeps asking for the next.
    pub fn draw(
        &mut self,
        scene: usize,
        t: f64,
        w: u32,
        h: u32,
        sample: Option<u32>,
    ) -> Result<String, String> {
        use wgpu::CurrentSurfaceTexture as Acquired;
        let p = self.project.as_ref().ok_or("no project loaded")?;
        let s = p.scenes.get(scene).ok_or("no such scene")?;
        let (w, h) = (w.max(1), h.max(1));
        if (self.config.width, self.config.height) != (w, h) {
            (self.config.width, self.config.height) = (w, h);
            self.surface.configure(&self.canvas.device, &self.config);
            self.canvas.resize([w, h])?;
        }
        let frame = match self.surface.get_current_texture() {
            Acquired::Success(f) | Acquired::Suboptimal(f) => f,
            Acquired::Outdated | Acquired::Lost => {
                self.surface.configure(&self.canvas.device, &self.config);
                return Err("canvas surface reset; draw again".into());
            }
            other => return Err(format!("no canvas texture: {other:?}")),
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut f = eval(p, s, t);
        if let (Some(v), Some([yaw, pitch, zoom])) = (f.view.as_mut(), self.orbit) {
            v.camera = v.camera.orbit(yaw, pitch, zoom);
        }
        let quads = match sample {
            None => self.canvas.draw(&self.assets, &f, &view)?,
            Some(i) => {
                let format = self.config.format;
                if self.shutter.as_ref().is_none_or(|sh| sh.size() != [w, h]) {
                    self.shutter = Some(Shutter::new(&self.canvas.device, [w, h], format, format));
                }
                let shutter = self.shutter.as_ref().expect("made above");
                let quads = shutter.expose_sample(&mut self.canvas, &self.assets, &f, i)?;
                let mut enc = self
                    .canvas
                    .device
                    .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
                shutter.resolve(&mut enc, &view);
                self.canvas.queue.submit([enc.finish()]);
                quads
            }
        };
        self.canvas.queue.present(frame);
        serde_json::to_string(&quads).map_err(|e| e.to_string())
    }
}
