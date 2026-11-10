//! A single-line text field.
//!
//! Wraps Bevy's headless [`TextInput`]. Provides typing, the cursor,
//! selection, clipboard and IME composition.
//!
//! A [`masked`](TextInputConfig::masked) field, for passwords and PINs, shows
//! a mask character for every character typed. The real text is kept beside
//! the editor, never in it, so copying gives only mask characters and no font
//! or theme can reveal it.

use bevy::ecs::entity::EntityHashSet;
use bevy::ecs::query::QueryData;
use bevy::ecs::system::SystemParam;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::input_focus::{FocusLost, FocusedInput, InputFocus};
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::text::{
    EditableText, EditableTextFilter, EditableTextSystems, FontCx, LayoutCx, TextCursorStyle,
    TextEdit, TextEditChange, TextReadWriteMode,
};
use bevy::ui::{InteractionDisabled, UiSystems};
use bevy::ui_widgets::{TextInput, TextInputPlugin, ValueChange};
use bevy::window::SystemCursorIcon;

use crate::EnsurePlugin;
use crate::cursor::{HoverCursor, HoverCursorPlugin};
use crate::sanitize::at_least;
use crate::theme::palette::{
    ACCENT, CONTROL_CORNER_RADIUS, CONTROL_TEXT_SIZE, EDGE, FONT, HIGHLIGHT, SURFACE, TEXT,
    TEXT_MUTED,
};
use crate::theme::skins;
use crate::theme::{
    FocusVisible, Shade, Theme, ThemeAppExt, ThemeBinding, ThemeBorder, ThemeCornerRadius,
    ThemeFont, ThemeFontSize, ThemePlugin, ThemeSkin, ThemeTextColor, ThemeToken, WidgetFlags,
    WidgetState,
};

/// Prefix of the text input widget's theme tokens.
pub const TEXT_INPUT_STYLE: &str = "touchy.text_input";

const BORDER: f32 = 1.0;
const PADDING: Vec2 = Vec2::new(8.0, 5.0);
/// How many characters wide a field is at its natural size, unless its config
/// sets a width.
const DEFAULT_COLUMNS: f32 = 20.0;
/// The narrowest a field can be set, in characters.
const MIN_COLUMNS: f32 = 1.0;
/// What a masked field shows for each character, unless its config picks
/// another. In every font, Bevy's ASCII-only built-in font included.
const DEFAULT_MASK: char = '*';
/// Selected text sits on the accent, faded so the text stays readable.
const SELECTION_SHADE: Shade = Shade::Alpha(0.45);
/// The cursor over the editor: the text cursor.
const CURSOR: HoverCursor = HoverCursor::new(SystemCursorIcon::Text);

/// When a field announces its text as final.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CommitOn {
    /// Pressing Enter, or focus leaving the field with the text changed.
    #[default]
    EnterOrBlur,
    /// Only pressing Enter. Clicking away keeps the text without announcing it.
    Enter,
}

/// Theme tokens for a text input widget. A hovered, focused or disabled field
/// uses these tokens in a [`WidgetState`]: focused, it lights its border.
/// Insert new ones to restyle a field.
#[derive(Component, Clone, PartialEq, Debug)]
#[component(immutable)]
pub struct TextInputTokens {
    /// The field's background.
    pub bg: ThemeToken,
    /// The field's border.
    pub border: ThemeToken,
    /// The typed text.
    pub text: ThemeToken,
    /// The placeholder, shown while the field is empty.
    pub placeholder: ThemeToken,
    /// The text cursor.
    pub caret: ThemeToken,
    /// The selection's highlight.
    pub selection: ThemeToken,
    /// The size of the text and placeholder.
    pub font_size: ThemeToken,
    /// The font of the text and placeholder.
    pub font: ThemeToken,
    /// The field's corner radius.
    pub corner_radius: ThemeToken,
}

impl TextInputTokens {
    /// Names the tokens under `prefix`.
    pub fn with_prefix(prefix: &str) -> Self {
        let token = |suffix: &str| ThemeToken::new(format!("{prefix}.{suffix}"));
        Self {
            bg: token("bg"),
            border: token("border"),
            text: token("text"),
            placeholder: token("placeholder"),
            caret: token("caret"),
            selection: token("selection"),
            font_size: token("font_size"),
            font: token("font"),
            corner_radius: token("corner_radius"),
        }
    }

    /// Derives these tokens from the palette, unless the app sets them.
    pub fn register_theme_defaults(&self, theme: &mut Theme) {
        let lit_border = |state| self.border.clone().with_state(state);
        theme
            .default_derived(self.bg.clone(), SURFACE, Shade::Same)
            .default_derived(self.border.clone(), EDGE, Shade::Same)
            .default_derived(lit_border(WidgetState::Hover), HIGHLIGHT, Shade::Same)
            .default_derived(lit_border(WidgetState::Focused), HIGHLIGHT, Shade::Same)
            .default_derived(self.text.clone(), TEXT, Shade::Same)
            .default_derived(self.placeholder.clone(), TEXT_MUTED, Shade::Same)
            .default_derived(self.caret.clone(), TEXT, Shade::Same)
            .default_derived(self.selection.clone(), ACCENT, SELECTION_SHADE)
            .default_font_from(self.font.clone(), FONT)
            .default_size_from(self.font_size.clone(), CONTROL_TEXT_SIZE)
            .default_size_from(self.corner_radius.clone(), CONTROL_CORNER_RADIUS);
        // Each part draws its role's skin, once a theme gives the role one.
        theme.default_skin_from(self.bg.clone(), skins::SUNKEN);
    }

    /// Makes every token follow the same token in `base`, unless the app sets
    /// it, so a field built on another restyles with it. The number input's
    /// field follows the text input's this way.
    pub fn follow(&self, theme: &mut Theme, base: &TextInputTokens) {
        let lit = |token: &ThemeToken, state| token.clone().with_state(state);
        let colors = [
            (&self.bg, &base.bg),
            (&self.border, &base.border),
            (&self.text, &base.text),
            (&self.placeholder, &base.placeholder),
            (&self.caret, &base.caret),
            (&self.selection, &base.selection),
        ];
        for (token, from) in colors {
            theme.default_derived(token.clone(), from.clone(), Shade::Same);
        }
        theme.default_skin_from(self.bg.clone(), base.bg.clone());
        for state in [WidgetState::Hover, WidgetState::Focused] {
            let (border, from) = (lit(&self.border, state), lit(&base.border, state));
            theme.default_derived(border, from, Shade::Same);
        }
        theme
            .default_font_from(self.font.clone(), base.font.clone())
            .default_size_from(self.font_size.clone(), base.font_size.clone())
            .default_size_from(self.corner_radius.clone(), base.corner_radius.clone());
    }

    /// The frame's background, border and text tokens in this state.
    fn resolve(&self, state: FieldState) -> (ThemeSkin, ThemeBorder, ThemeTextColor) {
        let dimmed = state.disabled.then_some(WidgetState::Disabled);
        let flags = WidgetFlags {
            hovered: state.hovered,
            focused: state.focused,
            pressed: false,
            disabled: state.disabled,
        };
        let border = self.border.clone().with_state(flags.state());
        (
            ThemeSkin(self.bg.clone().with_state(dimmed)),
            ThemeBorder::all(border),
            ThemeTextColor(self.text.clone().with_state(dimmed)),
        )
    }

    /// The editor's text cursor and selection colours.
    fn caret(&self) -> ThemeCaret {
        ThemeCaret {
            caret: self.caret.clone(),
            selection: self.selection.clone(),
        }
    }
}

/// The built-in token set.
impl Default for TextInputTokens {
    fn default() -> Self {
        Self::with_prefix(TEXT_INPUT_STYLE)
    }
}

/// The text cursor's and selection's colours from tokens. Selection is hidden
/// while the field isn't focused.
#[derive(Component, Clone, PartialEq, Debug)]
#[component(immutable)]
#[require(TextCursorStyle)]
pub struct ThemeCaret {
    /// The text cursor's colour.
    pub caret: ThemeToken,
    /// The selection's colour.
    pub selection: ThemeToken,
}

impl ThemeBinding for ThemeCaret {
    type Target = &'static mut TextCursorStyle;

    fn apply(&self, theme: &Theme, mut style: Mut<TextCursorStyle>) {
        style.color = theme.color(&self.caret);
        style.selection_color = theme.color(&self.selection);
        style.unfocused_selection_color = Color::NONE;
    }
}

/// Which characters a field accepts.
type CharFilter = fn(char) -> bool;

/// Configuration for a text field. Consumed by
/// [`SpawnUi::text_input`](crate::spawn_ui::SpawnUi::text_input).
///
/// # Examples
/// ```
/// use touchy::prelude::*;
///
/// let password = TextInputConfig::new("")
///     .placeholder("Password")
///     .masked(true);
/// let pin = TextInputConfig::new("")
///     .filter(|c| c.is_ascii_digit())
///     .max_chars(4)
///     .columns(6.0);
/// ```
#[derive(Clone, Debug)]
pub struct TextInputConfig {
    /// The text the field starts with.
    pub text: String,
    /// Shown, faded, while the field is empty.
    pub placeholder: Option<String>,
    /// When the field announces its text as final. Defaults to [`CommitOn::EnterOrBlur`].
    pub commit_on: CommitOn,
    /// Also announce every edit as it happens, with `is_final: false`.
    pub live: bool,
    /// Typing and pasting skip characters this rejects.
    pub filter: Option<CharFilter>,
    /// Show this character in place of every character (e.g. for passwords and PINs).
    pub mask: Option<char>,
    /// The most characters the field holds. Starting text, or text set from
    /// code, that's longer keeps its first characters.
    pub max_chars: Option<usize>,
    /// The field's width, in characters of its font. Without one, the field
    /// takes whatever width its parent gives it.
    pub columns: Option<f32>,
    /// Where the text, and the placeholder, sit across the field. Defaults to the left.
    pub justify: Justify,
    /// Disabled text input cannot be focused or edited.
    pub disabled: bool,
    /// Theme tokens used to style the widget.
    pub tokens: TextInputTokens,
}

impl Default for TextInputConfig {
    fn default() -> Self {
        Self {
            text: String::new(),
            placeholder: None,
            commit_on: CommitOn::default(),
            live: false,
            filter: None,
            max_chars: None,
            mask: None,
            columns: None,
            justify: Justify::default(),
            disabled: false,
            tokens: TextInputTokens::with_prefix(TEXT_INPUT_STYLE),
        }
    }
}

impl TextInputConfig {
    /// A field starting with `text` in it.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            ..default()
        }
    }

    /// Shows `placeholder`, faded, while the field is empty.
    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = Some(placeholder.into());
        self
    }

    /// Sets when the field announces its text as final.
    pub fn commit_on(mut self, commit_on: CommitOn) -> Self {
        self.commit_on = commit_on;
        self
    }

    /// Sets whether the field also announces every edit as it happens.
    pub fn live(mut self, live: bool) -> Self {
        self.live = live;
        self
    }

    /// Accepts only characters `filter` approves, as in `|c| c.is_ascii_digit()`.
    pub fn filter(mut self, filter: CharFilter) -> Self {
        self.filter = Some(filter);
        self
    }

    /// Sets the most characters the field holds.
    pub fn max_chars(mut self, max_chars: usize) -> Self {
        self.max_chars = Some(max_chars);
        self
    }

    /// Hides what is typed behind `*` or character set with [`mask_char`](Self::mask_char).
    pub fn masked(mut self, masked: bool) -> Self {
        self.mask = masked.then_some(self.mask.unwrap_or(DEFAULT_MASK));
        self
    }

    /// Show this character in place of every character (e.g. for passwords and PINs).
    pub fn mask_char(mut self, mask: char) -> Self {
        self.mask = Some(mask);
        self
    }

    /// Fixes the field's width at `columns` characters, so it no longer
    /// stretches to fill its parent.
    pub fn columns(mut self, columns: f32) -> Self {
        self.columns = Some(columns);
        self
    }

    /// Sets where the text, and the placeholder, sit across the field: `Justify::Center`, say.
    pub fn justify(mut self, justify: Justify) -> Self {
        self.justify = justify;
        self
    }

    /// Sets whether the field is disabled.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Sets the field's theme tokens.
    pub fn tokens(mut self, tokens: TextInputTokens) -> Self {
        self.tokens = tokens;
        self
    }

    /// This config with every value made usable, warning about each repair.
    fn sanitized(mut self) -> Self {
        self.columns = at_least(self.columns, MIN_COLUMNS, "TextInputConfig columns");
        // A field that holds nothing can't be typed in.
        self.max_chars = at_least(self.max_chars, 1, "TextInputConfig max_chars");
        self
    }
}

/// How a field commits, which the app may change at any time.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextInputSettings {
    /// When the field announces its text as final.
    pub commit_on: CommitOn,
    /// Also announce every edit as it happens, with `is_final: false`.
    pub live: bool,
}

/// The parts of a text field, on the field. The field itself is the frame.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextInputParts {
    /// The entity that does the editing: it holds Bevy's [`TextInput`] and
    /// [`EditableText`], and takes the focus.
    pub editor: Entity,
    /// The placeholder, if it has one.
    pub placeholder: Option<Entity>,
}

/// The field's committed text: the last text it announced as final. Insert
/// a new value to replace the text from code; it isn't announced.
#[derive(Component, Clone, Debug, PartialEq, Eq, Default)]
pub struct TextInputValue(pub String);

/// A masked field's real text, kept in step with every edit to the mask
/// characters the editor shows instead. Allows editing and caret navigation.
/// Prevents copying the real text, or a fallback font/theme revealing it.
#[derive(Component, Clone, Debug)]
struct Masked {
    text: String,
    mask: char,
    /// The selection, as a character range, before this frame's edits.
    before: std::ops::Range<usize>,
    /// Whether this frame's edits change the text, rather than only the cursor.
    edited: bool,
}

/// The text last announced live, so cursor moves aren't announced as edits.
#[derive(Component, Clone, Debug, Default)]
struct LiveText(String);

/// What a key does to a focused field. The one place its keys are read, for
/// the key mapping to take over; Bevy's widget reads the editing keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TextInputKey {
    /// Announces the text as final.
    Commit,
    /// Puts back the committed text.
    Cancel,
}

impl TextInputKey {
    fn of(key: &Key) -> Option<Self> {
        match key {
            Key::Enter => Some(Self::Commit),
            Key::Escape => Some(Self::Cancel),
            _ => None,
        }
    }
}

/// Spawns it; documented on [`SpawnUi::text_input`](crate::spawn_ui::SpawnUi::text_input).
pub(crate) fn spawn_text_input<'a>(
    parent: &'a mut ChildSpawnerCommands,
    config: TextInputConfig,
) -> EntityCommands<'a> {
    spawn_text_parts(parent, config).0
}

/// Spawns a text field, returning it (and the entity inside it that does the editing) for widgets
/// built on a text field. Allows a widget to observe the editor directly.
pub(crate) fn spawn_text_parts<'a>(
    parent: &'a mut ChildSpawnerCommands,
    config: TextInputConfig,
) -> (EntityCommands<'a>, Entity) {
    let TextInputConfig {
        text,
        placeholder,
        commit_on,
        live,
        filter,
        mask,
        max_chars,
        justify,
        columns,
        disabled,
        tokens,
    } = config.sanitized();
    let padding = UiRect::axes(px(PADDING.x), px(PADDING.y));
    let layout = TextLayout {
        justify,
        ..default()
    };
    let text = fit(&text, max_chars);
    let mut editable = EditableText::new(shown(&text, mask));
    editable.visible_width = Some(columns.unwrap_or(DEFAULT_COLUMNS));
    editable.max_characters = max_chars;
    // A set width holds even where the parent stretches its children.
    let align_self = match columns {
        Some(_) => AlignSelf::FlexStart,
        None => AlignSelf::Auto,
    };

    let mut field = parent.spawn((
        Hovered::default(),
        Node {
            border: UiRect::all(px(BORDER)),
            align_items: AlignItems::Center,
            align_self,
            ..default()
        },
        TextInputSettings { commit_on, live },
        TextInputValue(text.clone()),
        LiveText(text.clone()),
    ));

    let mut parts = TextInputParts {
        editor: Entity::PLACEHOLDER,
        placeholder: None,
    };
    field.with_children(|frame| {
        let mut editor = frame.spawn((
            TextInput,
            editable,
            layout,
            CURSOR,
            // Tab reaches it in layout order; Bevy's widget handles the keys once focused.
            TabIndex(0),
            Node {
                flex_grow: 1.0,
                padding,
                ..default()
            },
        ));
        if let Some(mask) = mask {
            editor.insert(Masked {
                text: text.clone(),
                mask,
                before: 0..0,
                edited: false,
            });
        }
        if let Some(filter) = filter {
            editor.insert(EditableTextFilter::new(filter));
        }
        editor
            .observe(commit_or_cancel_on_keys)
            .observe(commit_on_blur)
            .observe(follow_edits);
        parts.editor = editor.id();

        parts.placeholder = placeholder.map(|hint| {
            frame
                .spawn((
                    Pickable::IGNORE,
                    Text::new(hint),
                    // Laid over the editor, its text where the editor's is.
                    layout,
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(0),
                        top: px(0),
                        width: percent(100),
                        padding,
                        ..default()
                    },
                    placeholder_visibility(&text),
                ))
                .id()
        });
    });
    // The parts go in first, for disabling to reach the editor, and the tokens
    // last: the look comes with them, in `style_text_input`, once the state is in.
    field.insert(parts);
    if disabled {
        field.insert(InteractionDisabled);
    }
    field.insert(tokens);
    (field, parts.editor)
}

/// `text` cut to its first `max` characters. Bevy's limit stops typing past
/// it, but not text that's put in whole.
fn fit(text: &str, max: Option<usize>) -> String {
    match max {
        Some(max) => text.chars().take(max).collect(),
        None => text.to_owned(),
    }
}

/// What the editor shows for `text`: the text itself, or a mask character per character.
fn shown(text: &str, mask: Option<char>) -> String {
    match mask {
        Some(mask) => mask.to_string().repeat(text.chars().count()),
        None => text.to_owned(),
    }
}

/// The field's real text: the editor's, or the one behind a masked field's mask.
fn content(editable: &EditableText, masked: Option<&Masked>) -> String {
    match masked {
        Some(masked) => masked.text.clone(),
        None => editable.value().to_string(),
    }
}

/// Puts `text` in the editor, masked if the field is, with the cursor at the end.
fn replace_text(editable: &mut EditableText, masked: Option<Mut<Masked>>, text: &str) {
    let mask = masked.as_ref().map(|masked| masked.mask);
    if let Some(mut masked) = masked {
        masked.text = text.to_owned();
        masked.edited = false;
    }
    editable.editor_mut().set_text(&shown(text, mask));
    editable.queue_edit(TextEdit::TextEnd(false));
}

/// The editor's selection as a range of characters. Allows editing of masked fields.
fn selection_chars(editable: &EditableText) -> std::ops::Range<usize> {
    let text = editable.editor().raw_text();
    let selection = editable.editor().raw_selection();
    let chars = |byte: usize| text[..byte.min(text.len())].chars().count();
    let (anchor, focus) = (
        chars(selection.anchor().index()),
        chars(selection.focus().index()),
    );
    anchor.min(focus)..anchor.max(focus)
}

/// Works out what a frame's edits did to a masked field's real text.
///
/// Every edit leaves the cursor just after whatever it inserted (0 for deletions).
/// So if the text after the cursor is as long as the text that was after the selection,
/// the selection was replaced by what now sits between its start and the cursor: typing,
/// pasting, or cutting. If not, characters were deleted at the cursor. `before` is the
/// selection before the edits, and `cursor` the cursor after, both in characters.
///
/// Returns the new real text, and the character range of the editor that
/// holds newly typed characters still to be masked.
fn reconcile(
    secret: &str,
    display: &str,
    before: std::ops::Range<usize>,
    cursor: usize,
) -> Option<(String, std::ops::Range<usize>)> {
    let secret: Vec<char> = secret.chars().collect();
    let display: Vec<char> = display.chars().collect();
    let after_cursor = display.len().checked_sub(cursor)?;
    let replaced_selection =
        cursor >= before.start && secret.len().checked_sub(before.end) == Some(after_cursor);
    let start = if replaced_selection {
        before.start
    } else {
        cursor
    };
    if start + after_cursor > secret.len() {
        return None;
    }
    let mut text: String = secret[..start].iter().collect();
    text.extend(&display[start..cursor]);
    text.extend(&secret[secret.len() - after_cursor..]);
    Some((text, start..cursor))
}

/// A placeholder (if there is one) is only visible when the box is empty.
fn placeholder_visibility(text: &str) -> Visibility {
    if text.is_empty() {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    }
}

/// Announces `text` as the field's final value and makes it the committed one.
fn commit(commands: &mut Commands, field: Entity, text: String) {
    commands.entity(field).insert(TextInputValue(text.clone()));
    commands.trigger(ValueChange {
        source: field,
        value: text,
        is_final: true,
    });
}

/// Enter commits, even unchanged, since pressing it is a deliberate act.
/// Escape puts back the committed text; Bevy's widget then drops focus, and
/// with the text unchanged the blur that follows has nothing to commit.
fn commit_or_cancel_on_keys(
    keys: On<FocusedInput<KeyboardInput>>,
    mut editors: Query<(&mut EditableText, Option<&mut Masked>, &ChildOf)>,
    fields: Query<&TextInputValue>,
    mut commands: Commands,
) {
    if !keys.input.state.is_pressed() {
        return;
    }
    let Some(key) = TextInputKey::of(&keys.input.logical_key) else {
        return;
    };
    let Ok((mut editable, masked, child_of)) = editors.get_mut(keys.focused_entity) else {
        return;
    };
    // Enter during IME composition confirms the composition, not the field.
    if editable.is_composing() {
        return;
    }
    let field = child_of.parent();
    let text = content(&editable, masked.as_deref());
    match key {
        TextInputKey::Commit => commit(&mut commands, field, text),
        TextInputKey::Cancel => {
            if let Ok(TextInputValue(committed)) = fields.get(field)
                && text != *committed
            {
                replace_text(&mut editable, masked, committed);
            }
        }
    }
}

/// Clicking away, or tabbing out, commits changed text.
fn commit_on_blur(
    lost: On<FocusLost>,
    editors: Query<(&EditableText, Option<&Masked>, &ChildOf)>,
    fields: Query<(&TextInputSettings, &TextInputValue)>,
    mut commands: Commands,
) {
    let Ok((editable, masked, child_of)) = editors.get(lost.entity) else {
        return;
    };
    let field = child_of.parent();
    let Ok((settings, TextInputValue(committed))) = fields.get(field) else {
        return;
    };
    let text = content(editable, masked);
    if settings.commit_on == CommitOn::EnterOrBlur && text != *committed {
        commit(&mut commands, field, text);
    }
}

/// After any edit: keep a masked field's real text in step, show the
/// placeholder only while empty, and announce the text if the field is live.
/// Cursor moves count as edits to Bevy, so the text is compared with what was
/// last announced.
fn follow_edits(
    edit: On<TextEditChange>,
    mut editors: Query<(&mut EditableText, Option<&mut Masked>, &ChildOf)>,
    mut fields: Query<(&TextInputParts, &TextInputSettings, &mut LiveText)>,
    mut placeholders: Query<&mut Visibility>,
    mut font_cx: ResMut<FontCx>,
    mut layout_cx: ResMut<LayoutCx>,
    mut commands: Commands,
) {
    let Ok((mut editable, masked, child_of)) = editors.get_mut(edit.event_target()) else {
        return;
    };
    let field = child_of.parent();
    let Ok((parts, settings, mut live_text)) = fields.get_mut(field) else {
        return;
    };
    // Characters being composed show until the IME commits them, then get masked.
    let text = match masked {
        Some(mut masked) if masked.edited && !editable.is_composing() => {
            masked.edited = false;
            let display = editable.value().to_string();
            let cursor = selection_chars(&editable).end;
            let before = masked.before.clone();
            if let Some((text, typed)) = reconcile(&masked.text, &display, before, cursor) {
                masked.text = text;
                if !typed.is_empty() {
                    let bytes = |char: usize| {
                        display
                            .char_indices()
                            .nth(char)
                            .map_or(display.len(), |(byte, _)| byte)
                    };
                    let cover = masked.mask.to_string().repeat(typed.len());
                    let mut driver = editable.editor.driver(&mut font_cx, &mut layout_cx.0);
                    driver.select_byte_range(bytes(typed.start), bytes(typed.end));
                    driver.insert_or_replace_selection(&cover);
                }
            }
            masked.text.clone()
        }
        masked => content(&editable, masked.as_deref()),
    };
    if let Some(placeholder) = parts.placeholder
        && let Ok(mut visibility) = placeholders.get_mut(placeholder)
    {
        visibility.set_if_neq(placeholder_visibility(&text));
    }
    if settings.live && live_text.0 != text {
        live_text.0 = text.clone();
        commands.trigger(ValueChange {
            source: field,
            value: text,
            is_final: false,
        });
    }
}

/// Notes, before Bevy applies them, whether a masked field's queued edits
/// change its text, and where its selection was, so the result can be
/// worked out afterwards. Edits queued over several frames, as while an IME
/// composes, keep the selection from before the first.
fn note_masked_edits(mut editors: Query<(&EditableText, &mut Masked)>) {
    for (editable, mut masked) in &mut editors {
        if !editable.pending_edits.iter().any(TextEdit::is_destructive) {
            continue;
        }
        if !masked.edited {
            masked.before = selection_chars(editable);
            masked.edited = true;
        }
    }
}

/// Puts a [`TextInputValue`] the app inserted into the editor, cut to fit
/// the field. Values the field committed itself already match, so they're
/// left alone.
fn apply_values_from_code(
    mut fields: Query<(&TextInputParts, &mut TextInputValue), Changed<TextInputValue>>,
    mut editors: Query<(&mut EditableText, Option<&mut Masked>)>,
) {
    for (parts, mut value) in &mut fields {
        let Ok((mut editable, masked)) = editors.get_mut(parts.editor) else {
            continue;
        };
        let text = fit(&value.0, editable.max_characters);
        if content(&editable, masked.as_deref()) != text {
            replace_text(&mut editable, masked, &text);
        }
        // The committed value is what the field shows.
        value.set_if_neq(TextInputValue(text));
    }
}

/// Disabling the field disables its editor, so typing, focus and Tab stop too.
fn disable_editor(
    insert: On<Insert<InteractionDisabled>>,
    fields: Query<&TextInputParts>,
    focus: Option<ResMut<InputFocus>>,
    mut commands: Commands,
) {
    let Ok(parts) = fields.get(insert.entity) else {
        return;
    };
    if let Some(mut focus) = focus
        && focus.get() == Some(parts.editor)
    {
        focus.clear();
    }
    commands
        .entity(parts.editor)
        .insert(InteractionDisabled)
        .remove::<TabIndex>();
}

/// Enabling the field enables its editor. Tab reaches it again, unless it's
/// only for display.
fn enable_editor(
    remove: On<Remove<InteractionDisabled>>,
    fields: Query<&TextInputParts>,
    modes: Query<&TextReadWriteMode>,
    mut commands: Commands,
) {
    let Ok(parts) = fields.get(remove.entity) else {
        return;
    };
    let mut editor = commands.entity(parts.editor);
    editor.remove::<InteractionDisabled>();
    if modes.get(parts.editor) != Ok(&TextReadWriteMode::Static) {
        editor.insert(TabIndex(0));
    }
}

/// What a field is doing, which picks its look.
#[derive(Clone, Copy, Debug)]
struct FieldState {
    hovered: bool,
    focused: bool,
    disabled: bool,
}

/// A field's state, as its components hold it. Its editor's focus is in
/// [`InputFocus`]; a widget built around the field, such as a number input,
/// shows its own focus here by putting [`FocusVisible`] on the field.
#[derive(QueryData)]
struct FieldStatus {
    parts: &'static TextInputParts,
    hovered: &'static Hovered,
    lent_focus: Has<FocusVisible>,
    disabled: Has<InteractionDisabled>,
}

impl FieldStatusItem<'_, '_> {
    /// A field shows its focus however its editor got it, a click included,
    /// since it's typed into.
    fn state(&self, focus: Option<Entity>) -> FieldState {
        FieldState {
            hovered: self.hovered.0,
            focused: focus == Some(self.parts.editor) || self.lent_focus,
            disabled: self.disabled,
        }
    }
}

/// State changes that can alter a field's look. Removals are read separately,
/// because change detection doesn't see them, its editor's focus from
/// [`InputFocus`], and new tokens in `style_text_input`.
type LookChanged = (
    With<TextInputParts>,
    Or<(
        Changed<Hovered>,
        Added<FocusVisible>,
        Added<InteractionDisabled>,
    )>,
);

/// Where the focus is, and which fields it just left and reached. Focus lives
/// in a resource rather than on a field, so this is how a field hears of it.
#[derive(SystemParam)]
struct FocusMoves<'w, 's> {
    focus: Res<'w, InputFocus>,
    last: Local<'s, Option<Entity>>,
    editors: Query<'w, 's, &'static ChildOf, With<TextInput>>,
}

impl FocusMoves<'_, '_> {
    /// The fields the focus left or reached since this last looked.
    fn moved(&mut self) -> Vec<Entity> {
        if !self.focus.is_changed() {
            return Vec::new();
        }
        let now = self.focus.get();
        let left = std::mem::replace(&mut *self.last, now);
        left.into_iter()
            .chain(now)
            .filter_map(|editor| self.editors.get(editor).ok().map(ChildOf::parent))
            .collect()
    }
}

/// Re-picks the frame's and text's tokens for fields whose hover, focus or
/// disabled state changed.
fn restyle_text_fields(
    changed: Query<Entity, LookChanged>,
    // States that ended, which change detection doesn't see.
    (mut enabled, mut unlent): (
        RemovedComponents<InteractionDisabled>,
        RemovedComponents<FocusVisible>,
    ),
    mut focus: FocusMoves,
    fields: Query<(&TextInputTokens, FieldStatus)>,
    frames: Query<(&ThemeSkin, &ThemeBorder)>,
    texts: Query<&ThemeTextColor>,
    mut commands: Commands,
) {
    let mut dirty: EntityHashSet = changed
        .iter()
        .chain(enabled.read())
        .chain(unlent.read())
        .collect();
    dirty.extend(focus.moved());
    for entity in dirty {
        let Ok((tokens, status)) = fields.get(entity) else {
            continue;
        };
        let (bg, border, text_color) = tokens.resolve(status.state(focus.focus.get()));
        if let Ok((old_bg, old_border)) = frames.get(entity) {
            if *old_bg != bg {
                commands.entity(entity).insert(bg);
            }
            if *old_border != border {
                commands.entity(entity).insert(border);
            }
        }
        let editor = status.parts.editor;
        if texts.get(editor).is_ok_and(|old| *old != text_color) {
            commands.entity(editor).insert(text_color);
        }
    }
}

/// Styles a text input when its tokens are set: at spawn, and whenever the app sets new ones.
fn style_text_input(
    insert: On<Insert<TextInputTokens>>,
    fields: Query<(&TextInputTokens, FieldStatus)>,
    focus: Option<Res<InputFocus>>,
    mut commands: Commands,
) {
    let Ok((tokens, status)) = fields.get(insert.entity) else {
        return;
    };
    let focused = focus.and_then(|focus| focus.get());
    let (bg, border, text_color) = tokens.resolve(status.state(focused));
    let font = ThemeFont(tokens.font.clone());
    let font_size = ThemeFontSize(tokens.font_size.clone());
    commands.entity(insert.entity).insert((
        bg,
        border,
        ThemeCornerRadius(tokens.corner_radius.clone()),
    ));
    commands.entity(status.parts.editor).insert((
        text_color,
        font.clone(),
        font_size.clone(),
        tokens.caret(),
    ));
    if let Some(placeholder) = status.parts.placeholder {
        commands.entity(placeholder).insert((
            ThemeTextColor(tokens.placeholder.clone()),
            font,
            font_size,
        ));
    }
}

/// Text field styling, commit behaviour and default token values.
pub struct TextInputWidgetPlugin;

impl Plugin for TextInputWidgetPlugin {
    fn build(&self, app: &mut App) {
        app.ensure_plugin(HoverCursorPlugin)
            .ensure_plugin(ThemePlugin)
            .ensure_plugin(TextInputPlugin)
            .add_observer(style_text_input)
            .add_observer(disable_editor)
            .add_observer(enable_editor)
            .register_theme_binding::<ThemeCaret>()
            .add_systems(
                PostUpdate,
                (
                    note_masked_edits.before(EditableTextSystems),
                    (apply_values_from_code, restyle_text_fields).before(UiSystems::Prepare),
                ),
            );
        TextInputTokens::with_prefix(TEXT_INPUT_STYLE)
            .register_theme_defaults(&mut app.world_mut().resource_mut::<Theme>());
    }
}

#[cfg(test)]
mod tests {
    use bevy::input::ButtonState;
    use bevy::input_focus::{FocusCause, InputFocusPlugin};
    use bevy::picking::cursor::EntityCursor;

    use super::*;

    /// Every `ValueChange<String>` the fields announced, in order.
    #[derive(Resource, Default)]
    struct Announced(Vec<(String, bool)>);

    fn app_with_field(config: TextInputConfig) -> (App, Entity, Entity) {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::asset::AssetPlugin::default(),
            bevy::input::InputPlugin,
            // IME and focus messages; no window is needed.
            bevy::window::WindowPlugin {
                primary_window: None,
                exit_condition: bevy::window::ExitCondition::DontExit,
                ..default()
            },
            InputFocusPlugin,
            // Bevy's text input also follows pointers and the UI scale.
            bevy::picking::DefaultPickingPlugins,
            bevy::text::TextPlugin,
            TextInputWidgetPlugin,
        ))
        .init_resource::<UiScale>()
        .init_resource::<Announced>()
        .add_observer(
            |change: On<ValueChange<String>>, mut announced: ResMut<Announced>| {
                announced.0.push((change.value.clone(), change.is_final));
            },
        );
        let mut field = None;
        app.world_mut()
            .commands()
            .spawn(Node::default())
            .with_children(|root| field = Some(spawn_text_input(root, config).id()));
        app.update();
        let field = field.unwrap();
        let editor = parts(&app, field).editor;
        (app, field, editor)
    }

    fn parts(app: &App, field: Entity) -> TextInputParts {
        *app.world().get::<TextInputParts>(field).unwrap()
    }

    fn type_text(app: &mut App, editor: Entity, text: &str) {
        let mut editable = app.world_mut().get_mut::<EditableText>(editor).unwrap();
        editable.editor_mut().set_text(text);
        app.update();
    }

    fn press(app: &mut App, editor: Entity, key: Key) {
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
            .trigger(FocusedInput::new(editor, keyboard, Entity::PLACEHOLDER));
        app.update();
    }

    fn focus(app: &mut App, editor: Option<Entity>) {
        let mut focus = app.world_mut().resource_mut::<InputFocus>();
        match editor {
            Some(editor) => focus.set(editor, FocusCause::Pressed),
            None => focus.clear(),
        }
        app.update();
    }

    fn announced(app: &App) -> Vec<(String, bool)> {
        app.world().resource::<Announced>().0.clone()
    }

    fn committed(app: &App, field: Entity) -> String {
        app.world().get::<TextInputValue>(field).unwrap().0.clone()
    }

    fn shown(app: &App, editor: Entity) -> String {
        let editable = app.world().get::<EditableText>(editor).unwrap();
        editable.value().to_string()
    }

    #[test]
    fn enter_commits_even_when_unchanged() {
        let (mut app, field, editor) = app_with_field(TextInputConfig::new("Ada"));
        press(&mut app, editor, Key::Enter);
        type_text(&mut app, editor, "Grace");
        press(&mut app, editor, Key::Enter);
        assert_eq!(
            announced(&app),
            [("Ada".into(), true), ("Grace".into(), true)]
        );
        assert_eq!(committed(&app, field), "Grace");
    }

    #[test]
    fn clicking_away_commits_only_changed_text() {
        let (mut app, field, editor) = app_with_field(TextInputConfig::new("Ada"));
        focus(&mut app, Some(editor));
        focus(&mut app, None);
        assert!(
            announced(&app).is_empty(),
            "nothing changed, nothing to say"
        );

        focus(&mut app, Some(editor));
        type_text(&mut app, editor, "Grace");
        focus(&mut app, None);
        assert_eq!(announced(&app), [("Grace".into(), true)]);
        assert_eq!(committed(&app, field), "Grace");
    }

    #[test]
    fn enter_only_fields_keep_text_on_blur_without_announcing() {
        let config = TextInputConfig::new("").commit_on(CommitOn::Enter);
        let (mut app, field, editor) = app_with_field(config);
        focus(&mut app, Some(editor));
        type_text(&mut app, editor, "gg");
        focus(&mut app, None);
        assert!(announced(&app).is_empty());
        assert_eq!(committed(&app, field), "");
        assert_eq!(shown(&app, editor), "gg", "the draft stays");
    }

    /// The app can change how a field commits after spawning it.
    #[test]
    fn settings_changed_at_runtime_take_effect() {
        let (mut app, field, editor) = app_with_field(TextInputConfig::new(""));
        app.world_mut().entity_mut(field).insert(TextInputSettings {
            commit_on: CommitOn::Enter,
            live: true,
        });
        focus(&mut app, Some(editor));
        type_text(&mut app, editor, "gg");
        focus(&mut app, None);
        assert_eq!(
            announced(&app),
            [("gg".into(), false)],
            "live, no blur commit"
        );
    }

    #[test]
    fn escape_puts_back_the_committed_text_and_says_nothing() {
        let (mut app, field, editor) = app_with_field(TextInputConfig::new("Ada"));
        focus(&mut app, Some(editor));
        type_text(&mut app, editor, "Gr");
        press(&mut app, editor, Key::Escape);
        // Bevy's widget drops focus on Escape.
        app.update();
        assert!(app.world().resource::<InputFocus>().get().is_none());
        assert!(announced(&app).is_empty());
        assert_eq!(committed(&app, field), "Ada");
        assert_eq!(shown(&app, editor), "Ada");

        // Nothing lingers from the cancel: the next blur commits as usual.
        focus(&mut app, Some(editor));
        type_text(&mut app, editor, "Grace");
        focus(&mut app, None);
        assert_eq!(announced(&app), [("Grace".into(), true)]);
    }

    #[test]
    fn live_fields_announce_each_edit_but_not_cursor_moves() {
        let (mut app, _, editor) = app_with_field(TextInputConfig::new("").live(true));
        type_text(&mut app, editor, "a");
        type_text(&mut app, editor, "ab");
        app.world_mut()
            .get_mut::<EditableText>(editor)
            .unwrap()
            .queue_edit(TextEdit::Left(false));
        app.update();
        press(&mut app, editor, Key::Enter);
        assert_eq!(
            announced(&app),
            [
                ("a".into(), false),
                ("ab".into(), false),
                ("ab".into(), true)
            ]
        );
    }

    #[test]
    fn the_placeholder_shows_only_while_empty() {
        let (mut app, field, editor) =
            app_with_field(TextInputConfig::new("").placeholder("Player name"));
        let placeholder = parts(&app, field).placeholder.unwrap();
        let visible =
            |app: &App| app.world().get::<Visibility>(placeholder) != Some(&Visibility::Hidden);
        assert!(visible(&app));
        type_text(&mut app, editor, "Ada");
        assert!(!visible(&app));
        type_text(&mut app, editor, "");
        assert!(visible(&app));
    }

    #[test]
    fn values_set_in_code_replace_the_text_silently() {
        let (mut app, field, editor) = app_with_field(TextInputConfig::new("Ada"));
        app.world_mut()
            .entity_mut(field)
            .insert(TextInputValue("Grace".into()));
        app.update();
        app.update();
        assert_eq!(shown(&app, editor), "Grace");
        assert!(announced(&app).is_empty());
    }

    #[test]
    fn focus_highlights_the_border_and_disabling_stops_editing() {
        let (mut app, field, editor) = app_with_field(TextInputConfig::new(""));
        let tokens = TextInputTokens::with_prefix(TEXT_INPUT_STYLE);
        let border = |app: &App| app.world().get::<ThemeBorder>(field).unwrap().top.clone();
        let cursor = |app: &App| app.world().get::<EntityCursor>(editor).cloned();
        assert_eq!(border(&app), tokens.border);
        assert_eq!(
            cursor(&app),
            Some(EntityCursor::System(SystemCursorIcon::Text))
        );

        focus(&mut app, Some(editor));
        assert_eq!(
            border(&app),
            tokens.border.clone().with_state(WidgetState::Focused)
        );

        app.world_mut()
            .entity_mut(field)
            .insert(InteractionDisabled);
        app.update();
        app.update();
        let world = app.world();
        assert!(
            world.resource::<InputFocus>().get().is_none(),
            "focus leaves"
        );
        assert!(world.get::<InteractionDisabled>(editor).is_some());
        assert!(world.get::<TabIndex>(editor).is_none(), "Tab skips it");
        assert_eq!(
            border(&app),
            tokens.border.clone().with_state(WidgetState::Disabled)
        );
        assert_eq!(
            cursor(&app),
            Some(EntityCursor::System(SystemCursorIcon::NotAllowed))
        );

        app.world_mut()
            .entity_mut(field)
            .remove::<InteractionDisabled>();
        app.update();
        assert!(app.world().get::<TabIndex>(editor).is_some(), "Tab is back");
        assert_eq!(border(&app), tokens.border);
    }

    /// A field only for display, like the number input's shown value, stays
    /// out of the Tab order when it's enabled again.
    #[test]
    fn re_enabling_a_display_field_keeps_it_out_of_tab() {
        let (mut app, field, editor) = app_with_field(TextInputConfig::new("12"));
        app.world_mut()
            .entity_mut(editor)
            .insert(TextReadWriteMode::Static)
            .remove::<TabIndex>();
        app.world_mut()
            .entity_mut(field)
            .insert(InteractionDisabled);
        app.update();
        app.world_mut()
            .entity_mut(field)
            .remove::<InteractionDisabled>();
        app.update();
        assert!(app.world().get::<TabIndex>(editor).is_none());
    }

    #[test]
    fn a_field_spawned_disabled_starts_out_of_reach() {
        let (app, field, editor) = app_with_field(TextInputConfig::new("").disabled(true));
        let world = app.world();
        assert!(world.get::<InteractionDisabled>(editor).is_some());
        assert!(world.get::<TabIndex>(editor).is_none());
        let tokens = TextInputTokens::with_prefix(TEXT_INPUT_STYLE);
        assert_eq!(
            world.get::<ThemeSkin>(field),
            Some(&ThemeSkin(tokens.bg.with_state(WidgetState::Disabled)))
        );
    }

    #[test]
    fn new_tokens_restyle_every_part() {
        let config = TextInputConfig::new("").placeholder("Search");
        let (mut app, field, editor) = app_with_field(config);
        let tokens = TextInputTokens::with_prefix("test.field");
        app.world_mut().entity_mut(field).insert(tokens.clone());
        app.update();
        let placeholder = parts(&app, field).placeholder.unwrap();
        let world = app.world();
        assert_eq!(
            world.get::<ThemeSkin>(field),
            Some(&ThemeSkin(tokens.bg.clone()))
        );
        assert_eq!(
            world.get::<ThemeTextColor>(editor),
            Some(&ThemeTextColor(tokens.text.clone()))
        );
        assert_eq!(world.get::<ThemeCaret>(editor), Some(&tokens.caret()));
        assert_eq!(
            world.get::<ThemeTextColor>(placeholder),
            Some(&ThemeTextColor(tokens.placeholder.clone()))
        );
    }

    /// A centred field centres its text and its placeholder alike.
    #[test]
    fn a_centred_field_centres_its_text_and_placeholder() {
        let config = TextInputConfig::new("")
            .placeholder("Name")
            .justify(Justify::Center);
        let (app, field, editor) = app_with_field(config);
        let justify = |entity| app.world().get::<TextLayout>(entity).unwrap().justify;
        assert_eq!(justify(editor), Justify::Center);
        let placeholder = app
            .world()
            .get::<TextInputParts>(field)
            .unwrap()
            .placeholder
            .unwrap();
        assert_eq!(justify(placeholder), Justify::Center);
        let spans = app.world().get::<Node>(placeholder).unwrap().width;
        assert_eq!(spans, percent(100), "across the field, to centre in it");
    }

    #[test]
    fn text_longer_than_the_limit_keeps_its_first_characters() {
        let config = TextInputConfig::new("Gretzky99").max_chars(7);
        let (mut app, field, editor) = app_with_field(config);
        assert_eq!(shown(&app, editor), "Gretzky");
        assert_eq!(committed(&app, field), "Gretzky");

        app.world_mut()
            .entity_mut(field)
            .insert(TextInputValue("Lemieux66".into()));
        app.update();
        assert_eq!(shown(&app, editor), "Lemieux");
        assert_eq!(committed(&app, field), "Lemieux");
    }

    #[test]
    fn a_set_width_holds_where_the_parent_stretches() {
        let align = |config| {
            let (app, field, _) = app_with_field(config);
            app.world().get::<Node>(field).unwrap().align_self
        };
        assert_eq!(align(TextInputConfig::new("")), AlignSelf::Auto);
        assert_eq!(
            align(TextInputConfig::new("").columns(10.0)),
            AlignSelf::FlexStart
        );
    }

    #[test]
    fn spawning_repairs_the_config() {
        let config = TextInputConfig::new("abc").max_chars(0).columns(-3.0);
        let (app, _, editor) = app_with_field(config);
        let editable = app.world().get::<EditableText>(editor).unwrap();
        assert_eq!(editable.max_characters, Some(1));
        assert_eq!(editable.visible_width, Some(MIN_COLUMNS));
        assert_eq!(shown(&app, editor), "a");
    }

    /// `n` mask characters.
    fn dots(n: usize) -> String {
        DEFAULT_MASK.to_string().repeat(n)
    }

    #[test]
    fn reconciling_typing_pasting_replacing_and_cutting() {
        // Typed at the end and in the middle: the cursor sits after the new character.
        assert_eq!(reconcile("ab", "**c", 2..2, 3), Some(("abc".into(), 2..3)));
        assert_eq!(reconcile("ac", "*b*", 1..1, 2).unwrap().0, "abc");
        // A typed mask character is still a character.
        assert_eq!(reconcile("ab", "***", 2..2, 3).unwrap().0, "ab*");
        // Pasted over a selection, and a selection cut.
        assert_eq!(reconcile("abcd", "*XY*", 1..3, 3).unwrap().0, "aXYd");
        assert_eq!(reconcile("abcd", "**", 1..3, 1), Some(("ad".into(), 1..1)));
    }

    #[test]
    fn reconciling_deletions_uses_the_cursor() {
        // Backspace at the end, Backspace in the middle, and Delete at the start.
        assert_eq!(reconcile("abc", "**", 3..3, 2).unwrap().0, "ab");
        assert_eq!(reconcile("abc", "**", 2..2, 1).unwrap().0, "ac");
        assert_eq!(reconcile("abc", "**", 0..0, 0).unwrap().0, "bc");
    }

    #[test]
    fn reconciling_refuses_what_no_edit_could_do() {
        assert_eq!(reconcile("a", "****", 0..0, 0), None, "text from nowhere");
        assert_eq!(reconcile("a", "*", 0..0, 5), None, "a cursor past the end");
    }

    /// Typing and deleting move the cursor through the text layout, which
    /// needs Bevy's UI plugin and fonts; the `reconcile` tests cover how edits
    /// are worked out. This checks everything else a masked field does.
    #[test]
    fn masked_fields_show_dots_and_keep_the_real_text() {
        let config = TextInputConfig::new("hunter2").masked(true);
        let (mut app, field, editor) = app_with_field(config);
        assert_eq!(shown(&app, editor), dots(7));
        press(&mut app, editor, Key::Enter);
        assert_eq!(announced(&app), [("hunter2".into(), true)]);

        app.world_mut()
            .entity_mut(field)
            .insert(TextInputValue("pin".into()));
        app.update();
        assert_eq!(shown(&app, editor), dots(3));
        assert_eq!(committed(&app, field), "pin");

        // Escape after an edit puts back the committed text, masked.
        app.world_mut().get_mut::<Masked>(editor).unwrap().text = "pi".into();
        press(&mut app, editor, Key::Escape);
        app.update();
        assert_eq!(shown(&app, editor), dots(3));
        assert_eq!(app.world().get::<Masked>(editor).unwrap().text, "pin");
    }
}
