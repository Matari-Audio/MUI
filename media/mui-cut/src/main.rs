//! `mui-cut`: render, look at, tidy and serve a `*.cut.json` project.
//!
//!     mui-cut render demo.cut.json -o out.mp4 [--scene NAME] [--mb N] [--size WxH] [--renderer R]
//!     mui-cut still  demo.cut.json --t 1.5 -o f.png [--scene NAME] [--size WxH] [--renderer R]
//!     mui-cut render promo.cut.json -o out/{name}.mp4 --variants all
//!     mui-cut eval   demo.cut.json --t 1.5 [--scene NAME]
//!     mui-cut fmt    demo.cut.json
//!     mui-cut serve  demo.cut.json [--port 8740] [--web DIR]
#![forbid(unsafe_code)]

mod encode;
mod mcp;
mod script;
mod serve;
mod tools;

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use mui_cut::yuv::{self, Yuv};
use mui_cut::{Assets, CpuPool, Engine, Frame, Offline, Project, Render, Scene, eval, subframes};

type Result<T> = std::result::Result<T, String>;

const USAGE: &str = "usage:
  mui-cut render PROJECT -o OUT.mp4|null [--scene NAME] [--mb N] [--size WxH] [--renderer R] [--threads N] [--stats]
                 [--codec h264|h265|av1] [--encoder auto|vaapi|software]
                 [--crf N | --bitrate 12M [--maxrate 20M]] [--preset P]
                 [--pix-fmt yuv420p|yuv420p10le] [--container mp4|mkv|mov]
                 [--variant NAME | --variants all|NAME,NAME -o out/{name}.mp4]
  mui-cut still  PROJECT --t SECONDS -o OUT.png [--scene NAME] [--size WxH] [--renderer R] [--variant NAME]
    R: classic (default; Vello compute on the GPU), gpu (vello_gpu), cpu (Vello CPU);
    --cpu is --renderer cpu. --threads: CPU frames drawn at once (default: one per core).
    --stats: per-frame wall time (evaluate, draw, hand to ffmpeg) p50/p95/max
  mui-cut eval   PROJECT --t SECONDS [--scene NAME] [--variant NAME]
  mui-cut fmt    PROJECT
  mui-cut schema                                   # the project JSON Schema
  mui-cut mcp    [PROJECT]                         # MCP server on stdio
  mui-cut gen    SCRIPT.rhai [-o OUT.cut.json] [--seed N] [--into PROJECT [--scene NAME]]
  mui-cut check  PROJECT [--json]
  mui-cut sheet  PROJECT [-o OUT.png] [--scene NAME] [--n 8] [--times 0,1.5] [--width 1600] [--cols 4] [--renderer R]
  mui-cut strip  PROJECT --layer ID [-o OUT.png] [--scene NAME] [--n 8] [--width 1600] [--renderer R]
  mui-cut diff   A B [-o OUT.png] [--n 6] [--width 1600] [--renderer R]
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
            let p = load_variant(&args)?;
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

/// The project, as `--variant` makes it.
fn load_variant(args: &Args) -> Result<Project> {
    let p = load(&args.project)?;
    match args.get("variant") {
        Some(v) => p.variant(v),
        None => Ok(p),
    }
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
/// Frames come back as straight RGBA, or as `Yuv` planes for an encoder.
enum Backend {
    /// With the planes an encoder wants, and the frame size.
    Cpu(CpuPool, Option<(Yuv, [u32; 2])>),
    Gpu(Box<Offline>),
}

impl Backend {
    /// `workers` CPU frames at once; a lone frame rasterises on every core
    /// instead.
    fn open(
        p: &Project,
        args: &Args,
        size: (u16, u16),
        workers: usize,
        yuv: Option<Yuv>,
    ) -> Result<Self> {
        Self::open_at(p, &args.project, renderer(args), size, workers, yuv)
    }
    /// [`Backend::open`] without the command line: `renderer` is
    /// `classic` (the default), `gpu` or `cpu`.
    fn open_at(
        p: &Project,
        project: &Path,
        renderer: Option<&str>,
        (w, h): (u16, u16),
        workers: usize,
        yuv: Option<Yuv>,
    ) -> Result<Self> {
        let engine = match renderer {
            None | Some("classic") => Some(Engine::Classic),
            Some("gpu") => Some(Engine::Sparse),
            Some("cpu") => None,
            Some(r) => return Err(format!("--renderer: `{r}` is not classic, gpu or cpu")),
        };
        let assets = read_assets(p, project);
        if let Some(engine) = engine {
            let size = [w.into(), h.into()];
            let gpu = match yuv {
                Some(y) => Offline::with_yuv(size, engine, y),
                None => Offline::new(size, engine),
            };
            match gpu {
                Ok(mut g) => {
                    g.assets = assets;
                    return Ok(Self::Gpu(Box::new(g)));
                }
                Err(e) => eprintln!("mui-cut: GPU unavailable ({e}); rendering on the CPU"),
            }
        }
        Ok(Self::cpu(p, (w, h), workers, yuv, &assets))
    }
    /// `workers` Vello CPU renderers.
    fn cpu(
        p: &Project,
        (w, h): (u16, u16),
        workers: usize,
        yuv: Option<Yuv>,
        assets: &Assets,
    ) -> Self {
        if p.scenes.iter().any(|s| s.mode == mui_cut::Mode::ThreeD) {
            eprintln!("mui-cut: the CPU renderer has no 3D pass; 3D scenes draw flat");
        }
        if p.has_effects() {
            eprintln!("mui-cut: effects need the GPU; the CPU renderer draws without them");
        }
        let cores = std::thread::available_parallelism().map_or(1, usize::from);
        let workers = workers.clamp(1, cores);
        let threads = if workers == 1 { cores - 1 } else { 0 };
        let threads = u16::try_from(threads).unwrap_or(u16::MAX);
        Self::Cpu(
            CpuPool::new(w, h, workers, threads, assets),
            yuv.map(|y| (y, [w.into(), h.into()])),
        )
    }
    /// The next variant: its size and assets, on the same GPU device (or a
    /// fresh CPU pool). Nothing may be in flight.
    fn retarget(
        &mut self,
        p: &Project,
        args: &Args,
        (w, h): (u16, u16),
        yuv: Option<Yuv>,
    ) -> Result<()> {
        let assets = read_assets(p, &args.project);
        match self {
            Self::Gpu(g) => {
                g.resize([w.into(), h.into()], yuv)?;
                g.assets = assets;
            }
            Self::Cpu(..) => *self = Self::cpu(p, (w, h), threads(args)?, yuv, &assets),
        }
        Ok(())
    }
    fn name(&self) -> String {
        match self {
            Self::Cpu(..) => "cpu (vello_cpu)".into(),
            Self::Gpu(g) => format!("gpu ({})", g.adapter),
        }
    }
    /// One output frame from its subframes; frames come back a few behind,
    /// in order.
    fn push(&mut self, subs: Vec<Frame>) -> Result<Option<Vec<u8>>> {
        match self {
            Self::Gpu(g) => g.push(&subs),
            Self::Cpu(pool, yuv) => Ok(pool.push(subs)?.map(|px| to_yuv(px, *yuv))),
        }
    }
    fn finish(&mut self) -> Result<Vec<Vec<u8>>> {
        match self {
            Self::Gpu(g) => g.finish(),
            Self::Cpu(pool, yuv) => {
                let yuv = *yuv;
                let px = pool.finish()?;
                Ok(px.into_iter().map(|px| to_yuv(px, yuv)).collect())
            }
        }
    }
}

/// [`load_assets`], saying what failed.
fn read_assets(p: &Project, project: &Path) -> Assets {
    let mut assets = Assets::default();
    for e in load_assets(p, project, &mut assets) {
        eprintln!("mui-cut: {e} (an image draws as its fill, the rest as nothing)");
    }
    assets
}

/// `--threads`: CPU frames drawn at once, one per core by default.
fn threads(args: &Args) -> Result<usize> {
    args.num(
        "threads",
        std::thread::available_parallelism().map_or(1, usize::from),
    )
}

/// A CPU frame as `yuv` planes, if an encoder wants them.
fn to_yuv(rgba: Vec<u8>, yuv: Option<(Yuv, [u32; 2])>) -> Vec<u8> {
    match yuv {
        Some((y, size)) => yuv::from_rgba(&rgba, size, y),
        None => rgba,
    }
}

fn still(args: &Args) -> Result<()> {
    let p = load_variant(args)?;
    let s = scene(&p, args)?;
    let out = args.get("o").ok_or("still needs -o OUT.png")?;
    let (w, h) = size(&p, args)?;
    let mut b = Backend::open(&p, args, (w, h), 1, None)?;
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

/// Encoder settings from the flags, over the project's `render`.
fn settings(p: &Project, args: &Args) -> Result<Render> {
    let s = |k: &str| args.get(k).map(str::to_owned);
    let flags = Render {
        codec: s("codec"),
        encoder: s("encoder"),
        crf: args.get("crf").map(|_| args.num("crf", 0)).transpose()?,
        bitrate: s("bitrate"),
        maxrate: s("maxrate"),
        preset: s("preset"),
        pix_fmt: s("pix-fmt"),
        container: s("container"),
        mb: args.get("mb").map(|_| args.num("mb", 1)).transpose()?,
    };
    let mut r = p.render.clone().unwrap_or_default().with(&flags);
    // A bitrate flag replaces a CRF from the file, and the other way round.
    if flags.bitrate.is_some() && flags.crf.is_none() {
        r.crf = None;
    }
    if flags.crf.is_some() && flags.bitrate.is_none() {
        r.bitrate = None;
    }
    r.check()?;
    Ok(r)
}

/// `--variants all|a,b` renders each variant to `-o` with `{name}` filled
/// in, on one GPU device; else one render, of `--variant` if given.
fn render(args: &Args) -> Result<()> {
    let out = args.get("o").ok_or("render needs -o OUT.mp4")?;
    let Some(list) = args.get("variants") else {
        return render_one(&load_variant(args)?, args, out, &mut None);
    };
    let base = load(&args.project)?;
    let names: Vec<&str> = match list {
        "all" => base.variants.iter().map(|v| v.name.as_str()).collect(),
        l => l.split(',').map(str::trim).collect(),
    };
    if names.is_empty() {
        return Err("the project has no variants".into());
    }
    if names.len() > 1 && !out.contains("{name}") {
        return Err("several variants need `{name}` in -o, e.g. out/{name}.mp4".into());
    }
    let mut b = None;
    for n in names {
        let p = base.variant(n)?;
        let path = out.replace("{name}", n);
        if let Some(dir) = Path::new(&path)
            .parent()
            .filter(|d| !d.as_os_str().is_empty())
        {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        render_one(&p, args, &path, &mut b)?;
    }
    Ok(())
}

/// Render `p` to `out`, on `backend` if one is open (else it opens one).
fn render_one(p: &Project, args: &Args, out: &str, backend: &mut Option<Backend>) -> Result<()> {
    let scenes: Vec<&Scene> = match args.get("scene") {
        Some(_) => vec![scene(p, args)?],
        None => p.scenes.iter().collect(),
    };
    let (w, h) = size(p, args)?;
    let r = settings(p, args)?;
    let mb = r.mb.unwrap_or(1).clamp(1, 64);
    // `-o null` renders and discards: the renderer's speed without an encoder's.
    let plan = if out == "null" {
        None
    } else {
        Some(encode::plan(&r, out)?)
    };
    let yuv = plan.as_ref().map_or(Yuv::Nv12, |p| p.yuv);
    let b = match backend {
        Some(b) => {
            b.retarget(p, args, (w, h), Some(yuv))?;
            b
        }
        None => backend.insert(Backend::open(p, args, (w, h), threads(args)?, Some(yuv))?),
    };
    let mut ff = Command::new("ffmpeg");
    // SVT-AV1 prints its banner through its own logger.
    ff.env("SVT_LOG", "1").args(["-y", "-loglevel", "error"]);
    match &plan {
        Some(plan) => ff
            .args(plan.input((w, h), p.fps))
            .args(plan.output())
            .arg(out),
        None => ff
            .args(["-f", "rawvideo", "-pix_fmt", yuv.pix_fmt()])
            .args(["-s", &format!("{w}x{h}"), "-r", &p.fps.to_string()])
            .args(["-i", "-", "-f", "null", "-"]),
    };
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
            let subs = subframes(p, s, t, mb);
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
        "wrote {out}: {frames} frames, {w}x{h} at {} fps, mb {mb}, {}, {} in {secs:.2} s = {:.1} frames/s",
        p.fps,
        b.name(),
        plan.as_ref()
            .map_or_else(|| "no encoder".into(), encode::Plan::describe),
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
