//! A control's frame, its border, lit while it shows the focus. For controls
//! whose parts don't light up on their own, such as a grid of cells, the frame
//! is where the focus shows.

use bevy::ecs::entity::EntityHashSet;
use bevy::prelude::*;

use crate::theme::{FocusVisible, ThemeBorder, ThemeToken, WidgetFlags};

/// A control's tokens, naming the border that frames it.
pub(crate) trait FramedControl: Component {
    /// The frame's colour token.
    fn frame(&self) -> &ThemeToken;
}

/// The control's frame: in [`WidgetState::Focused`](crate::theme::WidgetState)
/// while it shows the focus.
pub(crate) fn frame(tokens: &impl FramedControl, focused: bool) -> ThemeBorder {
    let state = WidgetFlags {
        focused,
        ..default()
    }
    .state();
    ThemeBorder::all(tokens.frame().clone().with_state(state))
}

/// Lights a control's frame when it gains the focus, and puts it back when
/// it loses it. Styling the control picks its frame with [`frame`] too.
pub(crate) fn light_focused_frames<T: FramedControl>(
    gained: Query<Entity, (With<T>, Added<FocusVisible>)>,
    mut lost: RemovedComponents<FocusVisible>,
    controls: Query<(&T, Has<FocusVisible>, &ThemeBorder)>,
    mut commands: Commands,
) {
    let changed: EntityHashSet = gained.iter().chain(lost.read()).collect();
    for entity in changed {
        let Ok((tokens, focused, had)) = controls.get(entity) else {
            continue;
        };
        let wanted = frame(tokens, focused);
        if *had != wanted {
            commands.entity(entity).insert(wanted);
        }
    }
}
