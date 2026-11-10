//! Radial menus: choices around a ring, picked by direction.
//!
//! Letting go in the center either keeps the current choice, or clears it:
//! ([`RadialCenter`]).
//!
//! Two ways to open:
//! - [`SpawnUi::radial_button`](crate::spawn_ui::SpawnUi::radial_button) makes a fixed button
//!   showing the current choice, which opens the ring around where it's pressed.
//! - [`SpawnUi::radial`](crate::spawn_ui::SpawnUi::radial) makes the ring alone, for the game to
//!   open with [`OpenRadial`]: at the cursor, the middle of the screen or a given point.
//!
//! Options are [choices](crate::choice). Each sits in a disc on the ring, built however it likes,
//! and carries its [`ChoiceState`](crate::choice::ChoiceState) for a look that responds to being
//! highlighted; the wedge it sits in lights up behind it. The middle names the highlighted option,
//! or with nothing highlighted, what letting go there leaves. The choice is the radial's
//! [`Chosen`].
//!
//! The opening and choosing are in `input`; the ring's drawing is in `ring`.

mod input;
mod ring;

use std::f32::consts::TAU;

use bevy::prelude::*;
use bevy::ui::{FixedNode, InteractionDisabled, UiSystems};

use crate::EnsurePlugin;
use crate::cancel::{CancelPhase, CancelPlugin};
use crate::choice::{Choice, ChoiceItem, ChoicePlugin, ChoiceSet, ChoiceSystems, Chosen};
use crate::cursor::{HoverCursor, HoverCursorPlugin};
use crate::motion::{ResponseCurve, Tween};
use crate::sanitize::{at_least, within};
use crate::theme::palette::{
    ACCENT, CONTROL, CONTROL_TEXT_SIZE, EDGE, FONT, HIGHLIGHT, SURFACE, TEXT, TEXT_MUTED,
};
use crate::theme::{
    Shade, Theme, ThemeBorder, ThemeFont, ThemeFontSize, ThemePlugin, ThemeSkin, ThemeTextColor,
    ThemeToken, WidgetState,
};
use crate::widgets::button::{ButtonConfig, ButtonTokens, ButtonWidgetPlugin, spawn_button};
use crate::widgets::wedge::bearing_of;

#[doc(inline)]
pub use self::input::{OpenRadial, RadialAt, ReleaseRadial};

/// Prefix of the radial selection widget's theme tokens.
pub const RADIAL_STYLE: &str = "touchy.radial";

/// The ring's outer radius, its dead zone's radius, and an option's disc, in
/// pixels, unless the config says otherwise.
const DEFAULT_RADIUS: f32 = 120.0;
const DEFAULT_DEAD_ZONE: f32 = 44.0;
const DEFAULT_ITEM_SIZE: f32 = 56.0;
/// Space between the middle's disc and the ring, and the disc's edge.
const HUB_GAP: f32 = 4.0;
const HUB_BORDER: f32 = 1.0;
const SEPARATOR_WIDTH: f32 = 1.0;
/// The mark pointing the way: its box, its stroke, and how far past the dead
/// zone it sits.
const MARK_SIZE: f32 = 8.0;
const MARK_STROKE: f32 = 2.0;
const MARK_GAP: f32 = 7.0;
/// How much the highlighted option's disc grows.
const HIGHLIGHT_SCALE: f32 = 1.12;
/// Opening, the ring grows from this scale, easing out. Real time, so a paused
/// game's menus move.
const OPEN_FROM_SCALE: f32 = 0.85;
const OPEN_SECONDS: f32 = 0.12;
const OPEN_CURVE: ResponseCurve = ResponseCurve::Power(0.5);
/// How strongly the highlighted option's wedge, and the chosen one's, are tinted.
const HIGHLIGHT_ALPHA: f32 = 0.35;
const CHOSEN_ALPHA: f32 = 0.3;
/// How faint a disabled option's words are.
const DISABLED_ALPHA: f32 = 0.4;

/// Theme tokens for a radial selection widget. If used with a button, the
/// button's theme tokens are set in [`ButtonTokens`]. Insert new ones to
/// restyle a radial.
#[derive(Component, Clone, PartialEq, Debug)]
#[component(immutable)]
pub struct RadialTokens {
    /// The ring the options sit on.
    pub ring: ThemeToken,
    /// The lines between options.
    pub separator: ThemeToken,
    /// The wedge of the highlighted option.
    pub wedge_highlighted: ThemeToken,
    /// The wedge of the chosen option, under the highlighted one's.
    pub wedge_chosen: ThemeToken,
    /// The middle's disc.
    pub center: ThemeToken,
    /// The middle's disc's edge.
    pub center_border: ThemeToken,
    /// The options' words, and the middle's when it names one; faded while disabled.
    pub text: ThemeToken,
    /// The middle's words when it names nothing, and a radial button's with nothing chosen.
    pub placeholder: ThemeToken,
    /// The mark pointing the way.
    pub mark: ThemeToken,
    /// What covers the screen while it's open: clear unless the app dims it.
    pub scrim: ThemeToken,
    /// The words' font.
    pub font: ThemeToken,
    /// The words' size.
    pub font_size: ThemeToken,
}

impl RadialTokens {
    /// Names the tokens under `prefix`.
    pub fn with_prefix(prefix: &str) -> Self {
        let token = |suffix: &str| ThemeToken::new(format!("{prefix}.{suffix}"));
        Self {
            ring: token("ring"),
            separator: token("separator"),
            wedge_highlighted: token("wedge_highlighted"),
            wedge_chosen: token("wedge_chosen"),
            center: token("center"),
            center_border: token("center_border"),
            text: token("text"),
            placeholder: token("placeholder"),
            mark: token("mark"),
            scrim: token("scrim"),
            font: token("font"),
            font_size: token("font_size"),
        }
    }

    /// Register the default values for these tokens, unless the app sets them.
    pub fn register_theme_defaults(&self, theme: &mut Theme) {
        let disabled_text = self.text.clone().with_state(WidgetState::Disabled);
        theme
            .default_derived(self.ring.clone(), SURFACE, Shade::Same)
            .default_derived(self.separator.clone(), EDGE, Shade::Same)
            .default_derived(
                self.wedge_highlighted.clone(),
                HIGHLIGHT,
                Shade::Alpha(HIGHLIGHT_ALPHA),
            )
            .default_derived(
                self.wedge_chosen.clone(),
                ACCENT,
                Shade::Alpha(CHOSEN_ALPHA),
            )
            .default_derived(self.center.clone(), CONTROL, Shade::Same)
            .default_derived(self.center_border.clone(), EDGE, Shade::Same)
            .default_derived(self.text.clone(), TEXT, Shade::Same)
            .default_derived(disabled_text, TEXT_MUTED, Shade::Alpha(DISABLED_ALPHA))
            .default_derived(self.placeholder.clone(), TEXT_MUTED, Shade::Same)
            .default_derived(self.mark.clone(), HIGHLIGHT, Shade::Same)
            .default_color(self.scrim.clone(), Color::NONE)
            .default_font_from(self.font.clone(), FONT)
            .default_size_from(self.font_size.clone(), CONTROL_TEXT_SIZE);
    }

    /// An option's words: faded while it's disabled.
    fn option_text(&self, disabled: bool) -> ThemeTextColor {
        ThemeTextColor(
            self.text
                .clone()
                .with_state(disabled.then_some(WidgetState::Disabled)),
        )
    }
}

/// The built-in token set.
impl Default for RadialTokens {
    fn default() -> Self {
        Self::with_prefix(RADIAL_STYLE)
    }
}

/// What letting go in the dead zone does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RadialCenter {
    /// Keeps the choice as it was: the middle means "never mind".
    #[default]
    Keep,
    /// Clears the choice: the middle means "none of these".
    Clear,
}

/// How to spawn a radial: its options, its starting choice, and its look.
#[derive(Clone, Debug)]
pub struct RadialConfig {
    /// The options around the ring, the first at the top and the rest clockwise.
    pub options: Vec<Choice>,
    /// The option chosen at spawn, if any.
    pub chosen: Option<usize>,
    /// What letting go in the middle does.
    pub center: RadialCenter,
    /// Named in the middle while nothing's highlighted and letting go there
    /// leaves nothing chosen, and shown on a radial button with nothing chosen.
    pub placeholder: String,
    /// The ring's outer radius, in pixels.
    pub radius: f32,
    /// The dead zone's radius, in pixels: the ring's inner edge.
    pub dead_zone: f32,
    /// The width of an option's disc, in pixels.
    pub item_size: f32,
    /// Whether the radial starts disabled.
    pub disabled: bool,
    /// The ring's theme tokens.
    pub tokens: RadialTokens,
    /// The look of a radial button's button.
    pub button: ButtonTokens,
}

impl RadialConfig {
    /// A radial of `options`, nothing chosen, keeping the choice when let go in
    /// the middle. Plain words make plain options.
    pub fn new<C: Into<Choice>>(options: impl IntoIterator<Item = C>) -> Self {
        Self {
            options: options.into_iter().map(Into::into).collect(),
            chosen: None,
            center: RadialCenter::Keep,
            placeholder: String::new(),
            radius: DEFAULT_RADIUS,
            dead_zone: DEFAULT_DEAD_ZONE,
            item_size: DEFAULT_ITEM_SIZE,
            disabled: false,
            tokens: RadialTokens::with_prefix(RADIAL_STYLE),
            button: ButtonTokens::secondary(),
        }
    }

    /// Starts with the option at `index` chosen.
    pub fn chosen(mut self, index: usize) -> Self {
        self.chosen = Some(index);
        self
    }

    /// Sets what letting go in the middle does.
    pub fn center(mut self, center: RadialCenter) -> Self {
        self.center = center;
        self
    }

    /// Sets what the middle and a radial button say with nothing to name.
    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    /// The ring's outer radius, in pixels.
    pub fn radius(mut self, pixels: f32) -> Self {
        self.radius = pixels;
        self
    }

    /// The dead zone's radius, in pixels, which is the ring's inner edge.
    pub fn dead_zone(mut self, pixels: f32) -> Self {
        self.dead_zone = pixels;
        self
    }

    /// The width of an option's disc, in pixels.
    pub fn item_size(mut self, pixels: f32) -> Self {
        self.item_size = pixels;
        self
    }

    /// Sets whether the radial starts disabled.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Sets the ring's theme tokens.
    pub fn tokens(mut self, tokens: RadialTokens) -> Self {
        self.tokens = tokens;
        self
    }

    /// The look of a radial button's button.
    pub fn button(mut self, tokens: ButtonTokens) -> Self {
        self.button = tokens;
        self
    }

    /// This config with every value made usable, warning about each repair.
    fn sanitized(mut self) -> Self {
        self.radius = at_least(self.radius, 0.0, "RadialConfig radius");
        self.dead_zone = within(self.dead_zone, 0.0, self.radius, "RadialConfig dead zone");
        self.item_size = at_least(self.item_size, 0.0, "RadialConfig item size");
        if let Some(index) = self.chosen
            && index >= self.options.len()
        {
            warn!("RadialConfig chosen was {index}, past its options; choosing none");
            self.chosen = None;
        }
        self
    }
}

/// The parts of a radial, on the radial.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub struct RadialParts {
    /// What covers the screen while it's open, holding the ring.
    pub scrim: Entity,
    /// The ring, placed where it opens and grown as it does.
    pub ring: Entity,
    /// The disc the options sit on.
    pub disc: Entity,
    /// The lit wedges, of the chosen and the highlighted options.
    pub wedges: Entity,
    /// The lines between options.
    pub separators: Vec<Entity>,
    /// The middle's disc.
    pub hub: Entity,
    /// The middle's words.
    pub hub_text: Entity,
    /// The mark pointing the way.
    pub mark: Entity,
    /// The options' discs, one for each option, in order.
    pub items: Vec<RadialItem>,
    /// A radial button's face, showing the choice.
    pub face: Option<Entity>,
}

/// One option's disc on a radial's ring.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RadialItem {
    /// The disc.
    pub item: Entity,
    /// The option's label in it, unless the option builds its own look.
    pub label: Option<Entity>,
}

/// Whether a radial is open, and how.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Phase {
    Closed,
    /// Held open: letting go chooses, unless the pointer never left the middle.
    Held {
        left_center: bool,
    },
    /// Left open: a click, once it's been pressed, chooses.
    Latched {
        armed: bool,
    },
}

/// A radial's state. Its options and their highlight are its [`ChoiceSet`].
#[derive(Component, Clone, Debug)]
struct Radial {
    center_means: RadialCenter,
    placeholder: String,
    radius: f32,
    dead_zone: f32,
    phase: Phase,
    /// Where it was asked to open, or the middle of the screen; and where its
    /// middle is, kept on screen.
    wanted: Option<Vec2>,
    middle: Vec2,
    /// The way it's pointing, clockwise from straight up, or `None` in the
    /// dead zone.
    aim: Option<f32>,
    /// Where the mouse was when last looked at: only moving it aims.
    last_pointer: Option<Vec2>,
    opening: Option<Tween<f32>>,
    /// Whether the ring is showing, and the focus it took on showing.
    shown: bool,
    focus_before: Option<Entity>,
}

impl Radial {
    fn is_open(&self) -> bool {
        self.phase != Phase::Closed
    }
}

/// A point `distance` from the middle of a ring of `radius` the way `bearing`,
/// from the ring's top left corner.
fn on_ring(bearing: f32, distance: f32, radius: f32) -> Vec2 {
    Vec2::new(
        radius + distance * bearing.sin(),
        radius - distance * bearing.cos(),
    )
}

/// Text in the radial's font, coloured by `colour`.
fn words(content: &str, colour: &ThemeToken, tokens: &RadialTokens) -> impl Bundle + use<> {
    (
        Text::new(content),
        ThemeTextColor(colour.clone()),
        ThemeFontSize(tokens.font_size.clone()),
        ThemeFont(tokens.font.clone()),
    )
}

/// A round node `size` across, its contents centred.
fn round(size: f32) -> Node {
    Node {
        position_type: PositionType::Absolute,
        width: px(size),
        height: px(size),
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        border_radius: BorderRadius::all(percent(50)),
        ..default()
    }
}

/// Spawns it; documented on [`SpawnUi::radial_button`](crate::spawn_ui::SpawnUi::radial_button).
pub(crate) fn spawn_radial_button<'a>(
    parent: &'a mut ChildSpawnerCommands,
    config: RadialConfig,
) -> EntityCommands<'a> {
    let button = ButtonConfig::default()
        .tokens(config.button.clone())
        .disabled(config.disabled);
    let mut button = spawn_button(parent, button);
    // As wide as what it shows, like a dropdown, not stretched by its parent.
    button
        .entry::<Node>()
        .and_modify(|mut node| node.align_self = AlignSelf::Start);
    let mut face = Entity::PLACEHOLDER;
    button.with_children(|button| {
        face = button
            .spawn((
                Node {
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                Pickable::IGNORE,
            ))
            .id();
    });
    build(button, config, Some(face))
}

/// Spawns it; documented on [`SpawnUi::radial`](crate::spawn_ui::SpawnUi::radial).
pub(crate) fn spawn_radial<'a>(
    parent: &'a mut ChildSpawnerCommands,
    config: RadialConfig,
) -> EntityCommands<'a> {
    let root = parent.spawn((
        Node {
            position_type: PositionType::Absolute,
            ..default()
        },
        Pickable::IGNORE,
    ));
    build(root, config, None)
}

/// Builds the ring under `root` and makes `root` the radial.
fn build<'a>(
    mut root: EntityCommands<'a>,
    config: RadialConfig,
    face: Option<Entity>,
) -> EntityCommands<'a> {
    let RadialConfig {
        options,
        chosen,
        center,
        placeholder,
        radius,
        dead_zone,
        item_size,
        disabled,
        tokens,
        ..
    } = config.sanitized();
    let (id, count) = (root.id(), options.len());
    let middle_of_band = (radius + dead_zone) / 2.0;
    let band = radius - dead_zone;
    let hub_size = 2.0 * (dead_zone - HUB_GAP).max(0.0);
    let mut parts = RadialParts {
        scrim: Entity::PLACEHOLDER,
        ring: Entity::PLACEHOLDER,
        disc: Entity::PLACEHOLDER,
        wedges: Entity::PLACEHOLDER,
        separators: Vec::new(),
        hub: Entity::PLACEHOLDER,
        hub_text: Entity::PLACEHOLDER,
        mark: Entity::PLACEHOLDER,
        items: Vec::new(),
        face,
    };
    root.with_children(|root| {
        let mut scrim = root.spawn((
            FixedNode,
            Node {
                width: percent(100),
                height: percent(100),
                ..default()
            },
            Visibility::Hidden,
        ));
        // Presses, releases and clicks on an open ring stay there.
        scrim
            .observe(input::keep_presses)
            .observe(input::keep_releases)
            .observe(input::keep_clicks);
        parts.scrim = scrim.id();
        scrim.with_children(|scrim| {
            let mut ring = scrim.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    width: px(2.0 * radius),
                    height: px(2.0 * radius),
                    ..default()
                },
                UiTransform::default(),
                Pickable::IGNORE,
            ));
            parts.ring = ring.id();
            ring.with_children(|ring| {
                // A whole disc, so nothing behind shows between it and the middle.
                parts.disc = ring.spawn((round(2.0 * radius), Pickable::IGNORE)).id();
                // The lit wedges, over the disc and under the middle's disc,
                // so they read as slices of the ring.
                parts.wedges = ring.spawn((round(2.0 * radius), Pickable::IGNORE)).id();
                for index in (0..count).filter(|_| count > 1) {
                    let between = bearing_of(index, count) + TAU / (2 * count) as f32;
                    let at = on_ring(between, middle_of_band, radius);
                    let line = ring.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: px(at.x - SEPARATOR_WIDTH / 2.0),
                            top: px(at.y - band / 2.0),
                            width: px(SEPARATOR_WIDTH),
                            height: px(band),
                            ..default()
                        },
                        UiTransform::from_rotation(Rot2::radians(between)),
                        Pickable::IGNORE,
                    ));
                    parts.separators.push(line.id());
                }
                for (index, option) in options.iter().enumerate() {
                    let at = on_ring(bearing_of(index, count), middle_of_band, radius);
                    let mut item = ring.spawn((
                        Node {
                            left: px(at.x - item_size / 2.0),
                            top: px(at.y - item_size / 2.0),
                            ..round(item_size)
                        },
                        ChoiceItem { set: id, index },
                        UiTransform::default(),
                    ));
                    if !option.is_disabled() {
                        item.insert(HoverCursor::POINTER);
                    }
                    let mut label = None;
                    item.with_children(|item| {
                        label = option.build(item, |label| {
                            (Text::new(label), TextLayout::justify(Justify::Center))
                        });
                    });
                    parts.items.push(RadialItem {
                        item: item.id(),
                        label,
                    });
                }
                let mut hub = ring.spawn((
                    Node {
                        left: px(radius - hub_size / 2.0),
                        top: px(radius - hub_size / 2.0),
                        border: UiRect::all(px(HUB_BORDER)),
                        ..round(hub_size)
                    },
                    Pickable::IGNORE,
                ));
                parts.hub = hub.id();
                hub.with_children(|hub| {
                    let text = (
                        Text::new(""),
                        TextLayout::justify(Justify::Center),
                        Node {
                            max_width: px(hub_size),
                            ..default()
                        },
                        Pickable::IGNORE,
                    );
                    parts.hub_text = hub.spawn(text).id();
                });
                // Two sides of a square, turned so their corner points the way.
                let mark = Node {
                    position_type: PositionType::Absolute,
                    width: px(MARK_SIZE),
                    height: px(MARK_SIZE),
                    border: UiRect {
                        top: px(MARK_STROKE),
                        right: px(MARK_STROKE),
                        ..default()
                    },
                    ..default()
                };
                parts.mark = ring
                    .spawn((
                        mark,
                        UiTransform::default(),
                        Visibility::Hidden,
                        Pickable::IGNORE,
                    ))
                    .id();
            });
        });
    });
    let choices = ChoiceSet::new(options)
        .wrapping(true)
        .clearable(center == RadialCenter::Clear);
    // Its look comes with its tokens, in `style_radial`, once its parts are in.
    root.insert((
        choices,
        Chosen(chosen),
        Radial {
            center_means: center,
            placeholder,
            radius,
            dead_zone,
            phase: Phase::Closed,
            wanted: None,
            middle: Vec2::ZERO,
            aim: None,
            last_pointer: None,
            opening: None,
            shown: false,
            focus_before: None,
        },
        parts,
        tokens,
    ));
    if disabled {
        root.insert(InteractionDisabled);
    }
    root
}

/// Styles a radial when its tokens are set: at spawn, and whenever the app
/// sets new ones. What follows its state (the wedges, the middle's words, the
/// mark's place, a button's face) is drawn in `ring`.
fn style_radial(
    insert: On<Insert<RadialTokens>>,
    radials: Query<(&RadialTokens, &RadialParts, &ChoiceSet)>,
    mut commands: Commands,
) {
    let Ok((tokens, parts, choices)) = radials.get(insert.entity) else {
        return;
    };
    let font = (
        ThemeFontSize(tokens.font_size.clone()),
        ThemeFont(tokens.font.clone()),
    );
    commands
        .entity(parts.scrim)
        .insert(ThemeSkin(tokens.scrim.clone()));
    commands
        .entity(parts.disc)
        .insert(ThemeSkin(tokens.ring.clone()));
    for &line in &parts.separators {
        commands
            .entity(line)
            .insert(ThemeSkin(tokens.separator.clone()));
    }
    commands.entity(parts.hub).insert((
        ThemeSkin(tokens.center.clone()),
        ThemeBorder::all(tokens.center_border.clone()),
    ));
    commands
        .entity(parts.hub_text)
        .insert((ThemeTextColor(tokens.placeholder.clone()), font.clone()));
    commands
        .entity(parts.mark)
        .insert(ThemeBorder::all(tokens.mark.clone()));
    for (index, item) in parts.items.iter().enumerate() {
        if let Some(label) = item.label {
            let disabled = choices.get(index).is_some_and(Choice::is_disabled);
            commands
                .entity(label)
                .insert((tokens.option_text(disabled), font.clone()));
        }
    }
}

/// Radial menus: opening, aiming, choosing, and default token values.
pub struct RadialWidgetPlugin;

impl Plugin for RadialWidgetPlugin {
    fn build(&self, app: &mut App) {
        app.ensure_plugin(ThemePlugin)
            .ensure_plugin(CancelPlugin)
            .ensure_plugin(ChoicePlugin)
            .ensure_plugin(HoverCursorPlugin)
            .ensure_plugin(ButtonWidgetPlugin)
            .add_message::<bevy::picking::pointer::PointerInput>()
            .add_observer(input::open_on_press)
            .add_observer(input::activate)
            .add_observer(input::open_on_request)
            .add_observer(input::release_on_request)
            .add_observer(input::work_with_keys)
            .add_observer(input::close_on_pick)
            .add_observer(style_radial)
            .add_systems(
                PreUpdate,
                input::close_on_cancel.in_set(CancelPhase::Picker),
            )
            .add_systems(
                Update,
                (
                    (input::follow_pointer, input::press_and_release).chain(),
                    ring::grow_rings,
                ),
            )
            .add_systems(
                PostUpdate,
                (
                    (ring::show_opening, ring::place_rings).before(ChoiceSystems),
                    (ring::show_chosen, ring::light_wedges, ring::restyle_rings)
                        .after(ChoiceSystems),
                )
                    .chain()
                    .before(UiSystems::Prepare),
            );
        RadialTokens::with_prefix(RADIAL_STYLE)
            .register_theme_defaults(&mut app.world_mut().resource_mut::<Theme>());
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use bevy::camera::NormalizedRenderTarget;
    use bevy::picking::pointer::{Location, PointerId, PointerLocation};
    use bevy::ui_widgets::ValueChange;

    use super::*;

    /// Where the tests open rings, and how far out to aim.
    pub(crate) const MIDDLE: Vec2 = Vec2::new(300.0, 300.0);
    pub(crate) const OUT: f32 = 90.0;
    pub(crate) const NORTH: Vec2 = Vec2::new(0.0, -OUT);
    pub(crate) const EAST: Vec2 = Vec2::new(OUT, 0.0);
    pub(crate) const SOUTH: Vec2 = Vec2::new(0.0, OUT);

    #[derive(Resource, Default)]
    pub(crate) struct Announced(pub Vec<usize>);

    #[derive(Resource, Default)]
    pub(crate) struct AnnouncedMaybe(pub Vec<Option<usize>>);

    pub(crate) fn location(position: Vec2) -> Location {
        Location {
            target: NormalizedRenderTarget::None {
                width: 1,
                height: 1,
            },
            position,
        }
    }

    pub(crate) fn app() -> App {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::input::InputPlugin,
            bevy::window::WindowPlugin {
                primary_window: None,
                exit_condition: bevy::window::ExitCondition::DontExit,
                ..default()
            },
            RadialWidgetPlugin,
        ))
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
        app.init_resource::<bevy::picking::pointer::PointerMap>();
        app.world_mut()
            .spawn((PointerId::Mouse, PointerLocation::new(location(MIDDLE))));
        app
    }

    pub(crate) fn spawn(app: &mut App, config: RadialConfig, button: bool) -> Entity {
        let mut radial = None;
        app.world_mut()
            .commands()
            .spawn(Node::default())
            .with_children(|parent| {
                radial = Some(if button {
                    spawn_radial_button(parent, config).id()
                } else {
                    spawn_radial(parent, config).id()
                });
            });
        app.update();
        radial.unwrap()
    }

    pub(crate) fn parts(app: &App, radial: Entity) -> RadialParts {
        app.world().get::<RadialParts>(radial).unwrap().clone()
    }

    pub(crate) fn compass() -> RadialConfig {
        RadialConfig::new(["North", "East", "South", "West"])
    }

    #[test]
    fn unusable_configs_are_repaired() {
        let config = RadialConfig {
            radius: -10.0,
            dead_zone: 50.0,
            item_size: f32::NAN,
            chosen: Some(9),
            ..compass()
        }
        .sanitized();
        assert_eq!((config.radius, config.dead_zone), (0.0, 0.0));
        assert_eq!(config.item_size, 0.0);
        assert_eq!(config.chosen, None);
    }

    #[test]
    fn new_tokens_restyle_the_ring_and_its_options() {
        let mut app = app();
        let config = RadialConfig::new([Choice::text("Go"), Choice::text("Stop").disabled(true)]);
        let radial = spawn(&mut app, config, false);
        let tokens = RadialTokens::with_prefix("game.wheel");
        app.world_mut().entity_mut(radial).insert(tokens.clone());
        app.update();
        let parts = parts(&app, radial);
        let world = app.world();
        let fill = |entity| world.get::<ThemeSkin>(entity).unwrap().0.clone();
        assert_eq!(fill(parts.disc), tokens.ring);
        assert_eq!(fill(parts.hub), tokens.center);
        let colour = |item: usize| {
            let label = parts.items[item].label.unwrap();
            world.get::<ThemeTextColor>(label).unwrap().0.clone()
        };
        assert_eq!(colour(0), tokens.text);
        assert_eq!(
            colour(1),
            tokens.text.clone().with_state(WidgetState::Disabled)
        );
    }
}
