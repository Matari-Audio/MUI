use super::callback;
use block2::RcBlock;
use objc2::rc::{Retained, Weak};
use objc2::runtime::{NSObjectProtocol, ProtocolObject};
use objc2_app_kit::{
    NSWindow, NSWindowDidBecomeKeyNotification, NSWindowDidChangeBackingPropertiesNotification,
    NSWindowDidChangeOcclusionStateNotification, NSWindowDidChangeScreenNotification,
    NSWindowDidDeminiaturizeNotification, NSWindowDidMiniaturizeNotification,
    NSWindowDidResignKeyNotification, NSWindowWillCloseNotification,
};
use objc2_foundation::{NSNotification, NSNotificationCenter};
use std::ptr::NonNull;

pub struct NotificationCenterObserver {
    notification_center: Weak<NSNotificationCenter>,
    handlers: Vec<Retained<ProtocolObject<dyn NSObjectProtocol>>>,
}

impl NotificationCenterObserver {
    pub fn register_window_changes(
        window: &NSWindow, handler: impl Fn(&NSNotification) + 'static,
    ) -> Self {
        let notification_center = NSNotificationCenter::defaultCenter();
        let block = RcBlock::new(move |n: NonNull<NSNotification>| {
            callback("window notification", (), || handler(unsafe { n.as_ref() }));
        });
        // Scope observers to this editor's current window. Never mutate a DAW's delegate.
        // AppKit window notifications run on the main thread with a nil queue.
        let handlers = unsafe {
            [
                NSWindowDidBecomeKeyNotification,
                NSWindowDidResignKeyNotification,
                NSWindowDidChangeOcclusionStateNotification,
                NSWindowDidMiniaturizeNotification,
                NSWindowDidDeminiaturizeNotification,
                NSWindowDidChangeBackingPropertiesNotification,
                NSWindowDidChangeScreenNotification,
                NSWindowWillCloseNotification,
            ]
            .into_iter()
            .map(|name| {
                notification_center.addObserverForName_object_queue_usingBlock(
                    Some(name),
                    Some(window),
                    None,
                    &block,
                )
            })
            .collect()
        };
        Self { notification_center: Weak::from_retained(&notification_center), handlers }
    }
}

impl Drop for NotificationCenterObserver {
    fn drop(&mut self) {
        let Some(notification_center) = self.notification_center.load() else { return };
        for h in &self.handlers {
            callback("remove window observer", (), || unsafe {
                notification_center.removeObserver(h.as_ref())
            });
        }
    }
}
