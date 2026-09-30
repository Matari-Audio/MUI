//! The vector kinds (text, path, duplicator, SVG, Lottie) as kurbo paths in
//! the layer's own pixels around its origin: built, trimmed, deformed, then
//! handed to MUI as one canvas of fills and strokes, so the CPU and GPU
//! backends draw them like any other layer.
use std::f64::consts::{PI, TAU};
use std::sync::Arc;

use mui_scene::prelude::{Color, Draw, Path, Point};
use mui_vello::kurbo::{
    self, Affine, BezPath, ParamCurve, ParamCurveArclen, ParamCurveDeriv, PathEl, PathSeg,
    Point as KPoint, Shape as _,
};
use noise::{NoiseFn, Perlin};
use vello_svg::peniko::{self, Brush};

use crate::{Align, Deform, Drawn, Fx, Layout, Rgba, Shape};

/// Arc length accuracy for trim and path layouts, pixels.
const ACCURACY: f64 = 1e-3;

/// One fill or stroke. Colours are straight sRGB with an extra opacity.
#[derive(Clone, Debug)]
pub struct Piece {
    pub path: BezPath,
    pub fill: Option<(Rgba, f64)>,
    /// Colour, opacity, width.
    pub stroke: Option<(Rgba, f64, f64)>,
}

impl Piece {
    fn transform(mut self, a: Affine) -> Self {
        self.path.apply_affine(a);
        if let Some(s) = &mut self.stroke {
            s.2 *= a.determinant().abs().sqrt();
        }
        self
    }
}

/// The layer's own fill and stroke, at `opacity`.
fn paint(l: &Drawn, fill: Rgba, opacity: f64, path: BezPath) -> Piece {
    Piece {
        path,
        fill: (fill.0[3] > 0).then_some((fill, opacity)),
        stroke: (l.stroke_width > 0. && l.stroke.0[3] > 0).then_some((
            l.stroke,
            opacity,
            l.stroke_width,
        )),
    }
}

/// `d`, or nothing if it does not parse (loading already refused that).
pub fn parse(d: &str) -> BezPath {
    BezPath::from_svg(d).unwrap_or_default()
}

/// A glyph, copy or piece's own place: `pivot` moved by `fx`, turned and
/// scaled about itself.
fn place(pivot: KPoint, fx: &Fx) -> Affine {
    Affine::translate(pivot.to_vec2() + kurbo::Vec2::new(fx.x, fx.y))
        * Affine::rotate(fx.rotation.to_radians())
        * Affine::scale(fx.scale)
        * Affine::translate(-pivot.to_vec2())
}

/// Text as one outline per grapheme cluster, lines split at `\n`, the block
/// centred on the origin. Each cluster takes its first char's [`Fx`].
pub fn text(
    l: &Drawn,
    s: &str,
    align: Align,
    font: &mui_scene::Font,
) -> Result<Vec<Piece>, String> {
    let fonts = std::slice::from_ref(font);
    let axes = [("wght", l.weight as f32)];
    let fs = l.font_size;
    let err = |e: mui_text::Error| format!("layer `{}`: {e:?}", l.id);
    let pitch = l.line_height * fs;
    let lines: Vec<&str> = s.split('\n').collect();
    let height = pitch * lines.len() as f64;
    let mut out = Vec::new();
    let mut ci = 0; // char index, newlines skipped: the index into `l.fx`
    for (k, line) in lines.iter().enumerate() {
        let run = mui_text::shape_run(fonts, line, fs, &axes).map_err(err)?;
        let adv = mui_text::char_advances(fonts, line, fs, &axes).map_err(err)?;
        // First pass: clusters and pen positions, for the line's width.
        let mut clusters = Vec::new();
        let mut pen = 0.;
        let chars: Vec<(usize, char)> = line.char_indices().collect();
        let mut j = 0;
        while j < chars.len() {
            let mut e = j + 1;
            while e < chars.len() && !adv[e].1 {
                e += 1;
            }
            let w: f64 = adv[j..e].iter().map(|a| a.0).sum();
            let end = chars.get(e).map_or(line.len(), |c| c.0);
            let fx = l.fx.get(ci + j);
            clusters.push((&line[chars[j].0..end], pen, w, fx));
            let track = (e - j) as f64 * l.tracking + fx.map_or(0., |f| f.tracking);
            pen += w + track;
            j = e;
        }
        let width = pen;
        let x0 = match align {
            Align::Left => 0.,
            Align::Center => -width / 2.,
            Align::Right => -width,
        };
        let mid = -height / 2. + (k as f64 + 0.5) * pitch;
        let baseline = mid + (run.ascent - run.descent) / 2.;
        for (g, x, w, fx) in clusters {
            if g.trim().is_empty() {
                continue;
            }
            let glyph = mui_text::text_run(fonts, g, fs, &axes, 0.05).map_err(err)?;
            let mut path =
                mui_geometry::bez_path(&glyph.path, 0.05).map_err(|e| format!("{e:?}"))?;
            path.apply_affine(Affine::translate((x0 + x, baseline)));
            let (fill, opacity, a) = match fx {
                Some(fx) => (
                    fx.fill,
                    fx.opacity,
                    place(KPoint::new(x0 + x + w / 2., mid), fx),
                ),
                None => (l.fill, 1., Affine::IDENTITY),
            };
            if opacity > 0. {
                out.push(paint(l, fill, opacity, path).transform(a));
            }
        }
        ci += chars.len();
    }
    Ok(out)
}

/// Where copy `i` of `n` sits and how it turns (degrees).
fn slots(l: &Drawn, layout: Layout, along: &str, orient: bool) -> Vec<(KPoint, f64)> {
    let n = l.count;
    let [sx, sy] = l.spacing;
    let centred = |i: usize, n: usize| i as f64 - (n as f64 - 1.) / 2.;
    match layout {
        Layout::Grid => {
            let cols = l.columns.min(n).max(1);
            let rows = n.div_ceil(cols);
            (0..n)
                .map(|i| {
                    let p = KPoint::new(centred(i % cols, cols) * sx, centred(i / cols, rows) * sy);
                    (p, 0.)
                })
                .collect()
        }
        Layout::Linear => (0..n)
            .map(|i| (KPoint::new(centred(i, n) * sx, 0.), 0.))
            .collect(),
        Layout::Radial => (0..n)
            .map(|i| {
                let a = TAU * i as f64 / n as f64 - PI / 2.;
                let p = (kurbo::Vec2::new(a.cos(), a.sin()) * l.ring_radius).to_point();
                (p, if orient { a.to_degrees() + 90. } else { 0. })
            })
            .collect(),
        Layout::Path => {
            let path = parse(along);
            let segs: Vec<PathSeg> = path.segments().collect();
            let lens: Vec<f64> = segs.iter().map(|s| s.arclen(ACCURACY)).collect();
            let total: f64 = lens.iter().sum();
            if segs.is_empty() || total <= 0. {
                return vec![(KPoint::ZERO, 0.); n];
            }
            let closed = path.elements().last() == Some(&PathEl::ClosePath);
            let step = if closed || n < 2 {
                total / n.max(1) as f64
            } else {
                total / (n - 1) as f64
            };
            (0..n)
                .map(|i| {
                    let mut s = l.path_offset * total + i as f64 * step;
                    s = if closed || l.path_offset != 0. {
                        s.rem_euclid(total)
                    } else {
                        s.min(total)
                    };
                    let (p, d) = at_length(&segs, &lens, s);
                    (p, if orient { d.atan2().to_degrees() } else { 0. })
                })
                .collect()
        }
    }
}

/// The point and direction `s` pixels along `segs`.
fn at_length(segs: &[PathSeg], lens: &[f64], mut s: f64) -> (KPoint, kurbo::Vec2) {
    for (seg, &len) in segs.iter().zip(lens) {
        if s <= len || std::ptr::eq(seg, segs.last().expect("not empty")) {
            let t = seg.inv_arclen(s.min(len), ACCURACY);
            return (seg.eval(t), seg.to_cubic().deriv().eval(t).to_vec2());
        }
        s -= len;
    }
    (KPoint::ZERO, kurbo::Vec2::ZERO)
}

pub fn duplicator(
    l: &Drawn,
    shape: Shape,
    d: &str,
    layout: Layout,
    along: &str,
    orient: bool,
) -> Vec<Piece> {
    let (w, h) = (l.width, l.height);
    let copy = match shape {
        Shape::Rect => {
            kurbo::RoundedRect::new(-w / 2., -h / 2., w / 2., h / 2., l.radius).to_path(0.1)
        }
        Shape::Ellipse => kurbo::Ellipse::new(KPoint::ZERO, (w / 2., h / 2.), 0.).to_path(0.1),
        Shape::Path => parse(d),
    };
    slots(l, layout, along, orient)
        .into_iter()
        .zip(&l.fx)
        .filter(|(_, fx)| fx.opacity > 0. && fx.scale != 0.)
        .map(|((p, turn), fx)| {
            let a = Affine::translate(p.to_vec2() + kurbo::Vec2::new(fx.x, fx.y))
                * Affine::rotate((turn + fx.rotation).to_radians())
                * Affine::scale(fx.scale);
            paint(l, fx.fill, fx.opacity, copy.clone()).transform(a)
        })
        .collect()
}

pub fn path(l: &Drawn, d: &str) -> Vec<Piece> {
    vec![paint(l, l.fill, 1., parse(d))]
}

/// Each contour of each piece cut to `[start, end]` of its length, shifted
/// by `offset` and wrapping. The whole range is a no-op.
pub fn trim(pieces: &mut [Piece], [start, end, offset]: [f64; 3]) {
    let (mut s, mut e) = (start.clamp(0., 1.), end.clamp(0., 1.));
    if s > e {
        std::mem::swap(&mut s, &mut e);
    }
    if s <= 0. && e >= 1. {
        return;
    }
    for p in pieces {
        let mut out = BezPath::new();
        for sub in contours(&p.path) {
            let segs: Vec<PathSeg> = sub.segments().collect();
            let lens: Vec<f64> = segs.iter().map(|g| g.arclen(ACCURACY)).collect();
            let total: f64 = lens.iter().sum();
            if total <= 0. || e - s <= 0. {
                continue;
            }
            let a = (s + offset).rem_euclid(1.) * total;
            let b = a + (e - s) * total;
            // A range past the end wraps round to the start.
            if b > total {
                cut(&segs, &lens, a, total, &mut out);
                cut(&segs, &lens, 0., b - total, &mut out);
            } else {
                cut(&segs, &lens, a, b, &mut out);
            }
        }
        p.path = out;
    }
}

fn contours(p: &BezPath) -> Vec<BezPath> {
    let mut out: Vec<BezPath> = Vec::new();
    for el in p.elements() {
        if matches!(el, PathEl::MoveTo(_)) || out.is_empty() {
            out.push(BezPath::new());
        }
        out.last_mut().expect("pushed").push(*el);
    }
    out
}

/// The part of `segs` from `a` to `b` pixels along, as a new open contour.
fn cut(segs: &[PathSeg], lens: &[f64], a: f64, b: f64, out: &mut BezPath) {
    let mut at = 0.;
    let mut first = true;
    for (seg, &len) in segs.iter().zip(lens) {
        let (lo, hi) = (a.max(at), b.min(at + len));
        if hi > lo && len > 0. {
            let t0 = seg.inv_arclen(lo - at, ACCURACY);
            let t1 = seg.inv_arclen(hi - at, ACCURACY);
            let part = seg.subsegment(t0..t1);
            if first {
                out.move_to(part.start());
                first = false;
            }
            match part {
                PathSeg::Line(l) => out.line_to(l.p1),
                PathSeg::Quad(q) => out.quad_to(q.p1, q.p2),
                PathSeg::Cubic(c) => out.curve_to(c.p1, c.p2, c.p3),
            }
        }
        at += len;
    }
}

/// Every point of every piece through `deforms`, in order, after
/// subdividing into lines no longer than a few pixels.
pub fn deform(pieces: &mut [Piece], deforms: &[Deform]) {
    if deforms.is_empty() {
        return;
    }
    let fields: Vec<Option<Perlin>> = deforms
        .iter()
        .map(|d| match d {
            Deform::Noise { seed, .. } => Some(Perlin::new(*seed)),
            _ => None,
        })
        .collect();
    let map = |mut p: KPoint| {
        for (d, field) in deforms.iter().zip(&fields) {
            p = match *d {
                Deform::Noise {
                    amount,
                    frequency,
                    phase,
                    ..
                } => {
                    let n = field.as_ref().expect("noise has a field");
                    let (x, y) = (p.x * frequency, p.y * frequency);
                    KPoint::new(
                        p.x + amount * n.get([x, y, phase]),
                        p.y + amount * n.get([x + 31.416, y + 17.32, phase]),
                    )
                }
                Deform::Twist { angle, radius } => {
                    let k = (1. - p.to_vec2().hypot() / radius).max(0.);
                    Affine::rotate(angle.to_radians() * k * k) * p
                }
                Deform::Bend { angle, length } => {
                    let phi = angle.to_radians();
                    if phi.abs() < 1e-9 {
                        p
                    } else {
                        let r = length / phi;
                        let a = p.x / r;
                        KPoint::new((r - p.y) * a.sin(), r - (r - p.y) * a.cos())
                    }
                }
                Deform::Wave {
                    amplitude,
                    wavelength,
                    phase,
                } => KPoint::new(
                    p.x,
                    p.y + amplitude * (TAU * (p.x / wavelength - phase)).sin(),
                ),
            };
        }
        p
    };
    for piece in pieces {
        let mut out = BezPath::new();
        let mut last = KPoint::ZERO;
        kurbo::flatten(&piece.path, 0.1, |el| match el {
            PathEl::MoveTo(p) => {
                out.move_to(map(p));
                last = p;
            }
            PathEl::LineTo(p) => {
                // Long straight edges bend too: split them to ~4 px.
                let n = ((p - last).hypot() / 4.).ceil().max(1.) as usize;
                for i in 1..=n {
                    out.line_to(map(last.lerp(p, i as f64 / n as f64)));
                }
                last = p;
            }
            PathEl::ClosePath => out.close_path(),
            _ => {}
        });
        piece.path = out;
    }
}

/// kurbo to MUI path commands (quads raised to cubics).
fn mui_path(p: &BezPath, d: kurbo::Vec2) -> Path {
    let pt = |q: KPoint| Point::new(q.x + d.x, q.y + d.y);
    let mut out = Path::default();
    let mut cur = KPoint::ZERO;
    for el in p.elements() {
        out = match *el {
            PathEl::MoveTo(q) => {
                cur = q;
                out.move_to(pt(q))
            }
            PathEl::LineTo(q) => {
                cur = q;
                out.line_to(pt(q))
            }
            PathEl::QuadTo(a, b) => {
                let c = kurbo::QuadBez::new(cur, a, b).raise();
                cur = b;
                out.cubic_to(pt(c.p1), pt(c.p2), pt(b))
            }
            PathEl::CurveTo(a, b, c) => {
                cur = c;
                out.cubic_to(pt(a), pt(b), pt(c))
            }
            PathEl::ClosePath => out.close(),
        };
    }
    out
}

fn color(c: Rgba, k: f64) -> Color {
    let [r, g, b, a] = c.0.map(|v| f32::from(v) / 255.);
    Color::srgba(r, g, b, a * k.clamp(0., 1.) as f32)
}

/// The pieces as MUI canvas draws, and the box they fill: its top left and
/// size in the layer's pixels. Draws are relative to that corner.
pub fn draws(pieces: &[Piece]) -> (Vec<Draw>, KPoint, (f64, f64)) {
    let bbox = pieces
        .iter()
        .filter(|p| !p.path.elements().is_empty())
        .map(|p| {
            let pad = p.stroke.map_or(0., |s| s.2 / 2.);
            p.path.bounding_box().inflate(pad, pad)
        })
        .reduce(|a, b| a.union(b))
        .unwrap_or_default();
    let d = -bbox.origin().to_vec2();
    let mut out = Vec::with_capacity(pieces.len());
    for p in pieces {
        if p.path.elements().is_empty() {
            continue;
        }
        let path = Arc::new(mui_path(&p.path, d));
        if let Some((c, k)) = p.fill {
            out.push(Draw::fill(path.clone(), color(c, k)));
        }
        if let Some((c, k, w)) = p.stroke {
            out.push(Draw::stroke(path, color(c, k), w));
        }
    }
    (out, bbox.origin(), (bbox.width(), bbox.height()))
}

/// Collects what vello_svg and velato draw, as pieces: transforms applied,
/// solid colours kept, a gradient drawn as its average colour, group
/// opacity multiplied in. Clips, masks, blend modes and images are dropped.
#[derive(Default)]
pub struct Sink {
    pub pieces: Vec<Piece>,
    alpha: Vec<f64>,
}

impl Sink {
    fn alpha(&self) -> f64 {
        self.alpha.iter().product()
    }
    fn push(&mut self, t: Affine, brush: &Brush, shape: &impl kurbo::Shape, stroke: Option<f64>) {
        let Some((c, k)) = solid(brush) else { return };
        let k = k * self.alpha();
        let path = shape.to_path(0.1);
        let piece = match stroke {
            None => Piece {
                path,
                fill: Some((c, k)),
                stroke: None,
            },
            Some(w) => Piece {
                path,
                fill: None,
                stroke: Some((c, k, w)),
            },
        };
        self.pieces.push(piece.transform(t));
    }
}

fn solid(b: &Brush) -> Option<(Rgba, f64)> {
    let rgba = |c: peniko::color::AlphaColor<peniko::color::Srgb>| {
        let c = c.to_rgba8();
        Rgba([c.r, c.g, c.b, c.a])
    };
    match b {
        Brush::Solid(c) => Some((rgba(*c), 1.)),
        Brush::Gradient(g) if !g.stops.is_empty() => {
            let n = g.stops.len() as f32;
            let sum = g.stops.iter().fold([0f32; 4], |acc, s| {
                let c = s.color.to_alpha_color::<peniko::color::Srgb>().components;
                std::array::from_fn(|i| acc[i] + c[i] / n)
            });
            Some((rgba(peniko::color::AlphaColor::new(sum)), 1.))
        }
        _ => None,
    }
}

impl vello_svg::RenderSink for Sink {
    fn push_layer(
        &mut self,
        _blend: impl Into<peniko::BlendMode>,
        alpha: f32,
        _transform: Affine,
        _shape: &impl kurbo::Shape,
    ) {
        self.alpha.push(f64::from(alpha));
    }
    fn pop_layer(&mut self) {
        self.alpha.pop();
    }
    fn fill(
        &mut self,
        _fill: peniko::Fill,
        transform: Affine,
        brush: &Brush,
        _brush_transform: Affine,
        shape: &impl kurbo::Shape,
    ) {
        self.push(transform, brush, shape, None);
    }
    fn stroke(
        &mut self,
        stroke: &kurbo::Stroke,
        transform: Affine,
        brush: &Brush,
        _brush_transform: Affine,
        shape: &impl kurbo::Shape,
    ) {
        self.push(transform, brush, shape, Some(stroke.width));
    }
}

impl velato::RenderSink for Sink {
    fn push_layer(
        &mut self,
        _blend: impl Into<peniko::BlendMode>,
        alpha: f32,
        _transform: Affine,
        _shape: &impl kurbo::Shape,
    ) {
        self.alpha.push(f64::from(alpha));
    }
    fn push_clip_layer(&mut self, _transform: Affine, _shape: &impl kurbo::Shape) {
        self.alpha.push(1.);
    }
    fn pop_layer(&mut self) {
        self.alpha.pop();
    }
    fn draw(
        &mut self,
        stroke: Option<&kurbo::Stroke>,
        transform: Affine,
        brush: &Brush,
        shape: &impl kurbo::Shape,
    ) {
        self.push(transform, brush, shape, stroke.map(|s| s.width));
    }
    fn draw_image(&mut self, _image: &velato::model::ImageAsset, _transform: Affine, _alpha: f64) {}
}

/// An SVG file as pieces centred on the origin. Its text is drawn in Inter.
pub fn svg(bytes: &[u8], font: &[u8]) -> Result<Vec<Piece>, String> {
    let mut opt = vello_svg::usvg::Options::default();
    opt.fontdb_mut().load_font_data(font.to_vec());
    let tree = vello_svg::usvg::Tree::from_data(bytes, &opt).map_err(|e| e.to_string())?;
    let size = tree.size();
    let mut sink = Sink::default();
    vello_svg::append_tree_with_transform(
        &mut sink,
        &tree,
        Affine::translate((
            -f64::from(size.width()) / 2.,
            -f64::from(size.height()) / 2.,
        )),
    );
    Ok(sink.pieces)
}

/// A Lottie composition at `time` seconds, centred on the origin.
pub fn lottie(comp: &velato::Composition, time: f64, looped: bool) -> Vec<Piece> {
    let (start, end) = (comp.frames.start, comp.frames.end);
    let mut f = start + time * comp.frame_rate;
    if end > start {
        f = if looped {
            start + (f - start).rem_euclid(end - start)
        } else {
            f.clamp(start, end - 1e-6)
        };
    }
    let mut sink = Sink::default();
    let centre = Affine::translate((-(comp.width as f64) / 2., -(comp.height as f64) / 2.));
    velato::Renderer::new().append(comp, f, centre, 1., &mut sink);
    sink.pieces
}
