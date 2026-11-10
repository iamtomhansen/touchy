//! Number scrubbers: a value you change by dragging across it, with step
//! buttons either side. Hold Shift for a finer step, while dragging or clicking.
//!
//! The gesture is a plain struct, [`Scrubbing`], so all the maths that keeps the
//! value steady (quantizing, hysteresis, clamping, rounding) is tested without
//! an app. Observers only feed it pointer deltas.

use bevy::ecs::entity::EntityHashSet;
use bevy::input::keyboard::KeyCode;
use bevy::picking::cursor::{CursorIconPlugin, EntityCursor};
use bevy::picking::events::{PointerDrag, PointerDragEnd, PointerDragStart};
use bevy::picking::hover::Hovered;
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use bevy::ui::UiSystems;
use bevy::ui_widgets::{Activate, ValueChange};
use bevy::window::SystemCursorIcon;

use crate::EnsurePlugin;
use crate::sanitize::{at_least, at_most};
use crate::theme::palette::{
    CONTROL, CONTROL_CORNER_RADIUS, EDGE, FONT, HIGHLIGHT, SMALL_SIZE, SURFACE, TEXT, TEXT_MUTED,
};
use crate::theme::skins;
use crate::theme::{
    Shade, Theme, ThemeBorder, ThemeCornerRadius, ThemeFont, ThemeFontSize, ThemePlugin, ThemeSkin,
    ThemeTextColor, ThemeToken, WidgetFlags, WidgetState,
};
use crate::widgets::button::{ButtonConfig, ButtonTokens, ButtonWidgetPlugin, spawn_button};

/// Prefix of the scrubber widget's theme tokens.
pub const SCRUBBER_STYLE: &str = "touchy.scrubber";

/// The keys that make steps finer while held. Rebinding comes with the crate's key mapping.
const FINE_KEYS: [KeyCode; 2] = [KeyCode::ShiftLeft, KeyCode::ShiftRight];
/// Step multiplier while a fine key is held.
const FINE_FACTOR: f64 = 0.1;
/// How far past a step the pointer must move before the value changes. Prevents flicker.
const DEAD_ZONE: f64 = 0.15;

/// Pixels of drag per step, normal or fine, and the least allowed.
const DEFAULT_PIXELS_PER_STEP: f32 = 10.0;
const MIN_PIXELS_PER_STEP: f32 = 0.5;
/// Decimal places the value is rounded to and shown with, by default and at most.
const DEFAULT_PRECISION: u32 = 3;
const MAX_PRECISION: u32 = 6;

/// The drag area's least width, the space inside its border, and its border width.
const AREA_MIN_WIDTH: f32 = 36.0;
const AREA_PADDING: Vec2 = Vec2::new(4.0, 1.0);
const AREA_BORDER: f32 = 1.0;
/// Space between the label, the step buttons and the drag area.
const ROW_GAP: f32 = 2.0;
/// A step button's least width and height, and the space inside its border.
const STEP_BUTTON_SIZE: Vec2 = Vec2::splat(18.0);
const STEP_BUTTON_PADDING: Vec2 = Vec2::new(4.0, 0.0);

/// Theme tokens for a scrubber widget. Insert new ones to restyle a scrubber.
///
/// The drag area's hovered and dragged looks are its tokens in a [`WidgetState`];
/// dragging is [`WidgetState::Pressed`].
#[derive(Component, Clone, PartialEq, Debug)]
#[component(immutable)]
pub struct ScrubberTokens {
    /// Background of the drag area.
    pub bg: ThemeToken,
    /// The border of the drag area.
    pub border: ThemeToken,
    /// The colour of the value.
    pub value: ThemeToken,
    /// The colour of the label, if any.
    pub label: ThemeToken,
    /// The font of the value and the label.
    pub font: ThemeToken,
    /// The font size of the value and the label.
    pub font_size: ThemeToken,
    /// The radius of the drag area's corners.
    pub corner_radius: ThemeToken,
    /// The step buttons' look.
    pub step: ButtonTokens,
}

impl ScrubberTokens {
    /// Names the tokens under `prefix`, and the step buttons' under `{prefix}.step`.
    pub fn with_prefix(prefix: &str) -> Self {
        let token = |suffix: &str| ThemeToken::new(format!("{prefix}.{suffix}"));
        Self {
            bg: token("area.bg"),
            border: token("area.border"),
            value: token("value"),
            label: token("label"),
            font: token("font"),
            font_size: token("font_size"),
            corner_radius: token("corner_radius"),
            step: ButtonTokens::with_prefix(&format!("{prefix}.step")),
        }
    }

    /// Derives these tokens from the palette, unless the app sets them.
    pub fn register_theme_defaults(&self, theme: &mut Theme) {
        let dragged_border = self.border.clone().with_state(WidgetState::Pressed);
        theme
            .default_derived(self.bg.clone(), SURFACE, Shade::Same)
            .default_derived(self.border.clone(), EDGE, Shade::Same)
            .default_derived(dragged_border, HIGHLIGHT, Shade::Same)
            .default_derived(self.value.clone(), TEXT, Shade::Same)
            .default_derived(self.label.clone(), TEXT_MUTED, Shade::Same)
            .default_font_from(self.font.clone(), FONT)
            .default_size_from(self.font_size.clone(), SMALL_SIZE)
            .default_size_scaled(self.corner_radius.clone(), CONTROL_CORNER_RADIUS, 0.5);
        self.step.derive_defaults(theme, CONTROL);
        // Each part draws its role's skin, once a theme gives the role one.
        theme.default_skin_from(self.bg.clone(), skins::SUNKEN);
    }

    /// The drag area's background and border in `state`.
    fn area_look(&self, state: Option<WidgetState>) -> (ThemeSkin, ThemeBorder) {
        (
            ThemeSkin(self.bg.clone().with_state(state)),
            ThemeBorder::all(self.border.clone().with_state(state)),
        )
    }
}

/// The built-in token set.
impl Default for ScrubberTokens {
    fn default() -> Self {
        Self::with_prefix(SCRUBBER_STYLE)
    }
}

/// The scrubber's value. Write it to change the value from code.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct ScrubValue(pub f64);

/// How a scrubber's value may change. Change it to change how the scrubber
/// behaves; keep `step` and `pixels_per_step` above zero, and `min` no more than `max`.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct ScrubSettings {
    /// The change in value of one step.
    pub step: f64,
    /// Minimum reachable value.
    pub min: f64,
    /// Maximum reachable value.
    pub max: f64,
    /// Decimal places the value is rounded to, and shown with.
    pub precision: u32,
    /// Pixels of drag per step; more is less sensitive.
    pub pixels_per_step: f32,
}

impl ScrubSettings {
    fn effective_step(&self, fine: bool) -> f64 {
        self.step * if fine { FINE_FACTOR } else { 1.0 }
    }

    fn clamp(&self, value: f64) -> f64 {
        value.clamp(self.min, self.max)
    }

    /// Rounds to `precision` decimals, which also absorbs float noise such as
    /// `0.1 * 3 = 0.30000000000000004`. Never returns `-0.0`.
    fn round(&self, value: f64) -> f64 {
        let scale = 10f64.powi(self.precision as i32);
        let rounded = (value * scale).round() / scale;
        if rounded == 0.0 { 0.0 } else { rounded }
    }

    /// The value one (fine) step from `value` in `direction` (+1 or -1).
    fn stepped(&self, value: f64, direction: f64, fine: bool) -> f64 {
        self.round(self.clamp(value + direction * self.effective_step(fine)))
    }

    fn format(&self, value: f64) -> String {
        format!("{:.*}", self.precision as usize, value)
    }
}

/// A drag in progress, on the scrubber's drag area.
///
/// `raw` follows the pointer exactly; the shown value moves in whole steps
/// from where the drag started.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct Scrubbing {
    anchor: f64,
    raw: f64,
    fine: bool,
}

impl Scrubbing {
    /// Starts a drag at `value`. A value the app set to NaN or infinity starts from 0 (clamped).
    fn begin(value: f64, settings: &ScrubSettings, fine: bool) -> Self {
        let start = if value.is_finite() {
            value
        } else {
            settings.clamp(0.0)
        };
        Self {
            anchor: start,
            raw: start,
            fine,
        }
    }

    /// Moves the drag by `delta_px` and returns the value to show, given the one shown now.
    fn drag(&mut self, delta_px: f32, settings: &ScrubSettings, fine: bool, shown: f64) -> f64 {
        // Toggling Shift mid-drag restarts the step grid from the current value,
        // so the value never jumps when the scale changes.
        if fine != self.fine {
            *self = Self::begin(shown, settings, fine);
        }
        let step = settings.effective_step(fine);
        // Clamping the raw position, not just the result, means dragging back
        // from past a bound responds at once, with nothing to unwind.
        let moved = f64::from(delta_px / settings.pixels_per_step) * step;
        self.raw = settings.clamp(self.raw + moved);

        let at_bound = self.raw <= settings.min || self.raw >= settings.max;
        let held = (self.raw - shown).abs() < step * (0.5 + DEAD_ZONE);
        if held && !at_bound {
            return shown;
        }
        let steps = ((self.raw - self.anchor) / step).round();
        settings.round(settings.clamp(self.anchor + steps * step))
    }
}

/// The parts of a scrubber, on the scrubber.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScrubberParts {
    /// The label, if it has one.
    pub label: Option<Entity>,
    /// The area dragged to change the value.
    pub area: Entity,
    /// The text showing the value, in the area.
    pub value: Entity,
    /// The step buttons, down and up, if it has them.
    pub steps: Option<ScrubberStepButtons>,
}

/// A scrubber's step buttons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScrubberStepButtons {
    /// The button that steps the value down.
    pub down: Entity,
    /// The button that steps the value up.
    pub up: Entity,
}

/// Construction-time configuration for a scrubber.
///
/// Consumed by [`SpawnUi::scrubber`](crate::spawn_ui::SpawnUi::scrubber) and not retained as a
/// component.
///
/// # Examples
/// ```
/// use touchy::prelude::*;
///
/// let config = ScrubberConfig::new(10.0)
///     .step(0.5)
///     .range(-10.0, 10.0)
///     .precision(1);
/// # assert_eq!((config.min, config.max), (-10.0, 10.0));
/// ```
#[derive(Clone, Debug)]
pub struct ScrubberConfig {
    /// The initial value. Will be clamped and rounded to the settings.
    pub value: f64,
    /// The change in value of one step.
    pub step: f64,
    /// Minimum reachable value. Unbounded by default.
    pub min: f64,
    /// Maximum reachable value. Unbounded by default.
    pub max: f64,
    /// Decimal places the value is rounded to and shown with, at most six.
    pub precision: u32,
    /// Pixels of drag per step; more is less sensitive.
    pub pixels_per_step: f32,
    /// Shown before the controls, such as an axis name.
    pub label: Option<String>,
    /// Whether it has step buttons either side of the drag area.
    pub step_buttons: bool,
    /// The theme tokens for a scrubber.
    pub tokens: ScrubberTokens,
}

impl Default for ScrubberConfig {
    fn default() -> Self {
        Self {
            value: 0.0,
            step: 1.0,
            min: f64::NEG_INFINITY,
            max: f64::INFINITY,
            precision: DEFAULT_PRECISION,
            pixels_per_step: DEFAULT_PIXELS_PER_STEP,
            label: None,
            step_buttons: true,
            tokens: ScrubberTokens::with_prefix(SCRUBBER_STYLE),
        }
    }
}

impl ScrubberConfig {
    /// Create a new scrubber config with the given initial value.
    pub fn new(value: f64) -> Self {
        Self { value, ..default() }
    }

    /// Set how much the value increments each step.
    pub fn step(mut self, step: f64) -> Self {
        self.step = step;
        self
    }

    /// Set the upper and lower bounds that can be scrubbed to.
    pub fn range(mut self, min: f64, max: f64) -> Self {
        self.min = min;
        self.max = max;
        self
    }

    /// Set how many decimal places the value is rounded to and shown with.
    pub fn precision(mut self, decimal_places: u32) -> Self {
        self.precision = decimal_places;
        self
    }

    /// Sets how many pixels of drag make one step.
    pub fn pixels_per_step(mut self, pixels: f32) -> Self {
        self.pixels_per_step = pixels;
        self
    }

    /// Sets the label shown before the controls.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Sets whether it has step buttons.
    pub fn step_buttons(mut self, step_buttons: bool) -> Self {
        self.step_buttons = step_buttons;
        self
    }

    /// Sets its look.
    pub fn tokens(mut self, tokens: ScrubberTokens) -> Self {
        self.tokens = tokens;
        self
    }

    /// This config with every value made usable, warning about each repair.
    fn sanitized(mut self) -> Self {
        self.precision = at_most(self.precision, MAX_PRECISION, "ScrubberConfig precision");
        // A step finer than the value is shown would round away to nothing.
        let least_step = 10f64.powi(-(self.precision as i32));
        self.step = at_least(self.step, least_step, "ScrubberConfig step");
        self.max = at_least(self.max, self.min, "ScrubberConfig max");
        self.pixels_per_step = at_least(
            self.pixels_per_step,
            MIN_PIXELS_PER_STEP,
            "ScrubberConfig pixels per step",
        );
        if !self.value.is_finite() {
            warn!("ScrubberConfig value was {}; using 0", self.value);
            self.value = 0.0;
        }
        self
    }

    /// How the scrubber's value may change.
    fn settings(&self) -> ScrubSettings {
        ScrubSettings {
            step: self.step,
            min: self.min,
            max: self.max,
            precision: self.precision,
            pixels_per_step: self.pixels_per_step,
        }
    }
}

/// Spawns it; documented on [`SpawnUi::scrubber`](crate::spawn_ui::SpawnUi::scrubber).
pub(crate) fn spawn_scrubber<'a>(
    parent: &'a mut ChildSpawnerCommands,
    config: ScrubberConfig,
) -> EntityCommands<'a> {
    let config = config.sanitized();
    let settings = config.settings();
    let value = settings.round(settings.clamp(config.value));

    let mut scrubber = parent.spawn((
        ScrubValue(value),
        settings,
        Node {
            align_items: AlignItems::Center,
            column_gap: px(ROW_GAP),
            ..default()
        },
    ));
    let mut label = None;
    let mut area = Entity::PLACEHOLDER;
    let mut shown = Entity::PLACEHOLDER;
    let mut steps = None;
    scrubber.with_children(|row| {
        label = config.label.map(|text| row.spawn(Text::new(text)).id());
        let step_button = |row: &mut ChildSpawnerCommands, text: &str| {
            let button = ButtonConfig {
                size: STEP_BUTTON_SIZE,
                padding: STEP_BUTTON_PADDING,
                tokens: Some(config.tokens.step.clone()),
                ..ButtonConfig::new(text)
            };
            spawn_button(row, button).observe(step_on_activate).id()
        };
        let down = config.step_buttons.then(|| step_button(row, "-"));
        area = row
            .spawn((
                Hovered::default(),
                EntityCursor::System(SystemCursorIcon::ColResize),
                Node {
                    min_width: px(AREA_MIN_WIDTH),
                    padding: UiRect::axes(px(AREA_PADDING.x), px(AREA_PADDING.y)),
                    border: UiRect::all(px(AREA_BORDER)),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
            ))
            .with_children(|area| {
                let text = Text::new(settings.format(value));
                shown = area.spawn((Pickable::IGNORE, text)).id();
            })
            .observe(start_scrub)
            .observe(scrub)
            .observe(end_scrub)
            .id();
        let up = config.step_buttons.then(|| step_button(row, "+"));
        steps = down
            .zip(up)
            .map(|(down, up)| ScrubberStepButtons { down, up });
    });
    // Its look comes with its tokens, in `style_scrubber`.
    scrubber.insert((
        ScrubberParts {
            label,
            area,
            value: shown,
            steps,
        },
        config.tokens,
    ));
    scrubber
}

/// Whether a key that makes steps finer is held.
fn fine_held(keyboard: Option<Res<ButtonInput<KeyCode>>>) -> bool {
    keyboard.is_some_and(|keys| keys.any_pressed(FINE_KEYS))
}

/// Sets the scrubber's value and announces it.
fn change_value(commands: &mut Commands, scrubber: Entity, value: f64, is_final: bool) {
    commands.entity(scrubber).insert(ScrubValue(value));
    commands.trigger(ValueChange {
        source: scrubber,
        value,
        is_final,
    });
}

/// Starts a drag on a scrubber's area.
fn start_scrub(
    start: On<PointerDragStart>,
    areas: Query<&ChildOf>,
    scrubbers: Query<(&ScrubValue, &ScrubSettings)>,
    keyboard: Option<Res<ButtonInput<KeyCode>>>,
    mut commands: Commands,
) {
    if start.button != PointerButton::Primary {
        return;
    }
    let area = start.entity;
    if let Ok(scrubber) = areas.get(area)
        && let Ok((value, settings)) = scrubbers.get(scrubber.parent())
    {
        let gesture = Scrubbing::begin(value.0, settings, fine_held(keyboard));
        commands.entity(area).insert(gesture);
    }
}

/// Moves a drag on a scrubber's area, changing the value in whole steps.
fn scrub(
    drag: On<PointerDrag>,
    mut areas: Query<(&ChildOf, &mut Scrubbing)>,
    scrubbers: Query<(&ScrubValue, &ScrubSettings)>,
    keyboard: Option<Res<ButtonInput<KeyCode>>>,
    mut commands: Commands,
) {
    let Ok((scrubber, mut gesture)) = areas.get_mut(drag.entity) else {
        return;
    };
    let Ok((shown, settings)) = scrubbers.get(scrubber.parent()) else {
        return;
    };
    let next = gesture.drag(drag.delta.x, settings, fine_held(keyboard), shown.0);
    if next != shown.0 {
        change_value(&mut commands, scrubber.parent(), next, false);
    }
}

/// Ends a drag on a scrubber's area, announcing the value it settled on.
fn end_scrub(
    end: On<PointerDragEnd>,
    areas: Query<&ChildOf, With<Scrubbing>>,
    scrubbers: Query<&ScrubValue>,
    mut commands: Commands,
) {
    let area = end.entity;
    let Ok(scrubber) = areas.get(area) else {
        return;
    };
    commands.entity(area).remove::<Scrubbing>();
    if let Ok(value) = scrubbers.get(scrubber.parent()) {
        change_value(&mut commands, scrubber.parent(), value.0, true);
    }
}

/// Steps the value when a step button is pressed.
fn step_on_activate(
    activate: On<Activate>,
    buttons: Query<&ChildOf>,
    scrubbers: Query<(&ScrubValue, &ScrubSettings, &ScrubberParts)>,
    keyboard: Option<Res<ButtonInput<KeyCode>>>,
    mut commands: Commands,
) {
    let button = activate.entity;
    let Ok(scrubber) = buttons.get(button) else {
        return;
    };
    let Ok((value, settings, parts)) = scrubbers.get(scrubber.parent()) else {
        return;
    };
    let Some(steps) = parts.steps else {
        return;
    };
    let direction = if button == steps.up { 1.0 } else { -1.0 };
    let next = settings.stepped(value.0, direction, fine_held(keyboard));
    if next != value.0 {
        change_value(&mut commands, scrubber.parent(), next, true);
    }
}

/// A scrubber's value or how it's shown changed.
type ShownChanged = Or<(Changed<ScrubValue>, Changed<ScrubSettings>)>;

/// Shows the value whenever it or its precision changes, including when the app sets them.
fn show_values(
    scrubbers: Query<(&ScrubValue, &ScrubSettings, &ScrubberParts), ShownChanged>,
    mut texts: Query<&mut Text>,
) {
    for (value, settings, parts) in &scrubbers {
        let shown = settings.format(value.0);
        if let Ok(mut text) = texts.get_mut(parts.value)
            && text.0 != shown
        {
            text.0 = shown;
        }
    }
}

/// A drag area was hovered, unhovered or grabbed. Releases are read separately,
/// because change detection doesn't see removals.
type AreaChanged = Or<(Changed<Hovered>, Added<Scrubbing>)>;

/// A drag area's state, its scrubber, and its current look to compare against.
type AreaLook = (
    &'static Hovered,
    Has<Scrubbing>,
    &'static ChildOf,
    &'static ThemeSkin,
    &'static ThemeBorder,
);

/// Restyles drag areas when hovered, grabbed or released.
fn restyle_areas(
    changed: Query<Entity, AreaChanged>,
    mut released: RemovedComponents<Scrubbing>,
    areas: Query<AreaLook>,
    scrubbers: Query<(&ScrubberTokens, &ScrubberParts)>,
    mut commands: Commands,
) {
    let mut dirty: EntityHashSet = changed.iter().collect();
    dirty.extend(released.read());
    for entity in dirty {
        let Ok((hovered, dragged, scrubber, old_bg, old_border)) = areas.get(entity) else {
            continue;
        };
        let Ok((tokens, parts)) = scrubbers.get(scrubber.parent()) else {
            continue;
        };
        if parts.area != entity {
            continue; // Something else hovered in a scrubber, such as a step button.
        }
        let state = WidgetFlags {
            hovered: hovered.0,
            pressed: dragged,
            ..default()
        }
        .state();
        let (bg, border) = tokens.area_look(state);
        if *old_bg != bg {
            commands.entity(entity).insert(bg);
        }
        if *old_border != border {
            commands.entity(entity).insert(border);
        }
    }
}

/// Styles a scrubber when its tokens are set: at spawn, and whenever the app sets new ones.
fn style_scrubber(
    insert: On<Insert<ScrubberTokens>>,
    scrubbers: Query<(&ScrubberTokens, &ScrubberParts)>,
    areas: Query<(&Hovered, Has<Scrubbing>)>,
    mut commands: Commands,
) {
    let Ok((tokens, parts)) = scrubbers.get(insert.entity) else {
        return;
    };
    let state = areas.get(parts.area).ok().and_then(|(hovered, dragged)| {
        WidgetFlags {
            hovered: hovered.0,
            pressed: dragged,
            ..default()
        }
        .state()
    });
    let (bg, border) = tokens.area_look(state);
    commands.entity(parts.area).insert((
        bg,
        border,
        ThemeCornerRadius(tokens.corner_radius.clone()),
    ));
    let font = (
        ThemeFont(tokens.font.clone()),
        ThemeFontSize(tokens.font_size.clone()),
    );
    commands
        .entity(parts.value)
        .insert((font.clone(), ThemeTextColor(tokens.value.clone())));
    if let Some(label) = parts.label {
        commands
            .entity(label)
            .insert((font, ThemeTextColor(tokens.label.clone())));
    }
    if let Some(steps) = parts.steps {
        for button in [steps.down, steps.up] {
            commands.entity(button).insert(tokens.step.clone());
        }
    }
}

/// Scrubber behaviour and default token values.
pub struct ScrubberWidgetPlugin;

impl Plugin for ScrubberWidgetPlugin {
    fn build(&self, app: &mut App) {
        app.ensure_plugin(CursorIconPlugin)
            .ensure_plugin(ThemePlugin)
            .ensure_plugin(ButtonWidgetPlugin)
            .add_observer(style_scrubber)
            .add_systems(
                PostUpdate,
                (show_values, restyle_areas).before(UiSystems::Prepare),
            );
        ScrubberTokens::with_prefix(SCRUBBER_STYLE)
            .register_theme_defaults(&mut app.world_mut().resource_mut::<Theme>());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> ScrubSettings {
        ScrubSettings {
            step: 0.1,
            min: 0.0,
            max: 1.0,
            precision: 3,
            pixels_per_step: 10.0,
        }
    }

    /// Drags by each delta in turn, returning the value shown after each.
    fn drag_through(start: f64, deltas: &[f32], fine: bool) -> Vec<f64> {
        let settings = settings();
        let mut gesture = Scrubbing::begin(start, &settings, fine);
        let mut shown = start;
        deltas
            .iter()
            .map(|&delta| {
                shown = gesture.drag(delta, &settings, fine, shown);
                shown
            })
            .collect()
    }

    #[test]
    fn small_wobbles_and_boundary_tremor_dont_change_the_value() {
        // A click that wobbles a few pixels changes nothing.
        assert_eq!(drag_through(0.5, &[3.0, -2.0, 4.0], false), [0.5, 0.5, 0.5]);
        // Hovering around the midpoint between 0.5 and 0.6 (5 px) doesn't flicker.
        assert_eq!(
            drag_through(0.5, &[5.5, -1.0, 1.0, -1.0], false),
            [0.5, 0.5, 0.5, 0.5]
        );
        // Clearly past it, the value moves one step, and float noise is rounded away.
        assert_eq!(drag_through(0.2, &[7.0], false), [0.3]);
    }

    #[test]
    fn bounds_are_reached_exactly_and_never_need_unwinding() {
        // Far past the maximum, then straight back: the value responds at once.
        let values = drag_through(0.5, &[500.0, -10.0], false);
        assert_eq!(values, [1.0, 0.9]);
        assert!(drag_through(0.0, &[-500.0], false)[0] == 0.0);
    }

    #[test]
    fn fine_mode_is_finer_and_toggling_it_never_jumps() {
        assert_eq!(drag_through(0.5, &[10.0], true), [0.51]);
        let settings = settings();
        let mut gesture = Scrubbing::begin(0.5, &settings, false);
        let coarse = gesture.drag(14.0, &settings, false, 0.5);
        assert_eq!(coarse, 0.6);
        // Pressing Shift mid-drag continues from 0.6 in fine steps.
        let fine = gesture.drag(10.0, &settings, true, coarse);
        assert_eq!(fine, 0.61);
    }

    #[test]
    fn rounding_never_shows_negative_zero_and_bad_input_is_contained() {
        let settings = ScrubSettings {
            min: -1.0,
            ..settings()
        };
        assert_eq!(settings.format(settings.round(-0.0001)), "0.000");
        let gesture = Scrubbing::begin(f64::NAN, &settings, false);
        assert_eq!(gesture.raw, 0.0, "a NaN value restarts from zero");
    }

    #[test]
    fn unusable_configs_are_repaired() {
        let config = ScrubberConfig {
            value: f64::NAN,
            pixels_per_step: 0.0,
            ..ScrubberConfig::new(0.0)
                .range(5.0, -5.0)
                .precision(9)
                .step(0.0)
        }
        .sanitized();
        assert_eq!(config.precision, MAX_PRECISION);
        assert_eq!(config.step, 1e-6, "the finest step the value shows");
        assert_eq!((config.min, config.max), (5.0, 5.0));
        assert_eq!(config.pixels_per_step, MIN_PIXELS_PER_STEP);
        assert_eq!(config.value, 0.0);
    }

    fn scrubber_app(config: ScrubberConfig) -> (App, Entity) {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, ScrubberWidgetPlugin))
            .init_resource::<Announced>()
            .add_observer(
                |change: On<ValueChange<f64>>, mut announced: ResMut<Announced>| {
                    announced.0.push((change.value, change.is_final));
                },
            );
        let mut scrubber = None;
        app.world_mut()
            .commands()
            .spawn(Node::default())
            .with_children(|root| scrubber = Some(spawn_scrubber(root, config).id()));
        app.update();
        (app, scrubber.unwrap())
    }

    fn parts(app: &App, scrubber: Entity) -> ScrubberParts {
        *app.world().get::<ScrubberParts>(scrubber).unwrap()
    }

    fn shown(app: &App, scrubber: Entity) -> String {
        let value = parts(app, scrubber).value;
        app.world().get::<Text>(value).unwrap().0.clone()
    }

    #[test]
    fn step_buttons_change_the_value_and_announce_it() {
        let config = ScrubberConfig::new(0.5).step(0.1).range(0.0, 1.0);
        let (mut app, scrubber) = scrubber_app(config);
        let up = parts(&app, scrubber).steps.unwrap().up;
        app.world_mut().trigger(Activate { entity: up });
        app.update();

        assert_eq!(
            app.world().get::<ScrubValue>(scrubber),
            Some(&ScrubValue(0.6))
        );
        assert_eq!(app.world().resource::<Announced>().0, [(0.6, true)]);
        assert_eq!(shown(&app, scrubber), "0.600");
    }

    #[test]
    fn values_and_precisions_set_by_the_app_are_shown() {
        let (mut app, scrubber) = scrubber_app(ScrubberConfig::new(2.0).step_buttons(false));
        assert_eq!(parts(&app, scrubber).steps, None);
        app.world_mut()
            .entity_mut(scrubber)
            .insert(ScrubValue(7.25));
        app.update();
        assert_eq!(shown(&app, scrubber), "7.250");

        app.world_mut()
            .get_mut::<ScrubSettings>(scrubber)
            .unwrap()
            .precision = 1;
        app.update();
        assert_eq!(shown(&app, scrubber), "7.2");
    }

    #[test]
    fn new_tokens_restyle_every_part() {
        let (mut app, scrubber) = scrubber_app(ScrubberConfig::new(0.0).label("X"));
        let tokens = ScrubberTokens::with_prefix("game.axis");
        app.world_mut().entity_mut(scrubber).insert(tokens.clone());
        app.update();

        let parts = parts(&app, scrubber);
        let world = app.world();
        let label = parts.label.unwrap();
        assert_eq!(world.get::<ThemeTextColor>(label).unwrap().0, tokens.label);
        assert_eq!(world.get::<ThemeFont>(label).unwrap().0, tokens.font);
        assert_eq!(world.get::<ThemeSkin>(parts.area).unwrap().0, tokens.bg);
        let up = parts.steps.unwrap().up;
        assert_eq!(world.get::<ButtonTokens>(up), Some(&tokens.step));
    }

    #[derive(Resource, Default)]
    struct Announced(Vec<(f64, bool)>);
}
