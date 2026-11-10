//! The transport bar: spawns its widgets and keeps them in sync with the timeline.
//!
//! What the bar contains (buttons, labels, tooltips) comes from [`TransportConfig`].
//! How it looks comes from the theme: buttons are touchy buttons, and everything
//! else is drawn from the `touchy.transport.*` tokens below.

use bevy::prelude::*;
use bevy::ui::Checked;
use bevy::ui_widgets::{SliderDragState, SliderRange, SliderValue, TrackClick};

use super::{
    Marker, Timeline, TimelineLabels, TransportCommand, TransportKeys, TransportOf,
    TransportSystems,
};
use crate::theme::palette::{FONT, HIGHLIGHT, SMALL_SIZE, TEXT, TEXT_MUTED};
use crate::theme::{Shade, Theme, ThemeFont, ThemeFontSize, ThemeSkin, ThemeTextColor, ThemeToken};
use crate::tooltip::Tooltip;
use crate::widgets::button::{ButtonConfig, ButtonParts, spawn_button};
use crate::widgets::slider::{SliderConfig, SliderMark, SliderTokens, spawn_slider};

/// Prefix of the transport bar's theme tokens.
pub const TRANSPORT_STYLE: &str = "touchy.transport";

/// Space between the bar's three sections, and around its edge.
const BAR_GAP: f32 = 12.0;
const BAR_PADDING: Vec2 = Vec2::new(12.0, 8.0);
const BUTTON_GAP: f32 = 6.0;
/// Height of the track's clickable area.
const TRACK_HEIGHT: f32 = 32.0;
const TRACK_MARGIN: f32 = 8.0;
const THUMB_LENGTH: f32 = 10.0;
const READOUT_MIN_WIDTH: f32 = 180.0;
const READOUT_GAP: f32 = 2.0;
const DEFAULT_CLOCK_FONT_SIZE: f32 = 22.0;

/// Theme tokens for a transport bar. Its buttons are touchy buttons, styled by
/// the button tokens.
#[derive(Component, Clone, PartialEq, Debug)]
#[component(immutable)]
pub struct TransportTokens {
    /// The clock's text.
    pub clock: ThemeToken,
    /// The clock's font size.
    pub clock_font_size: ThemeToken,
    /// The clock's font, so a clock can be monospaced alone.
    pub clock_font: ThemeToken,
    /// The status line's text.
    pub status: ThemeToken,
    /// The status line's font size.
    pub status_font_size: ThemeToken,
    /// The status line's font.
    pub status_font: ThemeToken,
    /// The style of markers that don't name their own.
    pub marker: ThemeToken,
    /// The timeline track's slider tokens.
    pub track: SliderTokens,
}

impl TransportTokens {
    /// Names the tokens under `prefix`: with `touchy.transport`, the clock is
    /// `touchy.transport.clock` and the track `touchy.transport.track.fill`, and so on.
    pub fn with_prefix(prefix: &str) -> Self {
        let token = |suffix: &str| ThemeToken::new(format!("{prefix}.{suffix}"));
        Self {
            clock: token("clock"),
            clock_font_size: token("clock.font_size"),
            clock_font: token("clock.font"),
            status: token("status"),
            status_font_size: token("status.font_size"),
            status_font: token("status.font"),
            marker: token("marker"),
            track: SliderTokens::with_prefix(&format!("{prefix}.track")),
        }
    }

    /// Register the default values for these tokens, unless the app sets them.
    pub fn register_theme_defaults(&self, theme: &mut Theme) {
        theme
            .default_derived(self.clock.clone(), TEXT, Shade::Same)
            .default_derived(self.status.clone(), TEXT_MUTED, Shade::Same)
            .default_size(self.clock_font_size.clone(), DEFAULT_CLOCK_FONT_SIZE)
            .default_size_from(self.status_font_size.clone(), SMALL_SIZE)
            .default_font_from(self.clock_font.clone(), FONT)
            .default_font_from(self.status_font.clone(), FONT)
            .default_derived(self.marker.clone(), HIGHLIGHT, Shade::Same);
        self.track.register_theme_defaults(theme);
    }
}

/// The built-in token set.
impl Default for TransportTokens {
    fn default() -> Self {
        Self::with_prefix(TRANSPORT_STYLE)
    }
}

/// A button on the transport bar, as configured and as stored on its entity.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct TransportButton {
    /// What pressing it asks the host to do.
    pub command: TransportCommand,
    /// Its label.
    pub label: String,
    /// Label while a toggle is on (playing, looping). Ignored for other commands.
    pub on_label: Option<String>,
    /// What its tooltip says, if it has one.
    pub tooltip: Option<String>,
}

impl TransportButton {
    /// A button labelled `label` that sends `command`.
    pub fn new(command: TransportCommand, label: impl Into<String>) -> Self {
        Self {
            command,
            label: label.into(),
            on_label: None,
            tooltip: None,
        }
    }

    /// Sets its label while its toggle is on.
    pub fn on_label(mut self, label: impl Into<String>) -> Self {
        self.on_label = Some(label.into());
        self
    }

    /// Gives it a tooltip.
    pub fn tooltip(mut self, text: impl Into<String>) -> Self {
        self.tooltip = Some(text.into());
        self
    }
}

/// Construction-time configuration for a transport bar: the timeline it
/// controls, and what it contains. The defaults are a minimal set.
///
/// Consumed by [`SpawnUi::transport`](crate::spawn_ui::SpawnUi::transport) and not retained as a
/// component.
///
/// # Examples
/// ```
/// use bevy::prelude::*;
/// use touchy::prelude::*;
///
/// # let timeline = Entity::PLACEHOLDER;
/// let replay = TransportConfig::new(timeline)
///     .keys(TransportKeys::standard())
///     .track_tooltip("Drag to scrub the replay");
/// # assert!(replay.track);
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct TransportConfig {
    /// The timeline the bar shows and controls: an entity with a [`Timeline`].
    pub timeline: Entity,
    /// Buttons, left to right.
    pub buttons: Vec<TransportButton>,
    /// Whether to show the seekable timeline track.
    pub track: bool,
    /// What the track's tooltip says, if it has one.
    pub track_tooltip: Option<String>,
    /// Its keyboard shortcuts; none by default.
    pub keys: TransportKeys,
    /// The bar's look.
    pub tokens: TransportTokens,
}

impl TransportConfig {
    /// The default bar for `timeline`: start, step back, play, step forward and
    /// loop, and the track.
    pub fn new(timeline: Entity) -> Self {
        use TransportCommand::*;
        Self {
            timeline,
            buttons: vec![
                TransportButton::new(JumpToStart, "|<").tooltip("Jump to start"),
                TransportButton::new(Step(-1), "<").tooltip("Step back"),
                TransportButton::new(TogglePlay, ">")
                    .on_label("||")
                    .tooltip("Play / pause"),
                TransportButton::new(Step(1), ">").tooltip("Step forward"),
                TransportButton::new(ToggleLoop, "ONCE")
                    .on_label("LOOP")
                    .tooltip("Toggle loop"),
            ],
            track: true,
            track_tooltip: None,
            keys: TransportKeys::default(),
            tokens: TransportTokens::with_prefix(TRANSPORT_STYLE),
        }
    }

    /// Sets the buttons, left to right.
    pub fn buttons(mut self, buttons: impl IntoIterator<Item = TransportButton>) -> Self {
        self.buttons = buttons.into_iter().collect();
        self
    }

    /// Sets whether to show the seekable timeline track.
    pub fn track(mut self, track: bool) -> Self {
        self.track = track;
        self
    }

    /// Gives the track a tooltip.
    pub fn track_tooltip(mut self, text: impl Into<String>) -> Self {
        self.track_tooltip = Some(text.into());
        self
    }

    /// Sets the bar's keyboard shortcuts.
    pub fn keys(mut self, keys: TransportKeys) -> Self {
        self.keys = keys;
        self
    }

    /// Sets the bar's theme tokens.
    pub fn tokens(mut self, tokens: TransportTokens) -> Self {
        self.tokens = tokens;
        self
    }
}

/// The seekable timeline track (a headless [`Slider`](bevy::ui_widgets::Slider)).
#[derive(Component, Clone, Copy, Debug)]
pub struct TimelineTrack;

/// The markers a track is currently showing, to tell when they need redrawing.
#[derive(Component, Clone, Debug, Default)]
struct ShownMarkers(Vec<Marker>);

/// A bar's parts its tokens style and its systems fill.
#[derive(Component, Clone, Debug)]
pub(super) struct TransportParts {
    buttons: Vec<Entity>,
    clock: Entity,
    status: Entity,
    track: Option<Entity>,
}

/// Registers the bar's systems and default token values. Called by `TransportWidgetPlugin`.
pub(super) fn build(app: &mut App) {
    app.add_observer(style_transport).add_systems(
        Update,
        (
            sync_labels,
            sync_toggle_buttons,
            sync_button_hotkeys,
            sync_track,
            sync_markers,
        )
            .in_set(TransportSystems::View),
    );
    TransportTokens::with_prefix(TRANSPORT_STYLE)
        .register_theme_defaults(&mut app.world_mut().resource_mut::<Theme>());
}

/// Spawns it; documented on [`SpawnUi::transport`](crate::spawn_ui::SpawnUi::transport).
pub(crate) fn spawn_transport<'a>(
    parent: &'a mut ChildSpawnerCommands,
    config: TransportConfig,
) -> EntityCommands<'a> {
    let TransportConfig {
        timeline,
        buttons,
        track,
        track_tooltip,
        keys,
        tokens,
    } = config;
    let mut bar = parent.spawn((
        TransportOf(timeline),
        keys,
        Node {
            width: percent(100),
            // Fills available height.
            align_self: AlignSelf::Stretch,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::SpaceBetween,
            padding: UiRect::axes(px(BAR_PADDING.x), px(BAR_PADDING.y)),
            column_gap: px(BAR_GAP),
            ..default()
        },
    ));
    let mut button_ids = Vec::with_capacity(buttons.len());
    let (mut clock, mut status, mut timeline_track) =
        (Entity::PLACEHOLDER, Entity::PLACEHOLDER, None);
    bar.with_children(|bar| {
        bar.spawn(Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(BUTTON_GAP),
            ..default()
        })
        .with_children(|group| {
            for button in buttons {
                button_ids.push(spawn_transport_button(group, button));
            }
        });

        if track {
            timeline_track = Some(spawn_track(bar, track_tooltip, tokens.track.clone()));
        } else {
            bar.spawn(Node {
                flex_grow: 1.0,
                ..default()
            });
        }

        bar.spawn(Node {
            min_width: px(READOUT_MIN_WIDTH),
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::End,
            row_gap: px(READOUT_GAP),
            ..default()
        })
        .with_children(|readout| {
            // Their colours and fonts come with the bar's tokens, in `style_transport`.
            clock = readout.spawn(Text::default()).id();
            status = readout.spawn(Text::default()).id();
        });
    });
    bar.insert((
        TransportParts {
            buttons: button_ids,
            clock,
            status,
            track: timeline_track,
        },
        tokens,
    ));
    bar
}

/// A touchy button that also carries its transport command.
fn spawn_transport_button(parent: &mut ChildSpawnerCommands, button: TransportButton) -> Entity {
    let mut config = ButtonConfig::new(button.label.clone());
    if let Some(text) = &button.tooltip {
        config = config.tooltip(Tooltip::new(text.clone()));
    }
    spawn_button(parent, config).insert(button).id()
}

/// Spawns the timeline track and returns the slider.
fn spawn_track(
    parent: &mut ChildSpawnerCommands,
    tooltip: Option<String>,
    tokens: SliderTokens,
) -> Entity {
    let mut slider = Entity::PLACEHOLDER;
    // The container takes the bar's spare width; the slider fills it.
    parent
        .spawn(Node {
            flex_grow: 1.0,
            margin: UiRect::horizontal(px(TRACK_MARGIN)),
            ..default()
        })
        .with_children(|container| {
            let config = SliderConfig::new(0.0)
                .range(0.0..=0.0)
                .tokens(tokens)
                .track_click(TrackClick::Snap)
                .thickness(TRACK_HEIGHT)
                .thumb_length(THUMB_LENGTH);
            let mut track = spawn_slider(container, config);
            track.insert((TimelineTrack, ShownMarkers::default()));
            if let Some(text) = tooltip {
                track.insert(Tooltip::new(text));
            }
            slider = track.id();
        });
    slider
}

/// Styles a bar's readout, and its track, when its tokens are set.
fn style_transport(
    insert: On<Insert<TransportTokens>>,
    bars: Query<(&TransportParts, &TransportTokens)>,
    mut commands: Commands,
) {
    let Ok((parts, tokens)) = bars.get(insert.entity) else {
        return;
    };
    commands.entity(parts.clock).insert((
        ThemeTextColor(tokens.clock.clone()),
        ThemeFontSize(tokens.clock_font_size.clone()),
        ThemeFont(tokens.clock_font.clone()),
    ));
    commands.entity(parts.status).insert((
        ThemeTextColor(tokens.status.clone()),
        ThemeFontSize(tokens.status_font_size.clone()),
        ThemeFont(tokens.status_font.clone()),
    ));
    if let Some(track) = parts.track {
        commands.entity(track).insert(tokens.track.clone());
    }
}

/// A bar, as the systems that keep it in step with its timeline read it.
type Bar = (Ref<'static, TransportParts>, Ref<'static, TransportOf>);

/// Whether a bar must catch up with its timeline: it's new, it was pointed at
/// another timeline, or the timeline itself changed.
fn behind<T: Component>(
    (parts, of): &(Ref<TransportParts>, Ref<TransportOf>),
    timeline: &Ref<T>,
) -> bool {
    parts.is_added() || of.is_changed() || timeline.is_changed()
}

/// Writes each bar's timeline labels into its readout.
fn sync_labels(
    bars: Query<Bar>,
    timelines: Query<Ref<TimelineLabels>>,
    mut texts: Query<&mut Text>,
) {
    for bar in &bars {
        let Ok(labels) = timelines.get(bar.1.0) else {
            continue;
        };
        if !behind(&bar, &labels) {
            continue;
        }
        for (part, wanted) in [(bar.0.clock, &labels.clock), (bar.0.status, &labels.status)] {
            if let Ok(mut text) = texts.get_mut(part)
                && text.0 != *wanted
            {
                text.0.clone_from(wanted);
            }
        }
    }
}

/// Marks toggle buttons [`Checked`] while their state is on, and swaps their label.
/// The button widget restyles itself when `Checked` changes.
fn sync_toggle_buttons(
    bars: Query<Bar>,
    timelines: Query<Ref<Timeline>>,
    buttons: Query<(&TransportButton, &ButtonParts, Has<Checked>)>,
    mut labels: Query<&mut Text>,
    mut commands: Commands,
) {
    for bar in &bars {
        let Ok(timeline) = timelines.get(bar.1.0) else {
            continue;
        };
        if !behind(&bar, &timeline) {
            continue;
        }
        for &entity in &bar.0.buttons {
            let Ok((button, parts, checked)) = buttons.get(entity) else {
                continue;
            };
            let on = match button.command {
                TransportCommand::TogglePlay => timeline.playing,
                TransportCommand::ToggleLoop => timeline.looping,
                _ => continue,
            };
            if on && !checked {
                commands.entity(entity).insert(Checked);
            } else if !on && checked {
                commands.entity(entity).remove::<Checked>();
            }
            let label = match (&button.on_label, on) {
                (Some(on_label), true) => on_label,
                _ => &button.label,
            };
            if let Some(Ok(mut text)) = parts.label.map(|part| labels.get_mut(part))
                && text.0 != *label
            {
                text.0.clone_from(label);
            }
        }
    }
}

/// Shows each button's shortcut in its tooltip, read from its bar's
/// [`TransportKeys`], so the hint always matches the actual binding.
fn sync_button_hotkeys(
    bars: Query<(&TransportParts, Ref<TransportKeys>)>,
    mut buttons: Query<(&TransportButton, &mut Tooltip)>,
) {
    for (parts, keys) in &bars {
        if !keys.is_changed() {
            continue;
        }
        for &entity in &parts.buttons {
            let Ok((button, mut tooltip)) = buttons.get_mut(entity) else {
                continue;
            };
            let hotkey = keys
                .0
                .iter()
                .find(|(_, command)| *command == button.command)
                .map(|&(key, _)| key_label(key));
            if tooltip.hotkey != hotkey {
                tooltip.hotkey = hotkey;
            }
        }
    }
}

/// A short, readable name for a key, for hotkey badges: `KeyR` becomes `"R"`,
/// `ArrowLeft` becomes `"Left"`.
fn key_label(key: KeyCode) -> String {
    let name = format!("{key:?}");
    ["Key", "Digit", "Arrow"]
        .iter()
        .find_map(|prefix| name.strip_prefix(prefix))
        .map_or_else(|| name.clone(), str::to_string)
}

/// Moves each track to its timeline's position, except while the user is dragging it.
fn sync_track(
    bars: Query<Bar>,
    timelines: Query<Ref<Timeline>>,
    tracks: Query<(&SliderDragState, &SliderValue, &SliderRange)>,
    mut commands: Commands,
) {
    for bar in &bars {
        let (Some(track), Ok(timeline)) = (bar.0.track, timelines.get(bar.1.0)) else {
            continue;
        };
        let Ok((drag, value, range)) = tracks.get(track) else {
            continue;
        };
        if drag.dragging || !behind(&bar, &timeline) {
            continue;
        }
        let new_range = SliderRange::new(0.0, timeline.duration.max(0.0));
        if *range != new_range {
            commands.entity(track).insert(new_range);
        }
        if value.0 != timeline.position {
            commands
                .entity(track)
                .insert(SliderValue(timeline.position));
        }
    }
}

/// Redraws a track's markers when its timeline's list changes. Markers change
/// rarely, so they are simply despawned and respawned.
fn sync_markers(
    bars: Query<(&TransportParts, &TransportOf, &TransportTokens)>,
    timelines: Query<&Timeline>,
    mut tracks: Query<(&mut ShownMarkers, Option<&Children>)>,
    existing: Query<(), With<SliderMark>>,
    mut commands: Commands,
) {
    for (parts, &TransportOf(timeline), tokens) in &bars {
        let (Some(track), Ok(timeline)) = (parts.track, timelines.get(timeline)) else {
            continue;
        };
        let Ok((mut shown, children)) = tracks.get_mut(track) else {
            continue;
        };
        if shown.0 == timeline.markers {
            continue;
        }
        for child in children
            .into_iter()
            .flatten()
            .filter(|&child| existing.contains(*child))
        {
            commands.entity(*child).despawn();
        }
        commands.entity(track).with_children(|track| {
            for marker in &timeline.markers {
                let style = marker
                    .style
                    .clone()
                    .unwrap_or_else(|| tokens.marker.clone());
                let mut node = track.spawn((SliderMark::at(marker.time), ThemeSkin(style)));
                if !marker.label.is_empty() {
                    node.insert(Tooltip::new(marker.label.clone()));
                }
            }
        });
        shown.0.clone_from(&timeline.markers);
    }
}

#[cfg(test)]
mod tests {
    use bevy::ui_widgets::{SliderPlugin, SliderThumb};

    use super::*;
    use crate::theme::ThemePlugin;
    use crate::widgets::button::ButtonWidgetPlugin;
    use crate::widgets::slider::SliderWidgetPlugin;
    use crate::widgets::transport::{TransportWidgetPlugin, Transports};

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            SliderPlugin,
            ThemePlugin,
            ButtonWidgetPlugin,
            SliderWidgetPlugin,
            TransportWidgetPlugin,
        ));
        app.update();
        app
    }

    fn timeline(app: &mut App, timeline: Timeline, labels: TimelineLabels) -> Entity {
        app.world_mut().spawn((timeline, labels)).id()
    }

    fn spawn_bar(app: &mut App, config: TransportConfig) -> Entity {
        let mut bar = None;
        app.world_mut()
            .commands()
            .spawn(Node::default())
            .with_children(|root| bar = Some(spawn_transport(root, config).id()));
        app.update();
        app.update();
        bar.unwrap()
    }

    fn parts(app: &App, bar: Entity) -> TransportParts {
        app.world().get::<TransportParts>(bar).unwrap().clone()
    }

    fn play_button(app: &App, bar: Entity) -> Entity {
        parts(app, bar)
            .buttons
            .into_iter()
            .find(|&button| {
                app.world().get::<TransportButton>(button).unwrap().command
                    == TransportCommand::TogglePlay
            })
            .unwrap()
    }

    #[test]
    fn key_labels_are_short() {
        assert_eq!(key_label(KeyCode::KeyR), "R");
        assert_eq!(key_label(KeyCode::Digit3), "3");
        assert_eq!(key_label(KeyCode::ArrowLeft), "Left");
        assert_eq!(key_label(KeyCode::Space), "Space");
    }

    /// A bar spawned mid-session (say, on a mode switch) must show the current
    /// state at once, not wait for the next change to the timeline or labels.
    #[test]
    fn bar_spawned_late_shows_current_state() {
        let mut app = app();
        let replay = timeline(
            &mut app,
            Timeline {
                duration: 100.0,
                position: 25.0,
                playing: true,
                ..default()
            },
            TimelineLabels {
                clock: "TICK 025 / 100".into(),
                status: String::new(),
            },
        );
        app.update();
        let bar = spawn_bar(&mut app, TransportConfig::new(replay));

        let parts = parts(&app, bar);
        let play = play_button(&app, bar);
        let world = app.world_mut();
        assert_eq!(world.get::<Text>(parts.clock).unwrap().0, "TICK 025 / 100");
        assert!(
            world.get::<Checked>(play).is_some(),
            "checked while playing"
        );
        let thumb = world
            .query_filtered::<&Node, With<SliderThumb>>()
            .single(world)
            .unwrap();
        assert_eq!(thumb.left, percent(25.0));
    }

    #[test]
    fn two_bars_follow_their_own_timelines_and_one_can_be_pointed_elsewhere() {
        let mut app = app();
        let playing = Timeline {
            playing: true,
            ..default()
        };
        let first = timeline(&mut app, playing, TimelineLabels::default());
        let second = timeline(&mut app, Timeline::default(), TimelineLabels::default());
        let on_first = spawn_bar(&mut app, TransportConfig::new(first));
        let on_second = spawn_bar(&mut app, TransportConfig::new(second));
        let checked = |app: &App, bar| app.world().get::<Checked>(play_button(app, bar)).is_some();
        assert!(checked(&app, on_first) && !checked(&app, on_second));

        app.world_mut()
            .entity_mut(on_second)
            .insert(TransportOf(first));
        app.update();
        assert!(checked(&app, on_second), "pointed at the playing timeline");
        let shown_by = app.world().get::<Transports>(first).unwrap();
        assert_eq!(shown_by.iter().count(), 2);
    }

    #[test]
    fn tooltips_show_the_bound_key_and_follow_rebinding() {
        let mut app = app();
        let replay = timeline(&mut app, Timeline::default(), TimelineLabels::default());
        let bar = spawn_bar(&mut app, TransportConfig::new(replay));
        let play_hotkey = |app: &App| {
            app.world()
                .get::<Tooltip>(play_button(app, bar))
                .and_then(|tooltip| tooltip.hotkey.clone())
        };
        assert_eq!(play_hotkey(&app), None, "no keys until the app binds them");

        app.world_mut()
            .entity_mut(bar)
            .insert(TransportKeys::standard());
        app.update();
        assert_eq!(play_hotkey(&app).as_deref(), Some("Space"));
        app.world_mut().entity_mut(bar).insert(TransportKeys(vec![(
            KeyCode::KeyP,
            TransportCommand::TogglePlay,
        )]));
        app.update();
        assert_eq!(play_hotkey(&app).as_deref(), Some("P"));
    }

    #[test]
    fn markers_follow_the_timeline_and_take_their_style_or_the_bars() {
        let goal = ThemeToken::new_static("test.marker.goal");
        let mut app = app();
        let replay = timeline(
            &mut app,
            Timeline {
                duration: 200.0,
                markers: vec![Marker::new(50.0, "Goal").style(goal.clone())],
                ..default()
            },
            TimelineLabels::default(),
        );
        spawn_bar(&mut app, TransportConfig::new(replay));

        let world = app.world_mut();
        let (node, style) = world
            .query_filtered::<(&Node, &ThemeSkin), With<SliderMark>>()
            .single(world)
            .unwrap();
        assert_eq!(style.0, goal);
        assert_eq!(node.left, percent(25.0));

        app.world_mut().get_mut::<Timeline>(replay).unwrap().markers =
            vec![Marker::new(20.0, ""), Marker::new(180.0, "")];
        app.update();
        let world = app.world_mut();
        let styles: Vec<ThemeToken> = world
            .query_filtered::<&ThemeSkin, With<SliderMark>>()
            .iter(world)
            .map(|style| style.0.clone())
            .collect();
        assert_eq!(
            styles.len(),
            2,
            "old markers should be replaced, not added to"
        );
        let bar_marker = TransportTokens::with_prefix(TRANSPORT_STYLE).marker;
        assert!(styles.iter().all(|style| *style == bar_marker));
    }
}
