//! Motion's claims: a solved frame glides instead of jumping and carries its
//! subtree, a nested glide is not chased twice, an appearing node comes in
//! from where it said and fades out where it stood, a renamed shape morphs
//! and lands exactly, and a channel keeps its spring when a sibling list
//! grows.
use mui::prelude::*;
use mui::scene::{Appear, Layer, Mix, ResolvedScene};

const DT: f64 = 1. / 60.;
const SIZE: Size = Size::new(200., 40.);

fn x(s: &ResolvedScene, id: &str) -> f64 {
    s.surface(id).expect(id).frame.x
}

/// A 20 px block pushed to the right end of a 200 px row, or left at the start.
fn slider(right: bool) -> El {
    row![
        spacer().grow(if right { 1. } else { 0. }),
        stack![block(8., 8.).id("dot")]
            .size(20., 20.)
            .animate_layout()
            .id("block"),
        spacer().grow(if right { 0. } else { 1. }),
    ]
}

#[test]
fn a_moved_frame_glides_and_carries_its_children() {
    let mut ui = Ui::default();
    let at = |ui: &mut Ui, right| {
        let f = ui
            .frame(slider(right), Some(SIZE), Input::default(), DT)
            .unwrap();
        (x(f.scene, "block"), x(f.scene, "dot"), f.animating)
    };
    let (start, dot, moving) = at(&mut ui, false);
    assert_eq!((start, moving), (0., false), "first sight: no fly-in");
    let inset = dot - start;
    let (next, dot, moving) = at(&mut ui, true);
    assert!(moving && next > 0. && next < 180., "on its way: {next}");
    assert!(
        (dot - next - inset).abs() < 1e-9,
        "an unanimated child rides with it"
    );
    let mut last = next;
    for _ in 0..90 {
        let (now, _, _) = at(&mut ui, true);
        assert!(now >= last - 1e-9, "critically damped: never back");
        last = now;
    }
    assert!((last - 180.).abs() < 1e-6, "lands: {last}");
    assert!(!at(&mut ui, true).2, "and rests");
}

#[test]
fn a_child_that_animates_too_is_not_chased_twice() {
    let tree = |right: bool| {
        row![
            spacer().grow(if right { 1. } else { 0. }),
            stack![block(8., 8.).animate_layout().id("inner")]
                .size(20., 20.)
                .animate_layout()
                .id("outer"),
            spacer().grow(if right { 0. } else { 1. }),
        ]
    };
    let mut ui = Ui::default();
    let f = ui
        .frame(tree(false), Some(SIZE), Input::default(), DT)
        .unwrap();
    let inset = x(f.scene, "inner") - x(f.scene, "outer");
    for _ in 0..10 {
        let f = ui
            .frame(tree(true), Some(SIZE), Input::default(), DT)
            .unwrap();
        // The inner node did not move relative to the outer one, so it
        // stays rigidly inside it rather than lagging behind.
        let now = x(f.scene, "inner") - x(f.scene, "outer");
        assert!(
            f.animating && (now - inset).abs() < 1e-9,
            "{now} vs {inset}"
        );
    }
}

fn opacity(s: &ResolvedScene, key: &str) -> Option<f32> {
    let mut depth: Vec<f32> = Vec::new();
    for p in &s.paint {
        match p.layer {
            Layer::Blend {
                opacity,
                mix: Mix::Normal,
            } => depth.push(opacity),
            Layer::Unblend => {
                depth.pop();
            }
            Layer::Fill if &*p.key == key => return Some(depth.iter().product()),
            _ => {}
        }
    }
    None
}

#[test]
fn an_appearing_node_slides_in_fades_in_and_fades_out_where_it_stood() {
    let toast = || {
        block(60., 20.)
            .fill(Role::Primary)
            .appear(Appear::Slide(0., 12.))
            .id("toast")
    };
    let mut ui = Ui::default();
    let shown = |on: bool| stack![if on { toast() } else { block(1., 1.) }];
    let f = ui
        .frame(shown(true), Some(SIZE), Input::default(), DT)
        .unwrap();
    let y0 = f.scene.surface("toast").unwrap().frame.y;
    let o0 = opacity(f.scene, "toast").unwrap();
    assert!(o0 < 0.2, "starts nearly clear: {o0}");
    for _ in 0..90 {
        ui.frame(shown(true), Some(SIZE), Input::default(), DT)
            .unwrap();
    }
    let f = ui
        .frame(shown(true), Some(SIZE), Input::default(), DT)
        .unwrap();
    let rest = f.scene.surface("toast").unwrap().frame.y;
    assert!(
        y0 - rest > 9.,
        "came up from its slide offset: {y0} -> {rest}"
    );
    assert!(opacity(f.scene, "toast").unwrap() > 0.999);

    // Gone from the tree: no surface, but its paint lingers, fading.
    let f = ui
        .frame(shown(false), Some(SIZE), Input::default(), DT)
        .unwrap();
    assert!(f.scene.surface("toast").is_none(), "not hit, not focusable");
    let ghost = opacity(f.scene, "toast").expect("still painted");
    assert!(ghost < 1. && f.animating, "{ghost}");
    for _ in 0..90 {
        ui.frame(shown(false), Some(SIZE), Input::default(), DT)
            .unwrap();
    }
    let f = ui
        .frame(shown(false), Some(SIZE), Input::default(), DT)
        .unwrap();
    assert!(opacity(f.scene, "toast").is_none() && !f.animating);
}

#[test]
fn a_renamed_shape_morphs_and_lands_on_the_exact_outline() {
    let icon = |pill: bool| {
        let el = block(40., 20.).fill(Role::Primary).id("shape");
        if pill {
            el.pill().morph("pill")
        } else {
            el.radius(0.).morph("box")
        }
    };
    let fill = |s: &ResolvedScene| {
        s.paint
            .iter()
            .find(|p| &*p.key == "shape" && p.layer == Layer::Fill)
            .unwrap()
            .path
            .clone()
    };
    let mut ui = Ui::default();
    let f = ui
        .frame(icon(false), Some(SIZE), Input::default(), DT)
        .unwrap();
    let square = fill(f.scene);
    let f = ui
        .frame(icon(true), Some(SIZE), Input::default(), DT)
        .unwrap();
    let pill = f.scene.surface("shape").unwrap().path.clone();
    let mid = fill(f.scene);
    assert!(
        f.animating && *mid != *square && *mid != *pill,
        "in between"
    );
    for _ in 0..120 {
        ui.frame(icon(true), Some(SIZE), Input::default(), DT)
            .unwrap();
    }
    let f = ui
        .frame(icon(true), Some(SIZE), Input::default(), DT)
        .unwrap();
    assert_eq!(
        *fill(f.scene),
        *f.scene.surface("shape").unwrap().path,
        "exact at rest"
    );
    assert!(!f.animating);
}

#[test]
fn a_new_shadow_does_not_steal_the_shells_spring() {
    // Before stable channel ids, a shell's spring lived at `7 + shadows`:
    // adding a shadow moved the shell onto the shadow's blur slot.
    let card = |shadow: bool| {
        let c = block(40., 40.)
            .fill(Role::Surface)
            .shell(6., Role::Primary)
            .animate()
            .id("c");
        if shadow {
            c.shadow(Shadow::soft(30.))
        } else {
            c
        }
    };
    let mut ui = Ui::default();
    for _ in 0..3 {
        ui.frame(card(false), Some(SIZE), Input::default(), DT)
            .unwrap();
    }
    let f = ui
        .frame(card(true), Some(SIZE), Input::default(), DT)
        .unwrap();
    assert!(
        !f.animating,
        "only a new channel appeared; it starts where it is declared"
    );
}

#[test]
fn opacity_and_gradient_stops_spring() {
    let node = |on: bool| {
        block(40., 40.)
            .fill(Gradient::linear(
                90.,
                [
                    (0., Role::Primary),
                    (if on { 0.9 } else { 0.1 }, Role::Surface),
                ],
            ))
            .opacity(if on { 1. } else { 0. })
            .animate()
            .id("n")
    };
    let mut ui = Ui::default();
    ui.frame(node(false), Some(SIZE), Input::default(), DT)
        .unwrap();
    let f = ui
        .frame(node(true), Some(SIZE), Input::default(), DT)
        .unwrap();
    let o = f
        .scene
        .paint
        .iter()
        .find_map(|p| match p.layer {
            Layer::Blend { opacity, .. } if &*p.key == "n" => Some(opacity),
            _ => None,
        })
        .unwrap();
    assert!(o > 0. && o < 1. && f.animating, "{o}");
}

#[test]
fn a_reordered_slot_keeps_its_state_and_glides_to_its_new_place() {
    // Two modules in a rack, named by slot index as a real rack names them.
    let rack = |order: [u64; 2]| {
        row(order.iter().enumerate().map(|(slot, &module)| {
            stack![block(10., 10.).id(format!("slot/{slot}/knob"))]
                .size(60., 30.)
                .animate_layout()
                .identity(module)
                .id(format!("slot/{slot}"))
        }))
        .gap(20.)
    };
    let mut ui = Ui::default();
    let f = ui
        .frame(rack([7, 9]), Some(SIZE), Input::default(), DT)
        .unwrap();
    let was = x(f.scene, "slot/0");
    let inset = x(f.scene, "slot/0/knob") - was;
    assert_eq!(was, 0.);
    // Swapped: module 7 is now slot 1. It moves from where it stood rather
    // than appearing in its new place.
    let f = ui
        .frame(rack([9, 7]), Some(SIZE), Input::default(), DT)
        .unwrap();
    let now = x(f.scene, "slot/1");
    assert!(
        f.animating && now > was && now < 80.,
        "module 7 on its way: {now}"
    );
    let knob = x(f.scene, "slot/1/knob");
    assert!(
        (knob - now - inset).abs() < 1e-9,
        "its composed child came along"
    );
    for _ in 0..120 {
        ui.frame(rack([9, 7]), Some(SIZE), Input::default(), DT)
            .unwrap();
    }
    let f = ui
        .frame(rack([9, 7]), Some(SIZE), Input::default(), DT)
        .unwrap();
    assert!((x(f.scene, "slot/1") - 80.).abs() < 1e-6 && !f.animating);
}
