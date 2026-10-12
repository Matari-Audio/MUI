#![allow(
    clippy::expect_used,
    reason = "native fixture setup must fail if AppKit allocation is unavailable"
)]

//! A harness-free executable keeps AppKit allocation on the process main thread.
#[cfg(target_os = "macos")]
#[path = "../src/platform/macos/drag_geometry.rs"]
mod drag_geometry;

#[cfg(target_os = "macos")]
fn main() {
    use objc2::rc::{autoreleasepool, Retained};
    use objc2::{define_class, msg_send, MainThreadMarker, MainThreadOnly};
    use objc2_app_kit::{NSApplication, NSBackingStoreType, NSView, NSWindow, NSWindowStyleMask};
    use objc2_foundation::{NSPoint, NSRect, NSSize};

    define_class!(
        // SAFETY: NSView permits subclassing; this subclass has no ivars or custom Drop.
        #[unsafe(super = NSView)]
        #[thread_kind = MainThreadOnly]
        struct FlippedDragView;
        impl FlippedDragView {
            // SAFETY: this matches NSView's BOOL-returning isFlipped selector.
            #[unsafe(method(isFlipped))]
            fn is_flipped(&self) -> bool { true }
        }
    );

    autoreleasepool(|_| {
        let mtm = MainThreadMarker::new().expect("fixture must run on the native main thread");
        let _ = NSApplication::sharedApplication(mtm);
        // SAFETY: main-thread allocation; automatic close release is disabled below.
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                NSRect::new(NSPoint::ZERO, NSSize::new(300.0, 240.0)),
                NSWindowStyleMask::Borderless,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        // SAFETY: false prevents a second release when the retained window closes.
        unsafe {
            window.setReleasedWhenClosed(false);
        }
        let content = window.contentView().expect("window has a content view");
        let rect = NSRect::new(NSPoint::new(40.0, 60.0), NSSize::new(100.0, 80.0));
        let plain = NSView::initWithFrame(NSView::alloc(mtm), rect);
        // SAFETY: initWithFrame is inherited from NSView with the same signature.
        let flipped: Retained<FlippedDragView> =
            unsafe { msg_send![FlippedDragView::alloc(mtm), initWithFrame: rect] };
        content.addSubview(&plain);
        content.addSubview(&flipped);
        let incoming = NSPoint::new(55.0, 75.0);
        let plain_point = drag_geometry::window_point_to_view(&plain, incoming);
        let flipped_point = drag_geometry::window_point_to_view(&flipped, incoming);
        assert_eq!(plain_point, NSPoint::new(15.0, 15.0));
        assert_eq!(flipped_point, NSPoint::new(15.0, 65.0));
        // The native conversion is logical; the recipient scales it only afterward.
        for scale in [1.0, 2.0] {
            assert_eq!(flipped_point.x * scale, 15.0 * scale);
            assert_eq!(flipped_point.y * scale, 65.0 * scale);
        }
        window.close();
    });
}

#[cfg(not(target_os = "macos"))]
fn main() {}
