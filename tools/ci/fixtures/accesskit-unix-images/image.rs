use accesskit::{
    Action, ActionHandler, ActionRequest, ActivationHandler, DeactivationHandler, Node, NodeId,
    Role, Tree, TreeId, TreeUpdate,
};
use accesskit_unix::Adapter;
use std::sync::atomic::{AtomicU64, Ordering};

static ACTIVATIONS: AtomicU64 = AtomicU64::new(0);
static ACTIONS: AtomicU64 = AtomicU64::new(0);
static DROPS: AtomicU64 = AtomicU64::new(0);

struct Activate;
struct Actions;
struct Deactivate;

impl ActivationHandler for Activate {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        ACTIVATIONS.fetch_add(1, Ordering::SeqCst);
        // The real MUI callback queues a request and returns no tree.
        None
    }
}
impl ActionHandler for Actions {
    fn do_action(&mut self, request: ActionRequest) {
        assert_eq!(request.action, Action::Click);
        ACTIONS.fetch_add(1, Ordering::SeqCst);
    }
}
impl DeactivationHandler for Deactivate {
    fn deactivate_accessibility(&mut self) {}
}
macro_rules! drops {
    ($($ty:ty),*) => {$ (impl Drop for $ty {
        fn drop(&mut self) { DROPS.fetch_add(1, Ordering::SeqCst); }
    })*};
}
drops!(Activate, Actions, Deactivate);

fn tree() -> TreeUpdate {
    let mut root = Node::new(Role::Window);
    root.set_label(env!("CARGO_PKG_NAME"));
    root.set_children(vec![NodeId(2)]);
    let mut button = Node::new(Role::Button);
    button.set_label("native action");
    button.add_action(Action::Click);
    TreeUpdate {
        nodes: vec![(NodeId(1), root), (NodeId(2), button)],
        tree: Some(Tree::new(NodeId(1))),
        focus: NodeId(1),
        tree_id: TreeId::ROOT,
    }
}

pub struct Image {
    adapters: Vec<Adapter>,
}

#[repr(C)]
pub struct Stats {
    pub activations: u64,
    pub actions: u64,
    pub drops: u64,
}

#[unsafe(no_mangle)]
pub extern "C" fn accesskit_probe_open() -> *mut Image {
    Box::into_raw(Box::new(Image {
        adapters: (0..2)
            .map(|_| Adapter::new(Activate, Actions, Deactivate))
            .collect(),
    }))
}

/// # Safety
/// The host passes only a live pointer returned by open, once at a time.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn accesskit_probe_publish(image: *mut Image) {
    for adapter in &mut unsafe { &mut *image }.adapters {
        adapter.update_if_active(tree);
    }
}

/// # Safety
/// The pointer is live and exclusively borrowed for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn accesskit_probe_close_one(image: *mut Image) -> usize {
    let image = unsafe { &mut *image };
    image.adapters.pop();
    image.adapters.len()
}

/// # Safety
/// Consumes the unique image pointer; the host never accesses it again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn accesskit_probe_close(image: *mut Image) {
    drop(unsafe { Box::from_raw(image) });
}

#[unsafe(no_mangle)]
pub extern "C" fn accesskit_probe_stats() -> Stats {
    Stats {
        activations: ACTIVATIONS.load(Ordering::SeqCst),
        actions: ACTIONS.load(Ordering::SeqCst),
        drops: DROPS.load(Ordering::SeqCst),
    }
}
