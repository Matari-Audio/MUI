//! GPUI presentation for the real MUI Item DSL. Layout has one owner: MUI.
use gpui::{
    App, CursorStyle, Div, Hsla, PathBuilder, Role, SharedString, TextRun, Window, WrappedLine,
    canvas, div, font, point, prelude::*, px, rgb, rgba, white,
};
pub use mui::prelude::*;
use std::rc::Rc;

pub struct Resolved {
    pub minimum_width: f64,
    text: std::collections::BTreeMap<String, String>,
    styles: std::collections::BTreeMap<String, mui::core::ItemStyle>,
    paths: std::cell::RefCell<Option<(u32, Vec<(gpui::Path<gpui::Pixels>, Hsla)>)>>,
    horizontal_stroke_gaps: Vec<(f64,f64,f64)>,
    ui: Ui,
    scene: mui::core::ResolvedScene,
}
fn ink(c: Rgb) -> Hsla {
    rgb((c.0 as u32) << 16 | (c.1 as u32) << 8 | c.2 as u32).into()
}
fn shape(text: &str, style: (f32, f32, Horizontal, bool), window: &Window) -> anyhow::Result<Vec<WrappedLine>> {
    Ok(window
        .text_system()
        .shape_text(
            text.to_owned().into(),
            px(style.0),
            &[TextRun {
                len: text.len(),
                font: gpui::Font { weight: gpui::FontWeight(style.1), ..font("DejaVu Sans") },
                color: white(),
                background_color: None,
                underline: None,
                strikethrough: None,
            }],
            None,
            None,
        )?
        .into_vec())
}
impl Resolved {
    pub fn new(tree: Item, width: f64, height: f64, window: &Window) -> anyhow::Result<Self> {
        Self::resolve(tree, width, height, true, window)
    }
    /// Honor the viewport width. Declare `.shrink(1.)` on flexible regions and
    /// keep meaningful minimums on controls; native slots use the resulting real bounds.
    pub fn responsive(tree: Item, width: f64, height: f64, window: &Window) -> anyhow::Result<Self> {
        Self::resolve(tree, width, height, false, window)
    }
    fn resolve(tree: Item, width: f64, height: f64, intrinsic_width: bool, window: &Window) -> anyhow::Result<Self> {
        let intrinsic = tree.clone().width(Hug).build()?;
        let measure = |id: &str, text: &str, _: mui::layout::MeasureInput| {
            let style = intrinsic.info(id).unwrap().text_style;
            if style.3 {
                let ink = crate::oscillator::label_weighted(text, 0., 0., style.0, true, 0, style.1)
                    .map_err(|e| mui::layout::Error::Backend(e.to_string()))?;
                return Ok(mui::layout::Measurement { baseline: None,
                    size: mui::layout::Size::new(f64::from(ink.path.bounds.size.width), f64::from(ink.path.bounds.size.height)) });
            }
            let lines =
                shape(text, style, window).map_err(|e| mui::layout::Error::Backend(e.to_string()))?;
            Ok(mui::layout::Measurement {
                baseline: Some(f64::from(style.0)),
                size: mui::layout::Size::new(
                    lines
                        .iter()
                        .map(|l| f64::from(l.size(px(style.0 * 1.25)).width))
                        .fold(0., f64::max),
                    lines.len() as f64 * f64::from(style.0 * 1.25),
                ),
            })
        };
        let minimum = intrinsic.resolve_with_baseline(measure)?;
        let minimum_width = intrinsic
            .items()
            .filter_map(|(id, _)| minimum.layout.frame(id))
            .map(|f| f.x + f.size.width)
            .fold(0., f64::max);
        let minimum_height = intrinsic
            .items()
            .filter_map(|(id, _)| minimum.layout.frame(id))
            .map(|f| f.y + f.size.height)
            .fold(0., f64::max);
        let ui = tree.build()?.offered(if intrinsic_width { width.max(minimum_width) } else { width }, height.max(minimum_height));
        let scene = ui.resolve_with_baseline(measure)?;
        let styles = ui.resolved_styles(None)?.into_iter().map(|(id,s)|(id.to_owned(),s)).collect();
        Ok(Self {
            styles,
            paths: Default::default(),
            horizontal_stroke_gaps: Vec::new(),
            ui,
            scene,
            minimum_width,
            text: Default::default(),
        })
    }
    /// Leave space for controls on horizontal outlines; tuples are (y, left, right).
    /// Fills retain their original geometry, with no background patch covering the stroke.
    pub fn set_horizontal_stroke_gaps(&mut self, gaps:Vec<(f64,f64,f64)>) {
        if self.horizontal_stroke_gaps!=gaps {
            self.horizontal_stroke_gaps=gaps;
            *self.paths.borrow_mut()=None;
        }
    }
    /// Update a readout without resolving geometry again. Reserve its widest value in the DSL.
    pub fn set_text(&mut self, id: impl Into<String>, text: impl Into<String>) {
        self.text.insert(id.into(), text.into());
    }
    /// Stack sticky surfaces in scroll-content coordinates; no springs or duplicate hitboxes.
    pub fn sticky_slot(&self, id: &str, scroll: gpui::ScrollHandle, stack_top: f32, child: impl IntoElement) -> anyhow::Result<Div> {
        let origin = self.frame(id)?.y as f32;
        self.slot(id, Sticky { child: child.into_any_element(), scroll, origin, stack_top })
    }
    pub fn frame(&self, id: &str) -> anyhow::Result<mui::layout::Frame> {
        self.scene
            .layout
            .content_frame(id)
            .ok_or_else(|| anyhow::anyhow!("Missing DSL slot: {id}"))
    }
    pub fn slot(&self, id: &str, child: impl IntoElement) -> anyhow::Result<Div> {
        let f = self.frame(id)?;
        Ok(div()
            .absolute()
            .left(px(f.x as f32))
            .top(px(f.y as f32))
            .w(px(f.size.width as f32))
            .h(px(f.size.height as f32))
            .opacity(self.ui.info(id).map_or(1., |info| info.opacity))
            .child(div().id(gpui::SharedString::from(format!("slot:{id}"))).size_full().child(child)))
    }
    pub fn element(
        &self,
        window: &Window,
        action: impl Fn(&str, &mut Window, &mut App) + 'static,
    ) -> anyhow::Result<Div> {
        let styles = &self.styles;
        let scale = window.scale_factor().to_bits();
        let mut cached = self.paths.borrow_mut();
        if cached.as_ref().is_none_or(|(old,_)| *old != scale) {
            let mut paths = Vec::new();
            for (surface, info) in self.ui.outlines(&self.scene) {
                if info.hover_only { continue; }
                let style = &styles[surface.id.as_str()];
                let rings = surface.path.flatten(0.1 / f64::from(window.scale_factor()), 100_000)?;
                for (color, stroke) in style.fill.map(|c|(c,None)).into_iter().chain(style.stroke.map(|(c,w)|(c,Some(w)))) {
                    let mut path = stroke.map_or_else(PathBuilder::fill, |w| PathBuilder::stroke(px(w as f32)));
                    for ring in &rings {
                        let Some(first)=ring.first() else {continue;};
                        let mut interrupted=false;
                        path.move_to(point(px(first.x as f32),px(first.y as f32)));
                        for pair in ring.iter().zip(ring.iter().skip(1).chain(std::iter::once(first))) {
                            let (a,b)=pair;
                            if stroke.is_some() && (a.y-b.y).abs()<0.01 {
                                if let Some((_,left,right))=self.horizontal_stroke_gaps.iter().find(|(y,left,right)|
                                    (a.y-y).abs()<=2. && a.x.min(b.x)<*right && a.x.max(b.x)>*left) {
                                    interrupted=true;
                                    let left=left.max(a.x.min(b.x));let right=right.min(a.x.max(b.x));
                                    let (start,end)=if a.x<b.x {(left,right)}else{(right,left)};
                                    path.line_to(point(px(start as f32),px(a.y as f32)));
                                    path.move_to(point(px(end as f32),px(a.y as f32)));
                                }
                            }
                            path.line_to(point(px(b.x as f32),px(b.y as f32)));
                        }
                        if !interrupted {path.close();}
                    }
                    let mut color=ink(color); color.a*=info.opacity;
                    paths.push((path.build()?,color));
                }
            }
            *cached=Some((scale,paths));
        }
        let paths=cached.as_ref().unwrap().1.clone();
        let painter = canvas(|_,_,_| (), move |bounds,_,window,_| {
            for (mut path,color) in paths {
                path.bounds.origin+=bounds.origin;
                for vertex in &mut path.vertices {vertex.xy_position+=bounds.origin;}
                window.paint_path(path,color);
            }
        }).absolute().size_full();
        let action = Rc::new(action);
        let mut root = div().relative().size_full().child(painter);
        for (id, info) in self.ui.items() {
            if info.text.is_none() && info.tap.is_none() { continue; }
            let f = self.scene.layout.frame(id).unwrap();
            let content = self.frame(id)?;
            let mut element = div().id(SharedString::from(id.to_owned())).absolute()
                .left(px(f.x as f32)).top(px(f.y as f32))
                .w(px(f.size.width as f32)).h(px(f.size.height as f32)).opacity(info.opacity);
            if let Some(text) = self.text.get(id).or(info.text.as_ref()) {
                let (size, weight, alignment, vertical) = info.text_style;
                let text = if vertical {
                    vertical_label(text.clone(), size, weight, ink(styles[id].text)).into_any_element()
                } else {
                    div().size_full().font_family("DejaVu Sans").text_size(px(size)).line_height(px(size * 1.25))
                        .font_weight(gpui::FontWeight(weight)).text_color(ink(styles[id].text))
                        .text_align(match alignment { Horizontal::Left => gpui::TextAlign::Left,
                            Horizontal::Right => gpui::TextAlign::Right, _ => gpui::TextAlign::Center })
                        .whitespace_nowrap().child(text.clone()).into_any_element()
                };
                element = element.child(div().absolute()
                    .left(px((content.x - f.x) as f32)).top(px((content.y - f.y) as f32))
                    .w(px(content.size.width as f32)).h(px(content.size.height as f32)).child(text));
            }
            if let Some(tap) = info.tap.as_ref().filter(|_| !info.disabled) {
                let (tap, key_tap) = (tap.clone(), tap.clone());
                let (action, key_action) = (action.clone(), action.clone());
                element = element.tab_index(0).role(Role::Button)
                    .aria_label(info.text.clone().unwrap_or_else(|| tap.clone()))
                    .cursor(CursorStyle::PointingHand).rounded_sm()
                    .hover(|s| s.bg(rgba(0xffffff12))).focus_visible(|s| s.bg(rgba(0xffffff22)))
                    .on_key_down(move |event, w, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            key_action(&key_tap, w, cx); cx.stop_propagation();
                        }
                    }).on_click(move |_, w, cx| action(&tap, w, cx));
            }
            if info.hover_only {
                let parent = self.scene.layout.frame(info.parent.as_deref().unwrap_or(id)).unwrap();
                let group = SharedString::from(format!("hover-{id}"));
                element = div().id(group.clone()).absolute().left(px(parent.x as f32)).top(px(parent.y as f32))
                    .w(px(parent.size.width as f32)).h(px(parent.size.height as f32)).group(group.clone())
                    .child(element.left(px((f.x - parent.x) as f32)).top(px((f.y - parent.y) as f32))
                        .opacity(0.).group_hover(group, |s| s.opacity(1.)).focus_visible(|s| s.opacity(1.)));
            }
            root = root.child(element);
        }
        Ok(root)
    }
}

/// Rotated identity text shares the existing cached variable-font outline renderer.
pub fn vertical_label(text: String, size: f32, weight: f32, color: Hsla) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            if let Ok(ink) = crate::oscillator::label_weighted(&text, 0., 0., size, true, 0, weight) {
                let mut path = ink.path;
                let old = path.bounds.center();
                let center = bounds.center();
                for vertex in &mut path.vertices {
                    vertex.xy_position = center + (vertex.xy_position - old);
                }
                path.bounds.origin = center - (path.bounds.size / 2.).into();
                window.paint_path(path, color);
            }
        },
    )
    .size_full()
}

/// Cached variable-font outlines can follow graph tangents without rotating a texture.
pub fn paint_angled_label(text: &str, size: f32, weight: f32, color: Hsla, angle: f32,
    center: gpui::Point<gpui::Pixels>, clip: gpui::Bounds<gpui::Pixels>, window: &mut Window) {
    if let Ok(ink) = crate::oscillator::label_weighted(text, 0., 0., size, false, 0, weight) {
        window.paint_path(angled_label_path(ink.path, angle, center, clip), color);
    }
}

fn angled_label_path(mut path: gpui::Path<gpui::Pixels>, angle: f32,
    center: gpui::Point<gpui::Pixels>, clip: gpui::Bounds<gpui::Pixels>) -> gpui::Path<gpui::Pixels> {
    let old = path.bounds.center();
    let (sin, cos) = angle.sin_cos();
    let mut low = point(px(f32::INFINITY), px(f32::INFINITY));
    let mut high = point(px(f32::NEG_INFINITY), px(f32::NEG_INFINITY));
    for vertex in &mut path.vertices {
        let delta=vertex.xy_position-old;
        let p=center+point(delta.x*cos-delta.y*sin,delta.x*sin+delta.y*cos);
        vertex.xy_position=p;
        low.x=low.x.min(p.x);low.y=low.y.min(p.y);
        high.x=high.x.max(p.x);high.y=high.y.max(p.y);
    }
    if path.vertices.is_empty(){return path;}
    let shift=point((clip.left()-low.x).max(px(0.))-(high.x-clip.right()).max(px(0.)),
        (clip.top()-low.y).max(px(0.))-(high.y-clip.bottom()).max(px(0.)));
    for vertex in &mut path.vertices {vertex.xy_position+=shift;}
    path.bounds=gpui::Bounds::from_corners(low+shift,high+shift);
    path
}

#[cfg(test)]
mod angled_label_tests {
    #[test]
    fn rotated_label_bounds_follow_vertices_and_stay_inside_graph() {
        use gpui::{point,px,Bounds,PathBuilder};
        let mut path=PathBuilder::fill();
        path.move_to(point(px(0.),px(0.)));path.line_to(point(px(30.),px(0.)));
        path.line_to(point(px(30.),px(10.)));path.line_to(point(px(0.),px(10.)));path.close();
        let clip=Bounds::from_corners(point(px(0.),px(0.)),point(px(80.),px(50.)));
        let rotated=super::angled_label_path(path.build().unwrap(),std::f32::consts::FRAC_PI_2,point(px(1.),px(1.)),clip);
        assert!((f32::from(rotated.bounds.size.width)-10.).abs()<0.01);
        assert!((f32::from(rotated.bounds.size.height)-30.).abs()<0.01);
        assert!(rotated.bounds.left()>=clip.left()&&rotated.bounds.top()>=clip.top());
        assert!(rotated.bounds.right()<=clip.right()&&rotated.bounds.bottom()<=clip.bottom());
    }
}

/// Bounded oblique projection of real waveform slices; selected signal remains full amplitude.
pub fn waveform(samples: std::sync::Arc<[f32]>, slices: std::sync::Arc<[std::sync::Arc<[f32]>]>, position: f32) -> impl IntoElement {
    waveform_tinted(samples, slices, position, crate::live_theme::color(0xbadc91))
}

fn waveform_sample_at(samples: &[f32], phase: f32) -> f32 {
    if samples.is_empty() { return 0.; }
    let source = phase.clamp(0., 1.) * samples.len().saturating_sub(1) as f32;
    let lower = source.floor() as usize;
    samples[lower] + (samples[(lower + 1).min(samples.len() - 1)] - samples[lower]) * source.fract()
}

pub fn waveform_tinted(samples: std::sync::Arc<[f32]>, slices: std::sync::Arc<[std::sync::Arc<[f32]>]>, position: f32, tint: Hsla) -> impl IntoElement {
    type Key = (usize, usize, [u32; 10]);
    type Mesh = (std::sync::Arc<[f32]>, std::sync::Arc<[std::sync::Arc<[f32]>]>, Vec<(gpui::Path<gpui::Pixels>, Hsla)>);
    thread_local! { static MESHES: std::cell::RefCell<std::collections::HashMap<Key, Mesh>> = Default::default(); }
    canvas(|_, _, _| (), move |bounds, _, window, _| {
        if samples.is_empty() { return; }
        let ink = |alpha| { let mut c = tint; c.a *= alpha; c };
        let color = ink(1.);
        let key = (samples.as_ptr() as usize, slices.as_ptr() as usize,
            [f32::from(bounds.left()), f32::from(bounds.top()), f32::from(bounds.size.width),
                f32::from(bounds.size.height), window.scale_factor(), position, color.h, color.s, color.l, color.a].map(f32::to_bits));
        MESHES.with(|cache| {
            let mut cache = cache.borrow_mut();
            // ponytail: bounded mesh cache; use LRU if large sessions churn this working set.
            if cache.len() >= 128 && !cache.contains_key(&key) { cache.clear(); }
            let (_, _, paths) = cache.entry(key).or_insert_with(|| {
            let mut paths = Vec::new();
            let depth = !slices.is_empty();
            for (wave, z, selected) in slices.iter().enumerate().rev().filter(|(_, wave)| !wave.is_empty())
                .map(|(i, wave)| (wave, i as f32 / (slices.len() - 1).max(1) as f32, false))
                .chain(std::iter::once((&samples, position.clamp(0., 1.), true))) {
                let mut line = Vec::new();
                let mut area = PathBuilder::fill();
                let project = |x: f32, y: f32| {
                    let (x, y) = if depth { (0.04 + x * 0.76 + z * 0.16, 0.70 - z * 0.38 - x * 0.10 - y * 0.21) }
                        else { (0.02 + x * 0.96, 0.5 - y * 0.42) };
                    bounds.origin + point(bounds.size.width * x, bounds.size.height * y)
                };
                area.move_to(project(0., 0.));
                let count = wave.len().saturating_sub(1).clamp(1, 4096);
                for n in 0..=count {
                    let x = n as f32 / count as f32;
                    let value = waveform_sample_at(wave, x);
                    let p = project(x, value);
                    line.push(p);
                    area.line_to(p);
                }
                area.line_to(project(1., 0.)); area.close();
                if selected { if let Ok(path) = area.build() { paths.push((path, ink(0.11))); } }
                paths.push((response_stroke(&line, if selected { 1.5 } else { 1. }, window.scale_factor()), ink(if selected { 1. } else { 0.25 })));
            }
            (samples.clone(), slices.clone(), paths)
            });
            window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
                for (path, color) in paths { window.paint_path(path.clone(), *color); }
            });
        });
    }).size_full()
}

/// Cumulative drag displacement with a latched dominant axis. Coordinates are logical pixels.
#[derive(Default)]
pub struct AxisLock {
    previous: [f64; 2],
    output: [f64; 2],
    pending: [f64; 2],
    pub axis: Option<usize>,
}
impl AxisLock {
    pub fn release(&mut self) { self.axis = None; self.pending = [0.; 2]; }
    pub fn drag(&mut self, position: [f64; 2], constrain: bool) -> [f64; 2] {
        let delta = [position[0] - self.previous[0], position[1] - self.previous[1]];
        self.previous = position;
        if !constrain {
            self.release();
            for i in 0..2 { self.output[i] += delta[i]; }
        } else if let Some(axis) = self.axis {
            self.output[axis] += delta[axis];
        } else {
            for i in 0..2 { self.pending[i] += delta[i]; }
            if self.pending[0].abs().max(self.pending[1].abs()) >= 3. {
                let axis = usize::from(self.pending[1].abs() > self.pending[0].abs());
                self.axis = Some(axis);
                self.output[axis] += self.pending[axis];
                self.pending = [0.; 2];
            }
        }
        self.output
    }
}

/// A labeled four-arrow cross describing an XY control's drag directions.
pub fn axis_cross(horizontal: &'static str, vertical: &'static str, locked: Option<usize>, active: Option<[f32; 2]>) -> impl IntoElement {
    axis_cross_tinted(horizontal, vertical, locked, active, crate::live_theme::color(0xbadc91))
}

pub fn axis_cross_tinted(horizontal: &'static str, vertical: &'static str, locked: Option<usize>, active: Option<[f32; 2]>, tint: Hsla) -> impl IntoElement {
    canvas(|_, _, _| (), move |bounds, _, window, cx| {
        window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
            if let Some(value) = active {
                let center = bounds.origin + point(bounds.size.width * value[0], bounds.size.height * (1. - value[1]));
                let mut path = PathBuilder::stroke(px(1.5));
                static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
                let angle = if cx.reduce_motion() { 0. } else { START.get_or_init(std::time::Instant::now).elapsed().as_secs_f32() * 1.8 };
                for (radius, count) in [(4., 1), (11., 8)] {
                    for dash in 0..count {
                        for step in 0..=8 {
                            let t = angle + (dash as f32 + step as f32 / 8. * if count == 1 { 1. } else { 0.55 }) * std::f32::consts::TAU / count as f32;
                            let p = center + point(px(t.cos() * radius), px(t.sin() * radius));
                            if step == 0 { path.move_to(p); } else { path.line_to(p); }
                        }
                    }
                }
                if let Ok(path) = path.build() { window.paint_path(path, tint); }
                if !cx.reduce_motion() { window.request_animation_frame(); }
            }
            let center = bounds.center();
            for (axis, text) in [horizontal, vertical].into_iter().enumerate() {
                let mut color = tint;
                color.a *= if locked == Some(axis) { 0.85 } else { 0.48 };
                let along = if axis == 0 { point(bounds.size.width * 0.42, px(0.)) }
                    else { point(px(0.), bounds.size.height * 0.42) };
                let direction = if axis == 0 { point(px(1.), px(0.)) } else { point(px(0.), px(1.)) };
                let across = point(direction.y, direction.x);
                let mut line = PathBuilder::stroke(px(1.));
                line.move_to(center - along); line.line_to(center + along);
                for sign in [-1., 1.] {
                    let end = center + along * sign;
                    line.move_to(end - direction * (4. * sign) + across * 3.);
                    line.line_to(end);
                    line.line_to(end - direction * (4. * sign) - across * 3.);
                }
                if let Ok(line) = line.build() { window.paint_path(line, color); }
                if let Ok(ink) = crate::oscillator::label_weighted(text, 0., 0., 11., axis == 1, 0, 900.) {
                    let mut path = ink.path;
                    let anchor = if axis == 0 { center + point(bounds.size.width * 0.23, px(10.)) }
                        else { center + point(px(10.), -bounds.size.height * 0.23) };
                    let offset = anchor - path.bounds.center();
                    path.bounds.origin += offset;
                    for vertex in &mut path.vertices { vertex.xy_position += offset; }
                    window.paint_path(path, color);
                }
            }
        });
    }).absolute().top_0().left_0().size_full()
}

#[cfg(test)]
#[test]
fn axis_lock_latches_and_releases_without_jumping() {
    let mut drag = AxisLock::default();
    assert_eq!(drag.drag([5., 1.], true), [5., 0.]);
    assert_eq!(drag.drag([6., 30.], true), [6., 0.]);
    assert_eq!(drag.drag([8., 32.], false), [8., 2.]);
    assert_eq!(drag.drag([9., 38.], true), [8., 8.]);
    assert_eq!(drag.axis, Some(1));
}

/// Full-rect normalized response; the caller supplies the same samples as playback.
pub fn response(samples: std::sync::Arc<[f32]>) -> impl IntoElement {
    response_tinted(samples,crate::live_theme::color(0xbadc91),None)
}
/// Coverage fringe for a polyline. Lyon stroke triangles otherwise have constant
/// shader coordinates, so shallow edges rely solely on the backend's MSAA grid.
pub fn response_stroke(points: &[gpui::Point<gpui::Pixels>], width: f32, scale: f32) -> gpui::Path<gpui::Pixels> {
    let mut points = points.to_vec();
    points.dedup();
    let mut path = gpui::Path::new(points.first().copied().unwrap_or_default());
    if points.len() < 2 { return path; }
    let fringe = 0.5 / scale.max(1.);
    let half = width * 0.5;
    let inner = (half - fringe).max(0.);
    let outer = half + fringe;
    let normals: Vec<_> = points.windows(2).map(|pair| {
        let dx = f32::from(pair[1].x - pair[0].x);
        let dy = f32::from(pair[1].y - pair[0].y);
        let len = dx.hypot(dy).max(f32::EPSILON);
        [-dy / len, dx / len]
    }).collect();
    let joins: Vec<_> = (0..points.len()).map(|i| {
        let a = normals[i.saturating_sub(1)];
        let b = normals[i.min(normals.len() - 1)];
        let denominator = (1. + a[0] * b[0] + a[1] * b[1]).max(0.25);
        [(a[0] + b[0]) / denominator, (a[1] + b[1]) / denominator]
    }).collect();
    let solid = point(0., 1.);
    // s varies normal to the edge; the native quadratic-path shader evaluates
    // its signed pixel distance. Large radius keeps its fringe locally linear.
    let coverage = |distance: f32| point(1. + distance * scale / 32., 1.);
    for i in 0..points.len() - 1 {
        let vertex = |j: usize, distance: f32| points[j] + point(px(joins[j][0] * distance), px(joins[j][1] * distance));
        for (low, high, low_st, high_st) in [
            (-inner, inner, solid, solid),
            (inner, outer, coverage(-fringe), coverage(fringe)),
            (-outer, -inner, coverage(fringe), coverage(-fringe)),
        ] {
            let (a,b,c,d) = (vertex(i, low), vertex(i + 1, low), vertex(i + 1, high), vertex(i, high));
            path.push_triangle((a,b,c), (low_st,low_st,high_st));
            path.push_triangle((a,c,d), (low_st,high_st,high_st));
        }
    }
    path
}

pub fn response_tinted(samples: std::sync::Arc<[f32]>, color: Hsla, baseline: Option<f32>) -> impl IntoElement {
    let points = samples.iter().enumerate().map(|(i, &value)|
        (i as f32 / samples.len().saturating_sub(1).max(1) as f32, value)).collect::<Vec<_>>().into();
    response_points_tinted(points, color, baseline)
}

/// Explicit x coordinates preserve knots and narrow analytic response features.
pub fn response_points_tinted(samples: std::sync::Arc<[(f32, f32)]>, color: Hsla, baseline: Option<f32>) -> impl IntoElement {
    type Key = (usize, [u32; 10]);
    type Mesh = (std::sync::Arc<[(f32, f32)]>, Vec<(gpui::Path<gpui::Pixels>, Hsla)>);
    thread_local! { static MESHES: std::cell::RefCell<std::collections::HashMap<Key, Mesh>> = Default::default(); }
    canvas(|_,_,_| (),move |bounds,_,window,_| {
        if samples.len()<2{return;}
        let key = (samples.as_ptr() as usize, [
            f32::from(bounds.left()).to_bits(), f32::from(bounds.top()).to_bits(),
            f32::from(bounds.size.width).to_bits(), f32::from(bounds.size.height).to_bits(),
            window.scale_factor().to_bits(), color.h.to_bits(), color.s.to_bits(),
            color.l.to_bits(), color.a.to_bits(), baseline.map_or(u32::MAX, f32::to_bits),
        ]);
        MESHES.with(|cache| {
            let mut cache = cache.borrow_mut();
            // ponytail: bounded mesh cache; use LRU only if live graphs churn this set.
            if cache.len() >= 32 && !cache.contains_key(&key) { cache.clear(); }
            let (_, paths) = cache.entry(key).or_insert_with(|| {
                let p=|x,y| bounds.origin+point(bounds.size.width*x,bounds.size.height*y);
                let mut points=Vec::with_capacity(samples.len());let mut area=PathBuilder::fill();
                area.move_to(p(samples[0].0,baseline.unwrap_or(0.5)));
                for &(x,v) in samples.iter(){let q=p(x,0.5-v*0.5);area.line_to(q);points.push(q);}
                area.line_to(p(samples.last().unwrap().0,baseline.unwrap_or(0.5)));area.close();
                let mut paths = Vec::with_capacity(2);
                if baseline.is_some(){if let Ok(path)=area.build(){let mut fill=color;fill.a*=0.10;paths.push((path,fill));}}
                paths.push((response_stroke(&points,1.5,window.scale_factor()),color));
                (samples.clone(), paths)
            });
            window.with_content_mask(Some(gpui::ContentMask{bounds}),|window|{
                for (path, color) in paths { window.paint_path(path.clone(), *color); }
            });
        });
    }).size_full()
}
pub fn control_point(position: [f32;2], bend: bool) -> Div {
    div().absolute().left(gpui::relative(position[0])).top(gpui::relative(1.-position[1]))
        .ml(px(-4.)).mt(px(-4.)).w(px(8.)).h(px(8.)).rounded_full().border_1()
        .border_color(crate::live_theme::color(0xbadc91))
        .bg(crate::live_theme::color(if bend { 0x111111 } else { 0xbadc91 }))
        .cursor(CursorStyle::PointingHand)
}

/// Measure the native plot once; pointer handlers share its scrolled screen bounds.
pub fn plot_bounds() -> (Rc<std::cell::Cell<gpui::Bounds<gpui::Pixels>>>, impl IntoElement) {
    let bounds = Rc::new(std::cell::Cell::new(gpui::Bounds::default()));
    let measured = bounds.clone();
    (bounds, canvas(move |b, _, _| measured.set(b), |_, _, _, _| {})
        .absolute().top_0().left_0().size_full())
}

/// Phosphor glyphs share native text shaping and accessibility with ordinary buttons.
pub fn icon(glyph: &'static str, _label: &'static str) -> Div {
    div().font_family("Phosphor").text_size(px(22.)).line_height(px(24.))
        .child(glyph)
}
pub fn init_icons(cx: &App) {
    let _ = cx.text_system().add_fonts(vec![std::borrow::Cow::Borrowed(include_bytes!("../assets/Phosphor.ttf"))]);
}
/// Two tangent cubic fillets join the slanted shoulder to the horizontal edges.
pub fn shoulder(tint: Hsla, split: f32, degrees: f32) -> impl IntoElement {
    canvas(move |b,_,_| -> Option<Vec<Vec<mui::geometry::Point>>> {
        use mui::geometry::{Point,Polygon,CornerStyle,union,fillet};
        let (w,h)=(f64::from(b.size.width),f64::from(b.size.height));
        let x=w*f64::from(split.clamp(0.,1.));let run=h/f64::from(degrees.clamp(15.,85.)).to_radians().tan();
        let polygon=Polygon::new(vec![Point::new(0.,0.),Point::new(x,0.),Point::new((x-run).max(0.),h),Point::new(w,h),Point::new(w,h+24.),Point::new(0.,h+24.)]);
        let topology=union(&[polygon.into()],Default::default()).ok()?;
        fillet(&topology,CornerStyle {convex_radius:10.,concave_radius:10.,..Default::default()}).ok()?.path.flatten(0.1,10000).ok()
    },move |b,rings,w,_| {
        if let Some(rings)=rings {let mut path=PathBuilder::fill();
            for ring in rings {for (i,p) in ring.iter().enumerate(){let p=b.origin+point(px(p.x as f32),px(p.y as f32));if i==0{path.move_to(p);}else{path.line_to(p);}}path.close();}
            if let Ok(path)=path.build(){w.with_content_mask(Some(gpui::ContentMask{bounds:b}),|w|w.paint_path(path,tint));}
        }
    }).absolute().top_0().left_0().size_full()
}
fn rgb_hsv(rgb: [u8; 3]) -> [f32; 3] {
    let [r,g,b] = rgb.map(|v| v as f32 / 255.);
    let max = r.max(g).max(b); let min = r.min(g).min(b); let d = max-min;
    let h = if d == 0. {0.} else if max == r {((g-b)/d).rem_euclid(6.)} else if max == g {(b-r)/d+2.} else {(r-g)/d+4.};
    [h/6., if max == 0. {0.} else {d/max}, max]
}
fn hsv_rgb([h,s,v]: [f32; 3]) -> [u8; 3] {
    [5.,3.,1.].map(|n| {
        let k = (n+h*6.).rem_euclid(6.);
        (255.*v*(1.-s*(k.min(4.-k)).clamp(0.,1.))).round().clamp(0.,255.) as u8
    })
}

#[derive(Clone, Copy)]
struct PickerState { rgb: [u8; 3], hsv: [f32; 3] }

/// Conventional saturation/value square and hue strip, with keyboard editing.
pub fn color_picker(id: impl Into<gpui::SharedString>, value: [u8;3], change: impl Fn([u8;3], &mut App) + 'static) -> gpui::Stateful<Div> {
    let id = id.into();
    let state = Rc::new(std::cell::RefCell::new(None::<gpui::Entity<PickerState>>));
    let current = state.clone();
    let read = { let state=state.clone(); move |cx: &App| state.borrow().as_ref().map_or(rgb_hsv(value), |s| s.read(cx).hsv) };
    let change = Rc::new(move |hsv, cx: &mut App| {
        let rgb=hsv_rgb(hsv);
        if let Some(state)=state.borrow().as_ref() {state.update(cx, |s,cx| {*s=PickerState{rgb,hsv};cx.notify();});}
        change(rgb,cx);
    });
    let mut root = div().id(id.clone()).relative().flex().flex_col().gap(px(10.))
        .child(canvas(move |_,w,cx| {
            let state=w.use_keyed_state(id.clone(),cx,|_,_|PickerState{rgb:value,hsv:rgb_hsv(value)});
            state.update(cx,|s,_| {if s.rgb!=value {*s=PickerState{rgb:value,hsv:rgb_hsv(value)};}});
            *current.borrow_mut()=Some(state);
        },|_,_,_,_|{}).absolute().size_0());
    for hue in [false,true] {
        let (bounds, marker) = plot_bounds(); let edit = change.clone(); let key = change.clone();
        let drag_read=read.clone(); let key_read=read.clone(); let paint_read=read.clone();
        let update = Rc::new(move |position, cx: &mut App| {
            let [x,y] = plot_position(bounds.get(), position); let mut next = drag_read(cx);
            if hue {next[0]=x;} else {next[1]=x;next[2]=y;}
            edit(next,cx);
        });
        let down = update.clone();
        root = root.child(div().id(if hue {"hue"} else {"saturation-value"}).relative()
            .h(px(if hue {20.} else {150.})).w(px(240.)).tab_index(0).role(Role::Slider)
            .aria_label(if hue {"Hue: Left and Right"} else {"Saturation: Left and Right; brightness: Up and Down"})
            .cursor(CursorStyle::Crosshair).child(marker)
            .child(canvas(|_,_,_| (), move |b,_,w,cx| {
                let hsv=paint_read(cx);
                let rows = if hue {1} else {32}; let cols = 64;
                for y in 0..rows {for x in 0..cols {
                    let sample = if hue {[x as f32/(cols-1) as f32,1.,1.]} else {[hsv[0],x as f32/(cols-1) as f32,1.-y as f32/(rows-1) as f32]};
                    let [r,g,blue] = hsv_rgb(sample);
                    let origin = b.origin+point(b.size.width*x as f32/cols as f32,b.size.height*y as f32/rows as f32);
                    w.paint_quad(gpui::fill(gpui::Bounds::new(origin,gpui::size(b.size.width/cols as f32+px(0.5),b.size.height/rows as f32+px(0.5))),ink(Rgb(r,g,blue))));
                }}
                let p=b.origin+point(b.size.width*if hue {hsv[0]} else {hsv[1]},b.size.height*if hue {0.5} else {1.-hsv[2]});
                w.paint_quad(gpui::outline(gpui::Bounds::new(p-point(px(4.),px(4.)),gpui::size(px(8.),px(8.))),gpui::white(),gpui::BorderStyle::Solid).corner_radii(px(4.)));
            }).size_full())
            .on_mouse_down(gpui::MouseButton::Left,move |e,_,cx| {down(e.position,cx);cx.stop_propagation();})
            .on_mouse_move(move |e,_,cx| {if e.pressed_button == Some(gpui::MouseButton::Left) {update(e.position,cx);}})
            .on_key_down(move |e,_,cx| {
                let mut next=key_read(cx); let step=if e.keystroke.modifiers.shift {0.1} else {0.01};
                match e.keystroke.key.as_str() {
                    "left"=>next[if hue {0} else {1}]-=step, "right"=>next[if hue {0} else {1}]+=step,
                    "up"=>next[if hue {0} else {2}]+=step, "down"=>next[if hue {0} else {2}]-=step, _=>return,
                }
                key(next.map(|v|v.clamp(0.,1.)),cx);cx.stop_propagation();
            }));
    }
    root.child(div().text_size(px(12.)).child(format!("#{:02X}{:02X}{:02X}",value[0],value[1],value[2])))
}
#[cfg(test)]
#[test]
fn hsv_preserves_rgb_and_wraps_hue() {
    for rgb in [[0,0,0],[255,255,255],[255,0,0],[0,255,0],[0,0,255],[73,122,211],[128,128,128]] {
        assert_eq!(hsv_rgb(rgb_hsv(rgb)),rgb);
    }
    assert_eq!(hsv_rgb([1.,1.,1.]),[255,0,0]);
}

/// Fit using shaped glyph advances, so labels stay within their allocated column.
pub fn fitted_label(text: &str, width: f64, size: f32, weight: f32, window: &Window) -> Item {
    let measured = shape(text, (size, weight, Horizontal::Center, false), window)
        .map(|lines| lines.iter().map(|l| f64::from(l.size(px(size*1.5)).width)).fold(0.,f64::max)).unwrap_or(width);
    item("").text(text).typography((size * ((width-12.).max(0.) / measured.max(1.)).min(1.) as f32).max(0.1),weight)
}

/// A full-parent overlay has an explicit origin, independent of preceding siblings.
pub fn overlay(child: impl IntoElement) -> Div { div().absolute().top_0().left_0().size_full().child(child) }

fn sticky_y(origin: f32, scroll: f32, top: f32) -> f32 { origin.max(top - scroll) }
#[cfg(test)]
#[test]
fn sticky_headers_stack_without_moving_future_headers() {
    assert_eq!(sticky_y(10., 0., 0.), 10.);
    assert_eq!(sticky_y(10., -500., 0.), 500.);
    assert_eq!(sticky_y(300., -500., 82.), 582.);
    assert_eq!(sticky_y(900., -500., 164.), 900.);
}

/// Coordinates shared by graph gestures: left/bottom is zero, right/top is one.
pub fn plot_position(bounds: gpui::Bounds<gpui::Pixels>, position: gpui::Point<gpui::Pixels>) -> [f32;2] {
    [(f32::from(position.x-bounds.left())/f32::from(bounds.size.width).max(1.)).clamp(0.,1.),
     (1.-f32::from(position.y-bounds.top())/f32::from(bounds.size.height).max(1.)).clamp(0.,1.)]
}

struct ReorderGhost(SharedString);
impl gpui::Render for ReorderGhost {
    fn render(&mut self,_:&mut Window,_:&mut gpui::Context<Self>)->impl IntoElement {
        div().p(px(8.)).rounded(px(6.)).bg(crate::live_theme::color(0x292929))
            .text_color(crate::live_theme::color(0xbadc91)).child(self.0.clone())
    }
}
/// Reorder behavior for a title, rail, or any other application-owned drag surface.
pub fn reorder_drag<T: Clone + 'static>(payload: T, label: impl Into<SharedString>) -> gpui::Stateful<Div> {
    let label = label.into();
    div().id("reorder-handle").size_full().cursor(CursorStyle::OpenHand).tab_index(0).role(Role::Button)
        .aria_label(format!("Reorder {label}; Alt Up/Down moves one position"))
        .on_drag(payload, move |_, _, _, cx| cx.new(|_| ReorderGhost(label.clone())))
}

/// Shared six-dot reorder handle; the application retains identity and drop policy.
pub fn reorder_handle<T: Clone + 'static>(payload: T, label: impl Into<SharedString>) -> gpui::Stateful<Div> {
    reorder_drag(payload, label).child(canvas(|_, _, _| (), |b, _, w, _| {
        for x in [-3., 3.] { for y in [-6., 0., 6.] {
            let p = b.center() + point(px(x - 1.), px(y - 1.));
            w.paint_quad(gpui::fill(gpui::Bounds::new(p, gpui::size(px(2.), px(2.))), crate::live_theme::color(0xe0e5da)));
        }}
    }).size_full())
}

/// Resolve pinning during prepaint, after native scrolling has updated its offset.
struct Sticky { child: gpui::AnyElement, scroll: gpui::ScrollHandle, origin: f32, stack_top: f32 }
impl IntoElement for Sticky { type Element = Self; fn into_element(self) -> Self { self } }
impl gpui::Element for Sticky {
    type RequestLayoutState = ();
    type PrepaintState = ();
    fn id(&self) -> Option<gpui::ElementId> { None }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> { None }
    fn request_layout(&mut self, _: Option<&gpui::GlobalElementId>, _: Option<&gpui::InspectorElementId>, w: &mut Window, cx: &mut App) -> (gpui::LayoutId, ()) {
        (self.child.request_layout(w, cx), ())
    }
    fn prepaint(&mut self, _: Option<&gpui::GlobalElementId>, _: Option<&gpui::InspectorElementId>, _: gpui::Bounds<gpui::Pixels>, _: &mut (), w: &mut Window, cx: &mut App) {
        let dy = sticky_y(self.origin, self.scroll.offset().y.into(), self.stack_top) - self.origin;
        w.with_element_offset(point(px(0.), px(dy)), |w| { self.child.prepaint(w, cx); });
    }
    fn paint(&mut self, _: Option<&gpui::GlobalElementId>, _: Option<&gpui::InspectorElementId>, _: gpui::Bounds<gpui::Pixels>, _: &mut (), _: &mut (), w: &mut Window, cx: &mut App) {
        self.child.paint(w, cx);
    }
}

/// Native delayed hints and drag labels share the same compact tonal surface.
pub fn tooltip(text: impl Into<SharedString>, cx: &mut App) -> gpui::AnyView {
    cx.new(|_| ReorderGhost(text.into())).into()
}

#[cfg(test)]
#[test]
fn waveform_resampling_does_not_quantize_a_linear_ramp() {
    let samples: Vec<_> = (0..=256).map(|n| n as f32 / 256.).collect();
    for n in 0..=192 {
        let x = n as f32 / 192.;
        assert!((waveform_sample_at(&samples, x) - x).abs() < 1e-6);
    }
    for distance in [-0.5_f32, 0., 0.5] {
        let s = 1. + distance / 32.;
        let shader_distance = (s * s - 1.) / (2. * s / 32.);
        assert!((shader_distance - distance).abs() < 0.005);
    }
    for scale in [1., 1.5, 2.] {
        let points = [point(px(0.),px(0.)),point(px(100.),px(20.))];
        let path = response_stroke(&points, 1.5, scale);
        assert_eq!(path.vertices.len(), 18);
        assert!(path.vertices.iter().all(|v| f32::from(v.xy_position.x).is_finite() && f32::from(v.xy_position.y).is_finite()));
        assert!(path.vertices.iter().any(|v| v.st_position.x < 1. && v.st_position.x > 0.));
        assert!(path.vertices.iter().any(|v| v.st_position.x > 1.));
    }
}
