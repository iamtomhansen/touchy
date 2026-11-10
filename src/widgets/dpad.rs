//! A directional pad: press a direction, slide between them, hold to repeat.
//!
//! The pad reads the pointer by where it is from the centre. It supports 2-,
//! 4-, or 8-way presses via [`DpadWays`]. It is drawn using [`DpadShape`].
//! Outputs directions as an `IVec2` with +y up and is announced as
//! `ValueChange<IVec2>` whenever it changes. Releasing announces zero with
//! `is_final: true`. Held presses can repeat announces at a specific interval
//! using [`repeat`](DpadConfig::repeat). A [`floating`](DpadConfig::floating)
//! pad waits, hidden, in an area and appears centred wherever it's pressed.

use std::f32::consts::{FRAC_PI_4, TAU};

use bevy::ecs::entity::EntityHashSet;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::input_focus::{FocusLost, FocusedInput};
use bevy::picking::events::{PointerDrag, PointerDragEnd, PointerPress, PointerRelease};
use bevy::picking::hover::Hovered;
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use bevy::ui::{BackgroundGradient, ComputedUiRenderTargetInfo, UiGlobalTransform, UiSystems};
use bevy::ui_widgets::ValueChange;

use crate::EnsurePlugin;
use crate::cursor::{HoverCursor, HoverCursorPlugin};
use crate::sanitize::{at_least, within};
use crate::theme::palette::{ACCENT, CONTROL, CONTROL_CORNER_RADIUS, EDGE, HIGHLIGHT};
use crate::theme::skins;
use crate::theme::{
    FocusVisible, Shade, Theme, ThemeCornerRadius, ThemePlugin, ThemeSkin, ThemeToken, WidgetFlags,
    WidgetState,
};
use crate::widgets::floating::{self, Floating, Placement};
use crate::widgets::wedge::wedges;

/// Prefix of the D-pad widget's theme tokens.
pub const DPAD_STYLE: &str = "touchy.dpad";

/// The pad's width, for four and eight ways, in pixels.
const DEFAULT_SIZE: f32 = 96.0;
/// The least a pad can be, in pixels.
const MIN_SIZE: f32 = 1.0;
/// A cross's arms are this fraction of the pad wide; so are the lit arms of a solid pad.
const ARM_FRACTION: f32 = 0.36;
/// A solid two-way pad is this fraction as deep as it is long.
const SOLID_TWO_WAY_DEPTH: f32 = 0.5;
/// The border around the whole pad.
const BORDER_WIDTH: f32 = 2.0;
/// Fraction of the pad, from the centre, that presses nothing.
const DEFAULT_DEAD_ZONE: f32 = 0.2;
/// Largest allowed dead zone; beyond this little of the pad would respond.
const MAX_DEAD_ZONE: f32 = 0.8;
/// The shortest repeat interval, in seconds: about a frame.
const MIN_REPEAT: f32 = 0.016;
/// The solid four- and eight-way shape is a square seen through a square
/// turned this far: an octagon.
const OCTAGON_TURN: f32 = FRAC_PI_4;

/// Which directions the pad snaps to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DpadWays {
    /// Left and right only.
    Horizontal,
    /// Up and down only.
    Vertical,
    /// Up, down, left and right.
    #[default]
    Four,
    /// The four, and the diagonals between them.
    Eight,
}

impl DpadWays {
    fn is_two_way(self) -> bool {
        matches!(self, Self::Horizontal | Self::Vertical)
    }

    /// How many slices a round pad is cut into.
    fn slices(self) -> usize {
        match self {
            Self::Horizontal | Self::Vertical => 2,
            Self::Four => 4,
            Self::Eight => 8,
        }
    }

    /// The arms it lights, one for each way along each axis.
    fn arms(self) -> &'static [IVec2] {
        match self {
            Self::Horizontal => &[IVec2::NEG_X, IVec2::X],
            Self::Vertical => &[IVec2::Y, IVec2::NEG_Y],
            Self::Four | Self::Eight => &[IVec2::NEG_X, IVec2::X, IVec2::Y, IVec2::NEG_Y],
        }
    }
}

/// How the pad is drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DpadShape {
    /// A plus sign, or a single bar for two ways.
    #[default]
    Cross,
    /// An octagon, or a rectangle for two ways.
    Solid,
    /// A circle, its pressed direction's slice lit: halves for two ways,
    /// quarters for four, eighths for eight.
    Circle,
}

/// Theme tokens for a D-pad widget. Insert new ones to restyle a pad.
///
/// Hovering, focusing and pressing the pad show on its border, in
/// [`WidgetState::Hover`], [`WidgetState::Focused`] and [`WidgetState::Pressed`]; the pressed direction's arms light in `arm`.
#[derive(Component, Clone, PartialEq, Debug)]
#[component(immutable)]
pub struct DpadTokens {
    /// The pad's face.
    pub bg: ThemeToken,
    /// The border around the pad, lit while it's hovered or pressed.
    pub border: ThemeToken,
    /// The arms of the pressed direction.
    pub arm: ThemeToken,
    /// The radius of the pad's corners, in pixels.
    pub corner_radius: ThemeToken,
}

impl DpadTokens {
    /// Names the tokens under `prefix`.
    pub fn with_prefix(prefix: &str) -> Self {
        let token = |suffix: &str| ThemeToken::new(format!("{prefix}.{suffix}"));
        Self {
            bg: token("bg"),
            border: token("border"),
            arm: token("arm"),
            corner_radius: token("corner_radius"),
        }
    }

    /// Derives these tokens from the palette, unless the app sets them.
    pub fn register_theme_defaults(&self, theme: &mut Theme) {
        let lit_border = |state| self.border.clone().with_state(state);
        theme
            .default_derived(self.bg.clone(), CONTROL, Shade::Same)
            .default_derived(self.border.clone(), EDGE, Shade::Same)
            .default_derived(lit_border(WidgetState::Hover), HIGHLIGHT, Shade::Same)
            .default_derived(lit_border(WidgetState::Focused), HIGHLIGHT, Shade::Same)
            .default_derived(lit_border(WidgetState::Pressed), HIGHLIGHT, Shade::Same)
            .default_derived(self.arm.clone(), ACCENT, Shade::Same)
            .default_size_from(self.corner_radius.clone(), CONTROL_CORNER_RADIUS);
        // Each part draws its role's skin, once a theme gives the role one. Not the pad: a
        // picture there is drawn in place of the arms, for a d-pad of its own.
        theme.default_skin_from(self.arm.clone(), skins::RAISED);
    }
}

/// The built-in token set.
impl Default for DpadTokens {
    fn default() -> Self {
        Self::with_prefix(DPAD_STYLE)
    }
}

/// The direction a pointer at `offset` from center presses.
pub fn read_pad(offset: Vec2, ways: DpadWays, dead_zone: f32) -> IVec2 {
    let dead_zone = dead_zone.clamp(0.0, MAX_DEAD_ZONE);
    if !offset.is_finite() || offset.length() < dead_zone {
        return IVec2::ZERO;
    }
    let sign = |value: f32| if value < 0.0 { -1 } else { 1 };
    match ways {
        DpadWays::Horizontal if offset.x.abs() >= dead_zone => IVec2::new(sign(offset.x), 0),
        DpadWays::Vertical if offset.y.abs() >= dead_zone => IVec2::new(0, sign(offset.y)),
        DpadWays::Horizontal | DpadWays::Vertical => IVec2::ZERO,
        DpadWays::Four if offset.x.abs() >= offset.y.abs() => IVec2::new(sign(offset.x), 0),
        DpadWays::Four => IVec2::new(0, sign(offset.y)),
        DpadWays::Eight => {
            let eighth = (offset.y.atan2(offset.x) / FRAC_PI_4).round();
            let angle = eighth * FRAC_PI_4;
            IVec2::new(angle.cos().round() as i32, angle.sin().round() as i32)
        }
    }
}

/// How a pad responds. Change it to change how the pad feels.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct DpadSettings {
    /// Fraction of the pad, from the centre, that presses nothing, up to 0.8.
    pub dead_zone: f32,
    /// Seconds between announcements of a held direction, or `None` to
    /// announce each direction once per press.
    pub repeat: Option<f32>,
}

/// The direction pressed, +y up; zero when nothing is.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DpadValue(pub IVec2);

/// The ways a pad snaps to, as it was drawn.
#[derive(Component, Clone, Copy, Debug)]
struct Ways(DpadWays);

/// What's pressing the pad: a pointer, and arrow keys while it's focused.
#[derive(Component, Clone, Copy, Debug, Default)]
struct PadInput {
    pointer: IVec2,
    /// The arrow keys held: left, right, down, up.
    keys: [bool; 4],
    /// The axis pressed most recently, which wins on a four-way pad.
    vertical_last: bool,
    /// Seconds since the direction was last announced.
    since_announced: f32,
    /// Whether a pointer is holding the pad.
    held: bool,
}

impl PadInput {
    fn keys_direction(&self, ways: DpadWays) -> IVec2 {
        let [left, right, down, up] = self.keys.map(i32::from);
        let both = IVec2::new(right - left, up - down);
        match ways {
            DpadWays::Horizontal => both.with_y(0),
            DpadWays::Vertical => both.with_x(0),
            DpadWays::Four if both.x != 0 && both.y != 0 => {
                if self.vertical_last {
                    both.with_x(0)
                } else {
                    both.with_y(0)
                }
            }
            DpadWays::Four | DpadWays::Eight => both,
        }
    }

    /// The pointer wins while it's pressing; otherwise the keys decide.
    fn direction(&self, ways: DpadWays) -> IVec2 {
        if self.pointer != IVec2::ZERO {
            self.pointer
        } else {
            self.keys_direction(ways)
        }
    }

    /// The pointer lets go, however it does: nothing it pressed stays pressed.
    fn let_go(&mut self) {
        self.held = false;
        self.pointer = IVec2::ZERO;
    }
}

/// The parts of a pad, on the pad.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub struct DpadParts {
    /// The node the pad is drawn in: the pad itself, or for a floating pad,
    /// the part that appears where its area is pressed.
    pub body: Entity,
    /// The nodes drawing its border.
    pub borders: Vec<Entity>,
    /// The nodes drawing its face, over the border.
    pub faces: Vec<Entity>,
    /// Its arms, each with the direction it lights for; a round pad has none.
    pub arms: Vec<(IVec2, Entity)>,
    /// A round pad's lit slice, drawn over its face.
    pub slice: Option<Entity>,
    /// A node over the whole pad, under its arms, that shows the face's skin once when the theme
    /// skins it, in place of the shape's own border and face.
    pub picture: Entity,
}

/// Construction-time configuration for a D-pad widget.
///
/// Consumed by [`SpawnUi::dpad`](crate::spawn_ui::SpawnUi::dpad) and not retained as a component.
///
/// # Examples
/// ```
/// use touchy::prelude::*;
///
/// let config = DpadConfig::new(DpadWays::Eight)
///     .size(100.0)
///     .dead_zone(0.2)
///     .repeat(0.5);
///
/// # assert_eq!(config.size, 100.0);
/// # assert_eq!(config.dead_zone, 0.2);
/// # assert_eq!(config.repeat, Some(0.5));
/// ```
#[derive(Clone, Debug)]
pub struct DpadConfig {
    /// How many directions it snaps to.
    pub ways: DpadWays,
    /// How it's drawn.
    pub shape: DpadShape,
    /// Its length along its directions, in pixels.
    pub size: f32,
    /// Fraction of the pad, from the centre, that presses nothing, up to 0.8.
    pub dead_zone: f32,
    /// Seconds between announcements of a held direction, or `None` to
    /// announce each direction once per press.
    pub repeat: Option<f32>,
    /// Hide until pressed, then appear centred where the press lands. The pad
    /// fills its parent, which sets the area that takes presses.
    pub floating: bool,
    /// The theme tokens for the pad.
    pub tokens: DpadTokens,
}

impl DpadConfig {
    /// Creates a new config with the given number of ways.
    pub fn new(ways: DpadWays) -> Self {
        Self {
            ways,
            shape: DpadShape::default(),
            size: DEFAULT_SIZE,
            dead_zone: DEFAULT_DEAD_ZONE,
            repeat: None,
            floating: false,
            tokens: DpadTokens::with_prefix(DPAD_STYLE),
        }
    }

    /// Sets the number of ways the pad snaps to: 2, 4, or 8.
    pub fn ways(mut self, ways: DpadWays) -> Self {
        self.ways = ways;
        self
    }

    /// Sets the pad's shape: a cross, a solid octagon, or a circle.
    pub fn shape(mut self, shape: DpadShape) -> Self {
        self.shape = shape;
        self
    }

    /// Sets the pad's length along its directions, in pixels.
    pub fn size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }

    /// Sets the fraction of the pad, from the centre, that presses nothing.
    pub fn dead_zone(mut self, fraction: f32) -> Self {
        self.dead_zone = fraction;
        self
    }

    /// Announce a held direction again every `seconds`.
    pub fn repeat(mut self, seconds: f32) -> Self {
        self.repeat = Some(seconds);
        self
    }

    /// Sets whether the pad hides until pressed, then appears where the press lands.
    pub fn floating(mut self, floating: bool) -> Self {
        self.floating = floating;
        self
    }

    /// Sets the theme tokens for the pad's face, border, and lit arms.
    pub fn tokens(mut self, tokens: DpadTokens) -> Self {
        self.tokens = tokens;
        self
    }

    /// This config with every value made usable, warning about each repair.
    fn sanitized(mut self) -> Self {
        self.size = at_least(self.size, MIN_SIZE, "DpadConfig size");
        self.dead_zone = within(self.dead_zone, 0.0, MAX_DEAD_ZONE, "DpadConfig dead zone");
        self.repeat = at_least(self.repeat, MIN_REPEAT, "DpadConfig repeat");
        self
    }
}

impl Default for DpadConfig {
    fn default() -> Self {
        Self::new(DpadWays::default())
    }
}

/// The pad's outer size: square for four and eight ways, a bar for two.
fn pad_size(ways: DpadWays, shape: DpadShape, size: f32) -> Vec2 {
    let depth = match shape {
        DpadShape::Cross => size * ARM_FRACTION,
        DpadShape::Solid => size * SOLID_TWO_WAY_DEPTH,
        // Round however many ways: two ways are halves of it.
        DpadShape::Circle => size,
    };
    match ways {
        DpadWays::Horizontal => Vec2::new(size, depth),
        DpadWays::Vertical => Vec2::new(depth, size),
        DpadWays::Four | DpadWays::Eight => Vec2::splat(size),
    }
}

/// A node of `size`, centred in a pad of `pad` size.
fn centered(pad: Vec2, size: Vec2) -> Node {
    let corner = (pad - size) / 2.0;
    Node {
        position_type: PositionType::Absolute,
        left: px(corner.x),
        top: px(corner.y),
        width: px(size.x),
        height: px(size.y),
        // Fixed size: a crowded parent can't squeeze it.
        flex_shrink: 0.0,
        ..default()
    }
}

/// The lit arm for `direction`, from the centre to the inside of the border.
fn arm_node(pad: Vec2, thickness: f32, direction: IVec2) -> Node {
    let reach = pad / 2.0 - BORDER_WIDTH;
    let (size, corner) = if direction.x != 0 {
        let left = if direction.x > 0 {
            pad.x / 2.0
        } else {
            BORDER_WIDTH
        };
        (
            Vec2::new(reach.x, thickness),
            Vec2::new(left, (pad.y - thickness) / 2.0),
        )
    } else {
        let top = if direction.y > 0 {
            BORDER_WIDTH
        } else {
            pad.y / 2.0
        };
        (
            Vec2::new(thickness, reach.y),
            Vec2::new((pad.x - thickness) / 2.0, top),
        )
    };
    Node {
        position_type: PositionType::Absolute,
        left: px(corner.x),
        top: px(corner.y),
        width: px(size.x),
        height: px(size.y),
        ..default()
    }
}

/// Spawns it; documented on [`SpawnUi::dpad`](crate::spawn_ui::SpawnUi::dpad).
pub(crate) fn spawn_dpad<'a>(
    parent: &'a mut ChildSpawnerCommands,
    config: DpadConfig,
) -> EntityCommands<'a> {
    let DpadConfig {
        ways,
        shape,
        size,
        dead_zone,
        repeat,
        floating,
        tokens,
    } = config.sanitized();
    let pad = pad_size(ways, shape, size);
    let arm = size * ARM_FRACTION;
    let inset = Vec2::splat(2.0 * BORDER_WIDTH);

    let mut root = parent.spawn((
        Ways(ways),
        DpadSettings { dead_zone, repeat },
        DpadValue::default(),
        PadInput::default(),
        Hovered::default(),
        // Tab reaches it, and the arrow keys press it once focused.
        TabIndex(0),
        HoverCursor::POINTER,
    ));
    let body = Node {
        width: px(pad.x),
        height: px(pad.y),
        ..default()
    };
    let mut parts = DpadParts {
        body: root.id(),
        borders: Vec::new(),
        faces: Vec::new(),
        arms: Vec::new(),
        slice: None,
        picture: Entity::PLACEHOLDER,
    };
    if floating {
        root.insert((Floating, floating::area()));
        root.with_children(|area| parts.body = area.spawn(floating::body(body)).id());
    } else {
        root.insert(body);
    }
    let body = parts.body;
    root.commands().entity(body).with_children(|pad_parts| {
        let part = |pad_parts: &mut ChildSpawnerCommands, node: Node| {
            pad_parts.spawn((Pickable::IGNORE, node)).id()
        };
        match (shape, ways.is_two_way()) {
            // A circle: the border, the face inset, the lit slice over it, and
            // the dead zone in the middle, in the face's colour.
            (DpadShape::Circle, _) => {
                let round = |size: Vec2| Node {
                    border_radius: BorderRadius::MAX,
                    ..centered(pad, size)
                };
                parts.borders.push(part(pad_parts, round(pad)));
                parts.faces.push(part(pad_parts, round(pad - inset)));
                parts.slice = Some(part(pad_parts, round(pad - inset)));
                let middle = Vec2::splat(dead_zone * size);
                parts.faces.push(part(pad_parts, round(middle)));
            }
            // A bar or a rectangle: the border, with the face inset.
            (_, true) => {
                parts.borders.push(part(pad_parts, centered(pad, pad)));
                parts
                    .faces
                    .push(part(pad_parts, centered(pad, pad - inset)));
            }
            // Two bars each for the border and the face: a plus sign.
            (DpadShape::Cross, false) => {
                let bars = [Vec2::new(pad.x, arm), Vec2::new(arm, pad.y)];
                for bar in bars {
                    parts.borders.push(part(pad_parts, centered(pad, bar)));
                }
                for bar in bars {
                    parts
                        .faces
                        .push(part(pad_parts, centered(pad, bar - inset)));
                }
            }
            // A square seen through a square turned an eighth: an octagon with
            // every side the same distance from the centre. The face is a
            // smaller octagon on top, so the border is even all round.
            (DpadShape::Solid, false) => {
                let octagon = |pad_parts: &mut ChildSpawnerCommands, size: Vec2| {
                    let window = Node {
                        overflow: Overflow::clip(),
                        ..centered(pad, size)
                    };
                    let turned = (
                        Pickable::IGNORE,
                        Node {
                            width: percent(100),
                            height: percent(100),
                            ..default()
                        },
                        UiTransform::from_rotation(Rot2::radians(OCTAGON_TURN)),
                    );
                    let mut square = Entity::PLACEHOLDER;
                    pad_parts
                        .spawn((Pickable::IGNORE, window))
                        .with_children(|window| square = window.spawn(turned).id());
                    square
                };
                parts.borders.push(octagon(pad_parts, pad));
                parts.faces.push(octagon(pad_parts, pad - inset));
            }
        }

        parts.picture = pad_parts
            .spawn((Pickable::IGNORE, centered(pad, pad), Visibility::Hidden))
            .id();
        let thickness = if ways.is_two_way() {
            pad.min_element() - 2.0 * BORDER_WIDTH
        } else {
            arm - 2.0 * BORDER_WIDTH
        };
        let arms = if shape == DpadShape::Circle {
            &[]
        } else {
            ways.arms()
        };
        for &direction in arms {
            let lit = (
                Pickable::IGNORE,
                arm_node(pad, thickness, direction),
                Visibility::Hidden,
            );
            parts.arms.push((direction, pad_parts.spawn(lit).id()));
        }
    });
    // Its look comes with its tokens, in `style_dpad`, once its parts are in.
    root.insert((parts, tokens));
    root
}

/// Where a pointer is on a pad, as a fraction of its half-size, +y up.
fn pointer_offset(
    position: Vec2,
    (node, target, transform): (
        &ComputedNode,
        &ComputedUiRenderTargetInfo,
        &UiGlobalTransform,
    ),
    ui_scale: f32,
) -> Option<Vec2> {
    let physical = position * target.scale_factor() / ui_scale;
    let local = transform.try_inverse()?.transform_point2(physical);
    let half = node.size() / 2.0;
    (half.min_element() > 0.0).then(|| Vec2::new(local.x / half.x, -local.y / half.y))
}

/// A pad as a pointer presses it.
type Pressable = (
    &'static Ways,
    &'static DpadSettings,
    &'static mut PadInput,
    &'static DpadParts,
    Has<Floating>,
);

/// A press presses the direction under the pointer. A floating pad appears
/// centred on the press instead, which presses nothing until it slides.
fn press(
    press: On<PointerPress>,
    mut pads: Query<Pressable>,
    placements: Query<Placement>,
    mut bodies: Query<(&mut Node, &ComputedNode, &mut Visibility)>,
    ui_scale: Option<Res<UiScale>>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    let Ok((&Ways(ways), settings, mut input, parts, floats)) = pads.get_mut(press.entity) else {
        return;
    };
    let (position, scale) = (press.pointer.position, floating::scale_of(ui_scale));
    if floats {
        if let Ok(area) = placements.get(press.entity)
            && let Some(point) = floating::point_in(position, area, scale)
            && let Ok(body) = bodies.get_mut(parts.body)
        {
            floating::show_at(point, body);
        }
        input.held = true;
        input.pointer = IVec2::ZERO;
    } else if let Ok(body) = placements.get(parts.body)
        && let Some(offset) = pointer_offset(position, body, scale)
    {
        input.held = true;
        input.pointer = read_pad(offset, ways, settings.dead_zone);
    }
}

/// Sliding the pointer moves between directions, even past the pad's edge.
fn slide(
    drag: On<PointerDrag>,
    mut pads: Query<Pressable>,
    placements: Query<Placement>,
    ui_scale: Option<Res<UiScale>>,
) {
    let Ok((&Ways(ways), settings, mut input, parts, _)) = pads.get_mut(drag.entity) else {
        return;
    };
    if !input.held {
        return;
    }
    let scale = floating::scale_of(ui_scale);
    if let Ok(body) = placements.get(parts.body)
        && let Some(offset) = pointer_offset(drag.pointer.position, body, scale)
    {
        input.pointer = read_pad(offset, ways, settings.dead_zone);
    }
}

/// A pad the pointer can let go of, and its parts, to hide a floating one.
type Releasable = (&'static mut PadInput, &'static DpadParts, Has<Floating>);

/// The pointer lets go of `pad`; a floating pad hides.
fn let_go_of(pad: Entity, pads: &mut Query<Releasable>, bodies: &mut Query<&mut Visibility>) {
    let Ok((mut input, parts, floats)) = pads.get_mut(pad) else {
        return;
    };
    input.let_go();
    if floats && let Ok(body) = bodies.get_mut(parts.body) {
        floating::hide(body);
    }
}

/// The pointer lets go over the pad.
fn let_go_on_release(
    release: On<PointerRelease>,
    mut pads: Query<Releasable>,
    mut bodies: Query<&mut Visibility>,
) {
    let_go_of(release.entity, &mut pads, &mut bodies);
}

/// The pointer lets go after sliding off the pad, which a release over it misses.
fn let_go_after_drag(
    end: On<PointerDragEnd>,
    mut pads: Query<Releasable>,
    mut bodies: Query<&mut Visibility>,
) {
    let_go_of(end.entity, &mut pads, &mut bodies);
}

/// Which arrow key `key` is, as an index into [`PadInput::keys`], and whether
/// it's vertical. The one place the pad reads keys, for the key mapping to take over.
fn arrow(key: &Key) -> Option<(usize, bool)> {
    match key {
        Key::ArrowLeft => Some((0, false)),
        Key::ArrowRight => Some((1, false)),
        Key::ArrowDown => Some((2, true)),
        Key::ArrowUp => Some((3, true)),
        _ => None,
    }
}

/// The arrow keys press the pad while it's focused. The pad repeats on its
/// own schedule, so the keyboard's own repeats are ignored.
fn press_with_keys(
    mut keys: On<FocusedInput<KeyboardInput>>,
    mut pads: Query<&mut PadInput, With<Ways>>,
) {
    let Ok(mut input) = pads.get_mut(keys.focused_entity) else {
        return;
    };
    let Some((index, vertical)) = arrow(&keys.input.logical_key) else {
        return;
    };
    // Arrow keys belong to the pad while it's focused, not to whatever's behind it.
    keys.propagate(false);
    if keys.input.repeat {
        return;
    }
    let pressed = keys.input.state.is_pressed();
    input.keys[index] = pressed;
    if pressed {
        input.vertical_last = vertical;
    }
}

/// Keys held when focus leaves would never see their release.
fn forget_keys_on_blur(lost: On<FocusLost>, mut pads: Query<&mut PadInput>) {
    if let Ok(mut input) = pads.get_mut(lost.entity) {
        input.keys = [false; 4];
    }
}

/// Announces each new direction, and a held one again at the pad's repeat
/// interval. Real time, so a paused game's menus still repeat.
fn announce_directions(
    time: Res<Time<Real>>,
    mut pads: Query<(Entity, &Ways, &DpadSettings, &mut PadInput, &mut DpadValue)>,
    mut commands: Commands,
) {
    for (entity, &Ways(ways), settings, mut input, mut value) in &mut pads {
        let direction = input.direction(ways);
        let announce = if direction != value.0 {
            value.0 = direction;
            input.since_announced = 0.0;
            true
        } else if let Some(interval) = settings.repeat
            && direction != IVec2::ZERO
        {
            input.since_announced += time.delta_secs();
            let due = input.since_announced >= interval;
            if due {
                input.since_announced -= interval;
            }
            due
        } else {
            false
        };
        if announce {
            commands.trigger(ValueChange {
                source: entity,
                value: direction,
                is_final: direction == IVec2::ZERO,
            });
        }
    }
}

/// What a pad's look is picked from.
type PadLook = (
    &'static DpadTokens,
    &'static DpadParts,
    &'static DpadValue,
    &'static PadInput,
    &'static Hovered,
    Has<FocusVisible>,
);

/// The pad's border in its state: lit while hovered, focused or pressed.
fn border_look(
    tokens: &DpadTokens,
    value: &DpadValue,
    input: &PadInput,
    hovered: &Hovered,
    focused: bool,
) -> ThemeSkin {
    ThemeSkin(
        tokens
            .border
            .clone()
            .with_state(pad_state(value, input, hovered, focused)),
    )
}

/// The pad's picture in its state, so a theme can give a pressed pad its own art.
fn picture_look(
    tokens: &DpadTokens,
    value: &DpadValue,
    input: &PadInput,
    hovered: &Hovered,
    focused: bool,
) -> ThemeSkin {
    ThemeSkin(
        tokens
            .bg
            .clone()
            .with_state(pad_state(value, input, hovered, focused)),
    )
}

/// The state a pad shows: hovered, focused or pressed.
fn pad_state(
    value: &DpadValue,
    input: &PadInput,
    hovered: &Hovered,
    focused: bool,
) -> Option<WidgetState> {
    let flags = WidgetFlags {
        hovered: hovered.0,
        focused,
        pressed: input.held || value.0 != IVec2::ZERO,
        disabled: false,
    };
    flags.state()
}

/// The way a direction points on screen, clockwise from straight up.
fn bearing_of_direction(direction: IVec2) -> f32 {
    // +y is up here, as the pad reads it; a bearing turns from up towards +x.
    (direction.x as f32)
        .atan2(direction.y as f32)
        .rem_euclid(TAU)
}

/// Lights a round pad's slice for the pressed direction, in the theme's
/// colour as it is now, when the direction, the tokens or the theme change.
fn light_slices(
    pads: Query<(&Ways, Ref<DpadTokens>, &DpadParts, Ref<DpadValue>)>,
    theme: Res<Theme>,
    mut gradients: Query<&mut BackgroundGradient>,
    mut commands: Commands,
) {
    for (&Ways(ways), tokens, parts, value) in &pads {
        let Some(slice) = parts.slice else {
            continue;
        };
        if !value.is_changed() && !tokens.is_changed() && !theme.is_changed() {
            continue;
        }
        let lit = (value.0 != IVec2::ZERO)
            .then(|| (bearing_of_direction(value.0), theme.color(&tokens.arm)));
        let wanted = wedges(lit, ways.slices());
        match gradients.get_mut(slice) {
            Ok(mut had) => {
                had.set_if_neq(wanted);
            }
            Err(_) => {
                commands.entity(slice).insert(wanted);
            }
        }
    }
}

/// A pad's direction, hold, hover or focus changed. Losing the focus is read
/// separately, because change detection doesn't see removals.
type PadChanged = (
    With<DpadTokens>,
    Or<(
        Changed<DpadValue>,
        Changed<PadInput>,
        Changed<Hovered>,
        Added<FocusVisible>,
    )>,
);

/// Lights the arms of the pressed direction, and the border while hovered,
/// focused or pressed.
fn restyle_pads(
    changed: Query<Entity, PadChanged>,
    mut unfocused: RemovedComponents<FocusVisible>,
    pads: Query<PadLook>,
    mut visibilities: Query<&mut Visibility>,
    backgrounds: Query<&ThemeSkin>,
    mut commands: Commands,
) {
    let dirty: EntityHashSet = changed.iter().chain(unfocused.read()).collect();
    for (tokens, parts, value, input, hovered, focused) in pads.iter_many(dirty).flatten() {
        let border = border_look(tokens, value, input, hovered, focused);
        for &part in &parts.borders {
            if backgrounds.get(part).is_ok_and(|old| *old != border) {
                commands.entity(part).insert(border.clone());
            }
        }
        let picture = picture_look(tokens, value, input, hovered, focused);
        if backgrounds
            .get(parts.picture)
            .is_ok_and(|old| *old != picture)
        {
            commands.entity(parts.picture).insert(picture);
        }
        for &(direction, arm) in &parts.arms {
            let lit = (direction.x != 0 && direction.x == value.0.x)
                || (direction.y != 0 && direction.y == value.0.y);
            if let Ok(mut visibility) = visibilities.get_mut(arm) {
                visibility.set_if_neq(if lit {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                });
            }
        }
    }
}

/// Styles a pad when its tokens are set: at spawn, and whenever the app sets new ones.
fn style_dpad(insert: On<Insert<DpadTokens>>, pads: Query<PadLook>, mut commands: Commands) {
    let Ok((tokens, parts, value, input, hovered, focused)) = pads.get(insert.entity) else {
        return;
    };
    let rounded = ThemeCornerRadius(tokens.corner_radius.clone());
    let looks = [
        (
            &parts.borders,
            border_look(tokens, value, input, hovered, focused),
        ),
        (&parts.faces, ThemeSkin(tokens.bg.clone())),
    ];
    for (nodes, look) in looks {
        for &node in nodes {
            let mut node = commands.entity(node);
            node.insert(look.clone());
            // A round pad stays round; the rest take the theme's corners.
            if parts.slice.is_none() {
                node.insert(rounded.clone());
            }
        }
    }
    let lit = ThemeSkin(tokens.arm.clone());
    for &(_, arm) in &parts.arms {
        commands.entity(arm).insert((lit.clone(), rounded.clone()));
    }
    let picture = picture_look(tokens, value, input, hovered, focused);
    commands.entity(parts.picture).insert(picture);
}

/// Shows a pad's skin once over the whole pad, hiding the shape's border and face, while the
/// theme skins its face; the shape again when it stops.
fn show_pad_pictures(
    theme: Res<Theme>,
    pads: Query<(Ref<DpadTokens>, &DpadParts)>,
    mut visibilities: Query<&mut Visibility>,
) {
    let show = |shown: bool| {
        if shown {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        }
    };
    for (tokens, parts) in &pads {
        if !theme.is_changed() && !tokens.is_changed() {
            continue;
        }
        let skinned = theme.is_skinned(&tokens.bg);
        let shape = parts.borders.iter().chain(&parts.faces).chain(&parts.slice);
        for &node in shape {
            if let Ok(mut visibility) = visibilities.get_mut(node) {
                visibility.set_if_neq(show(!skinned));
            }
        }
        if let Ok(mut visibility) = visibilities.get_mut(parts.picture) {
            visibility.set_if_neq(show(skinned));
        }
    }
}

/// D-pad behaviour and default token values.
pub struct DpadWidgetPlugin;

impl Plugin for DpadWidgetPlugin {
    fn build(&self, app: &mut App) {
        app.ensure_plugin(HoverCursorPlugin)
            .ensure_plugin(ThemePlugin)
            .add_observer(press)
            .add_observer(slide)
            .add_observer(let_go_on_release)
            .add_observer(let_go_after_drag)
            .add_observer(press_with_keys)
            .add_observer(forget_keys_on_blur)
            .add_observer(style_dpad)
            .add_systems(Update, announce_directions)
            .add_systems(
                PostUpdate,
                (restyle_pads, light_slices, show_pad_pictures).before(UiSystems::Prepare),
            );
        DpadTokens::with_prefix(DPAD_STYLE)
            .register_theme_defaults(&mut app.world_mut().resource_mut::<Theme>());
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy::camera::NormalizedRenderTarget;
    use bevy::input::ButtonState;
    use bevy::input::keyboard::NativeKeyCode;
    use bevy::picking::backend::HitData;
    use bevy::picking::events::Pointer;
    use bevy::picking::pointer::{Location, PointerId};
    use bevy::time::TimeUpdateStrategy;

    use super::*;

    const DEAD: f32 = 0.2;
    /// Frames are this long in the app tests.
    const FRAME_MS: u64 = 100;

    fn read(x: f32, y: f32, ways: DpadWays) -> IVec2 {
        read_pad(Vec2::new(x, y), ways, DEAD)
    }

    #[test]
    fn four_ways_split_the_pad_into_quarters_around_the_axes() {
        use DpadWays::Four;
        assert_eq!(read(0.1, 0.1, Four), IVec2::ZERO, "inside the dead zone");
        assert_eq!(read(0.9, 0.1, Four), IVec2::X);
        assert_eq!(read(-0.9, 0.4, Four), IVec2::NEG_X);
        assert_eq!(read(0.3, 0.8, Four), IVec2::Y, "+y is up");
        assert_eq!(read(-0.3, -0.8, Four), IVec2::NEG_Y);
        assert_eq!(read(f32::NAN, 0.5, Four), IVec2::ZERO);
    }

    #[test]
    fn eight_ways_give_each_diagonal_an_eighth() {
        use DpadWays::Eight;
        assert_eq!(read(0.7, 0.7, Eight), IVec2::new(1, 1));
        assert_eq!(read(-0.6, -0.5, Eight), IVec2::new(-1, -1));
        // 20° above the axis is still straight right; 25° is the diagonal.
        let at = |degrees: f32| {
            let radians = degrees.to_radians();
            read(radians.cos(), radians.sin(), Eight)
        };
        assert_eq!(at(20.0), IVec2::X);
        assert_eq!(at(25.0), IVec2::new(1, 1));
        assert_eq!(at(110.0), IVec2::Y);
        assert_eq!(at(-150.0), IVec2::new(-1, -1));
    }

    #[test]
    fn two_ways_look_at_their_axis_only() {
        use DpadWays::{Horizontal, Vertical};
        assert_eq!(
            read(0.5, 0.9, Horizontal),
            IVec2::X,
            "steep, but rightwards"
        );
        assert_eq!(read(-0.5, 0.0, Horizontal), IVec2::NEG_X);
        assert_eq!(
            read(0.1, 0.9, Horizontal),
            IVec2::ZERO,
            "too little sideways to count"
        );
        assert_eq!(read(0.9, -0.5, Vertical), IVec2::NEG_Y);
    }

    #[test]
    fn four_way_keys_take_the_axis_pressed_last_and_eight_combine_them() {
        let mut input = PadInput {
            keys: [false, true, false, true],
            vertical_last: true,
            ..default()
        };
        assert_eq!(input.keys_direction(DpadWays::Eight), IVec2::new(1, 1));
        assert_eq!(input.keys_direction(DpadWays::Four), IVec2::Y);
        input.vertical_last = false;
        assert_eq!(input.keys_direction(DpadWays::Four), IVec2::X);
        assert_eq!(input.keys_direction(DpadWays::Vertical), IVec2::Y);
        // Opposite keys cancel.
        input.keys = [true, true, false, false];
        assert_eq!(input.keys_direction(DpadWays::Eight), IVec2::ZERO);
        // A pointer pressing the pad wins over the keys.
        input.pointer = IVec2::NEG_Y;
        assert_eq!(input.direction(DpadWays::Eight), IVec2::NEG_Y);
    }

    #[test]
    fn unusable_configs_are_repaired() {
        let config = DpadConfig {
            size: -3.0,
            dead_zone: 2.0,
            repeat: Some(0.0),
            ..default()
        }
        .sanitized();
        assert_eq!(config.size, MIN_SIZE);
        assert_eq!(config.dead_zone, MAX_DEAD_ZONE);
        assert_eq!(config.repeat, Some(MIN_REPEAT), "not every frame");
    }

    #[derive(Resource, Default)]
    struct Heard(Vec<(IVec2, bool)>);

    fn app_with(config: DpadConfig) -> (App, Entity) {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, DpadWidgetPlugin))
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
                FRAME_MS,
            )))
            .init_resource::<Heard>()
            .add_observer(|change: On<ValueChange<IVec2>>, mut heard: ResMut<Heard>| {
                heard.0.push((change.value, change.is_final));
            });
        let mut pad = None;
        app.world_mut()
            .commands()
            .spawn(Node::default())
            .with_children(|root| pad = Some(spawn_dpad(root, config).id()));
        app.update();
        (app, pad.unwrap())
    }

    fn parts(app: &App, pad: Entity) -> DpadParts {
        app.world().get::<DpadParts>(pad).unwrap().clone()
    }

    fn hold(app: &mut App, pad: Entity, direction: IVec2) {
        let mut input = app.world_mut().get_mut::<PadInput>(pad).unwrap();
        input.held = direction != IVec2::ZERO;
        input.pointer = direction;
    }

    fn heard(app: &mut App) -> Vec<(IVec2, bool)> {
        std::mem::take(&mut app.world_mut().resource_mut::<Heard>().0)
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

    /// The tests have no layout, so nodes have no size: a press at a point
    /// puts the pad's corner, rather than its centre, there.
    #[test]
    fn a_floating_pad_appears_where_pressed_and_hides_when_let_go() {
        let (mut app, pad) = app_with(DpadConfig::default().floating(true));
        let body = parts(&app, pad).body;
        assert_ne!(body, pad, "drawn in its own node, inside its area");
        let shown = |app: &App| app.world().get::<Visibility>(body) != Some(&Visibility::Hidden);
        assert!(!shown(&app), "hidden until pressed");

        let at = Vec2::new(120.0, 80.0);
        app.world_mut().trigger(PointerPress {
            entity: pad,
            pointer: pointer(at),
            button: PointerButton::Primary,
            hit: hit(),
            count: 1,
        });
        app.update();
        assert!(shown(&app));
        let node = app.world().get::<Node>(body).unwrap();
        assert_eq!((node.left, node.top), (px(at.x), px(at.y)));
        let input = app.world().get::<PadInput>(pad).unwrap();
        assert!(input.held);
        assert_eq!(
            input.pointer,
            IVec2::ZERO,
            "it appears centred: nothing pressed"
        );

        app.world_mut().trigger(PointerRelease {
            entity: pad,
            pointer: pointer(at),
            button: PointerButton::Primary,
            hit: hit(),
        });
        app.update();
        assert!(!shown(&app));
        assert!(!app.world().get::<PadInput>(pad).unwrap().held);
    }

    #[test]
    fn a_fixed_pad_is_its_own_body() {
        let (app, pad) = app_with(DpadConfig::default());
        assert_eq!(parts(&app, pad).body, pad);
    }

    #[test]
    fn without_repeat_a_held_direction_is_announced_once_and_release_is_final() {
        let (mut app, pad) = app_with(DpadConfig::default());
        hold(&mut app, pad, IVec2::X);
        for _ in 0..10 {
            app.update();
        }
        assert_eq!(heard(&mut app), [(IVec2::X, false)]);
        assert_eq!(
            app.world().get::<DpadValue>(pad),
            Some(&DpadValue(IVec2::X))
        );
        // Sliding to another direction announces it straight away.
        hold(&mut app, pad, IVec2::Y);
        app.update();
        hold(&mut app, pad, IVec2::ZERO);
        app.update();
        assert_eq!(heard(&mut app), [(IVec2::Y, false), (IVec2::ZERO, true)]);
    }

    #[test]
    fn a_held_direction_repeats_at_the_interval() {
        // Three frames per repeat.
        let repeat = 3.0 * FRAME_MS as f32 / 1000.0;
        let (mut app, pad) = app_with(DpadConfig::default().repeat(repeat));
        hold(&mut app, pad, IVec2::NEG_X);
        app.update();
        assert_eq!(heard(&mut app).len(), 1, "announced on press");
        for _ in 0..9 {
            app.update();
        }
        assert_eq!(heard(&mut app), [(IVec2::NEG_X, false); 3]);
        // A new direction starts its own count.
        hold(&mut app, pad, IVec2::Y);
        app.update();
        app.update();
        assert_eq!(heard(&mut app), [(IVec2::Y, false)]);
        // Turned off from code, it stops repeating.
        app.world_mut().get_mut::<DpadSettings>(pad).unwrap().repeat = None;
        for _ in 0..9 {
            app.update();
        }
        assert!(heard(&mut app).is_empty());
    }

    /// Whether every part of the border is drawn in `state`.
    fn border_is(app: &App, pad: Entity, tokens: &DpadTokens, state: Option<WidgetState>) -> bool {
        let expected = tokens.border.clone().with_state(state);
        let borders = parts(app, pad).borders;
        !borders.is_empty()
            && borders
                .iter()
                .all(|&part| app.world().get::<ThemeSkin>(part).map(|bg| &bg.0) == Some(&expected))
    }

    fn lit_arms(app: &App, pad: Entity) -> Vec<IVec2> {
        let mut lit: Vec<_> = parts(app, pad)
            .arms
            .into_iter()
            .filter(|&(_, arm)| app.world().get::<Visibility>(arm) != Some(&Visibility::Hidden))
            .map(|(direction, _)| direction)
            .collect();
        lit.sort_by_key(|direction| (direction.x, direction.y));
        lit
    }

    #[test]
    fn the_whole_border_lights_on_hover_and_press_and_the_pressed_arms_light() {
        for shape in [DpadShape::Cross, DpadShape::Solid] {
            let (mut app, pad) = app_with(DpadConfig::new(DpadWays::Eight).shape(shape));
            let tokens = DpadTokens::with_prefix(DPAD_STYLE);
            assert!(border_is(&app, pad, &tokens, None), "{shape:?}");
            assert!(lit_arms(&app, pad).is_empty());

            app.world_mut().entity_mut(pad).insert(Hovered(true));
            app.update();
            let hovered = Some(WidgetState::Hover);
            assert!(border_is(&app, pad, &tokens, hovered), "{shape:?}");

            hold(&mut app, pad, IVec2::new(-1, 1));
            app.update();
            let pressed = Some(WidgetState::Pressed);
            assert!(border_is(&app, pad, &tokens, pressed), "{shape:?}");
            assert_eq!(
                lit_arms(&app, pad),
                [IVec2::NEG_X, IVec2::Y],
                "a diagonal lights both"
            );
        }
    }

    #[test]
    fn a_round_pad_lights_the_slice_it_presses() {
        use bevy::ui::Gradient;
        let config = DpadConfig::new(DpadWays::Eight).shape(DpadShape::Circle);
        let (mut app, pad) = app_with(config);
        let parts = parts(&app, pad);
        assert!(parts.arms.is_empty());
        let slice = parts.slice.unwrap();
        let lit = |app: &App| {
            app.world()
                .get::<BackgroundGradient>(slice)
                .map(|gradient| gradient.0.clone())
                .unwrap_or_default()
        };
        assert!(lit(&app).is_empty(), "nothing pressed, nothing lit");

        hold(&mut app, pad, IVec2::new(-1, 1));
        app.update();
        let lit = lit(&app);
        let [Gradient::Conic(conic)] = lit.as_slice() else {
            panic!("one slice, as a conic gradient");
        };
        // Up and left is 315°; an eighth wide, it starts 22.5° before.
        let eighth = TAU / 8.0;
        assert!((conic.start - (7.0 * eighth - eighth / 2.0)).abs() < 1e-5);
        let theme = app.world().resource::<Theme>();
        let arm = DpadTokens::with_prefix(DPAD_STYLE).arm;
        assert_eq!(conic.stops[0].color, theme.color(&arm));
        assert!(
            app.world()
                .get::<ThemeCornerRadius>(parts.faces[0])
                .is_none(),
            "round, not the theme's corners"
        );
    }

    #[test]
    fn new_tokens_restyle_every_part() {
        let (mut app, pad) = app_with(DpadConfig::new(DpadWays::Four).shape(DpadShape::Solid));
        let tokens = DpadTokens::with_prefix("game.pad");
        app.world_mut().entity_mut(pad).insert(tokens.clone());
        app.update();
        assert!(border_is(&app, pad, &tokens, None));
        let parts = parts(&app, pad);
        let background = |entity| app.world().get::<ThemeSkin>(entity).unwrap().0.clone();
        assert!(
            parts
                .faces
                .iter()
                .all(|&face| background(face) == tokens.bg)
        );
        assert!(
            parts
                .arms
                .iter()
                .all(|&(_, arm)| background(arm) == tokens.arm)
        );
    }

    fn key(app: &mut App, pad: Entity, logical_key: Key, state: ButtonState, repeat: bool) {
        let keyboard = KeyboardInput {
            key_code: KeyCode::Unidentified(NativeKeyCode::Unidentified),
            logical_key,
            state,
            text: None,
            repeat,
            window: Entity::PLACEHOLDER,
        };
        app.world_mut()
            .trigger(FocusedInput::new(pad, keyboard, Entity::PLACEHOLDER));
        app.update();
    }

    #[test]
    fn arrow_keys_press_a_focused_pad_and_their_own_repeats_are_ignored() {
        use ButtonState::{Pressed, Released};
        let (mut app, pad) = app_with(DpadConfig::new(DpadWays::Eight));
        key(&mut app, pad, Key::ArrowUp, Pressed, false);
        key(&mut app, pad, Key::ArrowLeft, Pressed, false);
        key(&mut app, pad, Key::ArrowLeft, Pressed, true);
        key(&mut app, pad, Key::ArrowUp, Released, false);
        key(&mut app, pad, Key::ArrowLeft, Released, false);
        assert_eq!(
            heard(&mut app),
            [
                (IVec2::Y, false),
                (IVec2::new(-1, 1), false),
                (IVec2::NEG_X, false),
                (IVec2::ZERO, true)
            ]
        );
        // Keys still held when focus moves on are let go.
        key(&mut app, pad, Key::ArrowDown, Pressed, false);
        app.world_mut().trigger(FocusLost { entity: pad });
        app.update();
        assert_eq!(
            heard(&mut app),
            [(IVec2::NEG_Y, false), (IVec2::ZERO, true)]
        );
    }

    #[test]
    fn two_way_pads_have_two_arms() {
        let (app, pad) = app_with(DpadConfig::new(DpadWays::Vertical));
        let mut arms: Vec<_> = parts(&app, pad)
            .arms
            .into_iter()
            .map(|(direction, _)| direction)
            .collect();
        arms.sort_by_key(|direction| direction.y);
        assert_eq!(arms, [IVec2::NEG_Y, IVec2::Y]);
    }

    /// A skinned face is drawn once over the whole pad, not once per bar of the cross, and
    /// the shape's own border and face step aside for it.
    #[test]
    fn a_skinned_face_is_one_picture_over_the_pad() {
        use crate::theme::Skin;

        let (mut app, pad) = app_with(DpadConfig::new(DpadWays::Four).shape(DpadShape::Cross));
        let parts = app.world().get::<DpadParts>(pad).unwrap().clone();
        let shown =
            |app: &App, entity| app.world().get::<Visibility>(entity) != Some(&Visibility::Hidden);
        assert!(!shown(&app, parts.picture) && shown(&app, parts.faces[0]));

        let tokens = DpadTokens::default();
        app.world_mut()
            .resource_mut::<Theme>()
            .set_skin(tokens.bg.clone(), Skin::new(Handle::default()));
        app.update();
        assert!(shown(&app, parts.picture));
        assert!(
            parts
                .faces
                .iter()
                .chain(&parts.borders)
                .all(|&node| !shown(&app, node))
        );
        assert_eq!(
            app.world().get::<ThemeSkin>(parts.picture),
            Some(&ThemeSkin(tokens.bg))
        );
    }
}
