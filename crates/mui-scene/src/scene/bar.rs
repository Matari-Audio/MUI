//! The overlay scrollbar a `.scroll()` node paints over its own edge while
//! it overflows. The walk paints the thumb and publishes the strip it sits in
//! as a surface; the runtime drags that surface. Both read the thumb off the
//! same two functions here, so the bar the pointer grabs is the bar that was
//! painted.

/// Width of the strip the bar owns along the viewport's far edge: the
/// pointer target, wider than the resting thumb so a thin bar is still easy
/// to catch.
pub const BAR_STRIP: f64 = 10.0;
/// The gap between the thumb and the viewport's edges.
pub(super) const BAR_MARGIN: f64 = 2.0;
/// Thumb thickness at rest and under the pointer.
pub(super) const BAR_THIN: f64 = 3.0;
pub(super) const BAR_WIDE: f64 = 6.0;
/// The shortest a thumb gets over a very long list, so it stays grabbable.
const BAR_MIN: f64 = 18.0;

/// The bar's surface key for scroll node `key` on one axis. Runtime-owned
/// (it starts with `/`), so mui-access skips it and no id can collide with it.
pub fn bar_key(key: &str, vertical: bool) -> String {
    format!("/bar:{}:{key}", if vertical { 'y' } else { 'x' })
}

/// The scroll node's key and axis a [`bar_key`] names.
pub fn bar_of(bar: &str) -> Option<(&str, bool)> {
    let rest = bar.strip_prefix("/bar:")?;
    match rest.split_at_checked(2)? {
        ("y:", key) => Some((key, true)),
        ("x:", key) => Some((key, false)),
        _ => None,
    }
}

/// The thumb in a track `len` long from `start`, for a viewport `view` long
/// over `content`, slid `offset`: where it starts and how long it is. `None`
/// when nothing overflows.
pub fn thumb(start: f64, len: f64, view: f64, content: f64, offset: f64) -> Option<(f64, f64)> {
    let (start, len) = (start + BAR_MARGIN, len - 2.0 * BAR_MARGIN);
    // Half a pixel of slack: content measured off the children's frames can
    // land a rounding error past the viewport, and that is not a list.
    if content - view <= 0.5 || len <= 0.0 {
        return None;
    }
    let size = (len * view / content).max(BAR_MIN).min(len);
    let travel = (len - size) * (offset / (content - view)).clamp(0.0, 1.0);
    Some((start + travel, size))
}

/// The offset that puts the thumb of [`thumb`] at `at`, clamped to the
/// content.
pub fn thumb_offset(start: f64, len: f64, view: f64, content: f64, at: f64) -> f64 {
    let Some((first, size)) = thumb(start, len, view, content, 0.0) else {
        return 0.0;
    };
    let travel = len - 2.0 * BAR_MARGIN - size;
    if travel <= 0.0 {
        return 0.0;
    }
    ((at - first) / travel).clamp(0.0, 1.0) * (content - view)
}
