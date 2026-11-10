//! A number field with step buttons, like HTML's `<input type="number">`.
//!
//! A number can be typed and/or have buttons on either side (`[−][ 12 ][+]`),
//! or in a stack on the right (`[ 12 ][±]`). Without a field at all it's a
//! stepper whose value the app shows however it likes.
//!
//! A typed number is announced when the field commits (Enter, or clicking away).

use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input_focus::FocusedInput;
use bevy::input_focus::tab_navigation::TabIndex;

use bevy::prelude::*;
use bevy::text::{EditableText, TextReadWriteMode};
use bevy::ui::{InteractionDisabled, UiSystems};
use bevy::ui_widgets::{Activate, ValueChange};
use bevy::window::SystemCursorIcon;

use super::button::{ButtonConfig, ButtonTokens, spawn_button};
use super::text_input::{
    TEXT_INPUT_STYLE, TextInputConfig, TextInputTokens, TextInputValue, TextInputWidgetPlugin,
    spawn_text_parts,
};
use crate::EnsurePlugin;
use crate::cursor::HoverCursor;
use crate::sanitize::{at_least, at_most};
use crate::theme::palette::CONTROL;
use crate::theme::{FocusVisible, Theme, ThemePlugin};
use crate::widgets::button::ButtonWidgetPlugin;

/// Prefix of the number input widget's theme tokens.
pub const NUMBER_INPUT_STYLE: &str = "touchy.number_input";

const DECREASE_LABEL: &str = "-";
const INCREASE_LABEL: &str = "+";
/// Space between the field and buttons beside it.
const GAP: f32 = 4.0;
/// A side button's least width and height.
const SIDE_BUTTON_SIZE: f32 = 28.0;
/// Space between the button's label and its edge.
const SIDE_BUTTON_PADDING: Vec2 = Vec2::new(8.0, 4.0);
/// Each stacked button is half the field's height.
const STACKED_BUTTON_WIDTH: f32 = 22.0;
const STACKED_BUTTON_HEIGHT: f32 = 14.0;
const STACKED_BUTTON_BORDER: f32 = 1.0;
/// How many characters wide the field is unless the config says otherwise.
const DEFAULT_COLUMNS: f32 = 6.0;
/// The most decimal places a value is rounded to and shown with, set or
/// worked out from the step, for steps like 1/3.
const MAX_DECIMALS: u32 = 6;
const STEP_TOLERANCE: f64 = 1e-9;

/// Where the step buttons go.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StepButtons {
    /// Decrease on the left, increase on the right.
    #[default]
    Sides,
    /// Increase above decrease, on the right.
    Stacked,
    /// No buttons; the arrow keys still step while the input is focused.
    None,
}

/// What the number input's field does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NumberField {
    /// Shows the number, and takes it typed.
    #[default]
    Typed,
    /// Shows the number only; the buttons and the arrow keys step it.
    Shown,
    /// No field: the app shows the value its own way, such as dots. The
    /// buttons and the arrow keys step it.
    None,
}

/// Theme tokens for a number input widget. Insert new ones to restyle an input.
#[derive(Component, Clone, PartialEq, Debug)]
#[component(immutable)]
pub struct NumberInputTokens {
    /// The number input field.
    pub field: TextInputTokens,
    /// The step buttons, if any, are control-coloured by default.
    pub step: ButtonTokens,
}

impl NumberInputTokens {
    /// Names the tokens under `prefix`, and the buttons' under `{prefix}.step`.
    pub fn with_prefix(prefix: &str) -> Self {
        Self {
            field: TextInputTokens::with_prefix(prefix),
            step: ButtonTokens::with_prefix(&format!("{prefix}.step")),
        }
    }

    /// The field follows the text input's tokens, and the buttons are
    /// control-coloured, unless the app sets them.
    pub fn register_theme_defaults(&self, theme: &mut Theme) {
        self.field
            .follow(theme, &TextInputTokens::with_prefix(TEXT_INPUT_STYLE));
        self.step.derive_defaults(theme, CONTROL);
    }
}

/// The built-in token set.
impl Default for NumberInputTokens {
    fn default() -> Self {
        Self::with_prefix(NUMBER_INPUT_STYLE)
    }
}

/// The input's value. Insert a new one to change it from code; it's clamped
/// and rounded like any other, and isn't announced.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct NumberValue(pub f64);

/// How the value may change.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
struct NumberRules {
    min: f64,
    max: f64,
    step: f64,
    precision: u32,
}

impl NumberRules {
    /// Clamps to the range and rounds to the precision, which also absorbs
    /// float noise such as `0.1 * 3 = 0.30000000000000004`. Never `-0.0`.
    fn settle(&self, value: f64) -> f64 {
        let scale = 10f64.powi(self.precision as i32);
        let rounded = (value.clamp(self.min, self.max) * scale).round() / scale;
        if rounded == 0.0 { 0.0 } else { rounded }
    }

    /// Steps from `value` in the given direction, then settles.
    fn stepped(&self, value: f64, direction: f64) -> f64 {
        self.settle(value + direction * self.step)
    }

    /// Formats the value as a string with the specified precision.
    fn format(&self, value: f64) -> String {
        format!("{:.*}", self.precision as usize, value)
    }

    /// The number in typed text, if it is one. Anything the field's filter
    /// lets through but isn't a number, such as `-` alone, gives `None`.
    fn parse(&self, text: &str) -> Option<f64> {
        text.trim()
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite())
    }
}

/// How many decimal places `step` needs: 1 needs none, 0.25 needs two.
fn decimal_places(step: f64) -> u32 {
    (0..MAX_DECIMALS)
        .find(|&places| {
            let scaled = step * 10f64.powi(places as i32);
            (scaled - scaled.round()).abs() < STEP_TOLERANCE * scaled.abs().max(1.0)
        })
        .unwrap_or(MAX_DECIMALS)
}

/// Characters a number can be typed with.
fn number_char(c: char) -> bool {
    c.is_ascii_digit() || c == '.' || c == '-'
}

/// Spawn time configuration for a number input widget.
///
/// Consumed by [`SpawnUi::number_input`](crate::spawn_ui::SpawnUi::number_input), which returns the
/// number input's entity.
///
/// ```
/// use touchy::prelude::*;
///
/// // A volume, in tenths.
/// let config = NumberInputConfig::new(0.5)
///     .range(0.0, 1.0)
///     .step(0.1);
/// # assert_eq!((config.min, config.max), (0.0, 1.0));
/// ```
#[derive(Clone, Debug)]
pub struct NumberInputConfig {
    /// The starting value; clamped and rounded like any other.
    pub value: f64,
    /// The least the value can be, inclusive; unbounded by default.
    pub min: f64,
    /// The most the value can be, inclusive; unbounded by default.
    pub max: f64,
    /// How much a button or arrow key changes the value.
    pub step: f64,
    /// Decimal places the value is rounded to and shown with. Without one,
    /// as many as the step needs.
    pub precision: Option<u32>,
    /// Where the step buttons go, if anywhere.
    pub buttons: StepButtons,
    /// Whether the number can be typed, only shown, or isn't shown at all.
    pub field: NumberField,
    /// The field's width, in characters.
    pub columns: f32,
    /// Whether the input starts disabled.
    pub disabled: bool,
    /// The input's theme tokens.
    pub tokens: NumberInputTokens,
}

impl NumberInputConfig {
    /// Creates a new config with the given value to start.
    pub fn new(value: f64) -> Self {
        Self { value, ..default() }
    }

    /// Sets the least and most the value can be.
    pub fn range(mut self, min: f64, max: f64) -> Self {
        self.min = min;
        self.max = max;
        self
    }

    /// Sets how much a button or arrow key changes the value.
    pub fn step(mut self, step: f64) -> Self {
        self.step = step;
        self
    }

    /// Sets how many decimal places the value is rounded to and shown with.
    pub fn precision(mut self, decimal_places: u32) -> Self {
        self.precision = Some(decimal_places);
        self
    }

    /// Sets where the step buttons go.
    pub fn buttons(mut self, buttons: StepButtons) -> Self {
        self.buttons = buttons;
        self
    }

    /// Sets what the field does: typed, only shown, or left out.
    pub fn field(mut self, field: NumberField) -> Self {
        self.field = field;
        self
    }

    /// Sets the field's width, in characters.
    pub fn columns(mut self, columns: f32) -> Self {
        self.columns = columns;
        self
    }

    /// Sets whether the input starts disabled.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Sets the input's theme tokens.
    pub fn tokens(mut self, tokens: NumberInputTokens) -> Self {
        self.tokens = tokens;
        self
    }

    /// This config with every value made usable, warning about each repair.
    fn sanitized(mut self) -> Self {
        self.max = at_least(self.max, self.min, "NumberInputConfig max");
        // A step finer than any value is shown would never move it.
        let least_step = 10f64.powi(-(MAX_DECIMALS as i32));
        self.step = at_least(self.step, least_step, "NumberInputConfig step");
        self.precision = at_most(self.precision, MAX_DECIMALS, "NumberInputConfig precision");
        self.columns = at_least(self.columns, 1.0, "NumberInputConfig columns");
        if !self.value.is_finite() {
            warn!("NumberInputConfig value was {}; using 0", self.value);
            self.value = 0.0;
        }
        self
    }

    /// How the value may change.
    fn rules(&self) -> NumberRules {
        NumberRules {
            min: self.min,
            max: self.max,
            step: self.step,
            precision: self.precision.unwrap_or_else(|| decimal_places(self.step)),
        }
    }
}

impl Default for NumberInputConfig {
    fn default() -> Self {
        Self {
            value: 0.0,
            min: f64::NEG_INFINITY,
            max: f64::INFINITY,
            step: 1.0,
            precision: None,
            buttons: StepButtons::default(),
            field: NumberField::default(),
            columns: DEFAULT_COLUMNS,
            disabled: false,
            tokens: NumberInputTokens::with_prefix(NUMBER_INPUT_STYLE),
        }
    }
}

/// The parts of a number input, on the input.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct NumberInputParts {
    /// The field showing the number, unless it has none.
    pub field: Option<Entity>,
    /// The decrease button, if any.
    pub decrease: Option<Entity>,
    /// The increase button, if any.
    pub increase: Option<Entity>,
}

impl NumberInputParts {
    /// Its step buttons, whichever it has.
    fn buttons(&self) -> impl Iterator<Item = Entity> {
        self.decrease.into_iter().chain(self.increase)
    }
}

/// On the field, its editor and the buttons: the number input they belong to.
#[derive(Component, Clone, Copy, Debug)]
struct PartOf(Entity);

/// On a step button: which way it steps.
#[derive(Component, Clone, Copy, Debug)]
struct Steps(f64);

/// Which way an arrow key steps, if it's one that does. The one place the
/// input reads keys, for the key mapping to take over.
fn step_key(key: &Key) -> Option<f64> {
    match key {
        Key::ArrowUp => Some(1.0),
        Key::ArrowDown => Some(-1.0),
        _ => None,
    }
}

/// Spawns it; documented on [`SpawnUi::number_input`](crate::spawn_ui::SpawnUi::number_input).
pub(crate) fn spawn_number_input<'a>(
    parent: &'a mut ChildSpawnerCommands,
    config: NumberInputConfig,
) -> EntityCommands<'a> {
    let config = config.sanitized();
    let rules = config.rules();
    let value = rules.settle(config.value);
    let tokens = config.tokens.clone();

    let mut control = parent.spawn((
        Node {
            align_items: AlignItems::Center,
            column_gap: px(GAP),
            ..default()
        },
        NumberValue(value),
        rules,
    ));
    let root = control.id();
    // Without a field to type in, the input itself takes the focus and the keys.
    if config.field != NumberField::Typed {
        control.insert(TabIndex(0)).observe(step_on_arrow_keys);
    }

    let mut parts = NumberInputParts {
        field: None,
        decrease: None,
        increase: None,
    };
    control.with_children(|row| {
        let step_button = |row: &mut ChildSpawnerCommands, direction: f64, stacked: bool| {
            let label = if direction > 0.0 {
                INCREASE_LABEL
            } else {
                DECREASE_LABEL
            };
            let button = ButtonConfig::new(label)
                .size(SIDE_BUTTON_SIZE, SIDE_BUTTON_SIZE)
                .padding(SIDE_BUTTON_PADDING.x, SIDE_BUTTON_PADDING.y)
                .tokens(tokens.step.clone())
                .disabled(config.disabled);
            let mut button = spawn_button(row, button);
            button.insert((PartOf(root), Steps(direction)));
            if stacked {
                button.insert(stacked_node());
            }
            button.observe(step_on_press).id()
        };

        if config.buttons == StepButtons::Sides {
            parts.decrease = Some(step_button(row, -1.0, false));
        }

        if config.field != NumberField::None {
            let field_config = TextInputConfig::new(rules.format(value))
                .filter(number_char)
                .columns(config.columns)
                .disabled(config.disabled)
                .tokens(tokens.field.clone());
            let (mut field, editor) = spawn_text_parts(row, field_config);
            field.insert(PartOf(root)).observe(take_typed_number);
            parts.field = Some(field.id());
            let mut commands = row.commands();
            let mut editor = commands.entity(editor);
            editor.insert(PartOf(root));
            if config.field == NumberField::Typed {
                editor.observe(step_on_arrow_keys);
            } else {
                // Shown, but not typed into or focused: the input takes the focus.
                editor
                    .insert((
                        TextReadWriteMode::Static,
                        HoverCursor::new(SystemCursorIcon::Default),
                    ))
                    .remove::<TabIndex>();
            }
        }

        match config.buttons {
            StepButtons::Sides => parts.increase = Some(step_button(row, 1.0, false)),
            StepButtons::Stacked => {
                row.spawn(Node {
                    flex_direction: FlexDirection::Column,
                    ..default()
                })
                .with_children(|stack| {
                    parts.increase = Some(step_button(stack, 1.0, true));
                    parts.decrease = Some(step_button(stack, -1.0, true));
                });
            }
            StepButtons::None => {}
        }
    });
    control.insert((parts, config.tokens));
    // After its parts: they're disabled with it, in `disable_parts`.
    if config.disabled {
        control.insert(InteractionDisabled);
    }
    control
}

/// A half-height button for the stacked layout.
fn stacked_node() -> Node {
    Node {
        width: px(STACKED_BUTTON_WIDTH),
        height: px(STACKED_BUTTON_HEIGHT),
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        border: UiRect::all(px(STACKED_BUTTON_BORDER)),
        ..default()
    }
}

/// Makes `value` the input's value and announces it as final.
fn set_value(commands: &mut Commands, root: Entity, value: f64) {
    commands.entity(root).insert(NumberValue(value));
    commands.trigger(ValueChange {
        source: root,
        value,
        is_final: true,
    });
}

/// Disables each step button while the input is, or the value is at the end
/// of the range it steps towards.
fn sync_buttons(
    commands: &mut Commands,
    parts: &NumberInputParts,
    value: f64,
    rules: &NumberRules,
    disabled: bool,
) {
    let ends = [
        (parts.decrease, value <= rules.min),
        (parts.increase, value >= rules.max),
    ];
    for (button, at_end) in ends {
        let Some(button) = button else { continue };
        let mut button = commands.entity(button);
        if disabled || at_end {
            button.insert(InteractionDisabled);
        } else {
            button.remove::<InteractionDisabled>();
        }
    }
}

/// Steps the value when a step button is pressed.
fn step_on_press(
    activate: On<Activate>,
    buttons: Query<(&PartOf, &Steps)>,
    inputs: Query<(&NumberValue, &NumberRules), Without<InteractionDisabled>>,
    mut commands: Commands,
) {
    let Ok((&PartOf(root), &Steps(direction))) = buttons.get(activate.entity) else {
        return;
    };
    let Ok((&NumberValue(value), rules)) = inputs.get(root) else {
        return;
    };
    let stepped = rules.stepped(value, direction);
    if stepped != value {
        set_value(&mut commands, root, stepped);
    }
}

/// Up and Down step the value while the input has the focus: from the number
/// in the field, typed or not, as HTML's number input does, or from the value.
fn step_on_arrow_keys(
    mut keys: On<FocusedInput<KeyboardInput>>,
    editors: Query<(&EditableText, &PartOf)>,
    inputs: Query<(&NumberValue, &NumberRules), Without<InteractionDisabled>>,
    mut commands: Commands,
) {
    if !keys.input.state.is_pressed() {
        return;
    }
    let Some(direction) = step_key(&keys.input.logical_key) else {
        return;
    };
    // The editor's own number, or the input's value when it has the focus itself.
    let (root, typed) = match editors.get(keys.focused_entity) {
        Ok((editable, &PartOf(root))) => (root, Some(editable.value().to_string())),
        Err(_) => (keys.focused_entity, None),
    };
    let Ok((&NumberValue(value), rules)) = inputs.get(root) else {
        return;
    };
    keys.propagate(false);
    let from = typed.and_then(|text| rules.parse(&text)).unwrap_or(value);
    set_value(&mut commands, root, rules.stepped(from, direction));
}

/// The field committed typed text: take it as the number if it is one, and
/// show the settled number either way, so `5.` reads `5` and `-` goes back.
fn take_typed_number(
    typed: On<ValueChange<String>>,
    fields: Query<&PartOf>,
    inputs: Query<(&NumberValue, &NumberRules)>,
    mut commands: Commands,
) {
    let Ok(&PartOf(root)) = fields.get(typed.source) else {
        return;
    };
    let Ok((&NumberValue(value), rules)) = inputs.get(root) else {
        return;
    };
    let settled = rules
        .parse(&typed.value)
        .map_or(value, |typed| rules.settle(typed));
    commands
        .entity(typed.source)
        .insert(TextInputValue(rules.format(settled)));
    if settled != value {
        set_value(&mut commands, root, settled);
    }
}

/// Shows each new value in the field, settling values the app set, and
/// disables a step button at the end of the range it steps towards.
fn show_values(
    mut inputs: Query<
        (
            &NumberInputParts,
            &mut NumberValue,
            &NumberRules,
            Has<InteractionDisabled>,
        ),
        Changed<NumberValue>,
    >,
    mut commands: Commands,
) {
    for (parts, mut value, rules, disabled) in &mut inputs {
        value.set_if_neq(NumberValue(rules.settle(value.0)));
        if let Some(field) = parts.field {
            commands
                .entity(field)
                .insert(TextInputValue(rules.format(value.0)));
        }
        sync_buttons(&mut commands, parts, value.0, rules, disabled);
    }
}

/// Disabling the input disables its field and buttons with it.
fn disable_parts(
    insert: On<Insert<InteractionDisabled>>,
    inputs: Query<&NumberInputParts>,
    mut commands: Commands,
) {
    if let Ok(parts) = inputs.get(insert.entity) {
        for part in parts.field.into_iter().chain(parts.buttons()) {
            commands.entity(part).insert(InteractionDisabled);
        }
    }
}

/// Enabling the input enables its field, and its buttons but at the ends.
fn enable_parts(
    remove: On<Remove<InteractionDisabled>>,
    inputs: Query<(&NumberInputParts, &NumberValue, &NumberRules)>,
    mut commands: Commands,
) {
    let Ok((parts, value, rules)) = inputs.get(remove.entity) else {
        return;
    };
    if let Some(field) = parts.field {
        commands.entity(field).remove::<InteractionDisabled>();
    }
    sync_buttons(&mut commands, parts, value.0, rules, false);
}

/// Styles a number input when its tokens are set: at spawn, and whenever the
/// app sets new ones, by handing its field and buttons their own.
fn style_number_input(
    insert: On<Insert<NumberInputTokens>>,
    inputs: Query<(&NumberInputTokens, &NumberInputParts)>,
    mut commands: Commands,
) {
    let Ok((tokens, parts)) = inputs.get(insert.entity) else {
        return;
    };
    if let Some(field) = parts.field {
        commands.entity(field).insert(tokens.field.clone());
    }
    for button in parts.buttons() {
        commands.entity(button).insert(tokens.step.clone());
    }
}

/// An input without a field to type in takes the focus itself, and shows it
/// on its field, which is what frames it.
fn lend_focus(
    gained: On<Add<FocusVisible>>,
    inputs: Query<&NumberInputParts>,
    mut commands: Commands,
) {
    if let Ok(NumberInputParts {
        field: Some(field), ..
    }) = inputs.get(gained.entity)
    {
        commands.entity(*field).try_insert(FocusVisible);
    }
}

/// The input lost the focus, so its field stops showing it.
fn take_focus_back(
    lost: On<Remove<FocusVisible>>,
    inputs: Query<&NumberInputParts>,
    mut commands: Commands,
) {
    if let Ok(NumberInputParts {
        field: Some(field), ..
    }) = inputs.get(lost.entity)
    {
        commands.entity(*field).try_remove::<FocusVisible>();
    }
}

/// Number input behaviour and default token values.
pub struct NumberInputWidgetPlugin;

impl Plugin for NumberInputWidgetPlugin {
    fn build(&self, app: &mut App) {
        app.ensure_plugin(ThemePlugin)
            .ensure_plugin(ButtonWidgetPlugin)
            .ensure_plugin(TextInputWidgetPlugin)
            .add_observer(style_number_input)
            .add_observer(disable_parts)
            .add_observer(enable_parts)
            .add_observer(lend_focus)
            .add_observer(take_focus_back)
            .add_systems(PostUpdate, show_values.before(UiSystems::Prepare));
        NumberInputTokens::with_prefix(NUMBER_INPUT_STYLE)
            .register_theme_defaults(&mut app.world_mut().resource_mut::<Theme>());
    }
}

#[cfg(test)]
mod tests {
    use bevy::input::ButtonState;
    use bevy::input::keyboard::{KeyCode, NativeKeyCode};
    use bevy::input_focus::InputFocusPlugin;

    use crate::theme::{ThemeBorder, WidgetState};

    use super::*;

    /// Every `ValueChange<f64>` announced, in order.
    #[derive(Resource, Default)]
    struct Announced(Vec<f64>);

    fn app_with_input(config: NumberInputConfig) -> (App, Entity) {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::asset::AssetPlugin::default(),
            bevy::input::InputPlugin,
            // Bevy's text input wants IME messages, pointers and the UI scale.
            bevy::window::WindowPlugin {
                primary_window: None,
                exit_condition: bevy::window::ExitCondition::DontExit,
                ..default()
            },
            InputFocusPlugin,
            bevy::picking::DefaultPickingPlugins,
            bevy::text::TextPlugin,
            NumberInputWidgetPlugin,
        ))
        .init_resource::<UiScale>()
        .init_resource::<Announced>()
        .add_observer(
            |change: On<ValueChange<f64>>, mut announced: ResMut<Announced>| {
                announced.0.push(change.value);
            },
        );
        let mut input = None;
        app.world_mut()
            .commands()
            .spawn(Node::default())
            .with_children(|root| input = Some(spawn_number_input(root, config).id()));
        app.update();
        app.update();
        (app, input.unwrap())
    }

    fn parts(app: &App, input: Entity) -> NumberInputParts {
        *app.world().get::<NumberInputParts>(input).unwrap()
    }

    fn value(app: &App, input: Entity) -> f64 {
        app.world().get::<NumberValue>(input).unwrap().0
    }

    fn shown(app: &App, input: Entity) -> String {
        let field = parts(app, input).field.unwrap();
        app.world().get::<TextInputValue>(field).unwrap().0.clone()
    }

    fn announced(app: &App) -> Vec<f64> {
        app.world().resource::<Announced>().0.clone()
    }

    fn press_button(app: &mut App, button: Option<Entity>) {
        app.world_mut().trigger(Activate {
            entity: button.unwrap(),
        });
        app.update();
    }

    fn disabled(app: &App, button: Option<Entity>) -> bool {
        app.world()
            .get::<InteractionDisabled>(button.unwrap())
            .is_some()
    }

    /// The text field's editor, which takes keys and holds typed text.
    fn editor(app: &mut App, input: Entity) -> Entity {
        let world = app.world_mut();
        world
            .query_filtered::<(Entity, &PartOf), With<EditableText>>()
            .iter(world)
            .find(|(_, part)| part.0 == input)
            .unwrap()
            .0
    }

    fn press_key(app: &mut App, focused: Entity, key: Key) {
        let keyboard = KeyboardInput {
            key_code: KeyCode::Unidentified(NativeKeyCode::Unidentified),
            logical_key: key,
            state: ButtonState::Pressed,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        };
        app.world_mut()
            .trigger(FocusedInput::new(focused, keyboard, Entity::PLACEHOLDER));
        app.update();
        app.update();
    }

    fn type_and_press(app: &mut App, input: Entity, text: &str, key: Key) {
        let editor = editor(app, input);
        app.world_mut()
            .get_mut::<EditableText>(editor)
            .unwrap()
            .editor_mut()
            .set_text(text);
        app.update();
        press_key(app, editor, key);
    }

    #[test]
    fn buttons_step_and_stop_at_the_ends() {
        let (mut app, input) = app_with_input(NumberInputConfig::new(9.0).range(0.0, 10.0));
        let NumberInputParts {
            decrease, increase, ..
        } = parts(&app, input);
        press_button(&mut app, increase);
        app.update();
        assert_eq!(value(&app, input), 10.0);
        assert_eq!(shown(&app, input), "10");
        assert!(disabled(&app, increase), "nothing above the top");
        assert!(!disabled(&app, decrease));

        press_button(&mut app, decrease);
        app.update();
        assert_eq!(value(&app, input), 9.0);
        assert!(!disabled(&app, increase));
        assert_eq!(announced(&app), [10.0, 9.0]);
    }

    #[test]
    fn typed_numbers_are_clamped_and_rounded_and_others_put_back() {
        let config = NumberInputConfig::new(2.0).range(0.0, 10.0).step(0.5);
        let (mut app, input) = app_with_input(config);
        assert_eq!(
            shown(&app, input),
            "2.0",
            "the step needs one decimal place"
        );

        type_and_press(&mut app, input, "7.25", Key::Enter);
        assert_eq!(value(&app, input), 7.3);
        assert_eq!(shown(&app, input), "7.3");

        type_and_press(&mut app, input, "12", Key::Enter);
        assert_eq!(value(&app, input), 10.0);

        type_and_press(&mut app, input, "-", Key::Enter);
        assert_eq!(value(&app, input), 10.0);
        assert_eq!(
            shown(&app, input),
            "10.0",
            "not a number: the value comes back"
        );
        assert_eq!(announced(&app), [7.3, 10.0]);
    }

    #[test]
    fn arrow_keys_step_from_what_is_typed() {
        let (mut app, input) = app_with_input(NumberInputConfig::new(1.0));
        type_and_press(&mut app, input, "5", Key::ArrowUp);
        assert_eq!(value(&app, input), 6.0);
        type_and_press(&mut app, input, "6", Key::ArrowDown);
        assert_eq!(value(&app, input), 5.0);
    }

    #[test]
    fn values_from_code_are_settled_shown_and_not_announced() {
        let config = NumberInputConfig::new(0.0).range(0.0, 1.0).precision(2);
        let (mut app, input) = app_with_input(config);
        app.world_mut()
            .entity_mut(input)
            .insert(NumberValue(0.3333));
        app.update();
        app.update();
        assert_eq!(value(&app, input), 0.33);
        assert_eq!(shown(&app, input), "0.33");
        assert!(announced(&app).is_empty());
    }

    #[test]
    fn a_shown_field_only_shows_the_number_and_the_input_takes_the_keys() {
        let config = NumberInputConfig::new(3.0).field(NumberField::Shown);
        let (mut app, input) = app_with_input(config);
        let editor = editor(&mut app, input);
        let world = app.world();
        assert_eq!(
            world.get::<TextReadWriteMode>(editor),
            Some(&TextReadWriteMode::Static)
        );
        assert!(world.get::<TabIndex>(editor).is_none(), "Tab skips it");
        assert!(world.get::<TabIndex>(input).is_some(), "and stops here");
        press_key(&mut app, input, Key::ArrowUp);
        assert_eq!(value(&app, input), 4.0);
        assert_eq!(shown(&app, input), "4");
    }

    #[test]
    fn without_a_field_it_is_a_stepper_the_app_shows() {
        let config = NumberInputConfig::new(3.0)
            .range(1.0, 9.0)
            .field(NumberField::None);
        let (mut app, input) = app_with_input(config);
        let parts = parts(&app, input);
        assert_eq!(parts.field, None);
        press_button(&mut app, parts.increase);
        press_key(&mut app, input, Key::ArrowUp);
        assert_eq!(value(&app, input), 5.0);
        assert_eq!(announced(&app), [4.0, 5.0]);
    }

    #[test]
    fn disabling_the_input_disables_its_parts_and_enabling_respects_the_ends() {
        let config = NumberInputConfig::new(10.0).range(0.0, 10.0).disabled(true);
        let (mut app, input) = app_with_input(config);
        let NumberInputParts {
            decrease, increase, ..
        } = parts(&app, input);
        assert!(disabled(&app, decrease) && disabled(&app, increase));
        press_button(&mut app, decrease);
        assert_eq!(value(&app, input), 10.0, "a disabled input doesn't step");

        app.world_mut()
            .entity_mut(input)
            .remove::<InteractionDisabled>();
        app.update();
        assert!(!disabled(&app, decrease));
        assert!(disabled(&app, increase), "still at the top");
    }

    #[test]
    fn new_tokens_reach_the_field_and_buttons() {
        let (mut app, input) = app_with_input(NumberInputConfig::new(0.0));
        let tokens = NumberInputTokens::with_prefix("game.count");
        app.world_mut().entity_mut(input).insert(tokens.clone());
        app.update();
        let parts = parts(&app, input);
        let world = app.world();
        assert_eq!(
            world.get::<TextInputTokens>(parts.field.unwrap()),
            Some(&tokens.field)
        );
        assert_eq!(
            world.get::<ButtonTokens>(parts.increase.unwrap()),
            Some(&tokens.step)
        );
    }

    #[test]
    fn button_layouts() {
        let has = |buttons| {
            let (app, input) = app_with_input(NumberInputConfig::new(0.0).buttons(buttons));
            let parts = parts(&app, input);
            (parts.decrease.is_some(), parts.increase.is_some())
        };
        assert_eq!(has(StepButtons::Sides), (true, true));
        assert_eq!(has(StepButtons::Stacked), (true, true));
        assert_eq!(has(StepButtons::None), (false, false));
    }

    #[test]
    fn precision_follows_the_step_unless_set() {
        assert_eq!(decimal_places(1.0), 0);
        assert_eq!(decimal_places(0.5), 1);
        assert_eq!(decimal_places(0.25), 2);
        assert_eq!(decimal_places(1.0 / 3.0), MAX_DECIMALS);
    }

    #[test]
    fn unusable_configs_are_repaired() {
        let config = NumberInputConfig {
            value: f64::NAN,
            min: 5.0,
            max: 1.0,
            step: 0.0,
            precision: Some(12),
            ..default()
        }
        .sanitized();
        assert_eq!((config.min, config.max), (5.0, 5.0));
        assert_eq!(config.step, 1e-6);
        assert_eq!(config.precision, Some(MAX_DECIMALS));
        assert_eq!(config.value, 0.0);
    }

    /// Without a field to type in, the input takes the focus and shows it on
    /// its field, which frames it.
    #[test]
    fn a_focused_input_shows_it_on_its_field() {
        let (mut app, input) =
            app_with_input(NumberInputConfig::new(3.0).field(NumberField::Shown));
        let field = parts(&app, input).field.unwrap();
        let resting = NumberInputTokens::with_prefix(NUMBER_INPUT_STYLE)
            .field
            .border;
        let border = |app: &App| app.world().get::<ThemeBorder>(field).unwrap().top.clone();
        assert_eq!(border(&app), resting);
        app.world_mut().entity_mut(input).insert(FocusVisible);
        app.update();
        assert_eq!(
            border(&app),
            resting.clone().with_state(WidgetState::Focused)
        );
        app.world_mut().entity_mut(input).remove::<FocusVisible>();
        app.update();
        assert_eq!(border(&app), resting);
    }
}
