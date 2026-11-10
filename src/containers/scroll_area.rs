//! Scroll areas: a box whose contents can be taller (or wider) than it,
//! scrolled by the mouse wheel, a trackpad, or a scrollbar.
//!
//! Each [`ScrollArea`] displays its contents in a `view`, and has `horizontal`
//! and or `vertical` scrollbars.

use bevy::ecs::query::QueryData;
use bevy::input::mouse::{MouseScrollPixelsPerLine, MouseScrollUnit};
use bevy::picking::events::{PointerPress, PointerScroll};
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui::{UiGlobalTransform, UiSystems};
use bevy::ui_widgets::ScrollIntoView;

use crate::EnsurePlugin;
use crate::motion::{ResponseCurve, Tween};
use crate::sanitize::at_least;
use crate::theme::palette::{HIGHLIGHT, TEXT_MUTED};
use crate::theme::skins;
use crate::theme::{Shade, Theme, ThemePlugin, ThemeSkin, ThemeToken, WidgetFlags, WidgetState};
use crate::widgets::grabber::{
    GrabAxis, GrabPhase, Grabbed, GrabberConfig, GrabberWidgetPlugin, spawn_grabber,
};

/// Prefix of the scroll container's theme tokens.
pub const SCROLL_AREA_STYLE: &str = "touchy.scroll_area";

/// How long a wheel step's glide takes, in seconds.
const DEFAULT_GLIDE_SECONDS: f32 = 0.18;
/// How a wheel step's glide eases: out, so it starts at once and settles.
const GLIDE_CURVE: ResponseCurve = ResponseCurve::Power(0.5);
/// How thick a scrollbar is, in pixels, unless the theme says otherwise.
const DEFAULT_BAR_WIDTH: f32 = 6.0;
/// The shortest a thumb gets, in pixels, however long the contents.
const MIN_THUMB_LENGTH: f32 = 24.0;
/// How faint a resting thumb is.
const THUMB_ALPHA: f32 = 0.5;
/// How near its end, in pixels, a container counts as at its end.
const END_TOLERANCE: f32 = 1.0;
/// Further than any contents reach: a sticky view is sent here before layout,
/// which stops it at its end, wherever that has got to.
const FAR_END: f32 = 1.0e7;

/// Theme tokens for a scroll container: the track (clear by default) and the thumb (faint at
/// rest and bright while hovered or dragged).
#[derive(Component, Clone, PartialEq, Debug)]
#[component(immutable)]
pub struct ScrollAreaTokens {
    /// The track behind the thumb.
    pub track: ThemeToken,
    /// The clickable indicator on the scroll bar.
    pub thumb: ThemeToken,
    /// The cross-axis dimension of the scrollbar.
    pub bar_width: ThemeToken,
}

impl ScrollAreaTokens {
    /// Derive tokens from a given prefix in the theme.
    pub fn with_prefix(prefix: &str) -> Self {
        let token = |suffix: &str| ThemeToken::new(format!("{prefix}.{suffix}"));
        Self {
            track: token("track"),
            thumb: token("thumb"),
            bar_width: token("bar_width"),
        }
    }

    /// Register the default values for the scroll tokens, unless the app sets them.
    pub fn register_theme_defaults(&self, theme: &mut Theme) {
        let state = |state| self.thumb.clone().with_state(state);
        theme
            .default_color(self.track.clone(), Color::NONE)
            .default_derived(self.thumb.clone(), TEXT_MUTED, Shade::Alpha(THUMB_ALPHA))
            .default_derived(state(WidgetState::Hover), HIGHLIGHT, Shade::Same)
            .default_derived(state(WidgetState::Pressed), HIGHLIGHT, Shade::Same)
            .default_size(self.bar_width.clone(), DEFAULT_BAR_WIDTH);
        // Each part draws its role's skin, once a theme gives the role one.
        theme
            .default_skin_from(self.track.clone(), skins::SUNKEN)
            .default_skin_from(self.thumb.clone(), skins::KNOB);
    }
}

/// The built-in token set.
impl Default for ScrollAreaTokens {
    fn default() -> Self {
        Self::with_prefix(SCROLL_AREA_STYLE)
    }
}

/// Which ways a container scrolls.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ScrollAxes {
    /// Container scrolls up and down.
    #[default]
    Vertical,
    /// Container scrolls left and right.
    Horizontal,
    /// Container scrolls vertically and horizontally.
    Both,
}

impl ScrollAxes {
    fn vertical(self) -> bool {
        matches!(self, Self::Vertical | Self::Both)
    }

    fn horizontal(self) -> bool {
        matches!(self, Self::Horizontal | Self::Both)
    }

    /// Keeps only the parts of `offset` along these axes.
    fn along(self, offset: Vec2) -> Vec2 {
        Vec2::new(
            if self.horizontal() { offset.x } else { 0.0 },
            if self.vertical() { offset.y } else { 0.0 },
        )
    }
    /// `keep`, with its parts along these axes replaced by `with`'s.
    fn replace(self, keep: Vec2, with: Vec2) -> Vec2 {
        Vec2::new(
            if self.horizontal() { with.x } else { keep.x },
            if self.vertical() { with.y } else { keep.y },
        )
    }
}

/// Construction-time configuration for a scroll container.
///
/// Consumed by [`SpawnUi::scroll_area`](crate::spawn_ui::SpawnUi::scroll_area) and not retained as
/// a component.
///
/// # Examples
/// ```
/// use touchy::prelude::*;
///
/// let log = ScrollAreaConfig::new(ScrollAxes::Vertical)
///     .stick_to_end(true);
/// # assert!(log.stick_to_end);
/// ```
#[derive(Clone, Debug)]
pub struct ScrollAreaConfig {
    /// Which directions the container scrolls.
    pub axes: ScrollAxes,
    /// How long a wheel step glides for, in seconds; `None` jumps.
    pub glide: Option<f32>,
    /// Whether scrolling to the end keeps the scroll there even as new contents grow.
    pub stick_to_end: bool,
    /// The tallest it grows with its contents, in pixels, before scrolling.
    /// Without one, it fills what its parent gives it.
    pub max_height: Option<f32>,
    /// Theme tokens for the scroll container: the track, the thumb, and the bar's thickness.
    pub tokens: ScrollAreaTokens,
}

impl Default for ScrollAreaConfig {
    fn default() -> Self {
        Self {
            axes: ScrollAxes::default(),
            glide: Some(DEFAULT_GLIDE_SECONDS),
            stick_to_end: false,
            max_height: None,
            tokens: ScrollAreaTokens::with_prefix(SCROLL_AREA_STYLE),
        }
    }
}

impl ScrollAreaConfig {
    /// Create a new configuration for a container that scrolls on `axes`.
    pub fn new(axes: ScrollAxes) -> Self {
        Self::default().axes(axes)
    }
    /// A container scrolling up and down.
    pub fn vertical() -> Self {
        Self::default()
    }
    /// A container scrolling left and right.
    pub fn horizontal() -> Self {
        Self::default().axes(ScrollAxes::Horizontal)
    }
    /// A container scrolling both ways.
    pub fn both() -> Self {
        Self::default().axes(ScrollAxes::Both)
    }
    /// Which directions the container scrolls.
    pub fn axes(mut self, axes: ScrollAxes) -> Self {
        self.axes = axes;
        self
    }
    /// How long a wheel step glides for, or `None` to jump.
    pub fn glide(mut self, seconds: Option<f32>) -> Self {
        self.glide = seconds;
        self
    }
    /// Maximum height before scrolling, in pixels. Without one, fills available space.
    pub fn max_height(mut self, pixels: f32) -> Self {
        self.max_height = Some(pixels);
        self
    }
    /// Keeps the scroll area pinned to its end as its contents grow.
    pub fn stick_to_end(mut self, stick: bool) -> Self {
        self.stick_to_end = stick;
        self
    }
    /// The theme tokens for the scroll container: the track, the thumb, the bar containing them.
    pub fn tokens(mut self, tokens: ScrollAreaTokens) -> Self {
        self.tokens = tokens;
        self
    }
    /// This config with every value made usable, warning about each repair.
    fn sanitized(mut self) -> Self {
        self.max_height = at_least(self.max_height, 0.0, "ScrollConfig.max_height");
        self.glide = match self.glide {
            Some(seconds) if seconds > 0.0 => Some(seconds),
            // No time to glide in: it jumps, as documented.
            Some(0.0) | None => None,
            Some(seconds) => {
                warn!("ScrollConfig.glide was {seconds} seconds; jumping instead");
                None
            }
        };
        self
    }
}

/// A scroll area: the node its contents are in, and its scrollbars.
#[derive(Component, Clone, Copy, Debug)]
pub struct ScrollArea {
    /// The node holding the contents to be scrolled.
    view: Entity,
    /// The horizontal scrollbar, if any.
    horizontal: Option<Entity>,
    /// The vertical scrollbar, if any.
    vertical: Option<Entity>,
}

impl ScrollArea {
    /// The Entity holding the contents to be scrolled.
    pub fn view(&self) -> Entity {
        self.view
    }
}

/// Scrolls a container to `offset`, in pixels from the start of its contents,
/// clamped to what there is. Trigger it on the container, or on its view.
#[derive(EntityEvent, Clone, Copy, Debug, PartialEq)]
pub struct ScrollTo {
    /// The container to scroll.
    #[event_target]
    pub container: Entity,
    /// The offset to scroll to, in pixels from the start of the contents.
    pub offset: Vec2,
    /// Glide there, as a wheel step does, rather than jumping.
    pub glide: bool,
}

/// A scrollbar's settings, describing how it should behave.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct ScrollAreaSettings {
    /// Which directions the view scrolls.
    pub axes: ScrollAxes,
    /// How long a wheel step glides for, in seconds; `None` jumps.
    pub glide: Option<f32>,
    /// Whether scrolling to the end keeps the scroll there even as new contents grow.
    pub stick_to_end: bool,
}

impl ScrollAreaSettings {
    /// Whether a view at `offset`, in contents that reach `reach`, is at its end.
    fn reaches_end(&self, offset: Vec2, reach: Vec2) -> bool {
        (self.axes.along(reach) - self.axes.along(offset)).max_element() <= END_TOLERANCE
    }
}

/// A view's current state: where it's going and how.
#[derive(Component, Clone, Copy, Debug)]
struct ScrollState {
    /// Where a run of wheel steps is going, and the glide there.
    target: Vec2,
    /// How far along the glide is, if any.
    tween: Option<Tween<Vec2>>,
    /// The position last written, to notice when a scrollbar moves it instead.
    written: Vec2,
    /// Whether it was at its end, as of the last layout.
    at_end: bool,
}

impl ScrollState {
    /// Where the view is heading: its glide's target, or where it is if it isn't gliding.
    fn heading(&self, position: Vec2) -> Vec2 {
        if self.tween.is_some() {
            self.target
        } else {
            position
        }
    }
}

/// A view, as the observers that scroll it work on it.
#[derive(QueryData)]
#[query_data(mutable)]
struct ScrollView {
    settings: &'static ScrollAreaSettings,
    state: &'static mut ScrollState,
    position: &'static mut ScrollPosition,
    node: &'static ComputedNode,
}

impl ScrollViewItem<'_, '_> {
    /// The furthest it can scroll, from its last layout, in logical pixels.
    fn reach(&self) -> Vec2 {
        reach(self.node)
    }
    /// `offset` kept to what there is to scroll to.
    fn clamp(&self, offset: Vec2) -> Vec2 {
        offset.clamp(Vec2::ZERO, self.reach())
    }
    /// Sends it to `to`, within what there is to scroll to: gliding over
    /// `glide` seconds, or at once.
    fn move_to(&mut self, to: Vec2, glide: Option<f32>) {
        let to = self.clamp(to);
        let state = &mut *self.state;
        state.target = to;
        state.at_end = self.settings.reaches_end(to, reach(self.node));
        match glide {
            Some(seconds) => {
                state.tween = Some(Tween::new(self.position.0, to, seconds, GLIDE_CURVE));
            }
            None => {
                state.tween = None;
                self.position.0 = to;
                state.written = to;
            }
        }
    }
}

/// An entire scrollbar: its view, its thumb, and where a drag of the thumb began.
#[derive(Component, Clone, Copy, Debug)]
struct Bar {
    view: Entity,
    thumb: Entity,
    vertical: bool,
    dragged_from: Option<f32>,
}

impl Bar {
    /// The single-axis magnitude of a given `value`.
    fn along(&self, value: Vec2) -> f32 {
        if self.vertical { value.y } else { value.x }
    }
    /// `value`, with its magnitude along this bar set to `along`.
    fn with_along(&self, value: Vec2, along: f32) -> Vec2 {
        if self.vertical {
            value.with_y(along)
        } else {
            value.with_x(along)
        }
    }
    /// A track's thickness, and how far short of its far end it stops.
    fn across(&self, node: &Node) -> (Val, Val) {
        if self.vertical {
            (node.width, node.bottom)
        } else {
            (node.height, node.right)
        }
    }
    /// Set a track's thickness, and how far short of its far end it stops.
    fn set_across(&self, node: &mut Node, (thickness, short_of): (Val, Val)) {
        if self.vertical {
            (node.width, node.bottom) = (thickness, short_of);
        } else {
            (node.height, node.right) = (thickness, short_of);
        }
    }
    /// Where a thumb starts along its track, and how long it is.
    fn span(&self, node: &Node) -> (Val, Val) {
        if self.vertical {
            (node.top, node.height)
        } else {
            (node.left, node.width)
        }
    }
    /// Set where a thumb starts along its track, and how long it is.
    fn set_span(&self, node: &mut Node, (start, length): (Val, Val)) {
        if self.vertical {
            (node.top, node.height) = (start, length);
        } else {
            (node.left, node.width) = (start, length);
        }
    }
}

/// The grabber filling a thumb node, and the Bar it belongs to.
#[derive(Component, Clone, Copy, Debug)]
struct ThumbGrip(Entity);

/// Spawns it; documented on [`SpawnUi::scroll_area`](crate::spawn_ui::SpawnUi::scroll_area).
pub(crate) fn spawn_scroll_area<'a>(
    parent: &'a mut ChildSpawnerCommands,
    config: ScrollAreaConfig,
    build: impl FnOnce(&mut ChildSpawnerCommands),
) -> EntityCommands<'a> {
    let ScrollAreaConfig {
        axes,
        glide,
        stick_to_end,
        max_height,
        tokens,
    } = config.sanitized();
    // Filling its parent, the view is as tall as the container; capped, the
    // container is as tall as the view, which grows with its contents.
    let (grow, view_height, view_max) = match max_height {
        Some(cap) => (0.0, Val::Auto, px(cap)),
        None => (1.0, percent(100), Val::Auto),
    };
    let scroll_if = |on| {
        if on {
            OverflowAxis::Scroll
        } else {
            OverflowAxis::Clip
        }
    };
    let overflow = Overflow {
        x: scroll_if(axes.horizontal()),
        y: scroll_if(axes.vertical()),
    };
    let mut container = parent.spawn(Node {
        flex_grow: grow,
        min_width: px(0.0),
        min_height: px(0.0),
        ..default()
    });
    let (mut view, mut vertical, mut horizontal) = (Entity::PLACEHOLDER, None, None);
    container.with_children(|inside| {
        view = inside
            .spawn((
                Node {
                    width: percent(100),
                    height: view_height,
                    max_height: view_max,
                    flex_direction: FlexDirection::Column,
                    overflow,
                    ..default()
                },
                ScrollPosition::default(),
                ScrollAreaSettings {
                    axes,
                    glide,
                    stick_to_end,
                },
                ScrollState {
                    target: Vec2::ZERO,
                    tween: None,
                    written: Vec2::ZERO,
                    at_end: true,
                },
            ))
            .with_children(build)
            .id();
        // Bars spawn after the view, so they lie over its edge.
        vertical = axes
            .vertical()
            .then(|| spawn_bar(inside, view, true, &tokens));
        horizontal = axes
            .horizontal()
            .then(|| spawn_bar(inside, view, false, &tokens));
    });
    container.insert((
        ScrollArea {
            view,
            horizontal,
            vertical,
        },
        tokens,
    ));
    container
}

/// A scrollbar *over* the view's edge, with its thumb, hidden until needed.
fn spawn_bar(
    inside: &mut ChildSpawnerCommands,
    view: Entity,
    vertical: bool,
    tokens: &ScrollAreaTokens,
) -> Entity {
    let (zero, full) = (px(0.0), percent(100));
    let edge = Node {
        position_type: PositionType::Absolute,
        right: zero,
        bottom: zero,
        ..default()
    };
    let (track, thumb_node, axis) = if vertical {
        (
            Node { top: zero, ..edge },
            Node {
                width: full,
                ..default()
            },
            GrabAxis::Vertical,
        )
    } else {
        (
            Node { left: zero, ..edge },
            Node {
                height: full,
                ..default()
            },
            GrabAxis::Horizontal,
        )
    };
    // The track's colour comes with the container's tokens, in `style_tracks`.
    let mut bar = inside.spawn((track, Visibility::Hidden));
    let bar_id = bar.id();
    let mut thumb = Entity::PLACEHOLDER;
    bar.with_children(|bar| {
        let node = Node {
            position_type: PositionType::Absolute,
            border_radius: BorderRadius::MAX,
            ..thumb_node
        };
        thumb = bar
            .spawn((node, ThemeSkin(tokens.thumb.clone()), Hovered::default()))
            .with_children(|thumb| {
                spawn_grabber(thumb, GrabberConfig::ghost().axis(axis)).insert(ThumbGrip(bar_id));
            })
            .id();
    });
    bar.insert(Bar {
        view,
        thumb,
        vertical,
        dragged_from: None,
    });
    bar_id
}

/// A thumb's length and its distance along a track.
/// View shows `visible` of contents `content` long, scrolled by `offset`.
fn thumb_span(track_length: f32, visible: f32, content: f32, offset: f32) -> (f32, f32) {
    if content <= visible || content <= 0.0 {
        return (track_length, 0.0);
    }
    // A track too short for the least thumb still leaves it room to travel.
    let least = MIN_THUMB_LENGTH.min(track_length / 2.0);
    let length = (track_length * visible / content)
        .max(least)
        .min(track_length);
    let travel = track_length - length;
    let reach = content - visible;
    (length, (offset / reach).clamp(0.0, 1.0) * travel)
}

/// Scrolls a view by dragging its thumb: the thumb follows the hand, so the
/// view moves by as much more as its contents are longer than the track.
fn drag_thumbs(
    grabbed: On<Grabbed>,
    grips: Query<&ThumbGrip>,
    mut bars: Query<(&mut Bar, &ComputedNode)>,
    thumbs: Query<&ComputedNode, Without<Bar>>,
    mut views: Query<ScrollView, Without<Bar>>,
) {
    let Ok(&ThumbGrip(bar)) = grips.get(grabbed.grabber) else {
        return;
    };
    let Ok((mut bar, track)) = bars.get_mut(bar) else {
        return;
    };
    let Ok(mut view) = views.get_mut(bar.view) else {
        return;
    };
    let reach = view.reach();
    match (grabbed.phase, bar.dragged_from) {
        (GrabPhase::Start, _) => bar.dragged_from = Some(bar.along(view.position.0)),
        (_, Some(from)) => {
            let logical = |node: &ComputedNode| node.size() * node.inverse_scale_factor;
            let thumb = thumbs.get(bar.thumb).map_or(Vec2::ZERO, logical);
            let travel = bar.along(logical(track)) - bar.along(thumb);
            let per_pixel = if travel > 0.0 {
                bar.along(reach) / travel
            } else {
                0.0
            };
            let moved = from + bar.along(grabbed.offset) * per_pixel;
            let to = bar.with_along(view.position.0, moved);
            view.move_to(to, None);
            // A cancel's offset is zero, so the view goes back where it was.
            if matches!(grabbed.phase, GrabPhase::End | GrabPhase::Cancel) {
                bar.dragged_from = None;
            }
        }
        _ => {}
    }
}

/// Clicking a track, beside its thumb, glides the view a page toward the click.
fn page_on_track_press(
    mut press: On<PointerPress>,
    bars: Query<(&Bar, &ComputedNode)>,
    thumbs: Query<&UiGlobalTransform>,
    mut views: Query<ScrollView>,
) {
    // Only a press on the track itself; one on the thumb is a drag.
    if press.original_event_target() != press.entity {
        return;
    }
    let Ok((bar, track)) = bars.get(press.entity) else {
        return;
    };
    press.propagate(false);
    let Ok(thumb_at) = thumbs.get(bar.thumb) else {
        return;
    };
    let Ok(mut view) = views.get_mut(bar.view) else {
        return;
    };
    // Both in physical pixels: the press's position is logical.
    let pressed = press.pointer.position / track.inverse_scale_factor;
    let toward = (bar.along(pressed) - bar.along(thumb_at.translation)).signum();
    let page = bar.along(view.node.size() * view.node.inverse_scale_factor) * toward;
    let from = view.state.heading(view.position.0);
    let to = bar.with_along(from, bar.along(from) + page);
    let glide = view.settings.glide;
    view.move_to(to, glide);
}

/// The furthest a view can scroll, from its last layout, in logical pixels.
fn reach(node: &ComputedNode) -> Vec2 {
    let visible = node.size() - node.scrollbar_size;
    ((node.content_size() - visible) * node.inverse_scale_factor).max(Vec2::ZERO)
}

/// Turns the wheel into scrolling: glided for a wheel's steps, direct for a
/// trackpad's. A view that can't move any further lets the scroll go on to
/// the one around it.
fn scroll_on_wheel(
    mut scroll: On<PointerScroll>,
    containers: Query<&ScrollArea>,
    mut views: Query<ScrollView>,
    pixels_per_line: Option<Res<MouseScrollPixelsPerLine>>,
) {
    let view = containers
        .get(scroll.entity)
        .map_or(scroll.entity, ScrollArea::view);
    let Ok(mut view) = views.get_mut(view) else {
        return;
    };
    let axes = view.settings.axes;
    let lines = scroll.unit == MouseScrollUnit::Line;
    let ratio = pixels_per_line.map(|ratio| *ratio).unwrap_or_default();
    let delta = Vec2::new(scroll.x, scroll.y) * if lines { *ratio } else { 1.0 };
    // A wheel turned only up and down scrolls sideways where that's all there is.
    let delta = if axes == ScrollAxes::Horizontal && delta.x == 0.0 {
        Vec2::new(delta.y, 0.0)
    } else {
        delta
    };
    let from = view.state.heading(view.position.0);
    let to = view.clamp(from - axes.along(delta));
    if to == from {
        return;
    }
    scroll.propagate(false);
    let glide = view.settings.glide.filter(|_| lines);
    view.move_to(to, glide);
}

/// Scrolls the container named in a [`ScrollTo`].
fn scroll_to(request: On<ScrollTo>, containers: Query<&ScrollArea>, mut views: Query<ScrollView>) {
    let view = containers
        .get(request.container)
        .map_or(request.container, ScrollArea::view);
    if let Ok(mut view) = views.get_mut(view) {
        let to = view.settings.axes.along(request.offset);
        let glide = view.settings.glide.filter(|_| request.glide);
        view.move_to(to, glide);
    }
}

/// Glides a view just far enough to show whatever `ScrollIntoView` was
/// triggered on inside it, whole, or as much of it as fits.
fn reveal_in_view(
    mut reveal: On<ScrollIntoView>,
    parents: Query<&ChildOf>,
    items: Query<(&ComputedNode, &UiGlobalTransform)>,
    mut views: Query<ScrollView>,
) {
    // It doesn't bubble by itself: find the nearest view around the item.
    let item = reveal.entity;
    let Some(view_entity) = parents.iter_ancestors(item).find(|&up| views.contains(up)) else {
        return;
    };
    let Ok(mut view_state) = views.get_mut(view_entity) else {
        return;
    };
    let (Ok((view, view_at)), Ok((shown, shown_at))) = (items.get(view_entity), items.get(item))
    else {
        return;
    };
    reveal.propagate(false);
    let logical = |node: &ComputedNode, at: &UiGlobalTransform| {
        let size = node.size() * node.inverse_scale_factor;
        (
            at.translation * node.inverse_scale_factor - size / 2.0,
            size,
        )
    };
    let (view_corner, visible) = logical(view, view_at);
    let (item_corner, size) = logical(shown, shown_at);
    // Where the item is in the contents, from where the view is showing now.
    let position = view_state.position.0;
    let start = item_corner - view_corner + position;
    let end = start + size;
    let from = view_state.state.heading(position);
    let mut to = from;
    for axis in 0..2 {
        if start[axis] < to[axis] {
            to[axis] = start[axis];
        } else if end[axis] > to[axis] + visible[axis] {
            to[axis] = (end[axis] - visible[axis]).min(start[axis]);
        }
    }
    // Only along the axes it scrolls; the others stay where they are.
    let to = view_state
        .settings
        .axes
        .replace(position, view_state.clamp(to));
    if to != from {
        let glide = view_state.settings.glide;
        view_state.move_to(to, glide);
    }
}

/// Moves each gliding view along, and keeps a view that sticks to its end
/// there as its contents grow. A view moved by something else, such as a
/// scrollbar's thumb, drops its glide.
fn advance_scrolls(
    time: Res<Time<Real>>,
    mut views: Query<(&mut ScrollState, &mut ScrollPosition)>,
) {
    for (mut scrolling, mut position) in &mut views {
        // Moved by something else, such as a scrollbar's thumb: it isn't stuck
        // to its end until layout finds it there again.
        if position.0 != scrolling.written {
            scrolling.tween = None;
            scrolling.target = position.0;
            scrolling.written = position.0;
            scrolling.at_end = false;
        }
        let Some(mut tween) = scrolling.tween else {
            continue;
        };
        let (at, arrived) = tween.advance(time.delta_secs());
        scrolling.tween = (!arrived).then_some(tween);
        position.0 = at;
        scrolling.written = at;
    }
}

/// Before layout: sends a view that sticks to its end, and is there, past
/// any end, so layout stops it at its end however far its contents have grown,
/// in the same frame.
fn stick_to_ends(mut views: Query<(&ScrollAreaSettings, &ScrollState, &mut ScrollPosition)>) {
    for (settings, state, mut position) in &mut views {
        if settings.stick_to_end && state.at_end && state.tween.is_none() {
            position.0 = settings.axes.replace(position.0, Vec2::splat(FAR_END));
        }
    }
}

/// After layout: brings each view's position back within what its contents
/// reach, as layout already showed it, and notes whether it's at its end.
fn settle_scrolls(
    mut views: Query<(
        &ScrollAreaSettings,
        &mut ScrollState,
        &mut ScrollPosition,
        &ComputedNode,
    )>,
) {
    for (settings, mut scrolling, mut position, node) in &mut views {
        let reach = reach(node);
        let settled = position.0.clamp(Vec2::ZERO, reach);
        if position.0 != settled {
            position.0 = settled;
        }
        if scrolling.written != position.0 && scrolling.tween.is_none() {
            scrolling.written = position.0;
            scrolling.target = position.0;
        }
        let at_end = settings.reaches_end(settled, reach);
        if scrolling.at_end != at_end {
            scrolling.at_end = at_end;
        }
    }
}

/// Shows each scrollbar only while there's something to scroll to, sized by
/// the theme, with its thumb sized and placed for the view's scroll and lit
/// while hovered or dragged.
fn restyle_bars(
    theme: Res<Theme>,
    containers: Query<(&ScrollArea, &ScrollAreaTokens)>,
    views: Query<(&ComputedNode, &ScrollPosition)>,
    mut bars: Query<(&Bar, &mut Node, &mut Visibility, &ComputedNode)>,
    mut thumbs: Query<(&mut Node, &Hovered, &ThemeSkin), Without<Bar>>,
    mut commands: Commands,
) {
    for (container, tokens) in &containers {
        let Ok((view, position)) = views.get(container.view) else {
            continue;
        };
        let reach = reach(view);
        let visible = view.size() * view.inverse_scale_factor;
        let content = view.content_size() * view.inverse_scale_factor;
        let width = px(theme.size(&tokens.bar_width));
        // With both bars, each stops short of the corner the other runs into.
        let corner = if container.vertical.is_some() && container.horizontal.is_some() {
            width
        } else {
            px(0.0)
        };
        for bar in [container.vertical, container.horizontal]
            .into_iter()
            .flatten()
        {
            let Ok((bar, mut node, mut visibility, track)) = bars.get_mut(bar) else {
                continue;
            };
            visibility.set_if_neq(if bar.along(reach) > 0.0 {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            });
            if bar.across(&node) != (width, corner) {
                bar.set_across(&mut node, (width, corner));
            }
            let Ok((mut thumb, hovered, colour)) = thumbs.get_mut(bar.thumb) else {
                continue;
            };
            let length = bar.along(track.size() * track.inverse_scale_factor);
            let (span, at) = thumb_span(
                length,
                bar.along(visible),
                bar.along(content),
                bar.along(position.0),
            );
            let place = (px(at), px(span));
            if bar.span(&thumb) != place {
                bar.set_span(&mut thumb, place);
            }
            let state = WidgetFlags {
                hovered: hovered.0,
                pressed: bar.dragged_from.is_some(),
                ..default()
            }
            .state();
            let wanted = ThemeSkin(tokens.thumb.clone().with_state(state));
            if *colour != wanted {
                commands.entity(bar.thumb).insert(wanted);
            }
        }
    }
}

/// Colours a container's tracks when its tokens are set: at spawn, and
/// whenever the app sets new ones. Thumbs follow the tokens in `restyle_bars`.
fn style_tracks(
    insert: On<Insert<ScrollAreaTokens>>,
    containers: Query<(&ScrollArea, &ScrollAreaTokens)>,
    mut commands: Commands,
) {
    let Ok((container, tokens)) = containers.get(insert.entity) else {
        return;
    };
    for bar in [container.vertical, container.horizontal]
        .into_iter()
        .flatten()
    {
        commands.entity(bar).insert(ThemeSkin(tokens.track.clone()));
    }
}

/// Scroll areas: wheel and trackpad scrolling, scrollbars, and default
/// token values.
pub struct ScrollAreaPlugin;

impl Plugin for ScrollAreaPlugin {
    fn build(&self, app: &mut App) {
        app.ensure_plugin(ThemePlugin)
            .ensure_plugin(GrabberWidgetPlugin)
            .add_observer(style_tracks)
            .add_observer(scroll_on_wheel)
            .add_observer(drag_thumbs)
            .add_observer(page_on_track_press)
            .add_observer(scroll_to)
            .add_observer(reveal_in_view)
            .add_systems(Update, advance_scrolls)
            .add_systems(
                PostUpdate,
                (
                    (stick_to_ends, restyle_bars).before(UiSystems::Layout),
                    settle_scrolls.after(UiSystems::Layout),
                ),
            );
        ScrollAreaTokens::with_prefix(SCROLL_AREA_STYLE)
            .register_theme_defaults(&mut app.world_mut().resource_mut::<Theme>());
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy::camera::NormalizedRenderTarget;
    use bevy::input::touch::TouchPhase;
    use bevy::picking::backend::HitData;
    use bevy::picking::events::Pointer;
    use bevy::picking::pointer::{Location, PointerId};
    use bevy::time::TimeUpdateStrategy;
    use bevy::ui_widgets::ScrollIntoView;

    use super::*;

    const FRAME_MS: u64 = 16;
    const VISIBLE: Vec2 = Vec2::new(200.0, 100.0);

    fn scroll_app(config: ScrollAreaConfig) -> (App, Entity, Entity) {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, ScrollAreaPlugin))
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
                FRAME_MS,
            )));
        let mut container = None;
        app.world_mut()
            .commands()
            .spawn(Node::default())
            .with_children(|parent| {
                container = Some(spawn_scroll_area(parent, config, |_| {}).id())
            });
        app.update();
        let container = container.unwrap();
        let view = app.world().get::<ScrollArea>(container).unwrap().view();
        (app, container, view)
    }

    /// Tells the view how big it and its contents are, since tests run no layout.
    fn lay_out(app: &mut App, view: Entity, content: Vec2) {
        app.world_mut().entity_mut(view).insert(ComputedNode {
            size: VISIBLE,
            content_size: content,
            inverse_scale_factor: 1.0,
            ..ComputedNode::DEFAULT
        });
    }

    fn wheel(app: &mut App, over: Entity, unit: MouseScrollUnit, y: f32) {
        let pointer = Pointer::new(
            PointerId::Mouse,
            Location {
                target: NormalizedRenderTarget::None {
                    width: 1,
                    height: 1,
                },
                position: Vec2::ZERO,
            },
        );
        app.world_mut().trigger(PointerScroll {
            entity: over,
            pointer,
            unit,
            x: 0.0,
            y,
            hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
            phase: TouchPhase::Moved,
        });
    }

    fn offset(app: &App, view: Entity) -> Vec2 {
        app.world().get::<ScrollPosition>(view).unwrap().0
    }

    #[test]
    fn a_wheel_step_glides_and_a_trackpad_moves_at_once() {
        let (mut app, container, view) = scroll_app(ScrollAreaConfig::vertical());
        lay_out(&mut app, view, Vec2::new(200.0, 1000.0));
        let line = *MouseScrollPixelsPerLine::default();
        wheel(&mut app, view, MouseScrollUnit::Line, -1.0);
        app.update();
        let partway = offset(&app, view).y;
        assert!(partway > 0.0 && partway < line, "on its way: {partway}");
        for _ in 0..30 {
            app.update();
        }
        assert_eq!(offset(&app, view), Vec2::new(0.0, line), "one line down");

        // Over the scrollbar, which belongs to the container rather than the view.
        wheel(&mut app, container, MouseScrollUnit::Pixel, -30.0);
        assert_eq!(offset(&app, view).y, line + 30.0, "at once");
        wheel(&mut app, view, MouseScrollUnit::Pixel, -5000.0);
        assert_eq!(offset(&app, view).y, 900.0, "no further than the end");
    }

    #[test]
    fn a_view_at_its_end_lets_the_wheel_go_on() {
        let (mut app, _, view) = scroll_app(ScrollAreaConfig::vertical());
        lay_out(&mut app, view, Vec2::new(200.0, 1000.0));
        // Pointer events look for windows on their way up, as a real app has.
        app.world_mut().register_component::<Window>();
        #[derive(Resource, Default)]
        struct Passed(u32);
        app.init_resource::<Passed>().add_observer(
            move |scroll: On<PointerScroll>,
                  parents: Query<&ChildOf>,
                  mut passed: ResMut<Passed>| {
                if parents.iter_ancestors(view).any(|up| up == scroll.entity) {
                    passed.0 += 1;
                }
            },
        );
        wheel(&mut app, view, MouseScrollUnit::Pixel, 40.0);
        assert!(
            app.world().resource::<Passed>().0 > 0,
            "at the top, scrolling up goes to whatever's around it"
        );
        let passed = app.world().resource::<Passed>().0;
        wheel(&mut app, view, MouseScrollUnit::Pixel, -40.0);
        assert_eq!(
            app.world().resource::<Passed>().0,
            passed,
            "scrolling down, it's this view's"
        );
    }

    #[test]
    fn a_sticky_view_follows_its_end_until_scrolled_back() {
        let (mut app, container, view) =
            scroll_app(ScrollAreaConfig::vertical().stick_to_end(true));
        lay_out(&mut app, view, Vec2::new(200.0, 300.0));
        app.update();
        app.update();
        assert_eq!(offset(&app, view).y, 200.0, "at its end");
        lay_out(&mut app, view, Vec2::new(200.0, 450.0));
        app.update();
        assert_eq!(offset(&app, view).y, 350.0, "kept at its end as it grew");

        app.world_mut().trigger(ScrollTo {
            container,
            offset: Vec2::new(0.0, 100.0),
            glide: false,
        });
        app.update();
        lay_out(&mut app, view, Vec2::new(200.0, 600.0));
        app.update();
        assert_eq!(offset(&app, view).y, 100.0, "scrolled back, it stays put");
    }

    #[test]
    fn a_scrollbar_shows_only_while_theres_something_to_scroll_to() {
        let (mut app, container, view) = scroll_app(ScrollAreaConfig::vertical());
        let bar = app
            .world()
            .get::<ScrollArea>(container)
            .unwrap()
            .vertical
            .unwrap();
        let shown = |app: &App| app.world().get::<Visibility>(bar) != Some(&Visibility::Hidden);
        lay_out(&mut app, view, Vec2::new(200.0, 80.0));
        app.update();
        assert!(!shown(&app), "it all fits");
        assert_eq!(
            app.world().get::<Node>(bar).unwrap().width,
            px(DEFAULT_BAR_WIDTH),
            "as thick as the theme says"
        );
        lay_out(&mut app, view, Vec2::new(200.0, 400.0));
        app.update();
        assert!(shown(&app));
        assert!(
            app.world()
                .get::<ScrollArea>(container)
                .unwrap()
                .horizontal
                .is_none(),
            "no sideways bar for a vertical container"
        );
    }

    #[test]
    fn a_thumb_is_as_long_as_the_view_is_of_its_contents() {
        assert_eq!(thumb_span(100.0, 100.0, 400.0, 0.0), (25.0, 0.0));
        assert_eq!(
            thumb_span(100.0, 100.0, 400.0, 300.0),
            (25.0, 75.0),
            "scrolled to the end, at the end"
        );
        assert_eq!(
            thumb_span(100.0, 100.0, 10_000.0, 0.0).0,
            MIN_THUMB_LENGTH,
            "never too small to grab"
        );
        assert_eq!(thumb_span(100.0, 100.0, 80.0, 0.0), (100.0, 0.0));
        let (length, _) = thumb_span(20.0, 20.0, 100.0, 0.0);
        assert!(
            length < 20.0,
            "a short track still leaves the thumb room to move"
        );
    }

    #[test]
    fn dragging_a_thumb_moves_the_view_by_the_contents_per_track_pixel() {
        let (mut app, container, view) = scroll_app(ScrollAreaConfig::vertical());
        lay_out(&mut app, view, Vec2::new(200.0, 400.0));
        let bar = app
            .world()
            .get::<ScrollArea>(container)
            .unwrap()
            .vertical
            .unwrap();
        let thumb = app.world().get::<Bar>(bar).unwrap().thumb;
        let sized = |size: Vec2| ComputedNode {
            size,
            inverse_scale_factor: 1.0,
            ..ComputedNode::DEFAULT
        };
        app.world_mut()
            .entity_mut(bar)
            .insert(sized(Vec2::new(6.0, 100.0)));
        app.world_mut()
            .entity_mut(thumb)
            .insert(sized(Vec2::new(6.0, 25.0)));
        let world = app.world_mut();
        let grip = world
            .query_filtered::<Entity, With<ThumbGrip>>()
            .single(world)
            .unwrap();
        let grab = |app: &mut App, phase, offset| {
            app.world_mut().trigger(Grabbed {
                grabber: grip,
                phase,
                offset,
                position: Vec2::ZERO,
            });
            app.update();
        };
        grab(&mut app, GrabPhase::Start, Vec2::ZERO);
        grab(&mut app, GrabPhase::Move, Vec2::new(0.0, 25.0));
        assert_eq!(
            offset(&app, view).y,
            100.0,
            "300 px of contents over 75 px of track: 4 each"
        );
        assert_eq!(
            app.world().get::<ThemeSkin>(thumb),
            Some(&ThemeSkin(
                ScrollAreaTokens::with_prefix(SCROLL_AREA_STYLE)
                    .thumb
                    .with_state(WidgetState::Pressed)
            )),
            "lit while held"
        );
        grab(&mut app, GrabPhase::End, Vec2::new(0.0, 500.0));
        assert_eq!(offset(&app, view).y, 300.0, "no further than the end");
    }

    #[test]
    fn scrolling_into_view_glides_just_far_enough_to_show_it() {
        let (mut app, _, view) = scroll_app(ScrollAreaConfig::vertical().max_height(100.0));
        lay_out(&mut app, view, Vec2::new(200.0, 1000.0));
        app.world_mut()
            .entity_mut(view)
            .insert(UiGlobalTransform::from_translation(VISIBLE / 2.0));
        // An item 20 tall, 340 down the contents, below what's showing.
        let item = app
            .world_mut()
            .spawn((
                ComputedNode {
                    size: Vec2::new(200.0, 20.0),
                    inverse_scale_factor: 1.0,
                    ..ComputedNode::DEFAULT
                },
                UiGlobalTransform::from_translation(Vec2::new(100.0, 350.0)),
                ChildOf(view),
            ))
            .id();
        app.world_mut().trigger(ScrollIntoView { entity: item });
        for _ in 0..30 {
            app.update();
        }
        assert_eq!(offset(&app, view).y, 260.0, "its foot at the view's foot");
        assert_eq!(
            app.world().get::<Node>(view).unwrap().max_height,
            px(100.0),
            "capped, the view grows to its cap"
        );
    }

    #[test]
    fn axes_keep_only_their_own_parts() {
        let offset = Vec2::new(3.0, 4.0);
        assert_eq!(ScrollAxes::Vertical.along(offset), Vec2::new(0.0, 4.0));
        assert_eq!(ScrollAxes::Horizontal.along(offset), Vec2::new(3.0, 0.0));
        assert_eq!(ScrollAxes::Both.along(offset), offset);
    }

    #[test]
    fn axes_replace_only_their_own_parts() {
        let (keep, with) = (Vec2::new(3.0, 4.0), Vec2::new(30.0, 40.0));
        assert_eq!(
            ScrollAxes::Vertical.replace(keep, with),
            Vec2::new(3.0, 40.0)
        );
        assert_eq!(
            ScrollAxes::Horizontal.replace(keep, with),
            Vec2::new(30.0, 4.0)
        );
        assert_eq!(ScrollAxes::Both.replace(keep, with), with);
    }

    #[test]
    fn tracks_wear_the_containers_tokens_and_change_with_them() {
        let (mut app, container, _) = scroll_app(ScrollAreaConfig::vertical());
        let bar = app
            .world()
            .get::<ScrollArea>(container)
            .unwrap()
            .vertical
            .unwrap();
        let track = |app: &App| app.world().get::<ThemeSkin>(bar).unwrap().0.clone();
        assert_eq!(
            track(&app),
            ScrollAreaTokens::with_prefix(SCROLL_AREA_STYLE).track
        );
        let custom = ScrollAreaTokens::with_prefix("test.scroll");
        custom.register_theme_defaults(&mut app.world_mut().resource_mut::<Theme>());
        app.world_mut().entity_mut(container).insert(custom.clone());
        app.world_mut().flush();
        assert_eq!(track(&app), custom.track);
    }

    #[test]
    fn settings_can_change_after_spawn() {
        let (mut app, _, view) = scroll_app(ScrollAreaConfig::vertical());
        lay_out(&mut app, view, Vec2::new(200.0, 300.0));
        app.update();
        assert_eq!(
            offset(&app, view).y,
            0.0,
            "not sticky, so it stays at the top"
        );
        app.world_mut()
            .get_mut::<ScrollAreaSettings>(view)
            .unwrap()
            .stick_to_end = true;
        app.world_mut().get_mut::<ScrollState>(view).unwrap().at_end = true;
        app.update();
        app.update();
        assert_eq!(offset(&app, view).y, 200.0, "sticky now, at its end");
    }

    #[test]
    fn an_unusable_glide_jumps() {
        let glide = |seconds| {
            ScrollAreaConfig::vertical()
                .glide(Some(seconds))
                .sanitized()
                .glide
        };
        assert_eq!(glide(0.25), Some(0.25));
        assert_eq!(glide(0.0), None);
        assert_eq!(glide(-1.0), None);
        assert_eq!(glide(f32::NAN), None);
    }
}
