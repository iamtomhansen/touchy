//! Tooltips: put a [`Tooltip`] on any UI entity, and it is described while the
//! pointer is over it (or over its children), or while it has keyboard focus.
//!
//! One shared tooltip node lives in the Feedback layer. It never takes hover
//! itself, so it can't flicker by covering what it describes.

use bevy::input_focus::InputFocus;
use bevy::picking::hover::HoverMap;
use bevy::picking::pointer::PointerId;
use bevy::prelude::*;
use bevy::ui::UiSystems;
use bevy::window::PrimaryWindow;

use crate::EnsurePlugin;
use crate::layers::{FeedbackLayerRoot, LayersPlugin};
use crate::theme::palette::{
    CONTROL, CONTROL_CORNER_RADIUS, EDGE, FONT, HIGHLIGHT, SMALL_SIZE, SURFACE, TEXT,
};
use crate::theme::skins;
use crate::theme::{
    Shade, Theme, ThemeBorder, ThemeCornerRadius, ThemeFont, ThemeFontSize, ThemePlugin, ThemeSkin,
    ThemeTextColor, ThemeToken,
};

/// Prefix of the tooltip's theme tokens.
pub const TOOLTIP_STYLE: &str = "touchy.tooltip";

/// The tooltip and its hotkey badge are slightly transparent by default.
const PANEL_OPACITY: Shade = Shade::Alpha(0.95);

/// Where a cursor-anchored tooltip sits relative to the cursor, in UI pixels.
const CURSOR_OFFSET: Vec2 = Vec2::new(12.0, 16.0);
/// Gap below a focused element for a focus-anchored tooltip.
const FOCUS_GAP: f32 = 8.0;
/// How far above its anchor a tooltip moves when there's no room below.
const FLIP_GAP: f32 = 40.0;
/// Closest a tooltip may come to the window edge.
const EDGE_MARGIN: f32 = 8.0;
/// Size assumed before the tooltip has been laid out once.
const SIZE_BEFORE_LAYOUT: Vec2 = Vec2::new(120.0, 28.0);
const PADDING: Vec2 = Vec2::new(8.0, 5.0);
const CONTENT_GAP: f32 = 8.0;
const HOTKEY_PADDING: Vec2 = Vec2::new(5.0, 2.0);
/// The width of the tooltip's and the badge's borders, in pixels.
const BORDER_WIDTH: f32 = 1.0;

/// Theme tokens for the tooltip and its hotkey badge.
#[derive(Component, Clone, PartialEq, Debug)]
#[component(immutable)]
pub struct TooltipTokens {
    /// The tooltip's background.
    pub bg: ThemeToken,
    /// The lit edge of the tooltip's bevel.
    pub border_light: ThemeToken,
    /// The shaded edge of the tooltip's bevel.
    pub border_dark: ThemeToken,
    /// The tooltip's text.
    pub text: ThemeToken,
    /// The tooltip's font size.
    pub font_size: ThemeToken,
    /// The font of the tooltip's text and hotkey.
    pub font: ThemeToken,
    /// The radius of the tooltip's and the badge's corners.
    pub corner_radius: ThemeToken,
    /// The hotkey badge's background.
    pub hotkey_bg: ThemeToken,
    /// The hotkey badge's border and text.
    pub hotkey_accent: ThemeToken,
    /// The hotkey's font size.
    pub hotkey_font_size: ThemeToken,
}

impl TooltipTokens {
    /// Names the tokens under `prefix`.
    pub fn with_prefix(prefix: &str) -> Self {
        let token = |suffix: &str| ThemeToken::new(format!("{prefix}.{suffix}"));
        Self {
            bg: token("bg"),
            border_light: token("border.light"),
            border_dark: token("border.dark"),
            text: token("text"),
            font_size: token("font_size"),
            font: token("font"),
            corner_radius: token("corner_radius"),
            hotkey_bg: token("hotkey.bg"),
            hotkey_accent: token("hotkey.accent"),
            hotkey_font_size: token("hotkey.font_size"),
        }
    }

    /// Register the default values for the tooltip tokens, unless the app sets them.
    pub fn register_theme_defaults(&self, theme: &mut Theme) {
        theme
            .default_derived(self.bg.clone(), SURFACE, PANEL_OPACITY)
            .default_derived(self.border_light.clone(), EDGE, Shade::Same)
            .default_derived(self.border_dark.clone(), SURFACE, Shade::Same)
            .default_derived(self.text.clone(), TEXT, Shade::Same)
            .default_derived(self.hotkey_bg.clone(), CONTROL, PANEL_OPACITY)
            .default_derived(self.hotkey_accent.clone(), HIGHLIGHT, Shade::Same)
            .default_size_from(self.font_size.clone(), SMALL_SIZE)
            .default_size_from(self.hotkey_font_size.clone(), SMALL_SIZE)
            .default_font_from(self.font.clone(), FONT)
            .default_size_from(self.corner_radius.clone(), CONTROL_CORNER_RADIUS);
        // Each part draws its role's skin, once a theme gives the role one.
        theme.default_skin_from(self.bg.clone(), skins::SURFACE);
    }
}

/// The built-in token set.
impl Default for TooltipTokens {
    fn default() -> Self {
        Self::with_prefix(TOOLTIP_STYLE)
    }
}

/// Describes the entity it's on. `hotkey` is shown in a badge, e.g. `"Space"`.
#[derive(Component, Clone, Debug, PartialEq, Eq, Reflect)]
#[reflect(Component, Debug)]
pub struct Tooltip {
    /// What the tooltip says.
    pub text: String,
    /// The key that does the same thing, if any, shown in a badge.
    pub hotkey: Option<String>,
}

impl Tooltip {
    /// A tooltip saying `text`.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            hotkey: None,
        }
    }

    /// Shows `hotkey` in a badge beside the text.
    pub fn hotkey(mut self, hotkey: impl Into<String>) -> Self {
        self.hotkey = Some(hotkey.into());
        self
    }
}

/// The shared tooltip's parts.
#[derive(Component, Clone, Copy, Debug)]
struct TooltipParts {
    text: Entity,
    badge: Entity,
    hotkey: Entity,
}

/// Shared tooltip, theme defaults, and the system that shows and places it.
/// The tooltip appears once a [`FeedbackLayerRoot`] exists.
pub struct TooltipPlugin;

impl Plugin for TooltipPlugin {
    fn build(&self, app: &mut App) {
        app.ensure_plugin(LayersPlugin)
            .ensure_plugin(ThemePlugin)
            .register_type::<Tooltip>()
            .init_resource::<ActiveTooltip>()
            .add_observer(spawn_tooltip_root)
            .add_observer(style_tooltip)
            .add_systems(
                PostUpdate,
                (choose_tooltip_target, show_tooltip)
                    .chain()
                    .before(UiSystems::Prepare),
            );
        TooltipTokens::with_prefix(TOOLTIP_STYLE)
            .register_theme_defaults(&mut app.world_mut().resource_mut::<Theme>());
    }
}

/// Spawns the shared tooltip inside the Feedback layer as soon as that layer exists.
fn spawn_tooltip_root(layer: On<Add<FeedbackLayerRoot>>, mut commands: Commands) {
    let root = commands
        .spawn((
            ChildOf(layer.entity),
            Pickable::IGNORE,
            Node {
                position_type: PositionType::Absolute,
                display: Display::None,
                align_items: AlignItems::Center,
                column_gap: px(CONTENT_GAP),
                padding: UiRect::axes(px(PADDING.x), px(PADDING.y)),
                border: UiRect::all(px(BORDER_WIDTH)),
                ..default()
            },
        ))
        .id();
    let text = commands
        .spawn((ChildOf(root), Pickable::IGNORE, Text::default()))
        .id();
    let badge = commands
        .spawn((
            ChildOf(root),
            Pickable::IGNORE,
            Node {
                display: Display::None,
                padding: UiRect::axes(px(HOTKEY_PADDING.x), px(HOTKEY_PADDING.y)),
                border: UiRect::all(px(BORDER_WIDTH)),
                ..default()
            },
        ))
        .id();
    let hotkey = commands
        .spawn((ChildOf(badge), Pickable::IGNORE, Text::default()))
        .id();
    // Its colours and fonts come with its tokens, in `style_tooltip`.
    commands.entity(root).insert((
        TooltipParts {
            text,
            badge,
            hotkey,
        },
        TooltipTokens::with_prefix(TOOLTIP_STYLE),
    ));
}

/// Styles the shared tooltip when its tokens are set.
fn style_tooltip(
    insert: On<Insert<TooltipTokens>>,
    tooltips: Query<(&TooltipParts, &TooltipTokens)>,
    mut commands: Commands,
) {
    let Ok((parts, tokens)) = tooltips.get(insert.entity) else {
        return;
    };
    commands.entity(insert.entity).insert((
        ThemeSkin(tokens.bg.clone()),
        ThemeBorder::bevel(tokens.border_light.clone(), tokens.border_dark.clone()),
        ThemeCornerRadius(tokens.corner_radius.clone()),
    ));
    commands.entity(parts.text).insert((
        ThemeTextColor(tokens.text.clone()),
        ThemeFontSize(tokens.font_size.clone()),
        ThemeFont(tokens.font.clone()),
    ));
    commands.entity(parts.badge).insert((
        ThemeSkin(tokens.hotkey_bg.clone()),
        ThemeBorder::all(tokens.hotkey_accent.clone()),
        ThemeCornerRadius(tokens.corner_radius.clone()),
    ));
    commands.entity(parts.hotkey).insert((
        ThemeTextColor(tokens.hotkey_accent.clone()),
        ThemeFontSize(tokens.hotkey_font_size.clone()),
        ThemeFont(tokens.font.clone()),
    ));
}

/// What the tooltip describes this frame, and where, in UI pixels.
#[derive(Resource, Default, Clone, Copy, Debug, PartialEq)]
struct ActiveTooltip {
    target: Option<Entity>,
    anchor: Vec2,
    /// Centered under the anchor (focus), rather than hanging off it (cursor).
    centered: bool,
    /// The window's size in UI pixels.
    bounds: Vec2,
}

type Anchorable = (&'static ComputedNode, &'static UiGlobalTransform);

/// Picks the nearest hovered entity with a tooltip, or else the focused one.
fn choose_tooltip_target(
    windows: Query<&Window, With<PrimaryWindow>>,
    ui_scale: Option<Res<UiScale>>,
    hover_map: Option<Res<HoverMap>>,
    focus: Option<Res<InputFocus>>,
    described: Query<Anchorable, With<Tooltip>>,
    parents: Query<&ChildOf>,
    mut active: ResMut<ActiveTooltip>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let ui_scale = ui_scale.map_or(1.0, |scale| scale.0).max(0.1);
    let bounds = Vec2::new(window.width(), window.height()) / ui_scale;

    // Start from what the mouse is over and walk up, so a label inside a button
    // finds the button's tooltip.
    let hovered = hover_map
        .as_ref()
        .and_then(|map| map.get(&PointerId::Mouse))
        .and_then(|hits| {
            hits.keys().find_map(|&entity| {
                std::iter::once(entity)
                    .chain(parents.iter_ancestors(entity))
                    .find(|&e| described.contains(e))
            })
        });

    let next = if let Some(target) = hovered
        && let Some(cursor) = window.cursor_position()
    {
        ActiveTooltip {
            target: Some(target),
            anchor: cursor / ui_scale + CURSOR_OFFSET,
            centered: false,
            bounds,
        }
    } else if let Some(target) = focus.as_ref().and_then(|focus| focus.get())
        && let Ok((node, transform)) = described.get(target)
    {
        // `ComputedNode` and `UiGlobalTransform` are in physical pixels.
        let bottom_center = transform.translation + Vec2::new(0.0, node.size().y / 2.0);
        ActiveTooltip {
            target: Some(target),
            anchor: bottom_center * node.inverse_scale_factor() + Vec2::new(0.0, FOCUS_GAP),
            centered: true,
            bounds,
        }
    } else {
        ActiveTooltip::default()
    };
    active.set_if_neq(next);
}

/// Shows, fills and places the shared tooltip for the active target.
fn show_tooltip(
    active: Res<ActiveTooltip>,
    tooltips: Query<&Tooltip>,
    roots: Query<(Entity, &TooltipParts, &ComputedNode)>,
    mut nodes: Query<&mut Node>,
    mut texts: Query<&mut Text>,
) {
    let Ok((root, parts, computed)) = roots.single() else {
        return;
    };
    let Ok(mut root_node) = nodes.get_mut(root) else {
        return;
    };
    let Some(tooltip) = active.target.and_then(|target| tooltips.get(target).ok()) else {
        if root_node.display != Display::None {
            root_node.display = Display::None;
        }
        return;
    };

    let laid_out = computed.size() * computed.inverse_scale_factor();
    let size = if laid_out.x > 0.0 {
        laid_out
    } else {
        SIZE_BEFORE_LAYOUT
    };
    let position = place_tooltip(active.anchor, active.centered, size, active.bounds);
    if root_node.display != Display::Flex {
        root_node.display = Display::Flex;
    }
    if (root_node.left, root_node.top) != (px(position.x), px(position.y)) {
        root_node.left = px(position.x);
        root_node.top = px(position.y);
    }

    let badge_display = if tooltip.hotkey.is_some() {
        Display::Flex
    } else {
        Display::None
    };
    if let Ok(mut badge) = nodes.get_mut(parts.badge)
        && badge.display != badge_display
    {
        badge.display = badge_display;
    }
    let hotkey = tooltip.hotkey.clone().unwrap_or_default();
    for (part, wanted) in [(parts.text, &tooltip.text), (parts.hotkey, &hotkey)] {
        if let Ok(mut text) = texts.get_mut(part)
            && text.0 != *wanted
        {
            text.0.clone_from(wanted);
        }
    }
}

/// Top-left corner for a tooltip of `size` at `anchor`, kept inside `bounds`
/// and flipped above the anchor when there's no room below. All in UI pixels.
fn place_tooltip(anchor: Vec2, centered: bool, size: Vec2, bounds: Vec2) -> Vec2 {
    let mut position = anchor;
    if centered {
        position.x -= size.x / 2.0;
    }
    if position.y + size.y > bounds.y - EDGE_MARGIN {
        position.y = anchor.y - size.y - FLIP_GAP;
    }
    let max = (bounds - size - Vec2::splat(EDGE_MARGIN)).max(Vec2::splat(EDGE_MARGIN));
    position.clamp(Vec2::splat(EDGE_MARGIN), max)
}

#[cfg(test)]
mod tests {
    use bevy::picking::backend::HitData;

    use super::*;
    use crate::theme::ThemePlugin;

    #[test]
    fn tooltips_stay_on_screen_and_flip_above_near_the_bottom() {
        let bounds = Vec2::new(800.0, 600.0);
        let size = Vec2::new(200.0, 30.0);
        // Room below: hangs off the anchor unchanged.
        assert_eq!(
            place_tooltip(Vec2::new(100.0, 100.0), false, size, bounds),
            Vec2::new(100.0, 100.0)
        );
        // At the right edge: pulled back inside the margin.
        assert_eq!(
            place_tooltip(Vec2::new(790.0, 100.0), false, size, bounds).x,
            800.0 - 200.0 - EDGE_MARGIN
        );
        // At the bottom: flipped above the anchor.
        assert_eq!(
            place_tooltip(Vec2::new(100.0, 590.0), false, size, bounds).y,
            590.0 - 30.0 - FLIP_GAP
        );
    }

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, ThemePlugin, TooltipPlugin))
            .init_resource::<HoverMap>();
        let mut window = Window::default();
        window.set_cursor_position(Some(Vec2::new(50.0, 50.0)));
        app.world_mut().spawn((window, PrimaryWindow));
        app
    }

    fn parts(app: &mut App) -> (Entity, TooltipParts) {
        let world = app.world_mut();
        let (root, parts) = world
            .query::<(Entity, &TooltipParts)>()
            .single(world)
            .unwrap();
        (root, *parts)
    }

    #[test]
    fn hovering_a_child_shows_its_ancestors_tooltip() {
        let mut app = app();
        let button = app
            .world_mut()
            .spawn((Node::default(), Tooltip::new("Play").hotkey("Space")))
            .id();
        let label = app
            .world_mut()
            .spawn((Text::new("Play label"), ChildOf(button)))
            .id();
        app.update();

        let hit = HitData::new(Entity::PLACEHOLDER, 0.0, None, None);
        app.world_mut()
            .resource_mut::<HoverMap>()
            .insert(PointerId::Mouse, [(label, hit)].into_iter().collect());
        app.update();

        let (root, parts) = parts(&mut app);
        let world = app.world();
        assert_eq!(world.get::<Node>(root).unwrap().display, Display::Flex);
        assert_eq!(world.get::<Text>(parts.text).unwrap().0, "Play");
        assert_eq!(world.get::<Text>(parts.hotkey).unwrap().0, "Space");
        assert_eq!(
            world.get::<Node>(parts.badge).unwrap().display,
            Display::Flex
        );
        // Only the tooltip's own text changes, never other text in the app.
        assert_eq!(world.get::<Text>(label).unwrap().0, "Play label");

        app.world_mut().resource_mut::<HoverMap>().clear();
        app.update();
        assert_eq!(
            app.world().get::<Node>(root).unwrap().display,
            Display::None
        );
    }

    #[test]
    fn new_tokens_restyle_the_tooltip_at_once() {
        let mut app = app();
        app.update();
        let (root, parts) = parts(&mut app);
        let custom = TooltipTokens::with_prefix("test.tooltip");
        custom.register_theme_defaults(&mut app.world_mut().resource_mut::<Theme>());
        app.world_mut().entity_mut(root).insert(custom.clone());
        app.world_mut().flush();
        let world = app.world();
        assert_eq!(world.get::<ThemeSkin>(root).unwrap().0, custom.bg);
        assert_eq!(
            world.get::<ThemeTextColor>(parts.hotkey),
            Some(&ThemeTextColor(custom.hotkey_accent))
        );
    }
}
