//! Bridge between resolved MUI geometry/paints and the portable welding backend.
//! There is no Vello dependency here. The backend image rides the existing image
//! paint path; geometry and hit testing use its companion field contour.
use crate::{Color, El, Fill, Fit, Frame, GradientKind, Image, Layer, Paint, SceneError};
use mui_geometry::{Path, Point, Rect, RoundedRect};
use mui_weld::{Brush, Color as WeldColor, Geometry, ImageFit, Source, Stop};
use std::collections::HashSet;
use std::sync::Arc;

pub(crate) struct MaterialWeld {
    pub outline: Arc<Path>,
    pub image_fill: Fill,
    pub image_rect: RoundedRect,
    pub members: HashSet<Arc<str>>,
    pub external: Option<crate::ExternalWeld>,
}
impl MaterialWeld {
    pub fn consumes(&self, key: &Arc<str>, layer: Layer) -> bool {
        self.members.contains(key) && matches!(layer, Layer::Fill | Layer::Stroke)
    }
}
pub(crate) fn color(c: Color) -> WeldColor {
    c.to_srgb().into()
}
pub(crate) fn paint(p: Paint, b: Rect) -> Brush {
    match p {
        Paint::Solid(c) => Brush::Solid(color(c)),
        Paint::Image { image, fit } => Brush::Image {
            image: mui_weld::Image {
                width: image.width,
                height: image.height,
                rgba: image.rgba.clone(),
            },
            bounds: b,
            fit: match fit {
                Fit::Fill => ImageFit::Fill,
                Fit::Cover => ImageFit::Cover,
                Fit::Contain => ImageFit::Contain,
            },
        },
        Paint::Gradient { kind, stops } => {
            let stops = stops
                .into_iter()
                .map(|(at, c)| Stop {
                    at: f64::from(at),
                    color: color(c),
                })
                .collect();
            let center = Point::new((b.x0 + b.x1) / 2.0, (b.y0 + b.y1) / 2.0);
            match kind {
                GradientKind::Linear { angle } => {
                    let a = angle.to_radians();
                    let d = Point::new(a.sin(), -a.cos());
                    let half = (d.x.abs() * b.width() + d.y.abs() * b.height()) / 2.0;
                    Brush::Linear {
                        from: Point::new(center.x - d.x * half, center.y - d.y * half),
                        to: Point::new(center.x + d.x * half, center.y + d.y * half),
                        stops,
                    }
                }
                GradientKind::Radial {
                    center: (x, y),
                    radius,
                } => Brush::Radial {
                    center: Point::new(b.x0 + x * b.width(), b.y0 + y * b.height()),
                    radius: radius * b.width().max(b.height()),
                    stops,
                },
                GradientKind::Conic { angle } => Brush::Conic {
                    center,
                    angle,
                    stops,
                },
            }
        }
    }
}

/// Check the capability boundary instead of silently flattening effect semantics.
/// Child content, clips, IDs and descendants remain independent. The first
/// integration consumes only the immediate plate's fill and inside stroke.
pub(crate) fn check_plate(n: &El, nested: bool) -> Result<(), SceneError> {
    let s = &n.payload().style;
    if nested && n.payload().extras().welding.is_some() {
        return Err(SceneError::UnsupportedWeld(
            "nested material-weld members; mark the nested group .unwelded()",
        ));
    }
    if !s.shells.as_deref().unwrap_or_default().is_empty()
        || !s.shadow.as_deref().unwrap_or_default().is_empty()
        || s.mask.as_ref().is_some_and(|m| !m.is_none())
    {
        return Err(SceneError::UnsupportedWeld(
            "shell, shadow, or mask on a welded plate; keep the effect on an excluded wrapper/descendant",
        ));
    }
    if nested
        && s.layer
            .is_some_and(|(mix, alpha)| mix != crate::Mix::Normal || alpha != 1.0)
    {
        return Err(SceneError::UnsupportedWeld(
            "per-member compositing layers; use paint alpha or a layer on the welded group",
        ));
    }
    if n.children().iter().any(|c| c.payload().carve.is_some())
        && n.payload().extras().welding.is_some()
    {
        return Err(SceneError::UnsupportedWeld(
            "carving the material-weld container; carve an individual source outline instead",
        ));
    }
    Ok(())
}

pub(crate) struct Plate {
    pub outline: Arc<Path>,
    pub rect: Option<RoundedRect>,
    pub frame: Frame,
    pub fill: Option<Paint>,
    pub border: Option<Paint>,
    pub width: f64,
}

pub(crate) fn source(plate: Plate, origin: Point, tolerance: f64) -> Result<Source, SceneError> {
    let Plate {
        outline: path,
        rect: rr,
        frame,
        fill,
        border,
        width,
    } = plate;
    let delta = -origin.to_vec2();
    let shape = if let Some(r) = rr {
        let b = r.bounds();
        Geometry::RoundedRect {
            bounds: Rect::new(
                b.x0 - origin.x,
                b.y0 - origin.y,
                b.x1 - origin.x,
                b.y1 - origin.y,
            ),
            radius: r.radius(),
        }
    } else {
        let local = path.rigid_transform(delta, 0.0)?;
        let rings = local.flatten(tolerance, 250_000)?;
        Geometry::Contours(
            rings
                .into_iter()
                .filter(|r| r.len() >= 3)
                .map(|mut ring| {
                    if ring.len() > 1 && ring.first() == ring.last() {
                        ring.pop();
                    }
                    ring.into_iter().map(|p| Point::new(p.x, p.y)).collect()
                })
                .collect(),
        )
    };
    // Paint remains anchored to the original source's own geometry. Moving a
    // whole group therefore changes neither its material nor its cache key.
    let b = shape.bounds().unwrap_or(Rect::new(
        frame.x - origin.x,
        frame.y - origin.y,
        frame.right() - origin.x,
        frame.bottom() - origin.y,
    ));
    Ok(Source {
        shape,
        fill: fill.map(|p| paint(p, b)),
        border: border.map(|p| paint(p, b)),
        width,
    })
}

pub(crate) fn finish(
    baked: &mui_weld::Baked,
    origin: Point,
    members: HashSet<Arc<str>>,
) -> Result<MaterialWeld, SceneError> {
    let mut outline = Path::default();
    for ring in &baked.contours {
        let p = Path::polyline(
            ring.iter()
                .map(|p| Point::new(p.x + origin.x, p.y + origin.y)),
            true,
        );
        outline.commands.extend(p.commands);
    }
    let b = baked.bounds;
    let image_rect = RoundedRect::new(
        Rect::new(
            b.x0 + origin.x,
            b.y0 + origin.y,
            b.x1 + origin.x,
            b.y1 + origin.y,
        ),
        0.0,
    )?;
    let image = Image::rgba(baked.width, baked.height, baked.rgba.clone()).ok_or(
        SceneError::UnsupportedWeld("invalid baked image dimensions"),
    )?;
    Ok(MaterialWeld {
        outline: Arc::new(outline),
        image_fill: Fill::Image(Arc::new(image), Fit::Fill),
        image_rect,
        members,
        external: None,
    })
}
