//! One spring. Hover, press, a knob settling, a panel sliding: all the same
//! second-order chase, stepped per frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spring {
    pub value: f64,
    pub velocity: f64,
    pub target: f64,
    pub stiffness: f64,
    /// Critical damping is `2 * stiffness.sqrt()`; less overshoots.
    pub damping: f64,
}
impl Spring {
    /// Critically damped, ~0.31 s response: the right feel for UI state.
    pub const DEFAULT: Self = Self {
        value: 0.0,
        velocity: 0.0,
        target: 0.0,
        stiffness: 400.0,
        damping: 40.0,
    };

    /// [`Spring::DEFAULT`], resting at `value`.
    pub fn at(value: f64) -> Self {
        Self {
            value,
            target: value,
            ..Self::DEFAULT
        }
    }

    /// The parametrisation designers actually think in: `response_s` is the
    /// period of the undamped oscillation, near enough "how long until it is
    /// there", and `damping` is the ratio -- `1.0` critical, below it
    /// overshoots, above it crawls. `Spring::new(0.3, 1.0)` is the usual
    /// button; `Spring::new(0.5, 0.7)` is a panel with a little bounce.
    /// A non-positive or non-finite response gives [`Spring::instant`].
    pub fn new(response_s: f64, damping: f64) -> Self {
        if !(response_s.is_finite() && response_s > 0.0) || !damping.is_finite() {
            return Self::instant();
        }
        let stiffness = (std::f64::consts::TAU / response_s).powi(2);
        Self {
            stiffness,
            damping: 2.0 * damping.max(0.0) * stiffness.sqrt(),
            ..Self::at(0.0)
        }
    }

    /// No spring at all: every step lands on the target. The honest way to
    /// say "this property does not animate" without a second code path.
    pub fn instant() -> Self {
        Self {
            stiffness: 0.0,
            damping: 0.0,
            ..Self::at(0.0)
        }
    }
    /// This spring's shape, resting at `value`: the state a channel starts
    /// in the first frame it is animated, so nothing flies in from zero.
    pub fn seeded(self, value: f64) -> Self {
        Self {
            value,
            velocity: 0.0,
            target: value,
            ..self
        }
    }
    pub fn to(&mut self, target: f64) {
        self.target = target;
    }
    pub fn settled(&self) -> bool {
        (self.value - self.target).abs() < 1e-3 && self.velocity.abs() < 1e-3
    }
    /// Advance `dt` seconds. Returns whether it is still moving. Uses the
    /// exact solution of the damped oscillator, so any `dt` is stable and
    /// sixty small steps land where one big step does.
    pub fn step(&mut self, dt: f64) -> bool {
        if self.stiffness <= 0.0 {
            self.value = self.target;
            self.velocity = 0.0;
            return false;
        }
        if !(dt.is_finite() && dt > 0.0) {
            return !self.settled();
        }
        // x'' + c x' + k x = 0 with x = value - target. Writing a = c / 2 and
        // d = k - a^2, x(t) = e^(-at) (x0 C + (v0 + a x0) S) and
        // v(t) = e^(-at) (v0 C - (a v0 + k x0) S), where (C, S) is
        // (cos wt, sin(wt) / w) under-damped, (1, t) critical and
        // (cosh wt, sinh(wt) / w) over-damped, w = sqrt(|d|). `e_c` and `e_s`
        // carry the e^(-at) factor already.
        let (k, a) = (self.stiffness, 0.5 * self.damping);
        let (x0, v0) = (self.value - self.target, self.velocity);
        let d = k - a * a;
        let decay = (-a * dt).exp();
        let (e_c, e_s) = if d.abs() <= 1e-9 * k {
            (decay, decay * dt)
        } else if d > 0.0 {
            let w = d.sqrt();
            let (sin, cos) = (w * dt).sin_cos();
            (decay * cos, decay * sin / w)
        } else {
            // Both exponents are <= 0; e^(-at) * cosh(wt) would overflow first.
            let w = (-d).sqrt();
            let (slow, fast) = (((w - a) * dt).exp(), (-(w + a) * dt).exp());
            (0.5 * (slow + fast), 0.5 * (slow - fast) / w)
        };
        self.value = self.target + x0 * e_c + (v0 + a * x0) * e_s;
        self.velocity = v0 * e_c - (a * v0 + k * x0) * e_s;
        if self.settled() {
            self.value = self.target;
            self.velocity = 0.0;
            return false;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::Spring;
    #[test]
    fn response_and_damping_agree_with_the_stiffness_form() {
        let s = Spring::new(std::f64::consts::TAU / 20.0, 1.0);
        assert!((s.stiffness - 400.0).abs() < 1e-6);
        assert!((s.damping - 40.0).abs() < 1e-6);
        let seeded = Spring::new(0.3, 1.0).seeded(7.0);
        assert_eq!(
            (seeded.value, seeded.target, seeded.velocity),
            (7.0, 7.0, 0.0)
        );
        assert_eq!(seeded.stiffness, Spring::new(0.3, 1.0).stiffness);
        let mut i = Spring::instant();
        i.to(3.0);
        assert!(!i.step(1.0 / 60.0));
        assert_eq!(i.value, 3.0);
    }

    #[test]
    fn a_stalled_frame_does_not_blow_the_integrator_up() {
        let mut s = Spring::at(20.0);
        s.to(0.0);
        for _ in 0..20 {
            s.step(5.0);
            assert!(s.value.is_finite() && s.velocity.is_finite(), "{s:?}");
        }
        assert!(s.settled(), "{s:?}");
    }

    #[test]
    fn a_spring_reaches_its_target_without_overshoot() {
        let mut s = Spring::at(0.0);
        s.to(1.0);
        let mut peak: f64 = 0.0;
        let mut frames = 0;
        while s.step(1.0 / 60.0) {
            peak = peak.max(s.value);
            frames += 1;
            assert!(frames < 120, "never settled");
        }
        assert_eq!(s.value, 1.0);
        assert!(peak <= 1.0 + 1e-6, "overshot to {peak}");
    }

    #[test]
    fn stiff_springs_settle_at_60fps() {
        // After 1 s these had diverged to -4.9e60, -1.2e51 and -1.6e14 under
        // 240 Hz Euler. Two seconds: damping 5 crawls, as it should.
        for mut s in [
            Spring::new(0.025, 1.0),
            Spring::new(0.1, 5.0),
            Spring::new(0.03, 1.0),
        ] {
            s.to(1.0);
            for _ in 0..120 {
                s.step(1.0 / 60.0);
                assert!(s.value.is_finite() && s.velocity.is_finite(), "{s:?}");
            }
            assert!(s.settled(), "{s:?}");
        }
    }

    #[test]
    fn the_trajectory_is_frame_rate_independent() {
        let run = |hz: usize| {
            let mut s = Spring::new(0.5, 0.2);
            s.to(1.0);
            for _ in 0..hz {
                s.step(1.0 / hz as f64);
            }
            s
        };
        let (a, b, c) = (run(60), run(240), run(1));
        assert!(!c.settled(), "pick a spring still moving at 1 s: {c:?}");
        for s in [a, b] {
            assert!((s.value - c.value).abs() < 1e-9, "{s:?} vs {c:?}");
            assert!((s.velocity - c.velocity).abs() < 1e-7, "{s:?} vs {c:?}");
        }
    }
}
