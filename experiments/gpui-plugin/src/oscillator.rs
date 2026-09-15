//! A standalone oscillator editor. No KURV shell code or DSP dependency.
use gpui::{prelude::*, *};
use mui::{
    core::{SceneSpec, resolve_scene},
    geometry::{self, CornerStyle, Polygon, fillet, union},
    layout::{Node, Size},
};
use std::fmt::Write;

const FONT: &[u8] = include_bytes!("../assets/Roboto-Variable.ttf");
const ACCENT: u32 = 0xbadc91;
const TEXT: u32 = 0xe0e5da;
const MUTED: u32 = 0x9ba697;
const GAP: f64 = 12.;
const PAD: f64 = 12.;
const HEIGHT: f64 = 288.;
const HEADER_WIDTH: f64 = 54.;
const HEADER_PAD: f64 = 12.;
const HEADER_BUTTON: f64 = 30.;
fn header_centers(top: f64) -> [f64; 3] {
    [
        top + HEADER_PAD + HEADER_BUTTON / 2.,
        top + HEIGHT / 2.,
        top + HEIGHT - HEADER_PAD - HEADER_BUTTON / 2.,
    ]
}

#[derive(Clone, Copy)]
pub struct Parameter {
    pub label: &'static str,
    pub default: f32,
    pub min: f32,
    pub max: f32,
    pub step: f32,
    pub unit: &'static str,
}
pub const PARAMS: [Parameter; 11] = [
    Parameter {
        label: "WAVE",
        default: 0.,
        min: 0.,
        max: 3.,
        step: 1.,
        unit: "",
    },
    Parameter {
        label: "LEVEL",
        default: 80.,
        min: 0.,
        max: 100.,
        step: 1.,
        unit: "%",
    },
    Parameter {
        label: "PITCH",
        default: 0.,
        min: -48.,
        max: 48.,
        step: 1.,
        unit: " st",
    },
    Parameter {
        label: "PAN",
        default: 0.,
        min: -100.,
        max: 100.,
        step: 1.,
        unit: "%",
    },
    Parameter {
        label: "PHASE",
        default: 0.,
        min: 0.,
        max: 360.,
        step: 1.,
        unit: "°",
    },
    Parameter {
        label: "RANDOM",
        default: 0.,
        min: 0.,
        max: 100.,
        step: 1.,
        unit: "%",
    },
    Parameter {
        label: "VOICES",
        default: 7.,
        min: 1.,
        max: 64.,
        step: 1.,
        unit: "",
    },
    Parameter {
        label: "RANGE",
        default: 0.24,
        min: 0.,
        max: 48.,
        step: 0.01,
        unit: " st",
    },
    Parameter {
        label: "JITTER",
        default: 0.,
        min: 0.,
        max: 100.,
        step: 1.,
        unit: "%",
    },
    Parameter {
        label: "RATE",
        default: 0.5,
        min: 0.01,
        max: 20.,
        step: 0.01,
        unit: " Hz",
    },
    Parameter {
        label: "WIDTH",
        default: 100.,
        min: 0.,
        max: 100.,
        step: 1.,
        unit: "%",
    },
];

type SkinCache = (u64, u64, bool, Vec<Ink>);

/// Local editing state; the owning plugin consumes `OscillatorEvent` to bind its parameters.
pub struct Oscillator {
    values: [f32; 11],
    parameters: [Parameter; 11],
    unison_editor: Entity<crate::curve_editor::CurveEditor>,
    unison_curve: mui::curve::Curve,
    unison_content: Option<AnyView>,
    engine_panels: Vec<AnyView>,
    waveform_samples: Option<std::sync::Arc<[f32]>>,
    _unison_subscription: Subscription,
    enabled: bool,
    text_scale: f32,
    skin: std::cell::RefCell<Option<SkinCache>>,
    ports: (usize, usize),
    width: f64,
    number: usize,
    in_group: bool,
    group_enabled: bool,
    removed: bool,
    hover: bool,
    focus: FocusHandle,
    drag: Option<(usize, Point<Pixels>, f32)>,
}
#[derive(Clone, Debug)]
pub enum OscillatorEvent {
    Begin(usize),
    Value(usize, f32),
    End(usize),
    Enabled(bool),
    UnisonCurve(crate::curve_editor::CurveEdit),
    Remove,
}
impl EventEmitter<OscillatorEvent> for Oscillator {}

impl Oscillator {
    pub fn new(cx: &mut Context<Self>) -> Self {
        load_fonts(cx);
        crate::controls::init(cx);
        let unison_curve = mui::curve::Curve::linear();
        let unison_editor = cx.new(|cx| {
            crate::curve_editor::CurveEditor::new(unison_curve.clone(), cx).compact(140.)
        });
        let subscription = cx.subscribe(
            &unison_editor,
            |this, _, event: &crate::curve_editor::CurveEdit, cx| {
                if let crate::curve_editor::CurveEdit::Changed(curve) = event {
                    this.unison_curve = curve.clone();
                    cx.notify();
                }
                cx.emit(OscillatorEvent::UnisonCurve(event.clone()));
            },
        );
        Self {
            unison_editor,
            unison_curve,
            unison_content: None,
            engine_panels: Vec::new(),
            waveform_samples: None,
            _unison_subscription: subscription,
            values: PARAMS.map(|p| p.default),
            parameters: PARAMS,
            enabled: true,
            text_scale: 1.,
            skin: Default::default(),
            ports: (0, 0),
            width: 960.,
            number: 1,
            in_group: false,
            group_enabled: true,
            removed: false,
            hover: false,
            focus: cx.focus_handle(),
            drag: None,
        }
    }
    pub fn values(&self) -> [f32; 11] {
        self.values
    }

    pub fn with_identity(mut self, number: usize) -> Self {
        self.number = number;
        self
    }
    /// Supply uniformly spaced, bipolar source samples from the owning synth.
    pub fn set_waveform_samples(
        &mut self,
        samples: std::sync::Arc<[f32]>,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            (2..=4097).contains(&samples.len()) && samples.iter().all(|v| v.is_finite()),
            "invalid waveform samples"
        );
        if self.waveform_samples.as_deref() != Some(samples.as_ref()) {
            self.waveform_samples = Some(samples);
            cx.notify();
        }
        Ok(())
    }

    /// Supply the actual engine composition; the shell owns only identity and geometry.
    pub fn with_engine_panels(mut self, panels: Vec<AnyView>) -> Self {
        self.engine_panels = panels;
        self.skin.borrow_mut().take();
        self
    }

    /// Embed a synth-owned distribution view in the shared shell.
    pub fn with_unison_content(mut self, content: impl Into<AnyView>) -> Self {
        self.unison_content = Some(content.into());
        self
    }

    /// Supply the synth's ranges and defaults for the eleven oscillator controls.
    pub fn with_parameters(mut self, parameters: [Parameter; 11]) -> anyhow::Result<Self> {
        anyhow::ensure!(
            parameters.iter().all(|p| p.min.is_finite()
                && p.max.is_finite()
                && p.min < p.max
                && p.default.is_finite()
                && (p.min..=p.max).contains(&p.default)
                && p.step.is_finite()
                && p.step > 0.),
            "Invalid oscillator parameter definition"
        );
        self.values = parameters.map(|p| p.default);
        self.parameters = parameters;
        Ok(self)
    }

    /// Synchronize owner values without emitting edits back to the owner.
    /// An active pointer gesture retains its value until the gesture ends.
    pub fn synchronize(
        &mut self,
        values: [f32; 11],
        enabled: bool,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            values.iter().all(|value| value.is_finite()),
            "Non-finite oscillator value"
        );
        let mut changed = self.removed;
        self.removed = false;
        if self.enabled != enabled {
            self.end(cx);
            self.enabled = enabled;
            self.skin.borrow_mut().take();
            changed = true;
        }
        for (index, value) in values.into_iter().enumerate() {
            if self.drag.is_some_and(|(active, _, _)| active == index) {
                continue;
            }
            if self.values[index] != value {
                self.values[index] = value;
                changed = true;
            }
        }
        if changed {
            cx.notify();
        }
        Ok(())
    }

    fn edit(&mut self, index: usize, value: f32, cx: &mut Context<Self>) {
        let p = self.parameters[index];
        let value = quantize(value, p);
        if self.values[index] == value {
            return;
        }
        self.values[index] = value;
        cx.emit(OscillatorEvent::Value(index, self.values[index]));
        cx.notify();
    }
    fn end(&mut self, cx: &mut Context<Self>) {
        if let Some((index, _, _)) = self.drag.take() {
            cx.emit(OscillatorEvent::End(index));
        }
    }
    pub fn benchmark(&mut self) -> anyhow::Result<()> {
        let values = self.values;
        let start = std::time::Instant::now();
        for frame in 0..120 {
            self.values[1] = 40. + (frame % 60) as f32;
            std::hint::black_box(self.drawing_mode(1440., native_text(), false)?);
        }
        self.values = values;
        eprintln!(
            "oscillator drag preparation: {:.3} ms/frame (120 frames)",
            start.elapsed().as_secs_f64() * 1000. / 120.
        );
        Ok(())
    }
    fn drawing(&self, width: f64) -> anyhow::Result<(Vec<Ink>, Vec<Cell>)> {
        self.drawing_mode(width, false, false)
    }
    fn drawing_mode(
        &self,
        width: f64,
        native: bool,
        interactive_unison: bool,
    ) -> anyhow::Result<(Vec<Ink>, Vec<Cell>)> {
        let label = |text: &str, x: f32, y: f32, size: f32, rotated: bool, color: u32| {
            if native && !rotated {
                Ok(Ink {
                    path: gpui::Path::new(point(px(0.), px(0.))),
                    svg: String::new(),
                    color,
                    stroke: false,
                    text: Some((text.to_owned(), x, y, size)),
                })
            } else {
                label(text, x, y, size, rotated, color)
            }
        };
        let panels = panels(width, if self.in_group { 0. } else { 44. })?;
        let top: f64 = if self.in_group { 0. } else { 44. };
        let active = self.enabled && self.group_enabled;
        let key = (width.to_bits(), top.to_bits(), active);
        let mut cached = self.skin.borrow_mut();
        if cached
            .as_ref()
            .is_none_or(|(w, t, a, _)| (*w, *t, *a) != key)
        {
            let mut shapes =
                vec![Polygon::rectangle(56., top, width.max(760.) - 112., HEIGHT)?.into()];
            shapes.extend(port_shapes(width.max(760.), top, self.ports)?);
            let shell = crate::pie_container::rounded(&shapes)?;
            let mut skin = vec![Ink::geometry(&shell, 0x343f32)?];
            for r in panels[1..].iter().filter(|_| self.engine_panels.is_empty()) {
                let shape = Polygon::rectangle(r.0, r.1, r.2, r.3)?;
                let path = fillet(
                    &union(&[shape.into()], Default::default())?,
                    CornerStyle {
                        convex_radius: 12.,
                        ..Default::default()
                    },
                )?
                .path;
                skin.push(Ink::geometry(&path, 0x252c28)?);
            }
            let switch = Polygon::rectangle(
                56. + HEADER_WIDTH / 2. - 10.,
                header_centers(top)[2] - 10.,
                20.,
                20.,
            )?;
            let path = fillet(
                &union(&[switch.into()], Default::default())?,
                CornerStyle {
                    convex_radius: 3.,
                    ..Default::default()
                },
            )?
            .path;
            skin.push(Ink::geometry(&path, if active { ACCENT } else { MUTED })?);
            *cached = Some((key.0, key.1, key.2, skin));
        }
        let mut ink = cached.as_ref().unwrap().3.clone();
        drop(cached);
        let h = panels[0];
        let centers = header_centers(h.1);
        ink.push(centered_label(
            &format!("OSCILLATOR {:02}", self.number),
            h.0 as f32 + HEADER_WIDTH as f32 / 2.,
            centers[1] as f32,
            19. * self.text_scale,
            true,
            TEXT,
        )?);
        if self.hover {
            ink.push(centered_label(
                "×",
                h.0 as f32 + HEADER_WIDTH as f32 / 2.,
                centers[0] as f32,
                22.,
                false,
                MUTED,
            )?);
        }
        let mut cells = Vec::new();
        for (panel_index, title, indices) in [(1, "WAVEFORM", 0..6), (2, "UNISON", 6..11)] {
            let r = panels[panel_index];
            if interactive_unison { continue; }
            ink.push(label(
                title,
                r.0 as f32 + 20.,
                r.1 as f32 + 32.,
                16. * self.text_scale,
                false,
                TEXT,
            )?);
            let chart = (r.0 + 16., r.1 + 54., r.2 - 32., 140.);
            if panel_index == 1 || !interactive_unison {
                ink.push(graph(
                    chart,
                    &self.values,
                    panel_index == 1,
                    &self.unison_curve,
                    self.waveform_samples.as_deref(),
                )?);
            }
            let count = indices.len();
            for (slot, index) in indices.enumerate() {
                let w = (r.2 - 32.) / count as f64;
                let x = r.0 + 16. + slot as f64 * w;
                let y = r.1 + 216.;
                let p = self.parameters[index];
                ink.push(label(
                    p.label,
                    x as f32,
                    y as f32,
                    8.5 * self.text_scale,
                    false,
                    MUTED,
                )?);
                let text = if index == 0 {
                    ["SINE", "TRI", "SAW", "SQUARE"][self.values[0] as usize].into()
                } else if p.step < 1. {
                    format!("{:.2}{}", self.values[index], p.unit)
                } else {
                    format!("{:.0}{}", self.values[index], p.unit)
                };
                ink.push(label(
                    &text,
                    x as f32,
                    y as f32 + 24.,
                    (w as f32 / 4.8).clamp(9., 16.) * self.text_scale,
                    false,
                    if self.enabled && self.group_enabled {
                        ACCENT
                    } else {
                        MUTED
                    },
                )?);
                cells.push(Cell {
                    index,
                    x: x as f32 - 4.,
                    y: y as f32 - 14.,
                    w: w as f32,
                    h: 46.,
                });
            }
        }
        if !self.in_group {
            ink.push(label("LOCAL PREVIEW   ·   DRAG VALUES / SHIFT: FINE / DOUBLE-CLICK: RESET   ·   +/-: TYPE SIZE",56.,357.,10.,false,MUTED)?);
        }
        Ok((ink, cells))
    }
    /// Uses the same paths as the live GPUI canvas, including variable glyph outlines.
    pub fn svg(&self, width: f64) -> anyhow::Result<String> {
        let (ink, _) = self.drawing(width)?;
        let height = if self.in_group { 288 } else { 376 };
        let mut svg = format!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}"><title>MUI oscillator · local parameter preview</title><rect width="100%" height="100%" fill="#171d1a"/>"##
        );
        for i in ink {
            if i.stroke {
                write!(
                    svg,
                    r##"<path d="{}" fill="none" stroke="#{:06x}" stroke-width="1.5"/>"##,
                    i.svg, i.color
                )?;
            } else {
                write!(svg, r##"<path d="{}" fill="#{:06x}"/>"##, i.svg, i.color)?;
            }
        }
        svg.push_str("</svg>");
        Ok(svg)
    }
}
fn quantize(value: f32, p: Parameter) -> f32 {
    if !value.is_finite() {
        return p.default;
    }
    ((value / p.step).round() * p.step).clamp(p.min, p.max)
}
fn width_axis(size: f32) -> f32 {
    (75. + (size - 10.) * 2.5).clamp(75., 100.)
}
fn weight(size: f32) -> f32 {
    (300. + (size - 10.) * 32.).clamp(100., 900.)
}
fn panels(width: f64, top: f64) -> anyhow::Result<Vec<(f64, f64, f64, f64)>> {
    let body = width.max(760.) - 112.;
    let panel = (body - 54. - 2. * PAD - GAP) / 2.;
    let root = Node::row(
        "oscillator",
        [
            Node::leaf("header", Size::new(54., HEIGHT)),
            Node::leaf("waveform", Size::new(panel, HEIGHT - 2. * PAD)),
            Node::leaf("unison", Size::new(panel, HEIGHT - 2. * PAD)),
        ],
    )
    .gap(GAP);
    let scene = resolve_scene(&SceneSpec::new(root).available_width(body))?;
    Ok(["header", "waveform", "unison"]
        .into_iter()
        .map(|id| {
            let f = scene.layout.frame(id).unwrap();
            (f.x + 56., f.y + top, f.size.width, f.size.height)
        })
        .collect())
}

struct Cell {
    index: usize,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}
#[derive(Clone)]
pub(super) struct Ink {
    pub(super) path: gpui::Path<Pixels>,
    svg: String,
    pub(super) color: u32,
    stroke: bool,
    text: Option<(String, f32, f32, f32)>,
}
impl Ink {
    pub(super) fn geometry(path: &geometry::Path, color: u32) -> anyhow::Result<Self> {
        let builder = geometry_builder(path, false)?;
        Ok(Self {
            path: builder.build()?,
            svg: path.to_svg_data()?,
            stroke: false,
            text: None,
            color,
        })
    }
}
// ponytail: these fixed Latin readouts use direct outlines; use the shared shaper for localized text.
struct Outline {
    builder: PathBuilder,
    svg: String,
    x: f32,
    y: f32,
    scale: f32,
    rotated: bool,
}
impl Outline {
    fn p(&self, x: f32, y: f32) -> Point<Pixels> {
        if self.rotated {
            point(px(self.x - y * self.scale), px(self.y - x * self.scale))
        } else {
            point(px(self.x + x * self.scale), px(self.y - y * self.scale))
        }
    }
}
impl ttf_parser::OutlineBuilder for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        let p = self.p(x, y);
        self.builder.move_to(p);
        write!(self.svg, "M{} {}", f32::from(p.x), f32::from(p.y)).unwrap();
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let p = self.p(x, y);
        self.builder.line_to(p);
        write!(self.svg, "L{} {}", f32::from(p.x), f32::from(p.y)).unwrap();
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let c = self.p(x1, y1);
        let p = self.p(x, y);
        self.builder.curve_to(p, c);
        write!(
            self.svg,
            "Q{} {} {} {}",
            f32::from(c.x),
            f32::from(c.y),
            f32::from(p.x),
            f32::from(p.y)
        )
        .unwrap();
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let a = self.p(x1, y1);
        let b = self.p(x2, y2);
        let p = self.p(x, y);
        self.builder.cubic_bezier_to(p, a, b);
        write!(
            self.svg,
            "C{} {} {} {} {} {}",
            f32::from(a.x),
            f32::from(a.y),
            f32::from(b.x),
            f32::from(b.y),
            f32::from(p.x),
            f32::from(p.y)
        )
        .unwrap();
    }
    fn close(&mut self) {
        self.builder.close();
        self.svg.push('Z');
    }
}
pub(super) fn label(
    text: &str,
    x: f32,
    y: f32,
    size: f32,
    rotated: bool,
    color: u32,
) -> anyhow::Result<Ink> {
    label_weighted(text, x, y, size, rotated, color, weight(size))
}
pub(super) fn label_weighted(text: &str, x: f32, y: f32, size: f32, rotated: bool, color: u32, weight: f32) -> anyhow::Result<Ink> {
    type Key = (String, u32, u32, u32, bool, u32, u32);
    thread_local! {static CACHE:std::cell::RefCell<std::collections::HashMap<Key,Ink>>=Default::default();}
    let key = (
        text.to_owned(),
        x.to_bits(),
        y.to_bits(),
        size.to_bits(),
        rotated,
        color,
        weight.to_bits(),
    );
    if let Some(ink) = CACHE.with(|cache| cache.borrow().get(&key).cloned()) {
        return Ok(ink);
    }
    let mut face = ttf_parser::Face::parse(FONT, 0)?;
    face.set_variation(ttf_parser::Tag::from_bytes(b"wght"), weight)
        .ok_or_else(|| anyhow::anyhow!("variable weight axis missing"))?;
    face.set_variation(ttf_parser::Tag::from_bytes(b"wdth"), width_axis(size))
        .ok_or_else(|| anyhow::anyhow!("variable width axis missing"))?;
    let mut out = Outline {
        builder: PathBuilder::fill().with_style(PathStyle::Fill(
            FillOptions::default()
                .with_fill_rule(FillRule::NonZero)
                .with_tolerance(0.05),
        )),
        svg: String::new(),
        x,
        y,
        scale: size / face.units_per_em() as f32,
        rotated,
    };
    for c in text.chars() {
        let id = face
            .glyph_index(c)
            .ok_or_else(|| anyhow::anyhow!("missing glyph {c}"))?;
        face.outline_glyph(id, &mut out);
        let advance = face.glyph_hor_advance(id).unwrap_or(0) as f32 * out.scale;
        if rotated {
            out.y -= advance;
        } else {
            out.x += advance;
        }
    }
    let ink = Ink {
        path: out.builder.build()?,
        svg: out.svg,
        stroke: false,
        text: None,
        color,
    };
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        // ponytail: bounded cache reset; use LRU if frequent resizing causes churn.
        if cache.len() >= 1024 {
            cache.clear();
        }
        cache.insert(key, ink.clone());
    });
    Ok(ink)
}
fn centered_label(
    text: &str,
    x: f32,
    y: f32,
    size: f32,
    rotated: bool,
    color: u32,
) -> anyhow::Result<Ink> {
    let measured = label(text, 0., 0., size, rotated, color)?;
    let b = measured.path.bounds;
    label(
        text,
        x - f32::from(b.origin.x + b.size.width / 2.),
        y - f32::from(b.origin.y + b.size.height / 2.),
        size,
        rotated,
        color,
    )
}
fn graph(
    r: (f64, f64, f64, f64),
    v: &[f32; 11],
    wave: bool,
    curve: &mui::curve::Curve,
    samples: Option<&[f32]>,
) -> anyhow::Result<Ink> {
    // A deterministic shape preview, not KURV's DSP evaluator.
    let mut b = PathBuilder::stroke(px(1.5));
    let mut svg = String::new();
    if wave {
        let mut segment = |x: f64, y: f64, first: bool| {
            let p = point(px(x as f32), px(y as f32));
            if first {
                b.move_to(p);
            } else {
                b.line_to(p);
            }
            write!(svg, "{}{} {}", if first { 'M' } else { 'L' }, x, y).unwrap();
        };
        if let Some(samples) = samples {
            for (i, sample) in samples.iter().enumerate() {
                segment(
                    r.0 + i as f64 / (samples.len() - 1) as f64 * r.2,
                    r.1 + r.3 / 2. - f64::from(*sample) * r.3 * 0.42,
                    i == 0,
                );
            }
        } else {
            for i in 0..=240 {
                let t = i as f64 / 240.;
                let phase = (t * 2. + v[4] as f64 / 360.).fract();
                let a = match v[0] as usize {
                    0 => (phase * std::f64::consts::TAU).sin(),
                    1 => 1. - 4. * (phase - 0.5).abs(),
                    2 => 2. * phase - 1.,
                    _ => {
                        if phase < 0.5 {
                            1.
                        } else {
                            -1.
                        }
                    }
                };
                segment(
                    r.0 + t * r.2,
                    r.1 + r.3 / 2. - a * r.3 * 0.42 * v[1] as f64 / 100.,
                    i == 0,
                );
            }
        }
    } else {
        // Exact cubics, with the same plot inset as the embedded editor.
        let pos = |p: mui::curve::CurvePoint| {
            point(
                px((r.0 + 10. + p.phase as f64 * (r.2 - 20.)) as f32),
                px((r.1 + 10. + (1. - p.value) as f64 * (r.3 - 20.)) as f32),
            )
        };
        let first = pos(curve.points()[0]);
        b.move_to(first);
        write!(svg, "M{} {}", f32::from(first.x), f32::from(first.y)).unwrap();
        for i in 0..curve.handles().len() {
            let cubic = curve.segment(i).unwrap();
            let a = pos(cubic.handles.outgoing);
            let c = pos(cubic.handles.incoming);
            let end = pos(cubic.end);
            b.cubic_bezier_to(end, a, c);
            write!(
                svg,
                "C{} {} {} {} {} {}",
                f32::from(a.x),
                f32::from(a.y),
                f32::from(c.x),
                f32::from(c.y),
                f32::from(end.x),
                f32::from(end.y)
            )
            .unwrap();
        }
        let count = (v[6] as usize).min(64);
        let mut values = [0.; 64];
        curve.sample_into(&mut values[..count]);
        for (i, value) in values[..count].iter().enumerate() {
            let phase = if count == 1 {
                0.5
            } else {
                i as f32 / (count - 1) as f32
            };
            let from = pos(mui::curve::CurvePoint { phase, value: 0.5 });
            let to = pos(mui::curve::CurvePoint {
                phase,
                value: *value,
            });
            b.move_to(from);
            b.line_to(to);
            write!(
                svg,
                "M{} {}L{} {}",
                f32::from(from.x),
                f32::from(from.y),
                f32::from(to.x),
                f32::from(to.y)
            )
            .unwrap();
        }
    }
    // SVG keeps the graph stroke; GPUI tessellates the same stroke.
    Ok(Ink {
        path: b.build()?,
        svg,
        stroke: true,
        text: None,
        color: ACCENT,
    })
}
impl Render for Oscillator {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let root = div()
            .id("oscillator")
            .size_full()
            .min_w(px(760.))
            .min_h(px(if self.in_group { 288. } else { 376. }))
            .relative()
            .tab_group()
            .bg(crate::live_theme::color(0x171d1a))
            .track_focus(&self.focus);
        if self.removed {
            return root
                .child(
                    div()
                        .p_8()
                        .text_color(crate::live_theme::color(MUTED))
                        .child("Oscillator removed"),
                )
                .into_any_element();
        }
        let ports = crate::modulation::port_counts(cx, self.number);
        if ports != self.ports {
            self.ports = ports;
            self.skin.borrow_mut().take();
        }
        let width = self.width;
        let (ink, _cells) = match self.drawing_mode(width, native_text(), true) {
            Ok(v) => v,
            Err(e) => return root.child(e.to_string()).into_any_element(),
        };
        let header = panels(width, if self.in_group { 0. } else { 44. })
            .expect("drawing validated layout")[0];
        let target = cx.entity().downgrade();
        let painter = canvas(
            move |bounds, window, cx| {
                let width = f64::from(bounds.size.width);
                let changed = target
                    .upgrade()
                    .is_some_and(|entity| (entity.read(cx).width - width).abs() > 0.5);
                if changed {
                    window.on_next_frame(move |_, cx| {
                        let _ = target.update(cx, |this, cx| {
                            this.width = width;
                            cx.notify();
                        });
                    });
                }
            },
            move |bounds, _, window, cx| {
                for mut i in ink {
                    if let Some((text, x, y, font_size)) = i.text {
                        paint_text(
                            &text,
                            bounds.origin + point(px(x), px(y - font_size)),
                            font_size,
                            i.color,
                            window,
                            cx,
                        );
                        continue;
                    }
                    i.path.bounds.origin += bounds.origin;
                    for vertex in &mut i.path.vertices {
                        vertex.xy_position += bounds.origin;
                    }
                    window.paint_path(i.path, crate::live_theme::color(i.color));
                }
            },
        )
        .absolute()
        .size_full();
        let mut root = root
            .child(painter)
            .map(crate::controls::navigation)
            .key_context("MUI MUIOscillator")
            .on_action(cx.listener(|this, _: &crate::controls::LargerType, _, cx| {
                this.text_scale = (this.text_scale + 0.05).min(1.2);
                cx.notify();
            }))
            .on_action(
                cx.listener(|this, _: &crate::controls::SmallerType, _, cx| {
                    this.text_scale = (this.text_scale - 0.05).max(0.85);
                    cx.notify();
                }),
            )
            .on_action(cx.listener(|this, _: &crate::controls::Cancel, _, cx| {
                if let Some((i, _, value)) = this.drag {
                    this.edit(i, value, cx);
                    this.end(cx);
                } else {
                    cx.propagate();
                }
            }))
            .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, _, cx| {
                if let Some((i, start, value)) = this.drag {
                    let delta =
                        f32::from(start.y - e.position.y) + f32::from(e.position.x - start.x);
                    let fine = if e.modifiers.shift { 0.1 } else { 1. };
                    this.edit(i, value + delta * this.parameters[i].step * fine, cx);
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.end(cx)),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.end(cx)),
            );
        if cx.has_global::<crate::modulation::Routing>() && self.enabled && self.group_enabled {
            use crate::modulation::{Anchor, Source, Target, port};
            let top = if self.in_group { 0. } else { 44. };
            for (anchor, x) in [
                (Anchor::Target(Target::Input(self.number)), 34.),
                (
                    Anchor::Source(Source::Oscillator(self.number)),
                    width as f32 - 58.,
                ),
            ] {
                root = root.child(
                    div()
                        .absolute()
                        .left(px(x))
                        .top(px(top + 7.))
                        .w(px(24.))
                        .h(px(24.))
                        .child(port(anchor, HEIGHT as f32 - 16.)),
                );
            }
        }
        let layout = panels(width, if self.in_group { 0. } else { 44. }).expect("valid layout");
        self.unison_editor.update(cx, |editor, cx| {
            editor.set_enabled(self.enabled && self.group_enabled, cx);
            editor.set_samples(self.values[6] as usize, cx);
        });
        for (panel, title, indices) in [(1, "WAVEFORM", 0..6), (2, "UNISON", 6..11)].into_iter().filter(|_| self.engine_panels.is_empty()) {
            let r = layout[panel];
            let mut controls = Vec::new();
            for i in indices {
                let p = self.parameters[i];
                let value = if i == 0 {
                    ["SINE", "TRI", "SAW", "SQUARE"][self.values[0].clamp(0., 3.) as usize].to_string()
                } else if p.step < 1. { format!("{:.2}{}", self.values[i], p.unit) }
                else { format!("{:.0}{}", self.values[i], p.unit) };
                let cell = crate::control_panel::parameter_cell(
                    ("parameter", i), p.label, value,
                    Some(crate::modulation::Target::Parameter(self.number, i)),
                    self.enabled && self.group_enabled, self.text_scale,
                    cx.listener(move |this, delta: &f32, _, cx| {
                        if this.enabled && this.group_enabled {
                            this.end(cx); cx.emit(OscillatorEvent::Begin(i));
                            this.edit(i, this.values[i] + delta.signum() * this.parameters[i].step, cx);
                            cx.emit(OscillatorEvent::End(i));
                        }
                    }),
                ).on_mouse_down(MouseButton::Left, cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                    if this.enabled && this.group_enabled {
                        this.end(cx); cx.emit(OscillatorEvent::Begin(i));
                        if e.click_count == 2 {
                            this.edit(i, this.parameters[i].default, cx); cx.emit(OscillatorEvent::End(i));
                        } else { this.drag = Some((i, e.position, this.values[i])); }
                    }
                }));
                controls.push(cell.into_any_element());
            }
            let content = if panel == 2 {
                self.unison_content.clone().unwrap_or_else(|| self.unison_editor.clone().into()).into_any_element()
            } else {
                let values = self.values;
                let curve = self.unison_curve.clone();
                let samples = self.waveform_samples.clone();
                canvas(|_, _, _| (), move |bounds, _, window, _| {
                    if let Ok(ink) = graph((f64::from(bounds.left()), f64::from(bounds.top()),
                        f64::from(bounds.size.width), f64::from(bounds.size.height)), &values, true, &curve, samples.as_deref()) {
                        window.paint_path(ink.path, crate::live_theme::color(ink.color));
                    }
                }).size_full().into_any_element()
            };
            root = root.child(div().absolute().left(px(r.0 as f32)).top(px(r.1 as f32))
                .w(px(r.2 as f32)).h(px(r.3 as f32))
                .child(crate::control_panel::engine_panel(("engine", panel), title, content, controls)));
        }
        if !self.engine_panels.is_empty() {
            root = root.child(div().absolute().left(px(56.)).right(px(56.))
                .top(px(if self.in_group { 12. } else { 56. })).h(px((HEIGHT - 24.) as f32))
                .child(crate::module_shell::module_shell("engine-composition", div().w(px(HEADER_WIDTH as f32)),
                    crate::module_shell::panel_row(self.engine_panels.iter().cloned().map(IntoElement::into_any_element).collect()))));
        }
        root = root.child(
            div()
                .id("identity")
                .absolute()
                .left(px(header.0 as f32))
                .top(px(header.1 as f32))
                .w(px(HEADER_WIDTH as f32))
                .h(px(HEIGHT as f32))
                .flex()
                .flex_col()
                .items_center()
                .p(px(HEADER_PAD as f32))
                .gap(px(HEADER_PAD as f32))
                .on_hover(cx.listener(|this, hover, _, cx| {
                    this.hover = *hover;
                    cx.notify();
                }))
                .child(
                    div()
                        .id("remove")
                        .flex_shrink_0()
                        .size(px(HEADER_BUTTON as f32))
                        .cursor_pointer()
                        .role(Role::Button)
                        .aria_label("Remove oscillator")
                        .tab_index(13)
                        .focus(|s| s.bg(rgba(0xffffff18)))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.end(cx);
                            this.removed = true;
                            cx.emit(OscillatorEvent::Remove);
                            cx.notify();
                        })),
                )
                .child(div().flex_1())
                .child(
                    div()
                        .id("enable")
                        .flex_shrink_0()
                        .size(px(HEADER_BUTTON as f32))
                        .cursor_pointer()
                        .role(Role::Button)
                        .aria_label("Toggle oscillator")
                        .tab_index(12)
                        .focus(|s| s.bg(rgba(0xffffff18)))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.end(cx);
                            this.enabled = !this.enabled;
                            crate::modulation::hide_oscillator(cx, this.number);
                            cx.emit(OscillatorEvent::Enabled(this.enabled));
                            cx.notify();
                            cx.refresh_windows();
                        })),
                ),
        );
        root.into_any_element()
    }
}
#[cfg(test)]
mod tests {
    use super::{
        CornerStyle, FONT, GAP, PARAMS, Polygon, TEXT, fillet, geometry, label, panels, quantize,
        union, weight,
    };
    #[test]
    fn oscillator_contract() {
        let invalid = geometry::Path {
            commands: vec![geometry::PathCommand::LineTo(geometry::Point::new(0., 0.))],
        };
        assert!(super::geometry_builder(&invalid, false).is_err());
        for direction in [-1., 1.] {
            let r = 10.;
            let sweep = direction * std::f64::consts::PI;
            let center = geometry::Point::new(0., 0.);
            let path = geometry::Path {
                commands: vec![
                    geometry::PathCommand::MoveTo(center),
                    geometry::PathCommand::LineTo(geometry::Point::new(r, 0.)),
                    geometry::PathCommand::ArcTo(geometry::Arc {
                        center,
                        radius: r,
                        start_angle: 0.,
                        sweep,
                        to: geometry::Point::new(sweep.cos() * r, sweep.sin() * r),
                    }),
                    geometry::PathCommand::Close,
                ],
            };
            assert!(
                !super::geometry_builder(&path, false)
                    .unwrap()
                    .build()
                    .unwrap()
                    .vertices
                    .is_empty()
            );
        }

        for count in [0, 1, 3, 7] {
            let path = super::group_border(960., count).unwrap();
            assert_eq!(path.flatten(0.1, 100_000).unwrap().len(), 1);
            for row in 0..count {
                let y = super::GROUP_HEAD + row as f64 * super::GROUP_ROW + 36.;
                for x in [40., 54., 906., 920.] {
                    assert!(
                        path.contains(geometry::Point::new(x, y), 0.1, 100_000)
                            .unwrap()
                    );
                }
                assert!(
                    !path
                        .contains(geometry::Point::new(40., y + 60.), 0.1, 100_000)
                        .unwrap()
                );
            }
        }
        for routes in [1, 10, 11, 26] {
            let path = super::group_border_ports(960., &[(routes, routes)]).unwrap();
            assert_eq!(path.flatten(0.1, 100_000).unwrap().len(), 1);
            for slot in 0..=routes {
                let col = slot / 11;
                let row = slot % 11;
                for x in [46. - col as f64 * 24., 914. + col as f64 * 24.] {
                    assert!(
                        path.contains(
                            geometry::Point::new(x, super::GROUP_HEAD + 19. + row as f64 * 24.),
                            0.1,
                            100_000
                        )
                        .unwrap(),
                        "port slot outside shell"
                    );
                }
            }
        }
        for width in [760., 960., 1440.] {
            let r = panels(width, 44.).unwrap();
            assert_eq!(r[1].2, r[2].2);
            assert_eq!(r[1].1 - r[0].1, super::PAD);
            assert_eq!(r[1].3, super::HEIGHT - 2. * super::PAD);
            assert_eq!(width - 56. - r[2].0 - r[2].2, super::PAD);
            assert_eq!(r[1].0 - r[0].0 - r[0].2, GAP);
            assert_eq!(r[2].0 - r[1].0 - r[1].2, GAP);
        }
        assert_eq!(quantize(1000., PARAMS[6]), 64.);
        assert_eq!(quantize(f32::NAN, PARAMS[6]), 7.);
        for scale in [0.85, 1., 1.2] {
            let ink =
                super::centered_label("OSCILLATOR 01", 83., 144., 19. * scale, true, TEXT).unwrap();
            let b = ink.path.bounds;
            assert!((f32::from(b.origin.x + b.size.width / 2.) - 83.).abs() < 0.1);
            assert!((f32::from(b.origin.y + b.size.height / 2.) - 144.).abs() < 0.1);
            assert!(f32::from(b.origin.y) > 42. && f32::from(b.bottom()) < 246.);
        }
        let centers = super::header_centers(0.);
        assert_eq!(centers[1] - centers[0], centers[2] - centers[1]);
        let thin = label("OSCILLATOR", 0., 0., 10., false, TEXT).unwrap();
        let bold = label("OSCILLATOR", 0., 0., 24., false, TEXT).unwrap();
        assert_ne!(thin.svg, bold.svg);
        assert!(weight(24.) > weight(10.));
        let mut face = ttf_parser::Face::parse(FONT, 0).unwrap();
        let glyph = face.glyph_index('M').unwrap();
        face.set_variation(ttf_parser::Tag::from_bytes(b"wght"), 300.)
            .unwrap();
        let thin = face.glyph_bounding_box(glyph).unwrap();
        face.set_variation(ttf_parser::Tag::from_bytes(b"wght"), 800.)
            .unwrap();
        assert_ne!(thin, face.glyph_bounding_box(glyph).unwrap());
        face.set_variation(ttf_parser::Tag::from_bytes(b"wdth"), 75.)
            .unwrap();
        let narrow = face.glyph_hor_advance(glyph).unwrap();
        face.set_variation(ttf_parser::Tag::from_bytes(b"wdth"), 100.)
            .unwrap();
        assert!(face.glyph_hor_advance(glyph).unwrap() > narrow);
        let shapes = [
            Polygon::rectangle(56., 44., 54., 288.).unwrap().into(),
            Polygon::rectangle(28., 44., 40., 38.).unwrap().into(),
        ];
        let path = fillet(
            &union(&shapes, Default::default()).unwrap(),
            CornerStyle {
                convex_radius: 12.,
                concave_radius: 10.,
                ..Default::default()
            },
        )
        .unwrap()
        .path;
        assert_eq!(path.flatten(0.1, 10000).unwrap().len(), 1);
        assert!(
            path.contains(geometry::Point::new(54., 60.), 0.1, 10000)
                .unwrap()
        );
        assert!(
            !path
                .contains(geometry::Point::new(30., 110.), 0.1, 10000)
                .unwrap()
        );
    }
}

/// A group owns independent oscillator entities; its outline is one union with all port tabs.
pub struct OscillatorGroup {
    oscillators: Vec<(usize, Entity<Oscillator>, Subscription)>,
    next_number: usize,
    text_scale: f32,
    width: f64,
    enabled: bool,
    collapsed: bool,
    controls: Entity<crate::group_header::GroupHeader>,
    skin: std::cell::RefCell<Option<GroupSkin>>,
}
#[derive(Clone, Debug)]
pub struct GroupEvent {
    pub oscillator: usize,
    pub event: OscillatorEvent,
}
impl EventEmitter<GroupEvent> for OscillatorGroup {}
const GROUP_HEAD: f64 = 148.;
const GROUP_ROW: f64 = HEIGHT;
impl OscillatorGroup {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let mut group = Self {
            oscillators: Vec::new(),
            next_number: 1,
            text_scale: 1.,
            width: 960.,
            enabled: true,
            collapsed: false,
            controls: cx.new(crate::group_header::GroupHeader::new),
            skin: Default::default(),
        };
        for _ in 0..3 {
            group.add(cx);
        }
        group
    }
    fn add(&mut self, cx: &mut Context<Self>) {
        let number = self.next_number;
        self.next_number += 1;
        let entity = cx.new(|cx| {
            let mut oscillator = Oscillator::new(cx);
            oscillator.number = number;
            oscillator.text_scale = self.text_scale;
            oscillator.in_group = true;
            oscillator.group_enabled = self.enabled;
            oscillator.values[0] = ((number - 1) % 4) as f32;
            oscillator
        });
        let subscription = cx.subscribe(&entity, move |this, _, event: &OscillatorEvent, cx| {
            cx.emit(GroupEvent {
                oscillator: number,
                event: event.clone(),
            });
            if matches!(event, OscillatorEvent::Remove) {
                this.oscillators.retain(|(id, _, _)| *id != number);
                if cx.has_global::<crate::modulation::Routing>() {
                    cx.global_mut::<crate::modulation::Routing>()
                        .remove_oscillator(number);
                    cx.refresh_windows();
                }
                cx.notify();
            }
        });
        self.oscillators.push((number, entity, subscription));
        self.collapsed = false;
        cx.notify();
    }
    pub(crate) fn check_unison_interactions(
        group: &Entity<Self>,
        window: &mut Window,
        cx: &mut App,
        done: impl FnOnce(&mut Window, &mut App) + 'static,
    ) {
        let oscillator = group.read(cx).oscillators[0].1.clone();
        let editor = oscillator.read(cx).unison_editor.clone();
        let original = oscillator.read(cx).unison_curve.clone();
        crate::curve_editor::check_interactions(&editor, window, cx, move |window, cx| {
            oscillator.update(cx, |oscillator, cx| {
                oscillator.unison_curve = original;
                cx.notify();
            });
            done(window, cx);
        });
    }
    pub fn set_text_scale(&mut self, scale: f32, cx: &mut Context<Self>) {
        if !scale.is_finite() {
            return;
        }
        self.text_scale = scale.clamp(0.85, 1.2);
        for (_, entity, _) in &self.oscillators {
            entity.update(cx, |oscillator, cx| {
                oscillator.text_scale = self.text_scale;
                cx.notify();
            });
        }
        cx.notify();
    }
    fn count(&self) -> usize {
        if self.collapsed {
            0
        } else {
            self.oscillators.len()
        }
    }
    fn header(&self) -> anyhow::Result<Vec<Ink>> {
        let r = Polygon::rectangle(56., 28., self.width - 112., 120.)?;
        let p = fillet(
            &union(&[r.into()], Default::default())?,
            CornerStyle {
                convex_radius: 12.,
                ..Default::default()
            },
        )?
        .path;
        Ok(vec![
            Ink::geometry(&p, 0x343f32)?,
            label(
                if self.collapsed { "+" } else { "−" },
                76.,
                63.,
                22.,
                false,
                ACCENT,
            )?,
            label(
                "+ ADD OSC",
                (self.width - 250.) as f32,
                62.,
                12.,
                false,
                ACCENT,
            )?,
            label(
                if self.enabled { "ON" } else { "OFF" },
                (self.width - 112.) as f32,
                62.,
                12.,
                false,
                if self.enabled { ACCENT } else { MUTED },
            )?,
        ])
    }
    pub fn svg(&self, cx: &App) -> anyhow::Result<String> {
        let height = group_height(self.count());
        let border = group_border(self.width, self.count())?.to_svg_data()?;
        let mut svg = format!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="{height}" viewBox="0 0 {} {height}"><title>MUI oscillator group</title><rect width="100%" height="100%" fill="#171d1a"/>"##,
            self.width, self.width
        );
        for i in self.header()? {
            write!(svg, r##"<path d="{}" fill="#{:06x}"/>"##, i.svg, i.color)?;
        }
        if !self.collapsed {
            for (i, (_, oscillator, _)) in self.oscillators.iter().enumerate() {
                let content = oscillator.read(cx).svg(self.width)?;
                write!(
                    svg,
                    r#"<g transform="translate(0,{})">{content}</g>"#,
                    GROUP_HEAD + i as f64 * GROUP_ROW
                )?;
            }
        }
        write!(
            svg,
            r##"<path d="{border}" fill="none" stroke="#718164" stroke-width="1.25"/></svg>"##
        )?;
        Ok(svg)
    }
}
fn group_height(count: usize) -> f64 {
    GROUP_HEAD + count as f64 * GROUP_ROW + 28.
}
fn port_shapes(
    width: f64,
    top: f64,
    ports: (usize, usize),
) -> anyhow::Result<Vec<mui::geometry::PlacedShape>> {
    let mut shapes = Vec::new();
    for (left, routes) in [(true, ports.0), (false, ports.1)] {
        let container =
            crate::pie_container::PieContainer::port(routes + 1, HEIGHT as f32 - 16., left);
        shapes.extend(container.outer_shapes(geometry::Point::new(
            if left { 46. } else { width - 46. },
            top + 19.,
        ))?);
    }
    Ok(shapes)
}
fn group_border(width: f64, count: usize) -> anyhow::Result<geometry::Path> {
    group_border_ports(width, &vec![(0, 0); count])
}
fn group_border_ports(width: f64, ports: &[(usize, usize)]) -> anyhow::Result<geometry::Path> {
    let mut shapes =
        vec![Polygon::rectangle(56., 28., width - 112., group_height(ports.len()) - 56.)?.into()];
    for (row, ports) in ports.iter().enumerate() {
        shapes.extend(port_shapes(
            width,
            GROUP_HEAD + row as f64 * GROUP_ROW,
            *ports,
        )?);
    }
    Ok(crate::pie_container::rounded(&shapes)?)
}
struct GroupSkin {
    key: Vec<u64>,
    border: Ink,
}
impl OscillatorGroup {
    pub fn benchmark(&self, cx: &App) -> anyhow::Result<()> {
        let start = std::time::Instant::now();
        for _ in 0..30 {
            std::hint::black_box((
                self.header()?,
                border_ink(&group_border(self.width, self.count())?)?,
            ));
        }
        let before = start.elapsed().as_secs_f64() * 1000. / 30.;
        self.skin(cx)?;
        let start = std::time::Instant::now();
        for _ in 0..120 {
            std::hint::black_box(self.skin(cx)?);
        }
        eprintln!(
            "group outline/header preparation: uncached {before:.3} ms/frame; cached {:.3} ms/frame",
            start.elapsed().as_secs_f64() * 1000. / 120.
        );
        Ok(())
    }
    fn skin(&self, cx: &App) -> anyhow::Result<Ink> {
        let ports: Vec<_> = self
            .oscillators
            .iter()
            .take(self.count())
            .map(|(id, _, _)| crate::modulation::port_counts(cx, *id))
            .collect();
        let mut key = vec![
            self.width.to_bits(),
            self.text_scale.to_bits() as u64,
            self.enabled as u64,
            self.collapsed as u64,
        ];
        key.extend(ports.iter().flat_map(|(a, b)| [*a as u64, *b as u64]));
        let mut cache = self.skin.borrow_mut();
        if cache.as_ref().is_none_or(|c| c.key != key) {
            *cache = Some(GroupSkin {
                key,
                border: border_ink(&group_border_ports(self.width, &ports)?)?,
            });
        }
        let cache = cache.as_ref().unwrap();
        Ok(cache.border.clone())
    }
}
pub(super) fn border_ink(path: &geometry::Path) -> anyhow::Result<Ink> {
    let builder = geometry_builder(path, true)?;
    Ok(Ink {
        path: builder.build()?,
        svg: path.to_svg_data()?,
        color: 0x718164,
        stroke: true,
        text: None,
    })
}
impl Render for OscillatorGroup {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let count = self.count();
        let (left_gutter, right_gutter) = crate::modulation::gutters(cx);
        let border = match self.skin(cx) {
            Ok(v) => v,
            Err(e) => return div().child(e.to_string()).into_any_element(),
        };
        let target = cx.entity().downgrade();
        let background = canvas(
            move |bounds, window, cx| {
                let width = f64::from(bounds.size.width);
                let changed = target
                    .upgrade()
                    .is_some_and(|entity| (entity.read(cx).width - width).abs() > 0.5);
                if changed {
                    window.on_next_frame(move |_, cx| {
                        let _ = target.update(cx, |this, cx| {
                            this.width = width;
                            cx.notify();
                        });
                    });
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full();
        let outline = canvas(
            |_, _, _| (),
            move |bounds, _, window, _| {
                let mut path = border.path;
                path.bounds.origin += bounds.origin;
                for v in &mut path.vertices {
                    v.xy_position += bounds.origin;
                }
                window.with_content_mask(
                    Some(ContentMask {
                        bounds: Bounds::new(
                            bounds.origin + point(px(0.), px(148.)),
                            size(bounds.size.width, bounds.size.height - px(148.)),
                        ),
                    }),
                    |window| window.paint_path(path, crate::live_theme::color(border.color)),
                );
            },
        )
        .absolute()
        .size_full();
        let mut content = div()
            .relative()
            .w_full()
            .min_w(px(760.))
            .h(px(group_height(count) as f32))
            .child(background);
        if !self.collapsed {
            for (i, (_, entity, _)) in self.oscillators.iter().enumerate() {
                content = content.child(
                    div()
                        .absolute()
                        .top(px((GROUP_HEAD + i as f64 * GROUP_ROW) as f32))
                        .w_full()
                        .h(px(GROUP_ROW as f32))
                        .child(entity.clone()),
                );
            }
        }
        content = content
            .child(
                div()
                    .absolute()
                    .left(px(56.))
                    .top(px(28.))
                    .w(px((self.width - 112.) as f32))
                    .h(px(120.))
                    .child(self.controls.clone()),
            )
            .child(outline)
            .child(
                div()
                    .id("collapse-group")
                    .absolute()
                    .left(px(64.))
                    .top(px(34.))
                    .w(px(42.))
                    .h(px(44.))
                    .tab_index(0)
                    .role(Role::Button)
                    .child(crate::kurv::ink(
                        if self.collapsed { "+" } else { "−" },
                        20.,
                        ACCENT,
                    ))
                    .aria_label("Expand or collapse group")
                    .cursor_pointer()
                    .focus(|s| s.bg(rgba(0xffffff18)))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.collapsed = !this.collapsed;
                        if this.collapsed {
                            for (id, _, _) in &this.oscillators {
                                crate::modulation::hide_oscillator(cx, *id);
                            }
                        }
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .id("add-oscillator")
                    .absolute()
                    .right(px(154.))
                    .top(px(34.))
                    .w(px(110.))
                    .h(px(44.))
                    .tab_index(1)
                    .role(Role::Button)
                    .child(crate::kurv::ink("+ ADD OSC", 11., ACCENT))
                    .aria_label("Add oscillator")
                    .cursor_pointer()
                    .hover(|s| s.bg(rgba(0xffffff08)))
                    .focus(|s| s.bg(rgba(0xffffff18)))
                    .on_click(cx.listener(|this, _, _, cx| this.add(cx))),
            )
            .child(
                div()
                    .id("group-power")
                    .absolute()
                    .right(px(64.))
                    .top(px(34.))
                    .w(px(60.))
                    .h(px(44.))
                    .tab_index(2)
                    .role(Role::Button)
                    .child(crate::kurv::ink(
                        if self.enabled { "ON" } else { "OFF" },
                        11.,
                        ACCENT,
                    ))
                    .aria_label("Toggle group")
                    .cursor_pointer()
                    .focus(|s| s.bg(rgba(0xffffff18)))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.enabled = !this.enabled;
                        for (id, _, _) in &this.oscillators {
                            crate::modulation::hide_oscillator(cx, *id);
                        }
                        for (_, entity, _) in &this.oscillators {
                            entity.update(cx, |oscillator, cx| {
                                oscillator.end(cx);
                                oscillator.group_enabled = this.enabled;
                                cx.notify();
                            });
                        }
                        cx.notify();
                    })),
            );
        div()
            .id("oscillator-group-scroll")
            .size_full()
            .bg(crate::live_theme::color(0x171d1a))
            .overflow_y_scroll()
            .map(crate::controls::navigation)
            .overflow_x_scroll()
            .child(
                div()
                    .w_full()
                    .min_w(px(760. + left_gutter + right_gutter))
                    .pl(px(left_gutter))
                    .pr(px(right_gutter))
                    .child(content),
            )
            .into_any_element()
    }
}

struct LoadedFonts;
impl Global for LoadedFonts {}
fn load_fonts(cx: &mut App) {
    if !cx.has_global::<LoadedFonts>() {
        cx.text_system()
            .add_fonts(vec![std::borrow::Cow::Borrowed(FONT)])
            .expect("bundled Roboto font");
        cx.set_global(LoadedFonts);
    }
}
pub(super) fn native_text() -> bool {
    static NATIVE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *NATIVE.get_or_init(|| std::env::var_os("MUI_TEXT_OUTLINES").is_none())
}
pub(super) fn text_font(size: f32) -> Font {
    let mut f = font("Roboto");
    f.weight = FontWeight(weight(size).max(400.));
    f
}
fn paint_text(
    text: &str,
    origin: Point<Pixels>,
    size: f32,
    role: u32,
    window: &mut Window,
    cx: &mut App,
) {
    if text.is_empty() {
        return;
    }
    let line = window.text_system().shape_line(
        text.to_owned().into(),
        px(size),
        &[TextRun {
            len: text.len(),
            font: text_font(size),
            color: crate::live_theme::color(role),
            background_color: None,
            underline: None,
            strikethrough: None,
        }],
        None,
    );
    if let Err(error) = line.paint(origin, px(size), TextAlign::Left, None, window, cx) {
        eprintln!("text paint: {error}");
    }
}

/// Preserve arcs until GPUI tessellation; both fill and border use identical cubics.
pub(super) fn geometry_builder(path: &geometry::Path, stroke: bool) -> anyhow::Result<PathBuilder> {
    path.validate(100_000)?;
    let mut b = if stroke {
        PathBuilder::stroke(px(1.)).with_style(PathStyle::Stroke(
            StrokeOptions::default()
                .with_line_width(1.)
                .with_tolerance(0.025),
        ))
    } else {
        PathBuilder::fill().with_style(PathStyle::Fill(
            FillOptions::default()
                .with_fill_rule(FillRule::NonZero)
                .with_tolerance(0.025),
        ))
    };
    let p = |p: geometry::Point| point(px(p.x as f32), px(p.y as f32));
    for command in &path.commands {
        match *command {
            geometry::PathCommand::MoveTo(a) => b.move_to(p(a)),
            geometry::PathCommand::LineTo(a) => b.line_to(p(a)),
            geometry::PathCommand::Close => b.close(),
            geometry::PathCommand::ArcTo(arc) => {
                let pieces = (arc.sweep.abs() / std::f64::consts::FRAC_PI_2)
                    .ceil()
                    .max(1.) as usize;
                for i in 0..pieces {
                    let start = arc.start_angle + arc.sweep * i as f64 / pieces as f64;
                    let end = arc.start_angle + arc.sweep * (i + 1) as f64 / pieces as f64;
                    let k = 4. / 3. * ((end - start) / 4.).tan() * arc.radius;
                    let a = arc.point_at(i as f64 / pieces as f64);
                    let z = if i + 1 == pieces {
                        arc.to
                    } else {
                        arc.point_at((i + 1) as f64 / pieces as f64)
                    };
                    b.cubic_bezier_to(
                        p(z),
                        p(a + geometry::Point::new(-start.sin(), start.cos()) * k),
                        p(z - geometry::Point::new(-end.sin(), end.cos()) * k),
                    );
                }
            }
        }
    }
    Ok(b)
}
