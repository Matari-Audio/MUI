use super::*;
use crate::widgets::{button, slider, toggle};

/// What the app behind [`app`] keeps between frames.
#[derive(Clone, Default)]
struct App {
    gains: [f64; 6],
    on: [bool; 6],
    clicks: usize,
}

/// Rows of controls (one switched off for a while), a `Ui::memo` panel,
/// then a scrolled list that grows with every click.
fn app(ui: &mut Ui, s: &mut App, step: usize) -> El {
    let mut rows = Vec::new();
    for i in 0..6 {
        let go = button(ui, format!("go{i}"), "Go");
        s.clicks += usize::from(go.changed);
        let go = if i == 2 && (30..60).contains(&step) {
            go.el.disabled().into_el()
        } else {
            go.into_el()
        };
        let sw = toggle(ui, format!("on{i}"), "On", &mut s.on[i]);
        let fader = slider(ui, format!("gain{i}"), "Gain", &mut s.gains[i], 0.0..=1.0);
        let r = row([
            go,
            sw.into_el(),
            fader.into_el(),
            text(format!("{:.2}", s.gains[i])),
        ])
        .gap(4.)
        .id(format!("row{i}"));
        rows.push(r);
    }
    let items = (0..3 + s.clicks).map(|i| {
        row([
            text(format!("item {i}")),
            block(20., 10.).fill(Role::Raised),
        ])
        .pad(2.)
        .id(format!("item{i}"))
    });
    let clicks = s.clicks;
    let panel = ui.memo("panel", clicks as u64 / 2, |ui| {
        let ok = button(ui, "ok", "Ok");
        row([ok.into_el(), text(format!("{clicks} clicks"))]).gap(4.)
    });
    col([
        col(rows).gap(4.),
        panel,
        col(items.collect::<Vec<_>>())
            .size(200., 60.)
            .scroll()
            .id("list"),
    ])
    .align(Align::Start)
}

const ROOM: Option<Size> = Some(Size::new(420., 360.));

/// Automatic retention is invisible: a `Ui` with it, driven through hover,
/// presses, drags, a growing keyed list, a disabled row, a `Ui::memo`,
/// `set_scroll`, a theme change and reduced motion,
/// paints and hit-tests exactly like one without it, frame for frame, and
/// still reuses most of the tree while the pointer rests.
#[test]
fn automatic_retention_paints_what_a_full_rebuild_does() {
    let (mut on, mut off) = (Ui::default(), Ui::default());
    off.set_auto_retain(false);
    let (mut a, mut b) = (App::default(), App::default());
    let centre = |ui: &Ui, id: &str| {
        let f = ui.scene_snapshot().unwrap().surface(id).unwrap().frame;
        (f.x + f.size.width / 2., f.y + f.size.height / 2.)
    };
    let mut reused = 0;
    for step in 0..120 {
        let target = ["go1", "on2", "gain3", "go4", "list", "ok", "go2"][step / 12 % 7];
        let input = match step % 12 {
            0..4 => PointerInput::default(),
            _ if step < 2 => PointerInput::default(),
            k => {
                let (x, y) = centre(&off, target);
                // Press on 6..8; a fader is dragged right while held.
                let held = (6..8).contains(&k);
                let drag = held && target.starts_with("gain");
                at(x + if drag { 8. * (k - 5) as f64 } else { 0. }, y, held)
            }
        };
        if step == 50 {
            on.set_scroll("list", [0., 20.]);
            off.set_scroll("list", [0., 20.]);
        }
        if step == 80 {
            let mut dark = on.theme().clone();
            dark.palette.mode = match dark.palette.mode {
                mui_scene::Mode::Dark => mui_scene::Mode::Light,
                _ => mui_scene::Mode::Dark,
            };
            on.set_theme(dark.clone());
            off.set_theme(dark);
        }
        if step == 100 {
            on.set_motion_policy(MotionPolicy::Reduced);
            off.set_motion_policy(MotionPolicy::Reduced);
        }
        let ta = app(&mut on, &mut a, step);
        let tb = app(&mut off, &mut b, step);
        let fa = on.frame(ta, ROOM, input, 0.016).unwrap();
        reused += usize::from(fa.scene.memos_at("row5").any(|(_, r)| r));
        let (pa, sa) = (
            fa.scene.paint.clone(),
            fa.scene.surfaces().cloned().collect::<Vec<_>>(),
        );
        let fb = off.frame(tb, ROOM, input, 0.016).unwrap();
        assert!(pa == fb.scene.paint, "paint differs at step {step}");
        let sb: Vec<_> = fb.scene.surfaces().cloned().collect();
        assert!(sa == sb, "surfaces differ at step {step}");
        assert!(
            fb.scene.memos_at("row5").next().is_none(),
            "off makes no memos"
        );
    }
    assert!(a.clicks > 0 && a.clicks == b.clicks, "the presses landed");
    assert!(reused > 60, "a resting row is reused: {reused} of 120");
}
