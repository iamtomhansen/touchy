//! Which entity shows the focus, as a component, so widgets hear of it the
//! way they hear of a hover or a press.
//!
//! Bevy keeps the focus in a resource, [`InputFocus`], and whether to show it
//! in another, [`InputFocusVisible`]: Tab shows it, a click hides it, as the
//! web's `:focus-visible` does. So a player moving through a menu with the
//! keyboard or a gamepad sees where they are, and a click leaves no lit
//! button behind.
//!
//! Bevy's directional navigation, which a gamepad usually drives, moves the
//! focus without showing it, so a game that uses it sets `InputFocusVisible`
//! to true as it moves, or keeps it true for a console-style UI.

use bevy::input_focus::{InputFocus, InputFocusVisible};
use bevy::prelude::*;

/// On the entity that has the focus while it's shown. A widget that picks
/// its look from components reads it as it reads [`Pressed`](bevy::ui::Pressed):
/// `Has<FocusVisible>` for its state, and `Added<FocusVisible>` with
/// `RemovedComponents<FocusVisible>` to restyle.
///
/// Fields that are typed into show their focus however they got it, so they
/// read [`InputFocus`] too. A widget built from others can pass the mark on
/// to the part that shows it, as a number input does to its field: touchy
/// only ever takes away the mark it put down.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[component(storage = "SparseSet")]
pub struct FocusVisible;

/// Moves [`FocusVisible`] to the entity that shows the focus, if any.
pub(super) fn mark_visible_focus(
    focus: Option<Res<InputFocus>>,
    visible: Option<Res<InputFocusVisible>>,
    mut marked: Local<Option<Entity>>,
    mut commands: Commands,
) {
    let (Some(focus), Some(visible)) = (focus, visible) else {
        return;
    };
    if !focus.is_changed() && !visible.is_changed() {
        return;
    }
    let shown = focus.get().filter(|_| visible.0);
    if *marked == shown {
        return;
    }
    if let Some(entity) = marked.take() {
        commands.entity(entity).try_remove::<FocusVisible>();
    }
    if let Some(entity) = shown {
        commands.entity(entity).try_insert(FocusVisible);
    }
    *marked = shown;
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::input_focus::FocusCause;

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<InputFocus>()
            .init_resource::<InputFocusVisible>()
            .add_systems(Update, mark_visible_focus);
        app
    }

    fn focus(app: &mut App, entity: Option<Entity>, visible: bool) {
        let mut focus = app.world_mut().resource_mut::<InputFocus>();
        match entity {
            Some(entity) => focus.set(entity, FocusCause::Navigated),
            None => focus.clear(),
        }
        app.world_mut().resource_mut::<InputFocusVisible>().0 = visible;
        app.update();
    }

    #[test]
    fn the_mark_follows_the_focus_only_while_it_shows() {
        let mut app = app();
        let [a, b] = [(); 2].map(|_| app.world_mut().spawn_empty().id());
        let marked =
            |app: &App, entity: Entity| app.world().entity(entity).contains::<FocusVisible>();

        focus(&mut app, Some(a), false);
        assert!(!marked(&app, a), "a click's focus isn't shown");
        focus(&mut app, Some(a), true);
        assert!(marked(&app, a));
        focus(&mut app, Some(b), true);
        assert!(!marked(&app, a) && marked(&app, b), "moved by Tab");
        focus(&mut app, Some(b), false);
        assert!(!marked(&app, b), "hidden by a click");
    }

    #[test]
    fn marks_put_down_elsewhere_are_left_alone() {
        let mut app = app();
        let [focused, part] = [(); 2].map(|_| app.world_mut().spawn_empty().id());
        app.world_mut().entity_mut(part).insert(FocusVisible);
        focus(&mut app, Some(focused), true);
        focus(&mut app, None, true);
        assert!(app.world().entity(part).contains::<FocusVisible>());
    }

    #[test]
    fn a_despawned_focus_is_no_trouble() {
        let mut app = app();
        let entity = app.world_mut().spawn_empty().id();
        focus(&mut app, Some(entity), true);
        app.world_mut().despawn(entity);
        focus(&mut app, None, true);
        focus(&mut app, Some(entity), true);
    }
}
