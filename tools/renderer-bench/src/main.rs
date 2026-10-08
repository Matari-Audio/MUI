//! Draw+readback microbenchmark; not a DAW, text, layout or presentation benchmark.
use anyhow::{Context, Result, ensure};
use serde_json::json;
use std::{fs::File, io::Write, time::Instant};
#[cfg(feature = "classic")]
mod classic;
#[cfg(feature = "vello-cpu")]
mod cpu;
#[cfg(any(feature = "classic", feature = "vello-gpu"))]
mod gpu;
#[cfg(feature = "gpui")]
mod gpui;
#[cfg(feature = "vello-gpu")]
mod hybrid;
#[cfg(feature = "skia-cpu")]
mod skia;

pub enum Shape {
    Rect {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        color: [u8; 4],
    },
    Path {
        points: Vec<[f64; 2]>,
        width: f64,
        color: [u8; 4],
    },
}
impl Shape {
    pub fn color(&self) -> [u8; 4] {
        match self {
            Self::Rect { color, .. } | Self::Path { color, .. } => *color,
        }
    }
}
#[cfg(any(feature = "classic", feature = "vello-cpu"))]
fn path(points: &[[f64; 2]]) -> kurbo::BezPath {
    let mut path = kurbo::BezPath::new();
    path.move_to((points[0][0], points[0][1]));
    for p in &points[1..] {
        path.line_to((p[0], p[1]));
    }
    path
}

fn fixture(workload: &str, scale: f64, phase: f64) -> Vec<Shape> {
    let mut shapes = Vec::new();
    let mut rect = |x, y, w, h, color| {
        shapes.push(Shape::Rect {
            x: x * scale,
            y: y * scale,
            w: w * scale,
            h: h * scale,
            color,
        })
    };
    rect(4., 4., 12., 12., [255, 0, 255, 255]);
    for i in 0..40 {
        let x = 24. + f64::from(i % 10) * 124.;
        let y = 32. + f64::from(i / 10) * 150.;
        rect(x, y, 110., 130., [24, 28, 34, 255]);
        rect(
            x + 12.,
            y + 106.,
            70. + phase.sin() * 10.,
            6.,
            [68, 184, 170, 255],
        );
    }
    let count = if workload == "vectors" { 96 } else { 8 };
    for i in 0..count {
        let x = 28. + f64::from(i % 8) * 154.;
        let y = 52. + f64::from(i / 8) * (680. / f64::from(count / 8));
        let points = (0..128)
            .map(|p| {
                let t = f64::from(p) / 127.;
                [
                    scale * (x + t * 140.),
                    scale
                        * (y + 30.
                            + (t * std::f64::consts::TAU * 3. + phase + f64::from(i)).sin() * 26.),
                ]
            })
            .collect();
        shapes.push(Shape::Path {
            points,
            width: 2. * scale,
            color: [220, 190, 100, 255],
        });
    }
    shapes
}

enum Renderer {
    #[cfg(feature = "classic")]
    Classic(classic::Renderer),
    #[cfg(feature = "vello-gpu")]
    Hybrid(hybrid::Renderer),
    #[cfg(feature = "vello-cpu")]
    Cpu(cpu::Renderer),
    #[cfg(feature = "skia-cpu")]
    Skia(skia::Renderer),
    #[cfg(feature = "gpui")]
    Gpui(gpui::Renderer),
}
impl Renderer {
    fn new(name: &str, w: u32, h: u32) -> Result<Self> {
        Ok(match name {
            #[cfg(feature = "classic")]
            "classic" => Self::Classic(pollster::block_on(classic::Renderer::new(w, h))?),
            #[cfg(feature = "vello-gpu")]
            "vello-gpu" => Self::Hybrid(pollster::block_on(hybrid::Renderer::new(w, h))?),
            #[cfg(feature = "vello-cpu")]
            "vello-cpu" => Self::Cpu(cpu::Renderer::new(w, h)),
            #[cfg(feature = "skia-cpu")]
            "skia-cpu" => Self::Skia(skia::Renderer::new(w, h)?),
            #[cfg(feature = "gpui")]
            "gpui" => Self::Gpui(gpui::Renderer::new(w, h)?),
            _ => anyhow::bail!("unknown or disabled renderer {name}"),
        })
    }
    fn draw(&mut self, shapes: &[Shape]) -> Result<Vec<u8>> {
        match self {
            #[cfg(feature = "classic")]
            Self::Classic(r) => r.draw(shapes),
            #[cfg(feature = "vello-gpu")]
            Self::Hybrid(r) => r.draw(shapes),
            #[cfg(feature = "vello-cpu")]
            Self::Cpu(r) => r.draw(shapes),
            #[cfg(feature = "skia-cpu")]
            Self::Skia(r) => r.draw(shapes),
            #[cfg(feature = "gpui")]
            Self::Gpui(r) => r.draw(shapes),
        }
    }
}
fn validate(pixels: &[u8], w: u32, h: u32, scale: u32) -> Result<()> {
    ensure!(
        pixels.len() == (w * h * 4) as usize,
        "incorrect readback length"
    );
    let offset = ((8 * scale * w + 8 * scale) * 4) as usize;
    ensure!(
        pixels[offset..offset + 4] == [255, 0, 255, 255],
        "sentinel missing or wrong channel order"
    );
    let corner = ((w * h - 1) * 4) as usize;
    ensure!(
        pixels[corner..corner + 4] == [0, 0, 0, 255],
        "opaque black background missing"
    );
    ensure!(
        pixels.chunks_exact(4).all(|p| p[3] == 255),
        "unexpected transparent pixels"
    );
    ensure!(
        pixels
            .chunks_exact(4)
            .filter(|p| p[0] > 180 && p[1] > 130 && p[2] < 130)
            .count()
            > 1000,
        "waveforms missing"
    );
    Ok(())
}
fn rss_kib() -> Option<u64> {
    std::fs::read_to_string("/proc/self/status")
        .ok()?
        .lines()
        .find(|l| l.starts_with("VmHWM:"))?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()
}
fn cpu_ms() -> Option<f64> {
    #[cfg(target_os = "linux")]
    {
        let mut time = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        // CLOCK_PROCESS_CPUTIME_ID includes renderer/driver worker threads.
        if unsafe { libc::clock_gettime(libc::CLOCK_PROCESS_CPUTIME_ID, &mut time) } == 0 {
            return Some(time.tv_sec as f64 * 1000. + time.tv_nsec as f64 / 1_000_000.);
        }
    }
    None
}
fn stage(file: &mut File, stage: &str, mode: &str, sample: i32) -> Result<()> {
    writeln!(
        file,
        "{}",
        json!({"stage":stage,"mode":mode,"sample":sample})
    )?;
    file.flush()?;
    Ok(())
}
fn main() -> Result<()> {
    env_logger::init();
    let args: Vec<_> = std::env::args().collect();
    ensure!(
        args.len() == 5,
        "usage: mui-renderer-bench RENDERER controls|vectors SCALE OUTPUT_DIR"
    );
    let name = &args[1];
    let workload = &args[2];
    ensure!(
        matches!(workload.as_str(), "controls" | "vectors"),
        "unknown workload"
    );
    let scale: u32 = args[3].parse()?;
    ensure!((1..=2).contains(&scale), "scale must be 1 or 2");
    let (w, h) = (1280 * scale, 800 * scale);
    let directory = std::path::Path::new(&args[4]);
    std::fs::create_dir_all(directory)?;
    let mut output = File::create(directory.join("samples.jsonl"))?;
    let mut stages = File::create(directory.join("stages.jsonl"))?;
    stage(&mut stages, "initialize", "first", -1)?;
    let start = Instant::now();
    let mut renderer = Renderer::new(name, w, h).context("renderer initialization")?;
    let startup_ms = start.elapsed().as_secs_f64() * 1000.;
    let first_shapes = fixture(workload, f64::from(scale), 0.);
    stage(&mut stages, "draw_readback", "first", -1)?;
    let start = Instant::now();
    let first = renderer.draw(&first_shapes)?;
    let first_ms = start.elapsed().as_secs_f64() * 1000.;
    stage(&mut stages, "validate_pixels", "first", -1)?;
    validate(&first, w, h, scale).context("first frame pixel checks")?;
    stage(&mut stages, "write_image", "first", -1)?;
    let mut png = png::Encoder::new(File::create(directory.join("first.png"))?, w, h);
    png.set_color(png::ColorType::Rgba);
    png.set_depth(png::BitDepth::Eight);
    png.write_header()?.write_image_data(&first)?;
    for mode in ["static-redraw", "dynamic-redraw"] {
        let mut samples = Vec::new();
        let mut cpu_samples = Vec::new();
        for i in 0..35 {
            // Fixture generation is excluded; each engine's scene encoding is included.
            let shapes = fixture(
                workload,
                f64::from(scale),
                if mode == "dynamic-redraw" {
                    f64::from(i + 1) * 0.13
                } else {
                    0.
                },
            );
            stage(&mut stages, "draw_readback", mode, i)?;
            let cpu_start = cpu_ms();
            let start = Instant::now();
            let pixels = renderer.draw(&shapes)?;
            let ms = start.elapsed().as_secs_f64() * 1000.;
            let cpu = cpu_start.zip(cpu_ms()).map(|(start, end)| end - start);
            stage(&mut stages, "validate_pixels", mode, i)?;
            validate(&pixels, w, h, scale).context("redraw pixel checks")?;
            if mode == "dynamic-redraw" {
                ensure!(pixels != first, "animation did not change pixels");
            }
            if i >= 5 {
                samples.push(ms);
                if let Some(cpu) = cpu {
                    cpu_samples.push(cpu);
                }
                writeln!(
                    output,
                    "{}",
                    json!({"renderer":name,"workload":workload,"scale":scale,"mode":mode,"sample":i-5,"draw_readback_ms":ms,"process_cpu_ms":cpu})
                )?;
                output.flush()?;
            }
        }
        stage(&mut stages, "summarize", mode, -1)?;
        samples.sort_by(f64::total_cmp);
        cpu_samples.sort_by(f64::total_cmp);
        let idle_cpu_start = cpu_ms();
        let idle_start = Instant::now();
        std::thread::sleep(std::time::Duration::from_millis(100));
        let idle_cpu_ms = idle_cpu_start.zip(cpu_ms()).map(|(start, end)| end - start);
        let idle_wall_ms = idle_start.elapsed().as_secs_f64() * 1000.;
        let row = json!({"renderer":name,"workload":workload,"scale":scale,"mode":mode,"startup_ms":startup_ms,"first_draw_readback_ms":first_ms,"median_ms":samples[15],"p95_ms":samples[28],"process_cpu_median_ms":cpu_samples.get(cpu_samples.len()/2),"idle_process_cpu_ms":idle_cpu_ms,"idle_wall_ms":idle_wall_ms,"samples":samples.len(),"peak_rss_kib":rss_kib(),"width":w,"height":h,"pixel_checks":"passed","scope":"scene encoding + completed raster + CPU RGBA readback; no native text/layout/presentation/effects; idle covers headless renderer workers only"});
        println!("{row}");
        writeln!(output, "{row}")?;
        output.flush()?;
    }
    stage(&mut stages, "complete", "all", -1)?;
    Ok(())
}
