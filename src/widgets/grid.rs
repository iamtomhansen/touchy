//! A grid: choices laid out in rows and columns, one of which is chosen.
//!
//! Options are [choices](crate::choice), filled in row by row, so a cell can
//! hold anything touchy can draw.

use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input_focus::FocusedInput;
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui::{InteractionDisabled, UiSystems};

use crate::EnsurePlugin;
use crate::choice::{
    Choice, ChoiceItem, ChoicePlugin, ChoiceSet, ChoiceState, ChoiceSystems, Chosen, Pick, Step,
};
use crate::cursor::{HoverCursor, HoverCursorPlugin};
use crate::motion::{ResponseCurve, Tween};
use crate::primitives::Unstretched;
use crate::sanitize::at_least;
use crate::theme::palette::{
    ACCENT, CONTROL_CORNER_RADIUS, EDGE, FONT, HIGHLIGHT, SMALL_SIZE, SURFACE, TEXT, TEXT_MUTED,
};
use crate::theme::skins;
use crate::theme::{
    FocusVisible, Shade, Theme, ThemeCornerRadius, ThemeFont, ThemeFontSize, ThemePlugin,
    ThemeSkin, ThemeTextColor, ThemeToken, WidgetState,
};
use crate::widgets::frame::{FramedControl, frame, light_focused_frames};

/// Prefix of the grid selection widget's theme tokens.
pub const GRID_STYLE: &str = "touchy.grid";

/// A cell's size, in pixels, unless the config says otherwise.
const DEFAULT_CELL_SIZE: Vec2 = Vec2::new(96.0, 36.0);
/// Space between cells, and inside the grid's edge.
const GAP: f32 = 2.0;
const BORDER: f32 = 1.0;
/// Space inside a cell, so its words don't touch its edges.
const CELL_PADDING: f32 = 4.0;
/// How long the highlight takes to slide to a new choice, when it slides.
const SLIDE_SECONDS: f32 = 0.16;
/// A hovered cell's tint: the palette text colour, barely there.
const CELL_HOVER: Shade = Shade::Alpha(0.06);

/// Theme tokens for a grid selection widget. Insert new ones to restyle a grid.
#[derive(Component, Clone, PartialEq, Debug)]
#[component(immutable)]
pub struct GridTokens {
    /// Behind the cells, showing between them.
    pub bg: ThemeToken,
    /// The border of the grid, around the track and cells.
    pub border: ThemeToken,
    /// A cell at rest: usually clear, so the track shows through.
    pub cell: ThemeToken,
    /// The highlight behind the chosen cell.
    pub chosen: ThemeToken,
    /// The words on a cell at rest, faded while it's disabled.
    pub text: ThemeToken,
    /// Words on the chosen cell.
    pub text_chosen: ThemeToken,
    /// The font for the words on the cells.
    pub font: ThemeToken,
    /// The font size for the words on the cells.
    pub font_size: ThemeToken,
    /// The corner radius of the cells and highlight.
    pub corner_radius: ThemeToken,
}

/// The control's border frames it, and lights up while it shows the focus.
impl FramedControl for GridTokens {
    fn frame(&self) -> &ThemeToken {
        &self.border
    }
}

impl GridTokens {
    /// Names the tokens under `prefix`.
    pub fn with_prefix(prefix: &str) -> Self {
        let token = |suffix: &str| ThemeToken::new(format!("{prefix}.{suffix}"));
        Self {
            bg: token("bg"),
            border: token("border"),
            cell: token("cell"),
            chosen: token("chosen"),
            text: token("text"),
            text_chosen: token("text_chosen"),
            font: token("font"),
            font_size: token("font_size"),
            corner_radius: token("corner_radius"),
        }
    }

    /// Derives these tokens from the palette, unless the app sets them. The
    /// chosen cell's words are whichever of black or white reads best on it.
    pub fn register_theme_defaults(&self, theme: &mut Theme) {
        // A clear cell can't be shaded lighter, so its hover tint is its own rule.
        let hovered_cell = self.cell.clone().with_state(WidgetState::Hover);
        theme
            .default_derived(self.bg.clone(), SURFACE, Shade::Same)
            .default_derived(self.border.clone(), EDGE, Shade::Same)
            .default_derived(
                self.border.clone().with_state(WidgetState::Focused),
                HIGHLIGHT,
                Shade::Same,
            )
            .default_color(self.cell.clone(), Color::NONE)
            .default_derived(hovered_cell, TEXT, CELL_HOVER)
            .default_derived(self.chosen.clone(), ACCENT, Shade::Same)
            .default_derived(self.text.clone(), TEXT_MUTED, Shade::Same)
            .default_derived(
                self.text_chosen.clone(),
                self.chosen.clone(),
                Shade::Contrast,
            )
            .default_font_from(self.font.clone(), FONT)
            .default_size_from(self.font_size.clone(), SMALL_SIZE)
            .default_size_from(self.corner_radius.clone(), CONTROL_CORNER_RADIUS);
        // Each part draws its role's skin, once a theme gives the role one.
        theme
            .default_skin_from(self.bg.clone(), skins::SUNKEN)
            .default_skin_from(self.chosen.clone(), skins::RAISED);
    }
}

/// The built-in token set.
impl Default for GridTokens {
    fn default() -> Self {
        Self::with_prefix(GRID_STYLE)
    }
}

/// A grid's columns and rows, to turn a chosen index into a place on the grid
/// and back. Cells are numbered row by row from the top left.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct GridShape {
    /// Number of columns in the grid.
    columns: usize,
    /// Number of cells in the grid.
    count: usize,
}

impl GridShape {
    /// Returns the number of columns in the grid.
    pub fn columns(&self) -> usize {
        self.columns
    }

    /// Rows, counting a last one that's only partly filled.
    pub fn rows(&self) -> usize {
        self.count.div_ceil(self.columns)
    }

    /// The column and row of the cell at `index`.
    pub fn cell(&self, index: usize) -> UVec2 {
        UVec2::new((index % self.columns) as u32, (index / self.columns) as u32)
    }

    /// The index of the cell at `column` and `row`, if there's one there.
    pub fn index(&self, cell: UVec2) -> Option<usize> {
        let (column, row) = (cell.x as usize, cell.y as usize);
        let index = row * self.columns + column;
        (column < self.columns && index < self.count).then_some(index)
    }
}

/// Spawn time instructions to make a grid selector.
///
/// Consumed by [`SpawnUi::grid`](crate::spawn_ui::SpawnUi::grid), which returns the grid's entity.
///
/// ```
/// use touchy::prelude::*;
///
/// const SEASONS: [&str; 4] = ["Spring", "Summer", "Autumn", "Winter"];
///
/// let config = GridConfig::new(SEASONS, 2).chosen(1);
/// # assert_eq!((config.columns, config.chosen), (2, Some(1)));
/// ```
#[derive(Clone, Debug)]
pub struct GridConfig {
    /// The options to show in the grid, in order.
    pub options: Vec<Choice>,
    /// Cells across; the choices fill as many rows as they need.
    pub columns: usize,
    /// The index of the chosen cell, if any.
    pub chosen: Option<usize>,
    /// Each cell's size, in pixels.
    pub cell_size: Vec2,
    /// How the highlight slides to a new choice; `None` jumps.
    pub motion: Option<ResponseCurve>,
    /// Whether the grid is disabled at spawn time.
    pub disabled: bool,
    /// The theme tokens to use for the grid.
    pub tokens: GridTokens,
}

impl GridConfig {
    /// A grid of `options`, `columns` across, nothing chosen. Plain words make
    /// plain cells.
    pub fn new<C: Into<Choice>>(options: impl IntoIterator<Item = C>, columns: usize) -> Self {
        Self {
            options: options.into_iter().map(Into::into).collect(),
            columns,
            chosen: None,
            cell_size: DEFAULT_CELL_SIZE,
            motion: None,
            disabled: false,
            tokens: GridTokens::with_prefix(GRID_STYLE),
        }
    }

    /// Starts with the cell at `index` chosen.
    pub fn chosen(mut self, index: usize) -> Self {
        self.chosen = Some(index);
        self
    }

    /// Each cell's size, in pixels.
    pub fn cell_size(mut self, width: f32, height: f32) -> Self {
        self.cell_size = Vec2::new(width, height);
        self
    }

    /// Slides the highlight to each new choice, eased by `curve`.
    pub fn motion(mut self, curve: ResponseCurve) -> Self {
        self.motion = Some(curve);
        self
    }

    /// Sets the grid's disabled state at spawn time.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Sets the theme tokens to use for the grid.
    pub fn tokens(mut self, tokens: GridTokens) -> Self {
        self.tokens = tokens;
        self
    }

    /// This config with every value made usable, warning about each repair.
    fn sanitized(mut self) -> Self {
        self.columns = at_least(self.columns, 1, "GridConfig columns");
        self.cell_size = at_least(self.cell_size, Vec2::ZERO, "GridConfig cell size");
        self
    }
}

/// The parts of a grid, on the grid.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub struct GridParts {
    /// The highlight behind the chosen cell.
    pub highlight: Entity,
    /// The cells, one for each choice, row by row.
    pub cells: Vec<GridCell>,
}

/// One cell of a grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GridCell {
    /// The cell.
    pub cell: Entity,
    /// The choice's label in it, unless the choice builds its own look.
    pub label: Option<Entity>,
}

/// A grid's highlight: where it is, in cells across and down, and its slide.
#[derive(Component, Clone, Debug)]
struct Grid {
    cell_size: Vec2,
    motion: Option<ResponseCurve>,
    /// Where the highlight is, and its slide to where it's going.
    lit_at: Option<Vec2>,
    sliding: Option<Tween<Vec2>>,
}

impl Grid {
    /// Where the cell at `at` (in cells across and down) sits, from the grid's
    /// padding edge, as an absolutely placed node is measured.
    fn place(&self, at: Vec2) -> Vec2 {
        Vec2::splat(GAP) + at * (self.cell_size + GAP)
    }
}

/// What a key does to a focused grid. The one place its keys are read, for
/// the key mapping to take over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GridKey {
    /// Chooses the next cell that can be, this way along the row or column.
    Toward(IVec2),
    /// Chooses the first cell that can be.
    First,
    /// Chooses the last cell that can be.
    Last,
}

impl GridKey {
    fn of(key: &Key) -> Option<Self> {
        match key {
            Key::ArrowLeft => Some(Self::Toward(IVec2::NEG_X)),
            Key::ArrowRight => Some(Self::Toward(IVec2::X)),
            Key::ArrowUp => Some(Self::Toward(IVec2::NEG_Y)),
            Key::ArrowDown => Some(Self::Toward(IVec2::Y)),
            Key::Home => Some(Self::First),
            Key::End => Some(Self::Last),
            _ => None,
        }
    }
}

/// Spawns it; documented on [`SpawnUi::grid`](crate::spawn_ui::SpawnUi::grid).
pub(crate) fn spawn_grid<'a>(
    parent: &'a mut ChildSpawnerCommands,
    config: GridConfig,
) -> EntityCommands<'a> {
    let GridConfig {
        options: choices,
        columns,
        chosen,
        cell_size,
        motion,
        disabled,
        tokens,
    } = config.sanitized();
    let chosen = chosen.filter(|&index| index < choices.len());
    let shape = GridShape {
        columns,
        count: choices.len(),
    };
    let mut grid = parent.spawn((
        Node {
            display: Display::Grid,
            justify_self: JustifySelf::Start,
            grid_template_columns: RepeatedGridTrack::px(columns as u16, cell_size.x),
            grid_auto_rows: vec![GridTrack::px(cell_size.y)],
            row_gap: px(GAP),
            column_gap: px(GAP),
            padding: UiRect::all(px(GAP)),
            border: UiRect::all(px(BORDER)),
            ..default()
        },
        Unstretched,
        shape,
        Chosen(chosen),
        Hovered::default(),
        TabIndex(0),
    ));
    let id = grid.id();
    let mut parts = GridParts {
        highlight: Entity::PLACEHOLDER,
        cells: Vec::new(),
    };
    grid.with_children(|grid| {
        // First, so it's drawn behind the cells.
        parts.highlight = grid
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    width: px(cell_size.x),
                    height: px(cell_size.y),
                    ..default()
                },
                Visibility::Hidden,
                Pickable::IGNORE,
            ))
            .id();
        for (index, choice) in choices.iter().enumerate() {
            let mut cell = grid.spawn((
                Node {
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    padding: UiRect::all(px(CELL_PADDING)),
                    ..default()
                },
                ChoiceItem { set: id, index },
            ));
            if !choice.is_disabled() {
                cell.insert(HoverCursor::POINTER);
            }
            let mut label = None;
            cell.with_children(|cell| {
                label = choice.build(cell, |label| {
                    (Text::new(label), TextLayout::justify(Justify::Center))
                });
            });
            parts.cells.push(GridCell {
                cell: cell.id(),
                label,
            });
        }
    });
    let highlight = Grid {
        cell_size,
        motion,
        lit_at: None,
        sliding: None,
    };
    // Its look comes with its tokens, in `style_grid`, once its parts are in.
    grid.insert((ChoiceSet::new(choices), highlight, parts, tokens));
    // After its parts: its cells are disabled with it, in `disable_cells`.
    if disabled {
        grid.insert(InteractionDisabled);
    }
    grid
}

/// Disabling a grid disables its cells too, so the cursor over them says so.
fn disable_cells(
    insert: On<Insert<InteractionDisabled>>,
    grids: Query<&GridParts>,
    mut commands: Commands,
) {
    if let Ok(parts) = grids.get(insert.entity) {
        for cell in &parts.cells {
            commands.entity(cell.cell).insert(InteractionDisabled);
        }
    }
}

/// Enabling a grid enables its cells again.
fn enable_cells(
    remove: On<Remove<InteractionDisabled>>,
    grids: Query<&GridParts>,
    mut commands: Commands,
) {
    if let Ok(parts) = grids.get(remove.entity) {
        for cell in &parts.cells {
            commands.entity(cell.cell).remove::<InteractionDisabled>();
        }
    }
}

/// From the cell at `from`, the next cell `toward` that can be picked, along
/// that row or column; `from` itself at the edge, or the first that can be
/// picked if nothing's chosen.
fn walk(
    choices: &ChoiceSet,
    shape: &GridShape,
    from: Option<usize>,
    toward: IVec2,
) -> Option<usize> {
    let Some(from) = from else {
        return (0..choices.len()).find(|&index| choices.can_pick(index));
    };
    let mut at = shape.cell(from).as_ivec2();
    loop {
        at += toward;
        if at.cmplt(IVec2::ZERO).any() {
            return Some(from);
        }
        match shape.index(at.as_uvec2()) {
            Some(index) if choices.can_pick(index) => return Some(index),
            Some(_) => continue,
            // Past the edge, or into the gap after a last row that's short.
            None if at.x as usize >= shape.columns || at.y as usize >= shape.rows() => {
                return Some(from);
            }
            None => continue,
        }
    }
}

/// The arrow keys, while the grid has the focus, choose the next cell their
/// way; Home and End the first and last.
fn work_with_keys(
    mut keys: On<FocusedInput<KeyboardInput>>,
    mut grids: Query<(&mut ChoiceSet, &Chosen, &GridShape), Without<InteractionDisabled>>,
    mut commands: Commands,
) {
    let grid = keys.focused_entity;
    let Ok((mut choices, chosen, shape)) = grids.get_mut(grid) else {
        return;
    };
    if !keys.input.state.is_pressed() {
        return;
    }
    let Some(key) = GridKey::of(&keys.input.logical_key) else {
        return;
    };
    keys.propagate(false);
    let to = match key {
        GridKey::First => choices.step(Step::First),
        GridKey::Last => choices.step(Step::Last),
        GridKey::Toward(toward) => walk(&choices, shape, chosen.0, toward),
    };
    if let Some(index) = to {
        choices.highlight(Some(index));
        commands.trigger(Pick {
            set: grid,
            index: Some(index),
        });
    }
}

/// When the pointer leaves a grid, its highlight goes back to the chosen cell,
/// so cells that show being highlighted don't stay lit behind it.
fn settle_on_leaving(mut grids: Query<(&mut ChoiceSet, &Chosen, &Hovered), Changed<Hovered>>) {
    for (mut choices, chosen, hovered) in &mut grids {
        if !hovered.0 && choices.highlighted() != chosen.0 {
            choices.highlight(chosen.0);
        }
    }
}

/// Sends each grid's highlight to its choice: sliding there if the grid has
/// motion and the highlight was already showing, otherwise straight there.
fn follow_choice(mut grids: Query<(&mut Grid, &Chosen, &GridShape), Changed<Chosen>>) {
    for (mut grid, chosen, shape) in &mut grids {
        let to = chosen.0.map(|index| shape.cell(index).as_vec2());
        grid.sliding = match (grid.motion, grid.lit_at, to) {
            (Some(curve), Some(from), Some(to)) if from != to => {
                Some(Tween::new(from, to, SLIDE_SECONDS, curve))
            }
            _ => {
                grid.lit_at = to;
                None
            }
        };
    }
}

/// Slides each sliding highlight along. Real time, so it finishes while the
/// game is paused.
fn slide_highlights(time: Res<Time<Real>>, mut grids: Query<&mut Grid>) {
    // Only sliding ones are touched, so resting ones aren't marked changed.
    for mut grid in grids.iter_mut().filter(|grid| grid.sliding.is_some()) {
        let Some(mut tween) = grid.sliding else {
            continue;
        };
        let (at, arrived) = tween.advance(time.delta_secs());
        grid.lit_at = Some(at);
        grid.sliding = (!arrived).then_some(tween);
    }
}

/// Puts each highlight where its grid says, hidden while nothing's chosen.
fn place_highlights(
    grids: Query<(&Grid, &GridParts), Changed<Grid>>,
    mut highlights: Query<(&mut Node, &mut Visibility)>,
) {
    for (grid, parts) in &grids {
        let Ok((mut node, mut visibility)) = highlights.get_mut(parts.highlight) else {
            continue;
        };
        let Some(at) = grid.lit_at else {
            visibility.set_if_neq(Visibility::Hidden);
            continue;
        };
        visibility.set_if_neq(Visibility::Inherited);
        let place = grid.place(at);
        let (left, top) = (px(place.x), px(place.y));
        if node.left != left || node.top != top {
            node.left = left;
            node.top = top;
        }
    }
}

/// A cell's background and words in its state: tinted while the pointer's
/// over it, and its words lit while it's chosen.
fn cell_look(
    tokens: &GridTokens,
    state: &ChoiceState,
    hovered: bool,
) -> (ThemeSkin, ThemeTextColor) {
    let bg = if state.highlighted && hovered && !state.chosen && !state.disabled {
        tokens.cell.clone().with_state(WidgetState::Hover)
    } else {
        tokens.cell.clone()
    };
    let words = if state.disabled {
        tokens.text.clone().with_state(WidgetState::Disabled)
    } else if state.chosen {
        tokens.text_chosen.clone()
    } else {
        tokens.text.clone()
    };
    (ThemeSkin(bg), ThemeTextColor(words))
}

/// Restyles each grid's cells by their states: the one the pointer's over
/// tinted while it's over the grid, and their words lit on the chosen one.
fn restyle_grids(
    grids: Query<(&GridParts, &GridTokens, &Hovered)>,
    cells: Query<(&ChoiceState, &ThemeSkin)>,
    labels: Query<&ThemeTextColor>,
    mut commands: Commands,
) {
    for (parts, tokens, hovered) in &grids {
        for cell in &parts.cells {
            let Ok((state, had)) = cells.get(cell.cell) else {
                continue;
            };
            let (bg, words) = cell_look(tokens, state, hovered.0);
            if *had != bg {
                commands.entity(cell.cell).insert(bg);
            }
            if let Some(label) = cell.label
                && labels.get(label).is_ok_and(|old| *old != words)
            {
                commands.entity(label).insert(words);
            }
        }
    }
}

/// Styles a grid when its tokens are set: at spawn, and whenever the app sets new ones.
fn style_grid(
    insert: On<Insert<GridTokens>>,
    grids: Query<(&GridTokens, &GridParts, &Hovered, Has<FocusVisible>)>,
    states: Query<&ChoiceState>,
    mut commands: Commands,
) {
    let Ok((tokens, parts, hovered, focused)) = grids.get(insert.entity) else {
        return;
    };
    let rounded = ThemeCornerRadius(tokens.corner_radius.clone());
    commands.entity(insert.entity).insert((
        ThemeSkin(tokens.bg.clone()),
        frame(tokens, focused),
        rounded.clone(),
    ));
    commands
        .entity(parts.highlight)
        .insert((ThemeSkin(tokens.chosen.clone()), rounded.clone()));
    let font = (
        ThemeFontSize(tokens.font_size.clone()),
        ThemeFont(tokens.font.clone()),
    );
    for cell in &parts.cells {
        // A cell's state comes with the choices, a frame after it's spawned.
        let state = states.get(cell.cell).copied().unwrap_or_default();
        let (bg, words) = cell_look(tokens, &state, hovered.0);
        commands.entity(cell.cell).insert((bg, rounded.clone()));
        if let Some(label) = cell.label {
            commands.entity(label).insert((words, font.clone()));
        }
    }
}

/// Grids: choosing by pointer and keys, and default token values.
pub struct GridWidgetPlugin;

impl Plugin for GridWidgetPlugin {
    fn build(&self, app: &mut App) {
        app.ensure_plugin(ThemePlugin)
            .ensure_plugin(HoverCursorPlugin)
            .ensure_plugin(ChoicePlugin)
            .add_observer(work_with_keys)
            .add_observer(disable_cells)
            .add_observer(enable_cells)
            .add_observer(style_grid)
            .add_systems(Update, slide_highlights)
            .add_systems(
                PostUpdate,
                light_focused_frames::<GridTokens>.before(UiSystems::Prepare),
            )
            .add_systems(
                PostUpdate,
                (
                    (follow_choice, place_highlights).chain(),
                    settle_on_leaving.before(ChoiceSystems),
                    restyle_grids.after(ChoiceSystems),
                )
                    .before(UiSystems::Prepare),
            );
        GridTokens::with_prefix(GRID_STYLE)
            .register_theme_defaults(&mut app.world_mut().resource_mut::<Theme>());
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy::camera::NormalizedRenderTarget;
    use bevy::input::ButtonState;
    use bevy::input::keyboard::KeyCode;
    use bevy::picking::backend::HitData;
    use bevy::picking::cursor::EntityCursor;
    use bevy::picking::events::{Pointer, PointerClick};
    use bevy::picking::pointer::{Location, PointerButton, PointerId};
    use bevy::time::TimeUpdateStrategy;
    use bevy::ui_widgets::ValueChange;
    use bevy::window::SystemCursorIcon;

    use super::*;
    use crate::theme::ThemeBorder;

    #[derive(Resource, Default)]
    struct Announced(Vec<usize>);

    /// Each test frame's length, so a slide takes a few of them.
    const FRAME: Duration = Duration::from_millis(50);

    fn grid_app(config: GridConfig) -> (App, Entity) {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, GridWidgetPlugin))
            .insert_resource(TimeUpdateStrategy::ManualDuration(FRAME))
            .init_resource::<Announced>()
            .add_observer(
                |change: On<ValueChange<usize>>, mut announced: ResMut<Announced>| {
                    announced.0.push(change.value);
                },
            );
        app.world_mut().register_component::<Window>();
        let mut grid = None;
        app.world_mut()
            .commands()
            .spawn(Node::default())
            .with_children(|parent| grid = Some(spawn_grid(parent, config).id()));
        app.update();
        (app, grid.unwrap())
    }

    fn press(app: &mut App, grid: Entity, key: Key) {
        let keyboard = KeyboardInput {
            key_code: KeyCode::Unidentified(bevy::input::keyboard::NativeKeyCode::Unidentified),
            logical_key: key,
            state: ButtonState::Pressed,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        };
        app.world_mut()
            .trigger(FocusedInput::new(grid, keyboard, Entity::PLACEHOLDER));
        app.update();
    }

    fn parts(app: &App, grid: Entity) -> GridParts {
        app.world().get::<GridParts>(grid).unwrap().clone()
    }

    fn chosen(app: &App, grid: Entity) -> Option<usize> {
        app.world().get::<Chosen>(grid).unwrap().0
    }

    fn announced(app: &mut App) -> Vec<usize> {
        std::mem::take(&mut app.world_mut().resource_mut::<Announced>().0)
    }

    /// Three across and three down, the middle disabled.
    fn alignment() -> GridConfig {
        let mut cells: Vec<Choice> = (0..9)
            .map(|index| Choice::text(index.to_string()))
            .collect();
        cells[4] = Choice::text("4").disabled(true);
        GridConfig::new(cells, 3)
    }

    #[test]
    fn a_shape_turns_indices_into_places_and_back() {
        let shape = GridShape {
            columns: 3,
            count: 7,
        };
        assert_eq!(shape.rows(), 3, "a short last row still counts");
        assert_eq!(shape.cell(5), UVec2::new(2, 1));
        assert_eq!(shape.index(UVec2::new(2, 1)), Some(5));
        assert_eq!(shape.index(UVec2::new(1, 2)), None, "past the last cell");
        assert_eq!(shape.index(UVec2::new(3, 0)), None, "past the edge");
    }

    #[test]
    fn the_arrows_choose_their_way_skipping_disabled_cells_and_stopping_at_edges() {
        let (mut app, grid) = grid_app(alignment());
        press(&mut app, grid, Key::ArrowRight);
        assert_eq!(chosen(&app, grid), Some(0), "from nothing, the first");
        press(&mut app, grid, Key::ArrowDown);
        assert_eq!(chosen(&app, grid), Some(3));
        press(&mut app, grid, Key::ArrowRight);
        assert_eq!(chosen(&app, grid), Some(5), "over the disabled middle");
        press(&mut app, grid, Key::ArrowRight);
        assert_eq!(chosen(&app, grid), Some(5), "at the edge, it stays");
        press(&mut app, grid, Key::ArrowDown);
        press(&mut app, grid, Key::ArrowDown);
        assert_eq!(chosen(&app, grid), Some(8));
        press(&mut app, grid, Key::Home);
        assert_eq!(chosen(&app, grid), Some(0));
        assert_eq!(announced(&mut app), [0, 3, 5, 8, 0]);
    }

    #[test]
    fn moving_toward_a_short_last_row_stops_where_there_are_no_cells() {
        let (mut app, grid) = grid_app(GridConfig::new(["a", "b", "c", "d"], 3).chosen(2));
        press(&mut app, grid, Key::ArrowDown);
        assert_eq!(chosen(&app, grid), Some(2), "nothing below the last column");
        press(&mut app, grid, Key::ArrowLeft);
        press(&mut app, grid, Key::ArrowLeft);
        press(&mut app, grid, Key::ArrowDown);
        assert_eq!(chosen(&app, grid), Some(3));
    }

    #[test]
    fn clicking_a_cell_chooses_it_and_lights_it() {
        let (mut app, grid) = grid_app(alignment());
        let cell = parts(&app, grid).cells[7].cell;
        app.world_mut().trigger(PointerClick {
            entity: cell,
            pointer: Pointer::new(
                PointerId::Mouse,
                Location {
                    target: NormalizedRenderTarget::None {
                        width: 1,
                        height: 1,
                    },
                    position: Vec2::ZERO,
                },
            ),
            button: PointerButton::Primary,
            hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
            duration: Duration::ZERO,
            count: 1,
        });
        app.update();
        assert_eq!(chosen(&app, grid), Some(7));
        assert_eq!(
            lit(&app, grid),
            Some((px(GAP + 98.0), px(GAP + 2.0 * 38.0)))
        );
    }

    /// Where a grid's highlight is, if it's showing.
    fn lit(app: &App, grid: Entity) -> Option<(Val, Val)> {
        let highlight = parts(app, grid).highlight;
        let node = app.world().get::<Node>(highlight).unwrap();
        let shown = app.world().get::<Visibility>(highlight) != Some(&Visibility::Hidden);
        shown.then_some((node.left, node.top))
    }

    #[test]
    fn with_motion_the_highlight_slides_across_both_axes_and_lands() {
        let config = GridConfig::new(["a", "b", "c", "d"], 2)
            .cell_size(100.0, 50.0)
            .chosen(0)
            .motion(ResponseCurve::Power(2.0));
        let (mut app, grid) = grid_app(config);
        assert_eq!(lit(&app, grid), Some((px(GAP), px(GAP))), "starts in place");
        app.world_mut().entity_mut(grid).insert(Chosen(Some(3)));
        app.update();
        app.update();
        let Some((Val::Px(left), Val::Px(top))) = lit(&app, grid) else {
            panic!("a placed highlight");
        };
        assert!(
            left > GAP && left < GAP + 102.0,
            "on its way across, at {left}"
        );
        assert!(top > GAP && top < GAP + 52.0, "and down, at {top}");
        for _ in 0..10 {
            app.update();
        }
        assert_eq!(lit(&app, grid), Some((px(GAP + 102.0), px(GAP + 52.0))));
    }

    #[test]
    fn with_nothing_chosen_the_highlight_hides() {
        let (app, grid) = grid_app(alignment());
        assert_eq!(lit(&app, grid), None);
    }

    #[test]
    fn a_disabled_grid_ignores_the_keys_and_says_so() {
        let (mut app, grid) = grid_app(alignment().disabled(true));
        press(&mut app, grid, Key::ArrowRight);
        assert_eq!(chosen(&app, grid), None);
        let cell = parts(&app, grid).cells[0].cell;
        let not_allowed = EntityCursor::System(SystemCursorIcon::NotAllowed);
        assert_eq!(app.world().get::<EntityCursor>(cell), Some(&not_allowed));
    }

    #[test]
    fn new_tokens_restyle_the_grid_and_its_cells_in_their_states() {
        let (mut app, grid) = grid_app(alignment().chosen(0));
        let tokens = GridTokens::with_prefix("game.board");
        app.world_mut().entity_mut(grid).insert(tokens.clone());
        app.update();

        let parts = parts(&app, grid);
        let world = app.world();
        let background = |entity| world.get::<ThemeSkin>(entity).unwrap().0.clone();
        let words = |cell: usize| {
            let label = parts.cells[cell].label.unwrap();
            world.get::<ThemeTextColor>(label).unwrap().0.clone()
        };
        assert_eq!(background(grid), tokens.bg);
        assert_eq!(background(parts.highlight), tokens.chosen);
        assert_eq!(words(0), tokens.text_chosen);
        assert_eq!(words(1), tokens.text);
        assert_eq!(
            words(4),
            tokens.text.clone().with_state(WidgetState::Disabled)
        );
    }

    /// A grid's cells don't light up for the focus, so its frame does.
    #[test]
    fn a_focused_grid_lights_its_frame() {
        let (mut app, grid) = grid_app(GridConfig::new(["a", "b"], 2));
        let tokens = GridTokens::with_prefix(GRID_STYLE);
        let frame = |app: &App| app.world().get::<ThemeBorder>(grid).unwrap().top.clone();
        assert_eq!(frame(&app), tokens.border);
        app.world_mut().entity_mut(grid).insert(FocusVisible);
        app.update();
        assert_eq!(
            frame(&app),
            tokens.border.clone().with_state(WidgetState::Focused)
        );
        app.world_mut().entity_mut(grid).remove::<FocusVisible>();
        app.update();
        assert_eq!(frame(&app), tokens.border);
    }
}
