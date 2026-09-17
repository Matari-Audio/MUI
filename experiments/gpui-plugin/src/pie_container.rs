//! Shared pie packing and layered geometry. Merge the outer shapes into any host;
//! paint the well above it. Parameter containers may omit the well entirely.
use mui::geometry::{self, CornerStyle, PlacedShape, Point, Polygon, fillet, union};

#[derive(Clone, Copy)]
enum Flow {
    Vertical { left: bool },
    Horizontal,
}
#[derive(Clone, Copy)]
pub struct PieContainer {
    slots: usize,
    capacity: usize,
    flow: Flow,
    background: bool,
}
impl PieContainer {
    pub fn port(slots: usize, height: f32, left: bool) -> Self {
        Self {
            slots,
            capacity: ((height - 8.) / 24.).floor().max(1.) as usize,
            flow: Flow::Vertical { left },
            background: true,
        }
    }
    pub fn parameters(slots: usize, columns: usize, background: bool) -> Self {
        Self {
            slots,
            capacity: columns.max(1),
            flow: Flow::Horizontal,
            background,
        }
    }
    pub fn capacity(self) -> usize {
        self.capacity
    }
    pub fn slot(self, index: usize) -> Point {
        assert!(index < self.slots, "pie slot is outside this container");
        let lane = index / self.capacity.max(1);
        let offset = index % self.capacity.max(1);
        match self.flow {
            Flow::Vertical { left } => Point::new(
                lane as f64 * 24. * if left { -1. } else { 1. },
                offset as f64 * 24.,
            ),
            Flow::Horizontal => {
                let n = (self.slots - lane * self.capacity).min(self.capacity);
                Point::new(
                    offset as f64 * 24. - (n as f64 - 1.) * 12.,
                    lane as f64 * 24.,
                )
            }
        }
    }
    fn shapes(self, origin: Point, outer: bool) -> Result<Vec<PlacedShape>, geometry::Error> {
        let mut shapes = Vec::new();
        for lane in 0..self.slots.div_ceil(self.capacity.max(1)) {
            let n = (self.slots - lane * self.capacity).min(self.capacity);
            let first = self.slot(lane * self.capacity);
            let (pad_x, pad_y) = if outer { (18., 18.) } else { (15., 15.) };
            let (w, h) = match self.flow {
                Flow::Vertical { .. } => (pad_x * 2., (n - 1) as f64 * 24. + pad_y * 2.),
                Flow::Horizontal => ((n - 1) as f64 * 24. + pad_x * 2., pad_y * 2.),
            };
            shapes.push(
                Polygon::rectangle(origin.x + first.x - pad_x, origin.y + first.y - pad_y, w, h)?
                    .into(),
            );
        }
        Ok(shapes)
    }
    pub fn outer_shapes(self, origin: Point) -> Result<Vec<PlacedShape>, geometry::Error> {
        self.shapes(origin, true)
    }
    pub fn well(self) -> Result<Option<geometry::Path>, geometry::Error> {
        if !self.background || self.slots == 0 {
            return Ok(None);
        }
        Ok(Some(rounded(&self.shapes(Point::new(0., 0.), false)?)?))
    }
}
pub fn rounded(shapes: &[PlacedShape]) -> Result<geometry::Path, geometry::Error> {
    Ok(fillet(
        &union(shapes, Default::default())?,
        CornerStyle {
            convex_radius: 12.,
            concave_radius: 10.,
            ..Default::default()
        },
    )?
    .path)
}
#[test]
fn container_contract() {
    // The well leaves 6px around normal 9px pies, 4px around hovered 11px pies.
    let well = PieContainer::port(1, 144., false).well().unwrap().unwrap();
    let points = well.flatten(0.05, 1000).unwrap();
    for (axis, expected) in [(0, 15.), (1, 15.)] {
        let extent = points
            .iter()
            .flatten()
            .map(|p| if axis == 0 { p.x.abs() } else { p.y.abs() })
            .fold(0_f64, f64::max);
        assert!((extent - expected).abs() < 0.01);
    }

    for slots in [1, 5, 6, 13] {
        let p = PieContainer::port(slots, 144., true);
        assert_eq!(p.capacity, 5);
        assert!(p.well().unwrap().is_some());
        let mut host = vec![Polygon::rectangle(12., -19., 200., 220.).unwrap().into()];
        host.extend(p.outer_shapes(Point::new(0., 0.)).unwrap());
        assert!(rounded(&host).is_ok());
        for i in 1..slots {
            assert_ne!(p.slot(i), p.slot(i - 1));
        }
    }
    assert_eq!(
        PieContainer::port(6, 144., true).slot(5),
        Point::new(-24., 0.)
    );
    assert_eq!(
        PieContainer::port(6, 144., false).slot(5),
        Point::new(24., 0.)
    );
    assert!(
        PieContainer::parameters(5, 2, false)
            .well()
            .unwrap()
            .is_none()
    );
    assert!(
        PieContainer::parameters(5, 2, true)
            .well()
            .unwrap()
            .is_some()
    );
}
