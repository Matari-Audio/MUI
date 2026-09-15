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
    pub fn to(&mut self, target: f64) {
        self.target = target;
    }
    pub fn settled(&self) -> bool {
        (self.value - self.target).abs() < 1e-3 && self.velocity.abs() < 1e-3
    }
    /// Advance `dt` seconds. Returns whether it is still moving. Substepped at
    /// 240 Hz so a dropped frame cannot blow the integrator up.
    pub fn step(&mut self, dt: f64) -> bool {
        if self.stiffness <= 0.0 {
            self.value = self.target;
            self.velocity = 0.0;
            return false;
        }
        if !(dt.is_finite() && dt > 0.0) {
            return !self.settled();
        }
        let n = (dt * 240.0).ceil().clamp(1.0, 64.0);
        let h = dt / n;
        for _ in 0..n as usize {
            let a = -self.stiffness * (self.value - self.target) - self.damping * self.velocity;
            self.velocity += a * h;
            self.value += self.velocity * h;
        }
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
        let mut i = Spring::instant();
        i.to(3.0);
        assert!(!i.step(1.0 / 60.0));
        assert_eq!(i.value, 3.0);
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
}
