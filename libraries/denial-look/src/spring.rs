//! Damped springs, as Denial's shell moves things (its
//! `docs/LIQUID_MOTION.md` and the SDK's `Motion` springs): a value heads
//! for a target and keeps its velocity when the target changes, so a new
//! event blends into the motion instead of restarting it.

use std::time::Instant;

/// Stiffness and damping of a spring of mass 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpringParams {
    pub stiffness: f32,
    pub damping: f32,
}

impl SpringParams {
    pub const fn new(stiffness: f32, damping: f32) -> Self {
        Self { stiffness, damping }
    }

    /// Stiffness k and damping ratio ζ, as LIQUID_MOTION.md's table gives
    /// them: ζ = 1 is critically damped, below it overshoots.
    pub fn ratio(stiffness: f32, ratio: f32) -> Self {
        Self::new(stiffness, 2.0 * ratio * stiffness.sqrt())
    }

    /// A spring that travels most of the way in `millis` and overshoots by
    /// about `bounce` (0 none, 0.3 a lot), as SwiftUI and the shell's
    /// Cupertino springs describe them.
    pub fn perceptual(millis: u64, bounce: f32) -> Self {
        let period = (millis as f32 / 1000.0).max(0.001);
        let omega = std::f32::consts::TAU / period;
        Self::new(omega * omega, 2.0 * omega * (1.0 - bounce.clamp(0.0, 0.99)))
    }
}

/// The shell's named springs, tuned for values from 0 to 1.
pub mod springs {
    use super::SpringParams;

    /// Layer-sized transitions: overview, shade.
    pub const GENTLE: SpringParams = SpringParams::new(360.0, 36.0);
    /// Smaller, quicker elements: cards, switches.
    pub const SNAPPY: SpringParams = SpringParams::new(520.0, 44.0);
    /// Critically damped: arrives without overshooting.
    pub const SETTLE: SpringParams = SpringParams::new(380.0, 39.0);
    /// A touch of overshoot, for a rubber-band return.
    pub const BOUNCY: SpringParams = SpringParams::new(460.0, 34.0);
}

/// Integration step: stiff springs stay stable at any refresh rate.
const STEP: f32 = 1.0 / 480.0;
/// A longer frame than this is taken as this long: a stall does not fling.
const LONGEST_FRAME: f32 = 1.0 / 30.0;

/// One value on a spring. Advance it once per frame with the frame's time;
/// it stops itself when it rests.
#[derive(Clone, Copy, Debug)]
pub struct Spring {
    params: SpringParams,
    value: f32,
    velocity: f32,
    target: f32,
    /// Denial's duration scale: 2 is half as fast; 0 settles at once.
    scale: f32,
    /// Closer than this and slower than `rest_speed` is at rest.
    rest_distance: f32,
    rest_speed: f32,
    /// The last frame it was advanced to; none while it rests.
    last: Option<Instant>,
}

impl Spring {
    /// At rest at `value`. `scale` is Denial's duration scale
    /// (`Look::spring` passes it).
    pub fn new(params: SpringParams, value: f32, scale: f32) -> Self {
        Self {
            params,
            value,
            velocity: 0.0,
            target: value,
            scale: scale.max(0.0),
            rest_distance: 0.001,
            rest_speed: 0.01,
            last: None,
        }
    }

    /// For values in logical pixels: rest within a tenth of a pixel and
    /// below 10 px/s, as the shell's pixel tolerance.
    pub fn pixels(mut self) -> Self {
        self.rest_distance = 0.1;
        self.rest_speed = 10.0;
        self
    }

    pub fn value(&self) -> f32 {
        self.value
    }

    pub fn target(&self) -> f32 {
        self.target
    }

    pub fn velocity(&self) -> f32 {
        self.velocity
    }

    /// Heads for `target` from where it is, keeping its velocity.
    pub fn set_target(&mut self, target: f32, now: Instant) {
        self.target = target;
        self.wake(now);
    }

    /// Adds `velocity` per second, as a flick or a step does.
    pub fn kick(&mut self, velocity: f32, now: Instant) {
        self.velocity += velocity;
        self.wake(now);
    }

    /// Puts it at `value` and its target, at rest, as a finger that holds
    /// the value does.
    pub fn jump(&mut self, value: f32) {
        self.value = value;
        self.target = value;
        self.velocity = 0.0;
        self.last = None;
    }

    pub fn at_rest(&self) -> bool {
        self.last.is_none()
    }

    /// Moves it on to `now` and says where it is.
    pub fn advance(&mut self, now: Instant) -> f32 {
        let Some(last) = self.last else {
            return self.value;
        };
        let elapsed = now.saturating_duration_since(last).as_secs_f32();
        self.last = Some(now);
        let mut left = elapsed.min(LONGEST_FRAME) / self.scale;
        while left > 0.0 {
            // Semi-implicit Euler.
            let step = left.min(STEP);
            let force = -self.params.stiffness * (self.value - self.target)
                - self.params.damping * self.velocity;
            self.velocity += force * step;
            self.value += self.velocity * step;
            left -= step;
        }
        if (self.value - self.target).abs() < self.rest_distance
            && self.velocity.abs() < self.rest_speed
        {
            self.jump(self.target);
        }
        self.value
    }

    fn wake(&mut self, now: Instant) {
        if self.scale == 0.0 {
            // Reduced motion: every spring settles at once.
            self.jump(self.target);
        } else if self.last.is_none() {
            self.last = Some(now);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn run(spring: &mut Spring, start: Instant, frames: u32) -> f32 {
        let mut peak = f32::MIN;
        for frame in 1..=frames {
            let value = spring.advance(start + Duration::from_millis(u64::from(frame) * 8));
            peak = peak.max(value);
        }
        peak
    }

    #[test]
    fn settles_on_its_target_and_rests() {
        let start = Instant::now();
        let mut spring = Spring::new(springs::SETTLE, 0.0, 1.0);
        spring.set_target(1.0, start);
        let peak = run(&mut spring, start, 250);
        assert!(spring.at_rest());
        assert_eq!(spring.value(), 1.0);
        // Critically damped: no overshoot worth seeing.
        assert!(peak <= 1.005, "peak {peak}");
    }

    #[test]
    fn a_bouncy_spring_overshoots() {
        let start = Instant::now();
        let mut spring = Spring::new(SpringParams::ratio(230.0, 0.6), 0.0, 1.0);
        spring.set_target(1.0, start);
        assert!(run(&mut spring, start, 250) > 1.05);
    }

    #[test]
    fn reduced_motion_settles_at_once() {
        let mut spring = Spring::new(springs::GENTLE, 0.0, 0.0);
        spring.set_target(1.0, Instant::now());
        assert!(spring.at_rest());
        assert_eq!(spring.value(), 1.0);
    }

    #[test]
    fn a_new_target_keeps_the_velocity() {
        let start = Instant::now();
        let mut spring = Spring::new(springs::SNAPPY, 0.0, 1.0);
        spring.set_target(1.0, start);
        run(&mut spring, start, 5);
        let moving = spring.velocity();
        assert!(moving > 0.0);
        spring.set_target(2.0, start);
        assert_eq!(spring.velocity(), moving);
    }

    #[test]
    fn perceptual_matches_its_period() {
        let params = SpringParams::perceptual(400, 0.0);
        let omega = std::f32::consts::TAU / 0.4;
        assert!((params.stiffness - omega * omega).abs() < 0.01);
        assert!((params.damping - 2.0 * omega).abs() < 0.01);
    }
}
