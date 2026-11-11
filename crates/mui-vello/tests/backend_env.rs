//! `WGPU_BACKEND` must reach the instance: CI forces "GPU unavailable" by
//! naming a backend the OS lacks. wgpu reads its environment only with its
//! `std` feature, which Vello's dependency used to switch on and the workspace
//! must now request itself (the macOS CPU-fallback job silently got a GPU).
#![cfg(feature = "gpu-quad")]

#[test]
fn wgpu_backend_env_reaches_the_instance() {
    if std::env::var_os("MUI_BACKEND_ENV_CHILD").is_some() {
        assert_eq!(
            mui_vello::host::instance_descriptor().backends,
            wgpu::Backends::DX12
        );
        return;
    }
    let status = std::process::Command::new(std::env::current_exe().expect("test exe"))
        .args(["--exact", "wgpu_backend_env_reaches_the_instance", "--nocapture"])
        .env("WGPU_BACKEND", "dx12")
        .env("MUI_BACKEND_ENV_CHILD", "1")
        .status()
        .expect("spawn");
    assert!(status.success(), "WGPU_BACKEND was ignored");
}
