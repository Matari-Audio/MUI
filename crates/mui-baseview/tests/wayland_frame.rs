//! The floating Wayland window under real headless compositors. weston
//! declines server decorations, so the toolkit's fallback frame is drawn
//! around the content; kwin decorates the window itself. Each half skips,
//! with a note, when its compositor isn't installed.
#![cfg(target_os = "linux")]

use mui::prelude::*;
use mui_baseview::{Requests, Shared, View, wayland};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const SIZE: (u32, u32) = (320, 240);

struct Magenta;
impl View for Magenta {
    fn build(&mut self, _: &mut mui::Ui, _: &mui::input::Input) -> mui::scene::El {
        block(SIZE.0 as f32, SIZE.1 as f32).fill(Color::srgb(1., 0., 1.))
    }
    fn changed(&mut self) -> bool {
        false
    }
    fn request_resize(&mut self, _: u32, _: u32) -> bool {
        false
    }
}

/// A compositor on a private socket, killed on drop.
struct Compositor {
    child: Child,
    socket: PathBuf,
}
impl Compositor {
    /// `None` when the program is missing or never opens its socket.
    fn start(program: &str, args: &[&str], name: &str) -> Option<Self> {
        let runtime = PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR")?);
        let socket = runtime.join(name);
        let _ = std::fs::remove_file(&socket);
        let child = Command::new(program)
            .args(args)
            .env_remove("WAYLAND_DISPLAY")
            .env_remove("DISPLAY")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let compositor = Self { child, socket };
        wait(Duration::from_secs(10), || compositor.socket.exists()).then_some(compositor)
    }
}
impl Drop for Compositor {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.socket);
        let _ = std::fs::remove_file(self.socket.with_extension("lock"));
    }
}

fn wait(timeout: Duration, mut done: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    while !done() {
        if Instant::now() > deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    true
}

/// Opens the probe on `socket` and waits for its first presented frame.
fn open(socket: &str, closed: &Arc<AtomicUsize>) -> wayland::FloatingWindow {
    // SAFETY: the one test in this binary sets these before opening each
    // window, from the test thread; no other thread reads the environment.
    unsafe { std::env::set_var("WAYLAND_DISPLAY", socket) };
    let shared = Arc::new(Mutex::new(Shared {
        ui: mui::Ui::default(),
        view: Magenta,
    }));
    let requests = Arc::new(Requests::default());
    let count = Arc::clone(closed);
    let window = wayland::open(
        "MUI frame probe",
        SIZE,
        SIZE,
        shared,
        Arc::clone(&requests),
        Arc::new(move || {
            count.fetch_add(1, Ordering::AcqRel);
        }),
    )
    .expect("the compositor takes a toplevel");
    assert!(
        wait(Duration::from_secs(20), || requests
            .presentation()
            .is_some()),
        "the window never presented"
    );
    window
}

fn close(window: wayland::FloatingWindow, closed: &AtomicUsize) {
    let before = closed.load(Ordering::Acquire);
    drop(window);
    assert_eq!(
        closed.load(Ordering::Acquire),
        before + 1,
        "closing reports once"
    );
}

/// The probe's fill, give or take colour management.
fn magenta(px: &[u8]) -> bool {
    px[0] > 200 && px[1] < 60 && px[2] > 200
}

/// The bounding box of the probe's fill in an RGB(A) screenshot: (x, y, w, h).
fn find(pixels: &[u8], width: usize, channels: usize) -> Option<(usize, usize, usize, usize)> {
    let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0, 0);
    for (i, px) in pixels.chunks_exact(channels).enumerate() {
        if magenta(px) {
            let (x, y) = (i % width, i / width);
            (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
        }
    }
    (x0 != usize::MAX).then(|| (x0, y0, x1 - x0 + 1, y1 - y0 + 1))
}

fn screenshot(dir: &Path, socket: &str) -> (Vec<u8>, usize, usize) {
    let status = Command::new("weston-screenshooter")
        .current_dir(dir)
        .env("WAYLAND_DISPLAY", socket)
        .status()
        .expect("weston-screenshooter runs");
    assert!(status.success(), "weston-screenshooter failed");
    let shot = std::fs::read_dir(dir)
        .expect("screenshot dir")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|e| e == "png"))
        .expect("a screenshot");
    let decoder = png::Decoder::new(std::io::BufReader::new(
        std::fs::File::open(&shot).expect("open png"),
    ));
    let mut reader = decoder.read_info().expect("png header");
    let mut pixels = vec![0; reader.output_buffer_size().expect("png size")];
    let info = reader.next_frame(&mut pixels).expect("png pixels");
    let channels = info.color_type.samples();
    pixels.truncate(info.buffer_size());
    // The next capture must not reuse this PNG, even within the same second.
    std::fs::remove_file(shot).expect("remove consumed screenshot");
    (pixels, info.width as usize, channels)
}

fn check_weston_frame(pixels: &[u8], width: usize, channels: usize) -> Result<(), String> {
    let (x, y, w, h) = find(pixels, width, channels).ok_or("the content is not on screen")?;
    if (w, h) != (SIZE.0 as usize, SIZE.1 as usize) {
        return Err(format!(
            "the frame should leave the content its logical size: got {w}x{h}"
        ));
    }
    // Weston has no server decorations. The fallback title bar above the
    // content is neither background black nor content magenta.
    if y < 10 {
        return Err(format!(
            "no room for the title bar above the content at y = {y}"
        ));
    }
    let at = ((y - 10) * width + x + w / 2) * channels;
    let bar = &pixels[at..at + 3];
    if bar == [0, 0, 0] || magenta(bar) {
        return Err(format!("no title bar above the content: {bar:?}"));
    }
    Ok(())
}

fn weston(closed: &Arc<AtomicUsize>) {
    let dir =
        Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("weston-frame-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let ini = dir.join("weston.ini");
    std::fs::write(
        &ini,
        "[shell]\nbackground-color=0xff000000\npanel-position=none\n",
    )
    .expect("weston.ini");
    let name = format!("mui-frame-weston-{}", std::process::id());
    let config = format!("--config={}", ini.display());
    let socket = format!("--socket={name}");
    let args = [
        "--backend=headless",
        "--renderer=pixman",
        "--width=640",
        "--height=480",
        "--debug",
        &socket,
        &config,
    ];
    let Some(_weston) = Compositor::start("weston", &args, &name) else {
        eprintln!("skipped: weston (headless) is not available");
        return;
    };
    let window = open(&name, closed);
    // A client submit does not mean Weston has painted the content and its
    // decoration subsurfaces yet.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let (pixels, width, channels) = screenshot(&dir, &name);
        let Err(problem) = check_weston_frame(&pixels, width, channels) else {
            break;
        };
        assert!(Instant::now() < deadline, "{problem}");
        std::thread::sleep(Duration::from_millis(100));
    }
    close(window, closed);
    let _ = std::fs::remove_dir_all(&dir);
}

fn kwin(closed: &Arc<AtomicUsize>) {
    let name = format!("mui-frame-kwin-{}", std::process::id());
    let args = [
        "--virtual",
        "--no-lockscreen",
        "--width",
        "800",
        "--height",
        "600",
        "--socket",
        &name,
    ];
    let Some(_kwin) = Compositor::start("kwin_wayland", &args, &name) else {
        eprintln!("skipped: kwin_wayland --virtual is not available");
        return;
    };
    let window = open(&name, closed);
    let ((w, h), scale) = window.size();
    close(window, closed);
    let logical = (
        (f64::from(w) / scale).round(),
        (f64::from(h) / scale).round(),
    );
    assert_eq!(
        logical,
        (f64::from(SIZE.0), f64::from(SIZE.1)),
        "server decorations leave the content its size"
    );
}

#[test]
fn the_floating_frame_under_headless_compositors() {
    // SAFETY: as in `open`; set once before any window thread starts.
    unsafe { std::env::set_var("MUI_RENDERER", "cpu") };
    let closed = Arc::new(AtomicUsize::new(0));
    weston(&closed);
    kwin(&closed);
}
