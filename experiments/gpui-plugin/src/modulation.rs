//! Shared routing gestures and pies; an optional owner supplies accepted musical state.
use crate::live_theme::color;
use gpui::{prelude::*, *};
use std::{collections::HashMap, time::Instant};

const PRIMARY_ROLE: u32 = 0xbadc91;
const SECONDARY_ROLE: u32 = 0x2cc2f6;
const TERTIARY_ROLE: u32 = 0xffad7c;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Source {
    Oscillator(usize),
    Modulator(usize),
    Warp(usize),
    Group(usize),
    /// Host performance controllers are first-class modulation sources.
    ModWheel,
    XyX,
    XyY,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Target {
    Parameter(usize, usize),
    GroupParameter(usize),
    ModuleParameter(Source, usize),
    Input(usize),
    /// Auxiliary audio input, keyed by its owning module identity.
    Aux(usize),
    Depth(usize),
}
impl Target {
    fn is_parameter(self) -> bool {
        matches!(
            self,
            Self::Parameter(..) | Self::GroupParameter(..) | Self::ModuleParameter(..)
        )
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct Route {
    pub id: usize,
    pub source: Source,
    pub target: Target,
    pub depth: f32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Anchor {
    Source(Source),
    Target(Target),
}
#[derive(Clone, Copy)]
struct Site {
    anchor: Anchor,
    bounds: Bounds<Pixels>,
    clip: Bounds<Pixels>,
    height: f32,
}
#[derive(Clone, Copy)]
enum Hit {
    Source,
    Target(Target),
    Pie(usize),
}
#[derive(Clone, Copy)]
enum Gesture {
    Source {
        source: Source,
        cursor: Point<Pixels>,
    },
    Depth {
        id: usize,
        start: Point<Pixels>,
        initial: f32,
        created: bool,
    },
}
struct Motion {
    position: Point<Pixels>,
    velocity: Point<Pixels>,
    last: Instant,
}
/// Musical state stays with the owner. IDs must survive reorder and must not be
/// reused for a different route during this controller's lifetime.
pub trait RouteOwner {
    fn snapshot(&mut self) -> Vec<Route>;
    fn legal(&self, source: Source, target: Target) -> bool;
    fn connect(&mut self, source: Source, target: Target) -> Option<usize>;
    fn begin(&mut self, id: usize);
    fn set_depth(&mut self, id: usize, depth: f32);
    fn end(&mut self, id: usize);
    fn remove(&mut self, id: usize);
}

type PieSite = (usize, Anchor, Point<Pixels>, Bounds<Pixels>);

/// One controller per GPUI application; sources and targets register clipped hitboxes.
#[derive(Default)]
pub struct Routing {
    pub routes: Vec<Route>,
    owner: Option<Box<dyn RouteOwner>>,
    pub armed: Option<Source>,
    next: usize,
    overlay: Option<EntityId>,
    probe: Option<(Instant, Vec<f64>)>,
    reduce_motion: bool,
    selected: Option<usize>,
    hover: Option<usize>,
    reveals: HashMap<usize, (f32, Instant)>,
    gesture: Option<Gesture>,
    sites: Vec<Site>,
    hits: Vec<(Bounds<Pixels>, Hit)>,
    cursor: Option<Point<Pixels>>,
    pie_sites: Vec<PieSite>,
    motions: HashMap<(usize, Anchor), Motion>,
    pills: HashMap<(usize, usize, bool), Vec<Path<Pixels>>>,
}
impl Global for Routing {}
impl Routing {
    pub fn new() -> Self {
        let mut routing = Self {
            probe: std::env::var_os("MUI_ROUTING_PROFILE").map(|_| (Instant::now(), Vec::new())),
            ..Self::default()
        };
        if std::env::var_os("MUI_ROUTING_STRESS").is_some() {
            routing.seed_stress();
        }
        routing
    }

    pub fn with_owner(owner: impl RouteOwner + 'static) -> Self {
        let mut routing = Self { owner: Some(Box::new(owner)), ..Self::default() };
        routing.synchronize();
        routing
    }

    /// Recall is authoritative, including deletion during a drag.
    pub fn synchronize(&mut self) -> bool {
        let Some(owner) = self.owner.as_mut() else { return false };
        let routes = owner.snapshot();
        if routes == self.routes { return false; }
        if let Some(Gesture::Depth { id, .. }) = self.gesture
            && !routes.iter().any(|r| r.id == id)
        {
            owner.end(id);
            self.gesture = None;
        }
        self.routes = routes;
        self.selected = self.selected.filter(|id| self.routes.iter().any(|r| r.id == *id));
        self.hover = self.hover.filter(|id| self.routes.iter().any(|r| r.id == *id));
        true
    }

    /// Finish accepted changes on focus loss/close; Escape instead restores them.
    pub fn finish(&mut self) {
        if let Some(Gesture::Depth { id, .. }) = self.gesture.take()
            && let Some(owner) = self.owner.as_mut()
        { owner.end(id); }
        self.armed = None;
    }

    /// Layout changes must not leave invisible drop sites behind.
    pub fn clear_sites(&mut self) {
        self.sites.clear();
        self.motions.clear();
        self.hits.clear();
        self.pie_sites.clear();
    }

    fn begin_depth(&mut self, id: usize) {
        if let Some(owner) = self.owner.as_mut() { owner.begin(id); }
    }

    fn end_depth(&mut self, id: usize) {
        if let Some(owner) = self.owner.as_mut() { owner.end(id); }
    }

    fn set_depth(&mut self, id: usize, depth: f32) {
        if !depth.is_finite() { return; }
        if let Some(owner) = self.owner.as_mut() {
            owner.set_depth(id, depth.clamp(-1., 1.));
            self.synchronize();
        } else if let Some(route) = self.routes.iter_mut().find(|r| r.id == id) {
            route.depth = depth.clamp(-1., 1.);
        }
    }

    fn remove(&mut self, id: usize) {
        if let Some(owner) = self.owner.as_mut() {
            owner.remove(id);
            self.synchronize();
        } else {
            self.routes.retain(|r| r.id != id);
            self.prune();
        }
    }

    fn seed_stress(&mut self) {
        for source in 1..=3 {
            for oscillator in 1..=3 {
                for parameter in 0..11 {
                    self.connect(
                        Source::Oscillator(source),
                        Target::Parameter(oscillator, parameter),
                    );
                }
            }
        }
        for id in self
            .routes
            .iter()
            .take(12)
            .map(|r| r.id)
            .collect::<Vec<_>>()
        {
            self.connect(Source::Group(0), Target::Depth(id));
        }
    }

    /// Reveal only the hovered route's ancestry, pinned while its depth is dragged.
    fn hover_branch(&self) -> (Option<PieSite>, Vec<usize>) {
            let focus = match self.gesture { Some(Gesture::Depth {id,..}) => self.pie_sites.iter().filter(|p|p.0==id).min_by(|a,b|distance(a.2,self.cursor.unwrap_or(a.2)).total_cmp(&distance(b.2,self.cursor.unwrap_or(b.2)))).copied(), _ => None }.or_else(|| self.cursor.and_then(|cursor| self.pie_sites.iter()
                .filter(|p| distance(p.2,cursor)<24. && p.3.contains(&cursor))
                .min_by(|a,b|distance(a.2,cursor).total_cmp(&distance(b.2,cursor))).copied()));
            let mut expanded=Vec::new();
            if let Some(site)=focus {
                let mut id=site.0;
                for _ in 0..=self.routes.len() {
                    if expanded.contains(&id) {break;} expanded.push(id);
                    match self.routes.iter().find(|r|r.id==id).map(|r|r.target) {
                        Some(Target::Depth(parent))=>id=parent, _=>break,
                    }
                }
            }
        (focus, expanded)
    }
    fn port_routes(&self, anchor: Anchor) -> Vec<Route> {
        self.routes.iter().filter(|r| !matches!(r.target, Target::Depth(_)) && match anchor {
            Anchor::Source(source) => r.source == source,
            Anchor::Target(target) => r.target == target,
        }).cloned().collect()
    }
    fn prune(&mut self) {
        loop {
            let before = self.routes.len();
            let ids: Vec<_> = self.routes.iter().map(|r| r.id).collect();
            self.routes
                .retain(|r| !matches!(r.target,Target::Depth(id) if !ids.contains(&id)));
            if self.routes.len() == before {
                break;
            }
        }
    }
    fn register(&mut self, site: Site) {
        if let Some(existing) = self.sites.iter_mut().find(|s| s.anchor == site.anchor) {
            let offset = site.bounds.origin - existing.bounds.origin;
            if offset != point(px(0.), px(0.)) { self.motions.clear(); }
            for (_, anchor, position, clip) in &mut self.pie_sites {
                if *anchor == site.anchor { *position += offset; *clip = site.clip; }
            }
            *existing = site;
        } else {
            self.sites.push(site);
        }
    }
    fn ancestry_allows(&self, source: Source, target: Target) -> bool {
        if let Target::Depth(mut id) = target {
            for _ in 0..=self.routes.len() {
                let Some(route) = self.routes.iter().find(|r| r.id == id) else {
                    return false;
                };
                if route.source == source {
                    return false;
                }
                if let Target::Depth(parent) = route.target {
                    id = parent;
                } else {
                    return true;
                }
            }
            return false;
        }
        !matches!((source,target), (Source::Oscillator(a),Target::Input(b)) if a == b)
    }
    fn legal(&self, source: Source, target: Target) -> bool {
        self.ancestry_allows(source,target) && self.owner.as_ref().is_none_or(|owner| owner.legal(source,target))
    }
    pub fn connect(&mut self, source: Source, target: Target) -> Option<usize> {
        if !self.ancestry_allows(source,target) { return None; }
        if let Some(owner) = self.owner.as_mut() {
            let id = owner.connect(source, target);
            self.synchronize();
            return id.filter(|id| self.routes.iter().any(|route| route.id == *id));
        }
        if !self.legal(source, target) {
            return None;
        }
        if let Some(route) = self
            .routes
            .iter()
            .find(|r| r.source == source && r.target == target)
        {
            return Some(route.id);
        }
        let id = self.next;
        self.next += 1;
        self.routes.push(Route {
            id,
            source,
            target,
            depth: 0.,
        });
        Some(id)
    }
    pub fn remove_oscillator(&mut self, number: usize) {
        self.sites.retain(|s|!matches!(s.anchor,Anchor::Source(Source::Oscillator(n))|Anchor::Target(Target::Input(n))|Anchor::Target(Target::Parameter(n,_)) if n==number));
        self.routes.retain(|r| {
            r.source != Source::Oscillator(number)
                && !matches!(r.target, Target::Input(n)|Target::Parameter(n,_) if n == number)
        });
        self.prune();
        if self.armed == Some(Source::Oscillator(number)) {
            self.armed = None;
        }
        self.gesture = None;
    }
    fn available(&self, target: Target) -> bool {
        self.armed.is_some_and(|s| {
            self.legal(s, target)
                && !self
                    .routes
                    .iter()
                    .any(|r| r.source == s && r.target == target)
        })
    }
    fn target_at(&self, cursor: Point<Pixels>, source: Source) -> Option<Target> {
        if let Some((id, _)) = self
            .pie_sites
            .iter()
            .filter_map(|site| {
                let (id, _, p, clip) = *site;
                let target = Target::Depth(id);
                if !clip.contains(&cursor)
                    || !self.legal(source, target)
                    || self
                        .routes
                        .iter()
                        .any(|r| r.source == source && r.target == target)
                {
                    return None;
                }
                let siblings = self.routes.iter().filter(|r| r.target == target).count();
                let ghost =
                    parent_position(*site, &self.sites, siblings);
                let near = distance(p, cursor);
                let near_ghost = distance(ghost, cursor);
                (near < 34. || near_ghost < 14.).then_some((id, near.min(near_ghost)))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
        {
            return Some(Target::Depth(id));
        }
        self.sites
            .iter()
            .filter_map(|s| {
                let Anchor::Target(t) = s.anchor else {
                    return None;
                };
                if !self.legal(source, t) || !s.clip.contains(&cursor) {
                    return None;
                }
                let b = s.bounds;
                let dx = (f32::from(b.origin.x - cursor.x))
                    .max(0.)
                    .max(f32::from(cursor.x - b.right()));
                let dy = (f32::from(b.origin.y - cursor.y))
                    .max(0.)
                    .max(f32::from(cursor.y - b.bottom()));
                let d = dx.hypot(dy);
                (d < 22.).then_some((d, t))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, t)| t)
    }
    fn hit(&self, p: Point<Pixels>) -> Option<Hit> {
        self.hits
            .iter()
            .rev()
            .find(|(b, _)| b.contains(&p))
            .map(|(_, h)| *h)
    }
    fn cancel(&mut self) {
        if let Some(Gesture::Depth {
            id,
            initial,
            created,
            ..
        }) = self.gesture.take()
        {
            if created {
                self.remove(id);
            } else {
                self.set_depth(id, initial);
            }
            self.end_depth(id);
        }
        self.armed = None;
    }
    fn down(&mut self, e: &MouseDownEvent) -> bool {
        if e.button == MouseButton::Right {
            let active = self.gesture.is_some() || self.armed.is_some();
            self.cancel();
            return active;
        }
        if e.button != MouseButton::Left {
            return false;
        }
        match self.hit(e.position) {
            // GPUI owns source click/drag recognition and focus acquisition.
            Some(Hit::Source) => return false,
            Some(Hit::Target(target)) => {
                let Some(source) = self.armed else {
                    return false;
                };
                let existed = self
                    .routes
                    .iter()
                    .any(|r| r.source == source && r.target == target);
                let Some(id) = self.connect(source, target) else {
                    return false;
                };
                let initial = self.routes.iter().find(|r| r.id == id).unwrap().depth;
                self.begin_depth(id);
                self.gesture = Some(Gesture::Depth {
                    id,
                    start: e.position,
                    initial,
                    created: !existed,
                });
            }
            Some(Hit::Pie(id)) => {
                if e.click_count == 2 {
                    self.remove(id);
                } else if let Some(initial) = self.routes.iter().find(|r| r.id == id).map(|r| r.depth) {
                    self.begin_depth(id);
                    self.gesture = Some(Gesture::Depth {
                        id,
                        start: e.position,
                        initial,
                        created: false,
                    });
                }
            }
            None => {
                self.armed = None;
                return false;
            }
        }
        true
    }
    fn movement(&mut self, e: &MouseMoveEvent) -> bool {
        let previous = self
            .armed
            .and_then(|source| self.cursor.and_then(|p| self.target_at(p, source)));
        self.cursor = Some(e.position);
        let parent_changed = previous
            != self
                .armed
                .and_then(|source| self.target_at(e.position, source));
        match self.gesture.as_mut() {
            Some(Gesture::Source { cursor, .. }) => {
                *cursor = e.position;
                true
            }
            Some(Gesture::Depth {
                id, start, initial, ..
            }) => {
                let id = *id;
                let fine = if e.modifiers.shift { 0.1 } else { 1. };
                let depth = *initial + f32::from(start.y - e.position.y) * 0.01 * fine;
                self.set_depth(id, depth);
                true
            }
            None => {
                let hover = match self.hit(e.position) {
                    Some(Hit::Pie(id)) => Some(id),
                    _ => None,
                };
                if self.hover == hover {
                    parent_changed
                } else {
                    self.hover = hover;
                    true
                }
            }
        }
    }
    fn up(&mut self, p: Point<Pixels>) -> bool {
        self.hover = match self.hit(p) {
            Some(Hit::Pie(id)) => Some(id),
            _ => None,
        };
        match self.gesture.take() {
            Some(Gesture::Source { source, .. }) => {
                if let Some(t) = self.target_at(p, source) {
                    self.connect(source, t);
                }
                self.armed = None;
                true
            }
            Some(Gesture::Depth { id, .. }) => { self.end_depth(id); true },
            None => false,
        }
    }
    fn reveal(&mut self, id: usize, animate: &mut bool) -> f32 {
        let goal = if self.armed.is_some()
            || self.hover == Some(id)
            || matches!(self.gesture,Some(Gesture::Depth{id:active,..}) if active==id)
        {
            1.
        } else {
            0.
        };
        if self.reduce_motion {
            return goal;
        }
        let now = Instant::now();
        let entry = self.reveals.entry(id).or_insert((goal, now));
        let dt = now.duration_since(entry.1).as_secs_f32().min(0.033);
        entry.1 = now;
        if (entry.0 - goal).abs() < 0.01 {
            entry.0 = goal;
        } else {
            entry.0 += (goal - entry.0) * (dt * 20.).min(1.);
            *animate = true;
        }
        entry.0
    }
    fn animate(
        &mut self,
        key: (usize, Anchor),
        destination: Point<Pixels>,
        animate: &mut bool,
    ) -> Point<Pixels> {
        if self.reduce_motion {
            self.motions.remove(&key);
            return destination;
        }
        let now = Instant::now();
        let m = self.motions.entry(key).or_insert(Motion {
            position: destination,
            velocity: point(px(0.), px(0.)),
            last: now,
        });
        let dt = now.duration_since(m.last).as_secs_f32().min(0.033);
        m.last = now;
        if distance(m.position, destination) < 0.15
            && distance(m.velocity, point(px(0.), px(0.))) < 0.3
        {
            m.position = destination;
            m.velocity = point(px(0.), px(0.));
        } else {
            let delta = destination - m.position;
            m.velocity += (delta * 220. - m.velocity * 24.) * dt;
            m.position += m.velocity * dt;
            *animate = true;
        }
        m.position
    }
}
fn distance(a: Point<Pixels>, b: Point<Pixels>) -> f32 {
    f32::from(a.x - b.x).hypot(f32::from(a.y - b.y))
}
fn center(b: Bounds<Pixels>) -> Point<Pixels> {
    b.origin + point(b.size.width / 2., b.size.height / 2.)
}
/// Parameter pies use the marker's full target bounds as their horizontal
/// anchor. The marker is an absolute, full-size child, so no parent padding or
/// transform belongs in this conversion.
fn parameter_pie_position(target: Bounds<Pixels>, offset_x: f32, y: Pixels) -> Point<Pixels> {
    point(center(target).x + px(offset_x), y)
}
fn circle_bounds(p: Point<Pixels>, r: f32) -> Bounds<Pixels> {
    Bounds::new(p - point(px(r), px(r)), size(px(2. * r), px(2. * r)))
}
fn circle(window: &mut Window, p: Point<Pixels>, r: f32, fill_color: u32) {
    window.paint_quad(fill(circle_bounds(p, r), color(fill_color)).corner_radii(px(r)));
}
fn source_role(source: Source) -> u32 {
    match source {
        Source::Modulator(_) => SECONDARY_ROLE,
        Source::Warp(_) => TERTIARY_ROLE,
        Source::Oscillator(_) | Source::Group(_) | Source::ModWheel | Source::XyX | Source::XyY => {
            PRIMARY_ROLE
        }
    }
}
fn ring_role(window: &mut Window, p: Point<Pixels>, r: f32, role: u32) {
    window.paint_quad(
        outline(circle_bounds(p, r), color(role), BorderStyle::Solid).corner_radii(px(r)),
    );
}
fn ring(window: &mut Window, p: Point<Pixels>, r: f32) {
    ring_role(window, p, r, 0x718164);
}
const PIE_BORDER: f32 = 1.5;
fn plus_role(window: &mut Window, p: Point<Pixels>, role: u32) {
    circle(window, p, 9., 0x252c28);
    ring_role(window, p, 9., role);
    window.paint_quad(fill(
        Bounds::new(p - point(px(4.), px(0.6)), size(px(8.), px(1.2))),
        color(role),
    ));
    window.paint_quad(fill(
        Bounds::new(p - point(px(0.6), px(4.)), size(px(1.2), px(8.))),
        color(role),
    ));
}
fn tinted_pie(window: &mut Window, p: Point<Pixels>, depth: f32, source: Source) {
    sized_pie_with_role(window, p, depth, 9., source_role(source));
}
pub fn sized_pie(window: &mut Window, p: Point<Pixels>, depth: f32, radius: f32) {
    sized_pie_with_role(window, p, depth, radius, PRIMARY_ROLE);
}
fn sized_pie_with_role(
    window: &mut Window,
    p: Point<Pixels>,
    depth: f32,
    radius: f32,
    role: u32,
) {
    // A filled rim keeps the border continuous at fractional positions. The
    // old outline sat on the same antialiased edge as the base disc, leaving
    // a hairline gap on parameter pies.
    circle(window, p, radius, role);
    let radius = (radius - PIE_BORDER).max(0.);
    circle(window, p, radius, 0x343f32);
    if depth.abs() >= 0.999 {
        circle(window, p, radius, role);
    } else if depth.abs() > 0.001 {
        use mui::geometry::{Arc, Path as Shape, PathCommand as C, Point as P};
        let center = P::new(f64::from(p.x), f64::from(p.y));
        let radius = f64::from(radius);
        let sweep = f64::from(depth) * std::f64::consts::TAU;
        let start = -std::f64::consts::FRAC_PI_2;
        let path = Shape {
            commands: vec![
                C::MoveTo(center),
                C::LineTo(center + P::new(0., -radius)),
                C::ArcTo(Arc {
                    center,
                    radius,
                    start_angle: start,
                    sweep,
                    to: center + P::new((start + sweep).cos(), (start + sweep).sin()) * radius,
                }),
                C::Close,
            ],
        };
        if let Ok(builder) = crate::oscillator::geometry_builder(&path, false)
            && let Ok(path) = builder.build()
        {
            window.paint_path(path, color(role));
        }
    }
}
/// Place inside the source or target's native semantic element, after sizing it.
pub fn marker(anchor: Anchor, height: f32) -> impl IntoElement {
    canvas(
        move |bounds, window, cx| {
            if cx.has_global::<Routing>() {
                cx.global_mut::<Routing>().register(Site {
                    anchor,
                    bounds,
                    clip: window.content_mask().bounds,
                    height,
                });
            }
        },
        |_, _, _, _| {},
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}
/// Place before an opaque surface's children so its own ports remain interactive.
/// Routes are painted in a global overlay and must honor the same occlusion as native controls.
pub fn occluder() -> impl IntoElement {
    canvas(|bounds,_,cx| {
        if cx.has_global::<Routing>() {
            let r=cx.global_mut::<Routing>();
            r.sites.retain(|site| !bounds.contains(&center(site.bounds)));
            r.pie_sites.retain(|(_,_,p,_)| !bounds.contains(p));
        }
    },|_,_,_,_|{}).absolute().top_0().left_0().size_full()
}
/// Retained overlay: depth and hover changes invalidate this entity, not the editor racks.
pub struct Overlay;
impl Overlay {
    pub fn new(cx: &mut Context<Self>) -> Self {
        cx.global_mut::<Routing>().overlay = Some(cx.entity_id());
        Self
    }
}
impl Render for Overlay {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        overlay()
    }
}
fn redraw(cx: &mut App) {
    if let Some(id) = cx.global::<Routing>().overlay {
        cx.notify(id);
    }
}
pub fn hide_oscillator(cx: &mut App, number: usize) {
    if cx.has_global::<Routing>() {
        cx.global_mut::<Routing>().sites.retain(|s| !matches!(s.anchor,Anchor::Source(Source::Oscillator(n))|Anchor::Target(Target::Input(n))|Anchor::Target(Target::Parameter(n,_)) if n==number));
    }
}
pub fn forget_source(cx: &mut App, source: Source) {
    if cx.has_global::<Routing>() {
        cx.global_mut::<Routing>()
            .sites
            .retain(|s| s.anchor != Anchor::Source(source) && !matches!(s.anchor, Anchor::Target(Target::ModuleParameter(owner, _)) if owner == source));
    }
}
pub fn source(source: Source) -> impl IntoElement {
    port(Anchor::Source(source), 144.)
}
pub fn port(anchor: Anchor, height: f32) -> impl IntoElement {
    let el = div()
        .id(SharedString::from(format!("port-{anchor:?}")))
        .w(px(24.))
        .h(px(24.))
        .cursor_pointer()
        .tab_index(0)
        .role(Role::Button)
        .aria_label(format!(
            "Modulation {anchor:?}; Enter to arm or connect; Alt arrows edit depth"
        ))
        .focus_visible(|s| s.bg(rgba(0xffffff28)).rounded_full())
        .child(marker(anchor, height));
    match anchor {
        Anchor::Source(source) => el
            .on_click(move |_, _, cx| {
                let r = cx.global_mut::<Routing>();
                r.armed = if r.armed == Some(source) {
                    None
                } else {
                    Some(source)
                };
                redraw(cx);
            })
            .on_drag(source, move |source, _, window, cx| {
                let cursor = window.mouse_position();
                cx.global_mut::<Routing>().gesture = Some(Gesture::Source {
                    source: *source,
                    cursor,
                });
                redraw(cx);
                cx.new(|cx| {
                    cx.on_release(|_, cx| {
                        if cx.has_global::<Routing>()
                            && matches!(
                                cx.global::<Routing>().gesture,
                                Some(Gesture::Source { .. })
                            )
                        {
                            let r = cx.global_mut::<Routing>();
                            r.gesture = None;
                            r.armed = None;
                            redraw(cx);
                        }
                    })
                    .detach();
                    CableDrag
                })
            }),
        Anchor::Target(target) => target_actions(el, target),
    }
}
// The retained overlay paints the cable; GPUI's drag entity only owns its lifecycle.
struct CableDrag;
impl Render for CableDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size(px(0.))
    }
}
pub fn drag_move(e: &DragMoveEvent<Source>, _: &mut Window, cx: &mut App) {
    cx.global_mut::<Routing>().movement(&e.event);
    redraw(cx);
}
pub fn drop_source(_: &Source, window: &mut Window, cx: &mut App) {
    cx.global_mut::<Routing>().up(window.mouse_position());
    cx.refresh_windows();
}
#[derive(Clone, Copy)]
enum RouteKey {
    Connect,
    Previous,
    Next,
    Increase,
    Decrease,
    Remove,
}
pub fn target_actions(el: Stateful<Div>, target: Target) -> Stateful<Div> {
    use crate::controls::*;
    el.key_context("MUIParameter MUIRoute")
        .on_action(move |_: &Connect, _, cx| {
            route_action(target, RouteKey::Connect, cx);
        })
        .on_action(move |_: &PreviousRoute, _, cx| {
            route_action(target, RouteKey::Previous, cx);
        })
        .on_action(move |_: &NextRoute, _, cx| {
            route_action(target, RouteKey::Next, cx);
        })
        .on_action(move |_: &IncreaseDepth, _, cx| {
            route_action(target, RouteKey::Increase, cx);
        })
        .on_action(move |_: &DecreaseDepth, _, cx| {
            route_action(target, RouteKey::Decrease, cx);
        })
        .on_action(move |_: &RemoveRoute, _, cx| {
            route_action(target, RouteKey::Remove, cx);
        })
}
fn route_action(target: Target, key: RouteKey, cx: &mut App) {
    if !cx.has_global::<Routing>() {
        cx.propagate();
        return;
    }
    if !cx
        .global::<Routing>()
        .sites
        .iter()
        .any(|s| s.anchor == Anchor::Target(target))
    {
        cx.propagate();
        return;
    }
    if !cx.global_mut::<Routing>().route_key(target, key) {
        cx.propagate();
        return;
    }
    cx.refresh_windows();
}
impl Routing {
    fn route_key(&mut self, target: Target, key: RouteKey) -> bool {
        if matches!(key, RouteKey::Connect) {
            return self
                .armed
                .is_some_and(|source| self.connect(source, target).is_some());
        }
        let ids: Vec<_> = self
            .port_routes(Anchor::Target(target))
            .iter()
            .map(|r| r.id)
            .collect();
        if ids.is_empty() {
            return false;
        }
        let selected = self
            .selected
            .filter(|id| ids.contains(id))
            .unwrap_or(ids[0]);
        let index = ids.iter().position(|id| *id == selected).unwrap();
        match key {
            RouteKey::Previous => self.selected = Some(ids[(index + ids.len() - 1) % ids.len()]),
            RouteKey::Next => self.selected = Some(ids[(index + 1) % ids.len()]),
            RouteKey::Increase | RouteKey::Decrease => {
                self.selected = Some(selected);
                let depth = self.routes.iter().find(|r| r.id == selected).unwrap().depth;
                self.begin_depth(selected);
                self.set_depth(selected, (depth
                    + if matches!(key, RouteKey::Increase) {
                        0.01
                    } else {
                        -0.01
                    })
                .clamp(-1., 1.));
                self.end_depth(selected);
            }
            RouteKey::Remove => {
                self.remove(selected);
            }
            RouteKey::Connect => unreachable!(),
        }
        true
    }
}
pub fn gutters(cx: &App) -> (f32, f32) {
    if !cx.has_global::<Routing>() {
        return (0., 0.);
    }
    let mut inputs = HashMap::<usize, usize>::new();
    let mut outputs = HashMap::<usize, usize>::new();
    for route in &cx.global::<Routing>().routes {
        if let Target::Input(n) = route.target {
            inputs.insert(
                n,
                cx.global::<Routing>()
                    .port_routes(Anchor::Target(Target::Input(n)))
                    .len(),
            );
        }
        if let Source::Oscillator(n) = route.source {
            outputs.insert(
                n,
                cx.global::<Routing>()
                    .port_routes(Anchor::Source(Source::Oscillator(n)))
                    .len(),
            );
        }
    }
    let extra = |map: HashMap<usize, usize>| {
        map.values()
            .map(|n| (n + 1).div_ceil(11).saturating_sub(2) * 24)
            .max()
            .unwrap_or(0) as f32
    };
    (extra(inputs), extra(outputs))
}
/// Rounded union of height-limited columns; the short final column creates the concave shoulder.
fn pill_paths(count: usize, capacity: usize, left: bool) -> Vec<Path<Pixels>> {
    let container =
        crate::pie_container::PieContainer::port(count, capacity as f32 * 24. + 8., left);
    let Ok(Some(path)) = container.well() else {
        return vec![];
    };
    let Ok(fill) = crate::oscillator::Ink::geometry(&path, 0x111111) else {
        return vec![];
    };
    let Ok(border) = crate::oscillator::border_ink(&path) else {
        return vec![];
    };
    vec![fill.path, border.path]
}

pub fn overlay() -> impl IntoElement {
    canvas(
        |_, _, _| (),
        |_, _, window, cx| {
            let reduce_motion=cx.reduce_motion();
            let r = cx.global_mut::<Routing>();
            r.reduce_motion=reduce_motion;
            r.hits.clear();
            let previous_pies=r.pie_sites.clone();
            let sites = r.sites.clone();
            let mut animate = false;
            let active = r.armed.or(match r.gesture {
                Some(Gesture::Source {
                    source,
                    ..
                }) => Some(source),
                _ => None,
            });
            let snap = match r.gesture {
                Some(Gesture::Source {
                    source,
                    cursor,
                    ..
                }) => r.target_at(cursor, source),
                _ => r.armed.and_then(|source|r.cursor.and_then(|p|r.target_at(p,source))),
            };
            let parent_hint=if let Some(Target::Depth(id))=snap {
                previous_pies.iter().filter(|p|p.0==id).min_by(|a,b|distance(a.2,r.cursor.unwrap_or(a.2)).total_cmp(&distance(b.2,r.cursor.unwrap_or(b.2)))).copied()
            }else{None};
            let (focus, expanded) = r.hover_branch();
            let mut reservations=Vec::new();
            for p in previous_pies.iter().filter(|p| expanded.contains(&p.0) && focus.is_some_and(|f|f.1==p.1)) {
                for (i,_) in r.routes.iter().filter(|route|route.target==Target::Depth(p.0)).enumerate() {
                    reservations.push((p.0,parent_position(*p,&sites,i)));
                }
            }
            let ghost=parent_hint.map(|p|parent_position(p,&sites,r.routes.iter().filter(|route|route.target==Target::Depth(p.0)).count()));
            if let Some(p)=parent_hint{reservations.push((p.0,ghost.unwrap()));}
            r.pie_sites.clear();
            for site in &sites {
                let b = site.bounds;
                let c = center(b);
                if !site.clip.contains(&c) {
                    continue;
                }
                let routes: Vec<_> = r
                    .routes
                    .iter()
                    .filter(|route| match site.anchor {
                        Anchor::Source(s) => route.source == s,
                        Anchor::Target(t) => route.target == t,
                    })
                    .cloned()
                    .collect();
                let port = !matches!(site.anchor, Anchor::Target(t) if t.is_parameter());
                let routes=if port {r.port_routes(site.anchor)}else{routes};
                if port {
                    let capacity = crate::pie_container::PieContainer::port(routes.len()+1,site.height,false).capacity();
                    let left = matches!(
                        site.anchor,
                        Anchor::Target(Target::Input(_) | Target::Aux(_))
                            | Anchor::Source(Source::Modulator(_) | Source::ModWheel | Source::XyX | Source::XyY)
                    );
                    let layout = crate::pie_container::PieContainer::port(routes.len() + 1, site.height, left);
                    let mut count = routes.len() + 1;
                    let positions: Vec<_> = routes.iter().enumerate().map(|(slot, route)| {
                        let offset = layout.slot(slot + 1);
                        let p = displace(c + point(px(offset.x as f32), px(offset.y as f32)), route.id, &mut reservations, Some(site));
                        count = count.max(port_index(p, site) + 1);
                        p
                    }).collect();
                    let paths = r
                        .pills
                        .entry((count, capacity, left))
                        .or_insert_with(|| pill_paths(count, capacity, left))
                        .clone();
                    window.with_content_mask(Some(ContentMask { bounds: site.clip }), |window| {
                        for (i, path) in paths.iter().enumerate() {
                            let mut path = path.clone();
                            path.bounds.origin += c;
                            for v in &mut path.vertices {
                                v.xy_position += c;
                            }
                            window
                                .paint_path(path, color(if i == 0 { 0x111111 } else { 0x292929 }));
                        }
                        match site.anchor {
                            Anchor::Source(s) => {
                                let role = source_role(s);
                                plus_role(window, c, role);
                                if active == Some(s) {
                                    ring_role(window, c, 12., role);
                                }
                                r.hits.push((b, Hit::Source));
                            }
                            Anchor::Target(t) => {
                                if let Some(Gesture::Depth{id,created:true,..})=r.gesture
                                    && let Some(route)=routes.iter().find(|route|route.id==id) {tinted_pie(window,c,route.depth,route.source);}
                                else {circle(window, c, 9., 0x171d1a);ring(window, c, 9.);ring(window, c, 5.);}
                                r.hits.push((b, Hit::Target(t)));
                                if snap == Some(t) {
                                    ring_role(
                                        window,
                                        c,
                                        12.,
                                        active.map(source_role).unwrap_or(PRIMARY_ROLE),
                                    );
                                }
                            }
                        }
                        for (slot, route) in routes.iter().enumerate() {
                            if matches!(site.anchor,Anchor::Target(Target::Input(_) | Target::Aux(_))) && matches!(r.gesture,Some(Gesture::Depth{id,created:true,..}) if id==route.id) {continue;}
                            let goal = positions[slot];
                            let p = r.animate((route.id, site.anchor), goal, &mut animate);
                                r.pie_sites.push((route.id,site.anchor,p,site.clip));
                            sized_pie_with_role(window, p, route.depth, if r.hover==Some(route.id){11.}else{9.}, source_role(route.source));
                            if r.selected == Some(route.id) {
                                ring_role(window, p, 11., source_role(route.source));
                            }
                            r.hits.push((
                                circle_bounds(p, 11.).intersect(&site.clip),
                                Hit::Pie(route.id),
                            ));
                        }
                    });
                } else if let Anchor::Target(t) = site.anchor {
                    let available = r.available(t);
                    let consuming = match r.gesture {
                        Some(Gesture::Depth {
                            id, created: true, ..
                        }) if routes.iter().any(|route| route.id == id) => Some(id),
                        _ => None,
                    };
                    let others: Vec<_> = routes
                        .iter()
                        .filter(|route| Some(route.id) != consuming)
                        .collect();
                    window.with_content_mask(Some(ContentMask { bounds: site.clip }), |window| {
                        if active.is_some_and(|s| r.legal(s, t)) {
                            let role=active.map(source_role).unwrap_or(PRIMARY_ROLE);
                            let mut tint=color(role);tint.a=0.13;
                            window.paint_quad(fill(b,tint).corner_radii(px(4.)));
                            window.paint_quad(
                                outline(
                                    b,
                                    color(if snap == Some(t) { role } else { 0x718164 }),
                                    BorderStyle::Solid,
                                )
                                .corner_radii(px(4.)),
                            );
                        }
                        // Number baseline is 8px above box bottom: 24px to the first circle center.
                        let row_y = b.bottom() + px(16.);
                        if available {
                            let p = parameter_pie_position(b, 0., row_y);
                            plus_role(window, p, active.map(source_role).unwrap_or(PRIMARY_ROLE));
                            r.hits.push((
                                circle_bounds(p, 11.).intersect(&site.clip),
                                Hit::Target(t),
                            ));
                        }
                        if let Some(id) = consuming {
                            let route = routes.iter().find(|route| route.id == id).unwrap();
                            let p = r.animate(
                                (id, site.anchor),
                                parameter_pie_position(b, 0., row_y),
                                &mut animate,
                            );
                            r.pie_sites.push((route.id,site.anchor,p,site.clip));
                            let reveal=r.reveal(route.id,&mut animate);
                            let full=if route.depth<0. {-1.}else{1.};
                            sized_pie_with_role(window,p,full+(route.depth-full)*reveal,6.+6.*reveal,source_role(route.source));
                            if r.selected == Some(route.id) {
                                ring_role(window, p, 11., source_role(route.source));
                            }
                        }
                        let columns = (f32::from(b.size.width) / 24.).floor().max(1.) as usize;
                        for (i, route) in others.iter().enumerate() {
                            let row = i / columns;
                            let offset=crate::pie_container::PieContainer::parameters(others.len(),columns,false).slot(i);
                            let x=offset.x as f32;
                            let y = row_y
                                + px((row + usize::from(available || consuming.is_some())) as f32
                                    * 24.);
                            let goal = displace(
                                parameter_pie_position(b, x, y),
                                route.id,
                                &mut reservations,
                                None,
                            );
                            let p = r.animate(
                                (route.id, site.anchor),
                                goal,
                                &mut animate,
                            );
                            r.pie_sites.push((route.id,site.anchor,p,site.clip));
                            let reveal=r.reveal(route.id,&mut animate);
                            let full=if route.depth<0. {-1.}else{1.};
                            sized_pie_with_role(window,p,full+(route.depth-full)*reveal,6.+6.*reveal,source_role(route.source));
                            if r.selected == Some(route.id) {
                                ring_role(window, p, 11., source_role(route.source));
                            }
                            r.hits.push((
                                circle_bounds(p, 11.).intersect(&site.clip),
                                Hit::Pie(route.id),
                            ));
                        }
                    });
                }
            }
            let children:Vec<_>=r.routes.iter().filter(|route|matches!(route.target,Target::Depth(_))).cloned().collect();
            for _ in 0..children.len(){
                let before=r.pie_sites.len();
                let parents=r.pie_sites.clone();
                for route in &children {
                    let Target::Depth(parent)=route.target else{continue;};
                    for site in parents.iter().filter(|site|site.0==parent && expanded.contains(&parent) && focus.is_some_and(|f|f.1==site.1)){
                        if r.pie_sites.iter().any(|site2|site2.0==route.id&&site2.1==site.1){continue;}
                        let sibling=children.iter().filter(|r|r.target==Target::Depth(parent)).position(|r|r.id==route.id).unwrap_or(0);
                        let goal=parent_position(*site,&sites,sibling);
                        let p=r.animate((route.id,site.1),goal,&mut animate);
                        let reveal=r.reveal(route.id,&mut animate);let full=if route.depth<0.{-1.}else{1.};
                        window.with_content_mask(Some(ContentMask{bounds:site.3}),|window|{sized_pie_with_role(window,p,full+(route.depth-full)*reveal,6.+6.*reveal,source_role(route.source));});
                        r.hits.push((circle_bounds(p,12.).intersect(&site.3),Hit::Pie(route.id)));
                        r.pie_sites.push((route.id,site.1,p,site.3));
                    }
                }
                if r.pie_sites.len()==before {break;}
            }
            if let Some(site)=parent_hint {
                let p=ghost.unwrap();
                window.with_content_mask(Some(ContentMask{bounds:site.3}),|window|{
                    circle(window,p,10.,0x343f32);
                    let mut dashed = PathBuilder::stroke(px(2.));
                    for dash in 0..8 { for step in 0..=6 {
                        let a = (dash as f32 + step as f32 / 10.) * std::f32::consts::TAU / 8.;
                        let q = p + point(px(a.cos()*10.),px(a.sin()*10.));
                        if step==0 { dashed.move_to(q); } else { dashed.line_to(q); }
                    }}
                    if let Ok(path)=dashed.build() {window.paint_path(path,color(active.map(source_role).unwrap_or(SECONDARY_ROLE)));}
                });
                r.hits.push((circle_bounds(p,13.).intersect(&site.3),Hit::Target(Target::Depth(site.0))));
            }
            if let Some(Gesture::Source {
                source,
                cursor,
                ..
            }) = r.gesture
                && let Some(site) = sites.iter().find(|s| s.anchor == Anchor::Source(source))
            {
                let start = center(site.bounds);
                let end=if let Some(p)=ghost{p}else{snap.and_then(|t|sites.iter().find(|s|s.anchor==Anchor::Target(t))).map(|s|center(s.bounds)*0.8+cursor*0.2).unwrap_or(cursor)};
                let bend = (f32::from(end.x - start.x).abs() * 0.45).max(48.);
                let mut path = PathBuilder::stroke(px(2.));
                path.move_to(start);
                path.cubic_bezier_to(
                    end,
                    start + point(px(-bend), px(0.)),
                    end + point(px(bend), px(0.)),
                );
                if let Ok(path) = path.build() {
                    window.paint_path(path, color(source_role(source)));
                }
                circle(window, end, 3., source_role(source));
            }
            if let Some(id)=r.hover {
                let ends:Vec<_>=r.pie_sites.iter().filter(|p|p.0==id).copied().collect();
                if let Some(from)=ends.iter().min_by(|a,b|distance(a.2,r.cursor.unwrap_or(a.2)).total_cmp(&distance(b.2,r.cursor.unwrap_or(b.2)))) {
                    for to in ends.iter().filter(|p|p.1!=from.1) {
                        let a=from.2;let b=to.2;
                        let bend=(f32::from(b.x-a.x).abs()*0.4).max(30.);
                        let curve=|t:f32|{let u=1.-t;a*(u*u*u)+(a+point(px(bend),px(0.)))*(3.*u*u*t)+(b-point(px(bend),px(0.)))*(3.*u*t*t)+b*(t*t*t)};
                        let steps=(distance(a,b)/5.).ceil().max(2.) as usize;
                        let mut path=PathBuilder::stroke(px(1.));
                        for i in (0..steps).step_by(2){path.move_to(curve(i as f32/steps as f32));path.line_to(curve((i+1) as f32/steps as f32));}
                        if let Ok(path)=path.build(){
                            let role=r.routes.iter().find(|route|route.id==id).map(|route|source_role(route.source)).unwrap_or(PRIMARY_ROLE);
                            window.paint_path(path,color(role));
                        }
                    }
                }
                if let Some(route)=r.routes.iter().find(|route|route.id==id) && let Some(site)=sites.iter().find(|s|s.anchor==Anchor::Source(route.source)) {ring_role(window,center(site.bounds),13.,source_role(route.source));}
            }
            r.reveals.retain(|id,_|r.routes.iter().any(|route|route.id==*id));
            r.motions
                .retain(|(id, _), _| r.routes.iter().any(|route| route.id == *id));
            let mut profiling=false;
            if let Some((last,samples))=&mut r.probe {
                let now=Instant::now();samples.push(now.duration_since(*last).as_secs_f64()*1000.);*last=now;
                profiling=samples.len()<210;
                if !profiling {
                    let mut samples=samples[30..].to_vec();samples.sort_by(f64::total_cmp);
                    eprintln!("routing refresh cadence (180 frames, no GPU readback): median {:.2} ms, p95 {:.2} ms; {:.1} refreshes/s",samples[90],samples[171],180000./samples.iter().sum::<f64>());
                    #[cfg(feature="profile")]
                    {let stats=window.frame_duration_snapshot();eprintln!("GPUI frame profile: CPU draw median {:.2} ms / p95 {:.2} ms; present interval median {:.2} ms",stats.draw_duration_histogram.value_at_quantile(0.5) as f64/1e6,stats.draw_duration_histogram.value_at_quantile(0.95) as f64/1e6,stats.present_interval_histogram.value_at_quantile(0.5) as f64/1e6);}
                    r.probe=None;
                }
            }
            if animate || profiling {
                window.on_next_frame(|_, cx| redraw(cx));
            }
            window.on_mouse_event(|e:&MouseDownEvent,phase,window,cx| {
                if phase!=DispatchPhase::Capture{return;}
                if e.button==MouseButton::Right && cx.has_active_drag() {
                    cx.stop_active_drag(window);
                    cx.global_mut::<Routing>().cancel();
                    redraw(cx);
                    cx.stop_propagation();
                    return;
                }
                let before=cx.global::<Routing>().routes.len();
                let armed=cx.global::<Routing>().armed;
                let handled=cx.global_mut::<Routing>().down(e);
                if armed!=cx.global::<Routing>().armed {redraw(cx);}
                if handled{
                    cx.stop_propagation();
                    if before!=cx.global::<Routing>().routes.len(){cx.refresh_windows();}else{redraw(cx);}
                }
            });
            window.on_mouse_event(|e:&MouseMoveEvent,phase,_,cx| {
                if phase == DispatchPhase::Capture && !cx.has_active_drag()
                    && cx.global_mut::<Routing>().movement(e)
                {
                    // Hover invalidation observes input; only our gesture owns it.
                    if cx.global::<Routing>().gesture.is_some() {
                        cx.stop_propagation();
                    }
                    redraw(cx);
                }
            });
            window.on_mouse_event(|e:&MouseUpEvent,phase,_,cx| {
                if phase!=DispatchPhase::Capture || e.button!=MouseButton::Left || cx.has_active_drag(){return;}
                let before=cx.global::<Routing>().routes.len();
                if cx.global_mut::<Routing>().up(e.position){cx.stop_propagation();if before!=cx.global::<Routing>().routes.len(){cx.refresh_windows();}else{redraw(cx);}}
            });
        },
    )
    .absolute()
    .size_full()
}
pub fn cancel(cx: &mut App) {
    cx.global_mut::<Routing>().cancel();
    cx.refresh_windows();
}
#[::core::prelude::v1::test]
fn routing_contract() {
    let mut stress = Routing::default();
    stress.seed_stress();
    assert_eq!(stress.routes.len(), 111);
    stress.reduce_motion = true;
    let mut animating = false;
    assert_eq!(
        stress.animate(
            (0, Anchor::Source(Source::Group(0))),
            point(px(20.), px(40.)),
            &mut animating
        ),
        point(px(20.), px(40.))
    );
    assert!(!animating);

    let mut ghosts = Routing::default();
    let target = Target::Parameter(1, 1);
    let root = ghosts.connect(Source::Modulator(0), target).unwrap();
    for index in 1..=3 {
        ghosts
            .connect(Source::Modulator(index), Target::Depth(root))
            .unwrap();
    }
    let site = (
        root,
        Anchor::Target(target),
        point(px(100.), px(100.)),
        Bounds::new(point(px(0.), px(0.)), size(px(500.), px(500.))),
    );
    let ghost = parent_position(site, &ghosts.sites, 3);
    assert!(distance(site.2, ghost) > 34.);
    ghosts.pie_sites.push(site);
    assert_eq!(
        ghosts.target_at(ghost, Source::Modulator(4)),
        Some(Target::Depth(root)),
        "parent ghost remains droppable beyond its root's snap radius"
    );

    let mut r = Routing::default();
    let a = Source::Modulator(0);
    let b = Source::Modulator(1);
    let t = Target::Parameter(1, 1);
    r.armed = Some(a);
    assert!(r.available(t));
    let id = r.connect(a, t).unwrap();
    assert!(!r.available(t));
    assert_eq!(r.routes[0].depth, 0.);
    assert_eq!(r.connect(a, t), Some(id));
    assert_eq!(r.routes.len(), 1);
    r.armed = Some(b);
    assert!(r.available(t));
    r.connect(b, t);
    assert_eq!(r.routes.len(), 2);
    assert!(r.route_key(t, RouteKey::Next));
    assert_eq!(r.selected, Some(r.routes[1].id));
    assert!(r.route_key(t, RouteKey::Previous));
    assert_eq!(r.selected, Some(id));
    assert!(r.route_key(t, RouteKey::Increase));
    assert!(r.route_key(t, RouteKey::Decrease));
    assert_eq!(r.routes[0].depth, 0.);
    assert!(!r.available(t));
    assert!(r.connect(Source::Oscillator(1), Target::Input(1)).is_none());
    for count in [1, 11, 12, 27, 33] {
        assert_eq!(pill_paths(count, 11, false).len(), 2);
    }
    let parent = r.connect(Source::Warp(0), Target::Depth(id)).unwrap();
    assert!(
        r.connect(a, Target::Depth(id)).is_none(),
        "a source cannot parent its own route"
    );
    assert!(
        r.connect(a, Target::Depth(parent)).is_none(),
        "ancestor self-modulation is rejected"
    );
    assert!(r.connect(b, Target::Depth(usize::MAX)).is_none());
    let child = r.connect(Source::Group(0), Target::Depth(parent)).unwrap();
    assert_eq!(r.routes.iter().find(|r| r.id == child).unwrap().depth, 0.);
    assert_eq!(
        r.port_routes(Anchor::Source(a))
            .iter()
            .map(|r| r.id)
            .collect::<Vec<_>>(),
        vec![id]
    );
    assert!(r.port_routes(Anchor::Source(Source::Warp(0))).is_empty(), "parent depth pies have no permanent port slot");
    let slot = point(px(10.), px(20.));
    assert_eq!(
        displace(slot, 2, &mut vec![(1, slot)], None),
        slot + point(px(0.), px(24.))
    );
    r.remove_oscillator(1);
    assert!(
        r.routes.is_empty(),
        "removing a root must remove nested parents"
    );
    let start = point(px(40.), px(40.));
    r.armed = Some(a);
    r.hits = vec![(circle_bounds(start, 12.), Hit::Target(t))];
    assert!(r.down(&MouseDownEvent {
        button: MouseButton::Left,
        position: start,
        click_count: 1,
        ..Default::default()
    }));
    assert!(r.movement(&MouseMoveEvent {
        position: start - point(px(0.), px(35.)),
        pressed_button: Some(MouseButton::Left),
        ..Default::default()
    }));
    assert!((r.routes[0].depth - 0.35).abs() < 0.001);
    r.up(start);
    assert_eq!(r.armed, Some(a), "pie edits must preserve modulation mode");
    let id = r.routes[0].id;
    r.gesture = Some(Gesture::Depth {
        id,
        start,
        initial: 0.35,
        created: false,
    });
    r.routes[0].depth = 0.9;
    r.cancel();
    assert_eq!(r.armed, None);
    assert!((r.routes[0].depth - 0.35).abs() < 0.001);
    let target_bounds = Bounds::new(point(px(100.), px(100.)), size(px(40.), px(40.)));
    r.sites = vec![Site {
        anchor: Anchor::Target(Target::Input(2)),
        bounds: target_bounds,
        clip: Bounds::new(point(px(0.), px(0.)), size(px(500.), px(500.))),
        height: 272.,
    }];
    let end = point(px(95.), px(120.));
    assert_eq!(
        r.target_at(end, b),
        Some(Target::Input(2)),
        "soft snap outside hitbox"
    );
    r.gesture = Some(Gesture::Source {
        source: b,
        cursor: end,
    });
    r.up(end);
    assert_eq!(r.routes.last().unwrap().depth, 0.);
    assert_eq!(r.routes.last().unwrap().target, Target::Input(2));
    assert_eq!(r.armed, None);
    let mut site = r.sites[0];
    site.bounds.origin.x += px(30.);
    r.register(site);
    assert_eq!(r.sites.len(), 1);
    assert_eq!(r.sites[0].bounds, site.bounds);
    r.hover = None;
    let mut animate = false;
    assert_eq!(r.reveal(id, &mut animate), 0.);
    r.hover = Some(id);
    r.reveals.get_mut(&id).unwrap().1 = Instant::now() - std::time::Duration::from_millis(16);
    assert!(r.reveal(id, &mut animate) > 0.);
    assert!(animate);
}

#[::core::prelude::v1::test]
fn parameter_pie_positions_are_centered_and_symmetric() {
    let bounds = Bounds::new(point(px(101.25), px(100.75)), size(px(86.5), px(40.25)));
    let middle = f32::from(center(bounds).x);
    let singleton = parameter_pie_position(bounds, 0., px(156.));
    assert_eq!(singleton.x, center(bounds).x, "a singleton stays on the target center");
    assert_eq!(circle_bounds(singleton, 9.).center(), singleton, "the painted pie is centered");

    // This is the exact width-to-column rule used by the parameter renderer.
    let columns = (f32::from(bounds.size.width) / 24.).floor().max(1.) as usize;
    assert_eq!(columns, 3);
    let layout = crate::pie_container::PieContainer::parameters(5, columns, false);
    let x = |index| f32::from(parameter_pie_position(bounds, layout.slot(index).x as f32, px(156.)).x);
    assert!((x(0) + x(2) - 2. * middle).abs() < f32::EPSILON, "the full row is symmetric");
    assert!((x(3) + x(4) - 2. * middle).abs() < f32::EPSILON, "the partial row is symmetric");
}

#[::core::prelude::v1::test]
fn source_roles_match_palette_slots() {
    assert_eq!(source_role(Source::Oscillator(0)), PRIMARY_ROLE);
    assert_eq!(source_role(Source::Modulator(0)), SECONDARY_ROLE);
    assert_eq!(source_role(Source::Warp(0)), TERTIARY_ROLE);
}

pub fn port_counts(cx: &App, number: usize) -> (usize, usize) {
    if !cx.has_global::<Routing>() {
        return (0, 0);
    }
    let r = cx.global::<Routing>();
    (
        r.port_routes(Anchor::Target(Target::Input(number))).len(),
        r.port_routes(Anchor::Source(Source::Oscillator(number)))
            .len(),
    )
}

fn port_index(p: Point<Pixels>, site: &Site) -> usize {
    let delta = p - center(site.bounds);
    let capacity = crate::pie_container::PieContainer::port(1, site.height, false).capacity();
    (f32::from(delta.x).abs() / 24.).round() as usize * capacity
        + (f32::from(delta.y).max(0.) / 24.).round() as usize
}
fn port_position(p: Point<Pixels>, site: &Site, advance: usize) -> Point<Pixels> {
    let index = port_index(p, site) + advance;
    let left = matches!(
        site.anchor,
        Anchor::Target(Target::Input(_) | Target::Aux(_))
            | Anchor::Source(Source::Modulator(_) | Source::ModWheel | Source::XyX | Source::XyY)
    );
    let offset = crate::pie_container::PieContainer::port(index + 1, site.height, left).slot(index);
    center(site.bounds) + point(px(offset.x as f32), px(offset.y as f32))
}

fn parent_position(
    site: PieSite,
    sites: &[Site],
    sibling: usize,
) -> Point<Pixels> {
    if let Some(port) = sites.iter().find(|s| s.anchor == site.1)
        && !matches!(site.1, Anchor::Target(t) if t.is_parameter()) {
        return port_position(site.2, port, sibling + 1);
    }
    let p = site.2;
    let offset = point(px(sibling as f32 * 24.), px(0.));
    if let Some(target) = sites.iter().find(|target| target.anchor == site.1)
        && matches!(site.1, Anchor::Target(t) if t.is_parameter())
    {
        let middle = center(target.bounds).x;
        if p.x < middle - px(5.) {
            return p - point(px(24.), px(0.)) + offset;
        }
        if p.x > middle + px(5.) {
            return p + point(px(24.), px(0.)) + offset;
        }
    }
    p + point(px(0.), px(24.)) + offset
}

fn displace(
    mut p: Point<Pixels>,
    id: usize,
    reservations: &mut Vec<(usize, Point<Pixels>)>,
    port: Option<&Site>,
) -> Point<Pixels> {
    for _ in 0..=reservations.len() {
        if reservations
            .iter()
            .any(|(parent, slot)| *parent != id && distance(p, *slot) < 21.)
        {
            p = port.map_or(p + point(px(0.), px(24.)), |site| port_position(p, site, 1));
        } else {
            break;
        }
    }
    reservations.push((id, p));
    p
}

pub fn source_count(cx: &App, source: Source) -> usize {
    if cx.has_global::<Routing>() {
        cx.global::<Routing>()
            .port_routes(Anchor::Source(source))
            .len()
    } else {
        0
    }
}

/// Runnable native event/action regression check: oscillator --kurv --check-interactions.
pub(crate) fn check_interactions(
    window: &mut Window,
    group: &Entity<crate::oscillator::OscillatorGroup>,
    cx: &mut App,
) {
    use crate::oscillator::{GroupEvent, OscillatorEvent};
    struct Events(Vec<OscillatorEvent>);
    impl Global for Events {}
    cx.set_global(Events(Vec::new()));
    cx.subscribe(group, |_, event: &GroupEvent, cx| {
        if event.oscillator == 1 {
            cx.global_mut::<Events>().0.push(event.event.clone());
        }
    })
    .detach();
    fn step(n: usize, mut focus: Option<FocusHandle>, window: &mut Window, cx: &mut App) {
        let source = Source::Modulator(0);
        let target = Target::Input(1);
        let position = |anchor| {
            let site = cx
                .global::<Routing>()
                .sites
                .iter()
                .find(|s| s.anchor == anchor)
                .expect("registered route site");
            let p = center(site.bounds);
            assert!(
                site.clip.contains(&p)
                    && Bounds::new(Point::default(), window.viewport_size()).contains(&p),
                "step {n}: {anchor:?} is outside its visible viewport: {p:?}"
            );
            p
        };
        let from = position(Anchor::Source(source));
        let to = if n == 33 || n == 34 {
            position(Anchor::Target(Target::Input(2))) - point(px(0.), px(288.))
        } else {
            position(Anchor::Target(target))
        };
        let move_to = |p, held: bool, window: &mut Window, cx: &mut App| {
            window.dispatch_event(
                PlatformInput::MouseMove(MouseMoveEvent {
                    position: p,
                    pressed_button: held.then_some(MouseButton::Left),
                    ..Default::default()
                }),
                cx,
            );
        };
        let down = |p, window: &mut Window, cx: &mut App| {
            move_to(p, false, window, cx);
            window.dispatch_event(
                PlatformInput::MouseDown(MouseDownEvent {
                    position: p,
                    button: MouseButton::Left,
                    click_count: 1,
                    ..Default::default()
                }),
                cx,
            );
        };
        let up = |p, window: &mut Window, cx: &mut App| {
            window.dispatch_event(
                PlatformInput::MouseUp(MouseUpEvent {
                    position: p,
                    button: MouseButton::Left,
                    click_count: 1,
                    ..Default::default()
                }),
                cx,
            );
        };
        let key = |key: &str, window: &mut Window, cx: &mut App| {
            let keystroke = Keystroke::parse(key).unwrap();
            window.dispatch_event(
                PlatformInput::KeyDown(KeyDownEvent {
                    keystroke: keystroke.clone(),
                    is_held: false,
                    prefer_character_input: false,
                }),
                cx,
            );
            window.dispatch_event(PlatformInput::KeyUp(KeyUpEvent { keystroke }), cx);
        };
        match n {
            0 => down(to, window, cx),
            1 => {
                up(to, window, cx);
                focus = window.focused(cx);
                assert!(focus.is_some(), "native port focus");
            }
            2 => key("tab", window, cx),
            3 => {
                assert_ne!(window.focused(cx), focus, "Tab advances focus");
                key("shift-tab", window, cx);
            }
            4 => {
                assert_eq!(window.focused(cx), focus, "Shift Tab restores focus");
                down(from, window, cx);
            }
            5 => up(from, window, cx),
            6 => {
                assert_eq!(cx.global::<Routing>().armed, Some(source));
                window.focus(focus.as_ref().unwrap(), cx);
            }
            7 => key("enter", window, cx),
            8 => {
                assert_eq!(cx.global::<Routing>().routes.len(), 1, "Enter connects");
                key("alt-up", window, cx);
            }
            9 => {
                assert!((cx.global::<Routing>().routes[0].depth - 0.01).abs() < 0.001);
                key("alt-delete", window, cx);
            }
            10 => {
                assert!(cx.global::<Routing>().routes.is_empty());
                key("escape", window, cx);
            }
            11 => {
                assert_eq!(cx.global::<Routing>().armed, None);
                down(from, window, cx);
            }
            12 => move_to(from + point(px(12.), px(0.)), true, window, cx),
            13 => {
                assert!(cx.has_active_drag(), "GPUI owns the drag");
                move_to(to - point(px(20.), px(0.)), true, window, cx);
            }
            14 => up(to - point(px(20.), px(0.)), window, cx),
            15 => {
                assert!(!cx.has_active_drag());
                let r = cx.global::<Routing>();
                assert_eq!(r.routes.len(), 1, "native drop creates one route");
                assert_eq!(r.routes[0].target, target);
                assert_eq!(r.routes[0].depth, 0.);
                down(from, window, cx);
            }
            16 => {
                // A hover repaint must not swallow GPUI's first drag movement.
                let pie = cx
                    .global::<Routing>()
                    .pie_sites
                    .iter()
                    .find(|p| p.1 == Anchor::Source(source))
                    .expect("source route pie")
                    .2;
                move_to(pie, true, window, cx);
            }
            17 => {
                assert!(
                    cx.has_active_drag(),
                    "hovering a pie must not block native drag start"
                );
                key("escape", window, cx);
            }
            18 => {
                assert!(!cx.has_active_drag(), "Escape clears GPUI drag");
                assert!(cx.global::<Routing>().gesture.is_none());
                up(to, window, cx);
            }
            19 => {
                assert_eq!(
                    cx.global::<Routing>().routes.len(),
                    1,
                    "cancel must not drop"
                );
                down(from, window, cx);
            }
            20 => move_to(from + point(px(12.), px(0.)), true, window, cx),
            21 => move_to(point(px(10.), px(10.)), true, window, cx),
            22 => up(point(px(10.), px(10.)), window, cx),
            23 => {
                assert!(!cx.has_active_drag());
                assert!(cx.global::<Routing>().gesture.is_none());
                assert_eq!(
                    cx.global::<Routing>().routes.len(),
                    1,
                    "empty space is not a drop target"
                );
                key("enter", window, cx);
            }
            24 => {
                assert_eq!(
                    cx.global::<Routing>().armed,
                    Some(source),
                    "keyboard source activation"
                );
                key("enter", window, cx);
            }
            25 => {
                assert_eq!(cx.global::<Routing>().armed, None);
                let p = position(Anchor::Target(Target::Parameter(1, 1)));
                down(p, window, cx);
            }
            26 => up(
                position(Anchor::Target(Target::Parameter(1, 1))),
                window,
                cx,
            ),
            27 => {
                cx.global_mut::<Events>().0.clear();
                key("up", window, cx);
            }
            28 => {
                assert!(
                    matches!(cx.global::<Events>().0.as_slice(), [OscillatorEvent::Begin(1), OscillatorEvent::Value(1, v), OscillatorEvent::End(1)] if (*v-81.).abs()<0.001),
                    "one balanced parameter gesture per action"
                );
                cx.global_mut::<Events>().0.clear();
                key("shift-down", window, cx);
            }
            29 => {
                assert!(
                    matches!(cx.global::<Events>().0.as_slice(), [OscillatorEvent::Begin(1), OscillatorEvent::Value(1, v), OscillatorEvent::End(1)] if (*v-80.).abs()<0.001),
                    "quantized parameters retain their minimum step"
                );
                assert!(
                    cx.global::<Routing>()
                        .sites
                        .iter()
                        .any(|s| s.anchor == Anchor::Target(Target::GroupParameter(4))),
                    "group parameter automatically registers modulation"
                );
                assert!(
                    cx.global::<Routing>()
                        .sites
                        .iter()
                        .any(|s| s.anchor == Anchor::Target(Target::ModuleParameter(source, 0))),
                    "rack parameter automatically registers modulation"
                );
                down(from, window, cx);
            }
            30 => up(from, window, cx),
            31 => {
                assert_eq!(cx.global::<Routing>().armed, Some(source));
                down(to + point(px(37.), px(242.)), window, cx);
            }
            32 => up(to + point(px(37.), px(242.)), window, cx),
            33 => {
                assert!(
                    cx.global::<Events>()
                        .0
                        .iter()
                        .any(|e| matches!(e, OscillatorEvent::Enabled(false))),
                    "one square click switches off, including in modulation mode"
                );
                assert!(
                    !cx.global::<Routing>()
                        .sites
                        .iter()
                        .any(|s| s.anchor == Anchor::Target(target)),
                    "disabled oscillator removes live routing hitboxes"
                );
                down(to + point(px(37.), px(242.)), window, cx);
            }
            34 => up(to + point(px(37.), px(242.)), window, cx),
            35 => {
                assert!(
                    cx.global::<Events>()
                        .0
                        .iter()
                        .any(|e| matches!(e, OscillatorEvent::Enabled(true))),
                    "square switches back on"
                );
                assert!(
                    cx.global::<Routing>()
                        .sites
                        .iter()
                        .any(|s| s.anchor == Anchor::Target(target))
                );
                eprintln!(
                    "PASS: native focus/actions, balanced gestures, typed drag/drop, cancellation, automatic parameter routing, oscillator power off/on"
                );
                cx.quit();
                return;
            }
            _ => unreachable!(),
        }
        window.on_next_frame(move |window, cx| step(n + 1, focus, window, cx));
        window.refresh();
    }
    window.on_next_frame(|window, _| {
        window.on_next_frame(|window, cx| step(0, None, window, cx));
        window.refresh();
    });
}

#[cfg(test)]
mod owner_tests {
    use super::*;
    use core::prelude::v1::test;
    use std::{cell::RefCell, rc::Rc};

    #[derive(Default)]
    struct State {
        routes: Vec<Route>,
        gestures: Vec<(usize, bool)>,
        next: usize,
    }
    struct Owner(Rc<RefCell<State>>);
    impl RouteOwner for Owner {
        fn snapshot(&mut self) -> Vec<Route> { self.0.borrow().routes.clone() }
        fn legal(&self, _: Source, target: Target) -> bool { target != Target::Input(99) }
        fn connect(&mut self, source: Source, target: Target) -> Option<usize> {
            if !self.legal(source, target) { return None; }
            let mut s = self.0.borrow_mut();
            let id = s.next;
            s.next += 1;
            s.routes.push(Route { id, source, target, depth: 0. });
            Some(id)
        }
        fn begin(&mut self, id: usize) { self.0.borrow_mut().gestures.push((id, true)); }
        fn set_depth(&mut self, id: usize, depth: f32) {
            if let Some(r) = self.0.borrow_mut().routes.iter_mut().find(|r| r.id == id) { r.depth = depth; }
        }
        fn end(&mut self, id: usize) { self.0.borrow_mut().gestures.push((id, false)); }
        fn remove(&mut self, id: usize) { self.0.borrow_mut().routes.retain(|r| r.id != id); }
    }
    #[test]
    fn owner_receives_the_existing_pie_gesture_lifecycle() {
        let state = Rc::new(RefCell::new(State::default()));
        let mut r = Routing::with_owner(Owner(state.clone()));
        let target = Target::Parameter(42, 1);
        r.armed = Some(Source::Modulator(8));
        assert!(!r.legal(Source::Modulator(8), Target::Input(99)));
        assert!(r.route_key(target, RouteKey::Connect));
        let id = r.routes[0].id;
        assert!(!r.legal(Source::Modulator(8),Target::Depth(id)), "owner cannot bypass self-parent hover rejection");
        assert!(r.connect(Source::Modulator(8),Target::Depth(id)).is_none());
        r.hits.push((Bounds::new(point(px(0.), px(0.)), size(px(24.), px(24.))), Hit::Pie(id)));
        assert!(r.down(&MouseDownEvent {
            button: MouseButton::Left, position: point(px(10.), px(10.)), click_count: 1, ..Default::default()
        }));
        r.movement(&MouseMoveEvent {
            position: point(px(10.), px(-20.)), pressed_button: Some(MouseButton::Left), ..Default::default()
        });
        assert!((state.borrow().routes[0].depth - 0.3).abs() < 1e-6);
        r.cancel();
        assert_eq!(state.borrow().routes[0].depth, 0.);
        assert_eq!(state.borrow().gestures, [(id, true), (id, false)]);

        r.route_key(target, RouteKey::Increase);
        assert!((state.borrow().routes[0].depth - 0.01).abs() < 1e-6);
        assert_eq!(state.borrow().gestures.len(), 4);

        r.down(&MouseDownEvent {
            button: MouseButton::Left, position: point(px(10.), px(10.)), click_count: 1, ..Default::default()
        });
        state.borrow_mut().routes.clear();
        assert!(r.synchronize());
        assert!(r.gesture.is_none(), "recall/deletion ends the active gesture");
        assert_eq!(state.borrow().gestures.last(), Some(&(id, false)));
        r.finish();
        assert_eq!(state.borrow().gestures.len(), 6, "close does not duplicate end");
    }
}

pub fn port_count(cx: &App, anchor: Anchor) -> usize { cx.global::<Routing>().port_routes(anchor).len() }

#[::core::prelude::v1::test]
fn hover_branch_is_transient_and_displacement_cascades() {
    let mut r = Routing::default();
    let target = Target::Aux(42);
    let root = r.connect(Source::Modulator(0), target).unwrap();
    let parent = r.connect(Source::Modulator(1), Target::Depth(root)).unwrap();
    let clip = Bounds::new(point(px(0.), px(0.)), size(px(500.), px(500.)));
    let p = point(px(100.), px(100.));
    r.pie_sites = vec![(root, Anchor::Target(target), p, clip),
                      (parent, Anchor::Target(target), p + point(px(0.), px(24.)), clip)];
    assert!(r.hover_branch().1.is_empty());
    r.cursor = Some(p);
    assert_eq!(r.hover_branch().1, vec![root]);
    r.cursor = Some(p + point(px(0.), px(24.)));
    assert_eq!(r.hover_branch().1, vec![parent, root]);
    r.cursor = Some(point(px(450.), px(450.)));
    assert!(r.hover_branch().1.is_empty());
    r.gesture = Some(Gesture::Depth { id: parent, start: p, initial: 0., created: false });
    assert_eq!(r.hover_branch().1, vec![parent, root]);
    r.gesture = None;
    assert!(r.hover_branch().1.is_empty());
    assert_eq!(r.port_routes(Anchor::Target(target)).len(), 1);
    assert!(r.port_routes(Anchor::Source(Source::Modulator(1))).is_empty());

    let mut reserved = vec![(root, p)];
    let first = displace(p, 20, &mut reserved, None);
    let next = displace(p + point(px(0.), px(24.)), 21, &mut reserved, None);
    assert_eq!(first, p + point(px(0.), px(24.)));
    assert_eq!(next, p + point(px(0.), px(48.)), "displaced neighbors must not overlap");
    assert_eq!(displace(p, 20, &mut Vec::new(), None), p, "leaving hover restores the original slot");
    let port = Site { anchor: Anchor::Target(target), bounds: circle_bounds(p, 12.), clip, height: 56. };
    let last_row = p + point(px(0.), px(24.));
    let wrapped = displace(last_row, 20, &mut vec![(root, last_row)], Some(&port));
    assert_eq!(wrapped, p - point(px(24.), px(0.)), "port overflow wraps to the next column");
    assert_eq!(port_index(wrapped, &port), 2);
    assert_eq!(parent_position((root, port.anchor, last_row, clip), &[port], 0), wrapped);

}
