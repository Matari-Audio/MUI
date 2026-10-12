use objc2_app_kit::NSView;
use objc2_foundation::NSPoint;

/// NSDraggingInfo supplies destination-window coordinates, including for embedded views.
pub(super) fn window_point_to_view(view: &NSView, point: NSPoint) -> NSPoint {
    view.convertPoint_fromView(point, None)
}
