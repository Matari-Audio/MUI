use mui_core::*;
use mui_geometry::{GeometryOptions, PathCommand};
use mui_layout::{Fill, Hug};
fn chrome(padding: f64, align: Justify) -> SceneSpec {
    let header = Node::row("header", [Node::leaf("tab-frame", Size::new(80., 32.))])
        .padding_xy(0., padding)
        .width(Fill)
        .justify(align);
    let root = Node::column(
        "root",
        [
            header,
            Node::column("lower", [Node::leaf("panel-frame", Size::new(320., 100.))]),
        ],
    )
    .gap(20.)
    .width(Fill)
    .height(Hug);
    SceneSpec::new(root)
        .offered(Size::new(320., 0.))
        .surface(SurfaceSpec::frame("tab", "tab-frame").extend_to(Edge::Bottom, "panel-frame"))
        .surface(SurfaceSpec::frame("panel", "panel-frame"))
        .surface(SurfaceSpec::merge("shell", ["tab", "panel"]))
}
#[test]
fn chrome_tabs_bridge_padding_without_moving_content() {
    for pad in [0., 12., 40.] {
        for (alignment, shoulders) in [(Justify::Start, 1), (Justify::Center, 2), (Justify::End, 1)]
        {
            let spec = chrome(pad, alignment);
            let scene = resolve_scene(&spec).unwrap();
            let content = scene.layout.frame("tab-frame").unwrap();
            let panel = scene.layout.frame("panel-frame").unwrap();
            let tab = scene.surface("tab").unwrap().bounds.unwrap();
            let shell = scene.surface("shell").unwrap();
            assert_eq!(content.size, Size::new(80., 32.));
            assert_eq!(tab.min.x, content.x);
            assert_eq!(tab.min.y, content.y);
            assert_eq!(tab.width(), 80.);
            assert_eq!(tab.max.y, panel.y);
            assert!(tab.max.y > content.bottom());
            assert_eq!(shell.basis.components(), 1);
            assert_eq!(
                shell
                    .path
                    .commands
                    .iter()
                    .filter(|c| matches!(c,PathCommand::ArcTo(a) if a.sweep<0.))
                    .count(),
                shoulders,
                "pad={pad} alignment={alignment:?}"
            );
            let mut detached = spec.clone();
            detached.surfaces[0] = SurfaceSpec::frame("tab", "tab-frame");
            let detached = resolve_scene(&detached).unwrap();
            assert_eq!(detached.layout, scene.layout);
            assert_eq!(detached.surface("shell").unwrap().basis.components(), 2);
        }
    }
}
#[test]
fn attachments_work_in_every_direction_across_layout_branches() {
    for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
        let source =
            Node::column("source-parent", [Node::leaf("source", Size::new(40., 40.))]).padding(10.);
        let target =
            Node::column("target-parent", [Node::leaf("target", Size::new(80., 80.))]).padding(10.);
        let children = if matches!(edge, Edge::Top | Edge::Left) {
            [target, source]
        } else {
            [source, target]
        };
        let root = if matches!(edge, Edge::Top | Edge::Bottom) {
            Node::column("root", children)
        } else {
            Node::row("root", children)
        }
        .gap(20.);
        let scene = resolve_scene(
            &SceneSpec::new(root)
                .surface(SurfaceSpec::frame("a", "source").extend_to(edge, "target"))
                .surface(SurfaceSpec::frame("b", "target"))
                .surface(SurfaceSpec::merge("m", ["a", "b"])),
        )
        .unwrap();
        let a = scene.surface("a").unwrap().bounds.unwrap();
        let b = scene.surface("b").unwrap().bounds.unwrap();
        match edge {
            Edge::Top => assert_eq!(a.min.y, b.max.y),
            Edge::Bottom => assert_eq!(a.max.y, b.min.y),
            Edge::Left => assert_eq!(a.min.x, b.max.x),
            Edge::Auto => unreachable!(),
            Edge::Right => assert_eq!(a.max.x, b.min.x),
        }
        assert_eq!(scene.surface("m").unwrap().basis.components(), 1);
    }
}
#[test]
fn offset_material_survives_remerge_and_zero_surfaces_are_empty() {
    let spec = SceneSpec::new(Node::leaf("r", Size::new(100., 100.)))
        .surface(SurfaceSpec::frame("frame", "r").radius(FrameRadius::Absolute(20.)))
        .surface(SurfaceSpec::inset("inside", "frame", Spacing::px(5.)))
        .surface(
            SurfaceSpec::merge("merge", ["inside"])
                .corners(CornerRule::Absolute(CornerProfile::new(0., 0.))),
        );
    let scene = resolve_scene(&spec).unwrap();
    let inner = scene.surface("inside").unwrap();
    assert_eq!(inner.analytic_rect.unwrap().radius(), 15.);
    let expected = 8100. - (4. - std::f64::consts::PI) * 225.;
    assert!((scene.surface("merge").unwrap().basis.area() - expected).abs() < 3.);
    for size in [Size::new(0., 10.), Size::new(10., 0.), Size::new(0., 0.)] {
        let scene = resolve_scene(
            &SceneSpec::new(Node::leaf("r", size)).surface(SurfaceSpec::frame("empty", "r")),
        )
        .unwrap();
        assert!(scene.surface("empty").unwrap().path.commands.is_empty());
    }
}
#[test]
fn graph_order_and_validation_are_consistent() {
    for count in [12, 132] {
        let mut spec = SceneSpec::new(Node::leaf("r", Size::new(100., 100.)))
            .surface(SurfaceSpec::frame("s0", "r"));
        for i in 1..=count {
            spec.surfaces.push(SurfaceSpec::outset(
                format!("s{i}"),
                format!("s{}", i - 1),
                Spacing::px(0.),
            ));
        }
        let first = resolve_scene(&spec);
        spec.surfaces.reverse();
        let second = resolve_scene(&spec);
        assert_eq!(first.is_ok(), count <= 128);
        assert_eq!(first.is_ok(), second.is_ok());
        if let (Ok(a), Ok(b)) = (first, second) {
            assert_eq!(a, b);
        }
    }
    let cycle = SceneSpec::new(Node::leaf("r", Size::new(10., 10.)))
        .surface(SurfaceSpec::inset("a", "b", Spacing::s()))
        .surface(SurfaceSpec::inset("b", "a", Spacing::s()));
    assert!(matches!(
        resolve_scene(&cycle),
        Err(SceneError::DependencyCycle(_))
    ));
}
#[test]
fn invalid_scene_edits_preserve_previous_geometry() {
    let good = chrome(12., Justify::Center);
    let mut state = SceneState::default();
    state.commit(&good).unwrap();
    let old = state.current().cloned();
    let mut limited = good.clone();
    limited.geometry_options = GeometryOptions {
        coordinate_limit: 50.,
        ..Default::default()
    };
    let mut missing = good.clone();
    missing.surfaces[0] = SurfaceSpec::frame("tab", "tab-frame").extend_to(Edge::Bottom, "missing");
    let mut wrong_axis = good.clone();
    wrong_axis.surfaces[0] =
        SurfaceSpec::frame("tab", "tab-frame").extend_to(Edge::Right, "panel-frame");
    let mut empty_source = good.clone();
    empty_source.root = Node::column(
        "root",
        [
            Node::leaf("tab-frame", Size::new(80., 0.)),
            Node::leaf("panel-frame", Size::new(320., 100.)),
        ],
    );
    let mut wrong_modifier = good.clone();
    wrong_modifier.surfaces[2] =
        SurfaceSpec::merge("shell", ["tab", "panel"]).radius(FrameRadius::Absolute(3.));
    for spec in [limited, missing, wrong_axis, wrong_modifier, empty_source] {
        assert!(state.commit(&spec).is_err());
        assert_eq!(state.current(), old.as_ref());
        assert_eq!(state.revision(), 1);
    }
}
