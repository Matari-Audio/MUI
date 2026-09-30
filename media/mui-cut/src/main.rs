//! `mui-cut`: render, look at, tidy and serve a `*.cut.json` project.
//!
//!     mui-cut render demo.cut.json -o out.mp4 [--scene NAME] [--mb N] [--size WxH] [--cpu]
//!     mui-cut still  demo.cut.json --t 1.5 -o f.png [--scene NAME] [--size WxH] [--cpu]
//!     mui-cut eval   demo.cut.json --t 1.5 [--scene NAME]
//!     mui-cut fmt    demo.cut.json
//!     mui-cut serve  demo.cut.json [--port 8740] [--web DIR]
#![forbid(unsafe_code)]

mod serve;

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use mui_cut::{Frame, Kind, Offline, Project, Renderer, Scene, eval};

type Result<T> = std::result::Result<T, String>;

const USAGE: &str = "usage:
  mui-cut render PROJECT -o OUT.mp4|null [--scene NAME] [--mb N] [--size WxH] [--cpu]
  mui-cut still  PROJECT --t SECONDS -o OUT.png [--scene NAME] [--size WxH] [--cpu]
  mui-cut eval   PROJECT --t SECONDS [--scene NAME]
  mui-cut fmt    PROJECT
  mui-cut serve  PROJECT [--port 8740] [--web DIR]";

/// `--name value` pairs after the command and project path.
struct Args {
    project: PathBuf,
    flags: Vec<(String, String)>,
}
impl Args {
    fn get(&self, name: &str) -> Option<&str> {
        self.flags
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
    fn has(&self, name: &str) -> bool {
        self.get(name).is_some()
    }
    fn num<T: std::str::FromStr>(&self, name: &str, default: T) -> Result<T> {
        self.get(name).map_or(Ok(default), |v| {
            v.parse()
                .map_err(|_| format!("--{name}: `{v}` is not a number"))
        })
    }
}

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if let Err(e) = run(&argv) {
        eprintln!("mui-cut: {e}");
        std::process::exit(1);
    }
}

fn run(argv: &[String]) -> Result<()> {
    let (Some(cmd), Some(project)) = (argv.first(), argv.get(1)) else {
        return Err(USAGE.into());
    };
    let mut flags = Vec::new();
    let mut rest = argv[2..].iter();
    while let Some(k) = rest.next() {
        let name = k
            .strip_prefix("--")
            .or_else(|| (k == "-o").then_some("o"))
            .ok_or_else(|| format!("unexpected `{k}`\n{USAGE}"))?;
        // The switches take no value.
        if name == "cpu" {
            flags.push((name.to_owned(), String::new()));
            continue;
        }
        let v = rest.next().ok_or_else(|| format!("{k} needs a value"))?;
        flags.push((name.to_owned(), v.clone()));
    }
    let args = Args {
        project: project.into(),
        flags,
    };
    match cmd.as_str() {
        "render" => render(&args),
        "still" => still(&args),
        "eval" => {
            let p = load(&args.project)?;
            let s = scene(&p, &args)?;
            let f = eval(&p, s, args.num("t", 0.)?);
            println!(
                "{}",
                serde_json::to_string_pretty(&f).map_err(|e| e.to_string())?
            );
            Ok(())
        }
        "fmt" => {
            let p = load(&args.project)?;
            write_atomic(&args.project, &p.to_json())
        }
        "serve" => serve::serve(
            &args.project,
            args.num("port", 8740)?,
            args.get("web").map_or_else(
                || Path::new(env!("CARGO_MANIFEST_DIR")).join("web"),
                PathBuf::from,
            ),
        ),
        _ => Err(USAGE.into()),
    }
}

fn load(path: &Path) -> Result<Project> {
    let src = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Project::load(&src).map_err(|e| format!("{}: {e}", path.display()))
}

/// Write through a sibling temp file, so a reader never sees half a project.
fn write_atomic(path: &Path, text: &str) -> Result<()> {
    let tmp = path.with_extension("cut.json.tmp");
    std::fs::write(&tmp, text)
        .and_then(|()| std::fs::rename(&tmp, path))
        .map_err(|e| format!("{}: {e}", path.display()))
}

fn scene<'a>(p: &'a Project, args: &Args) -> Result<&'a Scene> {
    match args.get("scene") {
        Some(n) => p.scene(n).ok_or_else(|| format!("no scene `{n}`")),
        None => p
            .scenes
            .first()
            .ok_or_else(|| "the project has no scenes".into()),
    }
}

/// The output size: `--size WxH`, else the project's.
fn size(p: &Project, args: &Args) -> Result<(u16, u16)> {
    let (w, h) = match args.get("size") {
        Some(s) => s
            .split_once('x')
            .and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)))
            .ok_or_else(|| format!("--size: `{s}` is not WxH"))?,
        None => (p.size[0], p.size[1]),
    };
    // H.264 4:2:0 wants even sides.
    let even = |v: u32| u16::try_from(v.max(2) & !1).map_err(|_| "size too large".to_owned());
    Ok((even(w)?, even(h)?))
}

/// Where frames are drawn: MUI's GPU renderer by default, Vello CPU with
/// `--cpu` or when no GPU adapter opens.
enum Backend {
    Cpu(Box<Renderer>, Vec<f32>),
    Gpu(Box<Offline>),
}

impl Backend {
    fn open(p: &Project, project: &Path, (w, h): (u16, u16), cpu: bool) -> Self {
        let mut b = if cpu {
            Self::Cpu(Box::new(Renderer::new(w, h)), Vec::new())
        } else {
            match Offline::new([w.into(), h.into()]) {
                Ok(g) => Self::Gpu(Box::new(g)),
                Err(e) => {
                    eprintln!("mui-cut: GPU unavailable ({e}); rendering on the CPU");
                    Self::Cpu(Box::new(Renderer::new(w, h)), Vec::new())
                }
            }
        };
        let assets = match &mut b {
            Self::Cpu(r, _) => &mut r.assets,
            Self::Gpu(g) => &mut g.assets,
        };
        let dir = project.parent().unwrap_or(Path::new("."));
        for l in p.scenes.iter().flat_map(|s| &s.layers) {
            if let Kind::Image { path } = &l.kind {
                let loaded = std::fs::read(dir.join(path))
                    .map_err(|e| e.to_string())
                    .and_then(|b| assets.add_png(path, &b));
                if let Err(e) = loaded {
                    eprintln!("mui-cut: image `{path}`: {e} (drawn as its fill)");
                }
            }
        }
        b
    }
    fn name(&self) -> String {
        match self {
            Self::Cpu(..) => "cpu (vello_cpu)".into(),
            Self::Gpu(g) => format!("gpu ({})", g.adapter),
        }
    }
    /// One output frame from its subframes; the GPU hands frames back a few
    /// behind, the CPU at once.
    fn push(&mut self, subs: &[Frame]) -> Result<Option<Vec<u8>>> {
        match self {
            Self::Gpu(g) => g.push(subs),
            Self::Cpu(r, _) if subs.len() == 1 => Ok(Some(r.draw(&subs[0])?.0)),
            Self::Cpu(r, acc) => {
                // mui-reel's shutter: subframes averaged in linear light.
                let (w, h) = r.size();
                acc.clear();
                acc.resize(usize::from(w) * usize::from(h) * 4, 0.);
                for f in subs {
                    mui_reel::accumulate(acc, &r.draw(f)?.0);
                }
                Ok(Some(mui_reel::resolve(acc, subs.len())))
            }
        }
    }
    fn finish(&mut self) -> Result<Vec<Vec<u8>>> {
        match self {
            Self::Gpu(g) => g.finish(),
            Self::Cpu(..) => Ok(Vec::new()),
        }
    }
}

fn still(args: &Args) -> Result<()> {
    let p = load(&args.project)?;
    let s = scene(&p, args)?;
    let out = args.get("o").ok_or("still needs -o OUT.png")?;
    let (w, h) = size(&p, args)?;
    let mut b = Backend::open(&p, &args.project, (w, h), args.has("cpu"));
    let f = eval(&p, s, args.num("t", 0.)?);
    let px = match b.push(std::slice::from_ref(&f))? {
        Some(px) => px,
        None => b.finish()?.pop().ok_or("no frame came back")?,
    };
    let file = std::fs::File::create(out).map_err(|e| format!("{out}: {e}"))?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w.into(), h.into());
    enc.set_color(png::ColorType::Rgba);
    enc.write_header()
        .and_then(|mut w| w.write_image_data(&px))
        .map_err(|e| format!("{out}: {e}"))?;
    println!("wrote {out} ({})", b.name());
    Ok(())
}

/// The fraction of a frame the shutter is open: 180 degrees.
const SHUTTER: f64 = 0.5;

fn render(args: &Args) -> Result<()> {
    let p = load(&args.project)?;
    let out = args.get("o").ok_or("render needs -o OUT.mp4")?;
    let scenes: Vec<&Scene> = match args.get("scene") {
        Some(_) => vec![scene(&p, args)?],
        None => p.scenes.iter().collect(),
    };
    let (w, h) = size(&p, args)?;
    let mb: usize = args.num("mb", 1)?;
    let mb = mb.clamp(1, 64);
    let mut b = Backend::open(&p, &args.project, (w, h), args.has("cpu"));
    let mut ff = Command::new("ffmpeg");
    ff.args([
        "-y",
        "-loglevel",
        "error",
        "-f",
        "rawvideo",
        "-pix_fmt",
        "rgba",
    ])
    .args([
        "-s",
        &format!("{w}x{h}"),
        "-r",
        &p.fps.to_string(),
        "-i",
        "-",
    ]);
    // `-o null` renders and discards: the renderer's speed without x264's.
    if out == "null" {
        ff.args(["-f", "null", "-"]);
    } else {
        ff.args(["-c:v", "libx264", "-pix_fmt", "yuv420p", "-crf", "16"])
            .args(["-movflags", "+faststart", out]);
    }
    let mut ff = ff
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|e| format!("ffmpeg: {e} (is it on PATH?)"))?;
    let stdin = ff.stdin.as_mut().ok_or("ffmpeg stdin")?;
    let mut write = |px: &[u8]| {
        stdin
            .write_all(px)
            .map_err(|e| format!("ffmpeg stdin: {e}"))
    };
    let start = std::time::Instant::now();
    let mut frames = 0usize;
    for s in scenes {
        let n = (s.duration * p.fps).round().max(1.) as usize;
        for i in 0..n {
            let t = i as f64 / p.fps;
            let subs: Vec<Frame> = (0..mb)
                .map(|k| eval(&p, s, t + SHUTTER / p.fps * k as f64 / mb as f64))
                .collect();
            if let Some(px) = b.push(&subs)? {
                write(&px)?;
            }
            frames += 1;
        }
    }
    for px in b.finish()? {
        write(&px)?;
    }
    drop(ff.stdin.take());
    let ok = ff.wait().map_err(|e| e.to_string())?.success();
    if !ok {
        return Err(format!("ffmpeg failed writing {out}"));
    }
    let secs = start.elapsed().as_secs_f64();
    println!(
        "wrote {out}: {frames} frames, {w}x{h} at {} fps, mb {mb}, {} in {secs:.2} s = {:.1} frames/s",
        p.fps,
        b.name(),
        frames as f64 / secs
    );
    Ok(())
}
