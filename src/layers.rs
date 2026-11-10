//! Defines layer root nodes for the three persistent UI layers: Screen, Overlay, and Feedback.
//!
//! Put content on a layer with [`UiLayer`], as part of its bundle:
//! `commands.spawn((MyMenu, UiLayer::Screen, Node::default()))`. The roots
//! themselves are in [`UiLayers`], from `Startup`.
//!
//! The roots are containers: they never take pointer input themselves, so a
//! click lands on whatever content is under it. Only the overlay dimmer blocks
//! clicks, and only while it's shown.
//!
//! Popovers have no layer of their own: each stays a child of what opened it
//! and floats above that content's layer, so a popover on the
//! Screen layer stays under the dimmer and one in an overlay stays over it.
//! Pickers that hold the pointer, like an open radial menu, float
//! just above it, over that layer's popovers.

use bevy::ecs::lifecycle::HookContext;
use bevy::ecs::world::DeferredWorld;
use bevy::input_focus::tab_navigation::TabGroup;
use bevy::prelude::*;
use bevy::ui::UiSystems;

use crate::EnsurePlugin;
use crate::theme::palette::SCRIM;
use crate::theme::{ThemePlugin, ThemeSkin};

/// Tunable global ZIndex values for the three persistent UI layers.
const SCREEN_LAYER_Z: GlobalZIndex = GlobalZIndex(10);
const OVERLAY_DIMMER_Z: GlobalZIndex = GlobalZIndex(90);
const OVERLAY_LAYER_Z: GlobalZIndex = GlobalZIndex(100);
const FEEDBACK_LAYER_Z: GlobalZIndex = GlobalZIndex(1000);

/// How far above its layer a popover floats. It must stay below the next layer
/// up, so a game-screen popover never covers an overlay.
pub(crate) const POPOVER_LIFT: i32 = 40;
/// How far above its layer an open picker floats: over the layer's popovers,
/// and like them below the next layer up.
pub(crate) const PICKER_LIFT: i32 = POPOVER_LIFT + 1;

/// Tab order between layers. Screen content is tabbable; tabbing inside an
/// overlay stays inside it, as a modal should.
const SCREEN_TAB_GROUP: TabGroup = TabGroup {
    order: 0,
    modal: false,
};
const OVERLAY_TAB_GROUP: TabGroup = TabGroup {
    order: 1,
    modal: true,
};

// The dimmer must sit above the Screen layer and below Overlay content,
// and Feedback must sit above everything. A layer's popovers stay below the
// next layer up.
const _: () = assert!(
    SCREEN_LAYER_Z.0 < OVERLAY_DIMMER_Z.0
        && OVERLAY_DIMMER_Z.0 < OVERLAY_LAYER_Z.0
        && OVERLAY_LAYER_Z.0 < FEEDBACK_LAYER_Z.0
        && POPOVER_LIFT > 0
        && PICKER_LIFT > POPOVER_LIFT
        && SCREEN_LAYER_Z.0 + PICKER_LIFT < OVERLAY_DIMMER_Z.0
        && OVERLAY_LAYER_Z.0 + PICKER_LIFT < FEEDBACK_LAYER_Z.0
);

/// Marker for the Screen layer root node (GlobalZIndex 10).
/// Houses the active mode's layout (e.g. Match, ScenarioTest, Sandbox).
#[derive(Component, Debug, Clone, Copy, Reflect)]
#[reflect(Component, Debug)]
pub struct ScreenLayerRoot;

/// Marker for the Overlay layer root node (GlobalZIndex 100).
/// Houses modal overlay screens and the modal dimmer.
#[derive(Component, Debug, Clone, Copy, Reflect)]
#[reflect(Component, Debug)]
pub struct OverlayLayerRoot;

/// Dimmer backdrop node that blocks pointer clicks to the Screen layer.
#[derive(Component, Debug, Clone, Copy, Reflect)]
#[reflect(Component, Debug)]
pub struct OverlayDimmer;

/// Marker for the Feedback layer root node (GlobalZIndex 1000).
/// Houses transient feedback elements such as tooltips, toasts, and drag previews.
#[derive(Component, Debug, Clone, Copy, Reflect)]
#[reflect(Component, Debug)]
pub struct FeedbackLayerRoot;

/// The three persistent UI layer roots. Inserted at `PreStartup`, so it's
/// there for every `Startup` system on.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Reflect)]
#[reflect(Resource, Debug)]
pub struct UiLayers {
    /// The Screen layer's root: the game's main layout.
    pub screen: Entity,
    /// The Overlay layer's root: modal screens, over the dimmer.
    pub overlay: Entity,
    /// The Feedback layer's root: tooltips, toasts and drag previews.
    pub feedback: Entity,
}

impl UiLayers {
    /// The root of `layer`.
    pub fn root(&self, layer: UiLayer) -> Entity {
        match layer {
            UiLayer::Screen => self.screen,
            UiLayer::Overlay => self.overlay,
            UiLayer::Feedback => self.feedback,
        }
    }
}

/// Puts an entity on a UI layer: inserted, it makes the entity a child of
/// that layer's root, so `(MyMenu, UiLayer::Screen, Node::default())` is a
/// menu on the screen. Insert another to move it, as a panel becoming modal
/// moves to the overlay.
///
/// Synonymous with `ChildOf`, so an entity should have either one or the
/// other. The UiLayer roots spawn in PreStartup.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash, Reflect)]
#[component(on_insert = place_on_layer)]
#[reflect(Component, Debug)]
pub enum UiLayer {
    /// The game's main layout.
    Screen,
    /// Modal screens, over the dimmer.
    Overlay,
    /// Tooltips, toasts and drag previews, over everything.
    Feedback,
}

/// Makes an entity a child of its [`UiLayer`]'s root, as soon as it's inserted.
fn place_on_layer(mut world: DeferredWorld, context: HookContext) {
    let Some(&layer) = world.get::<UiLayer>(context.entity) else {
        return;
    };
    let Some(root) = world
        .get_resource::<UiLayers>()
        .map(|roots| roots.root(layer))
    else {
        warn!(
            "{} was put on the {layer:?} layer before it existed: add `LayersPlugin`, \
             and spawn onto a layer from `Startup` on",
            context.entity
        );
        return;
    };
    world
        .commands()
        .entity(context.entity)
        .try_insert(ChildOf(root));
}

/// Spawns the three persistent UI layer roots, and notes them in [`UiLayers`].
fn setup_ui_layers(mut commands: Commands) {
    let screen = commands
        .spawn((
            ScreenLayerRoot,
            SCREEN_LAYER_Z,
            SCREEN_TAB_GROUP,
            // UI nodes take clicks by default, even transparent ones. A root covers
            // the whole window, so it opts out; its children still take their own.
            Pickable::IGNORE,
            Node {
                width: percent(100),
                height: percent(100),
                position_type: PositionType::Absolute,
                display: Display::Flex,
                overflow: Overflow::clip(),
                ..default()
            },
        ))
        .id();

    let overlay = commands
        .spawn((
            OverlayLayerRoot,
            OVERLAY_LAYER_Z,
            OVERLAY_TAB_GROUP,
            Pickable::IGNORE,
            Node {
                width: percent(100),
                height: percent(100),
                position_type: PositionType::Absolute,
                display: Display::Flex,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
        ))
        .with_children(|overlay| {
            // The dimmer keeps the default `Pickable`: blocking clicks is its job.
            overlay.spawn((
                OverlayDimmer,
                OVERLAY_DIMMER_Z,
                ThemeSkin(SCRIM),
                Node {
                    width: percent(100),
                    height: percent(100),
                    position_type: PositionType::Absolute,
                    display: Display::None, // Starts hidden until an overlay is active
                    ..default()
                },
            ));
        })
        .id();

    let feedback = commands
        .spawn((
            FeedbackLayerRoot,
            Pickable::IGNORE,
            FEEDBACK_LAYER_Z,
            Node {
                width: percent(100),
                height: percent(100),
                position_type: PositionType::Absolute,
                display: Display::Flex,
                ..default()
            },
        ))
        .id();

    commands.insert_resource(UiLayers {
        screen,
        overlay,
        feedback,
    });
}

/// Shows the dimmer while the overlay holds anything but the dimmer itself.
fn dim_under_overlays(
    overlays: Query<&Children, (With<OverlayLayerRoot>, Changed<Children>)>,
    mut dimmers: Query<&mut Node, With<OverlayDimmer>>,
) {
    for children in &overlays {
        let shown = children.iter().any(|child| !dimmers.contains(child));
        let display = if shown { Display::Flex } else { Display::None };
        for mut dimmer in &mut dimmers {
            dimmer.display = display;
        }
    }
}

/// Spawns the three persistent UI layer roots at `PreStartup`.
pub struct LayersPlugin;

impl Plugin for LayersPlugin {
    fn build(&self, app: &mut App) {
        app.ensure_plugin(ThemePlugin)
            .add_systems(PreStartup, setup_ui_layers)
            .add_systems(PostUpdate, dim_under_overlays.before(UiSystems::Prepare));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A layer root that took clicks would swallow every click on the layers
    /// beneath it, since it covers the whole window.
    #[test]
    fn layer_roots_never_take_clicks_but_the_dimmer_does() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, LayersPlugin));
        app.update();
        let world = app.world_mut();
        let roots: Vec<_> = world
            .query_filtered::<Option<&Pickable>, Or<(
                With<ScreenLayerRoot>,
                With<OverlayLayerRoot>,
                With<FeedbackLayerRoot>,
            )>>()
            .iter(world)
            .collect();
        assert_eq!(roots.len(), 3);
        assert!(
            roots
                .iter()
                .all(|pickable| *pickable == Some(&Pickable::IGNORE))
        );

        let dimmer = world
            .query_filtered::<Option<&Pickable>, With<OverlayDimmer>>()
            .single(world)
            .unwrap();
        assert_ne!(dimmer, Some(&Pickable::IGNORE));
    }

    /// Content put on a layer lands under its root, and moves when told to.
    #[test]
    fn content_goes_on_its_layer_and_moves_between_them() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, LayersPlugin));
        app.update();
        let layers = *app.world().resource::<UiLayers>();
        let screen = app
            .world_mut()
            .query_filtered::<Entity, With<ScreenLayerRoot>>()
            .single(app.world())
            .unwrap();
        assert_eq!(layers.screen, screen);

        let menu = app
            .world_mut()
            .spawn((UiLayer::Screen, Node::default()))
            .id();
        app.update();
        let parent = |app: &App| app.world().get::<ChildOf>(menu).map(ChildOf::parent);
        assert_eq!(parent(&app), Some(layers.screen));

        app.world_mut().entity_mut(menu).insert(UiLayer::Overlay);
        app.update();
        assert_eq!(parent(&app), Some(layers.overlay));

        // A layer and a parent in the same bundle: the layer wins.
        let elsewhere = app.world_mut().spawn(Node::default()).id();
        let both = app
            .world_mut()
            .spawn((ChildOf(elsewhere), UiLayer::Feedback, Node::default()))
            .id();
        app.update();
        let parent = app.world().get::<ChildOf>(both).map(ChildOf::parent);
        assert_eq!(parent, Some(layers.feedback));
    }

    /// The dimmer shows while something is on the overlay, and hides when it's gone.
    #[test]
    fn the_dimmer_shows_while_the_overlay_holds_something() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, LayersPlugin));
        app.update();
        let display = |app: &mut App| {
            let world = app.world_mut();
            let mut dimmers = world.query_filtered::<&Node, With<OverlayDimmer>>();
            dimmers.single(world).unwrap().display
        };
        assert_eq!(display(&mut app), Display::None);
        let modal = app
            .world_mut()
            .spawn((UiLayer::Overlay, Node::default()))
            .id();
        app.update();
        assert_eq!(display(&mut app), Display::Flex);
        app.world_mut().entity_mut(modal).despawn();
        app.update();
        assert_eq!(display(&mut app), Display::None);
    }

    /// Too early, before the roots exist, an entity stays where it is.
    #[test]
    fn content_put_on_a_layer_too_early_is_left_alone() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        let menu = app
            .world_mut()
            .spawn((UiLayer::Screen, Node::default()))
            .id();
        app.update();
        assert!(app.world().get::<ChildOf>(menu).is_none());
    }

    #[test]
    fn the_dimmer_takes_the_palettes_scrim() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, LayersPlugin));
        app.update();
        let world = app.world_mut();
        let colour = world
            .query_filtered::<&BackgroundColor, With<OverlayDimmer>>()
            .single(world)
            .unwrap()
            .0;
        assert_eq!(
            colour,
            world.resource::<crate::theme::Theme>().color(&SCRIM)
        );
    }
}
