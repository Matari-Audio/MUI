//! A million uniform rows and 200,000 variable rows, rendered without a window.
//!
//! cargo run -p mui --features cpu --example large_lists -- /tmp/large-lists.png
//! Pass a second argument to jump to a different uniform row (default 500000).
//! Reports actual row constructor counts, not timing or inferred speedups.
use mui::prelude::*;
use mui::vello::vello_cpu::{Pixmap, RenderContext, Resources};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "large-lists.png".into());
    let destination: usize = std::env::args()
        .nth(2)
        .as_deref()
        .unwrap_or("500000")
        .parse()?;
    let mut ui = Ui::default().font(Font::new(epaint_default_fonts::HACK_REGULAR)?);
    let mut uniform = ListState::uniform_count(1_000_000, 32.0);
    let mut variable = ListState::variable(1_000_000..1_200_000, 40.0);
    uniform.focus_item(destination.min(uniform.len() - 1));
    variable.scroll_to_item(destination.min(variable.len() - 1), ScrollTo::Start);
    let size = Size::new(760.0, 480.0);
    let opts = ListOptions {
        overscan: 48.0,
        ..ListOptions::new(350.0, 380.0)
    };
    for pass in 0..2 {
        let left = uniform_list(&mut ui, "uniform", &mut uniform, opts, |_, item| {
            col([text(format!("Row {} · key {}", item.index, item.key)).text_size(14.0)])
                .pad(8.0)
                .fill(if item.active {
                    Role::Primary
                } else if item.index % 2 == 0 {
                    Role::Surface
                } else {
                    Role::Background
                })
        });
        let right = variable_list(&mut ui, "variable", &mut variable, opts, |_, item| {
            let tall = item.index % 3 == 0;
            let height = if tall { 64.0 } else { 32.0 };
            ListRow::new(
                col([
                    text(format!("Row {} · {}px", item.index, height)).text_size(14.0),
                    if tall {
                        text("A second line changes this row's height")
                            .text_size(12.0)
                            .fill(Role::Dim)
                    } else {
                        block(0.0, 0.0)
                    },
                ])
                .pad(8.0)
                .fill(if item.index % 2 == 0 {
                    Role::Surface
                } else {
                    Role::Background
                }),
                height,
            )
        });
        println!(
            "build {pass}: uniform {:?}: {} constructors / {} model rows; variable {:?}: {} constructors / {} model rows",
            left.changed.visible,
            left.changed.rows_built,
            uniform.len(),
            right.changed.visible,
            right.changed.rows_built,
            variable.len()
        );
        let root = col([
            text("Visible rows only").text_size(24.0),
            row([
                col([
                    text("1,000,000 uniform rows"),
                    left.el.named("Uniform collection"),
                ])
                .gap(8.0),
                col([
                    text("200,000 variable rows"),
                    right.el.named("Variable collection"),
                ])
                .gap(8.0),
            ])
            .gap(20.0),
        ])
        .pad(20.0)
        .gap(12.0)
        .fill(Role::Background);
        let frame = ui.frame(root, Some(size), Input::default(), 1.0 / 60.0)?;
        if pass == 1 {
            let mut ctx = RenderContext::new(760, 480);
            let mut resources = Resources::default();
            mui::vello::paint(
                &mut mui::vello::Cpu {
                    ctx: &mut ctx,
                    resources: &mut resources,
                    cache: &mut mui::vello::Cache::default(),
                },
                frame.scene,
                mui::vello::kurbo::Affine::IDENTITY,
            )?;
            ctx.flush();
            let mut pixmap = Pixmap::new(760, 480);
            ctx.render(&mut pixmap, &mut resources);
            let rgba: Vec<_> = pixmap
                .take_unpremultiplied()
                .iter()
                .flat_map(|p| [p.r, p.g, p.b, p.a])
                .collect();
            let mut encoder = png::Encoder::new(std::fs::File::create(&path)?, 760, 480);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder.write_header()?.write_image_data(&rgba)?;
        }
    }
    println!("wrote {path}");
    Ok(())
}
