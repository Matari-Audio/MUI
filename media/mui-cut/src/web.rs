//! The browser's handle on the engine: the web editor keeps the project as
//! JSON, hands every edit to [`Cut::load`], and draws the viewport with
//! [`GpuView`] on WebGPU or [`Cut::render`] on the CPU -- the same evaluator
//! and renderers the CLI uses.
use wasm_bindgen::prelude::*;

use crate::render::Assets;
use crate::{GpuCanvas, Project, Renderer, eval};

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
    /// Parse and keep `json`. On error the last good project stays loaded.
    pub fn load(&mut self, json: &str) -> Result<(), String> {
        self.project = Some(Project::load(json)?);
        Ok(())
    }
    /// The project as a save writes it.
    pub fn canonical(&self) -> String {
        self.project
            .as_ref()
            .map(Project::to_json)
            .unwrap_or_default()
    }
    pub fn add_png(&mut self, path: &str, bytes: &[u8]) -> Result<(), String> {
        self.renderer.add_png(path, bytes)
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
    /// The evaluated frame as JSON: what the inspector shows at the playhead.
    pub fn frame(&self, scene: usize, t: f64) -> String {
        self.project
            .as_ref()
            .and_then(|p| Some(eval(p, p.scenes.get(scene)?, t)))
            .and_then(|f| serde_json::to_string(&f).ok())
            .unwrap_or_default()
    }
}

/// The viewport on WebGPU: MUI's GPU renderer drawing straight into an
/// `OffscreenCanvas` (the editor's worker owns it).
#[wasm_bindgen]
pub struct GpuView {
    canvas: GpuCanvas,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    project: Option<Project>,
    assets: Assets,
    adapter: String,
}

#[wasm_bindgen]
impl GpuView {
    /// Fails, leaving `canvas` untouched (a 2D context still works), when
    /// there is no WebGPU adapter or device.
    pub async fn create(canvas: web_sys::OffscreenCanvas) -> Result<GpuView, String> {
        let instance = wgpu::Instance::default();
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                ..Default::default()
            })
            .await
            .map_err(|e| format!("no WebGPU adapter: {e}"))?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await
            .map_err(|e| e.to_string())?;
        let (w, h) = (canvas.width().max(1), canvas.height().max(1));
        let surface = instance
            .create_surface(wgpu::SurfaceTarget::OffscreenCanvas(canvas))
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
        let canvas = GpuCanvas::new(&device, &queue, format, [w, h]).await?;
        Ok(Self {
            canvas,
            surface,
            config,
            project: None,
            assets: Assets::default(),
            adapter: adapter.get_info().name,
        })
    }
    pub fn adapter(&self) -> String {
        self.adapter.clone()
    }
    pub fn load(&mut self, json: &str) -> Result<(), String> {
        self.project = Some(Project::load(json)?);
        Ok(())
    }
    pub fn add_png(&mut self, path: &str, bytes: &[u8]) -> Result<(), String> {
        self.assets.add_png(path, bytes)
    }
    /// Scene `scene` at `t` presented at `w` by `h`; the layers' quads as
    /// JSON `[{id, pts}]` in project pixels.
    pub fn draw(&mut self, scene: usize, t: f64, w: u32, h: u32) -> Result<String, String> {
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
        let quads = self.canvas.draw(&self.assets, &eval(p, s, t), &view)?;
        self.canvas.queue.present(frame);
        serde_json::to_string(&quads).map_err(|e| e.to_string())
    }
}
