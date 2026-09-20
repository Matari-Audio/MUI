use mui_layout::*;
#[test]
fn reframe_validates_tree_and_updates_named_frames() {
    let root = row([leaf(10., 10.).id("a"), leaf(10., 10.).id("b")]);
    let layout = resolve(&root, None, Default::default()).unwrap();
    let mut frames = layout.all().to_vec();
    frames[1].x = 3.;
    let updated = layout.clone().reframe(&root, frames.clone()).unwrap();
    assert_eq!(updated.frame("a"), Some(frames[1]));
    assert!(layout
        .clone()
        .reframe(&leaf(0., 0.), frames.clone())
        .is_err());
    assert!(layout
        .clone()
        .reframe(&row([root.clone(), leaf(0., 0.)]), frames.clone())
        .is_err());
    let duplicate = row([leaf(10., 10.).id("a"), leaf(10., 10.).id("a")]);
    assert!(layout.clone().reframe(&duplicate, frames.clone()).is_err());
    frames[1].x = f64::NAN;
    assert!(layout.reframe(&root, frames).is_err());
}
