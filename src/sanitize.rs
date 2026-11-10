//! Repairs for config values a widget can't use.
//!
//! A config that asks for something impossible, such as negative padding, is
//! fixed rather than refused, so a mistake never crashes a game. Each repair
//! logs a warning naming the value and what it became, so the mistake is
//! never silent either.
//!
//! These cover single values: numbers, each axis of a vector, and optional
//! ones, which are repaired only when they're there. A rule relating two
//! values (a minimum no larger than its maximum) is the config's own, and
//! warns the same way.

use std::fmt::Display;

use bevy::prelude::*;

/// `value`, or `min` where it's less or not a number, warning that `what` was repaired.
pub(crate) fn at_least<T: Bounded>(value: T, min: T::Bound, what: &str) -> T {
    value.at_least(min, what)
}

/// `value`, or `max` where it's more or not a number, warning that `what` was repaired.
pub(crate) fn at_most<T: Bounded>(value: T, max: T::Bound, what: &str) -> T {
    value.at_most(max, what)
}

/// `value`, kept between `min` and `max`: [`at_least`] one and [`at_most`] the other.
pub(crate) fn within<T: Bounded>(value: T, min: T::Bound, max: T::Bound, what: &str) -> T {
    at_most(at_least(value, min, what), max, what)
}

/// A config value that can be held to a bound.
pub(crate) trait Bounded: Sized {
    /// What the value is held to: a number, or a vector for a vector.
    type Bound;

    /// This, or `min` where it's less or not a number, warning that `what` was repaired.
    fn at_least(self, min: Self::Bound, what: &str) -> Self;

    /// This, or `max` where it's more or not a number, warning that `what` was repaired.
    fn at_most(self, max: Self::Bound, what: &str) -> Self;
}

/// A number held to `min`. A NaN fails every comparison, so it's repaired too.
fn floor<T: PartialOrd + Display>(value: T, min: T, what: &str) -> T {
    if value >= min {
        return value;
    }
    warn!("{what} was {value}, below its least of {min}; using {min}");
    min
}

/// A number held to `max`. A NaN fails every comparison, so it's repaired too.
fn ceiling<T: PartialOrd + Display>(value: T, max: T, what: &str) -> T {
    if value <= max {
        return value;
    }
    warn!("{what} was {value}, above its most of {max}; using {max}");
    max
}

/// Numbers are held to a number of their own kind.
macro_rules! bounded_numbers {
    ($($number:ty),*) => {$(
        impl Bounded for $number {
            type Bound = Self;

            fn at_least(self, min: Self, what: &str) -> Self {
                floor(self, min, what)
            }

            fn at_most(self, max: Self, what: &str) -> Self {
                ceiling(self, max, what)
            }
        }
    )*};
}

bounded_numbers!(f32, f64, i32, u32, usize);

/// A vector is held axis by axis.
impl Bounded for Vec2 {
    type Bound = Vec2;

    fn at_least(self, min: Vec2, what: &str) -> Self {
        if self.cmpge(min).all() {
            return self;
        }
        // `max` takes `min` over a NaN, as a number's repair does.
        let repaired = self.max(min);
        warn!("{what} was {self}, below its least of {min}; using {repaired}");
        repaired
    }

    fn at_most(self, max: Vec2, what: &str) -> Self {
        if self.cmple(max).all() {
            return self;
        }
        let repaired = self.min(max);
        warn!("{what} was {self}, above its most of {max}; using {repaired}");
        repaired
    }
}

/// An optional value is repaired only when it's there.
impl<T: Bounded> Bounded for Option<T> {
    type Bound = T::Bound;

    fn at_least(self, min: T::Bound, what: &str) -> Self {
        self.map(|value| value.at_least(min, what))
    }

    fn at_most(self, max: T::Bound, what: &str) -> Self {
        self.map(|value| value.at_most(max, what))
    }
}
