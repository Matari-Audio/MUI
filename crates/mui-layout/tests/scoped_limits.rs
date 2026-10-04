use mui_layout::{
    Error, LayoutCache, Limits, Node, Size, SpacingScale, resolve, resolve_cached_with,
    resolve_with,
};

const NORMAL: f64 = 1_000_000.;
const VIRTUAL: f64 = 5_000_000.;
fn limits() -> Limits {
    Limits {
        extent: VIRTUAL,
        ..Limits::default()
    }
}
fn capped(mut node: Node, cap: f64) -> Node {
    node.set_layout_extent_limit(cap);
    node
}
fn virtual_list() -> Node {
    capped(
        Node::col([
            capped(Node::block(100., 2_000_000.), VIRTUAL),
            capped(Node::block(100., 20.).id("row"), VIRTUAL),
            capped(Node::block(100., 2_000_000.), VIRTUAL),
        ])
        .size(100., 50.)
        .scroll()
        .scrolled(0., 2_000_000.)
        .id("list"),
        NORMAL,
    )
}
fn cached(tree: &Node, cache: &mut LayoutCache) -> Result<mui_layout::Layout, Error> {
    resolve_cached_with(
        tree,
        None,
        limits(),
        SpacingScale::default(),
        cache,
        |_, _| {},
        |_, _| Size::ZERO,
    )
}

#[test]
fn default_viewport_retains_large_virtual_content_and_coordinates() {
    let tree = capped(Node::row([virtual_list()]), NORMAL);
    let fresh = resolve(&tree, None, limits()).unwrap();
    assert_eq!(fresh.frame("list").unwrap().size, Size::new(100., 50.));
    let row = fresh.frame("row").unwrap();
    assert_eq!((row.x, row.y, row.size), (0., 0., Size::new(100., 20.)));
    let mut cache = LayoutCache::default();
    for _ in 0..3 {
        assert_eq!(cached(&tree, &mut cache).unwrap().all(), fresh.all());
    }
}

#[test]
fn virtual_sibling_does_not_raise_authored_or_aggregate_caps() {
    let ordinary = capped(Node::block(100., 2_000_000.), NORMAL);
    assert!(ordinary.validate_layout_values(limits()).is_err());
    let tree = capped(Node::row([virtual_list(), ordinary]), NORMAL);
    assert!(resolve(&tree, None, limits()).is_err());
    let scroll = capped(
        Node::col([
            capped(Node::block(100., 600_000.), NORMAL),
            capped(Node::block(100., 600_000.), NORMAL),
        ])
        .size(100., 50.)
        .scroll(),
        NORMAL,
    );
    let tree = capped(Node::row([virtual_list(), scroll]), NORMAL);
    assert!(matches!(
        resolve(&tree, None, limits()),
        Err(Error::BudgetExceeded)
    ));
    let mut cache = LayoutCache::default();
    for _ in 0..3 {
        assert!(cached(&tree, &mut cache).is_err());
    }
}

#[test]
fn oversized_intrinsic_is_rejected_before_constraints_and_cap_edits_invalidate_cache() {
    let metric = |_: &(), _: Option<f64>| Size::new(1_500_000., 20.);
    let mut content = capped(Node::content().id("content"), 2_000_000.);
    let mut cache = LayoutCache::default();
    for _ in 0..3 {
        let fresh =
            resolve_with(&content, None, limits(), SpacingScale::default(), metric).unwrap();
        let retained = resolve_cached_with(
            &content,
            None,
            limits(),
            SpacingScale::default(),
            &mut cache,
            |_, _| {},
            metric,
        )
        .unwrap();
        assert_eq!(retained.all(), fresh.all());
        assert_eq!(
            fresh.frame("content").unwrap().size,
            Size::new(1_500_000., 20.)
        );
    }
    content.set_layout_extent_limit(NORMAL);
    for _ in 0..2 {
        assert!(matches!(
            resolve_with(&content, None, limits(), SpacingScale::default(), metric),
            Err(Error::InvalidValue)
        ));
        assert!(matches!(
            resolve_cached_with(
                &content,
                None,
                limits(),
                SpacingScale::default(),
                &mut cache,
                |_, _| {},
                metric
            ),
            Err(Error::InvalidValue)
        ));
    }
    content = content.max_size((10., 10.));
    assert!(resolve_with(&content, None, limits(), SpacingScale::default(), metric).is_err());
    content.set_layout_extent_limit(2_000_000.);
    let retained = resolve_cached_with(
        &content,
        None,
        limits(),
        SpacingScale::default(),
        &mut cache,
        |_, _| {},
        metric,
    )
    .unwrap();
    assert_eq!(retained.frame("content").unwrap().size, Size::new(10., 10.));
}

#[test]
fn local_caps_cannot_raise_global_limits_or_accept_oversized_final_offers() {
    let tree = capped(Node::block(100., 2_000_000.), VIRTUAL);
    assert!(resolve(&tree, None, Limits::default()).is_err());
    let tree = capped(Node::block(100., 20.), NORMAL);
    assert!(matches!(
        resolve(&tree, Some(Size::new(100., 2_000_000.)), limits()),
        Err(Error::BudgetExceeded)
    ));
    let child = capped(Node::block(20., 20.).grow(1.), 100.);
    let tree = Node::row([child]).size(200., 20.);
    assert!(matches!(
        resolve(&tree, None, limits()),
        Err(Error::BudgetExceeded)
    ));
    for cap in [0., -1., f64::NAN, f64::INFINITY] {
        let node = capped(Node::block(10., 10.), cap);
        assert!(node.validate_layout_values(limits()).is_err());
        assert!(resolve(&node, None, limits()).is_err());
    }
}
