//! Per-driver crash sentinel for automatic GPU → CPU fallback.
//!
//! Before risky GPU initialization or first presentation on a specific driver
//! (`DriverId`), callers enter a [`SentinelGuard`]. Normal return and caught
//! Rust unwinds drop the guard cleanly, deleting the `.inflight` marker.
//!
//! If the process terminates abruptly inside the GPU driver (SIGSEGV, SIGABRT,
//! driver hang killed by OS), the OS automatically releases the advisory file
//! lock on the `.inflight` marker. On the next launch, [`CrashSentinel::is_tripped`]
//! detects the abandoned marker, promotes it to `.tripped`, and directs the host
//! straight to CPU rendering for that exact driver version. Updating the driver
//! changes its [`DriverId`] fingerprint and automatically re-enables GPU probing.

use std::fmt;
#[cfg(not(target_arch = "wasm32"))]
use std::fs::{self, File, OpenOptions};
use std::io;
#[cfg(not(target_arch = "wasm32"))]
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
#[cfg(not(target_arch = "wasm32"))]
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(not(target_arch = "wasm32"))]
static NEXT_MARKER: AtomicU64 = AtomicU64::new(0);

/// Identifies a specific GPU adapter + driver version for crash isolation.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct DriverId {
    /// Graphics API name (e.g. `"vulkan"`, `"metal"`, `"d3d11"`).
    pub backend: String,
    /// PCI vendor ID (0 if unknown).
    pub vendor: u32,
    /// PCI device ID (0 if unknown).
    pub device: u32,
    /// Driver name/version string (changing this clears prior trip records).
    pub driver: String,
}

impl DriverId {
    /// Creates a new driver identifier.
    #[must_use]
    pub fn new(
        backend: impl Into<String>,
        vendor: u32,
        device: u32,
        driver: impl Into<String>,
    ) -> Self {
        Self {
            backend: backend.into(),
            vendor,
            device,
            driver: driver.into(),
        }
    }

    /// Stable, filesystem-safe hex fingerprint uniquely identifying this driver.
    #[must_use]
    pub fn fingerprint(&self) -> String {
        // 128-bit FNV-1a over length-delimited fields: zero external hash deps.
        let mut h1: u64 = 0xcbf2_9ce4_8422_2325;
        let mut h2: u64 = 0x8422_2325_cbf2_9ce4;
        let mut feed = |bytes: &[u8]| {
            let len = (bytes.len() as u64).to_le_bytes();
            for &b in len.iter().chain(bytes) {
                h1 ^= u64::from(b);
                h1 = h1.wrapping_mul(0x0000_0100_0000_01b3);
                h2 ^= u64::from(b.rotate_left(3));
                h2 = h2.wrapping_mul(0x0000_0100_0000_01b3);
            }
        };
        feed(self.backend.as_bytes());
        feed(&self.vendor.to_le_bytes());
        feed(&self.device.to_le_bytes());
        feed(self.driver.as_bytes());
        format!("{h1:016x}{h2:016x}")
    }
}

impl fmt::Display for DriverId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{:#06x}:{:#06x}:{}",
            self.backend, self.vendor, self.device, self.driver
        )
    }
}

/// Manages per-driver crash sentinels on disk.
#[derive(Clone, Debug)]
pub struct CrashSentinel {
    dir: PathBuf,
}

/// Active RAII guard around a risky GPU driver call.
///
/// Dropping this guard marks the operation complete and removes the `.inflight`
/// file. Only an abrupt native crash skips `Drop`, leaving an unlocked marker
/// that trips the sentinel on the next run.
#[must_use = "dropping SentinelGuard immediately marks the GPU operation complete"]
pub struct SentinelGuard {
    #[cfg(not(target_arch = "wasm32"))]
    file: File,
    #[cfg(not(target_arch = "wasm32"))]
    path: PathBuf,
}

impl CrashSentinel {
    /// Opens a crash sentinel store in `dir`.
    #[must_use]
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    /// Opens the platform-default sentinel directory (overridable via `MUI_SENTINEL_DIR`).
    #[must_use]
    pub fn default_store() -> Self {
        Self::new(default_sentinel_dir())
    }

    /// Directory where sentinel files are stored.
    #[must_use]
    pub fn directory(&self) -> &Path {
        &self.dir
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn tripped_path(&self, driver: &DriverId) -> PathBuf {
        self.dir.join(format!("{}.tripped", driver.fingerprint()))
    }

    /// Returns `true` if a previous run crashed during GPU init/present on `driver`.
    ///
    /// Also sweeps any abandoned (unlocked) `.inflight` markers matching `driver`
    /// and promotes them into a durable `.tripped` record.
    #[cfg(not(target_arch = "wasm32"))]
    #[must_use]
    pub fn is_tripped(&self, driver: &DriverId) -> bool {
        let tripped = self.tripped_path(driver);
        if tripped.exists() {
            return true;
        }
        if let Some(reason) = self.sweep_abandoned(driver) {
            let _ = self.trip(driver, &reason);
            return true;
        }
        tripped.exists()
    }

    /// Removes every abandoned (unlocked) `.inflight` marker for `driver`.
    ///
    /// Returns the recorded stage of one marker whose operation never
    /// completed, or `None` when every abandoned marker had completed. Markers
    /// still locked by a live process are left alone.
    #[cfg(not(target_arch = "wasm32"))]
    fn sweep_abandoned(&self, driver: &DriverId) -> Option<String> {
        let prefix = format!("{}-", driver.fingerprint());
        let entries = fs::read_dir(&self.dir).ok()?;
        let mut crash = None;
        for path in entries
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| {
                p.extension().is_some_and(|e| e == "inflight")
                    && p.file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.starts_with(&prefix))
            })
        {
            let Ok(mut file) = OpenOptions::new().read(true).write(true).open(&path) else {
                continue;
            };
            if file.try_lock().is_err() {
                // Another live process is currently inside a guarded section.
                continue;
            }
            let mut status = [0u8; 1];
            let completed = file.read_exact(&mut status).is_ok() && status[0] == 1;
            if !completed {
                let mut reason = String::new();
                let _ = file.read_to_string(&mut reason);
                let reason = reason.trim();
                crash = Some(if reason.is_empty() {
                    "abrupt process termination during GPU operation".to_owned()
                } else {
                    reason.to_owned()
                });
            }
            let _ = fs::remove_file(&path);
        }
        crash
    }

    /// Always `false` on WebAssembly (no native driver crash persistence).
    #[cfg(target_arch = "wasm32")]
    #[must_use]
    pub fn is_tripped(&self, _driver: &DriverId) -> bool {
        false
    }

    /// Enters a guarded GPU section for `driver` at `stage`.
    ///
    /// # Errors
    /// Returns an I/O error if the sentinel directory or marker file cannot be created.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn enter(&self, driver: &DriverId, stage: &str) -> io::Result<SentinelGuard> {
        fs::create_dir_all(&self.dir)?;
        let prefix = format!("{}-{}", driver.fingerprint(), std::process::id());
        loop {
            let id = NEXT_MARKER.fetch_add(1, Ordering::Relaxed);
            let path = self.dir.join(format!("{prefix}-{id}.inflight"));
            if path.exists() {
                // The OS can reuse a PID after a crash. Keep the old marker.
                continue;
            }
            let staging = path.with_extension("pending");
            let file = match OpenOptions::new()
                .create_new(true)
                .read(true)
                .write(true)
                .open(&staging)
            {
                Ok(file) => file,
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            };
            let mut guard = SentinelGuard {
                file,
                path: staging,
            };
            guard.file.try_lock().map_err(io::Error::other)?;
            guard.file.write_all(&[0u8])?;
            writeln!(guard.file, "{stage} ({driver})")?;
            guard.file.flush()?;
            // A scanner can see the marker only after it is locked and initialized.
            fs::rename(&guard.path, &path)?;
            guard.path = path;
            return Ok(guard);
        }
    }

    /// No-op guard on WebAssembly.
    #[cfg(target_arch = "wasm32")]
    pub fn enter(&self, _driver: &DriverId, _stage: &str) -> io::Result<SentinelGuard> {
        Ok(SentinelGuard {})
    }

    /// Explicitly marks `driver` as tripped so future sessions default to CPU.
    ///
    /// # Errors
    /// Returns an I/O error if writing the tripped marker fails.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn trip(&self, driver: &DriverId, reason: &str) -> io::Result<()> {
        fs::create_dir_all(&self.dir)?;
        let path = self.tripped_path(driver);
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(path)?;
        writeln!(file, "driver={driver}\nreason={reason}")?;
        file.flush()
    }

    /// No-op on WebAssembly.
    #[cfg(target_arch = "wasm32")]
    pub fn trip(&self, _driver: &DriverId, _reason: &str) -> io::Result<()> {
        Ok(())
    }

    /// Clears any tripped record for `driver` (e.g. after user requests GPU retry).
    ///
    /// Also discards abandoned markers left behind while the record existed,
    /// so they cannot re-trip the driver on the next check.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn clear(&self, driver: &DriverId) {
        let _ = self.sweep_abandoned(driver);
        let _ = fs::remove_file(self.tripped_path(driver));
    }

    /// No-op on WebAssembly.
    #[cfg(target_arch = "wasm32")]
    pub fn clear(&self, _driver: &DriverId) {}
}

#[cfg(not(target_arch = "wasm32"))]
impl Drop for SentinelGuard {
    fn drop(&mut self) {
        // Mark completion byte while still holding the advisory lock before unlinking.
        let _ = self
            .file
            .seek(SeekFrom::Start(0))
            .and_then(|_| self.file.write_all(&[1u8]))
            .and_then(|_| self.file.flush());
        let _ = fs::remove_file(&self.path);
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn default_sentinel_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("MUI_SENTINEL_DIR").filter(|v| !v.is_empty()) {
        return PathBuf::from(dir);
    }
    if cfg!(test) {
        return std::env::temp_dir().join(format!("mui-sentinels-test-{}", std::process::id()));
    }
    let root = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|v| PathBuf::from(v).join("Library/Caches"))
    } else {
        std::env::var_os("XDG_STATE_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|v| PathBuf::from(v).join(".local/state")))
    };
    root.unwrap_or_else(std::env::temp_dir)
        .join("MUI")
        .join("gpu-sentinels")
}

#[cfg(target_arch = "wasm32")]
fn default_sentinel_dir() -> PathBuf {
    PathBuf::new()
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn clean_exit_does_not_trip_sentinel_but_hard_crash_trips_exact_driver() {
        const CRASH_DIR_ENV: &str = "MUI_PRESENT_TEST_SENTINEL_CRASH_DIR";
        let driver_v1 = DriverId::new("vulkan", 0x10de, 0x2684, "550.54.14");
        let driver_v2 = DriverId::new("vulkan", 0x10de, 0x2684, "555.42.02");

        if let Some(dir) = std::env::var_os(CRASH_DIR_ENV) {
            let sentinel = CrashSentinel::new(PathBuf::from(dir));
            let _guard = sentinel
                .enter(&driver_v1, "vk_create_swapchain")
                .expect("enter sentinel");
            // Simulate a hard native driver crash (no Drop runs, OS drops lock).
            std::process::exit(42);
        }

        let dir = std::env::temp_dir().join(format!("mui-present-sentinel-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let sentinel = CrashSentinel::new(dir.clone());

        assert!(!sentinel.is_tripped(&driver_v1));
        {
            let first = sentinel.enter(&driver_v1, "init").expect("enter");
            let second = sentinel.enter(&driver_v1, "present").expect("enter again");
            assert_ne!(
                first.path, second.path,
                "live operations need distinct markers"
            );
            assert!(!sentinel.is_tripped(&driver_v1));
            drop(first);
            assert!(
                second.path.exists(),
                "first operation must leave second marker intact"
            );
            assert!(!sentinel.is_tripped(&driver_v1));
        }
        assert!(!sentinel.is_tripped(&driver_v1));

        // Spawn child that exits abruptly while holding the sentinel.
        let status = std::process::Command::new(std::env::current_exe().expect("test binary"))
            .args([
                "--exact",
                "sentinel::tests::clean_exit_does_not_trip_sentinel_but_hard_crash_trips_exact_driver",
            ])
            .env(CRASH_DIR_ENV, &dir)
            .status()
            .expect("spawn crash child");
        assert_eq!(status.code(), Some(42));

        // Abandoned lock is detected and trips v1, but NOT upgraded driver v2.
        assert!(sentinel.is_tripped(&driver_v1));
        assert!(!sentinel.is_tripped(&driver_v2));

        // Clearing resets v1 cleanly.
        sentinel.clear(&driver_v1);
        assert!(!sentinel.is_tripped(&driver_v1));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn clear_discards_markers_abandoned_while_tripped() {
        let driver = DriverId::new("vulkan", 1, 2, "3");
        let dir = std::env::temp_dir().join(format!("mui-present-clear-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let sentinel = CrashSentinel::new(dir.clone());
        sentinel.trip(&driver, "earlier crash").expect("trip");
        // An unlocked, never-completed marker that `is_tripped` skips because
        // the trip record already exists.
        let marker = dir.join(format!("{}-0-0.inflight", driver.fingerprint()));
        fs::write(&marker, b"\0init").expect("write marker");
        assert!(sentinel.is_tripped(&driver));

        sentinel.clear(&driver);
        assert!(!marker.exists());
        assert!(!sentinel.is_tripped(&driver));
        let _ = fs::remove_dir_all(&dir);
    }
}
