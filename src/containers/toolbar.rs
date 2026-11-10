//! Toolbars divide a container into two regions: a bar along one edge, and the remaining body.
//!
//! Space is reserved for the bar, the body gets what is left. Toolbars can be placed along
//! any edge of their parent and can be nested. The body is an independent container. Panels
//! spawned in it dock to its edges, including the edge with the toolbar.
//!
//! # Usage
//!
//! Construct a [`ToolbarConfig`] and pass it to
//! [`SpawnUi::toolbar`](crate::spawn_ui::SpawnUi::toolbar) to create the toolbar and populate its
//! two regions.

use bevy::prelude::*;

use crate::EnsurePlugin;
use crate::theme::palette::{EDGE, SURFACE};
use crate::theme::skins;
use crate::theme::{Shade, Theme, ThemeBorder, ThemePlugin, ThemeSkin, ThemeToken};

/// Prefix of the toolbar's theme tokens.
pub const TOOLBAR_STYLE: &str = "touchy.toolbar";

/// Flexbox padding in pixels.
const DEFAULT_PADDING: f32 = 4.0;
/// Flexbox gap in pixels.
const DEFAULT_GAP: f32 = 2.0;
/// The line between the bar and the body.
const BORDER: f32 = 1.0;

/// Which edge of its area a toolbar runs along.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ToolbarSide {
    #[default]
    /// Along the top edge; body sits below.
    Top,
    /// Along the bottom edge; body sits above.
    Bottom,
    /// Along the left edge; body sits to the right.
    Left,
    /// Along the right edge; body sits to the left.
    Right,
}

impl ToolbarSide {
    /// Whether the bar runs horizontally (top or bottom).
    fn is_across(self) -> bool {
        matches!(self, Self::Top | Self::Bottom)
    }
    /// Whether the bar comes before the body (top or left).
    fn leads(self) -> bool {
        matches!(self, Self::Top | Self::Left)
    }
    /// Which edge of the bar gets the border, and how thick it is.
    fn inner_edge(self, width: Val) -> UiRect {
        match self {
            Self::Top => UiRect::bottom(width),
            Self::Bottom => UiRect::top(width),
            Self::Left => UiRect::right(width),
            Self::Right => UiRect::left(width),
        }
    }
}

/// Theme tokens for a toolbar.
#[derive(Component, Clone, PartialEq, Debug)]
#[component(immutable)]
pub struct ToolbarTokens {
    /// The toolbar's background.
    pub bg: ThemeToken,
    /// The line between the bar and the body.
    pub border: ThemeToken,
}

impl ToolbarTokens {
    /// Names the tokens under `prefix`. Call
    /// [`register_theme_defaults`](Self::register_theme_defaults) to derive defaults from the palette.
    /// e.g. if "my_theme.toolbar" is the prefix, "my_theme.toolbar.bg" will define the background.
    pub fn with_prefix(prefix: &str) -> Self {
        let token = |suffix: &str| ThemeToken::new(format!("{prefix}.{suffix}"));
        Self {
            bg: token("bg"),
            border: token("border"),
        }
    }
    /// Register the default values for the toolbar tokens, unless the app sets them.
    pub fn register_theme_defaults(&self, theme: &mut Theme) {
        theme
            .default_derived(self.bg.clone(), SURFACE, Shade::Same)
            .default_derived(self.border.clone(), EDGE, Shade::Same);
        // Each part draws its role's skin, once a theme gives the role one.
        theme.default_skin_from(self.bg.clone(), skins::SURFACE);
    }
}

/// The built-in token set.
impl Default for ToolbarTokens {
    fn default() -> Self {
        Self::with_prefix(TOOLBAR_STYLE)
    }
}

/// Construction-time configuration for a toolbar.
///
/// Consumed by [`SpawnUi::toolbar`](crate::spawn_ui::SpawnUi::toolbar) and not retained as a
/// component.
///
/// # Examples
/// ```
/// use touchy::prelude::*;
///
/// let config = ToolbarConfig::new(ToolbarSide::Left)
///     .padding(8.0)
///     .gap(4.0);
///
/// # assert_eq!(config.side, ToolbarSide::Left);
/// # assert_eq!(config.padding, 8.0);
/// ```
#[derive(Clone, Debug)]
pub struct ToolbarConfig {
    /// Which edge of its area the bar runs along.
    pub side: ToolbarSide,
    /// Space inside the bar's edge, in pixels.
    pub padding: f32,
    /// Space between the things in the bar, in pixels.
    pub gap: f32,
    /// The bar's look. The remainder (the body) is transparent and has no theme tokens.
    pub tokens: ToolbarTokens,
}

impl ToolbarConfig {
    /// Create a new configuration for a toolbar along `side` of its area.
    pub fn new(side: ToolbarSide) -> Self {
        Self {
            side,
            padding: DEFAULT_PADDING,
            gap: DEFAULT_GAP,
            tokens: ToolbarTokens::with_prefix(TOOLBAR_STYLE),
        }
    }
    /// Set the space inside the bar's edge, in pixels.
    pub fn padding(mut self, pixels: f32) -> Self {
        self.padding = pixels;
        self
    }
    /// Set the space between the things in the bar, in pixels.
    pub fn gap(mut self, pixels: f32) -> Self {
        self.gap = pixels;
        self
    }
    /// Set the theme tokens for the toolbar.
    pub fn tokens(mut self, tokens: ToolbarTokens) -> Self {
        self.tokens = tokens;
        self
    }
    /// Private helper to validate Config immediately before spawn time.
    fn sanitized(mut self) -> Self {
        self.padding = self.padding.max(0.0);
        self.gap = self.gap.max(0.0);
        self
    }
}

impl Default for ToolbarConfig {
    fn default() -> Self {
        Self::new(ToolbarSide::default())
    }
}

/// Identifies the root entity that gets split into a bar and a body.
#[derive(Component, Clone, Copy, Debug)]
pub struct Toolbar {
    bar: Entity,
    body: Entity,
}

impl Toolbar {
    /// The toolbar's strip along the edge.
    pub fn bar(&self) -> Entity {
        self.bar
    }

    /// The remaining, non-toolbar portion of the container.
    pub fn body(&self) -> Entity {
        self.body
    }
}

/// Spawns it; documented on [`SpawnUi::toolbar`](crate::spawn_ui::SpawnUi::toolbar).
pub(crate) fn spawn_toolbar<'a>(
    parent: &'a mut ChildSpawnerCommands,
    config: ToolbarConfig,
    bar: impl FnOnce(&mut ChildSpawnerCommands),
    body: impl FnOnce(&mut ChildSpawnerCommands),
) -> EntityCommands<'a> {
    let ToolbarConfig {
        side,
        padding,
        gap,
        tokens,
    } = config.sanitized();
    let (direction, bar_direction) = if side.is_across() {
        (FlexDirection::Column, FlexDirection::Row)
    } else {
        (FlexDirection::Row, FlexDirection::Column)
    };
    // Grows to fill its parent along a row or a column, and stretches across
    // it, so it fills a screen, a panel or another toolbar's body alike. Only
    // the bar takes the pointer: the rest is see-through, as the layers are,
    // so a press on an empty body reaches whatever is under it, such as a world.
    let mut root = parent.spawn((
        Node {
            flex_direction: direction,
            flex_grow: 1.0,
            flex_basis: px(0),
            align_self: AlignSelf::Stretch,
            min_width: px(0),
            min_height: px(0),
            ..default()
        },
        Pickable::IGNORE,
    ));
    let mut parts = [Entity::PLACEHOLDER; 2];
    root.with_children(|root| {
        let spawn_bar = |root: &mut ChildSpawnerCommands| {
            root.spawn(Node {
                flex_direction: bar_direction,
                flex_grow: 0.0,
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                border: side.inner_edge(px(BORDER)),
                padding: UiRect::all(px(padding)),
                row_gap: px(gap),
                column_gap: px(gap),
                // Toolbar doesn't bleed into the body.
                overflow: Overflow::clip(),
                ..default()
            })
            .with_children(bar)
            .id()
        };
        let spawn_body = |root: &mut ChildSpawnerCommands| {
            // A container of its own: what's docked in it docks to its edges,
            // and nothing in it spills out over the bar.
            root.spawn((
                Node {
                    flex_grow: 1.0,
                    flex_basis: px(0),
                    min_width: px(0),
                    min_height: px(0),
                    overflow: Overflow::clip(),
                    ..default()
                },
                Pickable::IGNORE,
            ))
            .with_children(body)
            .id()
        };
        if side.leads() {
            parts[0] = spawn_bar(root);
            parts[1] = spawn_body(root);
        } else {
            parts[1] = spawn_body(root);
            parts[0] = spawn_bar(root);
        }
    });
    let [bar, body] = parts;
    root.insert((Toolbar { bar, body }, tokens));
    root
}

/// Restyles a toolbar's bar when its theme tokens are set.
fn restyle_toolbar(
    insert: On<Insert<ToolbarTokens>>,
    toolbars: Query<(&Toolbar, &ToolbarTokens)>,
    mut commands: Commands,
) {
    // Tokens on a non-toolbar entity.
    let Ok((toolbar, tokens)) = toolbars.get(insert.entity) else {
        return;
    };
    commands.entity(toolbar.bar()).insert((
        ThemeSkin(tokens.bg.clone()),
        ThemeBorder::all(tokens.border.clone()),
    ));
}

/// Adds toolbars and their theme tokens to the app.
pub struct ToolbarPlugin;

impl Plugin for ToolbarPlugin {
    fn build(&self, app: &mut App) {
        app.ensure_plugin(ThemePlugin).add_observer(restyle_toolbar);
        ToolbarTokens::with_prefix(TOOLBAR_STYLE)
            .register_theme_defaults(&mut app.world_mut().resource_mut::<Theme>());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toolbar(side: ToolbarSide) -> (App, Toolbar, Entity) {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, ToolbarPlugin));
        let mut root = None;
        app.world_mut()
            .commands()
            .spawn(Node::default())
            .with_children(|parent| {
                root = Some(
                    spawn_toolbar(
                        parent,
                        ToolbarConfig::new(side),
                        |bar| {
                            bar.spawn(Name::new("tool"));
                        },
                        |body| {
                            body.spawn(Name::new("content"));
                        },
                    )
                    .id(),
                );
            });
        app.update();
        let root = root.unwrap();
        let parts = *app.world().get::<Toolbar>(root).unwrap();
        (app, parts, root)
    }

    fn child_names(app: &App, parent: Entity) -> Vec<String> {
        app.world()
            .get::<Children>(parent)
            .unwrap()
            .iter()
            .filter_map(|child| app.world().get::<Name>(child))
            .map(|name| name.to_string())
            .collect()
    }

    #[test]
    fn the_bar_and_the_body_hold_what_each_was_given() {
        let (app, parts, _) = toolbar(ToolbarSide::Top);
        assert_eq!(child_names(&app, parts.bar()), ["tool"]);
        assert_eq!(child_names(&app, parts.body()), ["content"]);
    }

    // A press goes through body to whatever is under, but is blocked by bar.
    #[test]
    fn only_the_bar_takes_the_pointer() {
        let (app, parts, root) = toolbar(ToolbarSide::Top);
        let blocks = |entity| {
            app.world()
                .get::<Pickable>(entity)
                .is_none_or(|pickable| pickable.should_block_lower)
        };
        assert!(!blocks(root));
        assert!(!blocks(parts.body()));
        assert!(blocks(parts.bar()));
    }

    #[test]
    fn the_bar_sits_on_its_side_with_its_line_facing_the_body() {
        for (side, direction, first, edge) in [
            (
                ToolbarSide::Top,
                FlexDirection::Column,
                true,
                UiRect::bottom(px(BORDER)),
            ),
            (
                ToolbarSide::Bottom,
                FlexDirection::Column,
                false,
                UiRect::top(px(BORDER)),
            ),
            (
                ToolbarSide::Left,
                FlexDirection::Row,
                true,
                UiRect::right(px(BORDER)),
            ),
            (
                ToolbarSide::Right,
                FlexDirection::Row,
                false,
                UiRect::left(px(BORDER)),
            ),
        ] {
            let (app, parts, root) = toolbar(side);
            assert_eq!(
                app.world().get::<Node>(root).unwrap().flex_direction,
                direction
            );
            let children: Vec<Entity> = app.world().get::<Children>(root).unwrap().to_vec();
            let bar_first = children[0] == parts.bar();
            assert_eq!(bar_first, first, "{side:?}");
            assert_eq!(app.world().get::<Node>(parts.bar()).unwrap().border, edge);
        }
    }

    #[test]
    fn the_bar_wears_its_tokens_and_changes_with_them() {
        let (mut app, parts, root) = toolbar(ToolbarSide::Top);
        let background = |app: &App| app.world().get::<ThemeSkin>(parts.bar()).unwrap().0.clone();
        assert_eq!(
            background(&app),
            ToolbarTokens::with_prefix(TOOLBAR_STYLE).bg
        );

        let custom = ToolbarTokens::with_prefix("test.toolbar");
        custom.register_theme_defaults(&mut app.world_mut().resource_mut::<Theme>());
        app.world_mut().entity_mut(root).insert(custom.clone());
        // The observer restyles through commands, so apply them.
        app.world_mut().flush();
        assert_eq!(background(&app), custom.bg);
    }
}
