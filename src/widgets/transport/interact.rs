//! Turns transport widget activity and keyboard shortcuts into [`TransportRequest`]s.

use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::input_focus::FocusedInput;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, ValueChange};

use super::view::{TimelineTrack, TransportButton};
use super::{TransportCommand, TransportOf, TransportRequest};

/// A transport bar's keyboard shortcuts. None by default, as a widget shouldn't
/// claim keys a game may want; set [`TransportKeys::standard`] for the usual
/// set, or your own. Change it on the bar to rebind.
#[derive(Component, Clone, Debug, Default, PartialEq)]
pub struct TransportKeys(pub Vec<(KeyCode, TransportCommand)>);

impl TransportKeys {
    /// The usual shortcuts: Space plays or pauses, the arrows step, R restarts
    /// and L toggles looping.
    pub fn standard() -> Self {
        Self(vec![
            (KeyCode::Space, TransportCommand::TogglePlay),
            (KeyCode::ArrowLeft, TransportCommand::Step(-1)),
            (KeyCode::ArrowRight, TransportCommand::Step(1)),
            (KeyCode::KeyR, TransportCommand::Restart),
            (KeyCode::KeyL, TransportCommand::ToggleLoop),
        ])
    }
}

/// Registers the observers that send requests. Called by `TransportWidgetPlugin`.
pub(super) fn build(app: &mut App) {
    app.add_observer(request_on_button_activate)
        .add_observer(request_on_track_change)
        .add_observer(request_on_unhandled_key);
}

/// The timeline of the bar `entity` is in, if the bar has one.
fn timeline_of(
    entity: Entity,
    parents: &Query<&ChildOf>,
    bars: &Query<&TransportOf>,
) -> Option<Entity> {
    parents
        .iter_ancestors(entity)
        .find_map(|ancestor| bars.get(ancestor).ok())
        .map(|of| of.0)
}

/// A transport button was clicked or keyboard-activated.
fn request_on_button_activate(
    activate: On<Activate>,
    buttons: Query<&TransportButton>,
    parents: Query<&ChildOf>,
    bars: Query<&TransportOf>,
    mut commands: Commands,
) {
    if let Ok(button) = buttons.get(activate.entity)
        && let Some(timeline) = timeline_of(activate.entity, &parents, &bars)
    {
        commands.trigger(TransportRequest {
            timeline,
            command: button.command,
        });
    }
}

/// The timeline track's slider moved.
fn request_on_track_change(
    change: On<ValueChange<f32>>,
    tracks: Query<(), With<TimelineTrack>>,
    parents: Query<&ChildOf>,
    bars: Query<&TransportOf>,
    mut commands: Commands,
) {
    if tracks.contains(change.source)
        && let Some(timeline) = timeline_of(change.source, &parents, &bars)
    {
        commands.trigger(TransportRequest {
            timeline,
            command: TransportCommand::Seek(change.value),
        });
    }
}

/// Keyboard input bubbles from the focused widget up to the window, and widgets
/// stop the keys they use. Acting only once a key reaches the window means a
/// focused button's Space, or typing in a text field, never triggers a shortcut.
/// Each timeline hears a key once, however many of its bars bind it.
fn request_on_unhandled_key(
    key: On<FocusedInput<KeyboardInput>>,
    windows: Query<(), With<Window>>,
    bars: Query<(&TransportKeys, &TransportOf)>,
    mut commands: Commands,
) {
    let input = &key.input;
    if !windows.contains(key.focused_entity) || input.state != ButtonState::Pressed || input.repeat
    {
        return;
    }
    let mut requests: Vec<TransportRequest> = Vec::new();
    for (keys, &TransportOf(timeline)) in &bars {
        let bound = keys.0.iter().filter(|(code, _)| *code == input.key_code);
        for &(_, command) in bound {
            let request = TransportRequest { timeline, command };
            if !requests.contains(&request) {
                requests.push(request);
            }
        }
    }
    for request in requests {
        commands.trigger(request);
    }
}

#[cfg(test)]
mod tests {
    use bevy::input::ButtonState;
    use bevy::input::keyboard::{Key, KeyboardInput, NativeKey};
    use bevy::input_focus::{FocusCause, InputDispatchPlugin, InputFocus};
    use bevy::prelude::*;
    use bevy::ui_widgets::{Activate, Button, ButtonPlugin};
    use bevy::window::PrimaryWindow;

    use super::super::view::TransportButton;
    use super::super::{TransportCommand, TransportOf, TransportRequest, TransportWidgetPlugin};
    use super::TransportKeys;

    #[derive(Resource, Default)]
    struct Heard(Vec<TransportRequest>);

    fn app_with_window() -> (App, Entity) {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::input::InputPlugin,
            InputDispatchPlugin,
            ButtonPlugin,
            TransportWidgetPlugin,
        ))
        .init_resource::<InputFocus>()
        .init_resource::<Heard>()
        .add_observer(|request: On<TransportRequest>, mut heard: ResMut<Heard>| {
            heard.0.push(*request);
        });
        let window = app
            .world_mut()
            .spawn((Window::default(), PrimaryWindow))
            .id();
        (app, window)
    }

    /// A bar on a new timeline, with `keys`, holding a play button; returns
    /// (timeline, bar, button).
    fn bar(app: &mut App, keys: TransportKeys) -> (Entity, Entity, Entity) {
        let world = app.world_mut();
        let timeline = world.spawn_empty().id();
        let bar = world.spawn((TransportOf(timeline), keys)).id();
        let button = world
            .spawn((
                Button,
                TransportButton::new(TransportCommand::TogglePlay, ">"),
                ChildOf(bar),
            ))
            .id();
        (timeline, bar, button)
    }

    /// Presses `key_code` for one frame and returns the requests sent that frame.
    fn press(app: &mut App, window: Entity, key_code: KeyCode) -> Vec<TransportRequest> {
        app.world_mut().write_message(KeyboardInput {
            key_code,
            logical_key: Key::Unidentified(NativeKey::Unidentified),
            state: ButtonState::Pressed,
            text: None,
            repeat: false,
            window,
        });
        app.update();
        std::mem::take(&mut app.world_mut().resource_mut::<Heard>().0)
    }

    fn request(timeline: Entity, command: TransportCommand) -> TransportRequest {
        TransportRequest { timeline, command }
    }

    #[test]
    fn no_key_is_bound_until_the_app_binds_it() {
        let (mut app, window) = app_with_window();
        bar(&mut app, TransportKeys::default());
        assert!(press(&mut app, window, KeyCode::Space).is_empty());
    }

    #[test]
    fn a_button_asks_for_its_command_on_its_bars_timeline() {
        let (mut app, _) = app_with_window();
        let (timeline, _, button) = bar(&mut app, TransportKeys::default());
        app.world_mut().trigger(Activate { entity: button });
        app.update();
        assert_eq!(
            app.world().resource::<Heard>().0,
            [request(timeline, TransportCommand::TogglePlay)]
        );
    }

    #[test]
    fn shortcut_fires_when_nothing_is_focused() {
        let (mut app, window) = app_with_window();
        let (timeline, ..) = bar(&mut app, TransportKeys::standard());
        assert_eq!(
            press(&mut app, window, KeyCode::Space),
            [request(timeline, TransportCommand::TogglePlay)]
        );
    }

    /// Two bars on one timeline mustn't toggle it twice, which would look like nothing.
    #[test]
    fn a_timeline_hears_a_shortcut_once_however_many_bars_bind_it() {
        let (mut app, window) = app_with_window();
        let (timeline, ..) = bar(&mut app, TransportKeys::standard());
        app.world_mut()
            .spawn((TransportOf(timeline), TransportKeys::standard()));
        let (other, ..) = bar(&mut app, TransportKeys::standard());
        let heard = press(&mut app, window, KeyCode::Space);
        assert_eq!(heard.len(), 2, "once for each timeline: {heard:?}");
        assert!(heard.contains(&request(timeline, TransportCommand::TogglePlay)));
        assert!(heard.contains(&request(other, TransportCommand::TogglePlay)));
    }

    /// A focused button consumes Space itself. If the shortcut also fired,
    /// play would toggle twice and appear to do nothing.
    #[test]
    fn focused_button_takes_space_but_other_shortcuts_still_fire() {
        let (mut app, window) = app_with_window();
        let (timeline, _, button) = bar(&mut app, TransportKeys::standard());
        app.world_mut()
            .resource_mut::<InputFocus>()
            .set(button, FocusCause::Navigated);

        assert_eq!(
            press(&mut app, window, KeyCode::Space),
            [request(timeline, TransportCommand::TogglePlay)]
        );
        assert_eq!(
            press(&mut app, window, KeyCode::ArrowRight),
            [request(timeline, TransportCommand::Step(1))]
        );
    }
}
