//! How values reach components: binding components, and the systems that
//! apply them when they're inserted and whenever the theme changes.

use bevy::ecs::query::{IterQueryData, QueryItem};
use bevy::image::TRANSPARENT_IMAGE_HANDLE;
use bevy::prelude::*;
use bevy::text::{Font, FontSize, FontSource};
use bevy::ui::{BackgroundGradient, BorderGradient, UiSystems};

use super::{SkinFrame, SkinGradient, Theme, ThemeToken};

/// A component that sets visual properties from the theme.
///
/// Implement this to make a new property themeable, then register it with
/// [`ThemeAppExt::register_theme_binding`].
pub trait ThemeBinding: Component {
    /// What the binding writes, as query data: `&'static mut TextColor` for one
    /// component, or a tuple of them for several. Require them on the binding
    /// too, so it always finds them.
    type Target: IterQueryData;
    /// Applies the binding to its target, using the theme.
    fn apply(&self, theme: &Theme, target: QueryItem<'_, '_, Self::Target>);
}

/// A node's skin from a token: its flat colour or, if the theme gives the
/// token a [`Skin`](super::Skin), that skin's layers. Either way, it's how the
/// token says the node looks.
///
/// The binding owns the components a skin draws with: the node's
/// [`BackgroundColor`], [`ImageNode`], [`BackgroundGradient`],
/// [`BorderGradient`], [`BoxShadow`] and [`Outline`], and sets them all from
/// the theme. Until a skin uses them they draw nothing: the image is
/// `TRANSPARENT_IMAGE_HANDLE`, which Bevy neither draws nor measures, and the
/// rest are empty. To draw any of them yourself, put them on a child node, or
/// give the token a skin.
#[derive(Component, Clone, PartialEq, Debug)]
#[component(immutable)]
#[require(
    BackgroundColor,
    ImageNode,
    BackgroundGradient,
    BorderGradient,
    BoxShadow,
    Outline = NO_OUTLINE
)]
pub struct ThemeSkin(pub ThemeToken);

/// Bevy's default outline is a white line; an unskinned node has none.
const NO_OUTLINE: Outline = Outline::new(Val::ZERO, Val::ZERO, Color::NONE);

impl ThemeBinding for ThemeSkin {
    type Target = (
        &'static mut BackgroundColor,
        &'static mut ImageNode,
        &'static mut BackgroundGradient,
        &'static mut BorderGradient,
        &'static mut BoxShadow,
        &'static mut Outline,
    );

    fn apply(&self, theme: &Theme, target: QueryItem<'_, '_, Self::Target>) {
        let (mut fill, mut image, mut gradients, mut border, mut shadows, mut outline) = target;
        let Some(painted) = theme.skin(&self.0) else {
            fill.set_if_neq(BackgroundColor(theme.color(&self.0)));
            hide(&mut image);
            gradients.set_if_neq(default());
            border.set_if_neq(default());
            shadows.set_if_neq(default());
            outline.set_if_neq(NO_OUTLINE);
            return;
        };
        let skin = painted.skin;
        let color = |color: &_| painted.color(color);
        let filled = skin.fill.as_ref().map_or(Color::NONE, color);
        fill.set_if_neq(BackgroundColor(filled));
        match (&skin.picture, painted.tint()) {
            (Some(picture), Some(tint)) => {
                // A sprite sheet's frame is cut in `show_skin_frames`, once its picture is loaded.
                let rect = picture.frames.and(image.rect);
                // Images draw after borders, so a picture over the border box hides it.
                let shown = (
                    &image.image,
                    &image.image_mode,
                    image.color,
                    image.visual_box,
                );
                let flips = (image.flip_x, image.flip_y, image.rect);
                // Only a real change, so an unchanged node isn't sent to the renderer again.
                if shown != (&picture.image, &picture.mode, tint, picture.area)
                    || flips != (picture.flip_x, picture.flip_y, rect)
                {
                    image.image = picture.image.clone();
                    image.image_mode = picture.mode.clone();
                    image.color = tint;
                    image.visual_box = picture.area;
                    (image.flip_x, image.flip_y, image.rect) =
                        (picture.flip_x, picture.flip_y, rect);
                }
            }
            _ => hide(&mut image),
        }
        let paint = |list: &[SkinGradient]| list.iter().map(|g| g.paint(color)).collect();
        gradients.set_if_neq(BackgroundGradient(paint(&skin.gradients)));
        border.set_if_neq(BorderGradient(paint(&skin.border)));
        let painted_shadows = skin.shadows.iter().map(|s| s.paint(color)).collect();
        shadows.set_if_neq(BoxShadow(painted_shadows));
        let painted_outline = skin.outline.as_ref().map_or(NO_OUTLINE, |o| o.paint(color));
        outline.set_if_neq(painted_outline);
    }
}

/// Stops drawing a node's picture.
fn hide(image: &mut Mut<ImageNode>) {
    if image.image != TRANSPARENT_IMAGE_HANDLE {
        image.image = TRANSPARENT_IMAGE_HANDLE;
    }
}

/// Border colors from tokens, one per side so bevelled borders are possible.
///
/// A node whose skin has a picture over its border box draws no border.
#[derive(Component, Clone, PartialEq, Debug)]
#[component(immutable)]
#[require(BorderColor)]
pub struct ThemeBorder {
    /// The top edge's colour token.
    pub top: ThemeToken,
    /// The right edge's colour token.
    pub right: ThemeToken,
    /// The bottom edge's colour token.
    pub bottom: ThemeToken,
    /// The left edge's colour token.
    pub left: ThemeToken,
}

impl ThemeBorder {
    /// All four edges from the same token.
    pub fn all(token: ThemeToken) -> Self {
        Self {
            top: token.clone(),
            right: token.clone(),
            bottom: token.clone(),
            left: token,
        }
    }

    /// `light` on the top and left, `dark` on the bottom and right: a raised look.
    /// Swap the arguments for a pressed-in look.
    pub fn bevel(light: ThemeToken, dark: ThemeToken) -> Self {
        Self {
            top: light.clone(),
            left: light,
            bottom: dark.clone(),
            right: dark,
        }
    }
}

impl ThemeBinding for ThemeBorder {
    type Target = (&'static mut BorderColor, Option<&'static ThemeSkin>);

    fn apply(&self, theme: &Theme, (mut target, background): QueryItem<'_, '_, Self::Target>) {
        let covered = background
            .and_then(|background| theme.skin(&background.0))
            .and_then(|painted| painted.skin.picture.as_ref())
            .is_some_and(|picture| picture.area == VisualBox::BorderBox);
        if covered {
            target.set_if_neq(BorderColor::all(Color::NONE));
            return;
        }
        target.set_if_neq(BorderColor {
            top: theme.color(&self.top),
            right: theme.color(&self.right),
            bottom: theme.color(&self.bottom),
            left: theme.color(&self.left),
        });
    }
}

/// Text color from a token (SRGBA).
#[derive(Component, Clone, PartialEq, Debug)]
#[component(immutable)]
#[require(TextColor)]
pub struct ThemeTextColor(pub ThemeToken);

impl ThemeBinding for ThemeTextColor {
    type Target = &'static mut TextColor;

    fn apply(&self, theme: &Theme, mut target: Mut<TextColor>) {
        target.set_if_neq(TextColor(theme.color(&self.0)));
    }
}

/// Font size from a token (in pixels).
#[derive(Component, Clone, PartialEq, Debug)]
#[component(immutable)]
#[require(TextFont)]
pub struct ThemeFontSize(pub ThemeToken);

impl ThemeBinding for ThemeFontSize {
    type Target = &'static mut TextFont;

    fn apply(&self, theme: &Theme, mut target: Mut<TextFont>) {
        let size = FontSize::Px(theme.size(&self.0));
        // Only a real change, since a new size makes the text lay out again.
        if target.font_size != size {
            target.font_size = size;
        }
    }
}

/// Font from a token.
/// Bevy's own font shows until the token's file has loaded.
#[derive(Component, Clone, PartialEq, Debug)]
#[component(immutable)]
#[require(TextFont)]
pub struct ThemeFont(pub ThemeToken);

impl ThemeBinding for ThemeFont {
    type Target = &'static mut TextFont;

    fn apply(&self, theme: &Theme, mut target: Mut<TextFont>) {
        let font = FontSource::Handle(theme.font(&self.0));
        // Only a real change, since a new font makes the text lay out again.
        if target.font != font {
            target.font = font;
        }
    }
}

/// Corner radius from a size token. A skin isn't rounded by it: Bevy draws
/// images square, so a rounded skin has its corners in the picture.
#[derive(Component, Clone, PartialEq, Debug)]
#[component(immutable)]
#[require(Node)]
pub struct ThemeCornerRadius(pub ThemeToken);

impl ThemeBinding for ThemeCornerRadius {
    type Target = &'static mut Node;

    fn apply(&self, theme: &Theme, mut target: Mut<Node>) {
        let radius = BorderRadius::all(px(theme.size(&self.0)));
        // Only a real change, since any write to a node lays it out again.
        if target.border_radius != radius {
            target.border_radius = radius;
        }
    }
}

/// Registers bindings, including ones defined outside touchy.
pub trait ThemeAppExt {
    /// Registers a binding type, so it applies when inserted and whenever the theme changes.
    fn register_theme_binding<B: ThemeBinding>(&mut self) -> &mut Self;
}

impl ThemeAppExt for App {
    fn register_theme_binding<B: ThemeBinding>(&mut self) -> &mut Self {
        self.add_observer(apply_on_insert::<B>).add_systems(
            PostUpdate,
            apply_on_theme_change::<B>
                .run_if(resource_changed::<Theme>)
                .before(UiSystems::Prepare),
        )
    }
}

/// A binding was added or swapped (bindings are immutable, so a state change
/// re-inserts them): apply it straight away.
fn apply_on_insert<B: ThemeBinding>(
    insert: On<Insert<B>>,
    theme: Res<Theme>,
    mut bound: Query<(&B, B::Target)>,
) {
    if let Ok((binding, target)) = bound.get_mut(insert.entity) {
        binding.apply(&theme, target);
    }
}

/// A swapped background can gain or lose a skin that covers the
/// border, so the border is rechecked with it.
pub(super) fn apply_border_on_background_insert(
    insert: On<Insert<ThemeSkin>>,
    theme: Res<Theme>,
    mut bound: Query<(&ThemeBorder, <ThemeBorder as ThemeBinding>::Target)>,
) {
    if let Ok((border, target)) = bound.get_mut(insert.entity) {
        border.apply(&theme, target);
    }
}

/// Shows each sprite-sheet skin's frame: by time if it loops, else as the
/// node's [`SkinFrame`] picks. Real time, so a paused game's menus still move.
pub(super) fn show_skin_frames(
    theme: Res<Theme>,
    time: Res<Time<Real>>,
    images: Option<Res<Assets<Image>>>,
    mut nodes: Query<(&ThemeSkin, &mut ImageNode, Option<&SkinFrame>)>,
) {
    let Some(images) = images else {
        return;
    };
    for (binding, mut image, frame) in &mut nodes {
        if image.image == TRANSPARENT_IMAGE_HANDLE {
            continue;
        }
        let Some(skinned) = theme.skin(&binding.0) else {
            continue;
        };
        let Some(picture) = &skinned.skin.picture else {
            continue;
        };
        let (Some(frames), Some(loaded)) = (picture.frames, images.get(&picture.image)) else {
            continue;
        };
        let index = frames.index(frame.map_or(0.0, |frame| frame.0), time.elapsed_secs());
        let rect = Some(frames.rect(loaded.size(), index));
        if image.rect != rect {
            image.rect = rect;
        }
    }
}

/// Keeps the theme's record of loaded fonts current.
pub(super) fn track_loaded_fonts(fonts: Res<Assets<Font>>, mut theme: ResMut<Theme>) {
    if !fonts.is_changed() && !theme.is_changed() {
        return;
    }
    let stale: Vec<_> = theme
        .named_fonts()
        .filter(|&font| fonts.contains(font) != theme.is_font_loaded(font))
        .collect();
    for font in stale {
        let loaded = fonts.contains(font);
        theme.set_font_loaded(font, loaded);
    }
}

/// The theme changed: re-apply every `B`, each binding type having its own copy of this system.
pub(super) fn apply_on_theme_change<B: ThemeBinding>(
    theme: Res<Theme>,
    mut bound: Query<(&B, B::Target)>,
) {
    for (binding, target) in bound.iter_mut() {
        binding.apply(&theme, target);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{Shade, Skin, ThemePlugin, WidgetState};

    const BG: ThemeToken = ThemeToken::new_static("test.bg");
    const BG_HOVER: ThemeToken = ThemeToken::new_static("test.bg.hover");
    const RED: Color = Color::srgb(1.0, 0.0, 0.0);
    const BLUE: Color = Color::srgb(0.0, 0.0, 1.0);
    const GREEN: Color = Color::srgb(0.0, 1.0, 0.0);

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, ThemePlugin));
        app.world_mut()
            .resource_mut::<Theme>()
            .set_color(BG, RED)
            .set_color(BG_HOVER, BLUE);
        app
    }

    fn background(app: &App, entity: Entity) -> Color {
        app.world().get::<BackgroundColor>(entity).unwrap().0
    }

    /// Widgets show state by swapping tokens; the new color must apply at once.
    #[test]
    fn spawning_and_swapping_a_binding_applies_it_immediately() {
        let mut app = app();
        let entity = app.world_mut().spawn(ThemeSkin(BG)).id();
        assert_eq!(background(&app, entity), RED);

        app.world_mut()
            .entity_mut(entity)
            .insert(ThemeSkin(BG_HOVER));
        assert_eq!(background(&app, entity), BLUE);
    }

    /// Text and node sizes are written only when they change: any write to
    /// them lays the UI out again, and the theme changes for any token.
    #[test]
    fn an_unrelated_theme_change_leaves_layout_alone() {
        const SIZE: ThemeToken = ThemeToken::new_static("test.size");
        let mut app = app();
        app.world_mut().resource_mut::<Theme>().set_size(SIZE, 4.0);
        let entity = app
            .world_mut()
            .spawn((
                Text::new("score"),
                ThemeFontSize(SIZE),
                ThemeCornerRadius(SIZE),
            ))
            .id();
        app.update();
        let written = |app: &App| {
            let entity = app.world().entity(entity);
            let node = entity.get_ref::<Node>().unwrap().last_changed();
            let font = entity.get_ref::<TextFont>().unwrap().last_changed();
            (node, font)
        };
        let before = written(&app);
        app.world_mut().resource_mut::<Theme>().set_color(BG, GREEN);
        app.update();
        assert_eq!(written(&app), before);
    }

    /// A skinned node's components are written only when they change, so an
    /// unchanged node isn't sent to the renderer again.
    #[test]
    fn an_unrelated_theme_change_leaves_a_skin_alone() {
        use crate::theme::SkinShadow;

        let mut app = app();
        let skin = Skin::new(Handle::default()).shadow(SkinShadow::new(Color::BLACK).blur(4.0));
        app.world_mut().resource_mut::<Theme>().set_skin(BG, skin);
        let entity = app.world_mut().spawn(ThemeSkin(BG)).id();
        app.update();
        let written = |app: &App| {
            let entity = app.world().entity(entity);
            let image = entity.get_ref::<ImageNode>().unwrap().last_changed();
            let shadow = entity.get_ref::<BoxShadow>().unwrap().last_changed();
            (image, shadow)
        };
        let before = written(&app);
        app.world_mut()
            .resource_mut::<Theme>()
            .set_color(BG_HOVER, GREEN);
        app.update();
        assert_eq!(written(&app), before);
    }

    #[test]
    fn changing_the_theme_restyles_existing_entities() {
        let mut app = app();
        let entity = app.world_mut().spawn(ThemeSkin(BG)).id();
        app.update();

        app.world_mut().resource_mut::<Theme>().set_color(BG, GREEN);
        app.update();
        assert_eq!(background(&app, entity), GREEN);
    }

    /// Derived colours resolve at apply time, so changing a base restyles
    /// everything that derives from it with no extra bookkeeping.
    #[test]
    fn changing_a_base_restyles_entities_bound_to_derived_tokens() {
        const DERIVED: ThemeToken = ThemeToken::new_static("test.derived");
        let mut app = app();
        app.world_mut()
            .resource_mut::<Theme>()
            .default_derived(DERIVED, BG, Shade::Darker(0.1));
        let entity = app.world_mut().spawn(ThemeSkin(DERIVED)).id();
        assert_eq!(background(&app, entity), Shade::Darker(0.1).apply(RED));

        app.world_mut().resource_mut::<Theme>().set_color(BG, GREEN);
        app.update();
        assert_eq!(background(&app, entity), Shade::Darker(0.1).apply(GREEN));
    }

    fn image(app: &App, entity: Entity) -> &ImageNode {
        app.world().get::<ImageNode>(entity).unwrap()
    }

    /// A skin replaces the flat fill, tinted by the token's colour, and
    /// clearing it brings the fill back.
    #[test]
    fn skins_replace_the_fill_until_cleared() {
        let frame: Handle<Image> = Handle::default();
        let mut app = app();
        let entity = app.world_mut().spawn(ThemeSkin(BG)).id();
        assert_eq!(image(&app, entity).image, TRANSPARENT_IMAGE_HANDLE);

        app.world_mut()
            .resource_mut::<Theme>()
            .set_skin(BG, Skin::new(frame.clone()).sliced(4.0));
        app.update();
        assert_eq!(background(&app, entity), Color::NONE);
        let skinned = image(&app, entity);
        assert_eq!(skinned.image, frame);
        assert_eq!(skinned.color, RED);
        assert!(matches!(skinned.image_mode, NodeImageMode::Sliced(_)));
        assert_eq!(skinned.visual_box, VisualBox::BorderBox);

        app.world_mut().resource_mut::<Theme>().clear_skin(&BG);
        app.update();
        assert_eq!(background(&app, entity), RED);
        assert_eq!(image(&app, entity).image, TRANSPARENT_IMAGE_HANDLE);
    }

    /// Each layer lands on the component Bevy draws it with, coloured for the
    /// node's state, and clearing the skin takes every one away.
    #[test]
    fn layers_draw_on_the_node_and_follow_its_state() {
        use crate::theme::{SkinColor, SkinGradient, SkinOutline, SkinShadow};
        use bevy::ui::LinearGradient;

        let mut app = app();
        let glow = SkinShadow::new(SkinColor::OWN).blur(8.0);
        let skin = Skin::default()
            .gradient(SkinGradient::linear(
                LinearGradient::TO_BOTTOM,
                [Color::WHITE, Color::BLACK],
            ))
            .border(SkinGradient::flat(GREEN))
            .shadow(glow)
            .outline(SkinOutline::new(SkinColor::OWN, 2.0));
        app.world_mut().resource_mut::<Theme>().set_skin(BG, skin);
        let entity = app.world_mut().spawn(ThemeSkin(BG)).id();
        let world = app.world();
        assert_eq!(
            background(&app, entity),
            RED,
            "layers keep the token's colour"
        );
        assert_eq!(world.get::<BackgroundGradient>(entity).unwrap().0.len(), 1);
        assert_eq!(world.get::<BorderGradient>(entity).unwrap().0.len(), 1);
        assert_eq!(world.get::<BoxShadow>(entity).unwrap().0[0].color, RED);
        assert_eq!(
            world.get::<BoxShadow>(entity).unwrap().0[0].blur_radius,
            px(8.0)
        );
        assert_eq!(world.get::<Outline>(entity).unwrap().width, px(2.0));
        assert_eq!(image(&app, entity).image, TRANSPARENT_IMAGE_HANDLE);

        // A hovered state borrows the skin; its own colour now carries the state.
        let hover = BG.with_state(WidgetState::Hover);
        let hovered = app.world().resource::<Theme>().color(&hover);
        app.world_mut().entity_mut(entity).insert(ThemeSkin(hover));
        assert_eq!(
            app.world().get::<BoxShadow>(entity).unwrap().0[0].color,
            hovered
        );
        assert_ne!(hovered, RED);

        app.world_mut().resource_mut::<Theme>().clear_skin(&BG);
        app.world_mut().entity_mut(entity).insert(ThemeSkin(BG));
        let world = app.world();
        assert!(
            world
                .get::<BackgroundGradient>(entity)
                .unwrap()
                .0
                .is_empty()
        );
        assert!(world.get::<BorderGradient>(entity).unwrap().0.is_empty());
        assert!(world.get::<BoxShadow>(entity).unwrap().0.is_empty());
        assert_eq!(*world.get::<Outline>(entity).unwrap(), NO_OUTLINE);
    }

    /// An unskinned node draws no outline: Bevy's default is a white line.
    #[test]
    fn an_unskinned_node_has_no_outline() {
        let mut app = app();
        let entity = app.world_mut().spawn(ThemeSkin(BG)).id();
        assert_eq!(*app.world().get::<Outline>(entity).unwrap(), NO_OUTLINE);
    }

    /// A sprite-sheet skin shows the frame its node's `SkinFrame` picks, cut
    /// from the picture once it's loaded.
    #[test]
    fn a_sprite_sheet_shows_the_frame_its_node_picks() {
        use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

        let mut app = app();
        app.add_plugins(bevy::asset::AssetPlugin::default())
            .init_asset::<Image>();
        let size = Extent3d {
            width: 50,
            height: 10,
            depth_or_array_layers: 1,
        };
        let format = TextureFormat::Rgba8UnormSrgb;
        let picture = Image::new_fill(size, TextureDimension::D2, &[255; 4], format, default());
        let sheet = app.world_mut().resource_mut::<Assets<Image>>().add(picture);
        app.world_mut()
            .resource_mut::<Theme>()
            .set_skin(BG, Skin::new(sheet).frames(5, 1).flip_x());
        let entity = app.world_mut().spawn((ThemeSkin(BG), SkinFrame(0.75))).id();
        app.update();
        // Five frames 10 pixels wide; three quarters of the way is the fourth.
        assert_eq!(
            image(&app, entity).rect,
            Some(Rect::new(30.0, 0.0, 40.0, 10.0))
        );
        assert!(image(&app, entity).flip_x);

        app.world_mut().entity_mut(entity).insert(SkinFrame(0.0));
        app.update();
        assert_eq!(
            image(&app, entity).rect,
            Some(Rect::new(0.0, 0.0, 10.0, 10.0))
        );
    }

    /// Widgets swap to stateful tokens; the resting skin follows them, shaded.
    #[test]
    fn swapping_to_a_state_keeps_the_skin() {
        let mut app = app();
        app.world_mut()
            .resource_mut::<Theme>()
            .set_skin(BG, Skin::new(Handle::default()));
        let entity = app.world_mut().spawn(ThemeSkin(BG)).id();
        let pressed = BG.with_state(WidgetState::Pressed);
        app.world_mut()
            .entity_mut(entity)
            .insert(ThemeSkin(pressed.clone()));

        let expected = app.world().resource::<Theme>().color(&pressed);
        assert_eq!(image(&app, entity).color, expected);
        assert_eq!(background(&app, entity), Color::NONE);
    }

    #[test]
    fn skins_inside_the_border_leave_it_showing() {
        let mut app = app();
        app.world_mut()
            .resource_mut::<Theme>()
            .set_skin(BG, Skin::new(Handle::default()).inside_border());
        let entity = app.world_mut().spawn(ThemeSkin(BG)).id();
        assert_eq!(image(&app, entity).visual_box, VisualBox::PaddingBox);
    }

    const EDGE: ThemeToken = ThemeToken::new_static("test.edge");

    fn border(app: &App, entity: Entity) -> BorderColor {
        *app.world().get::<BorderColor>(entity).unwrap()
    }

    fn framed(app: &mut App, background: ThemeToken) -> Entity {
        app.world_mut()
            .resource_mut::<Theme>()
            .set_color(EDGE, GREEN);
        app.world_mut()
            .spawn((ThemeSkin(background), ThemeBorder::all(EDGE)))
            .id()
    }

    /// A border under a picture would only show through its transparent corners.
    #[test]
    fn skins_over_the_border_hide_it_until_cleared() {
        let mut app = app();
        app.world_mut()
            .resource_mut::<Theme>()
            .set_skin(BG, Skin::new(Handle::default()));
        let entity = framed(&mut app, BG);
        assert_eq!(border(&app, entity), BorderColor::all(Color::NONE));

        app.world_mut().resource_mut::<Theme>().clear_skin(&BG);
        app.update();
        assert_eq!(border(&app, entity), BorderColor::all(GREEN));
    }

    #[test]
    fn skins_inside_the_border_keep_it() {
        let mut app = app();
        app.world_mut()
            .resource_mut::<Theme>()
            .set_skin(BG, Skin::new(Handle::default()).inside_border());
        let entity = framed(&mut app, BG);
        assert_eq!(border(&app, entity), BorderColor::all(GREEN));
    }

    /// Widgets swap only the background when only it changes, so the border
    /// has to follow a state that has a skin of its own.
    #[test]
    fn swapping_in_a_skinned_state_hides_the_border() {
        let mut app = app();
        let pressed = BG.with_state(WidgetState::Pressed);
        app.world_mut()
            .resource_mut::<Theme>()
            .set_skin(pressed.clone(), Skin::new(Handle::default()));
        let entity = framed(&mut app, BG);
        assert_eq!(border(&app, entity), BorderColor::all(GREEN));

        app.world_mut()
            .entity_mut(entity)
            .insert(ThemeSkin(pressed));
        assert_eq!(border(&app, entity), BorderColor::all(Color::NONE));

        app.world_mut().entity_mut(entity).insert(ThemeSkin(BG));
        assert_eq!(border(&app, entity), BorderColor::all(GREEN));
    }

    /// Text keeps Bevy's font until the theme's font has loaded, then
    /// switches without anything respawning.
    #[test]
    fn text_shows_a_font_once_it_has_loaded() {
        use crate::theme::palette;

        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::asset::AssetPlugin::default(),
            ThemePlugin,
        ))
        .init_asset::<Font>();
        let font = app.world().resource::<Assets<Font>>().reserve_handle();
        app.world_mut()
            .resource_mut::<Theme>()
            .set_font(palette::FONT, font.clone());
        let text = app
            .world_mut()
            .spawn((Text::new("score"), ThemeFont(palette::FONT)))
            .id();
        let shown = |app: &App| app.world().get::<TextFont>(text).unwrap().font.clone();
        app.update();
        assert_eq!(shown(&app), FontSource::Handle(Handle::default()));

        app.world_mut()
            .resource_mut::<Assets<Font>>()
            .insert(font.id(), Font::from_bytes(Vec::new()))
            .unwrap();
        // The load is announced at the end of one frame and applied the next.
        app.update();
        app.update();
        assert_eq!(shown(&app), FontSource::Handle(font.clone()));

        // A copy of the theme from before the font loaded, put back whole, as
        // the showcase does on leaving a page: the font has still loaded.
        let mut before = app.world().resource::<Theme>().clone();
        before.set_font_loaded(font.id(), false);
        *app.world_mut().resource_mut::<Theme>() = before;
        app.update();
        assert_eq!(shown(&app), FontSource::Handle(font));
    }
}
