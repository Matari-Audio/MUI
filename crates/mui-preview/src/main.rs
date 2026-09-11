//! MUI preview gallery.
//!
//! A dev-only host that resolves a [`scenes::PreviewScene`] and paints its
//! surfaces through Vello. This is the first thing in the workspace that puts a
//! pixel on screen; everything else prints numbers.
//!
//! No toolkit in the graph: `mui-layout` places the frames, `mui-core` merges
//! and fillets them, `mui-vello` hands the result to the rasteriser, and
//! `mui-input` decides what the pointer means. winit and wgpu are this
//! binary's own, standing in for the plugin wrapper that owns the window in a
//! real host.
//!
//! Not a dependency of `mui`, so nothing here reaches a plugin build.
//!
//! Run:   `cargo run -p mui-preview`
//! Watch: `bacon` (see bacon.toml)
#![forbid(unsafe_code)]

mod host;
mod scenes;

use std::sync::Arc;

use host::Gpu;
use mui_core::{resolve_scene, ResolvedScene};
use mui_geometry::Point;
use mui_input::{Hit, Interaction, PointerInput};
use mui_vello::ARC_TOLERANCE;
use scenes::PreviewScene;
use vello_common::kurbo::{Affine, BezPath, Stroke};
use vello_common::peniko::color::AlphaColor;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

type Rgba = AlphaColor<vello_common::peniko::color::Srgb>;

const BACKDROP: Rgba = AlphaColor::new([0.07, 0.08, 0.10, 1.0]);
const OUTLINE: Rgba = AlphaColor::new([0.47, 0.75, 1.0, 1.0]);

/// A resolved scene plus the Bézier paths it paints and the hit geometry that
/// answers for them. Re-solving is the expensive half, so it happens when the
/// scene changes, never per frame -- the same boundary a file-watching backend
/// would commit on.
struct Baked {
    scene: Option<ResolvedScene>,
    /// `(id, path)` in paint order: back to front.
    paths: Vec<(String, BezPath)>,
    /// The same paths, same order, same tolerance. What responds is what was
    /// drawn, by construction.
    hit: Hit,
    error: Option<String>,
}

impl Baked {
    fn build(source: &dyn PreviewScene) -> Self {
        let spec = source.spec();
        let scene = match resolve_scene(&spec) {
            Ok(scene) => scene,
            Err(e) => {
                return Self {
                    scene: None,
                    paths: Vec::new(),
                    hit: Hit::default(),
                    error: Some(format!("resolve failed: {e}")),
                }
            }
        };
        let mut paths = Vec::new();
        let mut hit = Hit::default();
        let mut error = None;
        // Declaration order, taken from the spec. `ResolvedScene::surfaces` is
        // a `BTreeMap`, so reading paint order off it would sort the scene
        // alphabetically and put the panel on top of its own controls.
        let surfaces = spec
            .surfaces
            .iter()
            .filter_map(|s| scene.surface(&s.id).map(|r| (s.id.clone(), r.path.clone())))
            .chain(source.overlay());
        for (id, path) in surfaces {
            match mui_vello::bez_path(&path, ARC_TOLERANCE) {
                Ok(bez) => {
                    if let Err(e) = hit.push(id.clone(), &path) {
                        error = Some(format!("hit {id}: {e}"));
                    }
                    paths.push((id, bez));
                }
                Err(e) => error = Some(format!("path {id}: {e}")),
            }
        }
        Self {
            scene: Some(scene),
            paths,
            hit,
            error,
        }
    }

    /// Bézier segments across every path. The number that moves when geometry
    /// is re-solved rather than re-rasterised.
    fn segments(&self) -> usize {
        self.paths.iter().map(|(_, b)| b.elements().len()).sum()
    }
}

struct App {
    scenes: Vec<Box<dyn PreviewScene>>,
    selected: usize,
    baked: Baked,
    input: Interaction,
    /// Where the specimen sits, in physical pixels. `None` until the user drags
    /// it, so until then it re-centres itself in whatever the window is now.
    pan: Option<Point>,
    /// The origin as of the press that started the gesture in flight.
    drag_from: Option<Point>,
    pointer: PointerInput,
    gpu: Option<Gpu>,
}

impl App {
    fn new() -> Self {
        let scenes = scenes::all();
        let baked = Baked::build(scenes[0].as_ref());
        let app = Self {
            scenes,
            selected: 0,
            baked,
            input: Interaction::new(),
            pan: None,
            drag_from: None,
            pointer: PointerInput::default(),
            gpu: None,
        };
        app.report();
        app
    }

    fn rebake(&mut self) {
        self.baked = Baked::build(self.scenes[self.selected].as_ref());
        self.report();
    }

    /// Until the status bar lands, the numbers go to stderr. They are the
    /// point of the gallery -- a segment count that moves when an axis moves is
    /// the proof that the shape was re-solved and not re-rasterised.
    fn report(&self) {
        let scene = &self.scenes[self.selected];
        match &self.baked.error {
            Some(e) => eprintln!("{}: {e}", scene.name()),
            None => eprintln!(
                "{}: {} surfaces, {} segments -- {}",
                scene.name(),
                self.baked.paths.len(),
                self.baked.segments(),
                scene.about()
            ),
        }
    }

    fn select(&mut self, index: usize) {
        if index >= self.scenes.len() || index == self.selected {
            return;
        }
        self.selected = index;
        self.pan = None;
        self.rebake();
    }

    /// The specimen's top-left in physical pixels: wherever it was dragged to,
    /// or centred in the window. Translation only -- scaling here would stretch
    /// radii the geometry resolved exactly, which is the bug this library
    /// exists to avoid.
    fn origin(&self, (width, height): (u32, u32)) -> Point {
        if let Some(pan) = self.pan {
            return pan;
        }
        let size = match &self.baked.scene {
            Some(scene) => scene.layout.size,
            None => return Point::new(0., 0.),
        };
        Point::new(
            (width as f64 - size.width) / 2.,
            (height as f64 - size.height) / 2.,
        )
    }

    /// One interaction frame. Separate from drawing so a missed redraw can
    /// never swallow a gesture.
    fn tick(&mut self, size: (u32, u32)) {
        let origin = self.origin(size);
        // The frame the pointer is reported in must hold still for as long as a
        // gesture does. Reporting against the live origin while panning by the
        // delta that comes back differences the pan against itself -- the scene
        // then alternates between two positions every frame instead of
        // following the hand. The captured target is the same either way, which
        // is why freezing the frame costs nothing.
        let frame = self.drag_from.unwrap_or(origin);
        self.input.update(
            &self.baked.hit,
            PointerInput {
                pos: self.pointer.pos.map(|p| p - frame),
                primary_down: self.pointer.primary_down,
            },
        );
        // Drag anywhere on the specimen to move it. A press captures its
        // target, so it keeps tracking past the edge of the window.
        match self.input.held() {
            Some(id) => {
                let delta = self.input.get(id).drag_delta;
                self.drag_from.get_or_insert(origin);
                if delta != Point::new(0., 0.) {
                    self.pan = Some(origin + delta);
                }
            }
            None => self.drag_from = None,
        }
    }

    fn draw(&mut self) {
        let Some(size) = self.gpu.as_ref().map(Gpu::size) else {
            return;
        };
        let origin = self.origin(size);
        let gpu = self.gpu.as_mut().expect("checked just above");
        let scene = gpu.begin();
        scene.set_paint(BACKDROP);
        scene.fill_rect(&vello_common::kurbo::Rect::new(
            0.,
            0.,
            size.0 as f64,
            size.1 as f64,
        ));
        scene.set_transform(Affine::translate((origin.x, origin.y)));
        scene.set_stroke(Stroke::new(1.));
        for (i, (_, path)) in self.baked.paths.iter().enumerate() {
            let shade = 0.16 + (i % 5) as f32 * 0.06;
            scene.set_paint(Rgba::new([shade, shade + 0.02, shade + 0.05, 1.0]));
            scene.fill_path(path);
            scene.set_paint(OUTLINE);
            scene.stroke_path(path);
        }
        scene.reset_transform();
        gpu.present();
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gpu.is_some() {
            return; // Resumed can fire more than once; the surface is still good.
        }
        let attrs = Window::default_attributes()
            .with_title("MUI preview")
            .with_inner_size(winit::dpi::LogicalSize::new(1000, 680));
        let window = Arc::new(event_loop.create_window(attrs).expect("window"));
        // The GL backend needs the display handle to reach EGL/GLX. KURV's
        // Windows path prefers GL, so hand it over rather than hope for Vulkan.
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
                self.pointer.pos = Some(Point::new(position.x, position.y));
            }
            WindowEvent::CursorLeft { .. } => self.pointer.pos = None,
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => self.pointer.primary_down = state == ElementState::Pressed,
            WindowEvent::KeyboardInput { event, .. } if event.state.is_pressed() => {
                // Until the sidebar lands, the scene list is the number row.
                match event.logical_key {
                    Key::Character(ref c) => {
                        if let Some(n) = c.chars().next().and_then(|c| c.to_digit(10)) {
                            self.select(n.saturating_sub(1) as usize);
                        }
                    }
                    Key::Named(NamedKey::Escape) => event_loop.exit(),
                    _ => {}
                }
            }
            WindowEvent::RedrawRequested => {
                if let Some(gpu) = &self.gpu {
                    self.tick(gpu.size());
                }
                self.draw();
                return; // Drawing must not ask for another frame, or Wait spins.
            }
            _ => {}
        }
        // Every other arm changed something the next frame has to show. Asking
        // here rather than in `about_to_wait` is what keeps `ControlFlow::Wait`
        // from being a busy loop.
        if let Some(gpu) = &self.gpu {
            gpu.window().request_redraw();
        }
    }
}

fn main() {
    for (i, scene) in scenes::all().iter().enumerate() {
        eprintln!("  {}  {}", i + 1, scene.name());
    }
    let event_loop = EventLoop::new().expect("event loop");
    event_loop.set_control_flow(ControlFlow::Wait);
    event_loop.run_app(&mut App::new()).expect("run");
}

#[cfg(test)]
mod tests {
    use super::*;
    use vello_common::kurbo::{Point as KPoint, Shape as _};

    /// Every point on a grid spanning the scene, inflated far enough to include
    /// the outside.
    fn grid(baked: &Baked) -> Vec<Point> {
        let size = baked.scene.as_ref().expect("resolved").layout.size;
        let (w, h) = (size.width + 64., size.height + 64.);
        (0..64)
            .flat_map(move |iy| {
                (0..64).map(move |ix| {
                    Point::new(-32. + w * ix as f64 / 63., -32. + h * iy as f64 / 63.)
                })
            })
            .collect()
    }

    /// Every gallery scene must resolve and convert. This is the check that
    /// fails if a geometry change breaks a shape the gallery is meant to show.
    #[test]
    fn every_scene_bakes() {
        for scene in scenes::all() {
            let baked = Baked::build(scene.as_ref());
            assert!(baked.error.is_none(), "{}: {:?}", scene.name(), baked.error);
            assert!(!baked.paths.is_empty(), "{}: no surfaces", scene.name());
            assert!(baked.segments() > 0, "{}: no segments", scene.name());
        }
    }

    /// Paint order is the order the scene was written in, not the order a
    /// `BTreeMap` happens to hold. For `PillTab` those differ, which is what
    /// makes this test able to fail.
    #[test]
    fn paint_order_is_authored_order() {
        let scene = scenes::PillTab;
        let baked = Baked::build(&scene);
        let painted: Vec<&str> = baked.paths.iter().map(|(id, _)| id.as_str()).collect();
        let spec = scene.spec();
        let authored: Vec<&str> = spec.surfaces.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(painted, authored);

        let sorted: Vec<&str> = baked
            .scene
            .as_ref()
            .unwrap()
            .surfaces()
            .map(|(id, _)| id)
            .collect();
        assert_ne!(painted, sorted, "the BTreeMap order happens to match");
    }

    /// What answers the pointer is what was drawn. A grid this dense lands
    /// between two polylines at every fillet, so a tolerance mismatch, a paint
    /// order mismatch and a fill rule mismatch all show up here.
    #[test]
    fn hit_is_what_was_painted() {
        for scene in scenes::all() {
            let baked = Baked::build(scene.as_ref());
            for p in grid(&baked) {
                let q = KPoint::new(p.x, p.y);
                let painted = baked
                    .paths
                    .iter()
                    .rev()
                    .find(|(_, b)| b.winding(q) != 0)
                    .map(|(id, _)| id.as_str());
                assert_eq!(baked.hit.at(p), painted, "{} at {p:?}", scene.name());
            }
        }
    }

    /// The regression guard for `mui_geometry::boolean::topology`, which forces
    /// every exterior ring positive and every hole negative. While it does, the
    /// two fill rules agree on every resolved surface; if anyone stops
    /// normalising ring windings, this is what says so.
    #[test]
    fn both_fill_rules_agree_on_every_surface() {
        for scene in scenes::all() {
            let baked = Baked::build(scene.as_ref());
            let resolved = baked.scene.as_ref().expect("resolved");
            for (id, surface) in resolved.surfaces() {
                let bez = mui_vello::bez_path(&surface.path, ARC_TOLERANCE).expect("path");
                for p in grid(&baked) {
                    let w = bez.winding(KPoint::new(p.x, p.y));
                    assert_eq!(
                        w != 0,
                        w % 2 != 0,
                        "{} / {id} disagrees at {p:?} (winding {w})",
                        scene.name()
                    );
                }
            }
        }
    }

    /// A drag moves the scene by exactly what the hand moved, every frame.
    /// The frame that fails here is the second one: pan by a delta measured
    /// against an origin that already moved and the scene oscillates.
    #[test]
    fn a_drag_tracks_the_hand_one_to_one() {
        let mut app = App::new();
        let size = (800, 600);
        let start = app.origin(size);
        // Press inside the specimen, then three frames of steady motion. The
        // first crosses the drag threshold; the rest are pure tracking.
        let on = start + Point::new(60., 120.);
        for (i, step) in [0., 0., 20., 20., 20.].iter().enumerate() {
            app.pointer.pos = Some(on + Point::new(*step * i as f64, 0.));
            app.pointer.primary_down = i > 0;
            app.tick(size);
        }
        let moved = app.origin(size) - start;
        assert_eq!(moved.y, 0.);
        assert!(moved.x > 0., "the drag went nowhere: {moved:?}");
        // Two more identical steps must move the scene by two identical
        // amounts. Oscillation shows up as the second one being zero.
        let mut deltas = Vec::new();
        for i in 5..7 {
            let before = app.origin(size);
            app.pointer.pos = Some(on + Point::new(20. * i as f64, 0.));
            app.tick(size);
            deltas.push(app.origin(size) - before);
        }
        assert_eq!(
            deltas[0], deltas[1],
            "the pan alternates instead of tracking"
        );
        assert_eq!(deltas[0], Point::new(20., 0.));
    }

    /// The stage is a translation and nothing else, so a pointer taken into
    /// scene space and back lands where it started.
    #[test]
    fn the_stage_round_trips() {
        let mut app = App::new();
        app.pan = Some(Point::new(37.5, -11.25));
        let origin = app.origin((800, 600));
        let p = Point::new(123.75, 44.5);
        assert_eq!(origin + (p - origin), p);
        assert_eq!(origin - origin, Point::new(0., 0.));
    }
}
