// Copyright 2026 Matari Audio. Licensed under MIT OR Apache-2.0.
//! Native-only regression support; enabled solely by MUI's two-image fixture.

use crate::{SubclassingAdapter, class_macro::declare_class};
use accesskit::{
    Action, ActionHandler, ActionRequest, ActivationHandler, Node, NodeId, Role, Tree, TreeUpdate,
};
use objc2::{
    ClassType, DeclaredClass, msg_send, msg_send_id,
    mutability::InteriorMutable,
    rc::{Allocated, Id, PartialInit, autoreleasepool},
    runtime::AnyObject,
};
use objc2_app_kit::{NSApplication, NSView};
use objc2_foundation::{NSArray, NSObject, NSPoint, NSRect, NSSize, NSThread};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

pub(crate) static NODE_IVAR_DROPS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

struct DropIvars(Rc<RefCell<Vec<&'static str>>>);
impl Drop for DropIvars {
    fn drop(&mut self) {
        self.0.borrow_mut().push("ivars");
    }
}

declare_class!(
    struct DropProbe;
    unsafe impl ClassType for DropProbe {
        type Super = NSObject;
        type Mutability = InteriorMutable;
        const NAME: &'static str = "AccessKitImageDropProbe";
    }
    impl DeclaredClass for DropProbe { type Ivars = DropIvars; }
);
impl Drop for DropProbe {
    fn drop(&mut self) {
        self.ivars().0.borrow_mut().push("class");
    }
}

// Read the actual byte written by the pinned objc2 initializer, not a duplicate
// test implementation. Every pointer remains owned by its live allocation.
unsafe fn flag(ptr: *const DropProbe) -> u8 {
    unsafe { *ptr.cast::<u8>().offset(DropProbe::__drop_flag_offset()) }
}
fn check_drop_contract() {
    autoreleasepool(|_| {
        let events = Rc::new(RefCell::new(Vec::new()));
        let allocated = DropProbe::alloc();
        assert_eq!(unsafe { flag(Allocated::as_ptr(&allocated)) }, 0x00);
        drop(allocated);
        assert!(events.borrow().is_empty());
        let partial = DropProbe::alloc().set_ivars(DropIvars(events.clone()));
        assert_eq!(unsafe { flag(PartialInit::as_ptr(&partial)) }, 0x0f);
        drop(partial);
        assert_eq!(&*events.borrow(), &["ivars"]);
        events.borrow_mut().clear();
        let partial = DropProbe::alloc().set_ivars(DropIvars(events.clone()));
        let complete: Id<DropProbe> = unsafe { msg_send_id![super(partial), init] };
        assert_eq!(unsafe { flag(Id::as_ptr(&complete)) }, 0xff);
        drop(complete);
        assert_eq!(&*events.borrow(), &["class", "ivars"]);
    });
}

#[derive(Default)]
struct Counts {
    activated: Cell<usize>,
    actions: Cell<usize>,
    dropped: Cell<usize>,
}
struct Activate(Rc<Counts>);
impl ActivationHandler for Activate {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        self.0.activated.set(self.0.activated.get() + 1);
        Some(tree())
    }
}
impl Drop for Activate {
    fn drop(&mut self) {
        self.0.dropped.set(self.0.dropped.get() + 1);
    }
}
struct Actions(Rc<Counts>);
impl ActionHandler for Actions {
    fn do_action(&mut self, request: ActionRequest) {
        assert_eq!(request.action, Action::Click);
        assert_eq!(request.target_node, NodeId(1));
        self.0.actions.set(self.0.actions.get() + 1);
    }
}
impl Drop for Actions {
    fn drop(&mut self) {
        self.0.dropped.set(self.0.dropped.get() + 1);
    }
}
fn tree() -> TreeUpdate {
    let mut node = Node::new(Role::Button);
    node.set_label("image isolation button");
    node.add_action(Action::Click);
    TreeUpdate {
        tree_id: accesskit::TreeId::ROOT,
        nodes: vec![(NodeId(1), node)],
        tree: Some(Tree::new(NodeId(1))),
        focus: NodeId(1),
    }
}

/// Opaque state owned and destroyed inside one fixture dylib.
pub struct Probe {
    node_drop_start: usize,
    counts: Rc<Counts>,
    views: Vec<Id<NSView>>,
    adapters: Vec<SubclassingAdapter>,
    nodes: Vec<Id<NSObject>>,
    names: Vec<String>,
}
impl Probe {
    /// Exercise both Rust-bearing declarations and the dynamic view subclass.
    pub fn new() -> Self {
        assert!(NSThread::isMainThread_class());
        check_drop_contract();
        autoreleasepool(|_| {
            let _app: Id<NSApplication> =
                unsafe { msg_send_id![NSApplication::class(), sharedApplication] };
            let counts = Rc::new(Counts::default());
            let mut result = Self {
                node_drop_start: NODE_IVAR_DROPS.load(std::sync::atomic::Ordering::Relaxed),
                counts: counts.clone(),
                views: Vec::new(),
                adapters: Vec::new(),
                nodes: Vec::new(),
                names: Vec::new(),
            };
            for _ in 0..3 {
                let view = unsafe {
                    NSView::initWithFrame(
                        msg_send_id![NSView::class(), alloc],
                        NSRect::new(NSPoint::new(0., 0.), NSSize::new(100., 50.)),
                    )
                };
                let adapter = unsafe {
                    SubclassingAdapter::new(
                        Id::as_ptr(&view).cast_mut().cast(),
                        Activate(counts.clone()),
                        Actions(counts.clone()),
                    )
                };
                let children: Id<NSArray<NSObject>> =
                    unsafe { msg_send_id![&*view, accessibilityChildren] };
                assert_eq!(children.count(), 1);
                let node = unsafe { children.objectAtIndex(0) };
                let associated = unsafe {
                    objc2::ffi::objc_getAssociatedObject(
                        Id::as_ptr(&view).cast(),
                        crate::image::marker_key(),
                    )
                };
                assert!(!associated.is_null());
                let associated = unsafe { &*associated.cast::<AnyObject>() };
                let names = vec![
                    view.class().name().to_string(),
                    associated.class().name().to_string(),
                    node.class().name().to_string(),
                ];
                if result.names.is_empty() {
                    result.names = names;
                } else {
                    assert_eq!(result.names, names);
                }
                result.views.push(view);
                result.adapters.push(adapter);
                result.nodes.push(node);
            }
            assert_eq!(counts.activated.get(), 3);
            // Explicitly simulate the namespace allocator encountering an old
            // anchor at the same marker address after an unload/reload.
            if std::env::var_os("MUI_ACCESSKIT_PROBE_UNISOLATED").is_none() {
                let retry = crate::image::reserve_namespace();
                assert!(!result.names[0].ends_with(&retry));
            }
            result
                .names
                .push(format!("marker:{:x}", crate::image::marker_key() as usize));
            result
        })
    }
    pub fn names(&self) -> &[String] {
        &self.names
    }
    /// Invoke the native node IMP, update every live adapter, then query again.
    pub fn action(&mut self) -> usize {
        autoreleasepool(|_| {
            for (node, adapter) in self.nodes.iter().zip(&mut self.adapters) {
                let pressed: bool = unsafe { msg_send![&**node, accessibilityPerformPress] };
                assert!(pressed);
                // Querying after the update verifies the live native hierarchy.
                if let Some(events) = adapter.update_if_active(tree) {
                    events.raise();
                }
            }
            for view in &self.views {
                let children: Id<NSArray<NSObject>> =
                    unsafe { msg_send_id![&**view, accessibilityChildren] };
                assert_eq!(children.count(), 1);
            }
            assert_eq!(self.counts.activated.get(), 3);
            self.counts.actions.get()
        })
    }
    /// Release all Rust-bearing objects and drain autoreleases before dlclose.
    pub fn finish(mut self) {
        autoreleasepool(|_| {
            self.nodes.clear();
            self.adapters.clear();
            assert_eq!(self.counts.dropped.get(), 6);
            for view in &self.views {
                let associated = unsafe {
                    objc2::ffi::objc_getAssociatedObject(
                        Id::as_ptr(view).cast(),
                        crate::image::marker_key(),
                    )
                };
                assert!(associated.is_null());
                assert_eq!(view.class(), NSView::class());
            }
            self.views.clear();
        });
        assert_eq!(
            NODE_IVAR_DROPS.load(std::sync::atomic::Ordering::Relaxed) - self.node_drop_start,
            3
        );
    }
}
impl Default for Probe {
    fn default() -> Self {
        Self::new()
    }
}
