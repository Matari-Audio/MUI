//! Real native first-present/resize/close smoke for two independent windows.
//! `cargo run -p mui-winit --example native_windows`
//! Linux X11: `xvfb-run -a cargo run -p mui-winit --example native_windows`
//! Native Wayland: run in an active compositor with DISPLAY unset (select the
//! backend before starting the process, never change global environment in-app).
//! Timeout closes the windows and FAILS; only actual Presented callbacks pass.
use mui::prelude::*;
use mui_winit::{HostEvent, Options, Shared, View, WindowSpec, WindowToken};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

struct Pad;
impl View for Pad {
    fn build(&mut self, _: &mut Ui, _: &Input) -> El {
        block(180., 120.).id("native-pad")
    }
    fn changed(&mut self) -> bool {
        false
    }
    fn request_resize(&mut self, _: u32, _: u32) -> bool {
        false
    }
}
#[derive(Default)]
struct Progress {
    first: BTreeMap<WindowToken, (u32, u32)>,
    resized: BTreeSet<WindowToken>,
    closed: BTreeSet<WindowToken>,
    timed_out: bool,
}
fn window(index: usize, progress: Arc<Mutex<Progress>>) -> WindowSpec<Pad> {
    let shared = Arc::new(Mutex::new(Shared {
        view: Pad,
        ui: Ui::default(),
    }));
    let callback_model = shared.clone();
    WindowSpec::shared(
        shared,
        Options {
            title: format!("MUI native window {}", index + 1),
            size: (320, 240),
            ..Options::default()
        },
    )
    .on_event(move |token, event, controller| {
        drop(
            callback_model
                .try_lock()
                .expect("native notification must release the model lock"),
        );
        match event {
            HostEvent::Presented {
                size,
                rendering_mode,
            } => {
                let forced_cpu =
                    std::env::var("MUI_RENDERER").is_ok_and(|v| v.eq_ignore_ascii_case("cpu"));
                if !forced_cpu && rendering_mode == "cpu" {
                    // Startup pixels are required, but are not evidence that
                    // the GPU path survived open/resize/close. Wait for handover.
                    println!("{token:?}: Frame::Startup {size:?}, renderer=cpu");
                    return;
                }
                println!("{token:?}: Frame::Presented {size:?}, renderer={rendering_mode}");
                if forced_cpu {
                    assert_eq!(rendering_mode, "cpu", "forced CPU run used a GPU");
                }
                let mut progress = progress.lock().unwrap();
                if let Some(first) = progress.first.get(&token) {
                    if size != *first && progress.resized.insert(token) {
                        controller.close(token).unwrap();
                    }
                } else {
                    progress.first.insert(token, size);
                    controller.redraw(token).unwrap();
                    controller.resize(token, (400, 300)).unwrap();
                }
            }
            HostEvent::Closed => {
                progress.lock().unwrap().closed.insert(token);
            }
            _ => {}
        }
    })
}
fn main() -> Result<(), String> {
    let progress = Arc::new(Mutex::new(Progress::default()));
    let windows = vec![window(0, progress.clone())];
    let (done, completed) = mpsc::channel();
    let timeout_progress = progress.clone();
    let mut watchdog = None;
    let second_progress = progress.clone();
    let result = mui_winit::run_windows(windows, |controller, mut tokens| {
        // The factory executes on the event-loop thread; no Ui crosses threads.
        tokens.push(
            controller
                .open(move || window(1, second_progress))
                .expect("queue second window"),
        );
        watchdog = Some(std::thread::spawn(move || {
            if completed.recv_timeout(Duration::from_secs(20)).is_err() {
                timeout_progress.lock().unwrap().timed_out = true;
                for token in tokens {
                    let _ = controller.close(token);
                }
            }
        }));
    });
    let _ = done.send(());
    if let Some(watchdog) = watchdog {
        watchdog.join().unwrap();
    }
    result?;
    let progress = progress.lock().unwrap();
    if progress.timed_out
        || progress.first.len() != 2
        || progress.resized.len() != 2
        || progress.closed.len() != 2
    {
        return Err(format!(
            "native smoke failed: timeout={}, first={}, resized={}, closed={}",
            progress.timed_out,
            progress.first.len(),
            progress.resized.len(),
            progress.closed.len()
        ));
    }
    println!("two native windows presented, resized, presented again and closed");
    Ok(())
}
