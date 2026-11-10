//! Grabbers: something to take hold of and drag, which reports how far it's
//! been dragged and leaves the rest to its owner. A panel moves by one and
//! resizes by another; a splitter or a timeline's playhead could be one too.
//!
//! The grab and its look are separate. Any UI entity becomes grabbable with a [`Grabbable`],
//! whatever it looks like; [`SpawnUi::grabber`](crate::spawn_ui::SpawnUi::grabber) spawns one with
//! a look of touchy's own, by [`GrabberStyle`]:
//!
//! - **Ghost**: invisible, filling its parent, so the whole of the parent can
//!   be taken hold of. Spawn it first: anything spawned after it covers it, so
//!   it's only grabbed where nothing else takes the click.
//! - **Grip**: six dots, a small tactile patch to grab.
//! - **Handle**: a short line across its axis, on a strip that spans the
//!   length of where it sits; the strip takes the grab, so it needn't be aimed
//!   at exactly.
//! - **Corner**: six dots in a triangle, tucked into the corner it sits in
//!   ([`GrabCorner`]), to drag that corner both ways.
//!
//! When the theme skins a grip's or a corner's mark token, the picture takes
//! the place of the dots altogether, drawn once at the token set's
//! `skin_size`, and the dots come back if the skin goes. Corners have tokens
//! of their own ([`GRABBER_CORNER_STYLE`]), following the grip's colours, so a
//! theme can skin the two differently.
//!
//! A drag is announced as [`Grabbed`]: when it starts, as it moves, and when
//! it ends or is cancelled, each with the offset from where it started,
//! constrained to the grabbable's [`GrabAxis`]. A cancel (Escape or a right
//! click, through [`CancelPlugin`]) lets go mid-drag, and so does disabling
//! it (Bevy's `InteractionDisabled`). A disabled grabber fades and leaves the
//! cursor alone, since there's nothing to refuse: it just isn't there to grab.

mod look;

use bevy::picking::events::{PointerDrag, PointerDragEnd, PointerDragStart};
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use bevy::ui::InteractionDisabled;

pub(crate) use self::look::spawn_grabber;
pub use self::look::{
    GRABBER_CORNER_STYLE, GRABBER_STYLE, GrabCorner, GrabberConfig, GrabberStyle, GrabberTokens,
    GrabberWidgetPlugin,
};
use crate::EnsurePlugin;
use crate::cancel::{CancelPhase, CancelPlugin, CancelRequest};

/// Which ways a grabbable's drags count. Change it and a handle turns to match.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GrabAxis {
    /// Any direction.
    #[default]
    Free,
    /// Left and right only.
    Horizontal,
    /// Up and down only.
    Vertical,
}

impl GrabAxis {
    fn constrain(self, offset: Vec2) -> Vec2 {
        match self {
            Self::Free => offset,
            Self::Horizontal => offset.with_y(0.0),
            Self::Vertical => offset.with_x(0.0),
        }
    }
}

/// Makes a UI entity something to take hold of and drag, whatever it looks
/// like: dragging it with the primary button announces [`Grabbed`] on it.
///
/// # Examples
/// ```
/// use bevy::prelude::*;
/// use touchy::widgets::grabber::{GrabAxis, GrabPhase, Grabbable, Grabbed};
///
/// /// A label dragged left and right, back where it was if the drag is cancelled.
/// fn spawn_sliding_label(parent: &mut ChildSpawnerCommands) {
///     parent
///         .spawn((Text::new("Drag me"), Grabbable::default(), GrabAxis::Horizontal))
///         .observe(|grabbed: On<Grabbed>, mut nodes: Query<&mut Node>| {
///             let Ok(mut node) = nodes.get_mut(grabbed.grabber) else {
///                 return;
///             };
///             node.left = match grabbed.phase {
///                 GrabPhase::Cancel => px(0.0),
///                 _ => px(grabbed.offset.x),
///             };
///         });
/// }
/// ```
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
#[require(GrabAxis)]
pub struct Grabbable {
    /// Where the pointer is, in UI pixels, while it's held.
    held_at: Option<Vec2>,
}

impl Grabbable {
    /// Whether it's held right now.
    pub fn is_held(&self) -> bool {
        self.held_at.is_some()
    }
}

/// Where a drag is in its life.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GrabPhase {
    /// The pointer took hold.
    Start,
    /// The pointer moved while holding on.
    Move,
    /// The pointer let go.
    End,
    /// The drag was called off: by a cancel input, or by disabling the
    /// grabbable. Its offset is zero, back where it started; put back
    /// whatever the drag changed.
    Cancel,
}

/// A grabbable being dragged. `offset` is how far from where the drag
/// started, in UI pixels with +y down, along the grabbable's axis;
/// `position` is where the pointer is, in UI pixels from the window's top
/// left corner.
#[derive(EntityEvent, Clone, Copy, Debug, PartialEq)]
pub struct Grabbed {
    /// The grabbable being dragged.
    #[event_target]
    pub grabber: Entity,
    /// Where the drag is in its life.
    pub phase: GrabPhase,
    /// How far from where the drag started, along its axis, in UI pixels.
    pub offset: Vec2,
    /// Where the pointer is, in UI pixels from the window's top left corner.
    pub position: Vec2,
}

/// The app's UI scale, which turns window pixels into UI pixels.
fn ui_scale(scale: &Option<Res<UiScale>>) -> f32 {
    scale
        .as_ref()
        .map_or(1.0, |scale| scale.0)
        .max(f32::EPSILON)
}

type Grab = (
    &'static GrabAxis,
    &'static mut Grabbable,
    Has<InteractionDisabled>,
);

fn start_grab(
    drag: On<PointerDragStart>,
    mut grabbables: Query<Grab>,
    scale: Option<Res<UiScale>>,
    mut commands: Commands,
) {
    if drag.button != PointerButton::Primary {
        return;
    }
    let Ok((_, mut grabbable, disabled)) = grabbables.get_mut(drag.entity) else {
        return;
    };
    if disabled {
        return;
    }
    let position = drag.pointer.position / ui_scale(&scale);
    grabbable.held_at = Some(position);
    commands.trigger(Grabbed {
        grabber: drag.entity,
        phase: GrabPhase::Start,
        offset: Vec2::ZERO,
        position,
    });
}

fn move_grab(
    drag: On<PointerDrag>,
    mut grabbables: Query<Grab>,
    scale: Option<Res<UiScale>>,
    mut commands: Commands,
) {
    if let Ok((axis, mut grabbable, _)) = grabbables.get_mut(drag.entity)
        && grabbable.is_held()
    {
        let position = drag.pointer.position / ui_scale(&scale);
        grabbable.held_at = Some(position);
        commands.trigger(Grabbed {
            grabber: drag.entity,
            phase: GrabPhase::Move,
            offset: axis.constrain(drag.distance / ui_scale(&scale)),
            position,
        });
    }
}

fn end_grab(
    drag: On<PointerDragEnd>,
    mut grabbables: Query<Grab>,
    scale: Option<Res<UiScale>>,
    mut commands: Commands,
) {
    if let Ok((axis, mut grabbable, _)) = grabbables.get_mut(drag.entity)
        && grabbable.held_at.take().is_some()
    {
        commands.trigger(Grabbed {
            grabber: drag.entity,
            phase: GrabPhase::End,
            offset: axis.constrain(drag.distance / ui_scale(&scale)),
            position: drag.pointer.position / ui_scale(&scale),
        });
    }
}

/// Lets go of `grabbable`, if it's held, announcing the cancel. The pointer
/// may still be down: the rest of its drag is ignored.
fn cancel(entity: Entity, grabbable: &mut Grabbable, commands: &mut Commands) -> bool {
    let Some(position) = grabbable.held_at.take() else {
        return false;
    };
    commands.trigger(Grabbed {
        grabber: entity,
        phase: GrabPhase::Cancel,
        offset: Vec2::ZERO,
        position,
    });
    true
}

/// Disabling a grabbable mid-drag calls the drag off, so it never ends up held.
fn cancel_when_disabled(
    added: On<Add<InteractionDisabled>>,
    mut grabbables: Query<&mut Grabbable>,
    mut commands: Commands,
) {
    if let Ok(mut grabbable) = grabbables.get_mut(added.entity) {
        cancel(added.entity, &mut grabbable, &mut commands);
    }
}

/// A cancel input calls off every drag in progress, ahead of anything else
/// it might cancel, as a drag holds the pointer.
fn cancel_on_request(
    mut request: ResMut<CancelRequest>,
    mut grabbables: Query<(Entity, &mut Grabbable)>,
    mut commands: Commands,
) {
    if request.peek().is_none() {
        return;
    }
    let mut cancelled = false;
    // Only held ones are touched, so the rest aren't marked changed.
    for (entity, mut grabbable) in grabbables.iter_mut().filter(|(_, g)| g.is_held()) {
        cancelled |= cancel(entity, &mut grabbable, &mut commands);
    }
    if cancelled {
        request.consume();
    }
}

/// Grabbing and dragging, for any [`Grabbable`], with or without a look.
pub struct GrabPlugin;

impl Plugin for GrabPlugin {
    fn build(&self, app: &mut App) {
        app.ensure_plugin(CancelPlugin)
            .add_observer(start_grab)
            .add_observer(move_grab)
            .add_observer(end_grab)
            .add_observer(cancel_when_disabled)
            .add_systems(PreUpdate, cancel_on_request.in_set(CancelPhase::Picker));
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use bevy::camera::NormalizedRenderTarget;
    use bevy::input::keyboard::{Key, KeyboardInput};
    use bevy::input::{ButtonState, InputPlugin};
    use bevy::picking::backend::HitData;
    use bevy::picking::events::Pointer;
    use bevy::picking::pointer::{Location, PointerId};

    use super::*;

    #[derive(Resource, Default)]
    pub(crate) struct Heard(pub Vec<(GrabPhase, Vec2)>);

    /// An app that hears every [`Grabbed`].
    pub(crate) fn hearing_app() -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, InputPlugin))
            .init_resource::<Heard>()
            .add_observer(|grabbed: On<Grabbed>, mut heard: ResMut<Heard>| {
                heard.0.push((grabbed.phase, grabbed.offset))
            });
        app
    }

    fn pointer() -> Pointer {
        Pointer::new(
            PointerId::Mouse,
            Location {
                target: NormalizedRenderTarget::None {
                    width: 1,
                    height: 1,
                },
                position: Vec2::ZERO,
            },
        )
    }

    /// Takes hold of `entity` with the primary button.
    pub(crate) fn take_hold(app: &mut App, entity: Entity) {
        app.world_mut().trigger(PointerDragStart {
            entity,
            pointer: pointer(),
            button: PointerButton::Primary,
            hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
        });
    }

    /// Drags `entity`, already held, by each of `steps` in turn.
    pub(crate) fn drag_by(app: &mut App, entity: Entity, steps: &[Vec2]) -> Vec2 {
        let mut distance = Vec2::ZERO;
        for &step in steps {
            distance += step;
            app.world_mut().trigger(PointerDrag {
                entity,
                pointer: pointer(),
                button: PointerButton::Primary,
                distance,
                delta: step,
            });
        }
        distance
    }

    /// Lets go of `entity` after dragging it `distance`.
    pub(crate) fn let_go(app: &mut App, entity: Entity, distance: Vec2) {
        app.world_mut().trigger(PointerDragEnd {
            entity,
            pointer: pointer(),
            button: PointerButton::Primary,
            distance,
        });
    }

    /// Drags `entity` by each of `steps` in turn, then lets go.
    pub(crate) fn drag(app: &mut App, entity: Entity, steps: &[Vec2]) {
        take_hold(app, entity);
        let distance = drag_by(app, entity, steps);
        let_go(app, entity, distance);
        app.update();
    }

    /// Presses Escape, and runs a frame.
    pub(crate) fn press_escape(app: &mut App) {
        app.world_mut().write_message(KeyboardInput {
            key_code: KeyCode::Escape,
            logical_key: Key::Escape,
            state: ButtonState::Pressed,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        });
        app.update();
    }

    pub(crate) fn heard(app: &mut App) -> Vec<(GrabPhase, Vec2)> {
        std::mem::take(&mut app.world_mut().resource_mut::<Heard>().0)
    }

    fn grabbable_app(axis: GrabAxis) -> (App, Entity) {
        let mut app = hearing_app();
        app.add_plugins(GrabPlugin);
        let grabbable = app
            .world_mut()
            .spawn((Node::default(), Grabbable::default(), axis))
            .id();
        app.update();
        (app, grabbable)
    }

    #[test]
    fn a_drag_is_announced_from_start_to_end_along_its_axis() {
        let (mut app, grabbable) = grabbable_app(GrabAxis::Horizontal);
        let steps = [Vec2::new(10.0, 5.0), Vec2::new(4.0, -2.0)];
        drag(&mut app, grabbable, &steps);
        assert_eq!(
            heard(&mut app),
            [
                (GrabPhase::Start, Vec2::ZERO),
                (GrabPhase::Move, Vec2::new(10.0, 0.0)),
                (GrabPhase::Move, Vec2::new(14.0, 0.0)),
                (GrabPhase::End, Vec2::new(14.0, 0.0)),
            ]
        );
    }

    #[test]
    fn offsets_are_in_ui_pixels() {
        let (mut app, grabbable) = grabbable_app(GrabAxis::Free);
        app.insert_resource(UiScale(2.0));
        drag(&mut app, grabbable, &[Vec2::new(20.0, 10.0)]);
        assert_eq!(
            heard(&mut app).last(),
            Some(&(GrabPhase::End, Vec2::new(10.0, 5.0)))
        );
    }

    #[test]
    fn escape_cancels_a_drag_and_the_rest_of_it_is_ignored() {
        let (mut app, grabbable) = grabbable_app(GrabAxis::Free);
        take_hold(&mut app, grabbable);
        let distance = drag_by(&mut app, grabbable, &[Vec2::new(8.0, 0.0)]);
        press_escape(&mut app);
        assert_eq!(
            app.world().resource::<CancelRequest>().peek(),
            None,
            "the cancel is claimed"
        );
        drag_by(&mut app, grabbable, &[Vec2::new(8.0, 0.0)]);
        let_go(&mut app, grabbable, distance);
        app.update();
        assert_eq!(
            heard(&mut app),
            [
                (GrabPhase::Start, Vec2::ZERO),
                (GrabPhase::Move, Vec2::new(8.0, 0.0)),
                (GrabPhase::Cancel, Vec2::ZERO),
            ]
        );
    }

    #[test]
    fn a_cancel_with_nothing_held_is_left_for_others() {
        let (mut app, _) = grabbable_app(GrabAxis::Free);
        press_escape(&mut app);
        assert!(app.world().resource::<CancelRequest>().peek().is_some());
    }

    #[test]
    fn disabling_mid_drag_cancels_it_and_a_disabled_grabbable_cant_be_taken() {
        let (mut app, grabbable) = grabbable_app(GrabAxis::Free);
        take_hold(&mut app, grabbable);
        app.world_mut()
            .entity_mut(grabbable)
            .insert(InteractionDisabled);
        app.update();
        assert_eq!(
            heard(&mut app),
            [
                (GrabPhase::Start, Vec2::ZERO),
                (GrabPhase::Cancel, Vec2::ZERO)
            ]
        );
        drag(&mut app, grabbable, &[Vec2::new(10.0, 0.0)]);
        assert!(heard(&mut app).is_empty());
    }
}
