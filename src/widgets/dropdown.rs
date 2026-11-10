//! Dropdowns: a button showing the chosen option, opening a list to choose
//! another.
//!
//! Options are [choices](crate::choice), so anything touchy can draw. A plain
//! [`Choice`] is its label as text; a richer one builds whatever its row should
//! hold (an icon and a name, a swatch, a skinned plate, a meter) with a closure.
//!
//! The list is a [popover](crate::popover) under the button, as wide as the
//! button at least, holding a [scroll area](crate::containers::scroll_area).
//!
//! A [menu](crate::widgets::menu) is a dropdown that shows a title and keeps
//! no choice, and is built on this one.

use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input_focus::FocusedInput;
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::picking::events::PointerClick;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui::{InteractionDisabled, UiSystems};
use bevy::ui_widgets::ScrollIntoView;

use crate::EnsurePlugin;
use crate::choice::{
    Choice, ChoiceItem, ChoicePlugin, ChoiceSet, ChoiceState, ChoiceSystems, Chosen, Pick, Step,
};
use crate::containers::scroll_area::{ScrollAreaConfig, ScrollAreaPlugin, spawn_scroll_area};
use crate::cursor::{HoverCursor, HoverCursorPlugin};
use crate::keyed::SyncChildren;
use crate::popover::{PopoverConfig, PopoverOpen, PopoverPlugin, spawn_popover};
use crate::sanitize::at_least;
use crate::theme::palette::{
    ACCENT, CONTROL, CONTROL_CORNER_RADIUS, CONTROL_TEXT_SIZE, EDGE, FONT, HIGHLIGHT, TEXT,
    TEXT_MUTED,
};
use crate::theme::skins;
use crate::theme::{
    FocusVisible, Shade, Theme, ThemeBorder, ThemeCornerRadius, ThemeFont, ThemeFontSize,
    ThemePlugin, ThemeSkin, ThemeTextColor, ThemeToken, WidgetFlags, WidgetState,
};
use crate::widgets::chevron::{Chevron, ChevronPlugin, ChevronTurn, chevron_colour, spawn_chevron};

/// Prefix of the dropdown's tokens: `touchy.dropdown.bg`, and so on.
pub const DROPDOWN_STYLE: &str = "touchy.dropdown";

/// The width of the button's border, in pixels.
const BORDER: f32 = 1.0;
/// Space inside the button's edge, across and down, in pixels.
const PADDING: Vec2 = Vec2::new(10.0, 6.0);
/// Space between the chosen option and the chevron.
const CHEVRON_GAP: f32 = 10.0;
/// Space inside an option's row, across and down.
const OPTION_PADDING: Vec2 = Vec2::new(8.0, 5.0);
/// Space inside the list's edge.
const LIST_PADDING: f32 = 4.0;
/// The tallest the list grows before it scrolls, unless the config says otherwise.
pub(crate) const DEFAULT_LIST_HEIGHT: f32 = 240.0;
/// How strongly a highlighted option, and the chosen one, are tinted.
const HIGHLIGHT_ALPHA: f32 = 0.2;
const CHOSEN_ALPHA: f32 = 0.35;
/// How faint a disabled dropdown is.
const DISABLED_ALPHA: f32 = 0.4;

/// The dropdown's look: its button, its options' rows, and its text. The list
/// itself is a popover, in the popover's tokens. Insert new ones to restyle a
/// dropdown.
#[derive(Component, Clone, PartialEq, Debug)]
#[component(immutable)]
pub struct DropdownTokens {
    /// The button's face: hovered, held open and disabled are its states.
    pub bg: ThemeToken,
    /// The button's border, lit while hovered or open.
    pub border: ThemeToken,
    /// The chosen option's text, and every option's.
    pub text: ThemeToken,
    /// The placeholder, shown while nothing's chosen.
    pub placeholder: ThemeToken,
    /// The chevron at the button's end.
    pub chevron: ThemeToken,
    /// An option's row: clear at rest, tinted while highlighted.
    pub option: ThemeToken,
    /// The chosen option's row.
    pub option_chosen: ThemeToken,
    /// The text's font.
    pub font: ThemeToken,
    /// The text's size.
    pub font_size: ThemeToken,
    /// The radius of the button's and the rows' corners.
    pub corner_radius: ThemeToken,
}

impl DropdownTokens {
    /// Names the tokens under `prefix`.
    pub fn with_prefix(prefix: &str) -> Self {
        let token = |suffix: &str| ThemeToken::new(format!("{prefix}.{suffix}"));
        Self {
            bg: token("bg"),
            border: token("border"),
            text: token("text"),
            placeholder: token("placeholder"),
            chevron: token("chevron"),
            option: token("option"),
            option_chosen: token("option_chosen"),
            font: token("font"),
            font_size: token("font_size"),
            corner_radius: token("corner_radius"),
        }
    }

    /// Derives these tokens from the palette, unless the app sets them.
    pub fn register_theme_defaults(&self, theme: &mut Theme) {
        let state = |token: &ThemeToken, state| token.clone().with_state(state);
        theme
            .default_derived(self.bg.clone(), CONTROL, Shade::Same)
            // Held open, the button keeps its face; the border shows it's open.
            .default_derived(state(&self.bg, WidgetState::Pressed), CONTROL, Shade::Same)
            .default_derived(
                state(&self.bg, WidgetState::Disabled),
                CONTROL,
                Shade::Alpha(DISABLED_ALPHA),
            )
            .default_derived(self.border.clone(), EDGE, Shade::Same)
            .default_derived(
                state(&self.border, WidgetState::Hover),
                HIGHLIGHT,
                Shade::Same,
            )
            .default_derived(
                state(&self.border, WidgetState::Focused),
                HIGHLIGHT,
                Shade::Same,
            )
            .default_derived(
                state(&self.border, WidgetState::Pressed),
                HIGHLIGHT,
                Shade::Same,
            )
            .default_derived(self.text.clone(), TEXT, Shade::Same)
            .default_derived(
                state(&self.text, WidgetState::Disabled),
                TEXT_MUTED,
                Shade::Alpha(DISABLED_ALPHA),
            )
            .default_derived(self.placeholder.clone(), TEXT_MUTED, Shade::Same)
            .default_derived(self.chevron.clone(), TEXT_MUTED, Shade::Same)
            .default_color(self.option.clone(), Color::NONE)
            .default_derived(
                state(&self.option, WidgetState::Hover),
                HIGHLIGHT,
                Shade::Alpha(HIGHLIGHT_ALPHA),
            )
            .default_derived(
                self.option_chosen.clone(),
                ACCENT,
                Shade::Alpha(CHOSEN_ALPHA),
            )
            .default_font_from(self.font.clone(), FONT)
            .default_size_from(self.font_size.clone(), CONTROL_TEXT_SIZE)
            .default_size_from(self.corner_radius.clone(), CONTROL_CORNER_RADIUS);
        // Each part draws its role's skin, once a theme gives the role one.
        theme.default_skin_from(self.bg.clone(), skins::RAISED);
    }
}

/// The built-in token set.
impl Default for DropdownTokens {
    fn default() -> Self {
        Self::with_prefix(DROPDOWN_STYLE)
    }
}

/// How to spawn a dropdown: its options, its starting choice, and its look.
#[derive(Clone, Debug)]
pub struct DropdownConfig {
    /// The options to choose from, in order.
    pub options: Vec<Choice>,
    /// The option chosen at spawn, if any.
    pub chosen: Option<usize>,
    /// Shown, faded, in the button while nothing's chosen.
    pub placeholder: String,
    /// The button's width, in pixels. Without one, it's as wide as what's chosen.
    pub width: Option<f32>,
    /// The tallest the list grows before it scrolls, in pixels.
    pub list_height: f32,
    /// Whether the dropdown starts disabled.
    pub disabled: bool,
    /// The dropdown's theme tokens.
    pub tokens: DropdownTokens,
}

impl DropdownConfig {
    /// A dropdown of `options`, nothing chosen. Plain words make plain options,
    /// so `DropdownConfig::new(["Easy", "Normal", "Hard"])` is a whole dropdown.
    pub fn new<O: Into<Choice>>(options: impl IntoIterator<Item = O>) -> Self {
        Self {
            options: options.into_iter().map(Into::into).collect(),
            chosen: None,
            placeholder: String::new(),
            width: None,
            list_height: DEFAULT_LIST_HEIGHT,
            disabled: false,
            tokens: DropdownTokens::with_prefix(DROPDOWN_STYLE),
        }
    }

    /// Replace the options with anything that turns into a list of choices.
    pub fn options<O: Into<Choice>>(mut self, options: impl IntoIterator<Item = O>) -> Self {
        self.options = options.into_iter().map(Into::into).collect();
        self
    }

    /// Starts with the option at `index` chosen.
    pub fn chosen(mut self, index: usize) -> Self {
        self.chosen = Some(index);
        self
    }

    /// Sets the placeholder text when no Choice is chosen.
    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    /// Fixes the button's width, in pixels, so choosing doesn't resize it.
    pub fn width(mut self, pixels: f32) -> Self {
        self.width = Some(pixels);
        self
    }

    /// The tallest the list grows before it scrolls, in pixels.
    pub fn list_height(mut self, pixels: f32) -> Self {
        self.list_height = pixels;
        self
    }

    /// Disables the dropdown, preventing user interaction.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Sets the dropdown's theme tokens.
    pub fn tokens(mut self, tokens: DropdownTokens) -> Self {
        self.tokens = tokens;
        self
    }

    /// This config with every value made usable, warning about each repair.
    fn sanitized(mut self) -> Self {
        self.list_height = at_least(self.list_height, 0.0, "DropdownConfig list height");
        self.width = at_least(self.width, 0.0, "DropdownConfig width");
        self
    }
}

/// The parts of a dropdown, on the dropdown.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub struct DropdownParts {
    /// The button that opens the list.
    pub button: Entity,
    /// Where the button shows the chosen option, the placeholder, or a menu's title.
    pub face: Entity,
    /// The chevron at the button's end; a menu has none.
    pub chevron: Option<Entity>,
    /// The popover holding the list.
    pub popover: Entity,
    /// The list's rows, one for each option, in order.
    pub rows: Vec<DropdownRow>,
}

/// The hidden labels that size a dropdown's button to its widest option, in the button's font.
#[derive(Component, Clone, Debug)]
struct FaceSizer(Vec<Entity>);

/// One row of a dropdown's list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DropdownRow {
    /// The row.
    pub row: Entity,
    /// The option's label in it, unless the option builds its own look.
    pub label: Option<Entity>,
}

/// A dropdown's state. Its options and their highlight are its [`ChoiceSet`].
#[derive(Component, Clone, Debug)]
struct Dropdown {
    placeholder: String,
    /// A menu's title, shown in place of a choice.
    title: Option<String>,
    /// Whether to scroll the highlighted row into view once the list's laid out.
    reveal: bool,
}

/// The dropdown's button.
#[derive(Component, Clone, Copy, Debug)]
struct DropdownButton(Entity);

/// What a key does to a focused dropdown. The one place its keys are read,
/// for the key mapping to take over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DropdownKey {
    /// Opens the list, or chooses the highlighted option.
    Confirm,
    /// Opens the list, or moves the highlight down it.
    Next,
    /// Opens the list, or moves the highlight up it.
    Previous,
    /// Moves the highlight to the first option.
    First,
    /// Moves the highlight to the last option.
    Last,
}

impl DropdownKey {
    fn of(key: &Key) -> Option<Self> {
        match key {
            Key::Enter | Key::Space => Some(Self::Confirm),
            Key::ArrowDown => Some(Self::Next),
            Key::ArrowUp => Some(Self::Previous),
            Key::Home => Some(Self::First),
            Key::End => Some(Self::Last),
            _ => None,
        }
    }
}

/// The dropdown's font and size, and `colour`, for its text.
fn text_style(
    colour: &ThemeToken,
    tokens: &DropdownTokens,
) -> (ThemeTextColor, ThemeFontSize, ThemeFont) {
    (
        ThemeTextColor(colour.clone()),
        ThemeFontSize(tokens.font_size.clone()),
        ThemeFont(tokens.font.clone()),
    )
}

/// Text in the dropdown's font, coloured by `colour`, on one line.
fn text(content: &str, colour: &ThemeToken, tokens: &DropdownTokens) -> impl Bundle + use<> {
    (
        Text::new(content),
        TextLayout::no_wrap(),
        text_style(colour, tokens),
    )
}

/// Spawns it; documented on [`SpawnUi::dropdown`](crate::spawn_ui::SpawnUi::dropdown).
pub(crate) fn spawn_dropdown<'a>(
    parent: &'a mut ChildSpawnerCommands,
    config: DropdownConfig,
) -> EntityCommands<'a> {
    spawn_list_button(parent, config, None)
}

/// Spawns a dropdown, or with a `title`, a menu: a button showing the title,
/// with no chevron, whose list keeps no choice and announces none.
pub(crate) fn spawn_list_button<'a>(
    parent: &'a mut ChildSpawnerCommands,
    config: DropdownConfig,
    title: Option<String>,
) -> EntityCommands<'a> {
    let DropdownConfig {
        options,
        chosen,
        placeholder,
        width,
        list_height,
        disabled,
        tokens,
    } = config.sanitized();
    // A menu keeps no choice.
    let chosen = chosen.filter(|&index| index < options.len() && title.is_none());
    // The dropdown is the popover's anchor, holding the button and the list
    // side by side, and takes the focus, so the keys reach it with the list open.
    let mut dropdown = parent.spawn((
        Node {
            flex_direction: FlexDirection::Column,
            align_self: AlignSelf::Start,
            ..default()
        },
        Chosen(chosen),
        TabIndex(0),
    ));
    let id = dropdown.id();
    let mut face_sizer = None;
    let mut parts = DropdownParts {
        button: Entity::PLACEHOLDER,
        face: Entity::PLACEHOLDER,
        chevron: None,
        popover: Entity::PLACEHOLDER,
        rows: Vec::new(),
    };
    dropdown.with_children(|anchor| {
        let mut button = anchor.spawn((
            Node {
                width: width.map_or(Val::Auto, px),
                align_items: AlignItems::Center,
                column_gap: px(CHEVRON_GAP),
                padding: UiRect::axes(px(PADDING.x), px(PADDING.y)),
                border: UiRect::all(px(BORDER)),
                ..default()
            },
            DropdownButton(id),
            Hovered::default(),
            HoverCursor::POINTER,
        ));
        parts.button = button.id();
        button.with_children(|button| {
            // The face and a hidden copy of every option share one grid cell,
            // so the button is as wide as its widest option. Prevents jumps in width.
            let in_slot = || Node {
                grid_row: GridPlacement::start(1),
                grid_column: GridPlacement::start(1),
                align_items: AlignItems::Center,
                ..default()
            };
            // A width the app pins can be narrower than the words, so they're cut off rather than wrapped.
            let slot = Node {
                display: Display::Grid,
                flex_grow: 1.0,
                min_width: px(0.0),
                overflow: Overflow::clip(),
                ..default()
            };
            button
                .spawn((slot, Pickable::IGNORE))
                .with_children(|slot| {
                    parts.face = slot.spawn((in_slot(), Pickable::IGNORE)).id();
                    if title.is_some() {
                        return;
                    }
                    let words = options.iter().map(|option| option.label.as_str());
                    let sizer = Node {
                        display: Display::Grid,
                        ..in_slot()
                    };
                    let mut labels = Vec::new();
                    slot.spawn((sizer, Visibility::Hidden, Pickable::IGNORE))
                        .with_children(|sizer| {
                            for words in words.chain([placeholder.as_str()]) {
                                let label = (
                                    Text::new(words),
                                    TextLayout::no_wrap(),
                                    in_slot(),
                                    Pickable::IGNORE,
                                );
                                labels.push(sizer.spawn(label).id());
                            }
                        });
                    face_sizer = Some(FaceSizer(labels));
                });
            // A menu's title says enough without one.
            if title.is_none() {
                let chevron = spawn_chevron(button, UiRect::ZERO, ChevronTurn::LIST, false);
                parts.chevron = Some(chevron);
            }
        });
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
                            // One line each, so the list is as wide as its widest option.
                            label =
                                option.build(row, |label| (Text::new(label), TextLayout::no_wrap()))
                        });
                        parts.rows.push(DropdownRow {
                            row: row.id(),
                            label,
                        });
                    }
                },
            );
        });
        parts.popover = popover.id();
    });
    let mut choices = ChoiceSet::new(options).quiet(title.is_some());
    choices.highlight(chosen);
    // Before its tokens, so `style_dropdown` finds it.
    if let Some(sizer) = face_sizer {
        dropdown.insert(sizer);
    }
    // Its look comes with its tokens, in `style_dropdown`, once its parts are in.
    dropdown.insert((
        choices,
        Dropdown {
            placeholder,
            title,
            reveal: false,
        },
        parts,
        tokens,
    ));
    // After its parts: its button is disabled with it, in `disable_button`.
    if disabled {
        dropdown.insert(InteractionDisabled);
    }
    dropdown
}

/// Disabling a dropdown disables its button too, so the cursor over it says so.
fn disable_button(
    insert: On<Insert<InteractionDisabled>>,
    dropdowns: Query<&DropdownParts>,
    mut commands: Commands,
) {
    if let Ok(parts) = dropdowns.get(insert.entity) {
        commands.entity(parts.button).insert(InteractionDisabled);
    }
}

/// Enabling a dropdown enables its button again.
fn enable_button(
    remove: On<Remove<InteractionDisabled>>,
    dropdowns: Query<&DropdownParts>,
    mut commands: Commands,
) {
    if let Ok(parts) = dropdowns.get(remove.entity) {
        commands
            .entity(parts.button)
            .remove::<InteractionDisabled>();
    }
}

/// Clicking the button opens or closes the list.
fn toggle_on_click(
    mut click: On<PointerClick>,
    buttons: Query<&DropdownButton>,
    dropdowns: Query<&DropdownParts, Without<InteractionDisabled>>,
    mut popovers: Query<&mut PopoverOpen>,
) {
    let Ok(&DropdownButton(dropdown)) = buttons.get(click.entity) else {
        return;
    };
    click.propagate(false);
    if let Ok(parts) = dropdowns.get(dropdown)
        && let Ok(mut open) = popovers.get_mut(parts.popover)
    {
        open.0 = !open.0;
    }
}

/// Picking an option, by pointer or key, closes the list.
fn close_on_pick(
    pick: On<Pick>,
    dropdowns: Query<(&DropdownParts, &ChoiceSet), Without<InteractionDisabled>>,
    mut popovers: Query<&mut PopoverOpen>,
) {
    let Ok((parts, choices)) = dropdowns.get(pick.set) else {
        return;
    };
    if pick.index.is_some_and(|index| choices.can_pick(index))
        && let Ok(mut open) = popovers.get_mut(parts.popover)
    {
        open.set_if_neq(PopoverOpen(false));
    }
}

/// The keys, while the dropdown has the focus: Enter, Space or an arrow opens
/// the list; the arrows, Home and End move the highlight; Enter or Space
/// chooses it.
fn work_with_keys(
    mut keys: On<FocusedInput<KeyboardInput>>,
    mut dropdowns: Query<(&DropdownParts, &mut ChoiceSet), Without<InteractionDisabled>>,
    mut popovers: Query<&mut PopoverOpen>,
    mut commands: Commands,
) {
    let dropdown = keys.focused_entity;
    let Ok((parts, mut choices)) = dropdowns.get_mut(dropdown) else {
        return;
    };
    if !keys.input.state.is_pressed() {
        return;
    }
    let Some(key) = DropdownKey::of(&keys.input.logical_key) else {
        return;
    };
    let Ok(mut open) = popovers.get_mut(parts.popover) else {
        return;
    };
    let step = match (key, open.0) {
        (DropdownKey::Confirm, true) => {
            match choices.highlighted() {
                Some(index) => commands.trigger(Pick {
                    set: dropdown,
                    index: Some(index),
                }),
                None => open.0 = false,
            }
            keys.propagate(false);
            return;
        }
        (DropdownKey::Confirm | DropdownKey::Next | DropdownKey::Previous, false) => {
            open.0 = true;
            keys.propagate(false);
            return;
        }
        (DropdownKey::Next, true) => Step::Next,
        (DropdownKey::Previous, true) => Step::Previous,
        (DropdownKey::First, true) => Step::First,
        (DropdownKey::Last, true) => Step::Last,
        (DropdownKey::First | DropdownKey::Last, false) => return,
    };
    keys.propagate(false);
    if let Some(row) = choices.step(step).and_then(|index| parts.rows.get(index)) {
        commands.trigger(ScrollIntoView { entity: row.row });
    }
}

/// As the list opens: highlights the chosen option, and starts the chevron
/// turning. As it closes, turns it back.
fn follow_opening(
    popovers: Query<(Entity, &PopoverOpen), Changed<PopoverOpen>>,
    mut dropdowns: Query<(&mut Dropdown, &DropdownParts, &mut ChoiceSet, &Chosen)>,
    mut chevrons: Query<&mut Chevron>,
) {
    for (popover, open) in &popovers {
        let Some((mut dropdown, parts, mut choices, chosen)) = dropdowns
            .iter_mut()
            .find(|(_, parts, ..)| parts.popover == popover)
        else {
            continue;
        };
        if open.0 {
            choices.highlight(chosen.0);
            dropdown.reveal = true;
        }
        if let Some(chevron) = parts.chevron
            && let Ok(mut chevron) = chevrons.get_mut(chevron)
        {
            chevron.point(open.0);
        }
    }
}

/// Once an opened list is laid out, scrolls its chosen option into view.
fn reveal_chosen(
    mut dropdowns: Query<(&mut Dropdown, &DropdownParts, &ChoiceSet)>,
    mut commands: Commands,
) {
    for (mut dropdown, parts, choices) in &mut dropdowns {
        if !std::mem::take(&mut dropdown.reveal) {
            continue;
        }
        if let Some(row) = choices
            .highlighted()
            .and_then(|index| parts.rows.get(index))
        {
            commands.trigger(ScrollIntoView { entity: row.row });
        }
    }
}

/// What the button shows changed: the choice, or its tokens.
type FaceChanged = Or<(Changed<Chosen>, Changed<DropdownTokens>)>;

/// Shows the chosen option in the button: built afresh only when the choice
/// changes, as a keyed child, or when new tokens restyle it.
fn show_chosen(
    dropdowns: Query<
        (
            &Dropdown,
            &DropdownParts,
            &ChoiceSet,
            &Chosen,
            Ref<DropdownTokens>,
        ),
        FaceChanged,
    >,
    mut commands: Commands,
) {
    for (dropdown, parts, choices, chosen, tokens) in &dropdowns {
        if tokens.is_changed() && !tokens.is_added() {
            // The same choice in a new look: built again from scratch.
            commands.entity(parts.face).despawn_related::<Children>();
        }
        // A menu always shows its title, in the text's colour, as a choice
        // would show; a dropdown with no choice shows its placeholder, faded.
        let (chosen, words, colour) = match &dropdown.title {
            Some(title) => (None, title.clone(), tokens.text.clone()),
            None => (
                chosen.0.filter(|&index| index < choices.len()),
                dropdown.placeholder.clone(),
                tokens.placeholder.clone(),
            ),
        };
        let (choices, tokens) = (choices.choices().to_vec(), DropdownTokens::clone(&tokens));
        commands.entity(parts.face).sync_children(
            [chosen],
            |chosen| *chosen,
            move |face, chosen| match chosen {
                Some(index) => {
                    choices[*index].build(face, |label| text(label, &tokens.text, &tokens));
                }
                None => {
                    face.spawn((text(&words, &colour, &tokens), Pickable::IGNORE));
                }
            },
        );
    }
}

/// What the button's look is picked from.
type DropdownLook = (
    &'static DropdownParts,
    &'static DropdownTokens,
    Has<InteractionDisabled>,
    Has<FocusVisible>,
);

/// The button's face and border in this state.
fn button_look(tokens: &DropdownTokens, flags: WidgetFlags) -> (ThemeSkin, ThemeBorder) {
    // Held open is pressed.
    let state = flags.state();
    (
        ThemeSkin(tokens.bg.clone().with_state(state)),
        ThemeBorder::all(tokens.border.clone().with_state(state)),
    )
}

/// Restyles each dropdown: its button by hover, the open list and being
/// disabled; its rows by their choices' states. Keeps the list at least as
/// wide as the button.
fn restyle_dropdowns(
    dropdowns: Query<DropdownLook>,
    buttons: Query<(&Hovered, &ComputedNode, &ThemeSkin, &ThemeBorder)>,
    rows: Query<(&ChoiceState, &ThemeSkin)>,
    popovers: Query<&PopoverOpen>,
    mut nodes: Query<&mut Node>,
    mut commands: Commands,
) {
    for (parts, tokens, disabled, focused) in &dropdowns {
        let Ok((hovered, button, had_bg, had_border)) = buttons.get(parts.button) else {
            continue;
        };
        let open = popovers.get(parts.popover).is_ok_and(|open| open.0);
        let flags = WidgetFlags {
            hovered: hovered.0,
            focused,
            pressed: open,
            disabled,
        };
        let (bg, border) = button_look(tokens, flags);
        if *had_bg != bg {
            commands.entity(parts.button).insert(bg);
        }
        if *had_border != border {
            commands.entity(parts.button).insert(border);
        }
        if !open {
            continue;
        }
        let width = px(button.size().x * button.inverse_scale_factor);
        if let Ok(mut list) = nodes.get_mut(parts.popover)
            && list.min_width != width
        {
            list.min_width = width;
        }
        for row in &parts.rows {
            let Ok((state, had)) = rows.get(row.row) else {
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
                commands.entity(row.row).insert(wanted);
            }
        }
    }
}

/// Styles a dropdown when its tokens are set: at spawn, and whenever the app
/// sets new ones. The button's face follows in `show_chosen`, and the rows'
/// highlights in `restyle_dropdowns`, as they show.
fn style_dropdown(
    insert: On<Insert<DropdownTokens>>,
    dropdowns: Query<(DropdownLook, &ChoiceSet, Option<&FaceSizer>)>,
    hovered: Query<&Hovered>,
    popovers: Query<&PopoverOpen>,
    mut commands: Commands,
) {
    let Ok(((parts, tokens, disabled, focused), choices, sizer)) = dropdowns.get(insert.entity)
    else {
        return;
    };
    // The sizer's labels measure in the face's font, so the width fits it.
    for &label in sizer.map_or(&[][..], |sizer| &sizer.0) {
        let font = (
            ThemeFontSize(tokens.font_size.clone()),
            ThemeFont(tokens.font.clone()),
        );
        commands.entity(label).insert(font);
    }
    let flags = WidgetFlags {
        hovered: hovered.get(parts.button).is_ok_and(|hovered| hovered.0),
        focused,
        pressed: popovers.get(parts.popover).is_ok_and(|open| open.0),
        disabled,
    };
    let rounded = ThemeCornerRadius(tokens.corner_radius.clone());
    let (bg, border) = button_look(tokens, flags);
    commands
        .entity(parts.button)
        .insert((bg, border, rounded.clone()));
    if let Some(chevron) = parts.chevron {
        commands
            .entity(chevron)
            .insert(chevron_colour(&tokens.chevron));
    }
    for (index, row) in parts.rows.iter().enumerate() {
        commands
            .entity(row.row)
            .insert((ThemeSkin(tokens.option.clone()), rounded.clone()));
        let Some(label) = row.label else {
            continue;
        };
        let disabled = choices.get(index).is_some_and(Choice::is_disabled);
        let colour = tokens
            .text
            .clone()
            .with_state(disabled.then_some(WidgetState::Disabled));
        commands.entity(label).insert(text_style(&colour, tokens));
    }
}

/// Dropdowns: opening, choosing, the keys, and default token values.
pub struct DropdownWidgetPlugin;

impl Plugin for DropdownWidgetPlugin {
    fn build(&self, app: &mut App) {
        app.ensure_plugin(ThemePlugin)
            .ensure_plugin(HoverCursorPlugin)
            .ensure_plugin(ChevronPlugin)
            .ensure_plugin(ChoicePlugin)
            .ensure_plugin(PopoverPlugin)
            .ensure_plugin(ScrollAreaPlugin)
            .add_observer(toggle_on_click)
            .add_observer(close_on_pick)
            .add_observer(work_with_keys)
            .add_observer(disable_button)
            .add_observer(enable_button)
            .add_observer(style_dropdown)
            .add_systems(Update, reveal_chosen)
            .add_systems(
                PostUpdate,
                (
                    // Highlighting the choice as the list opens shows in its
                    // rows' states the same frame.
                    follow_opening.before(ChoiceSystems),
                    (show_chosen, restyle_dropdowns).after(ChoiceSystems),
                )
                    .chain()
                    .before(UiSystems::Prepare),
            );
        DropdownTokens::with_prefix(DROPDOWN_STYLE)
            .register_theme_defaults(&mut app.world_mut().resource_mut::<Theme>());
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::time::Duration;

    use bevy::camera::NormalizedRenderTarget;
    use bevy::input::ButtonState;
    use bevy::picking::backend::HitData;
    use bevy::picking::cursor::EntityCursor;
    use bevy::picking::events::Pointer;
    use bevy::picking::pointer::{Location, PointerButton, PointerId};
    use bevy::ui_widgets::ValueChange;
    use bevy::window::SystemCursorIcon;

    use super::*;

    #[derive(Resource, Default)]
    struct Announced(Vec<usize>);

    /// An app with what a dropdown needs, hearing what it announces.
    pub(crate) fn list_app() -> App {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::input::InputPlugin,
            bevy::window::WindowPlugin {
                primary_window: None,
                exit_condition: bevy::window::ExitCondition::DontExit,
                ..default()
            },
            DropdownWidgetPlugin,
        ))
        .init_resource::<Announced>()
        .add_observer(
            |change: On<ValueChange<usize>>, mut announced: ResMut<Announced>| {
                announced.0.push(change.value);
            },
        );
        app
    }

    fn dropdown_app(config: DropdownConfig) -> (App, Entity) {
        let mut app = list_app();
        let mut dropdown = None;
        app.world_mut()
            .commands()
            .spawn(Node::default())
            .with_children(|parent| dropdown = Some(spawn_dropdown(parent, config).id()));
        app.update();
        (app, dropdown.unwrap())
    }

    /// The button keeps the width of its widest option: hidden beside the
    /// face are every option and the placeholder, in the face's font.
    #[test]
    fn the_button_is_sized_by_every_option_and_the_placeholder() {
        let config = DropdownConfig::new(["Button", "Capsule flip"]).placeholder("Pick one");
        let (app, dropdown) = dropdown_app(config);
        let sizer = app.world().get::<FaceSizer>(dropdown).unwrap();
        let words: Vec<_> = sizer
            .0
            .iter()
            .map(|&label| app.world().get::<Text>(label).unwrap().0.clone())
            .collect();
        assert_eq!(words, ["Button", "Capsule flip", "Pick one"]);
        let label = sizer.0[0];
        let tokens = DropdownTokens::with_prefix(DROPDOWN_STYLE);
        assert_eq!(
            app.world().get::<ThemeFontSize>(label),
            Some(&ThemeFontSize(tokens.font_size))
        );
        let holder = app.world().get::<ChildOf>(label).unwrap().parent();
        assert_eq!(
            app.world().get::<Visibility>(holder),
            Some(&Visibility::Hidden)
        );
    }

    pub(crate) fn parts(app: &App, dropdown: Entity) -> DropdownParts {
        app.world().get::<DropdownParts>(dropdown).unwrap().clone()
    }

    pub(crate) fn open(app: &App, dropdown: Entity) -> bool {
        let popover = parts(app, dropdown).popover;
        app.world().get::<PopoverOpen>(popover).unwrap().0
    }

    pub(crate) fn chosen(app: &App, dropdown: Entity) -> Option<usize> {
        app.world().get::<Chosen>(dropdown).unwrap().0
    }

    fn highlighted(app: &App, dropdown: Entity) -> Option<usize> {
        app.world()
            .get::<ChoiceSet>(dropdown)
            .unwrap()
            .highlighted()
    }

    pub(crate) fn announced(app: &mut App) -> Vec<usize> {
        std::mem::take(&mut app.world_mut().resource_mut::<Announced>().0)
    }

    pub(crate) fn press(app: &mut App, dropdown: Entity, key: Key) {
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
        app.world_mut()
            .trigger(FocusedInput::new(dropdown, keyboard, Entity::PLACEHOLDER));
        app.update();
    }

    pub(crate) fn click(app: &mut App, entity: Entity) {
        let pointer = Pointer::new(
            PointerId::Mouse,
            Location {
                target: NormalizedRenderTarget::None {
                    width: 1,
                    height: 1,
                },
                position: Vec2::ZERO,
            },
        );
        app.world_mut().trigger(PointerClick {
            entity,
            pointer,
            button: PointerButton::Primary,
            hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
            duration: Duration::ZERO,
            count: 1,
        });
        app.update();
    }

    /// Whether `entity` is somewhere inside `ancestor`.
    fn inside(world: &World, entity: Entity, ancestor: Entity) -> bool {
        let mut at = entity;
        while let Some(parent) = world.get::<ChildOf>(at) {
            if parent.parent() == ancestor {
                return true;
            }
            at = parent.parent();
        }
        false
    }

    /// The text showing in the button, if any.
    pub(crate) fn face_text(app: &mut App, dropdown: Entity) -> Option<String> {
        face(app, dropdown).map(|(_, text)| text)
    }

    /// The text showing in the button, and its entity.
    fn face(app: &mut App, dropdown: Entity) -> Option<(Entity, String)> {
        let face = parts(app, dropdown).face;
        let world = app.world_mut();
        let texts: Vec<(Entity, String)> = world
            .query::<(Entity, &Text)>()
            .iter(world)
            .map(|(entity, text)| (entity, text.0.clone()))
            .collect();
        texts
            .into_iter()
            .find(|(text, _)| inside(world, *text, face))
    }

    #[test]
    fn the_keys_open_the_list_move_through_it_and_choose() {
        let (mut app, dropdown) = dropdown_app(DropdownConfig::new(["Easy", "Normal", "Hard"]));
        press(&mut app, dropdown, Key::ArrowDown);
        assert!(open(&app, dropdown), "an arrow opens it");
        assert_eq!(highlighted(&app, dropdown), None, "nothing chosen yet");
        press(&mut app, dropdown, Key::ArrowDown);
        press(&mut app, dropdown, Key::ArrowDown);
        press(&mut app, dropdown, Key::ArrowDown);
        press(&mut app, dropdown, Key::ArrowDown);
        assert_eq!(highlighted(&app, dropdown), Some(2), "stopping at the last");
        press(&mut app, dropdown, Key::Home);
        press(&mut app, dropdown, Key::ArrowDown);
        press(&mut app, dropdown, Key::Enter);
        assert_eq!(chosen(&app, dropdown), Some(1));
        assert!(!open(&app, dropdown), "choosing closes it");
        assert_eq!(announced(&mut app), [1]);

        press(&mut app, dropdown, Key::Space);
        assert_eq!(
            highlighted(&app, dropdown),
            Some(1),
            "reopened at the choice"
        );
        press(&mut app, dropdown, Key::Enter);
        assert!(announced(&mut app).is_empty(), "the same choice isn't news");
    }

    #[test]
    fn clicking_opens_the_list_and_chooses() {
        let (mut app, dropdown) =
            dropdown_app(DropdownConfig::new(["Easy", "Normal", "Hard"]).placeholder("Difficulty"));
        assert_eq!(face_text(&mut app, dropdown).as_deref(), Some("Difficulty"));
        let parts = parts(&app, dropdown);
        click(&mut app, parts.button);
        assert!(open(&app, dropdown));
        click(&mut app, parts.rows[2].row);
        assert_eq!(chosen(&app, dropdown), Some(2));
        assert!(!open(&app, dropdown));
        assert_eq!(announced(&mut app), [2]);
        assert_eq!(face_text(&mut app, dropdown).as_deref(), Some("Hard"));

        // Chosen from code: shown, not announced.
        app.world_mut().entity_mut(dropdown).insert(Chosen(Some(0)));
        app.update();
        assert_eq!(face_text(&mut app, dropdown).as_deref(), Some("Easy"));
        assert!(announced(&mut app).is_empty());
    }

    #[test]
    fn an_option_can_be_built_from_anything_and_shows_the_same_in_the_button() {
        #[derive(Component)]
        struct Swatch;
        let swatch = Choice::new("Red", |row| {
            row.spawn((
                Swatch,
                Node::default(),
                BackgroundColor(Color::srgb(1.0, 0.0, 0.0)),
            ));
        });
        let (mut app, dropdown) =
            dropdown_app(DropdownConfig::new([swatch, "Plain".into()]).chosen(0));
        let face = parts(&app, dropdown).face;
        let world = app.world_mut();
        let swatches: Vec<Entity> = world
            .query_filtered::<Entity, With<Swatch>>()
            .iter(world)
            .collect();
        assert_eq!(swatches.len(), 2, "one in the list, one in the button");
        let in_face = swatches
            .iter()
            .filter(|&&swatch| inside(world, swatch, face));
        assert_eq!(in_face.count(), 1);
    }

    #[test]
    fn a_disabled_option_is_skipped_by_the_keys_and_ignored_by_the_pointer() {
        let options = [
            "Easy".into(),
            Choice::text("Normal").disabled(true),
            "Hard".into(),
        ];
        let (mut app, dropdown) = dropdown_app(DropdownConfig::new(options));
        press(&mut app, dropdown, Key::ArrowDown);
        press(&mut app, dropdown, Key::ArrowDown);
        assert_eq!(highlighted(&app, dropdown), Some(0));
        press(&mut app, dropdown, Key::ArrowDown);
        assert_eq!(
            highlighted(&app, dropdown),
            Some(2),
            "past the disabled one"
        );
        let middle = parts(&app, dropdown).rows[1].row;
        assert!(app.world().get::<EntityCursor>(middle).is_none());
        click(&mut app, middle);
        assert!(
            open(&app, dropdown),
            "clicking it neither chooses nor closes"
        );
        assert_eq!(chosen(&app, dropdown), None);
        let state = *app.world().get::<ChoiceState>(middle).unwrap();
        assert!(state.disabled);
    }

    #[test]
    fn a_disabled_dropdown_neither_opens_nor_chooses_and_says_so() {
        let (mut app, dropdown) = dropdown_app(DropdownConfig::new(["A", "B"]).disabled(true));
        let button = parts(&app, dropdown).button;
        click(&mut app, button);
        press(&mut app, dropdown, Key::Enter);
        assert!(!open(&app, dropdown));
        let cursor = |app: &App| app.world().get::<EntityCursor>(button).cloned();
        let not_allowed = EntityCursor::System(SystemCursorIcon::NotAllowed);
        assert_eq!(cursor(&app), Some(not_allowed));

        app.world_mut()
            .entity_mut(dropdown)
            .remove::<InteractionDisabled>();
        app.update();
        assert_eq!(
            cursor(&app),
            Some(EntityCursor::System(SystemCursorIcon::Pointer))
        );
    }

    #[test]
    fn new_tokens_restyle_the_button_its_face_and_its_rows() {
        let options = ["Easy".into(), Choice::text("Hard").disabled(true)];
        let (mut app, dropdown) = dropdown_app(DropdownConfig::new(options).chosen(0));
        let tokens = DropdownTokens::with_prefix("game.difficulty");
        app.world_mut().entity_mut(dropdown).insert(tokens.clone());
        app.update();

        let parts = parts(&app, dropdown);
        let (face, words) = face(&mut app, dropdown).unwrap();
        assert_eq!(words, "Easy", "the same choice");
        let world = app.world();
        let colour = |entity| world.get::<ThemeTextColor>(entity).unwrap().0.clone();
        assert_eq!(colour(face), tokens.text);
        let chevron = world.get::<ThemeBorder>(parts.chevron.unwrap()).unwrap();
        assert_eq!(*chevron, ThemeBorder::all(tokens.chevron.clone()));
        assert_eq!(colour(parts.rows[0].label.unwrap()), tokens.text);
        assert_eq!(
            colour(parts.rows[1].label.unwrap()),
            tokens.text.clone().with_state(WidgetState::Disabled)
        );
        let button = world.get::<ThemeBorder>(parts.button).unwrap();
        assert_eq!(*button, ThemeBorder::all(tokens.border.clone()));
    }
}
