//! `mui-cut`: render, look at, tidy and serve a `*.cut.json` project.
//!
//!     mui-cut render demo.cut.json -o out.mp4 [--scene NAME] [--mb N] [--size WxH] [--renderer R]
//!     mui-cut still  demo.cut.json --t 1.5 -o f.png [--scene NAME] [--size WxH] [--renderer R]
//!     mui-cut eval   demo.cut.json --t 1.5 [--scene NAME]
//!     mui-cut fmt    demo.cut.json
//!     mui-cut serve  demo.cut.json [--port 8740] [--web DIR]
#![forbid(unsafe_code)]

mod mcp;
mod script;
mod serve;
mod tools;

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use mui_cut::{Assets, CpuPool, Engine, Frame, Offline, Project, Scene, eval};

type Result<T> = std::result::Result<T, String>;

const USAGE: &str = "usage:
  mui-cut render PROJECT -o OUT.mp4|null [--scene NAME] [--mb N] [--size WxH] [--renderer R] [--threads N] [--stats]
  mui-cut still  PROJECT --t SECONDS -o OUT.png [--scene NAME] [--size WxH] [--renderer R]
    R: classic (default; Vello compute on the GPU), gpu (vello_gpu), cpu (Vello CPU);
    --cpu is --renderer cpu. --threads: CPU frames drawn at once (default: one per core).
    --stats: per-frame wall time (evaluate, draw, hand to ffmpeg) p50/p95/max
  mui-cut eval   PROJECT --t SECONDS [--scene NAME]
  mui-cut fmt    PROJECT
  mui-cut schema                                   # the project JSON Schema
  mui-cut mcp    [PROJECT]                         # MCP server on stdio
  mui-cut gen    SCRIPT.rhai [-o OUT.cut.json] [--seed N] [--into PROJECT [--scene NAME]]
  mui-cut check  PROJECT [--json]
  mui-cut sheet  PROJECT [-o OUT.png] [--scene NAME] [--n 8] [--times 0,1.5] [--width 1600] [--cols 4] [--cpu]
  mui-cut strip  PROJECT --layer ID [-o OUT.png] [--scene NAME] [--n 8] [--width 1600] [--cpu]
  mui-cut diff   A B [-o OUT.png] [--n 6] [--width 1600] [--cpu]
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
    if argv.first().map(String::as_str) == Some("schema") {
        let schema =
            serde_json::to_string_pretty(&Project::json_schema()).map_err(|e| e.to_string())?;
        println!("{schema}");
        return Ok(());
    }
    if argv.first().map(String::as_str) == Some("mcp") {
        return mcp::serve(argv.get(1).map(String::as_str));
    }
    let (Some(cmd), Some(project)) = (argv.first(), argv.get(1)) else {
        return Err(USAGE.into());
    };
    let mut flags = Vec::new();
    let mut rest = argv[2..].iter();
    // `diff A B`: the second project is positional.
    if cmd == "diff" {
        let b = rest.next().ok_or("diff needs two projects: diff A B")?;
        flags.push(("against".to_owned(), b.clone()));
    }
    while let Some(k) = rest.next() {
        let name = k
            .strip_prefix("--")
            .or_else(|| (k == "-o").then_some("o"))
            .ok_or_else(|| format!("unexpected `{k}`\n{USAGE}"))?;
        // The switches take no value.
        if matches!(name, "cpu" | "stats" | "json") {
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
        "check" => tools::check(&args),
        "gen" => script::cmd(&args),
        "sheet" => tools::sheet_cmd(&args),
        "strip" => tools::strip_cmd(&args),
        "diff" => tools::diff_cmd(&args),
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

/// `--renderer`, with `--cpu` as its old spelling of `cpu`.
fn renderer(args: &Args) -> Option<&str> {
    if args.has("cpu") {
        Some("cpu")
    } else {
        args.get("renderer")
    }
}

/// Every file the project's layers name, read relative to the project;
/// what failed, as messages.
fn load_assets(p: &Project, project: &Path, assets: &mut Assets) -> Vec<String> {
    let dir = project.parent().unwrap_or(Path::new("."));
    let mut errs = Vec::new();
    for l in p.scenes.iter().flat_map(|s| &s.layers) {
        if let Some(path) = l.asset() {
            let loaded = std::fs::read(dir.join(path))
                .map_err(|e| e.to_string())
                .and_then(|b| assets.add_asset(path, &b));
            if let Err(e) = loaded {
                errs.push(format!("`{path}`: {e}"));
            }
        }
    }
    errs
}

/// Where frames are drawn: a Vello engine on the GPU (classic by default),
/// or Vello CPU with `--renderer cpu` or when no GPU adapter opens.
enum Backend {
    Cpu(CpuPool),
    Gpu(Box<Offline>),
}

impl Backend {
    /// `workers` CPU frames at once; a lone frame rasterises on every core
    /// instead.
    fn open(p: &Project, args: &Args, size: (u16, u16), workers: usize) -> Result<Self> {
        Self::open_at(p, &args.project, renderer(args), size, workers)
    }
    /// [`Backend::open`] without the command line: `renderer` is
    /// `classic` (the default), `gpu` or `cpu`.
    fn open_at(
        p: &Project,
        project: &Path,
        renderer: Option<&str>,
        (w, h): (u16, u16),
        workers: usize,
    ) -> Result<Self> {
        let cores = std::thread::available_parallelism().map_or(1, usize::from);
        let engine = match renderer {
            None | Some("classic") => Some(Engine::Classic),
            Some("gpu") => Some(Engine::Sparse),
            Some("cpu") => None,
            Some(r) => return Err(format!("--renderer: `{r}` is not classic, gpu or cpu")),
        };
        let mut assets = Assets::default();
        for e in load_assets(p, project, &mut assets) {
            eprintln!("mui-cut: {e} (an image draws as its fill, the rest as nothing)");
        }
        if let Some(engine) = engine {
            match Offline::new([w.into(), h.into()], engine) {
                Ok(mut g) => {
                    g.assets = assets;
                    return Ok(Self::Gpu(Box::new(g)));
                }
                Err(e) => eprintln!("mui-cut: GPU unavailable ({e}); rendering on the CPU"),
            }
        }
        let workers = workers.clamp(1, cores);
        let threads = if workers == 1 { cores - 1 } else { 0 };
        let threads = u16::try_from(threads).unwrap_or(u16::MAX);
        Ok(Self::Cpu(CpuPool::new(w, h, workers, threads, &assets)))
    }
    fn name(&self) -> String {
        match self {
            Self::Cpu(_) => "cpu (vello_cpu)".into(),
            Self::Gpu(g) => format!("gpu ({})", g.adapter),
        }
    }
    /// One output frame from its subframes; frames come back a few behind,
    /// in order.
    fn push(&mut self, subs: Vec<Frame>) -> Result<Option<Vec<u8>>> {
        match self {
            Self::Gpu(g) => g.push(&subs),
            Self::Cpu(pool) => pool.push(subs),
        }
    }
    fn finish(&mut self) -> Result<Vec<Vec<u8>>> {
        match self {
            Self::Gpu(g) => g.finish(),
            Self::Cpu(pool) => pool.finish(),
        }
    }
}

fn still(args: &Args) -> Result<()> {
    let p = load(&args.project)?;
    let s = scene(&p, args)?;
    let out = args.get("o").ok_or("still needs -o OUT.png")?;
    let (w, h) = size(&p, args)?;
    let mut b = Backend::open(&p, args, (w, h), 1)?;
    let f = eval(&p, s, args.num("t", 0.)?);
    let px = match b.push(vec![f])? {
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
    let cores = std::thread::available_parallelism().map_or(1, usize::from);
    let mut b = Backend::open(&p, args, (w, h), args.num("threads", cores)?)?;
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
    // Wall time per loop pass: a stall anywhere (a readback, a full pipe)
    // shows up as a tail.
    let mut times = Vec::new();
    for s in scenes {
        let n = (s.duration * p.fps).round().max(1.) as usize;
        for i in 0..n {
            let pass = std::time::Instant::now();
            let t = i as f64 / p.fps;
            let subs: Vec<Frame> = (0..mb)
                .map(|k| eval(&p, s, t + SHUTTER / p.fps * k as f64 / mb as f64))
                .collect();
            if let Some(px) = b.push(subs)? {
                write(&px)?;
            }
            frames += 1;
            times.push(pass.elapsed().as_secs_f64() * 1e3);
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
    if args.has("stats") {
        let mut slow: Vec<(usize, f64)> = times.iter().copied().enumerate().collect();
        slow.sort_by(|a, b| b.1.total_cmp(&a.1));
        let q = |f: f64| slow[((slow.len() - 1) as f64 * (1. - f)) as usize].1;
        let worst: Vec<String> = slow
            .iter()
            .take(3)
            .map(|(i, ms)| format!("#{i} {ms:.1}"))
            .collect();
        println!(
            "frame time: p50 {:.2} ms, p95 {:.2} ms, max {:.2} ms; slowest {}",
            q(0.5),
            q(0.95),
            q(1.),
            worst.join(", ")
        );
    }
    Ok(())
}
