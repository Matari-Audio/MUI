//! Opt-in native smoke. Select X11/Wayland in the parent process environment.
//! Requires Linux coreutils `timeout` to bound the entire native subprocess group.
//! `xvfb-run -a cargo test -p mui-winit --test native_windows -- --ignored`
#![cfg(target_os = "linux")]

#[test]
#[ignore = "requires a native display and a surface-compatible GPU backend"]
fn two_native_windows_present_resize_and_close() {
    // Cargo has released its build lock before running integration tests. The
    // child is needed because winit requires the application's main thread.
    // Kill the whole group if native initialization blocks, including Cargo's
    // example child; killing only the Cargo wrapper would leave it running.
    let status = std::process::Command::new("timeout")
        .args([
            "--kill-after=5s",
            "90s",
            env!("CARGO"),
            "run",
            "--locked",
            "-p",
            "mui-winit",
            "--example",
            "native_windows",
        ])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .status()
        .expect("start bounded native smoke example");
    assert!(status.success(), "native smoke returned {status}");
}
