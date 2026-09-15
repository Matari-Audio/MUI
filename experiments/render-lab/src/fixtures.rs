use mui::{geometry::PathCommand, prelude::*};
use serde::{Deserialize, Serialize};
use vello::kurbo::{Affine, Arc, BezPath, Circle, Rect, RoundedRect, Shape};

#[allow(dead_code)]
#[path = "../../gpui-plugin/src/pie_container.rs"]
mod pie_container;

pub const WIDTH: u32 = 768;
pub const HEIGHT: u32 = 512;
#[derive(Clone, Serialize, Deserialize)]
pub struct Mark {
    pub name: String,
    pub path: String,
    pub color: [u8; 4],
    pub gradient: Option<[u8; 4]>,
    #[serde(default)]
    pub vertical_gradient: bool,
    #[serde(default)]
    pub oklab: bool,
    #[serde(default)]
    pub radius: f64,
    pub stroke: Option<f64>,
    pub clip: Option<[f64; 4]>,
    pub native_rect: Option<[f64; 4]>,
}
fn mark(name: &str, path: BezPath, color: [u8; 4]) -> Mark {
    Mark {
        name: name.into(),
        path: path.to_svg(),
        color,
        gradient: None,
        vertical_gradient: false,
        oklab: false,
        radius: 0.,
        stroke: None,
        clip: None,
        native_rect: None,
    }
}
pub fn fixture() -> Result<Vec<Mark>, Box<dyn std::error::Error>> {
    let mut out = vec![mark(
        "canvas",
        Rect::new(0., 0., 768., 512.).to_path(0.001),
        [20, 20, 24, 255],
    )];
    // Equal native rectangular color/gradient/alpha primitives, not path-only proxies.
    for (i, (a, b)) in [
        ([255, 255, 255, 255], None),
        ([128, 128, 128, 255], None),
        ([255, 0, 0, 255], None),
        ([0, 255, 0, 255], None),
        ([0, 0, 255, 255], None),
        ([255, 255, 255, 128], None),
        ([0, 0, 0, 255], Some([255, 255, 255, 255])),
        ([255, 0, 0, 255], Some([0, 0, 255, 255])),
    ]
    .into_iter()
    .enumerate()
    {
        let r = [16. + i as f64 * 94., 16., 16. + i as f64 * 94. + 82., 82.];
        let mut m = mark(
            &format!("patch-{i}"),
            Rect::new(r[0], r[1], r[2], r[3]).to_path(0.001),
            a,
        );
        m.gradient = b;
        m.native_rect = Some(r);
        out.push(m);
    }
    let ui = container([
        item("tab").width(100.).height(36.).extend_to("panel"),
        item("panel").width(330.).height(135.),
    ])
    .layout(Column)
    .gap(18.)
    .round(Rounding::separate(20., 14.))
    .merge(["tab", "panel"])
    .build()?;
    let scene = ui.resolve()?;
    let merged = ui
        .outlines(&scene)
        .last()
        .ok_or("missing merged outline")?
        .0;
    let mut p = BezPath::new();
    for cmd in &merged.path.commands {
        match *cmd {
            PathCommand::MoveTo(q) => p.move_to((q.x, q.y)),
            PathCommand::LineTo(q) => p.line_to((q.x, q.y)),
            PathCommand::Close => p.close_path(),
            PathCommand::ArcTo(a) => Arc::new(
                (a.center.x, a.center.y),
                (a.radius, a.radius),
                a.start_angle,
                a.sweep,
                0.,
            )
            .to_cubic_beziers(0.001, |a, b, c| p.curve_to(a, b, c)),
        }
    }
    p.apply_affine(Affine::translate((18.25, 112.5)));
    out.push(mark("merged-concave", p, [174, 151, 240, 255]));
    // Fill-rule hole, concave star and rotations.
    let mut hole = Rect::new(390., 112., 510., 224.).to_path(0.001);
    hole.extend(
        Rect::new(415., 137., 485., 199.)
            .to_path(0.001)
            .reverse_subpaths()
            .elements()
            .iter()
            .copied(),
    );
    out.push(mark("hole-nonzero", hole, [100, 220, 190, 255]));
    let mut star = BezPath::new();
    for i in 0..10 {
        let a = i as f64 * std::f64::consts::PI / 5.;
        let r = if i % 2 == 0 { 58. } else { 22. };
        let p = (610. + a.cos() * r, 168. + a.sin() * r);
        if i == 0 {
            star.move_to(p)
        } else {
            star.line_to(p)
        }
    }
    star.close_path();
    out.push(mark("concave-star", star, [246, 180, 72, 230]));
    for i in 0..12 {
        let mut m = mark(
            &format!("thin-{i}"),
            Circle::new((30. + i as f64 * 62., 370.), 24.25).to_path(0.001),
            [230, 230, 235, 255],
        );
        m.stroke = Some([0.25, 0.5, 1., 1.5][i % 4]);
        out.push(m);
    }
    for i in 0..16 {
        let mut p = RoundedRect::new(0., 0., 25., 25., 5.).to_path(0.001);
        p.apply_affine(
            Affine::translate((24. + i as f64 * 47., 446.)) * Affine::rotate(i as f64 * 0.13),
        );
        let mut m = mark(&format!("rotated-{i}"), p, [90, 170, 245, 200]);
        m.clip = Some([10., 417., 752., 480.]);
        out.push(m);
    }
    Ok(out)
}

pub fn tiled(marks: &[Mark], side: usize) -> Result<Vec<Mark>, Box<dyn std::error::Error>> {
    if side == 0 || side > 8 {
        return Err("tile side must be 1..8".into());
    }
    let s = 1. / side as f64;
    let mut result = Vec::new();
    for y in 0..side {
        for x in 0..side {
            for m in marks {
                let tx = x as f64 * WIDTH as f64 * s;
                let ty = y as f64 * HEIGHT as f64 * s;
                let mut m = m.clone();
                let mut path = BezPath::from_svg(&m.path)?;
                path.apply_affine(Affine::translate((tx, ty)) * Affine::scale(s));
                m.path = path.to_svg();
                let map =
                    |r: [f64; 4]| [r[0] * s + tx, r[1] * s + ty, r[2] * s + tx, r[3] * s + ty];
                m.clip = m.clip.map(map);
                m.native_rect = m.native_rect.map(map);
                m.stroke = m.stroke.map(|w| w * s);
                m.radius *= s;
                result.push(m);
            }
        }
    }
    Ok(result)
}

/// Snapshot of the composition's actual port geometry and ADSR curve, with
/// fractional translations and native-vs-path circles beside it.
pub fn components() -> Result<Vec<Mark>, Box<dyn std::error::Error>> {
    use mui::geometry::{Point, Polygon};
    let mut out = vec![mark(
        "canvas",
        Rect::new(0., 0., 768., 512.).to_path(0.001),
        [20, 20, 24, 255],
    )];
    for i in 0..4 {
        let dx = i as f64 * 180. + i as f64 * 0.25;
        let dy = i as f64 * 0.25;
        let origin = Point::new(70. + dx, 110. + dy);
        let port = pie_container::PieContainer::port(8, 80., true);
        let mut shapes = port.outer_shapes(origin)?;
        shapes.push(Polygon::rectangle(origin.x + 12., origin.y - 19., 70., 120.)?.into());
        let convert = |path: &mui::geometry::Path| {
            let mut p = BezPath::new();
            for cmd in &path.commands {
                match *cmd {
                    PathCommand::MoveTo(q) => p.move_to((q.x, q.y)),
                    PathCommand::LineTo(q) => p.line_to((q.x, q.y)),
                    PathCommand::Close => p.close_path(),
                    PathCommand::ArcTo(a) => Arc::new(
                        (a.center.x, a.center.y),
                        (a.radius, a.radius),
                        a.start_angle,
                        a.sweep,
                        0.,
                    )
                    .to_cubic_beziers(0.001, |a, b, c| p.curve_to(a, b, c)),
                }
            }
            p
        };
        let shell = convert(&pie_container::rounded(&shapes)?);
        out.push(mark(
            &format!("port-shell-{i}"),
            shell.clone(),
            [44, 44, 48, 255],
        ));
        let mut edge = mark(&format!("port-border-{i}"), shell, [135, 135, 140, 255]);
        edge.stroke = Some(1.);
        out.push(edge);
        let mut well = convert(&port.well()?.ok_or("missing well")?);
        well.apply_affine(Affine::translate((origin.x, origin.y)));
        out.push(mark(&format!("port-well-{i}"), well, [20, 20, 24, 255]));
        for n in 0..8 {
            let p = port.slot(n);
            let (x, y) = (origin.x + p.x, origin.y + p.y);
            let mut m = mark(
                &format!("port-dot-{i}-{n}"),
                Circle::new((x, y), 9.).to_path(0.001),
                [145, 145, 150, 255],
            );
            m.native_rect = Some([x - 9., y - 9., x + 9., y + 9.]);
            m.radius = 9.;
            out.push(m);
        }
        // The group header's default ADSR shape, including translucent vertical fill.
        let mut env = BezPath::new();
        env.move_to((0., 1.));
        env.line_to((0.08 + 0.08 * 0.18, 0.));
        env.curve_to((0.3, 0.), (0.33, 0.3), (0.30 + 0.25 * 0.30, 0.3));
        env.line_to((0.7, 0.3));
        env.curve_to((0.8, 0.3), (0.82 + 0.4 * 0.15, 1.), (0.80 + 0.4 * 0.2, 1.));
        env.apply_affine(
            Affine::translate((15. + dx, 25. + dy)) * Affine::scale_non_uniform(150., 46.),
        );
        let mut fill = env.clone();
        fill.close_path();
        let mut m = mark(&format!("adsr-fill-{i}"), fill, [220, 230, 240, 82]);
        m.gradient = Some([220, 230, 240, 0]);
        m.vertical_gradient = true;
        out.push(m);
        let mut m = mark(&format!("adsr-edge-{i}"), env, [220, 230, 240, 255]);
        m.stroke = Some(1.5);
        out.push(m);
        for (row, width) in [0.25, 0.5, 1., 1.5].into_iter().enumerate() {
            for native in [false, true] {
                let (x, y) = (
                    45. + dx + if native { 65. } else { 0. },
                    280. + row as f64 * 52. + dy,
                );
                let mut m = mark(
                    &format!("circle-{i}-{row}-{native}"),
                    Circle::new((x, y), 20.).to_path(0.001),
                    [230, 230, 235, 255],
                );
                m.stroke = Some(width);
                if native {
                    m.native_rect = Some([x - 20., y - 20., x + 20., y + 20.]);
                    m.radius = 20.;
                }
                out.push(m);
            }
        }
    }
    // Matching colored endpoint ramp catches transfer errors hidden by black/white stops.
    for (i, native) in [false, true].into_iter().enumerate() {
        let r = [16. + i as f64 * 376., 475., 366. + i as f64 * 376., 505.];
        let mut m = mark(
            &format!("colored-ramp-{native}"),
            Rect::new(r[0], r[1], r[2], r[3]).to_path(0.001),
            [64, 128, 192, 255],
        );
        m.gradient = Some([192, 64, 128, 255]);
        if native {
            m.native_rect = Some(r);
        }
        out.push(m.clone());
        let mut p = BezPath::from_svg(&m.path)?;
        p.apply_affine(Affine::translate((0., -250.)));
        m.path = p.to_svg();
        m.native_rect = m
            .native_rect
            .map(|r| [r[0], r[1] - 250., r[2], r[3] - 250.]);
        m.name = format!("oklab-ramp-{native}");
        m.oklab = true;
        out.push(m);
    }
    Ok(out)
}
