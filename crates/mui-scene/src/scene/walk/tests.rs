use super::super::*;
use crate::Paint;
use crate::prelude::*;

#[test]
fn subtree_rotation_keeps_layout_clips_order_and_retained_pose() {
    let spec = |angle, reused| {
        let mut scope = stack([block(10., 4.)
            .offset(6., 2.)
            .anchor(Align::Start, Align::Start)
            .fill(Role::Primary)
            .id("rot.leaf")])
        .size(20., 10.)
        .offset(10., 10.)
        .anchor(Align::Start, Align::Start)
        .rotation(angle)
        .id("rot.scope");
        scope.payload_mut().extras_mut().memo = Some(crate::Memo { id: 91, reused });
        SceneSpec::new(stack([scope]).size(22., 18.).clip().id("rot.root"))
    };
    let mut cache = TextState::default();
    let angle = std::f64::consts::FRAC_PI_2;
    let first = retained(&spec(angle, false), &mut cache, None);
    let leaf = first.surface("rot.leaf").unwrap();
    assert_eq!(
        (
            leaf.frame.x,
            leaf.frame.y,
            leaf.frame.size.width,
            leaf.frame.size.height
        ),
        (16., 12., 10., 4.)
    );
    assert!(
        leaf.local(Point::new(21., 15.))
            .distance(Point::new(4., 2.))
            < 1e-8
    );
    assert_eq!(leaf.clip, Some(Rect::new(0., 0., 22., 18.)));
    let clip = first
        .paint
        .iter()
        .position(|p| p.layer == Layer::Clip)
        .unwrap();
    let fill = first
        .paint
        .iter()
        .position(|p| p.key.as_str() == "rot.leaf" && p.layer == Layer::Fill)
        .unwrap();
    assert!(clip < fill);
    assert_eq!(
        first.paint[clip].transform,
        mui_geometry::kurbo::Affine::IDENTITY
    );
    assert_ne!(
        first.paint[fill].transform,
        mui_geometry::kurbo::Affine::IDENTITY
    );
    let pose = leaf.local_pose();
    let same = retained(&spec(angle, true), &mut cache, Some(&first));
    assert_eq!(same.paint, first.paint);
    assert!(Arc::ptr_eq(
        &same.surface("rot.leaf").unwrap().path,
        &leaf.path
    ));
    assert_eq!(
        same.memos_at("rot.scope").collect::<Vec<_>>(),
        vec![(91, true)]
    );
    let changed = retained(&spec(std::f64::consts::PI, true), &mut cache, Some(&same));
    assert_ne!(
        changed.paint, first.paint,
        "rotation after a memo must invalidate its retained paint"
    );
    assert!(
        !Arc::ptr_eq(&changed.surface("rot.leaf").unwrap().path, &leaf.path),
        "a retained tree with changed pose must regenerate its world hit path"
    );
    assert_eq!(changed.surface("rot.leaf").unwrap().frame, leaf.frame);
    assert!(
        pose.local(Point::new(21., 15.))
            .distance(Point::new(4., 2.))
            < 1e-8,
        "captured coordinate pose must remain unchanged"
    );
    let fresh = resolve(&spec(std::f64::consts::PI, false)).unwrap();
    assert_eq!(changed.paint, fresh.paint);
    for angle in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(resolve(&spec(angle, false)).is_err());
    }
}

#[test]
fn underlay_follows_foreground_geometry_and_keeps_paint_clip_and_hit_order() {
    let spec = |width| {
        SceneSpec::new(
            stack([
                block(width, 40.).fill(Role::Primary).id("foreground"),
                block(Len::Pct(100.), Len::Pct(100.))
                    .fill(Role::Raised)
                    .push(
                        block(Len::Container(100.), 10.)
                            .offset(-3., 4.)
                            .fill(Role::Danger)
                            .id("decoration-child"),
                    )
                    .underlay()
                    .id("background"),
            ])
            .pad(4.)
            .radius(8.)
            .clip()
            .id("composition"),
        )
    };
    let mut cache = TextState::default();
    for width in [120., 180.] {
        let scene = cache.resolve(&spec(width)).unwrap();
        assert_eq!(scene.layout.size, Size::new(width + 8., 48.));
        assert_eq!(
            scene.layout.frame("background"),
            scene.layout.frame("foreground")
        );
        let decoration = scene.layout.frame("decoration-child").unwrap();
        assert_eq!(
            (decoration.x, decoration.size.width),
            (1., width),
            "final parent width drives decoration measurement"
        );
        let layers: Vec<_> = scene.paint.iter().map(|p| (&*p.key, p.layer)).collect();
        let at = |key, layer| layers.iter().position(|p| *p == (key, layer)).unwrap();
        assert!(at("composition", Layer::Clip) < at("background", Layer::Fill));
        assert!(at("background", Layer::Fill) < at("foreground", Layer::Fill));
        assert!(at("foreground", Layer::Fill) < at("composition", Layer::Unclip));
        let parent_clip = scene.surface("composition").unwrap().bounds;
        assert_eq!(scene.surface("background").unwrap().clip, parent_clip);
        assert_eq!(scene.surface("decoration-child").unwrap().clip, parent_clip);
        let surfaces: Vec<_> = scene.surfaces().map(|s| &*s.key).collect();
        assert!(
            surfaces.iter().position(|k| *k == "background").unwrap()
                < surfaces.iter().position(|k| *k == "foreground").unwrap(),
            "foreground keeps topmost hit ownership"
        );
    }
}

#[test]
fn local_overlay_follows_foreground_and_paints_inside_clip_after_hit_owner() {
    let tree = || {
        stack([
            block(Len::Pct(100.), Len::Pct(100.))
                .push(
                    block(Len::Container(100.), 10.)
                        .fill(Role::Danger)
                        .id("overlay-child"),
                )
                .fill(Role::Raised)
                .overlay()
                .id("overlay"),
            block(120., 40.).fill(Role::Primary).id("foreground"),
            block(Len::Pct(100.), Len::Pct(100.))
                .fill(Role::Surface)
                .underlay()
                .id("background"),
        ])
        .pad(4.)
        .radius(8.)
        .clip()
        .id("composition")
    };
    let scene = resolve(&SceneSpec::new(tree())).unwrap();
    assert_eq!(scene.layout.size, Size::new(128., 48.));
    assert_eq!(
        scene.layout.frame("overlay"),
        scene.layout.frame("foreground")
    );
    assert_eq!(
        scene.layout.frame("overlay-child").unwrap().size.width,
        120.
    );
    let layers: Vec<_> = scene.paint.iter().map(|p| (&*p.key, p.layer)).collect();
    let at = |key, layer| layers.iter().position(|p| *p == (key, layer)).unwrap();
    assert!(at("composition", Layer::Clip) < at("background", Layer::Fill));
    assert!(at("background", Layer::Fill) < at("foreground", Layer::Fill));
    assert!(at("foreground", Layer::Fill) < at("overlay", Layer::Fill));
    assert!(at("overlay-child", Layer::Fill) < at("composition", Layer::Unclip));
    let clip = scene.surface("composition").unwrap().bounds;
    assert_eq!(scene.surface("overlay").unwrap().clip, clip);
    assert_eq!(scene.surface("overlay-child").unwrap().clip, clip);
    let surfaces: Vec<_> = scene.surfaces().map(|s| &*s.key).collect();
    assert!(
        surfaces.iter().position(|k| *k == "foreground").unwrap()
            < surfaces.iter().position(|k| *k == "overlay-child").unwrap()
    );
    assert!(
        !tree().children()[0].clone().float().is_overlay(),
        "portal float replaces local overlay placement"
    );
}

/// Paths are local: two same-sized buttons paint one outline `Arc`, each
/// at its own offset, and a button that only moved keeps it -- its
/// surface too, so the hit map converts nothing new.
#[test]
fn same_shapes_share_one_path_and_a_move_keeps_it() {
    let spec = |gap: f64| {
        SceneSpec::new(
            row((0..2).map(|i| {
                block(40., 20.)
                    .radius(6.)
                    .fill(Role::Primary)
                    .id(format!("b{i}"))
            }))
            .gap(gap)
            .pad(gap),
        )
    };
    let fill = |s: &ResolvedScene, k: &str| {
        s.paint
            .iter()
            .find(|p| p.layer == Layer::Fill && &*p.key == k)
            .map(|p| (p.path.clone(), p.offset))
            .unwrap()
    };
    let mut cache = TextState::default();
    let first = cache.resolve(&spec(4.)).unwrap();
    let ((a, at_a), (b, at_b)) = (fill(&first, "b0"), fill(&first, "b1"));
    assert!(Arc::ptr_eq(&a, &b), "one path for both buttons");
    assert_eq!((at_a, at_b), (Point::new(4., 4.), Point::new(48., 4.)));
    let moved = cache.resolve(&spec(10.)).unwrap();
    let (c, at_c) = fill(&moved, "b1");
    assert!(Arc::ptr_eq(&a, &c), "a move keeps the path");
    assert_eq!(at_c, Point::new(60., 10.));
    let s = moved.surface("b1").unwrap();
    assert!(Arc::ptr_eq(&s.path, &a) && s.offset == at_c);
    let placed = moved.surface("b1").unwrap().placed();
    let first_point = match placed.commands[0] {
        mui_geometry::PathCommand::MoveTo(p) => p,
        ref c => panic!("{c:?}"),
    };
    assert!(first_point.x >= 60. && first_point.y >= 10.);
}

/// The fade paints last, source-atop, and only inside the layer the node
/// opened for it.
#[test]
fn a_mask_paints_inside_the_nodes_own_blend_layer() {
    let fade = Gradient::linear(
        180.,
        [(0.7, Role::Surface.alpha(0.)), (1., Role::Surface.into())],
    );
    let row = col![block(40., 20.).fill(Role::Primary)]
        .pad(8.)
        .mask(fade)
        .id("list");
    let s = resolve(&SceneSpec::new(row)).unwrap();
    let at = |l: Layer| s.paint.iter().position(|p| p.layer == l).unwrap();
    let blend = at(Layer::Blend {
        mix: Mix::Normal,
        opacity: 1.0,
    });
    assert!(
        blend < at(Layer::Fill),
        "the subtree paints inside the layer"
    );
    assert!(at(Layer::Fill) < at(Layer::Mask));
    assert!(at(Layer::Mask) < at(Layer::Unblend));
    assert!(!s.paint[at(Layer::Mask)].path.commands.is_empty());
}

/// A blended node's whole subtree, clip included, sits between the pair.
#[test]
fn a_blended_node_is_wrapped_in_a_layer_pair() {
    let dim = col([block(10., 10.).fill(Role::Ink).id("kid")])
        .fill(Role::Surface)
        .blend(Mix::Multiply)
        .opacity(0.5)
        .id("dim");
    let root = row([dim, block(10., 10.).fill(Role::Surface).id("plain")]).id("root");
    let s = resolve(&SceneSpec::new(root).offered(Size::new(60., 20.))).unwrap();
    let layers: Vec<_> = s.paint.iter().map(|p| (&*p.key, p.layer)).collect();
    let at = |k, l| layers.iter().position(|x| *x == (k, l)).unwrap();
    let open = at(
        "dim",
        Layer::Blend {
            mix: Mix::Multiply,
            opacity: 0.5,
        },
    );
    assert!(open < at("dim", Layer::Fill));
    assert!(at("kid", Layer::Fill) < at("dim", Layer::Unblend));
    assert!(
        !layers
            .iter()
            .any(|(k, l)| *k == "plain" && matches!(l, Layer::Blend { .. } | Layer::Unblend)),
        "{layers:?}"
    );
}

#[test]
fn a_weld_clips_its_children_to_the_shared_contour() {
    // A square tab welded to a rounded body: the tab's own fill must not
    // paint the corner the fillet rounded off, so the weld clips.
    let root = row([
        block(36., 60.).fill(Role::Surface).radius(0.).id("tab"),
        block(200., 120.).id("body"),
    ])
    .radius(20.)
    .union(Role::Surface)
    .id("weld");
    let s = resolve(&SceneSpec::new(root).offered(Size::new(236., 120.))).unwrap();
    let order: Vec<_> = s.paint.iter().map(|p| (&*p.key, p.layer)).collect();
    let at = |key: &str, layer: Layer| order.iter().position(|x| *x == (key, layer)).unwrap();
    assert!(
        at("weld", Layer::Fill) < at("weld", Layer::Clip)
            && at("weld", Layer::Clip) < at("tab", Layer::Fill)
            && at("tab", Layer::Fill) < at("weld", Layer::Unclip),
        "the tab painted outside the welded contour: {order:?}"
    );
    let clip = &s.paint[at("weld", Layer::Clip)].path;
    let corner = clip.flatten(0.1, 20_000).unwrap().concat();
    assert!(
        !corner.iter().any(|p| p.x.abs() < 1e-6 && p.y.abs() < 1e-6),
        "the clip is the square frame, not the filleted contour"
    );
}

#[test]
fn clip_floats_canvas_cursor_and_content() {
    let list = col([block(50., 30.).id("a"), block(50., 30.), block(50., 30.)])
        .gap(10.)
        .scroll()
        .scrolled(0., 25.)
        .size(60., 60.)
        .cursor(Cursor::Hand)
        .id("list");
    let tip = block(10., 10.).fill(Role::Primary).float().id("tip");
    let draw = canvas(|s| {
        vec![Draw::stroke(
            Path::default().move_to(Point::new(0., s.height)).cubic_to(
                Point::new(s.width / 2., 0.),
                Point::new(s.width / 2., 0.),
                Point::new(s.width, s.height),
            ),
            Role::Primary,
            2.,
        )]
    })
    .size(40., 40.)
    .id("curve");
    let root = col([list, tip, draw]).id("root");
    let s = resolve(&SceneSpec::new(root)).unwrap();
    let layers: Vec<_> = s.paint.iter().map(|p| (&*p.key, p.layer)).collect();
    let at = |k: &str, l: Layer| layers.iter().position(|x| *x == (k, l)).unwrap();
    assert!(at("list", Layer::Clip) < at("list", Layer::Unclip));
    assert_eq!(*layers.last().unwrap(), ("tip", Layer::Fill), "{layers:?}");
    assert!(
        s.paint
            .iter()
            .any(|p| &*p.key == "curve" && p.layer == Layer::Draw(0) && p.width == 2.)
    );
    let list = s.surface("list").unwrap();
    assert_eq!(
        list.content,
        Size::new(55., 110.),
        "rows centred at x=5, three rows and two gaps tall"
    );
    assert_eq!(list.clip, None);
    assert_eq!(s.surface("a").unwrap().clip, Some(list.bounds.unwrap()));
    assert_eq!(s.surface("a").unwrap().cursor, Some(Cursor::Hand));
    assert_eq!(s.layout.frame("a").unwrap().y, -25.);
    assert_eq!(s.surfaces().last().map(|s| &*s.key), Some("tip"));
}

#[test]
fn rounded_clip_exposes_cached_path_alongside_rect_bounds() {
    let clip = col([block(20., 20.).id("a"), block(20., 20.).id("b")])
        .size(40., 40.)
        .radius(10.)
        .clip()
        .id("clip");
    let s = resolve(&SceneSpec::new(clip)).unwrap();
    let parent = s.surface("clip").unwrap();
    let child = s.surface("a").unwrap();
    let sibling = s.surface("b").unwrap();
    assert_eq!(parent.clip, None);
    assert_eq!(child.clip, parent.bounds);
    assert_eq!(sibling.clip, parent.bounds);
    let paths = child.clip_path.as_ref().expect("rounded clip path");
    assert_eq!(paths.len(), 1);
    assert!(
        paths[0]
            .0
            .commands
            .iter()
            .any(|c| matches!(c, mui_geometry::PathCommand::ArcTo(_))),
        "clip path lost its rounded corners"
    );
    assert!(
        Arc::ptr_eq(
            child.clip_path.as_ref().unwrap(),
            sibling.clip_path.as_ref().unwrap()
        ),
        "clip path must remain cached for repeated hit tests"
    );
}

#[test]
fn nested_rounded_clips_keep_every_cached_path() {
    let inner = col([block(30., 30.).id("leaf")])
        .size(30., 30.)
        .radius(6.)
        .clip()
        .id("inner");
    let outer = col([inner]).size(40., 40.).radius(10.).clip().id("outer");
    let s = resolve(&SceneSpec::new(outer)).unwrap();
    let paths = s.surface("leaf").unwrap().clip_path.as_ref().unwrap();
    assert_eq!(
        paths.len(),
        2,
        "inner and outer clips must both filter hits"
    );
    for (p, k) in paths.iter().zip(["outer", "inner"]) {
        assert!(
            Arc::ptr_eq(&p.0, &s.surface(k).unwrap().path),
            "{k}'s clip is a copy of its outline"
        );
    }
    assert!(paths.iter().all(|p| {
        p.0.commands
            .iter()
            .any(|c| matches!(c, mui_geometry::PathCommand::ArcTo(_)))
    }));
}

/// A sticky header paints after the rows that slide under it, and stays
/// inside the scroll's clip -- a float would escape it.
#[test]
fn a_sticky_header_paints_over_its_section_and_keeps_the_clip() {
    let section = col([
        block(60., 20.).fill(Role::Surface).id("head").sticky(),
        block(60., 60.).id("row"),
    ])
    .id("section");
    let list = col([section]).scroll().size(60., 40.).id("list");
    let s = resolve(&SceneSpec::new(list)).unwrap();
    let order: Vec<_> = s.surfaces().map(|s| &*s.key).collect();
    assert_eq!(
        order,
        ["list", "section", "row", "head"],
        "the header paints last"
    );
    let clip = s.surface("list").unwrap().bounds;
    assert_eq!(s.surface("head").unwrap().clip, clip);
}

/// A tagged draw is hit geometry in scene space; an untagged one is
/// paint and nothing else.
#[test]
fn a_tagged_draw_becomes_the_surfaces_hit_shape() {
    let box_ = |w: f64, h: f64| {
        Path::polyline(
            [(0., 0.), (w, 0.), (w, h), (0., h)].map(|(x, y)| Point::new(x, y)),
            true,
        )
    };
    let plot = canvas(move |s| {
        vec![
            Draw::fill(box_(s.width, s.height), Role::Primary),
            Draw::hit(box_(s.width / 2., s.height), "left"),
        ]
    })
    .size(40., 20.)
    .id("plot");
    let root = col([block(40., 30.), plot]);
    let s = resolve(&SceneSpec::new(root)).unwrap();
    let surface = s.surface("plot").unwrap();
    let [(tag, path)] = &surface.hits[..] else {
        panic!("one tagged draw, got {:?}", surface.hits.len())
    };
    assert_eq!(&**tag, "left");
    let pts = path.flatten(0.1, 1000).unwrap().concat();
    let top = pts.iter().map(|p| p.y).fold(f64::MAX, f64::min);
    assert_eq!(top + surface.offset.y, 30., "placed in the node's frame");
    assert!(
        !s.paint
            .iter()
            .any(|p| &*p.key == "plot" && p.layer == Layer::Draw(1)),
        "a hit-only draw paints nothing"
    );
}

#[test]
fn a_baseline_row_lines_two_sizes_up_on_the_letters() {
    let root = row([
        text("a").text_size(12.).id("small"),
        text("b").text_size(24.).id("big"),
    ])
    .baseline();
    let mut sp = SceneSpec::new(root);
    sp.font = Some(font());
    let s = resolve(&sp).unwrap();
    let y = |k: &str| {
        s.paint
            .iter()
            .find(|p| &*p.key == k && p.layer == Layer::Text)
            .unwrap()
            .text
            .as_ref()
            .unwrap()
            .origin
            .y
    };
    assert_eq!(y("small"), y("big"), "one baseline, two sizes");
    let mut plain = sp.clone();
    plain.root = row([
        text("a").text_size(12.).id("small"),
        text("b").text_size(24.).id("big"),
    ]);
    let p = resolve(&plain).unwrap();
    let py = |k: &str| {
        p.paint
            .iter()
            .find(|x| &*x.key == k && x.layer == Layer::Text)
            .unwrap()
            .text
            .as_ref()
            .unwrap()
            .origin
            .y
    };
    assert_ne!(py("small"), py("big"), "and centring alone does not");
}

/// Every text layer's baseline for a key, in paint order.
fn baselines(s: &ResolvedScene, k: &str) -> Vec<f64> {
    s.paint
        .iter()
        .filter(|p| &*p.key == k && p.layer == Layer::Text)
        .map(|p| p.text.as_ref().unwrap().origin.y)
        .collect()
}

#[test]
fn a_baseline_row_taller_than_its_text_keeps_the_letters_in_their_frames() {
    let root = row([
        text("Synth").text_size(22.).id("title"),
        text("v1.0").text_size(11.).id("ver"),
        block(80., 40.).id("btn"),
    ])
    .baseline()
    .gap(10.);
    let mut sp = SceneSpec::new(root).offered(Size::new(400., 60.));
    sp.font = Some(font());
    let s = resolve(&sp).unwrap();
    let t = s.layout.frame("title").unwrap();
    let b = baselines(&s, "title")[0];
    assert_eq!(b, baselines(&s, "ver")[0], "one baseline, two sizes");
    assert!(
        b > t.y && b < t.bottom(),
        "baseline {b} outside the title's frame {t:?}"
    );
}

#[test]
fn a_wrapping_baseline_row_gives_every_line_its_own_baseline() {
    let root = row([
        text("alpha").text_size(20.).id("a"),
        text("beta").text_size(11.).id("b"),
        text("gamma").text_size(20.).id("c"),
    ])
    .wrap()
    .baseline()
    .gap(8.);
    let mut sp = SceneSpec::new(root).offered(Size::new(120., 200.));
    sp.font = Some(font());
    let s = resolve(&sp).unwrap();
    let (a, c) = (baselines(&s, "a")[0], baselines(&s, "c")[0]);
    assert_eq!(a, baselines(&s, "b")[0], "line one shares a baseline");
    assert!(a < c, "line two sits below line one: {a} {c}");
    let f = s.layout.frame("c").unwrap();
    assert!(c > f.y && c < f.bottom(), "baseline {c} outside {f:?}");
}

#[test]
fn a_device_scale_puts_every_edge_and_every_baseline_on_the_grid() {
    let long = "wrap ".repeat(40);
    let row = row![
        block(0., 20.).grow(1.).id("a"),
        block(0., 20.).grow(1.).id("b"),
        block(0., 20.).grow(1.).id("c"),
    ];
    let root = col([row, col([text(long).id("p")]).w(120)]);
    // Tall enough for the paragraph: a column it overflows squeezes the
    // row, whose blocks may be squeezed and text may not.
    let mut sp = SceneSpec::new(root)
        .offered(Size::new(41., 1000.))
        .scale(1.);
    sp.font = Some(font());
    let s = resolve(&sp).unwrap();
    let edges: Vec<[f64; 2]> = ["a", "b", "c"]
        .iter()
        .map(|k| {
            let s = s.surface(k).unwrap();
            let b = s.rect.unwrap().bounds() + s.offset.to_vec2();
            [b.x0, b.x1]
        })
        .collect();
    for e in edges.iter().flatten() {
        assert_eq!(*e, e.round(), "{edges:?}");
    }
    assert_eq!(edges[0][1], edges[1][0], "no seam between shares");
    assert_eq!(edges[1][1], edges[2][0], "no seam between shares");
    let ys = baselines(&s, "p");
    assert!(ys.len() > 3, "wrapped into {} lines", ys.len());
    let gaps: Vec<f64> = ys.windows(2).map(|w| w[1] - w[0]).collect();
    assert!(
        gaps.windows(2).all(|g| g[0] == g[1]) && ys[0] == ys[0].round(),
        "uneven leading {gaps:?} from {ys:?}"
    );
}

/// Fill, clip, mask and surface all hold the node's one outline.
#[test]
fn a_filled_clipping_node_shares_one_outline() {
    let card = col([block(10., 10.)])
        .fill(Role::Surface)
        .clip()
        .mask(Role::Surface)
        .id("card");
    let s = resolve(&SceneSpec::new(card)).unwrap();
    let outline = &s.surface("card").unwrap().path;
    for layer in [Layer::Fill, Layer::Clip, Layer::Mask] {
        let p = s.paint.iter().find(|p| p.layer == layer).unwrap();
        assert!(Arc::ptr_eq(&p.path, outline), "{layer:?} copied it");
    }
}

/// A warm resolve hands every node the key it had last frame: its id,
/// or its tree path.
#[test]
fn a_warm_resolve_gives_every_node_the_same_key() {
    let spec = SceneSpec::new(col([block(10., 10.).id("named"), block(10., 10.)]));
    let mut text = TextState::default();
    let cold = text.resolve(&spec).unwrap();
    let warm = text.resolve(&spec).unwrap();
    assert_eq!(cold.surfaces().count(), 3);
    for (a, b) in cold.surfaces().zip(warm.surfaces()) {
        assert_eq!(a.key, b.key);
    }
    let keys: Vec<&str> = warm.surfaces().map(|s| s.key.as_str()).collect();
    assert_eq!(keys, ["", "named", "/1"]);
}

/// A memo moved by a whole number of device pixels is copied, even when
/// float noise keeps the product off the exact integer.
#[test]
fn a_move_is_on_the_grid_up_to_float_noise() {
    assert_ne!((0.1 * 3.0 * 10.0_f64).fract(), 0.0);
    assert!(super::on_grid(0.1 * 3.0, Some(10.)));
    assert!(super::on_grid(0.25, None));
    assert!(!super::on_grid(0.25, Some(1.)));
    assert!(!super::on_grid(f64::NAN, Some(1.)));
}

/// A NaN size is no size: the node is skipped like a collapsed one,
/// not painted with a NaN outline. (The layout refuses NaN input, so
/// this is the walk's own guard, tested on its predicate.)
#[test]
fn a_nan_frame_is_collapsed() {
    let f = |w, h| Frame {
        x: 0.,
        y: 0.,
        size: Size::new(w, h),
    };
    assert!(super::collapsed(f(f64::NAN, 10.)));
    assert!(super::collapsed(f(10., f64::NAN)));
    assert!(super::collapsed(f(0., 10.)));
    assert!(!super::collapsed(f(10., 10.)));
}

#[test]
fn a_rect_surface_reads_its_bounds_off_the_rect() {
    let s = resolve(&SceneSpec::new(col([block(40., 20.).id("k")]))).unwrap();
    let k = s.surface("k").unwrap();
    assert_eq!(k.bounds.unwrap(), k.rect.unwrap().bounds());
}

#[test]
fn a_text_node_keeps_no_fill_layer_and_no_glyph_path() {
    let root = col([text("hi").fill(Role::Primary).id("t")]);
    let mut sp = SceneSpec::new(root);
    sp.font = Some(font());
    let s = resolve(&sp).unwrap();
    assert!(
        !s.paint
            .iter()
            .any(|p| &*p.key == "t" && p.layer == Layer::Fill),
        "a label's fill is its ink, not a box: {:?}",
        s.paint.iter().map(|p| p.layer).collect::<Vec<_>>()
    );
    let ink = s.paint.iter().find(|p| p.layer == Layer::Text).unwrap();
    assert_eq!(ink.paint, Paint::Solid(Theme::default().palette.primary()));
    assert!(ink.text.is_some(), "the glyphs are the ink");
    assert!(
        ink.path.commands.is_empty(),
        "and the outline is not built twice"
    );
}

/// Like a canvas, an outline or a state style may capture an `Rc`.
#[test]
fn outline_and_state_closures_may_capture_a_non_send_handle() {
    let seen = std::rc::Rc::new(std::cell::Cell::new(0));
    let (c, d) = (seen.clone(), seen.clone());
    let root = block(10., 10.)
        .outline(move |s| {
            c.set(c.get() + 1);
            Path::polyline(
                [
                    Point::ZERO,
                    Point::new(s.width, 0.),
                    Point::new(0., s.height),
                ],
                true,
            )
        })
        .on(State::Hover, move |st| {
            d.set(d.get() + 1);
            st
        });
    resolve(&SceneSpec::new(root)).unwrap();
    assert!(seen.get() >= 1);
}

#[test]
fn a_canvas_closure_may_capture_a_non_send_handle() {
    let seen = std::rc::Rc::new(std::cell::Cell::new(0));
    let c = seen.clone();
    let root = canvas(move |_| {
        c.set(c.get() + 1);
        Vec::new()
    })
    .size(10., 10.);
    resolve(&SceneSpec::new(root)).unwrap();
    assert_eq!(seen.get(), 1);
}
/// A cached draw list paints the very same paths every frame, where it
/// stays and where it moves: only its offset follows it.
#[test]
fn a_cached_canvas_reuses_its_placed_paths() {
    let cache = crate::CanvasCache::new();
    let spec = |pad: f64| {
        let draw = crate::canvas_keyed(&cache, 1, |s| {
            vec![crate::Draw::fill(
                Path::polyline(
                    [
                        Point::ZERO,
                        Point::new(s.width, 0.),
                        Point::new(0., s.height),
                    ],
                    true,
                ),
                Role::Primary,
            )]
        })
        .size(20., 20.)
        .id("c");
        SceneSpec::new(col([draw]).pad(Spacing::Px(pad)))
    };
    let path = |s: &ResolvedScene| {
        let p = s.paint.iter().find(|p| p.layer == Layer::Draw(0)).unwrap();
        (p.path.clone(), p.placed())
    };
    let mut text = TextState::default();
    let (a, _) = path(&text.resolve(&spec(4.)).unwrap());
    let (b, _) = path(&text.resolve(&spec(4.)).unwrap());
    assert!(Arc::ptr_eq(&a, &b), "a still canvas re-placed its paths");
    let (moved, placed) = path(&text.resolve(&spec(9.)).unwrap());
    assert!(
        Arc::ptr_eq(&a, &moved),
        "a moved canvas re-placed its paths"
    );
    assert_eq!(
        placed.commands[0],
        mui_geometry::PathCommand::MoveTo(Point::new(9., 9.))
    );
}

/// A memoised panel after a lead of width `lead`: a rounded button, a
/// label and a canvas that counts its draws.
fn memo_spec(lead: f64, reused: bool, draws: &Arc<std::sync::atomic::AtomicUsize>) -> SceneSpec {
    static FONT: std::sync::LazyLock<Font> = std::sync::LazyLock::new(super::super::font);
    let count = draws.clone();
    let mut panel = col([
        block(40., 20.).fill(Role::Primary).radius(6.).id("m.a"),
        text("kept").id("m.t"),
        canvas(move |s| {
            count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            vec![crate::Draw::fill(
                Path::polyline([Point::ZERO, Point::new(s.width, s.height)], true),
                Role::Ink,
            )]
        })
        .size(10., 10.)
        .id("m.c"),
    ])
    .fill(Role::Surface)
    .id("m");
    panel.payload_mut().extras_mut().memo = Some(crate::Memo { id: 7, reused });
    let lead = block(lead, 10.).fill(Role::Raised);
    let mut spec =
        SceneSpec::new(row([lead, panel]).align(Align::Start)).offered(Size::new(300., 100.));
    spec.font = Some(FONT.clone());
    spec
}
fn draws() -> Arc<std::sync::atomic::AtomicUsize> {
    Arc::default()
}
fn count(d: &Arc<std::sync::atomic::AtomicUsize>) -> usize {
    d.load(std::sync::atomic::Ordering::Relaxed)
}
fn retained(spec: &SceneSpec, text: &mut TextState, prev: Option<&ResolvedScene>) -> ResolvedScene {
    super::super::resolve_with(
        spec,
        text,
        &mut crate::WeldCache::default(),
        &mut |_, _, f| f,
        prev,
    )
    .unwrap()
}

/// A reused memo is copied, not walked: its canvas is not drawn again,
/// and what it paints is what a walk would, down to the same `Arc`s.
#[test]
fn a_reused_memo_copies_last_resolves_paint() {
    let d = draws();
    let mut text = TextState::default();
    let a = retained(&memo_spec(50., false, &d), &mut text, None);
    let b = retained(&memo_spec(50., true, &d), &mut text, Some(&a));
    assert_eq!(count(&d), 1, "the reused canvas was drawn again");
    assert_eq!(a.paint, b.paint);
    let shared = a
        .paint
        .iter()
        .zip(&b.paint)
        .filter(|(x, y)| Arc::ptr_eq(&x.path, &y.path));
    assert_eq!(
        shared.count(),
        a.paint.len(),
        "a copy keeps every path by pointer"
    );
    assert_eq!(b.surface("m.a"), a.surface("m.a"));
}

/// `Resolver::resolve` keeps its last scene, so a reused memo is copied
/// out of it with no `prev` to thread by hand.
#[test]
fn resolver_resolve_reuses_memos() {
    let d = draws();
    let mut r = Resolver::default();
    r.resolve(&memo_spec(50., false, &d)).unwrap();
    r.resolve(&memo_spec(50., true, &d)).unwrap();
    assert_eq!(count(&d), 1, "the reused memo was walked again");
}

/// Moved by its neighbour, a reused memo is copied translated: paint
/// and surfaces land exactly where a walk puts them.
#[test]
fn a_moved_memo_translates_its_paint_and_surfaces() {
    let d = draws();
    let mut text = TextState::default();
    let a = retained(&memo_spec(50., false, &d), &mut text, None);
    let b = retained(&memo_spec(70., true, &d), &mut text, Some(&a));
    assert_eq!(count(&d), 1, "a moved memo was walked instead of copied");
    let walked = resolve(&memo_spec(70., false, &draws())).unwrap();
    assert_eq!(b.paint, walked.paint);
    for k in ["m", "m.a", "m.t", "m.c"] {
        assert_eq!(b.surface(k), walked.surface(k), "{k}");
    }
    assert_eq!(b.surface("m.a").unwrap().frame.x, 70.);
}

/// A memo scrolling under a viewport, with a clip of its own inside, cut
/// by the viewport on one frame and not the next.
fn scrolled_spec(by: f64, reused: bool, draws: &Arc<std::sync::atomic::AtomicUsize>) -> SceneSpec {
    let count = draws.clone();
    let strip = row([
        block(30., 20.).fill(Role::Primary).id("s.a"),
        canvas(move |_| {
            count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Vec::new()
        })
        .size(10., 10.)
        .id("s.c"),
    ])
    .w(60.)
    .radius(4.)
    .clip()
    .id("s.strip");
    let mut panel = col([block(40., 50.).fill(Role::Raised), strip, block(40., 80.)]).id("s");
    panel.payload_mut().extras_mut().memo = Some(crate::Memo { id: 9, reused });
    let view = col([panel])
        .h(60.)
        .radius(8.)
        .scroll()
        .scrolled(0., by)
        .id("view");
    SceneSpec::new(row([view]).align(Align::Start)).offered(Size::new(200., 100.))
}

/// Scrolling copies the memo under the viewport instead of walking it: its
/// own clip moves with it and the viewport's stays, as a walk would have it.
#[test]
fn a_memo_scrolled_under_a_viewport_is_copied_with_the_viewport_clip_kept() {
    let d = draws();
    let mut text = TextState::default();
    let mut prev = retained(&scrolled_spec(0., false, &d), &mut text, None);
    for by in [12., 40., 3.] {
        prev = retained(&scrolled_spec(by, true, &d), &mut text, Some(&prev));
        let walked = resolve(&scrolled_spec(by, false, &draws())).unwrap();
        assert_eq!(prev.paint, walked.paint, "paint at {by}");
        for k in ["s", "s.strip", "s.a", "s.c"] {
            assert_eq!(prev.surface(k), walked.surface(k), "{k} at {by}");
        }
    }
    assert_eq!(count(&d), 1, "a scrolled memo was walked instead of copied");
}

/// The caches a copied memo used stay warm while it is copied, and its
/// walk renews them once they are [`MEMO_AGE`] resolves old.
#[test]
fn a_copied_memo_keeps_its_cache_entries() {
    let d = draws();
    let mut text = TextState::default();
    let mut prev = retained(&memo_spec(50., false, &d), &mut text, None);
    for i in 0..MEMO_AGE + 3 {
        prev = retained(&memo_spec(50., true, &d), &mut text, Some(&prev));
        assert!(
            text.runs.contains_key("kept"),
            "the label's run was swept at {i}"
        );
        assert!(
            !text.outlines.rects.is_empty(),
            "the button's rect was swept at {i}"
        );
    }
    assert_eq!(count(&d), 2, "walked once when first built, once to renew");
}

#[test]
fn a_turn_rotates_paint_a_quarter_and_composes_with_rotation() {
    let spec = |e: El| SceneSpec::new(row([e.fill(Role::Primary).id("t")]).id("r"));
    let fill = |s: &ResolvedScene| {
        s.paint
            .iter()
            .find(|p| p.key.as_str() == "t" && p.layer == Layer::Fill)
            .unwrap()
            .transform
    };
    let turned = resolve(&spec(block(60., 12.).turn(Turn::Ccw))).unwrap();
    let f = turned.surface("t").unwrap().frame;
    assert_eq!((f.x, f.y, f.size), (-24., 24., Size::new(60., 12.)));
    let centre = Point::new(6., 30.);
    let want = mui_geometry::kurbo::Affine::rotate_about(-std::f64::consts::FRAC_PI_2, centre);
    assert_eq!(fill(&turned), want);
    // The painted outline covers exactly the 12 x 60 slot.
    let bounds = turned.surface("t").unwrap().bounds.unwrap();
    assert!(
        (bounds.x0 - 0.).abs() < 1e-9 && (bounds.y1 - 60.).abs() < 1e-9,
        "{bounds:?}"
    );
    // An explicit rotation adds to the turn: a Ccw quarter undone.
    let undone = resolve(&spec(
        block(60., 12.)
            .turn(Turn::Ccw)
            .rotation(std::f64::consts::FRAC_PI_2),
    ))
    .unwrap();
    let t = fill(&undone);
    assert!((t.as_coeffs()[0] - 1.).abs() < 1e-12 && t.as_coeffs()[1].abs() < 1e-12);
}
