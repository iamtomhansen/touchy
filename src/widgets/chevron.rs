//! Chevrons: two sides of a square, turned so their corner points one way
//! while something is closed and another while it's open, such as a list
//! (down, then up) or a section (right, then down).
//!
//! A chevron turns itself over a short time when told which way to point, or
//! follows its owner exactly when its owner has a motion of its own to match.

use bevy::prelude::*;
use bevy::ui::UiSystems;

use crate::motion::{ResponseCurve, Tween};
use crate::theme::{ThemeBorder, ThemeToken};

/// The chevron's box, the length of its arms, and their stroke, in pixels.
const BOX: f32 = 12.0;
const ARM: f32 = 6.0;
const STROKE: f32 = 1.5;
/// How long a turn of its own takes, and how it eases.
const TURN_SECONDS: f32 = 0.15;
const TURN_CURVE: ResponseCurve = ResponseCurve::Power(0.5);

/// Which ways a chevron points, closed and open, as turns of its arms in degrees.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ChevronTurn {
    closed: f32,
    open: f32,
}

impl ChevronTurn {
    /// A list's: down while closed, up while open.
    pub(crate) const LIST: Self = Self {
        closed: 135.0,
        open: -45.0,
    };
    /// A section's: right while closed, down while open.
    pub(crate) const FOLD: Self = Self {
        closed: 45.0,
        open: 135.0,
    };

    /// The rotation of a chevron turned this far: 0 closed, 1 open.
    fn rotation(self, turned: f32) -> Rot2 {
        Rot2::degrees(self.closed + (self.open - self.closed) * turned)
    }
}

/// A chevron, and how far it's turned: 0 pointing the closed way, 1 the open way.
#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct Chevron {
    turn: ChevronTurn,
    turned: f32,
    turning: Option<Tween<f32>>,
}

impl Chevron {
    /// Starts it turning, over a short time, to point the open way if `open`.
    pub(crate) fn point(&mut self, open: bool) {
        let to = if open { 1.0 } else { 0.0 };
        if self.turned != to || self.turning.is_some() {
            self.turning = Some(Tween::new(self.turned, to, TURN_SECONDS, TURN_CURVE));
        }
    }

    /// Turns it `turned` of the way open at once, for an owner moving it along.
    pub(crate) fn follow(&mut self, turned: f32) {
        self.turning = None;
        self.turned = turned;
    }
}

/// Spawns a chevron under `parent`, `margin` around its box, pointing the
/// `open` or the closed way of `turn`. Returns the chevron itself, for its
/// [`chevron_colour`] and [`Chevron`].
pub(crate) fn spawn_chevron(
    parent: &mut ChildSpawnerCommands,
    margin: UiRect,
    turn: ChevronTurn,
    open: bool,
) -> Entity {
    let turned = if open { 1.0 } else { 0.0 };
    let mut chevron = Entity::PLACEHOLDER;
    let frame = Node {
        width: px(BOX),
        height: px(BOX),
        margin,
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        flex_shrink: 0.0,
        ..default()
    };
    parent
        .spawn((frame, Pickable::IGNORE))
        .with_children(|frame| {
            let arms = Node {
                width: px(ARM),
                height: px(ARM),
                border: UiRect {
                    top: px(STROKE),
                    right: px(STROKE),
                    ..default()
                },
                ..default()
            };
            let state = Chevron {
                turn,
                turned,
                turning: None,
            };
            let pointing = UiTransform::from_rotation(turn.rotation(turned));
            chevron = frame.spawn((state, arms, pointing, Pickable::IGNORE)).id();
        });
    chevron
}

/// The chevron's colour, `token`, on its arms.
pub(crate) fn chevron_colour(token: &ThemeToken) -> ThemeBorder {
    ThemeBorder::all(token.clone())
}

/// Turns each chevron turning of its own along. Real time, so a paused game's
/// menus move.
fn turn_chevrons(time: Res<Time<Real>>, mut chevrons: Query<&mut Chevron>) {
    // Only turning ones are touched, so resting ones aren't marked changed.
    for mut chevron in chevrons
        .iter_mut()
        .filter(|chevron| chevron.turning.is_some())
    {
        let Some(mut tween) = chevron.turning else {
            continue;
        };
        let (turned, arrived) = tween.advance(time.delta_secs());
        chevron.turned = turned;
        chevron.turning = (!arrived).then_some(tween);
    }
}

/// Draws each chevron that turned.
fn draw_chevrons(mut chevrons: Query<(&Chevron, &mut UiTransform), Changed<Chevron>>) {
    for (chevron, mut transform) in &mut chevrons {
        let rotation = chevron.turn.rotation(chevron.turned);
        if transform.rotation != rotation {
            transform.rotation = rotation;
        }
    }
}

/// Where chevrons are drawn, after which a turn shows next frame: owners
/// moving theirs along go before it.
#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ChevronSystems;

/// Turns and draws chevrons.
pub(crate) struct ChevronPlugin;

impl Plugin for ChevronPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, turn_chevrons).add_systems(
            PostUpdate,
            draw_chevrons
                .in_set(ChevronSystems)
                .before(UiSystems::Prepare),
        );
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy::time::TimeUpdateStrategy;

    use super::*;

    fn chevron_app(turn: ChevronTurn, open: bool) -> (App, Entity) {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, ChevronPlugin))
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
                20,
            )));
        let mut chevron = None;
        app.world_mut()
            .commands()
            .spawn(Node::default())
            .with_children(|parent| {
                chevron = Some(spawn_chevron(parent, UiRect::ZERO, turn, open));
            });
        app.update();
        (app, chevron.unwrap())
    }

    fn rotation(app: &App, chevron: Entity) -> Rot2 {
        app.world().get::<UiTransform>(chevron).unwrap().rotation
    }

    #[test]
    fn a_chevron_turns_of_its_own_and_lands_exactly() {
        let (mut app, chevron) = chevron_app(ChevronTurn::LIST, false);
        assert_eq!(rotation(&app, chevron), ChevronTurn::LIST.rotation(0.0));

        app.world_mut()
            .get_mut::<Chevron>(chevron)
            .unwrap()
            .point(true);
        app.update();
        app.update();
        assert_ne!(rotation(&app, chevron), ChevronTurn::LIST.rotation(0.0));
        for _ in 0..20 {
            app.update();
        }
        assert_eq!(rotation(&app, chevron), ChevronTurn::LIST.rotation(1.0));
    }

    #[test]
    fn a_followed_chevron_turns_exactly_as_far_as_told() {
        let (mut app, chevron) = chevron_app(ChevronTurn::FOLD, true);
        assert_eq!(rotation(&app, chevron), ChevronTurn::FOLD.rotation(1.0));
        app.world_mut()
            .get_mut::<Chevron>(chevron)
            .unwrap()
            .follow(0.25);
        app.update();
        assert_eq!(rotation(&app, chevron), ChevronTurn::FOLD.rotation(0.25));
    }
}
