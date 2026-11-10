//! The cursor over a control: its own while the control is enabled, and
//! not-allowed while it's disabled.
//!
//! A control holds a [`HoverCursor`], and its [`EntityCursor`] follows from it
//! and from [`InteractionDisabled`], whenever either changes.

use bevy::picking::cursor::{CursorIconPlugin, EntityCursor};
use bevy::prelude::*;
use bevy::ui::InteractionDisabled;
use bevy::window::SystemCursorIcon;

use crate::EnsurePlugin;

/// The cursor shown over this entity: `enabled` while it's enabled, and
/// `disabled` while it's [disabled](InteractionDisabled), or the window's own
/// cursor if that's `None`. Insert a new one to change it.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
#[component(immutable)]
pub struct HoverCursor {
    /// The cursor while it's enabled.
    pub enabled: SystemCursorIcon,
    /// The cursor while it's disabled, or `None` for the window's own.
    pub disabled: Option<SystemCursorIcon>,
}

impl HoverCursor {
    /// The pointing hand, for something to click.
    pub const POINTER: Self = Self::new(SystemCursorIcon::Pointer);

    /// Shows `icon`, and not-allowed while disabled.
    pub const fn new(icon: SystemCursorIcon) -> Self {
        Self {
            enabled: icon,
            disabled: Some(SystemCursorIcon::NotAllowed),
        }
    }

    /// Leaves the window's own cursor while disabled, for something that
    /// isn't refused so much as not there, such as a grabber.
    pub const fn plain_while_disabled(self) -> Self {
        Self {
            disabled: None,
            ..self
        }
    }

    /// Shows the right cursor on `entity`, disabled or not.
    fn apply(self, entity: &mut EntityCommands, disabled: bool) {
        let icon = if disabled {
            self.disabled
        } else {
            Some(self.enabled)
        };
        match icon {
            Some(icon) => entity.insert(EntityCursor::System(icon)),
            None => entity.remove::<EntityCursor>(),
        };
    }
}

/// Sets the cursor when the hover cursor is set.
fn on_set(
    insert: On<Insert<HoverCursor>>,
    hovers: Query<(&HoverCursor, Has<InteractionDisabled>)>,
    mut commands: Commands,
) {
    if let Ok((hover, disabled)) = hovers.get(insert.entity) {
        hover.apply(&mut commands.entity(insert.entity), disabled);
    }
}

/// Shows the disabled cursor when an entity with a hover cursor is disabled.
fn on_disable(
    insert: On<Insert<InteractionDisabled>>,
    hovers: Query<&HoverCursor>,
    mut commands: Commands,
) {
    if let Ok(hover) = hovers.get(insert.entity) {
        hover.apply(&mut commands.entity(insert.entity), true);
    }
}

/// Shows the enabled cursor again when the entity is enabled.
fn on_enable(
    remove: On<Remove<InteractionDisabled>>,
    hovers: Query<&HoverCursor>,
    mut commands: Commands,
) {
    if let Ok(hover) = hovers.get(remove.entity) {
        hover.apply(&mut commands.entity(remove.entity), false);
    }
}

/// Keeps each [`HoverCursor`]'s entity showing the right cursor.
pub struct HoverCursorPlugin;

impl Plugin for HoverCursorPlugin {
    fn build(&self, app: &mut App) {
        app.ensure_plugin(CursorIconPlugin)
            .add_observer(on_set)
            .add_observer(on_disable)
            .add_observer(on_enable);
    }
    fn is_unique(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cursor(app: &App, entity: Entity) -> Option<EntityCursor> {
        app.world().get::<EntityCursor>(entity).cloned()
    }

    #[test]
    fn the_cursor_follows_the_hover_cursor_and_disabling() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, HoverCursorPlugin));
        let world = app.world_mut();
        let control = world
            .spawn((HoverCursor::POINTER, InteractionDisabled))
            .id();
        world.flush();
        let not_allowed = EntityCursor::System(SystemCursorIcon::NotAllowed);
        assert_eq!(cursor(&app, control), Some(not_allowed.clone()));

        let world = app.world_mut();
        world.entity_mut(control).remove::<InteractionDisabled>();
        world.flush();
        let pointer = EntityCursor::System(SystemCursorIcon::Pointer);
        assert_eq!(cursor(&app, control), Some(pointer));

        let world = app.world_mut();
        let resize = HoverCursor::new(SystemCursorIcon::ColResize);
        world.entity_mut(control).insert(resize);
        world.entity_mut(control).insert(InteractionDisabled);
        world.flush();
        assert_eq!(cursor(&app, control), Some(not_allowed));

        let plain = app.world_mut().spawn(InteractionDisabled).id();
        app.world_mut().flush();
        assert_eq!(
            cursor(&app, plain),
            None,
            "only entities with a hover cursor"
        );
    }

    #[test]
    fn a_plain_while_disabled_cursor_leaves_the_windows_own() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, HoverCursorPlugin));
        let world = app.world_mut();
        let grab = HoverCursor::new(SystemCursorIcon::Grab).plain_while_disabled();
        let control = world.spawn(grab).id();
        world.flush();
        let grab_icon = EntityCursor::System(SystemCursorIcon::Grab);
        assert_eq!(cursor(&app, control), Some(grab_icon.clone()));

        let world = app.world_mut();
        world.entity_mut(control).insert(InteractionDisabled);
        world.flush();
        assert_eq!(cursor(&app, control), None);

        let world = app.world_mut();
        world.entity_mut(control).remove::<InteractionDisabled>();
        world.flush();
        assert_eq!(cursor(&app, control), Some(grab_icon));
    }
}
