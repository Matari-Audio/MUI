use super::*;
use crate::wrappers::appkit::{callback, new_class_name};
use objc2::__framework_prelude::{AnyClass, AnyObject, Bool, Sel};
use objc2::rc::Retained;
use objc2::runtime::ClassBuilder;
use objc2::{msg_send, sel, ClassType};
use objc2_app_kit::{NSEvent, NSView};
use objc2_foundation::{NSArray, NSAttributedString, NSRange, NSRect};
use objc2_quartz_core::CALayer;
use std::any::TypeId;
use std::ffi::c_void;
use std::sync::Mutex;

// AccessKit caches subclasses of our view class for the process lifetime.
// Share one base class per Rust implementation and image, without disposing its superclass.
static VIEW_CLASSES: Mutex<Vec<(TypeId, &'static AnyClass)>> = Mutex::new(Vec::new());

pub fn create_view_class<V: ViewImpl>() -> crate::platform::Result<&'static AnyClass> {
    let mut classes = VIEW_CLASSES.lock().unwrap_or_else(|e| e.into_inner());
    let view_type = TypeId::of::<V>();
    if let Some((_, class)) = classes.iter().find(|(kind, _)| *kind == view_type) {
        return Ok(class);
    }
    // UUID names also keep independent plugin images' Rust callbacks separate.
    let class_name = new_class_name("BaseviewNSView_").ok_or(
        crate::platform::PlatformError::CreationFailed("could not create view class UUID"),
    )?;

    let Some(mut class) = ClassBuilder::new(&class_name, NSView::class()) else {
        return Err(crate::platform::PlatformError::CreationFailed(
            "could not register NSView subclass",
        ));
    };

    // SAFETY: All of these function signatures are correct
    unsafe {
        class.add_method(sel!(acceptsFirstResponder), property_yes as extern "C" fn(_, _) -> _);
        class.add_method(
            sel!(becomeFirstResponder),
            become_first_responder::<V> as extern "C" fn(_, _) -> _,
        );
        class.add_method(
            sel!(resignFirstResponder),
            resign_first_responder::<V> as extern "C" fn(_, _) -> _,
        );
        class.add_method(sel!(makeBackingLayer), make_backing_layer as extern "C" fn(_, _) -> _);
        class.add_method(sel!(wantsUpdateLayer), property_yes as extern "C" fn(_, _) -> _);
        class.add_method(sel!(updateLayer), update_layer::<V> as extern "C" fn(_, _));
        class.add_method(sel!(setFrameSize:), set_frame_size::<V> as extern "C" fn(_, _, _));
        class.add_method(
            sel!(resizeWithOldSuperviewSize:),
            resize_with_old_superview_size::<V> as extern "C" fn(_, _, _),
        );
        class.add_method(sel!(viewDidHide), visibility_changed::<V> as extern "C" fn(_, _));
        class.add_method(sel!(viewDidUnhide), visibility_changed::<V> as extern "C" fn(_, _));
        class.add_method(sel!(isFlipped), property_yes as extern "C" fn(_, _) -> _);
        class.add_method(
            sel!(preservesContentInLiveResize),
            property_no as extern "C" fn(_, _) -> _,
        );
        class.add_method(
            sel!(acceptsFirstMouse:),
            accepts_first_mouse as extern "C" fn(_, _, _) -> _,
        );

        class.add_method(
            sel!(windowShouldClose:),
            window_should_close::<V> as extern "C" fn(_, _, _) -> _,
        );
        class.add_method(
            sel!(windowDidResize:),
            window_did_resize::<V> as extern "C" fn(_, _, _) -> _,
        );
        class.add_method(sel!(dealloc), dealloc::<V> as extern "C" fn(_, _));
        class.add_method(
            sel!(viewWillMoveToWindow:),
            view_will_move_to_window::<V> as extern "C" fn(_, _, _) -> _,
        );
        // MOOSE: a plugin view is often created before the host puts it in a
        // window, so its first backing scale is a guess. Re-read it on entry.
        class.add_method(
            sel!(viewDidMoveToWindow),
            view_did_move_to_window::<V> as extern "C" fn(_, _) -> _,
        );
        class.add_method(sel!(hitTest:), hit_test::<V> as extern "C" fn(_, _, _) -> _);
        class.add_method(
            sel!(updateTrackingAreas),
            update_tracking_areas::<V> as extern "C" fn(_, _) -> _,
        );

        class.add_method(sel!(mouseMoved:), mouse_moved::<V> as extern "C" fn(_, _, _) -> _);
        class.add_method(sel!(mouseDragged:), mouse_moved::<V> as extern "C" fn(_, _, _) -> _);
        class.add_method(sel!(rightMouseDragged:), mouse_moved::<V> as extern "C" fn(_, _, _) -> _);
        class.add_method(sel!(otherMouseDragged:), mouse_moved::<V> as extern "C" fn(_, _, _) -> _);

        class.add_method(sel!(scrollWheel:), scroll_wheel::<V> as extern "C" fn(_, _, _) -> _);

        class.add_method(
            sel!(viewDidChangeBackingProperties),
            view_did_change_backing_properties::<V> as extern "C" fn(_, _) -> _,
        );

        class.add_method(
            sel!(draggingEntered:),
            dragging_entered::<V> as extern "C" fn(_, _, _) -> _,
        );
        class.add_method(
            sel!(prepareForDragOperation:),
            prepare_for_drag_operation::<V> as extern "C" fn(_, _, _) -> _,
        );
        class.add_method(
            sel!(performDragOperation:),
            perform_drag_operation::<V> as extern "C" fn(_, _, _) -> _,
        );
        class.add_method(
            sel!(draggingUpdated:),
            dragging_updated::<V> as extern "C" fn(_, _, _) -> _,
        );
        class
            .add_method(sel!(draggingExited:), dragging_exited::<V> as extern "C" fn(_, _, _) -> _);
        class.add_method(
            sel!(handleNotification:),
            handle_notification::<V> as extern "C" fn(_, _, _) -> _,
        );

        class.add_method(sel!(mouseDown:), mouse_down::<V> as extern "C" fn(_, _, _));
        class.add_method(sel!(mouseUp:), mouse_up::<V> as extern "C" fn(_, _, _));
        class.add_method(sel!(rightMouseDown:), right_mouse_down::<V> as extern "C" fn(_, _, _));
        class.add_method(sel!(rightMouseUp:), right_mouse_up::<V> as extern "C" fn(_, _, _));
        class.add_method(sel!(otherMouseDown:), other_mouse_down::<V> as extern "C" fn(_, _, _));
        class.add_method(sel!(otherMouseUp:), other_mouse_up::<V> as extern "C" fn(_, _, _));

        class.add_method(sel!(mouseEntered:), mouse_entered::<V> as extern "C" fn(_, _, _));
        class.add_method(sel!(mouseExited:), mouse_exited::<V> as extern "C" fn(_, _, _));

        class.add_method(sel!(hasMarkedText), has_marked_text::<V> as extern "C" fn(_, _) -> _);
        class.add_method(sel!(markedRange), marked_range::<V> as extern "C" fn(_, _) -> _);
        class.add_method(sel!(selectedRange), selected_range::<V> as extern "C" fn(_, _) -> _);
        class.add_method(
            sel!(insertText:replacementRange:),
            insert_text::<V> as extern "C" fn(_, _, _, _),
        );
        class.add_method(
            sel!(setMarkedText:selectedRange:replacementRange:),
            set_marked_text::<V> as extern "C" fn(_, _, _, _, _),
        );
        class.add_method(sel!(unmarkText), unmark_text::<V> as extern "C" fn(_, _));
        class.add_method(
            sel!(firstRectForCharacterRange:actualRange:),
            first_rect::<V> as extern "C" fn(_, _, _, _) -> _,
        );
        class.add_method(
            sel!(attributedSubstringForProposedRange:actualRange:),
            attributed_substring::<V> as extern "C" fn(_, _, _, _) -> _,
        );
        class.add_method(
            sel!(validAttributesForMarkedText),
            valid_attributes as extern "C" fn(_, _) -> _,
        );
        class.add_method(
            sel!(characterIndexForPoint:),
            character_index as extern "C" fn(_, _, _) -> _,
        );
        class.add_method(sel!(doCommandBySelector:), do_command::<V> as extern "C" fn(_, _, _));
        class.add_method(sel!(keyDown:), key_down::<V> as extern "C" fn(_, _, _));
        class.add_method(sel!(keyUp:), key_up::<V> as extern "C" fn(_, _, _));
        class.add_method(sel!(flagsChanged:), flags_changed::<V> as extern "C" fn(_, _, _));

        class.add_method(sel!(cursorUpdate:), cursor_update::<V> as extern "C" fn(_, _, _));

        class.add_method(
            sel!(mooseDisplayLinkFired:),
            display_link_fired::<V> as extern "C" fn(_, _, _),
        );
    }

    if let Some(protocol) = objc2::runtime::AnyProtocol::get(c"NSTextInputClient") {
        class.add_protocol(protocol);
    }
    class.add_ivar::<*mut c_void>(BASEVIEW_STATE_IVAR);

    let class = class.register();
    classes.push((view_type, class));
    Ok(class)
}

pub extern "C" fn dealloc<V: ViewImpl>(this: &mut AnyObject, _sel: Sel) {
    callback("dealloc state", (), || {
        let class = this.class();
        View::<V>::free_inner(this, class);
    });
    // Always free AppKit's storage, even if a user destructor panicked.
    // Dispatch from NSView, not a dynamic accessibility/KVO subclass.
    callback("dealloc superclass", (), || unsafe {
        let () = msg_send![super(this, NSView::class()), dealloc];
    });
}

extern "C" fn display_link_fired<V: ViewImpl>(this: &View<V>, _: Sel, _link: &AnyObject) {
    callback("display_link_fired", (), || {
        let _keep_alive = this.retain();
        let Some(inner) = this.callback_inner() else { return };
        V::display_link_fired(inner);
    })
}

// Keep CAMetalLayer as the backing layer so wgpu's raw-window-metal helper
// adopts it directly, rather than installing an observer/sublayer. Runtime
// lookup avoids adding renderer-specific Cargo features to this window crate.
extern "C" fn make_backing_layer(this: &NSView, _: Sel) -> *mut CALayer {
    callback("makeBackingLayer", None, || {
        let _keep_alive = this.retain();
        let class = AnyClass::get(c"CAMetalLayer")?;
        let layer: Option<Retained<CALayer>> = unsafe { msg_send![class, new] };
        layer
    })
    .map_or(core::ptr::null_mut(), Retained::autorelease_ptr)
}

extern "C" fn update_layer<V: ViewImpl>(this: &View<V>, _: Sel) {
    callback("updateLayer", (), || {
        let _keep_alive = this.retain();
        if let Some(inner) = this.callback_inner() {
            V::update_layer(inner);
        }
    });
}

extern "C" fn visibility_changed<V: ViewImpl>(this: &View<V>, _: Sel) {
    callback("view visibility changed", (), || {
        let _keep_alive = this.retain();
        if let Some(inner) = this.callback_inner() {
            V::visibility_changed(inner);
        }
    });
}

extern "C" fn set_frame_size<V: ViewImpl>(this: &View<V>, _: Sel, size: objc2_foundation::NSSize) {
    callback("setFrameSize", (), || {
        let _keep_alive = this.retain();
        unsafe {
            let () = msg_send![super(this, NSView::class()), setFrameSize: size];
        }
        if let Some(inner) = this.callback_inner() {
            V::resized_by_appkit(inner);
        }
    });
}

extern "C" fn resize_with_old_superview_size<V: ViewImpl>(
    this: &View<V>, _: Sel, size: objc2_foundation::NSSize,
) {
    callback("resizeWithOldSuperviewSize", (), || {
        let _keep_alive = this.retain();
        unsafe {
            let () = msg_send![super(this, NSView::class()), resizeWithOldSuperviewSize: size];
        }
        if let Some(inner) = this.callback_inner() {
            V::resized_by_appkit(inner);
        }
    });
}

extern "C" fn property_yes(_this: &NSView, _sel: Sel) -> Bool {
    callback("property_yes", Bool::NO, || Bool::YES)
}

extern "C" fn property_no(_this: &NSView, _sel: Sel) -> Bool {
    callback("property_no", Bool::NO, || Bool::NO)
}

extern "C" fn accepts_first_mouse(_this: &NSView, _sel: Sel, _event: &NSEvent) -> Bool {
    callback("accepts_first_mouse", Bool::NO, || Bool::YES)
}

extern "C" fn become_first_responder<V: ViewImpl>(this: &View<V>, _sel: Sel) -> Bool {
    callback("become_first_responder", Bool::NO, || {
        let _keep_alive = this.retain();
        let Some(inner) = this.callback_inner() else { return false.into() };
        V::become_first_responder(inner).into()
    })
}

extern "C" fn resign_first_responder<V: ViewImpl>(this: &View<V>, _sel: Sel) -> Bool {
    callback("resign_first_responder", Bool::YES, || {
        let _keep_alive = this.retain();
        let Some(inner) = this.callback_inner() else { return true.into() };
        V::resign_first_responder(inner).into()
    })
}

extern "C" fn window_should_close<V: ViewImpl>(
    this: &View<V>, _: Sel, _sender: &AnyObject,
) -> Bool {
    callback("window_should_close", Bool::YES, || {
        let _keep_alive = this.retain();
        let Some(inner) = this.callback_inner() else { return true.into() };
        V::window_should_close(inner).into()
    })
}

extern "C" fn view_did_change_backing_properties<V: ViewImpl>(this: &View<V>, _: Sel) {
    callback("view_did_change_backing_properties", (), || {
        let _keep_alive = this.retain();
        let Some(inner) = this.callback_inner() else { return };
        V::view_did_change_backing_properties(inner, true);
    })
}

extern "C" fn hit_test<V: ViewImpl>(this: &View<V>, _sel: Sel, point: NSPoint) -> Option<&NSView> {
    callback("hit_test", None, || {
        let _keep_alive = this.retain();
        V::hit_test(this.callback_inner()?, point)
    })
}

extern "C" fn view_will_move_to_window<V: ViewImpl>(
    this: &View<V>, _self: Sel, new_window: Option<&NSWindow>,
) {
    callback("view_will_move_to_window", (), || {
        let _keep_alive = this.retain();
        let Some(inner) = this.callback_inner() else { return };
        V::view_will_move_to_window(inner, new_window);
    })
}

extern "C" fn view_did_move_to_window<V: ViewImpl>(this: &View<V>, _self: Sel) {
    callback("view_did_move_to_window", (), || {
        let _keep_alive = this.retain();
        if let Some(inner) = this.callback_inner() {
            V::view_did_move_to_window(inner);
        }
    })
}

extern "C" fn update_tracking_areas<V: ViewImpl>(this: &View<V>, _self: Sel) {
    callback("update_tracking_areas", (), || {
        let _keep_alive = this.retain();
        let Some(inner) = this.callback_inner() else { return };
        V::update_tracking_areas(inner);
    })
}

extern "C" fn mouse_moved<V: ViewImpl>(this: &View<V>, _sel: Sel, event: &NSEvent) {
    callback("mouse_moved", (), || {
        let _keep_alive = this.retain();
        let Some(inner) = this.callback_inner() else { return };
        V::mouse_moved(inner, event);
    })
}

extern "C" fn scroll_wheel<V: ViewImpl>(this: &View<V>, _: Sel, event: &NSEvent) {
    callback("scroll_wheel", (), || {
        let _keep_alive = this.retain();
        let Some(inner) = this.callback_inner() else { return };
        V::scroll_wheel(inner, event);
    })
}

extern "C" fn dragging_entered<V: ViewImpl>(
    this: &View<V>, _sel: Sel, sender: Option<&ProtocolObject<dyn NSDraggingInfo>>,
) -> NSDragOperation {
    callback("dragging_entered", NSDragOperation::None, || {
        let _keep_alive = this.retain();
        let Some(inner) = this.callback_inner() else { return NSDragOperation::None };
        V::dragging_entered(inner, sender)
    })
}

extern "C" fn dragging_updated<V: ViewImpl>(
    this: &View<V>, _sel: Sel, sender: Option<&ProtocolObject<dyn NSDraggingInfo>>,
) -> NSDragOperation {
    callback("dragging_updated", NSDragOperation::None, || {
        let _keep_alive = this.retain();
        let Some(inner) = this.callback_inner() else { return NSDragOperation::None };
        V::dragging_updated(inner, sender)
    })
}

extern "C" fn prepare_for_drag_operation<V: ViewImpl>(
    this: &View<V>, _sel: Sel, sender: Option<&ProtocolObject<dyn NSDraggingInfo>>,
) -> Bool {
    callback("prepare_for_drag_operation", Bool::NO, || {
        let _keep_alive = this.retain();
        let Some(inner) = this.callback_inner() else { return false.into() };
        V::prepare_for_drag_operation(inner, sender).into()
    })
}

extern "C" fn perform_drag_operation<V: ViewImpl>(
    this: &View<V>, _sel: Sel, sender: Option<&ProtocolObject<dyn NSDraggingInfo>>,
) -> Bool {
    callback("perform_drag_operation", Bool::NO, || {
        let _keep_alive = this.retain();
        let Some(inner) = this.callback_inner() else { return false.into() };
        V::perform_drag_operation(inner, sender).into()
    })
}

extern "C" fn dragging_exited<V: ViewImpl>(
    this: &View<V>, _sel: Sel, sender: Option<&ProtocolObject<dyn NSDraggingInfo>>,
) {
    callback("dragging_exited", (), || {
        let _keep_alive = this.retain();
        let Some(inner) = this.callback_inner() else { return };
        V::dragging_exited(inner, sender)
    })
}

extern "C" fn handle_notification<V: ViewImpl>(
    this: &View<V>, _cmd: Sel, notification: &NSNotification,
) {
    callback("handle_notification", (), || {
        let _keep_alive = this.retain();
        let Some(inner) = this.callback_inner() else { return };
        V::handle_notification(inner, notification)
    })
}

extern "C" fn mouse_entered<V: ViewImpl>(this: &View<V>, _: Sel, event: &NSEvent) {
    callback("mouse_entered", (), || {
        let _keep_alive = this.retain();
        // SAFETY: Our superclass is NSView
        let _: () = unsafe { msg_send![super(this, NSView::class()), mouseEntered: event] };

        let Some(inner) = this.callback_inner() else { return };
        V::mouse_entered(inner);
    })
}

extern "C" fn mouse_exited<V: ViewImpl>(this: &View<V>, _: Sel, event: &NSEvent) {
    callback("mouse_exited", (), || {
        let _keep_alive = this.retain();
        // SAFETY: Our superclass is NSView
        let _: () = unsafe { msg_send![super(this, NSView::class()), mouseExited: event] };

        let Some(inner) = this.callback_inner() else { return };
        V::mouse_exited(inner);
    })
}

extern "C" fn key_down<V: ViewImpl>(this: &View<V>, _: Sel, event: &NSEvent) {
    callback("key_down", (), || {
        let _keep_alive = this.retain();
        let Some(inner) = this.callback_inner() else { return };
        V::key_down(inner, event);
    })
}

extern "C" fn key_up<V: ViewImpl>(this: &View<V>, _: Sel, event: &NSEvent) {
    callback("key_up", (), || {
        let _keep_alive = this.retain();
        let Some(inner) = this.callback_inner() else { return };
        V::key_up(inner, event);
    })
}

extern "C" fn flags_changed<V: ViewImpl>(this: &View<V>, _: Sel, event: &NSEvent) {
    callback("flags_changed", (), || {
        let _keep_alive = this.retain();
        let Some(inner) = this.callback_inner() else { return };
        V::flags_changed(inner, event);
    })
}

extern "C" fn mouse_down<V: ViewImpl>(this: &View<V>, _sel: Sel, event: &NSEvent) {
    callback("mouse_down", (), || {
        let _keep_alive = this.retain();
        let Some(inner) = this.callback_inner() else { return };
        V::mouse_down(inner, event);
    })
}

extern "C" fn mouse_up<V: ViewImpl>(this: &View<V>, _sel: Sel, event: &NSEvent) {
    callback("mouse_up", (), || {
        let _keep_alive = this.retain();
        let Some(inner) = this.callback_inner() else { return };
        V::mouse_up(inner, event);
    })
}

extern "C" fn right_mouse_down<V: ViewImpl>(this: &View<V>, _sel: Sel, event: &NSEvent) {
    callback("right_mouse_down", (), || {
        let _keep_alive = this.retain();
        let Some(inner) = this.callback_inner() else { return };
        V::right_mouse_down(inner, event);
    })
}

extern "C" fn right_mouse_up<V: ViewImpl>(this: &View<V>, _sel: Sel, event: &NSEvent) {
    callback("right_mouse_up", (), || {
        let _keep_alive = this.retain();
        let Some(inner) = this.callback_inner() else { return };
        V::right_mouse_up(inner, event);
    })
}

extern "C" fn other_mouse_down<V: ViewImpl>(this: &View<V>, _sel: Sel, event: &NSEvent) {
    callback("other_mouse_down", (), || {
        let _keep_alive = this.retain();
        let Some(inner) = this.callback_inner() else { return };
        V::other_mouse_down(inner, event);
    })
}

extern "C" fn other_mouse_up<V: ViewImpl>(this: &View<V>, _sel: Sel, event: &NSEvent) {
    callback("other_mouse_up", (), || {
        let _keep_alive = this.retain();
        let Some(inner) = this.callback_inner() else { return };
        V::other_mouse_up(inner, event);
    })
}

extern "C" fn window_did_resize<V: ViewImpl>(
    this: &View<V>, _sel: Sel, _notification: &NSNotification,
) {
    callback("window_did_resize", (), || {
        let _keep_alive = this.retain();
        let Some(inner) = this.callback_inner() else { return };
        V::window_did_resize(inner);
    })
}

extern "C" fn cursor_update<V: ViewImpl>(this: &View<V>, _sel: Sel, event: Option<&NSEvent>) {
    callback("cursor_update", (), || {
        let _keep_alive = this.retain();
        if let Some(inner) = this.callback_inner() {
            if V::cursor_update(inner, event) {
                return;
            };
        }

        // SAFETY: Our superclass is NSView
        let _: () = unsafe { msg_send![super(this, NSView::class()), cursorUpdate: event] };
    })
}

extern "C" fn has_marked_text<V: ViewImpl>(this: &View<V>, _: Sel) -> Bool {
    callback("has_marked_text", Bool::NO, || {
        let _keep_alive = this.retain();
        Bool::new(this.callback_inner().is_some_and(V::has_marked_text))
    })
}
extern "C" fn marked_range<V: ViewImpl>(this: &View<V>, _: Sel) -> NSRange {
    callback("marked_range", NSRange::new(usize::MAX, 0), || {
        let _keep_alive = this.retain();
        this.callback_inner().map_or(NSRange::new(usize::MAX, 0), V::marked_range)
    })
}
extern "C" fn selected_range<V: ViewImpl>(this: &View<V>, _: Sel) -> NSRange {
    callback("selected_range", NSRange::new(usize::MAX, 0), || {
        let _keep_alive = this.retain();
        this.callback_inner().map_or(NSRange::new(usize::MAX, 0), V::selected_range)
    })
}
extern "C" fn insert_text<V: ViewImpl>(
    this: &View<V>, _: Sel, text: &AnyObject, replacement: NSRange,
) {
    callback("insert_text", (), || {
        let _keep_alive = this.retain();
        if let Some(inner) = this.callback_inner() {
            V::insert_text(inner, text, replacement);
        }
    })
}
extern "C" fn set_marked_text<V: ViewImpl>(
    this: &View<V>, _: Sel, text: &AnyObject, selected: NSRange, replacement: NSRange,
) {
    callback("set_marked_text", (), || {
        let _keep_alive = this.retain();
        if let Some(inner) = this.callback_inner() {
            V::set_marked_text(inner, text, selected, replacement);
        }
    })
}
extern "C" fn unmark_text<V: ViewImpl>(this: &View<V>, _: Sel) {
    callback("unmark_text", (), || {
        let _keep_alive = this.retain();
        if let Some(inner) = this.callback_inner() {
            V::unmark_text(inner);
        }
    })
}
extern "C" fn first_rect<V: ViewImpl>(
    this: &View<V>, _: Sel, range: NSRange, actual: *mut NSRange,
) -> NSRect {
    callback("first_rect", NSRect::ZERO, || {
        let _keep_alive = this.retain();
        this.callback_inner().map_or(NSRect::ZERO, |inner| V::first_rect(inner, range, actual))
    })
}
extern "C" fn attributed_substring<V: ViewImpl>(
    this: &View<V>, _: Sel, range: NSRange, actual: *mut NSRange,
) -> *mut NSAttributedString {
    // Autorelease in the caller's pool, not our shorter callback pool.
    callback("attributedSubstring", None, || {
        let _keep_alive = this.retain();
        this.callback_inner().and_then(|inner| V::attributed_substring(inner, range, actual))
    })
    .map_or(core::ptr::null_mut(), Retained::autorelease_ptr)
}
extern "C" fn valid_attributes(_: &AnyObject, _: Sel) -> *mut NSArray<AnyObject> {
    callback("validAttributes", None, || Some(NSArray::new()))
        .map_or(core::ptr::null_mut(), Retained::autorelease_ptr)
}
extern "C" fn character_index(_: &AnyObject, _: Sel, _: objc2_foundation::NSPoint) -> usize {
    callback("character_index", usize::MAX, || usize::MAX)
}
extern "C" fn do_command<V: ViewImpl>(this: &View<V>, _: Sel, selector: Sel) {
    callback("do_command", (), || {
        let _keep_alive = this.retain();
        if let Some(inner) = this.callback_inner() {
            V::do_command(inner, selector);
        }
    })
}
