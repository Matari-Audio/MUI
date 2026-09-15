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
    /// Critically damped, settles in ~150 ms: the right feel for UI state.
    pub fn at(value: f64) -> Self {
        Self {
            value,
            velocity: 0.0,
            target: value,
            stiffness: 400.0,
            damping: 40.0,
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
