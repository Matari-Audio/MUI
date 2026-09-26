use mui_geometry::{
    bez_path, boundary_distance,
    kurbo::{Point as KPoint, Shape},
};
use mui_scene::{Layer, TextCache, prelude::*, resolve_scene_with};

fn has(path: &Path, x: f64, y: f64) -> bool {
    bez_path(path, 0.01).unwrap().winding(KPoint::new(x, y)) != 0
}
fn card(radius: f64) -> El {
    stack![
        leaf(220., 244.)
            .anchor(Align::Start, Align::Start)
            .offset(8., 8.)
            .inset_surface_of([Id::of("above"), Id::of("main"), Id::of("below")])
            .fill(Field)
            .id("well"),
        leaf(156., 244.)
            .anchor(Align::Start, Align::Start)
            .offset(236., 8.)
            .inset_surface()
            .fill(Field)
            .id("other"),
        leaf(48., 82.)
            .anchor(Align::Start, Align::Start)
            .offset(8., 8.)
            .id("above"),
        leaf(48., 82.)
            .anchor(Align::Start, Align::Start)
            .offset(8., 170.)
            .id("below"),
        leaf(172., 244.)
            .anchor(Align::Start, Align::Start)
            .offset(56., 8.)
            .id("main"),
        leaf(40., 80.)
            .anchor(Align::Start, Align::Start)
            .offset(8., 90.)
            .join_border("body")
            .fill(Primary)
            .focusable()
            .id("title"),
    ]
    .w(400.)
    .h(260.)
    .id("body")
    .fill(Surface)
    .radius((radius, radius * 0.7))
    .surface_layout(8.)
    .border_ramp(BorderRamp::horizontal((Primary, 4.), (Dim, 1.5)).transition(0.35, 0.65))
}

#[test]
fn title_joins_and_panel_clearance_follow_the_owner() {
    let mut cache = TextCache::default();
    let mut previous = None;
    for radius in [16., 24., 32.] {
        let spec = SceneSpec::new(card(radius));
        let scene = resolve_scene_with(&spec, &mut cache).unwrap();
        let border = scene
            .paint
            .iter()
            .find(|p| p.key.as_ref() == "body" && p.layer == Layer::Stroke)
            .unwrap();
        assert!(
            has(&border.path, 6., 88.),
            "upper concave root is filled: radius={radius}, title={:?}, body={:?}",
            scene.surface("title").unwrap().frame,
            scene.surface("body").unwrap().frame
        );
        assert!(has(&border.path, 6., 172.), "lower concave root is filled");
        assert!(
            !has(&border.path, 47., 91.),
            "upper exposed corner is rounded"
        );
        assert!(
            !has(&border.path, 47., 169.),
            "lower exposed corner is rounded"
        );
        assert!(has(&border.path, 40., 130.));
        let well = &scene.surface("well").unwrap().path;
        assert_eq!(well.flatten(0.1, 10000).unwrap().len(), 1);
        assert!(!has(well, 40., 130.), "title is outside the panel");
        let band = border.path.flatten(0.05, 100000).unwrap();
        for p in well.flatten(0.05, 100000).unwrap().concat() {
            assert!(boundary_distance(p, &band) > 7.7, "clearance at {p:?}");
        }
        let other = scene.surface("other").unwrap();
        if let Some(old) = previous.replace(other.path.clone()) {
            assert_ne!(old, other.path, "changing only the owner changes the panel");
        }
        let title = scene.surface("title").unwrap();
        assert!(title.focusable);
        assert_eq!(title.frame.size, Size::new(40., 80.));
        assert!(
            !scene
                .paint
                .iter()
                .any(|p| p.key.as_ref() == "title" && p.layer == Layer::Fill)
        );
        let again = resolve_scene_with(&spec, &mut cache).unwrap();
        assert_eq!(other.path, again.surface("other").unwrap().path);
    }
}

#[test]
fn invalid_surface_declarations_fail_instead_of_drawing_an_unrelated_box() {
    for root in [
        card(20.).surface_layout(-1.),
        leaf(20., 20.).inset_surface(),
        leaf(20., 20.).join_border("missing"),
        stack![leaf(20., 20.).inset_surface_of([Id::of("missing")])]
            .w(100.)
            .h(100.)
            .surface_layout(2.),
        stack![leaf(20., 20.).join_border("body")]
            .w(100.)
            .h(100.)
            .id("body")
            .surface_layout(2.),
    ] {
        assert!(resolve_scene(&SceneSpec::new(root)).is_err());
    }
}

#[test]
fn a_welded_port_outline_can_own_the_same_surfaces() {
    let mut body = card(24.);
    body.payload_mut().extras_mut().surface_padding = None;
    body.payload_mut().extras_mut().border_ramp = None;
    body.payload_mut().style.radius = Radius::Pair(0., 0.);
    let root = row![
        leaf(36., 36.)
            .radius(0.)
            .align_self(Align::Center)
            .id("input"),
        body,
        leaf(36., 36.)
            .radius(0.)
            .align_self(Align::Center)
            .id("output"),
    ]
    .gap(0.)
    .union(Surface)
    .radius((24., 16.))
    .surface_layout(8.)
    .border_ramp(
        BorderRamp::horizontal((Primary, 4.), (Dim, 1.5))
            .over("body")
            .tabs([Id::of("input"), Id::of("output")]),
    )
    .w(472.)
    .h(260.);
    let scene = resolve_scene(&SceneSpec::new(root)).unwrap();
    assert_eq!(
        scene
            .surface("well")
            .unwrap()
            .path
            .flatten(0.1, 10000)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn a_uniform_border_preserves_the_panel_interior() {
    let root = stack![leaf(180., 80.).inset_surface().fill(Field).id("panel")]
        .w(200.)
        .h(100.)
        .fill(Surface)
        .radius(20.)
        .stroke(Dim)
        .stroke_width(1.5)
        .surface_layout(8.);
    let scene = resolve_scene(&SceneSpec::new(root)).unwrap();
    let panel = scene.surface("panel").unwrap();
    assert!(has(&panel.path, 100., 50.));
    assert!(
        !has(&panel.path, 10., 10.),
        "partition corner must be rounded"
    );
}

#[test]
fn an_attached_footer_does_not_pull_panels_past_the_body_inset() {
    let mut body = card(24.);
    body.payload_mut().extras_mut().surface_padding = None;
    body.payload_mut().extras_mut().border_ramp = None;
    body.payload_mut().style.radius = Radius::Pair(0., 0.);
    let root = col![body, leaf(24., 24.).align_self(Align::Center)]
        .gap(0.)
        .w(400.)
        .union(Surface)
        .radius((24., 16.))
        .surface_layout(8.)
        .border_ramp(BorderRamp::horizontal((Primary, 4.), (Dim, 1.5)).over("body"));
    let scene = resolve_scene(&SceneSpec::new(root)).unwrap();
    let bottom = scene.surface("body").unwrap().frame.bottom() - 8. - 4.;
    for id in ["well", "other"] {
        for point in scene
            .surface(id)
            .unwrap()
            .path
            .flatten(0.1, 10000)
            .unwrap()
            .concat()
        {
            assert!(
                point.y <= bottom + 0.01,
                "{id} entered the footer: {point:?}"
            );
        }
    }
}

/// A stroked weld owner: path outline, no analytic rect, uniform border.
fn stroked_weld() -> El {
    row![
        leaf(24., 24.)
            .radius(0.)
            .align_self(Align::Center)
            .id("port"),
        stack![leaf(180., 80.).inset_surface().fill(Field).id("panel")]
            .radius(0.)
            .id("module"),
    ]
    .gap(0.)
    .union(Surface)
    .radius((16., 10.))
    .stroke(Dim)
    .stroke_width(1.5)
    .surface_layout(8.)
    .id("owner")
}

#[test]
fn steady_and_tooltip_frames_run_no_boolean_pass() {
    for root in [card(24.), stroked_weld()] {
        let mut cache = TextCache::default();
        let cold = resolve_scene_with(&SceneSpec::new(root.clone()), &mut cache).unwrap();
        let passes = mui_geometry::boolean_passes();
        let steady = resolve_scene_with(&SceneSpec::new(root.clone()), &mut cache).unwrap();
        assert_eq!(
            mui_geometry::boolean_passes(),
            passes,
            "a steady frame reuses every surface, border and weld band"
        );
        // `Ui::frame` wraps the root like this while a tooltip shows: every
        // pre-order index shifts, but no node's identity or geometry does.
        let anchor = root.key().unwrap().to_owned();
        let tip = overlay([
            root,
            leaf(40., 16.)
                .fill(Raised)
                .pin(Pin::to(anchor).area(Area::BottomStart))
                .id("tip"),
        ]);
        let tipped = resolve_scene_with(&SceneSpec::new(tip), &mut cache).unwrap();
        assert_eq!(
            mui_geometry::boolean_passes(),
            passes,
            "a tooltip must not invalidate surface caches"
        );
        for scene in [&steady, &tipped] {
            for p in &cold.paint {
                assert!(
                    scene
                        .paint
                        .iter()
                        .any(|q| q.key == p.key && q.layer == p.layer && q.path == p.path),
                    "{} {:?} changed",
                    p.key,
                    p.layer
                );
            }
        }
    }
}

/// Owner geometry is cached relative to the owner: sliding the whole card
/// over reruns none of its Boolean passes.
#[test]
fn a_moved_owner_reuses_its_surfaces() {
    let spec = |pad: f64| SceneSpec::new(column([card(16.)]).pad(Spacing::Px(pad)));
    let mut cache = TextCache::default();
    resolve_scene_with(&spec(0.), &mut cache).unwrap();
    for pad in [0., 7., 7.5] {
        let before = mui_geometry::boolean_passes();
        let moved = resolve_scene_with(&spec(pad), &mut cache).unwrap();
        assert_eq!(mui_geometry::boolean_passes(), before, "pad {pad}");
        let well = &moved.surface("well").unwrap().path;
        assert!(has(well, pad + 20., pad + 20.), "the well stayed behind");
    }
}
