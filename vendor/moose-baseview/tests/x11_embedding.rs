//! An embedded X11 child as a host sees it: XEmbed focus and `Xft.dpi`
//! changes. Needs an isolated X server: `xvfb-run cargo test --test
//! x11_embedding -- --ignored --test-threads 1`.
#![cfg(target_os = "linux")]

use moose_baseview::{
    Event, EventStatus, HandlerError, Window, WindowEvent, WindowHandler, WindowSettings,
    WindowSize,
};
use raw_window_handle::{
    HandleError, HasWindowHandle, RawWindowHandle, WindowHandle, XlibWindowHandle,
};
use std::ffi::CString;
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};
use x11_dl::xlib;

struct Parent(u64);
impl HasWindowHandle for Parent {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        // SAFETY: the test owns the parent until the child is closed.
        Ok(unsafe { WindowHandle::borrow_raw(XlibWindowHandle::new(self.0).into()) })
    }
}

/// What the child's handler saw.
#[derive(Default)]
struct Seen {
    focused: bool,
    scales: Vec<f64>,
    sizes: Vec<(u32, u32, f64)>,
}
struct Probe(Arc<Mutex<Seen>>);
impl WindowHandler for Probe {
    fn resized(&self, size: WindowSize) -> Result<(), HandlerError> {
        let mut seen = self.0.lock().expect("lock poisoned");
        seen.sizes.push((size.physical.width, size.physical.height, size.scale_factor));
        Ok(())
    }
    fn on_event(&self, event: Event) -> EventStatus {
        let mut seen = self.0.lock().expect("lock poisoned");
        match event {
            Event::Window(WindowEvent::Focused) => seen.focused = true,
            Event::Window(WindowEvent::ScaleFactorChanged(scale)) => seen.scales.push(scale),
            _ => {}
        }
        EventStatus::Ignored
    }
}

struct Host {
    lib: xlib::Xlib,
    display: *mut xlib::Display,
    root: xlib::Window,
    parent: xlib::Window,
}
impl Host {
    fn new() -> Self {
        let lib = xlib::Xlib::open().expect("libX11");
        // SAFETY: Xlib calls on a display this test opens and owns.
        unsafe {
            assert_ne!((lib.XInitThreads)(), 0);
            let display = (lib.XOpenDisplay)(std::ptr::null());
            assert!(!display.is_null(), "requires an isolated X11 server");
            let root = (lib.XDefaultRootWindow)(display);
            let parent = (lib.XCreateSimpleWindow)(display, root, 0, 0, 64, 64, 0, 0, 0);
            (lib.XMapWindow)(display, parent);
            (lib.XSync)(display, 0);
            Self { lib, display, root, parent }
        }
    }
    fn atom(&self, name: &str) -> xlib::Atom {
        let name = CString::new(name).expect("atom name");
        // SAFETY: live display, NUL-terminated name.
        unsafe { (self.lib.XInternAtom)(self.display, name.as_ptr(), 0) }
    }
    /// `_XEMBED` `message` to `child`, as an embedder sends it.
    fn xembed(&self, child: xlib::Window, message: i64, data1: i64) {
        // SAFETY: a zeroed client message is valid; live display and window.
        unsafe {
            let mut event: xlib::XEvent = std::mem::zeroed();
            let msg = &mut event.client_message;
            msg.type_ = xlib::ClientMessage;
            msg.window = child;
            msg.message_type = self.atom("_XEMBED");
            msg.format = 32;
            msg.data.set_long(0, 0);
            msg.data.set_long(1, message);
            msg.data.set_long(3, data1);
            (self.lib.XSendEvent)(self.display, child, 0, 0, &raw mut event);
            (self.lib.XSync)(self.display, 0);
        }
    }
    /// The first two CARDINALs of `_XEMBED_INFO` on `window`.
    fn xembed_info(&self, window: xlib::Window) -> Option<[u64; 2]> {
        let atom = self.atom("_XEMBED_INFO");
        // SAFETY: out-params for XGetWindowProperty; the data is freed below.
        unsafe {
            let (mut kind, mut format, mut count, mut after) = (0, 0, 0, 0);
            let mut data: *mut u8 = std::ptr::null_mut();
            (self.lib.XGetWindowProperty)(
                self.display,
                window,
                atom,
                0,
                2,
                0,
                atom,
                &raw mut kind,
                &raw mut format,
                &raw mut count,
                &raw mut after,
                &raw mut data,
            );
            if data.is_null() {
                return None;
            }
            let longs = std::slice::from_raw_parts(data.cast::<std::ffi::c_ulong>(), 2);
            let info = (count == 2).then(|| [longs[0], longs[1]]);
            (self.lib.XFree)(data.cast());
            info
        }
    }
    /// Sets the desktop's `Xft.dpi`, as `xrdb -merge` does.
    fn set_xft_dpi(&self, dpi: u32) {
        let resources = format!("Xft.dpi:\t{dpi}\n");
        // SAFETY: live display; the bytes outlive the call.
        unsafe {
            (self.lib.XChangeProperty)(
                self.display,
                self.root,
                xlib::XA_RESOURCE_MANAGER,
                xlib::XA_STRING,
                8,
                xlib::PropModeReplace,
                resources.as_ptr(),
                resources.len() as i32,
            );
            (self.lib.XSync)(self.display, 0);
        }
    }
}
impl Drop for Host {
    fn drop(&mut self) {
        // SAFETY: the child is closed before the host drops.
        unsafe {
            (self.lib.XDeleteProperty)(self.display, self.root, xlib::XA_RESOURCE_MANAGER);
            (self.lib.XDestroyWindow)(self.display, self.parent);
            (self.lib.XCloseDisplay)(self.display);
        }
    }
}

fn wait(mut done: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now().checked_add(Duration::from_secs(2)).expect("deadline");
    while !done() {
        if Instant::now() > deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    true
}

fn open(host: &Host, seen: &Arc<Mutex<Seen>>) -> (Window, xlib::Window) {
    let (tx, rx) = mpsc::channel();
    let seen = Arc::clone(seen);
    let child = Window::create(
        WindowSettings::new()
            .with_size(moose_baseview::dpi::LogicalSize::new(64.0, 64.0))
            .with_parent(&Parent(host.parent)),
        move |cx| {
            let RawWindowHandle::Xlib(h) = cx.window_handle().expect("handle").as_raw() else {
                panic!("expected an Xlib window");
            };
            tx.send(h.window).expect("send id");
            Ok(Probe(seen))
        },
    )
    .expect("child window");
    let id = rx.recv_timeout(Duration::from_secs(1)).expect("child id");
    child.show().expect("show");
    (child, id)
}

#[test]
#[ignore = "requires an isolated X11 server; run with xvfb-run"]
fn xembed_embedder_gives_the_focus() {
    let host = Host::new();
    let seen = Arc::new(Mutex::new(Seen::default()));
    let (child, id) = open(&host, &seen);
    assert_eq!(host.xembed_info(id), Some([0, 1]), "version 0, mapped");
    host.xembed(id, 0, i64::try_from(host.parent).expect("window id"));
    host.xembed(id, 4, 0);
    let focused = wait(|| seen.lock().expect("lock poisoned").focused);
    child.close();
    assert!(focused, "XEMBED_FOCUS_IN must focus the editor");
}

#[test]
#[ignore = "requires an isolated X11 server; run with xvfb-run"]
fn xft_dpi_change_keeps_the_logical_size() {
    let host = Host::new();
    host.set_xft_dpi(96);
    let seen = Arc::new(Mutex::new(Seen::default()));
    let (child, _) = open(&host, &seen);
    host.set_xft_dpi(192);
    let rescaled = wait(|| {
        let seen = seen.lock().expect("lock poisoned");
        seen.sizes.last() == Some(&(128, 128, 2.0))
    });
    let scales = seen.lock().expect("lock poisoned").scales.clone();
    child.close();
    assert!(rescaled, "64x64 points at 2x is 128x128 pixels");
    assert_eq!(scales, [2.0]);
}
