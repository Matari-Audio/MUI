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
//! real host. The sidebar is [`ui`], built out of the same geometry.
//!
//! Not a dependency of `mui`, so nothing here reaches a plugin build.
//!
//! Run:   `cargo run -p mui-preview`
//! Watch: `bacon` (see bacon.toml)
#![forbid(unsafe_code)]

mod host;
mod scenes;
mod ui;

use std::sync::Arc;

use host::Gpu;
use mui_core::{resolve_scene, ResolvedScene};
use mui_geometry::{Bounds, Point, RoundedRect};
use mui_input::{Hit, Interaction, PointerInput};
use mui_vello::ARC_TOLERANCE;
use scenes::PreviewScene;
use ui::{Chrome, Rgba};
use vello_common::kurbo::{Affine, BezPath, Stroke};
use vello_common::peniko::color::AlphaColor;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

const BACKDROP: Rgba = AlphaColor::new([0.07, 0.08, 0.10, 1.0]);
const OUTLINE: Rgba = AlphaColor::new([0.47, 0.75, 1.0, 1.0]);
const FRAME: Rgba = AlphaColor::new([0.95, 0.45, 0.75, 0.60]);

/// The sidebar's width in logical points. The stage centres in what is left.
const SIDEBAR: f64 = 232.0;

/// Logical points per wheel notch, for the mice that report notches.
const ROW_SCROLL: f64 = 48.0;

/// A resolved scene plus the Bézier paths it paints and the hit geometry that
/// answers for them. Re-solving is the expensive half, so it happens when the
/// scene changes, never per frame -- the same boundary a file-watching backend
/// would commit on.
#[derive(Default)]
struct Baked {
    scene: Option<ResolvedScene>,
    /// `(id, path)` in paint order: back to front.
    paths: Vec<(String, BezPath)>,
    /// The same paths, same order, same tolerance. What responds is what was
    /// drawn, by construction.
    hit: Hit,
    /// The layout's frames, stroked, and their keys, filled. Two lists because
    /// one is stroked and the other is not; a flag per entry would only move
    /// that fact somewhere harder to read.
    frame_rects: Vec<BezPath>,
    frame_labels: Vec<BezPath>,
    error: Option<String>,
}

impl Baked {
    fn build(source: &dyn PreviewScene, font: &[u8]) -> Self {
        let spec = source.spec();
        let scene = match resolve_scene(&spec) {
            Ok(scene) => scene,
            Err(e) => {
                return Self {
                    error: Some(format!("resolve failed: {e}")),
                    ..Self::default()
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
        let (frame_rects, frame_labels) = frame_overlay(&scene, font);
        Self {
            scene: Some(scene),
            paths,
            hit,
            frame_rects,
            frame_labels,
            error,
        }
    }

    /// Bézier segments across every path. The number that moves when geometry
    /// is re-solved rather than re-rasterised.
    fn segments(&self) -> usize {
        self.paths.iter().map(|(_, b)| b.elements().len()).sum()
    }
}

/// What `mui-layout` decided, drawn on top of what `mui-core` made of it. The
/// two disagreeing is the failure this is here to show, so the frames are taken
/// from the layout and never from the surfaces.
///
/// Scene units, not pixels: the stage is a pure translation, so the label size
/// here is the size on screen.
fn frame_overlay(scene: &ResolvedScene, font: &[u8]) -> (Vec<BezPath>, Vec<BezPath>) {
    let mut rects = Vec::new();
    let mut labels = Vec::new();
    for (key, frame) in scene.layout.frames() {
        let bounds = Bounds::new(frame.x, frame.y, frame.right(), frame.bottom());
        if let Ok(rect) = RoundedRect::new(bounds, 0.) {
            if let Ok(bez) = mui_vello::bez_path(&rect.path(), ARC_TOLERANCE) {
                rects.push(bez);
            }
        }
        let placed = mui_text::text_run(font, key, 10., &[], ARC_TOLERANCE)
            .ok()
            .and_then(|run| {
                run.path
                    .rigid_transform(Point::new(frame.x + 3., frame.y + 12.), 0.)
                    .ok()
            });
        if let Some(path) = placed
            .as_ref()
            .and_then(|p| mui_vello::bez_path(p, ARC_TOLERANCE).ok())
        {
            labels.push(path);
        }
    }
    (rects, labels)
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
    /// Ownership of a gesture is decided at the press and held until release.
    /// Recomputing it per frame lets a press that began in the sidebar grab the
    /// specimen the instant the pointer crosses into the stage.
    sidebar_gesture: bool,
    /// Button transitions since the last frame. winit dispatches every queued
    /// event and *then* one coalesced redraw, so a tap whose press and release
    /// land in the same batch has no edge left if we only keep the final level.
    buttons: Vec<bool>,
    /// One character, consumed by whichever field has focus. Dropped if none
    /// does -- a keystroke with nowhere to go is not an error.
    typed: Option<char>,
    chrome: Chrome,
    font: Arc<Vec<u8>>,
    show_fill: bool,
    show_frames: bool,
    /// How many times the geometry has been re-solved. Named in the sidebar
    /// because it is the claim the gallery makes: a moved axis re-solves the
    /// shape, and an idle frame does not.
    rebakes: usize,
    /// This frame's sidebar, ready to paint.
    chrome_paint: Vec<(BezPath, Rgba)>,
    gpu: Option<Gpu>,
}

impl App {
    fn new() -> Self {
        let font = Arc::new(epaint_default_fonts::HACK_REGULAR.to_vec());
        let scenes = scenes::all();
        let baked = Baked::build(scenes[0].as_ref(), &font);
        Self {
            scenes,
            selected: 0,
            baked,
            input: Interaction::new(),
            pan: None,
            drag_from: None,
            pointer: PointerInput::default(),
            sidebar_gesture: false,
            buttons: Vec::new(),
            typed: None,
            chrome: Chrome::new(font.clone()),
            font,
            rebakes: 0,
            show_fill: true,
            show_frames: false,
            chrome_paint: Vec::new(),
            gpu: None,
        }
    }

    fn rebake(&mut self) {
        self.baked = Baked::build(self.scenes[self.selected].as_ref(), &self.font);
        self.rebakes += 1;
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
    /// or centred in what the sidebar leaves. Translation only -- scaling here
    /// would stretch radii the geometry resolved exactly, which is the bug this
    /// library exists to avoid.
    fn origin(&self, (width, height): (u32, u32), scale: f64) -> Point {
        if let Some(pan) = self.pan {
            return pan;
        }
        let left = SIDEBAR * scale;
        let size = match &self.baked.scene {
            Some(scene) => scene.layout.size,
            None => return Point::new(left, 0.),
        };
        Point::new(
            left + (width as f64 - left - size.width) / 2.,
            (height as f64 - size.height) / 2.,
        )
    }

    /// One interaction frame: the sidebar first, then the stage with whatever
    /// the sidebar did not claim. Separate from drawing so a missed redraw can
    /// never swallow a gesture.
    fn tick(&mut self, size: (u32, u32), scale: f64) {
        self.chrome_frame(size, scale);

        let origin = self.origin(size, scale);
        // A gesture already on the specimen keeps it, wherever the pointer
        // goes; otherwise the sidebar gets first refusal on its own column.
        let dragging = self.input.held().is_some();
        if !self.pointer.primary_down {
            self.sidebar_gesture = false;
        }
        let claimed = !dragging
            && (self.sidebar_gesture
                || self.chrome.busy()
                || self.pointer.pos.is_some_and(|p| p.x < SIDEBAR * scale));
        self.sidebar_gesture |= claimed && self.pointer.primary_down;

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
                pos: (!claimed)
                    .then_some(self.pointer.pos)
                    .flatten()
                    .map(|p| p - frame),
                primary_down: !claimed && self.pointer.primary_down,
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

    /// Lay the sidebar out and act on it. Returns nothing: everything it can do
    /// -- select a scene, toggle a view, force a rebake -- is done here, so the
    /// caller has no result to forget to handle.
    fn chrome_frame(&mut self, size: (u32, u32), scale: f64) {
        let Self {
            chrome,
            scenes,
            selected,
            baked,
            rebakes,
            pointer,
            typed,
            show_fill,
            show_frames,
            chrome_paint,
            ..
        } = self;
        let bounds = Bounds::new(0., 0., SIDEBAR * scale, size.1 as f64);
        let mut ui = chrome.column(bounds, *pointer, typed.take(), scale);
        ui.label("MUI preview");
        ui.note("mui-layout places, mui-core merges, vello draws");
        ui.separator();

        let mut pick = None;
        for (i, scene) in scenes.iter().enumerate() {
            if ui.option(i == *selected, scene.name()) {
                pick = Some(i);
            }
        }
        ui.separator();
        ui.checkbox(show_fill, "fill surfaces");
        ui.checkbox(show_frames, "layout frames");
        let rebuild = ui.button("Rebuild");
        ui.separator();

        match &baked.error {
            Some(e) => ui.error(e),
            None => ui.note(&format!(
                "{} surfaces, {} segments, {rebakes} re-solves",
                baked.paths.len(),
                baked.segments()
            )),
        }
        ui.note(scenes[*selected].about());
        ui.separator();
        // The selected scene's own knobs, last, so adding one never moves the
        // gallery's controls out from under the pointer.
        // Scene ids live in their own namespace, so switching scenes can never
        // resolve a stale press onto the new scene's controls.
        ui.scope(scenes[*selected].name());
        let retune = scenes[*selected].controls(&mut ui);

        *chrome_paint = ui
            .finish()
            .iter()
            .filter_map(|(path, ink)| Some((mui_vello::bez_path(path, ARC_TOLERANCE).ok()?, *ink)))
            .collect();

        if let Some(i) = pick {
            self.select(i);
        } else if rebuild || retune {
            self.rebake();
        }
    }

    fn draw(&mut self) {
        let Some((size, scale)) = self
            .gpu
            .as_ref()
            .map(|gpu| (gpu.size(), gpu.window().scale_factor()))
        else {
            return;
        };
        let origin = self.origin(size, scale);
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
            if self.show_fill {
                let shade = 0.16 + (i % 5) as f32 * 0.06;
                scene.set_paint(Rgba::new([shade, shade + 0.02, shade + 0.05, 1.0]));
                scene.fill_path(path);
            }
            scene.set_paint(OUTLINE);
            scene.stroke_path(path);
        }
        if self.show_frames {
            scene.set_paint(FRAME);
            for rect in &self.baked.frame_rects {
                scene.stroke_path(rect);
            }
            for label in &self.baked.frame_labels {
                scene.fill_path(label);
            }
        }

        // The sidebar is drawn in window space, last, over its own opaque
        // panel -- which is what keeps a panned specimen from showing through.
        scene.reset_transform();
        for (path, ink) in &self.chrome_paint {
            scene.set_paint(*ink);
            scene.fill_path(path);
        }
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
            // The column is the only thing that scrolls; the stage is dragged.
            WindowEvent::MouseWheel { delta, .. } => {
                let scale = self.gpu.as_ref().map_or(1., |g| g.window().scale_factor());
                if self.pointer.pos.is_some_and(|p| p.x < SIDEBAR * scale) {
                    self.chrome.scroll_by(match delta {
                        MouseScrollDelta::LineDelta(_, y) => f64::from(y) * ROW_SCROLL * scale,
                        MouseScrollDelta::PixelDelta(p) => p.y,
                    });
                }
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => self.buttons.push(state == ElementState::Pressed),
            WindowEvent::KeyboardInput { ref event, .. } if event.state.is_pressed() => {
                if event.logical_key == Key::Named(NamedKey::Escape) {
                    event_loop.exit();
                } else {
                    // Typed characters belong to whatever field has focus. The
                    // sidebar decides; this only carries. `text` is what the
                    // keypress actually produced -- `logical_key` would hand a
                    // plain "v" to Ctrl+V and would drop a composed dead key.
                    self.typed = event.text.as_ref().and_then(|t| t.chars().next());
                }
            }
            WindowEvent::RedrawRequested => {
                if let Some(gpu) = &self.gpu {
                    let (size, scale) = (gpu.size(), gpu.window().scale_factor());
                    let buttons = std::mem::take(&mut self.buttons);
                    if buttons.is_empty() {
                        self.tick(size, scale);
                    }
                    for down in buttons {
                        self.pointer.primary_down = down;
                        self.tick(size, scale);
                    }
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
    let event_loop = EventLoop::new().expect("event loop");
    event_loop.set_control_flow(ControlFlow::Wait);
    event_loop.run_app(&mut App::new()).expect("run");
}

#[cfg(test)]
mod tests {
    use super::*;
    use vello_common::kurbo::{Point as KPoint, Shape as _};

    fn font() -> Vec<u8> {
        epaint_default_fonts::HACK_REGULAR.to_vec()
    }

    fn bake(scene: &dyn PreviewScene) -> Baked {
        Baked::build(scene, &font())
    }

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
            let baked = bake(scene.as_ref());
            assert!(baked.error.is_none(), "{}: {:?}", scene.name(), baked.error);
            assert!(!baked.paths.is_empty(), "{}: no surfaces", scene.name());
            assert!(baked.segments() > 0, "{}: no segments", scene.name());
        }
    }

    /// Every layout frame gets an outline and a legible key. A scene whose
    /// frames vanish from the overlay is a scene the gallery cannot debug.
    #[test]
    fn every_frame_is_drawn_and_labelled() {
        for scene in scenes::all() {
            let baked = bake(scene.as_ref());
            let frames = baked
                .scene
                .as_ref()
                .expect("resolved")
                .layout
                .frames()
                .count();
            assert_eq!(baked.frame_rects.len(), frames, "{}", scene.name());
            assert_eq!(baked.frame_labels.len(), frames, "{}", scene.name());
        }
    }

    /// Paint order is the order the scene was written in, not the order a
    /// `BTreeMap` happens to hold. For `PillTab` those differ, which is what
    /// makes this test able to fail.
    #[test]
    fn paint_order_is_authored_order() {
        let scene = scenes::PillTab;
        let baked = bake(&scene);
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
            let baked = bake(scene.as_ref());
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
            let baked = bake(scene.as_ref());
            let resolved = baked.scene.as_ref().expect("resolved");
            let grid = grid(&baked);
            for (id, surface) in resolved.surfaces() {
                let bez = mui_vello::bez_path(&surface.path, ARC_TOLERANCE).expect("path");
                for p in grid.iter().copied() {
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
        let start = app.origin(size, 1.);
        // Press inside the specimen, then three frames of steady motion. The
        // first crosses the drag threshold; the rest are pure tracking.
        let on = start + Point::new(60., 120.);
        for (i, step) in [0., 0., 20., 20., 20.].iter().enumerate() {
            app.pointer.pos = Some(on + Point::new(*step * i as f64, 0.));
            app.pointer.primary_down = i > 0;
            app.tick(size, 1.);
        }
        let moved = app.origin(size, 1.) - start;
        assert_eq!(moved.y, 0.);
        assert!(moved.x > 0., "the drag went nowhere: {moved:?}");
        // Two more identical steps must move the scene by two identical
        // amounts. Oscillation shows up as the second one being zero.
        let mut deltas = Vec::new();
        for i in 5..7 {
            let before = app.origin(size, 1.);
            app.pointer.pos = Some(on + Point::new(20. * i as f64, 0.));
            app.tick(size, 1.);
            deltas.push(app.origin(size, 1.) - before);
        }
        assert_eq!(
            deltas[0], deltas[1],
            "the pan alternates instead of tracking"
        );
        assert_eq!(deltas[0], Point::new(20., 0.));
    }

    /// The sidebar column is not part of the stage. A press there must not
    /// drag the specimen out from under the cursor.
    #[test]
    fn the_sidebar_does_not_drag_the_stage() {
        let mut app = App::new();
        let size = (800, 600);
        app.pan = Some(Point::new(-40., 40.)); // Overlap the column deliberately.
        let start = app.origin(size, 1.);
        // Past the column's own edge, because the interesting failure is the
        // crossing: if ownership is recomputed per frame instead of latched at
        // the press, the stage sees a fresh press the moment x clears SIDEBAR.
        for i in 0..10 {
            app.pointer.pos = Some(Point::new(40. + 40. * i as f64, 200.));
            app.pointer.primary_down = i > 0;
            app.tick(size, 1.);
        }
        assert_eq!(app.origin(size, 1.), start, "the sidebar moved the stage");
    }

    /// A tap whose press and release arrive in the same winit batch is still a
    /// click. The event loop keeps the transitions, not the final level, so no
    /// edge can be coalesced away while the loop is behind.
    #[test]
    fn a_tap_inside_one_event_batch_still_clicks() {
        let mut app = App::new();
        let size = (800, 600);
        app.tick(size, 1.);
        let at = (0..500)
            .map(|i| Point::new(40., i as f64 * 2.))
            .find(|p| {
                app.chrome
                    .at(*p)
                    .is_some_and(|id| id.ends_with(app.scenes[1].name()))
            })
            .unwrap();
        app.pointer.pos = Some(at);
        // Both transitions queued before a single redraw ever runs.
        app.buttons.extend([true, false]);
        let buttons = std::mem::take(&mut app.buttons);
        for down in buttons {
            app.pointer.primary_down = down;
            app.tick(size, 1.);
        }
        assert_eq!(app.selected, 1, "the tap was swallowed");
    }

    /// Clicking a scene in the sidebar is the same thing the number row used
    /// to do: the gallery shows the next specimen.
    #[test]
    fn the_scene_list_selects() {
        let mut app = App::new();
        let size = (800, 600);
        assert!(app.scenes.len() > 1, "one scene cannot test a list");
        // Find the second option's row by laying the column out once, then
        // click it. Frame one commits the geometry; two and three are the
        // press and the release that make a click.
        let second = app.scenes[1].name();
        click(&mut app, size, second);
        assert_eq!(app.selected, 1);
    }

    /// Move the pointer onto the first sidebar widget whose id ends in
    /// `suffix`, then press and release: a click.
    fn click(app: &mut App, size: (u32, u32), suffix: &str) -> Point {
        app.tick(size, 1.);
        let at = (0..500)
            .map(|i| Point::new(40., i as f64 * 2.))
            .find(|p| app.chrome.at(*p).is_some_and(|id| id.ends_with(suffix)))
            .unwrap_or_else(|| panic!("no widget ending in {suffix:?} in the column"));
        for down in [true, false] {
            app.pointer.pos = Some(at);
            app.pointer.primary_down = down;
            app.tick(size, 1.);
        }
        at
    }

    /// Frames where nothing happened cost nothing. The gallery re-solves on
    /// change, not on redraw -- this is the test that says so.
    #[test]
    fn an_idle_frame_never_rebakes() {
        let mut app = App::new();
        let before = app.rebakes;
        for _ in 0..8 {
            app.tick((800, 600), 1.);
        }
        assert_eq!(app.rebakes, before);
    }

    /// Dragging a variation axis re-solves the glyph. Without a variable face
    /// the size slider is the axis that always exists, and moving it has to
    /// change the segment count -- otherwise the outline was scaled, not
    /// rebuilt, which is the whole thing this gallery denies.
    #[test]
    fn a_dragged_axis_rebakes() {
        let mut app = App::new();
        let size = (900, 700);
        let glyphs = app
            .scenes
            .iter()
            .position(|s| s.name() == "Glyph axes")
            .expect("the glyph scene");
        app.select(glyphs);

        let knob = click(&mut app, size, ":size");
        let before = (app.rebakes, app.baked.segments());
        // Press the track and drag right: a bigger glyph, more segments. Right
        // because the click above already pinned the value near the low end,
        // and left of the track is outside the widget.
        for (i, dx) in [0., 30., 60.].iter().enumerate() {
            app.pointer.pos = Some(knob + Point::new(*dx, 0.));
            app.pointer.primary_down = i > 0;
            app.tick(size, 1.);
        }
        assert!(app.rebakes > before.0, "the axis never re-solved");
        assert_ne!(
            app.baked.segments(),
            before.1,
            "the outline was not rebuilt, only redrawn"
        );
    }

    /// The stage is a translation and nothing else, so a pointer taken into
    /// scene space and back lands where it started.
    #[test]
    fn the_stage_centres_in_what_the_sidebar_leaves() {
        let app = App::new();
        let size = (1000_u32, 700_u32);
        let o = app.origin(size, 1.);
        let specimen = app
            .baked
            .scene
            .as_ref()
            .expect("the first scene baked")
            .layout
            .size;
        // Equal margins inside the stage, not inside the window: forgetting the
        // sidebar would hide the specimen's left edge under the column.
        assert_eq!(
            o.x - SIDEBAR,
            size.0 as f64 - specimen.width - o.x,
            "not centred beside the sidebar"
        );
        assert_eq!(o.y, size.1 as f64 - specimen.height - o.y, "not centred");
        assert!(o.x >= SIDEBAR, "the specimen starts under the column");
    }
}
