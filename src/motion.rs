//! Shared motion maths for UI elements that move with feel: response curves, a
//! latch with hysteresis, and an accelerating glide.
//!
//! Everything here is plain data and functions with no ECS, so each widget
//! keeps control of what it moves and where it writes the result.

use bevy::prelude::*;

/// Shapes a signed input in `-1..=1` into an output in the same range.
///
/// Every curve keeps `0`, `1` and `-1` fixed, treats both signs alike, and
/// never decreases, so a control driven by one never jumps and never moves
/// against the user. Curves differ in where they spend their precision.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ResponseCurve {
    /// Output equals input.
    Linear,
    /// `sign(u)·|u|^p`. Above 1, gentle near zero for fine control and
    /// steep near the ends; below 1, the reverse. Must be positive.
    Power(f32),
    /// A mix of linear and [`ResponseCurve::Power`]: `(1 − amount)·u + amount·sign(u)·|u|^power`.
    /// Keeps a slope of `1 − amount` at zero, so the start is slowed but never dead.
    Blend {
        /// Exponent for the power curve, must be positive.
        power: f32,
        /// How much of the power curve to mix in, from `0` (linear) to `1` (full power).
        amount: f32,
    },
}

impl ResponseCurve {
    /// Applies the curve to `u`, clamping it to `-1..=1` first. The result is also in that range.
    pub fn apply(self, u: f32) -> f32 {
        let u = u.clamp(-1.0, 1.0);
        let power = |p: f32| u.signum() * u.abs().powf(p.max(f32::EPSILON));
        match self {
            Self::Linear => u,
            Self::Power(p) => power(p),
            Self::Blend { power: p, amount } => {
                let amount = amount.clamp(0.0, 1.0);
                (1.0 - amount) * u + amount * power(p)
            }
        }
    }
}

/// An on/off threshold with hysteresis: it turns on above `on_at` and only
/// turns off again below `off_at`. Keeps a value resting near the threshold
/// from flickering.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Latch {
    /// The value above which the latch turns on.
    pub on_at: f32,
    /// The value below which the latch turns off.
    pub off_at: f32,
}

impl Latch {
    /// Creates a new latch with the given on and off thresholds.
    pub fn new(on_at: f32, off_at: f32) -> Self {
        assert!(
            on_at >= off_at,
            "latch on_at must be at least off_at, got {on_at} and {off_at}"
        );
        Self {
            on_at,
            off_at: off_at.min(on_at),
        }
    }

    /// Whether the latch is on for `value`, given whether it was on before.
    pub fn update(self, was_on: bool, value: f32) -> bool {
        if was_on {
            value >= self.off_at
        } else {
            value > self.on_at
        }
    }
}

/// Something a [`Glide`] can move: a position on a line or in a plane.
pub trait GlideValue: Copy + Send + Sync + 'static {
    /// Linearly interpolates between `self` and `to` by `t` in `0..=1`.
    fn lerp_to(self, to: Self, t: f32) -> Self;
    /// Returns the distance from `self` to `to` in the value's units.
    fn distance_to(self, to: Self) -> f32;
}

impl GlideValue for f32 {
    fn lerp_to(self, to: Self, t: f32) -> Self {
        self + (to - self) * t
    }

    fn distance_to(self, to: Self) -> f32 {
        (to - self).abs()
    }
}

impl GlideValue for Vec2 {
    fn lerp_to(self, to: Self, t: f32) -> Self {
        self.lerp(to, t)
    }

    fn distance_to(self, to: Self) -> f32 {
        self.distance(to)
    }
}

/// Four numbers moving together, such as a rectangle's position and size.
impl GlideValue for Vec4 {
    fn lerp_to(self, to: Self, t: f32) -> Self {
        self.lerp(to, t)
    }

    fn distance_to(self, to: Self) -> f32 {
        self.distance(to)
    }
}

/// A move from `from` to `to` that starts at rest and accelerates steadily,
/// arriving at full speed with no overshoot: x(t) = ½·a·t². Short moves are
/// quick and long ones take longer, the way a real mechanism behaves.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glide<T: GlideValue> {
    from: T,
    to: T,
    elapsed: f32,
    /// In the value's units per second squared.
    acceleration: f32,
}

impl<T: GlideValue> Glide<T> {
    /// Creates a new glide from `from` to `to` with the given acceleration.
    pub fn new(from: T, to: T, acceleration: f32) -> Self {
        Self {
            from,
            to,
            elapsed: 0.0,
            acceleration: acceleration.max(f32::EPSILON),
        }
    }
    /// Returns the target position of the glide.
    pub fn target(&self) -> T {
        self.to
    }
    /// Advances by `delta_secs` and returns the new position, and whether it has arrived.
    /// The final position is exactly `to`.
    pub fn advance(&mut self, delta_secs: f32) -> (T, bool) {
        self.elapsed += delta_secs;
        let distance = self.from.distance_to(self.to);
        let travelled = 0.5 * self.acceleration * self.elapsed * self.elapsed;
        if distance <= 0.0 || travelled >= distance {
            (self.to, true)
        } else {
            (self.from.lerp_to(self.to, travelled / distance), false)
        }
    }
}

/// A move from `from` to `to` over a fixed `duration`, eased by a
/// [`ResponseCurve`]. A curve's guarantees (0 and 1 fixed, never decreasing)
/// are exactly what an easing needs: `Power(2.0)` eases in, `Power(0.5)` eases
/// out, `Linear` moves at constant speed.
///
/// Use a [`Glide`] when the move should feel physical (longer moves take
/// longer), and a `Tween` when every move should take the same time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tween<T: GlideValue> {
    from: T,
    to: T,
    elapsed: f32,
    duration: f32,
    curve: ResponseCurve,
}

impl<T: GlideValue> Tween<T> {
    /// Creates a new tween from `from` to `to` over `duration_secs`, eased by `curve`.
    pub fn new(from: T, to: T, duration_secs: f32, curve: ResponseCurve) -> Self {
        Self {
            from,
            to,
            elapsed: 0.0,
            duration: duration_secs.max(f32::EPSILON),
            curve,
        }
    }
    /// Returns the target position of the tween.
    pub fn target(&self) -> T {
        self.to
    }
    /// Advances by `delta_secs` and returns the new position, and whether it has arrived.
    /// The final position is exactly `to`.
    pub fn advance(&mut self, delta_secs: f32) -> (T, bool) {
        self.elapsed += delta_secs;
        let progress = self.elapsed / self.duration;
        if progress >= 1.0 {
            (self.to, true)
        } else {
            (
                self.from.lerp_to(self.to, self.curve.apply(progress)),
                false,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CURVES: [ResponseCurve; 5] = [
        ResponseCurve::Linear,
        ResponseCurve::Power(2.0),
        ResponseCurve::Power(0.5),
        ResponseCurve::Blend {
            power: 3.0,
            amount: 0.6,
        },
        ResponseCurve::Blend {
            power: 3.0,
            amount: 1.0,
        },
    ];

    /// The guarantees every widget relies on, for every curve.
    #[test]
    fn every_curve_keeps_ends_fixed_is_symmetric_and_never_decreases() {
        for curve in CURVES {
            assert_eq!(curve.apply(0.0), 0.0, "{curve:?}");
            assert!((curve.apply(1.0) - 1.0).abs() < 1e-6, "{curve:?}");
            assert!((curve.apply(-1.0) + 1.0).abs() < 1e-6, "{curve:?}");
            let mut previous = curve.apply(-1.0);
            for step in 1..=400 {
                let u = -1.0 + step as f32 / 200.0;
                let out = curve.apply(u);
                assert!(
                    (out + curve.apply(-u)).abs() < 1e-5,
                    "{curve:?} not symmetric at {u}"
                );
                assert!(out >= previous - 1e-6, "{curve:?} decreased at {u}");
                previous = out;
            }
            assert_eq!(
                curve.apply(5.0),
                curve.apply(1.0),
                "{curve:?} out-of-range input is clamped"
            );
        }
        assert!(
            ResponseCurve::Power(2.0).apply(0.5) < 0.5,
            "power above 1 is gentle near zero"
        );
    }

    #[test]
    fn latch_ignores_tremor_near_its_threshold() {
        let latch = Latch::new(0.2, 0.15);
        let mut on = false;
        let readings = [0.19, 0.21, 0.18, 0.16, 0.19, 0.14, 0.18];
        let states: Vec<bool> = readings
            .iter()
            .map(|&value| {
                on = latch.update(on, value);
                on
            })
            .collect();
        assert_eq!(states, [false, true, true, true, true, false, false]);
    }

    #[test]
    fn glides_accelerate_and_land_exactly() {
        let mut glide = Glide::new(0.0_f32, 1.0, 8.0); // Arrives at t = 0.5 s.
        let (quarter, _) = glide.advance(0.125);
        let (half, _) = glide.advance(0.125);
        assert!(
            half - quarter > quarter,
            "covers more ground the longer it runs"
        );
        let (end, arrived) = glide.advance(1.0);
        assert!(arrived);
        assert_eq!(end, 1.0);

        let mut glide = Glide::new(Vec2::new(3.0, 4.0), Vec2::ZERO, 1000.0);
        assert_eq!(glide.advance(1.0), (Vec2::ZERO, true));
    }

    #[test]
    fn tweens_follow_their_curve_and_land_exactly() {
        let mut eased = Tween::new(0.0_f32, 10.0, 1.0, ResponseCurve::Power(2.0));
        let mut linear = Tween::new(0.0_f32, 10.0, 1.0, ResponseCurve::Linear);
        assert_eq!(linear.advance(0.5).0, 5.0);
        assert_eq!(
            eased.advance(0.5).0,
            2.5,
            "eases in: a quarter of the way at half time"
        );
        assert_eq!(eased.advance(0.6), (10.0, true));
    }
}
