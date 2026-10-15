use super::*;

fn cached(root: &Node, offered: Size, cache: &mut LayoutCache) -> Layout {
    resolve_cached_with(
        root,
        Some(offered),
        Limits::default(),
        SpacingScale::DEFAULT,
        cache,
        |_, _| {},
        |_, _| Size::ZERO,
    )
    .unwrap()
}

#[test]
fn named_minimum_is_the_measured_floor_not_its_allocated_frame() {
    let root = col([
        Node::content().id("label"),
        block(40., 12.).min_size(Size::new(40., 12.)).id("cell"),
    ])
    .pad(4.)
    .id("root");
    let layout = resolve_with(
        &root,
        Some(Size::new(200., 100.)),
        Limits::default(),
        SpacingScale::DEFAULT,
        |_: &(), _| Intrinsic {
            size: Size::new(90., 18.),
            min_width: 35.,
        },
    )
    .unwrap();
    assert_eq!(layout.min_size_of("label"), Some(Size::new(35., 18.)));
    assert_eq!(layout.min_size_of("cell"), Some(Size::new(40., 12.)));
    assert_eq!(layout.min_size_of("root"), Some(Size::new(48., 38.)));
    assert_eq!(layout.min_size_of("root"), Some(layout.min_size()));
    assert!(layout.frame("label").unwrap().size.width > 35.);
    assert_eq!(layout.frame("root").unwrap().size, Size::new(200., 100.));
    assert_eq!(layout.min_size_of("missing"), None);
}

#[test]
fn cold_and_cached_minimum_receipts_match_and_warm_clones_share_storage() {
    let root = col([
        block(310., 24.).min_size(Size::new(310., 24.)).id("wide"),
        block(80., 40.).min_size(Size::new(80., 40.)).id("small"),
    ])
    .gap(5.)
    .id("root");
    let offered = Size::new(500., 100.);
    let oracle = resolve(&root, Some(offered), Limits::default()).unwrap();
    let mut cache = LayoutCache::default();
    assert_eq!(cached(&root, offered, &mut cache), oracle);
    let measured = cached(&root, offered, &mut cache);
    let warm = cached(&root, offered, &mut cache);
    assert_eq!(warm, oracle);
    assert_eq!(warm.min_size_of("wide"), Some(Size::new(310., 24.)));
    assert_eq!(warm.min_size_of("small"), Some(Size::new(80., 40.)));
    assert!(Arc::ptr_eq(&measured.minima, &warm.minima));
    assert_eq!(cache.stats().measured_nodes, 0);
    assert_eq!(cache.stats().arranged_nodes, 0);
    assert_eq!(warm.minima.len(), warm.all().len());
}

#[test]
fn scroll_placement_does_not_change_measured_child_minimums() {
    let make = |offset| {
        col([
            block(120., 40.).min_size(Size::new(120., 40.)).id("one"),
            block(90., 40.).min_size(Size::new(90., 40.)).id("two"),
            block(90., 40.).min_size(Size::new(90., 40.)).id("three"),
        ])
        .gap(5.)
        .pad(3.)
        .scroll()
        .scrolled(0., offset)
        .id("scroll")
    };
    let offered = Size::new(200., 90.);
    let mut cache = LayoutCache::default();
    cached(&make(0.), offered, &mut cache);
    let before = cached(&make(0.), offered, &mut cache);
    let moved = cached(&make(17.), offered, &mut cache);
    let oracle = resolve(&make(17.), Some(offered), Limits::default()).unwrap();
    assert_eq!(moved, oracle);
    assert_eq!(moved.min_size_of("scroll"), Some(Size::new(126., 6.)));
    for id in ["one", "two", "three"] {
        assert_eq!(moved.min_size_of(id), before.min_size_of(id));
        assert_eq!(
            moved.frame(id).unwrap().y,
            before.frame(id).unwrap().y - 17.
        );
    }
}

#[test]
fn hidden_fits_candidates_keep_their_original_measured_minimums() {
    let root = fits([
        col([block(200., 20.)
            .min_size(Size::new(200., 20.))
            .id("wide-leaf")])
        .id("wide"),
        block(60., 20.).min_size(Size::new(60., 20.)).id("thin"),
    ])
    .id("choice");
    let offered = Size::new(80., 20.);
    let mut cache = LayoutCache::default();
    for _ in 0..3 {
        let layout = cached(&root, offered, &mut cache);
        assert_eq!(layout.frame("wide").unwrap().size, Size::ZERO);
        assert_eq!(layout.frame("wide-leaf").unwrap().size, Size::ZERO);
        assert_eq!(layout.min_size_of("wide"), Some(Size::new(200., 20.)));
        assert_eq!(layout.min_size_of("wide-leaf"), Some(Size::new(200., 20.)));
        assert_eq!(layout.min_size_of("thin"), Some(Size::new(60., 20.)));
        assert_eq!(layout.minima.len(), 4);
        assert_eq!(layout.minima.len(), layout.all().len());
    }
}

#[test]
fn pin_second_pass_preserves_measurement_when_anchor_sizes_a_menu() {
    let root = stack([
        block(90., 24.).min_size(Size::new(80., 12.)).id("anchor"),
        col([block(40., 20.).min_size(Size::new(40., 20.)).id("item")])
            .min_w(55.)
            .pin(Pin::to("anchor").area(Area::BottomStart).match_width())
            .id("menu"),
    ])
    .id("root");
    let layout = resolve(&root, Some(Size::new(300., 200.)), Limits::default()).unwrap();
    assert_eq!(layout.frame("menu").unwrap().size.width, 90.);
    assert_eq!(layout.min_size_of("menu"), Some(Size::new(55., 20.)));
    assert_eq!(layout.min_size_of("item"), Some(Size::new(40., 20.)));
    assert_eq!(layout.min_size_of("root"), Some(layout.min_size()));
    assert_eq!(
        layout.minima.len(),
        layout.all().len(),
        "the pin pass does not append duplicate receipts"
    );
    let mut cache = LayoutCache::default();
    for _ in 0..3 {
        assert_eq!(cached(&root, Size::new(300., 200.), &mut cache), layout);
    }
}

#[test]
fn contour_reframe_rebinds_names_to_original_measurement_indices() {
    let root = row([
        block(20., 5.).min_size(Size::new(20., 5.)),
        block(80., 30.).min_size(Size::new(80., 30.)).id("old"),
    ])
    .id("root");
    let original = resolve(&root, Some(Size::new(200., 60.)), Limits::default()).unwrap();
    let mut frames = original.all().to_vec();
    frames[1].size = Size::new(400., 60.);
    frames[2].size = Size::new(1., 1.);
    // The contour publisher may add/change names. It does not ask layout to
    // measure this replacement's authored sizes or minimums a second time.
    let replacement = row([
        block(999., 999.).min_w(999.).id("newly-named"),
        block(1., 1.).id("renamed"),
    ])
    .id("replacement");
    let reframed = original
        .clone()
        .reframe(&replacement, frames.clone())
        .unwrap();
    assert_eq!(reframed.frame("newly-named"), Some(frames[1]));
    assert_eq!(
        reframed.min_size_of("newly-named"),
        Some(Size::new(20., 5.))
    );
    assert_eq!(reframed.min_size_of("renamed"), Some(Size::new(80., 30.)));
    assert_eq!(
        reframed.min_size_of("replacement"),
        Some(original.min_size())
    );
    assert_eq!(reframed.min_size_of("old"), None);
    assert!(Arc::ptr_eq(&original.minima, &reframed.minima));
}

#[test]
fn declaration_and_visual_reorder_bind_each_named_minimum_correctly() {
    let mut root = row([
        block(120., 20.)
            .min_size(Size::new(120., 20.))
            .order(2)
            .id("wide"),
        block(40., 30.).min_size(Size::new(40., 30.)).id("narrow"),
    ])
    .gap(8.)
    .id("root");
    let offered = Size::new(300., 60.);
    let mut cache = LayoutCache::default();
    cached(&root, offered, &mut cache);
    let before = cached(&root, offered, &mut cache);
    assert!(before.frame("wide").unwrap().x > before.frame("narrow").unwrap().x);
    root.children_mut().swap(0, 1);
    let after = cached(&root, offered, &mut cache);
    assert_eq!(
        after,
        resolve(&root, Some(offered), Limits::default()).unwrap()
    );
    assert_eq!(after.min_size_of("wide"), Some(Size::new(120., 20.)));
    assert_eq!(after.min_size_of("narrow"), Some(Size::new(40., 30.)));
    assert_eq!(after.min_size_of("root"), Some(Size::new(168., 30.)));
}

#[test]
fn receipt_lookup_and_reframe_refuse_inconsistent_metadata() {
    let root = col([block(10., 10.).min_w(10.).id("cell")]);
    let mut layout = resolve(&root, None, Limits::default()).unwrap();
    layout.named = Arc::new(vec![(Id::from("bad-index"), 99)]);
    assert_eq!(layout.min_size_of("bad-index"), None);
    assert_eq!(layout.min_size_of("absent"), None);
    let frames = layout.all().to_vec();
    layout.minima = Arc::new(Vec::new());
    assert_eq!(layout.reframe(&root, frames), Err(Error::InvalidValue));
}

#[test]
fn collecting_minimum_receipts_does_not_expand_the_node_budget() {
    let root = col([
        block(20., 10.).min_w(20.).id("one"),
        block(30., 10.).min_w(30.).id("two"),
        block(40., 10.).min_w(40.).id("three"),
    ])
    .id("root");
    let limits = Limits {
        nodes: 4,
        ..Limits::default()
    };
    let layout = resolve(&root, None, limits).unwrap();
    assert_eq!(layout.minima.len(), 4);
    assert_eq!(layout.min_size_of("three"), Some(Size::new(40., 0.)));
    assert_eq!(
        resolve(&root, None, Limits { nodes: 3, ..limits }),
        Err(Error::BudgetExceeded)
    );
    let mut cache = LayoutCache::default();
    for _ in 0..2 {
        resolve_cached_with(
            &root,
            None,
            limits,
            SpacingScale::DEFAULT,
            &mut cache,
            |_, _| {},
            |_, _| Size::ZERO,
        )
        .unwrap();
    }
    assert_eq!(
        resolve_cached_with(
            &root,
            None,
            Limits { nodes: 3, ..limits },
            SpacingScale::DEFAULT,
            &mut cache,
            |_, _| {},
            |_, _| Size::ZERO
        ),
        Err(Error::BudgetExceeded)
    );
}
