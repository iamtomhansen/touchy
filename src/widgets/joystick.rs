//! A joystick: drag the thumb within a circular base to steer in two dimensions.
//!
//! The output is a vector in the unit disc, with +y up. Readings pass through
//! [`read_stick`]: clamp to the disc, apply a dead zone with hysteresis,
//! rescale, then a [`ResponseCurve`]. On release the output returns to zero at
//! once and the thumb glides home.
//!
//! How far the hand moves for full output, the stick's
//! [`reach`](JoystickSettings::reach), is its own setting, apart from how far
//! the thumb travels on screen: a small joystick, as on a phone, can still
//! take a long, fine-grained throw. The thumb moves in proportion, reaching
//! the rim as the output reaches its most.
//!
//! A [`throttle`](JoystickConfig::throttle) moves along one axis only, on a
//! track rather than a disc. A [`floating`](JoystickConfig::floating) stick
//! waits, hidden, in an area and appears wherever the area is pressed, with
//! its thumb under the pointer. A right-click sends a stick that's been let go
//! back to the centre, for one that stays where it's left.

use bevy::ecs::entity::EntityHashSet;
use bevy::picking::events::{
    PointerDrag, PointerDragEnd, PointerDragStart, PointerPress, PointerRelease,
};
use bevy::picking::hover::Hovered;
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use bevy::ui::UiSystems;
use bevy::ui_widgets::ValueChange;
use bevy::window::SystemCursorIcon;

use crate::EnsurePlugin;
use crate::cursor::{HoverCursor, HoverCursorPlugin};
use crate::motion::{Glide, Latch, ResponseCurve};
use crate::sanitize::{at_least, within};
use crate::theme::palette::{CONTROL, EDGE, HIGHLIGHT, SURFACE, TEXT_MUTED};
use crate::theme::skins;
use crate::theme::{
    Shade, Theme, ThemeBorder, ThemePlugin, ThemeSkin, ThemeToken, WidgetFlags, WidgetState,
};
use crate::widgets::floating::{self, Floating, Placement};

/// Prefix of the joystick widget's theme tokens.
pub const JOYSTICK_STYLE: &str = "touchy.joystick";

/// How far the thumb's centre can travel from the base's centre, in pixels.
const DEFAULT_RADIUS: f32 = 48.0;
const DEFAULT_THUMB_SIZE: f32 = 32.0;
/// The least a joystick's radius, reach and thumb can be, in pixels.
const MIN_SIZE: f32 = 1.0;
/// Fraction of the reach that counts as centred, so a resting hand reads as zero.
const DEFAULT_DEAD_ZONE: f32 = 0.15;
/// Largest allowed dead zone; beyond this almost nothing would register.
const MAX_DEAD_ZONE: f32 = 0.9;
/// Once outside the dead zone, the stick stays engaged until it falls below
/// this fraction of the dead zone, so tremor at the edge can't flicker it.
const DEAD_ZONE_EXIT: f32 = 0.8;
/// Gentle near the centre for fine control, full speed at the edge.
const DEFAULT_CURVE: ResponseCurve = ResponseCurve::Power(1.5);
/// Constant acceleration of the thumb's return home, in radii per second squared.
const RETURN_ACCELERATION: f32 = 40.0;
const BASE_BORDER: f32 = 2.0;
const THUMB_BORDER: f32 = 2.0;
const DEAD_ZONE_BORDER: f32 = 1.0;
/// The dead-zone ring: the palette's muted text colour, half see-through.
const DEAD_ZONE_RING: Shade = Shade::Alpha(0.5);

/// Theme tokens for a joystick widget. Insert new ones to restyle a joystick.
#[derive(Component, Clone, PartialEq, Debug)]
#[component(immutable)]
pub struct JoystickTokens {
    /// The base's fill.
    pub bg: ThemeToken,
    /// The base's border.
    pub border: ThemeToken,
    /// The ring marking the dead zone.
    pub dead_zone: ThemeToken,
    /// The thumb's fill.
    pub thumb: ThemeToken,
    /// The thumb's border, lit while the joystick is hovered or held.
    pub thumb_border: ThemeToken,
}

impl JoystickTokens {
    /// Names the tokens under `prefix`.
    pub fn with_prefix(prefix: &str) -> Self {
        let token = |suffix: &str| ThemeToken::new(format!("{prefix}.{suffix}"));
        Self {
            bg: token("bg"),
            border: token("border"),
            dead_zone: token("dead_zone"),
            thumb: token("thumb"),
            thumb_border: token("thumb.border"),
        }
    }

    /// Derives these tokens from the palette, unless the app sets them.
    pub fn register_theme_defaults(&self, theme: &mut Theme) {
        let lit_border = |state| self.thumb_border.clone().with_state(state);
        theme
            .default_derived(self.bg.clone(), SURFACE, Shade::Same)
            .default_derived(self.border.clone(), EDGE, Shade::Same)
            .default_derived(self.dead_zone.clone(), TEXT_MUTED, DEAD_ZONE_RING)
            .default_derived(self.thumb.clone(), CONTROL, Shade::Same)
            .default_derived(self.thumb_border.clone(), TEXT_MUTED, Shade::Same)
            .default_derived(lit_border(WidgetState::Hover), HIGHLIGHT, Shade::Same)
            .default_derived(lit_border(WidgetState::Pressed), HIGHLIGHT, Shade::Same);
        // Each part draws its role's skin, once a theme gives the role one. Not the round
        // base, whose roundness a role's square picture would spoil.
        theme.default_skin_from(self.thumb.clone(), skins::KNOB);
    }
}

/// The built-in token set.
impl Default for JoystickTokens {
    fn default() -> Self {
        Self::with_prefix(JOYSTICK_STYLE)
    }
}

/// One reading of the stick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StickReading {
    /// Where to draw the thumb, as a fraction of its travel in screen axes
    /// (+y down). Always within the unit disc.
    pub thumb: Vec2,
    /// The value to report: in the unit disc, +y up, zero inside the dead zone.
    pub output: Vec2,
    /// Whether the stick is outside the dead zone.
    pub engaged: bool,
}

impl StickReading {
    const CENTRED: Self = Self {
        thumb: Vec2::ZERO,
        output: Vec2::ZERO,
        engaged: false,
    };
}

/// Turns a hand's offset from the centre, in UI pixels, into a reading, for a
/// stick that reaches full output `reach` pixels out. The dead zone's edge uses
/// a [`Latch`] for hysteresis to avoid flickering.
pub fn read_stick(
    offset: Vec2,
    reach: f32,
    dead_zone: f32,
    curve: ResponseCurve,
    was_engaged: bool,
) -> StickReading {
    if !offset.is_finite() || reach <= 0.0 {
        return StickReading::CENTRED;
    }
    let dead_zone = dead_zone.clamp(0.0, MAX_DEAD_ZONE);
    let scaled = offset / reach;
    let length = scaled.length();
    let direction = scaled.normalize_or_zero();
    let extent = length.min(1.0);

    let engaged = Latch::new(dead_zone, dead_zone * DEAD_ZONE_EXIT).update(was_engaged, length);
    let magnitude = if engaged {
        curve.apply(((extent - dead_zone) / (1.0 - dead_zone)).clamp(0.0, 1.0))
    } else {
        0.0
    };
    StickReading {
        thumb: direction * extent,
        output: Vec2::new(direction.x, -direction.y) * magnitude,
        engaged,
    }
}

/// Which ways a joystick's thumb moves.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum JoystickAxes {
    /// Anywhere in a disc.
    #[default]
    Both,
    /// Left and right, along a track: a throttle lying down.
    Horizontal,
    /// Up and down, along a track: a throttle.
    Vertical,
}

impl JoystickAxes {
    /// `offset` with the axis this stick doesn't move along taken out.
    fn constrain(self, offset: Vec2) -> Vec2 {
        match self {
            Self::Both => offset,
            Self::Horizontal => offset.with_y(0.0),
            Self::Vertical => offset.with_x(0.0),
        }
    }

    /// Swaps `along` and `across` into screen axes for this stick's way.
    fn oriented(self, along: f32, across: f32) -> Vec2 {
        match self {
            Self::Both | Self::Horizontal => Vec2::new(along, across),
            Self::Vertical => Vec2::new(across, along),
        }
    }

    /// The base's size: a disc the thumb can cross every way, or a track as
    /// long as that and as deep as the thumb, inside the border.
    fn base_size(self, radius: f32, thumb_size: f32) -> Vec2 {
        let long = 2.0 * radius + thumb_size;
        match self {
            Self::Both => Vec2::splat(long),
            Self::Horizontal | Self::Vertical => {
                self.oriented(long, thumb_size + 2.0 * BASE_BORDER)
            }
        }
    }

    /// The dead zone's outline, `diameter` across: a circle, or on a track no
    /// deeper than the thumb.
    fn dead_zone_size(self, diameter: f32, thumb_size: f32) -> Vec2 {
        match self {
            Self::Both => Vec2::splat(diameter),
            Self::Horizontal | Self::Vertical => self.oriented(diameter, diameter.min(thumb_size)),
        }
    }
}

/// How a joystick responds. Change it to change how the joystick feels.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct JoystickSettings {
    /// How far the hand moves from the centre for full output, in UI pixels.
    pub reach: f32,
    /// The radius of the dead zone, as a fraction of the reach, up to 0.9.
    pub dead_zone: f32,
    /// How the output grows from the dead zone to full.
    pub curve: ResponseCurve,
    /// Whether the thumb springs home on release. Off for a stick that stays
    /// where it's left, like a throttle.
    pub returns: bool,
}

/// The joystick's current output: in the unit disc, +y up.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct JoystickValue(pub Vec2);

/// How the thumb travels on screen, which ways, and how big it is, as drawn.
#[derive(Component, Clone, Copy, Debug)]
struct Travel {
    radius: f32,
    thumb_size: f32,
    axes: JoystickAxes,
}

/// Where the hand has the stick, in UI pixels from the centre, +y down, and
/// whether it's outside its dead zone. The thumb is drawn in proportion.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
struct Stick {
    hand: Vec2,
    engaged: bool,
}

/// A drag in progress, starting from where the stick was when it was grabbed.
#[derive(Component, Clone, Copy, Debug)]
struct Gripped {
    origin: Vec2,
}

/// The stick gliding home after release: only the thumb's way back, as the
/// output is zero from the moment it's let go.
#[derive(Component, Clone, Copy, Debug)]
struct Returning(Glide<Vec2>);

/// The parts of a joystick, on the joystick.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct JoystickParts {
    /// The base the thumb moves over: the joystick itself, or for a floating
    /// one, the part that appears where its area is pressed.
    pub base: Entity,
    /// The ring marking the dead zone.
    pub dead_zone: Entity,
    /// The thumb.
    pub thumb: Entity,
}

/// Spawn time configuration for a joystick widget.
///
/// Consumed by [`SpawnUi::joystick`](crate::spawn_ui::SpawnUi::joystick), which returns the
/// joystick's entity.
///
/// ```
/// use touchy::prelude::*;
///
/// // Small on screen, with a long throw for fine steering.
/// let config = JoystickConfig::new()
///     .radius(32.0)
///     .reach(96.0)
///     .thumb_size(28.0)
///     .dead_zone(0.2)
///     .curve(ResponseCurve::Power(2.0));
/// # assert_eq!(config.reach, Some(96.0));
///
/// // A phone game's stick: anywhere in its area, springing home.
/// let thumbstick = JoystickConfig::new().floating(true);
///
/// // A throttle that springs back to idle when it's let go.
/// let throttle = JoystickConfig::throttle().returns(true);
/// ```
#[derive(Clone, Debug)]
pub struct JoystickConfig {
    /// How far the thumb's centre travels from the base's, in pixels.
    pub radius: f32,
    /// How far the hand moves for full output, in pixels, or the radius if `None`.
    pub reach: Option<f32>,
    /// The thumb's width and height, in pixels.
    pub thumb_size: f32,
    /// The radius of the dead zone, as a fraction of the reach.
    pub dead_zone: f32,
    /// How the output grows from the dead zone to full.
    pub curve: ResponseCurve,
    /// Whether the thumb springs home on release.
    pub returns: bool,
    /// Where a stick with `returns=false`` starts, as a fraction of its reach.
    pub start: Vec2,
    /// Which ways the thumb moves.
    pub axes: JoystickAxes,
    /// Hide until pressed, then appear where the press lands. The joystick
    /// fills its parent, which sets the area that takes presses.
    pub floating: bool,
    /// Theme tokens for the joystick.
    pub tokens: JoystickTokens,
}

impl JoystickConfig {
    /// Creates a config with default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// A throttle: up and down only, staying where it's left.
    pub fn throttle() -> Self {
        Self {
            axes: JoystickAxes::Vertical,
            returns: false,
            ..default()
        }
    }

    /// Sets where a stick that stays where it's left starts, as a fraction of its reach, +y up.
    pub fn start(mut self, start: Vec2) -> Self {
        self.start = start;
        self
    }

    /// Sets how far the thumb travels from the centre, in pixels.
    pub fn radius(mut self, radius: f32) -> Self {
        self.radius = radius;
        self
    }

    /// Sets how far the hand moves for full output, in pixels.
    pub fn reach(mut self, reach: f32) -> Self {
        self.reach = Some(reach);
        self
    }

    /// Sets the size of the thumb, in pixels.
    pub fn thumb_size(mut self, size: f32) -> Self {
        self.thumb_size = size;
        self
    }

    /// Sets the radius of the dead zone, as a fraction of the reach.
    pub fn dead_zone(mut self, fraction: f32) -> Self {
        self.dead_zone = fraction;
        self
    }

    /// Sets how the output grows from the dead zone to full.
    pub fn curve(mut self, curve: ResponseCurve) -> Self {
        self.curve = curve;
        self
    }

    /// Sets whether the joystick returns to the center on release.
    pub fn returns(mut self, returns: bool) -> Self {
        self.returns = returns;
        self
    }

    /// Sets which ways the thumb moves.
    pub fn axes(mut self, axes: JoystickAxes) -> Self {
        self.axes = axes;
        self
    }

    /// Sets whether the joystick hides until pressed, then appears where the press lands.
    pub fn floating(mut self, floating: bool) -> Self {
        self.floating = floating;
        self
    }

    /// Sets the theme tokens for the joystick widget.
    pub fn tokens(mut self, tokens: JoystickTokens) -> Self {
        self.tokens = tokens;
        self
    }

    /// This config with every value made usable, warning about each repair.
    fn sanitized(mut self) -> Self {
        self.radius = at_least(self.radius, MIN_SIZE, "JoystickConfig radius");
        self.reach = at_least(self.reach, MIN_SIZE, "JoystickConfig reach");
        self.thumb_size = at_least(self.thumb_size, MIN_SIZE, "JoystickConfig thumb size");
        self.dead_zone = within(
            self.dead_zone,
            0.0,
            MAX_DEAD_ZONE,
            "JoystickConfig dead zone",
        );
        self
    }
}

impl Default for JoystickConfig {
    fn default() -> Self {
        Self {
            radius: DEFAULT_RADIUS,
            reach: None,
            thumb_size: DEFAULT_THUMB_SIZE,
            dead_zone: DEFAULT_DEAD_ZONE,
            curve: DEFAULT_CURVE,
            returns: true,
            start: Vec2::ZERO,
            axes: JoystickAxes::default(),
            floating: false,
            tokens: JoystickTokens::with_prefix(JOYSTICK_STYLE),
        }
    }
}

/// The cursor over a joystick: an open hand, closed while it's held.
fn hand(held: bool) -> HoverCursor {
    HoverCursor::new(if held {
        SystemCursorIcon::Grabbing
    } else {
        SystemCursorIcon::Grab
    })
}

/// Spawns it; documented on [`SpawnUi::joystick`](crate::spawn_ui::SpawnUi::joystick).
pub(crate) fn spawn_joystick<'a>(
    parent: &'a mut ChildSpawnerCommands,
    config: JoystickConfig,
) -> EntityCommands<'a> {
    let JoystickConfig {
        radius,
        reach,
        thumb_size,
        dead_zone,
        curve,
        returns,
        start,
        axes,
        floating,
        tokens,
    } = config.sanitized();
    let reach = reach.unwrap_or(radius);

    // Initial stick position: Centered for most, but can be preset if `returns=false`.
    let (stick, value) = if returns || floating || !start.is_finite() {
        (Stick::default(), JoystickValue::default())
    } else {
        let offset = axes.constrain(Vec2::new(start.x, -start.y).clamp_length_max(1.0) * reach);
        let reading = read_stick(offset, reach, dead_zone, curve, false);
        let stick = Stick {
            hand: reading.thumb * reach,
            engaged: reading.engaged,
        };
        (stick, JoystickValue(reading.output))
    };

    let base_size = axes.base_size(radius, thumb_size);
    // The outline sits where the dead zone's edge puts the thumb.
    let ring_size = axes.dead_zone_size(2.0 * radius * dead_zone, thumb_size);
    let centred = |size: Vec2| Node {
        position_type: PositionType::Absolute,
        left: percent(50),
        top: percent(50),
        width: px(size.x),
        height: px(size.y),
        margin: UiRect {
            left: px(-size.x / 2.0),
            top: px(-size.y / 2.0),
            ..default()
        },
        border_radius: BorderRadius::MAX,
        ..default()
    };
    let base = Node {
        width: px(base_size.x),
        height: px(base_size.y),
        border: UiRect::all(px(BASE_BORDER)),
        border_radius: BorderRadius::MAX,
        // Fixed size: a crowded parent can't squeeze its thumb's room away.
        flex_shrink: 0.0,
        ..default()
    };

    let mut joystick = parent.spawn((
        JoystickSettings {
            reach,
            dead_zone,
            curve,
            returns,
        },
        Travel {
            radius,
            thumb_size,
            axes,
        },
        value,
        stick,
        Hovered::default(),
        hand(false),
    ));
    let mut parts = JoystickParts {
        base: joystick.id(),
        dead_zone: Entity::PLACEHOLDER,
        thumb: Entity::PLACEHOLDER,
    };
    if floating {
        joystick.insert((Floating, floating::area()));
        joystick.with_children(|area| parts.base = area.spawn(floating::body(base)).id());
    } else {
        joystick.insert(base);
    }
    joystick
        .commands()
        .entity(parts.base)
        .with_children(|base| {
            let ring = Node {
                border: UiRect::all(px(DEAD_ZONE_BORDER)),
                ..centred(ring_size)
            };
            parts.dead_zone = base.spawn((Pickable::IGNORE, ring)).id();
            let thumb = Node {
                border: UiRect::all(px(THUMB_BORDER)),
                ..centred(Vec2::splat(thumb_size))
            };
            parts.thumb = base.spawn((Pickable::IGNORE, thumb)).id();
        });
    // Its look comes with its tokens, in `style_joystick`, once its parts are in.
    joystick.insert((parts, tokens));
    joystick
}

/// Sets the output and announces it, if it changed.
fn report(
    commands: &mut Commands,
    joystick: Entity,
    current: JoystickValue,
    output: Vec2,
    is_final: bool,
) {
    if current.0 != output || is_final {
        commands.entity(joystick).insert(JoystickValue(output));
        commands.trigger(ValueChange {
            source: joystick,
            value: output,
            is_final,
        });
    }
}

/// Where a floating stick's thumb is, and where its area is laid out.
type Appearing = (
    &'static JoystickSettings,
    &'static Travel,
    &'static Stick,
    &'static JoystickParts,
    Placement,
);

/// A press in a floating joystick's area shows it there, with its thumb,
/// wherever the stick was left, under the pointer.
fn appear(
    press: On<PointerPress>,
    sticks: Query<Appearing, (With<Floating>, Without<Gripped>)>,
    mut bases: Query<(&mut Node, &ComputedNode, &mut Visibility)>,
    ui_scale: Option<Res<UiScale>>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    let Ok((settings, travel, stick, parts, area)) = sticks.get(press.entity) else {
        return;
    };
    let scale = floating::scale_of(ui_scale);
    let Some(point) = floating::point_in(press.pointer.position, area, scale) else {
        return;
    };
    let thumb = stick.hand / settings.reach * travel.radius;
    if let Ok(base) = bases.get_mut(parts.base) {
        floating::show_at(point - thumb, base);
    }
}

/// Takes hold of the stick where it is, stopping any glide home.
fn grip(start: On<PointerDragStart>, sticks: Query<&Stick>, mut commands: Commands) {
    if start.button != PointerButton::Primary {
        return;
    }
    if let Ok(stick) = sticks.get(start.entity) {
        commands
            .entity(start.entity)
            .remove::<Returning>()
            .insert((Gripped { origin: stick.hand }, hand(true)));
    }
}

/// Moves the held stick with the hand, and reports the new output.
fn steer(
    drag: On<PointerDrag>,
    sticks: Query<(&JoystickSettings, &Travel, &Gripped, &Stick, &JoystickValue)>,
    ui_scale: Option<Res<UiScale>>,
    mut commands: Commands,
) {
    let Ok((settings, travel, gripped, stick, &value)) = sticks.get(drag.entity) else {
        return;
    };
    // Pointer distances are window pixels; node sizes are UI pixels.
    let offset = gripped.origin + drag.distance / floating::scale_of(ui_scale);
    let offset = travel.axes.constrain(offset);
    let reading = read_stick(
        offset,
        settings.reach,
        settings.dead_zone,
        settings.curve,
        stick.engaged,
    );
    commands.entity(drag.entity).insert(Stick {
        hand: reading.thumb * settings.reach,
        engaged: reading.engaged,
    });
    report(&mut commands, drag.entity, value, reading.output, false);
}

/// Sends the stick home: it reads zero from now on. A thumb on show glides
/// back; a floating one, hidden, is simply there.
fn send_home(
    entity: &mut EntityCommands,
    settings: &JoystickSettings,
    stick: &Stick,
    floating: bool,
) {
    if floating {
        entity.remove::<Returning>().insert(Stick::default());
    } else {
        let home = Glide::new(stick.hand, Vec2::ZERO, RETURN_ACCELERATION * settings.reach);
        entity.insert((
            Stick {
                hand: stick.hand,
                engaged: false,
            },
            Returning(home),
        ));
    }
}

/// A stick's response, where it is, what it reads, and how it's drawn.
type LetGo = (
    &'static JoystickSettings,
    &'static Stick,
    &'static JoystickValue,
    &'static JoystickParts,
    Has<Floating>,
);

/// Lets go of the stick: a returning one reads zero at once and goes home;
/// any other stays, and its output is final. A floating one hides.
fn release(
    end: On<PointerDragEnd>,
    sticks: Query<LetGo, With<Gripped>>,
    mut bases: Query<&mut Visibility>,
    mut commands: Commands,
) {
    let Ok((settings, stick, &value, parts, floating)) = sticks.get(end.entity) else {
        return;
    };
    if floating && let Ok(base) = bases.get_mut(parts.base) {
        floating::hide(base);
    }
    let mut entity = commands.entity(end.entity);
    entity.remove::<Gripped>().insert(hand(false));
    if settings.returns {
        send_home(&mut entity, settings, stick, floating);
        report(&mut commands, end.entity, value, Vec2::ZERO, true);
    } else {
        report(&mut commands, end.entity, value, value.0, true);
    }
}

/// A floating joystick pressed and let go without a drag hides again,
/// reading what it did.
fn vanish(
    release: On<PointerRelease>,
    sticks: Query<&JoystickParts, (With<Floating>, Without<Gripped>)>,
    mut bases: Query<&mut Visibility>,
) {
    if release.button != PointerButton::Primary {
        return;
    }
    if let Ok(parts) = sticks.get(release.entity)
        && let Ok(base) = bases.get_mut(parts.base)
    {
        floating::hide(base);
    }
}

/// A right-click sends a stick that isn't held back to the centre, reading
/// zero: the way to let go of one that stays where it's left.
fn reset(press: On<PointerPress>, sticks: Query<LetGo, Without<Gripped>>, mut commands: Commands) {
    if press.button != PointerButton::Secondary {
        return;
    }
    let Ok((settings, stick, &value, _, floating)) = sticks.get(press.entity) else {
        return;
    };
    if stick.hand == Vec2::ZERO && value.0 == Vec2::ZERO {
        return;
    }
    send_home(
        &mut commands.entity(press.entity),
        settings,
        stick,
        floating,
    );
    report(&mut commands, press.entity, value, Vec2::ZERO, true);
}

/// Glides released thumbs home. The output is already zero: only the thumb
/// moves. Real time, so it runs while the game is paused.
fn animate_return(
    time: Res<Time<Real>>,
    mut sticks: Query<(Entity, &mut Stick, &mut Returning)>,
    mut commands: Commands,
) {
    for (entity, mut stick, mut returning) in &mut sticks {
        let (hand, arrived) = returning.0.advance(time.delta_secs());
        stick.hand = hand;
        if arrived {
            commands.entity(entity).remove::<Returning>();
        }
    }
}

/// Draws each thumb where its stick is, in proportion: at the rim when the
/// hand is at the stick's reach.
fn position_thumbs(
    sticks: Query<(&JoystickSettings, &Travel, &Stick, &JoystickParts), Changed<Stick>>,
    mut nodes: Query<&mut Node>,
) {
    for (settings, travel, stick, parts) in &sticks {
        let at = stick.hand / settings.reach * travel.radius;
        let half = travel.thumb_size / 2.0;
        let (left, top) = (px(at.x - half), px(at.y - half));
        if let Ok(mut thumb) = nodes.get_mut(parts.thumb)
            && (thumb.margin.left, thumb.margin.top) != (left, top)
        {
            thumb.margin.left = left;
            thumb.margin.top = top;
        }
    }
}

/// The thumb's border, lit while the joystick is hovered or held.
fn thumb_border(tokens: &JoystickTokens, hovered: bool, held: bool) -> ThemeBorder {
    let state = WidgetFlags {
        hovered,
        pressed: held,
        ..default()
    }
    .state();
    ThemeBorder::all(tokens.thumb_border.clone().with_state(state))
}

/// A joystick was hovered, unhovered or grabbed. Releases are read separately,
/// because change detection doesn't see removals.
type StickChanged = (With<JoystickParts>, Or<(Changed<Hovered>, Added<Gripped>)>);

/// Highlights the thumb while the joystick is hovered or held.
fn restyle_thumbs(
    changed: Query<Entity, StickChanged>,
    mut released: RemovedComponents<Gripped>,
    sticks: Query<(&JoystickTokens, &JoystickParts, &Hovered, Has<Gripped>)>,
    borders: Query<&ThemeBorder>,
    mut commands: Commands,
) {
    let mut dirty: EntityHashSet = changed.iter().collect();
    dirty.extend(released.read());
    for entity in dirty {
        let Ok((tokens, parts, hovered, held)) = sticks.get(entity) else {
            continue;
        };
        let border = thumb_border(tokens, hovered.0, held);
        if borders.get(parts.thumb).is_ok_and(|old| *old != border) {
            commands.entity(parts.thumb).insert(border);
        }
    }
}

/// Styles a joystick when its tokens are set: at spawn, and whenever the app sets new ones.
fn style_joystick(
    insert: On<Insert<JoystickTokens>>,
    sticks: Query<(&JoystickTokens, &JoystickParts, &Hovered, Has<Gripped>)>,
    mut commands: Commands,
) {
    let Ok((tokens, parts, hovered, held)) = sticks.get(insert.entity) else {
        return;
    };
    commands.entity(parts.base).insert((
        ThemeSkin(tokens.bg.clone()),
        ThemeBorder::all(tokens.border.clone()),
    ));
    commands
        .entity(parts.dead_zone)
        .insert(ThemeBorder::all(tokens.dead_zone.clone()));
    commands.entity(parts.thumb).insert((
        ThemeSkin(tokens.thumb.clone()),
        thumb_border(tokens, hovered.0, held),
    ));
}

/// Joystick behaviour and default token values.
pub struct JoystickWidgetPlugin;

impl Plugin for JoystickWidgetPlugin {
    fn build(&self, app: &mut App) {
        app.ensure_plugin(HoverCursorPlugin)
            .ensure_plugin(ThemePlugin)
            .add_observer(style_joystick)
            .add_observer(appear)
            .add_observer(grip)
            .add_observer(steer)
            .add_observer(release)
            .add_observer(vanish)
            .add_observer(reset)
            .add_systems(Update, animate_return)
            .add_systems(
                PostUpdate,
                (position_thumbs, restyle_thumbs).before(UiSystems::Prepare),
            );
        JoystickTokens::with_prefix(JOYSTICK_STYLE)
            .register_theme_defaults(&mut app.world_mut().resource_mut::<Theme>());
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy::camera::NormalizedRenderTarget;
    use bevy::picking::backend::HitData;
    use bevy::picking::events::Pointer;
    use bevy::picking::pointer::{Location, PointerId};
    use bevy::time::TimeUpdateStrategy;

    use super::*;

    const REACH: f32 = 100.0;

    fn read(offset: Vec2, was_engaged: bool) -> StickReading {
        read_stick(offset, REACH, 0.2, ResponseCurve::Linear, was_engaged)
    }

    #[test]
    fn dead_zone_reads_zero_and_output_grows_smoothly_from_its_edge() {
        assert_eq!(read(Vec2::new(15.0, 10.0), false).output, Vec2::ZERO);
        // Just past the edge, output is tiny, not a jump to 0.2.
        let just_past = read(Vec2::new(21.0, 0.0), false);
        assert!(just_past.engaged && just_past.output.x > 0.0 && just_past.output.x < 0.02);
        // Halfway between edge and rim reads 0.5.
        assert!((read(Vec2::new(60.0, 0.0), false).output.x - 0.5).abs() < 1e-5);
    }

    #[test]
    fn extremes_clamp_by_length_and_up_is_positive() {
        let corner = read(Vec2::new(1000.0, -1000.0), false);
        assert!(
            (corner.output.length() - 1.0).abs() < 1e-5,
            "diagonals reach 1, not 1.41"
        );
        assert!(
            (corner.thumb.length() - 1.0).abs() < 1e-5,
            "the thumb stays on the rim"
        );
        assert!(corner.output.y > 0.0, "dragging up (screen -y) reads as +y");
        assert_eq!(read(Vec2::NAN, true), StickReading::CENTRED);
        assert_eq!(
            read_stick(Vec2::X, 0.0, 0.2, ResponseCurve::Linear, false),
            StickReading::CENTRED
        );
    }

    #[test]
    fn tremor_at_the_dead_zone_edge_does_not_flicker() {
        let mut engaged = false;
        let lengths = [19.0, 21.0, 19.0, 17.0, 20.0, 15.0];
        let states: Vec<bool> = lengths
            .iter()
            .map(|&x| {
                engaged = read(Vec2::new(x, 0.0), engaged).engaged;
                engaged
            })
            .collect();
        assert_eq!(states, [false, true, true, true, true, false]);
    }

    #[test]
    fn the_curve_shapes_output_but_not_the_thumb() {
        let reading = read_stick(
            Vec2::new(60.0, 0.0),
            REACH,
            0.2,
            ResponseCurve::Power(2.0),
            false,
        );
        assert!((reading.output.x - 0.25).abs() < 1e-5, "0.5 squared");
        assert!(
            (reading.thumb.x - 0.6).abs() < 1e-5,
            "the thumb still follows the hand"
        );
    }

    fn joystick_app(config: JoystickConfig) -> (App, Entity) {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, JoystickWidgetPlugin))
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
                20,
            )));
        let mut joystick = None;
        app.world_mut()
            .commands()
            .spawn(Node::default())
            .with_children(|root| joystick = Some(spawn_joystick(root, config).id()));
        app.update();
        (app, joystick.unwrap())
    }

    /// Where the thumb's left edge is, from the centre.
    fn thumb_left(app: &App, joystick: Entity) -> Val {
        let thumb = app.world().get::<JoystickParts>(joystick).unwrap().thumb;
        app.world().get::<Node>(thumb).unwrap().margin.left
    }

    #[test]
    fn a_released_thumb_glides_home() {
        let (mut app, joystick) = joystick_app(JoystickConfig::default());
        let start = Vec2::new(30.0, -40.0);
        app.world_mut().entity_mut(joystick).insert((
            Stick {
                hand: start,
                engaged: false,
            },
            Returning(Glide::new(
                start,
                Vec2::ZERO,
                RETURN_ACCELERATION * DEFAULT_RADIUS,
            )),
        ));
        app.update();
        let home = px(-DEFAULT_THUMB_SIZE / 2.0);
        assert_ne!(
            thumb_left(&app, joystick),
            home,
            "drawn away from the centre"
        );
        for _ in 0..20 {
            app.update();
        }
        assert_eq!(thumb_left(&app, joystick), home, "thumb is home");
        assert!(app.world().get::<Returning>(joystick).is_none());
    }

    #[test]
    fn a_long_reach_moves_the_thumb_in_proportion() {
        let config = JoystickConfig::new()
            .radius(20.0)
            .reach(80.0)
            .thumb_size(10.0);
        let (mut app, joystick) = joystick_app(config);
        // The hand halfway to its reach: the thumb halfway to the rim.
        app.world_mut().entity_mut(joystick).insert(Stick {
            hand: Vec2::new(40.0, 0.0),
            engaged: true,
        });
        app.update();
        assert_eq!(thumb_left(&app, joystick), px(10.0 - 5.0));
    }

    #[test]
    fn new_tokens_restyle_every_part() {
        let (mut app, joystick) = joystick_app(JoystickConfig::default());
        let tokens = JoystickTokens::with_prefix("game.stick");
        app.world_mut().entity_mut(joystick).insert(tokens.clone());
        app.update();
        let parts = *app.world().get::<JoystickParts>(joystick).unwrap();
        let world = app.world();
        let fill = |entity| world.get::<ThemeSkin>(entity).unwrap().0.clone();
        assert_eq!(fill(joystick), tokens.bg);
        assert_eq!(fill(parts.thumb), tokens.thumb);
        assert_eq!(
            world.get::<ThemeBorder>(parts.dead_zone),
            Some(&ThemeBorder::all(tokens.dead_zone.clone()))
        );
    }

    /// Every `ValueChange<Vec2>` the joysticks announced, in order.
    #[derive(Resource, Default)]
    struct Heard(Vec<(Vec2, bool)>);

    fn heard_app(config: JoystickConfig) -> (App, Entity) {
        let (mut app, joystick) = joystick_app(config);
        app.init_resource::<Heard>().add_observer(
            |change: On<ValueChange<Vec2>>, mut heard: ResMut<Heard>| {
                heard.0.push((change.value, change.is_final));
            },
        );
        (app, joystick)
    }

    fn pointer(at: Vec2) -> Pointer {
        let target = NormalizedRenderTarget::None {
            width: 800,
            height: 600,
        };
        Pointer::new(
            PointerId::Mouse,
            Location {
                target,
                position: at,
            },
        )
    }

    fn hit() -> HitData {
        HitData::new(Entity::PLACEHOLDER, 0.0, None, None)
    }

    fn press(app: &mut App, entity: Entity, at: Vec2, button: PointerButton) {
        let pointer = pointer(at);
        let (hit, count) = (hit(), 1);
        app.world_mut().trigger(PointerPress {
            entity,
            pointer,
            button,
            hit,
            count,
        });
        app.update();
    }

    fn release_at(app: &mut App, entity: Entity, at: Vec2) {
        let (pointer, hit, button) = (pointer(at), hit(), PointerButton::Primary);
        app.world_mut().trigger(PointerRelease {
            entity,
            pointer,
            button,
            hit,
        });
        app.update();
    }

    /// Grabs the stick and drags it `distance` window pixels.
    fn drag(app: &mut App, entity: Entity, distance: Vec2) {
        let button = PointerButton::Primary;
        app.world_mut().trigger(PointerDragStart {
            entity,
            pointer: pointer(Vec2::ZERO),
            button,
            hit: hit(),
        });
        app.update();
        app.world_mut().trigger(PointerDrag {
            entity,
            pointer: pointer(distance),
            button,
            distance,
            delta: distance,
        });
        app.update();
    }

    fn drag_end(app: &mut App, entity: Entity, distance: Vec2) {
        app.world_mut().trigger(PointerDragEnd {
            entity,
            pointer: pointer(distance),
            button: PointerButton::Primary,
            distance,
        });
        app.update();
    }

    fn value(app: &App, joystick: Entity) -> Vec2 {
        app.world().get::<JoystickValue>(joystick).unwrap().0
    }

    fn base_shown(app: &App, joystick: Entity) -> bool {
        let base = app.world().get::<JoystickParts>(joystick).unwrap().base;
        app.world().get::<Visibility>(base) != Some(&Visibility::Hidden)
    }

    #[test]
    fn a_throttle_moves_along_its_axis_only() {
        let config = JoystickConfig::throttle()
            .dead_zone(0.0)
            .curve(ResponseCurve::Linear);
        let (mut app, joystick) = joystick_app(config);
        // Up and to the side: only the up counts, half the reach of 48.
        drag(&mut app, joystick, Vec2::new(40.0, -24.0));
        assert_eq!(value(&app, joystick), Vec2::new(0.0, 0.5));
        let stick = app.world().get::<Stick>(joystick).unwrap();
        assert_eq!(
            stick.hand,
            Vec2::new(0.0, -24.0),
            "the thumb stays on its track"
        );
        // A throttle stays where it's left.
        drag_end(&mut app, joystick, Vec2::new(40.0, -24.0));
        assert_eq!(value(&app, joystick), Vec2::new(0.0, 0.5));
    }

    #[test]
    fn a_throttles_track_is_as_deep_as_its_thumb() {
        let config = JoystickConfig::throttle().radius(40.0).thumb_size(20.0);
        let (app, joystick) = joystick_app(config);
        let node = app.world().get::<Node>(joystick).unwrap();
        let depth = 20.0 + 2.0 * BASE_BORDER;
        assert_eq!((node.width, node.height), (px(depth), px(100.0)));
    }

    #[test]
    fn a_right_click_sends_a_left_stick_home() {
        let config = JoystickConfig::new()
            .returns(false)
            .curve(ResponseCurve::Linear);
        let (mut app, joystick) = heard_app(config);
        drag(&mut app, joystick, Vec2::new(40.0, 0.0));
        drag_end(&mut app, joystick, Vec2::new(40.0, 0.0));
        assert_ne!(value(&app, joystick), Vec2::ZERO, "left where it was");

        // A primary click does nothing to it; a right-click resets it.
        press(&mut app, joystick, Vec2::ZERO, PointerButton::Primary);
        assert_ne!(value(&app, joystick), Vec2::ZERO);
        app.world_mut().resource_mut::<Heard>().0.clear();
        press(&mut app, joystick, Vec2::ZERO, PointerButton::Secondary);
        assert_eq!(value(&app, joystick), Vec2::ZERO);
        assert_eq!(app.world().resource::<Heard>().0, [(Vec2::ZERO, true)]);
        assert!(
            app.world().get::<Returning>(joystick).is_some(),
            "the thumb glides"
        );
    }

    /// The tests have no layout, so nodes have no size: a press at a point
    /// puts the base's corner, rather than its centre, there.
    #[test]
    fn a_floating_stick_appears_with_its_thumb_under_the_press() {
        let config = JoystickConfig::new().floating(true).returns(false);
        let (mut app, joystick) = joystick_app(config);
        assert!(!base_shown(&app, joystick), "hidden until pressed");

        // Left halfway out to the right: half the radius of 48.
        app.world_mut().entity_mut(joystick).insert(Stick {
            hand: Vec2::new(24.0, 0.0),
            engaged: true,
        });
        press(
            &mut app,
            joystick,
            Vec2::new(100.0, 50.0),
            PointerButton::Primary,
        );
        assert!(base_shown(&app, joystick));
        let base = app.world().get::<JoystickParts>(joystick).unwrap().base;
        let node = app.world().get::<Node>(base).unwrap();
        assert_eq!((node.left, node.top), (px(76.0), px(50.0)));

        // A tap without a drag hides it again, leaving the stick alone.
        release_at(&mut app, joystick, Vec2::new(100.0, 50.0));
        assert!(!base_shown(&app, joystick));
        let stick = app.world().get::<Stick>(joystick).unwrap();
        assert_eq!(stick.hand, Vec2::new(24.0, 0.0));
    }

    #[test]
    fn a_floating_stick_that_returns_is_home_and_hidden_once_let_go() {
        let config = JoystickConfig::new().floating(true);
        let (mut app, joystick) = joystick_app(config);
        press(
            &mut app,
            joystick,
            Vec2::new(100.0, 50.0),
            PointerButton::Primary,
        );
        drag(&mut app, joystick, Vec2::new(40.0, 0.0));
        assert_ne!(value(&app, joystick), Vec2::ZERO);

        drag_end(&mut app, joystick, Vec2::new(40.0, 0.0));
        assert!(!base_shown(&app, joystick));
        assert_eq!(value(&app, joystick), Vec2::ZERO);
        let world = app.world();
        assert_eq!(world.get::<Stick>(joystick).unwrap().hand, Vec2::ZERO);
        assert!(
            world.get::<Returning>(joystick).is_none(),
            "no glide, unseen"
        );
    }

    #[test]
    fn unusable_configs_are_repaired() {
        let config = JoystickConfig::new()
            .radius(-1.0)
            .reach(0.0)
            .thumb_size(f32::NAN)
            .dead_zone(3.0)
            .sanitized();
        assert_eq!(config.radius, MIN_SIZE);
        assert_eq!(config.reach, Some(MIN_SIZE));
        assert_eq!(config.thumb_size, MIN_SIZE);
        assert_eq!(config.dead_zone, MAX_DEAD_ZONE);
    }

    /// A stick that stays where it's left can start partway, reading as if left there.
    #[test]
    fn a_stick_that_stays_can_start_partway() {
        let config = JoystickConfig::throttle()
            .start(Vec2::new(0.0, 0.5))
            .dead_zone(0.0)
            .curve(ResponseCurve::Linear);
        let (app, joystick) = joystick_app(config);
        let value = app.world().get::<JoystickValue>(joystick).unwrap().0;
        assert!((value - Vec2::new(0.0, 0.5)).length() < 1e-4, "{value}");

        let springy = JoystickConfig::new().start(Vec2::new(0.0, 0.5));
        let (app, joystick) = joystick_app(springy);
        let value = app.world().get::<JoystickValue>(joystick).unwrap().0;
        assert_eq!(value, Vec2::ZERO, "a stick that returns starts centred");
    }
}
