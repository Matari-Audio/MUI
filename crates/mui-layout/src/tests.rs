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
    // One child is `flex-start`, like CSS: a header row whose second child is
    // conditionally rendered must not jump to the middle when it goes.
    let one = row([leaf(40., 20.).id("a")]).justify(Justify::SpaceBetween);
    let l = resolve(&one, Some(Size::new(300., 40.)), Default::default()).unwrap();
    assert_eq!(l.frame("a").unwrap().x, 0.);
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
        Err(Error::InsufficientSpace { .. })
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
        Err(Error::InsufficientSpace { .. })
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
    let l = resolve_with(
        &t,
        None,
        Default::default(),
        SpacingScale::DEFAULT,
        |s, room| {
            assert_eq!(room, None);
            Size::new(s.len() as f64 * 7., 12.)
        },
    )
    .unwrap();
    assert_eq!(l.frame("t").unwrap().size, Size::new(35., 12.));
}

#[test]
fn content_is_told_the_room_it_has_and_a_scroll_withholds_it() {
    // A definite column pads 10 a side: 80 of room. The grid splits that
    // into two 35 columns. The scroll's child gets none.
    let t = Node::<&str>::column([
        Node::content().with("a").id("a"),
        Node::<&str>::grid(2, [Node::content().with("g").id("g")]).gap(10.),
        Node::<&str>::column([Node::content().with("s").id("s")]).scroll(),
    ])
    .width(Len::Px(100.))
    .pad(10.);
    let mut seen = std::collections::BTreeMap::new();
    let wrap = |s: &&str, room: Option<f64>| {
        seen.insert(s.to_string(), room);
        Size::new(room.map_or(200., |r| r.min(200.)), 12.)
    };
    let l = resolve_with(&t, None, Default::default(), SpacingScale::DEFAULT, wrap).unwrap();
    assert_eq!(seen["a"], Some(80.));
    assert_eq!(seen["g"], Some(35.));
    assert_eq!(seen["s"], None);
    assert_eq!(l.frame("a").unwrap().size.width, 80.);
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

#[test]
fn a_content_leaf_never_keeps_a_cross_extent_wider_than_its_parent() {
    // Both columns take half the row; the text measured at the row's width
    // must be clamped to its own column, not centred half outside it.
    let t = Node::<&str>::row([
        Node::<&str>::column([Node::content().with("p").id("a")]).id("ca"),
        Node::<&str>::column([Node::content().with("p")]).id("cb"),
    ]);
    let l = resolve_with(
        &t,
        Some(Size::new(400., 300.)),
        Default::default(),
        SpacingScale::DEFAULT,
        |_: &&str, room: Option<f64>| Size::new(room.unwrap_or(400.), 12.),
    )
    .unwrap();
    let a = l.frame("a").unwrap();
    assert_eq!(a.x, 0.);
    assert!(a.size.width <= l.frame("ca").unwrap().size.width + 1e-9);
}

#[test]
fn a_percentage_below_the_childs_own_floor_squeezes_instead_of_erroring() {
    let t = row([leaf(0., 0.).pad(5.4).height(Len::Pct(84.6))]);
    assert!(resolve(&t, None, Default::default()).is_ok());
}

#[test]
fn a_declared_size_smaller_than_its_own_padding_is_not_a_squeeze() {
    assert!(resolve(&leaf(10., 10.).pad(6.), None, Default::default()).is_ok());
    assert!(resolve(
        &row([leaf(0., 0.).pad(5.9)]),
        Some(Size::new(354., 231.6)),
        Default::default()
    )
    .is_ok());
}

#[test]
fn grid_columns_never_go_negative_when_the_gaps_outgrow_the_grid() {
    // 21 wide less 15.4 of padding leaves 5.6 for two columns and an 8.4
    // gap. The column is zero-wide, not -1.4, so the percentage child is
    // offered a valid width and the grid reports the squeeze it really is.
    let t = column([grid(2, [Node::leaf(0., 0.).width(Len::Pct(98.8))]).gap(8.4)]).pad(7.7);
    assert!(matches!(
        resolve(&t, Some(Size::new(21., 174.4)), Default::default()),
        Err(Error::InsufficientSpace { .. })
    ));
}

#[test]
fn room_handed_to_the_measurer_leaves_out_the_nodes_own_padding() {
    let t = Node::<&str>::overlay([Node::content().with("t").id("t").pad(4.6)]);
    let l = resolve_with(
        &t,
        Some(Size::new(170., 340.)),
        Default::default(),
        SpacingScale::DEFAULT,
        |_: &&str, room: Option<f64>| Size::new(room.unwrap_or(170.), 12.),
    )
    .unwrap();
    let f = l.frame("t").unwrap();
    assert!(f.x >= 0. && f.size.width <= 170. + 1e-9);
}

#[test]
fn a_grid_cell_never_outgrows_its_column() {
    let t = grid(3, [column([]).width(Len::Px(108.5)).id("c")]).id("g");
    let l = resolve(&t, Some(Size::new(135.5, 62.4)), Default::default()).unwrap();
    let c = l.frame("c").unwrap();
    // The declared width is wider than the track: it starts at the track and
    // is cut to it, rather than painting through the next cell.
    assert_eq!(c.x, 0.);
    assert!((c.size.width - 135.5 / 3.).abs() < 1e-9);
    // Two cells, the first declared three times its column.
    let t = grid(2, [leaf(300., 10.).id("a"), leaf(10., 10.).id("b")]).gap(8.);
    let l = resolve(&t, Some(Size::new(200., 40.)), Default::default()).unwrap();
    let (a, b) = (l.frame("a").unwrap(), l.frame("b").unwrap());
    assert_eq!(a.size.width, 96.);
    assert!(b.x >= a.right() && b.right() <= 200.);
}

#[test]
fn a_float_is_pulled_back_inside_a_thin_window() {
    let menu = || {
        leaf(120., 40.)
            .float()
            .anchor(Align::Start, Align::Start)
            .offset(200., 120.)
            .id("menu")
    };
    let t = column([leaf(10., 10.)]).push(menu()).id("root");
    let l = resolve(&t, Some(Size::new(240., 300.)), Default::default()).unwrap();
    let f = l.frame("menu").unwrap();
    assert_eq!((f.x, f.y), (120., 120.));
    assert!(f.right() <= 240. && f.bottom() <= 300.);
    // One too big to fit has no inside to be pulled to: it keeps its offset.
    let t = column([leaf(10., 10.)])
        .push(menu().size(400., 40.))
        .id("root");
    let l = resolve(&t, Some(Size::new(240., 300.)), Default::default()).unwrap();
    assert_eq!(l.frame("menu").unwrap().x, 200.);
}

#[test]
fn a_squeezed_wrapping_row_says_so_instead_of_overflowing() {
    let bar = || {
        column([row([leaf(250., 40.), leaf(250., 40.)])
            .gap(10.)
            .wrap()
            .id("bar")])
    };
    // 510 wide: one line, and the row is one child tall.
    let l = resolve(&bar(), Some(Size::new(510., 40.)), Default::default()).unwrap();
    assert_eq!(l.frame("bar").unwrap().size, Size::new(510., 40.));
    // Squeezed to 420 the row needs a second line, and the 40 px it was given
    // on the cross axis is a line short. Nothing clips a row, so this errors.
    assert!(matches!(
        resolve(&bar(), Some(Size::new(420., 40.)), Default::default()),
        Err(Error::InsufficientSpace { .. })
    ));
    // Given the stacked height it fits, both children inside the frame.
    let l = resolve(&bar(), Some(Size::new(420., 90.)), Default::default()).unwrap();
    let f = l.frame("bar").unwrap();
    assert!(l.all()[2..].iter().all(|c| c.bottom() <= f.bottom() + 1e-9));
}

#[test]
fn the_error_carries_what_the_whole_tree_needed() {
    let panel = || column([]).min_width(120.).min_height(30.);
    let t = row([panel(), panel()]).gap(10.).id("bar");
    assert!(resolve(&t, Some(Size::new(250., 40.)), Default::default()).is_ok());
    let Err(Error::InsufficientSpace { needs, .. }) =
        resolve(&t, Some(Size::new(240., 40.)), Default::default())
    else {
        panic!("240 is ten short of the two panels and their gap");
    };
    assert_eq!(needs, Size::new(250., 30.));
    // Which is the whole point: the host can scale by it.
    assert!((240. / needs.width - 0.96).abs() < 1e-9);
}

#[test]
fn growth_without_a_declared_maximum_is_not_capped_at_a_magic_number() {
    let t = row([leaf(1., 1.).id("a").grow(1.)]).id("r");
    let l = resolve(
        &t,
        Some(Size::new(3e6, 1.)),
        Limits {
            extent: 5e6,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(l.frame("a").unwrap().size.width, 3e6);
}

#[test]
fn the_gallery_grid_reflows_from_240_to_2000_without_leaving_the_window() {
    // The shape the preview's Grid scene ships: a fixed-size cell in a
    // min_col grid, resolved at the three window shapes a plugin editor is
    // actually dragged to.
    let tree = || {
        column([grid(3, (0..6).map(|i| leaf(90., 54.).id(format!("c{i}"))))
            .gap(10.)
            .min_col(120.)
            .id("grid")])
        .pad(16.)
    };
    let cols = |w: f64, h: f64| {
        let l = resolve(&tree(), Some(Size::new(w, h)), Default::default()).unwrap();
        let top = l.frame("c0").unwrap().y;
        let n = (0..6)
            .filter(|i| l.frame(&format!("c{i}")).unwrap().y == top)
            .count();
        // Nothing the solver placed may leave the window, at any width.
        assert!(
            l.all().iter().all(|f| f.x >= -1e-9
                && f.y >= -1e-9
                && f.right() <= l.size.width + 1e-9
                && f.bottom() <= l.size.height + 1e-9),
            "a frame escaped the {w}x{h} root"
        );
        n
    };
    assert_eq!(
        (cols(240., 600.), cols(800., 500.), cols(2000., 300.)),
        (1, 3, 3)
    );
}

#[test]
fn a_min_col_grid_inside_a_flex_share_drops_columns() {
    // The grid's width is a row share, not a declared length: it is only
    // known once the flex pass deals it.
    let tree = |_w: f64| {
        row([
            leaf(100., 20.).id("side"),
            grid(3, (0..6).map(|i| leaf(90., 54.).id(format!("c{i}"))))
                .gap(10.)
                .min_col(120.)
                .grow(1.0)
                .id("grid"),
        ])
    };
    let cols = |w: f64| {
        let l = resolve(&tree(w), Some(Size::new(w, 600.)), Default::default()).unwrap();
        let top = l.frame("c0").unwrap().y;
        (0..6)
            .filter(|i| l.frame(&format!("c{i}")).unwrap().y == top)
            .count()
    };
    assert_eq!((cols(240.), cols(600.)), (1, 3));
}

#[test]
fn a_squeezed_flex_item_is_measured_again_at_its_share() {
    // Height-for-width in one pass: the paragraph's last measure is at the
    // width the row actually deals it, and the row is as tall as that.
    let calls = std::cell::RefCell::new(Vec::new());
    let root = row([
        Node::content().id("p").shrink(1.0),
        leaf(200., 20.).id("side").shrink(0.0),
    ]);
    let l = resolve_with(
        &root,
        Some(Size::new(300., 400.)),
        Default::default(),
        SpacingScale::DEFAULT,
        |_, room| {
            calls.borrow_mut().push(room);
            // 1000 px of text on one line, or as many lines as it takes.
            room.map_or(Size::new(1000., 10.), |w| {
                Size::new(w, (1000.0 / w).ceil() * 10.0)
            })
        },
    )
    .unwrap();
    let p = l.frame("p").unwrap();
    assert_eq!(*calls.borrow().last().unwrap(), Some(p.size.width));
    assert_eq!((p.size.width, p.size.height), (100., 100.));
    assert_eq!(l.frame("root").map(|f| f.size.height), None);
}

/// Padding has one slot: the last call wins, whatever spelling it used.
#[test]
fn a_pixel_pad_clears_the_token_before_it() {
    let scale = SpacingScale::DEFAULT;
    let n = Node::<()>::row([]).pad(SpacingToken::M).pad(12.0);
    assert_eq!(n.padding(scale), Insets::all(12.0));
    let n = Node::<()>::row([]).pad(12.0).pad(SpacingToken::M);
    assert_eq!(n.padding(scale), Insets::all(scale.m));
    let n = Node::<()>::row([])
        .pad(SpacingToken::M)
        .pad(Spacing::step(3.0));
    assert_eq!(n.padding(scale), Insets::all(12.0));
}

/// Three real subtrees, largest first, at the three window widths a plugin
/// is dragged to: the richest one that fits wins, the losers get empty
/// frames in their own slots, and nothing is built twice.
#[test]
fn fits_keeps_the_first_candidate_that_clears_the_offered_width() {
    let head = || {
        Node::fits([
            row([
                leaf(120., 20.).id("name"),
                leaf(80., 20.).id("version"),
                leaf(80., 20.).id("meter"),
            ])
            .gap(10.)
            .id("wide"),
            row([leaf(120., 20.).id("name2"), leaf(80., 20.).id("version2")])
                .gap(10.)
                .id("mid"),
            leaf(120., 20.).id("thin"),
        ])
        .id("head")
    };
    let shown = |w: f64| {
        let l = resolve(&head(), Some(Size::new(w, 20.)), Default::default()).unwrap();
        // Every candidate has a frame, in tree order; only one has a size.
        assert_eq!(l.all().len(), 1 + 3 + 1 + 2 + 1 + 1);
        ["wide", "mid", "thin"]
            .into_iter()
            .filter(|k| l.frame(k).unwrap().size.width > 0.0)
            .collect::<Vec<_>>()
    };
    assert_eq!(shown(400.), ["wide"]);
    assert_eq!(shown(220.), ["mid"]);
    assert_eq!(shown(60.), ["thin"]);
    // The children of a candidate that lost are empty too, and sit at the
    // fits node's own origin rather than at the window's.
    let l = resolve(
        &column([leaf(0., 40.), head()]),
        Some(Size::new(60., 60.)),
        Default::default(),
    )
    .unwrap();
    assert_eq!(l.frame("name").unwrap().size, Size::ZERO);
    assert_eq!(l.frame("name").unwrap().y, l.frame("head").unwrap().y);
}

/// A percentage of the parent and a percentage of the container are the same
/// number until the parent stops having a size of its own.
#[test]
fn a_container_share_skips_the_hugging_parent_between() {
    let bar = |len: Len| row([row([leaf(0., 8.).width(len).id("bar")]).id("hug")]).width(200.);
    let at = |len: Len| {
        resolve(&bar(len), Some(Size::new(200., 8.)), Default::default())
            .unwrap()
            .frame("bar")
            .unwrap()
            .size
            .width
    };
    assert_eq!(at(Len::Container(50.)), 100.);
    // The hugging row measured nothing, so a share of *it* is nothing.
    assert_eq!(at(Len::Pct(50.)), 0.);
    // With a definite parent the two agree: the parent is the container.
    let sized = |len: Len| {
        let t = row([row([leaf(0., 8.).width(len).id("bar")]).width(80.)]).width(200.);
        resolve(&t, Some(Size::new(200., 8.)), Default::default())
            .unwrap()
            .frame("bar")
            .unwrap()
            .size
            .width
    };
    assert_eq!(
        (sized(Len::Container(50.)), sized(Len::Pct(50.))),
        (40., 40.)
    );
}

#[test]
fn a_pin_falls_back_to_the_first_area_that_fits_the_root() {
    // The field sits at the bottom of the window, so a menu under it would
    // hang out of the root; the fallback puts it above instead.
    let menu = |gap: f64| {
        let m = leaf(80., 60.)
            .pin(
                Pin::to("field")
                    .area(Area::BottomStart)
                    .gap(gap)
                    .fallback(Area::TopStart),
            )
            .id("menu");
        let field = leaf(80., 24.).anchor(Align::Start, Align::End).id("field");
        resolve(
            &overlay([field, m]),
            Some(Size::new(200., 100.)),
            Default::default(),
        )
        .unwrap()
        .frame("menu")
        .unwrap()
    };
    assert_eq!((menu(4.).x, menu(4.).y), (0., 12.), "above, by the gap");
    // Neither area fits with a gap that big, so the preferred one is pulled
    // back inside the root rather than painted off-window.
    assert_eq!(menu(90.).y, 40.);
}

#[test]
fn a_matched_pin_takes_the_anchor_width() {
    let tree = overlay([
        leaf(90., 24.)
            .anchor(Align::Start, Align::Start)
            .id("field"),
        leaf(10., 20.)
            .pin(Pin::to("field").area(Area::BottomStart).match_width())
            .id("menu"),
    ]);
    let l = resolve(&tree, Some(Size::new(200., 200.)), Default::default()).unwrap();
    assert_eq!(l.frame("menu").unwrap().size.width, 90.);
    assert_eq!(l.frame("menu").unwrap().y, 24.);
}

#[test]
fn a_sticky_header_holds_the_viewport_edge_until_its_section_ends() {
    let section = |k: &str| {
        column([
            leaf(100., 20.).id(format!("{k}-head")).sticky(),
            leaf(100., 180.).id(k),
        ])
    };
    let at = |dy: f64| {
        let list = column([section("a"), section("b")])
            .scroll()
            .scrolled(0., dy)
            .id("list");
        let l = resolve(&list, Some(Size::new(100., 120.)), Default::default()).unwrap();
        (
            l.frame("a-head").unwrap().y,
            l.frame("b-head").unwrap().y,
            l.frame("a").unwrap().y,
        )
    };
    // Unscrolled, both headers sit where the flow put them.
    assert_eq!(at(0.), (0., 200., 20.));
    // Scrolled into the first section: its header holds the top edge while the
    // rows slide under it, and the second one has not arrived yet.
    assert_eq!(at(60.), (0., 140., -40.));
    // Past the first section's end, the first header is pushed off the edge by
    // its own section's bottom, just as the second one takes the edge.
    assert_eq!(at(190.), (-10., 10., -170.));
}

#[test]
fn min_size_is_the_intrinsic_floor_and_a_scroll_gives_up_its_axis() {
    let rows = || {
        column([
            leaf(40., 30.).min_size(Size::new(40., 30.)),
            leaf(40., 30.).min_size(Size::new(40., 30.)),
        ])
        .gap(4.)
        .pad(8.)
    };
    // Nothing here can shrink, so the floor is the intrinsic pass exactly.
    let hug = resolve(&rows(), None, Default::default()).unwrap();
    let l = resolve(&rows(), Some(Size::new(400., 300.)), Default::default()).unwrap();
    assert_eq!(l.min_size(), hug.size);
    assert_eq!(l.min_size(), Size::new(56., 80.));
    // The same rows behind a scroll cost nothing on the scrolling axis: a host
    // may make the window as short as the padding.
    let scrolled = resolve(
        &rows().scroll(),
        Some(Size::new(400., 300.)),
        Default::default(),
    )
    .unwrap();
    assert_eq!(scrolled.min_size(), Size::new(56., 16.));
}

#[test]
fn min_col_widens_a_hugging_grid_instead_of_squeezing_its_columns() {
    // A KURV modal: nothing offers it a width, so the grid used to keep its
    // declared columns at the width of their narrowest content and ignore the
    // minimum entirely. Hugging is not a licence to go under a declared floor.
    let modal = |min: f64| {
        column([grid(2, (0..4).map(|i| leaf(90., 40.).id(format!("c{i}"))))
            .gap(10.)
            .min_col(min)
            .id("grid")])
        .pad(12.)
        .id("modal")
    };
    let l = resolve(&modal(160.), None, Default::default()).unwrap();
    // Two 160-wide columns and one gap, not two 90-wide cells.
    assert_eq!(l.frame("grid").unwrap().size.width, 330.);
    assert_eq!(l.size.width, 354.);
    // A minimum the content already clears changes nothing.
    let l = resolve(&modal(80.), None, Default::default()).unwrap();
    assert_eq!(l.frame("grid").unwrap().size.width, 190.);
    // And the floor is still the cells', so a squeeze past the hug is a
    // column drop, not an error: `min_col` widens, it does not pin.
    let l = resolve(
        &modal(160.),
        Some(Size::new(200., 300.)),
        Default::default(),
    )
    .unwrap();
    let top = l.frame("c0").unwrap().y;
    assert_eq!(
        (0..4)
            .filter(|i| l.frame(&format!("c{i}")).unwrap().y == top)
            .count(),
        1
    );
}
