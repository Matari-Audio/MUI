use mui::{geometry::PathCommand, prelude::*};
use std::time::Instant;
pub fn run(out: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    let mut seed = 0x5eed_u64;
    let mut next = || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        ((seed >> 32) as u32) as f64 / u32::MAX as f64
    };
    let mut samples = Vec::new();
    let mut arcs = 0;
    let mut commands = 0;
    let mut worst_bridge_error: f64 = 0.;
    for _ in 0..1200 {
        let width = 80. + next() * 500.;
        let tab = 12. + next() * (width - 12.);
        let height = 12. + next() * 90.;
        let gap = next() * 48.;
        let radius = next() * 80.;
        let start = Instant::now();
        let ui = container([
            item("tab")
                .width(tab)
                .height(height)
                .extend_to("panel")
                .on_tap("select"),
            item("panel").width(width).height(100.),
        ])
        .layout(Column)
        .gap(gap)
        .round(Rounding::separate(radius, radius * 0.7))
        .merge(["tab", "panel"])
        .build()?;
        let scene = ui.resolve()?;
        let panel = scene.layout.frame("panel").unwrap();
        let original = scene.layout.frame("tab").unwrap();
        let shape = scene.surface("tab").unwrap().bounds.unwrap();
        worst_bridge_error = worst_bridge_error.max((shape.max.y - panel.y).abs());
        assert!(
            (shape.max.y - panel.y).abs() < 0.001,
            "extension does not reach target"
        );
        assert_eq!(
            ui.tap_at(
                &scene,
                original.x + original.size.width / 2.,
                original.y + original.size.height / 2.
            ),
            Some("select")
        );
        if gap > 0.01 {
            assert_eq!(
                ui.tap_at(
                    &scene,
                    original.x + original.size.width / 2.,
                    original.bottom() + gap / 2.
                ),
                None,
                "bridge stole hit area"
            );
        }
        for (surface, _) in ui.outlines(&scene) {
            surface.path.validate(10000)?;
            commands += surface.path.commands.len();
            arcs += surface
                .path
                .commands
                .iter()
                .filter(|c| matches!(c, PathCommand::ArcTo(_)))
                .count();
            let rings = surface.path.flatten(0.02, 100000)?;
            assert!(rings
                .iter()
                .flatten()
                .all(|p| p.x.is_finite() && p.y.is_finite()));
        }
        samples.push(start.elapsed().as_secs_f64() * 1000.);
    }
    let result = serde_json::json!({"seed":"0x5eed","cases":1200,"validated_commands":commands,"validated_arcs":arcs,"max_extension_error":worst_bridge_error,"build_resolve_validate_flatten_ms":crate::stats(&mut samples),"scope":"MUI geometry/layout correctness and cost; not renderer or GPUI runtime performance"});
    std::fs::write(
        out.join("geometry.json"),
        serde_json::to_vec_pretty(&result)?,
    )?;
    println!("{result}");
    Ok(())
}
