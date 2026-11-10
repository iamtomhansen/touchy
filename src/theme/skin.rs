//! What a skin is: how a token looks when a flat colour isn't enough. A skin
//! is a short stack of layers, every one of them something Bevy draws on the
//! node itself, so a skinned node needs no extra nodes and its layout is
//! untouched.
//!
//! Bevy draws a node's layers in a fixed order, back to front: shadows, the
//! fill, the border's colours, gradients, border gradients, the picture, then
//! the outline around it all. A skin sets any of them; that order is its stack.
//!
//! In Bevy UI, pictures are [`ImageNode`]s, not `Sprite`s, which belong to the
//! 2D world.

use bevy::prelude::*;
use bevy::ui::{
    ColorStop, LinearGradient, RadialGradient, RadialGradientShape, ShadowStyle, UiPosition,
};

use super::{Shade, ThemeToken};

/// How tiled parts repeat: once every picture's width, at the picture's own size.
const TILE: SliceScaleMode = SliceScaleMode::Tile { stretch_value: 1.0 };

/// How a token looks: layers drawn on the node in place of its flat colour.
/// Give one to the theme with [`Theme::set_skin`](super::Theme::set_skin):
///
/// ```
/// # use bevy::prelude::*;
/// # use touchy::prelude::*;
/// # use touchy::theme::{Skin, SkinColor, SkinGradient, SkinShadow, Shade};
/// fn skin_buttons(asset_server: Res<AssetServer>, mut theme: ResMut<Theme>) {
///     // A nine-sliced picture in place of the colour.
///     let frame = asset_server.load("ui/button.png");
///     theme.set_skin(ButtonTokens::primary().bg, Skin::new(frame).sliced(6.0));
///     // The button's own colour, lit from above, with a soft glow.
///     let sheen = SkinGradient::linear(LinearGradient::TO_BOTTOM, [
///         SkinColor::Own(Shade::Lighter(0.15)),
///         SkinColor::Own(Shade::Darker(0.1)),
///     ]);
///     let glow = SkinShadow::new(SkinColor::Own(Shade::Lighter(0.3))).blur(12.0);
///     theme.set_skin(ButtonTokens::secondary().bg, Skin::default().gradient(sheen).shadow(glow));
/// }
/// ```
///
/// Every colour in a skin is a [`SkinColor`], so it can be the token's own
/// colour, another token's, or one of its own, shaded on the way. A state
/// without a skin of its own borrows its resting token's, and its state shade
/// still shows: the token's own colour already carries it, and every other
/// colour in the skin is shaded by it.
///
/// A skin without a picture keeps the token's colour as its [fill](Self::fill),
/// so a skin that only adds a glow leaves the rest of the look alone. A
/// [picture](Self::new) replaces the colour.
///
/// The fields are public, and the builders set them; a builder for a part of
/// the picture does nothing on a skin without one.
#[derive(Clone, Debug, PartialEq)]
pub struct Skin {
    /// A flat colour under everything else, or none.
    pub fill: Option<SkinColor>,
    /// Gradients over the fill, each drawn over the one before. Bevy draws
    /// them inside the node's border, so its border colours still frame them.
    pub gradients: Vec<SkinGradient>,
    /// Gradients drawn on the node's border, each over the one before, and
    /// over the border's own colours.
    pub border: Vec<SkinGradient>,
    /// Shadows behind the node, each over the one before (unlike CSS, where
    /// the first is on top). One with no offset and a bright colour is a glow.
    pub shadows: Vec<SkinShadow>,
    /// A line around the node, outside its border, which takes no space.
    pub outline: Option<SkinOutline>,
    /// A picture over everything but the outline.
    pub picture: Option<SkinPicture>,
    /// How long each layer takes to change into this skin's when a node
    /// takes it on, as a widget changes state. Instant unless set.
    pub transition: SkinTransition,
}

/// A skin that looks just like no skin: the token's colour, ready for layers.
impl Default for Skin {
    fn default() -> Self {
        Self {
            fill: Some(SkinColor::OWN),
            gradients: Vec::new(),
            border: Vec::new(),
            shadows: Vec::new(),
            outline: None,
            picture: None,
            transition: SkinTransition::default(),
        }
    }
}

impl Skin {
    /// `image` in place of the token's colour, stretched over the node and
    /// tinted by that colour.
    ///
    /// By default the picture is stretched over the whole node, border included,
    /// and multiplied by the token's colour, so white or grey art takes on the
    /// palette and every state shade, and one picture serves every variant and state.
    ///
    /// A [sliced](Self::sliced) picture keeps its corners and tiles its edges and
    /// middle, so patterned art repeats rather than smears;
    /// [`stretched`](Self::stretched) stretches them instead, for smooth art like a gradient.
    pub fn new(image: Handle<Image>) -> Self {
        Self {
            fill: None,
            ..default()
        }
        .picture(image)
    }

    /// Draws `image` over the skin's other layers, keeping its fill, so a
    /// frame with a clear middle can sit on the token's colour.
    pub fn picture(mut self, image: Handle<Image>) -> Self {
        self.picture = Some(SkinPicture::new(image));
        self
    }

    /// A flat colour under every other layer.
    pub fn fill(mut self, color: impl Into<SkinColor>) -> Self {
        self.fill = Some(color.into());
        self
    }

    /// Adds a gradient over the fill and any gradient before it.
    pub fn gradient(mut self, gradient: SkinGradient) -> Self {
        self.gradients.push(gradient);
        self
    }

    /// Adds a gradient on the node's border, over any before it.
    pub fn border(mut self, gradient: SkinGradient) -> Self {
        self.border.push(gradient);
        self
    }

    /// Adds a shadow or a glow behind the node, over any before it.
    pub fn shadow(mut self, shadow: SkinShadow) -> Self {
        self.shadows.push(shadow);
        self
    }

    /// How long each layer takes to change into this skin's: `0.15` for all of
    /// them, or a [`SkinTransition`] for each its own, such as a glow fading in
    /// while the colour changes at once.
    pub fn transition(mut self, transition: impl Into<SkinTransition>) -> Self {
        self.transition = transition.into();
        self
    }

    /// Draws a line around the node.
    pub fn outline(mut self, outline: SkinOutline) -> Self {
        self.outline = Some(outline);
        self
    }

    /// Every colour the skin uses.
    pub(super) fn colors(&self) -> impl Iterator<Item = &SkinColor> {
        let gradients = (self.gradients.iter().chain(&self.border))
            .flat_map(SkinGradient::stops)
            .map(|stop| &stop.color);
        self.fill
            .iter()
            .chain(gradients)
            .chain(self.shadows.iter().map(|shadow| &shadow.color))
            .chain(self.outline.iter().map(|outline| &outline.color))
            .chain(self.picture.iter().map(|picture| &picture.tint))
    }

    /// Changes the picture, if there is one.
    fn with_picture(mut self, change: impl FnOnce(&mut SkinPicture)) -> Self {
        if let Some(picture) = &mut self.picture {
            change(picture);
        }
        self
    }

    /// Nine-slice the picture: its corners keep their `border` pixels, one
    /// number for every side or a [`BorderRect`], and its edges and middle
    /// tile, so framed art fits a node of any size.
    pub fn sliced(self, border: impl Into<BorderRect>) -> Self {
        self.mode(NodeImageMode::Sliced(TextureSlicer {
            border: border.into(),
            center_scale_mode: TILE,
            sides_scale_mode: TILE,
            max_corner_scale: 1.0,
        }))
    }

    /// Tiles the whole picture across the node, at its own size.
    pub fn tiled(self) -> Self {
        self.mode(NodeImageMode::Tiled {
            tile_x: true,
            tile_y: true,
            stretch_value: 1.0,
        })
    }

    /// Stretches instead of tiling: a sliced picture's edges and middle, or a
    /// tiled picture whole.
    pub fn stretched(self) -> Self {
        self.with_picture(|picture| match &mut picture.mode {
            NodeImageMode::Sliced(slicer) => {
                slicer.sides_scale_mode = SliceScaleMode::Stretch;
                slicer.center_scale_mode = SliceScaleMode::Stretch;
            }
            mode => *mode = NodeImageMode::Stretch,
        })
    }

    /// How a sliced picture's four edges fill their length: tiled or stretched.
    /// No effect on a picture that isn't sliced.
    pub fn sides(self, scale: SliceScaleMode) -> Self {
        self.with_picture(|picture| {
            if let NodeImageMode::Sliced(slicer) = &mut picture.mode {
                slicer.sides_scale_mode = scale;
            }
        })
    }

    /// How a sliced picture's middle fills the node: tiled or stretched. No
    /// effect on a picture that isn't sliced.
    pub fn center(self, scale: SliceScaleMode) -> Self {
        self.with_picture(|picture| {
            if let NodeImageMode::Sliced(slicer) = &mut picture.mode {
                slicer.center_scale_mode = scale;
            }
        })
    }

    /// The most a sliced picture's corners may grow, as a multiple of their
    /// size in the picture. No effect on a picture that isn't sliced.
    pub fn max_corner_scale(self, scale: f32) -> Self {
        self.with_picture(|picture| {
            if let NodeImageMode::Sliced(slicer) = &mut picture.mode {
                slicer.max_corner_scale = scale;
            }
        })
    }

    /// Fits the picture any way Bevy can: tiling one axis only, say.
    pub fn mode(self, mode: NodeImageMode) -> Self {
        self.with_picture(|picture| picture.mode = mode)
    }

    /// Draws the picture inside the node's border instead of over it, so the
    /// widget's themed border (a bevel, a hover highlight) frames the art.
    pub fn inside_border(self) -> Self {
        self.with_picture(|picture| picture.area = VisualBox::PaddingBox)
    }

    /// Cuts the picture into a sprite sheet of `columns` by `rows` frames, and
    /// shows the one the node's [`SkinFrame`] picks.
    pub fn frames(self, columns: u32, rows: u32) -> Self {
        self.with_picture(|picture| {
            picture.frames = Some(SkinFrames {
                columns,
                rows,
                fps: None,
            });
        })
    }

    /// Loops a sprite sheet's frames at `fps` frames a second. No effect on a
    /// picture that isn't cut into [`frames`](Self::frames).
    pub fn animated(self, fps: f32) -> Self {
        self.with_picture(|picture| {
            if let Some(frames) = &mut picture.frames {
                frames.fps = Some(fps);
            }
        })
    }

    /// Mirrors the picture left to right.
    pub fn flip_x(self) -> Self {
        self.with_picture(|picture| picture.flip_x = true)
    }

    /// Mirrors the picture top to bottom.
    pub fn flip_y(self) -> Self {
        self.with_picture(|picture| picture.flip_y = true)
    }

    /// Multiplies the picture by `color` rather than the token's own colour.
    pub fn tint(self, color: impl Into<SkinColor>) -> Self {
        let color = color.into();
        self.with_picture(|picture| picture.tint = color)
    }

    /// Draws the picture's own colours instead of tinting it. States without
    /// a skin of their own still shade it.
    pub fn untinted(self) -> Self {
        self.tint(Color::WHITE)
    }
}

/// How long, in seconds, each of a skin's layers takes to change into it.
/// A node changing state changes into the new state's skin, or the resting
/// token's skin that the state borrows, so a transition on the resting skin
/// covers every state, and one on a state's own skin covers entering it. A
/// layer at 0 changes at once.
///
/// Colours, sizes and gradients of the same shape change smoothly; a layer
/// that appears fades in, and one that goes fades out. A new picture, or a
/// gradient of another shape, changes at once.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SkinTransition {
    /// The flat fill's colour.
    pub fill: f32,
    /// The gradients over the fill.
    pub gradients: f32,
    /// The gradients on the border.
    pub border: f32,
    /// The shadows and glows.
    pub shadows: f32,
    /// The outline.
    pub outline: f32,
    /// The colour the picture is tinted by.
    pub tint: f32,
}

impl SkinTransition {
    /// Every layer over `seconds`.
    pub const fn all(seconds: f32) -> Self {
        Self {
            fill: seconds,
            gradients: seconds,
            border: seconds,
            shadows: seconds,
            outline: seconds,
            tint: seconds,
        }
    }
}

/// Every layer over this many seconds.
impl From<f32> for SkinTransition {
    fn from(seconds: f32) -> Self {
        Self::all(seconds)
    }
}

/// A colour in a skin.
#[derive(Clone, Debug, PartialEq)]
pub enum SkinColor {
    /// The colour of the token the skin is drawn for, shaded: a skin made of
    /// these follows the palette and every state.
    Own(Shade),
    /// Another token's colour, shaded.
    Derived {
        /// The token whose colour is used.
        from: ThemeToken,
        /// How it's changed on the way.
        shade: Shade,
    },
    /// A colour as given.
    Value(Color),
}

impl SkinColor {
    /// The colour of the token the skin is drawn for, as it is.
    pub const OWN: Self = Self::Own(Shade::Same);
}

impl SkinColor {
    /// The token this colour is taken from, other than the skin's own.
    pub(super) fn token(&self) -> Option<&ThemeToken> {
        match self {
            Self::Derived { from, .. } => Some(from),
            Self::Own(_) | Self::Value(_) => None,
        }
    }
}

impl From<Color> for SkinColor {
    fn from(color: Color) -> Self {
        Self::Value(color)
    }
}

/// A gradient in a skin, filling the node or drawn on its border.
#[derive(Clone, Debug, PartialEq)]
pub enum SkinGradient {
    /// Colours changing along a line.
    Linear {
        /// The line's direction, in radians clockwise from pointing up;
        /// [`LinearGradient`]'s constants name the usual ones.
        angle: f32,
        /// The colours along the line.
        stops: Vec<SkinStop>,
    },
    /// Colours changing outward from a point.
    Radial {
        /// Where the gradient starts.
        position: UiPosition,
        /// How far it reaches, and whether it's a circle or an ellipse.
        shape: RadialGradientShape,
        /// The colours from the centre out.
        stops: Vec<SkinStop>,
    },
}

impl SkinGradient {
    /// Colours along a line at `angle`, in radians clockwise from pointing up.
    pub fn linear(angle: f32, stops: impl IntoIterator<Item = impl Into<SkinStop>>) -> Self {
        Self::Linear {
            angle,
            stops: stops.into_iter().map(Into::into).collect(),
        }
    }

    /// Colours outward from `position`, as far as `shape` reaches.
    pub fn radial(
        position: UiPosition,
        shape: RadialGradientShape,
        stops: impl IntoIterator<Item = impl Into<SkinStop>>,
    ) -> Self {
        Self::Radial {
            position,
            shape,
            stops: stops.into_iter().map(Into::into).collect(),
        }
    }

    /// One colour all over: a single stop, which Bevy draws flat. On the
    /// border, it's a border colour.
    pub fn flat(color: impl Into<SkinColor>) -> Self {
        Self::linear(0.0, [color.into()])
    }

    /// The gradient's colours.
    fn stops(&self) -> &[SkinStop] {
        match self {
            Self::Linear { stops, .. } | Self::Radial { stops, .. } => stops,
        }
    }

    /// The gradient Bevy draws, with each colour as `color` resolves it.
    pub(super) fn paint(&self, color: impl Fn(&SkinColor) -> Color) -> Gradient {
        let stops = |stops: &[SkinStop]| -> Vec<ColorStop> {
            let point = |at: Option<f32>| at.map_or(Val::Auto, |at| percent(at * 100.0));
            let stop = |stop: &SkinStop| ColorStop::new(color(&stop.color), point(stop.at));
            stops.iter().map(stop).collect()
        };
        match self {
            Self::Linear { angle, stops: list } => LinearGradient::new(*angle, stops(list)).into(),
            Self::Radial {
                position,
                shape,
                stops: list,
            } => RadialGradient::new(*position, *shape, stops(list)).into(),
        }
    }
}

/// One colour of a gradient.
#[derive(Clone, Debug, PartialEq)]
pub struct SkinStop {
    /// The colour.
    pub color: SkinColor,
    /// How far along the gradient it sits, from 0 at the start to 1 at the
    /// end, or `None` to share the space evenly with its neighbours.
    pub at: Option<f32>,
}

impl SkinStop {
    /// `color`, `at` the way along the gradient, from 0 to 1.
    pub fn new(color: impl Into<SkinColor>, at: f32) -> Self {
        Self {
            color: color.into(),
            at: Some(at),
        }
    }
}

/// A stop placed evenly between its neighbours.
impl From<SkinColor> for SkinStop {
    fn from(color: SkinColor) -> Self {
        Self { color, at: None }
    }
}

/// A stop placed evenly between its neighbours.
impl From<Color> for SkinStop {
    fn from(color: Color) -> Self {
        SkinColor::from(color).into()
    }
}

/// A shadow behind a node, the shape of the node and its rounded corners.
/// With no offset and a bright colour, it's a glow; bloom never reaches UI,
/// so this is how UI glows.
#[derive(Clone, Debug, PartialEq)]
pub struct SkinShadow {
    /// The shadow's colour.
    pub color: SkinColor,
    /// How far it's moved from the node, in pixels; positive is right and down.
    pub offset: Vec2,
    /// How far it blurs, in pixels.
    pub blur: f32,
    /// How much bigger than the node it is, in pixels on every side.
    pub spread: f32,
}

impl SkinShadow {
    /// A hard-edged shadow of `color` right behind the node: give it an
    /// offset, a blur or a spread to show.
    pub fn new(color: impl Into<SkinColor>) -> Self {
        Self {
            color: color.into(),
            offset: Vec2::ZERO,
            blur: 0.0,
            spread: 0.0,
        }
    }

    /// Moves the shadow by `offset` pixels; positive is right and down.
    pub fn offset(mut self, offset: Vec2) -> Self {
        self.offset = offset;
        self
    }

    /// Blurs the shadow over `blur` pixels.
    pub fn blur(mut self, blur: f32) -> Self {
        self.blur = blur;
        self
    }

    /// Grows the shadow by `spread` pixels on every side.
    pub fn spread(mut self, spread: f32) -> Self {
        self.spread = spread;
        self
    }

    /// The shadow Bevy draws, with its colour as `color` resolves it.
    pub(super) fn paint(&self, color: impl Fn(&SkinColor) -> Color) -> ShadowStyle {
        ShadowStyle {
            color: color(&self.color),
            x_offset: px(self.offset.x),
            y_offset: px(self.offset.y),
            spread_radius: px(self.spread),
            blur_radius: px(self.blur),
        }
    }
}

/// A line around a node, outside its border, following its rounded corners.
/// It takes no space, so it can come and go without moving anything.
#[derive(Clone, Debug, PartialEq)]
pub struct SkinOutline {
    /// The line's colour.
    pub color: SkinColor,
    /// How thick it is, in pixels.
    pub width: f32,
    /// How far out from the border it's drawn, in pixels.
    pub offset: f32,
}

impl SkinOutline {
    /// A line of `color`, `width` pixels thick, right against the border.
    pub fn new(color: impl Into<SkinColor>, width: f32) -> Self {
        Self {
            color: color.into(),
            width,
            offset: 0.0,
        }
    }

    /// Draws the line `offset` pixels out from the border.
    pub fn offset(mut self, offset: f32) -> Self {
        self.offset = offset;
        self
    }

    /// The outline Bevy draws, with its colour as `color` resolves it.
    pub(super) fn paint(&self, color: impl Fn(&SkinColor) -> Color) -> Outline {
        Outline::new(px(self.width), px(self.offset), color(&self.color))
    }
}

/// A skin's picture and how it's fitted to the node. [`Skin`]'s builders set
/// these too.
#[derive(Clone, Debug, PartialEq)]
pub struct SkinPicture {
    /// The picture.
    pub image: Handle<Image>,
    /// How it's fitted: stretched, sliced or tiled.
    pub mode: NodeImageMode,
    /// What it's multiplied by: the token's own colour unless told otherwise.
    pub tint: SkinColor,
    /// Whether it's drawn over the border or inside it.
    pub area: VisualBox,
    /// The sprite sheet it's cut into, if any.
    pub frames: Option<SkinFrames>,
    /// Whether it's mirrored left to right.
    pub flip_x: bool,
    /// Whether it's mirrored top to bottom.
    pub flip_y: bool,
}

impl SkinPicture {
    /// `image`, stretched over the node and tinted by the token's colour.
    pub fn new(image: Handle<Image>) -> Self {
        Self {
            image,
            mode: NodeImageMode::Stretch,
            tint: SkinColor::OWN,
            area: VisualBox::BorderBox,
            frames: None,
            flip_x: false,
            flip_y: false,
        }
    }
}

/// A sprite sheet: a picture cut into a grid of equal frames, counted left
/// to right, then top to bottom. A skin made from one shows a single frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SkinFrames {
    /// How many frames across.
    pub columns: u32,
    /// How many frames down.
    pub rows: u32,
    /// Frames a second to loop through, or `None` to show the frame the
    /// node's [`SkinFrame`] picks.
    pub fps: Option<f32>,
}

impl SkinFrames {
    /// How many frames the sheet holds; at least one.
    fn count(self) -> u32 {
        (self.columns.max(1) * self.rows.max(1)).max(1)
    }

    /// The frame to show: by time if the sheet loops, else at `at`, from 0
    /// (the first frame) to 1 (the last).
    pub(super) fn index(self, at: f32, seconds: f32) -> u32 {
        let last = self.count() - 1;
        match self.fps {
            Some(fps) => (seconds * fps.max(0.0)) as u32 % self.count(),
            None => (at.clamp(0.0, 1.0) * last as f32).round() as u32,
        }
    }

    /// Where frame `index` sits in a picture of `size`, in pixels.
    pub(super) fn rect(self, size: UVec2, index: u32) -> Rect {
        let grid = UVec2::new(self.columns.max(1), self.rows.max(1));
        let frame = size.as_vec2() / grid.as_vec2();
        let at = UVec2::new(index % grid.x, index / grid.x).as_vec2() * frame;
        Rect::from_corners(at, at + frame)
    }
}

/// Which frame of its skin's sprite sheet a node shows, from 0, the first,
/// to 1, the last. Widgets set it from their value, as a slider's thumb does
/// from where it sits; the app can set it on anything. A looping sheet
/// ignores it.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct SkinFrame(pub f32);

#[cfg(test)]
mod tests {
    use super::*;

    fn slicer(skin: &Skin) -> &TextureSlicer {
        match &skin.picture.as_ref().unwrap().mode {
            NodeImageMode::Sliced(slicer) => slicer,
            other => panic!("not sliced: {other:?}"),
        }
    }

    #[test]
    fn sliced_skins_tile_and_can_stretch_each_part() {
        let skin = Skin::new(Handle::default()).sliced(4.0);
        assert_eq!(slicer(&skin).sides_scale_mode, TILE);
        assert_eq!(slicer(&skin).center_scale_mode, TILE);

        let skin = skin.center(SliceScaleMode::Stretch);
        assert_eq!(slicer(&skin).sides_scale_mode, TILE);
        assert_eq!(slicer(&skin).center_scale_mode, SliceScaleMode::Stretch);
    }

    /// A picture replaces the token's colour; layers alone keep it.
    #[test]
    fn a_picture_replaces_the_fill_and_layers_keep_it() {
        assert_eq!(Skin::new(Handle::default()).fill, None);
        let glow = Skin::default().shadow(SkinShadow::new(Color::WHITE).blur(8.0));
        assert_eq!(glow.fill, Some(SkinColor::OWN));
        let framed = Skin::default().picture(Handle::default());
        assert_eq!(framed.fill, Some(SkinColor::OWN));
    }

    #[test]
    fn picture_builders_leave_a_skin_without_one_alone() {
        let skin = Skin::default().sliced(4.0).frames(5, 1).flip_x().untinted();
        assert_eq!(skin, Skin::default());
    }

    /// Stops without a place share the gradient; placed ones sit where told.
    #[test]
    fn gradients_paint_their_colours_and_places() {
        const RED: Color = Color::srgb(1.0, 0.0, 0.0);
        let gradient = SkinGradient::linear(
            LinearGradient::TO_BOTTOM,
            [
                SkinStop::from(SkinColor::OWN),
                SkinStop::new(Color::BLACK, 0.8),
            ],
        );
        let own = |color: &SkinColor| match color {
            SkinColor::Own(_) => RED,
            SkinColor::Value(color) => *color,
            SkinColor::Derived { .. } => unreachable!(),
        };
        let Gradient::Linear(painted) = gradient.paint(own) else {
            panic!("not linear");
        };
        assert_eq!(painted.angle, LinearGradient::TO_BOTTOM);
        assert_eq!(painted.stops[0], ColorStop::auto(RED));
        assert_eq!(painted.stops[1], ColorStop::percent(Color::BLACK, 80.0));
    }

    const ROW_OF_FIVE: SkinFrames = SkinFrames {
        columns: 5,
        rows: 1,
        fps: None,
    };

    #[test]
    fn a_value_picks_a_frame_from_first_to_last() {
        assert_eq!(ROW_OF_FIVE.index(0.0, 0.0), 0);
        assert_eq!(ROW_OF_FIVE.index(0.5, 0.0), 2);
        assert_eq!(ROW_OF_FIVE.index(1.0, 0.0), 4);
        assert_eq!(ROW_OF_FIVE.index(7.0, 0.0), 4, "held to the last");
    }

    #[test]
    fn a_looping_sheet_picks_by_time_and_wraps() {
        let looping = SkinFrames {
            fps: Some(10.0),
            ..ROW_OF_FIVE
        };
        assert_eq!(looping.index(1.0, 0.25), 2, "ignores the value");
        assert_eq!(looping.index(0.0, 0.55), 0, "wrapped after five");
    }

    #[test]
    fn frames_are_cut_left_to_right_then_down() {
        let grid = SkinFrames {
            columns: 2,
            rows: 2,
            fps: None,
        };
        let size = UVec2::new(64, 32);
        let rect = |index| grid.rect(size, index);
        assert_eq!(rect(0), Rect::new(0.0, 0.0, 32.0, 16.0));
        assert_eq!(rect(1), Rect::new(32.0, 0.0, 64.0, 16.0));
        assert_eq!(rect(2), Rect::new(0.0, 16.0, 32.0, 32.0));
    }

    #[test]
    fn stretching_a_tiled_skin_stretches_it_whole() {
        let skin = Skin::new(Handle::default()).tiled().stretched();
        assert_eq!(skin.picture.unwrap().mode, NodeImageMode::Stretch);
    }
}
