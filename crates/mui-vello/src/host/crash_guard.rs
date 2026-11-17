//! A native fault cannot unwind. Keep a locked, durable trial until the first
//! GPU frame completes. The OS releases its lock if the owning process dies.
//! All I/O is on initialization/render workers, never an audio callback.
//!
//! A process can die mid-trial for reasons that are not the GPU's (a DAW
//! killed or crashing elsewhere, a power cut), so one unfinished trial costs
//! one retry, never the GPU: a key is denied only after unfinished trials in
//! [`STRIKES`] different processes, a trial that completes clears every
//! unfinished one for its key, and markers expire after [`EXPIRY`]. A driver
//! or kernel update changes the key, so it tries the GPU again too. An app
//! can show and clear denials: [`gpu_denials`], [`clear_gpu_denials`].
use std::path::Path;
#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;

#[derive(Clone)]
pub(super) struct Key(String);

/// This build of MUI: its version and source checkout (a git dependency's
/// checkout path names its commit). A trial is denied for one build only, so
/// a crash cannot disable the GPU for every build after it.
const BUILD: &str = concat!(env!("CARGO_PKG_VERSION"), "@", env!("CARGO_MANIFEST_DIR"));

/// Unfinished trials, each in a different process, that deny a key.
pub const STRIKES: usize = 2;
/// An unfinished trial older than this is forgotten.
pub const EXPIRY: std::time::Duration = std::time::Duration::from_secs(7 * 24 * 3600);

/// The running kernel, where it is cheap to know: an update re-arms a key
/// whose driver version the trial never got to read.
fn system() -> &'static str {
    static SYSTEM: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
        #[cfg(target_os = "linux")]
        if let Ok(release) = std::fs::read_to_string("/proc/sys/kernel/osrelease") {
            return release.trim().to_owned();
        }
        std::env::consts::OS.to_owned()
    });
    &SYSTEM
}

impl Key {
    pub(super) fn enumeration(backends: wgpu::Backends) -> Self {
        Self(format!(
            "backend={backends:?};adapter=unknown;driver=unknown;system={};build={BUILD}",
            system()
        ))
    }

    pub(super) fn adapter(info: &wgpu::AdapterInfo) -> Self {
        Self(format!(
            "backend={:?};vendor={:08x};device={:08x};driver[{}]={};driver_info[{}]={};system={};build={BUILD}",
            info.backend,
            info.vendor,
            info.device,
            info.driver.len(),
            info.driver,
            info.driver_info.len(),
            info.driver_info,
            system(),
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
        use std::io::Write as _;
        let attempt = (|| -> std::io::Result<Self> {
            let _registry = registry(directory)?;
            if key.0.len() > 64 * 1024 {
                return Err(std::io::Error::other("GPU identity exceeds 64 KiB"));
            }
            let prefix = prefix(key);
            let dead = unfinished(directory, Some(&prefix), Some(&key.0))?;
            let pids: std::collections::BTreeSet<u32> = dead.iter().map(|d| d.pid).collect();
            if !dead.is_empty() {
                let denied = pids.len() >= STRIKES;
                crate::diagnostics::breadcrumb(
                    "mui-vello",
                    "gpu_denylist",
                    &format!(
                        "{} after unfinished GPU trials in {} process(es); identity={}",
                        match (denied, ignore) {
                            (true, true) => "override ignores a denial",
                            (true, false) => "denied",
                            (false, _) => "retrying",
                        },
                        pids.len(),
                        key.0,
                    ),
                );
                if denied && !ignore {
                    return Err(std::io::Error::other(format!(
                        "GPU disabled after unfinished trials in {} processes \
                         (clear with mui::vello::host::clear_gpu_denials): {}",
                        pids.len(),
                        key.0
                    )));
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

    /// The GPU drew: end this trial and forget the unfinished ones of its key.
    pub(super) fn finish(&self) {
        self.close(true);
    }

    fn close(&self, worked: bool) {
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
                        if let Err(error) = std::fs::remove_file(&marker.path) {
                            crate::diagnostics::error(
                                "mui-vello",
                                "gpu_sentinel_clear",
                                &error.to_string(),
                            );
                        }
                        // The GPU works for this key: the processes that died
                        // in its trials did not die of it.
                        if worked {
                            let prefix = prefix(&self.key);
                            for dead in unfinished(directory, Some(&prefix), Some(&self.key.0))
                                .unwrap_or_default()
                            {
                                let _ = std::fs::remove_file(dead.path);
                            }
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
        // None of them proves the GPU works, so earlier strikes stay.
        self.close(false);
    }
}

#[cfg(not(target_arch = "wasm32"))]
struct Marker {
    path: PathBuf,
    file: std::fs::File,
}

/// The filename index of `key`'s markers. FNV is only an index: a marker's
/// complete identity is compared too, so a collision denies nothing else.
fn prefix(key: &Key) -> String {
    let hash = key.0.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3)
    });
    format!("{hash:016x}-")
}

/// A trial whose process died before it finished.
#[cfg(not(target_arch = "wasm32"))]
struct Unfinished {
    path: PathBuf,
    pid: u32,
    identity: String,
    at: std::time::SystemTime,
}

/// Every unfinished trial in `directory` (named under `prefix`, for
/// `identity`, when given). A marker its process still locks is a trial
/// under way, skipped; an expired one is removed. Call under [`registry`].
#[cfg(not(target_arch = "wasm32"))]
fn unfinished(
    directory: &Path,
    prefix: Option<&str>,
    identity: Option<&str>,
) -> std::io::Result<Vec<Unfinished>> {
    use std::io::Read as _;
    let mut out = Vec::new();
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let Some(rest) = name.strip_suffix(".pending") else {
            continue;
        };
        if prefix.is_some_and(|p| !rest.starts_with(p)) {
            continue;
        }
        // `{hash}-{pid}-{stamp}-{collision}`
        let pid = rest
            .split('-')
            .nth(1)
            .and_then(|p| p.parse().ok())
            .unwrap_or(0);
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
        let at = file.metadata()?.modified()?;
        let mut stored = String::new();
        (&file).take(64 * 1024).read_to_string(&mut stored)?;
        drop(file);
        if at.elapsed().is_ok_and(|age| age > EXPIRY) {
            let _ = std::fs::remove_file(entry.path());
            continue;
        }
        if identity.is_some_and(|i| i != stored) {
            continue;
        }
        out.push(Unfinished {
            path: entry.path(),
            pid,
            identity: stored,
            at,
        });
    }
    Ok(out)
}

/// A GPU key this machine will not try: unfinished trials in
/// [`STRIKES`] or more processes, none finished since.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GpuDenial {
    /// Backend, adapter, driver, system and MUI build.
    pub identity: String,
    /// Processes that died during its trials.
    pub processes: usize,
    /// The last of them.
    pub since: std::time::SystemTime,
}

/// Every GPU key denied on this machine, for a settings page to show. A
/// window presenting on the CPU because of one says so in its reason.
pub fn gpu_denials() -> Vec<GpuDenial> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        denials_at(&crate::diagnostics::directory().join("gpu-trials")).unwrap_or_default()
    }
    #[cfg(target_arch = "wasm32")]
    Vec::new()
}

/// Forget every unfinished GPU trial: the next window tries the GPU again
/// ("try the GPU again" in a settings page). Returns how many were removed.
pub fn clear_gpu_denials() -> std::io::Result<usize> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        clear_at(&crate::diagnostics::directory().join("gpu-trials"))
    }
    #[cfg(target_arch = "wasm32")]
    Ok(0)
}

#[cfg(not(target_arch = "wasm32"))]
fn denials_at(directory: &Path) -> std::io::Result<Vec<GpuDenial>> {
    use std::collections::{BTreeMap, BTreeSet};
    let _registry = registry(directory)?;
    let mut by: BTreeMap<String, (BTreeSet<u32>, std::time::SystemTime)> = BTreeMap::new();
    for d in unfinished(directory, None, None)? {
        let (pids, since) = by.entry(d.identity).or_insert((BTreeSet::new(), d.at));
        pids.insert(d.pid);
        *since = (*since).max(d.at);
    }
    Ok(by
        .into_iter()
        .filter(|(_, (pids, _))| pids.len() >= STRIKES)
        .map(|(identity, (pids, since))| GpuDenial {
            identity,
            processes: pids.len(),
            since,
        })
        .collect())
}

#[cfg(not(target_arch = "wasm32"))]
fn clear_at(directory: &Path) -> std::io::Result<usize> {
    let _registry = registry(directory)?;
    let dead = unfinished(directory, None, None)?;
    let n = dead.len();
    for d in dead {
        std::fs::remove_file(d.path)?;
    }
    Ok(n)
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
        /// A trial whose process died: the lock is gone, the file stays.
        /// Each call models a different process.
        fn stale(&self, key: &Key) -> PathBuf {
            static PID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);
            let trial = self.begin(key, false).unwrap();
            let marker = trial.marker.lock().unwrap().take().unwrap();
            drop(marker.file);
            let name = marker.path.file_name().unwrap().to_string_lossy().replacen(
                &format!("-{}-", std::process::id()),
                &format!(
                    "-{}-",
                    PID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                ),
                1,
            );
            let path = self.0.join(name);
            std::fs::rename(&marker.path, &path).unwrap();
            path
        }
        /// Unfinished trials in [`STRIKES`] processes.
        fn deny(&self, key: &Key) {
            for _ in 0..STRIKES {
                self.stale(key);
            }
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
    fn one_dead_trial_costs_a_retry_and_two_deny_only_that_backend() {
        let directory = Directory::new();
        let key = adapter("old driver");
        directory.stale(&key);
        drop(
            directory
                .begin(&key, false)
                .expect("one death is one retry"),
        );
        directory.stale(&key);
        assert!(directory.begin(&key, false).is_err());
        assert!(
            directory
                .begin(&adapter_backend("old driver", wgpu::Backend::Dx12), false)
                .is_ok()
        );
        assert_eq!(gpu_denials_in(&directory).len(), 1);
    }

    fn gpu_denials_in(directory: &Directory) -> Vec<GpuDenial> {
        denials_at(&directory.0).unwrap()
    }

    #[test]
    fn driver_update_clears_the_effective_denial() {
        let directory = Directory::new();
        directory.deny(&adapter("old driver"));
        assert!(directory.begin(&adapter("updated driver"), false).is_ok());
        assert!(directory.begin(&adapter("old driver"), false).is_err());
    }

    #[test]
    fn a_new_build_retries_a_denied_key_and_crashes_there_deny_it_again() {
        let directory = Directory::new();
        let on = |build: &str| Key(format!("backend=Vulkan;adapter=unknown;build={build}"));
        directory.deny(&on("old"));
        assert!(directory.begin(&on("old"), false).is_err());
        let retry = directory.begin(&on("new"), false);
        assert!(retry.is_ok(), "a new build tries the GPU again");
        drop(retry);
        directory.deny(&on("new"));
        assert!(directory.begin(&on("new"), false).is_err());
    }

    #[test]
    fn live_trials_are_not_strikes_and_success_clears_dead_ones() {
        let directory = Directory::new();
        let key = adapter("driver");
        let live = directory.begin(&key, false).unwrap();
        let concurrent = directory.begin(&key, false).unwrap();
        concurrent.finish();
        assert!(directory.begin(&key, false).is_ok());
        // Other processes die while this one is still in its trial.
        directory.deny(&key);
        assert!(directory.begin(&key, false).is_err());
        live.finish();
        assert!(gpu_denials_in(&directory).is_empty());
        assert!(directory.begin(&key, false).is_ok());
    }

    #[test]
    fn expired_trials_are_forgotten_and_clear_resets_the_rest() {
        let directory = Directory::new();
        let key = adapter("driver");
        let old = directory.stale(&key);
        std::fs::File::options()
            .write(true)
            .open(&old)
            .unwrap()
            .set_modified(std::time::SystemTime::now() - EXPIRY - EXPIRY)
            .unwrap();
        directory.stale(&key);
        assert!(
            directory.begin(&key, false).is_ok(),
            "the old strike expired"
        );
        assert!(!old.exists());
        directory.stale(&key);
        assert!(directory.begin(&key, false).is_err());
        assert_eq!(clear_at(&directory.0).unwrap(), 2);
        assert!(directory.begin(&key, false).is_ok());
    }

    #[test]
    fn all_denied_backends_leave_the_cpu_fallback() {
        let directory = Directory::new();
        let keys = [
            Key::enumeration(wgpu::Backends::VULKAN),
            Key::enumeration(wgpu::Backends::DX12),
        ];
        for key in &keys {
            directory.deny(key);
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
        directory.deny(&key);
        for value in [None, Some("0"), Some("")] {
            assert!(directory.begin(&key, ignore_denials(value)).is_err());
        }
        drop(directory.begin(&key, ignore_denials(Some("1"))).unwrap());
        assert!(directory.begin(&key, false).is_err());
    }
}
