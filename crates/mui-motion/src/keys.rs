//! Keyframes: motion that has to *land on a time* rather than chase a
//! target. A spring is right for a UI answering a hand; a trailer beat, an
//! onboarding reveal or a staggered entrance needs "at 0.5 s be at 1".
//!
//! [`Keys`] is a list of `(time, value, ease)`; each ease shapes the segment
//! that *arrives* at its key. Pure maths over `f64`: the caller owns the
//! clock, so the same keys play live and render offline frame for frame.
use crate::Spring;

/// How a segment gets from the previous key to this one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Ease {
    Linear,
    /// Stay on the previous value, then cut on the key.
    Hold,
    /// CSS `cubic-bezier(x1, y1, x2, y2)`: what a designer's tool exports.
    Cubic(f64, f64, f64, f64),
    /// Released at the previous key toward this one and left to its own
    /// physics: it can overshoot and may still be settling at the key time.
    /// The key's time only says when the *next* segment starts.
    Spring(Spring),
}
impl Ease {
    pub const IN: Self = Self::Cubic(0.42, 0., 1., 1.);
    pub const OUT: Self = Self::Cubic(0., 0., 0.58, 1.);
    pub const IN_OUT: Self = Self::Cubic(0.42, 0., 0.58, 1.);
    /// The fast-out, slow-settle curve most UI motion wants.
    pub const EMPHASIZED: Self = Self::Cubic(0.2, 0., 0., 1.);

    /// The segment's progress `0..=1` (a spring may leave that range) after
    /// `elapsed` of `span` seconds.
    fn progress(self, elapsed: f64, span: f64) -> f64 {
        let u = if span > 0. {
            (elapsed / span).clamp(0., 1.)
        } else {
            1.
        };
        match self {
            Self::Linear => u,
            Self::Hold => f64::from(u >= 1.),
            Self::Cubic(x1, y1, x2, y2) => cubic(x1, y1, x2, y2, u),
            Self::Spring(s) => {
                // The oscillator's closed form: one step of `elapsed` is exact.
                let mut s = s.seeded(0.);
                s.to(1.);
                s.step(elapsed.max(0.));
                s.value
            }
        }
    }
}

/// `y` of the CSS cubic Bezier at `x`, solving `x(t) = x` by Newton with a
/// bisection fallback.
fn cubic(x1: f64, y1: f64, x2: f64, y2: f64, x: f64) -> f64 {
    let bez = |a: f64, b: f64, t: f64| {
        let m = 1. - t;
        3. * m * m * t * a + 3. * m * t * t * b + t * t * t
    };
    let d = |a: f64, b: f64, t: f64| {
        let m = 1. - t;
        3. * m * m * a + 6. * m * t * (b - a) + 3. * t * t * (1. - b)
    };
    let mut t = x;
    for _ in 0..8 {
        let (e, slope) = (bez(x1, x2, t) - x, d(x1, x2, t));
        if e.abs() < 1e-7 {
            return bez(y1, y2, t);
        }
        if slope.abs() < 1e-6 {
            break;
        }
        t = (t - e / slope).clamp(0., 1.);
    }
    let (mut lo, mut hi) = (0., 1.);
    for _ in 0..40 {
        t = 0.5 * (lo + hi);
        if bez(x1, x2, t) < x {
            lo = t;
        } else {
            hi = t;
        }
    }
    bez(y1, y2, t)
}

/// A value over time. Built in time order; a key earlier than the last is
/// moved up to it, so a track can never run backwards.
///
/// ```
/// use mui_motion::{Ease, Keys};
/// let fade = Keys::new(0.).to(0.2, 1., Ease::OUT).hold(1.0).to(1.3, 0., Ease::IN);
/// assert_eq!(fade.at(-1.), 0.);
/// assert_eq!(fade.at(0.2), 1.);
/// assert_eq!(fade.at(0.9), 1.);
/// assert_eq!(fade.at(5.), 0.);
/// assert_eq!(fade.end(), 1.3);
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct Keys {
    start: f64,
    /// When the first segment leaves `start`: zero until [`Keys::delay`].
    from: f64,
    keys: Vec<(f64, f64, Ease)>,
}
impl Keys {
    /// Resting at `value` from time zero.
    pub fn new(value: f64) -> Self {
        Self {
            start: value,
            from: 0.,
            keys: Vec::new(),
        }
    }
    /// Arrive at `value` at `time`, shaped by `ease`.
    pub fn to(mut self, time: f64, value: f64, ease: Ease) -> Self {
        let time = if time.is_finite() {
            time.max(self.end())
        } else {
            self.end()
        };
        self.keys.push((time, value, ease));
        self
    }
    /// Stay where the last key left it until `time`.
    pub fn hold(self, time: f64) -> Self {
        let v = self.keys.last().map_or(self.start, |k| k.1);
        self.to(time, v, Ease::Hold)
    }
    /// Every key moved `by` seconds: the same entrance, staggered per item
    /// (`keys.delay(i as f64 * 0.05)`).
    pub fn delay(mut self, by: f64) -> Self {
        self.from += by;
        for k in &mut self.keys {
            k.0 += by;
        }
        self
    }
    /// When the last key lands.
    pub fn end(&self) -> f64 {
        self.keys.last().map_or(self.from, |k| k.0)
    }
    /// The value at `time` seconds.
    pub fn at(&self, time: f64) -> f64 {
        let (mut begin, mut v0) = (self.from, self.start);
        for (i, &(t, v, ease)) in self.keys.iter().enumerate() {
            let spring = matches!(ease, Ease::Spring(_));
            let value = |x: f64| v0 + (v - v0) * ease.progress(x - begin, t - begin);
            // The last spring key keeps settling for ever after its time.
            if time < t || (spring && i + 1 == self.keys.len()) {
                return if time < begin { v0 } else { value(time) };
            }
            // A spring may not be there yet: the next segment leaves from
            // where it *is* at this key, so the motion never jumps.
            v0 = if spring { value(t) } else { v };
            begin = t;
        }
        v0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn css_curves_hit_their_ends_and_keep_their_shape() {
        for e in [
            Ease::IN,
            Ease::OUT,
            Ease::IN_OUT,
            Ease::EMPHASIZED,
            Ease::Linear,
        ] {
            assert!(e.progress(0., 1.).abs() < 1e-6, "{e:?}");
            assert!((e.progress(1., 1.) - 1.).abs() < 1e-6, "{e:?}");
        }
        assert!(Ease::IN.progress(0.5, 1.) < 0.5);
        assert!(Ease::OUT.progress(0.5, 1.) > 0.5);
        // `ease` in CSS: cubic-bezier(.25,.1,.25,1) at 50% is ~0.8024.
        assert!((cubic(0.25, 0.1, 0.25, 1., 0.5) - 0.8024).abs() < 1e-3);
    }

    #[test]
    fn keys_interpolate_hold_and_stagger() {
        let k = Keys::new(10.)
            .to(1., 20., Ease::Linear)
            .hold(2.)
            .to(3., 0., Ease::Hold);
        assert_eq!(k.at(0.5), 15.);
        assert_eq!(k.at(1.5), 20.);
        assert_eq!(k.at(2.99), 20.);
        assert_eq!(k.at(3.), 0.);
        let late = k.clone().delay(0.5);
        assert_eq!(late.at(1.), 15.);
        assert_eq!(late.end(), 3.5);
        // Out of order: the key waits for the one before it.
        let k = Keys::new(0.)
            .to(2., 1., Ease::Linear)
            .to(1., 5., Ease::Hold);
        assert_eq!(k.end(), 2.);
    }

    #[test]
    fn a_spring_key_overshoots_and_the_next_key_starts_from_where_it_is() {
        let bouncy = Spring::new(0.4, 0.3);
        let k = Keys::new(0.).to(0.4, 1., Ease::Spring(bouncy));
        let peak = (0..200)
            .map(|i| k.at(i as f64 * 0.01))
            .fold(f64::MIN, f64::max);
        assert!(peak > 1.05, "an underdamped key overshoots: {peak}");
        // It keeps settling after its key time, and lands.
        assert!((k.at(10.) - 1.).abs() < 1e-3);
        // A key after it leaves from where the spring is, not from 1.
        let k = k.to(0.8, 0., Ease::Linear);
        assert!(
            (k.at(0.4) - Keys::new(0.).to(0.4, 1., Ease::Spring(bouncy)).at(0.4)).abs() < 1e-12
        );
        assert_eq!(k.at(0.8), 0.);
    }
}
