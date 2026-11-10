//! Menus, as in a menu bar: a button showing a title, opening a list of items
//! to pick from.
//!
//! A menu is a [dropdown](crate::widgets::dropdown) that keeps no choice.
//! Picking an item announces a [`MenuPick`] on the menu, so the same item can
//! be picked again. Items are [choices](crate::choice), so a disabled one sits
//! in the list but can't be picked.
//!
//! Menus side by side, under the same parent, work as a bar: while one is
//! open, pointing at another opens that one instead.

use bevy::input_focus::{FocusCause, InputFocus};
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui::InteractionDisabled;

use crate::EnsurePlugin;
use crate::choice::{Choice, ChoiceSet, Chosen, Pick};
use crate::popover::PopoverOpen;
use crate::theme::palette::CONTROL;
use crate::theme::{Shade, Theme, ThemeToken, WidgetState};
use crate::widgets::dropdown::{
    DEFAULT_LIST_HEIGHT, DROPDOWN_STYLE, DropdownConfig, DropdownParts, DropdownTokens,
    DropdownWidgetPlugin, spawn_list_button,
};

/// Prefix of a menu's tokens: `touchy.menu.bg`, and so on.
pub const MENU_STYLE: &str = "touchy.menu";

/// A menu's button while hovered: the control colour, partly there.
const MENU_HOVER: Shade = Shade::Alpha(0.6);

/// A menu's look: a dropdown's tokens, under [`MENU_STYLE`]. By default the
/// button is flat, and everything else follows the dropdown's.
pub type MenuTokens = DropdownTokens;

/// Derives a menu's tokens, unless the app sets them: a flat button, the
/// control colour showing while hovered and open, no border; the list and
/// the text follow `base`, the dropdown's own.
fn default_menu_values(tokens: &MenuTokens, theme: &mut Theme, base: &DropdownTokens) {
    let state = |token: &ThemeToken, state| token.clone().with_state(state);
    theme
        .default_color(tokens.bg.clone(), Color::NONE)
        .default_derived(state(&tokens.bg, WidgetState::Hover), CONTROL, MENU_HOVER)
        .default_derived(
            state(&tokens.bg, WidgetState::Pressed),
            CONTROL,
            Shade::Same,
        )
        .default_color(state(&tokens.bg, WidgetState::Disabled), Color::NONE)
        .default_color(tokens.border.clone(), Color::NONE);
    let follows = [
        (tokens.text.clone(), base.text.clone()),
        (
            state(&tokens.text, WidgetState::Disabled),
            state(&base.text, WidgetState::Disabled),
        ),
        (tokens.placeholder.clone(), base.placeholder.clone()),
        (tokens.chevron.clone(), base.chevron.clone()),
        (tokens.option.clone(), base.option.clone()),
        (
            state(&tokens.option, WidgetState::Hover),
            state(&base.option, WidgetState::Hover),
        ),
        (tokens.option_chosen.clone(), base.option_chosen.clone()),
    ];
    for (token, from) in follows {
        theme.default_derived(token, from, Shade::Same);
    }
    theme
        .default_font_from(tokens.font.clone(), base.font.clone())
        .default_size_from(tokens.font_size.clone(), base.font_size.clone())
        .default_size_from(tokens.corner_radius.clone(), base.corner_radius.clone());
}

/// An item picked from a menu, by its place in the menu. Triggered on the
/// menu, so it can be observed there.
#[derive(EntityEvent, Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuPick {
    /// The menu picked from.
    #[event_target]
    pub menu: Entity,
    /// The item's place in the menu.
    pub index: usize,
}

/// How to spawn a menu: its title, its items, and its look.
#[derive(Clone, Debug)]
pub struct MenuConfig {
    /// What its button shows.
    pub title: String,
    /// The items to pick from, in order.
    pub items: Vec<Choice>,
    /// The tallest the list grows before it scrolls, in pixels.
    pub list_height: f32,
    /// Whether the menu starts disabled.
    pub disabled: bool,
    /// The menu's theme tokens.
    pub tokens: MenuTokens,
}

impl MenuConfig {
    /// A menu titled `title` holding `items`. Plain words make plain items,
    /// so `MenuConfig::new("File", ["New", "Open", "Save"])` is a whole menu.
    pub fn new<O: Into<Choice>>(
        title: impl Into<String>,
        items: impl IntoIterator<Item = O>,
    ) -> Self {
        Self {
            title: title.into(),
            items: items.into_iter().map(Into::into).collect(),
            list_height: DEFAULT_LIST_HEIGHT,
            disabled: false,
            tokens: MenuTokens::with_prefix(MENU_STYLE),
        }
    }

    /// The tallest the list grows before it scrolls, in pixels.
    pub fn list_height(mut self, pixels: f32) -> Self {
        self.list_height = pixels;
        self
    }

    /// Sets whether the menu starts disabled.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Sets the menu's theme tokens.
    pub fn tokens(mut self, tokens: MenuTokens) -> Self {
        self.tokens = tokens;
        self
    }
}

/// Marks a menu: a dropdown that announces picks rather than keeping them.
#[derive(Component, Clone, Copy, Debug)]
struct Menu;

/// Spawns it; documented on [`SpawnUi::menu`](crate::spawn_ui::SpawnUi::menu).
pub(crate) fn spawn_menu<'a>(
    parent: &'a mut ChildSpawnerCommands,
    config: MenuConfig,
) -> EntityCommands<'a> {
    let MenuConfig {
        title,
        items,
        list_height,
        disabled,
        tokens,
    } = config;
    let list = DropdownConfig {
        list_height,
        disabled,
        tokens,
        ..DropdownConfig::new(items)
    };
    let mut menu = spawn_list_button(parent, list, Some(title));
    menu.insert(Menu);
    menu
}

/// Picking an item announces it, and keeps no choice, so the same item can be
/// picked again.
fn announce_picks(
    pick: On<Pick>,
    menus: Query<&ChoiceSet, (With<Menu>, Without<InteractionDisabled>)>,
    mut commands: Commands,
) {
    let Ok(choices) = menus.get(pick.set) else {
        return;
    };
    let Some(index) = pick.index.filter(|&index| choices.can_pick(index)) else {
        return;
    };
    commands.trigger(MenuPick {
        menu: pick.set,
        index,
    });
    commands.entity(pick.set).insert(Chosen(None));
}

/// Menus that can be opened.
type EnabledMenus = (With<Menu>, Without<InteractionDisabled>);

/// While one menu is open, pointing at another beside it (under the same
/// parent, as in a menu bar) opens that one instead, and gives it the keys.
fn switch_menus_on_hover(
    menus: Query<(Entity, &DropdownParts, &ChildOf), EnabledMenus>,
    hovered: Query<&Hovered, Changed<Hovered>>,
    mut popovers: Query<&mut PopoverOpen>,
    mut focus: Option<ResMut<InputFocus>>,
) {
    for (menu, parts, parent) in &menus {
        if !hovered.get(parts.button).is_ok_and(|hover| hover.0) {
            continue;
        }
        let open: Vec<Entity> = menus
            .iter()
            .filter(|(other, _, other_parent)| {
                *other != menu && other_parent.parent() == parent.parent()
            })
            .map(|(_, other, _)| other.popover)
            .filter(|&popover| popovers.get(popover).is_ok_and(|open| open.0))
            .collect();
        if open.is_empty() {
            continue;
        }
        for popover in open {
            if let Ok(mut open) = popovers.get_mut(popover) {
                open.0 = false;
            }
        }
        if let Ok(mut open) = popovers.get_mut(parts.popover) {
            open.0 = true;
        }
        if let Some(focus) = focus.as_deref_mut() {
            focus.set(menu, FocusCause::Navigated);
        }
    }
}

/// Menus: picking, menu bars, and default token values.
pub struct MenuWidgetPlugin;

impl Plugin for MenuWidgetPlugin {
    fn build(&self, app: &mut App) {
        app.ensure_plugin(DropdownWidgetPlugin)
            .add_observer(announce_picks)
            .add_systems(Update, switch_menus_on_hover);
        let base = DropdownTokens::with_prefix(DROPDOWN_STYLE);
        let menu = MenuTokens::with_prefix(MENU_STYLE);
        default_menu_values(&menu, &mut app.world_mut().resource_mut::<Theme>(), &base);
    }
}

#[cfg(test)]
mod tests {
    use bevy::input::keyboard::Key;

    use super::*;
    use crate::widgets::dropdown::tests::{
        announced, chosen, click, face_text, list_app, open, parts, press,
    };

    /// The items menus announced, in order.
    #[derive(Resource, Default)]
    struct Picked(Vec<usize>);

    fn menu_app(config: MenuConfig) -> (App, Entity) {
        let mut app = list_app();
        app.add_plugins(MenuWidgetPlugin)
            .init_resource::<Picked>()
            .add_observer(|pick: On<MenuPick>, mut picked: ResMut<Picked>| {
                picked.0.push(pick.index);
            });
        let mut menu = None;
        app.world_mut()
            .commands()
            .spawn(Node::default())
            .with_children(|parent| menu = Some(spawn_menu(parent, config).id()));
        app.update();
        (app, menu.unwrap())
    }

    #[test]
    fn a_menu_shows_its_title_announces_picks_and_keeps_no_choice() {
        let (mut app, menu) = menu_app(MenuConfig::new("File", ["New", "Open", "Save"]));
        assert_eq!(face_text(&mut app, menu).as_deref(), Some("File"));
        let parts = parts(&app, menu);
        assert_eq!(parts.chevron, None, "its title says enough");
        for _ in 0..2 {
            click(&mut app, parts.button);
            assert!(open(&app, menu));
            click(&mut app, parts.rows[2].row);
            assert!(!open(&app, menu));
        }
        assert_eq!(
            app.world().resource::<Picked>().0,
            [2, 2],
            "the same item, twice"
        );
        assert_eq!(chosen(&app, menu), None);
        assert!(announced(&mut app).is_empty(), "no choice to announce");
        assert_eq!(face_text(&mut app, menu).as_deref(), Some("File"));
        // The keys pick from a menu too.
        press(&mut app, menu, Key::ArrowDown);
        press(&mut app, menu, Key::ArrowDown);
        press(&mut app, menu, Key::Enter);
        assert_eq!(app.world().resource::<Picked>().0, [2, 2, 0]);
    }

    #[test]
    fn while_one_menu_is_open_pointing_at_its_neighbour_opens_that_one() {
        let (mut app, file) = menu_app(MenuConfig::new("File", ["New"]));
        let parent = app.world().get::<ChildOf>(file).unwrap().parent();
        let mut edit = None;
        app.world_mut()
            .commands()
            .entity(parent)
            .with_children(|bar| {
                edit = Some(spawn_menu(bar, MenuConfig::new("Edit", ["Undo"])).id());
            });
        app.update();
        let edit = edit.unwrap();
        let hover = |app: &mut App, menu: Entity, over: bool| {
            let button = parts(app, menu).button;
            app.world_mut().entity_mut(button).insert(Hovered(over));
            app.update();
        };
        hover(&mut app, edit, true);
        assert!(!open(&app, edit), "with none open, pointing opens nothing");
        hover(&mut app, edit, false);
        let file_button = parts(&app, file).button;
        click(&mut app, file_button);
        hover(&mut app, edit, true);
        assert!(open(&app, edit));
        assert!(!open(&app, file));
    }
}
