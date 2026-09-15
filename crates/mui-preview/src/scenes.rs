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
        "Four flush cells unioned into one strip; shared edges are emitted zero times."
    }
    fn specimen(&mut self, _: &mut Ui) -> El {
        row((0..4).map(|i| leaf(70.0, 44.0 + 12.0 * f64::from(i % 2)).id(format!("cell-{i}"))))
            .radius(22.0)
            .id("strip")
            .weld(Role::Raised)
            .shell(8.0, Role::Field)
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
                knob(ui, "cutoff", "Cutoff", &mut self.cutoff, 0.0..=1.0, 72.0),
                knob(ui, "res", "Res", &mut self.res, 0.0..=1.0, 72.0),
            ])
            .gap(L)
            .justify(Justify::Center),
            slider(ui, "gain", "Gain", &mut self.gain, -24.0..=6.0),
            row([
                text("Bypass").fill(Role::Dim),
                toggle(ui, "bypass", &mut self.bypass),
                spacer(),
                text(format!("{}×", self.clicks)).fill(Role::Dim),
                go,
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
            slider(ui, "size", "size", &mut self.size, 24.0..=400.0),
        ];
        if self.axes.is_empty() {
            rows.push(text("no variation axes").fill(Role::Dim));
        }
        for (tag, min, max, value) in &mut self.axes {
            rows.push(slider(ui, tag, tag, value, *min..=*max));
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
        "A .scroll() column taller than its box. The wheel picks the surface under the pointer; a hit outside the clip is not a hit."
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
            })
            .collect();
        column(rows)
            .gap(S)
            .pad(M)
            .scroll()
            .size(320.0, 340.0)
            .radius(16.0)
            .fill(Role::Surface)
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
/// open instead, which is the same overlay a menu would use.
pub struct Tips;
impl PreviewScene for Tips {
    fn name(&self) -> &'static str {
        "Tooltip"
    }
    fn about(&self) -> &'static str {
        "Rest on a button for a tip. Hold the last one for a float anchored under it."
    }
    fn specimen(&mut self, ui: &mut Ui) -> El {
        let (save, _) = button(ui, "tip-save", "Save");
        let (revert, _) = button(ui, "tip-revert", "Revert");
        let (more, _) = button(ui, "tip-more", "Hold for more");
        let held = ui.get("tip-more").held;
        let card = column([
            save.tip("Write the preset to disk"),
            revert.tip("Throw away every edit since the last save"),
            more,
        ])
        .gap(M)
        .pad(L)
        .width(260.0)
        .radius(16.0)
        .fill(Role::Surface)
        .id("tips-card");
        let mut layers = vec![card];
        if held {
            layers.push(
                column([text("Rename"), text("Duplicate"), text("Delete")])
                    .gap(S)
                    .pad(M)
                    .width(160.0)
                    .radius(12.0)
                    .fill(Role::Raised)
                    .anchor(Align::Center, Align::End)
                    .offset(0.0, 96.0)
                    .float()
                    .id("tips-menu"),
            );
        }
        overlay(layers).id("tips")
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
            slider(ui, "knee", "Cutoff", &mut self.cutoff, 0.0..=1.0),
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
            row([text(self.labels[i].clone())])
                .pad_xy(18.0, 12.0)
                .pill()
                .fill(if target { Role::Primary } else { Role::Raised })
                .cursor(Cursor::Grab)
                .id(id)
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
        "An RGBA buffer as a fill: Cover crops to the pill, Contain letterboxes in the card. The icon is a parsed SVG path."
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
            caption("Contain leaves the letterbox transparent").fill(Role::Dim),
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
            slider(ui, "wrap-w", "width", &mut self.width, 160.0..=520.0),
            slider(ui, "wrap-n", "lines (0 = all)", &mut self.lines, 0.0..=6.0),
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
            knob(ui, "mot-gain", "Gain", &mut self.gain, 0.0..=1.0, 72.0),
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
        vec![slider(ui, "sweep", "sweep", &mut self.sweep, 0.0..=1.0)]
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
            grid(3, cells).gap(S).id("grid"),
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
#[derive(Default)]
pub struct Select {
    value: String,
    clipboard: String,
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
        if self.value.is_empty() {
            self.value = "select me".to_owned();
        }
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

#[cfg(test)]
mod tests {
    use super::GlyphAxes;

    #[test]
    fn malformed_readable_font_falls_back_with_parse_diagnostic() {
        let (font, source, _) =
            GlyphAxes::load_font(Some(("broken.ttf".to_owned(), Ok(Vec::new()))));
        assert_eq!(font.as_slice(), epaint_default_fonts::HACK_REGULAR);
        assert!(source.starts_with("broken.ttf: ") && source.ends_with(" — using Hack"));
    }
}
