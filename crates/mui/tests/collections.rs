use mui::prelude::*;

fn options(height: f64) -> ListOptions {
    ListOptions {
        overscan: 0.0,
        ..ListOptions::new(100.0, height)
    }
}
fn resolve(ui: &mut Ui, el: El) {
    ui.frame(el, None, Input::default(), 0.016).unwrap();
}

#[test]
fn million_uniform_rows_construct_only_viewport_and_overscan() {
    let mut ui = Ui::default();
    let mut list = ListState::uniform_count(1_000_000, 20.0);
    let mut built = 0;
    let opts = ListOptions {
        overscan: 40.0,
        ..options(200.0)
    };
    let tree = uniform_list(&mut ui, "rows", &mut list, opts, |_, row| {
        built += 1;
        assert!(row.index < 12);
        block(100.0, 20.0)
    });
    assert_eq!(built, 12);
    assert_eq!(tree.changed.rows_built, built);
    assert_eq!(tree.changed.visible, 0..10);
    resolve(&mut ui, tree.el);
    let surface = ui.scene().unwrap().surface("rows").unwrap();
    assert_eq!(surface.content.height, 20_000_000.0);
    assert!(
        ui.scene().unwrap().surfaces().count() < 50,
        "retained scene contains only built rows"
    );
    list.scroll_to_item(500_000, ScrollTo::Start);
    built = 0;
    let tree = uniform_list(&mut ui, "rows", &mut list, opts, |_, row| {
        built += 1;
        assert!((499_998..500_012).contains(&row.index));
        block(100.0, 20.0)
    });
    assert_eq!(built, 14);
    assert_eq!(tree.changed.visible, 500_000..500_010);
    resolve(&mut ui, tree.el);
    assert_eq!(ui.scroll("rows"), [0.0, 10_000_000.0]);
    let row = ui.scene().unwrap().surface("rows/item/500000").unwrap();
    assert_eq!(row.frame.y, 0.0, "constructed and drawn offsets agree");
}

#[test]
fn variable_measurements_fill_viewport_and_index_updates_preserve_anchor() {
    let mut ui = Ui::default();
    let mut list = ListState::variable(0..100_000, 100.0);
    let mut built = 0;
    let tree = variable_list(&mut ui, "rows", &mut list, options(200.0), |_, row| {
        built += 1;
        let height = if row.index % 2 == 0 { 10.0 } else { 30.0 };
        ListRow::new(block(100.0, height), height)
    });
    assert_eq!(
        built, 10,
        "actual heights rather than estimates fill the viewport"
    );
    assert_eq!(tree.changed.visible, 0..10);
    resolve(&mut ui, tree.el);
    list.scroll_to_item(10, ScrollTo::Start);
    let tree = variable_list(&mut ui, "rows", &mut list, options(200.0), |_, row| {
        ListRow::new(block(100.0, row.height), row.height)
    });
    resolve(&mut ui, tree.el);
    assert_eq!(list.scroll_offset(), 200.0);
    assert!(list.set_height(0, 50.0));
    assert_eq!(list.scroll_offset(), 240.0);
    let tree = variable_list(&mut ui, "rows", &mut list, options(200.0), |_, row| {
        ListRow::new(block(100.0, row.height), row.height)
    });
    resolve(&mut ui, tree.el);
    assert_eq!(
        ui.scene().unwrap().surface("rows/item/10").unwrap().frame.y,
        0.0
    );
    assert!(!list.set_height(100_000, 2.0));
    assert!(!list.set_height(0, f64::NAN));
    assert!(!list.set_height(0, f64::INFINITY));
    assert!(!list.set_height(0, -1.0));
    assert!(!list.set_height(0, 0.0));
}

#[test]
fn edits_and_reorders_keep_the_same_key_and_pixel_offset() {
    let mut ui = Ui::default();
    let mut list = ListState::uniform(10..20, 20.0);
    let tree = uniform_list(&mut ui, "rows", &mut list, options(40.0), |_, _| {
        block(100.0, 20.0)
    });
    resolve(&mut ui, tree.el);
    ui.set_scroll("rows", [0.0, 65.0]);
    let tree = uniform_list(&mut ui, "rows", &mut list, options(40.0), |_, _| {
        block(100.0, 20.0)
    });
    resolve(&mut ui, tree.el);
    list.splice(0..0, [90, 91]);
    assert_eq!(list.scroll_offset(), 105.0);
    list.set_items([19, 18, 17, 16, 15, 14, 13, 12, 11, 10, 90, 91]);
    assert_eq!(list.scroll_offset(), 125.0);
    let tree = uniform_list(&mut ui, "rows", &mut list, options(40.0), |_, _| {
        block(100.0, 20.0)
    });
    assert_eq!(tree.changed.visible.start, 6);
    resolve(&mut ui, tree.el);
    assert_eq!(
        ui.scene().unwrap().surface("rows/item/13").unwrap().frame.y,
        -5.0
    );
    list.splice(6..7, []);
    assert_eq!(list.key(6), Some(12));
    assert_eq!(list.scroll_offset(), 125.0);
}

#[test]
fn native_wheel_is_consumed_once_and_offscreen_focus_yields_to_list() {
    let mut ui = Ui::default();
    let mut list = ListState::uniform_count(100, 20.0);
    let tree = uniform_list(&mut ui, "rows", &mut list, options(40.0), |_, row| {
        block(100.0, 20.0).id(row.id.field("child")).focusable()
    });
    resolve(&mut ui, tree.el);
    ui.focus("rows/item/0/child");
    let tree = uniform_list(&mut ui, "rows", &mut list, options(40.0), |_, _| {
        block(100.0, 20.0)
    });
    ui.frame(
        tree.el,
        None,
        Input {
            pointer: PointerInput {
                pos: Some(Point::new(10.0, 10.0)),
                ..PointerInput::default()
            },
            wheel: Vec2::new(0.0, 80.0),
            ..Input::default()
        },
        0.016,
    )
    .unwrap();
    assert_eq!(ui.scroll("rows"), [0.0, 80.0]);
    let tree = uniform_list(&mut ui, "rows", &mut list, options(40.0), |_, _| {
        block(100.0, 20.0)
    });
    assert_eq!(tree.changed.visible, 4..6);
    resolve(&mut ui, tree.el);
    assert_eq!(ui.scroll("rows"), [0.0, 80.0]);
    // A genuinely retained focused descendant is transferred, not silently lost.
    list.focus_item(0);
    let tree = uniform_list(&mut ui, "rows", &mut list, options(40.0), |_, row| {
        block(100.0, 20.0).id(row.id.field("child")).focusable()
    });
    resolve(&mut ui, tree.el);
    ui.focus("rows/item/0/child");
    ui.set_scroll("rows", [0.0, 80.0]);
    let tree = uniform_list(&mut ui, "rows", &mut list, options(40.0), |_, _| {
        block(100.0, 20.0)
    });
    assert_eq!(tree.changed.active, Some(0));
    assert!(ui.focused("rows"));
    resolve(&mut ui, tree.el);
}

#[test]
fn keyboard_can_focus_and_activate_a_previously_unbuilt_row() {
    let mut ui = Ui::default();
    let mut list = ListState::uniform_count(100_000, 20.0);
    let tree = uniform_list(&mut ui, "rows", &mut list, options(40.0), |_, _| {
        block(100.0, 20.0)
    });
    resolve(&mut ui, tree.el);
    ui.focus("rows");
    let tree = uniform_list(&mut ui, "rows", &mut list, options(40.0), |_, _| {
        block(100.0, 20.0)
    });
    ui.frame(
        tree.el,
        None,
        Input {
            keys: vec![KeyPress {
                key: Key::End,
                mods: Mods::default(),
            }],
            ..Input::default()
        },
        0.016,
    )
    .unwrap();
    let tree = uniform_list(&mut ui, "rows", &mut list, options(40.0), |_, _| {
        block(100.0, 20.0)
    });
    assert_eq!(tree.changed.visible, 99_998..100_000);
    assert_eq!(tree.changed.rows_built, 2);
    assert!(ui.focused("rows/item/99999"));
    ui.frame(
        tree.el,
        None,
        Input {
            keys: vec![KeyPress {
                key: Key::Enter,
                mods: Mods::default(),
            }],
            ..Input::default()
        },
        0.016,
    )
    .unwrap();
    let tree = uniform_list(&mut ui, "rows", &mut list, options(40.0), |_, _| {
        block(100.0, 20.0)
    });
    assert_eq!(tree.changed.activated, Some(99_999));
}

#[test]
fn shrinking_tail_constructs_newly_exposed_rows_in_the_same_build() {
    let mut ui = Ui::default();
    let mut list = ListState::variable(0..100, 100.0);
    list.scroll_to_item(99, ScrollTo::End);
    let tree = variable_list(&mut ui, "rows", &mut list, options(200.0), |_, _| {
        ListRow::new(block(100.0, 1.0), 1.0)
    });
    assert_eq!(tree.changed.visible, 0..100);
    assert_eq!(tree.changed.rendered, 0..100);
    assert_eq!(tree.changed.rows_built, 100);
    resolve(&mut ui, tree.el);
    assert_eq!(list.scroll_offset(), 0.0);
    assert_eq!(
        ui.scene().unwrap().surface("rows").unwrap().content.height,
        100.0
    );
}

#[test]
fn scroll_alignment_empty_lists_and_invalid_offsets() {
    let mut ui = Ui::default();
    let mut list = ListState::uniform_count(20, 20.0);
    for (align, expected) in [
        (ScrollTo::Start, 200.0),
        (ScrollTo::Center, 170.0),
        (ScrollTo::End, 140.0),
    ] {
        list.scroll_to_item(10, align);
        let tree = uniform_list(&mut ui, "rows", &mut list, options(80.0), |_, _| {
            block(100.0, 20.0)
        });
        assert_eq!(list.scroll_offset(), expected);
        resolve(&mut ui, tree.el);
    }
    assert!(!ui.set_scroll("rows", [f64::NAN, 0.0]));
    assert_eq!(ui.scroll("rows"), [0.0, 140.0]);
    assert!(ui.set_scroll("rows", [-10.0, -10.0]));
    assert_eq!(ui.scroll("rows"), [0.0, 0.0]);
    list.set_items([]);
    let tree = uniform_list(&mut ui, "rows", &mut list, options(80.0), |_, _| {
        panic!("empty collection built a row")
    });
    assert_eq!(tree.changed.rows_built, 0);
    assert!(!list.scroll_to_item(0, ScrollTo::Start));
    resolve(&mut ui, tree.el);
}

#[test]
#[should_panic(expected = "duplicate list key")]
fn duplicate_keys_are_rejected() {
    let _ = ListState::uniform([1, 1], 20.0);
}

#[test]
fn tab_focus_in_overscan_reveals_the_row_and_keeps_child_focus() {
    let mut ui = Ui::default();
    let mut list = ListState::uniform_count(100, 20.0);
    let opts = ListOptions {
        overscan: 40.0,
        ..options(40.0)
    };
    let tree = uniform_list(&mut ui, "rows", &mut list, opts, |_, row| {
        block(100.0, 20.0).id(row.id.field("child")).focusable()
    });
    resolve(&mut ui, tree.el);
    ui.focus("rows/item/3/child");
    let tree = uniform_list(&mut ui, "rows", &mut list, opts, |_, row| {
        block(100.0, 20.0).id(row.id.field("child")).focusable()
    });
    assert_eq!(list.scroll_offset(), 40.0);
    assert!(ui.focused("rows/item/3/child"));
    resolve(&mut ui, tree.el);
}

#[test]
fn a_model_edit_keeps_wheel_input_that_landed_after_the_last_build() {
    let mut ui = Ui::default();
    let mut list = ListState::uniform(0..100, 20.0);
    let tree = uniform_list(&mut ui, "rows", &mut list, options(40.0), |_, _| {
        block(100.0, 20.0)
    });
    ui.frame(
        tree.el,
        None,
        Input {
            pointer: PointerInput {
                pos: Some(Point::new(10.0, 10.0)),
                ..PointerInput::default()
            },
            wheel: Vec2::new(0.0, 80.0),
            ..Input::default()
        },
        0.016,
    )
    .unwrap();
    list.splice(0..0, [100, 101]);
    let tree = uniform_list(&mut ui, "rows", &mut list, options(40.0), |_, _| {
        block(100.0, 20.0)
    });
    assert_eq!(list.scroll_offset(), 120.0);
    assert_eq!(list.key(tree.changed.visible.start), Some(4));
    resolve(&mut ui, tree.el);
}

#[test]
fn a_large_collection_does_not_relax_the_next_ordinary_frames_extent_guard() {
    let mut ui = Ui::default();
    let mut list = ListState::uniform_count(100_000, 20.0);
    let tree = uniform_list(&mut ui, "rows", &mut list, options(40.0), |_, _| {
        block(100.0, 20.0)
    });
    resolve(&mut ui, tree.el);
    assert!(
        ui.frame(block(100.0, 2_000_000.0), None, Input::default(), 0.016)
            .is_err()
    );
}

#[test]
#[should_panic(expected = "invalid list height")]
fn overflowing_total_height_is_rejected() {
    let _ = ListState::uniform_count(2, f64::MAX);
}

#[test]
fn a_million_row_list_keeps_its_extent_when_the_subtree_is_memoized() {
    let mut ui = Ui::default();
    let mut list = ListState::uniform_count(1_000_000, 20.0);
    let mut calls = 0;
    for _ in 0..3 {
        let tree = ui.memo("cached", (), |ui| {
            uniform_list(ui, "rows", &mut list, options(40.0), |_, _| {
                calls += 1;
                block(100.0, 20.0)
            })
            .el
        });
        resolve(&mut ui, tree);
        assert_eq!(
            ui.scene().unwrap().surface("rows").unwrap().content.height,
            20_000_000.0
        );
    }
    assert_eq!(calls, 2, "memo reuse does not rerun row constructors");
}

#[test]
fn invalid_extent_declarations_are_rejected() {
    for extent in [-1.0, 0.0, f64::NAN, f64::INFINITY, f64::MAX] {
        let mut ui = Ui::default();
        let mut tree = col([block(100.0, 20.0)]).size(100.0, 40.0).scroll();
        tree.payload_mut().extras_mut().virtual_scroll_extent = Some(extent);
        assert!(ui.frame(tree, None, Input::default(), 0.016).is_err());
    }
    let mut ui = Ui::default();
    let mut tree = block(100.0, 2_000_000.0);
    tree.payload_mut().extras_mut().virtual_scroll_extent = Some(2_000_000.0);
    assert!(
        ui.frame(tree, None, Input::default(), 0.016).is_err(),
        "ordinary nodes cannot declare a virtual extent"
    );
}

#[test]
fn variable_height_overflow_is_rejected_without_changing_the_index() {
    let mut list = ListState::variable([10, 20], 1.0);
    let huge = f64::MAX * 0.75;
    assert!(list.set_height(0, huge));
    assert!(!list.set_height(1, huge));
    assert_eq!(list.height(1), Some(1.0));
    assert!(list.total_height().is_finite());
}

#[test]
fn tree_budgets_are_checked_before_ui_recursion_and_keep_the_previous_scene() {
    let mut ui = Ui::default();
    resolve(&mut ui, block(10.0, 10.0).id("previous"));
    let mut deep = block(1.0, 1.0);
    for _ in 0..10_000 {
        deep = col([deep]);
    }
    assert!(matches!(
        ui.frame(deep, None, Input::default(), 0.016),
        Err(mui_scene::SceneError::Layout(
            mui_layout::Error::BudgetExceeded
        ))
    ));
    assert!(ui.scene().unwrap().surface("previous").is_some());
    let wide = col((0..4096).map(|_| block(1.0, 1.0)));
    assert!(matches!(
        ui.frame(wide, None, Input::default(), 0.016),
        Err(mui_scene::SceneError::Layout(
            mui_layout::Error::BudgetExceeded
        ))
    ));
    assert!(ui.scene().unwrap().surface("previous").is_some());
}

#[test]
fn retained_memos_cannot_expand_beyond_the_frame_depth_budget() {
    let mut ui = Ui::default();
    let mut calls = 0;
    for frame in 0..3 {
        let mut tree = ui.memo("deep", (), |_| {
            calls += 1;
            let mut tree = block(1.0, 1.0);
            for _ in 0..40 {
                tree = col([tree]);
            }
            tree
        });
        if frame == 2 {
            for _ in 0..30 {
                tree = col([tree]);
            }
            assert!(matches!(
                ui.frame(tree, None, Input::default(), 0.016),
                Err(mui_scene::SceneError::Layout(
                    mui_layout::Error::BudgetExceeded
                ))
            ));
        } else {
            resolve(&mut ui, tree);
        }
    }
    assert_eq!(calls, 1);
}

#[test]
fn virtual_content_allowance_does_not_expand_sibling_or_viewport_dimensions() {
    for viewport in [false, true] {
        let mut ui = Ui::default();
        let mut state = ListState::uniform_count(1_000_000, 20.0);
        let mut list = uniform_list(&mut ui, "rows", &mut state, options(40.0), |_, _| {
            block(100.0, 20.0)
        })
        .el;
        let tree = if viewport {
            list = list.h(2_000_000.0);
            list
        } else {
            row([list, block(2_000_000.0, 20.0)])
        };
        assert!(ui.frame(tree, None, Input::default(), 0.016).is_err());
    }
}

#[test]
fn retained_memos_cannot_expand_beyond_the_frame_node_budget() {
    let mut ui = Ui::default();
    for frame in 0..3 {
        let tree = ui.memo("wide", (), |_| col((0..3000).map(|_| block(1.0, 1.0))));
        if frame == 2 {
            let tree = col(std::iter::once(tree).chain((0..1100).map(|_| block(1.0, 1.0))));
            assert!(matches!(
                ui.frame(tree, None, Input::default(), 0.016),
                Err(mui_scene::SceneError::Layout(
                    mui_layout::Error::BudgetExceeded
                ))
            ));
        } else {
            resolve(&mut ui, tree);
        }
    }
}

#[test]
fn virtual_content_allowance_keeps_ordinary_intrinsic_and_scroll_aggregate_limits() {
    for intrinsic in [false, true] {
        let mut ui = Ui::default().font(Font::new(epaint_default_fonts::HACK_REGULAR).unwrap());
        let mut state = ListState::uniform_count(1_000_000, 20.0);
        let list = uniform_list(&mut ui, "rows", &mut state, options(40.0), |_, _| {
            block(100.0, 20.0)
        })
        .el;
        let sibling = if intrinsic {
            text("M").text_size(2_000_000.0).size(10.0, 10.0)
        } else {
            col([block(20.0, 600_000.0), block(20.0, 600_000.0)])
                .size(20.0, 40.0)
                .scroll()
        };
        let tree = row([list, sibling]);
        assert!(ui.frame(tree, None, Input::default(), 0.016).is_err());
    }
}

#[test]
fn invalid_delta_also_discards_deep_owned_input_without_recursive_drop() {
    let mut ui = Ui::default();
    let mut tree = block(1.0, 1.0);
    for _ in 0..10_000 {
        tree = col([tree]);
    }
    assert!(matches!(
        ui.frame(tree, None, Input::default(), f64::NAN),
        Err(mui_scene::SceneError::InvalidFrameDelta)
    ));
}

#[test]
fn motion_frames_cannot_exceed_scoped_dimensions_before_paint() {
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let painted = calls.clone();
    let mut root = canvas(move |_| {
        painted.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Vec::new()
    })
    .size(10.0, 10.0)
    .id("moving")
    .animate_layout();
    root.set_layout_extent_limit(100.0);
    let spec = mui_scene::SceneSpec::new(root);
    let mut resolver = mui_scene::Resolver::default();
    assert!(matches!(
        resolver.resolve_after(
            &spec,
            &mut |_, _, mut frame| {
                frame.size.height = 200.0;
                frame
            },
            None
        ),
        Err(mui_scene::SceneError::Layout(
            mui_layout::Error::BudgetExceeded
        ))
    ));
    assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed), 0);
}
