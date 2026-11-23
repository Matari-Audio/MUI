//! Controls as compositions. Nothing here is placed absolutely: a thumb sits
//! where two flex weights put it, a knob's pointer is an anchored offset.
use std::ops::RangeInclusive;
use std::sync::Arc;

use mui_input::{FINE_DRAG, Key};
use mui_material::prelude::*;
use mui_scene::{Palette, Px, SpacingToken, Spring, Stroke};

use crate::Ui;
use crate::widgets::{Response, TextOpts, text_edit};

/// What a ranged control edits: its range, the value a double click puts
/// back, where its fill grows from, and how its value reads and is typed
/// in units. A bare `lo..=hi` converts, so a plain range still works
/// everywhere a `Domain` is asked for.
///
/// ```
/// use mui::prelude::*;
/// let mut ui = Ui::default();
/// let mut pan = 0.0;
/// // Fills from the centre (a range symmetric about 0 does by default);
/// // a double click puts it back to 0.
/// let p = knob(&mut ui, "pan", "Pan", &mut pan, Domain::new(-1.0..=1.0).default(0.0));
/// ```
#[derive(Clone)]
pub struct Domain {
    range: RangeInclusive<f64>,
    default: Option<f64>,
    origin: Option<f64>,
    format: Option<Format>,
    parse: Option<Parse>,
}
/// A value in the domain's units, as text.
type Format = Arc<dyn Fn(f64) -> String + Send + Sync>;
/// Typed text back to a value in the domain's units, `None` if it does not read.
type Parse = Arc<dyn Fn(&str) -> Option<f64> + Send + Sync>;

impl std::fmt::Debug for Domain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Domain")
            .field("range", &self.range)
            .field("default", &self.default)
            .field("origin", &self.origin)
            .field("format", &self.format.is_some())
            .field("parse", &self.parse.is_some())
            .finish()
    }
}
impl Domain {
    pub fn new(range: RangeInclusive<f64>) -> Self {
        Self {
            range,
            default: None,
            origin: None,
            format: None,
            parse: None,
        }
    }
    /// The value a double click restores.
    pub fn default(mut self, value: f64) -> Self {
        self.default = Some(value);
        self
    }
    /// Where the fill or arc starts. Default: the centre of a range
    /// symmetric about zero (pan, detune), otherwise its start.
    pub fn origin(mut self, value: f64) -> Self {
        self.origin = Some(value);
        self
    }
    /// How a value reads, in units. A [`knob`], [`slider`] or
    /// [`drag_value`] shows it when no `.value_text(..)` is given, reserves
    /// the formatted ends of the range, and opens its typing field with it.
    ///
    /// Build the domain once and clone it each frame: a clone shares the
    /// closure instead of allocating a new one.
    ///
    /// ```
    /// use mui::prelude::*;
    /// let mut ui = Ui::default();
    /// let hz = Domain::new(20.0..=20_000.0).format(|v| format!("{v:.0} Hz"));
    /// let mut cutoff = 440.0;
    /// let dial = knob(&mut ui, "cut", "Cutoff", &mut cutoff, hz.clone());
    /// assert_eq!(hz.formatted(440.0), "440 Hz");
    /// ```
    pub fn format(mut self, f: impl Fn(f64) -> String + Send + Sync + 'static) -> Self {
        self.format = Some(Arc::new(f));
        self
    }
    /// How typed text reads back as a value of this domain (`"1.2k"`,
    /// `"-3 dB"`); `None` leaves the value alone. Without one, typing reads a
    /// plain number. A [`knob`] or [`slider`] opens a typing field only when
    /// its domain has a parser; [`drag_value`] always does.
    ///
    /// ```
    /// use mui::prelude::*;
    /// let hz = Domain::new(20.0..=20_000.0).parse(|s| {
    ///     let s = s.trim().trim_end_matches("Hz").trim();
    ///     match s.strip_suffix('k') {
    ///         Some(k) => k.trim().parse::<f64>().ok().map(|k| k * 1000.0),
    ///         None => s.parse().ok(),
    ///     }
    /// });
    /// assert_eq!(hz.parsed("1.2k"), Some(1200.0));
    /// assert_eq!(hz.parsed("99999 Hz"), Some(20_000.0), "clamped to the range");
    /// assert_eq!(hz.parsed("loud"), None);
    /// ```
    pub fn parse(mut self, f: impl Fn(&str) -> Option<f64> + Send + Sync + 'static) -> Self {
        self.parse = Some(Arc::new(f));
        self
    }
    /// `value` as this domain formats it: the [`Domain::format`] callback,
    /// or two decimals.
    ///
    /// ```
    /// use mui::prelude::*;
    /// assert_eq!(Domain::new(0.0..=1.0).formatted(0.5), "0.50");
    /// ```
    pub fn formatted(&self, value: f64) -> String {
        match &self.format {
            Some(f) => f(value),
            None => format!("{value:.2}"),
        }
    }
    /// Typed text as a value of this domain, clamped to its range: the
    /// [`Domain::parse`] callback, or a plain number. `None` for text that
    /// does not read or a value that is not finite.
    ///
    /// ```
    /// use mui::prelude::*;
    /// assert_eq!(Domain::new(0.0..=1.0).parsed(" 2 "), Some(1.0));
    /// assert_eq!(Domain::new(0.0..=1.0).parsed("NaN"), None);
    /// ```
    pub fn parsed(&self, text: &str) -> Option<f64> {
        let v = match &self.parse {
            Some(f) => f(text),
            None => text.trim().parse().ok(),
        }?;
        let (lo, hi) = (*self.range.start(), *self.range.end());
        v.is_finite().then(|| v.clamp(lo.min(hi), lo.max(hi)))
    }
    fn origin_t(&self) -> f64 {
        let (lo, hi) = (*self.range.start(), *self.range.end());
        let origin = self
            .origin
            .unwrap_or(if lo == -hi && lo != 0.0 { 0.0 } else { lo });
        unit(origin, &self.range)
    }
}
impl From<RangeInclusive<f64>> for Domain {
    fn from(range: RangeInclusive<f64>) -> Self {
        Self::new(range)
    }
}

/// How solid a control looks. daisyUI's four button styles, resolved from
/// the role and the palette rather than from a table of colours.
///
/// ```
/// use mui::prelude::*;
/// let mut ui = Ui::default();
/// let quiet = button(&mut ui, "bypass", "Bypass").el;
/// // Ink only: the resting box paints nothing.
/// assert_eq!(quiet.variant(Variant::Ghost).el().payload().style.fill, None);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Variant {
    /// The role, filled. The one live action on a panel.
    #[default]
    Solid,
    /// The role at a low alpha: the same colour, mixed into whatever it sits
    /// on, with the role as its ink.
    Soft,
    /// The role as a stroke and as ink; nothing filled.
    Outline,
    /// Ink only, until the pointer arrives.
    Ghost,
}

/// What modulates a control, as fractions (`0..=1`) of its travel: the
/// reach of its routes from `lo` to `hi`, the base value `at` they swing
/// about, and where the modulated value is now, while it runs. A knob draws
/// it on a ring outside its arc; a plugin's own faces read the same numbers.
#[derive(Clone, Debug, PartialEq)]
pub struct Modulation {
    pub at: f64,
    pub lo: f64,
    pub hi: f64,
    pub live: Option<f64>,
    pub fill: Fill,
}

impl Modulation {
    /// Signed offsets in travel fractions -- `lo` and `hi` the routes'
    /// summed depths, `live` the current swing -- placed about the base
    /// `at` and clamped to the travel.
    pub fn around(at: f64, lo: f64, hi: f64, live: Option<f64>, fill: impl Into<Fill>) -> Self {
        let at = at.clamp(0.0, 1.0);
        let place = |offset: f64| (at + offset).clamp(0.0, 1.0);
        Self {
            at,
            lo: place(lo),
            hi: place(hi),
            live: live.map(place),
            fill: fill.into(),
        }
    }
}

/// A control's look, resolved: the pixel height it was sized to, the
/// palette the variant does its arithmetic with, and what the readout was
/// told to say.
struct Look {
    px: f64,
    variant: Variant,
    role: Role,
    accent: Option<Fill>,
    modulation: Option<Modulation>,
    palette: Palette,
    text: Option<String>,
    reserve: Option<Arc<[String]>>,
    description: Option<Arc<str>>,
    step: Option<f64>,
    /// The domain's formatter, for a reading no `.value_text` replaced.
    format: Option<Format>,
    knob: Knob,
    /// A slider drawn flat: a hairline rail, a block thumb across it and a
    /// tick at a bipolar origin (KONTRA's look). Off: a pill rail and a round
    /// thumb.
    flat: bool,
    /// A knob face that takes a square share of its cell.
    fill_cell: bool,
    /// A button's leading and trailing icons.
    icons: [Option<char>; 2],
    /// Where a reading sits in its reserved width.
    value_align: TextAlign,
    value_align_explicit: bool,
}

/// How a [`knob`] is painted: a 270-degree track, the value arc on it, a
/// disc inside and, optionally, a pointer. Lengths are shares of the knob's
/// radius (half the face), so one look fits every size; colours are fills,
/// roles by default, so it follows the theme. Set it per control with
/// [`Control::look`]; a bipolar arc comes from the domain's `origin`.
///
/// The presets are the Matari plugins' knobs, so they can share them:
/// [`Knob::smooth`] (the default), [`Knob::k`] (KURV and KONTRA),
/// [`Knob::drama`] and [`Knob::buffr`].
///
/// ```
/// use mui::prelude::*;
/// let mut ui = Ui::default();
/// let mut v = 0.5;
/// let k = knob(&mut ui, "depth", "Depth", &mut v, 0.0..=1.0).look(Knob::k());
/// // Or any part of a preset.
/// let thin = Knob { width: 0.08, ..Knob::smooth() };
/// assert_eq!(Knob::default(), Knob::smooth());
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct Knob {
    /// The track's centreline.
    pub ring: f64,
    /// The track's and the arc's stroke width.
    pub width: f64,
    /// The gap between the track's inner edge and the face disc.
    pub inset: f64,
    /// The track, the whole 270 degrees.
    pub track: Fill,
    /// The value arc. `None` takes the control's role (or its
    /// [`Control::accent`]) and brightens it under the pointer; a fill
    /// here is painted as given, but `accent` still wins.
    pub arc: Option<Fill>,
    /// The disc inside the ring.
    pub face: Fill,
    /// How the track and the arc end: [`Cap::Butt`] for a square-cut
    /// instrument arc, [`Cap::Round`] for a soft one.
    pub cap: Cap,
    /// A centred shadow the face casts over the ring: its blur as a share of
    /// the radius, and its paint. `None` paints no shadow.
    pub shadow: Option<(f64, Fill)>,
    /// A mark at the value's angle, painted over the face. `None` paints
    /// none.
    pub pointer: Option<Pointer>,
}

/// A knob's value mark: a radial line from `from` to `to` (shares of the
/// radius, round-ended) and a dot of radius `dot` at `to`. A zero `width`
/// leaves only the dot; a zero `dot` only the line.
///
/// ```
/// use mui::prelude::*;
/// // BUFFR's dot riding the ring.
/// let dot = Pointer { from: 0.86, to: 0.86, width: 0.0, dot: 0.13, fill: None };
/// let k = Knob { pointer: Some(dot), ..Knob::smooth() };
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct Pointer {
    pub from: f64,
    pub to: f64,
    pub width: f64,
    pub dot: f64,
    /// `None` paints it in the arc's colour.
    pub fill: Option<Fill>,
}

impl Knob {
    /// SMOOTH's knob, MUI's default: a thick square-cut track at the edge,
    /// an ink arc, and a raised face sitting on the ring with a soft shadow.
    pub fn smooth() -> Self {
        Self {
            ring: 0.92,
            width: 0.14,
            inset: 0.0,
            track: Role::Level(4).into(),
            arc: Some(Role::Ink.into()),
            face: Role::Level(5).into(),
            cap: Cap::Butt,
            shadow: Some((0.12, Fill::Faded(Role::Level(-12), 0.55))),
            pointer: None,
        }
    }
    /// The K knob (KURV, KONTRA): a solid track, the role's arc, a domed
    /// face lit from above, and an ink pointer line.
    pub fn k() -> Self {
        Self {
            ring: 0.755,
            width: 0.105,
            inset: 0.157,
            track: Role::Level(3).into(),
            arc: None,
            face: Gradient::linear(180., [(0., Role::Level(6)), (1., Role::Level(5))]).into(),
            cap: Cap::Round,
            shadow: None,
            pointer: Some(Pointer {
                from: 0.16,
                to: 0.46,
                width: 0.112,
                dot: 0.0,
                fill: Some(Role::Ink.into()),
            }),
        }
    }
    /// DRAMA's knob: a faint hairline track and an ink arc that brightens
    /// along its sweep, with no face, so an icon or reading sits bare.
    pub fn drama() -> Self {
        Self {
            ring: 0.885,
            width: 0.16,
            inset: 0.0,
            track: Fill::Faded(Role::Ink, 0.10),
            arc: Some(
                Gradient::linear(
                    90.,
                    [(0., Fill::Faded(Role::Ink, 0.55)), (1., Role::Ink.into())],
                )
                .into(),
            ),
            face: Fill::None,
            cap: Cap::Round,
            shadow: None,
            pointer: None,
        }
    }
    /// BUFFR's knob: a sunken face, a soft track, the role's arc and a dot
    /// riding the ring at the value.
    pub fn buffr() -> Self {
        Self {
            ring: 0.86,
            width: 0.13,
            inset: 0.107,
            track: Fill::Faded(Role::Ink, 0.12),
            arc: None,
            face: Role::Level(-1).into(),
            cap: Cap::Round,
            shadow: None,
            pointer: Some(Pointer {
                from: 0.86,
                to: 0.86,
                width: 0.0,
                dot: 0.13,
                fill: None,
            }),
        }
    }
}
impl Default for Knob {
    fn default() -> Self {
        Self::smooth()
    }
}

/// A keyboard activation is the same primary action as a click. `Ui::keys`
/// is already gated to the focused surface, so widgets do not need to invent
/// a second focus test or consume another key stream.
fn activated(ui: &Ui, id: &str) -> bool {
    ui.get(id).activated()
}
impl Look {
    /// The resting look: a solid primary control with nothing said yet.
    fn new(palette: Palette, px: f64) -> Self {
        Self {
            px,
            variant: Variant::Solid,
            role: Role::Primary,
            accent: None,
            modulation: None,
            palette,
            text: None,
            reserve: None,
            description: None,
            step: None,
            format: None,
            knob: Knob::smooth(),
            flat: false,
            fill_cell: false,
            icons: [None; 2],
            value_align: TextAlign::Start,
            value_align_explicit: false,
        }
    }
    fn describe(&self, mut el: El, numeric: bool) -> El {
        if let Some(description) = &self.description {
            el = el.described(description.clone());
        }
        if numeric && let Some(step) = self.step {
            el = el.numeric_step(step);
        }
        if numeric && let Some(value) = &self.text {
            el = el.value_description(value.as_str());
        }
        el
    }
    /// What the control's own box is painted with, where the role is the
    /// fill: a button, a toggle that is on.
    fn style(&self) -> Style {
        self.face(self.role)
    }
    /// The same, for a control whose resting box is *not* the role -- a
    /// knob's face, a slider's track: `Solid` leaves it alone.
    fn face(&self, resting: Role) -> Style {
        match self.variant {
            Variant::Solid => Style::default().fill(resting),
            // A faded role *is* the mix with the ground: it composites over
            // whatever is under it, so it tracks the panel it lands on.
            Variant::Soft => Style::default().fill(self.role.alpha(0.18)),
            Variant::Outline => Style {
                stroke: Some(Stroke {
                    fill: Some(self.role.into()),
                    width: None,
                }),
                ..Style::default()
            },
            Variant::Ghost => Style::default(),
        }
    }
    /// The ink that reads on that box: `Palette::on` computes the contrast
    /// for a filled one, and the role speaks for itself on the rest.
    fn ink(&self) -> Fill {
        match self.variant {
            Variant::Solid => Role::Ink.into(),
            _ => self.role.into(),
        }
    }
    /// The vertical padding that centres one line of 14-unit text in the
    /// control's height.
    fn pad_y(&self) -> f64 {
        ((self.px - 14.0) / 2.0).max(2.0)
    }
    /// The state closure that lifts whatever the control is filled with, so
    /// a hover is the palette's own step and not a second colour.
    fn hover(&self) -> impl Fn(Style) -> Style + use<> {
        let (p, role, variant) = (self.palette, self.role, self.variant);
        move |s: Style| {
            let ground = p.surface();
            match variant {
                // Nothing to lift: the hover is the role, arriving.
                Variant::Outline | Variant::Ghost => s.fill(role.alpha(0.12)),
                _ => {
                    let lifted = s
                        .fill
                        .as_ref()
                        .unwrap_or(&Fill::None)
                        .map(&p, ground, |c| p.hover(c));
                    s.fill(lifted)
                }
            }
        }
    }
}

/// A button's paint for a face another widget builds (`select`'s
/// trigger): the resting box style, the ink that reads on it, and the
/// hover, from one variant and role. Kept apart so the two looks cannot
/// drift.
pub(crate) fn button_paint(
    palette: Palette,
    variant: Variant,
    role: Role,
) -> (Style, Fill, impl Fn(Style) -> Style + use<>) {
    let look = Look {
        variant,
        role,
        ..Look::new(palette, 0.0)
    };
    (look.style(), look.ink(), look.hover())
}

/// A control the widget has taken its value from, with what is left to say
/// about how it looks: the variant, the size, and the pixels as an escape
/// hatch. Finish it with [`Control::el`], or drop it straight into a
/// `row![..]`.
///
/// ```
/// use mui::prelude::*;
/// let mut ui = Ui::default();
/// let mut cutoff = 0.5;
/// let dial = knob(&mut ui, "cut", "Cutoff", &mut cutoff, 0.0..=1.0).el;
/// let strip = row![dial.size(L), body("post")];
/// ```
pub struct Control {
    look: Look,
    size: SpacingToken,
    px: Option<f64>,
    kind: Kind,
    disabled: bool,
    label_visible: bool,
    quiet: bool,
    vertical: bool,
}

/// Which control, with what it read from the runtime: the tree waits for
/// the look, which the caller can still change. One label allocation,
/// shared by the text and the accessible name.
enum Kind {
    /// A ranged control, with its typing field while one is open.
    Slider(Id, Arc<str>, Reading, Option<Box<El>>),
    Knob(Id, Arc<str>, Reading, DialCache, Option<Box<El>>),
    /// A button; the symbol makes it an icon button.
    Button(Id, Arc<str>, Option<char>),
    Toggle(Id, Arc<str>, bool),
    Drag(Id, Arc<str>, Reading),
    /// A [`drag_value`] being typed into: its field, at the width it had.
    Field(Box<El>, f64),
}

/// A ranged control's reading: the value and its range, where the drawn
/// thumb or pointer sits (`t`, 0..1, tweened) and how hovered it is.
#[derive(Clone, Copy)]
struct Reading {
    value: f64,
    min: f64,
    max: f64,
    t: f64,
    /// Where the fill starts, 0..1 along the travel.
    origin: f64,
    hover: f64,
}
impl Reading {
    /// Drag `value` over `travel` (vertically for a dial), step it by the
    /// focused keys, and read it back with its thumb tweened toward it.
    fn read(
        ui: &mut Ui,
        id: &Id,
        value: &mut f64,
        domain: &Domain,
        travel: f64,
        vertical: bool,
    ) -> Self {
        let range = &domain.range;
        ui.drag(id, value, range.clone(), travel, vertical);
        stepped(ui, id, value, range);
        if let Some(default) = domain.default
            && ui.double_click(id.clone())
        {
            *value = default;
        }
        // Fast enough that a drag still feels direct, slow enough that a
        // preset change is a glide. The value and readout stay exact.
        let t = ui.tween_with(id, unit(*value, range), Spring::new(0.12, 1.0));
        let mut dial = Self::of(*value, range, t, ui.state(id).hover);
        dial.origin = domain.origin_t();
        dial
    }
    fn of(value: f64, range: &RangeInclusive<f64>, t: f64, hover: f64) -> Self {
        let (min, max) = (*range.start(), *range.end());
        Reading {
            value,
            min,
            max,
            t,
            origin: 0.0,
            hover,
        }
    }
    /// The filled span along the travel, low end first.
    fn span(&self) -> (f64, f64) {
        (self.origin.min(self.t), self.origin.max(self.t))
    }
    fn a11y(&self) -> A11y {
        A11y::Slider {
            value: self.value,
            min: self.min,
            max: self.max,
        }
    }
    fn readout(&self, look: &Look) -> El {
        let el =
            readout(look.text.clone(), self.value, self.min, self.max).text_align(look.value_align);
        match &look.reserve {
            Some(samples) => el.reserve_all(samples.clone()),
            None => el,
        }
    }
}

impl Control {
    fn new(ui: &Ui, kind: Kind) -> Self {
        Self {
            look: Look::new(ui.theme().palette, ui.theme().control),
            size: SpacingToken::M,
            px: None,
            kind,
            disabled: false,
            label_visible: true,
            quiet: false,
            vertical: false,
        }
    }

    /// Read with the domain's formatter when no `.value_text` is given.
    fn format(mut self, domain: &Domain) -> Self {
        self.look.format = domain.format.clone();
        self
    }

    pub(crate) fn hide_matching_adjustment_label(mut self, setting_label: &str) -> Self {
        if matches!(
            &self.kind,
            Kind::Slider(_, label, ..) | Kind::Knob(_, label, ..) if label.as_ref() == setting_label
        ) {
            self.label_visible = false;
        }
        self
    }
    /// How solid this control looks.
    ///
    /// ```
    /// use mui::prelude::*;
    /// let mut ui = Ui::default();
    /// let mut on = false;
    /// let sw = toggle(&mut ui, "bypass", "Bypass", &mut on).el;
    /// let sw = sw.variant(Variant::Outline);
    /// assert!(sw.el().payload().style.stroke.is_some(), "outlined");
    /// ```
    pub fn variant(mut self, v: Variant) -> Self {
        self.look.variant = v;
        self
    }
    /// The palette role this control speaks with: `Primary` unless it is a
    /// destructive action.
    ///
    /// ```
    /// use mui::prelude::*;
    /// let mut ui = Ui::default();
    /// let clear = button(&mut ui, "clear", "Clear").el;
    /// let el = clear.role(Role::Danger).variant(Variant::Outline).el();
    /// let ring = el.payload().style.stroke.clone().and_then(|s| s.fill);
    /// assert_eq!(ring, Some(Fill::Role(Role::Danger)));
    /// ```
    pub fn role(mut self, r: Role) -> Self {
        self.look.role = r;
        self
    }
    /// One of the five sizes, the same five everywhere: `Xs` through `Xl`
    /// multiply the theme's [`control`](mui_scene::Theme::control) unit.
    ///
    /// ```
    /// use mui::prelude::*;
    /// let mut ui = Ui::default();
    /// let mut on = false;
    /// let small = toggle(&mut ui, "bypass", "Bypass", &mut on).size(Xs);
    /// ```
    pub fn size(mut self, s: SpacingToken) -> Self {
        self.size = s;
        self
    }
    /// What the readout says, instead of the default two decimals: the
    /// value in the units the parameter actually has. A knob places this
    /// reading inside its arc and keeps its name below the dial.
    ///
    /// ```
    /// use mui::prelude::*;
    /// let mut ui = Ui::default();
    /// let mut cutoff = 440.0;
    /// let dial = knob(&mut ui, "cut", "Cutoff", &mut cutoff, 20.0..=20_000.0).el;
    /// let dial = dial.value_text(format!("{cutoff:.0} Hz"));
    /// ```
    pub fn value_text(mut self, s: impl Into<String>) -> Self {
        self.look.text = Some(s.into());
        self
    }
    /// Where the reading sits inside the width its reserve keeps:
    /// [`TextAlign::Start`] by default, `Center` so a short value does not
    /// hug one edge of a long reserve. Applies to the readouts of a
    /// [`knob`], [`slider`] and [`drag_value`]. A knob with a split value
    /// and unit stays centered until this is explicitly set.
    ///
    /// ```
    /// use mui::prelude::*;
    /// let mut ui = Ui::default();
    /// let mut v = 0.5;
    /// let fader = slider(&mut ui, "mix", "Mix", &mut v, 0.0..=1.0)
    ///     .value_reserve("100.00")
    ///     .value_align(TextAlign::Center);
    /// ```
    pub fn value_align(mut self, align: TextAlign) -> Self {
        self.look.value_align = align;
        self.look.value_align_explicit = true;
        self
    }
    /// Hide the visible caption while retaining the input's accessible name.
    pub fn hide_label(mut self) -> Self {
        self.label_visible = false;
        self
    }
    /// No caption and no readout: the name and the reading move to the
    /// tooltip, so a strip of these stays quiet until asked.
    pub fn quiet(mut self) -> Self {
        self.label_visible = false;
        self.quiet = true;
        self
    }
    /// A slider that rises instead of running left to right (a fader). Its
    /// drag follows its shape: taller than wide drags vertically.
    pub fn vertical(mut self) -> Self {
        self.vertical = true;
        self
    }
    /// Override a knob's arc and centered value with a custom instrument accent.
    /// Other controls retain their palette role.
    pub fn accent(mut self, fill: impl Into<Fill>) -> Self {
        self.look.accent = Some(fill.into());
        self
    }
    /// How a knob is painted: [`Knob::smooth`] unless told, or another
    /// preset such as [`Knob::drama`]. Other controls ignore it.
    ///
    /// ```
    /// use mui::prelude::*;
    /// let mut ui = Ui::default();
    /// let mut v = 0.5;
    /// let k = knob(&mut ui, "mix", "Mix", &mut v, 0.0..=1.0).look(Knob::drama());
    /// ```
    pub fn look(mut self, knob: Knob) -> Self {
        self.look.knob = knob;
        self
    }
    /// Draw a [`slider`] flat: a hairline rail, a block thumb standing
    /// across it, and a tick where a bipolar domain's fill starts (a pan's
    /// centre). The drag and keys are the same; only the paint changes.
    ///
    /// ```
    /// use mui::prelude::*;
    /// let mut ui = Ui::default();
    /// let mut pan = 0.0;
    /// let p = slider(&mut ui, "pan", "Pan", &mut pan, Domain::new(-1.0..=1.0).origin(0.0))
    ///     .el
    ///     .quiet()
    ///     .flat();
    /// ```
    pub fn flat(mut self) -> Self {
        self.look.flat = true;
        self
    }
    /// A knob whose face takes a square share of whatever its cell gives
    /// it -- the smaller of the cell's width and the height left under the
    /// caption -- instead of a diameter from [`Control::size`]. The knob
    /// stretches across its parent and grows along it (`.grow(1)`, which a
    /// later `.grow(n)` replaces): give sibling knobs, or the cells holding
    /// them, `.grow(2)` and `.grow(1)` and their dials can come out 2:1,
    /// provided the other axis has enough room: `aspect_fit(1)` limits each
    /// face to a square, so a narrow width can make both dials equal. The face
    /// has no size of its own, so a parent that only hugs its content
    /// leaves it a dot: give the panel a size. The reading and caption
    /// keep the size token's text size: scaling them by the diameter would
    /// need the layout twice. Other controls ignore it.
    ///
    /// ```
    /// use mui::prelude::*;
    /// let mut ui = Ui::default();
    /// let (mut a, mut b) = (0.5, 0.5);
    /// let hero = knob(&mut ui, "depth", "Depth", &mut a, 0.0..=1.0).fill_cell();
    /// let small = knob(&mut ui, "mix", "Mix", &mut b, 0.0..=1.0).fill_cell();
    /// // Enough width lets the dial sizes follow the available cell heights.
    /// let panel = col![hero.grow(2), small.grow(1)].w(300).h(300);
    /// ```
    pub fn fill_cell(mut self) -> Self {
        self.look.fill_cell = true;
        self
    }
    /// A leading icon on a [`button`], in the theme's
    /// [`icon_font`](mui_scene::Theme::icon_font) and the label's ink. Other
    /// controls ignore it.
    ///
    /// ```
    /// use mui::prelude::*;
    /// let mut ui = Ui::default();
    /// let save = button(&mut ui, "save", "Save").icon(sym::SAVE);
    /// ```
    pub fn icon(mut self, symbol: char) -> Self {
        self.look.icons[0] = Some(symbol);
        self
    }
    /// A trailing icon on a [`button`], as [`Control::icon`].
    ///
    /// ```
    /// use mui::prelude::*;
    /// let mut ui = Ui::default();
    /// let next = button(&mut ui, "next", "Next").icon_end(sym::ARROW_FORWARD);
    /// ```
    pub fn icon_end(mut self, symbol: char) -> Self {
        self.look.icons[1] = Some(symbol);
        self
    }
    /// The modulation on this control, drawn on a ring outside a knob's
    /// arc.
    ///
    /// ```
    /// use mui::prelude::*;
    /// let mut ui = Ui::default();
    /// let mut cut = 0.5;
    /// let lfo = Modulation::around(0.5, -0.2, 0.3, Some(0.1), Role::Secondary);
    /// let k = knob(&mut ui, "cut", "Cutoff", &mut cut, 0.0..=1.0).modulation(lfo);
    /// ```
    pub fn modulation(mut self, m: Modulation) -> Self {
        self.look.modulation = Some(m);
        self
    }
    /// Reserve a readout sample in the actual font instead of padding the
    /// displayed value. Supply the widest formatted value the domain can show.
    pub fn value_reserve(self, sample: impl Into<String>) -> Self {
        self.value_reserve_all(vec![sample.into()])
    }
    /// Reserve several candidate strings, measured rather than compared by
    /// character count. A cached `Arc<[String]>` avoids copying the samples.
    pub fn value_reserve_all(mut self, samples: impl Into<Arc<[String]>>) -> Self {
        self.look.reserve = Some(samples.into());
        self
    }
    /// Accessible help on the actual input target, including the inner
    /// lane of a slider or the face of a knob.
    pub fn described(mut self, description: impl Into<Arc<str>>) -> Self {
        self.look.description = Some(description.into());
        self
    }
    /// Minimum keyboard/accessibility step. An explicit step is not reduced
    /// by Shift; Page Up/Down use ten steps. Dragging remains continuous.
    pub fn step(mut self, step: f64) -> Self {
        self.look.step = (step.is_finite() && step > 0.0).then_some(step);
        self
    }
    /// Disable the complete control, including any nested input target.
    pub fn disabled(mut self) -> Self {
        self.disabled = true;
        self
    }
    /// The control's height in pixels, for the knob that has to match a
    /// hardware panel. Overrides [`Control::size`].
    ///
    /// ```
    /// use mui::prelude::*;
    /// let mut ui = Ui::default();
    /// let mut v = 0.5;
    /// let dial = knob(&mut ui, "cut", "Cutoff", &mut v, 0.0..=1.0).px(37);
    /// ```
    pub fn px(mut self, px: impl Px) -> Self {
        self.px = Some(px.px());
        self
    }
    /// The tree, finished.
    ///
    /// ```
    /// use mui::prelude::*;
    /// let mut ui = Ui::default();
    /// let el: El = button(&mut ui, "go", "Go").el.el();
    /// ```
    pub fn el(mut self) -> El {
        // Ten units at M, two either side: daisyUI's -xs .. -xl scale, so one
        // `Theme.control` rescales every control in the tree.
        let steps = match self.size {
            SpacingToken::Xs => 6.0,
            SpacingToken::S => 8.0,
            SpacingToken::M => 10.0,
            SpacingToken::L => 12.0,
            SpacingToken::Xl => 14.0,
        };
        self.look.px = self.px.unwrap_or(self.look.px * steps);
        // The domain's formatter speaks for a reading nobody set, and
        // reserves the formatted ends (a knob's face does not move with it).
        if self.look.text.is_none()
            && let Some(f) = &self.look.format
            && let Kind::Slider(_, _, d, _) | Kind::Knob(_, _, d, ..) | Kind::Drag(_, _, d) =
                &self.kind
        {
            self.look.text = Some(f(d.value));
            if self.look.reserve.is_none() && !matches!(self.kind, Kind::Knob(..)) {
                self.look.reserve = Some(Arc::from([f(d.min), f(d.max)]));
            }
        }
        let look = &self.look;
        let label_visible = self.label_visible;
        let tip = match &self.kind {
            Kind::Slider(_, label, d, _) | Kind::Knob(_, label, d, ..) if self.quiet => {
                let reading = look
                    .text
                    .clone()
                    .unwrap_or_else(|| format!("{:.2}", d.value));
                Some(format!("{label} {reading}"))
            }
            _ => None,
        };
        let el = match self.kind {
            Kind::Slider(id, label, d, field) if self.vertical => {
                fader_el(look, id, label, d, field, label_visible && !self.quiet)
            }
            Kind::Slider(id, label, d, field) if self.quiet => match field {
                Some(field) => col([lane_el(look, id, label, &d, false), *field]).gap(Xs),
                None => lane_el(look, id, label, &d, false),
            },
            Kind::Slider(id, label, d, field) => {
                slider_el(look, id, label, d, field, label_visible)
            }
            Kind::Knob(id, label, d, cache, field) if self.quiet => {
                knob_el(look, id, label, d, &cache, field, false, false)
            }
            Kind::Knob(id, label, d, cache, field) => {
                knob_el(look, id, label, d, &cache, field, label_visible, true)
            }
            Kind::Button(id, label, only) => button_el(look, id, label, only),
            Kind::Toggle(id, label, on) => toggle_el(look, id, label, on),
            Kind::Drag(id, label, d) => drag_el(look, id, label, d),
            Kind::Field(el, width) => look.describe(el.w(width), false),
        };
        let el = match tip {
            Some(tip) => el.tip(tip),
            None => el,
        };
        el.on(State::Disabled, |style| style.opacity(0.45))
            .when(self.disabled, Styled::disabled)
    }
}
impl From<Control> for El {
    fn from(c: Control) -> Self {
        c.el()
    }
}
impl IntoEl for Control {
    fn into_el(self) -> El {
        self.el()
    }
}

/// One arrow-key step of a slider or knob across `range`: a hundredth of it,
/// signed with the range, so an inverted control still steps toward its end.
/// A platform's `Increment` and `Decrement` take the same step.
pub(crate) fn step(range: &RangeInclusive<f64>) -> f64 {
    (range.end() - range.start()) / 100.0
}

/// Step `value` by the focused control's keys: arrows by a hundredth of
/// `range` (a tenth of that with Shift), Page Up and Down by ten steps, Home
/// and End to the ends. The runtime brackets the frame as one edit. Returns
/// whether the value moved. An explicit `.step(..)` on the last presented
/// control overrides the hundredth and is not reduced below that quantum
/// by Shift. Pointer dragging is unaffected.
///
/// The keyboard half of [`slider`] and [`knob`], for a control of your own:
///
/// ```
/// use mui::prelude::*;
/// let ui = Ui::default();
/// let mut trim = 0.0;
/// assert!(!stepped(&ui, "trim", &mut trim, &(-12.0..=12.0)), "nothing is focused");
/// ```
pub fn stepped(ui: &Ui, id: &str, value: &mut f64, range: &RangeInclusive<f64>) -> bool {
    let (lo, hi) = (*range.start(), *range.end());
    if !(lo.is_finite() && hi.is_finite()) {
        return false;
    }
    let before = *value;
    let explicit = ui
        .scene()
        .and_then(|s| s.surface(id))
        .and_then(|s| s.numeric_step);
    let step = explicit.map_or_else(|| step(range), |s| s.copysign(hi - lo));
    for k in ui.keys(id) {
        let one = step
            * if k.mods.shift && explicit.is_none() {
                FINE_DRAG
            } else {
                1.0
            };
        *value = match k.key {
            Key::Right | Key::Up => *value + one,
            Key::Left | Key::Down => *value - one,
            Key::PageUp => *value + 10.0 * step,
            Key::PageDown => *value - 10.0 * step,
            Key::Home => lo,
            Key::End => hi,
            _ => continue,
        }
        .clamp(lo.min(hi), lo.max(hi));
    }
    moved(before, *value)
}

/// Whether a drag or a key moved the value. Bitwise, so a NaN the caller
/// handed in is not a change on every frame.
fn moved(before: f64, after: f64) -> bool {
    before.to_bits() != after.to_bits()
}

fn unit(value: f64, range: &RangeInclusive<f64>) -> f64 {
    // A fixed parameter reports min == max; without this the divide is NaN,
    // `clamp` passes NaN through, and the flex weight fails validation.
    // An inverted range still divides correctly, so only zero bails.
    let span = range.end() - range.start();
    if span == 0.0 {
        return 0.0;
    }
    ((value - range.start()) / span).clamp(0.0, 1.0)
}

/// Default numeric endpoints are both measured, never chosen by byte count.
/// Arbitrary formats can reserve their complete domain with `value_reserve_all`.
/// These are floors: an unanticipated wider value is never clipped.
fn readout(given: Option<String>, value: f64, min: f64, max: f64) -> El {
    if let Some(t) = given {
        // Preserve the existing unwrapped width floor for supplied text;
        // explicit candidates supplement it, including in constrained rows.
        text(t.clone()).reserve(t)
    } else {
        text(format!("{value:.2}")).reserve_all(vec![format!("{min:.2}"), format!("{max:.2}")])
    }
}

/// A focus ring changes paint, not the authored control's layout or hit frame.
fn focus_ring(style: Style) -> Style {
    style.stroke(Role::Ink).stroke_width(2.0)
}

/// The thumb's diameter and the lane it slides in, as shares of the
/// control's height.
const THUMB: f64 = 0.35;
const LANE: f64 = 0.45;

/// Label, readout, and a track whose fill and thumb are flex shares. Returns
/// the control and whether the value changed.
///
/// The whole lane is the target: a press on the track jumps the value there
/// and the drag carries on from it, a full-width drag sweeps the full range,
/// and a focused slider steps with the arrow keys. As on a [`knob`], the
/// drawn thumb follows a tween, so a value set from outside glides. Call `Ui::edit` with the
/// same id to bracket the gesture for a host's automation: `Begin` on the
/// press, `End` on the release.
///
/// ```
/// use mui::prelude::*;
/// let mut ui = Ui::default();
/// let mut gain = 0.5;
/// let fader = slider(&mut ui, "gain", "Gain", &mut gain, 0.0..=1.0);
/// assert!(!fader.changed && gain == 0.5, "no gesture, no change");
/// let fader = fader.size(S);
/// ```
pub fn slider(
    ui: &mut Ui,
    id: impl Into<Id>,
    label: &str,
    value: &mut f64,
    range: impl Into<Domain>,
) -> Response<bool, Control> {
    let id: Id = id.into();
    let domain = range.into();
    let range = domain.range.clone();
    let before = *value;
    let h = ui.state(&id).hover;
    // Last frame's lane, less the thumb riding it, is the travel a
    // full-range drag covers; before the first frame nothing can drag. A
    // lane taller than wide is a fader: it travels, and drags, vertically.
    let (grip, travel, vertical) =
        ui.scene()
            .and_then(|s| s.surface(&id))
            .map_or((0.0, 0.0, false), |s| {
                let (w, h_) = (s.frame.size.width, s.frame.size.height);
                if h_ > w {
                    let grip = w * THUMB / LANE + 2.0 * h;
                    (grip, h_ - grip, true)
                } else {
                    let grip = h_ * THUMB / LANE + 2.0 * h;
                    (grip, w - grip, false)
                }
            });
    if ui.get(&id).pressed
        && travel > 0.0
        && let Some(p) = ui.local(&id)
    {
        // Off the thumb, a press puts the thumb's centre under it. A fader's
        // travel runs bottom to top.
        let along = if vertical { travel + grip - p.y } else { p.x };
        let at = unit(*value, &range) * travel + grip / 2.0;
        if (along - at).abs() > grip / 2.0 {
            let t = ((along - grip / 2.0) / travel).clamp(0.0, 1.0);
            *value = range.start() + t * (range.end() - range.start());
        }
    }
    let field = typed_or_idle(ui, &id, label, value, &domain);
    let dial = Reading::read(ui, &id, value, &domain, travel, vertical);
    Response {
        changed: moved(before, *value),
        el: Control::new(ui, Kind::Slider(id, label.into(), dial, field)).format(&domain),
    }
}

fn slider_el(
    look: &Look,
    id: Id,
    label: Arc<str>,
    d: Reading,
    field: Option<Box<El>>,
    label_visible: bool,
) -> El {
    // While typed into, the field stands where the reading was.
    let reading = match field {
        Some(field) => *field,
        None => d.readout(look).fill(Role::Dim).id(id.field("reading")),
    };
    let header = (if label_visible {
        row([text(label.clone()), spacer(), reading])
    } else {
        row([spacer(), reading])
    })
    .gap(S);
    col([header, lane_el(look, id, label, &d, false)]).gap(Xs)
}

/// A slider standing up: the lane alone, the caption under it when shown. The fill grows from the origin.
fn fader_el(
    look: &Look,
    id: Id,
    label: Arc<str>,
    d: Reading,
    field: Option<Box<El>>,
    label_visible: bool,
) -> El {
    let input = lane_el(look, id, label.clone(), &d, true);
    // No reading to stand in for: a field being typed goes under the lane.
    match field {
        Some(field) => col([input, *field]).gap(Xs).align(Align::Center),
        None if label_visible && !label.is_empty() => col([input, text(label).fill(Role::Dim)])
            .gap(Xs)
            .align(Align::Center),
        None => input,
    }
}

/// A slider's lane alone: the rail, the fill from the origin, the thumb.
/// Lying down it runs left to right; standing up, bottom to top. The thumb
/// sits centred in a box as long as [`slider`]'s grip, so a press lands on
/// the value it shows whatever the look.
fn lane_el(look: &Look, id: Id, label: Arc<str>, d: &Reading, vertical: bool) -> El {
    let (lane, grip) = (look.px * LANE, look.px * THUMB + 2.0 * d.hover);
    let (t, (a, b)) = (d.t, d.span());
    // Along the travel, from its start: three blocks of `cross` across.
    let along = |cross: f64, parts: [(f64, Option<Role>); 3]| {
        let blocks = parts.map(|(grow, fill)| {
            let b = if vertical {
                block(cross, 0.0)
            } else {
                block(0.0, cross)
            }
            .grow(grow);
            match fill {
                Some(role) => b.pill().fill(role),
                None => b,
            }
        });
        let [x, y, z] = blocks;
        if vertical {
            col([z, y, x]).anchor(Align::Center, Align::Stretch)
        } else {
            row([x, y, z]).anchor(Align::Stretch, Align::Center)
        }
    };
    // `thing` sitting at `at` (0..1) along the travel.
    let at = |at: f64, thing: El| {
        if vertical {
            col([spacer().grow(1.0 - at), thing, spacer().grow(at)])
                .anchor(Align::Center, Align::Stretch)
        } else {
            row([spacer().grow(at), thing, spacer().grow(1.0 - at)])
                .anchor(Align::Stretch, Align::Center)
        }
    };
    let sized = |el: El, along: f64, across: f64| {
        if vertical {
            el.w(across).h(along)
        } else {
            el.w(along).h(across)
        }
    };
    let mut layers = Vec::new();
    if look.flat {
        // The rail runs where the thumb's centre travels, half a grip in
        // from each end, so the fill ends under the thumb.
        let inset = |el: El| {
            let pad = || {
                if vertical {
                    block(0.0, grip / 2.0)
                } else {
                    block(grip / 2.0, 0.0)
                }
            };
            if vertical {
                col([pad(), el.grow(1.0), pad()]).anchor(Align::Center, Align::Stretch)
            } else {
                row([pad(), el.grow(1.0), pad()]).anchor(Align::Stretch, Align::Center)
            }
        };
        layers.push(inset(
            // A rail a shade above the panel, as KONTRA drew it, not a
            // sunken field.
            along(2.0, [(a, None), (b - a, Some(look.role)), (1.0 - b, None)])
                .fill(Role::Dim.alpha(0.35)),
        ));
        // Position both markers using the same grip, even though their
        // painted widths along the travel differ.
        let marker = |position, mark| {
            let centred = if vertical {
                col([spacer(), mark, spacer()]).align(Align::Center)
            } else {
                row([spacer(), mark, spacer()]).align(Align::Center)
            };
            at(position, sized(centred, grip, lane))
        };
        if d.origin > 0.0 && d.origin < 1.0 {
            let tick = sized(block(0.0, 0.0).fill(Role::Dim), 1.0, lane * 0.7);
            layers.push(marker(d.origin, tick));
        }
        let thumb = sized(block(0.0, 0.0).fill(look.role).radius(0.), 4.0, lane * 0.9);
        layers.push(marker(t, thumb));
    } else {
        layers.push(
            along(
                look.px * 0.15,
                [(a, None), (b - a, Some(look.role)), (1.0 - b, None)],
            )
            .pill()
            .preset(look.face(Role::Field)),
        );
        layers.push(at(t, block(grip, grip).pill().fill(look.role)));
    }
    let lane_el = stack(layers);
    let lane_el = if vertical {
        lane_el.w(lane).h(pct(100.)).cursor(Cursor::ResizeV)
    } else {
        lane_el.h(lane).cursor(Cursor::ResizeH)
    };
    look.describe(
        lane_el
            .a11y(d.a11y())
            .named(label)
            .focusable()
            .on(State::FocusVisible, focus_ring)
            .id(id),
        true,
    )
}

/// A dial: vertical drag, pointer on a 270° sweep; returns the control and
/// whether the value changed. The pointer follows a
/// tween, so a value set from outside -- a preset, a host automation curve --
/// glides instead of jumping, while a drag still tracks the pointer.
///
/// Bracket it with `Ui::edit`, as for [`slider`]. The
/// diameter comes from the theme and the size token; `.px(72.0)` is the
/// hatch for a dial that has to match a hardware panel.
///
/// ```
/// use mui::prelude::*;
/// let mut ui = Ui::default();
/// let mut cutoff = 0.5;
/// let dial = knob(&mut ui, "cut", "Cutoff", &mut cutoff, 0.0..=1.0);
/// assert!(!dial.changed && cutoff == 0.5, "no gesture, no change");
/// let dial = dial.size(Xl);
/// ```
pub fn knob(
    ui: &mut Ui,
    id: impl Into<Id>,
    label: &str,
    value: &mut f64,
    range: impl Into<Domain>,
) -> Response<bool, Control> {
    let id: Id = id.into();
    let range = range.into();
    let before = *value;
    let field = typed_or_idle(ui, &id, label, value, &range);
    // Unlike a slider's, a dial's travel is a hand distance, not a geometry:
    // the whole face is the target and 120 px of vertical drag sweeps the
    // range whatever the diameter, so a small knob is not a twitchy one.
    let dial = Reading::read(ui, &id, value, &range, 120.0, true);
    // The painted dial is three ~50-point paths; a still knob reuses them.
    let cache = ui.stash::<DialCache>(&id).cloned().unwrap_or_else(|| {
        let c = DialCache::new();
        ui.set_stash(&id, Some(c.clone()));
        c
    });
    Response {
        changed: moved(before, *value),
        el: Control::new(ui, Kind::Knob(id, label.into(), dial, cache, field)).format(&range),
    }
}

/// What a knob's painted dial reads: its sweep, the arc's paint, the
/// whole style and the modulation; the size is the cache's own.
type DialCache = CanvasCache<(u64, u64, Fill, Knob, Option<Modulation>)>;

#[expect(
    clippy::too_many_arguments,
    reason = "the pieces a knob is drawn from, moved out of its `Kind`"
)]
fn knob_el(
    look: &Look,
    id: Id,
    label: Arc<str>,
    d: Reading,
    cache: &DialCache,
    field: Option<Box<El>>,
    label_visible: bool,
    reading_visible: bool,
) -> El {
    // The macro grammar: a quiet inner disc, a 270-degree value arc and the
    // formatted reading in its centre. The square keeps one size across
    // values; units get a second centered line inside the same compact face.
    let size = look.px * 1.25;
    let dial = &look.knob;
    // An accent (`accent`) or the role brightens under the pointer; a
    // style's own arc paint is painted as given.
    let (color, active) = if let (None, Some(arc)) = (&look.accent, &dial.arc) {
        (arc.clone(), arc.clone())
    } else {
        let color = look.accent.clone().unwrap_or_else(|| look.role.into());
        let active = color.map(&look.palette, look.palette.surface(), |color| {
            color.with_alpha((0.82 + d.hover * 0.18) as f32)
        });
        (color, active)
    };
    let (from, to) = d.span();
    let t = d.t;
    let modulation = look.modulation.clone();
    let key = (
        from.to_bits(),
        to.to_bits(),
        active.clone(),
        dial.clone(),
        look.modulation.clone(),
    );
    let style = dial.clone();
    let paint = canvas_keyed(cache, key, move |size| {
        let center = Point::new(size.width * 0.5, size.height * 0.5);
        let r = size.width.min(size.height) * 0.5;
        let (ring, width) = (r * style.ring, r * style.width);
        let arc_at = |from: f64, to: f64, ring: f64| {
            Path::polyline(
                (0..=48).map(|i| {
                    let angle = std::f64::consts::PI * 0.75
                        + std::f64::consts::PI * 1.5 * (from + (to - from) * f64::from(i) / 48.);
                    Point::new(center.x + angle.cos() * ring, center.y + angle.sin() * ring)
                }),
                false,
            )
        };
        let arc = |from: f64, to: f64| arc_at(from, to, ring);
        let face = (ring - width * 0.5 - r * style.inset).max(0.0);
        let disc = Path::polyline(
            (0..64).map(|i| {
                let angle = std::f64::consts::TAU * f64::from(i) / 64.;
                Point::new(center.x + angle.cos() * face, center.y + angle.sin() * face)
            }),
            true,
        );
        let mut draws = vec![
            Draw::stroke(arc(0., 1.), style.track.clone(), width).cap(style.cap),
            Draw::stroke(arc(from, to), active.clone(), width).cap(style.cap),
        ];
        if let Some((blur, fill)) = &style.shadow
            && let Ok(circle) = Path::circle(center, face)
        {
            draws.push(Draw::shadow(circle, r * blur, (0., 0.), fill.clone()));
        }
        draws.push(Draw::fill(disc, style.face.clone()));
        if let Some(p) = &style.pointer {
            let fill = p.fill.clone().unwrap_or_else(|| active.clone());
            // The value's end of the arc, which a bipolar arc may start from.
            let angle = std::f64::consts::PI * (0.75 + 1.5 * t);
            let at = |share: f64| {
                Point::new(
                    center.x + angle.cos() * r * share,
                    center.y + angle.sin() * r * share,
                )
            };
            if p.width > 0.0 {
                let line = Path::polyline([at(p.from), at(p.to)], false);
                draws.push(Draw::stroke(line, fill.clone(), r * p.width).cap(Cap::Round));
            }
            if p.dot > 0.0
                && let Ok(dot) = Path::circle(at(p.to), r * p.dot)
            {
                draws.push(Draw::fill(dot, fill));
            }
        }
        // The modulation rides a thin ring just outside the track: its reach
        // faint, base to live value solid. A track at the edge leaves no
        // room outside, so there it rides just inside.
        if let Some(m) = &modulation {
            let outside = ring + width * 1.5;
            let ring = if outside + width * 0.25 <= r {
                outside
            } else {
                ring - width * 1.5
            };
            let faint = m.fill.clone().faded(0.35);
            draws.push(Draw::stroke(arc_at(m.lo, m.hi, ring), faint, width * 0.5));
            if let Some(live) = m.live {
                draws.push(Draw::stroke(
                    arc_at(m.at.min(live), m.at.max(live), ring),
                    m.fill.clone(),
                    width * 0.5,
                ));
            }
        }
        draws
    })
    .full();
    let reading =
        if let Some((value, unit)) = look.text.as_deref().and_then(|text| text.split_once(' ')) {
            let samples = look.reserve.as_deref().unwrap_or(&[]);
            let align = if look.value_align_explicit {
                look.value_align
            } else {
                TextAlign::Center
            };
            let line = |content: &str, unit_line: bool| {
                text(content)
                    .reserve_all(
                        samples
                            .iter()
                            .map(|sample| {
                                let (v, u) = sample.split_once(' ').unwrap_or((sample, ""));
                                if unit_line {
                                    u.to_owned()
                                } else {
                                    v.to_owned()
                                }
                            })
                            .collect::<Vec<_>>(),
                    )
                    .text_align(align)
                    .text_size(look.px * 0.3)
                    .fill(color.clone())
            };
            col![line(value, false), line(unit, true)]
                .gap(0.)
                .align(match align {
                    TextAlign::Start => Align::Start,
                    TextAlign::End => Align::End,
                    _ => Align::Center,
                })
        } else {
            d.readout(look)
                .text_align(look.value_align)
                .text_size(look.px * 0.3)
                .fill(color.clone())
        }
        .id(id.field("reading"));
    let input = look.describe(
        block(pct(100.), pct(100.))
            .no_fill()
            .a11y(d.a11y())
            .named(label.clone())
            .focusable()
            .cursor(Cursor::ResizeV)
            .on(State::FocusVisible, focus_ring)
            .id(id),
        true,
    );
    // The single input target sits above the painted arc and reading. Clicking
    // the value must begin the same drag as clicking anywhere else on the dial.
    // A field being typed into replaces the reading, above the target so it
    // takes its own clicks.
    let face = if look.fill_cell {
        // Decorations, so nothing but the cell sizes the face: its reading
        // would otherwise be a floor every filling knob shares, and sibling
        // cells' dials would not keep their cells' proportions.
        let layers = match field {
            Some(field) => vec![
                paint.underlay(),
                input,
                col![*field].center().full().overlay(),
            ],
            None if reading_visible => vec![
                paint.underlay(),
                col![reading].center().full().underlay(),
                input,
            ],
            None => vec![paint.underlay(), input],
        };
        stack(layers).aspect_fit(1).grow(1)
    } else {
        match field {
            Some(field) => stack![paint, input, col![*field].center().full()],
            None if reading_visible => stack![paint, col![reading].center().full(), input],
            None => stack![paint, input],
        }
        .square(size)
    };
    let mut content = vec![face];
    if label_visible && !label.is_empty() {
        content.push(text(label).text_size(look.px * 0.3).fill(Role::Dim));
    }
    let knob = col(content).gap(Xs).align(Align::Center);
    // Stretched across its parent and growing along it, not `100%`: a
    // percentage would be every sibling's basis and undo their weights.
    if look.fill_cell {
        knob.grow(1).align_self(Align::Stretch)
    } else {
        knob
    }
}

/// A labelled action. Returns the control and whether it was clicked last
/// frame.
///
/// ```
/// use mui::prelude::*;
/// let mut ui = Ui::default();
/// let save = button(&mut ui, "save", "Save");
/// assert!(!save.changed, "nothing pressed it last frame");
/// let save = save.variant(Variant::Soft).size(S);
/// ```
pub fn button(ui: &mut Ui, id: impl Into<Id>, label: &str) -> Response<bool, Control> {
    let id: Id = id.into();
    Response {
        changed: activated(ui, &id),
        el: Control::new(ui, Kind::Button(id, label.into(), None)),
    }
}

/// An action shown as an icon alone: a round target as tall as a text
/// button of the same size, every [`Variant`] and size a [`button`] takes.
/// The label is not drawn but is required: it is the button's accessible
/// name and its tooltip.
///
/// ```
/// use mui::prelude::*;
/// let mut ui = Ui::default();
/// let lock = icon_button(&mut ui, "lock", "Keep on preset load", sym::LOCK);
/// assert!(!lock.changed, "nothing pressed it last frame");
/// let el = lock.variant(Variant::Ghost).size(S).el.el();
/// let name = el.payload().semantics.as_ref().and_then(|s| s.label.as_deref());
/// assert_eq!(name, Some("Keep on preset load"));
/// assert_eq!(el.payload().extras().tip.as_deref(), Some("Keep on preset load"));
/// ```
pub fn icon_button(
    ui: &mut Ui,
    id: impl Into<Id>,
    label: &str,
    symbol: char,
) -> Response<bool, Control> {
    let id: Id = id.into();
    Response {
        changed: activated(ui, &id),
        el: Control::new(ui, Kind::Button(id, label.into(), Some(symbol))),
    }
}

fn button_el(look: &Look, id: Id, label: Arc<str>, only: Option<char>) -> El {
    let ink = look.ink();
    let face = match only {
        // Half the height, so the glyph scales with the size token.
        Some(symbol) => row([icon(symbol).text_size(look.px * 0.5).fill(ink)])
            .square(look.px)
            .tip(label.clone()),
        None if look.icons == [None; 2] => {
            row([text(label.clone()).fill(ink)]).pad((look.px * 0.4, look.pad_y()))
        }
        None => {
            let [start, end] = look.icons;
            let mut parts = Vec::with_capacity(3);
            parts.extend(start.map(|s| icon(s).fill(ink.clone())));
            parts.push(text(label.clone()).fill(ink.clone()));
            parts.extend(end.map(|s| icon(s).fill(ink.clone())));
            row(parts)
                .gap(Xs)
                .align(Align::Center)
                .pad((look.px * 0.4, look.pad_y()))
        }
    };
    look.describe(
        face.center()
            .pill()
            .preset(look.style())
            .on(State::Hover, look.hover())
            .on(State::FocusVisible, focus_ring)
            .cursor(Cursor::Hand)
            .animate()
            .a11y(A11y::Button)
            .named(label)
            .focusable()
            .id(id),
        false,
    )
}

/// A switch: the knob's side is a flex share it slides between, the click
/// flips it. Returns the control and whether it flipped. The label is the
/// switch's accessible name; the caption beside it is the caller's to draw.
///
/// ```
/// use mui::prelude::*;
/// let mut ui = Ui::default();
/// let mut bypass = false;
/// let sw = toggle(&mut ui, "bypass", "Bypass", &mut bypass);
/// assert!(!sw.changed && !bypass, "nothing clicked it, so it did not flip");
/// let sw = sw.size(Xs);
/// ```
pub fn toggle(
    ui: &mut Ui,
    id: impl Into<Id>,
    label: &str,
    on: &mut bool,
) -> Response<bool, Control> {
    let id: Id = id.into();
    let flipped = activated(ui, &id);
    if flipped {
        *on = !*on;
    }
    Response {
        el: Control::new(ui, Kind::Toggle(id, label.into(), *on)),
        changed: flipped,
    }
}

fn toggle_el(look: &Look, id: Id, label: Arc<str>, on: bool) -> El {
    let t = f64::from(on);
    // A track is as wide as the control's height and a bit over half as
    // tall: 40 x 22 at the default theme's `M`.
    let (w, h) = (look.px, look.px * 0.55);
    look.describe(
        row([
            spacer().grow(t),
            // The knob's side is a flex share; its frame glides between them.
            block(h * 0.73, h * 0.73)
                .pill()
                .fill(Role::Ink)
                .animate_layout(),
            spacer().grow(1.0 - t),
        ])
        .size(w, h)
        .pad(h * 0.14)
        .pill()
        .preset(if on {
            look.style()
        } else {
            look.face(Role::Field)
        })
        .animate()
        .a11y(A11y::Toggle { on })
        .cursor(Cursor::Hand)
        .on(State::FocusVisible, focus_ring)
        .when(!label.is_empty(), |e| e.named(label))
        .focusable()
        .id(id),
        false,
    )
}

/// How far a [`drag_value`] is dragged to sweep its whole range.
const DRAG_TRAVEL: f64 = 200.0;

/// A number to drag: a horizontal drag of 200 px sweeps `range` (Shift is
/// fine), the arrow keys step it, and a double click -- or Enter while it
/// is focused -- turns it into a field to type the number into. Enter or a
/// click away takes what was typed, Escape leaves the value alone. Returns
/// the control and whether the value changed. `.value_text(..)` says it
/// in units, as for a [`slider`]. The label is its accessible name.
///
/// ```
/// use mui::prelude::*;
/// let mut ui = Ui::default();
/// let mut bpm = 120.0;
/// let tempo = drag_value(&mut ui, "bpm", "Tempo", &mut bpm, 20.0..=300.0);
/// assert!(!tempo.changed, "no gesture, no change");
/// ```
pub fn drag_value(
    ui: &mut Ui,
    id: impl Into<Id>,
    label: &str,
    value: &mut f64,
    range: impl Into<Domain>,
) -> Response<bool, Control> {
    let id: Id = id.into();
    let domain = range.into();
    let before = *value;
    let r = ui.get(&id);
    let open = r.double_clicked || r.key_activated;
    match typed(ui, &id, label, value, &domain, open, &id) {
        Typed::Open(el, width) => {
            return Response {
                el: Control::new(ui, Kind::Field(el, width)),
                changed: false,
            };
        }
        // The frame that closed the field neither drags nor steps.
        Typed::Closed => {}
        Typed::Idle => {
            ui.drag(&id, value, domain.range.clone(), DRAG_TRAVEL, false);
            stepped(ui, &id, value, &domain.range);
        }
    }
    // Nothing is drawn sliding, so there is no thumb to tween.
    let dial = Reading::of(*value, &domain.range, 0.0, 0.0);
    Response {
        changed: moved(before, *value),
        el: Control::new(ui, Kind::Drag(id, label.into(), dial)).format(&domain),
    }
}

/// Where typing into a ranged control stands this frame.
enum Typed {
    /// No field.
    Idle,
    /// The field closed this frame: what was typed is taken, or dropped on
    /// Escape.
    Closed,
    /// The field, at the width it opened with.
    Open(Box<El>, f64),
}

/// Typing a value in: `open` turns the control into a field holding the
/// formatted value, selected, as wide as `at` was last frame. Enter or a
/// click away takes what [`Domain::parsed`] reads from it; Escape, or text
/// that does not read, leaves the value alone.
fn typed(
    ui: &mut Ui,
    id: &Id,
    label: &str,
    value: &mut f64,
    domain: &Domain,
    open: bool,
    at: &Id,
) -> Typed {
    let field = id.field("edit");
    // What was typed, and the width the number had when typing started.
    let mut typing = ui.stash::<(String, String, f64)>(&field).cloned();
    // Focus moves to the field only once it is built: this frame's keys --
    // the Enter that opened it -- are not the field's to type.
    let opening = typing.is_none() && open;
    if opening {
        let width = ui
            .scene()
            .and_then(|s| s.surface(at))
            .map_or(ui.theme().control * 16.0, |s| s.frame.size.width);
        let shown = match &domain.format {
            Some(f) => f(*value),
            None => format!("{value}"),
        };
        typing = Some((shown.clone(), shown, width));
    }
    let Some((mut s, initial, width)) = typing else {
        return Typed::Idle;
    };
    let take = |s: &str, value: &mut f64| {
        if s != initial
            && let Some(v) = domain.parsed(s)
        {
            *value = v;
        }
    };
    if opening || ui.focused(&field) {
        let opts = TextOpts {
            blur_on_submit: true,
            ..TextOpts::default()
        };
        let Response { el, changed: e } = text_edit(ui, field.as_str(), &mut s, opts);
        let el = el.named(label);
        if opening {
            ui.set_sel(&field, 0, s.chars().count());
            ui.focus(field.clone());
        }
        if !e.submitted {
            ui.set_stash(&field, Some((s, initial, width)));
            return Typed::Open(Box::new(el), width);
        }
        take(&s, value);
    } else if !ui.shortcuts().iter().any(|k| k.key == Key::Escape) {
        // The focus went elsewhere: a click away keeps what was typed.
        take(&s, value);
    }
    ui.set_stash::<(String, String, f64)>(&field, None);
    Typed::Closed
}

/// Typing into a knob or slider: only with a [`Domain::parse`], opened by
/// Enter on the focused control, or by a double click when the domain has
/// no default for the double click to restore. The field stands in for the
/// reading.
fn typed_or_idle(
    ui: &mut Ui,
    id: &Id,
    label: &str,
    value: &mut f64,
    domain: &Domain,
) -> Option<Box<El>> {
    if domain.parse.is_none() {
        let field = id.field("edit");
        ui.set_stash::<(String, String, f64)>(&field, None);
        if ui.focused(&field) {
            ui.focus(id.clone());
        }
        return None;
    }
    let r = ui.get(id);
    let open = domain.parse.is_some()
        && (r.key_activated || (r.double_clicked && domain.default.is_none()));
    match typed(ui, id, label, value, domain, open, &id.field("reading")) {
        Typed::Open(el, width) => Some(Box::new((*el).w(width))),
        Typed::Idle | Typed::Closed => None,
    }
}

fn drag_el(look: &Look, id: Id, label: Arc<str>, d: Reading) -> El {
    look.describe(
        row([d.readout(look)])
            .pad((look.px * 0.3, look.pad_y()))
            .radius(4.0)
            .preset(look.face(Role::Field))
            .cursor(Cursor::ResizeH)
            .on(State::FocusVisible, focus_ring)
            .a11y(d.a11y())
            .when(!label.is_empty(), |e| e.named(label))
            .focusable()
            .id(id),
        true,
    )
}
