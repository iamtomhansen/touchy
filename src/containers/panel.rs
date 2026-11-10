//! Panels: containers placed in their parent, docked to an edge or a corner,
//! centred, or floating at a point. A panel can be resized by its edge or
//! corner, and carried from one placement to another.
//!
//! A panel is placed within its parent, so one spawned in a toolbar's body
//! docks to the body's edges, not the window's. Every placement keeps the
//! settings' margin from the parent's edges.
//!
//! # Usage
//!
//! Construct a [`PanelConfig`] and pass it to [`SpawnUi::panel`](crate::spawn_ui::SpawnUi::panel)
//! to create the panel and fill it.
//!
//! # Moving
//!
//! Change a panel's [`PanelPlacement`] or [`PanelSize`] and it moves there,
//! gliding if its [`PanelSettings`] give it an acceleration. A panel carried
//! or resized by hand arrives the same way when it's let go.
//!
//! [`PanelSize`] is the size a panel keeps whatever its placement: docked to
//! an edge, it uses one side of the size and stretches the other along the
//! edge, and it takes the whole size back when it leaves the edge. A panel
//! bigger than its parent overflows it; a hand resizing it stops at the parent.

use bevy::ecs::query::QueryData;
use bevy::prelude::*;
use bevy::ui::UiGlobalTransform;

use crate::EnsurePlugin;
use crate::adorner::{ADORNED, AdornerPlugin, HideOutline, ShowOutline};
use crate::motion::Glide;
use crate::sanitize::at_least;
use crate::theme::palette::{CORNER_RADIUS, EDGE, SURFACE};
use crate::theme::skins;
use crate::theme::{
    Shade, Theme, ThemeBorder, ThemeCornerRadius, ThemePlugin, ThemeSkin, ThemeToken,
};
use crate::widgets::grabber::{
    GrabAxis, GrabCorner, GrabPhase, Grabbed, GrabberConfig, GrabberStyle, GrabberWidgetPlugin,
    spawn_grabber,
};

/// Prefix of the panel's theme tokens.
pub const PANEL_STYLE: &str = "touchy.panel";

/// How fast a panel picks up speed gliding to a new place, in pixels per second squared.
const DEFAULT_ACCELERATION: f32 = 12_000.0;
/// Space inside the panel's edge, in pixels.
const DEFAULT_PADDING: f32 = 12.0;
/// Space between the elements in the panel, in pixels.
const DEFAULT_GAP: f32 = 0.0;
/// A panel's size unless the config gives one, in pixels.
const DEFAULT_SIZE: Vec2 = Vec2::new(280.0, 200.0);
/// The smallest a hand can resize a panel to, unless the config says otherwise, in pixels.
const DEFAULT_MIN_SIZE: f32 = 80.0;
/// The width of a panel's border, in pixels.
const BORDER_WIDTH: f32 = 1.0;
/// How near an edge of the parent the pointer must be for a panel to dock there, in pixels.
const SNAP_DISTANCE: f32 = 28.0;
/// How near a corner, along an edge, the pointer must be for a panel to dock there, in pixels.
const CORNER_REACH: f32 = 40.0;
/// Where a held or gliding panel rides until it lands: above every sibling.
const LIFTED: ZIndex = ADORNED;
/// Space between a panel's grip and its corner, in pixels. A corner resize grabber needs none.
const GRIP_INSET: f32 = 4.0;
/// How far apart, in pixels, a panel's place and its target can be and still count as there.
const ARRIVED: f32 = 0.5;

// THEME
// =====

/// Theme tokens for a panel.
#[derive(Component, Clone, PartialEq, Debug)]
#[component(immutable)]
pub struct PanelTokens {
    /// The panel's background.
    pub bg: ThemeToken,
    /// The line around the panel's edge.
    pub border: ThemeToken,
    /// The radius of the panel's corners, in pixels. A panel flush against an
    /// edge has square corners instead.
    pub corner_radius: ThemeToken,
}

impl PanelTokens {
    /// Names the tokens under `prefix`: with `my_game.panel`, the background
    /// is `my_game.panel.bg`. Call [`register_theme_defaults`](Self::register_theme_defaults) to
    /// give a new prefix its defaults.
    pub fn with_prefix(prefix: &str) -> Self {
        let token = |suffix: &str| ThemeToken::new(format!("{prefix}.{suffix}"));
        Self {
            bg: token("bg"),
            border: token("border"),
            corner_radius: token("corner_radius"),
        }
    }

    /// Derives these tokens from the palette, unless the app sets them.
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
impl Default for PanelTokens {
    fn default() -> Self {
        Self::with_prefix(PANEL_STYLE)
    }
}

/// Styles a panel when its tokens are set: at spawn, and whenever the app sets new ones.
fn style_panel(
    insert: On<Insert<PanelTokens>>,
    panels: Query<(&PanelTokens, &PanelPlacement, &PanelSettings)>,
    mut commands: Commands,
) {
    // Tokens on an entity that isn't a panel: nothing to style.
    let Ok((tokens, &placement, settings)) = panels.get(insert.entity) else {
        return;
    };
    let mut panel = commands.entity(insert.entity);
    panel.insert((
        ThemeSkin(tokens.bg.clone()),
        ThemeBorder::all(tokens.border.clone()),
    ));
    if !placement.is_flush(settings.margin) {
        panel.insert(ThemeCornerRadius(tokens.corner_radius.clone()));
    }
}

// PLACEMENT
// =========

/// Where a panel sits in its parent. Change it and the panel moves there. Every placement but
/// [`Inline`](Self::Inline) pins it to its parent's edges, so that parent needs a size of its
/// own: a layer ([`UiLayer`](crate::layers::UiLayer)), or a node with a width and height.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub enum PanelPlacement {
    /// Along the whole left edge.
    Left,
    /// Along the whole right edge.
    Right,
    /// Along the whole top edge.
    Top,
    /// Along the whole bottom edge.
    Bottom,
    /// In the top-left corner.
    TopLeft,
    /// In the top-right corner.
    TopRight,
    /// In the bottom-left corner.
    BottomLeft,
    /// In the bottom-right corner.
    BottomRight,
    /// In its parent's layout, like a normal node. Can't be carried or resized.
    #[default]
    Inline,
    /// In the middle of its parent. The parent needs a size of its own.
    Center,
    /// With its top-left corner this far, in pixels, from the parent's top-left corner.
    Floating(Vec2),
}

impl PanelPlacement {
    /// Whether it runs along a whole edge of its parent.
    fn is_edge(self) -> bool {
        matches!(self, Self::Left | Self::Right | Self::Top | Self::Bottom)
    }

    /// Whether a panel here sits flush against its parent: along an edge, with
    /// no margin. A flush panel has square corners and a border only toward the
    /// rest of its parent.
    fn is_flush(self, margin: f32) -> bool {
        self.is_edge() && margin <= 0.0
    }

    /// Which way each axis of a panel here grows as its grabber moves: the
    /// grabber is on the side facing the rest of the parent.
    fn growth(self) -> Vec2 {
        match self {
            Self::Left => Vec2::X,
            Self::Right => Vec2::NEG_X,
            Self::Top => Vec2::Y,
            Self::Bottom => Vec2::NEG_Y,
            Self::TopLeft | Self::Inline | Self::Center | Self::Floating(_) => Vec2::ONE,
            Self::TopRight => Vec2::new(-1.0, 1.0),
            Self::BottomLeft => Vec2::new(1.0, -1.0),
            Self::BottomRight => Vec2::NEG_ONE,
        }
    }

    /// The corner a panel here resizes from, if it isn't along an edge.
    fn inner_corner(self) -> Option<GrabCorner> {
        match self {
            Self::TopLeft | Self::Inline | Self::Center | Self::Floating(_) => {
                Some(GrabCorner::BottomRight)
            }
            Self::TopRight => Some(GrabCorner::BottomLeft),
            Self::BottomLeft => Some(GrabCorner::TopRight),
            Self::BottomRight => Some(GrabCorner::TopLeft),
            Self::Left | Self::Right | Self::Top | Self::Bottom => None,
        }
    }

    /// How large a panel here can grow within `area`, keeping `margin` from its edges.
    fn room(self, margin: f32, area: Vec2) -> Vec2 {
        match self {
            Self::Floating(at) => (area - Vec2::splat(margin) - at).max(Vec2::ZERO),
            _ => inner(margin, area),
        }
    }

    /// The border a panel here shows.
    fn border(self, margin: f32) -> UiRect {
        let line = px(BORDER_WIDTH);
        if !self.is_flush(margin) {
            return UiRect::all(line);
        }
        match self {
            Self::Left => UiRect::right(line),
            Self::Right => UiRect::left(line),
            Self::Top => UiRect::bottom(line),
            _ => UiRect::top(line),
        }
    }

    /// Where a panel of `size` here sits within `area`, keeping `margin` from
    /// its edges, in the parent's space. The same place [`pin`](Self::pin)
    /// has layout put it, worked out ahead of layout so a glide can aim for it.
    fn rect(self, size: PanelSize, margin: f32, area: Vec2) -> Rect {
        let PanelSize(size) = size;
        let near = Vec2::splat(margin);
        // Where a panel's top-left corner goes to put it against the far edges.
        let far = area - near - size;
        let inside = inner(margin, area);
        let (min, extent) = match self {
            Self::Left => (near, Vec2::new(size.x, inside.y)),
            Self::Right => (Vec2::new(far.x, margin), Vec2::new(size.x, inside.y)),
            Self::Top => (near, Vec2::new(inside.x, size.y)),
            Self::Bottom => (Vec2::new(margin, far.y), Vec2::new(inside.x, size.y)),
            Self::TopLeft => (near, size),
            Self::TopRight => (Vec2::new(far.x, margin), size),
            Self::BottomLeft => (Vec2::new(margin, far.y), size),
            Self::BottomRight => (far, size),
            Self::Center => ((area - size) / 2.0, size),
            Self::Floating(at) => (at, size),
            // Layout decides position among siblings; this is where it would sit alone.
            Self::Inline => (Vec2::ZERO, size),
        };
        Rect {
            min,
            max: min + extent,
        }
    }

    /// How layout should place a panel of `size` here: by the edges it keeps
    /// to, so it follows its parent when the parent changes size.
    fn pin(self, size: PanelSize, margin: f32) -> Pin {
        let PanelSize(size) = size;
        let (m, auto) = (px(margin), Val::Auto);
        let (w, h) = (px(size.x), px(size.y));
        let half = percent(50);
        #[rustfmt::skip]
        let (left, right, top, bottom, width, height) = match self {
            Self::Left         => (m,        auto, m,        m,    w,    auto),
            Self::Right        => (auto,     m,    m,        m,    w,    auto),
            Self::Top          => (m,        m,    m,        auto, auto, h),
            Self::Bottom       => (m,        m,    auto,     m,    auto, h),
            Self::TopLeft      => (m,        auto, m,        auto, w,    h),
            Self::TopRight     => (auto,     m,    m,        auto, w,    h),
            Self::BottomLeft   => (m,        auto, auto,     m,    w,    h),
            Self::BottomRight  => (auto,     m,    auto,     m,    w,    h),
            // Its top-left corner at the middle, then pulled back by half its
            // size (below). Auto margins would centre it in the parent's
            // border box rather than inside the border.
            Self::Center       => (half,     auto, half,     auto, w,    h),
            Self::Floating(at) => (px(at.x), auto, px(at.y), auto, w,    h),
            Self::Inline       => (auto,     auto, auto,     auto, w,    h),
        };
        let position = match self {
            Self::Inline => PositionType::Relative,
            _ => PositionType::Absolute,
        };
        let margin = match self {
            Self::Center => UiRect {
                left: px(-size.x / 2.0),
                top: px(-size.y / 2.0),
                ..UiRect::ZERO
            },
            _ => UiRect::ZERO,
        };
        Pin {
            position, // only relative for Inline
            left,
            right,
            top,
            bottom,
            width,
            height,
            margin,
        }
    }
}

/// What's left of `area` inside `margin` all round.
fn inner(margin: f32, area: Vec2) -> Vec2 {
    (area - Vec2::splat(2.0 * margin)).max(Vec2::ZERO)
}

/// How a node is placed in its parent: the parts of its [`Node`] a panel sets.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Pin {
    position: PositionType,
    left: Val,
    right: Val,
    top: Val,
    bottom: Val,
    width: Val,
    height: Val,
    margin: UiRect,
}

impl Pin {
    /// At `rect` exactly, from the parent's top-left corner. Places/glides the panel.
    fn at(rect: Rect) -> Self {
        Self {
            position: PositionType::Absolute,
            left: px(rect.min.x),
            right: Val::Auto,
            top: px(rect.min.y),
            bottom: Val::Auto,
            width: px(rect.width()),
            height: px(rect.height()),
            margin: UiRect::ZERO,
        }
    }

    fn of(node: &Node) -> Self {
        Self {
            position: node.position_type,
            left: node.left,
            right: node.right,
            top: node.top,
            bottom: node.bottom,
            width: node.width,
            height: node.height,
            margin: node.margin,
        }
    }

    fn write(self, node: &mut Node) {
        node.position_type = self.position;
        node.left = self.left;
        node.right = self.right;
        node.top = self.top;
        node.bottom = self.bottom;
        node.width = self.width;
        node.height = self.height;
        node.margin = self.margin;
    }

    /// Writes this to `node` only if it differs, so an unmoved panel isn't laid out again.
    fn apply(self, node: &mut Mut<Node>) {
        if Self::of(node) != self {
            self.write(node);
        }
    }
}

/// Where a pointer at `point` in a parent of size `area` would dock a panel, if anywhere.
fn snap(point: Vec2, area: Vec2) -> Option<PanelPlacement> {
    // Each axis: near its start, near its end, or neither, within `reach`.
    let side = |at: f32, length: f32, reach: f32| {
        if at <= reach {
            Some(false)
        } else if at >= length - reach {
            Some(true)
        } else {
            None
        }
    };
    let edge = (
        side(point.x, area.x, SNAP_DISTANCE),
        side(point.y, area.y, SNAP_DISTANCE),
    );
    let reach = (
        side(point.x, area.x, CORNER_REACH),
        side(point.y, area.y, CORNER_REACH),
    );
    // At one edge and within reach of a corner along it is that corner.
    let corner = match (edge, reach) {
        ((Some(x), _), (_, Some(y))) | ((_, Some(y)), (Some(x), _)) => Some((x, y)),
        _ => None,
    };
    match (corner, edge) {
        (Some((false, false)), _) => Some(PanelPlacement::TopLeft),
        (Some((true, false)), _) => Some(PanelPlacement::TopRight),
        (Some((false, true)), _) => Some(PanelPlacement::BottomLeft),
        (Some((true, true)), _) => Some(PanelPlacement::BottomRight),
        (None, (Some(false), None)) => Some(PanelPlacement::Left),
        (None, (Some(true), None)) => Some(PanelPlacement::Right),
        (None, (None, Some(false))) => Some(PanelPlacement::Top),
        (None, (None, Some(true))) => Some(PanelPlacement::Bottom),
        _ => None,
    }
}

/// Where a panel let go of with the pointer at `point` will land/dock.
fn dock_at(
    point: Vec2,
    corner: Vec2,
    area: Vec2,
    allowed: PanelDockTargets,
    home: PanelPlacement,
) -> PanelPlacement {
    match snap(point, area) {
        Some(dock) if allowed.allow(dock) => dock,
        _ if allowed.floating => PanelPlacement::Floating(corner),
        _ => home,
    }
}

/// Position of the panel's top-left corner. `hand` is `offset` from that corner.
/// The panel has `size` and is placed in a parent of `area`.
fn carried_corner(hand: Vec2, offset: Vec2, size: Vec2, margin: f32, area: Vec2) -> Vec2 {
    let low = Vec2::splat(margin);
    let high = (area - low - size).max(low);
    (hand - offset).clamp(low, high)
}

// COMPONENTS AND SETTINGS
// =======================

/// Where a carried panel may be let go. With nothing allowed it always goes home.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct PanelDockTargets {
    /// Docked along an edge of the parent.
    pub edges: bool,
    /// Docked in a corner of the parent.
    pub corners: bool,
    /// Floating wherever it's let go.
    pub floating: bool,
}

impl PanelDockTargets {
    /// Nowhere: it always goes home.
    pub const NONE: Self = Self {
        edges: false,
        corners: false,
        floating: false,
    };
    /// Along the edges.
    pub const EDGES: Self = Self {
        edges: true,
        ..Self::NONE
    };
    /// In the corners.
    pub const CORNERS: Self = Self {
        corners: true,
        ..Self::NONE
    };
    /// Floating where it's let go.
    pub const FLOATING: Self = Self {
        floating: true,
        ..Self::NONE
    };
    /// Anywhere.
    pub const ALL: Self = Self {
        edges: true,
        corners: true,
        floating: true,
    };

    fn allow(self, place: PanelPlacement) -> bool {
        match place {
            PanelPlacement::Inline | PanelPlacement::Center => false,
            PanelPlacement::Floating(_) => self.floating,
            _ if place.is_edge() => self.edges,
            _ => self.corners,
        }
    }
}

impl Default for PanelDockTargets {
    fn default() -> Self {
        Self::ALL
    }
}

impl std::ops::BitOr for PanelDockTargets {
    type Output = Self;

    fn bitor(self, other: Self) -> Self {
        Self {
            edges: self.edges || other.edges,
            corners: self.corners || other.corners,
            floating: self.floating || other.floating,
        }
    }
}

/// A panel's size in pixels. Change it and the panel resizes.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct PanelSize(pub Vec2);

/// How small and how large a hand can resize a panel, in pixels. Code may set any size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PanelLimits {
    /// The smallest size a panel can be resized to.
    pub min: Vec2,
    /// The smallest size a panel can be resized to.
    pub max: Vec2,
}

impl Default for PanelLimits {
    fn default() -> Self {
        Self {
            min: Vec2::splat(DEFAULT_MIN_SIZE),
            max: Vec2::INFINITY,
        }
    }
}

impl PanelLimits {
    /// `size` within these limits and the `room` there is: the room wins.
    fn bound(&self, size: Vec2, room: Vec2) -> Vec2 {
        let high = self.max.min(room).max(Vec2::ZERO);
        size.clamp(self.min.min(high), high)
    }
}

/// How a panel shows a carry or a resize while it's held.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DragFeedback {
    /// The panel and its content move and resize as the panel is modified.
    #[default]
    Live,
    /// The panel stays in place until the move/resize is complete.
    /// A preview is shown indicating the new placement and size.
    Preview,
}

/// How a panel behaves.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct PanelSettings {
    /// Where a carried panel may be let go.
    pub dock_targets: PanelDockTargets,
    /// Space kept between the panel and its parent's edges, in pixels.
    pub margin: f32,
    /// How small and how large a panel can be resized.
    pub limits: PanelLimits,
    /// How a carry or a resize shows while the panel is held.
    pub feedback: DragFeedback,
    /// How fast a panel moves to its new place, in px/second squared; `None` jumps there.
    pub acceleration: Option<f32>,
}

impl Default for PanelSettings {
    fn default() -> Self {
        Self {
            dock_targets: PanelDockTargets::default(),
            margin: 0.0,
            limits: PanelLimits::default(),
            feedback: DragFeedback::default(),
            acceleration: Some(DEFAULT_ACCELERATION),
        }
    }
}

impl PanelSettings {
    /// These settings with every value made usable, warning about each repair.
    fn sanitized(mut self) -> Self {
        self.margin = at_least(self.margin, 0.0, "PanelSettings margin");
        let limits = &mut self.limits;
        limits.min = at_least(limits.min, Vec2::ZERO, "PanelLimits min");
        limits.max = at_least(limits.max, limits.min, "PanelLimits max");
        self.acceleration = match self.acceleration {
            Some(rate) if rate > 0.0 => Some(rate),
            // No acceleration means no glide: it jumps, as documented.
            Some(0.0) | None => None,
            Some(rate) => {
                warn!("PanelSettings acceleration was {rate}; jumping instead");
                None
            }
        };
        self
    }
}

/// What's happening to a panel right now.
#[derive(Component, Clone, Copy, Debug, Default)]
struct PanelActivity {
    /// If the panel is grabbed and for what purpose.
    grab: Option<PanelGrab>,
    /// Memory of the panel's z-index. Used while it is lifted above its sibling.
    resting_z: Option<ZIndex>,
    /// It is gliding to a new positoin.
    glide: Option<Glide<Vec4>>,
    /// Where it shows while it's carried or gliding, in the parent's space:
    /// where its next glide starts. `None` while it rests in its place.
    shown: Option<Rect>,
    /// Just released: settle it, even if neither its placement nor its size changed.
    settle: bool,
    /// Where in its parent a panel would land right now. Useful for previewing.
    landing: Option<Rect>,
}

impl PanelActivity {
    /// Raises the panel above its siblings, remembering where it was.
    fn lift(&mut self, z: &mut ZIndex) {
        if self.resting_z.is_none() {
            self.resting_z = Some(*z);
        }
        *z = LIFTED;
    }
}

/// What a hand is doing to a panel.
#[derive(Clone, Copy, Debug)]
enum PanelGrab {
    /// Resizing it, from this size.
    Resize(PanelSize),
    /// Carrying it, `grip` from its top-left corner, at `size`.
    Carry { size: Vec2, grip: Vec2 },
}

/// What a grabber does to its panel.
#[derive(Component, Clone, Copy, Debug)]
enum GrabberRole {
    Resizes(Entity),
    Carries(Entity),
}

/// A resizable panel's grabbers, each with the node that places it.
#[derive(Component, Clone, Copy, Debug)]
struct ResizeHandles {
    /// The strip along an edge panel's inner edge.
    edge_container: Entity,
    edge_grabber: Entity,
    /// The nook in any other panel's inner corner.
    corner_container: Entity,
    corner_grabber: Entity,
}

/// A movable panel's grip, by the node that places it.
#[derive(Component, Clone, Copy, Debug)]
struct CarryGrip(Entity);

// CONFIG AND SPAWNING
// ===================

/// Construction-time configuration for a panel.
///
/// Consumed by [`SpawnUi::panel`](crate::spawn_ui::SpawnUi::panel) and not retained as a component;
/// what the panel needs later lives in [`PanelPlacement`], [`PanelSize`], [`PanelSettings`] and
/// [`PanelTokens`].
///
/// # Examples
/// ```
/// use touchy::prelude::*;
///
/// let inspector = PanelConfig::new()
///     .placement(PanelPlacement::Right)
///     .resizable(true)
///     .movable(GrabberStyle::Grip);
/// # assert_eq!(inspector.placement, PanelPlacement::Right);
/// ```
#[derive(Clone, Debug)]
pub struct PanelConfig {
    /// Where the panel sits in its parent.
    pub placement: PanelPlacement,
    /// The panel's size.
    pub size: PanelSize,
    /// Space between the elements in the panel, in pixels.
    pub gap: f32,
    /// Space inside the panel's edge, around its contents, in pixels.
    pub padding: f32,
    /// Whether a hand can resize it by its inner edge or corner.
    pub resizable: bool,
    /// What carries it, if anything: a grip, a handle, or its whole body.
    pub movable: Option<GrabberStyle>,
    /// Its place among its siblings: higher is in front.
    pub z_index: i32,
    /// How it behaves once spawned.
    pub settings: PanelSettings,
    /// The theme tokens for a panel container.
    pub tokens: PanelTokens,
}

impl PanelConfig {
    /// A panel in the middle of its parent, at the default size, that stays put.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets where the panel sits in its parent.
    pub fn placement(mut self, placement: PanelPlacement) -> Self {
        self.placement = placement;
        self
    }

    /// Sets the panel's size, in pixels.
    pub fn size(mut self, width: f32, height: f32) -> Self {
        self.size = PanelSize(Vec2::new(width, height));
        self
    }

    /// Sets the space between the elements in the panel, in pixels.
    pub fn gap(mut self, pixels: f32) -> Self {
        self.gap = pixels;
        self
    }

    /// Sets the panel's width, in pixels.
    pub fn width(mut self, pixels: f32) -> Self {
        self.size.0.x = pixels;
        self
    }

    /// Sets the panel's height, in pixels.
    pub fn height(mut self, pixels: f32) -> Self {
        self.size.0.y = pixels;
        self
    }

    /// Sets the space inside the panel's edge, in pixels.
    pub fn padding(mut self, pixels: f32) -> Self {
        self.padding = pixels;
        self
    }

    /// Sets whether a hand can resize the panel.
    pub fn resizable(mut self, resizable: bool) -> Self {
        self.resizable = resizable;
        self
    }

    /// Adds a grabber to carry the panel ([`GrabberStyle::Grip`] or
    /// [`GrabberStyle::Handle`]), or makes its whole body one ([`GrabberStyle::Ghost`]).
    pub fn movable(mut self, by: GrabberStyle) -> Self {
        self.movable = Some(by);
        self
    }

    /// Sets the panel's place among its siblings: higher is in front.
    pub fn z_index(mut self, z_index: i32) -> Self {
        self.z_index = z_index;
        self
    }

    /// Sets where a carried panel may be let go.
    pub fn dock_targets(mut self, targets: PanelDockTargets) -> Self {
        self.settings.dock_targets = targets;
        self
    }

    /// Sets the space kept between the panel and its parent's edges, in pixels.
    pub fn margin(mut self, pixels: f32) -> Self {
        self.settings.margin = pixels;
        self
    }

    /// Sets the smallest a hand can resize the panel to, in pixels.
    pub fn min_size(mut self, width: f32, height: f32) -> Self {
        self.settings.limits.min = Vec2::new(width, height);
        self
    }

    /// Sets the largest a hand can resize the panel to, in pixels.
    pub fn max_size(mut self, width: f32, height: f32) -> Self {
        self.settings.limits.max = Vec2::new(width, height);
        self
    }

    /// Sets how a carry or a resize shows while the panel is held.
    pub fn feedback(mut self, feedback: DragFeedback) -> Self {
        self.settings.feedback = feedback;
        self
    }

    /// Sets how fast the panel picks up speed gliding to a new place, in
    /// pixels per second squared, or `None` to jump.
    pub fn acceleration(mut self, pixels_per_second_squared: Option<f32>) -> Self {
        self.settings.acceleration = pixels_per_second_squared;
        self
    }

    /// Sets the panel's theme tokens.
    pub fn tokens(mut self, tokens: PanelTokens) -> Self {
        self.tokens = tokens;
        self
    }

    /// This config with every value made usable, warning about each repair.
    fn sanitized(mut self) -> Self {
        self.size = PanelSize(at_least(self.size.0, Vec2::ZERO, "PanelConfig size"));
        self.padding = at_least(self.padding, 0.0, "PanelConfig padding");
        self.gap = at_least(self.gap, 0.0, "PanelConfig gap");
        // In its parent's layout, a panel can't be carried or resized: its siblings decide.
        if self.placement == PanelPlacement::Inline {
            if let Some(style) = self.movable.take() {
                warn!("PanelConfig was movable ({style:?}) but inline; it stays in place");
            }
            if std::mem::take(&mut self.resizable) {
                warn!("PanelConfig was resizable but inline; it keeps its size");
            }
        }
        self.settings = self.settings.sanitized();
        self
    }
}

impl Default for PanelConfig {
    fn default() -> Self {
        Self {
            placement: PanelPlacement::default(),
            size: PanelSize(DEFAULT_SIZE),
            padding: DEFAULT_PADDING,
            gap: DEFAULT_GAP,
            resizable: false,
            movable: None,
            z_index: 0,
            settings: PanelSettings::default(),
            tokens: PanelTokens::with_prefix(PANEL_STYLE),
        }
    }
}

/// Spawns it; documented on [`SpawnUi::panel`](crate::spawn_ui::SpawnUi::panel).
pub(crate) fn spawn_panel<'a>(
    parent: &'a mut ChildSpawnerCommands,
    config: PanelConfig,
    build: impl FnOnce(&mut ChildSpawnerCommands),
) -> EntityCommands<'a> {
    let PanelConfig {
        placement,
        size,
        gap,
        padding,
        resizable,
        movable,
        z_index,
        settings,
        tokens,
    } = config.sanitized();
    let mut node = Node {
        position_type: PositionType::Absolute,
        flex_direction: FlexDirection::Column,
        row_gap: px(gap),
        column_gap: px(gap),
        padding: UiRect::all(px(padding)),
        border: placement.border(settings.margin),
        overflow: Overflow::clip(),
        ..default()
    };
    placement.pin(size, settings.margin).write(&mut node);

    let mut panel = parent.spawn((
        placement,
        size,
        settings,
        PanelActivity::default(),
        node,
        ZIndex(z_index),
        tokens,
    ));
    let id = panel.id();
    let (mut grip, mut resize) = (None, None);
    panel.with_children(|inside| {
        // A body grabber goes first, so the contents cover it and keep their clicks.
        if movable == Some(GrabberStyle::Ghost) {
            spawn_grabber(inside, GrabberConfig::ghost()).insert(GrabberRole::Carries(id));
        }
        build(inside);
        if let Some(style @ (GrabberStyle::Grip | GrabberStyle::Handle)) = movable {
            let mut place = inside.spawn(grip_node(placement, resizable));
            grip = Some(CarryGrip(place.id()));
            place.with_children(|place| {
                spawn_grabber(place, GrabberConfig::new(style)).insert(GrabberRole::Carries(id));
            });
        }
        if resizable {
            resize = Some(spawn_resize_handles(inside, placement, id));
        }
    });
    if let Some(grip) = grip {
        panel.insert(grip);
    }
    if let Some(resize) = resize {
        panel.insert(resize);
    }
    panel
}

/// Where a panel's resize strip sits for a placement: along its inner edge, if it has one.
fn strip_node(placement: PanelPlacement) -> Node {
    let zero = px(0.0);
    let base = Node {
        position_type: PositionType::Absolute,
        ..default()
    };
    let across = FlexDirection::Column;
    match placement {
        PanelPlacement::Left => Node {
            right: zero,
            top: zero,
            bottom: zero,
            ..base
        },
        PanelPlacement::Right => Node {
            left: zero,
            top: zero,
            bottom: zero,
            ..base
        },
        PanelPlacement::Top => Node {
            left: zero,
            right: zero,
            bottom: zero,
            flex_direction: across,
            ..base
        },
        PanelPlacement::Bottom => Node {
            left: zero,
            right: zero,
            top: zero,
            flex_direction: across,
            ..base
        },
        _ => Node {
            display: Display::None,
            ..base
        },
    }
}

/// Where a panel's corner grabber sits for a placement: in its inner corner, if it has one.
fn nook_node(placement: PanelPlacement) -> Node {
    let zero = px(0.0);
    let base = Node {
        position_type: PositionType::Absolute,
        ..default()
    };
    match placement.inner_corner() {
        Some(GrabCorner::TopLeft) => Node {
            left: zero,
            top: zero,
            ..base
        },
        Some(GrabCorner::TopRight) => Node {
            right: zero,
            top: zero,
            ..base
        },
        Some(GrabCorner::BottomLeft) => Node {
            left: zero,
            bottom: zero,
            ..base
        },
        Some(GrabCorner::BottomRight) => Node {
            right: zero,
            bottom: zero,
            ..base
        },
        None => Node {
            display: Display::None,
            ..base
        },
    }
}

/// Where a panel's grip sits: in its top-right corner, unless a corner
/// grabber has that corner.
fn grip_node(placement: PanelPlacement, resizable: bool) -> Node {
    let inset = px(GRIP_INSET);
    let (left, right) = match placement.inner_corner() {
        Some(GrabCorner::TopRight) if resizable => (inset, Val::Auto),
        _ => (Val::Auto, inset),
    };
    Node {
        position_type: PositionType::Absolute,
        top: inset,
        left,
        right,
        ..default()
    }
}

/// Which way a panel's edge grabber drags for a placement.
fn resize_axis(placement: PanelPlacement) -> GrabAxis {
    match placement {
        PanelPlacement::Top | PanelPlacement::Bottom => GrabAxis::Vertical,
        _ => GrabAxis::Horizontal,
    }
}

/// Spawns both of a resizable panel's grabbers, each in a node placing it.
/// Only the one its placement uses shows.
fn spawn_resize_handles(
    inside: &mut ChildSpawnerCommands,
    placement: PanelPlacement,
    panel: Entity,
) -> ResizeHandles {
    let mut handle_in = |place: Node, config: GrabberConfig| {
        let mut place = inside.spawn(place);
        let mut grabber = Entity::PLACEHOLDER;
        place.with_children(|place| {
            grabber = spawn_grabber(place, config)
                .insert(GrabberRole::Resizes(panel))
                .id();
        });
        (place.id(), grabber)
    };
    let edge = GrabberConfig::handle(resize_axis(placement));
    let (edge_container, edge_grabber) = handle_in(strip_node(placement), edge);
    let corner = GrabberConfig::corner(placement.inner_corner().unwrap_or_default());
    let (corner_container, corner_grabber) = handle_in(nook_node(placement), corner);
    ResizeHandles {
        edge_container,
        edge_grabber,
        corner_container,
        corner_grabber,
    }
}

// GEOMETRY
// ========

/// A UI node's size and where it is, as layout last left them.
type UiGeometry = (&'static ComputedNode, &'static UiGlobalTransform);

/// The room inside a panel's parent, and where that room's top-left corner is on screen.
/// Excludes panel borders. In logical pixels.
fn dock_area((node, transform): (&ComputedNode, &UiGlobalTransform)) -> (Vec2, Vec2) {
    // Layout works in physical pixels; panels and pointers in logical ones.
    let scale = node.inverse_scale_factor();
    let border = node.border;
    let area = (node.size() - border.min_inset - border.max_inset).max(Vec2::ZERO) * scale;
    let origin = (transform.translation - node.size() / 2.0 + border.min_inset) * scale;
    (area, origin)
}

/// Where a node shows, in the space of a parent whose room starts at `origin`.
fn shown_rect((node, transform): (&ComputedNode, &UiGlobalTransform), origin: Vec2) -> Rect {
    let scale = node.inverse_scale_factor();
    let min = (transform.translation - node.size() / 2.0) * scale - origin;
    Rect {
        min,
        max: min + node.size() * scale,
    }
}

/// A rect as a glide moves it: its top-left corner, then its size.
fn to_vec4(rect: Rect) -> Vec4 {
    rect.min.extend(rect.width()).extend(rect.height())
}

fn from_vec4(at: Vec4) -> Rect {
    Rect {
        min: at.xy(),
        max: at.xy() + at.zw(),
    }
}

// THE PANEL AT WORK
// =================

/// A panel's state, as its systems and its observer work on it.
#[derive(QueryData)]
#[query_data(mutable)]
struct Panel {
    placement: &'static mut PanelPlacement,
    size: &'static mut PanelSize,
    settings: Ref<'static, PanelSettings>,
    activity: &'static mut PanelActivity,
    node: &'static mut Node,
    z: &'static mut ZIndex,
    tokens: &'static PanelTokens,
    parent: &'static ChildOf,
    /// Where layout put it last, if it has been laid out yet.
    layout: Option<UiGeometry>,
}

impl PanelItem<'_, '_> {
    /// The size a hand resizing it by `offset` from `start` gives it.
    fn resized(&self, start: PanelSize, offset: Vec2, area: Vec2) -> PanelSize {
        let placement = *self.placement;
        // A centred panel grows on both sides, so twice as fast as the hand.
        let pace = if placement == PanelPlacement::Center {
            2.0
        } else {
            1.0
        };
        let grown = start.0 + offset * placement.growth() * pace;
        let room = placement.room(self.settings.margin, area);
        PanelSize(self.settings.limits.bound(grown, room))
    }

    /// A hand resizing it: `offset` is how far the hand has moved since it
    /// took hold, in a parent with room `area`.
    fn resize(&mut self, phase: GrabPhase, offset: Vec2, area: Vec2) {
        let (placement, margin) = (*self.placement, self.settings.margin);
        let live = self.settings.feedback == DragFeedback::Live;
        match phase {
            GrabPhase::Start => {
                // Taken mid-glide, it stops where it is.
                self.activity.glide = None;
                self.activity.grab = Some(PanelGrab::Resize(*self.size));
                self.activity.lift(&mut self.z);
            }
            GrabPhase::Move => {
                let Some(PanelGrab::Resize(start)) = self.activity.grab else {
                    return;
                };
                let size = self.resized(start, offset, area);
                let rect = placement.rect(size, margin, area);
                if live {
                    self.size.set_if_neq(size);
                    placement.pin(size, margin).apply(&mut self.node);
                    self.activity.shown = Some(rect);
                } else {
                    self.activity.landing = Some(rect);
                }
            }
            GrabPhase::End => {
                let Some(PanelGrab::Resize(start)) = self.activity.grab.take() else {
                    return;
                };
                let size = self.resized(start, offset, area);
                if live {
                    // It shows at its new size already: nowhere to glide.
                    self.activity.shown = Some(placement.rect(size, margin, area));
                } else {
                    // It waited at its old size; it goes from there.
                    self.activity.shown = Some(placement.rect(start, margin, area));
                }
                *self.size = size;
                self.activity.landing = None;
                self.activity.settle = true;
            }
            GrabPhase::Cancel => {
                let Some(PanelGrab::Resize(start)) = self.activity.grab.take() else {
                    return;
                };
                if live {
                    // It shows at the size it was dragged to; it goes back from there.
                    self.activity.shown = Some(placement.rect(*self.size, margin, area));
                    *self.size = start;
                }
                self.activity.landing = None;
                self.activity.settle = true;
            }
        }
    }

    /// A hand carrying it: `hand` is where the hand is in the parent's space,
    /// in a parent with room `area`.
    fn carry(&mut self, phase: GrabPhase, hand: Vec2, area: Vec2) {
        let (placement, size) = (*self.placement, *self.size);
        let settings = *self.settings;
        let live = settings.feedback == DragFeedback::Live;
        // Where it shows now: on its way somewhere, or at rest in its place.
        let showing = self
            .activity
            .shown
            .unwrap_or_else(|| placement.rect(size, settings.margin, area));
        match phase {
            GrabPhase::Start => {
                // Taken from an edge it's carried at its own size, still held
                // at the same fraction of the way across.
                let held = ((hand - showing.min) / showing.size().max(Vec2::ONE))
                    .clamp(Vec2::ZERO, Vec2::ONE);
                let grip = held * size.0;
                self.activity.glide = None;
                self.activity.grab = Some(PanelGrab::Carry { size: size.0, grip });
                self.activity.lift(&mut self.z);
                if live {
                    self.hold(hand, size.0, grip, area);
                }
            }
            GrabPhase::Move => {
                let Some(PanelGrab::Carry { size: held, grip }) = self.activity.grab else {
                    return;
                };
                let corner = carried_corner(hand, grip, held, settings.margin, area);
                let lands = dock_at(hand, corner, area, settings.dock_targets, placement);
                let landing = lands.rect(size, settings.margin, area);
                if live {
                    self.hold(hand, held, grip, area);
                    // Floating, it lands where it is, so there's nothing to outline.
                    self.activity.landing =
                        (!matches!(lands, PanelPlacement::Floating(_))).then_some(landing);
                } else {
                    self.activity.landing = Some(landing);
                }
            }
            GrabPhase::End => {
                let Some(PanelGrab::Carry { size: held, grip }) = self.activity.grab.take() else {
                    return;
                };
                let corner = carried_corner(hand, grip, held, settings.margin, area);
                if live {
                    // In the hand where it was let go, which may be past its last move.
                    self.hold(hand, held, grip, area);
                } else {
                    // It waited in its old place; it goes from there.
                    self.activity.shown = Some(showing);
                }
                *self.placement = dock_at(hand, corner, area, settings.dock_targets, placement);
                self.activity.landing = None;
                self.activity.settle = true;
            }
            GrabPhase::Cancel => {
                if self.activity.grab.take().is_none() {
                    return;
                }
                // Its placement never changed. Live, it shows in the hand and
                // goes back from there; otherwise it never left.
                self.activity.landing = None;
                self.activity.settle = true;
            }
        }
    }

    /// Shows it in the hand at `hand`, held `grip` from its top-left corner.
    fn hold(&mut self, hand: Vec2, size: Vec2, grip: Vec2, area: Vec2) {
        let corner = carried_corner(hand, grip, size, self.settings.margin, area);
        let rect = Rect {
            min: corner,
            max: corner + size,
        };
        Pin::at(rect).apply(&mut self.node);
        self.activity.shown = Some(rect);
    }

    /// Sends it to its place and size from wherever it shows: gliding, if it
    /// has an acceleration and somewhere to go, or jumping. `area` and
    /// `origin` are its parent's room, if its parent has been laid out.
    fn arrive(&mut self, parent: Option<(Vec2, Vec2)>) {
        if let (Some(acceleration), Some((area, origin))) = (self.settings.acceleration, parent) {
            let to = self.placement.rect(*self.size, self.settings.margin, area);
            let layout = self.layout.map(|layout| shown_rect(layout, origin));
            if let Some(from) = self.activity.shown.or(layout)
                && (to_vec4(from) - to_vec4(to)).abs().max_element() > ARRIVED
            {
                self.activity.glide = Some(Glide::new(to_vec4(from), to_vec4(to), acceleration));
                return;
            }
        }
        self.land();
    }

    /// Pins it in its place at its size, and puts it back among its siblings.
    fn land(&mut self) {
        self.activity.glide = None;
        self.activity.shown = None;
        let pin = self.placement.pin(*self.size, self.settings.margin);
        pin.apply(&mut self.node);
        if let Some(resting) = self.activity.resting_z.take() {
            *self.z = resting;
        }
    }

    /// Styles it for its placement: border, and rounded corners unless flush.
    fn restyle(&mut self, panel: &mut EntityCommands) {
        let (placement, margin) = (*self.placement, self.settings.margin);
        let border = placement.border(margin);
        if self.node.border != border {
            self.node.border = border;
        }
        if placement.is_flush(margin) {
            panel.remove::<ThemeCornerRadius>();
            if self.node.border_radius != BorderRadius::ZERO {
                self.node.border_radius = BorderRadius::ZERO;
            }
        } else {
            panel.insert(ThemeCornerRadius(self.tokens.corner_radius.clone()));
        }
    }
}

/// Hands a grab to the panel. Shows outlines where it would land.
fn grab_panels(
    grabbed: On<Grabbed>,
    roles: Query<&GrabberRole>,
    mut panels: Query<Panel>,
    parents: Query<UiGeometry>,
    mut commands: Commands,
) {
    let Ok(&role) = roles.get(grabbed.grabber) else {
        return;
    };
    let (GrabberRole::Resizes(id) | GrabberRole::Carries(id)) = role;
    let Ok(mut panel) = panels.get_mut(id) else {
        return;
    };
    let Ok(parent) = parents.get(panel.parent.parent()) else {
        return;
    };
    let (area, origin) = dock_area(parent);
    let was_landing = panel.activity.landing;
    match role {
        GrabberRole::Resizes(_) => panel.resize(grabbed.phase, grabbed.offset, area),
        GrabberRole::Carries(_) => panel.carry(grabbed.phase, grabbed.position - origin, area),
    }
    // Use the Adorner to show where it would land, if it's not already there.
    match panel.activity.landing {
        landing if landing == was_landing => {}
        Some(rect) => commands.trigger(ShowOutline {
            parent: panel.parent.parent(),
            rect,
        }),
        None => commands.trigger(HideOutline),
    }
}

/// Hides the outline if a panel goes while it's being outlined.
fn hide_outline_with_panel(
    remove: On<Remove<PanelActivity>>,
    activities: Query<&PanelActivity>,
    mut commands: Commands,
) {
    if activities
        .get(remove.entity)
        .is_ok_and(|activity| activity.landing.is_some())
    {
        commands.trigger(HideOutline);
    }
}

/// Sends each panel whose placement, size or settings changed, or that a hand
/// just let go of, to its place, and restyles it for a new placement. The one
/// place a panel's arrival is decided, however it was moved.
fn settle_panels(
    mut panels: Query<(Entity, Panel)>,
    parents: Query<UiGeometry>,
    mut commands: Commands,
) {
    for (entity, mut panel) in &mut panels {
        // A held panel settles when it's let go.
        if panel.activity.grab.is_some() {
            continue;
        }
        let moved = panel.placement.is_changed() && !panel.placement.is_added();
        let reset = panel.settings.is_changed() && !panel.settings.is_added();
        let resized = panel.size.is_changed() && !panel.size.is_added();
        let let_go = std::mem::take(&mut panel.activity.settle);
        if !(moved || resized || reset || let_go) {
            continue;
        }
        if moved || reset {
            panel.restyle(&mut commands.entity(entity));
        }
        let parent = parents.get(panel.parent.parent()).ok().map(dock_area);
        panel.arrive(parent);
    }
}

/// Marks a panel already warned that its parent is too small for it, so it's warned once.
#[derive(Component)]
struct WarnedCramped;

/// Warns once about a pinned panel whose parent has less room than it needs, which shows it
/// partly outside its parent: often a parent sized only by its contents.
fn warn_of_cramped_panels(
    panels: Query<
        (
            Entity,
            &PanelPlacement,
            &PanelSize,
            &PanelSettings,
            &ChildOf,
        ),
        Without<WarnedCramped>,
    >,
    parents: Query<UiGeometry>,
    mut commands: Commands,
) {
    for (entity, &placement, &PanelSize(size), settings, parent) in &panels {
        let Ok(geometry) = parents.get(parent.parent()) else {
            continue;
        };
        let (area, _) = dock_area(geometry);
        // Not laid out yet, or an inline panel, which its parent makes room for.
        if area == Vec2::ZERO || placement == PanelPlacement::Inline {
            continue;
        }
        let room = placement.room(settings.margin, area);
        let short = match placement {
            PanelPlacement::Left | PanelPlacement::Right => size.x > room.x,
            PanelPlacement::Top | PanelPlacement::Bottom => size.y > room.y,
            _ => size.x > room.x || size.y > room.y,
        };
        if short {
            warn!(
                "{entity}: a {placement:?} panel {size} has only {room} of room in its parent; \
                 a pinned panel sits against its parent's edges, so give the parent a size, or \
                 put the panel on a layer with `UiLayer`"
            );
            commands.entity(entity).insert(WarnedCramped);
        }
    }
}

/// Moves each gliding panel along, and lands it when it arrives. Real time,
/// so a paused game's panels still move.
fn advance_glides(time: Res<Time<Real>>, mut panels: Query<Panel>) {
    for mut panel in &mut panels {
        let Some(mut glide) = panel.activity.glide else {
            continue;
        };
        let (at, arrived) = glide.advance(time.delta_secs());
        if arrived {
            panel.land();
        } else {
            let rect = from_vec4(at);
            panel.activity.glide = Some(glide);
            panel.activity.shown = Some(rect);
            Pin::at(rect).apply(&mut panel.node);
        }
    }
}

/// The parts of a panel its grabbers follow.
type PanelGrabbers = (
    &'static PanelPlacement,
    Option<&'static ResizeHandles>,
    Option<&'static CarryGrip>,
);

/// Moves a panel's grabbers to suit its placement.
fn follow_placement_with_grabbers(
    panels: Query<PanelGrabbers, Changed<PanelPlacement>>,
    mut nodes: Query<&mut Node>,
    mut axes: Query<&mut GrabAxis>,
    mut corners: Query<&mut GrabCorner>,
) {
    for (&placement, resize, grip) in &panels {
        if let Some(&CarryGrip(place)) = grip
            && let Ok(mut node) = nodes.get_mut(place)
        {
            node.set_if_neq(grip_node(placement, resize.is_some()));
        }
        let Some(resize) = resize else {
            continue;
        };
        for (place, wanted) in [
            (resize.edge_container, strip_node(placement)),
            (resize.corner_container, nook_node(placement)),
        ] {
            if let Ok(mut node) = nodes.get_mut(place) {
                node.set_if_neq(wanted);
            }
        }
        if let Ok(mut axis) = axes.get_mut(resize.edge_grabber) {
            axis.set_if_neq(resize_axis(placement));
        }
        if let Some(inner) = placement.inner_corner()
            && let Ok(mut corner) = corners.get_mut(resize.corner_grabber)
        {
            corner.set_if_neq(inner);
        }
    }
}

/// Adds panels, the grabbers they're carried and resized by, and their theme tokens.
pub struct PanelPlugin;

impl Plugin for PanelPlugin {
    fn build(&self, app: &mut App) {
        app.ensure_plugin(ThemePlugin)
            .ensure_plugin(GrabberWidgetPlugin)
            .ensure_plugin(AdornerPlugin)
            .add_observer(style_panel)
            .add_observer(grab_panels)
            .add_observer(hide_outline_with_panel)
            .add_systems(
                Update,
                (
                    settle_panels,
                    advance_glides,
                    follow_placement_with_grabbers,
                    warn_of_cramped_panels,
                )
                    .chain(),
            );
        PanelTokens::with_prefix(PANEL_STYLE)
            .register_theme_defaults(&mut app.world_mut().resource_mut::<Theme>());
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy::time::TimeUpdateStrategy;

    use super::*;

    /// The parent's room in these tests, in logical pixels, and a frame's length.
    const AREA: Vec2 = Vec2::new(800.0, 600.0);
    const FRAME_MS: u64 = 16;
    const SIZE: PanelSize = PanelSize(Vec2::new(280.0, 200.0));
    const MARGIN: f32 = 12.0;

    fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
        Rect {
            min: Vec2::new(x, y),
            max: Vec2::new(x + width, y + height),
        }
    }

    #[test]
    fn each_placement_has_its_place_inside_the_margin() {
        let at = |placement: PanelPlacement| placement.rect(SIZE, MARGIN, AREA);
        assert_eq!(at(PanelPlacement::Left), rect(12.0, 12.0, 280.0, 576.0));
        assert_eq!(at(PanelPlacement::Right), rect(508.0, 12.0, 280.0, 576.0));
        assert_eq!(at(PanelPlacement::Top), rect(12.0, 12.0, 776.0, 200.0));
        assert_eq!(at(PanelPlacement::Bottom), rect(12.0, 388.0, 776.0, 200.0));
        assert_eq!(at(PanelPlacement::TopLeft), rect(12.0, 12.0, 280.0, 200.0));
        assert_eq!(
            at(PanelPlacement::BottomRight),
            rect(508.0, 388.0, 280.0, 200.0)
        );
        assert_eq!(at(PanelPlacement::Center), rect(260.0, 200.0, 280.0, 200.0));
        let floating = PanelPlacement::Floating(Vec2::new(300.0, 100.0));
        assert_eq!(at(floating), rect(300.0, 100.0, 280.0, 200.0));
        // Bigger than its parent, it overflows, as layout would show it.
        let huge = PanelSize(Vec2::splat(2000.0));
        assert_eq!(
            PanelPlacement::TopLeft.rect(huge, MARGIN, AREA),
            rect(12.0, 12.0, 2000.0, 2000.0)
        );
    }

    #[test]
    fn layout_is_read_in_logical_pixels_at_any_scale() {
        // At a scale factor of 2: a parent whose room starts at (50, 25) and
        // is 800 by 600, holding a panel 12 in from its top-left corner.
        let parent = (
            ComputedNode {
                size: AREA * 2.0,
                inverse_scale_factor: 0.5,
                ..ComputedNode::DEFAULT
            },
            UiGlobalTransform::from_translation(Vec2::new(50.0, 25.0) * 2.0 + AREA),
        );
        let (area, origin) = dock_area((&parent.0, &parent.1));
        assert_eq!((area, origin), (AREA, Vec2::new(50.0, 25.0)));
        let panel = (
            ComputedNode {
                size: SIZE.0 * 2.0,
                inverse_scale_factor: 0.5,
                ..ComputedNode::DEFAULT
            },
            UiGlobalTransform::from_translation((origin + Vec2::splat(MARGIN)) * 2.0 + SIZE.0),
        );
        assert_eq!(
            shown_rect((&panel.0, &panel.1), origin),
            rect(12.0, 12.0, 280.0, 200.0)
        );
    }

    fn app(config: PanelConfig) -> (App, Entity) {
        app_at(config, 1.0)
    }

    /// A panel in a parent laid out at `scale` physical pixels to the logical pixel.
    fn app_at(config: PanelConfig, scale: f32) -> (App, Entity) {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, PanelPlugin))
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
                FRAME_MS,
            )));
        let parent = app
            .world_mut()
            .spawn((
                Node::default(),
                ComputedNode {
                    size: AREA * scale,
                    inverse_scale_factor: 1.0 / scale,
                    ..ComputedNode::DEFAULT
                },
                UiGlobalTransform::from_translation(AREA * scale / 2.0),
            ))
            .id();
        let mut panel = None;
        app.world_mut()
            .commands()
            .entity(parent)
            .with_children(|parent| {
                panel = Some(spawn_panel(parent, config, |_| {}).id());
            });
        app.update();
        let panel = panel.unwrap();
        (app, panel)
    }

    /// Tells the panel where layout put it, since tests run no layout.
    fn place(app: &mut App, panel: Entity, at: Rect) {
        let scale = app
            .world()
            .get::<ChildOf>(panel)
            .and_then(|parent| app.world().get::<ComputedNode>(parent.parent()))
            .map_or(1.0, |parent| 1.0 / parent.inverse_scale_factor);
        app.world_mut().entity_mut(panel).insert((
            ComputedNode {
                size: at.size() * scale,
                inverse_scale_factor: 1.0 / scale,
                ..ComputedNode::DEFAULT
            },
            UiGlobalTransform::from_translation(at.center() * scale),
        ));
    }

    fn node(app: &App, panel: Entity) -> Node {
        app.world().get::<Node>(panel).unwrap().clone()
    }

    fn placement(app: &App, panel: Entity) -> PanelPlacement {
        *app.world().get::<PanelPlacement>(panel).unwrap()
    }

    fn set_placement(app: &mut App, panel: Entity, to: PanelPlacement) {
        *app.world_mut().get_mut::<PanelPlacement>(panel).unwrap() = to;
    }

    fn activity(app: &App, panel: Entity) -> PanelActivity {
        *app.world().get::<PanelActivity>(panel).unwrap()
    }

    #[test]
    fn a_panel_spawns_pinned_and_styled_for_its_place() {
        let (flush_app, flush) = app(PanelConfig::new()
            .placement(PanelPlacement::Left)
            .size(280.0, 200.0));
        let left = node(&flush_app, flush);
        assert_eq!(
            (left.left, left.right, left.top, left.bottom),
            (px(0.0), Val::Auto, px(0.0), px(0.0)),
            "pinned to three edges, stretched along the left"
        );
        assert_eq!((left.width, left.height), (px(280.0), Val::Auto));
        assert_eq!(
            left.border,
            UiRect::right(px(BORDER_WIDTH)),
            "edged toward the content"
        );
        assert!(
            flush_app.world().get::<ThemeCornerRadius>(flush).is_none(),
            "flush, so square"
        );

        let (inset_app, inset) = app(PanelConfig::new()
            .placement(PanelPlacement::Left)
            .margin(MARGIN));
        assert_eq!(node(&inset_app, inset).left, px(MARGIN));
        assert_eq!(
            node(&inset_app, inset).border,
            UiRect::all(px(BORDER_WIDTH))
        );
        assert!(
            inset_app.world().get::<ThemeCornerRadius>(inset).is_some(),
            "kept off the edge, so a card like any other"
        );

        let (app, corner) = app(PanelConfig::new()
            .placement(PanelPlacement::BottomRight)
            .margin(MARGIN));
        let corner_node = node(&app, corner);
        assert_eq!(
            (corner_node.right, corner_node.bottom),
            (px(MARGIN), px(MARGIN))
        );
        assert!(app.world().get::<ThemeCornerRadius>(corner).is_some());
        assert!(app.world().get::<ThemeSkin>(corner).is_some());
    }

    #[test]
    fn new_tokens_restyle_the_panel_at_once() {
        let (mut app, panel) = app(PanelConfig::new());
        let custom = PanelTokens::with_prefix("test.panel");
        custom.register_theme_defaults(&mut app.world_mut().resource_mut::<Theme>());
        app.world_mut().entity_mut(panel).insert(custom.clone());
        app.world_mut().flush();
        assert_eq!(app.world().get::<ThemeSkin>(panel).unwrap().0, custom.bg);
    }

    #[test]
    fn a_new_placement_glides_there_then_pins() {
        let (mut app, panel) = app(PanelConfig::new()
            .placement(PanelPlacement::Right)
            .size(280.0, 200.0)
            .margin(MARGIN));
        let from = PanelPlacement::Right.rect(SIZE, MARGIN, AREA);
        let to = PanelPlacement::BottomLeft.rect(SIZE, MARGIN, AREA);
        place(&mut app, panel, from);
        set_placement(&mut app, panel, PanelPlacement::BottomLeft);
        app.update();
        app.update();
        let Val::Px(left) = node(&app, panel).left else {
            panic!("gliding at a worked-out place");
        };
        assert!(left < from.min.x && left > to.min.x, "on its way: {left}");

        for _ in 0..60 {
            app.update();
        }
        let pinned = node(&app, panel);
        assert_eq!(
            (pinned.left, pinned.bottom, pinned.right, pinned.top),
            (px(MARGIN), px(MARGIN), Val::Auto, Val::Auto)
        );
    }

    #[test]
    fn a_glide_starts_where_the_panel_shows_at_any_scale() {
        let (mut app, panel) = app_at(
            PanelConfig::new()
                .placement(PanelPlacement::Right)
                .size(280.0, 200.0),
            2.0,
        );
        let from = PanelPlacement::Right.rect(SIZE, 0.0, AREA);
        place(&mut app, panel, from);
        set_placement(&mut app, panel, PanelPlacement::Left);
        app.update();
        // One frame in, it has barely left: half of 12 000 px/s² for 16 ms squared.
        let Val::Px(left) = node(&app, panel).left else {
            panic!("gliding at a worked-out place");
        };
        assert!((from.min.x - left).abs() < 2.0, "no jump: {left}");
    }

    #[test]
    fn a_change_mid_glide_aims_the_glide_again_from_where_it_is() {
        let (mut app, panel) = app(PanelConfig::new()
            .placement(PanelPlacement::Right)
            .size(280.0, 200.0));
        place(&mut app, panel, PanelPlacement::Right.rect(SIZE, 0.0, AREA));
        set_placement(&mut app, panel, PanelPlacement::Left);
        app.update();
        app.update();
        let partway = activity(&app, panel).shown.unwrap();
        app.world_mut().get_mut::<PanelSize>(panel).unwrap().0.x = 100.0;
        app.update();
        let glide = activity(&app, panel).glide.expect("still gliding");
        assert_eq!(
            from_vec4(glide.target()),
            rect(0.0, 0.0, 100.0, 600.0),
            "to the new size"
        );
        let Val::Px(left) = node(&app, panel).left else {
            panic!("gliding at a worked-out place");
        };
        assert!(
            (partway.min.x - left).abs() < 2.0,
            "from where it was: {left}"
        );
    }

    #[test]
    fn without_acceleration_it_jumps_and_a_resize_is_immediate() {
        let (mut app, panel) = app(PanelConfig::new()
            .placement(PanelPlacement::Right)
            .acceleration(None));
        set_placement(&mut app, panel, PanelPlacement::Top);
        app.update();
        let top = node(&app, panel);
        assert_eq!((top.top, top.left, top.right), (px(0.0), px(0.0), px(0.0)));
        assert_eq!(top.border_radius, BorderRadius::ZERO);

        app.world_mut().get_mut::<PanelSize>(panel).unwrap().0.y = 90.0;
        app.update();
        assert_eq!(node(&app, panel).height, px(90.0));
    }

    #[test]
    fn a_pointer_at_an_edge_or_corner_docks_there() {
        let at = |x: f32, y: f32| snap(Vec2::new(x, y), AREA);
        assert_eq!(at(20.0, 300.0), Some(PanelPlacement::Left));
        assert_eq!(at(400.0, 590.0), Some(PanelPlacement::Bottom));
        assert_eq!(at(20.0, 20.0), Some(PanelPlacement::TopLeft));
        assert_eq!(
            at(790.0, 570.0),
            Some(PanelPlacement::BottomRight),
            "at an edge and near its end is a corner"
        );
        assert_eq!(
            at(770.0, 570.0),
            None,
            "near a corner but at neither edge is nothing"
        );
        assert_eq!(at(400.0, 300.0), None);
        assert_eq!(
            at(-30.0, 900.0),
            Some(PanelPlacement::BottomLeft),
            "past the edges counts as the edge"
        );
    }

    #[test]
    fn where_a_panel_may_not_go_it_floats_or_goes_home() {
        let corner = Vec2::new(300.0, 100.0);
        let home = PanelPlacement::TopLeft;
        let drop = |x: f32, y: f32, allowed| dock_at(Vec2::new(x, y), corner, AREA, allowed, home);
        assert_eq!(
            drop(400.0, 300.0, PanelDockTargets::ALL),
            PanelPlacement::Floating(corner),
            "away from the edges, it floats where it is"
        );
        let fixed_size = PanelDockTargets::CORNERS | PanelDockTargets::FLOATING;
        assert_eq!(drop(790.0, 10.0, fixed_size), PanelPlacement::TopRight);
        assert_eq!(
            drop(20.0, 300.0, fixed_size),
            PanelPlacement::Floating(corner),
            "not to an edge, so it floats"
        );
        assert_eq!(
            drop(400.0, 300.0, PanelDockTargets::CORNERS),
            home,
            "can't float, so it goes home"
        );
    }

    /// The panel's grabber that carries, or resizes.
    fn grabber(app: &mut App, panel: Entity, carries: bool) -> Entity {
        let world = app.world_mut();
        world
            .query::<(Entity, &GrabberRole)>()
            .iter(world)
            .find(|(_, role)| match role {
                GrabberRole::Carries(of) => carries && *of == panel,
                GrabberRole::Resizes(of) => !carries && *of == panel,
            })
            .unwrap()
            .0
    }

    fn grab(app: &mut App, grabber: Entity, phase: GrabPhase, offset: Vec2) {
        grab_at(app, grabber, phase, offset, Vec2::ZERO);
    }

    fn grab_at(app: &mut App, grabber: Entity, phase: GrabPhase, offset: Vec2, position: Vec2) {
        app.world_mut().trigger(Grabbed {
            grabber,
            phase,
            offset,
            position,
        });
        app.update();
    }

    #[test]
    fn a_floating_panel_resizes_from_its_bottom_right() {
        let floating = PanelPlacement::Floating(Vec2::new(300.0, 100.0));
        let (mut app, panel) = app(PanelConfig::new().placement(floating).resizable(true));
        let pinned = node(&app, panel);
        assert_eq!(
            (pinned.left, pinned.top, pinned.width, pinned.height),
            (px(300.0), px(100.0), px(DEFAULT_SIZE.x), px(DEFAULT_SIZE.y))
        );
        let corner = app
            .world()
            .get::<ResizeHandles>(panel)
            .unwrap()
            .corner_grabber;
        grab(&mut app, corner, GrabPhase::Start, Vec2::ZERO);
        grab(&mut app, corner, GrabPhase::End, Vec2::new(20.0, 10.0));
        assert_eq!(
            app.world().get::<PanelSize>(panel),
            Some(&PanelSize(DEFAULT_SIZE + Vec2::new(20.0, 10.0)))
        );
    }

    #[test]
    fn dragging_the_handle_resizes_an_edge_panel_within_limits() {
        let (mut app, panel) = app(PanelConfig::new()
            .placement(PanelPlacement::Right)
            .width(280.0)
            .resizable(true));
        let handle = grabber(&mut app, panel, false);
        let width = |app: &App| app.world().get::<PanelSize>(panel).unwrap().0.x;
        grab(&mut app, handle, GrabPhase::Start, Vec2::ZERO);
        grab(&mut app, handle, GrabPhase::Move, Vec2::new(-40.0, 15.0));
        assert_eq!(width(&app), 320.0, "dragged out, wider");
        assert_eq!(node(&app, panel).width, px(320.0), "at once, no glide");
        grab(&mut app, handle, GrabPhase::End, Vec2::new(500.0, 0.0));
        assert_eq!(width(&app), DEFAULT_MIN_SIZE, "no smaller than the least");
    }

    #[test]
    fn a_cancelled_grab_puts_the_panel_back() {
        let (mut app, panel) = app(PanelConfig::new()
            .placement(PanelPlacement::Right)
            .size(280.0, 200.0)
            .margin(MARGIN)
            .resizable(true)
            .movable(GrabberStyle::Grip));
        place(
            &mut app,
            panel,
            PanelPlacement::Right.rect(SIZE, MARGIN, AREA),
        );
        let handle = grabber(&mut app, panel, false);
        let width = |app: &App| app.world().get::<PanelSize>(panel).unwrap().0.x;
        grab(&mut app, handle, GrabPhase::Start, Vec2::ZERO);
        grab(&mut app, handle, GrabPhase::Move, Vec2::new(-40.0, 0.0));
        assert_eq!(width(&app), 320.0);
        grab(&mut app, handle, GrabPhase::Cancel, Vec2::ZERO);
        assert_eq!(width(&app), 280.0, "its size before the drag");

        let grip = grabber(&mut app, panel, true);
        grab(&mut app, grip, GrabPhase::Start, Vec2::ZERO);
        let middle = Vec2::new(400.0, 300.0);
        grab_at(&mut app, grip, GrabPhase::Move, Vec2::ZERO, middle);
        grab_at(&mut app, grip, GrabPhase::Cancel, Vec2::ZERO, middle);
        assert_eq!(placement(&app, panel), PanelPlacement::Right);
        let back = activity(&app, panel);
        assert!(back.grab.is_none() && back.landing.is_none());
        assert!(back.glide.is_some(), "it glides home from the hand");
    }

    #[test]
    fn a_carried_panel_follows_the_hand_then_docks_where_it_lands() {
        let (mut app, panel) = app(PanelConfig::new()
            .placement(PanelPlacement::Right)
            .size(280.0, 200.0)
            .margin(MARGIN)
            .movable(GrabberStyle::Grip));
        place(
            &mut app,
            panel,
            PanelPlacement::Right.rect(SIZE, MARGIN, AREA),
        );
        let grip = grabber(&mut app, panel, true);
        // Taken at its top right corner.
        grab_at(
            &mut app,
            grip,
            GrabPhase::Start,
            Vec2::ZERO,
            Vec2::new(788.0, 12.0),
        );
        let held = node(&app, panel);
        assert_eq!(
            (held.left, held.top, held.width, held.height),
            (px(508.0), px(12.0), px(280.0), px(200.0)),
            "at once its own size, still under the hand"
        );
        grab_at(
            &mut app,
            grip,
            GrabPhase::Move,
            Vec2::new(-300.0, 0.0),
            Vec2::new(488.0, 12.0),
        );
        assert_eq!(
            (node(&app, panel).left, node(&app, panel).top),
            (px(208.0), px(12.0))
        );
        assert_eq!(
            activity(&app, panel).landing,
            Some(PanelPlacement::Top.rect(SIZE, MARGIN, AREA)),
            "it would dock along the top"
        );
        // Let go of in the top left corner.
        grab_at(
            &mut app,
            grip,
            GrabPhase::End,
            Vec2::new(-768.0, 18.0),
            Vec2::new(20.0, 30.0),
        );
        assert_eq!(placement(&app, panel), PanelPlacement::TopLeft);
        let landed = activity(&app, panel);
        assert!(landed.glide.is_some(), "and glides there");
        assert_eq!(landed.landing, None, "nothing to outline once it's let go");
        assert!(
            app.world().get::<ThemeCornerRadius>(panel).is_some(),
            "restyled for the corner at once"
        );
        assert_eq!(node(&app, panel).border, UiRect::all(px(BORDER_WIDTH)));
    }

    #[test]
    fn a_carried_panel_rides_above_the_rest_until_it_lands() {
        let (mut app, panel) = app(PanelConfig::new()
            .placement(PanelPlacement::TopLeft)
            .size(280.0, 200.0)
            .margin(MARGIN)
            .z_index(3)
            .movable(GrabberStyle::Grip));
        let z = |app: &App| *app.world().get::<ZIndex>(panel).unwrap();
        place(
            &mut app,
            panel,
            PanelPlacement::TopLeft.rect(SIZE, MARGIN, AREA),
        );
        let grip = grabber(&mut app, panel, true);
        let hand = Vec2::splat(32.0);
        grab_at(&mut app, grip, GrabPhase::Start, Vec2::ZERO, hand);
        grab_at(
            &mut app,
            grip,
            GrabPhase::Move,
            Vec2::ZERO,
            Vec2::new(400.0, 300.0),
        );
        assert_eq!(z(&app), LIFTED, "in hand, above everything");
        assert_eq!(
            activity(&app, panel).landing,
            None,
            "it would float where it is"
        );
        grab_at(
            &mut app,
            grip,
            GrabPhase::End,
            Vec2::ZERO,
            Vec2::new(790.0, 300.0),
        );
        assert_eq!(placement(&app, panel), PanelPlacement::Right);
        assert_eq!(z(&app), LIFTED, "still above while it glides");
        for _ in 0..60 {
            app.update();
        }
        assert_eq!(z(&app), ZIndex(3), "landed, back in its place");

        // Let go where it shows, floating, it lands there at once.
        place(
            &mut app,
            panel,
            PanelPlacement::Right.rect(SIZE, MARGIN, AREA),
        );
        grab_at(
            &mut app,
            grip,
            GrabPhase::Start,
            Vec2::ZERO,
            Vec2::new(600.0, 100.0),
        );
        grab_at(
            &mut app,
            grip,
            GrabPhase::End,
            Vec2::ZERO,
            Vec2::new(400.0, 300.0),
        );
        assert!(matches!(
            placement(&app, panel),
            PanelPlacement::Floating(_)
        ));
        assert!(activity(&app, panel).glide.is_none());
        assert_eq!(z(&app), ZIndex(3));
    }

    /// How many of the outline's nodes are showing, found by where they sit.
    fn outline_parts_showing(app: &mut App) -> usize {
        let world = app.world_mut();
        world
            .query_filtered::<(&Node, &ZIndex), Without<PanelPlacement>>()
            .iter(world)
            .filter(|(node, z)| z.0 >= i32::MAX - 2 && node.display != Display::None)
            .count()
    }

    #[test]
    fn a_previewed_carry_leaves_the_panel_until_it_is_let_go() {
        let (mut app, panel) = app(PanelConfig::new()
            .placement(PanelPlacement::Right)
            .size(280.0, 200.0)
            .movable(GrabberStyle::Grip)
            .feedback(DragFeedback::Preview));
        let resting = node(&app, panel);
        place(&mut app, panel, PanelPlacement::Right.rect(SIZE, 0.0, AREA));
        let grip = grabber(&mut app, panel, true);
        grab_at(
            &mut app,
            grip,
            GrabPhase::Start,
            Vec2::ZERO,
            Vec2::new(700.0, 10.0),
        );
        grab_at(
            &mut app,
            grip,
            GrabPhase::Move,
            Vec2::ZERO,
            Vec2::new(10.0, 300.0),
        );
        assert_eq!(node(&app, panel), resting, "it stays put while held");
        assert_eq!(
            outline_parts_showing(&mut app),
            2,
            "outlined where it would land"
        );
        assert_eq!(*app.world().get::<ZIndex>(panel).unwrap(), LIFTED);
        assert_eq!(
            activity(&app, panel).landing,
            Some(PanelPlacement::Left.rect(SIZE, 0.0, AREA)),
            "but where it would land is known"
        );
        grab_at(
            &mut app,
            grip,
            GrabPhase::End,
            Vec2::ZERO,
            Vec2::new(10.0, 300.0),
        );
        assert_eq!(outline_parts_showing(&mut app), 0, "the outline's gone");
        assert_eq!(placement(&app, panel), PanelPlacement::Left);
        let Val::Px(left) = node(&app, panel).left else {
            panic!("gliding at a worked-out place");
        };
        assert!(left > 400.0, "gliding from where it waited: {left}");
        assert_eq!(*app.world().get::<ZIndex>(panel).unwrap(), LIFTED);
    }

    #[test]
    fn the_resize_grabbers_follow_the_placement_to_its_inner_edge_or_corner() {
        let (mut app, panel) = app(PanelConfig::new()
            .placement(PanelPlacement::Right)
            .resizable(true)
            .acceleration(None));
        let resize = *app.world().get::<ResizeHandles>(panel).unwrap();
        let shown = |app: &App, place: Entity| {
            app.world().get::<Node>(place).unwrap().display != Display::None
        };
        assert!(shown(&app, resize.edge_container) && !shown(&app, resize.corner_container));

        set_placement(&mut app, panel, PanelPlacement::Top);
        app.update();
        let strip = app.world().get::<Node>(resize.edge_container).unwrap();
        assert_eq!((strip.bottom, strip.top), (px(0.0), Val::Auto));
        assert_eq!(
            app.world().get::<GrabAxis>(resize.edge_grabber),
            Some(&GrabAxis::Vertical)
        );

        set_placement(&mut app, panel, PanelPlacement::BottomLeft);
        app.update();
        assert!(!shown(&app, resize.edge_container) && shown(&app, resize.corner_container));
        assert_eq!(
            app.world().get::<GrabCorner>(resize.corner_grabber),
            Some(&GrabCorner::TopRight),
            "facing the middle"
        );
    }

    #[test]
    fn a_corner_panel_resizes_both_ways_from_its_inner_corner() {
        let (mut app, panel) = app(PanelConfig::new()
            .placement(PanelPlacement::BottomRight)
            .size(280.0, 200.0)
            .margin(MARGIN)
            .resizable(true)
            .max_size(600.0, f32::INFINITY));
        let corner = app
            .world()
            .get::<ResizeHandles>(panel)
            .unwrap()
            .corner_grabber;
        let size = |app: &App| app.world().get::<PanelSize>(panel).unwrap().0;
        grab(&mut app, corner, GrabPhase::Start, Vec2::ZERO);
        grab(&mut app, corner, GrabPhase::Move, Vec2::new(-50.0, -30.0));
        assert_eq!(size(&app), Vec2::new(330.0, 230.0), "up and left is bigger");
        grab(&mut app, corner, GrabPhase::End, Vec2::new(-900.0, -900.0));
        assert_eq!(
            size(&app),
            Vec2::new(600.0, AREA.y - 2.0 * MARGIN),
            "no wider than its most, no taller than its margins allow"
        );
    }

    #[test]
    fn a_grip_keeps_out_of_a_corner_grabbers_way() {
        let grip_sides = |app: &App, panel: Entity| {
            let CarryGrip(place) = *app.world().get::<CarryGrip>(panel).unwrap();
            let node = app.world().get::<Node>(place).unwrap();
            (node.left, node.right)
        };
        let top_left = (px(GRIP_INSET), Val::Auto);
        let top_right = (Val::Auto, px(GRIP_INSET));
        let bottom_left = PanelConfig::new()
            .placement(PanelPlacement::BottomLeft)
            .movable(GrabberStyle::Grip);

        let (fixed_app, fixed) = app(bottom_left.clone());
        assert_eq!(grip_sides(&fixed_app, fixed), top_right);

        let (mut app, panel) = app(bottom_left.resizable(true).acceleration(None));
        assert_eq!(
            grip_sides(&app, panel),
            top_left,
            "the corner grabber has the top right"
        );
        set_placement(&mut app, panel, PanelPlacement::TopLeft);
        app.update();
        assert_eq!(grip_sides(&app, panel), top_right, "back once it's gone");
    }

    #[test]
    fn a_hand_keeps_to_the_panels_limits_but_code_need_not() {
        let (mut app, panel) = app(PanelConfig::new()
            .placement(PanelPlacement::Left)
            .width(280.0)
            .resizable(true)
            .min_size(200.0, 0.0)
            .max_size(400.0, 0.0));
        let handle = grabber(&mut app, panel, false);
        let width = |app: &App| app.world().get::<PanelSize>(panel).unwrap().0.x;
        grab(&mut app, handle, GrabPhase::Start, Vec2::ZERO);
        grab(&mut app, handle, GrabPhase::Move, Vec2::new(-200.0, 0.0));
        assert_eq!(width(&app), 200.0, "no narrower than its least");
        grab(&mut app, handle, GrabPhase::End, Vec2::new(500.0, 0.0));
        assert_eq!(width(&app), 400.0, "no wider than its most");
        app.world_mut().get_mut::<PanelSize>(panel).unwrap().0.x = 20.0;
        app.update();
        assert_eq!(width(&app), 20.0, "the app may set what it likes");

        let limits = PanelLimits {
            min: Vec2::splat(300.0),
            max: Vec2::splat(900.0),
        };
        assert_eq!(
            limits.bound(Vec2::new(1000.0, 100.0), AREA),
            Vec2::new(800.0, 300.0),
            "the room wins over the limits"
        );
    }

    #[test]
    fn an_unusable_config_is_repaired() {
        let config = PanelConfig::new()
            .size(-1.0, 50.0)
            .padding(-5.0)
            .margin(f32::NAN)
            .min_size(100.0, 100.0)
            .max_size(50.0, 200.0)
            .acceleration(Some(-3.0))
            .sanitized();
        assert_eq!(config.size, PanelSize(Vec2::new(0.0, 50.0)));
        assert_eq!(config.padding, 0.0);
        assert_eq!(config.settings.margin, 0.0);
        assert_eq!(
            config.settings.limits.max,
            Vec2::new(100.0, 200.0),
            "no less than the least"
        );
        assert_eq!(config.settings.acceleration, None);
        assert_eq!(
            PanelConfig::new()
                .acceleration(Some(0.0))
                .sanitized()
                .settings
                .acceleration,
            None,
            "none at all jumps, quietly"
        );
    }

    /// By default a panel is part of its parent's layout, as any node is:
    /// in a column it takes up room, rather than floating over the column.
    #[test]
    fn panels_sit_in_their_parents_layout_by_default() {
        let (app, panel) = app(PanelConfig::new().size(200.0, 120.0));
        let node = app.world().get::<Node>(panel).unwrap();
        assert_eq!(node.position_type, PositionType::Relative);
        assert_eq!((node.left, node.top), (Val::Auto, Val::Auto));
        assert_eq!((node.width, node.height), (px(200.0), px(120.0)));
        assert_eq!(node.margin, UiRect::ZERO);
    }

    /// A panel among its siblings can't be carried or resized; the config is
    /// repaired rather than refused.
    #[test]
    fn inline_panels_drop_carrying_and_resizing() {
        let config = PanelConfig::new()
            .movable(GrabberStyle::Grip)
            .resizable(true)
            .sanitized();
        assert_eq!(config.movable, None);
        assert!(!config.resizable);

        let docked = PanelConfig::new()
            .placement(PanelPlacement::Left)
            .resizable(true)
            .sanitized();
        assert!(docked.resizable, "only inline panels lose it");
    }

    /// A pinned panel in a parent too small for it is warned about once, and marked so.
    #[test]
    fn a_pinned_panel_too_big_for_its_parent_is_warned_about_once() {
        let roomy = PanelConfig::new().placement(PanelPlacement::TopLeft);
        let (mut fits, panel) = app(roomy.size(100.0, 50.0));
        fits.update();
        assert!(!fits.world().entity(panel).contains::<WarnedCramped>());

        let cramped = PanelConfig::new().placement(PanelPlacement::BottomLeft);
        let (mut spills, panel) = app(cramped.size(100.0, AREA.y * 2.0));
        spills.update();
        spills.update();
        assert!(spills.world().entity(panel).contains::<WarnedCramped>());
    }
}
