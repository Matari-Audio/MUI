//! The curve editor's four claims: the knot is the target and the line is
//! not, a drag moves one knot and clamps it, Shift drags fine, and what is
//! drawn is the cubic the DSP side samples.
use mui::input::{Button, Buttons, Mods};
use mui::prelude::*;
use mui::scene::Content;
use mui::scene::curve::Curve;

const SIZE: f64 = 200.0;
/// The widget's own inset: a knot's radius, so an end knot sits inside the
/// frame. Restated here so a change to it fails these tests loudly.
const KNOT: f64 = 5.0;

fn pointer(x: f64, y: f64, down: bool, shift: bool) -> PointerInput {
    PointerInput {
        pos: Some(Point::new(x, y)),
        buttons: Buttons::default().set(Button::Primary, down),
        mods: Mods {
            shift,
            ..Mods::default()
        },
    }
}
/// One frame of the widget at 200x200.
fn frame(ui: &mut Ui, c: &mut Curve, p: PointerInput) -> Option<CurveEdit> {
    let Response { el, changed: edit } = curve(ui, "env", c);
    ui.frame(el.square(SIZE), Some(Size::new(SIZE, SIZE)), p, 0.016)
        .expect("resolves");
    edit
}
/// Where knot `i` is drawn, in the canvas's own pixels.
fn knot(c: &Curve, i: usize) -> Point {
    let span = SIZE - 2.0 * KNOT;
    let p = c.points()[i];
    Point::new(
        KNOT + f64::from(p.phase) * span,
        KNOT + (1.0 - f64::from(p.value)) * span,
    )
}

/// The spine is a stroke with no tag, so the pointer on the line is on
/// nothing; a knot's disc is the only grab.
#[test]
fn the_knot_is_hit_and_the_line_between_knots_is_not() {
    let mut ui = Ui::default();
    let mut c = Curve::linear();
    let (a, b) = (knot(&c, 0), knot(&c, 1));
    let mid = Point::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
    // Two frames per move: a gesture reads the hit map built last frame.
    for _ in 0..2 {
        frame(&mut ui, &mut c, pointer(mid.x, mid.y, false, false));
    }
    assert_eq!(ui.tag("env"), None, "the spine is not a target");
    for _ in 0..2 {
        frame(&mut ui, &mut c, pointer(b.x, b.y, false, false));
    }
    assert_eq!(ui.tag("env"), Some("n1"), "the knot answers for itself");
}

/// A drag moves the knot it grabbed and no other, and it stays between its
/// neighbours however far the pointer goes.
#[test]
fn a_drag_moves_only_the_grabbed_knot_and_clamps_it_to_its_neighbours() {
    let mut ui = Ui::default();
    let mut c = Curve::default();
    let others: Vec<_> = c.points()[2..].to_vec();
    let start = knot(&c, 1);
    for _ in 0..2 {
        frame(&mut ui, &mut c, pointer(start.x, start.y, false, false));
    }
    frame(&mut ui, &mut c, pointer(start.x, start.y, true, false));
    // All the way to the right edge, past knot 2 and the end.
    frame(&mut ui, &mut c, pointer(SIZE * 2.0, start.y, true, false));
    let edit = frame(&mut ui, &mut c, pointer(SIZE * 2.0, start.y, true, false));
    assert_eq!(edit, Some(CurveEdit::Point(1)));
    assert!(
        c.points()[1].phase < c.points()[2].phase,
        "clamped under its neighbour: {:?}",
        c.points()
    );
    assert_eq!(&c.points()[2..], &others[..], "no other knot moved");
}

/// Shift is the fine drag: the same travel moves a tenth as far.
#[test]
fn shift_drags_fine() {
    let travel = |shift: bool| {
        let mut ui = Ui::default();
        let mut c = Curve::linear();
        let start = knot(&c, 0);
        for _ in 0..2 {
            frame(&mut ui, &mut c, pointer(start.x, start.y, false, shift));
        }
        frame(&mut ui, &mut c, pointer(start.x, start.y, true, shift));
        for _ in 0..2 {
            frame(
                &mut ui,
                &mut c,
                pointer(start.x, start.y - 50.0, true, shift),
            );
        }
        f64::from(c.points()[0].value)
    };
    let (coarse, fine) = (travel(false), travel(true));
    assert!(coarse > 0.2, "50 px of a 190 px plot: {coarse}");
    assert!(
        (fine - coarse * 0.1).abs() < 1e-3,
        "a tenth of it: {fine} vs {coarse}"
    );
}

/// What is drawn is what the DSP side samples: the spine's control points are
/// the model's own, and `Curve::evaluate` agrees at the knots.
#[test]
fn the_drawn_spine_is_the_models_cubic_and_matches_the_sampler() {
    let mut ui = Ui::default();
    let mut c = Curve::default();
    let el = curve(&mut ui, "env", &mut c).el;
    let draws = match &el.payload().content {
        Content::Canvas(f) => f.0(Size::new(SIZE, SIZE)),
        _ => panic!("a canvas"),
    };
    let point = |p: mui::scene::curve::CurvePoint| {
        let span = SIZE - 2.0 * KNOT;
        Point::new(
            KNOT + f64::from(p.phase) * span,
            KNOT + (1.0 - f64::from(p.value)) * span,
        )
    };
    let mut want = Path::default().move_to(point(c.points()[0]));
    for (j, h) in c.handles().iter().enumerate() {
        want = want.cubic_to(
            point(h.outgoing),
            point(h.incoming),
            point(c.points()[j + 1]),
        );
    }
    assert_eq!(
        *draws[0].path, want,
        "the path is the model's control points"
    );
    // And the sampler those control points belong to: every knot is on the
    // curve it draws.
    for p in c.points() {
        let got = c.evaluate(p.phase);
        assert!(
            (got - p.value).abs() < 1e-3,
            "evaluate({}) = {got}, want {}",
            p.phase,
            p.value
        );
    }
}
