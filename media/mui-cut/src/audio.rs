//! The soundtrack: every plugin layer's rendered notes and every audio
//! layer's file, each at its `volume`, scenes back to back, as stereo f32 at
//! the project's sample rate. `render` muxes it into the video; `serve`
//! plays the same mix (`src/live.rs`).
use std::collections::HashMap;
use std::path::Path;
use std::process::{Command, Stdio};

use mui_cut::{Kind, Layer, Project, Scene};

use crate::Result;

/// Volume and time are read once a block this long (samples).
const BLOCK: usize = 64;

/// Decoded audio files, stereo f32 at one rate, by path.
#[derive(Default)]
pub struct Files(HashMap<String, std::sync::Arc<Vec<f32>>>);

impl Files {
    /// `path` (relative to the project) decoded by ffmpeg to stereo f32
    /// at `rate`.
    pub fn get(
        &mut self,
        project: &Path,
        path: &str,
        rate: u32,
    ) -> Result<std::sync::Arc<Vec<f32>>> {
        if let Some(a) = self.0.get(path) {
            return Ok(a.clone());
        }
        let file = project.parent().unwrap_or(Path::new(".")).join(path);
        let out = Command::new("ffmpeg")
            .args(["-v", "error", "-i"])
            .arg(&file)
            .args(["-f", "f32le", "-ac", "2", "-ar", &rate.to_string(), "-"])
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| format!("ffmpeg: {e} (is it on PATH?)"))?;
        if !out.status.success() {
            return Err(format!(
                "`{path}`: ffmpeg could not decode it: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        let pcm: Vec<f32> = out
            .stdout
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect();
        let a = std::sync::Arc::new(pcm);
        self.0.insert(path.to_owned(), a.clone());
        Ok(a)
    }
}

/// Whether anything in `scenes` makes a sound.
pub fn sounds(scenes: &[&Scene]) -> bool {
    scenes
        .iter()
        .flat_map(|s| &s.layers)
        .any(|l| match &l.kind {
            Kind::Plugin { notes, .. } => !notes.is_empty(),
            Kind::Audio { .. } => true,
            _ => false,
        })
}

/// Add `src` (stereo, sample 0 at the scene's start) into `out` from scene
/// sample `from`, at the layer's volume.
pub fn add(
    out: &mut [f32],
    from: usize,
    rate: u32,
    l: &Layer,
    src: impl Fn(usize, f64) -> [f32; 2],
) {
    let frames = out.len() / 2;
    for b in (0..frames).step_by(BLOCK) {
        let t = (from + b) as f64 / f64::from(rate);
        let gain = l.volume.at(t).max(0.) as f32;
        if gain == 0. {
            continue;
        }
        for i in b..(b + BLOCK).min(frames) {
            let [a, c] = src(from + i, t);
            out[2 * i] += a * gain;
            out[2 * i + 1] += c * gain;
        }
    }
}

/// The frame of an audio layer's file sounding at scene sample `n`
/// (`t` its block's time): `time` seconds in at the scene's start.
pub fn file_at(pcm: &[f32], l: &Layer, rate: u32, n: usize, t: f64) -> [f32; 2] {
    let start = l.time.at(t) * f64::from(rate);
    let i = start.round() as i64 + n as i64;
    match usize::try_from(i) {
        Ok(i) if 2 * i + 1 < pcm.len() => [pcm[2 * i], pcm[2 * i + 1]],
        _ => [0., 0.],
    }
}

/// The mix of `scenes` back to back, `None` when nothing sounds. Plugin
/// soundtracks must be rendered ([`crate::host::capture_missing`]).
pub fn mix(p: &Project, project: &Path, scenes: &[&Scene]) -> Result<Option<Vec<f32>>> {
    if !sounds(scenes) {
        return Ok(None);
    }
    let rate = p.sample_rate;
    let mut files = Files::default();
    let mut out = Vec::new();
    for s in scenes {
        let mut part = vec![0f32; 2 * p.samples(s) as usize];
        for l in &s.layers {
            match &l.kind {
                Kind::Plugin { .. } => {
                    if let Some(pcm) = crate::host::layer_audio(p, project, s, l)? {
                        add(&mut part, 0, rate, l, |n, _| {
                            [
                                pcm.get(2 * n).copied().unwrap_or(0.),
                                pcm.get(2 * n + 1).copied().unwrap_or(0.),
                            ]
                        });
                    }
                }
                Kind::Audio { path } => {
                    let pcm = files.get(project, path, rate)?;
                    add(&mut part, 0, rate, l, |n, t| file_at(&pcm, l, rate, n, t));
                }
                _ => {}
            }
        }
        out.extend(part);
    }
    for v in &mut out {
        *v = v.clamp(-1., 1.);
    }
    Ok(Some(out))
}

/// Stereo f32 as a WAV file (IEEE float).
pub fn wav(pcm: &[f32], rate: u32) -> Vec<u8> {
    let data = (pcm.len() * 4) as u32;
    let mut b = Vec::with_capacity(44 + pcm.len() * 4);
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&3u16.to_le_bytes()); // IEEE float
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&rate.to_le_bytes());
    b.extend_from_slice(&(rate * 8).to_le_bytes());
    b.extend_from_slice(&8u16.to_le_bytes());
    b.extend_from_slice(&32u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data.to_le_bytes());
    for v in pcm {
        b.extend_from_slice(&v.to_le_bytes());
    }
    b
}

/// Put `pcm` into the finished video `out` as an AAC track, the video
/// stream copied as it is.
pub fn mux(plan: &crate::encode::Plan, out: &str, pcm: &[f32], rate: u32) -> Result<()> {
    let wav_file = format!("{out}.mix.wav");
    let tmp = format!("{out}.mux.tmp");
    std::fs::write(&wav_file, wav(pcm, rate)).map_err(|e| format!("{wav_file}: {e}"))?;
    let args: Vec<String> = ["-i", out, "-i", &wav_file]
        .into_iter()
        .chain([
            "-map", "0:v", "-map", "1:a", "-c:v", "copy", "-c:a", "aac", "-b:a", "256k",
        ])
        .map(String::from)
        .chain(plan.mux())
        .chain([tmp.clone()])
        .collect();
    let ok = crate::ffmpeg(&args)
        .status()
        .map_err(|e| format!("ffmpeg: {e}"))?;
    let _ = std::fs::remove_file(&wav_file);
    if !ok.success() {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("ffmpeg failed muxing the soundtrack into {out}"));
    }
    std::fs::rename(&tmp, out).map_err(|e| format!("{out}: {e}"))
}

/// `scene` of `project` rendered with its sound (this binary's `render`),
/// then cut to `[from, to]` seconds into `out`.
// ponytail: renders the whole scene for a slice; render only the span's
// frames if long scenes make previews slow.
pub fn clip(project: &Path, scene: &str, from: f64, to: f64, out: &Path) -> Result<()> {
    let full = out.with_extension("full.mp4");
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let o = Command::new(exe)
        .arg("render")
        .arg(project)
        .args(["--scene", scene, "-o"])
        .arg(&full)
        .stdout(Stdio::null())
        .output()
        .map_err(|e| e.to_string())?;
    if !o.status.success() {
        return Err(String::from_utf8_lossy(&o.stderr).into_owned());
    }
    let args: Vec<String> = [
        "-ss".into(),
        from.to_string(),
        "-to".into(),
        to.to_string(),
        "-i".into(),
    ]
    .into_iter()
    .chain([full.display().to_string()])
    .chain(
        [
            "-c:v", "libx264", "-crf", "20", "-c:a", "aac", "-b:a", "256k",
        ]
        .map(String::from),
    )
    .chain([out.display().to_string()])
    .collect();
    let ok = crate::ffmpeg(&args)
        .status()
        .map_err(|e| format!("ffmpeg: {e}"))?;
    let _ = std::fs::remove_file(&full);
    if ok.success() {
        Ok(())
    } else {
        Err(format!("ffmpeg failed cutting {}", out.display()))
    }
}
