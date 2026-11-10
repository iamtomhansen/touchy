//! Popovers: panels that float beside what opened them, above everything
//! around it, and close when the user presses elsewhere or cancels.

use bevy::input_focus::{FocusCause, InputFocus};
use bevy::picking::events::PointerPress;
use bevy::prelude::*;
use bevy::ui::{FixedNode, UiSystems};
use bevy::ui_widgets::popover::{Popover, PopoverPlacement, PopoverPlugin as BevyPopoverPlugin};
use bevy::ui_widgets::{Activate, ValueChange};

pub use bevy::ui_widgets::popover::{PopoverAlign, PopoverSide};

use crate::EnsurePlugin;
use crate::cancel::{CancelPhase, CancelPlugin, CancelRequest};
use crate::layers::POPOVER_LIFT;
use crate::sanitize::at_least;
use crate::theme::palette::{CORNER_RADIUS, EDGE, SURFACE};
use crate::theme::skins;
use crate::theme::{
    Shade, Theme, ThemeBorder, ThemeCornerRadius, ThemePlugin, ThemeSkin, ThemeToken,
};

/// Prefix of the popovers' theme tokens.
pub const POPOVER_STYLE: &str = "touchy.popover";

/// Space between the anchor and the popover, in pixels.
const DEFAULT_GAP: f32 = 4.0;
/// Space inside the popover's edge, in pixels.
const DEFAULT_PADDING: f32 = 8.0;
/// Closest a popover comes to the window's edge, in pixels.
const WINDOW_MARGIN: f32 = 8.0;
/// The width of the popover's border, in pixels.
const BORDER_WIDTH: f32 = 1.0;

/// Theme tokens for a popover.
#[derive(Component, Clone, PartialEq, Debug)]
#[component(immutable)]
pub struct PopoverTokens {
    /// The popover's background.
    pub bg: ThemeToken,
    /// The popover's border.
    pub border: ThemeToken,
    /// The popover's corner radius.
    pub corner_radius: ThemeToken,
}

impl PopoverTokens {
    /// Names the tokens under `prefix`
    pub fn with_prefix(prefix: &str) -> Self {
        let token = |suffix: &str| ThemeToken::new(format!("{prefix}.{suffix}"));
        Self {
            bg: token("bg"),
            border: token("border"),
            corner_radius: token("corner_radius"),
        }
    }

    /// Register the default values for the popover tokens, unless the app sets them.
    pub fn register_theme_defaults(&self, theme: &mut Theme) {
        theme
            .default_derived(self.bg.clone(), SURFACE, Shade::Same)
            .default_derived(self.border.clone(), EDGE, Shade::Same)
            .default_size_from(self.corner_radius.clone(), CORNER_RADIUS);
        // Each part draws its role's skin, once a theme gives the role one.
        theme.default_skin_from(self.bg.clone(), skins::SURFACE);
    }
}

/// The built-in token set.
impl Default for PopoverTokens {
    fn default() -> Self {
        Self::with_prefix(POPOVER_STYLE)
    }
}

/// Whether a popover is open.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PopoverOpen(pub bool);

/// Triggers a popover to open or close when pressed.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct TogglesPopover(pub Entity);

/// A touchy popover, and whether it closes on its own.
#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct PopoverBehaviour {
    light_dismiss: bool,
}

/// Light-dismiss popovers, in the order they opened. Cancelling closes the last.
#[derive(Resource, Default, Debug)]
struct OpenPopovers(Vec<Entity>);

/// Construction-time configuration for a popover.
///
/// Consumed by [`SpawnUi::popover`](crate::spawn_ui::SpawnUi::popover) and not retained as a
/// component.
///
/// # Examples
/// ```
/// use touchy::prelude::*;
///
/// let config = PopoverConfig::new()
///     .side(PopoverSide::Right)
///     .light_dismiss(false);
///
/// # assert!(!config.light_dismiss);
/// ```
#[derive(Clone, Debug)]
pub struct PopoverConfig {
    /// The side of the anchor it opens on when there's room; otherwise the opposite side.
    pub side: PopoverSide,
    /// Which of its edges lines up with the anchor's, along that side.
    pub align: PopoverAlign,
    /// The gap between the popover and its anchor, in pixels.
    pub gap: f32,
    /// The padding inside the popover, in pixels.
    pub padding: f32,
    /// Whether the popover is open when spawned.
    pub open: bool,
    /// Whether pressing elsewhere or cancelling (Escape or right-click) closes it.
    pub light_dismiss: bool,
    /// The theme tokens for the popover.
    pub tokens: PopoverTokens,
}

impl Default for PopoverConfig {
    fn default() -> Self {
        Self {
            side: PopoverSide::Bottom,
            align: PopoverAlign::Start,
            gap: DEFAULT_GAP,
            padding: DEFAULT_PADDING,
            open: false,
            light_dismiss: true,
            tokens: PopoverTokens::with_prefix(POPOVER_STYLE),
        }
    }
}

impl PopoverConfig {
    /// Creates a new popover config with default values.
    pub fn new() -> Self {
        Self::default()
    }
    /// The side of the anchor it prefers; it takes the other side when there's
    /// no room on this one.
    pub fn side(mut self, side: PopoverSide) -> Self {
        self.side = side;
        self
    }
    /// Which of its edges lines up with the anchor's, along that side.
    pub fn align(mut self, align: PopoverAlign) -> Self {
        self.align = align;
        self
    }
    /// The gap between the popover and its anchor, in pixels.
    pub fn gap(mut self, pixels: f32) -> Self {
        self.gap = pixels;
        self
    }
    /// The padding inside the popover, in pixels.
    pub fn padding(mut self, pixels: f32) -> Self {
        self.padding = pixels;
        self
    }
    /// Spawn it showing.
    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }
    /// Whether pressing elsewhere or cancelling (Escape or right-click)
    /// closes it. On by default; off leaves closing to the app.
    pub fn light_dismiss(mut self, light_dismiss: bool) -> Self {
        self.light_dismiss = light_dismiss;
        self
    }
    /// Set the theme tokens for the popover.
    pub fn tokens(mut self, tokens: PopoverTokens) -> Self {
        self.tokens = tokens;
        self
    }
    /// Where Bevy may put it, best first: the preferred side and alignment,
    /// then the other side, then each again with the opposite alignment.
    fn placements(&self) -> Vec<PopoverPlacement> {
        let opposite = match self.align {
            PopoverAlign::Start => PopoverAlign::End,
            PopoverAlign::End => PopoverAlign::Start,
            PopoverAlign::Center => PopoverAlign::Center,
        };
        let mut placements = Vec::new();
        for align in [self.align, opposite] {
            for side in [self.side, self.side.mirror()] {
                let placement = PopoverPlacement {
                    side,
                    align,
                    gap: self.gap,
                };
                if !placements.contains(&placement) {
                    placements.push(placement);
                }
            }
        }
        placements
    }
    /// Sanitizes the popover config, ensuring all values are within valid ranges.
    fn sanitized(mut self) -> Self {
        self.padding = at_least(self.padding, 0.0, "PopoverConfig padding");
        self.gap = at_least(self.gap, 0.0, "PopoverConfig gap");
        self
    }
}

/// Spawns it; documented on [`SpawnUi::popover`](crate::spawn_ui::SpawnUi::popover).
pub(crate) fn spawn_popover<'a>(
    anchor: &'a mut ChildSpawnerCommands,
    config: PopoverConfig,
    build: impl FnOnce(&mut ChildSpawnerCommands),
) -> EntityCommands<'a> {
    let config = config.sanitized();
    let placements = config.placements();
    let PopoverConfig {
        padding,
        open,
        light_dismiss,
        tokens,
        ..
    } = config;
    // Its colours come with its tokens, in `style_popover`.
    let mut popover = anchor.spawn((
        PopoverBehaviour { light_dismiss },
        PopoverOpen(open),
        FixedNode,
        Popover {
            positions: placements,
            window_margin: WINDOW_MARGIN,
        },
        Node {
            display: display(open),
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(px(padding)),
            border: UiRect::all(px(BORDER_WIDTH)),
            ..default()
        },
        tokens,
    ));
    popover.with_children(build);
    popover
}

/// Converts boolean to Display, for showing and hiding popovers.
fn display(open: bool) -> Display {
    if open { Display::Flex } else { Display::None }
}

/// What tells a layer from a popover floating above it, for [`layer_under`].
pub(crate) type Stacked = (Option<&'static GlobalZIndex>, Has<PopoverBehaviour>);

/// The global z of the layer `entity` is on, looking past the popovers it may
/// be inside, and how many popovers those are.
pub(crate) fn layer_under(
    entity: Entity,
    parents: &Query<&ChildOf>,
    stacked: &Query<Stacked>,
) -> (i32, i32) {
    let mut nested = 0;
    for ancestor in parents.iter_ancestors(entity) {
        match stacked.get(ancestor) {
            Ok((_, true)) => nested += 1,
            Ok((Some(layer), false)) => return (layer.0, nested),
            _ => {}
        }
    }
    (0, nested)
}

/// Computes Z-index for a popover, above its layer, allowing nested popovers to stack.
fn stacking(
    popover: Entity,
    parents: &Query<&ChildOf>,
    stacked: &Query<Stacked>,
) -> (GlobalZIndex, ZIndex) {
    let (layer, nested) = layer_under(popover, parents, stacked);
    (GlobalZIndex(layer + POPOVER_LIFT), ZIndex(nested))
}

/// Shows and hides popovers as they open and close, announcing each as a
/// `ValueChange<bool>`, and lifts each above its layer as it opens.
fn open_and_close_popovers(
    mut popovers: Query<(Entity, &PopoverBehaviour, &PopoverOpen, &mut Node), Changed<PopoverOpen>>,
    mut gone: RemovedComponents<PopoverOpen>,
    parents: Query<&ChildOf>,
    stacked: Query<Stacked>,
    mut open_popovers: ResMut<OpenPopovers>,
    mut commands: Commands,
) {
    for gone in gone.read() {
        open_popovers.0.retain(|&open| open != gone);
    }
    for (popover, panel, open, mut node) in &mut popovers {
        // A change in display indicates opening or closing.
        let shown = display(open.0);
        if node.display != shown {
            node.display = shown;
            commands.trigger(ValueChange {
                source: popover,
                value: open.0,
                is_final: true,
            });
        }
        open_popovers.0.retain(|&other| other != popover);
        if open.0 {
            commands
                .entity(popover)
                .insert(stacking(popover, &parents, &stacked));
            if panel.light_dismiss {
                open_popovers.0.push(popover);
            }
        }
    }
}

/// A popover that closes with focus inside hands it back to its opener, or
/// clears it: focus left on a hidden entity would send keys somewhere unseen.
fn return_focus_from_closed(
    popovers: Query<(Entity, &PopoverOpen), Changed<PopoverOpen>>,
    parents: Query<&ChildOf>,
    openers: Query<(Entity, &TogglesPopover)>,
    focus: Option<ResMut<InputFocus>>,
) {
    let Some(mut focus) = focus else {
        return;
    };
    for (popover, open) in &popovers {
        let focus_inside = focus.get().is_some_and(|focused| {
            focused == popover || parents.iter_ancestors(focused).any(|up| up == popover)
        });
        if open.0 || !focus_inside {
            continue;
        }
        match openers.iter().find(|(_, toggles)| toggles.0 == popover) {
            Some((opener, _)) => focus.set(opener, FocusCause::Navigated),
            None => focus.clear(),
        }
    }
}

/// Opens or closes the popover a [`TogglesPopover`] names when it's activated.
fn toggle_from_button(
    activate: On<Activate>,
    openers: Query<&TogglesPopover>,
    mut popovers: Query<&mut PopoverOpen>,
) {
    if let Ok(TogglesPopover(popover)) = openers.get(activate.entity)
        && let Ok(mut open) = popovers.get_mut(*popover)
    {
        open.0 = !open.0;
    }
}

/// A press outside a light-dismiss popover's anchor closes it. The press still
/// does whatever it does, so the user needn't click twice.
fn dismiss_on_press_elsewhere(
    press: On<PointerPress>,
    mut popovers: Query<(Entity, &PopoverBehaviour, &mut PopoverOpen, &ChildOf)>,
    parents: Query<&ChildOf>,
    openers: Query<&TogglesPopover>,
) {
    // Presses bubble; only the entity actually pressed matters.
    if press.entity != press.original_event_target() {
        return;
    }
    let pressed: Vec<Entity> = std::iter::once(press.entity)
        .chain(parents.iter_ancestors(press.entity))
        .collect();
    for (popover, panel, mut open, anchor) in &mut popovers {
        if !panel.light_dismiss || !open.0 {
            continue;
        }
        let inside = pressed.iter().any(|&entity| {
            entity == anchor.parent()
                || openers
                    .get(entity)
                    .is_ok_and(|toggles| toggles.0 == popover)
        });
        if !inside {
            open.0 = false;
        }
    }
}

/// Escape or right-click closes the popover opened last, before any overlay
/// or menu sees it.
fn dismiss_on_cancel(
    mut cancel: ResMut<CancelRequest>,
    open_popovers: Res<OpenPopovers>,
    mut popovers: Query<&mut PopoverOpen>,
) {
    if cancel.peek().is_none() {
        return;
    }
    // A press elsewhere may have closed some this frame already.
    let last_open = open_popovers
        .0
        .iter()
        .rev()
        .find(|&&popover| popovers.get(popover).is_ok_and(|open| open.0));
    if let Some(&popover) = last_open {
        cancel.consume();
        if let Ok(mut open) = popovers.get_mut(popover) {
            open.0 = false;
        }
    }
}

/// Styles a popover when its tokens are set: at spawn, and whenever the app sets new ones.
fn style_popover(
    insert: On<Insert<PopoverTokens>>,
    popovers: Query<&PopoverTokens>,
    mut commands: Commands,
) {
    // Tokens on an entity that isn't a popover: nothing to style.
    let Ok(tokens) = popovers.get(insert.entity) else {
        return;
    };
    let mut popover = commands.entity(insert.entity);
    popover.insert((
        ThemeSkin(tokens.bg.clone()),
        ThemeBorder::all(tokens.border.clone()),
        ThemeCornerRadius(tokens.corner_radius.clone()),
    ));
}

/// Popover placement, opening, closing and default token values.
pub struct PopoverPlugin;

impl Plugin for PopoverPlugin {
    fn build(&self, app: &mut App) {
        app.ensure_plugin(ThemePlugin)
            .ensure_plugin(CancelPlugin)
            .ensure_plugin(BevyPopoverPlugin)
            .init_resource::<OpenPopovers>()
            .add_observer(toggle_from_button)
            .add_observer(dismiss_on_press_elsewhere)
            .add_observer(style_popover)
            .add_systems(PreUpdate, dismiss_on_cancel.in_set(CancelPhase::Popover))
            .add_systems(
                PostUpdate,
                (open_and_close_popovers, return_focus_from_closed).before(UiSystems::Prepare),
            );
        PopoverTokens::with_prefix(POPOVER_STYLE)
            .register_theme_defaults(&mut app.world_mut().resource_mut::<Theme>());
    }
}

#[cfg(test)]
mod tests {
    use bevy::camera::NormalizedRenderTarget;
    use bevy::input::ButtonState;
    use bevy::input::InputPlugin;
    use bevy::input::keyboard::{Key, KeyboardInput};
    use bevy::picking::backend::HitData;
    use bevy::picking::events::Pointer;
    use bevy::picking::pointer::{Location, PointerButton, PointerId};

    use super::*;
    use crate::layers::{LayersPlugin, OverlayLayerRoot, ScreenLayerRoot};

    #[derive(Resource, Default)]
    struct Heard(Vec<bool>);

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, InputPlugin, LayersPlugin, PopoverPlugin))
            .init_resource::<InputFocus>();
        app.update();
        app
    }

    fn root<T: Component>(app: &mut App) -> Entity {
        let world = app.world_mut();
        world
            .query_filtered::<Entity, With<T>>()
            .single(world)
            .unwrap()
    }

    /// An anchor under `parent` holding an opener and a popover; returns
    /// (anchor, opener, popover).
    fn anchored(app: &mut App, parent: Entity, config: PopoverConfig) -> (Entity, Entity, Entity) {
        let world = app.world_mut();
        let anchor = world.spawn((Node::default(), ChildOf(parent))).id();
        let mut popover = Entity::PLACEHOLDER;
        world.commands().entity(anchor).with_children(|anchor| {
            popover = spawn_popover(anchor, config, |_| {}).id();
        });
        world.flush();
        let opener = world
            .spawn((Node::default(), ChildOf(anchor), TogglesPopover(popover)))
            .id();
        (anchor, opener, popover)
    }

    fn is_open(app: &App, popover: Entity) -> bool {
        app.world().get::<PopoverOpen>(popover).unwrap().0
    }

    fn press(app: &mut App, entity: Entity) {
        let location = Location {
            target: NormalizedRenderTarget::None {
                width: 1,
                height: 1,
            },
            position: Vec2::ZERO,
        };
        app.world_mut().trigger(PointerPress {
            entity,
            pointer: Pointer::new(PointerId::Mouse, location),
            button: PointerButton::Primary,
            hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
            count: 1,
        });
        app.update();
    }

    /// Presses and releases Escape.
    fn escape(app: &mut App) {
        for state in [ButtonState::Pressed, ButtonState::Released] {
            app.world_mut().write_message(KeyboardInput {
                key_code: KeyCode::Escape,
                logical_key: Key::Escape,
                state,
                text: None,
                repeat: false,
                window: Entity::PLACEHOLDER,
            });
            app.update();
        }
    }

    #[test]
    fn a_popover_floats_above_its_layer_and_a_nested_one_above_it() {
        let mut app = app();
        let open = PopoverConfig::default().open(true);
        let screen = root::<ScreenLayerRoot>(&mut app);
        let overlay = root::<OverlayLayerRoot>(&mut app);
        let (_, _, on_screen) = anchored(&mut app, screen, open.clone());
        let (_, _, in_overlay) = anchored(&mut app, overlay, open.clone());
        let (_, _, nested) = anchored(&mut app, on_screen, open.clone());
        let unlayered = app.world_mut().spawn(Node::default()).id();
        let (_, _, unlayered) = anchored(&mut app, unlayered, open);
        app.update();

        let z = |app: &App, entity| app.world().get::<GlobalZIndex>(entity).unwrap().0;
        let local = |app: &App, entity| app.world().get::<ZIndex>(entity).unwrap().0;
        assert_eq!(z(&app, on_screen), z(&app, screen) + POPOVER_LIFT);
        assert_eq!(z(&app, in_overlay), z(&app, overlay) + POPOVER_LIFT);
        assert_eq!(z(&app, unlayered), POPOVER_LIFT);
        assert_eq!(
            z(&app, nested),
            z(&app, on_screen),
            "nested stays on its layer"
        );
        assert!(
            local(&app, nested) > local(&app, on_screen),
            "but above its parent"
        );
    }

    #[test]
    fn it_shows_only_while_open_and_its_opener_toggles_it() {
        let mut app = app();
        let screen = root::<ScreenLayerRoot>(&mut app);
        let (_, opener, popover) = anchored(&mut app, screen, PopoverConfig::default());
        app.update();
        let display = |app: &App| app.world().get::<Node>(popover).unwrap().display;
        assert_eq!(display(&app), Display::None);

        app.world_mut().trigger(Activate { entity: opener });
        app.update();
        assert!(is_open(&app, popover));
        assert_eq!(display(&app), Display::Flex);

        app.world_mut().trigger(Activate { entity: opener });
        app.update();
        assert!(!is_open(&app, popover));
        assert_eq!(display(&app), Display::None);
    }

    #[test]
    fn every_opening_and_closing_is_announced_once_but_spawning_is_not() {
        let mut app = app();
        app.init_resource::<Heard>().add_observer(
            |change: On<ValueChange<bool>>, mut heard: ResMut<Heard>| {
                heard.0.push(change.value);
            },
        );
        let screen = root::<ScreenLayerRoot>(&mut app);
        let (_, opener, popover) = anchored(&mut app, screen, PopoverConfig::default().open(true));
        let elsewhere = app
            .world_mut()
            .spawn((Node::default(), ChildOf(screen)))
            .id();
        app.update();
        press(&mut app, elsewhere);
        app.world_mut().trigger(Activate { entity: opener });
        app.update();
        // Writing the same state again isn't a change.
        app.world_mut().get_mut::<PopoverOpen>(popover).unwrap().0 = true;
        app.update();
        escape(&mut app);
        assert_eq!(app.world().resource::<Heard>().0, [false, true, false]);
    }

    #[test]
    fn pressing_elsewhere_closes_it_but_pressing_its_anchor_or_opener_does_not() {
        let mut app = app();
        let screen = root::<ScreenLayerRoot>(&mut app);
        let open = PopoverConfig::default().open(true);
        let (anchor, opener, popover) = anchored(&mut app, screen, open.clone());
        let inside = app
            .world_mut()
            .spawn((Node::default(), ChildOf(popover)))
            .id();
        // An opener elsewhere on the screen, outside the anchor.
        let far_opener = app
            .world_mut()
            .spawn((Node::default(), ChildOf(screen), TogglesPopover(popover)))
            .id();
        let elsewhere = app
            .world_mut()
            .spawn((Node::default(), ChildOf(screen)))
            .id();
        let (_, _, stays) = anchored(&mut app, screen, open.light_dismiss(false));
        app.update();

        for target in [anchor, opener, inside, far_opener] {
            press(&mut app, target);
            assert!(is_open(&app, popover), "pressing {target} kept it open");
        }
        press(&mut app, elsewhere);
        assert!(!is_open(&app, popover));
        assert!(is_open(&app, stays), "not light-dismiss");
    }

    #[derive(Resource, Default)]
    struct MenuSawCancel(bool);

    #[test]
    fn cancelling_closes_the_last_opened_before_menus_see_it() {
        let mut app = app();
        app.init_resource::<MenuSawCancel>().add_systems(
            PreUpdate,
            (|cancel: Res<CancelRequest>, mut saw: ResMut<MenuSawCancel>| {
                saw.0 |= cancel.peek().is_some();
            })
            .in_set(CancelPhase::Menu),
        );
        let screen = root::<ScreenLayerRoot>(&mut app);
        let (_, _, first) = anchored(&mut app, screen, PopoverConfig::default());
        let (_, _, second) = anchored(&mut app, screen, PopoverConfig::default());
        for popover in [first, second] {
            app.world_mut().get_mut::<PopoverOpen>(popover).unwrap().0 = true;
            app.update();
        }

        escape(&mut app);
        assert!(is_open(&app, first) && !is_open(&app, second));
        escape(&mut app);
        assert!(!is_open(&app, first));
        assert!(!app.world().resource::<MenuSawCancel>().0);
        escape(&mut app);
        assert!(
            app.world().resource::<MenuSawCancel>().0,
            "nothing left to close"
        );
    }

    #[test]
    fn closing_with_focus_inside_hands_focus_back_to_the_opener() {
        let mut app = app();
        let screen = root::<ScreenLayerRoot>(&mut app);
        let (_, opener, popover) = anchored(&mut app, screen, PopoverConfig::default().open(true));
        let field = app
            .world_mut()
            .spawn((Node::default(), ChildOf(popover)))
            .id();
        app.update();
        app.world_mut()
            .resource_mut::<InputFocus>()
            .set(field, FocusCause::Navigated);
        escape(&mut app);
        assert_eq!(app.world().resource::<InputFocus>().get(), Some(opener));
    }

    #[test]
    fn it_prefers_its_side_then_the_other_then_the_other_alignment() {
        let placements = PopoverConfig::default()
            .side(PopoverSide::Right)
            .align(PopoverAlign::End)
            .placements();
        let found: Vec<_> = placements.iter().map(|p| (p.side, p.align)).collect();
        assert_eq!(
            found,
            [
                (PopoverSide::Right, PopoverAlign::End),
                (PopoverSide::Left, PopoverAlign::End),
                (PopoverSide::Right, PopoverAlign::Start),
                (PopoverSide::Left, PopoverAlign::Start),
            ]
        );
        let centred = PopoverConfig::default()
            .align(PopoverAlign::Center)
            .placements();
        assert_eq!(centred.len(), 2, "centred has no opposite alignment");
    }
}
