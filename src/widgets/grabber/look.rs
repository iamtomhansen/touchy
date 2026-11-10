//! touchy's own grabbers: a [`Grabbable`] with marks to show it, styled by
//! the theme. The grab itself is the parent module's.

use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui::{InteractionDisabled, UiSystems};
use bevy::window::SystemCursorIcon;

use super::{GrabAxis, GrabPlugin, Grabbable};
use crate::EnsurePlugin;
use crate::cursor::{HoverCursor, HoverCursorPlugin};
use crate::theme::palette::{HIGHLIGHT, TEXT_MUTED};
use crate::theme::{Shade, Theme, ThemePlugin, ThemeSkin, ThemeToken, WidgetFlags, WidgetState};

/// Prefix of the grabber widget's theme tokens.
pub const GRABBER_STYLE: &str = "touchy.grabber";
/// Prefix of the corner grabber's theme tokens, which follow [`GRABBER_STYLE`]'s.
pub const GRABBER_CORNER_STYLE: &str = "touchy.grabber.corner";

/// A grip's dots: their size, the space between them, and how many across and down.
/// At a scale that makes these fractions of a physical pixel, the dots round
/// unevenly; a theme that wants them crisp at every scale skins the mark.
const DOT_SIZE: f32 = 3.0;
const DOT_GAP: f32 = 2.0;
const DOTS: UVec2 = UVec2::new(3, 2);
/// Space around a grip's dots that still takes the grab.
const GRIP_PADDING: f32 = 3.0;
/// A corner grabber's dots, across and down; those nearer its corner than
/// the diagonal are shown, the rest left as space.
const CORNER_DOTS: u32 = 3;
/// A handle's strip, across its axis: how much takes the grab.
const HANDLE_STRIP: f32 = 8.0;
/// A handle's line: how long it is along the strip, and how thick.
const HANDLE_LENGTH: f32 = 28.0;
const HANDLE_THICKNESS: f32 = 3.0;
/// How faint a disabled grabber's marks are.
const DISABLED_ALPHA: f32 = 0.35;
/// How big a skinned grip's or corner's picture is, square, in pixels.
const DEFAULT_SKIN_SIZE: f32 = 16.0;

/// How a grabber looks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GrabberStyle {
    /// Invisible, and fills its parent.
    Ghost,
    /// Six dots.
    #[default]
    Grip,
    /// A line across its axis, on a strip spanning the length of its place.
    Handle,
    /// A triangle of dots in the corner of its place, dragging both ways.
    Corner,
}

impl GrabberStyle {
    /// How many dots it draws, across and down, if it draws dots.
    fn dots(self) -> Option<UVec2> {
        match self {
            Self::Grip => Some(DOTS),
            Self::Corner => Some(UVec2::splat(CORNER_DOTS)),
            Self::Ghost | Self::Handle => None,
        }
    }
}

/// Which corner of its place a corner grabber sits in, and so which way its
/// dots and its cursor face. Change it and the grabber turns to match.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GrabCorner {
    /// The top left corner.
    TopLeft,
    /// The top right corner.
    TopRight,
    /// The bottom left corner.
    BottomLeft,
    /// The bottom right corner.
    #[default]
    BottomRight,
}

impl GrabCorner {
    /// Whether the dot at `index`, counting across then down its grid, is
    /// shown: it is if it's on the diagonal or nearer the corner.
    fn shows(self, index: u32) -> bool {
        let (right, bottom) = match self {
            Self::TopLeft => (false, false),
            Self::TopRight => (true, false),
            Self::BottomLeft => (false, true),
            Self::BottomRight => (true, true),
        };
        let last = CORNER_DOTS - 1;
        let (column, row) = (index % CORNER_DOTS, index / CORNER_DOTS);
        let across = if right { column } else { last - column };
        let down = if bottom { row } else { last - row };
        across + down >= last
    }

    /// The diagonal a hand dragging this corner moves along.
    fn cursor(self) -> SystemCursorIcon {
        match self {
            Self::TopLeft | Self::BottomRight => SystemCursorIcon::NwseResize,
            Self::TopRight | Self::BottomLeft => SystemCursorIcon::NeswResize,
        }
    }
}

/// Theme tokens for a grabber widget. Insert new ones to restyle a grabber.
#[derive(Component, Clone, PartialEq, Debug)]
#[component(immutable)]
pub struct GrabberTokens {
    /// The colour of a grabber's marks, and the line of a handle.
    pub mark: ThemeToken,
    /// The size of a skinned grip's or corner's picture, square, in pixels.
    pub skin_size: ThemeToken,
}

impl GrabberTokens {
    /// Names the tokens under `prefix`.
    pub fn with_prefix(prefix: &str) -> Self {
        Self {
            mark: ThemeToken::new(format!("{prefix}.mark")),
            skin_size: ThemeToken::new(format!("{prefix}.skin_size")),
        }
    }

    /// Makes every token follow the same token in `base`, unless the app sets
    /// it, so these restyle with it. Skins aren't followed: each set is
    /// skinned on its own. Corner grabbers follow the grip's tokens this way.
    pub fn follow(&self, theme: &mut Theme, base: &GrabberTokens) {
        let states = [
            None,
            Some(WidgetState::Hover),
            Some(WidgetState::Pressed),
            Some(WidgetState::Disabled),
        ];
        for state in states {
            theme.default_derived(
                self.mark.clone().with_state(state),
                base.mark.clone().with_state(state),
                Shade::Same,
            );
        }
        theme.default_size_from(self.skin_size.clone(), base.skin_size.clone());
    }

    /// Register the default values for these tokens, unless the app sets them.
    pub fn register_theme_defaults(&self, theme: &mut Theme) {
        let state = |state| self.mark.clone().with_state(state);
        theme
            .default_derived(self.mark.clone(), TEXT_MUTED, Shade::Same)
            .default_derived(state(WidgetState::Hover), HIGHLIGHT, Shade::Same)
            .default_derived(state(WidgetState::Pressed), HIGHLIGHT, Shade::Same)
            .default_derived(
                state(WidgetState::Disabled),
                TEXT_MUTED,
                Shade::Alpha(DISABLED_ALPHA),
            )
            .default_size(self.skin_size.clone(), DEFAULT_SKIN_SIZE);
    }
}

/// The built-in token set.
impl Default for GrabberTokens {
    fn default() -> Self {
        Self::with_prefix(GRABBER_STYLE)
    }
}

/// How to spawn a grabber: its look, the way it drags, and its starting state.
#[derive(Clone, Debug)]
pub struct GrabberConfig {
    /// The style of the grabber.
    pub style: GrabberStyle,
    /// Which way the grabber's drags count.
    pub axis: GrabAxis,
    /// Which corner a corner grabber sits in. Other styles ignore it.
    pub corner: GrabCorner,
    /// Whether the grabber is disabled, so it doesn't take the grab and fades.
    pub disabled: bool,
    /// The theme tokens the grabber uses.
    pub tokens: GrabberTokens,
}

impl GrabberConfig {
    /// Creates a grabber config with `style` and default values for the rest.
    pub fn new(style: GrabberStyle) -> Self {
        Self { style, ..default() }
    }

    /// An invisible grabber filling its parent.
    pub fn ghost() -> Self {
        Self::new(GrabberStyle::Ghost)
    }

    /// Six dots.
    pub fn grip() -> Self {
        Self::new(GrabberStyle::Grip)
    }

    /// A handle that drags along `axis`; its line lies across it.
    pub fn handle(axis: GrabAxis) -> Self {
        Self::new(GrabberStyle::Handle).axis(axis)
    }

    /// A triangle of dots for the `corner` of its place, dragging both ways.
    /// It takes the corner tokens, [`GRABBER_CORNER_STYLE`].
    pub fn corner(corner: GrabCorner) -> Self {
        Self {
            corner,
            tokens: GrabberTokens::with_prefix(GRABBER_CORNER_STYLE),
            ..Self::new(GrabberStyle::Corner)
        }
    }

    /// Sets which way the grabber's drags count.
    pub fn axis(mut self, axis: GrabAxis) -> Self {
        self.axis = axis;
        self
    }

    /// Sets whether the grabber starts disabled.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Sets the theme tokens the grabber uses.
    pub fn tokens(mut self, tokens: GrabberTokens) -> Self {
        self.tokens = tokens;
        self
    }
}

impl Default for GrabberConfig {
    fn default() -> Self {
        Self {
            style: GrabberStyle::default(),
            axis: GrabAxis::default(),
            corner: GrabCorner::default(),
            disabled: false,
            tokens: GrabberTokens::with_prefix(GRABBER_STYLE),
        }
    }
}

/// A grabber's look, set when it's spawned.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
struct Look(GrabberStyle);

/// How a grip's or a corner's marks are drawn just now: as dots, or as one
/// picture of this size, while the theme skins its mark.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
enum Drawn {
    Dots,
    Picture(f32),
}

/// The nodes drawing a grabber's marks: its dots, its line, or its picture.
/// They're among its children, which may hold the app's own too.
#[derive(Component, Clone, Debug, Default, PartialEq, Eq)]
struct Marks(Vec<Entity>);

/// Spawns it; documented on [`SpawnUi::grabber`](crate::spawn_ui::SpawnUi::grabber).
pub(crate) fn spawn_grabber<'a>(
    parent: &'a mut ChildSpawnerCommands,
    config: GrabberConfig,
) -> EntityCommands<'a> {
    let GrabberConfig {
        style,
        axis,
        corner,
        disabled,
        tokens,
    } = config;
    let corner = (style == GrabberStyle::Corner).then_some(corner);
    let mut grabber = parent.spawn((
        Grabbable::default(),
        Look(style),
        axis,
        Hovered::default(),
        hover_cursor(style, axis, corner, false),
        grabber_node(style, axis),
    ));
    if let Some(corner) = corner {
        grabber.insert(corner);
    }
    if style.dots().is_some() {
        grabber.insert(Drawn::Dots);
    }
    if disabled {
        grabber.insert(InteractionDisabled);
    }
    let mut marks = Vec::new();
    grabber.with_children(|parent| marks = spawn_marks(parent, style, axis, corner));
    // Its colours come with its tokens, in `style_grabber`, once its state is in.
    grabber.insert((Marks(marks), tokens));
    grabber
}

/// The node a grabber of this look sits in.
fn grabber_node(style: GrabberStyle, axis: GrabAxis) -> Node {
    match style {
        GrabberStyle::Ghost => Node {
            position_type: PositionType::Absolute,
            left: px(0.0),
            right: px(0.0),
            top: px(0.0),
            bottom: px(0.0),
            ..default()
        },
        GrabberStyle::Grip => dot_grid(DOTS),
        GrabberStyle::Corner => dot_grid(UVec2::splat(CORNER_DOTS)),
        // A strip across the axis that stretches along its place, its line in the middle.
        GrabberStyle::Handle => {
            let (width, height) = strip_size(axis);
            Node {
                width,
                height,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                align_self: AlignSelf::Stretch,
                flex_shrink: 0.0,
                ..default()
            }
        }
    }
}

/// A grid of `dots`, across and down, with a margin that takes the grab too.
fn dot_grid(dots: UVec2) -> Node {
    Node {
        display: Display::Grid,
        grid_template_columns: RepeatedGridTrack::px(dots.x as u16, DOT_SIZE),
        grid_template_rows: RepeatedGridTrack::px(dots.y as u16, DOT_SIZE),
        column_gap: px(DOT_GAP),
        row_gap: px(DOT_GAP),
        padding: UiRect::all(px(GRIP_PADDING)),
        align_self: AlignSelf::Start,
        ..default()
    }
}

/// A handle's strip: narrow across its axis, stretching along its place.
fn strip_size(axis: GrabAxis) -> (Val, Val) {
    match axis {
        GrabAxis::Vertical => (Val::Auto, px(HANDLE_STRIP)),
        _ => (px(HANDLE_STRIP), Val::Auto),
    }
}

/// A handle's line: across the axis it drags along.
fn line_node(axis: GrabAxis) -> Node {
    let (width, height) = match axis {
        GrabAxis::Vertical => (HANDLE_LENGTH, HANDLE_THICKNESS),
        _ => (HANDLE_THICKNESS, HANDLE_LENGTH),
    };
    Node {
        width: px(width),
        height: px(height),
        border_radius: BorderRadius::MAX,
        ..default()
    }
}

/// The cursor a grabber shows: what it does, and the window's own while disabled.
fn hover_cursor(
    style: GrabberStyle,
    axis: GrabAxis,
    corner: Option<GrabCorner>,
    held: bool,
) -> HoverCursor {
    let icon = match (style, axis, corner) {
        (GrabberStyle::Handle, GrabAxis::Horizontal, _) => SystemCursorIcon::ColResize,
        (GrabberStyle::Handle, GrabAxis::Vertical, _) => SystemCursorIcon::RowResize,
        (GrabberStyle::Corner, _, Some(corner)) => corner.cursor(),
        _ if held => SystemCursorIcon::Grabbing,
        _ => SystemCursorIcon::Grab,
    };
    HoverCursor::new(icon).plain_while_disabled()
}

/// Spawns a grabber's marks in its style, returning them.
fn spawn_marks(
    parent: &mut ChildSpawnerCommands,
    style: GrabberStyle,
    axis: GrabAxis,
    corner: Option<GrabCorner>,
) -> Vec<Entity> {
    match style {
        GrabberStyle::Ghost => Vec::new(),
        GrabberStyle::Handle => vec![parent.spawn((Pickable::IGNORE, line_node(axis))).id()],
        GrabberStyle::Grip | GrabberStyle::Corner => {
            let dots = style.dots().unwrap_or(DOTS);
            (0..dots.x * dots.y)
                .map(|index| {
                    let shown = corner.map_or(Visibility::Inherited, |c| dot_visibility(c, index));
                    parent.spawn((Pickable::IGNORE, dot_node(), shown)).id()
                })
                .collect()
        }
    }
}

fn dot_node() -> Node {
    Node {
        border_radius: BorderRadius::MAX,
        ..default()
    }
}

/// A corner grabber's dot, shown or left as space by the way its corner faces.
fn dot_visibility(corner: GrabCorner, index: u32) -> Visibility {
    if corner.shows(index) {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    }
}

/// A dotted grabber, as its marks are drawn.
type Dotted = (
    Entity,
    &'static Look,
    Ref<'static, GrabberTokens>,
    &'static mut Drawn,
    &'static mut Node,
    &'static mut Marks,
    Option<&'static GrabCorner>,
);

/// Swaps a grip's or a corner's dots for one picture when the theme skins its
/// mark, and back when it stops. Its colours are left to [`restyle_grabbers`],
/// which follows.
fn draw_skinned_marks(theme: Res<Theme>, mut grabbers: Query<Dotted>, mut commands: Commands) {
    for (entity, &Look(style), tokens, mut drawn, mut node, mut marks, corner) in &mut grabbers {
        if !theme.is_changed() && !tokens.is_changed() {
            continue;
        }
        let wanted = if theme.is_skinned(&tokens.mark) {
            Drawn::Picture(theme.size(&tokens.skin_size))
        } else {
            Drawn::Dots
        };
        if *drawn == wanted {
            continue;
        }
        *drawn = wanted;
        for &mark in &marks.0 {
            commands.entity(mark).despawn();
        }
        let mut grabber = commands.entity(entity);
        match (wanted, style.dots()) {
            (Drawn::Picture(size), _) => {
                node.display = Display::Flex;
                let picture = Node {
                    width: px(size),
                    height: px(size),
                    ..default()
                };
                let mut spawned = None;
                grabber.with_children(|parent| {
                    spawned = Some(parent.spawn((Pickable::IGNORE, picture)).id());
                });
                marks.0 = spawned.into_iter().collect();
            }
            (Drawn::Dots, Some(dots)) => {
                let grid = dot_grid(dots);
                (
                    node.display,
                    node.grid_template_columns,
                    node.grid_template_rows,
                ) = (
                    grid.display,
                    grid.grid_template_columns,
                    grid.grid_template_rows,
                );
                let corner = corner.copied();
                grabber.with_children(|parent| {
                    marks.0 = spawn_marks(parent, style, GrabAxis::Free, corner);
                });
            }
            (Drawn::Dots, None) => {}
        }
    }
}

/// What a grabber's looks are read from.
type GrabberLook = (
    &'static Look,
    &'static GrabAxis,
    Option<&'static GrabCorner>,
    Option<&'static Drawn>,
    &'static GrabberTokens,
    &'static Marks,
    &'static Grabbable,
    &'static Hovered,
    Has<InteractionDisabled>,
    &'static HoverCursor,
);

/// What makes a grabber look different. New tokens are styled in `style_grabber`.
type LookChanged = Or<(
    Changed<GrabAxis>,
    Changed<GrabCorner>,
    Changed<Drawn>,
    Changed<Grabbable>,
    Changed<Hovered>,
    Added<InteractionDisabled>,
)>;

/// Redraws grabbers whose state, axis, corner or marks changed: their marks'
/// colour, a handle's strip and line, which of a corner's dots show, and the cursor.
fn restyle_grabbers(
    grabbers: Query<GrabberLook>,
    changed: Query<Entity, (With<Look>, LookChanged)>,
    mut enabled: RemovedComponents<InteractionDisabled>,
    mut nodes: Query<&mut Node>,
    mut visibilities: Query<&mut Visibility>,
    mut commands: Commands,
) {
    let mut dirty: Vec<Entity> = enabled.read().collect();
    dirty.extend(changed.iter());
    for entity in dirty {
        let Ok((
            &Look(style),
            &axis,
            corner,
            drawn,
            tokens,
            marks,
            grabbable,
            hovered,
            disabled,
            old_cursor,
        )) = grabbers.get(entity)
        else {
            continue;
        };
        let held = grabbable.is_held();
        let colour = mark_colour(tokens, hovered.0, held, disabled);
        let corner = corner.copied();
        let dotted_corner = corner.filter(|_| drawn == Some(&Drawn::Dots));
        for (index, &mark) in (0..).zip(&marks.0) {
            commands.entity(mark).insert(colour.clone());
            if let Some(corner) = dotted_corner
                && let Ok(mut visibility) = visibilities.get_mut(mark)
            {
                visibility.set_if_neq(dot_visibility(corner, index));
            }
            if style == GrabberStyle::Handle
                && let Ok(mut line) = nodes.get_mut(mark)
            {
                let wanted = line_node(axis);
                if (line.width, line.height) != (wanted.width, wanted.height) {
                    (line.width, line.height) = (wanted.width, wanted.height);
                }
            }
        }
        if style == GrabberStyle::Handle
            && let Ok(mut strip) = nodes.get_mut(entity)
            && (strip.width, strip.height) != strip_size(axis)
        {
            (strip.width, strip.height) = strip_size(axis);
        }
        let cursor = hover_cursor(style, axis, corner, held);
        if *old_cursor != cursor {
            commands.entity(entity).insert(cursor);
        }
    }
}

/// The colour of a grabber's marks in this state.
fn mark_colour(tokens: &GrabberTokens, hovered: bool, held: bool, disabled: bool) -> ThemeSkin {
    let state = WidgetFlags {
        hovered,
        pressed: held,
        disabled,
        ..default()
    }
    .state();
    ThemeSkin(tokens.mark.clone().with_state(state))
}

/// Styles a grabber's marks when its tokens are set: at spawn, and whenever
/// the app sets new ones. A skin's picture comes in `draw_skinned_marks`.
fn style_grabber(
    insert: On<Insert<GrabberTokens>>,
    grabbers: Query<(
        &GrabberTokens,
        &Marks,
        &Grabbable,
        &Hovered,
        Has<InteractionDisabled>,
    )>,
    mut commands: Commands,
) {
    let Ok((tokens, marks, grabbable, hovered, disabled)) = grabbers.get(insert.entity) else {
        return;
    };
    let colour = mark_colour(tokens, hovered.0, grabbable.is_held(), disabled);
    for &mark in &marks.0 {
        commands.entity(mark).insert(colour.clone());
    }
}

/// touchy's grabbers: their looks, cursors and default token values, and the
/// grab itself through [`GrabPlugin`].
pub struct GrabberWidgetPlugin;

impl Plugin for GrabberWidgetPlugin {
    fn build(&self, app: &mut App) {
        app.ensure_plugin(GrabPlugin)
            .ensure_plugin(HoverCursorPlugin)
            .ensure_plugin(ThemePlugin)
            .add_observer(style_grabber)
            .add_systems(
                PostUpdate,
                (draw_skinned_marks, restyle_grabbers)
                    .chain()
                    .before(UiSystems::Prepare),
            );
        let mut theme = app.world_mut().resource_mut::<Theme>();
        let grip = GrabberTokens::with_prefix(GRABBER_STYLE);
        grip.register_theme_defaults(&mut theme);
        GrabberTokens::with_prefix(GRABBER_CORNER_STYLE).follow(&mut theme, &grip);
    }
}

#[cfg(test)]
mod tests {
    use bevy::picking::cursor::EntityCursor;

    use super::super::tests::{drag, heard, hearing_app, take_hold};
    use super::*;
    use crate::theme::Skin;
    use crate::widgets::grabber::GrabPhase;

    fn grabber_app(config: GrabberConfig) -> (App, Entity) {
        let mut app = hearing_app();
        app.add_plugins(GrabberWidgetPlugin);
        let mut grabber = None;
        app.world_mut()
            .commands()
            .spawn(Node::default())
            .with_children(|parent| {
                grabber = Some(spawn_grabber(parent, config).id());
            });
        app.update();
        (app, grabber.unwrap())
    }

    /// The nodes drawing a grabber's marks, and their sizes.
    fn marks(app: &App, grabber: Entity) -> Vec<(Val, Val)> {
        let world = app.world();
        world
            .get::<Marks>(grabber)
            .unwrap()
            .0
            .iter()
            .map(|&mark| {
                let node = world.get::<Node>(mark).unwrap();
                (node.width, node.height)
            })
            .collect()
    }

    /// The colours of a grabber's marks.
    fn colours(app: &App, grabber: Entity) -> Vec<ThemeToken> {
        let world = app.world();
        world
            .get::<Marks>(grabber)
            .unwrap()
            .0
            .iter()
            .map(|&mark| world.get::<ThemeSkin>(mark).unwrap().0.clone())
            .collect()
    }

    fn cursor(app: &App, grabber: Entity) -> Option<EntityCursor> {
        app.world().get::<EntityCursor>(grabber).cloned()
    }

    #[test]
    fn a_disabled_grabber_fades_announces_nothing_and_leaves_the_cursor_alone() {
        let (mut app, grip) = grabber_app(GrabberConfig::grip().disabled(true));
        drag(&mut app, grip, &[Vec2::new(10.0, 0.0)]);
        assert!(heard(&mut app).is_empty());
        assert_eq!(cursor(&app, grip), None);
        let faded = GrabberTokens::with_prefix(GRABBER_STYLE)
            .mark
            .with_state(WidgetState::Disabled);
        assert!(colours(&app, grip).iter().all(|colour| *colour == faded));

        // Enabled again, it's grabbable again.
        app.world_mut()
            .entity_mut(grip)
            .remove::<InteractionDisabled>();
        app.update();
        assert!(cursor(&app, grip).is_some());
        drag(&mut app, grip, &[Vec2::new(10.0, 0.0)]);
        assert_eq!(heard(&mut app).len(), 3);
    }

    #[test]
    fn its_marks_light_while_held_and_a_grip_shows_a_closed_hand() {
        let (mut app, grip) = grabber_app(GrabberConfig::grip());
        take_hold(&mut app, grip);
        app.update();
        let lit = GrabberTokens::with_prefix(GRABBER_STYLE)
            .mark
            .with_state(WidgetState::Pressed);
        assert!(colours(&app, grip).iter().all(|colour| *colour == lit));
        assert_eq!(
            cursor(&app, grip),
            Some(EntityCursor::System(SystemCursorIcon::Grabbing))
        );
    }

    #[test]
    fn new_tokens_restyle_its_marks() {
        let (mut app, grip) = grabber_app(GrabberConfig::grip());
        let tokens = GrabberTokens::with_prefix("game.grip");
        app.world_mut().entity_mut(grip).insert(tokens.clone());
        app.update();
        assert!(
            colours(&app, grip)
                .iter()
                .all(|colour| *colour == tokens.mark)
        );
    }

    #[test]
    fn a_handle_turns_with_its_axis_and_a_ghost_fills_its_parent() {
        let (mut app, handle) = grabber_app(GrabberConfig::handle(GrabAxis::Horizontal));
        assert_eq!(
            cursor(&app, handle),
            Some(EntityCursor::System(SystemCursorIcon::ColResize))
        );
        *app.world_mut().get_mut::<GrabAxis>(handle).unwrap() = GrabAxis::Vertical;
        app.update();
        assert_eq!(
            app.world().get::<Node>(handle).unwrap().height,
            px(HANDLE_STRIP)
        );
        assert_eq!(
            marks(&app, handle),
            [(px(HANDLE_LENGTH), px(HANDLE_THICKNESS))],
            "lying across"
        );
        assert_eq!(
            cursor(&app, handle),
            Some(EntityCursor::System(SystemCursorIcon::RowResize))
        );

        let (app, ghost) = grabber_app(GrabberConfig::ghost());
        let node = app.world().get::<Node>(ghost).unwrap();
        assert_eq!(
            (
                node.position_type,
                node.left,
                node.right,
                node.top,
                node.bottom
            ),
            (PositionType::Absolute, px(0.0), px(0.0), px(0.0), px(0.0))
        );
        assert!(
            app.world().get::<Children>(ghost).is_none(),
            "nothing to see"
        );
    }

    #[test]
    fn a_ghost_shows_its_cursors_and_leaves_them_off_while_disabled() {
        let (mut app, ghost) = grabber_app(GrabberConfig::ghost());
        take_hold(&mut app, ghost);
        app.update();
        assert_eq!(
            cursor(&app, ghost),
            Some(EntityCursor::System(SystemCursorIcon::Grabbing))
        );
        app.world_mut()
            .entity_mut(ghost)
            .insert(InteractionDisabled);
        app.update();
        assert_eq!(cursor(&app, ghost), None, "nothing to grab, so no hand");
    }

    #[test]
    fn a_skin_takes_the_place_of_a_grips_dots_and_gives_it_back() {
        let (mut app, grip) = grabber_app(GrabberConfig::grip());
        // The app's own child stays through the swaps.
        let label = app.world_mut().spawn(Text::new("Move")).id();
        app.world_mut().entity_mut(grip).add_child(label);
        assert_eq!(marks(&app, grip).len(), 6);
        let tokens = GrabberTokens::with_prefix(GRABBER_STYLE);
        let mut theme = app.world_mut().resource_mut::<Theme>();
        theme
            .set_skin(tokens.mark.clone(), Skin::new(Handle::default()))
            .set_size(tokens.skin_size.clone(), 20.0);
        app.update();
        assert_eq!(marks(&app, grip), [(px(20.0), px(20.0))], "one picture");
        assert_eq!(
            app.world().get::<Node>(grip).unwrap().display,
            Display::Flex
        );

        // Held, the picture takes the pressed token like the dots would.
        take_hold(&mut app, grip);
        app.update();
        assert_eq!(
            colours(&app, grip),
            [tokens.mark.clone().with_state(WidgetState::Pressed)]
        );

        app.world_mut()
            .resource_mut::<Theme>()
            .clear_skin(&tokens.mark);
        app.update();
        assert_eq!(marks(&app, grip).len(), 6, "the dots again");
        assert_eq!(
            app.world().get::<Node>(grip).unwrap().display,
            Display::Grid
        );
        assert!(app.world().get_entity(label).is_ok());
    }

    #[test]
    fn a_corner_follows_the_grips_colours_but_not_its_skin() {
        let (mut app, corner) = grabber_app(GrabberConfig::corner(GrabCorner::TopLeft));
        let (grip, own) = (
            GrabberTokens::with_prefix(GRABBER_STYLE),
            GrabberTokens::with_prefix(GRABBER_CORNER_STYLE),
        );
        let mut theme = app.world_mut().resource_mut::<Theme>();
        theme
            .set_color(grip.mark.clone(), Color::WHITE)
            .set_skin(grip.mark.clone(), Skin::new(Handle::default()));
        assert_eq!(theme.color(&own.mark), Color::WHITE);
        app.update();
        assert_eq!(marks(&app, corner).len(), 9, "still dots");

        let mut theme = app.world_mut().resource_mut::<Theme>();
        theme.set_skin(own.mark.clone(), Skin::new(Handle::default()));
        app.update();
        assert_eq!(
            marks(&app, corner),
            [(px(DEFAULT_SKIN_SIZE), px(DEFAULT_SKIN_SIZE))],
            "its own skin, at the grip's size"
        );
        let picture = app.world().get::<Marks>(corner).unwrap().0[0];
        assert_ne!(
            app.world().get::<Visibility>(picture),
            Some(&Visibility::Hidden),
            "not mistaken for a dot off the diagonal"
        );
    }

    #[test]
    fn a_corner_drags_both_ways_and_its_dots_and_cursor_face_its_corner() {
        let (mut app, corner) = grabber_app(GrabberConfig::corner(GrabCorner::BottomRight));
        let shown = |app: &App| -> Vec<bool> {
            let world = app.world();
            world
                .get::<Marks>(corner)
                .unwrap()
                .0
                .iter()
                .map(|&dot| world.get::<Visibility>(dot) != Some(&Visibility::Hidden))
                .collect()
        };
        #[rustfmt::skip]
        assert_eq!(shown(&app), [
            false, false, true,
            false, true,  true,
            true,  true,  true,
        ], "a triangle into the bottom right");
        assert_eq!(
            cursor(&app, corner),
            Some(EntityCursor::System(SystemCursorIcon::NwseResize))
        );

        *app.world_mut().get_mut::<GrabCorner>(corner).unwrap() = GrabCorner::TopRight;
        app.update();
        #[rustfmt::skip]
        assert_eq!(shown(&app), [
            true,  true,  true,
            false, true,  true,
            false, false, true,
        ], "turned into the top right");
        assert_eq!(
            cursor(&app, corner),
            Some(EntityCursor::System(SystemCursorIcon::NeswResize))
        );

        drag(&mut app, corner, &[Vec2::new(10.0, -6.0)]);
        assert_eq!(
            heard(&mut app).last(),
            Some(&(GrabPhase::End, Vec2::new(10.0, -6.0)))
        );
    }
}
