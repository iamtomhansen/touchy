//! Choices: picking one of several options, apart from how they're laid out.
//!
//! A dropdown's list, a row of segments, a ring around a button and a track of
//! gears all do the same thing: show some options, highlight one as the user
//! moves among them, and pick one. This module is that part. The widget keeps
//! the rest, which is what makes it that widget: where the options sit, how the
//! highlight is drawn, what the keys mean, and when the user is asked at all.
//!
//! - A [`Choice`] is one option: its name in words, and what it shows. Plain
//!   words show themselves; a richer choice builds anything touchy can draw
//!   (a swatch and a name, a meter, a picture, an animation) with a closure.
//!   A look is a recipe rather than an entity, so a widget can build a choice
//!   wherever it's wanted and as often: in its row, and again on a button
//!   showing what's chosen.
//! - A [`ChoiceSet`] on the widget holds the choices and the highlighted one.
//!   The highlight is apart from the choice, which is [`Chosen`]: moving
//!   through a list doesn't choose until the user says so.
//! - A [`ChoiceItem`] marks where the user picks one choice: a row, a segment.
//!   The pointer highlights the item it moves over, and clicking one picks it.
//!   Only movement highlights: an item scrolled under a pointer that's
//!   standing still doesn't take the highlight from the keys. Widgets whose
//!   options aren't entities to point at (a ring, read by the pointer's angle)
//!   set the highlight themselves.
//! - [`Pick`] picks a choice, as the user does: it's chosen, highlighted, and,
//!   if that changed the choice, announced on the set. Inserting a new
//!   [`Chosen`] chooses from code, without announcing it.
//!
//! Whether the user can go back to choosing nothing is the widget's call. A
//! set is clearable or it isn't, and its announcement says which: a set that
//! always holds a choice once one's made announces `ValueChange<usize>`; a
//! [clearable](ChoiceSet::clearable) one announces `ValueChange<Option<usize>>`,
//! `None` when the user clears it, so one observer sees every change. Likewise
//! stepping stops at the ends, unless the set [wraps](ChoiceSet::wrapping), as
//! a ring does.
//!
//! A set can be [filtered](ChoiceSet::filter), as a combo box narrows its
//! list to what's typed: choices filtered out can't be highlighted or picked,
//! and stepping skips them. And a widget whose value isn't the index, such as
//! a combo box that also takes free text, can make its set
//! [quiet](ChoiceSet::quiet) and announce its own value instead.
//!
//! Each item carries its [`ChoiceState`] (highlighted, chosen, disabled), kept
//! up to date, so whatever a choice built can respond to it: a swatch outlined
//! while highlighted, an animation that plays only then. [`ChoiceStates`]
//! finds the state from anywhere inside an item.
//!
//! A disabled choice can't be highlighted or picked, and stepping skips it. A
//! set marked `InteractionDisabled` can't be changed by the user at all.

use std::sync::Arc;

use bevy::ecs::system::SystemParam;
use bevy::picking::events::{PointerClick, PointerMove};
use bevy::prelude::*;
use bevy::ui::{InteractionDisabled, UiSystems};
use bevy::ui_widgets::ValueChange;

/// What builds a choice's look.
type BuildChoice = dyn Fn(&mut ChildSpawnerCommands) + Send + Sync;

/// What a choice shows.
#[derive(Clone)]
enum ChoiceLook {
    /// Its label, as text.
    Label,
    /// Whatever the closure builds.
    Built(Arc<BuildChoice>),
}

/// One option: its name in words, what it shows, and whether it can be picked.
#[derive(Clone)]
pub struct Choice {
    /// The choice's name in words. A built choice keeps one too, for reading
    /// it back and for finding it by typing.
    pub label: String,
    look: ChoiceLook,
    disabled: bool,
}

impl Choice {
    /// A choice showing its label.
    pub fn text(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            look: ChoiceLook::Label,
            disabled: false,
        }
    }

    /// A choice showing whatever `build` spawns: an icon and a name, a picture,
    /// anything. A widget may build it more than once (in its list, and again
    /// where it shows the choice), so it should build the same way each time.
    /// `label` names it in words.
    pub fn new(
        label: impl Into<String>,
        build: impl Fn(&mut ChildSpawnerCommands) + Send + Sync + 'static,
    ) -> Self {
        Self {
            label: label.into(),
            look: ChoiceLook::Built(Arc::new(build)),
            disabled: false,
        }
    }

    /// A disabled choice is shown but can't be highlighted or picked.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
    /// Whether the choice can be picked.
    pub fn is_disabled(&self) -> bool {
        self.disabled
    }
    /// Builds what the choice shows into `parent`. A plain choice is its label,
    /// spawned as `words` makes it, so the widget decides how its text looks,
    /// and returned so the widget can restyle it; a built one is whatever its
    /// closure spawns, and returns `None`.
    pub fn build<B: Bundle>(
        &self,
        parent: &mut ChildSpawnerCommands,
        words: impl FnOnce(&str) -> B,
    ) -> Option<Entity> {
        match &self.look {
            ChoiceLook::Label => Some(parent.spawn((words(&self.label), Pickable::IGNORE)).id()),
            ChoiceLook::Built(build) => {
                build(parent);
                None
            }
        }
    }
}

impl std::fmt::Debug for Choice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Choice")
            .field("label", &self.label)
            .field("disabled", &self.disabled)
            .finish_non_exhaustive()
    }
}

impl<S: Into<String>> From<S> for Choice {
    fn from(label: S) -> Self {
        Self::text(label)
    }
}

/// Which choice is chosen, by its place in the set, or none yet. Insert a new
/// one to choose from code; that isn't announced.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Chosen(pub Option<usize>);

/// Where the highlight goes, for [`ChoiceSet::step`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// The next choice that can be picked, or the first if none is highlighted.
    /// Past the last, the first again if the set wraps.
    Next,
    /// The one before, or the last if none is highlighted. Before the first,
    /// the last if the set wraps.
    Previous,
    /// The first choice that can be picked, or none if there are none.
    First,
    /// The last choice that can be picked, or none if there are none.
    Last,
}

/// A widget's choices, and the one highlighted. Goes on the widget, with its
/// [`Chosen`].
#[derive(Component, Clone, Debug)]
#[require(Chosen)]
pub struct ChoiceSet {
    choices: Vec<Choice>,
    highlighted: Option<usize>,
    clearable: bool,
    wrapping: bool,
    quiet: bool,
    /// Which choices the filter keeps, or `None` for all of them.
    shown: Option<Vec<bool>>,
}

impl ChoiceSet {
    /// Creates a set of `choices` in order, with nothing highlighted by default.
    pub fn new(choices: impl IntoIterator<Item = impl Into<Choice>>) -> Self {
        Self {
            choices: choices.into_iter().map(Into::into).collect(),
            highlighted: None,
            clearable: false,
            wrapping: false,
            quiet: false,
            shown: None,
        }
    }
    /// Picks change the choice without announcing it, for a widget that
    /// announces a value of its own.
    pub fn quiet(mut self, quiet: bool) -> Self {
        self.quiet = quiet;
        self
    }
    /// Shows only the choices `keep` says to, until the next filter or
    /// [`clear_filter`](Self::clear_filter). A highlight on a choice filtered
    /// out goes.
    pub fn filter(&mut self, mut keep: impl FnMut(&Choice) -> bool) {
        self.shown = Some(self.choices.iter().map(&mut keep).collect());
        self.highlighted = self.highlighted.filter(|&index| self.can_pick(index));
    }
    /// Shows every choice again.
    pub fn clear_filter(&mut self) {
        self.shown = None;
    }
    /// Whether the filter keeps the choice at `index`.
    pub fn is_shown(&self, index: usize) -> bool {
        self.shown
            .as_ref()
            .is_none_or(|shown| shown.get(index).copied().unwrap_or(false))
    }
    /// Lets the user go back to choosing nothing, with a [`Pick`] of `None`.
    /// A clearable set announces `ValueChange<Option<usize>>` rather than
    /// `ValueChange<usize>`, since its value can be none.
    pub fn clearable(mut self, clearable: bool) -> Self {
        self.clearable = clearable;
        self
    }
    /// Steps past either end to the other, as around a ring.
    pub fn wrapping(mut self, wrapping: bool) -> Self {
        self.wrapping = wrapping;
        self
    }
    /// Whether the user can go back to choosing nothing.
    pub fn is_clearable(&self) -> bool {
        self.clearable
    }
    /// The list of choices, in order.
    pub fn choices(&self) -> &[Choice] {
        &self.choices
    }
    /// The choice at `index`, or `None` if there isn't one.
    pub fn get(&self, index: usize) -> Option<&Choice> {
        self.choices.get(index)
    }
    /// The number of choices in the set.
    pub fn len(&self) -> usize {
        self.choices.len()
    }
    /// Whether the set has no choices.
    pub fn is_empty(&self) -> bool {
        self.choices.is_empty()
    }
    /// Whether the choice at `index` exists, isn't disabled, and isn't
    /// filtered out.
    pub fn can_pick(&self, index: usize) -> bool {
        self.get(index).is_some_and(|choice| !choice.disabled) && self.is_shown(index)
    }
    /// The index of the highlighted choice, or `None` if there isn't one.
    pub fn highlighted(&self) -> Option<usize> {
        self.highlighted
    }
    /// Highlights the choice at `index`, or nothing if it's `None` or can't be
    /// picked.
    pub fn highlight(&mut self, index: Option<usize>) {
        self.highlighted = index.filter(|&index| self.can_pick(index));
    }
    /// Moves the highlight, skipping choices that can't be picked. At an end
    /// it wraps round if the set wraps, and otherwise stays put. Returns where
    /// it is.
    pub fn step(&mut self, step: Step) -> Option<usize> {
        let pickable = || (0..self.len()).filter(|&index| self.can_pick(index));
        let (first, last) = (pickable().next(), pickable().next_back());
        let past_the_end = |at| if self.wrapping { None } else { Some(at) };
        let to = match (step, self.highlighted) {
            (Step::First, _) | (Step::Next, None) => first,
            (Step::Last, _) | (Step::Previous, None) => last,
            (Step::Next, Some(at)) => pickable()
                .find(|&index| index > at)
                .or(past_the_end(at))
                .or(first),
            (Step::Previous, Some(at)) => pickable()
                .take_while(|&index| index < at)
                .last()
                .or(past_the_end(at))
                .or(last),
        };
        self.highlighted = to;
        to
    }
}

/// Where the user picks one choice of a set: a row, a segment. The pointer
/// highlights it, and clicking it picks it.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
#[require(ChoiceState)]
pub struct ChoiceItem {
    /// The entity holding the [`ChoiceSet`].
    pub set: Entity,
    /// The choice's numbered index in the set.
    pub index: usize,
}

/// How an item's choice stands, kept up to date for whatever it shows.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ChoiceState {
    /// The choice is currently highlighted within the set.
    pub highlighted: bool,
    /// The choice is currenty chosen within the set.
    pub chosen: bool,
    /// The choice is disabled, or its whole set is.
    pub disabled: bool,
}

/// Finds the [`ChoiceState`] of the item an entity is in, for something a
/// choice built that responds to it.
#[derive(SystemParam)]
pub struct ChoiceStates<'w, 's> {
    parents: Query<'w, 's, &'static ChildOf>,
    states: Query<'w, 's, &'static ChoiceState>,
}

impl ChoiceStates<'_, '_> {
    /// The state of the item `entity` is, or is inside. `None` outside any, as
    /// for a copy of a choice showing what's chosen.
    pub fn of(&self, entity: Entity) -> Option<ChoiceState> {
        std::iter::once(entity)
            .chain(self.parents.iter_ancestors(entity))
            .find_map(|entity| self.states.get(entity).ok().copied())
    }
}

/// Picks the choice at `index` of the set on `set`, as the user does, or, for
/// `None`, clears the choice. Ignored if the choice can't be picked, if the set
/// isn't clearable and this would clear it, or if the set is disabled.
#[derive(EntityEvent, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pick {
    /// The entity holding the [`ChoiceSet`].
    #[event_target]
    pub set: Entity,
    /// The choice's numbered index in the set, or `None` to clear it.
    pub index: Option<usize>,
}

/// Runs in `PostUpdate`, before UI layout, keeping each item's
/// [`ChoiceState`]. Widgets that draw from it go after.
#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ChoiceSystems;

fn apply_picks(
    pick: On<Pick>,
    mut sets: Query<(&mut ChoiceSet, &mut Chosen), Without<InteractionDisabled>>,
    mut commands: Commands,
) {
    let Ok((mut set, mut chosen)) = sets.get_mut(pick.set) else {
        return;
    };
    let allowed = match pick.index {
        Some(index) => set.can_pick(index),
        None => set.clearable,
    };
    if !allowed {
        return;
    }
    set.highlighted = pick.index;
    if chosen.0 == pick.index {
        return;
    }
    chosen.0 = pick.index;
    if set.quiet {
        return;
    }
    let source = pick.set;
    match (set.clearable, pick.index) {
        (true, value) => commands.trigger(ValueChange {
            source,
            value,
            is_final: true,
        }),
        (false, Some(value)) => commands.trigger(ValueChange {
            source,
            value,
            is_final: true,
        }),
        (false, None) => {}
    }
}

/// Clicking an item picks its choice.
fn pick_on_click(mut click: On<PointerClick>, items: Query<&ChoiceItem>, mut commands: Commands) {
    let Ok(&ChoiceItem { set, index }) = items.get(click.entity) else {
        return;
    };
    click.propagate(false);
    commands.trigger(Pick {
        set,
        index: Some(index),
    });
}

/// The pointer highlights the item it moves over, so the keys carry on from
/// it. Movement only: when the keys scroll a list under a pointer that's still,
/// whatever slides under it isn't highlighted in their place.
fn highlight_on_hover(
    moved: On<PointerMove>,
    items: Query<&ChoiceItem>,
    mut sets: Query<&mut ChoiceSet, Without<InteractionDisabled>>,
) {
    if let Ok(&ChoiceItem { set, index }) = items.get(moved.entity)
        && let Ok(mut set) = sets.get_mut(set)
    {
        let wanted = Some(index).filter(|&index| set.can_pick(index));
        if set.highlighted != wanted {
            set.highlighted = wanted;
        }
    }
}

fn show_choice_states(
    mut items: Query<(&ChoiceItem, &mut ChoiceState)>,
    sets: Query<(&ChoiceSet, &Chosen, Has<InteractionDisabled>)>,
) {
    for (item, mut state) in &mut items {
        let Ok((set, chosen, set_disabled)) = sets.get(item.set) else {
            continue;
        };
        state.set_if_neq(ChoiceState {
            highlighted: set.highlighted == Some(item.index),
            chosen: chosen.0 == Some(item.index),
            disabled: set_disabled || !set.can_pick(item.index),
        });
    }
}

/// Picking choices, by pointer and by [`Pick`], and keeping their states.
pub struct ChoicePlugin;

impl Plugin for ChoicePlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(apply_picks)
            .add_observer(pick_on_click)
            .add_observer(highlight_on_hover)
            .configure_sets(PostUpdate, ChoiceSystems.before(UiSystems::Prepare))
            .add_systems(PostUpdate, show_choice_states.in_set(ChoiceSystems));
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy::camera::NormalizedRenderTarget;
    use bevy::ecs::system::RunSystemOnce;
    use bevy::picking::backend::HitData;
    use bevy::picking::events::{Pointer, PointerOver};
    use bevy::picking::pointer::{Location, PointerButton, PointerId};

    use super::*;

    #[derive(Resource, Default)]
    struct Announced(Vec<usize>);

    /// What a clearable set announced.
    #[derive(Resource, Default)]
    struct AnnouncedMaybe(Vec<Option<usize>>);

    /// A set of `choices` with an item for each, under one parent.
    fn set_app(choices: Vec<Choice>) -> (App, Entity, Vec<Entity>) {
        set_app_with(ChoiceSet::new(choices))
    }

    fn set_app_with(choices: ChoiceSet) -> (App, Entity, Vec<Entity>) {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, ChoicePlugin))
            .init_resource::<Announced>()
            .init_resource::<AnnouncedMaybe>()
            .add_observer(
                |change: On<ValueChange<usize>>, mut announced: ResMut<Announced>| {
                    announced.0.push(change.value);
                },
            )
            .add_observer(
                |change: On<ValueChange<Option<usize>>>, mut announced: ResMut<AnnouncedMaybe>| {
                    announced.0.push(change.value);
                },
            );
        // Pointer events bubble up through parents only once windows are known.
        app.world_mut().register_component::<Window>();
        let count = choices.len();
        let world = app.world_mut();
        let set = world.spawn(choices).id();
        let items = (0..count)
            .map(|index| world.spawn((ChoiceItem { set, index }, ChildOf(set))).id())
            .collect();
        app.update();
        (app, set, items)
    }

    fn pick(app: &mut App, set: Entity, index: Option<usize>) {
        app.world_mut().trigger(Pick { set, index });
        app.update();
    }

    fn announced(app: &mut App) -> Vec<usize> {
        std::mem::take(&mut app.world_mut().resource_mut::<Announced>().0)
    }

    fn chosen(app: &App, set: Entity) -> Option<usize> {
        app.world().get::<Chosen>(set).unwrap().0
    }

    fn mouse() -> Pointer {
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

    fn no_hit() -> HitData {
        HitData::new(Entity::PLACEHOLDER, 0.0, None, None)
    }

    fn click(app: &mut App, entity: Entity) {
        app.world_mut().trigger(PointerClick {
            entity,
            pointer: mouse(),
            button: PointerButton::Primary,
            hit: no_hit(),
            duration: Duration::ZERO,
            count: 1,
        });
        app.update();
    }

    fn three_with_middle_disabled() -> Vec<Choice> {
        vec!["A".into(), Choice::text("B").disabled(true), "C".into()]
    }

    #[test]
    fn stepping_skips_what_cant_be_picked_and_stops_at_the_ends() {
        let mut set = ChoiceSet::new([
            Choice::text("A").disabled(true),
            "B".into(),
            Choice::text("C").disabled(true),
            "D".into(),
            Choice::text("E").disabled(true),
        ]);
        assert_eq!(set.step(Step::Next), Some(1), "from nothing, the first");
        assert_eq!(set.step(Step::Next), Some(3));
        assert_eq!(set.step(Step::Next), Some(3), "nowhere further");
        assert_eq!(set.step(Step::Previous), Some(1));
        assert_eq!(set.step(Step::Previous), Some(1));
        assert_eq!(set.step(Step::Last), Some(3));
        assert_eq!(set.step(Step::First), Some(1));
        set.highlight(None);
        assert_eq!(set.step(Step::Previous), Some(3), "from nothing, the last");
        set.highlight(Some(2));
        assert_eq!(
            set.highlighted(),
            None,
            "a disabled choice isn't highlighted"
        );
    }

    #[test]
    fn a_filter_hides_choices_from_highlighting_stepping_and_picking() {
        let mut set = ChoiceSet::new(["Louisville", "Moncton", "St. Louis"]);
        set.highlight(Some(1));
        set.filter(|choice| choice.label.contains("Louis"));
        assert_eq!(set.highlighted(), None, "a hidden highlight goes");
        assert_eq!(set.step(Step::Next), Some(0));
        assert_eq!(set.step(Step::Next), Some(2), "over the one filtered out");
        assert!(!set.can_pick(1));
        set.clear_filter();
        assert!(set.can_pick(1) && set.is_shown(1));
    }

    #[test]
    fn a_quiet_set_chooses_without_announcing() {
        let (mut app, set, _) = set_app_with(ChoiceSet::new(["A", "B"]).quiet(true));
        pick(&mut app, set, Some(1));
        assert_eq!(chosen(&app, set), Some(1));
        assert!(announced(&mut app).is_empty());
    }

    #[test]
    fn a_wrapping_set_steps_round_past_either_end() {
        let mut set = ChoiceSet::new(["A".into(), "B".into(), Choice::text("C").disabled(true)])
            .wrapping(true);
        set.highlight(Some(1));
        assert_eq!(
            set.step(Step::Next),
            Some(0),
            "past the disabled last, round to the first"
        );
        assert_eq!(set.step(Step::Previous), Some(1), "and back round");
    }

    #[test]
    fn a_clearable_set_goes_back_to_nothing_and_announces_an_option() {
        let (mut app, set, _) = set_app_with(ChoiceSet::new(["A", "B"]).clearable(true));
        pick(&mut app, set, Some(1));
        pick(&mut app, set, None);
        pick(&mut app, set, None);
        assert_eq!(chosen(&app, set), None);
        assert_eq!(
            std::mem::take(&mut app.world_mut().resource_mut::<AnnouncedMaybe>().0),
            [Some(1), None],
            "clearing twice is news once"
        );
        assert!(announced(&mut app).is_empty(), "never as a bare index");
    }

    #[test]
    fn moving_over_an_item_highlights_it_but_sliding_under_a_still_pointer_doesnt() {
        let (mut app, set, items) = set_app(three_with_middle_disabled());
        let highlighted = |app: &App| app.world().get::<ChoiceSet>(set).unwrap().highlighted();
        // What a list scrolled under the pointer gets: over, without a move.
        app.world_mut().trigger(PointerOver {
            entity: items[2],
            pointer: mouse(),
            hit: no_hit(),
        });
        app.update();
        assert_eq!(highlighted(&app), None);
        let move_over = |app: &mut App, item: Entity| {
            app.world_mut().trigger(PointerMove {
                entity: item,
                pointer: mouse(),
                hit: no_hit(),
                delta: Vec2::X,
            });
            app.update();
        };
        move_over(&mut app, items[2]);
        assert_eq!(highlighted(&app), Some(2));
        move_over(&mut app, items[1]);
        assert_eq!(highlighted(&app), None, "a disabled item takes none");
    }

    #[test]
    fn a_pick_chooses_and_announces_only_a_change() {
        let (mut app, set, _) = set_app(three_with_middle_disabled());
        pick(&mut app, set, Some(2));
        assert_eq!(chosen(&app, set), Some(2));
        assert_eq!(
            app.world().get::<ChoiceSet>(set).unwrap().highlighted(),
            Some(2)
        );
        pick(&mut app, set, Some(2));
        pick(&mut app, set, Some(1));
        pick(&mut app, set, None);
        assert_eq!(chosen(&app, set), Some(2), "the disabled one isn't picked");
        assert_eq!(announced(&mut app), [2]);

        app.world_mut().entity_mut(set).insert(InteractionDisabled);
        pick(&mut app, set, Some(0));
        assert_eq!(chosen(&app, set), Some(2), "nor anything in a disabled set");
    }

    #[test]
    fn clicking_an_item_or_anything_in_it_picks_its_choice() {
        let (mut app, set, items) = set_app(three_with_middle_disabled());
        let inside = app.world_mut().spawn(ChildOf(items[2])).id();
        click(&mut app, items[1]);
        assert_eq!(chosen(&app, set), None);
        click(&mut app, inside);
        assert_eq!(chosen(&app, set), Some(2));
        assert_eq!(announced(&mut app), [2]);
    }

    #[test]
    fn items_show_their_state_to_what_is_built_in_them() {
        #[derive(Resource, Default)]
        struct Seen(Option<ChoiceState>);
        let (mut app, set, items) = set_app(three_with_middle_disabled());
        let inside = app.world_mut().spawn(ChildOf(items[0])).id();
        let outside = app.world_mut().spawn_empty().id();
        app.world_mut()
            .get_mut::<ChoiceSet>(set)
            .unwrap()
            .highlight(Some(0));
        app.world_mut().entity_mut(set).insert(Chosen(Some(2)));
        app.update();
        let state = |app: &App, item: Entity| *app.world().get::<ChoiceState>(item).unwrap();
        assert_eq!(
            state(&app, items[0]),
            ChoiceState {
                highlighted: true,
                ..default()
            }
        );
        assert!(state(&app, items[1]).disabled);
        assert!(state(&app, items[2]).chosen);

        app.init_resource::<Seen>();
        let read = move |states: ChoiceStates, mut seen: ResMut<Seen>| {
            seen.0 = states.of(inside);
            assert_eq!(states.of(outside), None);
        };
        app.world_mut().run_system_once(read).unwrap();
        assert!(app.world().resource::<Seen>().0.unwrap().highlighted);
    }
}
