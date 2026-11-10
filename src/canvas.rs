//! Canvases: a picture in the UI, and where the pointer is in it.
//!
//! A canvas shows one of three things, chosen by its [`CanvasConfig`]:
//!
//! - **An image** ([`CanvasConfig::image`]): an image asset, fitted by its
//!   [`CanvasFit`]. This is touchy's image widget.
//! - **A painting** ([`CanvasConfig::painted`]): whatever the app draws on it,
//!   as entities under its [stage](Canvas::stage) or lines with [`CanvasSketch`].
//! - **A camera's view** ([`CanvasConfig::camera`]): what a camera the app owns
//!   sees, such as a minimap's. With a [`CursorPlane`](crate::world_view::CursorPlane)
//!   on the camera, the pointer reaches into that world through the canvas.
//!
//! Every canvas reports the mouse over it in its [`CanvasPointer`].
//!
//! To draw over a canvas, such as markers on a minimap: put what belongs to
//! the world (a player's dot) on a render layer only that camera sees, and
//! what belongs to the canvas (a compass, a frame) in absolutely positioned
//! UI children of the canvas, which lie over its picture.

use bevy::app::{HierarchyPropagatePlugin, Propagate, PropagateSet};
use bevy::asset::AssetMut;
use bevy::camera::visibility::{RenderLayers, VisibilitySystems};
use bevy::camera::{RenderTarget, ScalingMode};
use bevy::ecs::system::SystemParam;
use bevy::gizmos::config::GizmoLineConfig;
use bevy::gizmos::retained::Gizmo;
use bevy::picking::PickingSystems;
use bevy::picking::hover::HoverMap;
use bevy::picking::pointer::{PointerId, PointerLocation};
use bevy::prelude::*;
use bevy::ui::{ComputedUiRenderTargetInfo, UiGlobalTransform, UiSystems};

use crate::EnsurePlugin;
use crate::primitives::Unstretched;
use crate::sanitize::at_least;
use crate::theme::{Theme, ThemePlugin, ThemeSkin, ThemeToken};
use crate::world_view::viewport_render_target;

/// Prefix of the canvas's theme tokens.
pub const CANVAS_STYLE: &str = "touchy.canvas";

/// The first render layer handed to paintings, unless the app says otherwise
/// with [`CanvasLayers::starting_at`]. Layers below it are left to the game.
const FIRST_LAYER: usize = 32;
/// A painting's camera draws before the cameras that show it, so it's never a
/// frame behind.
const PAINTING_ORDER: isize = -1;
/// How thick a sketch's lines are, in logical pixels.
const DEFAULT_LINE_WIDTH: f32 = 2.0;

/// Theme tokens for a canvas. Transparent by default.
#[derive(Component, Clone, PartialEq, Debug)]
#[component(immutable)]
pub struct CanvasTokens {
    /// The canvas's background, behind what is drawn on it. Transparent by default.
    pub bg: ThemeToken,
}

impl CanvasTokens {
    /// Names the tokens under `prefix`.
    pub fn with_prefix(prefix: &str) -> Self {
        Self {
            bg: ThemeToken::new(format!("{prefix}.bg")),
        }
    }

    /// Register the default values for the canvas tokens, unless the app sets them.
    pub fn register_theme_defaults(&self, theme: &mut Theme) {
        theme.default_color(self.bg.clone(), Color::NONE);
    }
}

/// The built-in token set.
impl Default for CanvasTokens {
    fn default() -> Self {
        Self::with_prefix(CANVAS_STYLE)
    }
}

/// How an image fills its canvas. Change it to refit the image.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CanvasFit {
    /// To the canvas's size exactly, whatever its shape.
    #[default]
    Stretch,
    /// As large as fits inside, keeping its shape, centred.
    Contain,
    /// As small as covers it, keeping its shape, centred; the rest is clipped.
    Cover,
}

impl CanvasFit {
    /// The size an image of `image` pixels is drawn at in a canvas of `area`.
    fn fit(self, image: Vec2, area: Vec2) -> Vec2 {
        if image.min_element() <= 0.0 {
            return area;
        }
        let scales = area / image;
        match self {
            Self::Stretch => area,
            Self::Contain => image * scales.min_element(),
            Self::Cover => image * scales.max_element(),
        }
    }
}

/// An image canvas's image. Insert a new one to show another.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct CanvasImage(pub Handle<Image>);

/// Where the mouse is on a canvas, or `None` while it's elsewhere or something
/// covers the canvas. It's in the space of what the canvas shows: an image's
/// pixels from its top left corner, as an image is indexed; for a painting or
/// a camera's view, the canvas's logical pixels from its bottom left corner,
/// y up, as its picture is drawn.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct CanvasPointer(pub Option<Vec2>);

/// The render layers paintings are given, one each, handed back when a
/// painting goes. Insert one made with [`starting_at`](Self::starting_at)
/// before adding the plugin to keep paintings clear of layers the game uses.
#[derive(Resource, Clone, Debug)]
pub struct CanvasLayers {
    next: usize,
    free: Vec<usize>,
}

impl CanvasLayers {
    /// Hands out layers from `first` up.
    pub fn starting_at(first: usize) -> Self {
        Self {
            next: first,
            free: Vec::new(),
        }
    }
    /// A layer no painting has: one handed back, or the next unused.
    fn take(&mut self) -> usize {
        self.free.pop().unwrap_or_else(|| {
            self.next += 1;
            self.next - 1
        })
    }
    /// Returns `layer`, whose painting has gone, for the next painting.
    fn give_back(&mut self, layer: usize) {
        self.free.push(layer);
    }
}

impl Default for CanvasLayers {
    fn default() -> Self {
        Self::starting_at(FIRST_LAYER)
    }
}

/// What a canvas shows.
#[derive(Clone, Debug)]
enum Source {
    Image(Handle<Image>),
    Painting { line_width: f32 },
    Camera(Entity),
}

/// Construction-time configuration for a canvas.
///
/// Consumed by [`SpawnUi::canvas`](crate::spawn_ui::SpawnUi::canvas) and not retained as a
/// component.
///
/// # Examples
/// ```
/// use bevy::prelude::*;
/// use touchy::canvas::{CanvasConfig, CanvasFit};
///
/// # let portrait: Handle<Image> = Handle::default();
/// let config = CanvasConfig::image(portrait)
///     .size(200.0, 100.0)
///     .fit(CanvasFit::Contain);
/// # assert_eq!(config.size, Some(Vec2::new(200.0, 100.0)));
/// ```
#[derive(Clone, Debug)]
pub struct CanvasConfig {
    /// Where the data to be shown is coming from.
    source: Source,
    /// The canvas's size, in pixels. Without one, an image canvas takes its
    /// image's size, and the others fill what their parent gives them.
    pub size: Option<Vec2>,
    /// How an image fills the canvas; the others always fill it exactly.
    pub fit: CanvasFit,
    /// The canvas's theme tokens.
    pub tokens: CanvasTokens,
}

impl CanvasConfig {
    /// Create a new canvas config for `source`.
    fn new(source: Source) -> Self {
        Self {
            source,
            size: None,
            fit: CanvasFit::default(),
            tokens: CanvasTokens::with_prefix(CANVAS_STYLE),
        }
    }
    /// A canvas showing `image`.
    pub fn image(image: Handle<Image>) -> Self {
        Self::new(Source::Image(image))
    }
    /// A canvas for the app to draw on. It has a camera and a render layer of
    /// its own, so anything on that layer is drawn on it and nowhere else, in
    /// the canvas's own pixels.
    pub fn painted() -> Self {
        Self::new(Source::Painting {
            line_width: DEFAULT_LINE_WIDTH,
        })
    }
    /// A canvas showing what `camera` sees, in the camera's world: a viewport.
    /// The canvas has it draw offscreen for the canvas, if it doesn't already.
    pub fn camera(camera: Entity) -> Self {
        Self::new(Source::Camera(camera))
    }
    /// Fixes its size, in pixels: an image is fitted to it by [`fit`](Self::fit).
    pub fn size(mut self, width: f32, height: f32) -> Self {
        self.size = Some(Vec2::new(width, height));
        self
    }
    /// Sets how the canvas fits its content.
    pub fn fit(mut self, fit: CanvasFit) -> Self {
        self.fit = fit;
        self
    }
    /// How thick a painting's sketched lines are, in logical pixels.
    pub fn line_width(mut self, pixels: f32) -> Self {
        if let Source::Painting { line_width } = &mut self.source {
            *line_width = pixels;
        }
        self
    }
    /// Sets the canvas's theme tokens.
    pub fn tokens(mut self, tokens: CanvasTokens) -> Self {
        self.tokens = tokens;
        self
    }
    /// This config with every value made usable, warning about each repair.
    fn sanitized(mut self) -> Self {
        self.size = at_least(self.size, Vec2::ZERO, "CanvasConfig size");
        if let Source::Painting { line_width } = &mut self.source {
            *line_width = at_least(*line_width, 0.0, "CanvasConfig line width");
        }
        self
    }
}

/// A canvas: its size in the space of what it shows, and, if it's a painting,
/// where to put what's drawn on it.
#[derive(Component, Clone, Debug)]
pub struct Canvas {
    /// The node that shows the picture, inside the canvas's frame.
    picture: Entity,
    /// What visual data the canvas shows.
    shows: Shows,
    /// How big its picture is, in the picture's own units.
    picture_size: Vec2,
}

/// What a canvas shows, once it's set up.
#[derive(Clone, Debug)]
enum Shows {
    /// A static image, stretched to the canvas's size by its [`CanvasFit`].
    Image,
    /// A painting waiting for its camera and layer, with its lines' width.
    PendingPainting { line_width: f32 },
    /// A painting, with its camera and layer.
    Painting(Painting),
    /// A camera view.
    Camera(Entity),
}

/// The parts of a painting the canvas owns.
#[derive(Clone, Debug)]
struct Painting {
    layer: usize,
    camera: Entity,
    stage: Entity,
    sketch: Handle<GizmoAsset>,
    sketch_entity: Entity,
    line_width: f32,
}

impl Canvas {
    /// How big its picture is, in the picture's own units: an image's pixels,
    /// or a painting's or a camera view's logical pixels. Zero until it's laid
    /// out, or its image loads.
    pub fn picture_size(&self) -> Vec2 {
        self.picture_size
    }
    /// Where to spawn what's drawn on a painting: anything under it is put on
    /// the painting's layer, at the painting's coordinates. `None` for other canvases.
    pub fn stage(&self) -> Option<Entity> {
        self.painting().map(|painting| painting.stage)
    }
    /// A painting's render layer, for anything drawn on it from elsewhere.
    pub fn render_layers(&self) -> Option<RenderLayers> {
        self.painting()
            .map(|painting| RenderLayers::layer(painting.layer))
    }
    /// A painting's parts, once it's set up.
    fn painting(&self) -> Option<&Painting> {
        match &self.shows {
            Shows::Painting(painting) => Some(painting),
            _ => None,
        }
    }
}

/// Spawns it; documented on [`SpawnUi::canvas`](crate::spawn_ui::SpawnUi::canvas).
pub(crate) fn spawn_canvas<'a>(
    parent: &'a mut ChildSpawnerCommands,
    config: CanvasConfig,
) -> EntityCommands<'a> {
    let CanvasConfig {
        source,
        size,
        fit,
        tokens,
    } = config.sanitized();
    let image = matches!(source, Source::Image(_));
    let (width, height) = match size {
        Some(size) => (px(size.x), px(size.y)),
        None if image => (Val::Auto, Val::Auto),
        None => (percent(100), percent(100)),
    };
    let frame = Node {
        width,
        height,
        flex_grow: if size.is_none() && !image { 1.0 } else { 0.0 },
        // A size of its own is kept, where a crowded parent would squeeze it.
        flex_shrink: if size.is_some() { 0.0 } else { 1.0 },
        min_width: px(0.0),
        min_height: px(0.0),
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        overflow: Overflow::clip(),
        ..default()
    };
    let mut picture = Entity::PLACEHOLDER;
    // Its background comes with its tokens, in `style_canvas`.
    let mut canvas = parent.spawn((frame, CanvasPointer::default()));
    // An image keeps to its own size, where a parent would stretch it.
    if image {
        canvas.insert(Unstretched);
    }
    canvas.with_children(|frame| {
        let fills = Node {
            width: percent(100),
            height: percent(100),
            flex_shrink: 0.0,
            ..default()
        };
        let mut node = frame.spawn(fills);
        if let Source::Image(image) = &source {
            // Stretched to the node, which the canvas sizes and places for the fit.
            node.insert(ImageNode::new(image.clone()).with_mode(NodeImageMode::Stretch));
        }
        picture = node.id();
    });
    let shows = match source {
        Source::Image(image) => {
            canvas.insert((CanvasImage(image), fit));
            Shows::Image
        }
        Source::Painting { line_width } => Shows::PendingPainting { line_width },
        Source::Camera(camera) => Shows::Camera(camera),
    };
    canvas.insert((
        Canvas {
            picture,
            shows,
            picture_size: Vec2::ZERO,
        },
        tokens,
    ));
    canvas
}

/// Gives a new painting its camera, layer, stage and sketch, and points a
/// new camera view's camera at its canvas.
fn set_up_canvas(
    added: On<Add<Canvas>>,
    mut canvases: Query<&mut Canvas>,
    targets: Query<&RenderTarget>,
    mut images: ResMut<Assets<Image>>,
    mut sketches: ResMut<Assets<GizmoAsset>>,
    mut layers: ResMut<CanvasLayers>,
    mut commands: Commands,
) {
    let Ok(mut canvas) = canvases.get_mut(added.entity) else {
        return;
    };
    let picture = canvas.picture;
    match canvas.shows {
        Shows::Image | Shows::Painting(_) => {}
        Shows::Camera(camera) => {
            let draws_offscreen = targets
                .get(camera)
                .is_ok_and(|target| target.as_image().is_some());
            if !draws_offscreen {
                commands
                    .entity(camera)
                    .insert(viewport_render_target(&mut images));
            }
            commands.entity(picture).insert(ViewportNode::new(camera));
        }
        Shows::PendingPainting { line_width } => {
            let layer = layers.take();
            let on_layer = RenderLayers::layer(layer);
            let camera = commands
                .spawn((
                    Camera2d,
                    Camera {
                        order: PAINTING_ORDER,
                        clear_color: ClearColorConfig::Custom(Color::NONE),
                        ..default()
                    },
                    viewport_render_target(&mut images),
                    Projection::Orthographic(OrthographicProjection {
                        // The canvas's bottom left corner is the origin.
                        viewport_origin: Vec2::ZERO,
                        scaling_mode: ScalingMode::WindowSize,
                        ..OrthographicProjection::default_2d()
                    }),
                    on_layer.clone(),
                ))
                .id();
            let stage = commands
                .spawn((
                    Name::new("canvas stage"),
                    Transform::default(),
                    Visibility::default(),
                    Propagate(on_layer.clone()),
                ))
                .id();
            let sketch = sketches.add(GizmoAsset::new());
            let sketch_entity = commands
                .spawn((
                    Gizmo {
                        handle: sketch.clone(),
                        line_config: GizmoLineConfig {
                            width: line_width,
                            ..default()
                        },
                        ..default()
                    },
                    on_layer,
                ))
                .id();
            commands.entity(picture).insert(ViewportNode::new(camera));
            canvas.shows = Shows::Painting(Painting {
                layer,
                camera,
                stage,
                sketch,
                sketch_entity,
                line_width,
            });
        }
    }
}

/// Styles a canvas when its tokens are set: at spawn, and whenever the app sets new ones.
fn style_canvas(
    insert: On<Insert<CanvasTokens>>,
    canvases: Query<&CanvasTokens>,
    mut commands: Commands,
) {
    if let Ok(tokens) = canvases.get(insert.entity) {
        commands
            .entity(insert.entity)
            .insert(ThemeSkin(tokens.bg.clone()));
    }
}

/// Clears each painting to its canvas's background, so translucent paint blends with the canvas,
/// rather than with what is under it. A canvas with no background still blends with nothing.
fn clear_paintings_to_their_background(
    theme: Res<Theme>,
    canvases: Query<(&Canvas, &CanvasTokens)>,
    mut cameras: Query<&mut Camera>,
) {
    for (canvas, tokens) in &canvases {
        let Some(painting) = canvas.painting() else {
            continue;
        };
        let Ok(mut camera) = cameras.get_mut(painting.camera) else {
            continue;
        };
        let background = theme.color(&tokens.bg);
        // Only a real change: a camera that changes renders its target anew.
        if !matches!(camera.clear_color, ClearColorConfig::Custom(colour) if colour == background) {
            camera.clear_color = ClearColorConfig::Custom(background);
        }
    }
}

/// Takes a painting's parts with it when its canvas goes, and hands its layer back.
fn tear_down_painting(
    removed: On<Remove<Canvas>>,
    canvases: Query<&Canvas>,
    mut sketches: ResMut<Assets<GizmoAsset>>,
    mut layers: ResMut<CanvasLayers>,
    mut commands: Commands,
) {
    let Some(painting) = canvases.get(removed.entity).ok().and_then(Canvas::painting) else {
        return;
    };
    for entity in [painting.camera, painting.stage, painting.sketch_entity] {
        commands.entity(entity).try_despawn();
    }
    sketches.remove(&painting.sketch);
    layers.give_back(painting.layer);
}

/// Draws on paintings: like Bevy's `Gizmos`, but on one canvas, in its
/// coordinates, and wiped every frame.
///
/// ```
/// # use bevy::prelude::*;
/// # use touchy::canvas::{Canvas, CanvasSketch};
/// fn draw_a_diagonal(mut sketch: CanvasSketch, canvases: Query<Entity, With<Canvas>>) {
///     for canvas in &canvases {
///         let size = sketch.picture_size(canvas);
///         if let Some(mut lines) = sketch.on(canvas) {
///             lines.line_2d(Vec2::ZERO, size, Color::WHITE);
///         }
///     }
/// }
/// ```
#[derive(SystemParam)]
pub struct CanvasSketch<'w, 's> {
    canvases: Query<'w, 's, &'static Canvas>,
    sketches: ResMut<'w, Assets<GizmoAsset>>,
}

impl CanvasSketch<'_, '_> {
    /// The lines drawn on `canvas` this frame, to draw more; `None` if it
    /// isn't a painting, or isn't set up yet.
    pub fn on(&mut self, canvas: Entity) -> Option<AssetMut<'_, GizmoAsset>> {
        let painting = self.canvases.get(canvas).ok()?.painting()?;
        self.sketches.get_mut(&painting.sketch)
    }

    /// How big `canvas`'s picture is, in its own coordinates.
    pub fn picture_size(&self, canvas: Entity) -> Vec2 {
        self.canvases
            .get(canvas)
            .map_or(Vec2::ZERO, Canvas::picture_size)
    }
}

/// Wipes every painting's sketch, ready for this frame's drawing.
fn wipe_sketches(canvases: Query<&Canvas>, sketches: Option<ResMut<Assets<GizmoAsset>>>) {
    let Some(mut sketches) = sketches else {
        return;
    };
    for painting in canvases.iter().filter_map(Canvas::painting) {
        if let Some(mut sketch) = sketches.get_mut(&painting.sketch) {
            sketch.clear();
        }
    }
}

/// Fits each image canvas's picture to its frame, by its [`CanvasFit`], once
/// its image has loaded; a canvas without a size takes its image's.
fn fit_images(
    mut canvases: Query<(&mut Canvas, &CanvasImage, &CanvasFit, &Node, &ComputedNode)>,
    mut pictures: Query<(&mut Node, &mut ImageNode), Without<Canvas>>,
    images: Option<Res<Assets<Image>>>,
) {
    let Some(images) = images else {
        return;
    };
    for (mut canvas, image, &fit, frame, computed) in &mut canvases {
        let Ok((mut node, mut picture)) = pictures.get_mut(canvas.picture) else {
            continue;
        };
        if picture.image != image.0 {
            picture.image = image.0.clone();
        }
        let Some(pixels) = images.get(&image.0).map(|image| image.size_f32()) else {
            continue;
        };
        if canvas.picture_size != pixels {
            canvas.picture_size = pixels;
        }
        let natural = frame.width == Val::Auto && frame.height == Val::Auto;
        let area = computed.size() * computed.inverse_scale_factor;
        let drawn = if natural {
            pixels
        } else {
            fit.fit(pixels, area)
        };
        let size = (px(drawn.x), px(drawn.y));
        if (node.width, node.height) != size {
            (node.width, node.height) = size;
        }
    }
}

/// Keeps each painting's and camera view's size up to date, and a painting's
/// camera and lines at one unit and one line width per logical pixel.
fn follow_canvas_sizes(
    mut canvases: Query<&mut Canvas>,
    pictures: Query<&ComputedNode>,
    mut cameras: Query<&mut Projection>,
    mut gizmos: Query<&mut Gizmo>,
) {
    for mut canvas in &mut canvases {
        if matches!(canvas.shows, Shows::Image) {
            continue;
        }
        let Ok(computed) = pictures.get(canvas.picture) else {
            continue;
        };
        let scale = computed.inverse_scale_factor;
        let size = computed.size() * scale;
        if canvas.picture_size != size {
            canvas.picture_size = size;
        }
        let Some(painting) = canvas.painting().cloned() else {
            continue;
        };
        if let Ok(mut projection) = cameras.get_mut(painting.camera)
            && let Projection::Orthographic(ortho) = &mut *projection
            && ortho.scale != scale
        {
            ortho.scale = scale;
        }
        if let Ok(mut gizmo) = gizmos.get_mut(painting.sketch_entity) {
            let width = painting.line_width / scale.max(f32::EPSILON);
            if gizmo.line_config.width != width {
                gizmo.line_config.width = width;
            }
        }
    }
}

/// The mouse's place on a canvas whose picture is `size` big, from its
/// position relative to the picture's middle as a fraction of it, +y down.
fn place_on(shows: &Shows, normalized: Vec2, size: Vec2) -> Vec2 {
    let from_top_left = normalized + Vec2::splat(0.5);
    match shows {
        Shows::Image => from_top_left * size,
        _ => Vec2::new(from_top_left.x, 1.0 - from_top_left.y) * size,
    }
}

/// Where a picture is on screen, to find the mouse on it.
type PictureRect = (
    &'static ComputedNode,
    &'static UiGlobalTransform,
    &'static ComputedUiRenderTargetInfo,
);

/// Finds where the mouse is on each canvas, where nothing covers it. Works
/// from the pointer Bevy's picking keeps, so it's right on any render target.
fn point_into_canvases(
    mut canvases: Query<(&Canvas, &mut CanvasPointer)>,
    pictures: Query<PictureRect>,
    pointers: Query<(&PointerId, &PointerLocation)>,
    hover_map: Res<HoverMap>,
) {
    let hovered = hover_map.get(&PointerId::Mouse);
    let mouse = pointers
        .iter()
        .find(|(id, _)| **id == PointerId::Mouse)
        .and_then(|(_, location)| location.location.as_ref())
        .map(|location| location.position);
    for (canvas, mut pointer) in &mut canvases {
        let over = hovered.is_some_and(|hits| hits.contains_key(&canvas.picture));
        let place = mouse
            .filter(|_| over)
            .zip(pictures.get(canvas.picture).ok())
            .map(|(at, (node, transform, target))| {
                // Picking's position is logical; the layout's is physical.
                let offset = at * target.scale_factor() - transform.translation;
                let normalized = offset / node.size().max(Vec2::ONE);
                place_on(&canvas.shows, normalized, canvas.picture_size)
            });
        pointer.set_if_neq(CanvasPointer(place));
    }
}

/// Canvases: images, paintings and camera views, and the pointer on them.
pub struct CanvasPlugin;

impl Plugin for CanvasPlugin {
    fn build(&self, app: &mut App) {
        app.ensure_plugin(ThemePlugin)
            .ensure_plugin(HierarchyPropagatePlugin::<RenderLayers>::new(PostUpdate))
            .init_resource::<CanvasLayers>()
            .configure_sets(
                PostUpdate,
                PropagateSet::<RenderLayers>::default().before(VisibilitySystems::CheckVisibility),
            )
            .add_observer(set_up_canvas)
            .add_observer(style_canvas)
            .add_observer(tear_down_painting)
            .add_systems(First, wipe_sketches)
            .add_systems(PreUpdate, point_into_canvases.after(PickingSystems::Hover))
            .add_systems(
                PostUpdate,
                (
                    fit_images.before(UiSystems::Prepare),
                    follow_canvas_sizes.after(UiSystems::Layout),
                    clear_paintings_to_their_background,
                ),
            );
        // Bevy's gizmo plugin registers sketches too, but may come later; an
        // app with no assets at all can still have the rest of touchy.
        if app.is_plugin_added::<AssetPlugin>() {
            app.init_asset::<GizmoAsset>();
        }
        CanvasTokens::with_prefix(CANVAS_STYLE)
            .register_theme_defaults(&mut app.world_mut().resource_mut::<Theme>());
    }
}

#[cfg(test)]
mod tests {
    use bevy::ecs::system::RunSystemOnce;
    use bevy::picking::pointer::PointerMap;

    use super::*;

    #[test]
    fn an_image_fits_by_stretching_containing_or_covering() {
        let square = Vec2::splat(32.0);
        let wide = Vec2::new(96.0, 48.0);
        assert_eq!(CanvasFit::Stretch.fit(square, wide), wide);
        assert_eq!(CanvasFit::Contain.fit(square, wide), Vec2::splat(48.0));
        assert_eq!(CanvasFit::Cover.fit(square, wide), Vec2::splat(96.0));
        assert_eq!(
            CanvasFit::Contain.fit(Vec2::ZERO, wide),
            wide,
            "an image with no size just fills it"
        );
    }

    #[test]
    fn the_pointer_is_in_the_space_of_what_a_canvas_shows() {
        let size = Vec2::new(200.0, 100.0);
        let top_left = Vec2::splat(-0.5);
        assert_eq!(
            place_on(&Shows::Image, top_left, size),
            Vec2::ZERO,
            "an image's pixels run from its top left"
        );
        assert_eq!(
            place_on(&Shows::PendingPainting { line_width: 1.0 }, top_left, size),
            Vec2::new(0.0, 100.0),
            "a painting's run up from its bottom left"
        );
        assert_eq!(
            place_on(
                &Shows::PendingPainting { line_width: 1.0 },
                Vec2::new(0.25, 0.0),
                size
            ),
            Vec2::new(150.0, 50.0)
        );
    }

    #[test]
    fn layers_are_handed_out_once_and_back_again() {
        let mut layers = CanvasLayers::starting_at(40);
        assert_eq!((layers.take(), layers.take()), (40, 41));
        layers.give_back(40);
        assert_eq!(layers.take(), 40, "a layer handed back is used again");
        assert_eq!(layers.take(), 42);
    }

    fn canvas_app(config: CanvasConfig) -> (App, Entity) {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Image>()
            .init_resource::<HoverMap>()
            .init_resource::<PointerMap>()
            .add_plugins(CanvasPlugin);
        let mut canvas = None;
        app.world_mut()
            .commands()
            .spawn(Node::default())
            .with_children(|parent| canvas = Some(spawn_canvas(parent, config).id()));
        app.update();
        (app, canvas.unwrap())
    }

    #[test]
    fn a_painting_gets_a_camera_layer_and_stage_of_its_own_and_gives_them_back() {
        let (mut app, first) = canvas_app(CanvasConfig::painted());
        let canvas = app.world().get::<Canvas>(first).unwrap().clone();
        let painting = canvas.painting().unwrap().clone();
        assert_eq!(painting.layer, FIRST_LAYER);
        assert_eq!(
            app.world().get::<RenderLayers>(painting.camera),
            Some(&RenderLayers::layer(FIRST_LAYER)),
            "its camera sees only its layer"
        );
        assert_eq!(
            app.world()
                .get::<ViewportNode>(canvas.picture)
                .and_then(|viewport| viewport.camera),
            Some(painting.camera),
            "and its picture shows that camera"
        );
        assert_eq!(canvas.stage(), Some(painting.stage));
        assert!(
            app.world()
                .get::<Propagate<RenderLayers>>(painting.stage)
                .is_some(),
            "what's put on the stage goes on the layer"
        );

        app.world_mut().entity_mut(first).despawn();
        app.update();
        for part in [painting.camera, painting.stage, painting.sketch_entity] {
            assert!(app.world().get_entity(part).is_err(), "taken with it");
        }
        let world = app.world_mut();
        let mut again = None;
        world
            .commands()
            .spawn(Node::default())
            .with_children(|parent| {
                again = Some(spawn_canvas(parent, CanvasConfig::painted()).id())
            });
        app.update();
        let reused = app.world().get::<Canvas>(again.unwrap()).unwrap();
        assert_eq!(
            reused.painting().unwrap().layer,
            FIRST_LAYER,
            "its layer is handed on"
        );
    }

    #[test]
    fn new_tokens_restyle_a_canvas() {
        let (mut app, canvas) = canvas_app(CanvasConfig::painted());
        let background = |app: &App| app.world().get::<ThemeSkin>(canvas).unwrap().0.clone();
        assert_eq!(background(&app), CanvasTokens::with_prefix(CANVAS_STYLE).bg);

        let tokens = CanvasTokens::with_prefix("game.minimap");
        app.world_mut().entity_mut(canvas).insert(tokens.clone());
        assert_eq!(background(&app), tokens.bg);
    }

    #[test]
    fn a_sketch_is_wiped_every_frame() {
        let (mut app, canvas) = canvas_app(CanvasConfig::painted());
        app.world_mut()
            .run_system_once(move |mut sketch: CanvasSketch| {
                sketch
                    .on(canvas)
                    .unwrap()
                    .line_2d(Vec2::ZERO, Vec2::ONE, Color::WHITE);
            })
            .unwrap();
        let lines = |app: &App| {
            let handle = &app
                .world()
                .get::<Canvas>(canvas)
                .unwrap()
                .painting()
                .unwrap()
                .sketch;
            app.world()
                .resource::<Assets<GizmoAsset>>()
                .get(handle)
                .unwrap()
                .buffer()
                .list_positions
                .len()
        };
        assert_eq!(lines(&app), 2);
        app.update();
        assert_eq!(lines(&app), 0);
    }

    #[test]
    fn a_camera_view_points_the_camera_at_its_canvas() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Image>()
            .init_resource::<HoverMap>()
            .init_resource::<PointerMap>()
            .add_plugins(CanvasPlugin);
        let camera = app.world_mut().spawn(Camera2d).id();
        let mut canvas = None;
        app.world_mut()
            .commands()
            .spawn(Node::default())
            .with_children(|parent| {
                canvas = Some(spawn_canvas(parent, CanvasConfig::camera(camera)).id())
            });
        app.update();
        assert!(
            app.world()
                .get::<RenderTarget>(camera)
                .is_some_and(|target| target.as_image().is_some()),
            "drawing offscreen now"
        );
        let canvas = app.world().get::<Canvas>(canvas.unwrap()).unwrap();
        assert_eq!(canvas.stage(), None, "not a painting");
        assert_eq!(
            app.world()
                .get::<ViewportNode>(canvas.picture)
                .and_then(|viewport| viewport.camera),
            Some(camera)
        );
    }

    /// A canvas with a size of its own keeps it in a crowded column; one without fills.
    #[test]
    fn a_sized_canvas_keeps_its_size() {
        let (app, canvas) = canvas_app(CanvasConfig::painted().size(640.0, 480.0));
        let node = app.world().get::<Node>(canvas).unwrap();
        assert_eq!((node.height, node.flex_shrink), (px(480.0), 0.0));
        let (app, canvas) = canvas_app(CanvasConfig::painted());
        assert_eq!(app.world().get::<Node>(canvas).unwrap().flex_shrink, 1.0);
    }

    /// A painting clears to its canvas's background, so translucent paint is blended once.
    #[test]
    fn a_painting_clears_to_its_background() {
        let (mut app, canvas) = canvas_app(CanvasConfig::painted().size(64.0, 64.0));
        let bg = app.world().get::<CanvasTokens>(canvas).unwrap().bg.clone();
        app.world_mut()
            .resource_mut::<Theme>()
            .set_color(bg, Color::WHITE);
        app.update();
        let camera = app
            .world()
            .get::<Canvas>(canvas)
            .unwrap()
            .painting()
            .unwrap()
            .camera;
        let clear = app.world().get::<Camera>(camera).unwrap().clear_color;
        assert!(matches!(clear, ClearColorConfig::Custom(colour) if colour == Color::WHITE));
    }
}
