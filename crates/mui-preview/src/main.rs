//! MUI preview gallery.
//!
//! A dev-only host that resolves a [`scenes::PreviewScene`] and paints its
//! surfaces through `mui-egui`. This is the first thing in the workspace that
//! puts a pixel on screen; everything else prints numbers.
//!
//! Not a dependency of `mui`, so nothing here reaches a plugin build.
//!
//! Run:   `cargo run -p mui-preview`
//! Watch: `bacon` (see bacon.toml)
#![forbid(unsafe_code)]

mod scenes;

use eframe::egui;
use mui_core::{resolve_scene, ResolvedScene};
use mui_tessellate::{Tessellator, TriangleMesh};
use scenes::PreviewScene;

/// Chord tolerance for flattening arcs. Below a device pixel at any scale we
/// care about; raising it shows up as faceted fillets, which is exactly what
/// this gallery exists to catch.
const FLATTEN_TOLERANCE: f64 = 0.05;

fn main() -> eframe::Result {
    eframe::run_native(
        "MUI preview",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default().with_inner_size([1000.0, 680.0]),
            ..Default::default()
        },
        Box::new(|_cc| Ok(Box::new(App::new()))),
    )
}

/// A resolved scene plus its meshes. Retessellating is the expensive half, so
/// it happens when the scene changes, never per frame — the same boundary a
/// file-watching backend would commit on.
struct Baked {
    scene: Option<ResolvedScene>,
    /// `(surface id, mesh)` in resolution order.
    meshes: Vec<(String, TriangleMesh)>,
    error: Option<String>,
}

impl Baked {
    fn build(source: &dyn PreviewScene) -> Self {
        let scene = match resolve_scene(&source.spec()) {
            Ok(scene) => scene,
            Err(e) => {
                return Self {
                    scene: None,
                    meshes: Vec::new(),
                    error: Some(format!("resolve failed: {e}")),
                }
            }
        };
        let mut tess = Tessellator::default();
        let mut meshes = Vec::new();
        let mut error = None;
        let surfaces = scene
            .surfaces()
            .map(|(id, surface)| (id.to_owned(), surface.path.clone()))
            .chain(source.overlay());
        for (id, path) in surfaces {
            match tess.tessellate(&path, FLATTEN_TOLERANCE) {
                Ok(mesh) => meshes.push((id.clone(), mesh)),
                Err(e) => error = Some(format!("tessellate {id}: {e}")),
            }
        }
        Self {
            scene: Some(scene),
            meshes,
            error,
        }
    }

    fn triangles(&self) -> usize {
        self.meshes.iter().map(|(_, m)| m.indices.len() / 3).sum()
    }
}

struct App {
    scenes: Vec<Box<dyn PreviewScene>>,
    selected: usize,
    baked: Baked,
    show_frames: bool,
    show_fill: bool,
}

impl App {
    fn new() -> Self {
        let scenes = scenes::all();
        let baked = Baked::build(scenes[0].as_ref());
        Self {
            scenes,
            selected: 0,
            baked,
            show_frames: false,
            show_fill: true,
        }
    }

    fn rebake(&mut self) {
        self.baked = Baked::build(self.scenes[self.selected].as_ref());
    }

    fn sidebar(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        ui.heading("MUI");
        ui.label(egui::RichText::new("preview gallery").weak());
        ui.separator();

        let mut pick = self.selected;
        for (i, scene) in self.scenes.iter().enumerate() {
            ui.selectable_value(&mut pick, i, scene.name());
        }
        if pick != self.selected {
            self.selected = pick;
            self.rebake();
        }

        ui.separator();
        if self.scenes[self.selected].controls(ui) {
            self.rebake();
        }

        ui.separator();
        ui.checkbox(&mut self.show_fill, "Fill surfaces");
        ui.checkbox(&mut self.show_frames, "Layout frames");
        if ui.button("Rebuild").clicked() {
            self.rebake();
        }

        ui.separator();
        ui.label(
            egui::RichText::new(self.scenes[self.selected].about())
                .small()
                .weak(),
        );
    }

    fn stats(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            match &self.baked.scene {
                Some(scene) => ui.label(format!(
                    "layout {:.0}x{:.0}",
                    scene.layout.size.width, scene.layout.size.height
                )),
                None => ui.label("layout -"),
            };
            ui.separator();
            ui.label(format!("{} surfaces", self.baked.meshes.len()));
            ui.separator();
            ui.label(format!("{} triangles", self.baked.triangles()));
            if let Some(err) = &self.baked.error {
                ui.separator();
                ui.colored_label(egui::Color32::from_rgb(220, 90, 90), err);
            }
        });
    }

    fn stage(&self, ui: &mut egui::Ui) {
        let Some(scene) = &self.baked.scene else {
            return;
        };
        let available = ui.available_rect_before_wrap();
        let size = scene.layout.size;
        // Centre the scene in whatever room is left. Translation only: scaling
        // here would stretch radii the geometry resolved exactly, which is the
        // bug this whole library exists to avoid.
        let origin = egui::pos2(
            available.center().x - size.width as f32 / 2.0,
            available.center().y - size.height as f32 / 2.0,
        );
        let painter = ui.painter_at(available);

        for (i, (_, mesh)) in self.baked.meshes.iter().enumerate() {
            let shade = 40 + (i as u8 % 5) * 16;
            let fill = if self.show_fill {
                egui::Color32::from_rgb(shade, shade + 6, shade + 12)
            } else {
                egui::Color32::TRANSPARENT
            };
            mui_egui::paint(
                &painter,
                mesh,
                origin,
                fill,
                egui::Stroke::new(1.0, egui::Color32::from_rgb(120, 190, 255)),
            );
        }

        if self.show_frames {
            for (key, frame) in scene.layout.frames() {
                let rect = egui::Rect::from_min_size(
                    origin + egui::vec2(frame.x as f32, frame.y as f32),
                    egui::vec2(frame.size.width as f32, frame.size.height as f32),
                );
                painter.rect_stroke(
                    rect,
                    0.0,
                    egui::Stroke::new(1.0, egui::Color32::from_rgb(255, 160, 60)),
                    egui::StrokeKind::Outside,
                );
                painter.text(
                    rect.left_top(),
                    egui::Align2::LEFT_BOTTOM,
                    key,
                    egui::FontId::monospace(9.0),
                    egui::Color32::from_rgb(255, 160, 60),
                );
            }
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Panel::left("gallery")
            .default_size(240.0)
            .show(ui, |ui| self.sidebar(ui));
        egui::Panel::bottom("stats").show(ui, |ui| self.stats(ui));
        egui::CentralPanel::default().show(ui, |ui| self.stage(ui));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every gallery scene must resolve and tessellate. This is the check that
    /// fails if a geometry change breaks a shape the gallery is meant to show.
    #[test]
    fn every_scene_bakes() {
        for scene in scenes::all() {
            let baked = Baked::build(scene.as_ref());
            assert!(baked.error.is_none(), "{}: {:?}", scene.name(), baked.error);
            assert!(!baked.meshes.is_empty(), "{}: no surfaces", scene.name());
            for (id, mesh) in &baked.meshes {
                assert!(
                    !mesh.indices.is_empty(),
                    "{} / {id}: empty mesh",
                    scene.name()
                );
            }
        }
    }
}
