//! Combo boxes: a text field over a list of options, narrowed to what's typed.
//!
//! Type part of a name and the list shows only the options containing it,
//! ignoring case: "louis" leaves Louisville and St. Louis. The list opens as
//! the field takes the focus; the arrow keys move through it, scrolling the
//! highlight into view, and clicking an option chooses it.
//!
//! A combo box comes in two kinds:
//! - **Choosing** (the default): the text must name an option. While typing,
//!   the first option left is highlighted, and Enter, or the focus leaving the
//!   field, chooses the highlighted one. With none left, the text goes back to
//!   the choice. The choice is the combo's [`Chosen`], announced as
//!   `ValueChange<usize>`.
//! - **Free text** ([`ComboConfig::free_text`]): the options are suggestions.
//!   Enter, or the focus leaving, keeps the text as typed; an option is chosen
//!   only by clicking it, or by moving to it with the arrow keys first. The
//!   value is the text, announced as `ValueChange<String>`.
//!
//! Escape cancels, putting back the text as it was. Either way the text the
//! combo settled on is its [`ComboText`], and options are
//! [choices](crate::choice): the list's rows can be built from anything, while
//! the field shows their labels. Filtering matches the labels.
//!
//! The rows look like a dropdown's, and their tokens follow the dropdown's;
//! the field's follow the text input's.

use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input_focus::{
    AcquireFocus, FocusCause, FocusGained, FocusLost, FocusedInput, InputFocus, InputFocusSystems,
};
use bevy::picking::events::PointerPress;
use bevy::prelude::*;
use bevy::text::{EditableText, EditableTextSystems, TextEdit};
use bevy::ui::{InteractionDisabled, UiSystems};
use bevy::ui_widgets::{ScrollIntoView, SelectAllOnFocus, ValueChange};

use crate::EnsurePlugin;
use crate::choice::{
    Choice, ChoiceItem, ChoicePlugin, ChoiceSet, ChoiceState, ChoiceSystems, Chosen, Pick, Step,
};
use crate::containers::scroll_area::{ScrollAreaConfig, ScrollAreaPlugin, spawn_scroll_area};
use crate::cursor::{HoverCursor, HoverCursorPlugin};
use crate::popover::{PopoverConfig, PopoverOpen, PopoverPlugin, spawn_popover};
use crate::primitives::Unstretched;
use crate::sanitize::at_least;
use crate::theme::{
    Shade, Theme, ThemeCornerRadius, ThemeFont, ThemeFontSize, ThemePlugin, ThemeSkin,
    ThemeTextColor, ThemeToken, WidgetState,
};
use crate::widgets::chevron::{Chevron, ChevronPlugin, ChevronTurn, chevron_colour, spawn_chevron};
use crate::widgets::dropdown::{DROPDOWN_STYLE, DropdownTokens, DropdownWidgetPlugin};
use crate::widgets::text_input::{
    CommitOn, TEXT_INPUT_STYLE, TextInputConfig, TextInputTokens, TextInputValue,
    TextInputWidgetPlugin, spawn_text_parts,
};

/// Prefix of the combo's tokens: `touchy.combo.bg` for its field, and so on.
pub const COMBO_STYLE: &str = "touchy.combo";

/// Space inside an option's row, across and down.
const OPTION_PADDING: Vec2 = Vec2::new(8.0, 5.0);
/// Space inside the list's edge.
const LIST_PADDING: f32 = 4.0;
/// The tallest the list grows before it scrolls, unless the config says otherwise.
const DEFAULT_LIST_HEIGHT: f32 = 240.0;
/// How far the chevron at the field's end sits from its edge.
const CHEVRON_MARGIN: f32 = 8.0;

/// The combo's look: its field, which follows the text input's tokens, and its
/// list's rows and chevron, which follow the dropdown's. Insert new ones to
/// restyle a combo.
#[derive(Component, Clone, PartialEq, Debug)]
#[component(immutable)]
pub struct ComboTokens {
    /// The text input's tokens, for the field and its editor.
    pub field: TextInputTokens,
    /// An option's row: clear at rest, tinted while highlighted.
    pub option: ThemeToken,
    /// The chosen option's background.
    pub option_chosen: ThemeToken,
    /// The text in an option's row, tinted while disabled.
    pub option_text: ThemeToken,
    /// The chevron at the field's end, which turns when the list opens.
    pub chevron: ThemeToken,
}

impl ComboTokens {
    /// Names the tokens under `prefix`; the field's are the text input's, under the same.
    pub fn with_prefix(prefix: &str) -> Self {
        let token = |suffix: &str| ThemeToken::new(format!("{prefix}.{suffix}"));
        Self {
            field: TextInputTokens::with_prefix(prefix),
            option: token("option"),
            option_chosen: token("option_chosen"),
            option_text: token("option_text"),
            chevron: token("chevron"),
        }
    }

    /// Makes these tokens follow the text input's and the dropdown's, unless
    /// the app sets them, so a combo box restyles with both.
    pub fn register_theme_defaults(&self, theme: &mut Theme) {
        self.field
            .follow(theme, &TextInputTokens::with_prefix(TEXT_INPUT_STYLE));
        let list = DropdownTokens::with_prefix(DROPDOWN_STYLE);
        let state = |token: &ThemeToken, state| token.clone().with_state(state);
        let follows = [
            (self.option.clone(), list.option.clone()),
            (
                state(&self.option, WidgetState::Hover),
                state(&list.option, WidgetState::Hover),
            ),
            (self.option_chosen.clone(), list.option_chosen),
            (self.option_text.clone(), list.text.clone()),
            (
                state(&self.option_text, WidgetState::Disabled),
                state(&list.text, WidgetState::Disabled),
            ),
            (self.chevron.clone(), list.chevron),
        ];
        for (token, from) in follows {
            theme.default_derived(token, from, Shade::Same);
        }
    }
}

/// The built-in token set.
impl Default for ComboTokens {
    fn default() -> Self {
        Self::with_prefix(COMBO_STYLE)
    }
}

/// Spawn time instructions to make a combo box.
///
/// Consumed by [`SpawnUi::combo`](crate::spawn_ui::SpawnUi::combo), which returns the combo's
/// entity.
///
/// ```
/// use touchy::prelude::*;
///
/// const CITIES: [&str; 3] = ["Montreal", "Ottawa", "Toronto"];
///
/// let config = ComboConfig::new(CITIES).chosen(2);
/// ```
#[derive(Clone, Debug)]
pub struct ComboConfig {
    /// The list of options to choose from, in order.
    pub options: Vec<Choice>,
    /// The index of the option initially chosen at spawn.
    pub chosen: Option<usize>,
    /// Whether the text may be anything, the options only suggestions.
    pub free_text: bool,
    /// A free-text combo's starting text, if not a chosen option's label.
    pub text: String,
    /// Shown, faded, while the field is empty.
    pub placeholder: String,
    /// How many characters wide the field is.
    pub columns: Option<f32>,
    /// Where the text sits across the field. The list's choices keep their own look.
    pub justify: Justify,
    /// The tallest the list grows before it scrolls, in pixels.
    pub list_height: f32,
    /// Whether the combo is disabled at spawn.
    pub disabled: bool,
    /// The theme tokens for the combo box.
    pub tokens: ComboTokens,
}

impl ComboConfig {
    /// A combo box choosing from `options`, nothing chosen. Plain words make
    /// plain options.
    pub fn new<O: Into<Choice>>(options: impl IntoIterator<Item = O>) -> Self {
        Self {
            options: options.into_iter().map(Into::into).collect(),
            ..default()
        }
    }
    /// Replace the options with anything that turns into a list of choices.
    pub fn options<O: Into<Choice>>(mut self, options: impl IntoIterator<Item = O>) -> Self {
        self.options = options.into_iter().map(Into::into).collect();
        self
    }
    /// Starts with the option at `index` chosen, its label in the field.
    pub fn chosen(mut self, index: usize) -> Self {
        self.chosen = Some(index);
        self
    }
    /// Takes any text, the options only suggestions; see the module's docs.
    pub fn free_text(mut self, free_text: bool) -> Self {
        self.free_text = free_text;
        self
    }
    /// A free-text combo's starting text.
    pub fn text(mut self, text: impl Into<String>) -> Self {
        self.text = text.into();
        self
    }
    /// Sets the placeholder text for a combo box with nothing chosen.
    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    /// How many characters wide the field is.
    pub fn columns(mut self, columns: f32) -> Self {
        self.columns = Some(columns);
        self
    }
    /// Sets where the text sits across the field: `Justify::Center`, say.
    pub fn justify(mut self, justify: Justify) -> Self {
        self.justify = justify;
        self
    }
    /// The tallest the list grows before it scrolls, in pixels.
    pub fn list_height(mut self, pixels: f32) -> Self {
        self.list_height = pixels;
        self
    }
    /// Sets whether the combo box is disabled at spawn time.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
    /// Sets the theme tokens for the combo box.
    pub fn tokens(mut self, tokens: ComboTokens) -> Self {
        self.tokens = tokens;
        self
    }
    /// This config with every value made usable, warning about each repair.
    fn sanitized(mut self) -> Self {
        self.list_height = at_least(self.list_height, 0.0, "ComboConfig list height");
        self
    }
}

impl Default for ComboConfig {
    fn default() -> Self {
        Self {
            options: Vec::new(),
            chosen: None,
            free_text: false,
            text: String::new(),
            placeholder: String::new(),
            columns: None,
            justify: Justify::default(),
            list_height: DEFAULT_LIST_HEIGHT,
            disabled: false,
            tokens: ComboTokens::with_prefix(COMBO_STYLE),
        }
    }
}

/// The text the combo settled on: a chosen option's label, or for a free-text
/// combo, whatever was entered. Read-only; choose from code with [`Chosen`],
/// or for a free-text combo, by inserting the field's text.
#[derive(Component, Clone, Debug, Default, PartialEq, Eq)]
pub struct ComboText(pub String);

/// The parts of a combo box, on the combo.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub struct ComboParts {
    /// The text field.
    pub field: Entity,
    /// The editor inside the field, which takes the focus and the keys.
    pub editor: Entity,
    /// The chevron at the field's end.
    pub chevron: Entity,
    /// The popover holding the list.
    pub popover: Entity,
    /// The list's rows, one for each option, in order.
    pub rows: Vec<ComboRow>,
}

/// One row of a combo's list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ComboRow {
    /// The row.
    pub row: Entity,
    /// The option's label in it, unless the option builds its own look.
    pub label: Option<Entity>,
}

/// A combo's state. Its options, their filter and their highlight are its
/// [`ChoiceSet`].
#[derive(Component, Clone, Debug)]
struct Combo {
    free_text: bool,
    /// The text as typed, and whether it, or the highlight, has moved on from
    /// what was settled.
    typed: String,
    dirty: bool,
    /// Whether the arrow keys moved the highlight since the last typing, which
    /// lets a free-text combo choose an option rather than keep its text.
    keyed: bool,
    /// Text to put in the field once this frame's edits are done.
    settle: Option<String>,
    /// Whether to scroll the highlight into view once the list's laid out.
    reveal: bool,
}

/// The editor inside a combo's field, and the combo.
#[derive(Component, Clone, Copy, Debug)]
struct ComboEditor(Entity);

/// A combo's field, and the combo.
#[derive(Component, Clone, Copy, Debug)]
struct ComboField(Entity);

/// What the combo does when its text is entered or the focus leaves.
#[derive(Clone, Debug, PartialEq)]
enum Settle {
    /// Choose the option at this index.
    Pick(usize),
    /// Keep this text, as a free-text combo does.
    Keep(String),
    /// Put the settled text back: nothing typed names an option.
    Revert,
    /// Nothing changed; just close the list.
    Close,
}

impl Combo {
    /// What entering the text now does: by Enter if `entered`, by the focus
    /// leaving otherwise.
    fn settle(&self, choices: &ChoiceSet, entered: bool) -> Settle {
        let highlighted = choices.highlighted();
        match (self.free_text, highlighted) {
            (false, Some(index)) if self.dirty => Settle::Pick(index),
            (false, _) if self.dirty => Settle::Revert,
            (true, Some(index)) if self.keyed => Settle::Pick(index),
            (true, _) if self.dirty || entered => Settle::Keep(self.typed.clone()),
            _ => Settle::Close,
        }
    }
}

/// Whether `label` contains `query`, ignoring case.
fn matches(label: &str, query: &str) -> bool {
    label.to_lowercase().contains(&query.to_lowercase())
}

/// The index of the option labelled exactly `text`, if any.
fn named(choices: &ChoiceSet, text: &str) -> Option<usize> {
    choices
        .choices()
        .iter()
        .position(|choice| choice.label == text)
}

/// What a key does in a combo's field. The one place its keys are read, for
/// the key mapping to take over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ComboKey {
    /// Opens the list, or moves the highlight down it.
    Next,
    /// Opens the list, or moves the highlight up it.
    Previous,
    /// Enters the text.
    Enter,
    /// Cancels, putting the text back.
    Cancel,
}

impl ComboKey {
    fn of(key: &Key) -> Option<Self> {
        match key {
            Key::ArrowDown => Some(Self::Next),
            Key::ArrowUp => Some(Self::Previous),
            Key::Enter => Some(Self::Enter),
            Key::Escape => Some(Self::Cancel),
            _ => None,
        }
    }
}

/// Spawns it; documented on [`SpawnUi::combo`](crate::spawn_ui::SpawnUi::combo).
pub(crate) fn spawn_combo<'a>(
    parent: &'a mut ChildSpawnerCommands,
    config: ComboConfig,
) -> EntityCommands<'a> {
    let ComboConfig {
        options,
        chosen,
        free_text,
        text,
        placeholder,
        columns,
        justify,
        list_height,
        disabled,
        tokens,
    } = config.sanitized();
    let chosen = chosen.filter(|&index| index < options.len());
    let label = chosen.map(|index| options[index].label.clone());
    let text = match (free_text, label) {
        (true, _) if !text.is_empty() => text,
        (_, Some(label)) => label,
        _ => String::new(),
    };
    // The combo is the list's anchor, holding the field and the list.
    let mut combo = parent.spawn((
        Node {
            flex_direction: FlexDirection::Column,
            ..default()
        },
        Unstretched,
        Chosen(chosen),
        ComboText(text.clone()),
    ));
    if disabled {
        combo.insert(InteractionDisabled);
    }
    let id = combo.id();
    let mut field_config = TextInputConfig::new(text.clone())
        .placeholder(placeholder)
        .commit_on(CommitOn::Enter)
        .live(true)
        .justify(justify)
        .disabled(disabled)
        .tokens(tokens.field.clone());
    field_config.columns = columns;
    let mut parts = ComboParts {
        field: Entity::PLACEHOLDER,
        editor: Entity::PLACEHOLDER,
        chevron: Entity::PLACEHOLDER,
        popover: Entity::PLACEHOLDER,
        rows: Vec::new(),
    };
    combo.with_children(|anchor| {
        let (mut field, editor) = spawn_text_parts(anchor, field_config);
        field.insert(ComboField(id));
        parts.field = field.id();
        parts.editor = editor;
        field.with_children(|field| {
            let margin = UiRect::right(px(CHEVRON_MARGIN));
            parts.chevron = spawn_chevron(field, margin, ChevronTurn::LIST, false);
        });
        // Focusing the field selects its text, so typing replaces it.
        anchor
            .commands()
            .entity(editor)
            .insert((ComboEditor(id), SelectAllOnFocus));
        let list = PopoverConfig::default().padding(LIST_PADDING);
        let popover = spawn_popover(anchor, list, |popover| {
            spawn_scroll_area(
                popover,
                ScrollAreaConfig::vertical().max_height(list_height),
                |list| {
                    for (index, option) in options.iter().enumerate() {
                        let mut row = list.spawn((
                            Node {
                                align_items: AlignItems::Center,
                                padding: UiRect::axes(px(OPTION_PADDING.x), px(OPTION_PADDING.y)),
                                ..default()
                            },
                            ChoiceItem { set: id, index },
                        ));
                        if !option.is_disabled() {
                            row.insert(HoverCursor::POINTER);
                        }
                        let mut label = None;
                        row.with_children(|row| {
                            label = option.build(row, |label| Text::new(label))
                        });
                        parts.rows.push(ComboRow {
                            row: row.id(),
                            label,
                        });
                    }
                },
            );
        });
        parts.popover = popover.id();
    });
    let mut choices = ChoiceSet::new(options).quiet(free_text);
    choices.highlight(chosen);
    // Its look comes with its tokens, in `style_combo`, once its parts are in.
    combo.insert((
        choices,
        Combo {
            free_text,
            typed: text,
            dirty: false,
            keyed: false,
            settle: None,
            reveal: false,
        },
        parts,
        tokens,
    ));
    combo
}

/// Opens a combo's list with every option, the choice highlighted, as the
/// field takes the focus or is pressed.
fn open_all(
    combo: &mut Combo,
    choices: &mut ChoiceSet,
    chosen: Option<usize>,
    open: &mut Mut<PopoverOpen>,
) {
    choices.clear_filter();
    choices.highlight(chosen);
    combo.dirty = false;
    combo.keyed = false;
    combo.reveal = true;
    open.set_if_neq(PopoverOpen(true));
}

/// Closes a combo's list, its filter and typing done with.
fn close(combo: &mut Combo, choices: &mut ChoiceSet, open: &mut Mut<PopoverOpen>) {
    choices.clear_filter();
    combo.dirty = false;
    combo.keyed = false;
    open.set_if_neq(PopoverOpen(false));
}

type Live = (
    &'static mut Combo,
    &'static ComboParts,
    &'static mut ChoiceSet,
    &'static Chosen,
    &'static ComboText,
);

/// Acts on what entering the text, or the focus leaving, does.
fn apply_settle(
    root: Entity,
    settle: Settle,
    (combo, choices, text): (&mut Combo, &mut ChoiceSet, &ComboText),
    open: &mut Mut<PopoverOpen>,
    commands: &mut Commands,
) {
    match settle {
        Settle::Pick(index) => commands.trigger(Pick {
            set: root,
            index: Some(index),
        }),
        Settle::Keep(typed) => {
            combo.settle = Some(typed.clone());
            commands
                .entity(root)
                .insert((Chosen(named(choices, &typed)), ComboText(typed.clone())));
            if typed != text.0 {
                commands.trigger(ValueChange {
                    source: root,
                    value: typed,
                    is_final: true,
                });
            }
            close(combo, choices, open);
        }
        Settle::Revert => {
            combo.settle = Some(text.0.clone());
            combo.typed = text.0.clone();
            close(combo, choices, open);
        }
        Settle::Close => close(combo, choices, open),
    }
}

/// Typing narrows the list to the options containing the text. A choosing
/// combo highlights the first left; the list shows while any are.
fn follow_typing(
    change: On<ValueChange<String>>,
    fields: Query<&ComboField>,
    mut combos: Query<(&mut Combo, &ComboParts, &mut ChoiceSet, &Chosen)>,
    mut popovers: Query<&mut PopoverOpen>,
) {
    if change.is_final {
        return;
    }
    let Ok(&ComboField(root)) = fields.get(change.source) else {
        return;
    };
    let Ok((mut combo, parts, mut choices, chosen)) = combos.get_mut(root) else {
        return;
    };
    // The combo putting text in the field itself isn't typing.
    if change.value == combo.typed {
        return;
    }
    let query = change.value.clone();
    if query.is_empty() {
        // Emptied, the field shows everything again, from the choice.
        choices.clear_filter();
        choices.highlight(chosen.0);
    } else {
        choices.filter(|choice| matches(&choice.label, &query));
        if combo.free_text {
            choices.highlight(None);
        } else {
            choices.step(Step::First);
        }
    }
    combo.typed = query;
    combo.dirty = true;
    combo.keyed = false;
    let any = (0..choices.len()).any(|index| choices.is_shown(index));
    if let Ok(mut open) = popovers.get_mut(parts.popover) {
        open.set_if_neq(PopoverOpen(any));
    }
}

/// The keys in the field: the arrows open the list and move through it,
/// Enter enters the text, and Escape cancels.
fn work_with_keys(
    keys: On<FocusedInput<KeyboardInput>>,
    editors: Query<&ComboEditor>,
    mut combos: Query<Live, Without<InteractionDisabled>>,
    mut popovers: Query<&mut PopoverOpen>,
    mut commands: Commands,
) {
    // The editor's own step only: Enter bubbles on through the field.
    let Ok(&ComboEditor(root)) = editors.get(keys.event_target()) else {
        return;
    };
    let Ok((mut combo, parts, mut choices, chosen, text)) = combos.get_mut(root) else {
        return;
    };
    if !keys.input.state.is_pressed() {
        return;
    }
    let Some(key) = ComboKey::of(&keys.input.logical_key) else {
        return;
    };
    let Ok(mut open) = popovers.get_mut(parts.popover) else {
        return;
    };
    match key {
        ComboKey::Next | ComboKey::Previous if !open.0 => {
            open_all(&mut combo, &mut choices, chosen.0, &mut open);
        }
        ComboKey::Next | ComboKey::Previous => {
            let step = if key == ComboKey::Next {
                Step::Next
            } else {
                Step::Previous
            };
            if let Some(row) = choices.step(step).and_then(|index| parts.rows.get(index)) {
                combo.keyed = true;
                combo.dirty = true;
                commands.trigger(ScrollIntoView { entity: row.row });
            }
        }
        ComboKey::Enter => {
            let settle = combo.settle(&choices, true);
            let state = (&mut *combo, &mut *choices, text);
            apply_settle(root, settle, state, &mut open, &mut commands);
        }
        ComboKey::Cancel => {
            // The field puts its settled text back itself.
            combo.typed = text.0.clone();
            close(&mut combo, &mut choices, &mut open);
        }
    }
}

/// The focus leaving the field enters what was typed, as Enter does, unless
/// Escape cancelled first.
fn settle_on_blur(
    lost: On<FocusLost>,
    editors: Query<&ComboEditor>,
    mut combos: Query<Live>,
    mut popovers: Query<&mut PopoverOpen>,
    mut commands: Commands,
) {
    let Ok(&ComboEditor(root)) = editors.get(lost.entity) else {
        return;
    };
    let Ok((mut combo, parts, mut choices, _, text)) = combos.get_mut(root) else {
        return;
    };
    let Ok(mut open) = popovers.get_mut(parts.popover) else {
        return;
    };
    let settle = combo.settle(&choices, false);
    let state = (&mut *combo, &mut *choices, text);
    apply_settle(root, settle, state, &mut open, &mut commands);
}

/// The field taking the focus opens the list.
fn open_on_focus(
    gained: On<FocusGained>,
    editors: Query<&ComboEditor>,
    mut combos: Query<Live, Without<InteractionDisabled>>,
    mut popovers: Query<&mut PopoverOpen>,
) {
    let Ok(&ComboEditor(root)) = editors.get(gained.event_target()) else {
        return;
    };
    if let Ok((mut combo, parts, mut choices, chosen, _)) = combos.get_mut(root)
        && let Ok(mut open) = popovers.get_mut(parts.popover)
    {
        open_all(&mut combo, &mut choices, chosen.0, &mut open);
    }
}

/// Pressing the field, already focused but closed after a choice, opens the
/// list again.
fn open_on_press(
    press: On<PointerPress>,
    mut combos: Query<Live, Without<InteractionDisabled>>,
    mut popovers: Query<&mut PopoverOpen>,
) {
    let target = press.event_target();
    for (mut combo, parts, mut choices, chosen, _) in &mut combos {
        if target != parts.editor && target != parts.field {
            continue;
        }
        if let Ok(mut open) = popovers.get_mut(parts.popover)
            && !open.0
        {
            open_all(&mut combo, &mut choices, chosen.0, &mut open);
        }
    }
}

/// A press anywhere in the combo, its list included, keeps the focus in the
/// field, so choosing from the list doesn't count as leaving it.
fn keep_focus_in_field(
    mut acquire: On<AcquireFocus>,
    combos: Query<&ComboParts, Without<InteractionDisabled>>,
    mut focus: ResMut<InputFocus>,
) {
    let Ok(parts) = combos.get(acquire.event_target()) else {
        return;
    };
    acquire.propagate(false);
    if focus.get() != Some(parts.editor) {
        focus.set(parts.editor, FocusCause::Pressed);
    }
}

/// An option picked, by click or from the keys: its label goes in the field
/// and the list closes. A free-text combo announces the label as its text.
fn follow_picks(
    pick: On<Pick>,
    mut combos: Query<Live>,
    mut popovers: Query<&mut PopoverOpen>,
    mut commands: Commands,
) {
    let root = pick.set;
    let Ok((mut combo, parts, mut choices, _, text)) = combos.get_mut(root) else {
        return;
    };
    let Some(label) = pick
        .index
        .filter(|&index| choices.can_pick(index))
        .and_then(|index| choices.get(index))
        .map(|choice| choice.label.clone())
    else {
        return;
    };
    combo.settle = Some(label.clone());
    combo.typed = label.clone();
    commands.entity(root).insert(ComboText(label.clone()));
    if combo.free_text && label != text.0 {
        commands.trigger(ValueChange {
            source: root,
            value: label,
            is_final: true,
        });
    }
    if let Ok(mut open) = popovers.get_mut(parts.popover) {
        close(&mut combo, &mut choices, &mut open);
    }
}

/// Puts each combo's settled text in its field, after this frame's edits, so
/// it wins over what the field made of the typing.
fn settle_texts(mut combos: Query<(&mut Combo, &ComboParts)>, mut commands: Commands) {
    for (mut combo, parts) in &mut combos {
        if let Some(text) = combo.settle.take() {
            commands.entity(parts.field).insert(TextInputValue(text));
        }
    }
}

/// The arrow keys move through the list, not the caret: drops the caret moves
/// up and down the editor queued for them, which on one line would jump it to
/// the start or end.
fn keep_caret_on_the_line(mut editors: Query<&mut EditableText, With<ComboEditor>>) {
    for mut editable in &mut editors {
        if editable
            .pending_edits
            .iter()
            .any(|edit| matches!(edit, TextEdit::Up(_) | TextEdit::Down(_)))
        {
            editable
                .pending_edits
                .retain(|edit| !matches!(edit, TextEdit::Up(_) | TextEdit::Down(_)));
        }
    }
}

/// As a list opens or closes, starts its chevron turning.
fn follow_opening(
    popovers: Query<(Entity, &PopoverOpen), Changed<PopoverOpen>>,
    combos: Query<&ComboParts>,
    mut chevrons: Query<&mut Chevron>,
) {
    for (popover, open) in &popovers {
        if let Some(parts) = combos.iter().find(|parts| parts.popover == popover)
            && let Ok(mut chevron) = chevrons.get_mut(parts.chevron)
        {
            chevron.point(open.0);
        }
    }
}

/// Once an opened list is laid out, scrolls its highlight into view.
fn reveal_highlight(
    mut combos: Query<(&mut Combo, &ComboParts, &ChoiceSet)>,
    mut commands: Commands,
) {
    for (mut combo, parts, choices) in &mut combos {
        if std::mem::take(&mut combo.reveal)
            && let Some(row) = choices
                .highlighted()
                .and_then(|index| parts.rows.get(index))
        {
            commands.trigger(ScrollIntoView { entity: row.row });
        }
    }
}

/// Restyles each open list: hides the rows filtered out, tints the rest by
/// their states, and keeps the list at least as wide as the field.
fn restyle_lists(
    combos: Query<(&ComboParts, &ChoiceSet, &ComboTokens)>,
    fields: Query<&ComputedNode>,
    popovers: Query<&PopoverOpen>,
    mut nodes: Query<&mut Node>,
    rows: Query<(&ChoiceState, &ThemeSkin)>,
    mut commands: Commands,
) {
    for (parts, choices, tokens) in &combos {
        if !popovers.get(parts.popover).is_ok_and(|open| open.0) {
            continue;
        }
        if let Ok(field) = fields.get(parts.field) {
            let width = px(field.size().x * field.inverse_scale_factor);
            if let Ok(mut list) = nodes.get_mut(parts.popover)
                && list.min_width != width
            {
                list.min_width = width;
            }
        }
        for (index, &ComboRow { row, .. }) in parts.rows.iter().enumerate() {
            if let Ok(mut node) = nodes.get_mut(row) {
                let display = if choices.is_shown(index) {
                    Display::Flex
                } else {
                    Display::None
                };
                if node.display != display {
                    node.display = display;
                }
            }
            let Ok((state, had)) = rows.get(row) else {
                continue;
            };
            let token = if state.highlighted {
                tokens.option.clone().with_state(WidgetState::Hover)
            } else if state.chosen {
                tokens.option_chosen.clone()
            } else {
                tokens.option.clone()
            };
            let wanted = ThemeSkin(token);
            if *had != wanted {
                commands.entity(row).insert(wanted);
            }
        }
    }
}

/// Styles a combo when its tokens are set: at spawn, and whenever the app sets
/// new ones. Its rows' backgrounds follow in `restyle_lists`, as they show.
fn style_combo(
    insert: On<Insert<ComboTokens>>,
    combos: Query<(&ComboTokens, &ComboParts, &ChoiceSet)>,
    mut commands: Commands,
) {
    let Ok((tokens, parts, choices)) = combos.get(insert.entity) else {
        return;
    };
    commands.entity(parts.field).insert(tokens.field.clone());
    commands
        .entity(parts.chevron)
        .insert(chevron_colour(&tokens.chevron));
    let rounded = ThemeCornerRadius(tokens.field.corner_radius.clone());
    for (index, row) in parts.rows.iter().enumerate() {
        commands
            .entity(row.row)
            .insert((ThemeSkin(tokens.option.clone()), rounded.clone()));
        let Some(label) = row.label else {
            continue;
        };
        let disabled = choices.get(index).is_some_and(Choice::is_disabled);
        let colour = tokens
            .option_text
            .clone()
            .with_state(disabled.then_some(WidgetState::Disabled));
        commands.entity(label).insert((
            ThemeTextColor(colour),
            ThemeFontSize(tokens.field.font_size.clone()),
            ThemeFont(tokens.field.font.clone()),
        ));
    }
}

/// Combo boxes: filtering, choosing, free text, and default token values.
pub struct ComboWidgetPlugin;

impl Plugin for ComboWidgetPlugin {
    fn build(&self, app: &mut App) {
        app.ensure_plugin(ThemePlugin)
            .ensure_plugin(HoverCursorPlugin)
            .ensure_plugin(ChevronPlugin)
            .ensure_plugin(ChoicePlugin)
            .ensure_plugin(PopoverPlugin)
            .ensure_plugin(ScrollAreaPlugin)
            .ensure_plugin(TextInputWidgetPlugin)
            .ensure_plugin(DropdownWidgetPlugin)
            .add_observer(follow_typing)
            .add_observer(work_with_keys)
            .add_observer(settle_on_blur)
            .add_observer(open_on_focus)
            .add_observer(open_on_press)
            .add_observer(keep_focus_in_field)
            .add_observer(follow_picks)
            .add_observer(style_combo)
            .add_systems(Update, reveal_highlight)
            .add_systems(
                PostUpdate,
                (
                    keep_caret_on_the_line.before(EditableTextSystems),
                    // After the focus leaving, which settles too. The field
                    // takes the text in from there, by the next frame.
                    settle_texts.after(InputFocusSystems::FocusChangeEvents),
                    (follow_opening, restyle_lists)
                        .chain()
                        .after(ChoiceSystems)
                        .before(UiSystems::Prepare),
                ),
            );
        ComboTokens::with_prefix(COMBO_STYLE)
            .register_theme_defaults(&mut app.world_mut().resource_mut::<Theme>());
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy::camera::NormalizedRenderTarget;
    use bevy::input::ButtonState;
    use bevy::input_focus::InputFocusPlugin;
    use bevy::picking::backend::HitData;
    use bevy::picking::events::{Pointer, PointerClick};
    use bevy::picking::pointer::{Location, PointerButton, PointerId};

    use super::*;
    use crate::theme::ThemeBorder;

    const CITIES: [&str; 5] = ["Halifax", "Louisville", "Moncton", "St. Louis", "Toronto"];

    /// What the combos announced: indices, and texts.
    #[derive(Resource, Default)]
    struct Announced {
        indices: Vec<usize>,
        texts: Vec<String>,
    }

    struct Combo {
        app: App,
        root: Entity,
        editor: Entity,
    }

    fn combo(config: ComboConfig) -> Combo {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::asset::AssetPlugin::default(),
            bevy::input::InputPlugin,
            bevy::window::WindowPlugin {
                primary_window: None,
                exit_condition: bevy::window::ExitCondition::DontExit,
                ..default()
            },
            InputFocusPlugin,
            bevy::picking::DefaultPickingPlugins,
            bevy::text::TextPlugin,
            ComboWidgetPlugin,
        ))
        .init_resource::<UiScale>()
        .init_resource::<Announced>()
        .add_observer(
            |change: On<ValueChange<usize>>, mut announced: ResMut<Announced>| {
                announced.indices.push(change.value);
            },
        )
        .add_observer(
            |change: On<ValueChange<String>>,
             combos: Query<(), With<ComboText>>,
             mut announced: ResMut<Announced>| {
                // The combo's own, not its field's.
                if combos.contains(change.source) {
                    announced.texts.push(change.value.clone());
                }
            },
        );
        let mut root = None;
        app.world_mut()
            .commands()
            .spawn(Node::default())
            .with_children(|parent| root = Some(spawn_combo(parent, config).id()));
        app.update();
        let root = root.unwrap();
        let editor = app.world().get::<ComboParts>(root).unwrap().editor;
        Combo { app, root, editor }
    }

    impl Combo {
        fn parts(&self) -> &ComboParts {
            self.app.world().get::<ComboParts>(self.root).unwrap()
        }

        fn focus(&mut self, on: bool) {
            let editor = self.editor;
            let mut focus = self.app.world_mut().resource_mut::<InputFocus>();
            if on {
                focus.set(editor, FocusCause::Pressed);
            } else {
                focus.clear();
            }
            self.app.update();
        }

        fn type_text(&mut self, text: &str) {
            let mut editable = self
                .app
                .world_mut()
                .get_mut::<EditableText>(self.editor)
                .unwrap();
            editable.editor_mut().set_text(text);
            self.app.update();
        }

        fn press(&mut self, key: Key) {
            let keyboard = KeyboardInput {
                key_code: bevy::input::keyboard::KeyCode::Unidentified(
                    bevy::input::keyboard::NativeKeyCode::Unidentified,
                ),
                logical_key: key,
                state: ButtonState::Pressed,
                text: None,
                repeat: false,
                window: Entity::PLACEHOLDER,
            };
            let editor = self.editor;
            self.app
                .world_mut()
                .trigger(FocusedInput::new(editor, keyboard, Entity::PLACEHOLDER));
            self.app.update();
            self.app.update();
        }

        fn open(&self) -> bool {
            self.app
                .world()
                .get::<PopoverOpen>(self.parts().popover)
                .unwrap()
                .0
        }

        /// The options the list shows.
        fn shown(&self) -> Vec<usize> {
            let choices = self.app.world().get::<ChoiceSet>(self.root).unwrap();
            (0..choices.len())
                .filter(|&index| choices.is_shown(index))
                .collect()
        }

        fn highlighted(&self) -> Option<usize> {
            self.app
                .world()
                .get::<ChoiceSet>(self.root)
                .unwrap()
                .highlighted()
        }

        fn chosen(&self) -> Option<usize> {
            self.app.world().get::<Chosen>(self.root).unwrap().0
        }

        fn text(&self) -> String {
            self.app
                .world()
                .get::<ComboText>(self.root)
                .unwrap()
                .0
                .clone()
        }

        /// What the field shows, once settled.
        fn field_text(&self) -> String {
            let field = self.parts().field;
            self.app
                .world()
                .get::<TextInputValue>(field)
                .unwrap()
                .0
                .clone()
        }

        fn announced(&mut self) -> Announced {
            std::mem::take(&mut *self.app.world_mut().resource_mut::<Announced>())
        }
    }

    #[test]
    fn typing_narrows_the_list_and_enter_chooses_the_first_left() {
        let mut combo = combo(ComboConfig::new(CITIES));
        combo.focus(true);
        assert!(combo.open(), "the field taking the focus opens the list");
        assert_eq!(combo.shown().len(), CITIES.len());
        combo.type_text("louis");
        assert_eq!(combo.shown(), [1, 3], "ignoring case, anywhere in the name");
        assert_eq!(combo.highlighted(), Some(1));
        combo.press(Key::Enter);
        assert_eq!(combo.chosen(), Some(1));
        assert_eq!(combo.text(), "Louisville");
        assert_eq!(combo.field_text(), "Louisville");
        assert!(!combo.open());
        assert_eq!(combo.announced().indices, [1]);
    }

    #[test]
    fn the_arrows_move_through_what_is_left_and_leaving_chooses_it() {
        let mut combo = combo(ComboConfig::new(CITIES).chosen(0));
        combo.focus(true);
        combo.type_text("o");
        assert_eq!(combo.shown(), [1, 2, 3, 4], "Halifax has no o");
        assert_eq!(combo.highlighted(), Some(1));
        for _ in 0..4 {
            combo.press(Key::ArrowDown);
        }
        assert_eq!(combo.highlighted(), Some(4), "stopping at the last");
        combo.focus(false);
        assert_eq!(combo.chosen(), Some(4), "the focus leaving chooses too");
        assert_eq!(combo.field_text(), "Toronto");
    }

    #[test]
    fn escape_cancels_and_with_nothing_left_the_text_goes_back() {
        let mut combo = combo(ComboConfig::new(CITIES).chosen(2));
        combo.focus(true);
        combo.type_text("st");
        combo.press(Key::Escape);
        assert!(!combo.open());
        assert_eq!(combo.chosen(), Some(2));
        assert_eq!(combo.field_text(), "Moncton");

        combo.focus(true);
        combo.type_text("zzz");
        assert!(!combo.open(), "nothing left to show");
        combo.press(Key::Enter);
        assert_eq!(combo.chosen(), Some(2));
        assert_eq!(combo.field_text(), "Moncton", "back to the choice");
        assert!(combo.announced().indices.is_empty());
    }

    #[test]
    fn clicking_an_option_chooses_it() {
        let mut combo = combo(ComboConfig::new(CITIES));
        combo.focus(true);
        let row = combo.parts().rows[3].row;
        combo.app.world_mut().trigger(PointerClick {
            entity: row,
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
        combo.app.update();
        assert_eq!(combo.chosen(), Some(3));
        assert_eq!(combo.field_text(), "St. Louis");
        assert!(!combo.open());
    }

    #[test]
    fn new_tokens_restyle_the_field_chevron_and_rows() {
        let options = [
            Choice::text("Halifax"),
            Choice::text("Moncton").disabled(true),
        ];
        let mut combo = combo(ComboConfig::new(options));
        let tokens = ComboTokens::with_prefix("game.city");
        combo
            .app
            .world_mut()
            .entity_mut(combo.root)
            .insert(tokens.clone());
        combo.app.update();

        let parts = combo.parts().clone();
        let world = combo.app.world();
        assert_eq!(
            world.get::<TextInputTokens>(parts.field),
            Some(&tokens.field)
        );
        let chevron = world.get::<ThemeBorder>(parts.chevron).unwrap();
        assert_eq!(*chevron, ThemeBorder::all(tokens.chevron.clone()));
        let colour = |row: usize| {
            let label = parts.rows[row].label.unwrap();
            world.get::<ThemeTextColor>(label).unwrap().0.clone()
        };
        assert_eq!(colour(0), tokens.option_text);
        assert_eq!(
            colour(1),
            tokens.option_text.clone().with_state(WidgetState::Disabled)
        );
    }

    #[test]
    fn free_text_keeps_what_was_typed_unless_an_option_is_moved_to() {
        let mut combo = combo(ComboConfig::new(["md", "ron", "txt"]).free_text(true));
        combo.focus(true);
        combo.type_text("report");
        assert!(!combo.open(), "no suggestions left");
        combo.press(Key::Enter);
        assert_eq!(combo.text(), "report");
        assert_eq!(combo.chosen(), None);

        combo.type_text("t");
        assert_eq!(
            combo.highlighted(),
            None,
            "suggestions aren't chosen for you"
        );
        combo.press(Key::Enter);
        assert_eq!(combo.text(), "t");

        combo.type_text("r");
        combo.press(Key::ArrowDown);
        combo.press(Key::Enter);
        assert_eq!(combo.text(), "ron");
        assert_eq!(combo.field_text(), "ron");
        assert_eq!(combo.chosen(), Some(1));
        let announced = combo.announced();
        assert_eq!(announced.texts, ["report", "t", "ron"]);
        assert!(announced.indices.is_empty(), "its value is its text");
    }
}
