use mui_layout::{
    GridTrack as T, LayoutCache, Len, Limits, Node, Size, SpacingScale, resolve,
    resolve_cached_with,
};

fn rectangle(layout: &mui_layout::Layout, id: &str, expected: [f64; 4]) {
    let frame = layout.frame(id).unwrap();
    let actual = [frame.x, frame.y, frame.size.width, frame.size.height];
    for (actual, expected) in actual.into_iter().zip(expected) {
        assert!(
            (actual - expected).abs() < 1e-7,
            "{id}: {frame:?}, expected {expected}"
        );
    }
}
fn fixture(tree: &Node, offered: Option<Size>, expected: &[(&str, [f64; 4])]) {
    let fresh = resolve(tree, offered, Limits::default()).unwrap();
    for (id, rect) in expected {
        rectangle(&fresh, id, *rect);
    }
    let mut cache = LayoutCache::default();
    for _ in 0..3 {
        let cached = resolve_cached_with(
            tree,
            offered,
            Limits::default(),
            SpacingScale::default(),
            &mut cache,
            |_, _| {},
            |_, _| Size::ZERO,
        )
        .unwrap();
        assert_eq!(cached.all(), fresh.all());
        assert_eq!(cached.min_size(), fresh.min_size());
    }
}
fn row(a: Node, b: Node, width: f64) -> Node {
    Node::row([a.id("a"), b.id("b")]).size(width, 20.)
}
fn cell(id: &str) -> Node {
    Node::stack([]).id(id)
}

/// Independent numerical rectangles and warm-cache equivalence checks. Run
/// with `cargo run -p mui-layout --example conformance` or the integration test.
pub fn check() {
    fixture(
        &row(
            Node::block(200., 20.).max_size((100., 20.)),
            Node::block(200., 20.),
            250.,
        ),
        None,
        &[("a", [0., 0., 100., 20.]), ("b", [100., 0., 150., 20.])],
    );
    fixture(
        &row(
            Node::block(300., 20.).min_w(250.),
            Node::block(100., 20.),
            300.,
        ),
        None,
        &[("a", [0., 0., 250., 20.]), ("b", [250., 0., 50., 20.])],
    );
    fixture(
        &row(
            Node::block(20., 20.).grow(1.).max_size((40., 20.)),
            Node::block(20., 20.).grow(1.),
            100.,
        ),
        None,
        &[("a", [0., 0., 40., 20.]), ("b", [40., 0., 60., 20.])],
    );
    fixture(
        &row(
            Node::block(20., 20.).grow(0.25),
            Node::block(20., 20.).grow(0.25),
            100.,
        ),
        None,
        &[("a", [0., 0., 35., 20.]), ("b", [35., 0., 35., 20.])],
    );
    fixture(
        &row(
            Node::block(100., 20.).shrink(0.25),
            Node::block(100., 20.).shrink(0.25),
            100.,
        ),
        None,
        &[("a", [0., 0., 75., 20.]), ("b", [75., 0., 75., 20.])],
    );
    fixture(
        &row(
            Node::block(Len::Pct(100.), 20.).basis(40.).shrink(0.),
            Node::block(60., 20.),
            100.,
        ),
        None,
        &[("a", [0., 0., 40., 20.]), ("b", [40., 0., 60., 20.])],
    );
    fixture(
        &row(
            Node::block(20., 20.).min_w(100.),
            Node::block(20., 20.),
            100.,
        ),
        None,
        &[("a", [0., 0., 100., 20.]), ("b", [100., 0., 0., 20.])],
    );
    fixture(
        &Node::block(20., Len::Auto).aspect(2.).min_w(100.).id("a"),
        None,
        &[("a", [0., 0., 100., 50.])],
    );
    fixture(
        &Node::block(100., Len::Auto)
            .aspect(2.)
            .max_size((50., 1000.))
            .id("a"),
        None,
        &[("a", [0., 0., 50., 25.])],
    );
    let tracks = [T::Px(40.), T::Pct(25.), T::Fr(1.), T::Fr(2.)];
    let grid = Node::grid(4, [cell("a").span(2), cell("b").span(2)])
        .grid_tracks(tracks)
        .gap(10.)
        .size(400., 20.);
    fixture(
        &grid,
        None,
        &[("a", [0., 0., 150., 20.]), ("b", [160., 0., 240., 20.])],
    );
    let grid = Node::grid(
        3,
        [
            Node::block(40., 20.).id("a"),
            Node::block(80., 20.).id("b"),
            cell("c"),
        ],
    )
    .grid_tracks([T::Px(40.), T::Auto, T::Fr(1.)])
    .gap(10.)
    .size(300., 20.);
    fixture(
        &grid,
        None,
        &[
            ("a", [0., 0., 40., 20.]),
            ("b", [50., 0., 80., 20.]),
            ("c", [140., 0., 160., 20.]),
        ],
    );
    let grid = Node::grid(2, [cell("a"), cell("b")])
        .grid_tracks([
            T::MinMax { min: 30., max: 60. },
            T::MinMax {
                min: 20.,
                max: 120.,
            },
        ])
        .gap(10.)
        .size(200., 20.);
    fixture(
        &grid,
        None,
        &[("a", [0., 0., 60., 20.]), ("b", [70., 0., 120., 20.])],
    );
    let grid = Node::grid(
        2,
        [
            Node::block(60., 20.).id("a"),
            Node::block(100., 20.).id("b"),
        ],
    )
    .grid_tracks([T::Auto, T::Auto])
    .gap(10.);
    fixture(
        &grid,
        None,
        &[("a", [0., 0., 60., 20.]), ("b", [70., 0., 100., 20.])],
    );
    let span = Node::grid(
        2,
        [Node::block(180., 20.).span(2).id("a"), cell("b"), cell("c")],
    )
    .grid_tracks([T::Px(40.), T::Auto])
    .gap(10.);
    fixture(
        &span,
        None,
        &[
            ("a", [0., 0., 180., 20.]),
            ("b", [0., 30., 40., 0.]),
            ("c", [50., 30., 130., 0.]),
        ],
    );
    let grid = Node::grid(2, [cell("a").grid_row_span(2), cell("b"), cell("c")])
        .grid_tracks([T::Px(50.), T::Fr(1.)])
        .grid_rows([T::Px(30.), T::Fr(1.)])
        .gap(10.)
        .size(200., 100.);
    fixture(
        &grid,
        None,
        &[
            ("a", [0., 0., 50., 100.]),
            ("b", [60., 0., 140., 30.]),
            ("c", [60., 40., 140., 60.]),
        ],
    );
    // Explicitly positioned items reserve cells before earlier automatic ones.
    let grid = Node::grid(
        2,
        [
            cell("a"),
            cell("b").grid_at(0, 0).grid_row_span(2),
            cell("c"),
        ],
    )
    .grid_tracks([T::Px(50.), T::Fr(1.)])
    .grid_rows([T::Px(30.), T::Fr(1.)])
    .gap(10.)
    .size(200., 100.);
    fixture(
        &grid,
        None,
        &[
            ("a", [60., 0., 140., 30.]),
            ("b", [0., 0., 50., 100.]),
            ("c", [60., 40., 140., 60.]),
        ],
    );
    let grid = Node::grid(
        2,
        [cell("a").span(2).grid_row_span(2), cell("b"), cell("c")],
    )
    .grid_tracks([T::Fr(1.), T::Fr(1.)])
    .grid_rows([T::Px(20.), T::Px(30.), T::Px(40.)])
    .gap(5.)
    .size(105., 100.);
    fixture(
        &grid,
        None,
        &[
            ("a", [0., 0., 105., 55.]),
            ("b", [0., 60., 50., 40.]),
            ("c", [55., 60., 50., 40.]),
        ],
    );
    let nested = Node::grid(
        1,
        [Node::stack([cell("inner").size(Len::Pct(100.), Len::Pct(100.))]).id("a")],
    )
    .grid_rows([T::Px(50.)])
    .size(100., 50.);
    fixture(
        &nested,
        None,
        &[("a", [0., 0., 100., 50.]), ("inner", [0., 0., 100., 50.])],
    );
    let intrinsic_fraction = Node::grid(
        2,
        [Node::block(40., 20.).id("a"), Node::block(40., 20.).id("b")],
    )
    .grid_tracks([T::Fr(1.), T::Fr(2.)])
    .gap(10.);
    fixture(
        &intrinsic_fraction,
        None,
        &[("a", [0., 0., 40., 20.]), ("b", [70., 0., 40., 20.])],
    );
    let text_grid = Node::grid(1, [Node::content().id("a")])
        .grid_tracks([T::Auto])
        .w(50.);
    let text_metric = |_: &(), room: Option<f64>| mui_layout::Intrinsic {
        size: if room.is_some_and(|w| w <= 50.) {
            Size::new(50., 40.)
        } else {
            Size::new(100., 20.)
        },
        min_width: 20.,
    };
    let fresh = mui_layout::resolve_with(
        &text_grid,
        None,
        Limits::default(),
        SpacingScale::default(),
        text_metric,
    )
    .unwrap();
    rectangle(&fresh, "a", [0., 0., 50., 40.]);
    let mut text_cache = LayoutCache::default();
    for _ in 0..3 {
        let cached = resolve_cached_with(
            &text_grid,
            None,
            Limits::default(),
            SpacingScale::default(),
            &mut text_cache,
            |_, _| {},
            text_metric,
        )
        .unwrap();
        assert_eq!(cached.all(), fresh.all());
    }
    let max_content = Node::grid(2, [Node::block(80., 20.).id("a"), cell("b")])
        .grid_tracks([T::MaxContent, T::Fr(1.)])
        .gap(10.)
        .size(200., 20.);
    fixture(
        &max_content,
        None,
        &[("a", [0., 0., 80., 20.]), ("b", [90., 0., 110., 20.])],
    );
    let wrapped = Node::row([
        Node::content().basis(80.).grow(1.).id("a"),
        Node::block(10., 10.).id("b"),
    ])
    .wrap()
    .w(100.);
    let metric = |_: &(), room: Option<f64>| {
        if room.is_some_and(|w| w < 99.) {
            Size::new(90., 20.)
        } else {
            Size::new(100., 10.)
        }
    };
    let fresh = mui_layout::resolve_with(
        &wrapped,
        None,
        Limits::default(),
        SpacingScale::default(),
        metric,
    )
    .unwrap();
    rectangle(&fresh, "a", [0., 0., 90., 20.]);
    rectangle(&fresh, "b", [90., 5., 10., 10.]);
    let mut cache = LayoutCache::default();
    for _ in 0..3 {
        let cached = resolve_cached_with(
            &wrapped,
            None,
            Limits::default(),
            SpacingScale::default(),
            &mut cache,
            |_, _| {},
            metric,
        )
        .unwrap();
        assert_eq!(cached.all(), fresh.all());
    }
    fixture(
        &Node::grid(2, [cell("a").span(usize::MAX).grid_row_span(0)])
            .grid_tracks([T::Fr(1.), T::Fr(1.)])
            .size(100., 20.),
        None,
        &[("a", [0., 0., 100., 20.])],
    );
    for invalid in [
        Node::grid(usize::MAX, [cell("a")]),
        Node::grid(0, [cell("a")]),
        Node::grid(1, [cell("a")]).grid_tracks([T::Px(f64::NAN)]),
        Node::grid(1, [cell("a")]).grid_rows([T::Pct(f64::INFINITY)]),
        Node::grid(1, [cell("a").grid_at(usize::MAX, 0)]),
        Node::grid(1, [cell("a").grid_at(0, usize::MAX)]),
        Node::grid(1, [cell("a").grid_row_span(usize::MAX)]),
        Node::grid(1, [cell("a")]).grid_tracks([]),
        Node::grid(1, [cell("a")]).grid_tracks([T::Fr(-1.)]),
        Node::grid(1, [cell("a")]).grid_tracks([T::MinMax { min: 20., max: 10. }]),
        Node::grid(1, [cell("a").grid_at(2, 0)]),
        Node::row([cell("a")]).grid_rows([T::Auto]),
    ] {
        assert!(resolve(&invalid, None, Limits::default()).is_err());
    }
    let unlimited = Limits {
        nodes: usize::MAX,
        ..Limits::default()
    };
    assert!(resolve(&Node::grid(usize::MAX, [cell("a")]), None, unlimited).is_err());
    assert!(
        resolve(
            &Node::grid(1, [cell("a").grid_row_span(usize::MAX)]),
            None,
            unlimited
        )
        .is_err()
    );
    let budgeted = Node::grid(3, [cell("a").span(3).grid_row_span(3)]).grid_tracks([
        T::Fr(1.),
        T::Fr(1.),
        T::Fr(1.),
    ]);
    assert!(matches!(
        resolve(
            &budgeted,
            None,
            Limits {
                nodes: 8,
                ..Limits::default()
            }
        ),
        Err(mui_layout::Error::BudgetExceeded)
    ));
    let mut changed_cache = LayoutCache::default();
    for span in [1, 1, 2, 2] {
        for row_height in [20., 30.] {
            let tree = Node::grid(2, [cell("a").grid_at(0, 0).grid_row_span(span), cell("b")])
                .grid_rows([T::Px(row_height), T::Px(40.)])
                .size(100., 100.);
            let fresh = resolve(&tree, None, Limits::default()).unwrap();
            let cached = resolve_cached_with(
                &tree,
                None,
                Limits::default(),
                SpacingScale::default(),
                &mut changed_cache,
                |_, _| {},
                |_, _| Size::ZERO,
            )
            .unwrap();
            assert_eq!(cached.all(), fresh.all());
            rectangle(
                &cached,
                "a",
                [0., 0., 50., row_height + if span == 2 { 40. } else { 0. }],
            );
        }
    }
    // Track edits, reordering and resize must invalidate persistent snapshots.
    let mut cache = LayoutCache::default();
    for width in [300., 300., 300., 220., 220.] {
        for tracks in [[T::Px(40.), T::Fr(1.)], [T::Px(80.), T::Fr(2.)]] {
            let tree = Node::grid(2, [cell("a"), cell("b")]).grid_tracks(tracks);
            let offered = Some(Size::new(width, 20.));
            let fresh = resolve(&tree, offered, Limits::default()).unwrap();
            let cached = resolve_cached_with(
                &tree,
                offered,
                Limits::default(),
                SpacingScale::default(),
                &mut cache,
                |_, _| {},
                |_, _| Size::ZERO,
            )
            .unwrap();
            assert_eq!(cached.all(), fresh.all());
            rectangle(
                &cached,
                "a",
                [
                    0.,
                    0.,
                    match tracks[0] {
                        T::Px(w) => w,
                        _ => unreachable!(),
                    },
                    20.,
                ],
            );
        }
    }
}
#[test]
fn expected_rectangles_and_incremental_equivalence() {
    check();
}
