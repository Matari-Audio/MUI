use mui_geometry::*;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let outer = RoundedRect::new(Rect::new(0., 0., 92., 170.), 28.)?;
    let child = outer.inset(12.)?.shape.ok_or("child disappeared")?;
    println!(
        "Concentric: parent={} inset=12 child={}",
        outer.radius(),
        child.radius()
    );
    let topology = union(
        &[
            Polygon::rectangle(0., 150., 570., 230.)?.into(),
            Polygon::rectangle(0., 0., 92., 170.)?.into(),
        ],
        GeometryOptions::default(),
    )?;
    let surface = fillet(
        &topology,
        Fillet {
            convex_radius: 28.,
            concave_radius: 32.,
            ..Default::default()
        },
    )?;
    let inner = inset_path(&surface.path, 12., OffsetOptions::default())?;
    println!(
        "Merged contours: {} -> {}, counts changed: {}",
        topology.components(),
        inner.topology.components(),
        inner.counts_changed
    );
    let svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="-10 -10 590 400"><rect x="-10" y="-10" width="590" height="400" fill="#111616"/><path d="{}" fill="#45c8dd" fill-rule="evenodd"/><path d="{}" fill="#202b2e" fill-rule="evenodd"/></svg>"##,
        surface.path.to_svg_data()?,
        inner.path.to_svg_data()?
    );
    std::fs::write("merged-inset.svg", svg)?;
    Ok(())
}
