//! Normalized, single-valued cubic Bézier curves for editors and response shapers.
//! No renderer, plugin, allocation or locking is needed to evaluate an existing curve.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CurvePoint {
    pub phase: f32,
    pub value: f32,
}
impl CurvePoint {
    fn valid(self) -> bool {
        self.phase.is_finite()
            && self.value.is_finite()
            && (0. ..=1.).contains(&self.phase)
            && (0. ..=1.).contains(&self.value)
    }
    fn lerp(self, other: Self, t: f32) -> Self {
        Self {
            phase: self.phase * (1. - t) + other.phase * t,
            value: self.value * (1. - t) + other.value * t,
        }
    }
}

/// Handles are absolute normalized coordinates, ordered horizontally within their segment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CurveHandles {
    pub outgoing: CurvePoint,
    pub incoming: CurvePoint,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle {
    Outgoing,
    Incoming,
}

/// A single cubic, also the exact data a renderer consumes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CubicSegment {
    pub start: CurvePoint,
    pub handles: CurveHandles,
    pub end: CurvePoint,
}
impl CubicSegment {
    pub fn at(self, t: f32) -> CurvePoint {
        let t = if t.is_nan() { 0. } else { t.clamp(0., 1.) };
        let a = self.start.lerp(self.handles.outgoing, t);
        let b = self.handles.outgoing.lerp(self.handles.incoming, t);
        let c = self.handles.incoming.lerp(self.end, t);
        a.lerp(b, t).lerp(b.lerp(c, t), t)
    }
    fn time_at(self, phase: f32) -> f32 {
        let (mut low, mut high) = (0., 1.);
        // Bounded bisection handles vertical tangents without Newton's zero-derivative failure.
        for _ in 0..24 {
            let mid = (low + high) * 0.5;
            if self.at(mid).phase < phase {
                low = mid;
            } else {
                high = mid;
            }
        }
        (low + high) * 0.5
    }
    fn split(self, t: f32) -> (CurveHandles, CurvePoint, CurveHandles) {
        let a = self.start.lerp(self.handles.outgoing, t);
        let b = self.handles.outgoing.lerp(self.handles.incoming, t);
        let c = self.handles.incoming.lerp(self.end, t);
        let d = a.lerp(b, t);
        let e = b.lerp(c, t);
        (
            CurveHandles {
                outgoing: a,
                incoming: d,
            },
            d.lerp(e, t),
            CurveHandles {
                outgoing: e,
                incoming: c,
            },
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Curve {
    points: Vec<CurvePoint>,
    handles: Vec<CurveHandles>,
}
impl Curve {
    pub const MAX_POINTS: usize = 64;
    pub fn new(points: Vec<CurvePoint>) -> Result<Self, &'static str> {
        if !(2..=Self::MAX_POINTS).contains(&points.len()) {
            return Err("invalid curve point count");
        }
        let handles = points
            .windows(2)
            .map(|p| CurveHandles {
                outgoing: p[0].lerp(p[1], 1. / 3.),
                incoming: p[0].lerp(p[1], 2. / 3.),
            })
            .collect();
        Self::from_parts(points, handles)
    }
    /// Validated import boundary for an owner's persistence format. Failure changes no live state.
    pub fn from_parts(
        points: Vec<CurvePoint>,
        handles: Vec<CurveHandles>,
    ) -> Result<Self, &'static str> {
        if !(2..=Self::MAX_POINTS).contains(&points.len())
            || handles.len() + 1 != points.len()
            || points[0].phase != 0.
            || points.last().unwrap().phase != 1.
            || points.iter().any(|p| !p.valid())
            || points.windows(2).any(|p| p[0].phase >= p[1].phase)
            || handles.iter().enumerate().any(|(i, h)| {
                !h.outgoing.valid()
                    || !h.incoming.valid()
                    || h.outgoing.phase < points[i].phase
                    || h.incoming.phase > points[i + 1].phase
                    || h.outgoing.phase > h.incoming.phase
            })
        {
            return Err("invalid normalized Bézier curve");
        }
        Ok(Self { points, handles })
    }
    pub fn points(&self) -> &[CurvePoint] {
        &self.points
    }
    pub fn handles(&self) -> &[CurveHandles] {
        &self.handles
    }
    pub fn segment(&self, index: usize) -> Option<CubicSegment> {
        Some(CubicSegment {
            start: *self.points.get(index)?,
            handles: *self.handles.get(index)?,
            end: *self.points.get(index + 1)?,
        })
    }
    /// Clamp outside the domain. NaN chooses the start value; infinities choose their endpoint.
    pub fn evaluate(&self, phase: f32) -> f32 {
        if phase.is_nan() || phase <= 0. {
            return self.points[0].value;
        }
        if phase >= 1. {
            return self.points.last().unwrap().value;
        }
        let index = self.points.partition_point(|p| p.phase <= phase) - 1;
        let segment = self.segment(index).unwrap();
        segment.at(segment.time_at(phase)).value
    }
    /// Periodic lookup for LFO-style consumers. Endpoint continuity is the owner's choice.
    pub fn evaluate_wrapped(&self, phase: f32) -> f32 {
        self.evaluate(if phase.is_finite() {
            phase.rem_euclid(1.)
        } else {
            0.
        })
    }
    /// Samples a response once per lane, including endpoints; a single lane uses the centre.
    pub fn sample_into(&self, values: &mut [f32]) {
        let count = values.len();
        for (i, value) in values.iter_mut().enumerate() {
            *value = self.evaluate(if count <= 1 {
                0.5
            } else {
                i as f32 / (count - 1) as f32
            });
        }
    }
    fn constrain(&mut self, index: usize) {
        let h = &mut self.handles[index];
        h.outgoing.phase = h
            .outgoing
            .phase
            .clamp(self.points[index].phase, self.points[index + 1].phase);
        h.incoming.phase = h
            .incoming
            .phase
            .clamp(h.outgoing.phase, self.points[index + 1].phase);
        h.outgoing.value = h.outgoing.value.clamp(0., 1.);
        h.incoming.value = h.incoming.value.clamp(0., 1.);
    }
    pub fn move_point(&mut self, index: usize, phase: f32, value: f32) {
        if index >= self.points.len() || !phase.is_finite() || !value.is_finite() {
            return;
        }
        let phase = if index == 0 {
            0.
        } else if index == self.points.len() - 1 {
            1.
        } else {
            phase.clamp(
                self.points[index - 1].phase.next_up(),
                self.points[index + 1].phase.next_down(),
            )
        };
        let value = value.clamp(0., 1.);
        let old = self.points[index];
        self.points[index] = CurvePoint { phase, value };
        let translate = |h: &mut CurvePoint| {
            h.phase += phase - old.phase;
            h.value += value - old.value;
        };
        if index > 0 {
            translate(&mut self.handles[index - 1].incoming);
            self.constrain(index - 1);
        }
        if index < self.handles.len() {
            translate(&mut self.handles[index].outgoing);
            self.constrain(index);
        }
    }
    pub fn move_handle(&mut self, segment: usize, handle: Handle, point: CurvePoint) {
        if segment >= self.handles.len() || !point.phase.is_finite() || !point.value.is_finite() {
            return;
        }
        let h = &mut self.handles[segment];
        match handle {
            Handle::Outgoing => {
                h.outgoing = CurvePoint {
                    phase: point
                        .phase
                        .clamp(self.points[segment].phase, h.incoming.phase),
                    value: point.value.clamp(0., 1.),
                }
            }
            Handle::Incoming => {
                h.incoming = CurvePoint {
                    phase: point
                        .phase
                        .clamp(h.outgoing.phase, self.points[segment + 1].phase),
                    value: point.value.clamp(0., 1.),
                }
            }
        }
    }
    pub fn reset_segment(&mut self, index: usize) {
        if index >= self.handles.len() {
            return;
        }
        self.handles[index] = CurveHandles {
            outgoing: self.points[index].lerp(self.points[index + 1], 1. / 3.),
            incoming: self.points[index].lerp(self.points[index + 1], 2. / 3.),
        };
    }
    /// De Casteljau subdivision retains the curve's shape (up to f32 rounding).
    pub fn split(&mut self, phase: f32) -> Option<usize> {
        if !phase.is_finite() || phase <= 0. || phase >= 1. || self.points.len() == Self::MAX_POINTS
        {
            return None;
        }
        let index = self.points.partition_point(|p| p.phase < phase);
        if self.points[index].phase == phase {
            return None;
        }
        let segment = self.segment(index - 1)?;
        let (left, point, right) = segment.split(segment.time_at(phase));
        if point.phase <= segment.start.phase || point.phase >= segment.end.phase {
            return None;
        }
        self.points.insert(index, point);
        self.handles[index - 1] = left;
        self.handles.insert(index, right);
        Some(index)
    }
    pub fn insert(&mut self, point: CurvePoint) -> Option<usize> {
        if !point.valid() {
            return None;
        }
        let index = self.split(point.phase)?;
        self.move_point(index, point.phase, point.value);
        Some(index)
    }
    /// Removing a knot joins its neighbours with the two surviving outer handles.
    pub fn remove(&mut self, index: usize) -> bool {
        if index == 0 || index >= self.points.len() - 1 {
            return false;
        }
        let incoming = self.handles.remove(index).incoming;
        self.handles[index - 1].incoming = incoming;
        self.points.remove(index);
        self.constrain(index - 1);
        true
    }
    pub fn linear() -> Self {
        Self::new(vec![
            CurvePoint {
                phase: 0.,
                value: 0.,
            },
            CurvePoint {
                phase: 1.,
                value: 1.,
            },
        ])
        .unwrap()
    }
}
impl Default for Curve {
    fn default() -> Self {
        let mut curve = Self::new(vec![
            CurvePoint {
                phase: 0.,
                value: 0.5,
            },
            CurvePoint {
                phase: 0.25,
                value: 1.,
            },
            CurvePoint {
                phase: 0.75,
                value: 0.,
            },
            CurvePoint {
                phase: 1.,
                value: 0.5,
            },
        ])
        .unwrap();
        for (i, handles) in curve.handles.iter_mut().enumerate() {
            handles.outgoing.value = curve.points[i].value;
            handles.incoming.value = curve.points[i + 1].value;
        }
        curve
    }
}

/// Optional standalone curve history. Commit once per completed gesture, never per pointer move.
/// Whole-document history owners can use the editor's events instead.
#[derive(Default)]
pub struct CurveHistory {
    undo: std::collections::VecDeque<(Curve, Curve)>,
    redo: Vec<(Curve, Curve)>,
}
impl CurveHistory {
    pub const CAPACITY: usize = 32;
    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }
    pub fn commit(&mut self, before: &Curve, after: &Curve) {
        if before == after {
            return;
        }
        if self.undo.back().is_some_and(|(_, last)| last != before) {
            self.clear();
        }
        self.redo.clear();
        if self.undo.len() == Self::CAPACITY {
            self.undo.pop_front();
        }
        self.undo.push_back((before.clone(), after.clone()));
    }
    pub fn undo(&mut self, current: &Curve) -> Option<Curve> {
        if self.undo.back().is_some_and(|(_, after)| after != current) {
            self.clear();
        }
        let entry = self.undo.pop_back()?;
        let restored = entry.0.clone();
        self.redo.push(entry);
        Some(restored)
    }
    pub fn redo(&mut self, current: &Curve) -> Option<Curve> {
        if self
            .redo
            .last()
            .is_some_and(|(before, _)| before != current)
        {
            self.clear();
        }
        let entry = self.redo.pop()?;
        let restored = entry.1.clone();
        self.undo.push_back(entry);
        Some(restored)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn history_keeps_gestures_bounded_and_does_not_overwrite_external_state() {
        let initial = Curve::linear();
        let mut edited = initial.clone();
        edited.move_point(1, 1., 0.4);
        let mut history = CurveHistory::default();
        history.commit(&initial, &edited);
        history.commit(&edited, &edited);
        assert_eq!(history.undo(&edited), Some(initial.clone()));
        assert_eq!(history.redo(&initial), Some(edited.clone()));
        assert_eq!(history.undo(&edited), Some(initial.clone()));
        let mut branch = initial.clone();
        branch.move_point(1, 1., 0.7);
        history.commit(&initial, &branch);
        assert!(history.redo(&branch).is_none());
        assert!(history.undo(&Curve::default()).is_none());
        let mut current = initial;
        for i in 0..40 {
            let before = current.clone();
            current.move_point(1, 1., i as f32 / 100.);
            history.commit(&before, &current);
        }
        let mut count = 0;
        while let Some(previous) = history.undo(&current) {
            current = previous;
            count += 1;
        }
        assert_eq!(count, CurveHistory::CAPACITY);
        history.clear();
        assert!(history.redo(&current).is_none());
    }
    #[test]
    fn bezier_contract() {
        let mut curve = Curve::linear();
        // y(t)=t, x(t)=t³: evaluating x=1/8 must produce y=1/2, not y=1/8.
        curve.move_handle(
            0,
            Handle::Outgoing,
            CurvePoint {
                phase: 0.,
                value: 1. / 3.,
            },
        );
        curve.move_handle(
            0,
            Handle::Incoming,
            CurvePoint {
                phase: 0.,
                value: 2. / 3.,
            },
        );
        assert!((curve.evaluate(0.125) - 0.5).abs() < 0.00001);
        let before: Vec<_> = (0..=256).map(|i| curve.evaluate(i as f32 / 256.)).collect();
        let i = curve.split(0.4).unwrap();
        for (i, y) in before.iter().enumerate() {
            assert!((curve.evaluate(i as f32 / 256.) - y).abs() < 0.00001);
        }
        assert_eq!(
            Curve::from_parts(curve.points.clone(), curve.handles.clone()).unwrap(),
            curve
        );
        curve.move_point(i, 10., -10.);
        assert_eq!(curve.points[i].value, 0.);
        assert!(Curve::from_parts(curve.points.clone(), curve.handles.clone()).is_ok());
        assert!(!curve.remove(0));
        assert!(curve.remove(i));
        let prior = curve.clone();
        assert!(
            curve
                .insert(CurvePoint {
                    phase: f32::NAN,
                    value: 0.
                })
                .is_none()
        );
        curve.move_handle(
            0,
            Handle::Incoming,
            CurvePoint {
                phase: 0.5,
                value: f32::INFINITY,
            },
        );
        assert_eq!(prior, curve);
        assert!(Curve::new(vec![]).is_err());
        let mut bad = Curve::linear().handles;
        bad[0].incoming.phase = -1.;
        assert!(Curve::from_parts(Curve::linear().points, bad).is_err());
        let mut samples = [0.; 5];
        Curve::linear().sample_into(&mut samples);
        for (i, v) in samples.iter().enumerate() {
            assert!((*v - i as f32 / 4.).abs() < 0.00001);
        }
        let periodic = Curve::default();
        assert!((periodic.evaluate_wrapped(-0.25) - periodic.evaluate(0.75)).abs() < 0.00001);
        assert!(periodic.evaluate(f32::NAN).is_finite());
        assert_eq!(
            periodic.evaluate(f32::INFINITY),
            periodic.points.last().unwrap().value
        );
        let mut one = [0.];
        Curve::linear().sample_into(&mut one);
        assert!((one[0] - 0.5).abs() < 0.00001);
        let mut crowded = Curve::linear();
        for i in 1..63 {
            crowded.split(i as f32 / 64.).unwrap();
        }
        assert!(crowded.split(0.999).is_none());
    }
}
