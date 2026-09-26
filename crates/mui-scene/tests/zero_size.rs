//! A node that has shrunk to nothing paints nothing; it does not fail the
//! frame. A level meter at silence is `block(200. * level, 8.).pill()`.
use mui_scene::prelude::*;

#[test]
fn a_zero_sized_filled_node_resolves() {
    for el in [
        block(0., 8.).pill().fill(Role::Primary),
        block(0., 0.).fill(Role::Primary).shadow(Shadow::soft(8.)),
        block(20., 0.)
            .radius(4.)
            .fill(Role::Primary)
            .stroke(Role::Primary)
            .stroke_width(1.),
        block(0., 8.)
            .pill()
            .fill(Role::Primary)
            .shell(4., Role::Raised),
    ] {
        let s = resolve(&SceneSpec::new(row([el.id("m"), block(10., 10.)])));
        assert!(s.is_ok(), "{:?}", s.err());
    }
}
