//! The agent's tools on the CLI: `check` (what is wrong, where, when and
//! how to fix it), `sheet` (many frames in one image), `strip` (one layer's
//! motion as onion skins and a trail) and `diff` (what changed between two
//! versions, as pictures). The MCP server calls the same functions.
use std::path::Path;

use mui_cut::check::{Issue, Severity};
use mui_cut::{Project, Renderer};

use crate::{Args, Result, load_assets};

/// Check the file at `path`: its load error, or every lint.
pub fn check_file(path: &Path) -> Result<Vec<Issue>> {
    let src = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let Ok(p) = Project::load(&src) else {
        return Ok(mui_cut::check::check(
            &src,
            &mut Renderer::new(2, 2),
            &|_| true,
        ));
    };
    // Contrast is sampled small: an average over a box needs few pixels.
    let w = 480u16;
    let h = (f64::from(w) * f64::from(p.size[1]) / f64::from(p.size[0]))
        .round()
        .max(2.) as u16;
    let mut r = Renderer::new(w, h);
    let _ = load_assets(&p, path, &mut r.assets);
    let dir = path.parent().unwrap_or(Path::new("."));
    let mut issues = mui_cut::check::check(&src, &mut r, &|a| dir.join(a).is_file());
    if p.render
        .as_ref()
        .and_then(mui_cut::Render::rt_glass)
        .is_some()
    {
        let probe = mui_stage_rt::probe().map(|_| ()).map_err(|e| match e {
            mui_stage_rt::Error::Unavailable(why) => why,
            e => e.to_string(),
        });
        issues.extend(mui_cut::check::rt_glass(&src, probe));
    }
    Ok(issues)
}

pub fn check(args: &Args) -> Result<()> {
    let issues = check_file(&args.project)?;
    let count = |s| issues.iter().filter(|i| i.severity == s).count();
    let (e, w, i) = (
        count(Severity::Error),
        count(Severity::Warning),
        count(Severity::Info),
    );
    if args.has("json") {
        let out = serde_json::json!({ "errors": e, "warnings": w, "infos": i, "issues": issues });
        println!(
            "{}",
            serde_json::to_string_pretty(&out).map_err(|e| e.to_string())?
        );
    } else {
        for issue in &issues {
            println!("{issue}");
        }
        println!(
            "{}: {e} errors, {w} warnings, {i} notes",
            args.project.display()
        );
    }
    if e > 0 {
        return Err(format!("{e} errors"));
    }
    Ok(())
}

// ---------------------------------------------------------------- pictures

use mui_cut::{Drawn, Frame, Layer, Scene, eval};

use crate::Backend;

/// Every frame through `b`, in order (the GPU hands them back late).
pub fn draw_all(b: &mut Backend, frames: &[Frame]) -> Result<Vec<Vec<u8>>> {
    let mut out = Vec::with_capacity(frames.len());
    for f in frames {
        if let Some(px) = b.push(vec![f.clone()])? {
            out.push(px);
        }
    }
    out.extend(b.finish()?);
    Ok(out)
}

/// RGBA as PNG bytes.
pub fn png_bytes(w: u32, h: u32, px: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.write_header()
        .and_then(|mut w| w.write_image_data(px))
        .map_err(|e| e.to_string())?;
    Ok(out)
}

/// A picture for an agent: PNG bytes and a line or two on what it shows.
pub struct Picture {
    pub png: Vec<u8>,
    pub w: u32,
    pub h: u32,
    pub note: String,
}

/// A layer built from JSON, evaluated: labels and trails are drawn by the
/// same renderer as the project, so they need no font code of their own.
fn drawn(v: serde_json::Value) -> Drawn {
    let l: Layer = serde_json::from_value(v).expect("a built-in layer parses");
    l.at(0.)
}

/// A caption band along the top of a frame `fw` wide, `px` high on screen
/// when the frame is shown at `scale`.
fn caption(text: &str, fw: f64, scale: f64) -> [Drawn; 2] {
    let size = 14. / scale;
    [
        drawn(serde_json::json!({
            "id": "~band", "kind": "rect", "x": fw / 2., "y": size * 0.75,
            "width": fw, "height": size * 1.5, "fill": "#000000b0"
        })),
        drawn(serde_json::json!({
            "id": "~caption", "kind": "text", "text": text, "x": fw / 2., "y": size * 0.75,
            "font_size": size, "weight": 600, "fill": "#ffffff"
        })),
    ]
}

/// Rows of caption above each tile, in tile pixels.
const HEAD: u32 = 22;

/// `frames` drawn through `b` (frames of `size` project pixels at `tw`
/// wide), then those and `tiles`, each with its caption stacked above it
/// so no caption covers the frame: tiles `HEAD` rows taller. One batch,
/// since a backend finishes once.
fn headed(
    b: &mut Backend,
    size: [u32; 2],
    frames: &[Frame],
    tiles: Vec<Vec<u8>>,
    labels: &[String],
    tw: u32,
) -> Result<Vec<Vec<u8>>> {
    let scale = f64::from(tw) / f64::from(size[0]);
    let mut batch = frames.to_vec();
    batch.extend(labels.iter().map(|l| Frame {
        size,
        background: mui_cut::Rgba([0, 0, 0, 255]),
        layers: caption(l, f64::from(size[0]), scale).into(),
        view: None,
        effects: Vec::new(),
        t: 0.,
        seed: 0,
    }));
    let mut heads = draw_all(b, &batch)?;
    let tiles: Vec<Vec<u8>> = heads.drain(..frames.len()).chain(tiles).collect();
    let row = (tw * 4) as usize;
    Ok(heads
        .into_iter()
        .zip(tiles)
        .map(|(h, t)| [&h[..row * HEAD as usize], &t[..]].concat())
        .collect())
}

/// Tiles `tw` x `th` in rows of `cols`, `gap` apart on a grey that is
/// neither black nor white, so the frame edges show.
fn compose(tiles: &[Vec<u8>], (tw, th): (u32, u32), cols: usize) -> (Vec<u8>, u32, u32) {
    const GAP: u32 = 6;
    let cols = cols.clamp(1, tiles.len().max(1));
    let rows = tiles.len().div_ceil(cols);
    let w = GAP + cols as u32 * (tw + GAP);
    let h = GAP + rows as u32 * (th + GAP);
    let mut out: Vec<u8> = [0x2a, 0x2a, 0x33, 0xff].repeat((w * h) as usize);
    for (i, t) in tiles.iter().enumerate() {
        let (x0, y0) = (
            GAP + (i % cols) as u32 * (tw + GAP),
            GAP + (i / cols) as u32 * (th + GAP),
        );
        for y in 0..th {
            let src = (y * tw * 4) as usize..((y + 1) * tw * 4) as usize;
            let dst = (((y0 + y) * w + x0) * 4) as usize;
            out[dst..dst + src.len()].copy_from_slice(&t[src]);
        }
    }
    (out, w, h)
}

/// An even tile size `width` wide for `p`'s aspect.
fn tile(p: &Project, width: u32) -> (u16, u16) {
    let w = width.clamp(16, 4096) & !1;
    let h =
        ((f64::from(w) * f64::from(p.size[1]) / f64::from(p.size[0])).round() as u32).max(2) & !1;
    (w as u16, h.min(4096) as u16)
}

/// Times worth a look in `s`: its start and end, every key, and an even
/// spread, merged within two frames and thinned to `max`.
pub fn review_times(p: &Project, s: &Scene, max: usize) -> Vec<f64> {
    let end = (s.duration - 1. / p.fps).max(0.);
    let mut ts: Vec<f64> = (0..6).map(|i| end * f64::from(i) / 5.).collect();
    for l in &s.layers {
        ts.extend(mui_cut::check::key_times(l));
    }
    ts.retain(|t| (0. ..=end).contains(t));
    ts.sort_by(f64::total_cmp);
    ts.dedup_by(|a, b| (*a - *b).abs() < 2. / p.fps);
    thin(ts, max)
}

/// At most `max` of `v`, evenly picked, first and last kept.
fn thin<T: Clone>(v: Vec<T>, max: usize) -> Vec<T> {
    if v.len() <= max || max < 2 {
        return v.into_iter().take(max.max(1)).collect();
    }
    (0..max)
        .map(|i| v[i * (v.len() - 1) / (max - 1)].clone())
        .collect()
}

pub struct SheetOpts {
    pub scene: Option<String>,
    /// Explicit times (seconds into each scene), else [`review_times`].
    pub times: Option<Vec<f64>>,
    /// Frames per scene when choosing times.
    pub per_scene: usize,
    pub width: u32,
    pub cols: Option<usize>,
    /// `classic` (default), `gpu` or `cpu`.
    pub renderer: Option<String>,
}

/// A contact sheet: frames of one or every scene in a grid, each captioned
/// with its scene, time and frame number.
pub fn sheet(path: &Path, o: &SheetOpts) -> Result<Picture> {
    let p = crate::load(path)?;
    let scenes: Vec<&Scene> = match &o.scene {
        Some(n) => vec![p.scene(n).ok_or_else(|| format!("no scene `{n}`"))?],
        None => p.scenes.iter().collect(),
    };
    let cols = o.cols.unwrap_or(if p.size[0] >= p.size[1] { 4 } else { 6 });
    let (tw, th) = tile(
        &p,
        (o.width.saturating_sub(6) / cols as u32).saturating_sub(6),
    );
    let mut shots = Vec::new();
    for s in scenes {
        let ts = o
            .times
            .clone()
            .unwrap_or_else(|| review_times(&p, s, o.per_scene));
        for t in ts {
            let label = format!("{} · {t:.2}s · f{}", s.name, (t * p.fps).round());
            shots.push((eval(&p, s, t), label));
        }
    }
    if shots.is_empty() {
        return Err("no frames to show".into());
    }
    let (frames, labels): (Vec<Frame>, Vec<String>) = thin(shots, 64).into_iter().unzip();
    let mut b = Backend::open_at(&p, path, o.renderer.as_deref(), (tw, th), usize::MAX, None)?;
    let tiles = headed(&mut b, p.size, &frames, Vec::new(), &labels, tw.into())?;
    let (px, w, h) = compose(&tiles, (tw.into(), u32::from(th) + HEAD), cols);
    Ok(Picture {
        png: png_bytes(w, h, &px)?,
        w,
        h,
        note: format!(
            "{} frames, {cols} across, {w}x{h}: {}",
            tiles.len(),
            labels.join(", ")
        ),
    })
}

/// One layer's motion in one picture: the rest of the scene faded at the
/// start of the move, the layer drawn at `n` times from faint (early) to
/// solid (late), its path as a yellow trail with a dot per ghost.
pub fn strip(
    path: &Path,
    layer: &str,
    scene: Option<&str>,
    n: usize,
    width: u32,
    renderer: Option<&str>,
) -> Result<Picture> {
    let p = crate::load(path)?;
    let s = match scene {
        Some(name) => p.scene(name).ok_or_else(|| format!("no scene `{name}`"))?,
        None => p
            .scenes
            .iter()
            .find(|s| s.layers.iter().any(|l| l.id == layer))
            .ok_or_else(|| format!("no layer `{layer}` in any scene"))?,
    };
    let (li, l) = s
        .layers
        .iter()
        .enumerate()
        .find(|(_, l)| l.id == layer)
        .ok_or_else(|| format!("no layer `{layer}` in scene `{}`", s.name))?;
    let mut keys = mui_cut::check::key_times(l);
    keys.sort_by(f64::total_cmp);
    let end = (s.duration - 1. / p.fps).max(0.);
    let (t0, t1) = match (keys.first(), keys.last()) {
        (Some(&a), Some(&b)) if b - a > 1. / p.fps => (a.clamp(0., end), b.clamp(0., end)),
        _ => (0., end),
    };
    let n = n.clamp(2, 32);
    let ts: Vec<f64> = (0..n)
        .map(|i| t0 + (t1 - t0) * i as f64 / (n - 1) as f64)
        .collect();
    let (tw, th) = tile(&p, width);
    let scale = f64::from(tw) / f64::from(p.size[0]);

    let mut f = eval(&p, s, t0);
    let mut layers: Vec<Drawn> = f
        .layers
        .iter()
        .enumerate()
        .filter(|&(i, _)| i != li)
        .map(|(_, d)| Drawn {
            opacity: d.opacity * 0.25,
            ..d.clone()
        })
        .collect();
    for (i, &t) in ts.iter().enumerate() {
        let mut d = l.eval_at(t, p.fps, p.sample_rate);
        d.opacity *= 0.2 + 0.8 * i as f64 / (n - 1) as f64;
        layers.push(d);
    }
    let frames = ((t1 - t0) * p.fps).ceil().max(1.) as usize;
    let trail: Vec<String> = (0..=frames)
        .map(|i| {
            let t = t0 + (t1 - t0) * i as f64 / frames as f64;
            format!("{:.1} {:.1}", l.x.at(t), l.y.at(t))
        })
        .collect();
    let line = 2.5 / scale;
    layers.push(drawn(serde_json::json!({
        "id": "~trail", "kind": "path", "d": format!("M {}", trail.join(" L ")),
        "fill": "#00000000", "stroke": "#ffd400", "stroke_width": line
    })));
    // Time labels only where they will not pile up on each other.
    let mut labelled: Vec<(f64, f64)> = Vec::new();
    for &t in &ts {
        let (x, y) = (l.x.at(t), l.y.at(t));
        layers.push(drawn(serde_json::json!({
            "id": "~dot", "kind": "ellipse", "x": x, "y": y,
            "width": 4. * line, "height": 4. * line, "fill": "#ffd400"
        })));
        if labelled
            .iter()
            .all(|&(lx, ly)| (x - lx).hypot(y - ly) > 48. / scale)
        {
            labelled.push((x, y));
            layers.push(drawn(serde_json::json!({
                "id": "~t", "kind": "text", "text": format!("{t:.2}s"), "x": x, "y": y - 6. * line,
                "font_size": 12. / scale, "weight": 700, "fill": "#ffd400",
                "stroke": "#000000", "stroke_width": 0.8 / scale
            })));
        }
    }
    let label = format!(
        "{} · `{layer}` {t0:.2}s → {t1:.2}s, {n} ghosts, faint = early",
        s.name
    );
    layers.extend(caption(&label, f64::from(p.size[0]), scale));
    f.layers = layers;
    let mut b = Backend::open_at(&p, path, renderer, (tw, th), 1, None)?;
    let px = draw_all(&mut b, std::slice::from_ref(&f))?
        .pop()
        .ok_or("no frame came back")?;
    let (w, h) = (u32::from(tw), u32::from(th));
    Ok(Picture {
        png: png_bytes(w, h, &px)?,
        w,
        h,
        note: format!(
            "{label}; path from ({:.0}, {:.0}) to ({:.0}, {:.0})",
            l.x.at(t0),
            l.y.at(t0),
            l.x.at(t1),
            l.y.at(t1)
        ),
    })
}

/// How much two frames differ: the share of pixels whose largest channel
/// difference is over 8 of 255, and a heat map (the second frame dimmed,
/// changes in red).
fn difference(a: &[u8], b: &[u8]) -> (f64, Vec<u8>) {
    let mut changed = 0usize;
    let heat = a
        .as_chunks::<4>()
        .0
        .iter()
        .zip(b.as_chunks::<4>().0)
        .flat_map(|(a, b)| {
            let d = (0..3).map(|c| a[c].abs_diff(b[c])).max().unwrap_or(0);
            if d > 8 {
                changed += 1;
            }
            let grey = ((u16::from(b[0]) + u16::from(b[1]) + u16::from(b[2])) / 10) as u8;
            let k = (u16::from(d) * 4).min(255) as u8;
            [grey.max(k), grey, grey, 255]
        })
        .collect();
    (changed as f64 / (a.len() / 4).max(1) as f64, heat)
}

/// What looks different between two versions of a project: for the frames
/// that changed most (scenes matched by name), rows of A | B | heat map.
pub fn diff(
    a_path: &Path,
    b_path: &Path,
    n: usize,
    width: u32,
    renderer: Option<&str>,
) -> Result<Picture> {
    let (a, b) = (crate::load(a_path)?, crate::load(b_path)?);
    let (tw, th) = tile(&a, (width.saturating_sub(6) / 3).saturating_sub(6));
    let mut notes = Vec::new();
    let mut at: Vec<(String, f64)> = Vec::new();
    let (mut fa, mut fb) = (Vec::new(), Vec::new());
    for sa in &a.scenes {
        let Some(sb) = b.scene(&sa.name) else {
            notes.push(format!("scene `{}` only in A", sa.name));
            continue;
        };
        let mut ts = review_times(&a, sa, 24);
        ts.extend(review_times(&b, sb, 24));
        ts.sort_by(f64::total_cmp);
        ts.dedup_by(|x, y| (*x - *y).abs() < 1. / a.fps);
        for t in ts {
            fa.push(eval(&a, sa, t));
            fb.push(eval(&b, sb, t));
            at.push((sa.name.clone(), t));
        }
    }
    for sb in &b.scenes {
        if a.scene(&sb.name).is_none() {
            notes.push(format!("scene `{}` only in B", sb.name));
        }
    }
    if a.size != b.size || a.fps != b.fps {
        notes.push(format!(
            "size/fps {:?}@{} -> {:?}@{}",
            a.size, a.fps, b.size, b.fps
        ));
    }
    let pa = draw_all(
        &mut Backend::open_at(&a, a_path, renderer, (tw, th), usize::MAX, None)?,
        &fa,
    )?;
    let pb = draw_all(
        &mut Backend::open_at(&b, b_path, renderer, (tw, th), usize::MAX, None)?,
        &fb,
    )?;
    let mut changed: Vec<(usize, f64, Vec<u8>)> = pa
        .iter()
        .zip(&pb)
        .enumerate()
        .map(|(i, (x, y))| {
            let (d, heat) = difference(x, y);
            (i, d, heat)
        })
        .filter(|(_, d, _)| *d > 0.001)
        .collect();
    changed.sort_by(|x, y| y.1.total_cmp(&x.1));
    changed.truncate(n.max(1));
    changed.sort_by_key(|c| c.0);
    for (i, d, _) in &changed {
        notes.push(format!(
            "{} t={:.2}: {:.1}% of pixels changed",
            at[*i].0,
            at[*i].1,
            d * 100.
        ));
    }
    if changed.is_empty() {
        notes.push("no visible differences at the sampled times".into());
        let (w, h) = (u32::from(tw), u32::from(th));
        return Ok(Picture {
            png: png_bytes(w, h, &pb[0])?,
            w,
            h,
            note: notes.join("\n"),
        });
    }
    let mut tiles = Vec::new();
    let mut labels = Vec::new();
    for (i, d, heat) in changed {
        tiles.extend([pa[i].clone(), pb[i].clone(), heat]);
        labels.extend([
            format!("A · {} · {:.2}s", at[i].0, at[i].1),
            "B".to_owned(),
            format!("changed: {:.1}% of pixels", d * 100.),
        ]);
    }
    let tiles = headed(
        &mut Backend::open_at(&a, a_path, renderer, (tw, th), usize::MAX, None)?,
        a.size,
        &[],
        tiles,
        &labels,
        tw.into(),
    )?;
    let (px, w, h) = compose(&tiles, (tw.into(), u32::from(th) + HEAD), 3);
    Ok(Picture {
        png: png_bytes(w, h, &px)?,
        w,
        h,
        note: notes.join("\n"),
    })
}

/// `-o`, else the project's name with `ext`.
fn out_path(args: &Args, ext: &str) -> std::path::PathBuf {
    args.get("o").map_or_else(
        || {
            let name = args
                .project
                .file_name()
                .unwrap_or_default()
                .to_string_lossy();
            let stem = name.trim_end_matches(".json").trim_end_matches(".cut");
            args.project.with_file_name(format!("{stem}.{ext}"))
        },
        Into::into,
    )
}

fn save(pic: &Picture, out: &Path) -> Result<()> {
    std::fs::write(out, &pic.png).map_err(|e| format!("{}: {e}", out.display()))?;
    println!(
        "wrote {} ({}x{})\n{}",
        out.display(),
        pic.w,
        pic.h,
        pic.note
    );
    Ok(())
}

pub fn sheet_cmd(args: &Args) -> Result<()> {
    let times = args
        .get("times")
        .map(|v| {
            v.split(',')
                .map(|t| {
                    t.trim()
                        .parse::<f64>()
                        .map_err(|_| format!("--times: `{t}` is not seconds"))
                })
                .collect::<Result<Vec<_>>>()
        })
        .transpose()?;
    let o = SheetOpts {
        scene: args.get("scene").map(Into::into),
        times,
        per_scene: args.num("n", 8)?,
        width: args.num("width", 1600)?,
        cols: args
            .get("cols")
            .map(str::parse)
            .transpose()
            .map_err(|_| "--cols: not a number")?,
        renderer: crate::renderer(args).map(Into::into),
    };
    save(&sheet(&args.project, &o)?, &out_path(args, "sheet.png"))
}

pub fn strip_cmd(args: &Args) -> Result<()> {
    let layer = args.get("layer").ok_or("strip needs --layer ID")?;
    let pic = strip(
        &args.project,
        layer,
        args.get("scene"),
        args.num("n", 8)?,
        args.num("width", 1600)?,
        crate::renderer(args),
    )?;
    save(&pic, &out_path(args, &format!("{layer}.strip.png")))
}

pub fn diff_cmd(args: &Args) -> Result<()> {
    // `P@REV` (when no file has that name) or `--rev REV` against the
    // working copy: the revision is A, the file as it is now B.
    let spec = args.project.to_string_lossy().into_owned();
    let (project, rev) = match spec.rsplit_once('@') {
        Some((p, r)) if !args.project.exists() && Path::new(p).is_file() => {
            (Path::new(p).to_owned(), Some(r.to_owned()))
        }
        _ => (args.project.clone(), args.get("rev").map(Into::into)),
    };
    let old = rev.as_deref().map(|r| at_rev(&project, r)).transpose()?;
    let (a, b) = match (&old, args.get("against")) {
        (Some(old), _) => (old.path.clone(), project.clone()),
        (None, Some(other)) => (project.clone(), other.into()),
        (None, None) => return Err("diff needs a second project, P@REV or --rev REV".into()),
    };
    let pic = diff(
        &a,
        &b,
        args.num("n", 6)?,
        args.num("width", 1600)?,
        crate::renderer(args),
    )?;
    let args = Args {
        project,
        flags: args.flags.clone(),
    };
    save(&pic, &out_path(&args, "diff.png"))
}

/// A project as git has it at a revision: the file and the assets it
/// names, read with `git show`, in a scratch folder beside the project
/// (removed on drop), so relative paths resolve as they did then. Plugin
/// captures are content-addressed, so the folder shares the project's
/// capture cache. An asset git does not have at that revision is taken
/// from the working copy.
pub struct AtRev {
    pub path: std::path::PathBuf,
    dir: std::path::PathBuf,
}

impl Drop for AtRev {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

pub fn at_rev(project: &Path, rev: &str) -> Result<AtRev> {
    use std::path::Component;
    let here = project
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let name = project.file_name().ok_or("the project has no file name")?;
    let show = |rel: &str| -> Result<Vec<u8>> {
        let o = std::process::Command::new("git")
            .arg("show")
            .arg(format!("{rev}:./{rel}"))
            .current_dir(here)
            .output()
            .map_err(|e| format!("git: {e}"))?;
        if !o.status.success() {
            return Err(format!(
                "git show {rev}:{rel}: {}",
                String::from_utf8_lossy(&o.stderr).trim()
            ));
        }
        Ok(o.stdout)
    };
    let text = show(&name.to_string_lossy())?;
    let dir = here.join(format!(".mui-cut-rev-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let out = AtRev {
        path: dir.join(name),
        dir: dir.clone(),
    };
    std::fs::write(&out.path, &text).map_err(|e| format!("{}: {e}", out.path.display()))?;
    let p = Project::load(&String::from_utf8_lossy(&text))
        .map_err(|e| format!("{}@{rev}: {e}", project.display()))?;
    let hdris = p
        .scenes
        .iter()
        .filter_map(|s| s.environment.as_ref())
        .map(|e| e.hdri.as_str());
    let assets = p
        .scenes
        .iter()
        .flat_map(|s| &s.layers)
        .filter_map(|l| p.asset_of(l))
        .chain(hdris);
    for rel in assets {
        // Only paths inside the project's folder: the copy must not write
        // outside its own.
        if rel.is_empty()
            || !Path::new(rel)
                .components()
                .all(|c| matches!(c, Component::Normal(_)))
        {
            continue;
        }
        let bytes = show(rel).or_else(|_| std::fs::read(here.join(rel)).map_err(|e| e.to_string()));
        if let Ok(bytes) = bytes {
            let to = dir.join(rel);
            if let Some(d) = to.parent() {
                std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
            }
            std::fs::write(&to, bytes).map_err(|e| format!("{}: {e}", to.display()))?;
        }
    }
    #[cfg(unix)]
    {
        let cache = here.join(mui_cut::plugin::CACHE);
        if cache.is_dir() {
            let _ = std::os::unix::fs::symlink(
                std::fs::canonicalize(&cache).unwrap_or(cache),
                dir.join(mui_cut::plugin::CACHE),
            );
        }
    }
    Ok(out)
}
