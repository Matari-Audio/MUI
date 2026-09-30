//! The browser's handle on the engine: the web editor keeps the project as
//! JSON, hands every edit to [`Cut::load`], and draws the viewport with
//! [`Cut::render`] -- the same evaluator and renderer the CLI uses.
use wasm_bindgen::prelude::*;

use crate::{Project, Renderer, eval};

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
