use mui_layout::*;
fn leaf(id: &str, w: f64, h: f64) -> Node {
    Node::leaf(id, Size::new(w, h))
}
fn close(a: f64, b: f64) {
    assert!((a - b).abs() < 0.001, "{a} != {b}");
}
#[test]
fn content_sizing_tokens_and_scoped_components() {
    let component = || {
        Node::column("body", [leaf("knob", 28., 28.), leaf("label", 28., 10.)])
            .gap(Gap::S)
            .pad(Pad::M)
    };
    let root = Node::flow([component().scope("a"), component().scope("b")]).gap(Gap::L);
    let spacing = SpacingScale {
        m: 10.,
        s: 6.,
        l: 20.,
        ..Default::default()
    };
    let l = resolve_with_spacing(&root, None, Limits::default(), &spacing).unwrap();
    close(l.size.width, 116.);
    close(l.size.height, 64.);
    close(l.frame("a/knob").unwrap().x, 10.);
    close(l.frame("b/knob").unwrap().x, 78.);
}
#[test]
fn adaptive_direction_wrap_and_weighted_allocation() {
    let tree = Node::flow([leaf("a", 100., 20.), leaf("b", 100., 30.)])
        .axis(Axis::Auto)
        .gap(10.)
        .width(Fill)
        .height(Hug);
    for (width, height, second_x, second_y) in [
        (240., 30., 110., 0.),
        (150., 60., 25., 30.),
        (210., 30., 110., 0.),
    ] {
        let l = resolve(&tree, Some(Size::new(width, 0.)), Limits::default()).unwrap();
        close(l.size.height, height);
        close(l.frame("b").unwrap().x, second_x);
        close(l.frame("b").unwrap().y, second_y);
    }
    // The parent must switch first: that gives its nested flow enough room
    // to stay a row. Switching all undersized flows at once gets this wrong.
    let nested = Node::flow([
        Node::flow([leaf("x", 70., 20.), leaf("y", 70., 20.)])
            .id("inner")
            .axis(Axis::Auto)
            .shrink(1.)
            .gap(10.),
        leaf("sibling", 100., 20.),
    ])
    .axis(Axis::Auto)
    .gap(10.)
    .width(Fill)
    .height(Hug);
    let l = resolve(&nested, Some(Size::new(180., 0.)), Limits::default()).unwrap();
    close(l.frame("x").unwrap().y, l.frame("y").unwrap().y);
    close(l.frame("y").unwrap().x - l.frame("x").unwrap().x, 80.);
    close(l.frame("sibling").unwrap().y, 30.);
    let wrapped = Node::row(
        "r",
        [
            leaf("a", 80., 20.),
            leaf("b", 80., 20.),
            leaf("c", 80., 20.),
        ],
    )
    .wrap()
    .gap(10.)
    .width(Fill)
    .height(Hug);
    let l = resolve(&wrapped, Some(Size::new(170., 0.)), Limits::default()).unwrap();
    close(l.frame("b").unwrap().x, 90.);
    close(l.frame("c").unwrap().y, 30.);
    for width in [100., 2e6] {
        let root = Node::row(
            "r",
            [
                leaf("a", 10., 10.).grow(1.).max_size(Size::new(20., 20.)),
                leaf("b", 10., 10.).grow(1.),
            ],
        );
        let l = resolve(
            &root,
            Some(Size::new(width, 10.)),
            Limits {
                extent: 2e6,
                ..Default::default()
            },
        )
        .unwrap();
        close(l.frame("a").unwrap().size.width, 20.);
        close(l.frame("b").unwrap().size.width, width - 20.);
    }
}
#[test]
fn content_remeasures_under_available_width_and_overlay_hugs() {
    let root = Node::column("r", [Node::measured("text").width(Fill)])
        .width(Fill)
        .height(Hug);
    for width in [80., 160.] {
        let l = resolve_measured(
            &root,
            Constraints {
                width: Some(width),
                height: None,
            },
            Limits::default(),
            &SpacingScale::default(),
            |_, input| {
                let w = input
                    .known
                    .width
                    .unwrap_or(match input.width {
                        Available::Definite(w) => w,
                        Available::MinContent => 40.,
                        Available::MaxContent => 320.,
                    })
                    .min(320.);
                Ok(Size::new(w, (320. / w.max(1.)).ceil() * 20.))
            },
        )
        .unwrap();
        close(l.size.width, width);
        close(l.size.height, 320. / width * 20.);
    }
    let root = Node::overlay("o", [leaf("a", 10., 10.)])
        .padding(5.)
        .align(Align::Center)
        .justify(Justify::Center);
    let l = resolve(&root, Some(Size::new(40., 50.)), Limits::default()).unwrap();
    close(l.frame("a").unwrap().x, 15.);
    close(l.frame("a").unwrap().y, 20.);
}
#[test]
fn invalid_layouts_never_replace_the_committed_snapshot() {
    let valid = leaf("ok", 10., 10.);
    let mut state = LayoutState::default();
    state.commit(&valid, None, Limits::default()).unwrap();
    let old = state.current().cloned();
    let invalid = [
        leaf("nan", f64::NAN, 10.),
        Node::row("r", [leaf("x", 1., 1.), leaf("x", 1., 1.)]),
        leaf("max", 20., 20.).max_size(Size::new(10., 10.)),
        Node::flow([leaf("a", 1., 1.)]).gap(-1.),
    ];
    for root in invalid {
        assert!(
            state.commit(&root, None, Limits::default()).is_err(),
            "{root:?}"
        );
        assert_eq!(state.current(), old.as_ref());
        assert_eq!(state.revision(), 1);
    }
    assert!(resolve(&valid, Some(Size::new(5., 5.)), Limits::default()).is_err());
    assert!(resolve(
        &valid,
        None,
        Limits {
            nodes: 0,
            ..Default::default()
        }
    )
    .is_err());
}
