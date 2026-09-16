//! The bin display's claims: a drag paints a continuous run of bins, Shift
//! refines it, a secondary click resets one, the arrows select and nudge,
//! a thousand partials still cost one bar per pixel column, and a log axis
//! puts an octave where an octave belongs. Here rather than beside the code
//! because they need a [`Host`], and the only one is the runtime in `mui`.
use mui::input::{Button, Buttons, Key, KeyPress, Mods};
use mui::prelude::*;
use mui::scene::Content;

const W: f64 = 200.0;
const H: f64 = 100.0;
/// Sixteen bins across 200 px: 12.5 px each, so a bin centre is far enough
/// from its neighbour's that a rounding slip would fail loudly.
const N: usize = 16;

fn saw(n: usize) -> Vec<f32> {
    (1..=n).map(|h| 1.0 / h as f32).collect()
}
/// The x of bin `i`'s centre on a linear axis, and the y of a level.
fn at(i: usize, level: f64) -> Point {
    Point::new((i as f64 + 0.5) / N as f64 * W, H * (1.0 - level))
}
fn pointer(p: Point, button: Option<Button>, shift: bool) -> PointerInput {
    PointerInput {
        pos: Some(p),
        buttons: button.map_or_else(Buttons::default, |b| Buttons::default().set(b, true)),
        mods: Mods {
            shift,
            ..Mods::default()
        },
    }
}
/// One frame of the display at 200x100. The widget reads *last* frame's
/// gesture, so every step here is one call.
fn frame(ui: &mut Ui, b: &Bins, input: impl Into<Input>) -> Option<BinEdit> {
    let (el, edit) = bins(ui, "spec", b);
    ui.frame(el.size(W, H), Some(Size::new(W, H)), input, 0.016)
        .expect("resolves");
    edit
}
/// Settle the hit map, then press at `from`: the state a paint test starts in.
fn press(ui: &mut Ui, b: &Bins, from: Point, shift: bool) {
    for _ in 0..2 {
        frame(ui, b, pointer(from, None, shift));
    }
    frame(ui, b, pointer(from, Some(Button::Primary), shift));
    frame(ui, b, pointer(from, Some(Button::Primary), shift));
}

/// A fast drag leaves no gaps: every bin between the last sample and this
/// one is painted, at the level the pointer had as it crossed.
#[test]
fn a_drag_paints_every_bin_it_crossed_with_interpolated_levels() {
    let levels = saw(N);
    let b = Bins {
        authored: &levels,
        ..Bins::default()
    };
    let mut ui = Ui::new(Theme::DEFAULT);
    press(&mut ui, &b, at(3, 0.25), false);
    // One frame's jump of six bins and half the height.
    frame(
        &mut ui,
        &b,
        pointer(at(9, 0.75), Some(Button::Primary), false),
    );
    let edit = frame(
        &mut ui,
        &b,
        pointer(at(9, 0.75), Some(Button::Primary), false),
    );
    let Some(BinEdit::Paint(painted)) = edit else {
        panic!("a drag paints: {edit:?}");
    };
    assert_eq!(
        painted.iter().map(|(i, _)| *i).collect::<Vec<_>>(),
        (3..=9).collect::<Vec<_>>(),
        "no bin skipped"
    );
    // Ends at the two samples, and rises monotonically between them.
    assert!((painted[0].1 - 0.25).abs() < 1e-3, "{painted:?}");
    assert!((painted[6].1 - 0.75).abs() < 1e-3, "{painted:?}");
    assert!(
        painted.windows(2).all(|w| w[1].1 > w[0].1),
        "interpolated: {painted:?}"
    );
}

/// Shift refines from the level the press landed on, so the same travel
/// moves a fraction as far -- never more than half.
#[test]
fn shift_paints_fine_from_the_press_level() {
    let levels = saw(N);
    let b = Bins {
        authored: &levels,
        ..Bins::default()
    };
    let travel = |shift: bool| {
        let mut ui = Ui::new(Theme::DEFAULT);
        press(&mut ui, &b, at(5, 0.2), shift);
        let to = || pointer(at(5, 0.9), Some(Button::Primary), shift);
        frame(&mut ui, &b, to());
        match frame(&mut ui, &b, to()) {
            Some(BinEdit::Paint(p)) => f64::from(p[0].1),
            e => panic!("a drag paints: {e:?}"),
        }
    };
    let (coarse, fine) = (travel(false), travel(true));
    assert!((coarse - 0.9).abs() < 1e-3, "absolute: {coarse}");
    // 0.2 + (0.9 - 0.2) * FINE_DRAG, which is well under half the move.
    assert!(
        fine < 0.2 + (coarse - 0.2) * 0.5,
        "fine is at most half: {fine} vs {coarse}"
    );
    assert!(fine > 0.2, "and still in the direction of travel: {fine}");
}

/// The secondary button resets the bin under the pointer and nothing else.
#[test]
fn a_secondary_click_resets_the_bin_under_the_pointer() {
    let levels = saw(N);
    let b = Bins {
        authored: &levels,
        ..Bins::default()
    };
    let mut ui = Ui::new(Theme::DEFAULT);
    let p = at(11, 0.5);
    for _ in 0..2 {
        frame(&mut ui, &b, pointer(p, None, false));
    }
    frame(&mut ui, &b, pointer(p, Some(Button::Secondary), false));
    assert_eq!(
        frame(&mut ui, &b, pointer(p, Some(Button::Secondary), false)),
        None,
        "held by the secondary button is not a paint"
    );
    // The click is reported on release, and read the frame after that.
    frame(&mut ui, &b, pointer(p, None, false));
    assert_eq!(
        frame(&mut ui, &b, pointer(p, None, false)),
        Some(BinEdit::Reset(11))
    );
}

/// With the focus here, Left/Right move the selection, Home/End jump, and
/// Up/Down nudge the selected bin's level -- Shift finer.
#[test]
fn the_arrows_select_and_nudge_the_selected_bin() {
    let levels = saw(N);
    let b = Bins {
        authored: &levels,
        selected: Some(4),
        ..Bins::default()
    };
    let key = |k: Key, shift: bool| Input {
        keys: vec![KeyPress {
            key: k,
            mods: Mods {
                shift,
                ..Mods::default()
            },
        }],
        ..Input::default()
    };
    let mut ui = Ui::new(Theme::DEFAULT);
    frame(&mut ui, &b, PointerInput::default());
    ui.focus("spec");
    assert_eq!(
        frame(&mut ui, &b, key(Key::Right, false)),
        None,
        "next frame"
    );
    assert_eq!(
        frame(&mut ui, &b, key(Key::Left, false)),
        Some(BinEdit::Select(5)),
        "Right moved it before Left was read"
    );
    assert_eq!(
        frame(&mut ui, &b, key(Key::End, false)),
        Some(BinEdit::Select(3))
    );
    assert_eq!(
        frame(&mut ui, &b, key(Key::Up, false)),
        Some(BinEdit::Select(N - 1))
    );
    // Bin 4 of a 1/n saw is 0.2; one nudge is a hundredth, Shift a tenth of that.
    let nudged = |e: Option<BinEdit>| match e {
        Some(BinEdit::Paint(p)) => {
            assert_eq!(p.len(), 1, "one bin");
            assert_eq!(p[0].0, 4, "the selected one");
            f64::from(p[0].1)
        }
        other => panic!("a nudge paints: {other:?}"),
    };
    let base = f64::from(levels[4]);
    let up = nudged(frame(&mut ui, &b, key(Key::Down, false)));
    let down = nudged(frame(&mut ui, &b, key(Key::Up, true)));
    let fine = nudged(frame(&mut ui, &b, PointerInput::default()));
    assert!((up - (base + 0.01)).abs() < 1e-6, "{up} vs {base}");
    assert!((down - (base - 0.01)).abs() < 1e-6, "{down} vs {base}");
    assert!(
        (fine - (base + 0.001)).abs() < 1e-6,
        "Shift is finer: {fine}"
    );
}

/// A thousand partials across two hundred pixels is two hundred bars: the
/// display coalesces per pixel column, so the draw list stays the size of
/// the box rather than the size of the model.
#[test]
fn a_thousand_bins_at_two_hundred_pixels_draw_one_bar_per_column() {
    let levels = saw(1024);
    let ui = Ui::new(Theme::DEFAULT);
    let live: Vec<f32> = levels.iter().map(|v| v * 0.5).collect();
    let b = Bins {
        authored: &levels,
        live: Some(&live),
        ..Bins::default()
    };
    let (el, _) = bins(&ui, "spec", &b);
    let Content::Canvas(f) = &el.payload().content else {
        panic!("a canvas")
    };
    let draws = f.0(Size::new(W, H));
    let bars = draws.iter().filter(|d| d.width == 0.0).count();
    assert!(bars <= W as usize, "{bars} bars for {W} px");
    assert!(bars > W as usize / 2, "coalesced away: {bars} bars");
    // The grid, the bars and one cap line for the live overlay.
    let total = draws.len();
    assert!(total <= bars + 4, "{total} draws");
}

/// A log axis is an octave axis: harmonics 1, 2, 4 and 8 are evenly spaced,
/// whatever the bin count.
#[test]
fn the_log_axis_puts_an_octave_between_a_partial_and_its_double() {
    let levels = saw(64);
    let b = Bins {
        authored: &levels,
        x: BinAxis::Log,
        ..Bins::default()
    };
    let octave = b.x_of(1) - b.x_of(0);
    for (lo, hi) in [(1, 3), (3, 7), (7, 15), (15, 31)] {
        assert!(
            (b.x_of(hi) - b.x_of(lo) - octave).abs() < 1e-12,
            "partial {} to {}: {} vs {octave}",
            lo + 1,
            hi + 1,
            b.x_of(hi) - b.x_of(lo)
        );
    }
    // Linear is the other claim: evenly spaced, whatever the harmonic.
    let lin = Bins {
        authored: &levels,
        ..Bins::default()
    };
    assert!((lin.x_of(1) - lin.x_of(0) - (lin.x_of(31) - lin.x_of(30))).abs() < 1e-12);
}
