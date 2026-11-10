//! Opening a radial, aiming it and choosing: by a held or left-open pointer,
//! the keys, or the game.

use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input_focus::FocusedInput;
use bevy::picking::events::{PointerClick, PointerPress, PointerRelease};
use bevy::picking::pointer::{
    PointerAction, PointerButton, PointerId, PointerInput, PointerLocation,
};
use bevy::prelude::*;
use bevy::ui::{InteractionDisabled, UiGlobalTransform};
use bevy::ui_widgets::{Activate, Button};

use super::{Phase, Radial, RadialCenter};
use crate::cancel::{CancelRequest, CancelSource};
use crate::choice::{ChoiceSet, Chosen, Pick, Step};
use crate::widgets::wedge::{bearing, bearing_of, wedge_at};

/// Where a radial opens.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RadialAt {
    /// Around the mouse cursor.
    Cursor,
    /// In the middle of the screen.
    Center,
    /// Around a point, in logical pixels from the window's top left.
    Point(Vec2),
}

/// Opens a radial. `held` says it was opened by something the user is holding
/// down, so letting go chooses; otherwise it stays open until the user clicks
/// or presses Enter. Either way a ring that's let go of before the pointer
/// leaves the middle stays open. Trigger it on the radial.
#[derive(EntityEvent, Clone, Copy, Debug, PartialEq)]
pub struct OpenRadial {
    /// The radial to open.
    #[event_target]
    pub radial: Entity,
    /// Where it opens.
    pub at: RadialAt,
    /// Whether something the user is holding down opened it.
    pub held: bool,
}

/// Tells a radial opened as held that what held it, such as a key, was let go.
/// A mouse button let go needs no telling.
#[derive(EntityEvent, Clone, Copy, Debug, PartialEq)]
pub struct ReleaseRadial {
    /// The radial let go of.
    #[event_target]
    pub radial: Entity,
}

/// What a radial does when the user lets go or confirms.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Outcome {
    Close,
    Pick(Option<usize>),
}

impl Radial {
    /// Opens it. Opened from the keys, it starts at the choice; from the
    /// pointer, at nothing, until the pointer moves.
    fn open(
        &mut self,
        choices: &mut ChoiceSet,
        chosen: Option<usize>,
        phase: Phase,
        wanted: Option<Vec2>,
        from_keys: bool,
        mouse: Option<Vec2>,
    ) {
        self.phase = phase;
        self.wanted = wanted;
        self.middle = wanted.unwrap_or(self.middle);
        self.last_pointer = mouse;
        let start = chosen.filter(|&index| from_keys && choices.can_pick(index));
        choices.highlight(start);
        self.aim = start.map(|index| bearing_of(index, choices.len()));
    }

    /// What letting go now does.
    fn outcome(&self, choices: &ChoiceSet) -> Outcome {
        match (self.aim, choices.highlighted()) {
            (None, _) => match self.center_means {
                RadialCenter::Keep => Outcome::Close,
                RadialCenter::Clear => Outcome::Pick(None),
            },
            (Some(_), Some(index)) => Outcome::Pick(Some(index)),
            // Pointing at a disabled option.
            (Some(_), None) => Outcome::Close,
        }
    }

    /// Lets go of whatever held it open: chooses, or stays open if the pointer
    /// never left the middle.
    fn let_go(&mut self, choices: &ChoiceSet) -> Option<Outcome> {
        match self.phase {
            Phase::Held { left_center: false } => {
                self.phase = Phase::Latched { armed: false };
                None
            }
            Phase::Held { left_center: true } | Phase::Latched { armed: true } => {
                Some(self.outcome(choices))
            }
            Phase::Latched { armed: false } | Phase::Closed => None,
        }
    }
}

/// Closes `radial` with `outcome`, picking if it picks.
fn finish(root: Entity, radial: &mut Radial, outcome: Outcome, commands: &mut Commands) {
    radial.phase = Phase::Closed;
    if let Outcome::Pick(index) = outcome {
        commands.trigger(Pick { set: root, index });
    }
}

/// What a key does to an open radial. The one place its keys are read, for
/// the key mapping to take over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RadialKey {
    /// Goes round to the next option, clockwise.
    Next,
    /// Goes round to the previous option.
    Previous,
    /// Chooses what's highlighted, or does what the middle does.
    Confirm,
}

impl RadialKey {
    fn of(key: &Key) -> Option<Self> {
        match key {
            Key::ArrowRight | Key::ArrowDown => Some(Self::Next),
            Key::ArrowLeft | Key::ArrowUp => Some(Self::Previous),
            Key::Enter | Key::Space => Some(Self::Confirm),
            _ => None,
        }
    }
}

/// Where the mouse is, in logical pixels, if it's anywhere.
fn mouse_at(pointers: &Query<(&PointerId, &PointerLocation)>) -> Option<Vec2> {
    pointers
        .iter()
        .find(|(id, _)| **id == PointerId::Mouse)
        .and_then(|(_, location)| location.location.as_ref())
        .map(|location| location.position)
}

type Openable = (&'static mut Radial, &'static mut ChoiceSet, &'static Chosen);

/// Pressing a radial button opens its ring around the press, held.
pub(super) fn open_on_press(
    press: On<PointerPress>,
    mut radials: Query<Openable, (With<Button>, Without<InteractionDisabled>)>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    let Ok((mut radial, mut choices, chosen)) = radials.get_mut(press.entity) else {
        return;
    };
    if !radial.is_open() {
        let at = press.pointer.position;
        let held = Phase::Held { left_center: false };
        radial.open(&mut choices, chosen.0, held, Some(at), false, Some(at));
    }
}

/// A radial button activated (by Enter, Space or a click): opens at the
/// choice if closed, stays open if it was being held, chooses if left open.
pub(super) fn activate(
    activate: On<Activate>,
    mut radials: Query<(Openable, &ComputedNode, &UiGlobalTransform), Without<InteractionDisabled>>,
    pointers: Query<(&PointerId, &PointerLocation)>,
    mut commands: Commands,
) {
    let root = activate.entity;
    let Ok(((mut radial, mut choices, chosen), node, transform)) = radials.get_mut(root) else {
        return;
    };
    match radial.phase {
        Phase::Closed => {
            let middle = transform.translation * node.inverse_scale_factor;
            let latched = Phase::Latched { armed: false };
            let mouse = mouse_at(&pointers);
            radial.open(&mut choices, chosen.0, latched, Some(middle), true, mouse);
        }
        Phase::Held { .. } => radial.phase = Phase::Latched { armed: false },
        Phase::Latched { .. } => {
            let outcome = radial.outcome(&choices);
            finish(root, &mut radial, outcome, &mut commands);
        }
    }
}

/// Opens a radial where the game asks.
pub(super) fn open_on_request(
    request: On<OpenRadial>,
    mut radials: Query<Openable, Without<InteractionDisabled>>,
    pointers: Query<(&PointerId, &PointerLocation)>,
) {
    let Ok((mut radial, mut choices, chosen)) = radials.get_mut(request.radial) else {
        return;
    };
    if radial.is_open() {
        return;
    }
    let mouse = mouse_at(&pointers);
    let wanted = match request.at {
        RadialAt::Cursor => mouse,
        RadialAt::Center => None,
        RadialAt::Point(point) => Some(point),
    };
    let phase = if request.held {
        Phase::Held { left_center: false }
    } else {
        Phase::Latched { armed: false }
    };
    radial.open(&mut choices, chosen.0, phase, wanted, !request.held, mouse);
}

/// Lets go of a radial the game opened as held, when what held it is let go.
pub(super) fn release_on_request(
    request: On<ReleaseRadial>,
    mut radials: Query<(&mut Radial, &ChoiceSet)>,
    mut commands: Commands,
) {
    let root = request.radial;
    if let Ok((mut radial, choices)) = radials.get_mut(root)
        && let Some(outcome) = radial.let_go(choices)
    {
        finish(root, &mut radial, outcome, &mut commands);
    }
}

/// Aims each open radial where the mouse has moved to: the option whose wedge
/// it's in, or nothing in the dead zone.
pub(super) fn follow_pointer(
    pointers: Query<(&PointerId, &PointerLocation)>,
    mut radials: Query<(&mut Radial, &mut ChoiceSet)>,
) {
    let Some(mouse) = mouse_at(&pointers) else {
        return;
    };
    for (mut radial, mut choices) in &mut radials {
        if !radial.is_open() || radial.last_pointer == Some(mouse) {
            continue;
        }
        radial.last_pointer = Some(mouse);
        let offset = mouse - radial.middle;
        let aim = (offset.length() >= radial.dead_zone).then(|| bearing(offset));
        radial.aim = aim;
        if aim.is_some()
            && let Phase::Held { left_center } = &mut radial.phase
        {
            *left_center = true;
        }
        let wanted = aim.map(|aim| wedge_at(aim, choices.len()));
        let wanted = wanted.filter(|&index| choices.can_pick(index));
        if choices.highlighted() != wanted {
            choices.highlight(wanted);
        }
    }
}

/// Mouse buttons, wherever they're pressed: a press arms a radial left open,
/// so the click that follows chooses; letting go chooses on a held one.
pub(super) fn press_and_release(
    mut inputs: MessageReader<PointerInput>,
    mut radials: Query<(Entity, &mut Radial, &ChoiceSet)>,
    mut commands: Commands,
) {
    for input in inputs.read() {
        for (root, mut radial, choices) in &mut radials {
            match input.action {
                PointerAction::Press(PointerButton::Primary) => {
                    if radial.phase == (Phase::Latched { armed: false }) {
                        radial.phase = Phase::Latched { armed: true };
                    }
                }
                PointerAction::Release(_) => {
                    if let Some(outcome) = radial.let_go(choices) {
                        finish(root, &mut radial, outcome, &mut commands);
                    }
                }
                _ => {}
            }
        }
    }
}

/// While a radial is open: the arrow keys go round it, and Enter or Space
/// chooses, unless it's a button, whose activation chooses instead.
pub(super) fn work_with_keys(
    mut keys: On<FocusedInput<KeyboardInput>>,
    mut radials: Query<(&mut Radial, &mut ChoiceSet, Has<Button>)>,
    mut commands: Commands,
) {
    let root = keys.focused_entity;
    let Ok((mut radial, mut choices, is_button)) = radials.get_mut(root) else {
        return;
    };
    if !radial.is_open() || !keys.input.state.is_pressed() {
        return;
    }
    let step = match RadialKey::of(&keys.input.logical_key) {
        Some(RadialKey::Next) => Step::Next,
        Some(RadialKey::Previous) => Step::Previous,
        Some(RadialKey::Confirm) if !is_button => {
            keys.propagate(false);
            let outcome = radial.outcome(&choices);
            finish(root, &mut radial, outcome, &mut commands);
            return;
        }
        Some(RadialKey::Confirm) | None => return,
    };
    keys.propagate(false);
    let count = choices.len();
    radial.aim = choices
        .step(step)
        .map(|index| bearing_of(index, count))
        .or(radial.aim);
}

/// Escape closes an open radial without choosing, and so does a right click on
/// one left open, before anything under it sees them.
pub(super) fn close_on_cancel(mut cancel: ResMut<CancelRequest>, mut radials: Query<&mut Radial>) {
    let Some(source) = cancel.peek() else {
        return;
    };
    let mut claimed = false;
    for mut radial in &mut radials {
        let closes = match source {
            CancelSource::Escape => radial.is_open(),
            CancelSource::RightClick => matches!(radial.phase, Phase::Latched { .. }),
        };
        if closes {
            radial.phase = Phase::Closed;
            claimed = true;
        }
    }
    if claimed {
        cancel.consume();
    }
}

/// A pick by pointer on one of its options closes the radial.
pub(super) fn close_on_pick(pick: On<Pick>, mut radials: Query<(&mut Radial, &ChoiceSet)>) {
    if let Ok((mut radial, choices)) = radials.get_mut(pick.set)
        && pick.index.is_none_or(|index| choices.can_pick(index))
    {
        radial.phase = Phase::Closed;
    }
}

/// A press on an open ring, its options included, stays there: a radial
/// button under it isn't pressed again. Observed on each radial's scrim.
pub(super) fn keep_presses(mut press: On<PointerPress>) {
    press.propagate(false);
}

/// A release on an open ring stays there.
pub(super) fn keep_releases(mut release: On<PointerRelease>) {
    release.propagate(false);
}

/// A click on an open ring stays there: a radial button under it isn't
/// activated again.
pub(super) fn keep_clicks(mut click: On<PointerClick>) {
    click.propagate(false);
}

#[cfg(test)]
mod tests {
    use bevy::input::ButtonState;
    use bevy::input::keyboard::KeyCode;
    use bevy::picking::backend::HitData;
    use bevy::picking::events::Pointer;
    use bevy::ui::Pressed;

    use super::super::tests::{
        Announced, AnnouncedMaybe, EAST, MIDDLE, NORTH, SOUTH, app, compass, location, parts, spawn,
    };
    use super::*;
    use crate::choice::Choice;

    /// Moves the mouse to `offset` from the middle of the tests' rings.
    fn point(app: &mut App, offset: Vec2) {
        let world = app.world_mut();
        let mut pointers = world.query::<&mut PointerLocation>();
        for mut pointer in pointers.iter_mut(world) {
            *pointer = PointerLocation::new(location(MIDDLE + offset));
        }
        app.update();
    }

    fn mouse_input(app: &mut App, action: PointerAction) {
        let input = PointerInput::new(PointerId::Mouse, location(MIDDLE), action);
        app.world_mut().write_message(input);
    }

    /// Presses a radial button where the mouse is, as picking reports it.
    fn press_button(app: &mut App, button: Entity) {
        let world = app.world_mut();
        let at = world
            .query::<&PointerLocation>()
            .single(world)
            .unwrap()
            .location
            .as_ref()
            .unwrap()
            .position;
        world.trigger(PointerPress {
            entity: button,
            pointer: Pointer::new(PointerId::Mouse, location(at)),
            button: PointerButton::Primary,
            hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
            count: 1,
        });
        mouse_input(app, PointerAction::Press(PointerButton::Primary));
        app.update();
    }

    fn release(app: &mut App) {
        mouse_input(app, PointerAction::Release(PointerButton::Primary));
        app.update();
    }

    fn click(app: &mut App) {
        mouse_input(app, PointerAction::Press(PointerButton::Primary));
        app.update();
        release(app);
    }

    fn key(app: &mut App, radial: Entity, code: KeyCode, key: Key) {
        let keyboard = KeyboardInput {
            key_code: code,
            logical_key: key,
            state: ButtonState::Pressed,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        };
        app.world_mut()
            .trigger(FocusedInput::new(radial, keyboard, Entity::PLACEHOLDER));
        app.update();
    }

    fn is_open(app: &App, radial: Entity) -> bool {
        app.world().get::<Radial>(radial).unwrap().is_open()
    }

    fn chosen(app: &App, radial: Entity) -> Option<usize> {
        app.world().get::<Chosen>(radial).unwrap().0
    }

    fn highlighted(app: &App, radial: Entity) -> Option<usize> {
        app.world().get::<ChoiceSet>(radial).unwrap().highlighted()
    }

    fn announced(app: &mut App) -> Vec<usize> {
        std::mem::take(&mut app.world_mut().resource_mut::<Announced>().0)
    }

    fn announced_maybe(app: &mut App) -> Vec<Option<usize>> {
        std::mem::take(&mut app.world_mut().resource_mut::<AnnouncedMaybe>().0)
    }

    #[test]
    fn holding_the_button_aiming_and_letting_go_chooses() {
        let mut app = app();
        let radial = spawn(&mut app, compass(), true);
        press_button(&mut app, radial);
        assert!(is_open(&app, radial));
        point(&mut app, EAST);
        assert_eq!(highlighted(&app, radial), Some(1));
        point(&mut app, EAST * 3.0);
        assert_eq!(highlighted(&app, radial), Some(1), "however far out");
        release(&mut app);
        assert!(!is_open(&app, radial));
        assert_eq!(chosen(&app, radial), Some(1));
        assert_eq!(announced(&mut app), [1]);
        assert!(
            !app.world().entity(radial).contains::<Pressed>(),
            "the button's let go of"
        );
    }

    #[test]
    fn a_quick_press_leaves_it_open_for_a_click() {
        let mut app = app();
        let radial = spawn(&mut app, compass(), true);
        press_button(&mut app, radial);
        release(&mut app);
        assert!(is_open(&app, radial), "let go in the middle, it stays open");
        point(&mut app, SOUTH);
        click(&mut app);
        assert!(!is_open(&app, radial));
        assert_eq!(chosen(&app, radial), Some(2));
    }

    #[test]
    fn letting_go_in_the_middle_keeps_the_choice_or_clears_it() {
        let mut app = app();
        let keeps = spawn(&mut app, compass().chosen(1), true);
        press_button(&mut app, keeps);
        point(&mut app, NORTH);
        point(&mut app, Vec2::ZERO);
        release(&mut app);
        assert!(
            !is_open(&app, keeps),
            "it left the middle, so letting go closes"
        );
        assert_eq!(chosen(&app, keeps), Some(1));
        assert!(announced(&mut app).is_empty());

        let clears = spawn(
            &mut app,
            compass().chosen(1).center(RadialCenter::Clear),
            true,
        );
        press_button(&mut app, clears);
        point(&mut app, NORTH);
        point(&mut app, Vec2::ZERO);
        release(&mut app);
        assert_eq!(chosen(&app, clears), None);
        assert_eq!(announced_maybe(&mut app), [None]);
    }

    #[test]
    fn the_keys_open_it_at_the_choice_go_round_and_choose() {
        let mut app = app();
        let config = super::super::RadialConfig::new([
            Choice::text("North"),
            "East".into(),
            "South".into(),
            Choice::text("West").disabled(true),
        ])
        .chosen(0);
        let radial = spawn(&mut app, config, true);
        app.world_mut().trigger(Activate { entity: radial });
        app.update();
        assert!(is_open(&app, radial));
        assert_eq!(highlighted(&app, radial), Some(0), "opened at the choice");
        key(&mut app, radial, KeyCode::ArrowLeft, Key::ArrowLeft);
        assert_eq!(
            highlighted(&app, radial),
            Some(2),
            "round past the disabled one"
        );
        point(&mut app, Vec2::ZERO);
        assert_eq!(
            highlighted(&app, radial),
            Some(2),
            "the mouse only aims once it moves"
        );
        app.world_mut().trigger(Activate { entity: radial });
        app.update();
        assert!(!is_open(&app, radial));
        assert_eq!(chosen(&app, radial), Some(2));
    }

    #[test]
    fn a_ring_opened_by_the_game_chooses_when_its_key_is_let_go() {
        let mut app = app();
        let radial = spawn(&mut app, compass(), false);
        app.world_mut().trigger(OpenRadial {
            radial,
            at: RadialAt::Cursor,
            held: true,
        });
        app.update();
        assert!(is_open(&app, radial));
        point(&mut app, NORTH);
        app.world_mut().trigger(ReleaseRadial { radial });
        app.update();
        assert!(!is_open(&app, radial));
        assert_eq!(chosen(&app, radial), Some(0));
    }

    #[test]
    fn escape_closes_it_without_choosing() {
        let mut app = app();
        let radial = spawn(&mut app, compass(), false);
        app.world_mut().trigger(OpenRadial {
            radial,
            at: RadialAt::Cursor,
            held: false,
        });
        app.update();
        point(&mut app, EAST);
        for state in [ButtonState::Pressed, ButtonState::Released] {
            app.world_mut().write_message(KeyboardInput {
                key_code: KeyCode::Escape,
                logical_key: Key::Escape,
                state,
                text: None,
                repeat: false,
                window: Entity::PLACEHOLDER,
            });
            app.update();
        }
        assert!(!is_open(&app, radial));
        assert_eq!(chosen(&app, radial), None);
    }

    #[test]
    fn a_disabled_radial_doesnt_open() {
        let mut app = app();
        let radial = spawn(&mut app, compass().disabled(true), true);
        press_button(&mut app, radial);
        app.world_mut().trigger(Activate { entity: radial });
        app.update();
        assert!(!is_open(&app, radial));
    }

    #[test]
    fn a_press_on_the_open_ring_stays_on_the_ring() {
        let mut app = app();
        let radial = spawn(&mut app, compass(), true);
        let scrim = parts(&app, radial).scrim;
        press_button(&mut app, radial);
        release(&mut app);
        assert!(is_open(&app, radial));
        // A press on the scrim doesn't reach the button behind it.
        app.world_mut().trigger(PointerPress {
            entity: scrim,
            pointer: Pointer::new(PointerId::Mouse, location(MIDDLE)),
            button: PointerButton::Primary,
            hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
            count: 1,
        });
        app.update();
        assert!(is_open(&app, radial), "still left open, not reopened");
    }
}
