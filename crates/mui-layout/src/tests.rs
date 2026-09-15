use super::*;
#[test]
fn intrinsic_chain() {
    let controls = column([
        leaf(28., 28.).id("add"),
        leaf(28., 28.).id("a"),
        leaf(28., 28.).id("b"),
    ])
    .id("controls")
    .gap(10.);
    let tree = column([column([controls]).id("pill").pad(10.)])
        .id("tab")
        .pad(12.);
    let l = resolve(&tree, None, Default::default()).unwrap();
    assert_eq!(l.size, Size::new(72., 148.));
    assert_eq!(l.frame("pill").unwrap().size, Size::new(48., 124.));
}
#[test]
fn asymmetric_padding() {
    let t = column([leaf(10., 20.).id("c")]).id("p").insets(Insets {
        left: 1.,
        right: 2.,
        top: 3.,
        bottom: 4.,
    });
    let l = resolve(&t, None, Default::default()).unwrap();
    assert_eq!(l.size, Size::new(13., 27.));
    assert_eq!(l.frame("c").unwrap().x, 1.0);
    assert_eq!(l.frame("c").unwrap().y, 3.);
}
#[test]
fn space_between() {
    let t = row([leaf(10., 10.).id("a"), leaf(10., 10.).id("b")])
        .id("r")
        .justify(Justify::SpaceBetween);
    let l = resolve(&t, Some(Size::new(100., 10.)), Default::default()).unwrap();
    assert_eq!(l.frame("a").unwrap().x, 0.);
    assert_eq!(l.frame("b").unwrap().x, 90.);
}
#[test]
fn overlay_centers() {
    let t = overlay([leaf(10., 10.).id("a")])
        .id("o")
        .pad(5.)
        .align(Align::Center)
        .justify(Justify::Center);
    let l = resolve(&t, Some(Size::new(40., 50.)), Default::default()).unwrap();
    let f = l.frame("a").unwrap();
    assert_eq!(f.x, 15.);
    assert_eq!(f.y, 20.);
}
#[test]
fn growth_caps() {
    let t = row([
        leaf(10., 10.)
            .id("a")
            .grow(1.)
            .max_size(Size::new(20., 20.)),
        leaf(10., 10.).id("b").grow(1.),
    ])
    .id("r");
    let l = resolve(&t, Some(Size::new(100., 10.)), Default::default()).unwrap();
    assert_eq!(l.frame("a").unwrap().size.width, 20.);
    assert_eq!(l.frame("b").unwrap().size.width, 80.);
}
#[test]
fn failed_commit_preserves() {
    let mut s = LayoutState::default();
    s.commit(&leaf(1., 1.).id("x"), None, Default::default())
        .unwrap();
    let old = s.current.clone();
    assert!(s
        .commit(&leaf(f64::NAN, 1.).id("bad"), None, Default::default())
        .is_err());
    assert_eq!(s.current, old);
    assert_eq!(s.revision, 1);
}
#[test]
fn duplicate_ids() {
    assert!(matches!(
        resolve(
            &row([leaf(1., 1.).id("x"), leaf(1., 1.).id("x")]).id("r"),
            None,
            Default::default()
        ),
        Err(Error::DuplicateKey(_))
    ));
}
#[test]
fn a_declared_minimum_is_the_floor_and_content_alone_is_not() {
    // Content is squeezable; that is what shrink means.
    let l = resolve(
        &leaf(100., 100.).id("x"),
        Some(Size::new(50., 50.)),
        Default::default(),
    )
    .unwrap();
    assert_eq!(l.frame("x").unwrap().size, Size::new(50., 50.));
    // A minimum the author asked for is not.
    assert!(matches!(
        resolve(
            &leaf(100., 100.).id("x").min_size(Size::new(100., 100.)),
            Some(Size::new(50., 50.)),
            Default::default()
        ),
        Err(Error::InsufficientSpace(_))
    ));
}

/// The left/centre/right bar. Ends of different widths -- 50 and 20 -- must
/// still leave the middle child centred on the *container*, which neither
/// `SpaceBetween` nor `grow` can do, because both hand out the surplus left
/// after intrinsic sizing and so inherit the ends' asymmetry.
#[test]
fn basis_zero_shares_the_axis_rather_than_the_surplus() {
    let bar = |ends: Node| {
        let l = resolve(&ends, Some(Size::new(400., 20.)), Default::default()).unwrap();
        let m = l.frame("m").unwrap();
        m.x + m.size.width / 2.
    };
    let slots = |shape: fn(Node) -> Node| {
        row([
            shape(row([leaf(50., 20.).id("li")]).id("l")),
            leaf(30., 20.).id("m"),
            shape(row([leaf(20., 20.).id("ri")]).id("r").justify(Justify::End)),
        ])
        .id("bar")
    };
    assert_eq!(bar(slots(|n| n).justify(Justify::SpaceBetween)), 215.);
    assert_eq!(bar(slots(|n| n.grow(1.))), 215.);
    assert_eq!(bar(slots(|n| n.flex(1.))), 200.);

    // ...and the ends still sit against the edges they belong to.
    let l = resolve(
        &slots(|n| n.flex(1.)),
        Some(Size::new(400., 20.)),
        Default::default(),
    )
    .unwrap();
    assert_eq!(l.frame("li").unwrap().x, 0.);
    assert_eq!(l.frame("ri").unwrap().right(), 400.);

    // Hugging, the row is sized by the flex fraction: wide enough that the
    // hungriest flexible child's *share* still clears its content. The left
    // slot needs 50, so at one unit of grow each both slots are 50 and the
    // row is 130 -- not the 100 that summing the children would give, which
    // would have squashed that slot to 35.
    let hug = resolve(&slots(|n| n.flex(1.)), None, Default::default()).unwrap();
    assert_eq!(hug.size.width, 130.);
    assert_eq!(hug.frame("li").unwrap().size.width, 50.);
    assert_eq!(hug.frame("ri").unwrap().size.width, 20.);
    // ...so the middle child is centred at its hugging size too.
    let m = hug.frame("m").unwrap();
    assert_eq!(m.x + m.size.width / 2., 65.);
}

#[test]
fn a_deficit_comes_back_by_shrink_and_stops_at_each_minimum() {
    let row = |a: Node, b: Node| {
        let l = resolve(
            &row([a, b]).id("r"),
            Some(Size::new(150., 20.)),
            Default::default(),
        )
        .unwrap();
        (
            l.frame("a").unwrap().size.width,
            l.frame("b").unwrap().size.width,
        )
    };
    // Equal basis, equal shrink: the 50 px deficit splits evenly.
    assert_eq!(
        row(leaf(100., 20.).id("a"), leaf(100., 20.).id("b")),
        (75., 75.)
    );
    // `a` freezes at its minimum after giving up 10, and `b` absorbs the rest.
    assert_eq!(
        row(
            leaf(100., 20.).id("a").min_size(Size::new(90., 0.)),
            leaf(100., 20.).id("b")
        ),
        (90., 60.)
    );
    // `shrink(0.0)` opts out entirely.
    assert_eq!(
        row(leaf(100., 20.).id("a").shrink(0.), leaf(100., 20.).id("b")),
        (100., 50.)
    );
}

/// A node's floor is not just its own `min_size`. Without the children's
/// minimums summing upward, a parent is shrunk to a width its own contents
/// then overflow, and nothing anywhere reports it.
#[test]
fn a_parent_cannot_be_squeezed_past_what_its_children_refuse() {
    let root = row([row([
        leaf(100., 20.).id("a").min_size(Size::new(40., 0.)),
        leaf(100., 20.).id("b").min_size(Size::new(30., 0.)),
    ])
    .id("in")])
    .id("out");
    // 70 is exactly the two floors, and both children land on theirs.
    let l = resolve(&root, Some(Size::new(70., 20.)), Default::default()).unwrap();
    assert_eq!(l.frame("a").unwrap().size.width, 40.);
    assert_eq!(l.frame("b").unwrap().size.width, 30.);
    // A pixel under, and it is refused rather than silently overflowing.
    assert!(matches!(
        resolve(&root, Some(Size::new(69., 20.)), Default::default()),
        Err(Error::InsufficientSpace(_))
    ));

    // The floor also has to bind while the deficit is being shared out, not
    // only as a check afterwards. Given a squeezable sibling, `in` freezes
    // at 70 and the rest of the deficit goes to `c` -- an even split would
    // have put `in` at 50, under a floor it never declared itself.
    let pair = row([root, leaf(200., 20.).id("c")]).id("pair");
    let l = resolve(&pair, Some(Size::new(100., 20.)), Default::default()).unwrap();
    assert_eq!(l.frame("out").unwrap().size.width, 70.);
    assert_eq!(l.frame("c").unwrap().size.width, 30.);
}

#[test]
fn align_self_overrides_the_parent_for_one_child() {
    let l = resolve(
        &column([
            leaf(20., 10.).id("a"),
            leaf(20., 10.).id("b").align_self(Align::End),
        ])
        .id("c")
        .align(Align::Start),
        Some(Size::new(100., 20.)),
        Default::default(),
    )
    .unwrap();
    assert_eq!(l.frame("a").unwrap().x, 0.);
    assert_eq!(l.frame("b").unwrap().x, 80.);
}

#[test]
fn containers_stretch_and_content_centres_without_being_told() {
    let l = resolve(
        &column([leaf(20., 10.).id("label"), row([leaf(10., 10.)]).id("bar")]),
        Some(Size::new(100., 20.)),
        Default::default(),
    )
    .unwrap();
    assert_eq!(l.frame("label").unwrap().x, 40.);
    assert_eq!(l.frame("bar").unwrap().size.width, 100.);
}

#[test]
fn percent_is_a_share_of_the_parent_and_auto_while_hugging() {
    let t = row([
        row([leaf(10., 10.)]).id("a").width(Len::Pct(25.)),
        leaf(10., 10.).id("b"),
    ])
    .id("r");
    let l = resolve(&t, Some(Size::new(200., 10.)), Default::default()).unwrap();
    assert_eq!(l.frame("a").unwrap().size.width, 50.);
    assert_eq!(
        resolve(&t, None, Default::default()).unwrap().size.width,
        20.
    );
}

#[test]
fn aspect_derives_the_missing_axis() {
    // Width first: a column child takes the column's width, a row child its
    // allocated share, and the height follows either way.
    let knob = || Node::content().id("k").aspect(2.);
    let l = resolve(&column([knob()]).width(100.), None, Default::default()).unwrap();
    assert_eq!(l.frame("k").unwrap().size, Size::new(100., 50.));
    let l = resolve(
        &row([knob().flex(1.), knob().id("j").flex(1.)]),
        Some(Size::new(80., 40.)),
        Default::default(),
    )
    .unwrap();
    assert_eq!(l.frame("k").unwrap().size, Size::new(40., 20.));
    // A fixed height derives the width at measure, so hugging works too.
    let l = resolve(
        &row([leaf(0., 0.)
            .id("h")
            .height(30.)
            .width(Len::Auto)
            .aspect(0.5)]),
        None,
        Default::default(),
    )
    .unwrap();
    assert_eq!(l.size, Size::new(15., 30.));
}

#[test]
fn grid_flows_equal_columns_and_shares_vertical_surplus() {
    let cells = (0..5).map(|i| leaf(10., 10.).id(format!("c{i}")));
    let g = grid(2, cells).id("g").gap(4.);
    let hug = resolve(&g, None, Default::default()).unwrap();
    assert_eq!(hug.size, Size::new(24., 38.));
    let l = resolve(&g, Some(Size::new(104., 68.)), Default::default()).unwrap();
    // columns 50 wide, rows 20 tall: leaves centre in their cells.
    assert_eq!(l.frame("c1").unwrap().x, 54. + 20.);
    assert_eq!(l.frame("c4").unwrap().y, 48. + 5.);
}

#[test]
fn overlay_children_anchor_themselves_and_nudge() {
    let t = overlay([
        row([]).id("fill"),
        leaf(10., 10.)
            .id("badge")
            .anchor(Align::End, Align::Start)
            .offset(-2., 2.),
    ])
    .id("o");
    let l = resolve(&t, Some(Size::new(100., 50.)), Default::default()).unwrap();
    assert_eq!(l.frame("fill").unwrap().size, Size::new(100., 50.));
    let b = l.frame("badge").unwrap();
    assert_eq!((b.x, b.y), (88., 2.));
}

#[test]
fn space_around_and_evenly() {
    let r = |j| {
        let l = resolve(
            &row([leaf(10., 10.).id("a"), leaf(10., 10.).id("b")]).justify(j),
            Some(Size::new(100., 10.)),
            Default::default(),
        )
        .unwrap();
        (l.frame("a").unwrap().x, l.frame("b").unwrap().x)
    };
    assert_eq!(r(Justify::SpaceAround), (20., 70.));
    let (a, b) = r(Justify::SpaceEvenly);
    assert!((a - 80. / 3.).abs() < 1e-9 && (b - 190. / 3.).abs() < 1e-9);
}

#[test]
fn content_leaves_are_measured_by_the_caller() {
    let t = Node::<&str>::column([Node::content().with("hello").id("t")]).id("c");
    let l = resolve_with(&t, None, Default::default(), SpacingScale::DEFAULT, |s| {
        Size::new(s.len() as f64 * 7., 12.)
    })
    .unwrap();
    assert_eq!(l.frame("t").unwrap().size, Size::new(35., 12.));
}

#[test]
fn tokens_resolve_against_the_scale_and_frames_come_out_in_tree_order() {
    let t = column([leaf(10., 10.).id("a"), leaf(10., 10.)])
        .gap(SpacingToken::M)
        .pad(SpacingToken::S);
    let l = resolve(&t, None, Default::default()).unwrap();
    assert_eq!(l.size, Size::new(26., 48.));
    assert_eq!(l.all().len(), 3);
    assert_eq!(l.all()[1], l.frame("a").unwrap());
    assert_eq!(l.all()[2].y, 30.);
}

#[test]
fn a_scroll_column_overflows_and_slides_and_a_float_takes_no_space() {
    let list = |dy: f64| {
        column([
            leaf(50., 30.).id("a"),
            leaf(50., 30.).id("b"),
            leaf(50., 30.).id("c"),
        ])
        .gap(10.)
        .pad(5.)
        .scroll()
        .scrolled(0., dy)
        .grow(1.)
        .id("list")
    };
    // The window is 60 tall: three 30px rows plus gaps do not fit, and the
    // scroll floor lets the column be squeezed instead of erroring.
    let head = || leaf(60., 20.).shrink(0.);
    let t = column([head(), list(0.)]).size(60., 80.).id("root");
    let l = resolve(&t, None, Default::default()).unwrap();
    assert_eq!(l.frame("list").unwrap().size, Size::new(60., 60.));
    assert_eq!(l.frame("a").unwrap().size, Size::new(50., 30.));
    assert_eq!(l.frame("c").unwrap().y, 20. + 5. + 80.);
    // Scrolled by 40: everything slides up, the frame stays put.
    let t = column([head(), list(40.)]).size(60., 80.);
    let l = resolve(&t, None, Default::default()).unwrap();
    assert_eq!(l.frame("list").unwrap().y, 20.);
    assert_eq!(l.frame("a").unwrap().y, 25. - 40.);
    // Without scroll the rows themselves get squeezed.
    let plain = column([leaf(50., 30.).id("p"), leaf(50., 30.)])
        .gap(10.)
        .pad(5.);
    let l = resolve(
        &column([head(), plain]).size(60., 70.),
        None,
        Default::default(),
    )
    .unwrap();
    assert!(l.frame("p").unwrap().size.height < 30.);
    // A float sits in the parent's padding box and does not widen the row.
    let t = row([
        leaf(10., 10.).id("x"),
        leaf(80., 80.)
            .float()
            .anchor(Align::End, Align::Start)
            .offset(0., 4.)
            .id("tip"),
    ])
    .pad(2.)
    .id("r");
    let l = resolve(&t, None, Default::default()).unwrap();
    assert_eq!(l.frame("r").unwrap().size, Size::new(14., 14.));
    let tip = l.frame("tip").unwrap();
    assert_eq!((tip.x, tip.y), (2. + 10. - 80., 2. + 4.));
    assert_eq!(l.all()[2], tip, "frames stay in declaration order");
}

#[test]
fn a_wrapping_row_breaks_into_lines_and_grows_its_cross_size() {
    let cells = (0..5).map(|_| leaf(30., 10.));
    let t = column([row(cells).id("r").wrap()]);
    let l = resolve(&t, Some(Size::new(100., 200.)), Default::default()).unwrap();
    // three per line of 100, so two lines, and the row is two cells tall.
    assert_eq!(l.frame("r").unwrap().size, Size::new(100., 20.));
    let cells = &l.all()[2..];
    assert_eq!((cells[2].x, cells[2].y), (60., 0.));
    assert_eq!((cells[3].x, cells[3].y), (0., 10.));
}

#[test]
fn a_span_fills_the_row_and_pushes_the_next_cell_down() {
    let t = grid(
        3,
        [
            leaf(10., 10.).id("a"),
            leaf(10., 10.).id("b").span(2),
            leaf(10., 10.).id("c"),
        ],
    );
    let l = resolve(&t, Some(Size::new(30., 20.)), Default::default()).unwrap();
    // b owns columns 1 and 2 -- 20 px wide -- and centres its 10 px in them.
    assert_eq!(l.frame("b").unwrap().center().0, 20.);
    assert_eq!(l.frame("c").unwrap().center().0, 5.);
    assert!(l.frame("c").unwrap().y > l.frame("a").unwrap().y);
}

#[test]
fn order_moves_the_placement_but_not_the_frame() {
    let t = row([leaf(10., 10.).id("a").order(1), leaf(20., 10.).id("b")]);
    let l = resolve(&t, None, Default::default()).unwrap();
    assert_eq!(l.frame("b").unwrap().x, 0.);
    assert_eq!(l.frame("a").unwrap().x, 20.);
    // frames stay in declaration order: root, a, b.
    assert_eq!(l.all()[1], l.frame("a").unwrap());
}

#[test]
fn push_appends_a_child_and_keeps_pre_order_frames() {
    let t = column([leaf(10., 10.).id("a")])
        .id("c")
        .push(leaf(10., 10.).id("b").float());
    let l = resolve(&t, None, Default::default()).unwrap();
    assert_eq!(l.all().len(), 3);
    assert_eq!(l.all()[0], l.frame("c").unwrap());
    assert_eq!(l.all()[2], l.frame("b").unwrap());
    // a leaf has nowhere to put one.
    assert_eq!(leaf(1., 1.).push(leaf(1., 1.)).children().len(), 0);
}
