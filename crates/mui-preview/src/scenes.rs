//! The gallery contents: one `PreviewScene` per thing worth looking at.
//! Adding one is a struct and a line in [`all`].

use std::sync::Arc;

use mui::geometry::Path;
use mui::prelude::*;

pub trait PreviewScene {
    fn name(&self) -> &'static str;
    /// One line on what the scene is supposed to prove.
    fn about(&self) -> &'static str;
    /// The specimen. Built every frame like everything else, so a control
    /// that moved is simply visible next frame.
    fn specimen(&mut self, ui: &mut Ui) -> El;
    /// Sidebar rows under the description.
    fn controls(&mut self, _ui: &mut Ui) -> Vec<El> {
        Vec::new()
    }
    /// What the host just put on the clipboard, for a scene that shows it.
    fn clipboard(&mut self, _s: &str) {}
    /// A typed character; `true` if the scene used it.
    fn key(&mut self, _c: char) -> bool {
        false
    }
    /// Geometry drawn over the surface named `key`, in its local space.
    fn overlay(&self) -> Option<(&'static str, Path)> {
        None
    }
}

pub fn all() -> Vec<Box<dyn PreviewScene>> {
    vec![
        Box::new(PillTab),
        Box::new(ConstantThickness),
        Box::new(SegmentedRow),
        Box::new(Widgets::default()),
        Box::new(GlyphAxes::new()),
        Box::new(Scrolling::default()),
        Box::new(Fields::default()),
        Box::new(Tips),
        Box::new(Curve::default()),
        Box::new(Swap::default()),
        Box::new(Images::new()),
        Box::new(Wrapping::default()),
        Box::new(Motion::default()),
        Box::new(Cells),
        Box::new(Select::default()),
        Box::new(Editor),
        Box::new(Effects),
    ]
}

/// A tab welded to a panel: unioned sharp, filleted after, with a shell
/// derived from the merged outline.
pub struct PillTab;
impl PreviewScene for PillTab {
    fn name(&self) -> &'static str {
        "Pill tab + panel"
    }
    fn about(&self) -> &'static str {
        "Boolean union first, fillets second. The inner shell is a parallel offset of the merged outline."
    }
    fn specimen(&mut self, _: &mut Ui) -> El {
        let control = |id: &str| leaf(28.0, 28.0).pill().fill(Role::Primary).id(id);
        let tab = column([control("plus"), control("phase"), control("warp")])
            .gap(10.0)
            .pad(22.0)
            .min_width(92.0)
            .align(Align::Center)
            .id("tab")
            .shell(12.0, Role::Raised);
        column([tab, leaf(520.0, 230.0).id("panel")])
            .align(Align::Start)
            .id("pill")
            .weld(Role::Surface)
    }
}

/// Three shells off one card. Every gap measures the same all the way round:
/// radius 28 − 12 = 16, exactly, not "child radius = parent radius".
pub struct ConstantThickness;
impl PreviewScene for ConstantThickness {
    fn name(&self) -> &'static str {
        "Constant thickness"
    }
    fn about(&self) -> &'static str {
        "Each ring is a parallel offset of the ring outside it."
    }
    fn specimen(&mut self, _: &mut Ui) -> El {
        leaf(320.0, 220.0)
            .radius(28.0)
            .fill(Role::Raised)
            .shell(12.0, Role::Field)
            .shell(12.0, Role::Raised)
            .shell(12.0, Role::Field)
            .id("card")
    }
}

/// Four flush cells welded into one outline: interior edges vanish, the
/// junctions go concave instead of pinching.
pub struct SegmentedRow;
impl PreviewScene for SegmentedRow {
    fn name(&self) -> &'static str {
        "Segmented row"
    }
    fn about(&self) -> &'static str {
        "One .join(): the gap closes, every seam goes square, and the strip's own corner rounds the two ends."
    }
    fn specimen(&mut self, _: &mut Ui) -> El {
        row((0..4).map(|i| {
            leaf(70.0, 44.0)
                .fill(if i == 1 { Role::Primary } else { Role::Raised })
                .id(format!("cell-{i}"))
        }))
        .radius(22.0)
        .join()
        .id("strip")
    }
}

/// The controls a plugin is made of, each a composition of flex shares.
#[derive(Default)]
pub struct Widgets {
    cutoff: f64,
    res: f64,
    gain: f64,
    bypass: bool,
    clicks: usize,
}
impl PreviewScene for Widgets {
    fn name(&self) -> &'static str {
        "Widgets"
    }
    fn about(&self) -> &'static str {
        "Knob, slider, toggle, button. No thumb is placed: it sits where two flex weights put it."
    }
    fn specimen(&mut self, ui: &mut Ui) -> El {
        let (go, clicked) = button(ui, "go", "Trigger");
        self.clicks += usize::from(clicked);
        column([
            row([
                knob(ui, "cutoff", "Cutoff", &mut self.cutoff, 0.0..=1.0).el(),
                knob(ui, "res", "Res", &mut self.res, 0.0..=1.0)
                    .variant(Variant::Soft)
                    .el(),
            ])
            .gap(L)
            .justify(Justify::Center),
            slider(ui, "gain", "Gain", &mut self.gain, -24.0..=6.0).el(),
            row([
                text("Bypass").fill(Role::Dim),
                toggle(ui, "bypass", &mut self.bypass).el(),
                spacer(),
                text(format!("{}×", self.clicks)).fill(Role::Dim),
                go.variant(Variant::Solid).el(),
            ])
            .gap(S)
            .align(Align::Center),
        ])
        .gap(M)
        .pad(L)
        .width(300.0)
        .radius(20.0)
        .fill(Role::Surface)
        .id("widgets")
    }
}

/// A glyph as geometry, re-derived from the font at whatever axis position
/// the sliders are at. Point `MUI_PREVIEW_FONT` at a variable font for axes.
type Axis = (String, f64, f64, f64);

pub struct GlyphAxes {
    font: Vec<u8>,
    source: String,
    glyph: char,
    size: f64,
    /// `(tag, min, max, value)` per axis the face declares.
    axes: Vec<Axis>,
}
impl GlyphAxes {
    const CARD: f64 = 320.0;

    fn axes(font: &[u8]) -> Result<Vec<Axis>, mui_text::Error> {
        mui_text::axes(font).map(|a| {
            a.into_iter()
                .map(|a| (a.tag, a.min.into(), a.max.into(), a.default.into()))
                .collect()
        })
    }
    fn load_font(
        requested: Option<(String, std::io::Result<Vec<u8>>)>,
    ) -> (Vec<u8>, String, Vec<Axis>) {
        let fallback = |source| {
            let font = epaint_default_fonts::HACK_REGULAR.to_vec();
            let axes = Self::axes(&font).expect("bundled Hack font must be valid");
            (font, source, axes)
        };
        match requested {
            Some((path, Ok(font))) => match Self::axes(&font) {
                Ok(axes) => (font, path, axes),
                Err(e) => fallback(format!("{path}: {e} — using Hack")),
            },
            Some((path, Err(e))) => fallback(format!("{path}: {e} — using Hack")),
            None => fallback("Hack (static) — set MUI_PREVIEW_FONT for axes".to_owned()),
        }
    }
    pub fn new() -> Self {
        let requested = std::env::var_os("MUI_PREVIEW_FONT")
            .map(|p| (p.to_string_lossy().into_owned(), std::fs::read(&p)));
        let (font, source, axes) = Self::load_font(requested);
        Self {
            font,
            source,
            glyph: 'a',
            size: 220.0,
            axes,
        }
    }
}
impl PreviewScene for GlyphAxes {
    fn name(&self) -> &'static str {
        "Glyph axes"
    }
    fn about(&self) -> &'static str {
        "A variable-font outline rebuilt as geometry at every axis position. Type to change the glyph."
    }
    fn specimen(&mut self, _: &mut Ui) -> El {
        leaf(Self::CARD, Self::CARD)
            .radius(24.0)
            .fill(Role::Raised)
            .id("glyph-card")
    }
    fn controls(&mut self, ui: &mut Ui) -> Vec<El> {
        let mut rows = vec![
            text(self.source.clone()).fill(Role::Dim),
            slider(ui, "size", "size", &mut self.size, 24.0..=400.0).el(),
        ];
        if self.axes.is_empty() {
            rows.push(text("no variation axes").fill(Role::Dim));
        }
        for (tag, min, max, value) in &mut self.axes {
            rows.push(slider(ui, tag, tag, value, *min..=*max).el());
        }
        rows
    }
    fn key(&mut self, c: char) -> bool {
        let printable = !c.is_control();
        if printable {
            self.glyph = c;
        }
        printable
    }
    fn overlay(&self) -> Option<(&'static str, Path)> {
        let settings: Vec<(&str, f32)> = self
            .axes
            .iter()
            .map(|(t, _, _, v)| (t.as_str(), *v as f32))
            .collect();
        let path = mui_text::glyph_path(&self.font, self.glyph, self.size, &settings, 0.05).ok()?;
        let b = mui::geometry::Bounds::from_points(path.flatten(0.05, 250_000).ok()?.concat())?;
        let centre = mui::geometry::Point::new(
            Self::CARD / 2.0 - (b.min.x + b.max.x) / 2.0,
            Self::CARD / 2.0 - (b.min.y + b.max.y) / 2.0,
        );
        Some(("glyph-card", path.rigid_transform(centre, 0.0).ok()?))
    }
}

/// Forty rows in a fixed box: the wheel moves the innermost surface whose
/// content overflows, and the clip keeps hits off the rows it hides.
pub struct Scrolling {
    gains: Vec<f64>,
}
impl Default for Scrolling {
    fn default() -> Self {
        Self {
            gains: vec![0.0; 40],
        }
    }
}
impl PreviewScene for Scrolling {
    fn name(&self) -> &'static str {
        "Scroll"
    }
    fn about(&self) -> &'static str {
        "A .scroll() column taller than its box, with a .mask() fade over the last rows. The wheel picks the surface under the pointer; a hit outside the clip is not a hit."
    }
    fn specimen(&mut self, ui: &mut Ui) -> El {
        let rows: Vec<El> = self
            .gains
            .iter_mut()
            .enumerate()
            .map(|(i, g)| {
                slider(
                    ui,
                    &format!("band-{i}"),
                    &format!("band {i}"),
                    g,
                    -24.0..=6.0,
                )
                .el()
            })
            .collect();
        // Source-atop, so the ramp paints the surface colour back over the
        // rows at the bottom edge: the list reads as fading out under it.
        let fade = Gradient::linear(
            180.0,
            [
                (0.82, Role::Surface.alpha(0.0)),
                (1.0, Role::Surface.into()),
            ],
        );
        column(rows)
            .gap(S)
            .pad(M)
            .scroll()
            .size(320.0, 340.0)
            .radius(16.0)
            .fill(Role::Surface)
            .mask(fade)
            .id("scroll")
    }
}

/// Two fields and a label reading the first one back.
#[derive(Default)]
pub struct Fields {
    name: String,
    note: String,
}
impl PreviewScene for Fields {
    fn name(&self) -> &'static str {
        "Text"
    }
    fn about(&self) -> &'static str {
        "Click or Tab to focus, type, arrows and Backspace edit. The label mirrors the first field."
    }
    fn specimen(&mut self, ui: &mut Ui) -> El {
        let name = text_input(ui, "field-name", &mut self.name);
        let note = text_input(ui, "field-note", &mut self.note);
        column([
            label("name"),
            name,
            label("note"),
            note,
            text(format!("name = {}", self.name)).fill(Role::Dim),
        ])
        .gap(S)
        .pad(L)
        .width(320.0)
        .radius(16.0)
        .fill(Role::Surface)
        .id("fields")
    }
}

/// Tips come due after half a second of rest; the third button holds a float
/// open instead: an ordinary child of the card, so it reflows with it.
pub struct Tips;
impl PreviewScene for Tips {
    fn name(&self) -> &'static str {
        "Tooltip"
    }
    fn about(&self) -> &'static str {
        "Rest on a button for a tip. Hold the last one and the menu drops in under it -- nothing is placed."
    }
    fn specimen(&mut self, ui: &mut Ui) -> El {
        let (save, _) = button(ui, "tip-save", "Save");
        let (revert, _) = button(ui, "tip-revert", "Revert");
        let (more, _) = button(ui, "tip-more", "Hold for more");
        let (save, revert, more) = (
            save.el(),
            revert.variant(Variant::Outline).el(),
            more.variant(Variant::Ghost).el(),
        );
        let held = ui.get("tip-more").held;
        // The menu is an ordinary child under the button that opened it, not a
        // float at a hand-measured offset: .gap(M) supplies the distance, so a
        // fourth button or a different spacing scale reflows it.
        let card = column([
            save.tip("Write the preset to disk"),
            revert.tip("Throw away every edit since the last save"),
            more,
        ])
        .when(held, |c| {
            c.push(
                column([text("Rename"), text("Duplicate"), text("Delete")])
                    .gap(S)
                    .pad(M)
                    .radius(12.0)
                    .fill(Role::Raised)
                    .id("tips-menu"),
            )
        })
        .gap(M)
        .pad(L)
        .width(260.0)
        .radius(16.0)
        .fill(Role::Surface)
        .id("tips-card");
        overlay([card]).id("tips")
    }
}

/// A response curve drawn as cubics inside a canvas, re-derived every frame
/// from the cutoff below it.
pub struct Curve {
    cutoff: f64,
}
impl Default for Curve {
    fn default() -> Self {
        Self { cutoff: 0.45 }
    }
}
impl PreviewScene for Curve {
    fn name(&self) -> &'static str {
        "Canvas"
    }
    fn about(&self) -> &'static str {
        "canvas(|size| ..) hands the renderer raw Draws. The knee follows the slider; nothing is cached."
    }
    fn specimen(&mut self, ui: &mut Ui) -> El {
        let t = self.cutoff.clamp(0.08, 0.92);
        let plot = canvas(move |size| {
            let (w, h) = (size.width, size.height);
            let x = w * t;
            let path = Path::default()
                .move_to(Point::new(0.0, h * 0.55))
                .cubic_to(
                    Point::new(x * 0.5, h * 0.55),
                    Point::new(x * 0.7, h * 0.14),
                    Point::new(x, h * 0.22),
                )
                .cubic_to(
                    Point::new(x + (w - x) * 0.18, h * 0.30),
                    Point::new(x + (w - x) * 0.35, h * 0.96),
                    Point::new(w, h * 0.98),
                );
            vec![Draw::stroke(path, Role::Primary, 2.5)]
        })
        .size(320.0, 160.0)
        .radius(12.0)
        .fill(Role::Field);
        column([
            plot,
            slider(ui, "knee", "Cutoff", &mut self.cutoff, 0.0..=1.0).el(),
        ])
        .gap(M)
        .pad(L)
        .radius(16.0)
        .fill(Role::Surface)
        .id("curve")
    }
}

/// Three pills. Drag one onto another and they trade labels.
pub struct Swap {
    labels: [String; 3],
}
impl Default for Swap {
    fn default() -> Self {
        Self {
            labels: ["Osc".into(), "Filter".into(), "Amp".into()],
        }
    }
}
impl Swap {
    fn slot(id: &str) -> Option<usize> {
        id.strip_prefix("pill-")?.parse().ok()
    }
}
impl PreviewScene for Swap {
    fn name(&self) -> &'static str {
        "Drag"
    }
    fn about(&self) -> &'static str {
        "A press captures, a release over another target is a drop. The pill under the drag lights up."
    }
    fn specimen(&mut self, ui: &mut Ui) -> El {
        let swap = ui
            .dropped()
            .and_then(|(a, b)| Some((Self::slot(a)?, Self::slot(b)?)));
        if let Some((a, b)) = swap {
            self.labels.swap(a, b);
        }
        let pills = (0..3).map(|i| {
            let id = format!("pill-{i}");
            let target = ui.get(&id).drop_target;
            let (pill, _) = button(ui, &id, &self.labels[i]);
            pill.variant(if target {
                Variant::Solid
            } else {
                Variant::Soft
            })
            .size(L)
            .el()
            .cursor(Cursor::Grab)
        });
        row(pills)
            .gap(M)
            .pad(L)
            .radius(16.0)
            .fill(Role::Surface)
            .id("swap")
    }
}

/// A generated image as a fill, twice, and an icon that arrived as a `d`
/// string. Nothing here decodes a file: `Image::rgba` takes the bytes a
/// decoder would have produced, which is the seam a plugin actually has.
pub struct Images {
    image: Arc<Image>,
}
impl Images {
    /// 64x64 straight RGBA, red across, green down, a blue diagonal.
    fn gradient() -> Arc<Image> {
        const N: u32 = 64;
        let mut rgba = Vec::with_capacity((N * N * 4) as usize);
        for y in 0..N {
            for x in 0..N {
                let (x, y) = (x * 255 / (N - 1), y * 255 / (N - 1));
                rgba.extend([x as u8, y as u8, 255 - (x / 2 + y / 2) as u8, 255]);
            }
        }
        Arc::new(Image::rgba(N, N, rgba).expect("64x64x4 bytes"))
    }
    /// An icon exactly as it arrives in an icon set's `d` attribute, drawn
    /// in the 72 px box it is sized for: arcs, relative commands, a close.
    const ICON: &'static str = "M12 12 h48 v40 a8 8 0 0 1 -8 8 h-32 a8 8 0 0 1 -8 -8 Z";
    pub fn new() -> Self {
        Self {
            image: Self::gradient(),
        }
    }
}
impl PreviewScene for Images {
    fn name(&self) -> &'static str {
        "Image"
    }
    fn about(&self) -> &'static str {
        "The icon is a parsed SVG path. The two pills are Fill::Image, uploaded once into vello_hybrid's atlas and painted by id."
    }
    fn specimen(&mut self, _: &mut Ui) -> El {
        let icon = Path::from_svg_data(Self::ICON).ok();
        let glyph = canvas(move |_| match &icon {
            Some(p) => vec![Draw::fill(p.clone(), Role::Ink)],
            None => Vec::new(),
        })
        .square(72)
        .radius(16.0)
        .fill(Role::Field);
        column([
            row([text("from_svg_data").fill(Role::Dim), spacer(), glyph])
                .gap(M)
                .align(Align::Center),
            leaf(260.0, 64.0)
                .pill()
                .fill(Fill::Image(self.image.clone(), Fit::Cover))
                .id("img-pill"),
            leaf(260.0, 120.0)
                .radius(16.0)
                .fill(Fill::Image(self.image.clone(), Fit::Contain))
                .id("img-card"),
            caption("Cover crops, Contain letterboxes; the same buffer on vello_cpu is the snapshot test").fill(Role::Dim),
        ])
        .gap(M)
        .pad(L)
        .radius(20.0)
        .fill(Role::Surface)
        .id("images")
    }
}

/// A paragraph re-wrapped at whatever width the slider says, and a row that
/// breaks into lines instead of overflowing.
pub struct Wrapping {
    width: f64,
    lines: f64,
}
impl Default for Wrapping {
    fn default() -> Self {
        Self {
            width: 320.0,
            lines: 0.0,
        }
    }
}
impl PreviewScene for Wrapping {
    fn name(&self) -> &'static str {
        "Wrap"
    }
    fn about(&self) -> &'static str {
        "Text wraps to the room its parent has; .lines(n) caps it. A .wrap() row breaks into lines."
    }
    fn specimen(&mut self, _: &mut Ui) -> El {
        const PARA: &str = "A shell is a parallel inset of the outline before it, so every ring \
            measures the same all the way round, and a weld unions sharp frames before it fillets \
            them. Nothing here is placed absolutely.";
        let cap = self.lines.round() as usize;
        let para = text(PARA).when(cap > 0, |t| t.lines(cap));
        let pills = (0..12).map(|i| {
            row([text(format!("band {i}"))])
                .pad_xy(12.0, 6.0)
                .pill()
                .fill(Role::Raised)
        });
        column([
            para,
            row(pills).gap(S).wrap().id("wrap-row"),
            row([caption("12 px").fill(Role::Dim), title("24 px")])
                .gap(S)
                .baseline()
                .id("wrap-baseline"),
        ])
        .gap(M)
        .pad(L)
        .width(self.width)
        .radius(20.0)
        .fill(Role::Surface)
        .id("wrap")
    }
    fn controls(&mut self, ui: &mut Ui) -> Vec<El> {
        vec![
            slider(ui, "wrap-w", "width", &mut self.width, 160.0..=520.0).el(),
            slider(ui, "wrap-n", "lines (0 = all)", &mut self.lines, 0.0..=6.0).el(),
        ]
    }
}

/// Everything that moves without being told a frame number: a fill that
/// springs to its new role, a value that springs to the slider, and the two
/// edges of a gesture.
#[derive(Default)]
pub struct Motion {
    on: [bool; 4],
    sweep: f64,
    gain: f64,
    last: String,
}
impl PreviewScene for Motion {
    fn name(&self) -> &'static str {
        "Motion"
    }
    fn about(&self) -> &'static str {
        "Click a card: .animate() springs the fill to its new role. The pie follows ui.tween; the knob reports Begin/End."
    }
    fn specimen(&mut self, ui: &mut Ui) -> El {
        let cards: Vec<El> = (0..4)
            .map(|i| {
                let id = format!("card-{i}");
                self.on[i] ^= ui.get(&id).clicked;
                leaf(64.0, 64.0)
                    .radius(14.0)
                    .fill(if self.on[i] {
                        Role::Primary
                    } else {
                        Role::Raised
                    })
                    .animate()
                    .cursor(Cursor::Hand)
                    .id(id)
            })
            .collect();
        // The tween is the whole point: the slider jumps, the sweep glides.
        let t = ui.tween("sweep", self.sweep);
        let pie = canvas(move |size| {
            let r = size.width.min(size.height) / 2.0 - 4.0;
            let c = Point::new(size.width / 2.0, size.height / 2.0);
            let arc = (0..=64).map(|i| {
                let a =
                    -std::f64::consts::FRAC_PI_2 + t * std::f64::consts::TAU * f64::from(i) / 64.0;
                Point::new(c.x + r * a.cos(), c.y + r * a.sin())
            });
            let pts: Vec<Point> = std::iter::once(c).chain(arc).collect();
            vec![Draw::fill(Path::polyline(pts, true), Role::Primary)]
        })
        .square(140)
        .radius(70.0)
        .fill(Role::Field);
        if let Some(e) = ui.edit("mot-gain") {
            self.last = match e {
                Edit::Begin => "begin edit",
                Edit::End => "end edit",
            }
            .to_owned();
        }
        column([
            row(cards).gap(M).justify(Justify::Center),
            pie.anchor(Align::Center, Align::Center),
            knob(ui, "mot-gain", "Gain", &mut self.gain, 0.0..=1.0).el(),
            text(if self.last.is_empty() {
                "drag the knob".to_owned()
            } else {
                self.last.clone()
            })
            .fill(Role::Dim),
        ])
        .gap(M)
        .pad(L)
        .align(Align::Center)
        .radius(20.0)
        .fill(Role::Surface)
        .id("motion")
    }
    fn controls(&mut self, ui: &mut Ui) -> Vec<El> {
        vec![slider(ui, "sweep", "sweep", &mut self.sweep, 0.0..=1.0).el()]
    }
}

/// The three things a grid gained: a cell wider than one column, a cell that
/// jumps the queue, and a row that spaces itself.
pub struct Cells;
impl PreviewScene for Cells {
    fn name(&self) -> &'static str {
        "Grid"
    }
    fn about(&self) -> &'static str {
        ".span(n) makes a cell n columns wide, .order(n) moves it without moving its declaration, SpaceEvenly splits the slack."
    }
    fn specimen(&mut self, _: &mut Ui) -> El {
        let cell = |n: &str, fill: Role| {
            row([text(n.to_owned())])
                .size(90.0, 54.0)
                .align(Align::Center)
                .justify(Justify::Center)
                .radius(12.0)
                .fill(fill)
        };
        let cells = vec![
            cell("wide", Role::Primary).span(2),
            cell("b", Role::Raised),
            cell("c", Role::Raised),
            cell("first", Role::Field).order(-1),
            cell("e", Role::Raised),
        ];
        column([
            // min_col makes the declared 3 a ceiling: the same tree is three
            // columns on a wide window and one at 240.
            grid(3, cells).gap(S).min_col(120.0).id("grid"),
            row((0..3).map(|i| {
                leaf(44.0, 24.0)
                    .pill()
                    .fill(Role::Raised)
                    .id(format!("ev-{i}"))
            }))
            .justify(Justify::SpaceEvenly)
            .width(320.0)
            .id("evenly"),
        ])
        .gap(M)
        .pad(L)
        .radius(20.0)
        .fill(Role::Surface)
        .id("cells")
    }
}

/// Selection, copy and paste. The preview is its own clipboard: what a copy
/// hands back on `Frame::clipboard` is what the next paste gets.
pub struct Select {
    value: String,
    clipboard: String,
}
impl Default for Select {
    // Seeded here, not in `specimen`: seeding per frame means emptying the
    // field silently refills it, and the field is the thing under test.
    fn default() -> Self {
        Self {
            value: "select me".to_owned(),
            clipboard: String::new(),
        }
    }
}
impl PreviewScene for Select {
    fn name(&self) -> &'static str {
        "Select"
    }
    fn about(&self) -> &'static str {
        "Shift+arrows or a double click selects, ctrl+C/X/V move it through Frame.clipboard. The label is that clipboard."
    }
    fn clipboard(&mut self, s: &str) {
        self.clipboard = s.to_owned();
    }
    fn specimen(&mut self, ui: &mut Ui) -> El {
        let field = text_input(ui, "sel-field", &mut self.value);
        column([
            label("field"),
            field,
            text(format!("clipboard = {}", self.clipboard)).fill(Role::Dim),
        ])
        .gap(S)
        .pad(L)
        .width(320.0)
        .radius(16.0)
        .fill(Role::Surface)
        .id("select")
    }
}

/// A plugin editor that survives every window shape a host can drag it to.
/// Nothing in it is placed: the rail is one `clamp()`, the knob bank is one
/// `min_col` grid, the chips wrap, and the root is `pct(100)` of the stage.
pub struct Editor;
impl PreviewScene for Editor {
    fn name(&self) -> &'static str {
        "Responsive editor"
    }
    fn about(&self) -> &'static str {
        "Resize the window thin and wide: clamp() sizes the rail, min_col drops grid columns, the chips wrap. No breakpoints."
    }
    fn specimen(&mut self, _: &mut Ui) -> El {
        editor()
    }
}

/// The tree [`Editor`] shows, built by a free function so the test below can
/// resolve the same one at the shapes a plugin window is dragged to.
pub fn editor() -> El {
    // A section tab is fluid between two stops: three of them fill a 240 px
    // window and stop growing at 120 in a 2000 px one.
    let tab = |n: &str| {
        chip(n)
            .justify(Justify::Center)
            .w(clamp(64.0, 18.0, 120.0))
            .pad_xy(0.0, 8.0)
            .radius(8.0)
            .on(State::Hover, |s| s.stroke(Ink.alpha(0.12)))
            .id(format!("tab-{n}"))
    };
    let knob = |i: usize| {
        tile(col![
            leaf(40.0, 40.0).pill().fill(Primary).id(format!("k{i}")),
            caption(["cut", "res", "drv", "mix"][i]),
        ])
    };
    // The header keeps the widest version of itself that still fits: no
    // width branch in the scene, and no second build pass.
    // No spacer in a candidate: a flex base of 0 measures small, so a row
    // holding one always "fits" and the widest would always win.
    let head = fits![
        row![title("Kurv"), caption("v1.0 -- four operators")]
            .gap(M)
            .baseline(),
        row![title("Kurv"), caption("v1.0")].gap(S).baseline(),
        title("Kurv"),
    ]
    .id("head");
    col![
        head,
        row(["Osc", "Filter", "Env"].map(tab))
            .gap(S)
            .wrap()
            .id("tabs"),
        grid(4, (0..4).map(knob)).gap(S).min_col(120.0).id("bank"),
        row(["A", "B", "C", "D"].map(|n| chip(n).id(format!("chip-{n}"))))
            .gap(S)
            .wrap()
            .id("chips"),
    ]
    .gap(M)
    .pad(M)
    .full()
    .preset(&panel())
    .clip()
    .id("editor")
}

/// Paint the renderer could always do and the DSL could not say: a conic
/// arc, a radial glow, stacked shadows, and glass without a backdrop blur.
pub struct Effects;
impl PreviewScene for Effects {
    fn name(&self) -> &'static str {
        "Gradients and shadows"
    }
    fn about(&self) -> &'static str {
        "Conic and radial ramps, a two-shadow key, and glass as a translucent fill plus a bright edge plus an inner floor. No filter layer anywhere."
    }
    fn specimen(&mut self, _: &mut Ui) -> El {
        effects()
    }
}

/// The tree [`Effects`] shows, as a free function so the snapshot test below
/// paints the same one the gallery does.
pub fn effects() -> El {
    // A knob arc is a conic gradient and nothing else: no path, no per-frame
    // geometry, and the value is the stop position.
    let knob = leaf(96.0, 96.0)
        .pill()
        .fill(Gradient::conic(
            -135.0,
            [(0.0, Primary), (0.7, Primary), (0.7, Field), (1.0, Field)],
        ))
        .id("knob");
    let glow = leaf(96.0, 96.0)
        .pill()
        .fill(Gradient::radial(
            (0.35, 0.3),
            0.7,
            [(0.0, Primary), (1.0, Surface)],
        ))
        .id("glow");
    let key = leaf(96.0, 96.0)
        .radius(12.0)
        .fill(Field)
        .elevation(Elevation::Raised)
        .id("key");
    let pane = leaf(96.0, 96.0).preset(&glass()).id("glass");
    row![knob, glow, key, pane]
        .gap(L)
        .pad(L)
        .fill(Surface)
        .id("effects")
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "cpu")]
    use super::effects;
    use super::{editor, GlyphAxes};
    use mui::core::Frame as LayoutFrame;
    use mui::prelude::*;

    #[test]
    fn malformed_readable_font_falls_back_with_parse_diagnostic() {
        let (font, source, _) =
            GlyphAxes::load_font(Some(("broken.ttf".to_owned(), Ok(Vec::new()))));
        assert_eq!(font.as_slice(), epaint_default_fonts::HACK_REGULAR);
        assert!(source.starts_with("broken.ttf: ") && source.ends_with(" — using Hack"));
    }

    /// The effects scene on the CPU. Every claim here is one a flat fill
    /// cannot make: a ramp changes colour across a box, and a shadow darkens
    /// the surface outside one.
    #[cfg(feature = "cpu")]
    #[test]
    fn the_effects_scene_paints_ramps_and_shadows_a_flat_fill_cannot() {
        use mui::vello::vello_cpu::{Pixmap, RenderContext, Resources};
        let (w, h) = (620u16, 160u16);
        let scene =
            resolve_scene(&SceneSpec::new(effects()).offered(Size::new(w.into(), h.into())))
                .unwrap();
        let mut ctx = RenderContext::new(w, h);
        let mut res = Resources::default();
        mui::vello::paint(
            &mut mui::vello::Cpu {
                ctx: &mut ctx,
                resources: &mut res,
            },
            &scene,
            mui::vello::kurbo::Affine::IDENTITY,
        )
        .unwrap();
        ctx.flush();
        let mut pix = Pixmap::new(w, h);
        ctx.render(&mut pix, &mut res);
        let at = |x: f64, y: f64| {
            let p = pix.data()[(y as usize) * usize::from(w) + x as usize];
            (p.r, p.g, p.b)
        };
        let frame = |id: &str| scene.surface(id).unwrap().frame;
        // A 5x5 grid inside a box: one colour is a flat fill, several is a ramp.
        let shades = |id: &str| {
            let f = frame(id);
            let mut seen = Vec::new();
            for i in 1..6 {
                for j in 1..6 {
                    let c = at(
                        f.x + f.size.width * f64::from(i) / 6.0,
                        f.y + f.size.height * f64::from(j) / 6.0,
                    );
                    if !seen.contains(&c) {
                        seen.push(c);
                    }
                }
            }
            seen.len()
        };
        // The arc's stops are hard, so two colours is the whole ramp; the
        // glow's are not, so it has as many as the grid can find.
        assert!(shades("knob") > 1, "the conic arc painted flat");
        assert!(shades("glow") > 2, "the radial glow painted flat");
        // Glass is a fill plus an edge plus a floor: the edge is brighter
        // than the middle and the floor is darker, which no fill can be.
        let g = frame("glass");
        let (mx, my) = (g.x + g.size.width / 2.0, g.y + g.size.height / 2.0);
        let (edge, mid, floor) = (at(mx, g.y + 1.0), at(mx, my), at(mx, g.bottom() - 2.0));
        assert!(edge.0 > mid.0, "no bright top edge: {edge:?} vs {mid:?}");
        assert!(floor.0 < mid.0, "no inner floor: {floor:?} vs {mid:?}");
        // The key's contact and ambient shadows darken the surface under it.
        // The knob, the same box in the same row with no shadow, is the
        // control: same panel, same y, no darkening.
        let (k, n) = (frame("key"), frame("knob"));
        let under = at(k.x + k.size.width / 2.0, k.bottom() + 4.0);
        let bare = at(n.x + n.size.width / 2.0, n.bottom() + 4.0);
        assert!(
            under.0 < bare.0,
            "no shadow under the key: {under:?} vs {bare:?}"
        );
    }

    /// The three window shapes a plugin editor is actually dragged to. No
    /// child may leave the box its parent gave it, at any of them.
    #[test]
    fn the_editor_reflows_from_240x600_to_2000x300_without_overflowing() {
        fn walk(n: &El, frames: &[LayoutFrame], i: &mut usize, parent: Option<LayoutFrame>) {
            let f = frames[*i];
            *i += 1;
            assert!(
                [f.x, f.y, f.size.width, f.size.height]
                    .iter()
                    .all(|v| v.is_finite()),
                "non-finite frame {f:?}"
            );
            if let Some(p) = parent {
                assert!(
                    f.x >= p.x - 1e-6
                        && f.y >= p.y - 1e-6
                        && f.right() <= p.right() + 1e-6
                        && f.bottom() <= p.bottom() + 1e-6,
                    "{f:?} leaves its parent {p:?}"
                );
            }
            for c in n.children() {
                walk(c, frames, i, Some(f));
            }
        }
        let cols = |w: f64, h: f64| {
            let tree = editor();
            let mut spec = SceneSpec::new(editor()).offered(Size::new(w, h));
            spec.font = Some(std::sync::Arc::from(epaint_default_fonts::HACK_REGULAR));
            let scene = resolve_scene(&spec).unwrap();
            let frames = scene.layout.all();
            walk(&tree, frames, &mut 0, None);
            let top = scene.layout.frame("k0").unwrap().y;
            (0..4)
                .filter(|i| scene.layout.frame(&format!("k{i}")).unwrap().y == top)
                .count()
        };
        // One clamp and one min_col are the whole reflow: four knobs across a
        // wide window, stacked in a thin one.
        assert_eq!(
            (cols(240., 600.), cols(800., 500.), cols(2000., 300.)),
            (1, 4, 4)
        );
    }
}
