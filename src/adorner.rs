//! Adornments drawn around the UI rather than as part of it. For now, one:
//! the outline that shows where something being dragged would land.
//!
//! The outline is two nodes, kept loaded and hidden until they're needed:
//! a fill, drawn under the thing being dragged, and a line, drawn over it.
//! Showing them moves both into the dragged thing's parent, since a z-index
//! only orders siblings. Growing, the fill and line read as one shape around
//! the dragged thing; shrinking, the fill hides behind it and the line still
//! shows. The dragged thing takes [`ADORNED`] meanwhile, to sit between them.
//!
//! Only one outline shows at a time, as only one thing is dragged at a time.

use bevy::prelude::*;

use crate::EnsurePlugin;
use crate::theme::palette::HIGHLIGHT;
use crate::theme::{
    Shade, Theme, ThemeBorder, ThemeCornerRadius, ThemePlugin, ThemeSkin, ThemeToken,
};

/// The outline's fill.
const FILL: ThemeToken = ThemeToken::new_static("touchy.adorner.outline.fill");
/// The outline's line.
const LINE: ThemeToken = ThemeToken::new_static("touchy.adorner.outline.line");
/// The radius of the outline's corners, in pixels.
const CORNER_RADIUS: ThemeToken = ThemeToken::new_static("touchy.adorner.outline.corner_radius");

/// How opaque the fill is.
const FILL_ALPHA: f32 = 0.06;
/// How opaque the line is.
const LINE_ALPHA: f32 = 0.5;
/// The width of the line, in pixels.
const LINE_WIDTH: f32 = 2.0;
/// The corner radius of the outline, in pixels.
const DEFAULT_CORNER_RADIUS: f32 = 6.0;

/// Where the fill sits among the dragged thing's siblings.
const FILL_Z: ZIndex = ZIndex(i32::MAX - 2);
/// Where the line sits. Above everything.
const LINE_Z: ZIndex = ZIndex(i32::MAX);
/// Where the dragged thing sits while it's outlined: over the fill, under the
/// line, and above every other sibling.
pub(crate) const ADORNED: ZIndex = ZIndex(i32::MAX - 1);
/// The fill is under the dragged thing, which is under the line.
const _: () = assert!(FILL_Z.0 < ADORNED.0 && ADORNED.0 < LINE_Z.0);

/// Shows the outline at `rect`, in the space of `parent`'s inside: where its
/// absolutely placed children are measured from.
#[derive(Event, Clone, Copy, Debug, PartialEq)]
pub(crate) struct ShowOutline {
    pub parent: Entity,
    pub rect: Rect,
}

/// Hides the outline.
#[derive(Event, Clone, Copy, Debug, PartialEq)]
pub(crate) struct HideOutline;

/// The outline's two nodes, once they've been spawned.
#[derive(Resource, Default)]
struct Outline(Option<OutlineParts>);

#[derive(Clone, Copy)]
struct OutlineParts {
    fill: Entity,
    line: Entity,
}

/// One of the outline's nodes.
#[derive(Component)]
struct OutlinePart;

/// One of the outline's nodes, shown at `rect` in `parent`.
fn outline_part(parent: Entity, rect: Rect, z: ZIndex, border: UiRect) -> impl Bundle {
    let mut node = Node {
        position_type: PositionType::Absolute,
        border,
        ..default()
    };
    place(&mut node, rect);
    (
        OutlinePart,
        node,
        z,
        ThemeCornerRadius(CORNER_RADIUS),
        Pickable::IGNORE,
        ChildOf(parent),
    )
}

/// Moves the outline into the parent and shows it at the rect, spawning its
/// nodes the first time, or again if they went with a parent that was despawned.
fn show_outline(
    show: On<ShowOutline>,
    mut outline: ResMut<Outline>,
    mut parts: Query<(&mut Node, &ChildOf), With<OutlinePart>>,
    mut commands: Commands,
) {
    let ShowOutline { parent, rect } = *show;
    let loaded = outline
        .0
        .filter(|loaded| parts.contains(loaded.fill) && parts.contains(loaded.line));
    let Some(OutlineParts { fill, line }) = loaded else {
        let fill = commands
            .spawn((
                outline_part(parent, rect, FILL_Z, UiRect::ZERO),
                ThemeSkin(FILL),
            ))
            .id();
        let border = UiRect::all(px(LINE_WIDTH));
        let line = commands
            .spawn((
                outline_part(parent, rect, LINE_Z, border),
                ThemeBorder::all(LINE),
            ))
            .id();
        outline.0 = Some(OutlineParts { fill, line });
        return;
    };
    for part in [fill, line] {
        let Ok((mut node, child_of)) = parts.get_mut(part) else {
            continue;
        };
        place(&mut node, rect);
        if child_of.parent() != parent {
            commands.entity(part).insert(ChildOf(parent));
        }
    }
}

/// Shows `node` at `rect`.
fn place(node: &mut Node, rect: Rect) {
    node.display = Display::Flex;
    node.left = px(rect.min.x);
    node.top = px(rect.min.y);
    node.width = px(rect.width());
    node.height = px(rect.height());
}

fn hide_outline(
    _: On<HideOutline>,
    outline: Res<Outline>,
    mut parts: Query<&mut Node, With<OutlinePart>>,
) {
    let Some(OutlineParts { fill, line }) = outline.0 else {
        return;
    };
    for part in [fill, line] {
        if let Ok(mut node) = parts.get_mut(part)
            && node.display != Display::None
        {
            node.display = Display::None;
        }
    }
}

/// Adds the outline and its theme tokens.
pub(crate) struct AdornerPlugin;

impl Plugin for AdornerPlugin {
    fn build(&self, app: &mut App) {
        app.ensure_plugin(ThemePlugin)
            .init_resource::<Outline>()
            .add_observer(show_outline)
            .add_observer(hide_outline);
        app.world_mut()
            .resource_mut::<Theme>()
            .default_derived(FILL, HIGHLIGHT, Shade::Alpha(FILL_ALPHA))
            .default_derived(LINE, HIGHLIGHT, Shade::Alpha(LINE_ALPHA))
            .default_size(CORNER_RADIUS, DEFAULT_CORNER_RADIUS);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> (App, Entity) {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AdornerPlugin));
        let parent = app.world_mut().spawn(Node::default()).id();
        (app, parent)
    }

    fn show(app: &mut App, parent: Entity, rect: Rect) {
        app.world_mut().trigger(ShowOutline { parent, rect });
        app.update();
    }

    /// The outline's fill and line, as (parent, z, display, left, width) each.
    fn parts(app: &App) -> [(Entity, ZIndex, Display, Val, Val); 2] {
        let OutlineParts { fill, line } = app.world().resource::<Outline>().0.unwrap();
        [fill, line].map(|part| {
            let world = app.world();
            let node = world.get::<Node>(part).unwrap();
            (
                world.get::<ChildOf>(part).unwrap().parent(),
                *world.get::<ZIndex>(part).unwrap(),
                node.display,
                node.left,
                node.width,
            )
        })
    }

    #[test]
    fn the_outline_shows_in_the_parent_around_the_adorned_and_hides() {
        let (mut app, parent) = app();
        let rect = Rect::new(10.0, 20.0, 110.0, 70.0);
        show(&mut app, parent, rect);
        let [fill, line] = parts(&app);
        assert_eq!(fill, (parent, FILL_Z, Display::Flex, px(10.0), px(100.0)));
        assert_eq!(line, (parent, LINE_Z, Display::Flex, px(10.0), px(100.0)));

        app.world_mut().trigger(HideOutline);
        app.update();
        assert!(parts(&app).iter().all(|part| part.2 == Display::None));
    }

    #[test]
    fn the_outline_follows_the_drag_to_another_parent_and_outlives_one() {
        let (mut app, first) = app();
        let rect = Rect::new(0.0, 0.0, 50.0, 50.0);
        show(&mut app, first, rect);
        let before = app.world().resource::<Outline>().0.unwrap().fill;

        let second = app.world_mut().spawn(Node::default()).id();
        show(&mut app, second, rect);
        assert_eq!(parts(&app)[0].0, second, "moved, not spawned again");
        assert_eq!(app.world().resource::<Outline>().0.unwrap().fill, before);

        app.world_mut().entity_mut(second).despawn();
        show(&mut app, first, rect);
        assert_eq!(
            parts(&app)[0].0,
            first,
            "spawned again once its parent took it"
        );
    }
}
