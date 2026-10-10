//! Disk I/O belongs to the initializer, never to a steady frame or window teardown.
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

pub(crate) struct Cache {
    pub cache: wgpu::PipelineCache,
    path: Option<PathBuf>,
}

fn directory() -> Option<PathBuf> {
    if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|p| PathBuf::from(p).join("Library/Caches"))
    } else {
        std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".cache")))
    }
    .map(|p| p.join("mui/pipelines"))
}

fn checksum(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}
fn decode(bytes: &[u8]) -> Option<&[u8]> {
    let (header, data) = bytes.split_at_checked(8)?;
    (checksum(data) == u64::from_le_bytes(header.try_into().ok()?)).then_some(data)
}

pub(crate) fn load(device: &wgpu::Device) -> Option<Cache> {
    if !device.features().contains(wgpu::Features::PIPELINE_CACHE) {
        return None;
    }
    let key = wgpu::util::pipeline_cache_key(&device.adapter_info())?;
    let path = directory().map(|p| {
        p.join(format!(
            "{key}-wgpu30-vello0.10-mui{}.bin",
            env!("CARGO_PKG_VERSION")
        ))
    });
    // Cache files are private application data, not an import API. Reject truncated
    // or accidentally corrupted files before passing prior get_data bytes to wgpu.
    let bytes = path.as_ref().and_then(|p| {
        let metadata = std::fs::metadata(p).ok()?;
        (metadata.len() <= 64 * 1024 * 1024)
            .then(|| std::fs::read(p).ok())
            .flatten()
    });
    let data = bytes.as_deref().and_then(decode);
    // SAFETY: data is our persisted get_data output (checksum-checked), keyed by
    // wgpu's adapter key; fallback accepts stale driver/wgpu cache headers. None
    // starts an empty cache. This directory must not contain untrusted imports.
    #[expect(unsafe_code, reason = "wgpu's persisted pipeline-cache API is unsafe")]
    let cache = unsafe {
        device.create_pipeline_cache(&wgpu::PipelineCacheDescriptor {
            label: Some("MUI Vello pipeline cache"),
            data,
            fallback: true,
        })
    };
    Some(Cache { cache, path })
}

impl Cache {
    pub fn save(self) {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let (Some(path), Some(data)) = (self.path, self.cache.get_data()) else {
            return;
        };
        let Some(parent) = path.parent() else { return };
        let temp = path.with_extension(format!(
            "{}-{}.tmp",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let result = (|| -> std::io::Result<()> {
            std::fs::create_dir_all(parent)?;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)?;
            file.write_all(&checksum(&data).to_le_bytes())?;
            file.write_all(&data)?;
            drop(file);
            std::fs::rename(&temp, &path)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(temp);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn truncated_and_corrupt_cache_data_is_ignored() {
        assert!(decode(&[0; 7]).is_none());
        let data = b"prior driver output";
        let mut bytes = checksum(data).to_le_bytes().to_vec();
        bytes.extend_from_slice(data);
        assert_eq!(decode(&bytes), Some(data.as_slice()));
        bytes[9] ^= 1;
        assert!(decode(&bytes).is_none());
    }
}
