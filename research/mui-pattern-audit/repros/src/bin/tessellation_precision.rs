fn main() {
    use mui_geometry::{Path, PathCommand, Point};
    let path = Path {
        commands: vec![
            PathCommand::MoveTo(Point::new(16_777_217.0, 0.0)),
            PathCommand::LineTo(Point::new(16_777_219.0, 0.0)),
            PathCommand::LineTo(Point::new(16_777_219.0, 2.0)),
            PathCommand::LineTo(Point::new(16_777_217.0, 2.0)),
            PathCommand::Close,
        ],
    };
    let mesh = mui_tessellate::Tessellator::default()
        .tessellate(&path, 0.1)
        .expect("tessellate");
    let min = mesh
        .positions
        .iter()
        .map(|p| p[0])
        .fold(f32::INFINITY, f32::min);
    let max = mesh
        .positions
        .iter()
        .map(|p| p[0])
        .fold(f32::NEG_INFINITY, f32::max);
    assert_eq!(max - min, 4.0);
    println!("CONFIRMED LIMITATION: 2-unit f64 width becomes 4 in f32 mesh coordinates");
}
