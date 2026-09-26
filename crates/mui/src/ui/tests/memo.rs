use super::*;

/// A lead `lead` wide, then a memoised panel of two buttons, counting
/// its builds.
fn memo_tree(ui: &mut Ui, deps: u64, lead: f64, built: &mut usize) -> El {
    let panel = ui.memo("panel", deps, |_| {
        *built += 1;
        col([
            block(40., 40.)
                .fill(Role::Raised)
                .a11y(A11y::Button)
                .id("m.a"),
            block(40., 40.)
                .fill(Role::Raised)
                .a11y(A11y::Button)
                .id("m.b"),
        ])
        .id("m")
    });
    row([block(lead, 10.).fill(Role::Surface), panel]).align(Align::Start)
}
/// The same tree built plainly, for what a memo must paint like.
fn plain_tree(lead: f64) -> El {
    let mut t = memo_tree(&mut Ui::new(Theme::DEFAULT), 0, lead, &mut 0);
    t.children_mut()[1].payload_mut().extras_mut().memo = None;
    t
}
const ROOM: Option<Size> = Some(Size::new(300., 200.));

#[test]
fn a_ui_with_kept_memos_stays_send() {
    fn send<T: Send>() {}
    send::<Ui>();
}

#[test]
fn a_memo_builds_once_while_its_deps_hold_and_again_when_they_change() {
    let mut ui = Ui::new(Theme::DEFAULT);
    let mut built = 0;
    for _ in 0..4 {
        let t = memo_tree(&mut ui, 1, 50., &mut built);
        ui.frame(t, ROOM, PointerInput::default(), 0.016).unwrap();
    }
    assert_eq!(built, 1);
    let t = memo_tree(&mut ui, 2, 50., &mut built);
    ui.frame(t, ROOM, PointerInput::default(), 0.016).unwrap();
    assert_eq!(built, 2, "new deps build it again");
    let t = memo_tree(&mut ui, 2, 50., &mut built);
    let f = ui.frame(t, ROOM, PointerInput::default(), 0.016).unwrap();
    assert_eq!(built, 2);
    assert!(
        f.scene.surface("m.b").is_some(),
        "the kept subtree is in the scene"
    );
}

/// Hover warms a button inside a memo on the same frames it would outside
/// one, and the memo rests again once the spring does.
#[test]
fn a_hovered_button_in_a_memo_is_restyled() {
    let (mut ui, mut plain) = (Ui::new(Theme::DEFAULT), Ui::new(Theme::DEFAULT));
    let mut built = 0;
    let step = |ui: &mut Ui, plain: &mut Ui, p: PointerInput, built: &mut usize| {
        ui.anticipate(p.pos);
        let t = memo_tree(ui, 1, 50., built);
        let a = paint_color(&ui.frame(t, ROOM, p, 0.016).unwrap(), "m.a");
        let b = paint_color(
            &plain.frame(plain_tree(50.), ROOM, p, 0.016).unwrap(),
            "m.a",
        );
        (a, b)
    };
    let rest = step(&mut ui, &mut plain, PointerInput::default(), &mut built).0;
    step(&mut ui, &mut plain, PointerInput::default(), &mut built);
    // Told the pointer before the build, the memo warms on the frame the
    // hover lands, as a plain tree does.
    for _ in 0..60 {
        let (a, b) = step(&mut ui, &mut plain, at(70., 20., false), &mut built);
        assert_eq!(a, b, "the memo paints what a plain tree does");
    }
    let hot = step(&mut ui, &mut plain, at(70., 20., false), &mut built).0;
    assert_ne!(hot, rest, "hover warmed the fill");
    let settled = built;
    step(&mut ui, &mut plain, at(70., 20., false), &mut built);
    assert_eq!(built, settled, "a settled hover builds nothing");
    step(&mut ui, &mut plain, at(250., 150., false), &mut built);
    for _ in 0..60 {
        step(&mut ui, &mut plain, at(250., 150., false), &mut built);
    }
    assert_eq!(
        step(&mut ui, &mut plain, at(250., 150., false), &mut built).0,
        rest
    );
}

/// Pushed along by what precedes it, a kept memo is not built again, and
/// it paints and is hit where it now stands.
#[test]
fn a_moved_memo_paints_and_hits_where_it_went() {
    let mut ui = Ui::new(Theme::DEFAULT);
    let mut built = 0;
    for _ in 0..2 {
        let t = memo_tree(&mut ui, 1, 50., &mut built);
        ui.frame(t, ROOM, PointerInput::default(), 0.016).unwrap();
    }
    let t = memo_tree(&mut ui, 1, 90., &mut built);
    let f = ui.frame(t, ROOM, PointerInput::default(), 0.016).unwrap();
    assert_eq!(built, 1);
    let walked =
        mui_scene::resolve(&SceneSpec::new(plain_tree(90.)).offered(Size::new(300., 200.)))
            .unwrap();
    assert_eq!(f.scene.paint, walked.paint);
    assert_eq!(f.scene.surface("m.b").unwrap().frame.x, 90.);
    let t = memo_tree(&mut ui, 1, 90., &mut built);
    ui.frame(t, ROOM, at(110., 60., false), 0.016).unwrap();
    assert!(
        ui.get("m.b").hovered,
        "the moved button is hit where it stands"
    );
}

/// An outer memo built again keeps the inner one it nests; the inner
/// one's hover rebuilds both, since the outer one holds it.
#[test]
fn nested_memos_rebuild_only_what_changed() {
    let mut ui = Ui::new(Theme::DEFAULT);
    let frame = |ui: &mut Ui, deps: u64, p: PointerInput, n: &mut (usize, usize)| {
        let t = ui.memo("outer", deps, |ui| {
            n.0 += 1;
            let kid = ui.memo("inner", 0, |_| {
                n.1 += 1;
                block(40., 40.)
                    .fill(Role::Raised)
                    .a11y(A11y::Button)
                    .id("in.b")
            });
            col([block(40., 40.).fill(Role::Raised).id("out.a"), kid])
        });
        ui.frame(row([t]), ROOM, p, 0.016)
            .unwrap()
            .scene
            .paint
            .len()
    };
    let mut n = (0, 0);
    let full = frame(&mut ui, 1, PointerInput::default(), &mut n);
    assert_eq!(frame(&mut ui, 1, PointerInput::default(), &mut n), full);
    assert_eq!(n, (1, 1));
    assert_eq!(frame(&mut ui, 2, PointerInput::default(), &mut n), full);
    assert_eq!(
        n,
        (2, 1),
        "the outer memo rebuilt around the kept inner one"
    );
    assert_eq!(frame(&mut ui, 2, PointerInput::default(), &mut n), full);
    frame(&mut ui, 2, at(20., 60., false), &mut n);
    frame(&mut ui, 2, at(20., 60., false), &mut n);
    assert!(n.0 > 2 && n.1 > 1, "the inner hover built both again");
    assert!(ui.get("in.b").hovered);
}
