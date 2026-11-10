//! Primitive UI elements, themed: text, rows and columns, cards, a divider and a spacer.
//!
//! Each is a single entity with no parts to keep track of, so each is a bundle, and goes wherever a
//! bundle does, as well as through [`SpawnUi`](crate::spawn_ui::SpawnUi):
//!
//! ```
//! # use bevy::prelude::*;
//! # use touchy::prelude::*;
//! # use touchy::layers::UiLayer;
//! fn pause_menu(mut commands: Commands) {
//!     commands.spawn((
//!         UiLayer::Overlay,
//!         Node { flex_direction: FlexDirection::Column, ..default() },
//!         children![heading("Paused"), Divider, text("Press Esc to resume"), small("Autosaved")],
//!     ));
//! }
//! ```
//!
//! Colours, font and sizes come from [`Palette`](crate::theme::palette), so a
//! theme restyles all at once. Looks are required components, so anything added
//! beside one replaces its default: `(text("Lost"), ThemeTextColor(palette::DANGER))`.

use bevy::prelude::*;

use bevy::ecs::lifecycle::HookContext;
use bevy::ecs::world::DeferredWorld;

use crate::theme::palette::{
    BULLET, CORNER_RADIUS, EDGE, FONT, HEADING_SIZE, SMALL_SIZE, TEXT, TEXT_MUTED, TEXT_SIZE,
};
use crate::theme::skins;
use crate::theme::{
    ThemeBorder, ThemeCornerRadius, ThemeFont, ThemeFontSize, ThemeSkin, ThemeTextColor,
};

/// How thick a divider is, in pixels.
const DIVIDER_THICKNESS: f32 = 1.0;
/// The room between a row's or column's children, in pixels.
const GAP: f32 = 8.0;
/// A card's room inside its edge, in pixels.
const CARD_PADDING: f32 = 12.0;
const CARD_BORDER: f32 = 1.0;
/// The room between a list's markers and its words, and between its items, in pixels.
const LIST_MARKER_GAP: f32 = 8.0;
const LIST_ITEM_GAP: f32 = 4.0;
/// A bullet's size, and the height of a line, in multiples of the text's size: the bullet is
/// centred on its item's first line, whatever size the theme gives text.
const BULLET_SIZE: f32 = 0.35;
const LINE_HEIGHT: f32 = 1.2;

/// Body text, in the palette's text colour, font and size.
pub fn text(words: impl Into<String>) -> impl Bundle {
    (Text::new(words), BodyText)
}

/// A heading: body text, larger.
pub fn heading(words: impl Into<String>) -> impl Bundle {
    (Text::new(words), HeadingText)
}

/// Small print: body text, smaller and muted.
pub fn small(words: impl Into<String>) -> impl Bundle {
    (Text::new(words), SmallText)
}

/// Marks body text, and gives it the palette's look unless the bundle gives
/// its own: what [`text`] spawns.
#[derive(Component, Clone, Copy, Debug, Default)]
#[require(
    ThemeTextColor = ThemeTextColor(TEXT),
    ThemeFontSize = ThemeFontSize(TEXT_SIZE),
    ThemeFont = ThemeFont(FONT)
)]
pub struct BodyText;

/// Marks a heading, and gives it the palette's look unless the bundle gives
/// its own: what [`heading`] spawns.
#[derive(Component, Clone, Copy, Debug, Default)]
#[require(
    ThemeTextColor = ThemeTextColor(TEXT),
    ThemeFontSize = ThemeFontSize(HEADING_SIZE),
    ThemeFont = ThemeFont(FONT)
)]
pub struct HeadingText;

/// Marks small print, and gives it the palette's look unless the bundle
/// gives its own: what [`small`] spawns.
#[derive(Component, Clone, Copy, Debug, Default)]
#[require(
    ThemeTextColor = ThemeTextColor(TEXT_MUTED),
    ThemeFontSize = ThemeFontSize(SMALL_SIZE),
    ThemeFont = ThemeFont(FONT)
)]
pub struct SmallText;

/// A thin line across its parent, in the palette's edge colour: a rule
/// between the rows of a column, or between the items of a row. It takes its
/// direction from its parent, so it works in either.
#[derive(Component, Clone, Copy, Debug, Default)]
#[require(Node = divider_node(), ThemeSkin = ThemeSkin(EDGE))]
pub struct Divider;

impl Divider {
    /// `margin` around the divider.
    pub fn with_margin(margin: UiRect) -> impl Bundle {
        let node = Node {
            margin,
            ..divider_node()
        };
        (Divider, node)
    }
}

/// Thickness along the parent's direction, stretched across it.
fn divider_node() -> Node {
    Node {
        flex_basis: px(DIVIDER_THICKNESS),
        flex_shrink: 0.0,
        align_self: AlignSelf::Stretch,
        ..default()
    }
}

/// Lays its children out left to right, a gap apart, in its parent's layout. Change anything
/// else about it with [`node`](crate::spawn_ui::NodePatch::node).
#[derive(Component, Clone, Copy, Debug, Default)]
#[require(Node = flow_node(FlexDirection::Row))]
pub struct Row;

/// Lays its children out top to bottom, a gap apart, in its parent's layout. Change anything
/// else about it with [`node`](crate::spawn_ui::NodePatch::node).
#[derive(Component, Clone, Copy, Debug, Default)]
#[require(Node = flow_node(FlexDirection::Column))]
pub struct Column;

/// Children running `direction`, a gap apart.
fn flow_node(direction: FlexDirection) -> Node {
    Node {
        flex_direction: direction,
        row_gap: px(GAP),
        column_gap: px(GAP),
        ..default()
    }
}

/// A card's look on any node: the surface skin role, an edge, rounded corners, and room
/// inside. A binding or an edge or room the node already has stands, as with the other
/// primitives: `(Column, Card, ThemeSkin(my_token))`.
#[derive(Component, Clone, Copy, Debug, Default)]
#[component(on_add = edge_a_card)]
pub struct Card;

/// Gives a new card whatever of its look the node lacks. Done here rather than as required
/// components to avoid overwriting an existing `Node`.
fn edge_a_card(mut world: DeferredWorld, context: HookContext) {
    let entity = context.entity;
    world.commands().queue(move |world: &mut World| {
        let Ok(mut card) = world.get_entity_mut(entity) else {
            return;
        };
        if !card.contains::<ThemeSkin>() {
            card.insert(ThemeSkin(skins::SURFACE));
        }
        if !card.contains::<ThemeBorder>() {
            card.insert(ThemeBorder::all(EDGE));
        }
        if !card.contains::<ThemeCornerRadius>() {
            card.insert(ThemeCornerRadius(CORNER_RADIUS));
        }
        let Some(mut node) = card.get_mut::<Node>() else {
            return;
        };
        if node.border == UiRect::DEFAULT {
            node.border = UiRect::all(px(CARD_BORDER));
        }
        if node.padding == UiRect::DEFAULT {
            node.padding = UiRect::all(px(CARD_PADDING));
        }
    });
}

/// Keeps a control from being stretched across its parent, while following any other alignment
/// the parent sets ('start' in a stretching column, 'centered' in a centered toolbar).
#[derive(Component, Clone, Copy, Debug, Default)]
#[component(on_add = unstretch)]
#[require(Node)]
pub struct Unstretched;

/// Only a parent that would stretch its children (flexbox's default) is overruled.
fn unstretch(mut world: DeferredWorld, context: HookContext) {
    let parent = world.get::<ChildOf>(context.entity).map(ChildOf::parent);
    let stretches = parent
        .and_then(|parent| world.get::<Node>(parent))
        .is_none_or(|node| matches!(node.align_items, AlignItems::Default | AlignItems::Stretch));
    if stretches && let Some(mut node) = world.get_mut::<Node>(context.entity) {
        node.align_self = AlignSelf::Start;
    }
}

/// A list: [`bullets`](crate::spawn_ui::SpawnUi::bullets) or
/// [`numbered`](crate::spawn_ui::SpawnUi::numbered). Its markers sit in a column as wide as
/// the widest, and each item's words wrap under themselves.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct List;

/// A list's marker: a bullet, or an item's number. Its appearance is the [`BULLET`] token.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct ListMarker;

/// Spawns `items` as a list, bulleted or numbered from 1.
pub(crate) fn spawn_list<'a>(
    parent: &'a mut ChildSpawnerCommands,
    items: impl IntoIterator<Item = impl Into<String>>,
    numbered: bool,
) -> EntityCommands<'a> {
    let grid = Node {
        display: Display::Grid,
        grid_template_columns: vec![RepeatedGridTrack::auto(1), RepeatedGridTrack::flex(1, 1.0)],
        column_gap: px(LIST_MARKER_GAP),
        row_gap: px(LIST_ITEM_GAP),
        ..default()
    };
    let mut list = parent.spawn((List, grid));
    list.with_children(|list| {
        for (index, words) in items.into_iter().enumerate() {
            if numbered {
                let number = Node {
                    justify_self: JustifySelf::End,
                    ..default()
                };
                let marker = (text(format!("{}.", index + 1)), ThemeTextColor(BULLET));
                list.spawn((ListMarker, marker, number));
            } else {
                list.spawn(bullet());
            }
            list.spawn(text(words));
        }
    });
    list
}

/// A dot, centered on its item's first line. Drawn not typed: Bevy's default font has no bullet
/// character. Sized from the text's size.
fn bullet() -> impl Bundle {
    let node = Node {
        width: Val::Em(BULLET_SIZE),
        height: Val::Em(BULLET_SIZE),
        margin: UiRect::top(Val::Em((LINE_HEIGHT - BULLET_SIZE) / 2.0)),
        justify_self: JustifySelf::Center,
        border_radius: BorderRadius::all(percent(50)),
        ..default()
    };
    (
        ListMarker,
        node,
        ThemeSkin(BULLET),
        ThemeFontSize(TEXT_SIZE),
    )
}

/// Empty room that takes up whatever its parent has spare, pushing what
/// follows it to the far end: a toolbar's last buttons to its right, say.
#[derive(Component, Clone, Copy, Debug, Default)]
#[require(Node = Node { flex_grow: 1.0, ..default() })]
pub struct Spacer;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::palette::{DANGER, HEADING_SCALE, SMALL_SCALE};
    use crate::theme::{Theme, ThemePlugin};
    use bevy::text::FontSize;

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, ThemePlugin));
        app
    }

    /// Text takes the palette's size, and headings and small print follow
    /// it, so one entry resizes all three.
    #[test]
    fn text_sizes_follow_the_palette() {
        let mut app = app();
        app.world_mut()
            .resource_mut::<Theme>()
            .set_size(TEXT_SIZE, 20.0);
        let body = app.world_mut().spawn(text("a")).id();
        let title = app.world_mut().spawn(heading("b")).id();
        let print = app.world_mut().spawn(small("c")).id();
        app.update();
        let size = |entity| app.world().get::<TextFont>(entity).unwrap().font_size;
        assert_eq!(size(body), FontSize::Px(20.0));
        assert_eq!(size(title), FontSize::Px(20.0 * HEADING_SCALE));
        assert_eq!(size(print), FontSize::Px(20.0 * SMALL_SCALE));
        let colour = |entity| app.world().get::<TextColor>(entity).unwrap().0;
        let theme = app.world().resource::<Theme>();
        assert_eq!(colour(print), theme.color(&TEXT_MUTED));
    }

    /// A divider is a line across a column, or down a row.
    #[test]
    fn a_divider_crosses_its_parent_either_way() {
        let mut app = app();
        let divider = app.world_mut().spawn(Divider).id();
        let node = app.world().get::<Node>(divider).unwrap();
        assert_eq!(node.flex_basis, px(DIVIDER_THICKNESS));
        assert_eq!(node.align_self, AlignSelf::Stretch);
        assert_eq!(
            app.world().get::<ThemeSkin>(divider),
            Some(&ThemeSkin(EDGE))
        );
    }

    #[test]
    fn a_divider_can_keep_a_margin() {
        let mut app = app();
        let margin = UiRect::axes(px(8.0), px(4.0));
        let divider = app.world_mut().spawn(Divider::with_margin(margin)).id();
        let node = app.world().get::<Node>(divider).unwrap();
        assert_eq!(node.margin, margin);
        assert_eq!(node.flex_basis, px(DIVIDER_THICKNESS), "still a divider");
    }

    /// Rows and columns run their way, a gap apart.
    #[test]
    fn rows_and_columns_run_their_way_a_gap_apart() {
        let mut app = app();
        let row = app.world_mut().spawn(Row).id();
        let column = app.world_mut().spawn(Column).id();
        let node = |entity| app.world().get::<Node>(entity).unwrap().clone();
        assert_eq!(node(row).flex_direction, FlexDirection::Row);
        assert_eq!(node(column).flex_direction, FlexDirection::Column);
        assert_eq!(
            (node(row).row_gap, node(row).column_gap),
            (px(GAP), px(GAP))
        );
    }

    /// A card gives any node its look, and an edge and room inside unless it has its own.
    #[test]
    fn a_card_gives_a_node_its_look() {
        let mut app = app();
        let card = app.world_mut().spawn((Column, Card)).id();
        app.update();
        let node = app.world().get::<Node>(card).unwrap();
        assert_eq!(node.flex_direction, FlexDirection::Column, "still a column");
        assert_eq!(node.padding, UiRect::all(px(CARD_PADDING)));
        assert_eq!(node.border, UiRect::all(px(CARD_BORDER)));
        let skin = app.world().get::<ThemeSkin>(card);
        assert_eq!(skin, Some(&ThemeSkin(skins::SURFACE)));

        let tight = Node {
            padding: UiRect::all(px(2.0)),
            ..default()
        };
        let card = app.world_mut().spawn((tight, Card)).id();
        let node = app.world().get::<Node>(card).unwrap();
        assert_eq!(node.padding, UiRect::all(px(2.0)), "its own room stands");

        // A binding beside it stands.
        let danger = app
            .world_mut()
            .spawn((Column, Card, ThemeSkin(DANGER)))
            .id();
        app.update();
        let skin = app.world().get::<ThemeSkin>(danger);
        assert_eq!(skin, Some(&ThemeSkin(DANGER)));

        // Either way round in a bundle, a column card is still a column.
        let card = app.world_mut().spawn((Card, Column)).id();
        app.update();
        let node = app.world().get::<Node>(card).unwrap();
        assert_eq!(node.flex_direction, FlexDirection::Column);
    }

    /// A control keeps its own size rather than being stretched by a parent, and follows a
    /// parent that aligns its children some other way.
    #[test]
    fn an_unstretched_control_follows_any_alignment_but_stretching() {
        let mut app = app();
        let column = app.world_mut().spawn(Column).id();
        let in_column = app.world_mut().spawn((Unstretched, ChildOf(column))).id();
        let centred = Node {
            align_items: AlignItems::Center,
            ..default()
        };
        let bar = app.world_mut().spawn(centred).id();
        let in_bar = app.world_mut().spawn((Unstretched, ChildOf(bar))).id();
        let align = |entity| app.world().get::<Node>(entity).unwrap().align_self;
        assert_eq!(align(in_column), AlignSelf::Start);
        assert_eq!(
            align(in_bar),
            AlignSelf::Auto,
            "centred with the bar's items"
        );
    }

    /// A list pairs each item with its marker: a dot, or its number.
    #[test]
    fn a_list_pairs_each_item_with_its_marker() {
        let mut app = app();
        let mut lists = [Entity::PLACEHOLDER; 2];
        app.world_mut()
            .commands()
            .spawn(Node::default())
            .with_children(|page| {
                lists[0] = spawn_list(page, ["Food", "Production"], false).id();
                lists[1] = spawn_list(page, ["Settle", "Explore", "Expand"], true).id();
            });
        app.update();
        let children = |list: Entity| app.world().get::<Children>(list).unwrap().to_vec();
        let bullets = children(lists[0]);
        assert_eq!(bullets.len(), 4, "a marker and words for each item");
        assert!(app.world().get::<ListMarker>(bullets[0]).is_some());
        assert_eq!(
            app.world().get::<ThemeSkin>(bullets[0]),
            Some(&ThemeSkin(BULLET))
        );
        let numbered = children(lists[1]);
        let words = |entity| app.world().get::<Text>(entity).unwrap().0.clone();
        assert_eq!(words(numbered[4]), "3.");
        assert_eq!(words(numbered[5]), "Expand");
    }

    /// Anything beside a primitive replaces its default.
    #[test]
    fn a_binding_beside_text_replaces_its_default() {
        let mut app = app();
        let lost = app
            .world_mut()
            .spawn((text("Lost"), ThemeTextColor(DANGER)))
            .id();
        app.update();
        let theme = app.world().resource::<Theme>();
        let danger = theme.color(&DANGER);
        assert_eq!(app.world().get::<TextColor>(lost).unwrap().0, danger);
    }
}
