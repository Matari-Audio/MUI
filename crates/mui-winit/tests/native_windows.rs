//! Opt-in native smoke. Select X11/Wayland in the parent process environment.
//! `xvfb-run -a cargo test -p mui-winit --test native_windows -- --ignored`
#![cfg(target_os = "linux")]

#[test]
#[ignore = "requires a native display and a surface-compatible GPU backend"]
fn two_native_windows_present_resize_and_close() {
    // Cargo has released its build lock before running integration tests. The
    // child is needed because winit requires the application's main thread.
    let mut child = std::process::Command::new(env!("CARGO"))
        .args([
            "run",
            "--locked",
            "-p",
            "mui-winit",
            "--example",
            "native_windows",
        ])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .spawn()
        .expect("start native smoke example");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(90);
    loop {
        if let Some(status) = child.try_wait().expect("poll native smoke") {
            assert!(status.success(), "native smoke returned {status}");
            break;
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("native smoke exceeded its process deadline");
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}
