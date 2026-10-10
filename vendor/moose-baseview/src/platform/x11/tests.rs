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
    let visual = visual_info::WindowVisualConfig::find_best_visual_config(&conn).unwrap();
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
