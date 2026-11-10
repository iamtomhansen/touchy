//! Themed sliders built on Bevy's headless [`Slider`].
//!
//! Behaviour comes from `bevy_ui_widgets`: dragging, track clicks and keyboard
//! steps emit [`ValueChange<f32>`](bevy::ui_widgets::ValueChange), and the value
//! lives in [`SliderValue`]. This module adds the look: a track, a fill, a thumb,
//! optional marks, and token swaps for hover, drag and disabled states.
//!
//! A slider lies along its track, left to right, or bottom to top when vertical. Its sizes
//! say which way they run: a length runs along the track, a thickness across it.

use std::ops::RangeInclusive;

use bevy::ecs::entity::EntityHashSet;
use bevy::ecs::query::QueryData;
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui::{InteractionDisabled, UiSystems};
use bevy::ui_widgets::{
    Slider, SliderDragState, SliderOrientation, SliderPrecision, SliderRange, SliderStep,
    SliderThumb, SliderValue, TrackClick, ValueChange, slider_self_update,
};
use bevy::window::SystemCursorIcon;

use crate::EnsurePlugin;
use crate::cursor::{HoverCursor, HoverCursorPlugin};
use crate::sanitize::{at_least, at_most, within};
use crate::theme::palette::{
    ACCENT, CONTROL_CORNER_RADIUS, EDGE, HIGHLIGHT, SURFACE, TEXT, TEXT_MUTED,
};
use crate::theme::skins;
use crate::theme::{
    FocusVisible, Shade, SkinFrame, Theme, ThemeBorder, ThemeCornerRadius, ThemePlugin, ThemeSkin,
    ThemeToken, WidgetFlags, WidgetState,
};

/// Prefix of the slider widget's theme tokens.
pub const SLIDER_STYLE: &str = "touchy.slider";
const DEFAULT_THICKNESS: f32 = 25.0;
const DEFAULT_THUMB_LENGTH: f32 = 10.0;
/// A vertical slider's default length.
const DEFAULT_VERTICAL_LENGTH: f32 = 120.0;
/// A mark's default length along its track: a thin line across it.
const DEFAULT_MARK_LENGTH: f32 = 3.0;
/// The track's border width in pixels, drawn over the track and fill.
const TRACK_BORDER: f32 = 1.0;
/// The cursor over a slider, which moves along its track.
const CURSOR: HoverCursor = HoverCursor::new(SystemCursorIcon::ColResize);
const VERTICAL_CURSOR: HoverCursor = HoverCursor::new(SystemCursorIcon::RowResize);
/// The most decimal places a slider rounds to; f32 holds about seven digits.
const MAX_DECIMALS: i32 = 6;
/// How close a scaled step must be to a whole number to count as one, relative
/// to its size: f32 can't hold 0.1 exactly.
const STEP_TOLERANCE: f32 = 1e-4;
/// The default range, when the app gives none or gives one that can't be used.
const DEFAULT_RANGE: RangeInclusive<f32> = 0.0..=100.0;

/// The tokens a slider uses. Make a style for one kind of slider with
/// [`SliderTokens::with_prefix`] and give its tokens values in the theme.
/// Insert new ones to restyle a slider.
///
/// Hover, drag and disabled looks are these tokens in a [`WidgetState`]. By
/// default the thumb's border turns to the palette highlight while hovered or
/// dragged; everything else takes the theme's shade for the state.
#[derive(Component, Clone, PartialEq, Debug)]
#[component(immutable)]
pub struct SliderTokens {
    /// The track's background.
    pub bg: ThemeToken,
    /// The track's border, which is drawn over the track and fill.
    pub border: ThemeToken,
    /// The filled part of the track, from the start up to the thumb.
    pub fill: ThemeToken,
    /// The thumb's background.
    pub thumb: ThemeToken,
    /// The thumb's border.
    pub thumb_border: ThemeToken,
    /// The track and thumb's corner radius.
    pub corner_radius: ThemeToken,
}

impl SliderTokens {
    /// Tokens named `{prefix}.track`, `{prefix}.fill`, and so on.
    pub fn with_prefix(prefix: &str) -> Self {
        let token = |suffix: &str| ThemeToken::new(format!("{prefix}.{suffix}"));
        Self {
            bg: token("track"),
            border: token("track.border"),
            fill: token("fill"),
            thumb: token("thumb"),
            thumb_border: token("thumb.border"),
            corner_radius: token("corner_radius"),
        }
    }

    /// Derives these tokens from the palette, unless the app sets them.
    pub fn register_theme_defaults(&self, theme: &mut Theme) {
        let active_border = |state| self.thumb_border.clone().with_state(state);
        theme
            .default_derived(self.bg.clone(), SURFACE, Shade::Same)
            .default_derived(self.border.clone(), EDGE, Shade::Same)
            .default_derived(self.fill.clone(), ACCENT, Shade::Same)
            .default_derived(self.thumb.clone(), TEXT, Shade::Same)
            .default_derived(self.thumb_border.clone(), TEXT_MUTED, Shade::Same)
            .default_derived(active_border(WidgetState::Hover), HIGHLIGHT, Shade::Same)
            .default_derived(active_border(WidgetState::Focused), HIGHLIGHT, Shade::Same)
            .default_derived(active_border(WidgetState::Pressed), HIGHLIGHT, Shade::Same)
            .default_size_from(self.corner_radius.clone(), CONTROL_CORNER_RADIUS);
        // Each part draws its role's skin, once a theme gives the role one.
        theme
            .default_skin_from(self.bg.clone(), skins::SUNKEN)
            .default_skin_from(self.thumb.clone(), skins::KNOB);
    }

    /// Fill, thumb and thumb-border tokens for a slider in `state`. Dragging
    /// is [`WidgetState::Pressed`].
    fn resolve(&self, state: Option<WidgetState>) -> (ThemeSkin, ThemeSkin, ThemeBorder) {
        // Only disabling dims the fill; hovering the thumb leaves it alone.
        let fill_state = state.filter(|&state| state == WidgetState::Disabled);
        (
            ThemeSkin(self.fill.clone().with_state(fill_state)),
            ThemeSkin(self.thumb.clone().with_state(state)),
            ThemeBorder::all(self.thumb_border.clone().with_state(state)),
        )
    }
}

impl Default for SliderTokens {
    fn default() -> Self {
        Self::with_prefix(SLIDER_STYLE)
    }
}

/// Configuration for a slider. The [`Slider`] component is the only required
/// part of a slider; this struct is a convenience for setting up the rest.
///
/// # Examples
/// ```
/// use touchy::prelude::*;
///
/// let volume = SliderConfig::new(7.0).range(0.0..=10.0).step(1.0);
/// let fader = SliderConfig::new(50.0).vertical(true).length(200.0);
/// ```
#[derive(Clone, Debug)]
pub struct SliderConfig {
    /// The slider's value.
    pub value: f32,
    /// The slider's range. Defaults to 0 to 100.
    pub range: SliderRange,
    /// Keyboard and track-click step (Bevy's [`SliderStep`]); without one, Bevy's
    /// step of 1 applies. A self-updating slider's drags land on it too, counted
    /// from the range's start.
    pub step: Option<f32>,
    /// Decimal places the value is rounded to while dragging (Bevy's [`SliderPrecision`]).
    /// A self-updating slider with a step defaults to the step's precision.
    pub precision: Option<i32>,
    /// How the slider behaves when the user clicks on the track. Defaults to [`TrackClick::Drag`].
    pub track_click: TrackClick,
    /// The slider's theme tokens.
    pub tokens: SliderTokens,
    /// The track's length in pixels, or `None` to fill its parent that way.
    pub length: Option<f32>,
    /// The track's thickness in pixels, across it.
    pub thickness: f32,
    /// The thumb's length in pixels, along the track.
    pub thumb_length: f32,
    /// The thumb's thickness in pixels, across the track, or `None` for the track's own.
    pub thumb_thickness: Option<f32>,
    /// Whether the slider is vertical. Defaults to horizontal.
    pub vertical: bool,
    /// Whether the slider is disabled.
    pub disabled: bool,
    /// Move the thumb as soon as the user drags it. Turn off for a fully controlled
    /// slider whose value only changes when the app writes [`SliderValue`].
    pub self_update: bool,
}

impl SliderConfig {
    /// Creates a slider config at `value`.
    pub fn new(value: f32) -> Self {
        Self { value, ..default() }
    }

    /// Sets the range of the slider: `0.0..=100.0``. The value is clamped to it at spawn.
    pub fn range(mut self, range: RangeInclusive<f32>) -> Self {
        self.range = SliderRange::from_range(range);
        self
    }

    /// Sets the keyboard and track-click step (Bevy's [`SliderStep`]).
    pub fn step(mut self, step: f32) -> Self {
        self.step = Some(step);
        self
    }

    /// Sets the decimal places the value is rounded to while dragging.
    pub fn precision(mut self, decimal_places: i32) -> Self {
        self.precision = Some(decimal_places);
        self
    }

    /// How clicking on the track moves the thumb: [`TrackClick::Drag`],
    /// [`TrackClick::Step`] or [`TrackClick::Snap`].
    pub fn track_click(mut self, track_click: TrackClick) -> Self {
        self.track_click = track_click;
        self
    }

    /// Sets the slider's theme tokens.
    pub fn tokens(mut self, tokens: SliderTokens) -> Self {
        self.tokens = tokens;
        self
    }

    /// Sets the track's length in pixels, along it.
    pub fn length(mut self, pixels: f32) -> Self {
        self.length = Some(pixels);
        self
    }

    /// Sets the track's thickness in pixels, across it.
    pub fn thickness(mut self, pixels: f32) -> Self {
        self.thickness = pixels;
        self
    }

    /// Sets the thumb's length in pixels, along the track.
    pub fn thumb_length(mut self, pixels: f32) -> Self {
        self.thumb_length = pixels;
        self
    }

    /// Sets the thumb's thickness in pixels, across the track: a fader's wide cap.
    pub fn thumb_thickness(mut self, pixels: f32) -> Self {
        self.thumb_thickness = Some(pixels);
        self
    }

    /// Sets whether the slider is vertical. Defaults to horizontal.
    pub fn vertical(mut self, vertical: bool) -> Self {
        self.vertical = vertical;
        self
    }

    /// Sets whether the slider is disabled.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Sets whether the slider moves the thumb as soon as the user drags it.
    pub fn self_update(mut self, self_update: bool) -> Self {
        self.self_update = self_update;
        self
    }

    /// This config with every value made usable, warning about each repair.
    fn sanitized(mut self) -> Self {
        // Slider must be min < value < max.
        let (start, end) = (self.range.start(), self.range.end());
        if start.is_nan() || end.is_nan() {
            warn!(
                "SliderConfig range was {start}..={end}, but its ends must be numbers; using {DEFAULT_RANGE:?}"
            );
            self.range = SliderRange::from_range(DEFAULT_RANGE);
        } else if end < start {
            warn!("SliderConfig range was {start}..={end} (backwards); using {end}..={start}");
            self.range = SliderRange::from_range(end..=start);
        }
        self.value = within(self.value, start, end, "SliderConfig value");
        // A step that isn't more than nothing can't be stepped by, or rounded to.
        if let Some(step) = self.step.filter(|step| step.is_nan() || *step <= 0.0) {
            warn!("SliderConfig step was {step}, but a step must be more than 0; using none");
            self.step = None;
        }
        self.precision = at_most(self.precision, MAX_DECIMALS, "SliderConfig precision");
        self.length = at_least(self.length, 0.0, "SliderConfig length");
        self.thickness = at_least(self.thickness, 0.0, "SliderConfig thickness");
        self.thumb_length = at_least(self.thumb_length, 0.0, "SliderConfig thumb length");
        self.thumb_thickness = at_least(self.thumb_thickness, 0.0, "SliderConfig thumb thickness");
        self
    }
}

impl Default for SliderConfig {
    fn default() -> Self {
        Self {
            value: 0.0,
            range: SliderRange::from_range(DEFAULT_RANGE),
            step: None,
            precision: None,
            track_click: TrackClick::default(),
            tokens: SliderTokens::default(),
            length: None,
            thickness: DEFAULT_THICKNESS,
            thumb_length: DEFAULT_THUMB_LENGTH,
            thumb_thickness: None,
            vertical: false,
            disabled: false,
            self_update: true,
        }
    }
}

/// The parts of a slider, on the slider. The slider itself is the track.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct SliderParts {
    /// The filled part of the track, from the start up to the thumb.
    pub fill: Entity,
    /// The thumb, which also holds Bevy's [`SliderThumb`].
    pub thumb: Entity,
}

/// A mark on the track at `value`, such as a tick or an event on a timeline. Spawn one as a
/// child of a touchy slider, with its look, and the slider places it and turns it across the
/// track: `track.spawn((SliderMark::at(75.0), ThemeSkin(token)))`.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
#[require(Node = Node { position_type: PositionType::Absolute, ..default() })]
pub struct SliderMark {
    /// The mark's value, in the slider's units.
    pub value: f32,
    /// Its length in pixels, along the track.
    pub length: f32,
    /// Its thickness in pixels, across the track, or `None` for the track's own.
    pub thickness: Option<f32>,
}

impl SliderMark {
    /// A thin line across the track at `value`.
    pub fn at(value: f32) -> Self {
        Self {
            value,
            length: DEFAULT_MARK_LENGTH,
            thickness: None,
        }
    }

    /// Sets its length in pixels, along the track.
    pub fn length(mut self, pixels: f32) -> Self {
        self.length = pixels;
        self
    }

    /// Sets its thickness in pixels, across the track.
    pub fn thickness(mut self, pixels: f32) -> Self {
        self.thickness = Some(pixels);
        self
    }
}

/// Where to draw the thumb, when that differs from its [`SliderValue`]: for
/// animations, or widgets like the gearshift that bend the thumb toward detents.
/// The value keeps meaning "where the input put it"; this is only what's shown.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct SliderDisplay(pub f32);

/// Which way a slider lies and its thumb's length, used to keep it under the pointer.
#[derive(Component, Clone, Copy, Debug)]
struct Track {
    vertical: bool,
    thumb_length: f32,
}

impl Track {
    /// A node `length` along the track and `across` it.
    fn sized(self, mut node: Node, length: Val, across: Val) -> Node {
        (node.width, node.height) = if self.vertical {
            (across, length)
        } else {
            (length, across)
        };
        node
    }

    /// A node `length` along the track and `thickness` across it, centered on the track.
    fn crossing(self, node: Node, length: f32, thickness: Option<f32>) -> Node {
        let across = thickness.map_or(percent(100), px);
        let mut node = self.sized(node, px(length), across);
        let (inset, margin) = match thickness {
            Some(thickness) => (percent(50), px(-thickness / 2.0)),
            None => (Val::Auto, px(0)),
        };
        if self.vertical {
            (node.left, node.margin.left) = (inset, margin);
        } else {
            (node.top, node.margin.top) = (inset, margin);
        }
        node
    }
}

/// Drags on this self-updating slider land on multiples of the step.
#[derive(Component, Clone, Copy, Debug)]
struct SnapToStep(f32);

/// Spawns it; documented on [`SpawnUi::slider`](crate::spawn_ui::SpawnUi::slider).
pub(crate) fn spawn_slider<'a>(
    parent: &'a mut ChildSpawnerCommands,
    config: SliderConfig,
) -> EntityCommands<'a> {
    let SliderConfig {
        value,
        range,
        step,
        precision,
        track_click,
        tokens,
        length,
        thickness,
        thumb_length,
        thumb_thickness,
        vertical,
        disabled,
        self_update,
    } = config.sanitized();

    // Horizontal versus vertical orientation.
    let track = Track {
        vertical,
        thumb_length,
    };
    let border = UiRect::all(px(TRACK_BORDER));
    let mut node = track.sized(
        Node {
            border,
            ..default()
        },
        percent(100),
        px(thickness),
    );
    match length {
        Some(length) => node = track.sized(node, px(length), px(thickness)),
        None if vertical => node.min_height = px(DEFAULT_VERTICAL_LENGTH),
        None => {}
    }
    // Room beside the track for an overhanging thumb, so it doesn't cover its neighbours.
    let overhang = px((thumb_thickness.unwrap_or(thickness) - thickness).max(0.0) / 2.0);
    node.margin = if vertical {
        UiRect::horizontal(overhang)
    } else {
        UiRect::vertical(overhang)
    };
    let (orientation, cursor) = if vertical {
        (SliderOrientation::Vertical, VERTICAL_CURSOR)
    } else {
        // Never Bevy's `Auto`.
        (SliderOrientation::Horizontal, CURSOR)
    };

    // The slider itself is the track.
    let mut slider = parent.spawn((
        Slider {
            track_click,
            orientation,
        },
        SliderValue(value),
        range,
        Hovered::default(),
        cursor,
        // Tab reaches it in layout order; Bevy's slider handles the arrow keys once focused.
        TabIndex(0),
        track,
        node,
    ));
    if let Some(step) = step {
        slider.insert(SliderStep(step));
    }
    // Bevy rounds drags to a precision, never to a step. Rounding to the step's
    // decimal places makes the values it reports land on whole steps whenever
    // the step is a power of ten (1, 0.1, 10); the snap below handles the rest.
    let snapping = step.filter(|_| self_update);
    if let Some(precision) = precision.or(snapping.map(decimal_places)) {
        slider.insert(SliderPrecision(precision));
    }
    match (self_update, snapping) {
        (true, Some(step)) => {
            slider.insert(SnapToStep(step)).observe(snap_self_update);
        }
        (true, None) => {
            slider.observe(slider_self_update);
        }
        (false, _) => {}
    }
    if disabled {
        slider.insert(InteractionDisabled);
    }

    let mut parts = SliderParts {
        fill: Entity::PLACEHOLDER,
        thumb: Entity::PLACEHOLDER,
    };
    slider.with_children(|inside| {
        // The fill grows from the start: the left, or the bottom.
        let start = Node {
            position_type: PositionType::Absolute,
            left: px(0),
            bottom: px(0),
            ..default()
        };
        let fill = track.sized(start, percent(0), percent(100));
        parts.fill = inside.spawn(fill).id();
        let thumb = Node {
            position_type: PositionType::Absolute,
            border: UiRect::all(px(TRACK_BORDER)),
            ..default()
        };
        // A sprite-sheet skin shows the frame for where the thumb sits.
        let thumb = track.crossing(thumb, thumb_length, thumb_thickness);
        parts.thumb = inside
            .spawn((SliderThumb, SkinFrame::default(), thumb))
            .id();
    });
    // Its look comes with its tokens, in `style_slider`, once its state is in.
    slider.insert((parts, tokens));
    slider
}

/// Like Bevy's `slider_self_update`, but lands on the nearest step.
fn snap_self_update(
    change: On<ValueChange<f32>>,
    sliders: Query<(&SliderRange, &SnapToStep)>,
    mut commands: Commands,
) {
    if let Ok((range, &SnapToStep(step))) = sliders.get(change.source) {
        let value = snap_to_step(change.value, range, step);
        commands.entity(change.source).insert(SliderValue(value));
    }
}

/// `value` moved to the nearest multiple of `step` from the range's start.
fn snap_to_step(value: f32, range: &SliderRange, step: f32) -> f32 {
    let steps = ((value - range.start()) / step).round();
    range.clamp(range.start() + steps * step)
}

/// How many decimal places `step` needs: 1 needs none, 0.25 needs two.
fn decimal_places(step: f32) -> i32 {
    (0..MAX_DECIMALS)
        .find(|&places| {
            let scaled = step * 10f32.powi(places);
            (scaled - scaled.round()).abs() < STEP_TOLERANCE * scaled.abs().max(1.0)
        })
        .unwrap_or(MAX_DECIMALS)
}

/// Fraction of the way along the track for `value`. An empty range puts
/// everything at the start, rather than Bevy's middle.
fn fraction(range: &SliderRange, value: f32) -> f32 {
    if range.span() > 0.0 {
        range.thumb_position(value).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// The slider's value, display or range changed, or its marks were redrawn.
type SliderMoved = Or<(
    Changed<SliderValue>,
    Changed<SliderDisplay>,
    Changed<SliderRange>,
    Changed<Children>,
)>;

/// What positioning needs from a slider.
#[derive(QueryData)]
struct SliderGeometry {
    value: &'static SliderValue,
    display: Option<&'static SliderDisplay>,
    range: &'static SliderRange,
    track: &'static Track,
    parts: &'static SliderParts,
    children: &'static Children,
}

/// Positions the fill, thumb and marks, and picks the thumb's frame if its
/// skin is a sprite sheet. The thumb travels the track's length minus its own, matching how
/// the headless slider maps the pointer, and each mark is centred where the thumb's centre
/// would be at the mark's value.
fn position_slider_parts(
    sliders: Query<SliderGeometry, SliderMoved>,
    mut nodes: Query<(&mut Node, Option<&SliderMark>)>,
    mut frames: Query<&mut SkinFrame>,
) {
    for slider in &sliders {
        let track = *slider.track;
        let thumb_length = track.thumb_length;
        let shown = slider.display.map_or(slider.value.0, |display| display.0);
        let at = fraction(slider.range, shown);
        if let Ok((mut fill, _)) = nodes.get_mut(slider.parts.fill) {
            let sized = track.sized(fill.clone(), percent(at * 100.0), percent(100));
            if *fill != sized {
                *fill = sized;
            }
        }
        if let Ok((thumb, _)) = nodes.get_mut(slider.parts.thumb) {
            place(thumb, track, at, -thumb_length * at);
        }
        if let Ok(mut frame) = frames.get_mut(slider.parts.thumb) {
            frame.set_if_neq(SkinFrame(at));
        }
        for child in slider.children.iter() {
            if let Ok((mut node, Some(&mark))) = nodes.get_mut(child) {
                let length = mark.length.max(0.0);
                let thickness = mark.thickness.map(|thickness| thickness.max(0.0));
                let shaped = track.crossing(node.clone(), length, thickness);
                if *node != shaped {
                    *node = shaped;
                }
                let at = fraction(slider.range, mark.value);
                let nudge = (thumb_length - length) / 2.0 - thumb_length * at;
                place(node, track, at, nudge);
            }
        }
    }
}

/// Moves `node` to `at` along the track, from its start, nudged by `nudge` pixels.
fn place(mut node: Mut<Node>, track: Track, at: f32, nudge: f32) {
    let (offset, margin) = (percent(at * 100.0), px(nudge));
    let (at_offset, at_margin) = if track.vertical {
        (node.bottom, node.margin.bottom)
    } else {
        (node.left, node.margin.left)
    };
    // Only write real changes: any write to `Node` triggers a relayout.
    if (at_offset, at_margin) == (offset, margin) {
        return;
    }
    if track.vertical {
        (node.bottom, node.margin.bottom) = (offset, margin);
    } else {
        (node.left, node.margin.left) = (offset, margin);
    }
}

/// A slider's state, as its components hold it.
#[derive(QueryData)]
struct SliderStatus {
    hovered: &'static Hovered,
    focused: Has<FocusVisible>,
    drag: &'static SliderDragState,
    disabled: Has<InteractionDisabled>,
}

impl SliderStatusItem<'_, '_> {
    fn state(&self) -> Option<WidgetState> {
        let flags = WidgetFlags {
            hovered: self.hovered.0,
            focused: self.focused,
            pressed: self.drag.dragging,
            disabled: self.disabled,
        };
        flags.state()
    }
}

/// State changes that can alter a slider's look. Re-enabling and losing the
/// focus are read separately, because change detection doesn't see removals,
/// and new tokens in `style_slider`.
type LookChanged = (
    With<SliderParts>,
    Or<(
        Changed<Hovered>,
        Added<FocusVisible>,
        Changed<SliderDragState>,
        Added<InteractionDisabled>,
    )>,
);

/// Re-picks fill and thumb-border tokens for sliders whose state changed.
fn restyle_sliders(
    changed: Query<Entity, LookChanged>,
    mut enabled: RemovedComponents<InteractionDisabled>,
    mut unfocused: RemovedComponents<FocusVisible>,
    sliders: Query<(&SliderTokens, &SliderParts, SliderStatus)>,
    skins: Query<&ThemeSkin>,
    thumbs: Query<&ThemeBorder>,
    mut commands: Commands,
) {
    let mut dirty: EntityHashSet = changed.iter().collect();
    dirty.extend(enabled.read().chain(unfocused.read()));
    for entity in dirty {
        let Ok((tokens, parts, status)) = sliders.get(entity) else {
            continue;
        };
        let (fill, thumb, thumb_border) = tokens.resolve(status.state());
        if skins.get(parts.fill).is_ok_and(|old| *old != fill) {
            commands.entity(parts.fill).insert(fill);
        }
        if skins.get(parts.thumb).is_ok_and(|old| *old != thumb) {
            commands.entity(parts.thumb).insert(thumb);
        }
        if thumbs
            .get(parts.thumb)
            .is_ok_and(|old| *old != thumb_border)
        {
            commands.entity(parts.thumb).insert(thumb_border);
        }
    }
}

/// Styles a slider when its tokens are set: at spawn, and whenever the app sets new ones.
fn style_slider(
    insert: On<Insert<SliderTokens>>,
    sliders: Query<(&SliderTokens, &SliderParts, SliderStatus)>,
    mut commands: Commands,
) {
    let Ok((tokens, parts, status)) = sliders.get(insert.entity) else {
        return;
    };
    let (fill, thumb, thumb_border) = tokens.resolve(status.state());
    let rounded = ThemeCornerRadius(tokens.corner_radius.clone());
    commands.entity(insert.entity).insert((
        ThemeSkin(tokens.bg.clone()),
        ThemeBorder::all(tokens.border.clone()),
        rounded.clone(),
    ));
    commands.entity(parts.fill).insert((fill, rounded.clone()));
    commands
        .entity(parts.thumb)
        .insert((thumb, thumb_border, rounded));
}

/// Themed sliders: default token values, positioning, state styling and cursors.
pub struct SliderWidgetPlugin;

impl Plugin for SliderWidgetPlugin {
    fn build(&self, app: &mut App) {
        app.ensure_plugin(HoverCursorPlugin)
            .ensure_plugin(ThemePlugin)
            .add_observer(style_slider)
            .add_systems(
                PostUpdate,
                (position_slider_parts, restyle_sliders).before(UiSystems::Prepare),
            );
        SliderTokens::default()
            .register_theme_defaults(&mut app.world_mut().resource_mut::<Theme>());
    }
}

#[cfg(test)]
mod tests {
    use bevy::picking::cursor::EntityCursor;

    use super::*;
    use crate::theme::ThemePlugin;

    const STYLE: ThemeToken = ThemeToken::new_static("test.mark");

    fn app_with_slider(config: SliderConfig) -> (App, Entity) {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, ThemePlugin, SliderWidgetPlugin));
        let mut slider = None;
        app.world_mut()
            .commands()
            .spawn(Node::default())
            .with_children(|root| {
                let mut entity = spawn_slider(root, config);
                entity.with_child((SliderMark::at(75.0), ThemeSkin(STYLE)));
                slider = Some(entity.id());
            });
        app.update();
        (app, slider.unwrap())
    }

    fn parts(app: &App, slider: Entity) -> SliderParts {
        *app.world().get::<SliderParts>(slider).unwrap()
    }

    fn node(app: &App, entity: Entity) -> Node {
        app.world().get::<Node>(entity).unwrap().clone()
    }

    fn mark(app: &mut App) -> Node {
        let world = app.world_mut();
        world
            .query_filtered::<&Node, With<SliderMark>>()
            .single(world)
            .unwrap()
            .clone()
    }

    #[test]
    fn fill_thumb_and_marks_follow_the_value() {
        let config = SliderConfig::new(25.0)
            .range(0.0..=100.0)
            .thumb_length(10.0);
        let (mut app, slider) = app_with_slider(config);
        let parts = parts(&app, slider);
        assert_eq!(node(&app, parts.fill).width, percent(25.0));
        let thumb = node(&app, parts.thumb);
        assert_eq!((thumb.left, thumb.margin.left), (percent(25.0), px(-2.5)));
        // A mark at 75 sits where the thumb's centre would be at 75.
        let mark = mark(&mut app);
        assert_eq!(
            (mark.left, mark.margin.left),
            (percent(75.0), px((10.0 - DEFAULT_MARK_LENGTH) / 2.0 - 7.5))
        );

        app.world_mut().entity_mut(slider).insert(SliderValue(50.0));
        app.update();
        assert_eq!(node(&app, parts.fill).width, percent(50.0));
        // A sprite-sheet skin on the thumb shows the frame for where it sits.
        let frame = app.world().get::<SkinFrame>(parts.thumb);
        assert_eq!(frame, Some(&SkinFrame(0.5)));
    }

    /// Widgets built on the slider (the gearshift) animate by writing only the
    /// display, so a display change alone must move the thumb.
    #[test]
    fn a_display_alone_moves_the_thumb() {
        let config = SliderConfig::new(25.0)
            .range(0.0..=100.0)
            .self_update(false);
        let (mut app, slider) = app_with_slider(config);
        app.world_mut()
            .entity_mut(slider)
            .insert(SliderDisplay(50.0));
        app.update();
        let thumb = parts(&app, slider).thumb;
        assert_eq!(node(&app, thumb).left, percent(50.0));
        assert_eq!(
            app.world().get::<SliderValue>(slider),
            Some(&SliderValue(25.0))
        );
    }

    #[test]
    fn hover_drag_and_disable_restyle_the_slider() {
        let (mut app, slider) = app_with_slider(SliderConfig::default());
        let tokens = SliderTokens::default();
        let thumb = parts(&app, slider).thumb;
        let thumb_border = |app: &App| app.world().get::<ThemeBorder>(thumb).unwrap().top.clone();
        assert_eq!(thumb_border(&app), tokens.thumb_border);

        app.world_mut().entity_mut(slider).insert(Hovered(true));
        app.update();
        assert_eq!(
            thumb_border(&app),
            tokens.thumb_border.clone().with_state(WidgetState::Hover)
        );

        app.world_mut()
            .entity_mut(slider)
            .insert((Hovered(false), InteractionDisabled));
        app.update();
        assert_eq!(
            thumb_border(&app),
            tokens
                .thumb_border
                .clone()
                .with_state(WidgetState::Disabled)
        );
        assert_eq!(
            app.world().get::<EntityCursor>(slider),
            Some(&EntityCursor::System(SystemCursorIcon::NotAllowed))
        );

        app.world_mut()
            .entity_mut(slider)
            .remove::<InteractionDisabled>();
        app.update();
        assert_eq!(thumb_border(&app), tokens.thumb_border);
        assert_eq!(
            app.world().get::<EntityCursor>(slider),
            Some(&EntityCursor::System(SystemCursorIcon::ColResize))
        );
    }

    #[test]
    fn new_tokens_restyle_every_part() {
        let (mut app, slider) = app_with_slider(SliderConfig::default().disabled(true));
        let tokens = SliderTokens::with_prefix("test.slider");
        app.world_mut().entity_mut(slider).insert(tokens.clone());
        app.update();
        let parts = parts(&app, slider);
        let world = app.world();
        assert_eq!(
            world.get::<ThemeSkin>(slider),
            Some(&ThemeSkin(tokens.bg.clone()))
        );
        // The new thumb and fill keep the slider's state.
        let dimmed_thumb = tokens.thumb.clone().with_state(WidgetState::Disabled);
        assert_eq!(
            world.get::<ThemeSkin>(parts.thumb),
            Some(&ThemeSkin(dimmed_thumb))
        );
        let dimmed_fill = tokens.fill.clone().with_state(WidgetState::Disabled);
        assert_eq!(
            world.get::<ThemeSkin>(parts.fill),
            Some(&ThemeSkin(dimmed_fill))
        );
    }

    #[test]
    fn spawning_repairs_the_config() {
        let config = SliderConfig::new(40.0)
            .range(0.0..=10.0)
            .step(0.0)
            .precision(12);
        let (app, slider) = app_with_slider(config);
        let world = app.world();
        assert_eq!(world.get::<SliderValue>(slider), Some(&SliderValue(10.0)));
        // No step of 0: Bevy's own step stands.
        assert_eq!(world.get::<SliderStep>(slider).map(|s| s.0), Some(1.0));
        assert_eq!(
            world.get::<SliderPrecision>(slider).map(|p| p.0),
            Some(MAX_DECIMALS)
        );
    }

    /// A backwards range is turned round, and the value held to the range it became.
    #[test]
    fn a_backwards_range_is_turned_round() {
        let (app, slider) = app_with_slider(SliderConfig::new(-5.0).range(10.0..=2.0));
        let range = app.world().get::<SliderRange>(slider).unwrap();
        assert_eq!((range.start(), range.end()), (2.0, 10.0));
        let value = app.world().get::<SliderValue>(slider);
        assert_eq!(value, Some(&SliderValue(2.0)));

        let (app, slider) = app_with_slider(SliderConfig::new(5.0).range(f32::NAN..=2.0));
        let range = app.world().get::<SliderRange>(slider).unwrap();
        assert_eq!((range.start(), range.end()), (0.0, 100.0));
    }

    /// A vertical slider stands up: its thumb, fill and marks rise from the bottom with the
    /// value, across its whole thickness, and Bevy reads its drags up and down.
    #[test]
    fn a_vertical_slider_rises_from_the_bottom() {
        let config = SliderConfig::new(25.0)
            .vertical(true)
            .thickness(16.0)
            .thumb_length(10.0);
        let (mut app, slider) = app_with_slider(config);
        let world = app.world();
        let orientation = world.get::<Slider>(slider).unwrap().orientation;
        assert_eq!(orientation, SliderOrientation::Vertical);
        let track = node(&app, slider);
        assert_eq!((track.width, track.height), (px(16.0), percent(100)));
        assert_eq!(track.min_height, px(DEFAULT_VERTICAL_LENGTH));

        let parts = parts(&app, slider);
        let fill = node(&app, parts.fill);
        assert_eq!(
            (fill.width, fill.height, fill.bottom),
            (percent(100), percent(25.0), px(0))
        );
        let thumb = node(&app, parts.thumb);
        assert_eq!((thumb.width, thumb.height), (percent(100), px(10.0)));
        assert_eq!(
            (thumb.bottom, thumb.margin.bottom),
            (percent(25.0), px(-2.5))
        );
        assert_eq!(thumb.left, Val::Auto, "only the bottom moves it");
        let mark = mark(&mut app);
        assert_eq!(
            (mark.width, mark.height),
            (percent(100), px(DEFAULT_MARK_LENGTH))
        );
        assert_eq!(mark.bottom, percent(75.0));
        assert_eq!(
            app.world().get::<EntityCursor>(slider),
            Some(&EntityCursor::System(SystemCursorIcon::RowResize))
        );
    }

    /// A fader's wide thumb overhangs its thin track evenly, and the slider keeps room for it.
    #[test]
    fn a_thick_thumb_overhangs_its_track() {
        let fader = SliderConfig::default()
            .vertical(true)
            .thickness(6.0)
            .thumb_length(8.0)
            .thumb_thickness(30.0);
        let (app, slider) = app_with_slider(fader);
        let track = node(&app, slider);
        assert_eq!(track.margin, UiRect::horizontal(px(12.0)));
        let thumb = node(&app, parts(&app, slider).thumb);
        assert_eq!((thumb.width, thumb.height), (px(30.0), px(8.0)));
        assert_eq!((thumb.left, thumb.margin.left), (percent(50), px(-15.0)));
    }

    /// A mark takes its own size, and a thick one overhangs the track the same way.
    #[test]
    fn a_mark_takes_its_own_size() {
        let (mut app, slider) = app_with_slider(SliderConfig::default());
        let tall = app
            .world_mut()
            .spawn((
                SliderMark::at(25.0).length(2.0).thickness(24.0),
                ChildOf(slider),
            ))
            .id();
        app.update();
        let tall = node(&app, tall);
        assert_eq!((tall.width, tall.height), (px(2.0), px(24.0)));
        assert_eq!((tall.top, tall.margin.top), (percent(50), px(-12.0)));
        assert_eq!(tall.left, percent(25.0));
    }

    /// A length of its own sizes the track along it, either way it lies.
    #[test]
    fn a_length_sizes_the_track_along_it() {
        let (app, slider) = app_with_slider(SliderConfig::default().length(200.0));
        let track = node(&app, slider);
        assert_eq!(
            (track.width, track.height),
            (px(200.0), px(DEFAULT_THICKNESS))
        );
        let fader = SliderConfig::default().vertical(true).length(200.0);
        let (app, slider) = app_with_slider(fader);
        let track = node(&app, slider);
        assert_eq!(
            (track.width, track.height),
            (px(DEFAULT_THICKNESS), px(200.0))
        );
    }

    #[test]
    fn steps_are_counted_from_the_range_start() {
        let range = SliderRange::new(1.0, 12.0);
        assert_eq!(snap_to_step(5.37, &range, 1.0), 5.0);
        assert_eq!(snap_to_step(5.9, &range, 2.0), 5.0);
        assert_eq!(snap_to_step(11.9, &range, 5.0), 11.0);
        assert_eq!(snap_to_step(40.0, &range, 1.0), 12.0);
    }

    #[test]
    fn a_steps_precision_is_its_decimal_places() {
        assert_eq!(decimal_places(1.0), 0);
        assert_eq!(decimal_places(5.0), 0);
        assert_eq!(decimal_places(0.1), 1);
        assert_eq!(decimal_places(0.25), 2);
    }

    /// A stepped slider's thumb lands on whole steps, however far it's dragged.
    #[test]
    fn a_stepped_slider_lands_on_steps() {
        let config = SliderConfig::new(5.0).range(1.0..=12.0).step(1.0);
        let (mut app, slider) = app_with_slider(config);
        let precision = app.world().get::<SliderPrecision>(slider).map(|p| p.0);
        assert_eq!(precision, Some(0));

        app.world_mut().trigger(ValueChange {
            source: slider,
            value: 7.4_f32,
            is_final: false,
        });
        app.update();
        assert_eq!(
            app.world().get::<SliderValue>(slider),
            Some(&SliderValue(7.0))
        );
    }
}
