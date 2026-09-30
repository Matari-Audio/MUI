//! Segment cache: a render cut into fixed spans, each encoded on its own
//! (so it starts on a key frame and no frame refers across it) and kept
//! under `.mui-cut-cache/<variant>/` by a hash of everything that makes its
//! pixels. A re-render encodes only the spans whose hash changed and splices
//! the chunks with ffmpeg's concat demuxer, `-c copy`.
use std::io::Write as _;
use std::path::{Path, PathBuf};

use mui_cut::{Frame, Kind, Project};

pub use mui_cut::Fnv;

/// The key every span of one render shares: the encode settings, the
/// renderer and the bytes of every image and font the project draws.
pub fn base_key(p: &Project, project: &Path, settings: &str) -> Fnv {
    let mut h = Fnv::default();
    let _ = write!(h, "{}|{settings}|", env!("CARGO_PKG_VERSION"));
    let dir = project.parent().unwrap_or(Path::new("."));
    for s in &p.scenes {
        for l in &s.layers {
            let file = match &l.kind {
                Kind::Image { path } => Some(path.as_str()),
                Kind::Text { .. } => p.asset_of(l),
                _ => None,
            };
            if let Some(path) = file {
                let _ = write!(h, "{path}:");
                let _ = h.write_all(&std::fs::read(dir.join(path)).unwrap_or_default());
            }
            // A plugin's states name what it was told, not what it drew:
            // its manifests (content-named images, the adapter's stamp) do.
            let last = mui_cut::plugin::frame_at(s.duration, p.fps);
            for step in l.plugin_track(p.fps, p.sample_rate, last) {
                let manifest = dir
                    .join(mui_cut::plugin::CACHE)
                    .join(format!("{}.json", step.key));
                let _ = h.write_all(&std::fs::read(manifest).unwrap_or_default());
            }
        }
    }
    h
}

/// One span's key: the shared key and every subframe it draws.
pub fn span_key(base: &Fnv, frames: &[Vec<Frame>]) -> String {
    let mut h = Fnv(base.0);
    for subs in frames {
        let _ = serde_json::to_writer(&mut h, subs);
    }
    h.hex()
}

/// `3.2s-5.0s` (or `3.2-5`): output seconds to force.
pub fn range(s: &str) -> Result<(f64, f64), String> {
    let bad = || format!("--range: `{s}` is not START-END seconds, e.g. 3.2s-5.0s");
    let (a, b) = s.split_once('-').ok_or_else(bad)?;
    let n = |v: &str| {
        v.trim()
            .trim_end_matches('s')
            .parse::<f64>()
            .map_err(|_| bad())
    };
    let (a, b) = (n(a)?, n(b)?);
    if a.is_finite() && b.is_finite() && a <= b {
        Ok((a, b))
    } else {
        Err(bad())
    }
}

/// Where a project's chunks for `variant` live.
pub fn dir(project: &Path, variant: Option<&str>) -> PathBuf {
    project
        .parent()
        .unwrap_or(Path::new("."))
        .join(".mui-cut-cache")
        .join(variant.unwrap_or("default"))
}

/// The concat demuxer's list: one `file` line per chunk.
pub fn list(chunks: &[PathBuf]) -> String {
    chunks
        .iter()
        .map(|c| {
            format!(
                "file '{}'\n",
                c.display().to_string().replace('\'', "'\\''")
            )
        })
        .collect()
}
