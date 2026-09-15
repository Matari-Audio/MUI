//! Audit reproductions: assertions cover the corrected public contracts.
use mui_core::{resolve_scene, Palette, Radius, SceneSpec, Spacing, SurfaceSpec, Theme};
use mui_geometry::{Bounds, Error, Point, RoundedRect};
use mui_input::{Hit, Interaction, PointerInput};

fn input(x: f64, down: bool) -> PointerInput {
    PointerInput {
        pos: Some(Point::new(x, 10.0)),
        primary_down: down,
    }
}
fn click_after_move(mut interaction: Interaction, hit: &Hit, x: f64) -> bool {
    interaction.update(hit, input(10.0, true));
    interaction.update(hit, input(x, true));
    interaction.update(hit, input(x, false));
    interaction.get("a").clicked
}
fn rejects_panic<F>(f: F) -> bool
where
    F: FnOnce() + std::panic::UnwindSafe,
{
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let rejected = std::panic::catch_unwind(f).is_err();
    std::panic::set_hook(hook);
    rejected
}
fn main() {
    assert_eq!(
        mui_geometry::Polygon::rectangle(1e308, 0.0, 1e308, 1.0),
        Err(Error::NonFinite)
    );
    println!("PASS: rectangle constructor rejects overflowing endpoints");
    let rect = RoundedRect::new(
        Bounds {
            min: Point::ZERO,
            max: Point::new(100.0, 100.0),
        },
        0.0,
    )
    .unwrap();
    let mut hit = Hit::default();
    hit.push("a", &rect.path()).unwrap();
    assert!(click_after_move(Interaction::new(), &hit, 11.0));
    assert!(click_after_move(Interaction::default(), &hit, 11.0));
    println!("PASS: Default and new use the same drag threshold");

    assert!(rejects_panic(|| {
        Interaction::new().with_drag_threshold(f64::NAN);
    }));
    assert!(rejects_panic(|| {
        Interaction::new().with_drag_threshold(-1.0);
    }));
    println!("PASS: invalid drag thresholds are rejected");

    let palette = Palette {
        step: -0.045,
        hover: -0.11,
        ..Palette::NEUTRAL
    };
    let theme = Theme {
        palette,
        ..Theme::default()
    };
    let spec = SceneSpec::new(mui_layout::leaf(100.0, 100.0)).theme(theme);
    assert!(resolve_scene(&spec).is_err());
    println!("PASS: scene resolver rejects negative palette step and hover");

    assert!(rejects_panic(|| {
        SurfaceSpec::inset("inner", "outer", Spacing::px(4.0)).radius(Radius::Absolute(30.0));
    }));
    println!("PASS: wrong-variant radius setter rejects misuse");

    let rect_b = RoundedRect::new(
        Bounds {
            min: Point::new(200.0, 0.0),
            max: Point::new(300.0, 100.0),
        },
        0.0,
    )
    .unwrap();
    hit.push("b", &rect_b.path()).unwrap();
    // The preview queues each pointer sample, including the position attached
    // to every button edge, and replays the samples in chronological order.
    let mut queued = Interaction::new();
    for pointer in [input(10.0, true), input(210.0, true), input(210.0, false)] {
        queued.update(&hit, pointer);
    }
    assert!(!queued.get("b").clicked);
    println!("PASS: chronological pointer samples do not click the wrong target");
}
