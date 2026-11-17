use super::*;

/// A node the scan never stamped has no stamp: the measure runs
/// uncached instead of panicking on a missing map entry.
#[test]
fn an_unscanned_node_has_no_stamp() {
    let mut cache = LayoutCache::default();
    assert!(cache.stamp(&label("a", "x")).is_none());
}
#[test]
fn variant_churn_counts_evictions_without_counting_cache_hits() {
    let n = label("text", "A measured label");
    let mut cache = LayoutCache::default();
    let limits = Limits::default();
    cache
        .prepare(&n, limits, SpacingScale::DEFAULT, &mut key)
        .unwrap();
    let mut measurer = crate::measure::intrinsic(metric);
    let mut pass = Pass {
        left: limits.nodes,
        limits,
        scale: SpacingScale::DEFAULT,
        keys: rustc_hash::FxHashSet::default(),
        redo: false,
        pinned: false,
        measurer: &mut measurer,
        cache: Some(&mut cache),
        boxed: None,
    };
    for i in 0..LayoutCache::VARIANTS + 2 {
        let width = 32. + i as f64;
        let measured = measure_cached(
            &n,
            "",
            [Some(width), None],
            Some(width),
            [Some(width), None],
            0,
            &mut pass,
        )
        .unwrap();
        assert_eq!(measured.size.width, width);
    }
    assert_eq!(
        pass.cache.as_ref().unwrap().stats.measure_variant_evictions,
        2
    );
    let width = 32. + (LayoutCache::VARIANTS + 1) as f64;
    measure_cached(
        &n,
        "",
        [Some(width), None],
        Some(width),
        [Some(width), None],
        0,
        &mut pass,
    )
    .unwrap();
    let stats = pass.cache.as_ref().unwrap().stats;
    assert_eq!(stats.measure_hits, 1);
    assert_eq!(stats.measure_variant_evictions, 2);
}

#[test]
fn hot_constraint_hits_survive_a_five_key_cycle_with_the_same_four_entries() {
    let n = label("text", "A measured label");
    let mut cache = LayoutCache::default();
    let limits = Limits::default();
    cache.trace_requests(true);
    cache
        .prepare(&n, limits, SpacingScale::DEFAULT, &mut key)
        .unwrap();
    let mut measurer = crate::measure::intrinsic(metric);
    let mut pass = Pass {
        left: limits.nodes,
        limits,
        scale: SpacingScale::DEFAULT,
        keys: rustc_hash::FxHashSet::default(),
        redo: false,
        pinned: false,
        measurer: &mut measurer,
        cache: Some(&mut cache),
        boxed: None,
    };
    // A stays hot while B/C/D/E cycle. The old insertion-order selector
    // does not refresh A on its hits and eventually discards it.
    let widths = [
        32., 33., 32., 34., 32., 35., 32., 36., 32., 33., 32., 34., 32., 35., 32., 36., 32.,
    ];
    let mut fifo = Vec::new();
    let mut fifo_hits = 0;
    let mut hot_snapshot = None;
    let mut capacity = None;
    for width in widths {
        if fifo.contains(&width) {
            fifo_hits += 1;
        } else {
            if fifo.len() == LayoutCache::VARIANTS {
                fifo.remove(0);
            }
            fifo.push(width);
        }
        let measured = measure_cached(
            &n,
            "root",
            [Some(width), None],
            Some(width),
            [Some(width), None],
            0,
            &mut pass,
        )
        .unwrap();
        assert_eq!(measured.size, metric(&n.payload, Some(width)));
        let cache = pass.cache.as_deref_mut().unwrap();
        let variants = &cache.stamp(&n).unwrap().measured;
        assert!(variants.len() <= LayoutCache::VARIANTS);
        if variants.len() == LayoutCache::VARIANTS {
            let allocated = capacity.get_or_insert(variants.capacity());
            assert_eq!(
                variants.capacity(),
                *allocated,
                "hit promotion never grows storage"
            );
        }
        let hot = &variants
            .iter()
            .find(|(k, _)| k.definite[0] == Some(32_f64.to_bits()))
            .unwrap()
            .1;
        if let Some(original) = &hot_snapshot {
            assert!(
                Arc::ptr_eq(original, hot),
                "the hot snapshot is retained, not remeasured"
            );
        } else {
            hot_snapshot = Some(hot.clone());
        }
        assert_eq!(
            variants.last().unwrap().0.definite[0],
            Some(width.to_bits())
        );
    }
    let cache = pass.cache.as_deref_mut().unwrap();
    assert_eq!(cache.stats.measure_hits, 8, "every repeated A request hits");
    assert!(cache.stats.measure_hits > fifo_hits);
    let trace = cache.take_request_trace().unwrap();
    assert_eq!(trace.hits, cache.stats.measure_hits as u64);
    assert_eq!(trace.misses, cache.stats.measured_nodes as u64);
    let revision = &trace.nodes[0].revisions[0];
    assert_eq!(revision.keys.len(), 5);
    assert_eq!(revision.key_overflow_requests, 0);
    assert_eq!(revision.keys[0].misses, 1);
    assert_eq!(revision.keys[0].hits, 8);
    assert_eq!(
        revision.miss_reuse_distances[..LayoutCache::VARIANTS]
            .iter()
            .sum::<u64>(),
        0
    );

    // A remains a lookup hit, but retained data never bypasses the host's
    // node budget. Promotion changes replacement order, not admission.
    pass.left = 0;
    assert!(matches!(
        measure_cached(
            &n,
            "root",
            [Some(32.), None],
            Some(32.),
            [Some(32.), None],
            0,
            &mut pass,
        ),
        Err(Error::BudgetExceeded)
    ));
}

#[test]
fn promoted_constraint_variants_keep_exact_fresh_frames_and_warm_validation() {
    let n = scroll_fixture();
    let offers: Vec<_> = [
        300., 320., 300., 340., 300., 360., 300., 380., 300., 320., 300.,
    ]
    .into_iter()
    .map(|w| Size::new(w, 100.))
    .collect();
    offer_oracle(&n, &offers, None);
    let mut cache = LayoutCache::default();
    prime(&n, 400., &mut cache);
    let mut invalid = n.clone();
    invalid.children_mut()[0].width = Len::Px(f64::NAN);
    let actual = resolve_cached_with(
        &invalid,
        Some(Size::new(400., 100.)),
        Limits::default(),
        SpacingScale::DEFAULT,
        &mut cache,
        key,
        metric,
    );
    let fresh = resolve_with(
        &invalid,
        Some(Size::new(400., 100.)),
        Limits::default(),
        SpacingScale::DEFAULT,
        metric,
    );
    assert!(fresh.is_err());
    assert_eq!(actual, fresh);
    for limits in [
        Limits {
            nodes: 2,
            ..Limits::default()
        },
        Limits {
            depth: 1,
            ..Limits::default()
        },
    ] {
        let actual = resolve_cached_with(
            &n,
            Some(Size::new(400., 100.)),
            limits,
            SpacingScale::DEFAULT,
            &mut cache,
            key,
            metric,
        );
        let fresh = resolve_with(
            &n,
            Some(Size::new(400., 100.)),
            limits,
            SpacingScale::DEFAULT,
            metric,
        );
        assert!(fresh.is_err());
        assert_eq!(actual, fresh);
    }
}

#[test]
fn request_trace_records_exact_fifo_revisits_without_changing_measurements() {
    let n = label("text", "A measured label");
    let mut cache = LayoutCache::default();
    assert!(cache.trace.is_none());
    assert!(cache.take_request_trace().is_none());
    cache.trace_requests(false);
    assert!(
        cache.trace.is_none(),
        "disabled collection owns no recorder or history"
    );
    cache.trace_requests(true);
    let limits = Limits::default();
    cache
        .prepare(&n, limits, SpacingScale::DEFAULT, &mut key)
        .unwrap();
    let mut measurer = crate::measure::intrinsic(metric);
    let mut pass = Pass {
        left: limits.nodes,
        limits,
        scale: SpacingScale::DEFAULT,
        keys: rustc_hash::FxHashSet::default(),
        redo: false,
        pinned: false,
        measurer: &mut measurer,
        cache: Some(&mut cache),
        boxed: None,
    };
    for width in [32., 33., 34., 35., 36., 37., 32., 37.] {
        let measured = measure_cached(
            &n,
            "root",
            [Some(width), None],
            Some(width),
            [Some(width), None],
            0,
            &mut pass,
        )
        .unwrap();
        assert_eq!(measured.size, metric(&n.payload, Some(width)));
    }
    let cache = pass.cache.as_deref_mut().unwrap();
    assert_eq!(cache.stats.measure_variant_evictions, 3);
    let trace = cache.take_request_trace().unwrap();
    assert_eq!((trace.requests, trace.hits, trace.misses), (8, 1, 7));
    assert_eq!(trace.depths[0].requests, trace.requests);
    assert_eq!(trace.nodes.len(), 1);
    let revision = &trace.nodes[0].revisions[0];
    assert_eq!(revision.keys.len(), 6);
    assert_eq!(revision.miss_reuse_distances[5], 1);
    assert_eq!(revision.lru_reuse_distances[1], 1);
    assert_eq!(revision.repeated_misses, 1);
    assert_eq!(revision.keys[0].input.definite[0], Some(32_f64.to_bits()));
    assert!(
        cache.trace.is_none(),
        "export stops capture and keeps the geometry cache"
    );
    assert_eq!(
        cache.stamp(&n).unwrap().measured.len(),
        LayoutCache::VARIANTS
    );
}

#[test]
fn tracing_preserves_frames_stats_and_persistent_stamp_identity_across_rebuilt_trees() {
    let original = Box::new(scroll_fixture());
    let mut plain = LayoutCache::default();
    let mut traced = LayoutCache::default();
    traced.trace_requests(true);
    for _ in 0..2 {
        assert_eq!(
            cached(&original, 400., &mut traced),
            cached(&original, 400., &mut plain)
        );
        assert_eq!(traced.stats(), plain.stats());
    }
    for scroll in [3., 11., 29.] {
        let mut rebuilt = Box::new((*original).clone());
        assert!(!std::ptr::eq(&*rebuilt, &*original));
        rebuilt.children_mut()[0].scrolled = [0., scroll];
        for _ in 0..2 {
            let actual = cached(&rebuilt, 400., &mut traced);
            assert_eq!(actual, cached(&rebuilt, 400., &mut plain));
            assert_eq!(actual, oracle(&rebuilt, 400.));
            assert_eq!(traced.stats(), plain.stats());
        }
    }
    let trace = traced.take_request_trace().unwrap();
    assert!(trace.whole_layout_reuses >= 3);
    assert_eq!(trace.bare_resolves, 1);
    assert_eq!(trace.requests, trace.hits + trace.misses);
    assert_eq!(
        trace.requests,
        trace.depths.iter().map(|d| d.requests).sum::<u64>()
    );
    assert_eq!(
        trace.node_overflow_requests
            + trace.revision_overflow_requests
            + trace.key_overflow_requests,
        0
    );
    assert_eq!(trace.failed_requests, 0);
    let roots: Vec<_> = trace
        .nodes
        .iter()
        .filter(|n| n.id.as_deref() == Some("root"))
        .collect();
    assert_eq!(
        roots.len(),
        1,
        "rebuilt pointers retain one exact cache stamp slot"
    );
    assert_eq!(roots[0].revisions.len(), 1);
    assert!(roots[0].child_misses > 0);
    assert!(roots[0].rebound_nodes > 0);
    assert!(plain.trace.is_none());
    assert!(plain.take_request_trace().is_none());
}
#[derive(Clone, Debug, Default)]
struct Text {
    value: String,
    colour: u32,
}
fn key(t: &Text, out: &mut Vec<u8>) {
    out.extend_from_slice(t.value.as_bytes());
}
fn metric(t: &Text, room: Option<f64>) -> Size {
    let full = t.value.len() as f64 * 8.;
    let w = room.unwrap_or(full).max(1.);
    Size::new(full.min(w), (full / w).ceil().max(1.) * 16.)
}
fn label(id: &str, value: &str) -> Node<Text> {
    Node::content()
        .with(Text {
            value: value.into(),
            colour: 0,
        })
        .id(id)
}
fn fixture() -> Node<Text> {
    Node::row([
        Node::col([label("a", "A short label"), label("b", "Another label")])
            .id("left")
            .grow(1.),
        Node::col([label("c", "Static right panel"), label("d", "Stable")])
            .id("right")
            .grow(1.),
    ])
    .id("root")
}
fn cached(n: &Node<Text>, w: f64, c: &mut LayoutCache) -> Layout {
    resolve_cached_with(
        n,
        Some(Size::new(w, 100.)),
        Limits::default(),
        SpacingScale::DEFAULT,
        c,
        key,
        metric,
    )
    .unwrap()
}
/// `colour` doubles as a retained-subtree token in these tests.
fn retained(n: &Node<Text>, w: f64, c: &mut LayoutCache, reuse: bool) -> Layout {
    resolve_cached_retained(
        n,
        Some(Size::new(w, 100.)),
        Limits::default(),
        SpacingScale::DEFAULT,
        c,
        key,
        |t: &Text| (t.colour > 0).then_some((u64::from(t.colour), reuse)),
        metric,
    )
    .unwrap()
}
#[test]
fn a_reused_subtree_replays_its_stamps_and_lays_out_the_same() {
    let mut n = fixture();
    n.children_mut()[1].payload_mut().colour = 7;
    let mut c = LayoutCache::default();
    retained(&n, 400., &mut c, false);
    retained(&n, 400., &mut c, false);
    assert_eq!(c.stats().replayed_nodes, 0);
    assert_eq!(retained(&n, 400., &mut c, true), oracle(&n, 400.));
    let s = c.stats();
    assert_eq!((s.validated_nodes, s.replayed_nodes), (7, 3), "{s:?}");
    // The rest of the tree changes around it.
    n.children_mut()[0].children_mut()[0].payload_mut().value = "A much longer label".into();
    assert_eq!(retained(&n, 400., &mut c, true), oracle(&n, 400.));
    assert_eq!(c.stats().replayed_nodes, 3);
    assert!(c.stats().measured_nodes > 0);
}
#[test]
fn a_retained_subtree_whose_record_no_longer_fits_is_scanned() {
    let mut n = fixture();
    n.children_mut()[1].payload_mut().colour = 7;
    let mut c = LayoutCache::default();
    retained(&n, 400., &mut c, false);
    retained(&n, 400., &mut c, false);
    // Rebuilt with one child fewer, yet claimed reusable.
    n.children_mut()[1] = Node::col([label("c", "Static right panel")])
        .id("right")
        .grow(1.)
        .with(Text {
            value: String::new(),
            colour: 7,
        });
    assert_eq!(retained(&n, 400., &mut c, true), oracle(&n, 400.));
    let s = c.stats();
    assert_eq!((s.validated_nodes, s.replayed_nodes), (6, 0), "{s:?}");
    // What that scan recorded replays next time.
    assert_eq!(retained(&n, 400., &mut c, true), oracle(&n, 400.));
    assert_eq!(c.stats().replayed_nodes, 2);
}
/// A new size solves bare; the cache takes over once it holds.
fn prime(n: &Node<Text>, w: f64, c: &mut LayoutCache) -> Layout {
    let bare = cached(n, w, c);
    assert_eq!(c.stats().validated_nodes, 0);
    assert_eq!(cached(n, w, c), bare);
    bare
}
fn oracle(n: &Node<Text>, w: f64) -> Layout {
    resolve_with(
        n,
        Some(Size::new(w, 100.)),
        Limits::default(),
        SpacingScale::DEFAULT,
        metric,
    )
    .unwrap()
}

/// Exercise the measurement hook at changing offers, including offers
/// inside one solve. The public cache deliberately solves live resizes
/// bare, which would otherwise hide constraint-variant equivalence.
fn offer_oracle(n: &Node<Text>, offers: &[Size], boxed: Option<Insets>) -> LayoutStats {
    let mut cache = LayoutCache::default();
    let mut totals = LayoutStats::default();
    for &size in offers {
        cache
            .prepare(n, Limits::default(), SpacingScale::DEFAULT, &mut key)
            .unwrap();
        let actual = crate::resolve_impl(
            n,
            Some(size),
            Limits::default(),
            SpacingScale::DEFAULT,
            metric,
            Some(&mut cache),
            boxed,
        );
        let fresh = crate::resolve_impl(
            n,
            Some(size),
            Limits::default(),
            SpacingScale::DEFAULT,
            metric,
            None,
            boxed,
        );
        assert_eq!(actual, fresh, "offer {size:?}, boxed {boxed:?}");
        actual.unwrap();
        totals.measured_nodes += cache.stats.measured_nodes;
        totals.measure_hits += cache.stats.measure_hits;
        totals.measure_variant_evictions += cache.stats.measure_variant_evictions;
    }
    totals
}

#[test]
fn branch_working_set_survives_nested_flex_offers() {
    let n = Node::row([Node::block(24., 16.).shrink(0.).with(Text {
        value: String::new(),
        colour: 0,
    })])
    .id("branch");
    let widths: Vec<_> = (0..24).map(|i| Size::new(100. + i as f64, 80.)).collect();
    let once = offer_oracle(&n, &widths, None);
    let twice = offer_oracle(
        &n,
        &widths.iter().chain(&widths).copied().collect::<Vec<_>>(),
        None,
    );
    assert_eq!(
        twice.measured_nodes, once.measured_nodes,
        "a repeated branch offer must not rebuild the measured subtree"
    );
    assert!(twice.measure_hits >= once.measure_hits + widths.len());
    assert_eq!(
        twice.measure_variant_evictions,
        once.measure_variant_evictions
    );
}

#[test]
fn replaced_fixed_offers_share_one_variant_with_exact_fresh_layouts() {
    let n = Node::stack([
        Node::col([
            label("percent", "Percent descendant")
                .w(Len::Pct(50.))
                .h(Len::Pct(25.)),
            label("container", "Container descendant")
                .w(Len::Container(75.))
                .h(Len::Container(50.)),
        ])
        .scroll()
        .scrolled(0., 7.)
        .id("flow"),
        label("pin", "Pinned").pin(Pin::to("percent").area(Area::End)),
    ])
    .size(240., 160.)
    .shrink(0.)
    .pad(4.)
    .id("root");
    let offers: Vec<_> = (0..12)
        .map(|i| Size::new(280. + i as f64 * 7., 180. + i as f64 * 3.))
        .collect();
    offer_oracle(&n, &offers, None);
    let fixed_label = label("fixed-label", "Fixed measured content")
        .size(240., 160.)
        .shrink(0.);
    let stats = offer_oracle(&fixed_label, &offers, None);
    assert_eq!(stats.measure_hits, offers.len() - 1);
    assert_eq!(stats.measured_nodes, 1, "only the first offer is measured");
    assert_eq!(stats.measure_variant_evictions, 0);
}

#[test]
fn effective_offer_keys_preserve_redo_room_aspect_box_and_container_axes() {
    let fixed = label("fixed", "Fixed").size(120., 80.).shrink(0.);
    let input = |n: &Node<Text>, h, room, cq, redo, boxed| {
        MeasureInput::for_node(
            n,
            [Some(200.), Some(h)],
            Some(room),
            [Some(200.), Some(cq)],
            redo,
            boxed,
            SpacingScale::DEFAULT,
        )
    };
    let fixed_key = input(&fixed, 100., 200., 300., false, false);
    assert_eq!(fixed_key, input(&fixed, 500., 200., 600., false, false));
    assert_ne!(fixed_key, input(&fixed, 100., 100., 300., false, false));
    assert_ne!(fixed_key, input(&fixed, 100., 200., 300., true, false));
    for flexible in [fixed.clone().grow(1.), fixed.clone().shrink(1.)] {
        assert_eq!(
            input(&flexible, 100., 200., 300., false, false),
            input(&flexible, 500., 200., 600., false, false)
        );
        assert_ne!(
            input(&flexible, 100., 200., 300., true, false),
            input(&flexible, 500., 200., 600., true, false)
        );
    }
    let cq = fixed.clone().h(Len::Container(50.));
    assert_ne!(
        input(&cq, 100., 200., 300., false, false),
        input(&cq, 100., 200., 600., false, false)
    );
    for n in [fixed.clone().aspect(2.), fixed.clone().aspect_fit(2.)] {
        assert_ne!(
            input(&n, 100., 200., 300., false, false),
            input(&n, 100., 200., 600., false, false)
        );
        assert_ne!(
            input(&n, 100., 200., 300., false, false),
            input(&n, 500., 200., 300., false, false)
        );
    }
    assert_ne!(
        input(&fixed, 100., 200., 300., false, true),
        input(&fixed, 500., 200., 600., false, true)
    );
    let auto = fixed.h(Len::Auto);
    assert_ne!(
        MeasureInput::for_node(
            &auto,
            [Some(200.), None],
            None,
            [None, Some(300.)],
            false,
            false,
            SpacingScale::DEFAULT
        ),
        MeasureInput::for_node(
            &auto,
            [Some(200.), None],
            None,
            [None, Some(600.)],
            false,
            false,
            SpacingScale::DEFAULT
        )
    );
}

#[test]
fn unused_inherited_containers_reuse_natural_controls_with_fresh_frames() {
    let natural = Node::col([
        Node::row([
            label("wrapped", "A deliberately long musical parameter caption")
                .w(Len::Pct(60.))
                .min_w(18.),
            label("aspect", "Aspect").w(48.).aspect(2.).shrink(0.),
        ])
        .wrap()
        .id("rail"),
        label("bounded", "Minimum and maximum")
            .min_size(Size::new(20., 18.))
            .max_size(Size::new(160., 90.)),
    ])
    .w(180.)
    .shrink(0.)
    .id("natural");
    let mut cache = LayoutCache::default();
    for height in [320., 240., 160., 80., 20., 4., 160., 320.] {
        let root = Node::stack([
            Node::col([natural.clone()])
                .size(320., height)
                .shrink(0.)
                .scroll()
                .scrolled(0., height * 0.1)
                .id("shutter"),
            label("pin", "Pinned").pin(Pin::to("bounded").area(Area::End)),
        ])
        .id("root");
        assert_eq!(cached(&root, 400., &mut cache), oracle(&root, 400.));
        assert_eq!(cached(&root, 400., &mut cache), oracle(&root, 400.));
        let stamp = cache.stamp(&root.children()[0].children()[0]).unwrap();
        assert!(
            stamp
                .measured
                .iter()
                .all(|(input, _)| input.container == [None; 2]),
            "neither the initial pass nor the flex remeasure consumes the inherited shutter height"
        );
    }
}

#[test]
fn introducing_container_units_rebuilds_inherited_extent_dependencies() {
    let mut cache = LayoutCache::default();
    for use_container in [false, true, false] {
        for height in [320., 240., 160., 80., 20., 160., 320.] {
            let length = if use_container {
                Len::Container(50.)
            } else {
                Len::Px(24.)
            };
            let root = Node::col([Node::col([Node::block(32., length).id("control")])
                .w(180.)
                .h(Len::Pct(100.))
                .shrink(0.)
                .id("natural")])
            .size(320., height)
            .shrink(0.)
            .scroll()
            .id("root");
            let actual = cached(&root, 400., &mut cache);
            assert_eq!(actual, oracle(&root, 400.));
            assert_eq!(
                actual.frame("control").unwrap().size.height,
                if use_container { height * 0.5 } else { 24. },
                "the consumed CQ extent must change sound-independent geometry"
            );
            assert_eq!(cached(&root, 400., &mut cache), oracle(&root, 400.));
            if !use_container {
                assert!(
                    cache
                        .stamp(&root.children()[0])
                        .unwrap()
                        .measured
                        .iter()
                        .all(|(input, _)| input.container == [None; 2])
                );
            }
        }
    }
}

#[test]
fn fixed_natural_subtree_matches_fresh_inside_a_changing_scroll_shutter() {
    let natural = Node::col([
        label("relative", "A relative child of the natural content")
            .w(Len::Container(80.))
            .h(Len::Pct(50.)),
        label("tail", "Stable tail").h(32.).shrink(0.),
    ])
    .size(240., 160.)
    .shrink(0.)
    .id("natural");
    let mut cache = LayoutCache::default();
    for height in [100., 80., 60., 40., 20., 4., 40., 80., 100.] {
        let n = Node::stack([
            Node::col([natural.clone()])
                .size(240., height)
                .shrink(0.)
                .scroll()
                .scrolled(0., height * 0.1)
                .id("shutter"),
            label("pin", "Pinned").pin(Pin::to("relative").area(Area::End)),
        ])
        .id("root");
        assert_eq!(cached(&n, 300., &mut cache), oracle(&n, 300.));
        assert_eq!(cached(&n, 300., &mut cache), oracle(&n, 300.));
        let variants = cache
            .stamp(&n.children()[0].children()[0])
            .unwrap()
            .measured
            .len();
        assert_eq!(variants, 1, "the shutter does not resize natural content");
    }
}

#[test]
fn consumed_room_keeps_narrow_unresolved_cq_aspect_and_boxed_offers_distinct() {
    let fixed = label("fixed", "Fixed").size(120., 80.).shrink(0.);
    let input = |n: &Node<Text>, width, room, boxed, scale| {
        MeasureInput::for_node(
            n,
            [width, Some(100.)],
            room,
            [Some(300.), Some(200.)],
            false,
            boxed,
            scale,
        )
    };
    for xs in [2., 5., 11.] {
        let scale = SpacingScale {
            xs,
            ..SpacingScale::DEFAULT
        };
        let padded = fixed.clone().pad(SpacingToken::Xs);
        let capped = input(&padded, Some(200.), Some(120.), false, scale);
        assert_eq!(capped.room, Some((120. - 2. * xs).to_bits()));
        for room in [None, Some(200.), Some(500.)] {
            assert_eq!(capped, input(&padded, Some(200.), room, false, scale));
        }
        assert_ne!(capped, input(&padded, Some(200.), Some(60.), false, scale));
        for n in [
            padded.clone().aspect(2.),
            padded.clone().aspect_fit(2.),
            padded.clone().w(Len::Container(40.)),
        ] {
            assert_ne!(
                input(&n, Some(200.), Some(200.), false, scale),
                input(&n, Some(200.), Some(500.), false, scale)
            );
        }
        let unresolved = padded.clone().w(Len::Auto);
        assert_ne!(
            input(&unresolved, None, Some(200.), false, scale),
            input(&unresolved, None, Some(500.), false, scale)
        );
        assert_ne!(
            input(&padded, Some(200.), Some(200.), true, scale),
            input(&padded, Some(200.), Some(500.), true, scale)
        );
    }
}

#[test]
fn capped_parent_rooms_reuse_scaled_wrapping_subtrees_with_exact_fresh_frames() {
    for xs in [2., 5., 11.] {
        let scale = SpacingScale {
            xs,
            ..SpacingScale::DEFAULT
        };
        for scrolling in [false, true] {
            let mut panel = Node::col([
                Node::row([
                    label("first", "Percent wrapping child").w(Len::Pct(55.)),
                    label("second", "Clamped wrapping child").w(Len::Clamp {
                        min: 50.,
                        pct: 45.,
                        max: 100.,
                    }),
                ])
                .wrap()
                .id("wrap"),
                label("cq", "Container child").w(Len::Container(80.)).h(24.),
            ])
            .w(180.)
            .min_w(120.)
            .max_size(Size::new(220., 200.))
            .shrink(0.)
            .pad(SpacingToken::Xs)
            .id("panel");
            if scrolling {
                panel = panel.h(110.).scroll().scrolled(0., 13.);
            }
            let mut cache = LayoutCache::default();
            for width in [240., 260., 280., 300., 320., 340., 360., 240.] {
                let n = Node::col([Node::col([panel.clone()]).w(width).shrink(0.).id("parent")])
                    .id("root");
                let fresh = resolve_with(
                    &n,
                    Some(Size::new(600., 400.)),
                    Limits::default(),
                    scale,
                    metric,
                )
                .unwrap();
                for _ in 0..2 {
                    assert_eq!(
                        resolve_cached_with(
                            &n,
                            Some(Size::new(600., 400.)),
                            Limits::default(),
                            scale,
                            &mut cache,
                            key,
                            metric,
                        )
                        .unwrap(),
                        fresh
                    );
                }
                assert_eq!(
                    cache
                        .stamp(&n.children()[0].children()[0])
                        .unwrap()
                        .measured
                        .len(),
                    1,
                    "wider parent rooms do not create another panel variant"
                );
            }
            let invalid = panel.pad(f64::NAN);
            let offered = Some(Size::new(600., 400.));
            let cached = resolve_cached_with(
                &invalid,
                offered,
                Limits::default(),
                scale,
                &mut cache,
                key,
                metric,
            );
            let fresh = resolve_with(&invalid, offered, Limits::default(), scale, metric);
            assert!(fresh.is_err());
            assert_eq!(cached, fresh, "room reuse cannot bypass padding validation");
        }
    }
}

#[test]
fn normalized_flex_minmax_aspect_and_relative_lengths_match_fresh_layouts() {
    let offers = [
        Size::new(300., 160.),
        Size::new(180., 95.),
        Size::new(350., 220.),
        Size::new(220., 110.),
        Size::new(300., 160.),
    ];
    for (grow, shrink) in [(0., 0.), (1., 0.), (0., 1.), (1., 1.)] {
        for len in [
            Len::Px(120.),
            Len::Pct(60.),
            Len::Container(70.),
            Len::Clamp {
                min: 80.,
                pct: 55.,
                max: 180.,
            },
        ] {
            for aspect in [None, Some(false), Some(true)] {
                let mut panel = Node::col([
                    label("text", "Wrapping a long label inside the final flex share"),
                    label("relative", "Relative")
                        .w(Len::Container(80.))
                        .h(Len::Pct(40.)),
                ])
                .size(len, 90.)
                .min_size(Size::new(75., 50.))
                .max_size(Size::new(190., 120.))
                .grow(grow)
                .shrink(shrink)
                .pad(3.)
                .id("panel");
                panel = match aspect {
                    Some(false) => panel.h(Len::Auto).aspect(2.),
                    Some(true) => panel.aspect_fit(2.),
                    None => panel,
                };
                let n = Node::row([
                    panel,
                    label("sibling", "Sibling takes the remaining share").grow(1.),
                ])
                .id("root");
                offer_oracle(&n, &offers, None);
            }
        }
    }
    let n = Node::col([label("boxed-child", "Boxed child")])
        .size(100., 50.)
        .min_h(30.)
        .max_size(Size::new(200., 130.));
    offer_oracle(&n, &offers, Some(Insets::all(5.)));
}

#[test]
fn normalized_warm_measurements_keep_invalid_offers_and_limits_rejected() {
    let n = Node::col([label("child", "Child")])
        .size(120., 80.)
        .shrink(0.)
        .id("root");
    let mut cache = LayoutCache::default();
    prime(&n, 200., &mut cache);
    for limits in [
        Limits {
            nodes: 1,
            ..Limits::default()
        },
        Limits {
            depth: 0,
            ..Limits::default()
        },
        Limits {
            extent: 70.,
            ..Limits::default()
        },
    ] {
        let actual = resolve_cached_with(
            &n,
            Some(Size::new(200., 100.)),
            limits,
            SpacingScale::DEFAULT,
            &mut cache,
            key,
            metric,
        );
        let fresh = resolve_with(
            &n,
            Some(Size::new(200., 100.)),
            limits,
            SpacingScale::DEFAULT,
            metric,
        );
        assert!(fresh.is_err());
        assert_eq!(actual, fresh);
        assert_eq!(cached(&n, 200., &mut cache), oracle(&n, 200.));
        assert_eq!(cached(&n, 200., &mut cache), oracle(&n, 200.));
    }
    for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.] {
        let offered = Some(Size::new(200., invalid));
        let actual = resolve_cached_with(
            &n,
            offered,
            Limits::default(),
            SpacingScale::DEFAULT,
            &mut cache,
            key,
            metric,
        );
        let fresh = resolve_with(
            &n,
            offered,
            Limits::default(),
            SpacingScale::DEFAULT,
            metric,
        );
        assert_eq!(actual, fresh);
        assert!(fresh.is_err());
        assert_eq!(cached(&n, 200., &mut cache), oracle(&n, 200.));
        assert_eq!(cached(&n, 200., &mut cache), oracle(&n, 200.));
    }
}

fn scroll_fixture() -> Node<Text> {
    let nested = Node::col([
        label("nested-first", "Nested first")
            .size(240., 48.)
            .shrink(0.),
        label("nested-last", "Nested last")
            .size(240., 48.)
            .shrink(0.),
    ])
    .size(240., 64.)
    .scroll()
    .id("nested");
    let flow = Node::col([
        label("anchor", "Scrolling anchor")
            .size(240., 36.)
            .shrink(0.),
        nested.shrink(0.),
        label("tail", "Scrolling tail").size(240., 96.).shrink(0.),
    ])
    .shrink(0.)
    .id("flow");
    Node::stack([
        Node::col([
            label("header", "Sticky header")
                .size(240., 24.)
                .shrink(0.)
                .sticky(),
            flow,
        ])
        .size(260., 100.)
        .scroll()
        .id("scroll"),
        label("tip", "Pinned").pin(Pin::to("anchor").area(Area::End)),
    ])
    .pad(SpacingToken::Xs)
    .id("root")
}

#[test]
fn cached_scroll_binds_the_requested_root_without_rebuilding_descendant_measurements() {
    let mut n = Node::col((0..128).map(|i| label(&format!("row/{i}"), "Stable row").shrink(0.)))
        .size(240., 100.)
        .scroll()
        .id("scroll");
    let mut cache = LayoutCache::default();
    prime(&n, 400., &mut cache);
    for offset in [1., 17., 64., 120.] {
        n.scrolled = [0., offset];
        assert_eq!(cached(&n, 400., &mut cache), oracle(&n, 400.));
        assert_eq!(cache.stats().measured_nodes, 0);
        assert_eq!(
            cache.stats().rebound_nodes,
            1,
            "a root cache hit must not allocate and rebind the entire measured descendant tree"
        );
    }
}

#[test]
fn scroll_placement_reuses_measurements_and_rearranges_nested_sticky_pins() {
    let mut n = scroll_fixture();
    let mut c = LayoutCache::default();
    let before = prime(&n, 400., &mut c);
    for step in 1..=64 {
        n.children_mut()[0].scrolled = [step as f64 * 0.125, step as f64 * 0.75];
        n.children_mut()[0].children_mut()[1].children_mut()[1].scrolled = [0., step as f64 * 0.25];
        let result = cached(&n, 400., &mut c);
        assert_eq!(result, oracle(&n, 400.));
        assert_ne!(result.frame("anchor"), before.frame("anchor"));
        let stats = c.stats();
        assert_eq!(
            stats.measured_nodes, 0,
            "scroll does not change any intrinsic input"
        );
        assert_eq!(
            stats.measure_hits, 1,
            "the complete root measurement is shared"
        );
        assert_eq!(stats.rebound_nodes, result.all().len());
        assert_eq!(
            stats.arranged_nodes,
            result.all().len(),
            "scroll must update real frames"
        );
        assert_eq!(cached(&n, 400., &mut c), result);
        assert_eq!(
            c.stats().arranged_nodes,
            0,
            "unchanged placement reuses the whole layout"
        );
    }
}

#[test]
fn scroll_placement_preserves_reorder_offset_and_pin_invalidation() {
    let mut n = scroll_fixture();
    let mut c = LayoutCache::default();
    prime(&n, 400., &mut c);
    n.children_mut()[0].scrolled = [2., 17.];
    n.children_mut()[0].children_mut()[1]
        .children_mut()
        .swap(0, 2);
    assert_eq!(cached(&n, 400., &mut c), oracle(&n, 400.));
    assert!(
        c.stats().measured_nodes > 0,
        "a reordered intrinsic declaration is still invalidated"
    );
    n.children_mut()[0].offset = [9.5, -2.];
    assert_eq!(cached(&n, 400., &mut c), oracle(&n, 400.));
    assert!(
        c.stats().measured_nodes > 0,
        "OFFSET remains in the measurement projection"
    );
    n.children_mut()[1] =
        label("tip", "Pinned").pin(Pin::to("nested-last").area(Area::Bottom).match_width());
    assert_eq!(cached(&n, 400., &mut c), oracle(&n, 400.));
    n.children_mut()[0].scrolled = [3., 29.];
    assert_eq!(cached(&n, 400., &mut c), oracle(&n, 400.));
    assert_eq!(
        c.stats().measured_nodes,
        0,
        "a fresh pin uses freshly arranged scroll anchors"
    );

    // Anonymous ancestor addresses and named child identities both take
    // part in the intrinsic revision; placement is not keyed by hash.
    n.children_mut()[0].children_mut()[1].id = None;
    assert_eq!(cached(&n, 400., &mut c), oracle(&n, 400.));
    n.children_mut()[0].children_mut()[1]
        .children_mut()
        .swap(0, 2);
    n.children_mut()[0].scrolled = [0., 11.];
    assert_eq!(cached(&n, 400., &mut c), oracle(&n, 400.));
}

#[test]
fn scroll_placement_validates_warm_coordinates_and_budget_context() {
    let mut n = scroll_fixture();
    let mut c = LayoutCache::default();
    prime(&n, 400., &mut c);
    for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        n.children_mut()[0].scrolled = [0., invalid];
        assert!(matches!(
            resolve_cached_with(
                &n,
                Some(Size::new(400., 100.)),
                Limits::default(),
                SpacingScale::DEFAULT,
                &mut c,
                key,
                metric
            ),
            Err(Error::InvalidValue)
        ));
        n.children_mut()[0].scrolled = [0., 13.];
        assert_eq!(cached(&n, 400., &mut c), oracle(&n, 400.));
    }
    for limits in [
        Limits {
            nodes: 2,
            ..Limits::default()
        },
        Limits {
            depth: 1,
            ..Limits::default()
        },
        Limits {
            extent: 250.,
            ..Limits::default()
        },
    ] {
        assert!(
            resolve_cached_with(
                &n,
                Some(Size::new(400., 100.)),
                limits,
                SpacingScale::DEFAULT,
                &mut c,
                key,
                metric
            )
            .is_err(),
            "placement reuse never bypasses nodes, depth or extent limits"
        );
        assert_eq!(cached(&n, 400., &mut c), oracle(&n, 400.));
        assert_eq!(cached(&n, 400., &mut c), oracle(&n, 400.));
    }
    n.children_mut()[0].scrolled = [0., 19.];
    assert_eq!(cached(&n, 400., &mut c), oracle(&n, 400.));
    assert_eq!(c.stats().measured_nodes, 0);
}

#[test]
fn scroll_placement_survives_resizing_and_spacing_context_changes() {
    let mut n = scroll_fixture();
    let mut c = LayoutCache::default();
    let wider_spacing = SpacingScale {
        xs: 9.,
        ..SpacingScale::DEFAULT
    };
    for width in [400., 300., 600., 400.] {
        n.children_mut()[0].scrolled = [1., width * 0.04];
        for scale in [SpacingScale::DEFAULT, wider_spacing, SpacingScale::DEFAULT] {
            let fresh = resolve_with(
                &n,
                Some(Size::new(width, 100.)),
                Limits::default(),
                scale,
                metric,
            )
            .unwrap();
            for _ in 0..2 {
                assert_eq!(
                    resolve_cached_with(
                        &n,
                        Some(Size::new(width, 100.)),
                        Limits::default(),
                        scale,
                        &mut c,
                        key,
                        metric
                    )
                    .unwrap(),
                    fresh
                );
            }
            n.children_mut()[0].scrolled[1] += 0.375;
            assert_eq!(
                resolve_cached_with(
                    &n,
                    Some(Size::new(width, 100.)),
                    Limits::default(),
                    scale,
                    &mut c,
                    key,
                    metric
                )
                .unwrap(),
                resolve_with(
                    &n,
                    Some(Size::new(width, 100.)),
                    Limits::default(),
                    scale,
                    metric
                )
                .unwrap()
            );
            assert_eq!(c.stats().measured_nodes, 0);
        }
    }
}

#[test]
fn a_changed_word_moves_the_cached_floor() {
    // Eight per char, as `metric`, and the longest word is the floor:
    // "ab" cannot shrink, so "a" takes the whole squeeze down to its word.
    let words = |t: &Text, room: Option<f64>| Intrinsic {
        size: metric(t, room),
        min_width: t.value.split(' ').map(str::len).max().unwrap_or(0) as f64 * 8.,
    };
    let solve = |n: &Node<Text>, c: &mut LayoutCache| {
        resolve_cached_with(
            n,
            Some(Size::new(48., 100.)),
            Limits::default(),
            SpacingScale::DEFAULT,
            c,
            key,
            words,
        )
        .unwrap()
    };
    let mut n = Node::row([label("a", "ab abcd"), label("b", "ab")]).id("root");
    let mut c = LayoutCache::default();
    solve(&n, &mut c);
    let l = solve(&n, &mut c);
    assert_eq!(l.frame("a").unwrap().size.width, 32.);
    n.children_mut()[0].payload_mut().value = "ab abcdef".into();
    let l = solve(&n, &mut c);
    assert!(c.stats().measured_nodes > 0);
    assert_eq!(l.frame("a").unwrap().size.width, 48.);
    assert_eq!(l.min_size().width, 64.);
}
#[test]
fn warm_tree_skips_measure_and_arrange() {
    let n = fixture();
    let mut c = LayoutCache::default();
    prime(&n, 400., &mut c);
    let l = cached(&n, 400., &mut c);
    assert_eq!(l, oracle(&n, 400.));
    let s = c.stats();
    assert_eq!(s.measured_nodes, 0);
    assert_eq!(s.arranged_nodes, 0);
    assert_eq!(s.validated_nodes, 7);
}
#[test]
fn decorative_payload_does_not_invalidate_layout() {
    let mut n = fixture();
    let mut c = LayoutCache::default();
    let before = prime(&n, 400., &mut c);
    n.children_mut()[0].children_mut()[0].payload_mut().colour = 42;
    assert_eq!(cached(&n, 400., &mut c), before);
    assert_eq!(c.stats().measured_nodes, 0);
}
#[test]
fn changing_one_label_reuses_other_metrics() {
    let mut n = fixture();
    let mut c = LayoutCache::default();
    prime(&n, 400., &mut c);
    n.children_mut()[0].children_mut()[0].payload_mut().value = "Changed".into();
    assert_eq!(cached(&n, 400., &mut c), oracle(&n, 400.));
    assert!(c.stats().measure_hits > 0);
}
#[test]
fn resize_and_wrap_match_uncached() {
    let n = fixture();
    let mut c = LayoutCache::default();
    for w in [400., 120., 640., 121., 400.] {
        assert_eq!(prime(&n, w, &mut c), oracle(&n, w));
    }
}
#[test]
fn removed_node_does_not_leak_into_layout() {
    let mut n = fixture();
    let mut c = LayoutCache::default();
    prime(&n, 400., &mut c);
    n.children_mut()[0].children_mut()[0] = label("new", "Replacement");
    let l = cached(&n, 400., &mut c);
    assert!(l.frame("a").is_none());
    assert_eq!(l, oracle(&n, 400.));
}
#[test]
fn duplicate_ids_are_validated_on_a_warm_cache() {
    let mut n = fixture();
    let mut c = LayoutCache::default();
    prime(&n, 400., &mut c);
    n.children_mut()[1].children_mut()[0] = label("a", "Duplicate");
    assert!(matches!(
        resolve_cached_with(
            &n,
            Some(Size::new(400., 100.)),
            Limits::default(),
            SpacingScale::DEFAULT,
            &mut c,
            key,
            metric
        ),
        Err(Error::DuplicateKey(_))
    ));
}
#[test]
fn reordered_layout_matches_uncached() {
    let mut n = fixture();
    let mut c = LayoutCache::default();
    prime(&n, 400., &mut c);
    n.children_mut().swap(0, 1);
    assert_eq!(cached(&n, 400., &mut c), oracle(&n, 400.));
}
#[test]
fn pin_dependencies_never_use_stale_frames() {
    let mut n = Node::stack([
        label("anchor", "Hello"),
        label("tip", "Tip").pin(Pin::to("anchor")),
    ]);
    let mut c = LayoutCache::default();
    prime(&n, 400., &mut c);
    n.children_mut()[0] = label("anchor", "Longer anchor").offset(80., 0.);
    assert_eq!(cached(&n, 400., &mut c), oracle(&n, 400.));
}
/// Stamps are keyed by hashed paths now, and the pin lives outside the
/// projection: unkeyed moves, pin edits and an id reused down its own
/// subtree must all still be seen on a warm cache.
#[test]
fn hashed_addresses_see_moves_pins_and_nested_duplicates() {
    let mut n = Node::stack([
        Node::row([label("x", "One"), Node::block(30., 10.)]),
        Node::row([Node::block(50., 10.), label("y", "Two")]),
        label("tip", "Tip").pin(Pin::to("x")),
    ]);
    let mut c = LayoutCache::default();
    assert_eq!(prime(&n, 400., &mut c), oracle(&n, 400.));
    n.children_mut().swap(0, 1);
    assert_eq!(cached(&n, 400., &mut c), oracle(&n, 400.));
    n.children_mut()[2] = label("tip", "Tip").pin(Pin::to("y").area(Area::End));
    assert_eq!(cached(&n, 400., &mut c), oracle(&n, 400.));
    n.children_mut()[0].children_mut()[0] = label("y", "Dup");
    let n = n.id("y");
    assert!(matches!(
        resolve_cached_with(
            &n,
            Some(Size::new(400., 100.)),
            Limits::default(),
            SpacingScale::DEFAULT,
            &mut c,
            key,
            metric
        ),
        Err(Error::DuplicateKey(id)) if id == "y"
    ));
}
#[test]
fn budget_changes_invalidate_cached_work() {
    let n = fixture();
    let mut c = LayoutCache::default();
    prime(&n, 400., &mut c);
    assert!(matches!(
        resolve_cached_with(
            &n,
            Some(Size::new(400., 100.)),
            Limits {
                nodes: 2,
                ..Limits::default()
            },
            SpacingScale::DEFAULT,
            &mut c,
            key,
            metric
        ),
        Err(Error::BudgetExceeded)
    ));
}
#[test]
fn changing_measure_environment_requires_clear() {
    let n = fixture();
    let mut c = LayoutCache::default();
    prime(&n, 400., &mut c);
    c.clear();
    prime(&n, 400., &mut c);
    assert!(c.stats().measured_nodes > 0);
}
