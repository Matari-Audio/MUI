//! Which ffmpeg encoder a render uses, and its arguments. Frames arrive as
//! raw 4:2:0 planes (NV12, or P010 for 10-bit), BT.709 limited range and
//! tagged so, whichever encoder takes them: VAAPI uploads them as they are,
//! software encoders at most repack the chroma.
use std::process::{Command, Stdio};

use mui_cut::Render;
use mui_cut::yuv::Yuv;

/// The render node VAAPI encodes on.
pub const VAAPI_DEVICE: &str = "/dev/dri/renderD128";

/// A resolved encode: encoder, pixels, container and quality.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub codec: &'static str,
    pub encoder: &'static str,
    pub vaapi: bool,
    pub yuv: Yuv,
    pub container: &'static str,
    pub settings: Render,
}

const CODECS: [(&str, &str, &str); 3] = [
    ("h264", "libx264", "h264_vaapi"),
    ("h265", "libx265", "hevc_vaapi"),
    ("av1", "libsvtav1", "av1_vaapi"),
];

/// Pick the encoder for `r` writing `out`: `auto` tries VAAPI with a tiny
/// trial encode (the codec, the bit depth, the device) and falls back to
/// software; `vaapi` fails if the trial does.
pub fn plan(r: &Render, out: &str) -> Result<Plan, String> {
    r.check()?;
    let codec = r.codec.as_deref().unwrap_or("h264");
    let &(codec, soft, hard) = CODECS
        .iter()
        .find(|c| c.0 == codec)
        .ok_or("unknown codec")?;
    let yuv = match r.pix_fmt.as_deref() {
        Some("yuv420p10le") => Yuv::P010,
        _ => Yuv::Nv12,
    };
    let ext = out.rsplit_once('.').map(|(_, e)| e);
    let container = match r.container.as_deref().or(ext) {
        Some("mkv") => "mkv",
        Some("mov") => "mov",
        _ => "mp4",
    };
    if codec == "av1" && container == "mov" {
        return Err("av1 does not go in mov: use mp4 or mkv".into());
    }
    let vaapi = match r.encoder.as_deref().unwrap_or("auto") {
        "vaapi" if trial(hard, yuv) => true,
        "vaapi" => {
            return Err(format!(
                "{hard} does not encode {} on {VAAPI_DEVICE}",
                yuv.pix_fmt()
            ));
        }
        "auto" => trial(hard, yuv),
        _ => false,
    };
    Ok(Plan {
        codec,
        encoder: if vaapi { hard } else { soft },
        vaapi,
        yuv,
        container,
        settings: r.clone(),
    })
}

/// A 0.1 s encode of black on the VAAPI device: does this exact setup work?
fn trial(encoder: &str, yuv: Yuv) -> bool {
    if !std::path::Path::new(VAAPI_DEVICE).exists() {
        return false;
    }
    let format = match yuv {
        Yuv::Nv12 => "nv12",
        Yuv::P010 => "p010",
    };
    let mut c = Command::new("ffmpeg");
    c.args([
        "-hide_banner",
        "-loglevel",
        "error",
        "-vaapi_device",
        VAAPI_DEVICE,
    ])
    .args(["-f", "lavfi", "-i", "color=black:s=256x144:d=0.1"])
    .args(["-vf", &format!("format={format},hwupload"), "-c:v", encoder]);
    if yuv == Yuv::P010 && encoder == "hevc_vaapi" {
        c.args(["-profile:v", "main10"]);
    }
    c.args(["-f", "null", "-"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

const TAGS: [&str; 8] = [
    "-color_range",
    "tv",
    "-colorspace",
    "bt709",
    "-color_primaries",
    "bt709",
    "-color_trc",
    "bt709",
];

impl Plan {
    /// Up to and including `-i -`: raw planes of `w` x `h` on stdin.
    pub fn input(&self, (w, h): (u16, u16), fps: f64) -> Vec<String> {
        let mut a: Vec<String> = Vec::new();
        if self.vaapi {
            a.extend(["-vaapi_device".into(), VAAPI_DEVICE.into()]);
        }
        a.extend(
            ["-f", "rawvideo", "-pix_fmt", self.yuv.pix_fmt()]
                .map(String::from)
                .into_iter()
                .chain([
                    "-s".into(),
                    format!("{w}x{h}"),
                    "-r".into(),
                    fps.to_string(),
                ])
                .chain(TAGS.map(String::from))
                .chain(["-i".into(), "-".into()]),
        );
        a
    }

    /// The encoder, its quality, the tags and the container, before the
    /// output path.
    pub fn output(&self) -> Vec<String> {
        let s = &self.settings;
        let mut a: Vec<String> = Vec::new();
        let mut push = |v: &[&str]| a.extend(v.iter().map(|s| (*s).to_owned()));
        if self.vaapi {
            let format = match self.yuv {
                Yuv::Nv12 => "nv12",
                Yuv::P010 => "p010",
            };
            push(&["-vf", &format!("format={format},hwupload")]);
        }
        push(&["-c:v", self.encoder]);
        if self.vaapi && self.yuv == Yuv::P010 && self.codec == "h265" {
            push(&["-profile:v", "main10"]);
        }
        if !self.vaapi {
            push(&["-pix_fmt", s.pix_fmt.as_deref().unwrap_or("yuv420p")]);
        }
        if let Some(b) = &s.bitrate {
            push(&["-b:v", b]);
            if let Some(m) = &s.maxrate {
                push(&["-maxrate", m, "-bufsize", m]);
            }
        } else {
            let crf = s
                .crf
                .unwrap_or(match self.codec {
                    "h264" => 16,
                    "h265" => 18,
                    _ => 26,
                })
                .to_string();
            if self.vaapi {
                push(&["-rc_mode", "CQP", "-qp", &crf]);
            } else {
                push(&["-crf", &crf]);
            }
        }
        if let (Some(p), false) = (&s.preset, self.vaapi) {
            push(&["-preset", p]);
        }
        if self.encoder == "libx265" {
            push(&["-x265-params", "log-level=error"]);
        }
        push(&TAGS);
        a.extend(self.mux());
        a
    }

    /// The container: also what a lossless splice of chunks writes.
    pub fn mux(&self) -> Vec<String> {
        let mut a: Vec<&str> = Vec::new();
        if self.codec == "h265" && self.container != "mkv" {
            // What QuickTime and browsers look for.
            a.extend(["-tag:v", "hvc1"]);
        }
        match self.container {
            "mkv" => a.extend(["-f", "matroska"]),
            c => a.extend(["-f", c, "-movflags", "+faststart"]),
        }
        a.into_iter().map(String::from).collect()
    }

    /// For the summary line: `h264_vaapi (nv12, mp4)`.
    pub fn describe(&self) -> String {
        format!(
            "{} ({}, {})",
            self.encoder,
            self.yuv.pix_fmt(),
            self.container
        )
    }
}
