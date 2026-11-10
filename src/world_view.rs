//! Views of the game world, and where the pointer is in them.
//!
//! A world is shown one of two ways: by a camera drawing straight to the
//! window, with the UI on top, or by a camera drawing offscreen into a UI
//! `ViewportNode` (see [`viewport_render_target`]). Either way, put a
//! [`CursorPlane`] on the camera, and touchy finds where the pointer meets that
//! plane: [`WorldPointer`] while hovering, [`WorldPress`] on a press. Both
//! exist only while no UI covers the world, so a click on a panel, or on a menu
//! hanging over the world, never reaches it.
//!
//! Bevy's picking decides what's covered: a UI node blocks what's under it
//! unless its `Pickable` says otherwise, and a viewport node passes the pointer
//! on into its camera's world. A press on empty space reaches the world through
//! the window itself, which Bevy picks behind everything else by default.
//!
//! Entities inside a viewport can also set the cursor icon with `EntityCursor`.

use bevy::camera::RenderTarget;
use bevy::picking::PickingSystems;
use bevy::picking::backend::ray::{RayId, RayMap};
use bevy::picking::cursor::{CursorIconPlugin, EntityCursor};
use bevy::picking::hover::HoverMap;
use bevy::picking::pointer::{PointerAction, PointerButton, PointerId, PointerInput};
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;

use crate::ensure_plugin;

/// Pixel format of viewport textures.
const TARGET_FORMAT: TextureFormat = TextureFormat::Bgra8UnormSrgb;

/// Put on a camera to make what it shows a world the pointer can be in: the
/// pointer's ray through the camera is met with this plane. Cameras without
/// it, such as an object inspector's, are never worlds.
#[derive(Component, Clone, Copy, Debug)]
pub struct CursorPlane {
    /// The plane's origin, in world space.
    pub origin: Vec3,
    /// The plane's normal, in world space.
    pub plane: InfinitePlane3d,
}

impl CursorPlane {
    /// The XY plane through the world origin, facing +Z: the plane of a 2D world.
    pub const XY: Self = Self {
        origin: Vec3::ZERO,
        plane: InfinitePlane3d { normal: Dir3::Z },
    };
}

/// A point in a world, and the camera it was seen through.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorldPoint {
    /// The camera Entity that was used to find this point.
    pub camera: Entity,
    /// The point in world space.
    pub point: Vec3,
}

/// Where the mouse is in the world, or `None` while it's over UI or no world.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq)]
pub struct WorldPointer(pub Option<WorldPoint>);

impl WorldPointer {
    /// Where the mouse is in the world, if it's over one.
    pub fn point(&self) -> Option<Vec3> {
        self.0.map(|hovered| hovered.point)
    }
}

/// A press that landed on the world rather than on UI. Observe it to pick
/// things in a world that isn't made of pickable entities.
#[derive(Event, Clone, Copy, Debug, PartialEq)]
pub struct WorldPress {
    /// Where the press met the camera's [`CursorPlane`].
    pub point: Vec3,
    /// The camera Entity that was used to find this point.
    pub camera: Entity,
    /// The button associated with this press.
    pub button: PointerButton,
    /// The mouse, or a touch.
    pub pointer: PointerId,
}

/// The cursor to show over a viewport when nothing inside it sets one.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct ViewportCursor(pub EntityCursor);

/// World pointing, and cursors inside viewports.
pub struct WorldViewPlugin;

impl Plugin for WorldViewPlugin {
    fn build(&self, app: &mut App) {
        ensure_plugin(app, CursorIconPlugin);
        app.init_resource::<WorldPointer>().add_systems(
            PreUpdate,
            (
                (update_world_pointer, announce_world_presses),
                mirror_viewport_cursors,
            )
                .in_set(PickingSystems::PostHover)
                .before(PickingSystems::Last),
        );
    }
}

/// A render target for a camera that will be shown in a `ViewportNode`.
pub fn viewport_render_target(images: &mut Assets<Image>) -> RenderTarget {
    // 1×1 placeholder: `ViewportNode` resizes it to match the node.
    let texture = Image::new_target_texture(1, 1, TARGET_FORMAT, None);
    RenderTarget::Image(images.add(texture).into())
}

/// What finding a pointer's place in the world reads.
#[derive(bevy::ecs::system::SystemParam)]
struct WorldLookup<'w, 's> {
    hover_map: Res<'w, HoverMap>,
    rays: Res<'w, RayMap>,
    viewports: Query<'w, 's, (&'static ViewportNode, &'static PointerId)>,
    planes: Query<'w, 's, (&'static CursorPlane, Option<&'static Camera>)>,
    ui_nodes: Query<'w, 's, Option<&'static Pickable>, With<Node>>,
}

impl WorldLookup<'_, '_> {
    /// Where `pointer` is in a world, if it's over one and no UI covers it.
    ///
    /// Bevy's hover map holds what's under the pointer down to the first thing
    /// that blocks, so: over a viewport, the answer is in that viewport's world,
    /// through the viewport's own pointer; over any other UI that blocks, there
    /// is none; otherwise it's in the world behind the UI, through whichever
    /// window camera with a plane is frontmost there.
    fn find(&self, pointer: PointerId) -> Option<WorldPoint> {
        // A pointer over nothing pickable has no entry at all: nothing covers it.
        let hovered = || {
            self.hover_map
                .get(&pointer)
                .into_iter()
                .flat_map(|hits| hits.keys().copied())
        };
        if let Some((viewport, &inner)) = hovered().find_map(|e| self.viewports.get(e).ok()) {
            return self.meet(viewport.camera?, inner);
        }
        let covered = hovered().any(|entity| {
            self.ui_nodes
                .get(entity)
                .is_ok_and(|pickable| pickable.is_none_or(|p| p.should_block_lower))
        });
        if covered {
            return None;
        }
        // Bevy casts the pointer's ray through every camera drawing where it is.
        let (frontmost, _) = self
            .rays
            .map
            .keys()
            .filter(|ray| ray.pointer == pointer)
            .filter_map(|ray| {
                let (_, camera) = self.planes.get(ray.camera).ok()?;
                Some((ray.camera, camera.map_or(0, |camera| camera.order)))
            })
            .max_by_key(|&(_, order)| order)?;
        self.meet(frontmost, pointer)
    }

    /// Where `pointer`'s ray through `camera` meets the camera's plane.
    fn meet(&self, camera: Entity, pointer: PointerId) -> Option<WorldPoint> {
        let (plane, _) = self.planes.get(camera).ok()?;
        let ray = self.rays.map.get(&RayId::new(camera, pointer))?;
        let point = ray.plane_intersection_point(plane.origin, plane.plane)?;
        Some(WorldPoint { camera, point })
    }
}

fn update_world_pointer(lookup: WorldLookup, mut pointer: ResMut<WorldPointer>) {
    pointer.set_if_neq(WorldPointer(lookup.find(PointerId::Mouse)));
}

/// Announces each press that lands on a world. A viewport's own pointer
/// repeats the presses made over it; those are answered through the pointer
/// that made them, so each press is announced once.
fn announce_world_presses(
    lookup: WorldLookup,
    mut inputs: MessageReader<PointerInput>,
    mut commands: Commands,
) {
    for input in inputs.read() {
        let PointerAction::Press(button) = input.action else {
            continue;
        };
        let repeated = lookup
            .viewports
            .iter()
            .any(|(_, &inner)| inner == input.pointer_id);
        if repeated {
            continue;
        }
        if let Some(WorldPoint { camera, point }) = lookup.find(input.pointer_id) {
            commands.trigger(WorldPress {
                point,
                camera,
                button,
                pointer: input.pointer_id,
            });
        }
    }
}

/// A viewport node, its picking pointer, and its current and fallback cursors.
type ViewportCursors = (
    Entity,
    &'static PointerId,
    Option<&'static EntityCursor>,
    Option<&'static ViewportCursor>,
);

/// Bevy's cursor plugin only follows the mouse pointer, but entities inside a
/// viewport are hovered by that viewport's own pointer, so their `EntityCursor`
/// would be ignored. This copies the cursor of the nearest hovered entity inside
/// each viewport onto the viewport node itself, which the mouse does hover.
/// It owns the node's `EntityCursor`; set [`ViewportCursor`] for a fallback.
fn mirror_viewport_cursors(
    viewports: Query<ViewportCursors, With<ViewportNode>>,
    cursors: Query<&EntityCursor>,
    parents: Query<&ChildOf>,
    hover_map: Res<HoverMap>,
    mut commands: Commands,
) {
    for (viewport, pointer, current, fallback) in &viewports {
        let hovered = hover_map.get(pointer).and_then(|hits| {
            hits.iter()
                .filter_map(|(&entity, hit)| {
                    let cursor = cursors.get(entity).ok().or_else(|| {
                        parents
                            .iter_ancestors(entity)
                            .find_map(|a| cursors.get(a).ok())
                    })?;
                    Some((hit.depth, cursor))
                })
                .min_by(|(a, _), (b, _)| a.total_cmp(b))
                .map(|(_, cursor)| cursor)
        });
        let wanted = hovered.or(fallback.map(|f| &f.0));
        match wanted {
            Some(cursor) if current != Some(cursor) => {
                commands.entity(viewport).insert(cursor.clone());
            }
            None if current.is_some() => {
                commands.entity(viewport).remove::<EntityCursor>();
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use bevy::camera::NormalizedRenderTarget;
    use bevy::picking::backend::HitData;
    use bevy::picking::pointer::{Location, PointerMap};
    use bevy::window::{CursorIcon, SystemCursorIcon};

    use super::*;

    /// Where the test rays meet the XY plane.
    const SPOT: Vec3 = Vec3::new(5.0, 7.0, 0.0);

    #[derive(Resource, Default)]
    struct Presses(Vec<WorldPress>);

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, WorldViewPlugin))
            .init_resource::<HoverMap>()
            .init_resource::<RayMap>()
            .init_resource::<PointerMap>()
            .init_resource::<Presses>()
            .add_message::<PointerInput>()
            .add_observer(|press: On<WorldPress>, mut presses: ResMut<Presses>| {
                presses.0.push(*press);
            });
        app
    }

    fn hit() -> HitData {
        HitData::new(Entity::PLACEHOLDER, 0.0, None, None)
    }

    /// Makes `entities` what `pointer` hovers.
    fn hover(app: &mut App, pointer: PointerId, entities: &[Entity]) {
        let hits = entities.iter().map(|&entity| (entity, hit())).collect();
        app.world_mut()
            .resource_mut::<HoverMap>()
            .insert(pointer, hits);
    }

    /// Casts `pointer`'s ray through `camera`, straight down onto `at`.
    fn cast(app: &mut App, camera: Entity, pointer: PointerId, at: Vec3) {
        let ray = Ray3d::new(at + Vec3::Z * 10.0, Dir3::NEG_Z);
        app.world_mut()
            .resource_mut::<RayMap>()
            .map
            .insert(RayId::new(camera, pointer), ray);
    }

    fn pointed(app: &mut App) -> Option<WorldPoint> {
        app.update();
        app.world().resource::<WorldPointer>().0
    }

    fn press(app: &mut App, pointer: PointerId) -> Vec<WorldPress> {
        let location = Location {
            target: NormalizedRenderTarget::None {
                width: 1,
                height: 1,
            },
            position: Vec2::ZERO,
        };
        app.world_mut().write_message(PointerInput::new(
            pointer,
            location,
            PointerAction::Press(PointerButton::Primary),
        ));
        app.update();
        std::mem::take(&mut app.world_mut().resource_mut::<Presses>().0)
    }

    #[test]
    fn the_world_behind_the_ui_is_pointed_at_only_where_no_ui_covers_it() {
        let mut app = app();
        let camera = app.world_mut().spawn(CursorPlane::XY).id();
        let window = app.world_mut().spawn(Window::default()).id();
        let panel = app.world_mut().spawn(Node::default()).id();
        let see_through = app
            .world_mut()
            .spawn((Node::default(), Pickable::IGNORE))
            .id();
        cast(&mut app, camera, PointerId::Mouse, SPOT);

        let expected = Some(WorldPoint {
            camera,
            point: SPOT,
        });
        // With no window to pick, as when drawing to an image, Bevy's hover
        // map has no entry for a pointer over empty space.
        assert_eq!(pointed(&mut app), expected, "over nothing pickable");
        hover(&mut app, PointerId::Mouse, &[window]);
        assert_eq!(pointed(&mut app), expected, "over empty space");
        hover(&mut app, PointerId::Mouse, &[panel]);
        assert_eq!(pointed(&mut app), None, "a panel covers the world");
        hover(&mut app, PointerId::Mouse, &[see_through, window]);
        assert_eq!(pointed(&mut app), expected, "UI that lets clicks through");
    }

    #[test]
    fn over_a_viewport_the_pointer_is_in_the_viewports_world() {
        let mut app = app();
        let behind = app.world_mut().spawn(CursorPlane::XY).id();
        let shown = app.world_mut().spawn(CursorPlane::XY).id();
        let viewport = app.world_mut().spawn(ViewportNode::new(shown)).id();
        let inner = *app.world().get::<PointerId>(viewport).unwrap();
        // The mouse's ray through the window camera lands elsewhere, but the
        // viewport covers that world.
        cast(&mut app, behind, PointerId::Mouse, Vec3::ZERO);
        cast(&mut app, shown, inner, SPOT);
        hover(&mut app, PointerId::Mouse, &[viewport]);
        let expected = Some(WorldPoint {
            camera: shown,
            point: SPOT,
        });
        assert_eq!(pointed(&mut app), expected);

        // A viewport showing something that isn't a world, like an object
        // inspector, is no world, and still covers the one behind it.
        let inspector = app.world_mut().spawn_empty().id();
        app.world_mut()
            .entity_mut(viewport)
            .insert(ViewportNode::new(inspector));
        cast(&mut app, inspector, inner, SPOT);
        assert_eq!(pointed(&mut app), None);
    }

    #[test]
    fn the_frontmost_window_camera_is_the_one_pointed_through() {
        let mut app = app();
        let back = app
            .world_mut()
            .spawn((
                CursorPlane::XY,
                Camera {
                    order: 0,
                    ..default()
                },
            ))
            .id();
        let front = app
            .world_mut()
            .spawn((
                CursorPlane::XY,
                Camera {
                    order: 1,
                    ..default()
                },
            ))
            .id();
        let window = app.world_mut().spawn(Window::default()).id();
        cast(&mut app, back, PointerId::Mouse, Vec3::ZERO);
        cast(&mut app, front, PointerId::Mouse, SPOT);
        hover(&mut app, PointerId::Mouse, &[window]);
        assert_eq!(pointed(&mut app).map(|at| at.camera), Some(front));
    }

    #[test]
    fn a_press_on_the_world_is_announced_once_and_a_press_on_ui_never() {
        let mut app = app();
        let camera = app.world_mut().spawn(CursorPlane::XY).id();
        let window = app.world_mut().spawn(Window::default()).id();
        let panel = app.world_mut().spawn(Node::default()).id();
        cast(&mut app, camera, PointerId::Mouse, SPOT);

        hover(&mut app, PointerId::Mouse, &[window]);
        let presses = press(&mut app, PointerId::Mouse);
        assert_eq!(
            presses,
            [WorldPress {
                point: SPOT,
                camera,
                button: PointerButton::Primary,
                pointer: PointerId::Mouse,
            }]
        );
        hover(&mut app, PointerId::Mouse, &[panel]);
        assert!(press(&mut app, PointerId::Mouse).is_empty());

        // Over a viewport, its own pointer repeats the press; still one press.
        let shown = app.world_mut().spawn(CursorPlane::XY).id();
        let viewport = app.world_mut().spawn(ViewportNode::new(shown)).id();
        let inner = *app.world().get::<PointerId>(viewport).unwrap();
        cast(&mut app, shown, inner, SPOT);
        hover(&mut app, PointerId::Mouse, &[viewport]);
        // Inside, the viewport's pointer hovers something in its world.
        let player = app.world_mut().spawn_empty().id();
        hover(&mut app, inner, &[player]);
        let location = Location {
            target: NormalizedRenderTarget::None {
                width: 1,
                height: 1,
            },
            position: Vec2::ZERO,
        };
        app.world_mut().write_message(PointerInput::new(
            inner,
            location,
            PointerAction::Press(PointerButton::Primary),
        ));
        let presses = press(&mut app, PointerId::Mouse);
        assert_eq!(presses.len(), 1);
        assert_eq!(presses[0].camera, shown);
    }

    /// An entity inside the game view, hovered through the viewport's pointer,
    /// must still change the window cursor.
    #[test]
    fn hovering_an_entity_inside_a_viewport_sets_the_cursor() {
        let mut app = app();
        let window = app.world_mut().spawn(Window::default()).id();
        let camera = app.world_mut().spawn_empty().id();
        let crosshair = EntityCursor::System(SystemCursorIcon::Crosshair);
        let viewport = app
            .world_mut()
            .spawn((ViewportNode::new(camera), ViewportCursor(crosshair.clone())))
            .id();
        let viewport_pointer = *app.world().get::<PointerId>(viewport).unwrap();
        // The cursor is set on the player, but the hit lands on its sprite child.
        let player = app
            .world_mut()
            .spawn(EntityCursor::System(SystemCursorIcon::Pointer))
            .id();
        let sprite = app.world_mut().spawn(ChildOf(player)).id();

        hover(&mut app, PointerId::Mouse, &[viewport]);
        hover(&mut app, viewport_pointer, &[sprite]);
        app.update();
        app.update();
        let icon = app.world().get::<CursorIcon>(window).unwrap();
        assert_eq!(icon.as_system(), Some(&SystemCursorIcon::Pointer));

        // Over empty space in the viewport, the fallback applies.
        hover(&mut app, viewport_pointer, &[]);
        app.update();
        app.update();
        let icon = app.world().get::<CursorIcon>(window).unwrap();
        assert_eq!(icon.as_system(), Some(&SystemCursorIcon::Crosshair));
    }
}
