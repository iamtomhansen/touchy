//! A segmented control: a row of options, one of which is selected.
//!
//! Options are [choices](crate::choice), so a segment can show plain words or
//! anything a closure builds, and its [`ChoiceState`](crate::choice::ChoiceState)
//! lets that respond to being selected.
//!
//! Segments can be equal-width, or content-width ([`SegmentWidths`]).

use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input_focus::FocusedInput;
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui::{InteractionDisabled, UiSystems};

use crate::EnsurePlugin;
use crate::choice::{Choice, ChoiceItem, ChoicePlugin, ChoiceSet, Chosen, Pick, Step};
use crate::cursor::{HoverCursor, HoverCursorPlugin};
use crate::motion::{ResponseCurve, Tween};
use crate::theme::palette::{
    ACCENT, CONTROL_CORNER_RADIUS, EDGE, FONT, HIGHLIGHT, SMALL_SIZE, SURFACE, TEXT, TEXT_MUTED,
};
use crate::theme::skins;
use crate::theme::{
    FocusVisible, Shade, Theme, ThemeCornerRadius, ThemeFont, ThemeFontSize, ThemePlugin,
    ThemeSkin, ThemeTextColor, ThemeToken, WidgetFlags, WidgetState,
};
use crate::widgets::frame::{FramedControl, frame, light_focused_frames};

/// Prefix of the segmented selection widget's theme tokens.
pub const SEGMENTED_STYLE: &str = "touchy.segmented";

/// A hovered segment's tint: the palette text colour, barely there.
const SEGMENT_HOVER: Shade = Shade::Alpha(0.06);
/// How long the highlight takes to slide to a new selection, when it slides.
const SLIDE_DURATION: f32 = 0.16;
/// The control's height, its border's width, and the space either side of a
/// segment's label, in pixels.
const HEIGHT: f32 = 28.0;
const BORDER: f32 = 1.0;
const SEGMENT_PADDING: f32 = 10.0;

/// How the selection is shown. The output is the same index either way.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Highlight {
    /// Light only the selected segment.
    #[default]
    Single,
    /// Fill every segment from the first up to and including the selected one.
    Cumulative,
}

/// How wide each segment is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SegmentWidths {
    /// All the same width, for options of equal standing. A label too long
    /// for its share still fits: its segment grows and the others give way.
    #[default]
    Equal,
    /// Each as wide as its label, sharing any spare room evenly.
    FitLabels,
}

/// Theme tokens for a segmented selection widget. Insert new ones to restyle a control.
#[derive(Component, Clone, PartialEq, Debug)]
#[component(immutable)]
pub struct SegmentedTokens {
    /// The control's background, behind the segments and the highlight.
    pub bg: ThemeToken,
    /// The control's border, around the track and segments.
    pub border: ThemeToken,
    /// The highlight behind the lit segments.
    pub highlight: ThemeToken,
    /// Behind each segment: usually clear, so the track and highlight show
    /// through; tinted while hovered.
    pub segment: ThemeToken,
    /// Text on a segment, faded while it's disabled.
    pub text: ThemeToken,
    /// Text on a lit segment.
    pub text_lit: ThemeToken,
    /// The font size of a segment's label.
    pub font_size: ThemeToken,
    /// The font of a segment's label.
    pub font: ThemeToken,
    /// The corner radius of the control and highlight.
    pub corner_radius: ThemeToken,
}

/// The control's border frames it, and lights up while it shows the focus.
impl FramedControl for SegmentedTokens {
    fn frame(&self) -> &ThemeToken {
        &self.border
    }
}

impl SegmentedTokens {
    /// Names the tokens under `prefix`.
    pub fn with_prefix(prefix: &str) -> Self {
        let token = |suffix: &str| ThemeToken::new(format!("{prefix}.{suffix}"));
        Self {
            bg: token("bg"),
            border: token("border"),
            highlight: token("highlight"),
            segment: token("segment"),
            text: token("text"),
            text_lit: token("text_lit"),
            font_size: token("font_size"),
            font: token("font"),
            corner_radius: token("corner_radius"),
        }
    }

    /// Derives these tokens from the palette, unless the app sets them. Lit
    /// text is whichever of black or white reads best on the highlight.
    pub fn register_theme_defaults(&self, theme: &mut Theme) {
        // A clear segment can't be shaded lighter, so its hover tint is its own rule.
        let hovered_segment = self.segment.clone().with_state(WidgetState::Hover);
        theme
            .default_derived(self.bg.clone(), SURFACE, Shade::Same)
            .default_derived(self.border.clone(), EDGE, Shade::Same)
            .default_derived(
                self.border.clone().with_state(WidgetState::Focused),
                HIGHLIGHT,
                Shade::Same,
            )
            .default_derived(self.highlight.clone(), ACCENT, Shade::Same)
            .default_color(self.segment.clone(), Color::NONE)
            .default_derived(hovered_segment, TEXT, SEGMENT_HOVER)
            .default_derived(self.text.clone(), TEXT_MUTED, Shade::Same)
            .default_derived(
                self.text_lit.clone(),
                self.highlight.clone(),
                Shade::Contrast,
            )
            .default_font_from(self.font.clone(), FONT)
            .default_size_from(self.font_size.clone(), SMALL_SIZE)
            .default_size_from(self.corner_radius.clone(), CONTROL_CORNER_RADIUS);
        // Each part draws its role's skin, once a theme gives the role one.
        theme
            .default_skin_from(self.bg.clone(), skins::SUNKEN)
            .default_skin_from(self.highlight.clone(), skins::RAISED);
    }

    /// A segment's label colour.
    fn label(&self, lit: bool, disabled: bool) -> ThemeTextColor {
        let token = if disabled {
            self.text.clone().with_state(WidgetState::Disabled)
        } else if lit {
            self.text_lit.clone()
        } else {
            self.text.clone()
        };
        ThemeTextColor(token)
    }

    /// A segment's background: tinted while hovered, unless it's disabled.
    fn segment(&self, hovered: bool, disabled: bool) -> ThemeSkin {
        let state = WidgetFlags {
            hovered,
            disabled,
            ..default()
        }
        .state();
        ThemeSkin(self.segment.clone().with_state(state))
    }
}

/// The built-in token set.
impl Default for SegmentedTokens {
    fn default() -> Self {
        Self::with_prefix(SEGMENTED_STYLE)
    }
}

/// Spawn time configuration for a segmented selector widget.
///
/// Consumed by [`SpawnUi::segmented`](crate::spawn_ui::SpawnUi::segmented), which returns the
/// control's entity.
///
/// ```
/// use touchy::prelude::*;
///
/// let config = SegmentedConfig::new(["Low", "Mid", "High"])
///     .chosen(1)
///     .highlight(Highlight::Cumulative);
/// # assert_eq!(config.chosen, Some(1));
/// ```
#[derive(Clone, Debug)]
pub struct SegmentedConfig {
    /// The segments, in order. Each is a [`Choice`], so it can be a plain
    /// word or anything a closure builds.
    pub options: Vec<Choice>,
    /// The segment selected at spawn, or none, so nothing is lit until one's picked.
    pub chosen: Option<usize>,
    /// Whether to highlight a single segment or all segments cumulatively up to the choice.
    pub highlight: Highlight,
    /// Whether the segments are equal width, or fit their labels.
    pub widths: SegmentWidths,
    /// Slide the highlight to a new selection with this easing, or jump if `None`.
    pub motion: Option<ResponseCurve>,
    /// Whether the control starts disabled.
    pub disabled: bool,
    /// The theme tokens to use for the control.
    pub tokens: SegmentedTokens,
}

impl SegmentedConfig {
    /// A control of `options`, the first selected. Plain words make plain
    /// segments, so `SegmentedConfig::new(["Low", "Mid", "High"])` is a whole
    /// control.
    pub fn new<C: Into<Choice>>(options: impl IntoIterator<Item = C>) -> Self {
        Self {
            options: options.into_iter().map(Into::into).collect(),
            chosen: Some(0),
            highlight: Highlight::Single,
            widths: SegmentWidths::Equal,
            motion: None,
            disabled: false,
            tokens: SegmentedTokens::with_prefix(SEGMENTED_STYLE),
        }
    }

    /// Selects the segment at `index` at spawn.
    pub fn chosen(mut self, index: usize) -> Self {
        self.chosen = Some(index);
        self
    }

    /// Replace the options: anything that turns into a list of choices, such
    /// as an array of words, `["Low", "High"]`.
    pub fn options<C: Into<Choice>>(mut self, options: impl IntoIterator<Item = C>) -> Self {
        self.options = options.into_iter().map(Into::into).collect();
        self
    }

    /// Sets how the selection is lit.
    pub fn highlight(mut self, highlight: Highlight) -> Self {
        self.highlight = highlight;
        self
    }

    /// Sets how wide each segment is.
    pub fn widths(mut self, widths: SegmentWidths) -> Self {
        self.widths = widths;
        self
    }

    /// Slides the highlight to each new selection, eased by `curve`.
    pub fn motion(mut self, curve: ResponseCurve) -> Self {
        self.motion = Some(curve);
        self
    }

    /// Sets whether the control starts disabled.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Sets the theme tokens to use for the control.
    pub fn tokens(mut self, tokens: SegmentedTokens) -> Self {
        self.tokens = tokens;
        self
    }

    /// This config with every value made usable, warning about each repair.
    fn sanitized(mut self) -> Self {
        let count = self.options.len();
        if let Some(index) = self.chosen
            && index >= count
        {
            warn!("SegmentedConfig chosen was {index}, past its {count} options; choosing none");
            self.chosen = None;
        }
        self
    }
}

/// The control's shape: how many segments, and how the selection is shown.
#[derive(Component, Clone, Copy, Debug)]
struct Shape {
    count: usize,
    highlight: Highlight,
    motion: Option<ResponseCurve>,
}

impl Shape {
    /// Whether segment `index` is lit when `selected` is selected.
    fn lit(&self, index: usize, selected: Option<usize>) -> bool {
        match (self.highlight, selected) {
            (_, None) => false,
            (Highlight::Single, Some(selected)) => index == selected,
            (Highlight::Cumulative, Some(selected)) => index <= selected,
        }
    }

    /// The highlight's left edge and width, as percentages of the control,
    /// for a fractional segment position. `widths` are the segments' laid-out
    /// widths in order; before the first layout they're unknown (zero), and the
    /// segments are taken to be equal.
    fn highlight_span(&self, position: f32, widths: &[f32]) -> (f32, f32) {
        if self.count == 0 {
            return (0.0, 0.0);
        }
        let edges = segment_edges(self.count, widths);
        // An edge at a fractional index: partway between two real edges.
        let edge_at = |index: f32| {
            let index = index.clamp(0.0, self.count as f32);
            let below = (index.floor() as usize).min(self.count - 1);
            edges[below].lerp(edges[below + 1], index - below as f32)
        };
        let right = edge_at(position + 1.0);
        let left = match self.highlight {
            Highlight::Single => edge_at(position),
            Highlight::Cumulative => 0.0,
        };
        (left * 100.0, (right - left) * 100.0)
    }
}

/// Where each segment starts and ends, as fractions of the control's width:
/// `count + 1` edges from 0 to 1.
fn segment_edges(count: usize, widths: &[f32]) -> Vec<f32> {
    let total: f32 = widths.iter().sum();
    if widths.len() != count || total <= 0.0 {
        return (0..=count).map(|edge| edge as f32 / count as f32).collect();
    }
    let mut edge = 0.0;
    let mut edges = vec![0.0];
    for width in widths {
        edge += width / total;
        edges.push(edge);
    }
    edges
}

/// Where the highlight is drawn, in segments (fractional while sliding), or
/// `None` while nothing's selected.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
struct HighlightPosition(Option<f32>);

/// The highlight sliding to a new selection.
#[derive(Component, Clone, Copy, Debug)]
struct Sliding(Tween<f32>);

/// The parts of a segmented control, on the control.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub struct SegmentedParts {
    /// The highlight behind the lit segments.
    pub highlight: Entity,
    /// The segments, one for each option, in order.
    pub segments: Vec<Segment>,
}

/// One segment of a segmented control.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Segment {
    /// The segment.
    pub segment: Entity,
    /// The option's label in it, unless the option builds its own look.
    pub label: Option<Entity>,
}

/// What a key does to a focused control. The one place its keys are read,
/// for the key mapping to take over.
fn segmented_step(key: &Key) -> Option<Step> {
    match key {
        Key::ArrowLeft | Key::ArrowUp => Some(Step::Previous),
        Key::ArrowRight | Key::ArrowDown => Some(Step::Next),
        Key::Home => Some(Step::First),
        Key::End => Some(Step::Last),
        _ => None,
    }
}

/// Spawns it; documented on [`SpawnUi::segmented`](crate::spawn_ui::SpawnUi::segmented).
pub(crate) fn spawn_segmented<'a>(
    parent: &'a mut ChildSpawnerCommands,
    config: SegmentedConfig,
) -> EntityCommands<'a> {
    let SegmentedConfig {
        options,
        chosen,
        highlight,
        widths,
        motion,
        disabled,
        tokens,
    } = config.sanitized();
    let shape = Shape {
        count: options.len(),
        highlight,
        motion,
    };
    let at = chosen.map(|index| index as f32);
    let (left, width) = at.map_or((0.0, 0.0), |at| shape.highlight_span(at, &[]));
    let (grow, basis) = match widths {
        SegmentWidths::Equal => (1.0, px(0)),
        SegmentWidths::FitLabels => (1.0, Val::Auto),
    };

    let mut control = parent.spawn((
        // Focusable, so the arrow keys work once the app enables tab navigation.
        TabIndex(0),
        shape,
        Chosen(chosen),
        HighlightPosition(at),
        Node {
            height: px(HEIGHT),
            border: UiRect::all(px(BORDER)),
            overflow: Overflow::clip(),
            ..default()
        },
    ));
    let id = control.id();
    let mut parts = SegmentedParts {
        highlight: Entity::PLACEHOLDER,
        segments: Vec::new(),
    };
    control.with_children(|row| {
        // First, so the segments' labels draw over it.
        let span = Node {
            position_type: PositionType::Absolute,
            left: percent(left),
            width: percent(width),
            height: percent(100),
            ..default()
        };
        parts.highlight = row.spawn((Pickable::IGNORE, span)).id();
        for (index, option) in options.iter().enumerate() {
            let mut segment = row.spawn((
                ChoiceItem { set: id, index },
                Hovered::default(),
                HoverCursor::POINTER,
                Node {
                    flex_grow: grow,
                    flex_basis: basis,
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    padding: UiRect::horizontal(px(SEGMENT_PADDING)),
                    ..default()
                },
            ));
            if option.is_disabled() {
                segment.insert(InteractionDisabled);
            }
            let mut label = None;
            segment
                .with_children(|segment| label = option.build(segment, |label| Text::new(label)));
            parts.segments.push(Segment {
                segment: segment.id(),
                label,
            });
        }
    });
    let choices = ChoiceSet::new(options).wrapping(true);
    // Its look comes with its tokens, in `style_segmented`, once its parts are in.
    control.insert((choices, parts, tokens));
    // After its parts: its segments are disabled with it, in `disable_segments`.
    if disabled {
        control.insert(InteractionDisabled);
    }
    control
}

/// Disabling a control disables its segments too, so the cursor over them says so.
fn disable_segments(
    insert: On<Insert<InteractionDisabled>>,
    controls: Query<&SegmentedParts>,
    mut commands: Commands,
) {
    if let Ok(parts) = controls.get(insert.entity) {
        for segment in &parts.segments {
            commands.entity(segment.segment).insert(InteractionDisabled);
        }
    }
}

/// Enabling a control enables its segments again, but for disabled options.
fn enable_segments(
    remove: On<Remove<InteractionDisabled>>,
    controls: Query<(&SegmentedParts, &ChoiceSet)>,
    mut commands: Commands,
) {
    let Ok((parts, choices)) = controls.get(remove.entity) else {
        return;
    };
    for (index, segment) in parts.segments.iter().enumerate() {
        if !choices.get(index).is_some_and(Choice::is_disabled) {
            commands
                .entity(segment.segment)
                .remove::<InteractionDisabled>();
        }
    }
}

/// Controls the keys can change: segmented, and not disabled.
type Steerable = (With<Shape>, Without<InteractionDisabled>);

/// The keys, while the control has the focus: the arrows select the next or
/// previous segment, going round at the ends; Home and End the first and last.
fn work_with_keys(
    mut keys: On<FocusedInput<KeyboardInput>>,
    mut controls: Query<(&mut ChoiceSet, &Chosen), Steerable>,
    mut commands: Commands,
) {
    let control = keys.focused_entity;
    let Ok((mut choices, chosen)) = controls.get_mut(control) else {
        return;
    };
    if !keys.input.state.is_pressed() {
        return;
    }
    let Some(step) = segmented_step(&keys.input.logical_key) else {
        return;
    };
    keys.propagate(false);
    choices.highlight(chosen.0);
    if let Some(index) = choices.step(step) {
        commands.trigger(Pick {
            set: control,
            index: Some(index),
        });
    }
}

/// A control and everything needed to show its selection.
type ControlState = (
    Entity,
    &'static Shape,
    &'static Chosen,
    &'static HighlightPosition,
    &'static SegmentedTokens,
    &'static SegmentedParts,
    &'static ChoiceSet,
);

/// When the selection changes, by a click, the keys or the app: relight the
/// labels, and move the highlight, sliding if the control has motion.
fn follow_selection(
    controls: Query<ControlState, Changed<Chosen>>,
    labels: Query<&ThemeTextColor>,
    mut commands: Commands,
) {
    for (entity, shape, chosen, position, tokens, parts, choices) in &controls {
        for (index, segment) in parts.segments.iter().enumerate() {
            let Some(label) = segment.label else {
                continue;
            };
            let off = choices.get(index).is_some_and(Choice::is_disabled);
            let color = tokens.label(shape.lit(index, chosen.0), off);
            if labels.get(label).is_ok_and(|old| *old != color) {
                commands.entity(label).insert(color);
            }
        }

        let target = chosen.0.map(|index| index as f32);
        if position.0 == target {
            continue;
        }
        match (shape.motion, position.0, target) {
            (Some(curve), Some(from), Some(to)) => {
                let slide = Tween::new(from, to, SLIDE_DURATION, curve);
                commands.entity(entity).insert(Sliding(slide));
            }
            _ => {
                commands
                    .entity(entity)
                    .remove::<Sliding>()
                    .insert(HighlightPosition(target));
            }
        }
    }
}

/// Moves sliding highlights. Real time, so they finish while the game is paused.
fn animate_slides(
    time: Res<Time<Real>>,
    mut controls: Query<(Entity, &mut HighlightPosition, &mut Sliding)>,
    mut commands: Commands,
) {
    for (entity, mut position, mut sliding) in &mut controls {
        let (next, arrived) = sliding.0.advance(time.delta_secs());
        position.0 = Some(next);
        if arrived {
            commands.entity(entity).remove::<Sliding>();
        }
    }
}

/// Draws the highlight at its position, as one segment or a fill from the
/// left, over the segments as they were last laid out, hidden while nothing's
/// selected. Only when the position or a segment's layout changed, since a
/// relayout (new text, a resized window) moves segments without moving the selection.
fn position_highlights(
    controls: Query<(&Shape, Ref<HighlightPosition>, &SegmentedParts)>,
    segments: Query<Ref<ComputedNode>>,
    mut highlights: Query<(&mut Node, &mut Visibility)>,
) {
    for (shape, position, parts) in &controls {
        let relaid = parts.segments.iter().any(|segment| {
            segments
                .get(segment.segment)
                .is_ok_and(|node| node.is_changed())
        });
        if !position.is_changed() && !relaid {
            continue;
        }
        let Ok((mut node, mut visibility)) = highlights.get_mut(parts.highlight) else {
            continue;
        };
        let Some(at) = position.0 else {
            visibility.set_if_neq(Visibility::Hidden);
            continue;
        };
        visibility.set_if_neq(Visibility::Inherited);
        let widths: Vec<f32> = parts
            .segments
            .iter()
            .filter_map(|segment| segments.get(segment.segment).ok())
            .map(|segment| segment.size().x)
            .collect();
        let (left, width) = shape.highlight_span(at, &widths);
        let (left, width) = (percent(left), percent(width));
        if (node.left, node.width) != (left, width) {
            node.left = left;
            node.width = width;
        }
    }
}

/// A segment's hover state and whether it's disabled, and its current background.
type HoveredSegment = (
    Entity,
    &'static Hovered,
    Has<InteractionDisabled>,
    &'static ChildOf,
    &'static ThemeSkin,
);

/// Tints a segment while hovered, unless it's disabled.
fn tint_hovered_segments(
    segments: Query<HoveredSegment, (With<ChoiceItem>, Changed<Hovered>)>,
    controls: Query<&SegmentedTokens>,
    mut commands: Commands,
) {
    for (entity, hovered, disabled, parent, old) in &segments {
        let Ok(tokens) = controls.get(parent.parent()) else {
            continue;
        };
        let wanted = tokens.segment(hovered.0, disabled);
        if *old != wanted {
            commands.entity(entity).insert(wanted);
        }
    }
}

/// What a control's look is picked from.
type SegmentedLook = (
    &'static SegmentedTokens,
    &'static SegmentedParts,
    &'static Shape,
    &'static Chosen,
    &'static ChoiceSet,
    Has<FocusVisible>,
);

/// Styles a control when its tokens are set: at spawn, and whenever the app sets new ones.
fn style_segmented(
    insert: On<Insert<SegmentedTokens>>,
    controls: Query<SegmentedLook>,
    hovered: Query<&Hovered>,
    mut commands: Commands,
) {
    let Ok((tokens, parts, shape, chosen, choices, focused)) = controls.get(insert.entity) else {
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
        .insert((ThemeSkin(tokens.highlight.clone()), rounded));
    let font = (
        ThemeFontSize(tokens.font_size.clone()),
        ThemeFont(tokens.font.clone()),
    );
    for (index, segment) in parts.segments.iter().enumerate() {
        let off = choices.get(index).is_some_and(Choice::is_disabled);
        let over = hovered.get(segment.segment).is_ok_and(|hovered| hovered.0);
        commands
            .entity(segment.segment)
            .insert(tokens.segment(over, off));
        if let Some(label) = segment.label {
            let colour = tokens.label(shape.lit(index, chosen.0), off);
            commands.entity(label).insert((colour, font.clone()));
        }
    }
}

/// Segmented control behaviour, motion, styling and default token values.
pub struct SegmentedWidgetPlugin;

impl Plugin for SegmentedWidgetPlugin {
    fn build(&self, app: &mut App) {
        app.ensure_plugin(HoverCursorPlugin)
            .ensure_plugin(ThemePlugin)
            .ensure_plugin(ChoicePlugin)
            .add_observer(work_with_keys)
            .add_observer(disable_segments)
            .add_observer(enable_segments)
            .add_observer(style_segmented)
            .add_systems(Update, (follow_selection, animate_slides).chain())
            .add_systems(
                PostUpdate,
                (
                    position_highlights,
                    tint_hovered_segments,
                    light_focused_frames::<SegmentedTokens>,
                )
                    .before(UiSystems::Prepare),
            );
        SegmentedTokens::with_prefix(SEGMENTED_STYLE)
            .register_theme_defaults(&mut app.world_mut().resource_mut::<Theme>());
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy::input::ButtonState;
    use bevy::input::keyboard::{KeyCode, NativeKeyCode};
    use bevy::picking::cursor::EntityCursor;
    use bevy::time::TimeUpdateStrategy;
    use bevy::ui_widgets::ValueChange;
    use bevy::window::SystemCursorIcon;

    use super::*;
    use crate::choice::ChoiceState;

    #[derive(Resource, Default)]
    struct Announced(Vec<usize>);

    fn app_with(config: SegmentedConfig) -> (App, Entity) {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, SegmentedWidgetPlugin))
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
                20,
            )))
            .init_resource::<Announced>()
            .add_observer(
                |change: On<ValueChange<usize>>, mut announced: ResMut<Announced>| {
                    announced.0.push(change.value);
                },
            );
        let mut control = None;
        app.world_mut()
            .commands()
            .spawn(Node::default())
            .with_children(|root| {
                control = Some(spawn_segmented(root, config).id());
            });
        app.update();
        (app, control.unwrap())
    }

    fn options() -> Vec<Choice> {
        ["Low", "Mid", "High", "Max"].map(Choice::text).into()
    }

    fn press(app: &mut App, control: Entity, key: Key) {
        let keyboard = KeyboardInput {
            key_code: KeyCode::Unidentified(NativeKeyCode::Unidentified),
            logical_key: key,
            state: ButtonState::Pressed,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        };
        app.world_mut()
            .trigger(FocusedInput::new(control, keyboard, Entity::PLACEHOLDER));
        app.update();
    }

    fn selected(app: &App, control: Entity) -> Option<usize> {
        app.world().get::<Chosen>(control).unwrap().0
    }

    fn parts(app: &App, control: Entity) -> SegmentedParts {
        app.world().get::<SegmentedParts>(control).unwrap().clone()
    }

    fn segment(app: &App, control: Entity, index: usize) -> Entity {
        parts(app, control).segments[index].segment
    }

    /// The highlight's left edge and width, if it's showing.
    fn highlight(app: &App, control: Entity) -> Option<(Val, Val)> {
        let highlight = parts(app, control).highlight;
        let node = app.world().get::<Node>(highlight).unwrap();
        let shown = app.world().get::<Visibility>(highlight) != Some(&Visibility::Hidden);
        shown.then_some((node.left, node.width))
    }

    fn lit_labels(app: &App, control: Entity) -> usize {
        let lit = SegmentedTokens::with_prefix(SEGMENTED_STYLE).text_lit;
        parts(app, control)
            .segments
            .iter()
            .filter_map(|segment| segment.label)
            .filter(|&label| app.world().get::<ThemeTextColor>(label).map(|c| &c.0) == Some(&lit))
            .count()
    }

    #[test]
    fn choosing_a_segment_selects_announces_and_marks_its_state() {
        let (mut app, control) = app_with(SegmentedConfig::new(options()));
        let third = segment(&app, control, 2);
        // What clicking the third segment does.
        app.world_mut().trigger(Pick {
            set: control,
            index: Some(2),
        });
        app.update();

        assert_eq!(selected(&app, control), Some(2));
        assert_eq!(app.world().resource::<Announced>().0, [2]);
        assert!(app.world().get::<ChoiceState>(third).unwrap().chosen);
        let first = segment(&app, control, 0);
        assert!(!app.world().get::<ChoiceState>(first).unwrap().chosen);
        assert_eq!(
            highlight(&app, control),
            Some((percent(50.0), percent(25.0)))
        );
        assert_eq!(lit_labels(&app, control), 1);
    }

    #[test]
    fn the_arrows_select_their_way_round_skipping_disabled_segments() {
        let mut options = options();
        options[1] = Choice::text("Mid").disabled(true);
        let (mut app, control) = app_with(SegmentedConfig::new(options));
        let not_allowed = EntityCursor::System(SystemCursorIcon::NotAllowed);
        let middle = segment(&app, control, 1);
        assert_eq!(app.world().get::<EntityCursor>(middle), Some(&not_allowed));
        press(&mut app, control, Key::ArrowRight);
        assert_eq!(selected(&app, control), Some(2), "over the disabled one");
        press(&mut app, control, Key::ArrowRight);
        press(&mut app, control, Key::ArrowRight);
        assert_eq!(selected(&app, control), Some(0), "round from the last");
        press(&mut app, control, Key::ArrowLeft);
        assert_eq!(selected(&app, control), Some(3), "and back");
        press(&mut app, control, Key::Home);
        assert_eq!(selected(&app, control), Some(0));
        assert_eq!(app.world().resource::<Announced>().0, [2, 3, 0, 3, 0]);
    }

    #[test]
    fn a_segment_can_be_built_from_anything() {
        #[derive(Component)]
        struct Swatch;
        let swatch = Choice::new("Red", |segment| {
            segment.spawn((Swatch, Node::default()));
        });
        let (mut app, control) = app_with(SegmentedConfig::new([swatch, "Plain".into()]));
        let world = app.world_mut();
        let swatches = world
            .query_filtered::<(), With<Swatch>>()
            .iter(world)
            .count();
        assert_eq!(swatches, 1);
        assert_eq!(parts(&app, control).segments[0].label, None);
        assert_eq!(lit_labels(&app, control), 0, "only plain words are labels");
    }

    #[test]
    fn cumulative_fills_from_the_left_and_lights_every_segment_up_to_the_selection() {
        let config = SegmentedConfig::new(options())
            .highlight(Highlight::Cumulative)
            .chosen(2);
        let (app, control) = app_with(config);
        assert_eq!(
            highlight(&app, control),
            Some((percent(0.0), percent(75.0)))
        );
        assert_eq!(lit_labels(&app, control), 3);
    }

    #[test]
    fn with_nothing_chosen_nothing_is_lit_until_a_pick() {
        let config = SegmentedConfig {
            chosen: None,
            ..SegmentedConfig::new(options())
        };
        let (mut app, control) = app_with(config);
        assert_eq!(highlight(&app, control), None);
        assert_eq!(lit_labels(&app, control), 0);
        app.world_mut().trigger(Pick {
            set: control,
            index: Some(1),
        });
        app.update();
        assert_eq!(
            highlight(&app, control),
            Some((percent(25.0), percent(25.0)))
        );
    }

    fn shape(highlight: Highlight) -> Shape {
        Shape {
            count: 3,
            highlight,
            motion: None,
        }
    }

    /// The highlight covers a segment's real extent, however wide it is.
    #[test]
    fn the_highlight_follows_uneven_segments() {
        let widths = [100.0, 50.0, 50.0];
        let single = shape(Highlight::Single);
        assert_eq!(single.highlight_span(0.0, &widths), (0.0, 50.0));
        assert_eq!(single.highlight_span(1.0, &widths), (50.0, 25.0));
        // Halfway through a slide from the first to the second segment.
        assert_eq!(single.highlight_span(0.5, &widths), (25.0, 37.5));

        let cumulative = shape(Highlight::Cumulative);
        assert_eq!(cumulative.highlight_span(1.0, &widths), (0.0, 75.0));
        assert_eq!(cumulative.highlight_span(2.0, &widths), (0.0, 100.0));
    }

    #[test]
    fn before_layout_segments_are_taken_as_equal() {
        let (left, width) = shape(Highlight::Single).highlight_span(1.0, &[0.0; 3]);
        assert!((left - 100.0 / 3.0).abs() < 1e-4 && (width - 100.0 / 3.0).abs() < 1e-4);
    }

    /// Layout finishing (or changing) moves the highlight onto the real segments.
    #[test]
    fn a_relayout_moves_the_highlight_onto_the_segments() {
        let config = SegmentedConfig::new(options()).widths(SegmentWidths::FitLabels);
        let (mut app, control) = app_with(config);
        for (index, width) in [40.0, 40.0, 80.0, 40.0].into_iter().enumerate() {
            let segment = segment(&app, control, index);
            app.world_mut().entity_mut(segment).insert(ComputedNode {
                size: Vec2::new(width, 28.0),
                ..default()
            });
        }
        app.update();
        assert_eq!(
            highlight(&app, control),
            Some((percent(0.0), percent(20.0)))
        );
    }

    #[test]
    fn with_motion_the_highlight_slides_and_lands_exactly() {
        let config = SegmentedConfig::new(options())
            .highlight(Highlight::Cumulative)
            .motion(ResponseCurve::Power(2.0));
        let (mut app, control) = app_with(config);
        app.world_mut().entity_mut(control).insert(Chosen(Some(3)));
        app.update();
        app.update();
        let Some((_, Val::Percent(width))) = highlight(&app, control) else {
            panic!("percent width")
        };
        assert!(
            width > 25.0 && width < 100.0,
            "fill is growing, at {width}%"
        );
        for _ in 0..10 {
            app.update();
        }
        assert_eq!(
            highlight(&app, control),
            Some((percent(0.0), percent(100.0)))
        );
        assert!(
            app.world().resource::<Announced>().0.is_empty(),
            "changes from code aren't announced"
        );
    }

    #[test]
    fn disabling_the_control_disables_its_segments_and_the_keys() {
        let mut options = options();
        options[3] = Choice::text("Max").disabled(true);
        let (mut app, control) = app_with(SegmentedConfig::new(options).disabled(true));
        press(&mut app, control, Key::ArrowRight);
        assert_eq!(selected(&app, control), Some(0));
        let off = |app: &App, index| {
            let segment = segment(app, control, index);
            app.world().get::<InteractionDisabled>(segment).is_some()
        };
        assert!((0..4).all(|index| off(&app, index)));

        app.world_mut()
            .entity_mut(control)
            .remove::<InteractionDisabled>();
        app.update();
        assert!(!off(&app, 0));
        assert!(off(&app, 3), "its own option is still disabled");
    }

    #[test]
    fn new_tokens_restyle_the_control_and_its_labels() {
        let (mut app, control) = app_with(SegmentedConfig::new(options()).chosen(1));
        let tokens = SegmentedTokens::with_prefix("game.quality");
        app.world_mut().entity_mut(control).insert(tokens.clone());
        app.update();
        let parts = parts(&app, control);
        let world = app.world();
        let fill = |entity| world.get::<ThemeSkin>(entity).unwrap().0.clone();
        assert_eq!(fill(control), tokens.bg);
        assert_eq!(fill(parts.highlight), tokens.highlight);
        let colour = |index: usize| {
            let label = parts.segments[index].label.unwrap();
            world.get::<ThemeTextColor>(label).unwrap().0.clone()
        };
        assert_eq!(colour(1), tokens.text_lit);
        assert_eq!(colour(0), tokens.text);
    }
}
