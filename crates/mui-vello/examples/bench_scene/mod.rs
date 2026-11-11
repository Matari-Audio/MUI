//! The synth-editor scene the benches and coverage reports share: 40 knobs,
//! 8 sliders, 200 labels, a clipped list, curves, paragraphs, image pills
//! and spring-driven cards.

use std::sync::Arc;

use mui::Ui;
use mui::prelude::*;

/// `MUI_BENCH_SCENE=vectors` swaps the editor for a grid of moving curves.
pub fn vectors() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var("MUI_BENCH_SCENE").as_deref() == Ok("vectors"))
}

// ---------------------------------------------------------------- the scene

/// The knobs, sliders and labels an editor actually carries. One `f64` per
/// control is the whole application state; the tree is rebuilt from it.
pub struct App {
    pub knobs: Vec<f64>,
    pub sliders: Vec<f64>,
    /// Decoded once and kept, as a host keeps what it decoded: a renderer
    /// keys its upload on this `Arc`, and so does the retained paint list.
    pub swatch: Arc<Image>,
}

impl App {
    pub fn new() -> Self {
        Self {
            knobs: (0..40).map(|i| f64::from(i) / 40.0).collect(),
            sliders: (0..8).map(|i| f64::from(i) / 8.0).collect(),
            swatch: swatch(),
        }
    }
}

/// A four-pixel texture, standing in for whatever a host decodes. One buffer,
/// shared by every pill: each renderer's `Cache` keys on the `Arc`, so four
/// pills cost one upload.
pub fn swatch() -> Arc<Image> {
    let px: Vec<u8> = [
        [220, 60, 60, 255],
        [60, 200, 120, 255],
        [70, 110, 240, 255],
        [240, 210, 80, 255],
    ]
    .concat();
    Arc::new(Image::rgba(2, 2, px).expect("2x2 rgba"))
}

/// A response curve: 50 cubics through the node's own frame.
fn curve(seed: f64) -> El {
    canvas(move |size: Size| {
        let step = size.width / 50.0;
        let mut p = Path::default().move_to(Point::new(0.0, size.height * 0.5));
        for i in 0..50 {
            let (x0, x1) = (step * f64::from(i), step * f64::from(i + 1));
            let y = size.height * (0.5 + 0.4 * (f64::from(i) * 0.6 + seed).sin());
            p = p.cubic_to(
                Point::new(x0 + step / 3.0, y),
                Point::new(x1 - step / 3.0, y),
                Point::new(x1, size.height * 0.5),
            );
        }
        vec![Draw::stroke(p, Role::Primary, 2.0)]
    })
    .h(90.0)
}

const BLURB: &str = "the filter tracks the key and the envelope follows it, \
which is what the second knob is for when the resonance is high";

/// 40 knobs, 8 sliders, 200 labels, a clipped 60-row list, two curves, three
/// floats, 20 wrapped paragraphs, 4 image-filled pills and 6 cards whose fill
/// is spring-driven -- roughly what a synth editor puts on screen at once.
pub fn editor(ui: &mut Ui, app: &mut App) -> El {
    if vectors() {
        return grid(
            8,
            (0..96).map(|i| curve(f64::from(i) * 0.13 + app.knobs[5] * 2.0).h(50.0)),
        )
        .gap(2.0)
        .pad(4.0)
        .fill(Role::Surface);
    }
    let knobs: Vec<El> = (0..40)
        .map(|i| {
            knob(ui, format!("k{i}"), "cut", &mut app.knobs[i], 0.0..=1.0)
                .el
                .size(S)
                .el()
        })
        .collect();
    let sliders: Vec<El> = (0..8)
        .map(|i| {
            slider(
                ui,
                format!("s{i}"),
                "amount",
                &mut app.sliders[i],
                0.0..=1.0,
            )
            .el
            .into_el()
        })
        .collect();
    let labels: Vec<El> = (0..200).map(|i| caption(format!("p{i:03}"))).collect();
    let list: Vec<El> = (0..60)
        .map(|i| {
            row![body(format!("step {i:02}")), spacer(), caption("0.00")]
                .pad(Xs)
                .id(format!("row{i}"))
        })
        .collect();

    // 20 wrapped paragraphs: each is narrower than its text, so the scene
    // resolver breaks lines and solves the layout a second time.
    let blurbs: Vec<El> = (0..20)
        .map(|i| body(format!("{i}. {BLURB}")).w(150.0).lines(4))
        .collect();
    // 4 image-filled pills, all sharing one 2x2 buffer.
    let img = app.swatch.clone();
    let pills: Vec<El> = [Fit::Cover, Fit::Contain, Fit::Fill, Fit::Cover]
        .into_iter()
        .map(|fit| {
            block(120.0, 40.0)
                .pill()
                .fill(Fill::Image(img.clone(), fit))
        })
        .collect();
    // 6 cards carrying a transition, so every frame walks their spring
    // channels whether or not the colour moved.
    let cards: Vec<El> = (0..6)
        .map(|i| {
            col![title(format!("card {i}")), caption("ready")]
                .gap(Xs)
                .pad(M)
                .shell(8.0, Role::Raised)
                .animate()
                .id(format!("card{i}"))
        })
        .collect();

    let body = col![
        row![title("Synth"), spacer(), caption("48 kHz")].pad(S),
        grid(8, knobs).gap(M).pad(M).shell(10.0, Role::Raised),
        col(sliders).gap(S).pad(M),
        grid(20, labels).gap(Xs).pad(S),
        col(list).gap(2.0).scroll().h(240.0).pad(Xs),
        row![curve(0.0), curve(1.3)].gap(M).pad(M),
        grid(5, blurbs).gap(S).pad(S),
        row(pills).gap(S).pad(S),
        grid(6, cards).gap(S).pad(S),
    ]
    .union(Role::Surface);

    // Three floats: a tooltip, a readout and a menu, painted over everything.
    stack![
        body,
        caption("tip")
            .pad(S)
            .fill(Role::Raised)
            .float()
            .offset(200.0, 120.0),
        caption("-6.0 dB")
            .pad(S)
            .fill(Role::Raised)
            .float()
            .offset(900.0, 60.0),
        col![caption("open"), caption("save")]
            .pad(S)
            .fill(Role::Raised)
            .float()
            .offset(600.0, 400.0),
    ]
}
