# Pending geometry rules

Rules the geometry pass needs that the tool cannot express yet. `rules.rs`
reports each as a `Manual` note today; `rewrite.rs` was mid-edit by the
widget pass, so no new rule kind was added.

- Path move (new rule kind, e.g. `Moved { name, from, to }`): rewrite the
  crate root of `mui_geometry::{Spacing, SpacingScale, SpacingToken}` to
  `mui_layout`, in `use` trees (splitting a brace list) and inline paths.
- `Bounds::from_points(x)` -> `mui_geometry::bounds(x)`: an associated-fn
  call rewrite (`FnCall` only matches free functions).
- `Affine::translation(x, y)` -> `Affine::translate((x, y))`,
  `Affine::scale(x, y)` -> `Affine::scale_non_uniform(x, y)`,
  `Affine::rotation(a)` -> `Affine::rotate(a)`,
  `Affine::rotation_about(a, p)` -> `Affine::rotate_about(a, p)`,
  `a.then(b)` -> `b * a`: associated-fn renames plus one operand swap.
- `.min.x/.min.y/.max.x/.max.y` on a former `Bounds` -> `.x0/.y0/.x1/.y1`:
  needs the receiver's type, so it stays a report.
- `mui_weld::boundary::Plate::half` is a `Vec2`, and `Path::translate`,
  `Path::rigid_transform`, `RoundedRect::translated` take a `Vec2`: a
  `Point::new(..)` argument becomes `Vec2::new(..)`.
