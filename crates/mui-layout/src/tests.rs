use super::*;
#[test]
fn intrinsic_chain() {
    let controls = col([
        block(28., 28.).id("add"),
        block(28., 28.).id("a"),
        block(28., 28.).id("b"),
    ])
    .id("controls")
    .gap(10.);
    let tree = col([col([controls]).id("pill").pad(10.)])
        .id("tab")
        .pad(12.);
    let l = resolve(&tree, None, Limits::default()).unwrap();
    assert_eq!(l.size, Size::new(72., 148.));
    assert_eq!(l.frame("pill").unwrap().size, Size::new(48., 124.));
}
#[test]
fn asymmetric_padding() {
    let t = col([block(10., 20.).id("c")]).id("p").pad(Insets {
        left: 1.,
        right: 2.,
        top: 3.,
        bottom: 4.,
    });
    let l = resolve(&t, None, Limits::default()).unwrap();
    assert_eq!(l.size, Size::new(13., 27.));
    assert_eq!(l.frame("c").unwrap().x, 1.0);
    assert_eq!(l.frame("c").unwrap().y, 3.);
}
#[test]
fn space_between() {
    let t = row([block(10., 10.).id("a"), block(10., 10.).id("b")])
        .id("r")
        .justify(Justify::SpaceBetween);
    let l = resolve(&t, Some(Size::new(100., 10.)), Limits::default()).unwrap();
    assert_eq!(l.frame("a").unwrap().x, 0.);
    assert_eq!(l.frame("b").unwrap().x, 90.);
    // One child is `flex-start`, like CSS: a header row whose second child is
    // conditionally rendered must not jump to the middle when it goes.
    let one = row([block(40., 20.).id("a")]).justify(Justify::SpaceBetween);
    let l = resolve(&one, Some(Size::new(300., 40.)), Limits::default()).unwrap();
    assert_eq!(l.frame("a").unwrap().x, 0.);
}
#[test]
fn overlay_centers() {
    let t = stack([block(10., 10.).id("a")])
        .id("o")
        .pad(5.)
        .align(Align::Center)
        .justify(Justify::Center);
    let l = resolve(&t, Some(Size::new(40., 50.)), Limits::default()).unwrap();
    let f = l.frame("a").unwrap();
    assert_eq!(f.x, 15.);
    assert_eq!(f.y, 20.);
}
#[test]
fn growth_caps() {
    let t = row([
        block(10., 10.)
            .id("a")
            .grow(1.)
            .max_size(Size::new(20., 20.)),
        block(10., 10.).id("b").grow(1.),
    ])
    .id("r");
    let l = resolve(&t, Some(Size::new(100., 10.)), Limits::default()).unwrap();
    assert_eq!(l.frame("a").unwrap().size.width, 20.);
    assert_eq!(l.frame("b").unwrap().size.width, 80.);
}
#[test]
fn duplicate_ids() {
    assert!(matches!(
        resolve(
            &row([block(1., 1.).id("x"), block(1., 1.).id("x")]).id("r"),
            None,
            Limits::default()
        ),
        Err(Error::DuplicateKey(_))
    ));
}
#[test]
fn a_declared_minimum_is_the_floor_and_content_alone_is_not() {
    // Content is squeezable; that is what shrink means.
    let l = resolve(
        &block(100., 100.).id("x"),
        Some(Size::new(50., 50.)),
        Limits::default(),
    )
    .unwrap();
    assert_eq!(l.frame("x").unwrap().size, Size::new(50., 50.));
    // A minimum the author asked for is not: it overflows its parent.
    let l = resolve(
        &col([block(100., 100.).id("x").min_size(Size::new(100., 100.))]),
        Some(Size::new(50., 50.)),
        Limits::default(),
    )
    .unwrap();
    assert_eq!(l.frame("x").unwrap().size, Size::new(100., 100.));
    assert_eq!(l.size, Size::new(50., 50.));
}

/// The left/centre/right bar. Ends of different widths -- 50 and 20 -- must
/// still leave the middle child centred on the *container*, which neither
/// `SpaceBetween` nor `grow` can do, because both hand out the surplus left
/// after intrinsic sizing and so inherit the ends' asymmetry.
#[test]
fn basis_zero_shares_the_axis_rather_than_the_surplus() {
    let bar = |ends: Node| {
        let l = resolve(&ends, Some(Size::new(400., 20.)), Limits::default()).unwrap();
        let m = l.frame("m").unwrap();
        m.x + m.size.width / 2.
    };
    let slots = |shape: fn(Node) -> Node| {
        row([
            shape(row([block(50., 20.).id("li")]).id("l")),
            block(30., 20.).id("m"),
            shape(
                row([block(20., 20.).id("ri")])
                    .id("r")
                    .justify(Justify::End),
            ),
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
        Limits::default(),
    )
    .unwrap();
    assert_eq!(l.frame("li").unwrap().x, 0.);
    assert_eq!(l.frame("ri").unwrap().right(), 400.);

    // Hugging, the row is sized by the flex fraction: wide enough that the
    // hungriest flexible child's *share* still clears its content. The left
    // slot needs 50, so at one unit of grow each both slots are 50 and the
    // row is 130 -- not the 100 that summing the children would give, which
    // would have squashed that slot to 35.
    let hug = resolve(&slots(|n| n.flex(1.)), None, Limits::default()).unwrap();
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
            Limits::default(),
        )
        .unwrap();
        (
            l.frame("a").unwrap().size.width,
            l.frame("b").unwrap().size.width,
        )
    };
    // Equal basis, equal shrink: the 50 px deficit splits evenly.
    assert_eq!(
        row(block(100., 20.).id("a"), block(100., 20.).id("b")),
        (75., 75.)
    );
    // `a` freezes at its minimum after giving up 10, and `b` absorbs the rest.
    assert_eq!(
        row(
            block(100., 20.).id("a").min_size(Size::new(90., 0.)),
            block(100., 20.).id("b")
        ),
        (90., 60.)
    );
    // `shrink(0.0)` opts out entirely.
    assert_eq!(
        row(
            block(100., 20.).id("a").shrink(0.),
            block(100., 20.).id("b")
        ),
        (100., 50.)
    );
}

/// A node's floor is not just its own `min_size`. Without the children's
/// minimums summing upward, a parent is shrunk to a width its own contents
/// then overflow, and nothing anywhere reports it.
#[test]
fn a_parent_cannot_be_squeezed_past_what_its_children_refuse() {
    let root = row([row([
        block(100., 20.).id("a").min_size(Size::new(40., 0.)),
        block(100., 20.).id("b").min_size(Size::new(30., 0.)),
    ])
    .id("in")])
    .id("out");
    // 70 is exactly the two floors, and both children land on theirs.
    let l = resolve(&root, Some(Size::new(70., 20.)), Limits::default()).unwrap();
    assert_eq!(l.frame("a").unwrap().size.width, 40.);
    assert_eq!(l.frame("b").unwrap().size.width, 30.);
    // A pixel under, and both keep their floors and overflow.
    let l = resolve(&root, Some(Size::new(69., 20.)), Limits::default()).unwrap();
    assert_eq!(l.frame("a").unwrap().size.width, 40.);
    assert_eq!(l.frame("b").unwrap().size.width, 30.);
    assert_eq!(l.min_size().width, 70.);

    // The floor also has to bind while the deficit is being shared out, not
    // only as a check afterwards. Given a squeezable sibling, `in` freezes
    // at 70 and the rest of the deficit goes to `c` -- an even split would
    // have put `in` at 50, under a floor it never declared itself.
    let pair = row([root, block(200., 20.).id("c")]).id("pair");
    let l = resolve(&pair, Some(Size::new(100., 20.)), Limits::default()).unwrap();
    assert_eq!(l.frame("out").unwrap().size.width, 70.);
    assert_eq!(l.frame("c").unwrap().size.width, 30.);
}

#[test]
fn align_self_overrides_the_parent_for_one_child() {
    let l = resolve(
        &col([
            block(20., 10.).id("a"),
            block(20., 10.).id("b").align_self(Align::End),
        ])
        .id("c")
        .align(Align::Start),
        Some(Size::new(100., 20.)),
        Limits::default(),
    )
    .unwrap();
    assert_eq!(l.frame("a").unwrap().x, 0.);
    assert_eq!(l.frame("b").unwrap().x, 80.);
}

#[test]
fn containers_stretch_and_content_centres_without_being_told() {
    let l = resolve(
        &col([
            block(20., 10.).id("label"),
            row([block(10., 10.)]).id("bar"),
        ]),
        Some(Size::new(100., 20.)),
        Limits::default(),
    )
    .unwrap();
    assert_eq!(l.frame("label").unwrap().x, 40.);
    assert_eq!(l.frame("bar").unwrap().size.width, 100.);
}

#[test]
fn percent_is_a_share_of_the_parent_and_auto_while_hugging() {
    let t = row([
        row([block(10., 10.)]).id("a").w(Len::Pct(25.)),
        block(10., 10.).id("b"),
    ])
    .id("r");
    let l = resolve(&t, Some(Size::new(200., 10.)), Limits::default()).unwrap();
    assert_eq!(l.frame("a").unwrap().size.width, 50.);
    assert_eq!(
        resolve(&t, None, Limits::default()).unwrap().size.width,
        20.
    );
}

#[test]
fn aspect_derives_the_missing_axis() {
    // Width first: a column child takes the column's width, a row child its
    // allocated share, and the height follows either way.
    let knob = || Node::content().id("k").aspect(2.);
    let l = resolve(&col([knob()]).w(100.), None, Limits::default()).unwrap();
    assert_eq!(l.frame("k").unwrap().size, Size::new(100., 50.));
    let l = resolve(
        &row([knob().flex(1.), knob().id("j").flex(1.)]),
        Some(Size::new(80., 40.)),
        Limits::default(),
    )
    .unwrap();
    assert_eq!(l.frame("k").unwrap().size, Size::new(40., 20.));
    // A fixed height derives the width at measure, so hugging works too.
    let l = resolve(
        &row([block(0., 0.).id("h").h(30.).w(Len::Auto).aspect(0.5)]),
        None,
        Limits::default(),
    )
    .unwrap();
    assert_eq!(l.size, Size::new(15., 30.));
}

#[test]
fn grid_flows_equal_columns_and_shares_vertical_surplus() {
    let cells = (0..5).map(|i| block(10., 10.).id(format!("c{i}")));
    let g = grid(2, cells).id("g").gap(4.);
    let hug = resolve(&g, None, Limits::default()).unwrap();
    assert_eq!(hug.size, Size::new(24., 38.));
    let l = resolve(&g, Some(Size::new(104., 68.)), Limits::default()).unwrap();
    // columns 50 wide, rows 20 tall: leaves centre in their cells.
    assert_eq!(l.frame("c1").unwrap().x, 54. + 20.);
    assert_eq!(l.frame("c4").unwrap().y, 48. + 5.);
}

#[test]
fn overlay_children_anchor_themselves_and_nudge() {
    let t = stack([
        row([]).id("fill"),
        block(10., 10.)
            .id("badge")
            .anchor(Align::End, Align::Start)
            .offset(-2., 2.),
    ])
    .id("o");
    let l = resolve(&t, Some(Size::new(100., 50.)), Limits::default()).unwrap();
    assert_eq!(l.frame("fill").unwrap().size, Size::new(100., 50.));
    let b = l.frame("badge").unwrap();
    assert_eq!((b.x, b.y), (88., 2.));
}

#[test]
fn space_around_and_evenly() {
    let r = |j| {
        let l = resolve(
            &row([block(10., 10.).id("a"), block(10., 10.).id("b")]).justify(j),
            Some(Size::new(100., 10.)),
            Limits::default(),
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
    let t = Node::<&str>::col([Node::content().with("hello").id("t")]).id("c");
    let l = resolve_with(
        &t,
        None,
        Limits::default(),
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
    let t = Node::<&str>::col([
        Node::content().with("a").id("a"),
        Node::<&str>::grid(2, [Node::content().with("g").id("g")]).gap(10.),
        Node::<&str>::col([Node::content().with("s").id("s")]).scroll(),
    ])
    .w(Len::Px(100.))
    .pad(10.);
    let mut seen = std::collections::BTreeMap::new();
    let wrap = |s: &&str, room: Option<f64>| {
        seen.insert(s.to_string(), room);
        Size::new(room.map_or(200., |r| r.min(200.)), 12.)
    };
    let l = resolve_with(&t, None, Limits::default(), SpacingScale::DEFAULT, wrap).unwrap();
    assert_eq!(seen["a"], Some(80.));
    assert_eq!(seen["g"], Some(35.));
    assert_eq!(seen["s"], None);
    assert_eq!(l.frame("a").unwrap().size.width, 80.);
}

#[test]
fn tokens_resolve_against_the_scale_and_frames_come_out_in_tree_order() {
    let t = col([block(10., 10.).id("a"), block(10., 10.)])
        .gap(SpacingToken::M)
        .pad(SpacingToken::S);
    let l = resolve(&t, None, Limits::default()).unwrap();
    assert_eq!(l.size, Size::new(26., 48.));
    assert_eq!(l.all().len(), 3);
    assert_eq!(l.all()[1], l.frame("a").unwrap());
    assert_eq!(l.all()[2].y, 30.);
}

#[test]
fn a_scroll_column_overflows_and_slides_and_a_float_takes_no_space() {
    let list = |dy: f64| {
        col([
            block(50., 30.).id("a"),
            block(50., 30.).id("b"),
            block(50., 30.).id("c"),
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
    let head = || block(60., 20.).shrink(0.);
    let t = col([head(), list(0.)]).size(60., 80.).id("root");
    let l = resolve(&t, None, Limits::default()).unwrap();
    assert_eq!(l.frame("list").unwrap().size, Size::new(60., 60.));
    assert_eq!(l.frame("a").unwrap().size, Size::new(50., 30.));
    assert_eq!(l.frame("c").unwrap().y, 20. + 5. + 80.);
    // Scrolled by 40: everything slides up, the frame stays put.
    let t = col([head(), list(40.)]).size(60., 80.);
    let l = resolve(&t, None, Limits::default()).unwrap();
    assert_eq!(l.frame("list").unwrap().y, 20.);
    assert_eq!(l.frame("a").unwrap().y, 25. - 40.);
    // Without scroll the rows themselves get squeezed.
    let plain = col([block(50., 30.).id("p"), block(50., 30.)])
        .gap(10.)
        .pad(5.);
    let l = resolve(
        &col([head(), plain]).size(60., 70.),
        None,
        Limits::default(),
    )
    .unwrap();
    assert!(l.frame("p").unwrap().size.height < 30.);
    // A float sits in the parent's padding box and does not widen the row.
    let t = row([
        block(10., 10.).id("x"),
        block(80., 80.)
            .float()
            .anchor(Align::End, Align::Start)
            .offset(0., 4.)
            .id("tip"),
    ])
    .pad(2.)
    .id("r");
    let l = resolve(&t, None, Limits::default()).unwrap();
    assert_eq!(l.frame("r").unwrap().size, Size::new(14., 14.));
    let tip = l.frame("tip").unwrap();
    assert_eq!((tip.x, tip.y), (2. + 10. - 80., 2. + 4.));
    assert_eq!(l.all()[2], tip, "frames stay in declaration order");
}

#[test]
fn a_wrapping_row_breaks_into_lines_and_grows_its_cross_size() {
    let cells = (0..5).map(|_| block(30., 10.));
    let t = col([row(cells).id("r").wrap()]);
    let l = resolve(&t, Some(Size::new(100., 200.)), Limits::default()).unwrap();
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
            block(10., 10.).id("a"),
            block(10., 10.).id("b").span(2),
            block(10., 10.).id("c"),
        ],
    );
    let l = resolve(&t, Some(Size::new(30., 20.)), Limits::default()).unwrap();
    // b owns columns 1 and 2 -- 20 px wide -- and centres its 10 px in them.
    assert_eq!(l.frame("b").unwrap().center().0, 20.);
    assert_eq!(l.frame("c").unwrap().center().0, 5.);
    assert!(l.frame("c").unwrap().y > l.frame("a").unwrap().y);
}

#[test]
fn order_moves_the_placement_but_not_the_frame() {
    let t = row([block(10., 10.).id("a").order(1), block(20., 10.).id("b")]);
    let l = resolve(&t, None, Limits::default()).unwrap();
    assert_eq!(l.frame("b").unwrap().x, 0.);
    assert_eq!(l.frame("a").unwrap().x, 20.);
    // frames stay in declaration order: root, a, b.
    assert_eq!(l.all()[1], l.frame("a").unwrap());
}

#[test]
fn push_appends_a_child_and_keeps_pre_order_frames() {
    let t = col([block(10., 10.).id("a")])
        .id("c")
        .push(block(10., 10.).id("b").float());
    let l = resolve(&t, None, Limits::default()).unwrap();
    assert_eq!(l.all().len(), 3);
    assert_eq!(l.all()[0], l.frame("c").unwrap());
    assert_eq!(l.all()[2], l.frame("b").unwrap());
    // A block becomes a stack of its own size to hold one.
    assert_eq!(block(1., 1.).push(block(1., 1.)).children().len(), 1);
}

#[test]
fn a_content_leaf_never_keeps_a_cross_extent_wider_than_its_parent() {
    // Both columns take half the row; the text measured at the row's width
    // must be clamped to its own column, not centred half outside it.
    let t = Node::<&str>::row([
        Node::<&str>::col([Node::content().with("p").id("a")]).id("ca"),
        Node::<&str>::col([Node::content().with("p")]).id("cb"),
    ]);
    let l = resolve_with(
        &t,
        Some(Size::new(400., 300.)),
        Limits::default(),
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
    let t = row([block(0., 0.).pad(5.4).h(Len::Pct(84.6))]);
    assert!(resolve(&t, None, Limits::default()).is_ok());
}

#[test]
fn a_declared_size_smaller_than_its_own_padding_is_not_a_squeeze() {
    assert!(resolve(&block(10., 10.).pad(6.), None, Limits::default()).is_ok());
    assert!(
        resolve(
            &row([block(0., 0.).pad(5.9)]),
            Some(Size::new(354., 231.6)),
            Limits::default()
        )
        .is_ok()
    );
}

#[test]
fn grid_columns_never_go_negative_when_the_gaps_outgrow_the_grid() {
    // 21 wide less 15.4 of padding leaves 5.6 for two columns and an 8.4
    // gap. The column is zero-wide, not -1.4, so the percentage child is
    // offered a valid width and the grid overflows the squeeze it really is.
    let t = col([grid(2, [Node::block(0., 0.).w(Len::Pct(98.8))]).gap(8.4)]).pad(7.7);
    assert!(resolve(&t, Some(Size::new(21., 174.4)), Limits::default()).is_ok());
}

#[test]
fn room_handed_to_the_measurer_leaves_out_the_nodes_own_padding() {
    let t = Node::<&str>::stack([Node::content().with("t").id("t").pad(4.6)]);
    let l = resolve_with(
        &t,
        Some(Size::new(170., 340.)),
        Limits::default(),
        SpacingScale::DEFAULT,
        |_: &&str, room: Option<f64>| Size::new(room.unwrap_or(170.), 12.),
    )
    .unwrap();
    let f = l.frame("t").unwrap();
    assert!(f.x >= 0. && f.size.width <= 170. + 1e-9);
}

#[test]
fn a_grid_cell_never_outgrows_its_column() {
    let t = grid(3, [col([]).w(Len::Px(108.5)).id("c")]).id("g");
    let l = resolve(&t, Some(Size::new(135.5, 62.4)), Limits::default()).unwrap();
    let c = l.frame("c").unwrap();
    // The declared width is wider than the track: it starts at the track and
    // is cut to it, rather than painting through the next cell.
    assert_eq!(c.x, 0.);
    assert!((c.size.width - 135.5 / 3.).abs() < 1e-9);
    // Two cells, the first declared three times its column.
    let t = grid(2, [block(300., 10.).id("a"), block(10., 10.).id("b")]).gap(8.);
    let l = resolve(&t, Some(Size::new(200., 40.)), Limits::default()).unwrap();
    let (a, b) = (l.frame("a").unwrap(), l.frame("b").unwrap());
    assert_eq!(a.size.width, 96.);
    assert!(b.x >= a.right() && b.right() <= 200.);
}

#[test]
fn a_float_is_pulled_back_inside_a_thin_window() {
    let menu = || block(120., 40.).float().at(200., 120.).id("menu");
    let t = col([block(10., 10.)]).push(menu()).id("root");
    let l = resolve(&t, Some(Size::new(240., 300.)), Limits::default()).unwrap();
    let f = l.frame("menu").unwrap();
    assert_eq!((f.x, f.y), (120., 120.));
    assert!(f.right() <= 240. && f.bottom() <= 300.);
    // One too big to fit has no inside to be pulled to: it keeps its offset.
    let t = col([block(10., 10.)])
        .push(menu().size(400., 40.))
        .id("root");
    let l = resolve(&t, Some(Size::new(240., 300.)), Limits::default()).unwrap();
    assert_eq!(l.frame("menu").unwrap().x, 200.);
}

#[test]
fn a_squeezed_wrapping_row_overflows_its_cross_axis() {
    let bar = || {
        col([row([block(250., 40.), block(250., 40.)])
            .gap(10.)
            .wrap()
            .id("bar")])
    };
    // 510 wide: one line, and the row is one child tall.
    let l = resolve(&bar(), Some(Size::new(510., 40.)), Limits::default()).unwrap();
    assert_eq!(l.frame("bar").unwrap().size, Size::new(510., 40.));
    // Squeezed to 420 the row needs a second line, and the 40 px it was given
    // on the cross axis is a line short: the second line hangs below it.
    let l = resolve(&bar(), Some(Size::new(420., 40.)), Limits::default()).unwrap();
    assert_eq!(l.frame("bar").unwrap().size, Size::new(420., 40.));
    assert_eq!(l.all()[3].y, 50.);
    // Given the stacked height it fits, both children inside the frame.
    let l = resolve(&bar(), Some(Size::new(420., 90.)), Limits::default()).unwrap();
    let f = l.frame("bar").unwrap();
    assert!(l.all()[2..].iter().all(|c| c.bottom() <= f.bottom() + 1e-9));
}

#[test]
fn an_over_constrained_tree_overflows_and_says_what_it_needed() {
    let panel = || col([]).min_w(120.).min_h(30.).id("p2");
    let t = row([col([]).min_w(120.).min_h(30.), panel()])
        .gap(10.)
        .id("bar");
    // 240 is ten short of the two panels and their gap: it lays out anyway,
    // both panels at their floors, the second hanging past the window.
    let l = resolve(&t, Some(Size::new(240., 40.)), Limits::default()).unwrap();
    let p = l.frame("p2").unwrap();
    assert_eq!((p.x, p.size.width), (130., 120.));
    let needs = l.min_size();
    assert_eq!(needs, Size::new(250., 30.));
    // Which is the whole point: the host can scale by it.
    assert!((240. / needs.width - 0.96).abs() < 1e-9);
}

#[test]
fn growth_without_a_declared_maximum_is_not_capped_at_a_magic_number() {
    let t = row([block(1., 1.).id("a").grow(1.)]).id("r");
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
        col(
            [grid(3, (0..6).map(|i| block(90., 54.).id(format!("c{i}"))))
                .gap(10.)
                .min_col(120.)
                .id("grid")],
        )
        .pad(16.)
    };
    let cols = |w: f64, h: f64| {
        let l = resolve(&tree(), Some(Size::new(w, h)), Limits::default()).unwrap();
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
            block(100., 20.).id("side"),
            grid(3, (0..6).map(|i| block(90., 54.).id(format!("c{i}"))))
                .gap(10.)
                .min_col(120.)
                .grow(1.0)
                .id("grid"),
        ])
    };
    let cols = |w: f64| {
        let l = resolve(&tree(w), Some(Size::new(w, 600.)), Limits::default()).unwrap();
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
        block(200., 20.).id("side").shrink(0.0),
    ]);
    let l = resolve_with(
        &root,
        Some(Size::new(300., 400.)),
        Limits::default(),
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

#[test]
fn a_flex_overlay_with_percentage_body_is_remeasured_at_its_share() {
    // A rack is an overlay over a scrolling column. Its body fills the rack
    // by percentage, so its first intrinsic pass must not pin the rack to the
    // whole row's width before flex distribution gives it a share.
    let rack = |id: &str, minimum: f64| {
        stack([col([block(32., 30.).w(Len::Pct(100.))])
            .w(Len::Pct(100.))
            .h(Len::Pct(100.))
            .scroll()])
        .flex(1.0)
        .min_w(minimum)
        .id(id)
    };
    let tree = row([rack("warps", 120.), rack("synth", 120.)])
        .gap(8.)
        .w(Len::Pct(100.))
        .h(30.);
    let layout = resolve(&tree, Some(Size::new(300., 30.)), Limits::default())
        .expect("percentage bodies must follow their flex share");
    assert_eq!(layout.frame("warps").unwrap().size.width, 146.);
    assert_eq!(layout.frame("synth").unwrap().size.width, 146.);
}

#[test]
fn a_zero_basis_flex_rack_remeasures_wrapped_controls_at_its_final_width() {
    // The first pass sees the rack's zero-width basis, so every control lands
    // on its own line. The final flex share is wide enough for three controls
    // per line; retaining the first cross size would leave a six-line hole.
    let controls = row((0..6).map(|i| block(80., 44.7).id(format!("control-{i}"))))
        .wrap()
        .gap(4.)
        .w(Len::Pct(100.))
        .id("controls");
    let rack = stack([col([controls]).w(Len::Pct(100.))])
        .w(Len::Px(0.))
        .flex(1.)
        .h(Len::Pct(100.))
        .id("rack");
    let root = row([rack]).w(Len::Pct(100.)).h(Len::Pct(100.));
    let layout = resolve(&root, Some(Size::new(300., 400.)), Limits::default())
        .expect("the flex rack resolves");
    let row = layout.frame("controls").expect("wrapped controls");
    assert_eq!(layout.frame("rack").unwrap().size.width, 300.);
    assert!((row.size.height - 93.4).abs() < 1e-9, "{row:?}");
    let lines: Vec<_> = (0..6)
        .map(|i| layout.frame(&format!("control-{i}")).unwrap().y)
        .collect();
    assert_eq!(&lines[..3], &[0.0, 0.0, 0.0]);
    assert_eq!(&lines[3..], &[48.7, 48.7, 48.7]);
}

#[test]
fn a_sized_flex_rack_remeasures_wrapped_controls_after_growth_and_shrink() {
    let layout_for = |basis| {
        let controls = row((0..6).map(|i| block(80., 44.7).id(format!("control-{i}"))))
            .wrap()
            .gap(4.)
            .w(Len::Pct(100.))
            .id("controls");
        let rack = stack([col([controls]).w(Len::Pct(100.))])
            .w(Len::Px(basis))
            .grow(1.)
            .shrink(1.)
            .h(Len::Pct(100.))
            .id("rack");
        let root = row([rack]).w(Len::Pct(100.)).h(Len::Pct(100.));
        resolve(&root, Some(Size::new(300., 400.)), Limits::default())
            .expect("the sized flex rack resolves")
    };

    for basis in [80., 400.] {
        let layout = layout_for(basis);
        let controls = layout.frame("controls").expect("wrapped controls");
        assert_eq!(layout.frame("rack").unwrap().size.width, 300.);
        assert!(
            (controls.size.height - 93.4).abs() < 1e-9,
            "basis={basis}: {controls:?}"
        );
    }
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

/// A float appended to a `fits` -- a tooltip -- is no candidate: the
/// fallback still wins when nothing fits, and the float still shows.
#[test]
fn a_float_in_fits_is_not_a_candidate() {
    let tree = Node::fits([
        block(200., 20.).id("wide"),
        block(100., 20.).id("thin"),
        block(30., 10.).float().id("tip"),
    ]);
    let l = resolve(&tree, Some(Size::new(60., 20.)), Limits::default()).unwrap();
    assert!(l.frame("thin").unwrap().size.width > 0.0);
    assert_eq!(l.frame("tip").unwrap().size, Size::new(30., 10.));
}

/// Three real subtrees, largest first, at the three window widths a plugin
/// is dragged to: the richest one that fits wins, the losers get empty
/// frames in their own slots, and nothing is built twice.
#[test]
fn fits_keeps_the_first_candidate_that_clears_the_offered_width() {
    let head = || {
        Node::fits([
            row([
                block(120., 20.).id("name"),
                block(80., 20.).id("version"),
                block(80., 20.).id("meter"),
            ])
            .gap(10.)
            .id("wide"),
            row([block(120., 20.).id("name2"), block(80., 20.).id("version2")])
                .gap(10.)
                .id("mid"),
            block(120., 20.).id("thin"),
        ])
        .id("head")
    };
    let shown = |w: f64| {
        let l = resolve(&head(), Some(Size::new(w, 20.)), Limits::default()).unwrap();
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
        &col([block(0., 40.), head()]),
        Some(Size::new(60., 60.)),
        Limits::default(),
    )
    .unwrap();
    assert_eq!(l.frame("name").unwrap().size, Size::ZERO);
    assert_eq!(l.frame("name").unwrap().y, l.frame("head").unwrap().y);
}

/// A percentage of the parent and a percentage of the container are the same
/// number until the parent stops having a size of its own.
#[test]
fn a_container_share_skips_the_hugging_parent_between() {
    let bar = |len: Len| row([row([block(0., 8.).w(len).id("bar")]).id("hug")]).w(200.);
    let at = |len: Len| {
        resolve(&bar(len), Some(Size::new(200., 8.)), Limits::default())
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
        let t = row([row([block(0., 8.).w(len).id("bar")]).w(80.)]).w(200.);
        resolve(&t, Some(Size::new(200., 8.)), Limits::default())
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
        let m = block(80., 60.)
            .pin(
                Pin::to("field")
                    .area(Area::BottomStart)
                    .gap(gap)
                    .fallback(Area::TopStart),
            )
            .id("menu");
        let field = block(80., 24.).anchor(Align::Start, Align::End).id("field");
        resolve(
            &stack([field, m]),
            Some(Size::new(200., 100.)),
            Limits::default(),
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
    let tree = stack([
        block(90., 24.)
            .anchor(Align::Start, Align::Start)
            .id("field"),
        block(10., 20.)
            .pin(Pin::to("field").area(Area::BottomStart).match_width())
            .id("menu"),
    ]);
    let l = resolve(&tree, Some(Size::new(200., 200.)), Limits::default()).unwrap();
    assert_eq!(l.frame("menu").unwrap().size.width, 90.);
    assert_eq!(l.frame("menu").unwrap().y, 24.);
}

#[test]
fn a_sticky_header_holds_the_viewport_edge_until_its_section_ends() {
    let section = |k: &str| {
        col([
            block(100., 20.).id(format!("{k}-head")).sticky(),
            block(100., 180.).id(k),
        ])
    };
    let at = |dy: f64| {
        let list = col([section("a"), section("b")])
            .scroll()
            .scrolled(0., dy)
            .id("list");
        let l = resolve(&list, Some(Size::new(100., 120.)), Limits::default()).unwrap();
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
        col([
            block(40., 30.).min_size(Size::new(40., 30.)),
            block(40., 30.).min_size(Size::new(40., 30.)),
        ])
        .gap(4.)
        .pad(8.)
    };
    // Nothing here can shrink, so the floor is the intrinsic pass exactly.
    let hug = resolve(&rows(), None, Limits::default()).unwrap();
    let l = resolve(&rows(), Some(Size::new(400., 300.)), Limits::default()).unwrap();
    assert_eq!(l.min_size(), hug.size);
    assert_eq!(l.min_size(), Size::new(56., 80.));
    // The same rows behind a scroll cost nothing on the scrolling axis: a host
    // may make the window as short as the padding.
    let scrolled = resolve(
        &rows().scroll(),
        Some(Size::new(400., 300.)),
        Limits::default(),
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
        col(
            [grid(2, (0..4).map(|i| block(90., 40.).id(format!("c{i}"))))
                .gap(10.)
                .min_col(min)
                .id("grid")],
        )
        .pad(12.)
        .id("modal")
    };
    let l = resolve(&modal(160.), None, Limits::default()).unwrap();
    // Two 160-wide columns and one gap, not two 90-wide cells.
    assert_eq!(l.frame("grid").unwrap().size.width, 330.);
    assert_eq!(l.size.width, 354.);
    // A minimum the content already clears changes nothing.
    let l = resolve(&modal(80.), None, Limits::default()).unwrap();
    assert_eq!(l.frame("grid").unwrap().size.width, 190.);
    // And the floor is still the cells', so a squeeze past the hug is a
    // column drop, not an error: `min_col` widens, it does not pin.
    let l = resolve(&modal(160.), Some(Size::new(200., 300.)), Limits::default()).unwrap();
    let top = l.frame("c0").unwrap().y;
    assert_eq!(
        (0..4)
            .filter(|i| l.frame(&format!("c{i}")).unwrap().y == top)
            .count(),
        1
    );
}

#[test]
fn a_row_shrinks_content_only_down_to_its_min_content() {
    // Both 100 wide; "a" may go to 60, "b" to 20.
    let t = Node::row([
        Node::content().with(60.).id("a"),
        Node::content().with(20.).id("b"),
    ]);
    let solve = |w: f64| {
        resolve_with(
            &t,
            Some(Size::new(w, 10.)),
            Limits::default(),
            SpacingScale::DEFAULT,
            |min: &f64, _| Intrinsic {
                size: Size::new(100., 10.),
                min_width: *min,
            },
        )
        .unwrap()
    };
    let w = |l: &Layout, k| l.frame(k).unwrap().size.width;
    // An even squeeze would put "a" at 50: it stops at 60, "b" takes the rest.
    let l = solve(100.);
    assert_eq!((w(&l, "a"), w(&l, "b")), (60., 40.));
    // Below both floors the row overflows instead of refusing.
    let l = solve(50.);
    assert_eq!((w(&l, "a"), w(&l, "b")), (60., 20.));
    assert_eq!(l.min_size().width, 80.);
    // An explicit `shrink(0)` still never shrinks.
    let t = Node::row([Node::content().with(60.).id("a").shrink(0.)]);
    let l = resolve_with(
        &t,
        Some(Size::new(50., 10.)),
        Limits::default(),
        SpacingScale::DEFAULT,
        |_: &f64, _| Size::new(100., 10.),
    )
    .unwrap();
    assert_eq!(w(&l, "a"), 100.);
}
/// KURV's parameter well: a wrapping row, width 100% through padded
/// columns, in a hugging column beside a fixed port. Measure used to break
/// the lines at one width and arrange at a narrower one, so a third line
/// hung below the row's two-line height.
#[test]
fn a_wrap_row_under_a_flex_share_arranges_the_lines_it_measured() {
    let ws = [65., 66.6, 66.6, 79.7, 70., 50., 52., 50., 48.];
    // `above` adds a sibling outside the row, so a warm solve rebinds the
    // whole row from the cache rather than measuring it.
    let tree = |above: bool| {
        let cells = ws
            .iter()
            .enumerate()
            .map(|(i, w)| block(*w, 36.).id(format!("c{i}")).grow(1.));
        let params = Node::row(cells)
            .wrap()
            .gap(8.)
            .w(Len::Pct(100.))
            .id("params");
        let well = col([params]).pad(8.).w(Len::Pct(100.));
        let body = col([block(10., 24.), col([well]).w(Len::Pct(100.))])
            .pad(8.)
            .id("body");
        let port = block(24., 72.).id("port");
        let row = row([body.grow(1.).shrink(1.).min_w(0.), port])
            .gap(12.)
            .w(Len::Pct(100.))
            .id("row");
        let above = above.then(|| block(10., 10.)).into_iter();
        col(above.chain([row])).w(360.)
    };
    let offered = Some(Size::new(360., 650.));
    let mut cache = LayoutCache::default();
    let mut cached = |above: bool| {
        let solve = |_: &(), _: Option<f64>| Size::ZERO;
        let (limits, scale) = (Limits::default(), SpacingScale::DEFAULT);
        resolve_cached_with(
            &tree(above),
            offered,
            limits,
            scale,
            &mut cache,
            |_, _| {},
            solve,
        )
        .unwrap()
    };
    let layouts = [
        resolve(&tree(false), offered, Limits::default()).unwrap(),
        cached(false),
        cached(false),
        cached(true),
    ];
    assert!(cache.stats().measure_hits > 0);
    for l in layouts {
        let p = l.frame("params").unwrap();
        for i in 0..ws.len() {
            let c = l.frame(&format!("c{i}")).unwrap();
            assert!(c.bottom() <= p.bottom() + 1e-9, "c{i} below the row");
        }
        let (body, port) = (l.frame("body").unwrap(), l.frame("port").unwrap());
        assert!((body.size.width + 12. + port.size.width - 360.).abs() < 1e-9);
        // Its longest line, not all nine cells, is the body's flex basis.
        assert!(port.size.width > 23., "{}", port.size.width);
    }
}
