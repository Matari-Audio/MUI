//! A native fault cannot unwind. Keep a locked, durable trial until the first
//! GPU frame completes. The OS releases its lock if the owning process dies.
//! All I/O is on initialization/render workers, never an audio callback.
use std::path::Path;
#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;

#[derive(Clone)]
pub(super) struct Key(String);

impl Key {
    pub(super) fn enumeration(backends: wgpu::Backends) -> Self {
        Self(format!(
            "backend={backends:?};adapter=unknown;driver=unknown"
        ))
    }

    pub(super) fn adapter(info: &wgpu::AdapterInfo) -> Self {
        Self(format!(
            "backend={:?};vendor={:08x};device={:08x};driver[{}]={};driver_info[{}]={}",
            info.backend,
            info.vendor,
            info.device,
            info.driver.len(),
            info.driver,
            info.driver_info.len(),
            info.driver_info,
        ))
    }
}

pub(super) fn ignore_denials(value: Option<&str>) -> bool {
    value.is_some_and(|v| v == "1")
}

/// One file per attempt, rather than one shared marker per GPU. Success removes
/// only this attempt; it can never erase another DAW's crash evidence.
pub(super) struct Attempt {
    key: Key,
    #[cfg(not(target_arch = "wasm32"))]
    marker: std::sync::Mutex<Option<Marker>>,
}

impl Attempt {
    pub(super) fn begin(key: &Key) -> Result<Self, String> {
        Self::begin_at(
            &crate::diagnostics::directory().join("gpu-trials"),
            key,
            ignore_denials(std::env::var("MUI_GPU_IGNORE_DENYLIST").ok().as_deref()),
        )
        .inspect_err(|error| crate::diagnostics::error("mui-vello", "gpu_sentinel", error))
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn begin_at(directory: &Path, key: &Key, ignore: bool) -> Result<Self, String> {
        use std::io::{Read as _, Write as _};
        let attempt = (|| -> std::io::Result<Self> {
            let _registry = registry(directory)?;
            if key.0.len() > 64 * 1024 {
                return Err(std::io::Error::other("GPU identity exceeds 64 KiB"));
            }
            // FNV is only a filename index. Compare the complete identity too,
            // so a hash collision cannot deny a different adapter/driver.
            let hash = key.0.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
                (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3)
            });
            let prefix = format!("{hash:016x}-");
            for entry in std::fs::read_dir(directory)? {
                let entry = entry?;
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if !name.starts_with(&prefix) || !name.ends_with(".pending") {
                    continue;
                }
                let file = std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(entry.path())?;
                match file.try_lock() {
                    // The creating process still owns this attempt, including
                    // another instance in this very process. PID reuse is safe.
                    Err(std::fs::TryLockError::WouldBlock) => continue,
                    Err(std::fs::TryLockError::Error(error)) => return Err(error),
                    Ok(()) => {}
                }
                let mut stored = String::new();
                file.take(64 * 1024).read_to_string(&mut stored)?;
                if stored == key.0 {
                    crate::diagnostics::breadcrumb(
                        "mui-vello",
                        "gpu_denylist",
                        &format!(
                            "{} stale GPU trial {}; identity={}",
                            if ignore { "override ignores" } else { "skip" },
                            entry.path().display(),
                            key.0,
                        ),
                    );
                    if !ignore {
                        return Err(std::io::Error::other(format!(
                            "GPU disabled after an unfinished trial in a dead process: {}",
                            key.0
                        )));
                    }
                }
            }
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let mut options = std::fs::OpenOptions::new();
            options.create_new(true).read(true).write(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt as _;
                options.mode(0o600);
            }
            let mut collision = 0;
            let (path, mut file) = loop {
                let path = directory.join(format!(
                    "{prefix}{}-{stamp:x}-{collision}.pending",
                    std::process::id()
                ));
                match options.open(&path) {
                    Ok(file) => break (path, file),
                    Err(error)
                        if error.kind() == std::io::ErrorKind::AlreadyExists && collision < 16 =>
                    {
                        collision += 1;
                    }
                    Err(error) => return Err(error),
                }
            };
            file.lock()?;
            file.write_all(key.0.as_bytes())?;
            file.sync_all()?;
            Ok(Self {
                key: key.clone(),
                marker: std::sync::Mutex::new(Some(Marker { path, file })),
            })
        })();
        attempt.map_err(|error| error.to_string())
    }

    #[cfg(target_arch = "wasm32")]
    fn begin_at(_: &Path, key: &Key, _: bool) -> Result<Self, String> {
        // Browser persistence/driver crashes are owned by the browser process.
        Ok(Self { key: key.clone() })
    }

    pub(super) fn phase(&self, stage: &str) {
        crate::diagnostics::breadcrumb("mui-vello", stage, &self.key.0);
    }

    pub(super) fn finish(&self) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(marker) = self
            .marker
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            // Serialize close+unlink with scanners, including on Windows where
            // the file may not be deletable while its lock handle remains open.
            if let Some(directory) = marker.path.parent() {
                match registry(directory) {
                    Ok(_registry) => {
                        drop(marker.file);
                        if let Err(error) = std::fs::remove_file(marker.path) {
                            crate::diagnostics::error(
                                "mui-vello",
                                "gpu_sentinel_clear",
                                &error.to_string(),
                            );
                        }
                    }
                    Err(error) => crate::diagnostics::error(
                        "mui-vello",
                        "gpu_sentinel_clear",
                        &error.to_string(),
                    ),
                }
            }
        }
    }
}

impl Drop for Attempt {
    fn drop(&mut self) {
        // Rust errors, panics and cancelled initialization are recoverable;
        // only termination without unwinding leaves crash evidence behind.
        self.finish();
    }
}

#[cfg(not(target_arch = "wasm32"))]
struct Marker {
    path: PathBuf,
    file: std::fs::File,
}

#[cfg(not(target_arch = "wasm32"))]
fn registry(directory: &Path) -> std::io::Result<std::fs::File> {
    std::fs::create_dir_all(directory)?;
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(directory.join("registry.lock"))?;
    // Held only during filesystem transactions, never across a driver call.
    file.lock()?;
    Ok(file)
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    struct Directory(PathBuf);
    impl Directory {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            Self(
                crate::diagnostics::directory()
                    .join(format!("sentinel-{}-{stamp:x}-{id}", std::process::id())),
            )
        }
        fn begin(&self, key: &Key, ignore: bool) -> Result<Attempt, String> {
            Attempt::begin_at(&self.0, key, ignore)
        }
        fn stale(&self, key: &Key) {
            let trial = self.begin(key, false).unwrap();
            // Model OS cleanup on process death: close the lock, leave the file.
            let marker = trial.marker.lock().unwrap().take().unwrap();
            drop(marker);
        }
    }
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn adapter(driver: &str) -> Key {
        adapter_backend(driver, wgpu::Backend::Vulkan)
    }
    fn adapter_backend(driver: &str, backend: wgpu::Backend) -> Key {
        let mut info = wgpu::AdapterInfo::new(wgpu::DeviceType::IntegratedGpu, backend);
        info.vendor = 0x8086;
        info.device = 0x1234;
        info.driver = "Intel".into();
        info.driver_info = driver.into();
        Key::adapter(&info)
    }

    #[test]
    fn stale_marker_skips_only_the_faulted_backend() {
        let directory = Directory::new();
        let key = adapter("old driver");
        directory.stale(&key);
        assert!(directory.begin(&key, false).is_err());
        assert!(
            directory
                .begin(&adapter_backend("old driver", wgpu::Backend::Dx12), false)
                .is_ok()
        );
    }

    #[test]
    fn driver_update_clears_the_effective_denial() {
        let directory = Directory::new();
        directory.stale(&adapter("old driver"));
        assert!(directory.begin(&adapter("updated driver"), false).is_ok());
        assert!(directory.begin(&adapter("old driver"), false).is_err());
    }

    #[test]
    fn live_pid_marker_is_ignored_and_success_clears_only_its_own_attempt() {
        let directory = Directory::new();
        let key = adapter("driver");
        let live = directory.begin(&key, false).unwrap();
        let concurrent = directory.begin(&key, false).unwrap();
        concurrent.finish();
        assert!(directory.begin(&key, false).is_ok());
        // A different process can die while this successful one remains alive.
        directory.stale(&key);
        live.finish();
        assert!(directory.begin(&key, false).is_err());
    }

    #[test]
    fn all_denied_backends_leave_the_cpu_fallback() {
        let directory = Directory::new();
        let keys = [
            Key::enumeration(wgpu::Backends::VULKAN),
            Key::enumeration(wgpu::Backends::DX12),
        ];
        for key in &keys {
            directory.stale(key);
        }
        let gpu =
            super::super::try_backends(wgpu::Backends::VULKAN | wgpu::Backends::DX12, |backend| {
                directory.begin(&Key::enumeration(backend), false)
            });
        assert!(
            gpu.is_err(),
            "no GPU candidate: the host keeps its CPU presenter"
        );
    }

    #[test]
    fn override_env_policy_ignores_without_erasing_denials() {
        let directory = Directory::new();
        let key = adapter("driver");
        directory.stale(&key);
        for value in [None, Some("0"), Some("")] {
            assert!(directory.begin(&key, ignore_denials(value)).is_err());
        }
        assert!(directory.begin(&key, ignore_denials(Some("1"))).is_ok());
        assert!(directory.begin(&key, false).is_err());
    }
}
