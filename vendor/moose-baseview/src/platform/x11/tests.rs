//! Native regressions: explicitly opt in with --ignored on a live X11 display.
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "native regression setup and assertions must fail the test on errors"
)]
use super::*;
use crate::dpi::PhysicalSize;
use crate::{FrameDemand, HandlerError, Window, WindowHandler, WindowSettings};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{ConnectionExt, CreateWindowAux, MapState, WindowClass};

struct CountingHandler(Arc<AtomicUsize>, FrameDemand);
impl WindowHandler for CountingHandler {
    fn frame_demand(&self) -> FrameDemand {
        self.1
    }
    fn on_frame(&self) -> std::result::Result<(), HandlerError> {
        self.0.fetch_add(1, Ordering::AcqRel);
        Ok(())
    }
    fn resized(&self, _: crate::WindowSize) -> std::result::Result<(), HandlerError> {
        Ok(())
    }
    fn on_event(&self, _: crate::Event) -> crate::EventStatus {
        crate::EventStatus::Ignored
    }
}

fn parent(conn: &x11rb::rust_connection::RustConnection, root: u32) -> u32 {
    let id = conn.generate_id().unwrap();
    conn.create_window(
        x11rb::COPY_DEPTH_FROM_PARENT,
        id,
        root,
        0,
        0,
        96,
        64,
        0,
        WindowClass::INPUT_OUTPUT,
        x11rb::COPY_FROM_PARENT,
        &CreateWindowAux::new().override_redirect(1),
    )
    .unwrap()
    .check()
    .unwrap();
    id
}

fn editor(parent: u32, count: Arc<AtomicUsize>, demand: FrameDemand) -> Window {
    let mut settings = WindowSettings::new().with_size(PhysicalSize::new(96u32, 64));
    settings.parent = Some(crate::ParentWindowHandle {
        inner: ParentWindowHandle { window_id: NonZeroU32::new(parent).unwrap() },
    });
    Window::create(settings, move |_| Ok(CountingHandler(count, demand))).unwrap()
}

fn wait_for_frame(count: &AtomicUsize, previous: usize) {
    let end = Instant::now() + Duration::from_secs(2);
    while count.load(Ordering::Acquire) <= previous && Instant::now() < end {
        thread::sleep(Duration::from_millis(2));
    }
    assert!(count.load(Ordering::Acquire) > previous, "no frame after wake/map");
}

#[test]
#[ignore = "requires a live X11 display"]
fn native_idle_has_no_frames_and_explicit_request_wakes() {
    let (conn, screen) = x11rb::rust_connection::RustConnection::connect(None).unwrap();
    let root = conn.setup().roots[screen].root;
    let host = parent(&conn, root);
    let count = Arc::new(AtomicUsize::new(0));
    let window = editor(host, Arc::clone(&count), FrameDemand::Idle);
    window.show().unwrap();
    // A child mapped before its parent must not draw yet.
    thread::sleep(Duration::from_millis(40));
    assert_eq!(count.load(Ordering::Acquire), 0);
    conn.map_window(host).unwrap().check().unwrap();
    conn.flush().unwrap();
    wait_for_frame(&count, 0);
    thread::sleep(Duration::from_millis(100));
    let settled = count.load(Ordering::Acquire);
    thread::sleep(Duration::from_millis(300));
    assert_eq!(count.load(Ordering::Acquire), settled, "idle timer kept drawing");
    let requester = window.frame_requester().unwrap();
    let wake = requester.clone();
    thread::spawn(move || wake.request_frame()).join().unwrap();
    wait_for_frame(&count, settled);
    thread::sleep(Duration::from_millis(50));
    let redrawn = count.load(Ordering::Acquire);
    assert_eq!(redrawn, settled + 1, "one request must produce exactly one frame");
    thread::sleep(Duration::from_millis(150));
    assert_eq!(count.load(Ordering::Acquire), redrawn);
    window.close_bounded(Duration::from_millis(250));
    requester.request_frame(); // inert after close, with no retained native window
    conn.destroy_window(host).unwrap().check().unwrap();
}

#[test]
#[ignore = "requires a live X11 display"]
fn native_request_racing_demand_and_timer_rearm_is_not_lost() {
    use std::cell::RefCell;
    use std::sync::atomic::AtomicBool;
    use std::sync::mpsc;
    struct Race {
        count: Arc<AtomicUsize>,
        ready: Arc<AtomicBool>,
        query: mpsc::Sender<()>,
        requested: RefCell<mpsc::Receiver<()>>,
    }
    impl WindowHandler for Race {
        fn frame_demand(&self) -> FrameDemand {
            if self.ready.swap(false, Ordering::AcqRel) {
                // Compute the answer, then force a producer request before the
                // loop can act on this stale answer and arm a long timer.
                let deadline = Instant::now() + Duration::from_secs(5);
                self.query.send(()).unwrap();
                self.requested.borrow().recv_timeout(Duration::from_secs(1)).unwrap();
                FrameDemand::At(deadline)
            } else {
                FrameDemand::Idle
            }
        }
        fn on_frame(&self) -> std::result::Result<(), HandlerError> {
            self.count.fetch_add(1, Ordering::AcqRel);
            Ok(())
        }
        fn resized(&self, _: crate::WindowSize) -> std::result::Result<(), HandlerError> {
            Ok(())
        }
        fn on_event(&self, _: crate::Event) -> crate::EventStatus {
            crate::EventStatus::Ignored
        }
    }
    let (conn, screen) = x11rb::rust_connection::RustConnection::connect(None).unwrap();
    let host = parent(&conn, conn.setup().roots[screen].root);
    conn.map_window(host).unwrap().check().unwrap();
    let count = Arc::new(AtomicUsize::new(0));
    let ready = Arc::new(AtomicBool::new(false));
    let (query, query_rx) = mpsc::channel();
    let (requested, requested_rx) = mpsc::channel();
    let mut settings = WindowSettings::new().with_size(PhysicalSize::new(96u32, 64));
    settings.parent = Some(crate::ParentWindowHandle {
        inner: ParentWindowHandle { window_id: NonZeroU32::new(host).unwrap() },
    });
    let h_count = Arc::clone(&count);
    let h_ready = Arc::clone(&ready);
    let window = Window::create(settings, move |_| {
        Ok(Race { count: h_count, ready: h_ready, query, requested: RefCell::new(requested_rx) })
    })
    .unwrap();
    window.show().unwrap();
    wait_for_frame(&count, 0);
    thread::sleep(Duration::from_millis(100));
    let before = count.load(Ordering::Acquire);
    let wake = window.frame_requester().unwrap();
    let racing_wake = wake.clone();
    let producer = thread::spawn(move || {
        query_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        racing_wake.request_frame();
        requested.send(()).unwrap();
    });
    ready.store(true, Ordering::Release);
    wake.request_frame();
    wait_for_frame(&count, before + 1);
    producer.join().unwrap();
    thread::sleep(Duration::from_millis(200));
    assert_eq!(
        count.load(Ordering::Acquire),
        before + 2,
        "the racing request must draw once, not wait five seconds or remain latched"
    );
    window.close_bounded(Duration::from_millis(250));
    conn.destroy_window(host).unwrap().check().unwrap();
}

#[test]
#[ignore = "requires a live X11 display"]
fn native_two_host_threads_open_close_and_preserve_default_cadence() {
    let (conn, screen) = x11rb::rust_connection::RustConnection::connect(None).unwrap();
    let root = conn.setup().roots[screen].root;
    let hosts = [parent(&conn, root), parent(&conn, root)];
    for host in hosts {
        conn.map_window(host).unwrap().check().unwrap();
    }
    conn.flush().unwrap();
    let threads: Vec<_> = hosts
        .into_iter()
        .map(|host| {
            thread::spawn(move || {
                for _ in 0..3 {
                    let count = Arc::new(AtomicUsize::new(0));
                    let window = editor(host, Arc::clone(&count), FrameDemand::Continuous);
                    window.show().unwrap();
                    wait_for_frame(&count, 0);
                    let before = count.load(Ordering::Acquire);
                    thread::sleep(Duration::from_millis(80));
                    assert!(count.load(Ordering::Acquire) > before);
                    window.close_bounded(Duration::from_millis(250));
                }
            })
        })
        .collect();
    for worker in threads {
        worker.join().unwrap();
    }
    for host in hosts {
        conn.destroy_window(host).unwrap().check().unwrap();
    }
}

#[test]
#[ignore = "requires a live X11 display"]
fn native_root_visual_checked_creation_zero_size_and_xembed_reparent() {
    let conn = Rc::new(X11Connection::new().unwrap());
    let root = conn.default_screen().root;
    let p1 = conn.conn.generate_id().unwrap();
    let p2 = conn.conn.generate_id().unwrap();
    let argb = conn
        .default_screen()
        .allowed_depths
        .iter()
        .find(|d| d.depth == 32)
        .and_then(|d| d.visuals.first());
    let argb_map = argb.map(|v| {
        let map = conn.conn.generate_id().unwrap();
        conn.conn
            .create_colormap(x11rb::protocol::xproto::ColormapAlloc::NONE, map, root, v.visual_id)
            .unwrap()
            .check()
            .unwrap();
        (map, v.visual_id)
    });
    for p in [p1, p2] {
        let different_depth = (p == p2).then_some(argb_map).flatten();
        let attrs = CreateWindowAux::new()
            .override_redirect(1)
            .colormap(different_depth.map(|(map, _)| map))
            .border_pixel(0);
        conn.conn
            .create_window(
                if different_depth.is_some() { 32 } else { x11rb::COPY_DEPTH_FROM_PARENT },
                p,
                root,
                0,
                0,
                96,
                64,
                0,
                WindowClass::INPUT_OUTPUT,
                different_depth.map_or(x11rb::COPY_FROM_PARENT, |(_, visual)| visual),
                &attrs,
            )
            .unwrap()
            .check()
            .unwrap();
        conn.conn.map_window(p).unwrap().check().unwrap();
    }
    let visual = visual_info::WindowVisualConfig::find_best_visual_config(&conn, false).unwrap();
    assert_eq!(visual.visual_id, conn.default_screen().root_visual);
    assert_eq!(visual.visual_depth, conn.default_screen().root_depth);
    let child = xcb_window::XcbWindow::new(
        Rc::clone(&conn),
        PhysicalSize::new(0, 0),
        &visual,
        NonZeroU32::new(p1),
    )
    .unwrap();
    child.map_window().unwrap().check().unwrap();
    let attrs = conn.conn.get_window_attributes(child.id().get()).unwrap().reply().unwrap();
    assert_eq!(attrs.visual, visual.visual_id);
    assert_eq!(attrs.map_state, MapState::VIEWABLE);
    let visibility =
        visibility_tree::AncestorVisibilityState::discover(&conn, child.id(), true).unwrap();
    assert_eq!(visibility.parent_id().unwrap().get(), p1);
    child.reparent(NonZeroU32::new(p2)).unwrap().check().unwrap();
    visibility.window_reparented(child.id(), NonZeroU32::new(p2), &conn);
    assert_eq!(visibility.parent_id().unwrap().get(), p2);
    assert!(visibility.own_window_is_viewable());
    let geometry = conn.conn.get_geometry(child.id().get()).unwrap().reply().unwrap();
    assert_eq!((geometry.width, geometry.height), (1, 1));
    assert_eq!(geometry.depth, visual.visual_depth);
    assert!(
        xcb_window::XcbWindow::new(
            Rc::clone(&conn),
            PhysicalSize::new(96, 64),
            &visual,
            NonZeroU32::new(u32::MAX)
        )
        .is_err(),
        "invalid parent must fail at creation"
    );
    drop(child);
    for p in [p1, p2] {
        conn.conn.destroy_window(p).unwrap().check().unwrap();
    }
    if let Some((map, _)) = argb_map {
        conn.conn.free_colormap(map).unwrap().check().unwrap();
    }
}

#[test]
fn threaded_controller_is_send_without_unsafe_assertions() {
    fn assert_send<T: Send>() {}
    assert_send::<super::window_thread::WindowThreadControl>();
    assert_send::<crate::ThreadedWindow>();
}

struct OwnedHandler {
    context: crate::WindowContext,
    frames: Arc<AtomicUsize>,
    dropped: std::sync::mpsc::Sender<thread::ThreadId>,
    block: Option<(
        Arc<std::sync::atomic::AtomicBool>,
        std::sync::mpsc::Sender<()>,
        std::sync::mpsc::Receiver<()>,
    )>,
}
impl WindowHandler for OwnedHandler {
    fn frame_demand(&self) -> FrameDemand {
        FrameDemand::Idle
    }
    fn poll(&self) {
        if let Some((block, entered, release)) = &self.block {
            if block.swap(false, Ordering::AcqRel) {
                entered.send(()).unwrap();
                release.recv_timeout(Duration::from_secs(3)).unwrap();
            }
        }
    }
    fn on_frame(&self) -> std::result::Result<(), HandlerError> {
        self.frames.fetch_add(1, Ordering::AcqRel);
        Ok(())
    }
    fn resized(&self, _: crate::WindowSize) -> std::result::Result<(), HandlerError> {
        Ok(())
    }
    fn on_event(&self, _: crate::Event) -> crate::EventStatus {
        crate::EventStatus::Ignored
    }
}
impl Drop for OwnedHandler {
    fn drop(&mut self) {
        // Reading the non-Send context in Drop also exercises its owner-thread lifetime.
        let _ = self.context.size();
        self.dropped.send(thread::current().id()).unwrap();
    }
}

fn embedded_settings(parent: u32) -> WindowSettings {
    let mut settings = WindowSettings::new().with_size(PhysicalSize::new(96u32, 64));
    settings.parent = Some(crate::ParentWindowHandle {
        inner: ParentWindowHandle { window_id: NonZeroU32::new(parent).unwrap() },
    });
    settings
}

#[test]
#[ignore = "requires a live X11 display"]
fn native_threaded_controller_moves_closes_and_reopens() {
    let (conn, screen) = x11rb::rust_connection::RustConnection::connect(None).unwrap();
    let host = parent(&conn, conn.setup().roots[screen].root);
    conn.map_window(host).unwrap().check().unwrap();
    for _ in 0..3 {
        let frames = Arc::new(AtomicUsize::new(0));
        let handler_frames = Arc::clone(&frames);
        let (dropped, drop_rx) = std::sync::mpsc::channel();
        let (owner, owner_rx) = std::sync::mpsc::channel();
        let window = crate::ThreadedWindow::create(embedded_settings(host), move |context| {
            owner.send(thread::current().id()).unwrap();
            Ok(OwnedHandler { context, frames: handler_frames, dropped, block: None })
        })
        .unwrap();
        window.show().unwrap();
        wait_for_frame(&frames, 0);
        let wake = window.waker();
        let visibility = window.visibility_control();
        assert_eq!(conn.query_tree(host).unwrap().reply().unwrap().children.len(), 1);
        thread::spawn(move || window.close_bounded(Duration::ZERO)).join().unwrap();
        assert_eq!(drop_rx.try_recv().unwrap(), owner_rx.recv().unwrap());
        assert!(conn.query_tree(host).unwrap().reply().unwrap().children.is_empty());
        let after = frames.load(Ordering::Acquire);
        wake.request_redraw();
        wake.request_poll();
        visibility.set_visible(true);
        thread::sleep(Duration::from_millis(20));
        assert_eq!(frames.load(Ordering::Acquire), after);
    }
    conn.destroy_window(host).unwrap().check().unwrap();
}

#[test]
#[ignore = "requires a live X11 display"]
fn native_embedded_close_waits_for_inflight_callback_and_native_drop() {
    let (conn, screen) = x11rb::rust_connection::RustConnection::connect(None).unwrap();
    let host = parent(&conn, conn.setup().roots[screen].root);
    conn.map_window(host).unwrap().check().unwrap();
    let block = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let handler_block = Arc::clone(&block);
    let (entered, enter_rx) = std::sync::mpsc::channel();
    let (release, release_rx) = std::sync::mpsc::channel();
    let (dropped, drop_rx) = std::sync::mpsc::channel();
    let window = crate::ThreadedWindow::create(embedded_settings(host), move |context| {
        Ok(OwnedHandler {
            context,
            frames: Arc::new(AtomicUsize::new(0)),
            dropped,
            block: Some((handler_block, entered, release_rx)),
        })
    })
    .unwrap();
    window.show().unwrap();
    block.store(true, Ordering::Release);
    window.request_poll().unwrap();
    enter_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let (closed, close_rx) = std::sync::mpsc::channel();
    let closer = thread::spawn(move || {
        window.close_bounded(Duration::ZERO);
        closed.send(()).unwrap();
    });
    assert_eq!(
        close_rx.recv_timeout(Duration::from_millis(60)),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout),
        "embedded close returned while a callback still held host/native state"
    );
    release.send(()).unwrap();
    close_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    drop_rx.try_recv().unwrap();
    assert!(conn.query_tree(host).unwrap().reply().unwrap().children.is_empty());
    conn.destroy_window(host).unwrap().check().unwrap();
    closer.join().unwrap();
}

#[test]
#[ignore = "requires a live X11 display"]
fn native_parent_deletion_quiesces_threaded_window() {
    let (conn, screen) = x11rb::rust_connection::RustConnection::connect(None).unwrap();
    let host = parent(&conn, conn.setup().roots[screen].root);
    conn.map_window(host).unwrap().check().unwrap();
    let frames = Arc::new(AtomicUsize::new(0));
    let handler_frames = Arc::clone(&frames);
    let (dropped, drop_rx) = std::sync::mpsc::channel();
    let window = crate::ThreadedWindow::create(embedded_settings(host), move |context| {
        Ok(OwnedHandler { context, frames: handler_frames, dropped, block: None })
    })
    .unwrap();
    window.show().unwrap();
    wait_for_frame(&frames, 0);
    let wake = window.waker();
    conn.destroy_window(host).unwrap().check().unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while window.is_open() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(2));
    }
    assert!(!window.is_open(), "parent deletion did not stop its child worker");
    window.close();
    drop_rx.try_recv().unwrap();
    let after = frames.load(Ordering::Acquire);
    wake.request_redraw();
    thread::sleep(Duration::from_millis(20));
    assert_eq!(frames.load(Ordering::Acquire), after);
}

#[test]
#[ignore = "requires a live X11 display"]
fn native_embedded_startup_timeout_waits_for_builder_teardown() {
    let (conn, screen) = x11rb::rust_connection::RustConnection::connect(None).unwrap();
    let host = parent(&conn, conn.setup().roots[screen].root);
    let (entered, enter_rx) = std::sync::mpsc::channel();
    let (release, release_rx) = std::sync::mpsc::channel();
    let (dropped, drop_rx) = std::sync::mpsc::channel();
    let (returned, return_rx) = std::sync::mpsc::channel();
    let opener = thread::spawn(move || {
        let result = crate::ThreadedWindow::create(embedded_settings(host), move |context| {
            entered.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(3)).unwrap();
            Ok(OwnedHandler {
                context,
                frames: Arc::new(AtomicUsize::new(0)),
                dropped,
                block: None,
            })
        });
        returned.send(result.is_err()).unwrap();
    });
    enter_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(
        return_rx.recv_timeout(Duration::from_millis(300)),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout),
        "startup failure released the parent's lifetime before its builder finished"
    );
    release.send(()).unwrap();
    assert!(return_rx.recv_timeout(Duration::from_secs(2)).unwrap());
    drop_rx.try_recv().unwrap();
    assert!(conn.query_tree(host).unwrap().reply().unwrap().children.is_empty());
    conn.destroy_window(host).unwrap().check().unwrap();
    opener.join().unwrap();
}

#[test]
#[ignore = "requires a live X11 display"]
fn native_floating_bounded_close_revokes_work_and_allows_late_teardown() {
    let block = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let handler_block = Arc::clone(&block);
    let (entered, enter_rx) = std::sync::mpsc::channel();
    let (release, release_rx) = std::sync::mpsc::channel();
    let (dropped, drop_rx) = std::sync::mpsc::channel();
    let window = crate::ThreadedWindow::create(WindowSettings::new(), move |context| {
        Ok(OwnedHandler {
            context,
            frames: Arc::new(AtomicUsize::new(0)),
            dropped,
            block: Some((handler_block, entered, release_rx)),
        })
    })
    .unwrap();
    window.show().unwrap();
    block.store(true, Ordering::Release);
    window.request_poll().unwrap();
    enter_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let wake = window.waker();
    let (closed, close_rx) = std::sync::mpsc::channel();
    let closer = thread::spawn(move || {
        window.close_bounded(Duration::ZERO);
        closed.send(()).unwrap();
    });
    close_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(drop_rx.try_recv().is_err(), "blocked native callback unexpectedly finished");
    wake.request_redraw();
    release.send(()).unwrap();
    drop_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    closer.join().unwrap();
}

#[test]
#[ignore = "requires a live X11 display"]
fn native_failed_show_stops_worker_before_joining() {
    let block = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let handler_block = Arc::clone(&block);
    let (entered, enter_rx) = std::sync::mpsc::channel();
    let (release, release_rx) = std::sync::mpsc::channel();
    let (dropped, drop_rx) = std::sync::mpsc::channel();
    let window = crate::ThreadedWindow::create(WindowSettings::new(), move |context| {
        Ok(OwnedHandler {
            context,
            frames: Arc::new(AtomicUsize::new(0)),
            dropped,
            block: Some((handler_block, entered, release_rx)),
        })
    })
    .unwrap();
    window.show().unwrap();
    block.store(true, Ordering::Release);
    window.request_poll().unwrap();
    enter_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    window.visibility_control().set_visible(false);
    let (done, receive) = std::sync::mpsc::channel();
    let runner = thread::spawn(move || done.send(window.run_until_closed().is_err()).unwrap());
    thread::sleep(Duration::from_millis(75));
    release.send(()).unwrap();
    assert!(receive.recv_timeout(Duration::from_secs(2)).unwrap());
    drop_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    runner.join().unwrap();
}
