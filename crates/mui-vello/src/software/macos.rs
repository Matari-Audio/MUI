//! Softbuffer 0.4.8 adds a CPU sublayer, but its Drop only removes observers.
//! Retain and remove that added layer on handover so it cannot cover the root
//! CAMetalLayer's GPU drawable. Never remove the root or pre-existing siblings.
#![expect(
    unsafe_code,
    reason = "AppKit layer ownership on the native window thread"
)]
#![expect(
    unexpected_cfgs,
    reason = "objc 0.2 msg_send checks the retired cargo-clippy feature"
)]
use objc::runtime::Object;
use objc::{msg_send, sel, sel_impl};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::{marker::PhantomData, rc::Rc};
type Id = *mut Object;

pub(super) struct Snapshot {
    view: Id,
    before: Vec<Id>,
}
pub(super) struct Layers {
    added: Vec<Id>,
    _window_thread: PhantomData<Rc<()>>,
}
impl Snapshot {
    /// Caller creates/drops the software surface on its live NSView's thread.
    pub fn new(window: &impl HasWindowHandle) -> Self {
        let view = match window.window_handle().map(|h| h.as_raw()) {
            Ok(RawWindowHandle::AppKit(handle)) => handle.ns_view.as_ptr().cast(),
            _ => std::ptr::null_mut(),
        };
        Self {
            view,
            before: children(view),
        }
    }
    pub fn finish(self) -> Layers {
        let added = children(self.view)
            .into_iter()
            .filter(|layer| !self.before.contains(layer))
            .map(|layer| {
                // SAFETY: the live root and softbuffer own each new CALayer.
                // An extra retain keeps it valid until our matching Drop.
                unsafe {
                    let retained: Id = msg_send![layer, retain];
                    retained
                }
            })
            .collect();
        Layers {
            added,
            _window_thread: PhantomData,
        }
    }
}
impl Drop for Layers {
    fn drop(&mut self) {
        for layer in self.added.drain(..) {
            // SAFETY: finish retained this layer; the !Send marker keeps
            // removal on the owning thread. Window drops this before its
            // softbuffer surface, so that surface still owns its observer.
            unsafe {
                let _: () = msg_send![layer, removeFromSuperlayer];
                let _: () = msg_send![layer, release];
            }
        }
    }
}
fn children(view: Id) -> Vec<Id> {
    if view.is_null() {
        return Vec::new();
    }
    // SAFETY: view is borrowed from a live AppKit window on the owning thread;
    // layer/sublayers/count/objectAtIndex are standard NSView/CALayer/NSArray
    // queries. The root retains its children across this synchronous snapshot.
    unsafe {
        let root: Id = msg_send![view, layer];
        if root.is_null() {
            return Vec::new();
        }
        let list: Id = msg_send![root, sublayers];
        if list.is_null() {
            return Vec::new();
        }
        let count: usize = msg_send![list, count];
        (0..count)
            .map(|i| {
                let layer: Id = msg_send![list, objectAtIndex: i];
                layer
            })
            .collect()
    }
}
