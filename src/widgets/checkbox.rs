//! A standard checkbox: a box that shows a mark when checked, and an optional label.

use bevy::ecs::entity::EntityHashSet;
use bevy::ecs::query::QueryData;
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui::{Checked, InteractionDisabled, UiSystems};
use bevy::ui_widgets::{Checkbox, checkbox_self_update};

use crate::EnsurePlugin;
use crate::cursor::{HoverCursor, HoverCursorPlugin};
use crate::sanitize::at_least;
use crate::theme::palette::{
    ACCENT, CONTROL_CORNER_RADIUS, CONTROL_TEXT_SIZE, EDGE, FONT, HIGHLIGHT, SURFACE, TEXT,
};
use crate::theme::skins;
use crate::theme::{
    FocusVisible, Shade, Theme, ThemeBorder, ThemeCornerRadius, ThemeFont, ThemeFontSize,
    ThemePlugin, ThemeSkin, ThemeTextColor, ThemeToken, WidgetFlags, WidgetState,
};

/// Prefix of the checkbox widget's theme tokens.
pub const CHECKBOX_STYLE: &str = "touchy.checkbox";
/// Default size of the checkbox's box, in pixels.
const DEFAULT_BOX_SIZE: f32 = 18.0;
/// Width of the box's border, in pixels.
const BOX_BORDER: f32 = 1.0;
/// Inset of the mark from the box's edge.
const MARK_INSET: f32 = 4.0;
/// Gap between the box and the label in pixels.
const LABEL_GAP: f32 = 8.0;

/// Theme tokens for a checkbox. Insert new ones to restyle a checkbox.
#[derive(Component, Clone, PartialEq, Debug)]
#[component(immutable)]
pub struct CheckboxTokens {
    /// The background of the box when unchecked.
    pub box_bg: ThemeToken,
    /// The background of the box when checked.
    pub box_checked: ThemeToken,
    /// The border of the box.
    pub border: ThemeToken,
    /// The mark used to indicate the checkbox is checked.
    pub mark: ThemeToken,
    /// The color of the label text.
    pub label: ThemeToken,
    /// The font used for the label text.
    pub font: ThemeToken,
    /// The font size used for the label text.
    pub font_size: ThemeToken,
    /// The corner radius of the box and mark.
    pub corner_radius: ThemeToken,
}

impl CheckboxTokens {
    /// Names the tokens under `prefix`.
    pub fn with_prefix(prefix: &str) -> Self {
        let token = |suffix: &str| ThemeToken::new(format!("{prefix}.{suffix}"));
        Self {
            box_bg: token("box"),
            box_checked: token("box.checked"),
            border: token("border"),
            mark: token("mark"),
            label: token("label"),
            font: token("font"),
            font_size: token("font_size"),
            corner_radius: token("corner_radius"),
        }
    }

    /// Sets default values for the tokens in `theme` if they are not already set.
    pub fn register_theme_defaults(&self, theme: &mut Theme) {
        let lit_border = |state| self.border.clone().with_state(state);
        theme
            .default_derived(self.box_bg.clone(), SURFACE, Shade::Same)
            .default_derived(self.box_checked.clone(), ACCENT, Shade::Same)
            .default_derived(self.border.clone(), EDGE, Shade::Same)
            .default_derived(lit_border(WidgetState::Hover), HIGHLIGHT, Shade::Same)
            .default_derived(lit_border(WidgetState::Focused), HIGHLIGHT, Shade::Same)
            .default_derived(self.mark.clone(), self.box_checked.clone(), Shade::Contrast)
            .default_derived(self.label.clone(), TEXT, Shade::Same)
            .default_font_from(self.font.clone(), FONT)
            .default_size_from(self.font_size.clone(), CONTROL_TEXT_SIZE)
            .default_size_scaled(self.corner_radius.clone(), CONTROL_CORNER_RADIUS, 0.75);
        // Each part draws its role's skin, once a theme gives the role one.
        theme
            .default_skin_from(self.box_bg.clone(), skins::SUNKEN)
            .default_skin_from(self.box_checked.clone(), skins::SUNKEN)
            .default_skin_from(self.mark.clone(), skins::MARK);
    }

    /// Box background, box border and label tokens for a checkbox in this state.
    fn resolve(&self, state: CheckboxState) -> (ThemeSkin, ThemeBorder, ThemeTextColor) {
        let flags = WidgetFlags {
            hovered: state.hovered,
            focused: state.focused,
            pressed: false,
            disabled: state.disabled,
        };
        let widget_state = flags.state();
        let box_bg = if state.checked {
            &self.box_checked
        } else {
            &self.box_bg
        };
        // Only the border responds to hovering and the focus; the box and label keep their colours.
        let dimmed = state.disabled.then_some(WidgetState::Disabled);
        (
            ThemeSkin(box_bg.clone().with_state(dimmed)),
            ThemeBorder::all(self.border.clone().with_state(widget_state)),
            ThemeTextColor(self.label.clone().with_state(dimmed)),
        )
    }
}

/// The built-in token set.
impl Default for CheckboxTokens {
    fn default() -> Self {
        Self::with_prefix(CHECKBOX_STYLE)
    }
}

/// Construction-time configuration for a checkbox widget.
///
/// Consumed by [`SpawnUi::checkbox`](crate::spawn_ui::SpawnUi::checkbox) and not retained as a
/// component.
///
/// # Examples
/// ```
/// use touchy::prelude::*;
///
/// let config = CheckboxConfig::new().label("Loop").checked(true);
/// ```
#[derive(Clone, Debug)]
pub struct CheckboxConfig {
    /// Optional label text. If `None`, a bare checkbox with no label.
    pub label: Option<String>,
    /// The width and height of the box, in pixels.
    pub size: f32,
    /// Whether the checkbox is checked at spawn.
    pub checked: bool,
    /// Whether the checkbox is disabled at spawn.
    pub disabled: bool,
    /// Check and uncheck on click. Turn off for a controlled checkbox whose
    /// state only changes when the app adds or removes [`Checked`].
    pub self_update: bool,
    /// Theme tokens for the checkbox.
    pub tokens: CheckboxTokens,
}

impl CheckboxConfig {
    /// Create a new checkbox config with no label.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the label beside the box.
    pub fn label(mut self, text: impl Into<String>) -> Self {
        self.label = Some(text.into());
        self
    }

    /// Set the width and height of the box.
    pub fn size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }

    /// Set whether the checkbox is checked at spawn.
    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }

    /// Set whether the checkbox is disabled at spawn.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Set whether the checkbox self-updates on click.
    pub fn self_update(mut self, self_update: bool) -> Self {
        self.self_update = self_update;
        self
    }

    /// Set the theme tokens for the checkbox.
    pub fn tokens(mut self, tokens: CheckboxTokens) -> Self {
        self.tokens = tokens;
        self
    }

    /// This config with every value made usable, warning about each repair.
    fn sanitized(mut self) -> Self {
        // Room for the mark inside the box.
        self.size = at_least(self.size, 2.0 * MARK_INSET, "CheckboxConfig size");
        self
    }
}

impl Default for CheckboxConfig {
    fn default() -> Self {
        Self {
            label: None,
            size: DEFAULT_BOX_SIZE,
            checked: false,
            disabled: false,
            self_update: true,
            tokens: CheckboxTokens::with_prefix(CHECKBOX_STYLE),
        }
    }
}

/// The parts of a checkbox, on the checkbox.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct CheckboxParts {
    /// The box. (`box` is a Rust keyword.)
    pub frame: Entity,
    /// The mark in the box, shown while checked.
    pub mark: Entity,
    /// The label, if it has one.
    pub label: Option<Entity>,
}

/// Spawns it; documented on [`SpawnUi::checkbox`](crate::spawn_ui::SpawnUi::checkbox).
pub(crate) fn spawn_checkbox<'a>(
    parent: &'a mut ChildSpawnerCommands,
    config: CheckboxConfig,
) -> EntityCommands<'a> {
    let CheckboxConfig {
        label,
        size,
        checked,
        disabled,
        self_update,
        tokens,
    } = config.sanitized();
    let mut checkbox = parent.spawn((
        Checkbox,
        // Tab reaches it in layout order; Bevy's checkbox toggles on Enter and Space once focused.
        TabIndex(0),
        Hovered::default(),
        HoverCursor::POINTER,
        Node {
            align_items: AlignItems::Center,
            column_gap: px(LABEL_GAP),
            ..default()
        },
    ));
    if checked {
        checkbox.insert(Checked);
    }
    if disabled {
        checkbox.insert(InteractionDisabled);
    }
    if self_update {
        checkbox.observe(checkbox_self_update);
    }

    let mut parts = CheckboxParts {
        frame: Entity::PLACEHOLDER,
        mark: Entity::PLACEHOLDER,
        label: None,
    };
    checkbox.with_children(|row| {
        let frame = Node {
            width: px(size),
            height: px(size),
            border: UiRect::all(px(BOX_BORDER)),
            padding: UiRect::all(px(MARK_INSET - BOX_BORDER)),
            ..default()
        };
        parts.frame = row
            .spawn((Pickable::IGNORE, frame))
            .with_children(|frame| {
                let mark = Node {
                    width: percent(100),
                    height: percent(100),
                    ..default()
                };
                let shown = mark_visibility(checked);
                parts.mark = frame.spawn((Pickable::IGNORE, mark, shown)).id();
            })
            .id();
        parts.label = label.map(|text| row.spawn((Pickable::IGNORE, Text::new(text))).id());
    });
    // Its look comes with its tokens, in `style_checkbox`, once its state is in.
    checkbox.insert((parts, tokens));
    checkbox
}

/// The mark's visibility: shown while checked.
fn mark_visibility(checked: bool) -> Visibility {
    if checked {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    }
}

/// What a checkbox is doing, which picks its look.
#[derive(Clone, Copy, Debug)]
struct CheckboxState {
    hovered: bool,
    focused: bool,
    checked: bool,
    disabled: bool,
}

/// A checkbox's state, as its components hold it.
#[derive(QueryData)]
struct CheckboxStatus {
    hovered: &'static Hovered,
    focused: Has<FocusVisible>,
    checked: Has<Checked>,
    disabled: Has<InteractionDisabled>,
}

impl CheckboxStatusItem<'_, '_> {
    fn state(&self) -> CheckboxState {
        CheckboxState {
            hovered: self.hovered.0,
            focused: self.focused,
            checked: self.checked,
            disabled: self.disabled,
        }
    }
}

/// State changes that can alter a checkbox's look. Removals are read separately,
/// because change detection doesn't see them, and new tokens in `style_checkbox`.
type LookChanged = (
    With<CheckboxParts>,
    Or<(
        Changed<Hovered>,
        Added<FocusVisible>,
        Added<Checked>,
        Added<InteractionDisabled>,
    )>,
);

/// Re-picks tokens for checkboxes whose state changed.
fn restyle_checkboxes(
    changed: Query<Entity, LookChanged>,
    // States that ended, which change detection doesn't see.
    (mut unchecked, mut enabled, mut unfocused): (
        RemovedComponents<Checked>,
        RemovedComponents<InteractionDisabled>,
        RemovedComponents<FocusVisible>,
    ),
    checkboxes: Query<(&CheckboxTokens, &CheckboxParts, CheckboxStatus)>,
    frames: Query<(&ThemeSkin, &ThemeBorder)>,
    labels: Query<&ThemeTextColor>,
    mut commands: Commands,
) {
    let mut dirty: EntityHashSet = changed.iter().collect();
    dirty.extend(
        unchecked
            .read()
            .chain(enabled.read())
            .chain(unfocused.read()),
    );
    for entity in dirty {
        let Ok((tokens, parts, status)) = checkboxes.get(entity) else {
            continue;
        };
        let (box_bg, border, label_color) = tokens.resolve(status.state());
        if let Ok((old_bg, old_border)) = frames.get(parts.frame) {
            if *old_bg != box_bg {
                commands.entity(parts.frame).insert(box_bg);
            }
            if *old_border != border {
                commands.entity(parts.frame).insert(border);
            }
        }
        if let Some(label) = parts.label
            && labels.get(label).is_ok_and(|old| *old != label_color)
        {
            commands.entity(label).insert(label_color);
        }
    }
}

/// Shows the mark while checked, whether a click or the app changed it.
fn show_marks(
    checked: Query<Entity, (With<CheckboxParts>, Added<Checked>)>,
    mut unchecked: RemovedComponents<Checked>,
    checkboxes: Query<(&CheckboxParts, Has<Checked>)>,
    mut marks: Query<&mut Visibility>,
) {
    for entity in checked.iter().chain(unchecked.read()) {
        if let Ok((parts, is_checked)) = checkboxes.get(entity)
            && let Ok(mut visibility) = marks.get_mut(parts.mark)
        {
            visibility.set_if_neq(mark_visibility(is_checked));
        }
    }
}

/// Styles a checkbox when its tokens are set: at spawn, and whenever the app sets new ones.
fn style_checkbox(
    insert: On<Insert<CheckboxTokens>>,
    checkboxes: Query<(&CheckboxTokens, &CheckboxParts, CheckboxStatus)>,
    mut commands: Commands,
) {
    let Ok((tokens, parts, status)) = checkboxes.get(insert.entity) else {
        return;
    };
    let (box_bg, border, label_color) = tokens.resolve(status.state());
    let rounded = ThemeCornerRadius(tokens.corner_radius.clone());
    commands
        .entity(parts.frame)
        .insert((box_bg, border, rounded.clone()));
    commands
        .entity(parts.mark)
        .insert((ThemeSkin(tokens.mark.clone()), rounded));
    if let Some(label) = parts.label {
        commands.entity(label).insert((
            label_color,
            ThemeFont(tokens.font.clone()),
            ThemeFontSize(tokens.font_size.clone()),
        ));
    }
}

/// Checkbox styling and default token values.
pub struct CheckboxWidgetPlugin;

impl Plugin for CheckboxWidgetPlugin {
    fn build(&self, app: &mut App) {
        app.ensure_plugin(HoverCursorPlugin)
            .ensure_plugin(ThemePlugin)
            .add_observer(style_checkbox)
            .add_systems(
                PostUpdate,
                (restyle_checkboxes, show_marks).before(UiSystems::Prepare),
            );
        CheckboxTokens::with_prefix(CHECKBOX_STYLE)
            .register_theme_defaults(&mut app.world_mut().resource_mut::<Theme>());
    }
}

#[cfg(test)]
mod tests {
    use bevy::ui_widgets::{CheckboxPlugin, ToggleChecked};

    use super::*;

    fn app_with_checkbox(config: CheckboxConfig) -> (App, Entity) {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, CheckboxPlugin, CheckboxWidgetPlugin));
        let mut checkbox = None;
        app.world_mut()
            .commands()
            .spawn(Node::default())
            .with_children(|root| {
                checkbox = Some(spawn_checkbox(root, config).id());
            });
        app.update();
        (app, checkbox.unwrap())
    }

    fn parts(app: &App, checkbox: Entity) -> CheckboxParts {
        *app.world().get::<CheckboxParts>(checkbox).unwrap()
    }

    fn mark_shown(app: &App, checkbox: Entity) -> bool {
        let mark = parts(app, checkbox).mark;
        app.world().get::<Visibility>(mark) != Some(&Visibility::Hidden)
    }

    #[test]
    fn toggling_checks_shows_the_mark_and_recolours_the_box() {
        let (mut app, checkbox) = app_with_checkbox(CheckboxConfig::new().label("Loop"));
        assert!(!mark_shown(&app, checkbox));

        app.world_mut().trigger(ToggleChecked { entity: checkbox });
        app.update();
        assert!(app.world().get::<Checked>(checkbox).is_some());
        assert!(mark_shown(&app, checkbox));
        let frame = parts(&app, checkbox).frame;
        assert_eq!(
            app.world().get::<ThemeSkin>(frame).unwrap().0,
            CheckboxTokens::with_prefix(CHECKBOX_STYLE).box_checked
        );

        app.world_mut().trigger(ToggleChecked { entity: checkbox });
        app.update();
        assert!(
            !mark_shown(&app, checkbox),
            "unchecking hides the mark again"
        );
    }

    #[test]
    fn disabled_checkboxes_ignore_toggles_and_dim_their_label() {
        let (mut app, checkbox) =
            app_with_checkbox(CheckboxConfig::new().label("Loop").disabled(true));
        app.world_mut().trigger(ToggleChecked { entity: checkbox });
        app.update();
        assert!(app.world().get::<Checked>(checkbox).is_none());
        let label = parts(&app, checkbox).label.unwrap();
        let tokens = CheckboxTokens::with_prefix(CHECKBOX_STYLE);
        assert_eq!(
            app.world().get::<ThemeTextColor>(label).unwrap().0,
            tokens.label.with_state(WidgetState::Disabled)
        );
    }

    #[test]
    fn new_tokens_restyle_every_part_in_its_current_state() {
        let config = CheckboxConfig::new().label("Loop").checked(true).size(24.0);
        let (mut app, checkbox) = app_with_checkbox(config);
        let tokens = CheckboxTokens::with_prefix("game.option");
        app.world_mut().entity_mut(checkbox).insert(tokens.clone());
        app.update();

        let parts = parts(&app, checkbox);
        let world = app.world();
        assert_eq!(world.get::<Node>(parts.frame).unwrap().width, px(24.0));
        let frame = world.get::<ThemeSkin>(parts.frame).unwrap();
        assert_eq!(frame.0, tokens.box_checked, "still checked");
        let mark = world.get::<ThemeSkin>(parts.mark).unwrap();
        assert_eq!(mark.0, tokens.mark);
        let label = parts.label.unwrap();
        assert_eq!(world.get::<ThemeFont>(label).unwrap().0, tokens.font);
    }

    #[test]
    fn a_box_too_small_for_its_mark_is_repaired() {
        let config = CheckboxConfig::new().size(2.0).sanitized();
        assert_eq!(config.size, 2.0 * MARK_INSET);
    }

    /// Tab reaches a checkbox, and its border shows the focus.
    #[test]
    fn checkboxes_take_the_focus_and_show_it() {
        use bevy::input_focus::tab_navigation::TabIndex;

        let (mut app, checkbox) = app_with_checkbox(CheckboxConfig::new().label("Loop"));
        assert!(app.world().get::<TabIndex>(checkbox).is_some());
        let tokens = CheckboxTokens::with_prefix(CHECKBOX_STYLE);
        let border = |app: &App| {
            let frame = parts(app, checkbox).frame;
            app.world().get::<ThemeBorder>(frame).unwrap().top.clone()
        };
        app.world_mut().entity_mut(checkbox).insert(FocusVisible);
        app.update();
        assert_eq!(
            border(&app),
            tokens.border.clone().with_state(WidgetState::Focused)
        );
    }
}
