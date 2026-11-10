//! Wedges: the slices of a circle that directions fall into, and drawing one
//! lit on a round node.
//!
//! A bearing is a direction on screen, clockwise from straight up, in radians
//! from 0 to a full turn, as Bevy's conic gradients measure it. A circle cut
//! into `count` wedges has the first centred straight up.
//!
//! A wedge is drawn as a conic gradient with hard edges on a node rounded into
//! a circle: lit across the wedge, clear everywhere else. Layered under a disc
//! in the middle, it's a slice of a ring.

use std::f32::consts::TAU;

use bevy::prelude::*;
use bevy::ui::{AngularColorStop, BackgroundGradient, ConicGradient, Gradient, UiPosition};

/// The way to `offset` on screen (y down), clockwise from straight up.
pub(crate) fn bearing(offset: Vec2) -> f32 {
    offset.x.atan2(-offset.y).rem_euclid(TAU)
}

/// The way to the middle of wedge `index` of `count`, the first straight up.
pub(crate) fn bearing_of(index: usize, count: usize) -> f32 {
    index as f32 * TAU / count.max(1) as f32
}

/// The wedge of `count` that holds `bearing`.
pub(crate) fn wedge_at(bearing: f32, count: usize) -> usize {
    let count = count.max(1);
    (bearing * count as f32 / TAU).round() as usize % count
}

/// One wedge of `count` around `middle`, in `colour`.
pub(crate) fn wedge_gradient(middle: f32, count: usize, colour: Color) -> Gradient {
    let width = TAU / count.max(1) as f32;
    let stops = vec![
        AngularColorStop::new(colour, 0.0),
        AngularColorStop::new(colour, width),
        AngularColorStop::new(Color::NONE, width),
        AngularColorStop::new(Color::NONE, TAU),
    ];
    // The gradient starts at the wedge's leading edge, so it never wraps.
    let start = (middle - width / 2.0).rem_euclid(TAU);
    ConicGradient::new(UiPosition::CENTER, stops)
        .with_start(start)
        .into()
}

/// Wedges lit on a round node: each a bearing and its colour, of `count`
/// around. Later ones are drawn over earlier ones.
pub(crate) fn wedges(
    lit: impl IntoIterator<Item = (f32, Color)>,
    count: usize,
) -> BackgroundGradient {
    BackgroundGradient(
        lit.into_iter()
            .map(|(middle, colour)| wedge_gradient(middle, count, colour))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use std::f32::consts::FRAC_PI_2;

    use super::*;

    #[test]
    fn a_direction_falls_in_the_wedge_of_the_nearest_middle() {
        let north = Vec2::new(0.0, -1.0);
        let east = Vec2::new(1.0, 0.0);
        assert_eq!(bearing(north), 0.0);
        assert!((bearing(east) - FRAC_PI_2).abs() < 1e-6, "clockwise");
        assert_eq!(wedge_at(bearing(north), 4), 0);
        assert_eq!(wedge_at(bearing(east), 4), 1);
        assert_eq!(wedge_at(bearing(Vec2::new(0.0, 1.0)), 4), 2);
        assert_eq!(wedge_at(bearing(Vec2::new(-1.0, 0.0)), 4), 3);
        // Just west of north is still north, not wrapped past the last.
        assert_eq!(wedge_at(bearing(Vec2::new(-0.1, -1.0)), 4), 0);
        assert_eq!(wedge_at(bearing(Vec2::new(1.0, -1.0)), 3), 0);
        assert_eq!(wedge_at(bearing(Vec2::new(1.0, 0.2)), 3), 1);
    }

    #[test]
    fn a_wedge_is_lit_across_its_width_from_its_leading_edge() {
        let Gradient::Conic(conic) = wedge_gradient(0.0, 4, Color::WHITE) else {
            panic!("a conic gradient");
        };
        let quarter = TAU / 4.0;
        assert!(
            (conic.start - (TAU - quarter / 2.0)).abs() < 1e-6,
            "from 315°"
        );
        let angles: Vec<f32> = conic.stops.iter().map(|stop| stop.angle.unwrap()).collect();
        assert_eq!(angles, [0.0, quarter, quarter, TAU], "hard edges");
    }
}
