//! Render a MUI scene to a PNG with no window, no egui and no tessellator.
//!
//!     cargo run -p mui-vello --example headless -- /tmp/pill.png
//!
//! The whole stack end to end: a styled tree resolves to frames, welds and
//! shells become filleted outlines, [`mui_vello::paint`] hands the z-ordered
//! paint list to Vello, and Vello antialiases it analytically. It writes a
//! file instead of opening a window only because a window is a separate
//! problem: a surface would get the same pixels.

use mui_scene::Corners;
use mui_scene::prelude::*;
use mui_vello::effects::{Budget, GpuRenderer};
use mui_vello::kurbo::Affine;
#[allow(dead_code)]
mod gpu_support;

const SIZE: [u32; 2] = [640, 360];

fn spec() -> SceneSpec {
    let control = |id: &str| leaf(28.0, 28.0).pill().fill(Role::Primary).id(id);
    let tab = column([control("plus"), control("phase"), control("warp")])
        .gap(10.0)
        .pad(22.0)
        .min_width(92.0)
        .align(Align::Center)
        .id("tab")
        .shell(12.0, Role::Raised);
    let panel = row([text("welded").text_size(22.0)])
        .size(520.0, 230.0)
        .pad(L)
        .id("panel");
    let root = column([tab, panel])
        .align(Align::Start)
        .id("root")
        .union(Role::Surface);
    SceneSpec::new(root)
        .theme(Theme {
            corners: Corners {
                box_: 28.0,
                concave: 32.0,
                ..Corners::DEFAULT
            },
            ..Theme::DEFAULT
        })
        .font(Font::new(epaint_default_fonts::HACK_REGULAR).unwrap())
}

fn main() -> gpu_support::Result<()> {
    let out = std::env::args().nth(1).unwrap_or("mui-vello.png".into());
    let resolved = resolve_scene(&spec()).expect("scene resolves");
    let rgba = pollster::block_on(rasterise(&resolved))?;
    gpu_support::save(out.as_ref(), SIZE, &rgba)?;
    println!(
        "wrote {out}: {} surfaces, {} paint ops",
        resolved.surfaces().count(),
        resolved.paint.len()
    );
    Ok(())
}

async fn rasterise(resolved: &mui_scene::ResolvedScene) -> gpu_support::Result<Vec<u8>> {
    let (_, device, queue) = gpu_support::device().await?;
    let texture = gpu_support::target(&device, SIZE);
    let format = texture.format();
    let mut renderer = GpuRenderer::new(&device, &queue, format, SIZE, Budget::default()).await?;
    let view = texture.create_view(&Default::default());
    renderer.render(resolved, Affine::translate((32.0, 32.0)), &view)?;
    gpu_support::readback(&device, &queue, &texture, SIZE)
}
