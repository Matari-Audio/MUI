use mui::{geometry::PathCommand, prelude::*};
use serde::{Deserialize, Serialize};
use vello::kurbo::{Affine, Arc, BezPath, Circle, Rect, RoundedRect, Shape};

pub const WIDTH: u32 = 768;
pub const HEIGHT: u32 = 512;
#[derive(Clone, Serialize, Deserialize)]
pub struct Mark {
    pub name: String,
    pub path: String,
    pub color: [u8; 4],
    pub gradient: Option<[u8; 4]>,
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
                result.push(m);
            }
        }
    }
    Ok(result)
}
