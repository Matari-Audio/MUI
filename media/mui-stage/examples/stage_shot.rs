//! A four-second 3D shot of a live MUI card: it flies in from deep space as a
//! lit, extruded slab, turns while its knob sweeps and its toggle flips, over
//! a WGSL background and a glossy floor that mirrors it, with depth of
//! field, bloom, grain and 8-subframe motion blur.
//!
//!     cargo run --manifest-path media/Cargo.toml -p mui-stage --example stage_shot --release -- /tmp/stage
//!
//! Writes `take.mp4` (BT.709, converted and tagged) when ffmpeg is on PATH,
//! and `sheet.png`, five frames stacked, either way.

use std::io::Write as _;
use std::process::{Command, Stdio};

use mui::motion::{Ease, Keys};
use mui::prelude::*;
use mui_stage::{Camera, Floor, Plane, Post, Shot, Stage};

const W: u32 = 1280;
const H: u32 = 720;
const FPS: u32 = 60;
const SECONDS: f64 = 4.;
const CARD: Size = Size {
    width: 360.,
    height: 200.,
};

fn card(ui: &mut Ui, t: f64) -> El {
    let mut gain = Keys::new(0.1)
        .hold(1.3)
        .to(2.6, 0.9, Ease::EMPHASIZED)
        .at(t);
    let (dial, _) = knob(ui, "gain", "Gain", &mut gain, 0.0..=1.0);
    let mut on = t > 2.2;
    let (sw, _) = toggle(ui, "drive", &mut on);
    let meter = (gain * 180.).max(6.);
    col([
        row([text("SATURN").text_size(20.), spacer(), sw.el()]).align(Align::Center),
        row([
            dial.size(Xl).el(),
            col([
                text(format!("{:+.1} dB", -24. + gain * 30.)).text_size(15.),
                block(meter, 8.).pill().fill(Role::Primary).animate_layout(),
            ])
            .gap(S),
        ])
        .gap(L)
        .align(Align::Center),
    ])
    .gap(M)
    .pad(L)
    .size(CARD.width, CARD.height)
    .radius(26.)
    .fill(Role::Surface)
    .id("card")
}

fn shot(t: f64) -> Shot {
    let fly = Keys::new(0.).to(1.3, 1., Ease::EMPHASIZED);
    let turn = Keys::new(0.).hold(1.3).to(3.6, 1., Ease::IN_OUT);
    let (f, u) = (fly.at(t) as f32, turn.at(t) as f32);
    let mut cam = Camera::front(360., 35.).orbit(-14. * u, 8. * u);
    cam.roll = 3. * (1. - f);
    Shot {
        planes: vec![
            Plane::new("card", CARD.width as f32, CARD.height as f32)
                .at(0., 0., -1400. * (1. - f))
                .rotate(25. * (1. - f), -60. * (1. - f) + 22. * u, 0.)
                .depth(22.)
                .edge([0.08, 0.06, 0.2])
                .glow(1.25),
        ],
        floor: Some(Floor::at(-CARD.height as f32 / 2. - 30.)),
        post: Post {
            bloom: 0.8,
            aberration: 0.003,
            // Sharp where it lands; soft while it is still far off.
            focus: cam.distance(),
            aperture: 20.,
            ..Post::default()
        },
        ..Shot::new(cam)
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = std::path::PathBuf::from(std::env::args().nth(1).unwrap_or("stage".into()));
    std::fs::create_dir_all(&dir)?;
    let mut stage = Stage::new(W, H)?;
    let mut ui = Ui::new(Theme::DEFAULT).font(Font::new(epaint_default_fonts::HACK_REGULAR)?);
    let mut ffmpeg = Command::new("ffmpeg")
        .args([
            "-y",
            "-loglevel",
            "error",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgba",
        ])
        .args(["-color_range", "pc", "-colorspace", "rgb"])
        .args(["-s", &format!("{W}x{H}"), "-r", &FPS.to_string(), "-i", "-"])
        .args([
            "-vf",
            "scale=out_color_matrix=bt709:out_range=tv,\
            setparams=colorspace=bt709:color_primaries=bt709:color_trc=bt709:range=tv",
        ])
        .args([
            "-c:v",
            "libx264",
            "-profile:v",
            "high",
            "-pix_fmt",
            "yuv420p",
            "-crf",
            "14",
        ])
        .args([
            "-colorspace",
            "bt709",
            "-color_primaries",
            "bt709",
            "-color_trc",
            "bt709",
        ])
        .arg(dir.join("take.mp4"))
        .stdin(Stdio::piped())
        .spawn()
        .ok();
    let frames = (SECONDS * f64::from(FPS)) as usize;
    let keep = [0, frames / 6, frames / 3, frames / 2, frames - 1];
    let mut sheet = Vec::new();
    let started = std::time::Instant::now();
    for i in 0..frames {
        let t = i as f64 / f64::from(FPS);
        let root = card(&mut ui, t);
        let f = ui.frame(
            root,
            Some(CARD),
            PointerInput::default(),
            1. / f64::from(FPS),
        )?;
        let outline = f.scene.surface("card").map(|s| s.path.clone());
        stage.layer("card", f.scene, CARD, 3.)?;
        let frame = stage.render(t, 0.5 / f64::from(FPS), 8, &|at| {
            let mut s = shot(at);
            if let Some(o) = &outline {
                s.planes[0] = s.planes[0].clone().outline(o.clone());
            }
            s
        })?;
        let rgba = frame.rgba8();
        if let Some(stdin) = ffmpeg.as_mut().and_then(|c| c.stdin.as_mut()) {
            stdin.write_all(&rgba)?;
        }
        if keep.contains(&i) {
            sheet.extend_from_slice(&rgba);
        }
    }
    if let Some(mut c) = ffmpeg {
        drop(c.stdin.take());
        c.wait()?;
    }
    let file = std::fs::File::create(dir.join("sheet.png"))?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), W, H * keep.len() as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.write_header()?.write_image_data(&sheet)?;
    println!(
        "{frames} frames in {:.1}s -> {}",
        started.elapsed().as_secs_f64(),
        dir.display()
    );
    Ok(())
}
