//! Themed buttons built on Bevy's headless [`Button`].
//!
//! Behaviour comes from `bevy_ui_widgets`: clicks and keyboard activation fire
//! [`Activate`](bevy::ui_widgets::Activate), and pressing adds [`Pressed`]. This module adds the
//! look. It picks theme tokens from the button's state ([`Hovered`], [`Pressed`],
//! [`Checked`], [`InteractionDisabled`]) and the theme supplies the values.

use bevy::ecs::entity::EntityHashSet;
use bevy::ecs::query::QueryData;
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui::{Checked, InteractionDisabled, Pressed, UiSystems};
use bevy::ui_widgets::Button;

use crate::EnsurePlugin;
use crate::cursor::{HoverCursor, HoverCursorPlugin};
use crate::sanitize::at_least;
use crate::theme::palette::{
    ACCENT, CONTROL, CONTROL_CORNER_RADIUS, CONTROL_TEXT_SIZE, DANGER, FONT, HIGHLIGHT, TEXT,
    TEXT_MUTED,
};
use crate::theme::skins;
use crate::theme::{
    FocusVisible, Shade, Theme, ThemeBorder, ThemeCornerRadius, ThemeFont, ThemeFontSize,
    ThemePlugin, ThemeSkin, ThemeTextColor, ThemeToken, WidgetFlags, WidgetState,
};
use crate::tooltip::Tooltip;

/// The button family's sizes. They are the primary variant's own sizes, and
/// every other variant follows them unless a theme sets its own.
pub const BUTTON_FONT_SIZE: ThemeToken = ThemeToken::new_static("touchy.button.font_size");
/// The button family's icon size; see [`BUTTON_FONT_SIZE`].
pub const BUTTON_ICON_SIZE: ThemeToken = ThemeToken::new_static("touchy.button.icon_size");
/// The button family's corner radius; see [`BUTTON_FONT_SIZE`].
pub const BUTTON_CORNER_RADIUS: ThemeToken = ThemeToken::new_static("touchy.button.corner_radius");
/// The button family's font, which follows the palette font.
pub const BUTTON_FONT: ThemeToken = ThemeToken::new_static("touchy.button.font");

/// A button's least width and its height, and the space inside its border, in pixels.
const DEFAULT_SIZE: Vec2 = Vec2::new(42.0, 34.0);
const DEFAULT_PADDING: Vec2 = Vec2::new(12.0, 6.0);
/// The width of the bevel, in pixels.
const BORDER_WIDTH: f32 = 2.0;

/// The bevel's edges, relative to the button's face.
const BEVEL_LIGHT: Shade = Shade::Lighter(0.15);
const BEVEL_DARK: Shade = Shade::Darker(0.15);
/// A ghost button is invisible at rest; hovering and pressing show a control-coloured face.
const GHOST_HOVER: Shade = Shade::Alpha(0.4);
const GHOST_PRESSED: Shade = Shade::Darker(0.08);
/// The icon's size unless a theme sets it; the label and corners follow the palette's.
const DEFAULT_ICON_SIZE: f32 = 15.0;

/// Theme tokens for a button widget. Insert new ones to restyle a button.
#[derive(Component, Clone, PartialEq, Debug)]
#[component(immutable)]
pub struct ButtonTokens {
    /// The face.
    pub bg: ThemeToken,
    /// The bevel's lit edges, top and left, and the whole border while hovered.
    pub border_light: ThemeToken,
    /// The bevel's shaded edges, bottom and right.
    pub border_dark: ThemeToken,
    /// The label and icon.
    pub text: ThemeToken,
    /// The label's font size.
    pub font_size: ThemeToken,
    /// The icon's font size.
    pub icon_size: ThemeToken,
    /// The radius of the corners.
    pub corner_radius: ThemeToken,
    /// The label's and icon's font.
    pub font: ThemeToken,
}

impl ButtonTokens {
    /// Names the tokens under `prefix`.
    pub fn with_prefix(prefix: &str) -> Self {
        let token = |suffix: &str| ThemeToken::new(format!("{prefix}.{suffix}"));
        Self {
            bg: token("bg"),
            border_light: token("border.light"),
            border_dark: token("border.dark"),
            text: token("text"),
            font_size: token("font_size"),
            icon_size: token("icon_size"),
            corner_radius: token("corner_radius"),
            font: token("font"),
        }
    }

    /// Filled with the palette accent.
    pub fn primary() -> Self {
        Self::with_prefix("touchy.button")
    }

    /// Filled with the palette's control colour.
    pub fn secondary() -> Self {
        Self::with_prefix("touchy.button.secondary")
    }

    /// No background or border until hovered.
    pub fn ghost() -> Self {
        Self::with_prefix("touchy.button.ghost")
    }

    /// Filled with the palette's danger colour.
    pub fn danger() -> Self {
        Self::with_prefix("touchy.button.danger")
    }

    /// Derives this variant from `base`, unless the app sets its tokens: the
    /// face is `base`, the bevel a lighter and a darker edge of the face, the
    /// label whichever of black or white reads best on the face, and the
    /// border, hovered or focused, the palette highlight. Its sizes and font follow the
    /// button family's ([`BUTTON_FONT_SIZE`], [`BUTTON_FONT`] and the rest).
    pub fn derive_defaults(&self, theme: &mut Theme, base: ThemeToken) {
        let lit_border = |state| self.border_light.clone().with_state(state);
        // A disabled face is mostly see-through, so its label sits on whatever is
        // behind the button. Contrast with the opaque face would pick the wrong colour.
        let disabled_label = self.text.clone().with_state(WidgetState::Disabled);
        theme
            .default_derived(disabled_label, TEXT_MUTED, Shade::Same)
            .default_derived(self.bg.clone(), base, Shade::Same)
            .default_derived(self.border_light.clone(), self.bg.clone(), BEVEL_LIGHT)
            .default_derived(self.border_dark.clone(), self.bg.clone(), BEVEL_DARK)
            .default_derived(self.text.clone(), self.bg.clone(), Shade::Contrast)
            .default_derived(lit_border(WidgetState::Hover), HIGHLIGHT, Shade::Same)
            .default_derived(lit_border(WidgetState::Focused), HIGHLIGHT, Shade::Same);
        let family = [
            (&self.font_size, BUTTON_FONT_SIZE),
            (&self.icon_size, BUTTON_ICON_SIZE),
            (&self.corner_radius, BUTTON_CORNER_RADIUS),
        ];
        for (size, family_size) in family {
            // The primary variant's sizes are the family's, and can't follow themselves.
            if *size != family_size {
                theme.default_size_from(size.clone(), family_size);
            }
        }
        if self.font != BUTTON_FONT {
            theme.default_font_from(self.font.clone(), BUTTON_FONT);
        }
        // Each part draws its role's skin, once a theme gives the role one.
        theme.default_skin_from(self.bg.clone(), skins::RAISED);
    }

    /// Derives a ghost variant: invisible at rest, showing a face of `base`
    /// only while hovered or pressed, with a label in the palette's text colour.
    pub fn derive_ghost_defaults(&self, theme: &mut Theme, base: ThemeToken) {
        // The first default for a token wins, so the ghost's exceptions go before
        // the shared derivations. Its bevel derives from the clear face, so the
        // border is clear too.
        let face = |state| self.bg.clone().with_state(state);
        theme
            .default_color(self.bg.clone(), Color::NONE)
            .default_derived(face(WidgetState::Hover), base.clone(), GHOST_HOVER)
            .default_derived(face(WidgetState::Pressed), base.clone(), GHOST_PRESSED)
            // The label sits on whatever is behind the button, not on a face.
            .default_derived(self.text.clone(), TEXT, Shade::Same);
        self.derive_defaults(theme, base);
    }

    /// The background, border and label tokens for a button in this state.
    fn resolve(&self, state: ButtonState) -> (ThemeSkin, ThemeBorder, ThemeTextColor) {
        // A checked toggle button stays pressed in.
        let pressed_in = state.pressed || state.checked;
        let flags = WidgetFlags {
            hovered: state.hovered,
            focused: state.focused,
            pressed: pressed_in,
            disabled: state.disabled,
        };
        let widget_state = flags.state();
        let (border, text) = match widget_state {
            Some(WidgetState::Disabled) => (
                ThemeBorder::all(self.border_dark.clone().with_state(widget_state)),
                self.text.clone().with_state(widget_state),
            ),
            // Pressed in: the bevel is reversed.
            Some(WidgetState::Pressed) => (
                ThemeBorder::bevel(self.border_dark.clone(), self.border_light.clone()),
                self.text.clone(),
            ),
            // Lit all round, by the cursor or the focus.
            Some(WidgetState::Hover | WidgetState::Focused) => (
                ThemeBorder::all(self.border_light.clone().with_state(widget_state)),
                self.text.clone(),
            ),
            None => (
                ThemeBorder::bevel(self.border_light.clone(), self.border_dark.clone()),
                self.text.clone(),
            ),
        };
        (
            ThemeSkin(self.bg.clone().with_state(widget_state)),
            border,
            ThemeTextColor(text),
        )
    }
}

/// The primary variant's tokens.
impl Default for ButtonTokens {
    fn default() -> Self {
        Self::primary()
    }
}

/// What a button is doing, which picks its look.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct ButtonState {
    hovered: bool,
    focused: bool,
    pressed: bool,
    checked: bool,
    disabled: bool,
}

/// A button's state, as its components hold it.
#[derive(QueryData)]
struct ButtonStatus {
    hovered: &'static Hovered,
    focused: Has<FocusVisible>,
    pressed: Has<Pressed>,
    checked: Has<Checked>,
    disabled: Has<InteractionDisabled>,
}

impl ButtonStatusItem<'_, '_> {
    fn state(&self) -> ButtonState {
        ButtonState {
            hovered: self.hovered.0,
            focused: self.focused,
            pressed: self.pressed,
            checked: self.checked,
            disabled: self.disabled,
        }
    }
}

/// The parts of a button, on the button.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct ButtonParts {
    /// The text label, if it has one.
    pub label: Option<Entity>,
    /// The icon glyph, if it has one.
    pub icon: Option<Entity>,
}

impl ButtonParts {
    /// The label and the icon, whichever it has.
    fn texts(&self) -> impl Iterator<Item = Entity> {
        self.label.into_iter().chain(self.icon)
    }
}

/// How to spawn a button: what it shows, its size and its starting state.
///
/// A button with both an icon and a label shows the icon first:
///
/// ```
/// use touchy::widgets::button::ButtonConfig;
///
/// let save = ButtonConfig {
///     icon: Some("💾".into()),
///     ..ButtonConfig::new("Save")
/// };
/// ```
#[derive(Clone, Debug)]
pub struct ButtonConfig {
    /// The text label.
    pub label: Option<String>,
    /// The icon glyph, in the button's font.
    pub icon: Option<String>,
    /// Its look; the primary variant's, unless set.
    pub tokens: Option<ButtonTokens>,
    /// Its least width and its height, in pixels. It widens to fit what it shows.
    pub size: Vec2,
    /// The space between its border and what it shows, in pixels, across and
    /// down. Across is also the gap between its icon and label.
    pub padding: Vec2,
    /// Starts disabled.
    pub disabled: bool,
    /// Starts checked (pressed in), for toggles.
    pub checked: bool,
    /// A tooltip shown while it's hovered.
    pub tooltip: Option<Tooltip>,
}

impl Default for ButtonConfig {
    fn default() -> Self {
        Self {
            label: None,
            icon: None,
            tokens: None,
            size: DEFAULT_SIZE,
            padding: DEFAULT_PADDING,
            disabled: false,
            checked: false,
            tooltip: None,
        }
    }
}

impl ButtonConfig {
    /// A button showing `text`.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            label: Some(text.into()),
            ..default()
        }
    }

    /// A button showing the glyph `symbol`.
    pub fn icon(symbol: impl Into<String>) -> Self {
        Self {
            icon: Some(symbol.into()),
            ..default()
        }
    }

    /// Sets its look.
    pub fn tokens(mut self, tokens: ButtonTokens) -> Self {
        self.tokens = Some(tokens);
        self
    }

    /// Sets its least width and its height.
    pub fn size(mut self, width: f32, height: f32) -> Self {
        self.size = Vec2::new(width, height);
        self
    }

    /// Sets the space between its border and what it shows.
    pub fn padding(mut self, across: f32, down: f32) -> Self {
        self.padding = Vec2::new(across, down);
        self
    }

    /// Sets whether it starts disabled.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Sets whether it starts checked.
    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }

    /// Sets the tooltip shown while it's hovered.
    pub fn tooltip(mut self, tooltip: Tooltip) -> Self {
        self.tooltip = Some(tooltip);
        self
    }

    /// This config with every value made usable, warning about each repair.
    fn sanitized(mut self) -> Self {
        self.size = at_least(self.size, Vec2::ZERO, "ButtonConfig size");
        self.padding = at_least(self.padding, Vec2::ZERO, "ButtonConfig padding");
        self
    }

    /// The button's layout.
    fn node(&self) -> Node {
        Node {
            height: px(self.size.y),
            min_width: px(self.size.x),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            column_gap: px(self.padding.y),
            padding: UiRect::axes(px(self.padding.x), px(self.padding.y)),
            border: UiRect::all(px(BORDER_WIDTH)),
            ..default()
        }
    }
}

/// Spawns it; documented on [`SpawnUi::button`](crate::spawn_ui::SpawnUi::button).
pub(crate) fn spawn_button<'a>(
    parent: &'a mut ChildSpawnerCommands,
    config: ButtonConfig,
) -> EntityCommands<'a> {
    let config = config.sanitized();
    let tokens = config.tokens.clone().unwrap_or_else(ButtonTokens::primary);
    let mut button = parent.spawn((
        Button,
        Hovered::default(),
        // Tab reaches it in layout order; Bevy's widget handles the keys once focused.
        TabIndex(0),
        HoverCursor::POINTER,
        config.node(),
    ));
    if config.disabled {
        button.insert(InteractionDisabled);
    }
    if config.checked {
        button.insert(Checked);
    }
    if let Some(tooltip) = config.tooltip {
        button.insert(tooltip);
    }
    let mut parts = ButtonParts {
        label: None,
        icon: None,
    };
    button.with_children(|content| {
        parts.icon = config.icon.map(|icon| content.spawn(Text::new(icon)).id());
        parts.label = config
            .label
            .map(|label| content.spawn(Text::new(label)).id());
    });
    // Its look comes with its tokens, in `style_button`, once its state is in.
    button.insert((parts, tokens));
    button
}

/// State changes that can alter a button's look. Removals are read separately,
/// because change detection doesn't see them, and new tokens in `style_button`.
type LookChanged = Or<(
    Changed<Hovered>,
    Added<FocusVisible>,
    Added<Pressed>,
    Added<Checked>,
    Added<InteractionDisabled>,
)>;

/// Everything needed to re-pick a button's tokens, plus its current bindings to compare against.
type ButtonLook = (
    &'static ButtonTokens,
    &'static ButtonParts,
    ButtonStatus,
    &'static ThemeSkin,
    &'static ThemeBorder,
);

/// Re-picks tokens for buttons whose state changed.
fn restyle_buttons(
    changed: Query<Entity, (With<ButtonTokens>, LookChanged)>,
    // States that ended, which change detection doesn't see.
    (mut unfocused, mut unpressed, mut unchecked, mut enabled): (
        RemovedComponents<FocusVisible>,
        RemovedComponents<Pressed>,
        RemovedComponents<Checked>,
        RemovedComponents<InteractionDisabled>,
    ),
    buttons: Query<ButtonLook>,
    texts: Query<&ThemeTextColor>,
    mut commands: Commands,
) {
    let mut dirty: EntityHashSet = changed.iter().collect();
    dirty.extend(
        unfocused
            .read()
            .chain(unpressed.read())
            .chain(unchecked.read())
            .chain(enabled.read()),
    );

    for entity in dirty {
        let Ok((tokens, parts, status, old_bg, old_border)) = buttons.get(entity) else {
            continue; // Despawned, or not a themed button.
        };
        let (background, border, text_color) = tokens.resolve(status.state());

        // Re-insert only what changed, so unchanged bindings don't re-fire observers.
        let mut button = commands.entity(entity);
        if *old_bg != background {
            button.insert(background);
        }
        if *old_border != border {
            button.insert(border);
        }
        for text in parts.texts() {
            if texts.get(text).is_ok_and(|old| *old != text_color) {
                commands.entity(text).insert(text_color.clone());
            }
        }
    }
}

/// Styles a button when its tokens are set: at spawn, and whenever the app sets new ones.
fn style_button(
    insert: On<Insert<ButtonTokens>>,
    buttons: Query<(&ButtonTokens, &ButtonParts, ButtonStatus)>,
    mut commands: Commands,
) {
    let Ok((tokens, parts, status)) = buttons.get(insert.entity) else {
        return;
    };
    let (background, border, text_color) = tokens.resolve(status.state());
    commands.entity(insert.entity).insert((
        background,
        border,
        ThemeCornerRadius(tokens.corner_radius.clone()),
    ));
    let font = ThemeFont(tokens.font.clone());
    let sized = [
        (parts.icon, &tokens.icon_size),
        (parts.label, &tokens.font_size),
    ];
    for (text, size) in sized {
        if let Some(text) = text {
            commands.entity(text).insert((
                ThemeFontSize(size.clone()),
                font.clone(),
                text_color.clone(),
            ));
        }
    }
}

/// Themed buttons: default token values and styling.
pub struct ButtonWidgetPlugin;

impl Plugin for ButtonWidgetPlugin {
    fn build(&self, app: &mut App) {
        app.ensure_plugin(HoverCursorPlugin)
            .ensure_plugin(ThemePlugin)
            .add_observer(style_button)
            .add_systems(PostUpdate, restyle_buttons.before(UiSystems::Prepare));

        let mut theme = app.world_mut().resource_mut::<Theme>();
        ButtonTokens::primary().derive_defaults(&mut theme, ACCENT);
        ButtonTokens::secondary().derive_defaults(&mut theme, CONTROL);
        ButtonTokens::danger().derive_defaults(&mut theme, DANGER);

        ButtonTokens::ghost().derive_ghost_defaults(&mut theme, CONTROL);

        theme
            .default_size_from(BUTTON_FONT_SIZE, CONTROL_TEXT_SIZE)
            .default_size(BUTTON_ICON_SIZE, DEFAULT_ICON_SIZE)
            .default_size_from(BUTTON_CORNER_RADIUS, CONTROL_CORNER_RADIUS)
            .default_font_from(BUTTON_FONT, FONT);
    }
}

#[cfg(test)]
mod tests {
    use bevy::text::FontSize;

    use super::*;

    fn app_with_button(config: ButtonConfig) -> (App, Entity, Entity) {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, ThemePlugin, ButtonWidgetPlugin));
        let mut button = None;
        app.world_mut()
            .commands()
            .spawn(Node::default())
            .with_children(|root| {
                button = Some(spawn_button(root, config).id());
            });
        app.update();
        let button = button.unwrap();
        let label = app.world().get::<ButtonParts>(button).unwrap().label;
        (app, button, label.unwrap())
    }

    fn background(app: &App, entity: Entity) -> ThemeToken {
        app.world().get::<ThemeSkin>(entity).unwrap().0.clone()
    }

    #[test]
    fn hover_press_and_release_swap_tokens() {
        let (mut app, button, _) = app_with_button(ButtonConfig::new("OK"));
        let tokens = ButtonTokens::primary();
        assert_eq!(background(&app, button), tokens.bg);

        app.world_mut().entity_mut(button).insert(Hovered(true));
        app.update();
        let hover = tokens.bg.clone().with_state(WidgetState::Hover);
        assert_eq!(background(&app, button), hover);

        app.world_mut().entity_mut(button).insert(Pressed);
        app.update();
        assert_eq!(
            background(&app, button),
            tokens.bg.clone().with_state(WidgetState::Pressed)
        );

        // Removal isn't visible to change detection; this checks it's still handled.
        app.world_mut().entity_mut(button).remove::<Pressed>();
        app.update();
        assert_eq!(background(&app, button), hover);
    }

    #[test]
    fn disabling_restyles_label_and_cursor() {
        let (mut app, button, label) = app_with_button(ButtonConfig::new("OK"));
        app.world_mut()
            .entity_mut(button)
            .insert(InteractionDisabled);
        app.update();

        let tokens = ButtonTokens::primary();
        let disabled = |token: &ThemeToken| token.clone().with_state(WidgetState::Disabled);
        assert_eq!(background(&app, button), disabled(&tokens.bg));
        assert_eq!(
            app.world().get::<ThemeTextColor>(label).unwrap().0,
            disabled(&tokens.text)
        );
        assert_eq!(
            app.world()
                .get::<bevy::picking::cursor::EntityCursor>(button),
            Some(&bevy::picking::cursor::EntityCursor::System(
                bevy::window::SystemCursorIcon::NotAllowed
            ))
        );
        // The disabled face derives from the accent, through the theme's state shade.
        let theme = app.world().resource::<Theme>();
        let color = app.world().get::<BackgroundColor>(button).unwrap().0;
        assert_eq!(color, theme.color(&ACCENT).with_alpha(0.35));
    }

    /// One palette colour restyles a primary button: its face, its bevel, and
    /// a label that flips to stay readable.
    #[test]
    fn the_palette_accent_drives_primary_buttons() {
        const PALE: Color = Color::srgb(0.95, 0.9, 0.6);
        let (mut app, button, label) = app_with_button(ButtonConfig::new("OK"));
        app.world_mut()
            .resource_mut::<Theme>()
            .set_color(ACCENT, PALE);
        app.update();

        let world = app.world();
        assert_eq!(world.get::<BackgroundColor>(button).unwrap().0, PALE);
        assert_eq!(
            world.get::<BorderColor>(button).unwrap().top,
            BEVEL_LIGHT.apply(PALE)
        );
        assert_eq!(world.get::<TextColor>(label).unwrap().0, Color::BLACK);
    }

    /// A ghost button is invisible at rest but must still show a hover.
    #[test]
    fn ghost_buttons_appear_on_hover() {
        let config = ButtonConfig::new("Skip").tokens(ButtonTokens::ghost());
        let (mut app, button, _) = app_with_button(config);
        let alpha = |app: &App| {
            app.world()
                .get::<BackgroundColor>(button)
                .unwrap()
                .0
                .alpha()
        };
        assert_eq!(alpha(&app), 0.0);

        app.world_mut().entity_mut(button).insert(Hovered(true));
        app.update();
        assert!(alpha(&app) > 0.0);
    }

    /// Every variant takes the family's sizes until a theme gives it its own.
    #[test]
    fn variant_sizes_follow_the_family_until_set() {
        let config = ButtonConfig::new("Delete").tokens(ButtonTokens::danger());
        let (mut app, _, label) = app_with_button(config);
        let label_size = |app: &App| app.world().get::<TextFont>(label).unwrap().font_size;
        let control_text = app.world().resource::<Theme>().size(&CONTROL_TEXT_SIZE);
        assert_eq!(label_size(&app), FontSize::Px(control_text));

        app.world_mut()
            .resource_mut::<Theme>()
            .set_size(BUTTON_FONT_SIZE, 16.0);
        app.update();
        assert_eq!(label_size(&app), FontSize::Px(16.0));

        app.world_mut()
            .resource_mut::<Theme>()
            .set_size(ButtonTokens::danger().font_size, 20.0);
        app.update();
        assert_eq!(label_size(&app), FontSize::Px(20.0));
        let theme = app.world().resource::<Theme>();
        assert_eq!(theme.size(&BUTTON_FONT_SIZE), 16.0);
    }

    #[test]
    fn new_tokens_restyle_a_button_in_its_current_state() {
        let config = ButtonConfig {
            icon: Some("+".into()),
            ..ButtonConfig::new("Add")
        };
        let (mut app, button, label) = app_with_button(config);
        app.world_mut().entity_mut(button).insert(Hovered(true));
        app.update();

        let danger = ButtonTokens::danger();
        app.world_mut().entity_mut(button).insert(danger.clone());
        app.update();
        let hovered = danger.bg.clone().with_state(WidgetState::Hover);
        assert_eq!(background(&app, button), hovered);
        let icon = app
            .world()
            .get::<ButtonParts>(button)
            .unwrap()
            .icon
            .unwrap();
        for (text, size) in [(label, &danger.font_size), (icon, &danger.icon_size)] {
            let world = app.world();
            assert_eq!(world.get::<ThemeTextColor>(text).unwrap().0, danger.text);
            assert_eq!(world.get::<ThemeFontSize>(text).unwrap().0, *size);
        }
    }

    #[test]
    fn unusable_sizes_are_repaired() {
        let config = ButtonConfig::new("OK").size(-5.0, 20.0).padding(4.0, -1.0);
        let (app, button, _) = app_with_button(config);
        let node = app.world().get::<Node>(button).unwrap();
        assert_eq!((node.min_width, node.height), (px(0.0), px(20.0)));
        assert_eq!(node.padding.top, px(0.0));
    }

    /// A game's own token set gets the family's sizes too, and can change them alone.
    #[test]
    fn custom_token_sets_follow_the_family_sizes() {
        let launch = ButtonTokens::with_prefix("game.launch");
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, ButtonWidgetPlugin));
        let mut theme = app.world_mut().resource_mut::<Theme>();
        launch.derive_defaults(&mut theme, ACCENT);
        let control_radius = theme.size(&CONTROL_CORNER_RADIUS);
        assert_eq!(theme.size(&launch.corner_radius), control_radius);

        theme.set_size(launch.corner_radius.clone(), 0.0);
        assert_eq!(theme.size(&launch.corner_radius), 0.0);
        assert_eq!(theme.size(&BUTTON_CORNER_RADIUS), control_radius);
    }

    /// Focus shown by the keyboard or a gamepad lights the button as hovering
    /// does, stays lit while the cursor passes over, and gives way to a press.
    #[test]
    fn a_focused_button_is_lit_until_pressed_or_left() {
        let (mut app, button, _) = app_with_button(ButtonConfig::new("Play"));
        let tokens = ButtonTokens::primary();
        let focused = Some(WidgetState::Focused);
        app.world_mut().entity_mut(button).insert(FocusVisible);
        app.update();
        assert_eq!(
            background(&app, button),
            tokens.bg.clone().with_state(focused)
        );
        let lit = ThemeBorder::all(tokens.border_light.clone().with_state(focused));
        assert_eq!(app.world().get::<ThemeBorder>(button), Some(&lit));

        app.world_mut().entity_mut(button).insert(Hovered(true));
        app.update();
        assert_eq!(
            background(&app, button),
            tokens.bg.clone().with_state(focused)
        );

        app.world_mut().entity_mut(button).insert(Pressed);
        app.update();
        let pressed = Some(WidgetState::Pressed);
        assert_eq!(
            background(&app, button),
            tokens.bg.clone().with_state(pressed)
        );

        app.world_mut()
            .entity_mut(button)
            .remove::<(Pressed, FocusVisible)>()
            .insert(Hovered(false));
        app.update();
        assert_eq!(background(&app, button), tokens.bg);
    }
}
