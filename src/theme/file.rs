//! The `.theme.ron` format: how a [`ThemeSheet`] is written as text.
//!
//! ```ron
//! (
//!     colors: {
//!         "touchy.palette.accent": "#ee7340",
//!         "touchy.palette.highlight": (from: "touchy.palette.accent", shade: Lighter(0.2)),
//!         "touchy.button.bg:hover": "#ff8855",
//!     },
//!     sizes: {
//!         "touchy.button.corner_radius": 6.0,
//!         "touchy.button.ghost.font_size": (from: "touchy.button.font_size"),
//!     },
//!     fonts: {
//!         "touchy.palette.font": "/fonts/Inter-Regular.ttf",
//!     },
//!     skins: {
//!         "touchy.button.bg": (image: "panel.png", slice: 5.0),
//!         "touchy.slider.track": (image: "track.png", slice: (8.0, 3.0)),
//!         "touchy.button.primary.bg": (
//!             gradients: [Linear(stops: [(shade: Lighter(0.15)), (shade: Darker(0.1))])],
//!             shadows: [(color: (from: "touchy.palette.accent"), blur: 12.0)],
//!         ),
//!     },
//!     state_shades: {
//!         "hover": Lighter(0.12),
//!     },
//! )
//! ```
//!
//! `assets/themes/reference.theme.ron` lists every token touchy defines.
//!
//! Every section is optional. Tokens are written as [`ThemeToken`]'s `Display`
//! writes them, so `name:state` gives a token in a state (e.g. `bg:disabled`).
//! Colours are hex (`#3873cc`), with an optional alpha pair (`#3873ccFF`).
//! `(from: "token")` follows another token's value; on the way, a colour can
//! be shaded (`shade: Lighter(0.1)`) and a size scaled (`scale: 2.0`).
//!
//! A skin is a stack of layers, each optional, which Bevy draws back to front:
//! `shadows`, `fill`, `gradients`, `border`, the picture (`image`), then `outline`.
//! - `fill` is a flat colour. A skin without an `image` keeps its token's
//!   colour as its fill unless it says otherwise; one with an `image` has none.
//! - `gradients` fill the node, and `border` is drawn on its border, each a
//!   list drawn in order, each over the one before. A gradient is
//!   `Linear(angle: 90.0, stops: [...])`, in degrees clockwise from pointing up
//!   (the default, 180, points down), or `Radial(center: (0.5, 0.0), stops: [...])`,
//!   from fractions of the node's width and height (the default is the middle);
//!   a radial's `shape` is `FarthestCorner` (the default), `ClosestCorner`,
//!   `FarthestSide`, `ClosestSide`, `Circle(20.0)` or `Ellipse(30.0, 20.0)`.
//!   A stop is a colour, shared evenly with its neighbours, or `(colour, 0.25)`,
//!   placed a quarter of the way along. A colour on its own is a flat gradient,
//!   so `border: ["#ffffff"]` is a border colour.
//! - `shadows` are `(color: ..., offset: (0.0, 2.0), blur: 4.0, spread: 1.0)`,
//!   each optional but the colour, in pixels, and each drawn over the one
//!   before; with no offset, a shadow is a glow.
//! - `outline` is `(color: ..., width: 2.0, offset: 1.0)`: a line outside the
//!   border, which takes no space.
//! - `transition: 0.15` eases every layer into this skin over 0.15 seconds as
//!   a widget changes state; `(shadows: 0.5, outline: 0.2)` times each layer
//!   on its own, among `fill`, `gradients`, `border`, `shadows`, `outline` and
//!   `tint` (the picture's colour), and the rest change at once.
//!
//! A colour in a skin is written as under `colors`, or as `(shade: Lighter(0.1))`
//! for the skin's own token's colour, shaded, so it follows the palette and
//! every state.
//!
//! A skin's `image` is stretched over its node, unless it's sliced or tiled:
//! - `slice: 5.0` keeps 5-pixel corners and tiles the edges and middle; `(8.0, 3.0)`
//!   gives the left and right borders, then the top and bottom. `sides: Stretch`
//!   and `center: Stretch` stretch those parts instead, and `corner_scale: 2.0`
//!   lets corners grow to twice their size.
//! - `tile: Both` repeats the whole picture; `X` or `Y` repeats it along one axis.
//!
//! `frames: (columns: 5)` cuts the picture into a sprite sheet (`rows` too, if
//! it has more than one) and shows the frame the widget picks, such as a
//! slider's thumb by its value; `fps: 12.0` loops through the frames instead.
//! `flip: X`, `Y` or `Both` mirrors the picture. These need an `image`.
//!
//! Image paths are relative to the theme file; a leading `/` can instead start
//! from the asset folder. No path leaves the asset folder: Bevy refuses one
//! that climbs out of it, unless the app sets `AssetPlugin::unapproved_path_mode`
//! to `Allow`.
//!
//! The types in this module are private on purpose: File parsing rigidly
//! populates [`ThemeSheet`], which can be changed later and have [`Shade`] applied.
//! This module is the only place the two meet.

use std::collections::BTreeMap;

use bevy::asset::ParseAssetPathError;
use bevy::prelude::*;
use bevy::sprite::{BorderRect, SliceScaleMode};
use bevy::ui::{RadialGradientShape, UiPosition};
use ron::extensions::Extensions;
use serde::Deserialize;
use thiserror::Error;

use super::{
    ParseTokenError, Shade, Skin, SkinColor, SkinGradient, SkinOutline, SkinShadow, SkinStop,
    SkinTransition, ThemeSheet, ThemeToken, WidgetState,
};

/// RON extensions every theme file gets: an optional field is written bare,
/// so `slice: 5.0` is read as `slice: Some(5.0)`, and a gradient's fields sit
/// right in its name's brackets, as in `Linear(angle: 90.0, stops: [...])`.
const RON_EXTENSIONS: Extensions =
    Extensions::IMPLICIT_SOME.union(Extensions::UNWRAP_VARIANT_NEWTYPES);

/// The one colour written by name: no colour at all, as in CSS. Easier to
/// read and to search for than `"#00000000"`.
pub(super) const TRANSPARENT: &str = "transparent";

/// Where a linear gradient points unless told: down, as in CSS.
const LINEAR_ANGLE_DEGREES: f32 = 180.0;

/// The middle of a node, where a radial gradient starts unless told, in
/// fractions of its width and height.
const RADIAL_CENTER: (f32, f32) = (0.5, 0.5);

/// How far a radial gradient reaches unless told: to the farthest corner,
/// as in CSS, so it covers the node.
const RADIAL_SHAPE: RadialGradientShape = RadialGradientShape::FarthestCorner;

/// The one font written by name: Bevy's own, which has no file.
pub(super) const DEFAULT_FONT: &str = "default";

/// Turns a file path, as written in a theme file, into a handle. The asset
/// loader resolves it against the theme file's own location.
pub(super) trait LoadFile {
    fn load<A: Asset>(&mut self, path: &str) -> Result<Handle<A>, ParseAssetPathError>;
}

/// What went wrong reading a theme file. Each message names the entry at fault.
#[derive(Debug, Error)]
pub(super) enum ThemeFileError {
    #[error("couldn't read the theme file: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Syntax(#[from] ron::error::SpannedError),
    #[error("`{key}` isn't a valid token: {source}")]
    Token {
        key: String,
        source: ParseTokenError,
    },
    #[error("in `state_shades`: {0}")]
    State(ParseTokenError),
    #[error("`{token}`: `{value}` isn't a hex colour like \"#3873cc\", or `transparent`")]
    Color { token: String, value: String },
    #[error("`{token}`: only colours can be shaded")]
    Shade { token: String },
    #[error("`{token}`: only sizes can be scaled")]
    Scale { token: String },
    #[error("`{token}`: {problem}")]
    Skin {
        token: String,
        problem: &'static str,
    },
    #[error("`{token}`: `{path}` isn't a valid file path: {source}")]
    FilePath {
        token: String,
        path: String,
        source: ParseAssetPathError,
    },
}

/// Reads a theme file into a sheet. `files` turns each file path, as written,
/// into a handle; the asset loader resolves it against the theme file's own path.
pub(super) fn read_sheet(
    bytes: &[u8],
    files: &mut impl LoadFile,
) -> Result<ThemeSheet, ThemeFileError> {
    let file: SheetFile = ron::Options::default()
        .with_default_extension(RON_EXTENSIONS)
        .from_bytes(bytes)?;

    let mut sheet = ThemeSheet::default();
    // Tokens under the `colors` key are either hex or derived from another token.
    for (key, entry) in file.colors {
        let token = parse_token(&key)?;
        sheet = match entry {
            ColorEntry::Written(value) => match parse_color(&value) {
                Some(color) => sheet.color(token, color),
                None => return Err(ThemeFileError::Color { token: key, value }),
            },
            ColorEntry::From(entry) => {
                let (from, shade) = entry.for_color(&key)?;
                sheet.derived(token, from, shade)
            }
        };
    }
    // Tokens under the `sizes` key are either a number or follow another token.
    for (key, entry) in file.sizes {
        let token = parse_token(&key)?;
        sheet = match entry {
            SizeEntry::Value(size) => sheet.size(token, size),
            SizeEntry::From(entry) => {
                let (from, scale) = entry.for_size(&key)?;
                sheet.size_scaled(token, from, scale)
            }
        };
    }
    // Tokens under the `fonts` key are either a file path, "default", or follow another token.
    for (key, entry) in file.fonts {
        let token = parse_token(&key)?;
        sheet = match entry {
            FontEntry::File(path) if path == DEFAULT_FONT => sheet.font(token, Handle::default()),
            FontEntry::File(path) => sheet.font(token, load_file(files, &key, &path)?),
            FontEntry::From(entry) => sheet.font_from(token, entry.for_font(&key)?),
        };
    }
    // Tokens under the `skins` key are a stack of optional layers, or follow another skin.
    for (key, mut entry) in file.skins {
        let token = parse_token(&key)?;
        sheet = match entry.from.take() {
            Some(_) if !entry.is_empty() => {
                return Err(skin_problem(
                    &key,
                    "a skin that follows another (`from`) has no layers of its own",
                ));
            }
            Some(from) => sheet.skin_from(token, parse_token(&from)?),
            None => sheet.skin(token, entry.into_skin(&key, files)?),
        };
    }
    // Tokens under the `state_shades` key are a state and a shade.
    for (key, shade) in file.state_shades {
        let state: WidgetState = key.parse().map_err(ThemeFileError::State)?;
        sheet = sheet.state_shade(state, shade.into());
    }
    Ok(sheet)
}

/// Load a file path into a handle.
fn load_file<A: Asset>(
    files: &mut impl LoadFile,
    token: &str,
    path: &str,
) -> Result<Handle<A>, ThemeFileError> {
    files.load(path).map_err(|source| ThemeFileError::FilePath {
        token: token.to_owned(),
        path: path.to_owned(),
        source,
    })
}

/// For now, only aliases "transparent" to `Color::NONE`. Otherwise checks hex codes.
fn parse_color(value: &str) -> Option<Color> {
    if value == TRANSPARENT {
        return Some(Color::NONE);
    }
    Srgba::hex(value).ok().map(Color::from)
}

/// Parse a token, reporting the key it was read from if it fails.
fn parse_token(key: &str) -> Result<ThemeToken, ThemeFileError> {
    key.parse().map_err(|source| ThemeFileError::Token {
        key: key.to_owned(),
        source,
    })
}

/// The whole file. Maps are ordered so the first error reported is the same every time.
#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct SheetFile {
    colors: BTreeMap<String, ColorEntry>,
    sizes: BTreeMap<String, SizeEntry>,
    fonts: BTreeMap<String, FontEntry>,
    skins: BTreeMap<String, SkinEntry>,
    state_shades: BTreeMap<String, ShadeEntry>,
}

/// `"#DA1A32"`, or `(from: "token", shade: Lighter(0.1))`.
#[derive(Deserialize)]
#[serde(
    untagged,
    expecting = "a hex colour like \"#3873cc\", \"transparent\", or (from: \"token\", shade: Lighter(0.1))"
)]
enum ColorEntry {
    /// Hex, or the name [`TRANSPARENT`].
    Written(String),
    From(FromEntry),
}

/// `6.0`, or `(from: "token")` to follow another size.
#[derive(Deserialize)]
#[serde(untagged, expecting = "a size like 6.0, or (from: \"token\")")]
enum SizeEntry {
    Value(f32),
    From(FromEntry),
}

/// `"fonts/SansSerif.ttf"` or `"default"`, or `(from: "token")` to follow another font.
#[derive(Deserialize)]
#[serde(
    untagged,
    expecting = "a font file like \"/fonts/SansSerif.ttf\", \"default\", or (from: \"token\")"
)]
enum FontEntry {
    File(String),
    From(FromEntry),
}

/// `(from: "token")`, following another token's value, with the modifier
/// its kind takes: a colour `shade: Lighter(0.1)`, a size `scale: 2.0`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FromEntry {
    from: String,
    #[serde(default)]
    shade: Option<ShadeEntry>,
    #[serde(default)]
    scale: Option<f32>,
}

impl FromEntry {
    /// The colour followed and its shade, as under `key`.
    fn for_color(self, key: &str) -> Result<(ThemeToken, Shade), ThemeFileError> {
        if self.scale.is_some() {
            return Err(ThemeFileError::Scale {
                token: key.to_owned(),
            });
        }
        let shade = self.shade.map_or(Shade::Same, Shade::from);
        Ok((parse_token(&self.from)?, shade))
    }

    /// The size followed and its scale, as under `key`.
    fn for_size(self, key: &str) -> Result<(ThemeToken, f32), ThemeFileError> {
        if self.shade.is_some() {
            return Err(ThemeFileError::Shade {
                token: key.to_owned(),
            });
        }
        Ok((parse_token(&self.from)?, self.scale.unwrap_or(1.0)))
    }

    /// The font followed, as under `key`: fonts take no modifier.
    fn for_font(self, key: &str) -> Result<ThemeToken, ThemeFileError> {
        if self.scale.is_some() {
            return Err(ThemeFileError::Scale {
                token: key.to_owned(),
            });
        }
        let (from, _) = self.for_size(key)?;
        Ok(from)
    }
}

/// [`Shade`], as written in a file.
#[derive(Deserialize)]
enum ShadeEntry {
    Same,
    Lighter(f32),
    Darker(f32),
    Alpha(f32),
    Contrast,
}

impl From<ShadeEntry> for Shade {
    fn from(shade: ShadeEntry) -> Self {
        match shade {
            ShadeEntry::Same => Shade::Same,
            ShadeEntry::Lighter(amount) => Shade::Lighter(amount),
            ShadeEntry::Darker(amount) => Shade::Darker(amount),
            ShadeEntry::Alpha(alpha) => Shade::Alpha(alpha),
            ShadeEntry::Contrast => Shade::Contrast,
        }
    }
}

/// A [`Skin`], as written in a file. Leaving every layer but the picture out
/// gives the same skin as [`Skin::new`] does; leaving the picture out too
/// gives [`Skin::default`].
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SkinEntry {
    /// Another token's skin to draw instead, such as a role like `touchy.skin.raised`.
    #[serde(default)]
    from: Option<String>,
    /// A flat colour under the other layers.
    #[serde(default)]
    fill: Option<SkinColorEntry>,
    /// Gradients over the fill.
    #[serde(default)]
    gradients: Vec<GradientEntry>,
    /// Gradients on the border.
    #[serde(default)]
    border: Vec<GradientEntry>,
    /// Shadows and glows behind the node.
    #[serde(default)]
    shadows: Vec<ShadowEntry>,
    /// A line around the node.
    #[serde(default)]
    outline: Option<OutlineEntry>,
    /// How long the layers take to change into this skin.
    #[serde(default)]
    transition: Option<TransitionEntry>,
    /// Path to the picture's file, relative to the theme file.
    #[serde(default)]
    image: Option<String>,
    /// Nine-slice border in pixels, as [`Skin::sliced`].
    #[serde(default)]
    slice: Option<SliceEntry>,
    /// How a sliced skin's edges fill their length.
    #[serde(default)]
    sides: Option<ScaleEntry>,
    /// How a sliced skin's middle fills the node.
    #[serde(default)]
    center: Option<ScaleEntry>,
    /// How much a sliced skin's corners may grow.
    #[serde(default)]
    corner_scale: Option<f32>,
    /// Repeat the whole picture, as [`Skin::tiled`], instead of stretching it.
    #[serde(default)]
    tile: Option<AxesEntry>,
    /// Cut the picture into a sprite sheet, as [`Skin::frames`].
    #[serde(default)]
    frames: Option<FramesEntry>,
    /// Mirror the picture.
    #[serde(default)]
    flip: Option<AxesEntry>,
    /// Whether the picture is multiplied by the token's colour.
    #[serde(default)]
    tinted: Option<bool>,
    /// Draw the picture inside the border.
    #[serde(default)]
    inside_border: Option<bool>,
}

/// A colour in a skin: as under `colors`, or `(shade: Lighter(0.1))` for the
/// skin's own token's colour, shaded.
#[derive(Deserialize)]
#[serde(
    untagged,
    expecting = "a hex colour like \"#3873cc\", \"transparent\", (from: \"token\", shade: Lighter(0.1)), \
                 or (shade: Lighter(0.1)) for the skin's own colour"
)]
enum SkinColorEntry {
    /// Hex, or the name [`TRANSPARENT`].
    Written(String),
    Shaded(ShadedEntry),
}

/// Another token's colour, or without `from` the skin's own, shaded.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ShadedEntry {
    #[serde(default)]
    from: Option<String>,
    #[serde(default)]
    shade: Option<ShadeEntry>,
}

impl SkinColorEntry {
    /// The colour this describes, in the skin under `key`.
    fn into_color(self, key: &str) -> Result<SkinColor, ThemeFileError> {
        match self {
            SkinColorEntry::Written(value) => match parse_color(&value) {
                Some(color) => Ok(SkinColor::Value(color)),
                None => Err(ThemeFileError::Color {
                    token: key.to_owned(),
                    value,
                }),
            },
            SkinColorEntry::Shaded(ShadedEntry { from, shade }) => {
                let shade = shade.map_or(Shade::Same, Shade::from);
                Ok(match from {
                    Some(from) => SkinColor::Derived {
                        from: parse_token(&from)?,
                        shade,
                    },
                    None => SkinColor::Own(shade),
                })
            }
        }
    }
}

/// A gradient: a colour on its own, drawn flat, or a shaped one.
#[derive(Deserialize)]
#[serde(
    untagged,
    expecting = "a colour, or a gradient like Linear(stops: [...]) or Radial(stops: [...])"
)]
enum GradientEntry {
    Flat(SkinColorEntry),
    Shaped(ShapedEntry),
}

/// A linear or radial gradient.
#[derive(Deserialize)]
enum ShapedEntry {
    Linear(LinearEntry),
    Radial(RadialEntry),
}

/// Colours along a line.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LinearEntry {
    /// Degrees clockwise from pointing up.
    #[serde(default = "linear_angle")]
    angle: f32,
    stops: Vec<StopEntry>,
}

/// Colours outward from a point.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RadialEntry {
    /// In fractions of the node's width and height, from its top-left.
    #[serde(default = "radial_center")]
    center: (f32, f32),
    #[serde(default)]
    shape: Option<ShapeEntry>,
    stops: Vec<StopEntry>,
}

fn linear_angle() -> f32 {
    LINEAR_ANGLE_DEGREES
}

fn radial_center() -> (f32, f32) {
    RADIAL_CENTER
}

/// How far a radial gradient reaches.
#[derive(Deserialize)]
enum ShapeEntry {
    ClosestSide,
    FarthestSide,
    ClosestCorner,
    FarthestCorner,
    Circle(f32),
    Ellipse(f32, f32),
}

impl From<ShapeEntry> for RadialGradientShape {
    fn from(shape: ShapeEntry) -> Self {
        match shape {
            ShapeEntry::ClosestSide => RadialGradientShape::ClosestSide,
            ShapeEntry::FarthestSide => RadialGradientShape::FarthestSide,
            ShapeEntry::ClosestCorner => RadialGradientShape::ClosestCorner,
            ShapeEntry::FarthestCorner => RadialGradientShape::FarthestCorner,
            ShapeEntry::Circle(radius) => RadialGradientShape::Circle(px(radius)),
            ShapeEntry::Ellipse(width, height) => {
                RadialGradientShape::Ellipse(px(width), px(height))
            }
        }
    }
}

/// A gradient's colour: `(colour, 0.25)` placed a quarter of the way along,
/// or a colour alone, shared evenly with its neighbours.
#[derive(Deserialize)]
#[serde(
    untagged,
    expecting = "a colour, or (colour, 0.25) to place it along the gradient"
)]
enum StopEntry {
    At(SkinColorEntry, f32),
    Even(SkinColorEntry),
}

impl StopEntry {
    fn into_stop(self, key: &str) -> Result<SkinStop, ThemeFileError> {
        Ok(match self {
            StopEntry::At(color, at) => SkinStop::new(color.into_color(key)?, at),
            StopEntry::Even(color) => color.into_color(key)?.into(),
        })
    }
}

impl GradientEntry {
    /// The gradient this describes, in the skin under `key`.
    fn into_gradient(self, key: &str) -> Result<SkinGradient, ThemeFileError> {
        let stops = |stops: Vec<StopEntry>| -> Result<Vec<SkinStop>, ThemeFileError> {
            if stops.is_empty() {
                return Err(skin_problem(key, "a gradient needs at least one stop"));
            }
            stops.into_iter().map(|stop| stop.into_stop(key)).collect()
        };
        Ok(match self {
            GradientEntry::Flat(color) => SkinGradient::flat(color.into_color(key)?),
            GradientEntry::Shaped(ShapedEntry::Linear(LinearEntry { angle, stops: list })) => {
                SkinGradient::linear(angle.to_radians(), stops(list)?)
            }
            GradientEntry::Shaped(ShapedEntry::Radial(RadialEntry {
                center: (x, y),
                shape,
                stops: list,
            })) => {
                // Bevy anchors from the middle, from -0.5 to 0.5 each way.
                let position = UiPosition::anchor(Vec2::new(x, y) - Vec2::splat(0.5));
                let shape = shape.map_or(RADIAL_SHAPE, RadialGradientShape::from);
                SkinGradient::radial(position, shape, stops(list)?)
            }
        })
    }
}

/// How long a skin's layers take to change into it: `0.15` for every layer,
/// or `(shadows: 0.5)` for each its own.
#[derive(Deserialize)]
#[serde(
    untagged,
    expecting = "seconds like 0.15, or a time for each layer like (shadows: 0.5, outline: 0.2)"
)]
enum TransitionEntry {
    All(f32),
    Layers(LayerTimesEntry),
}

/// Seconds for each layer; a layer left out changes at once.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LayerTimesEntry {
    #[serde(default)]
    fill: f32,
    #[serde(default)]
    gradients: f32,
    #[serde(default)]
    border: f32,
    #[serde(default)]
    shadows: f32,
    #[serde(default)]
    outline: f32,
    #[serde(default)]
    tint: f32,
}

impl From<TransitionEntry> for SkinTransition {
    fn from(entry: TransitionEntry) -> Self {
        match entry {
            TransitionEntry::All(seconds) => SkinTransition::all(seconds),
            TransitionEntry::Layers(times) => SkinTransition {
                fill: times.fill,
                gradients: times.gradients,
                border: times.border,
                shadows: times.shadows,
                outline: times.outline,
                tint: times.tint,
            },
        }
    }
}

/// A shadow, or with no offset a glow, in pixels.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ShadowEntry {
    color: SkinColorEntry,
    #[serde(default)]
    offset: (f32, f32),
    #[serde(default)]
    blur: f32,
    #[serde(default)]
    spread: f32,
}

/// A line around the node, in pixels.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OutlineEntry {
    color: SkinColorEntry,
    width: f32,
    #[serde(default)]
    offset: f32,
}

/// A mistake in the skin under `key`.
fn skin_problem(key: &str, problem: &'static str) -> ThemeFileError {
    ThemeFileError::Skin {
        token: key.to_owned(),
        problem,
    }
}

/// A nine-slice border: `5.0` on every side, or `(8.0, 3.0)`, the left and
/// right then the top and bottom.
#[derive(Deserialize)]
#[serde(
    untagged,
    expecting = "a border like 5.0, or (8.0, 3.0) for left and right, top and bottom"
)]
enum SliceEntry {
    All(f32),
    Axes(f32, f32),
}

impl From<SliceEntry> for BorderRect {
    fn from(slice: SliceEntry) -> Self {
        match slice {
            SliceEntry::All(border) => BorderRect::all(border),
            SliceEntry::Axes(x, y) => BorderRect::axes(x, y),
        }
    }
}

/// A sprite sheet: `(columns: 5)`, `(columns: 4, rows: 2)`, and `fps: 12.0`
/// to loop through it.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FramesEntry {
    columns: u32,
    #[serde(default = "one_row")]
    rows: u32,
    #[serde(default)]
    fps: Option<f32>,
}

fn one_row() -> u32 {
    1
}

impl AxesEntry {
    /// Whether this names the x axis, and the y axis.
    fn axes(self) -> (bool, bool) {
        match self {
            AxesEntry::X => (true, false),
            AxesEntry::Y => (false, true),
            AxesEntry::Both => (true, true),
        }
    }
}

/// How part of a sliced skin fills its space.
#[derive(Deserialize)]
enum ScaleEntry {
    Stretch,
    Tile,
}

impl From<ScaleEntry> for SliceScaleMode {
    fn from(scale: ScaleEntry) -> Self {
        match scale {
            ScaleEntry::Stretch => SliceScaleMode::Stretch,
            ScaleEntry::Tile => SliceScaleMode::Tile { stretch_value: 1.0 },
        }
    }
}

/// Which way a skin tiles or flips.
#[derive(Deserialize, Clone, Copy)]
enum AxesEntry {
    X,
    Y,
    Both,
}

impl SkinEntry {
    /// Whether it has no layers or picture: all a skin that follows another may be.
    fn is_empty(&self) -> bool {
        self.fill.is_none()
            && self.gradients.is_empty()
            && self.border.is_empty()
            && self.shadows.is_empty()
            && self.outline.is_none()
            && self.transition.is_none()
            && self.image.is_none()
            && self.slice.is_none()
            && self.sides.is_none()
            && self.center.is_none()
            && self.corner_scale.is_none()
            && self.tile.is_none()
            && self.frames.is_none()
            && self.flip.is_none()
            && self.tinted.is_none()
            && self.inside_border.is_none()
    }

    /// The skin this entry, under `key`, describes, or what's wrong with it.
    fn into_skin(self, key: &str, files: &mut impl LoadFile) -> Result<Skin, ThemeFileError> {
        let problem = |problem| Err(skin_problem(key, problem));
        let fitting = self.slice.is_some() || self.tile.is_some() || self.frames.is_some();
        let dressing = self.flip.is_some() || self.tinted.is_some() || self.inside_border.is_some();
        let slicing = self.sides.is_some() || self.center.is_some() || self.corner_scale.is_some();
        if self.image.is_none() && (fitting || dressing || slicing) {
            return problem(
                "`slice`, `tile`, `frames`, `flip`, `tinted` and `inside_border` need an `image`",
            );
        }
        if self.slice.is_some() && self.tile.is_some() {
            return problem("a skin is sliced or tiled, not both");
        }
        if slicing && self.slice.is_none() {
            return problem("`sides`, `center` and `corner_scale` need a `slice`");
        }
        let mut skin = match &self.image {
            Some(path) => Skin::new(load_file(files, key, path)?),
            None => Skin::default(),
        };
        if let Some(fill) = self.fill {
            skin = skin.fill(fill.into_color(key)?);
        }
        for gradient in self.gradients {
            skin = skin.gradient(gradient.into_gradient(key)?);
        }
        for gradient in self.border {
            skin = skin.border(gradient.into_gradient(key)?);
        }
        for shadow in self.shadows {
            let (x, y) = shadow.offset;
            skin = skin.shadow(
                SkinShadow::new(shadow.color.into_color(key)?)
                    .offset(Vec2::new(x, y))
                    .blur(shadow.blur)
                    .spread(shadow.spread),
            );
        }
        if let Some(transition) = self.transition {
            let transition = SkinTransition::from(transition);
            let times = [
                transition.fill,
                transition.gradients,
                transition.border,
                transition.shadows,
                transition.outline,
                transition.tint,
            ];
            if !times
                .iter()
                .all(|seconds| seconds.is_finite() && *seconds >= 0.0)
            {
                return problem("a transition takes a number of seconds, 0 or more");
            }
            skin = skin.transition(transition);
        }
        if let Some(outline) = self.outline {
            let line = SkinOutline::new(outline.color.into_color(key)?, outline.width);
            skin = skin.outline(line.offset(outline.offset));
        }
        if let Some(border) = self.slice {
            skin = skin.sliced(border);
        }
        if let Some(sides) = self.sides {
            skin = skin.sides(sides.into());
        }
        if let Some(center) = self.center {
            skin = skin.center(center.into());
        }
        if let Some(scale) = self.corner_scale {
            skin = skin.max_corner_scale(scale);
        }
        if let Some(tile) = self.tile {
            let (tile_x, tile_y) = tile.axes();
            skin = skin.mode(NodeImageMode::Tiled {
                tile_x,
                tile_y,
                stretch_value: 1.0,
            });
        }
        if self.tinted == Some(false) {
            skin = skin.untinted();
        }
        if let Some(frames) = self.frames {
            if frames.columns == 0 || frames.rows == 0 {
                return problem("`frames` needs at least one column and one row");
            }
            skin = skin.frames(frames.columns, frames.rows);
            if let Some(fps) = frames.fps {
                skin = skin.animated(fps);
            }
        }
        if let Some(flip) = self.flip {
            let (flip_x, flip_y) = flip.axes();
            if flip_x {
                skin = skin.flip_x();
            }
            if flip_y {
                skin = skin.flip_y();
            }
        }
        if self.inside_border == Some(true) {
            skin = skin.inside_border();
        }
        Ok(skin)
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::super::resolve::{ColorSource, Follows, SizeSource};
    use super::*;
    use bevy::ui::{RadialGradientShape, UiPosition};

    /// Stands in for the asset loader: records each path and returns a
    /// placeholder handle.
    #[derive(Default)]
    pub(in crate::theme) struct Placeholders(pub(in crate::theme) Vec<String>);

    impl LoadFile for Placeholders {
        fn load<A: Asset>(&mut self, path: &str) -> Result<Handle<A>, ParseAssetPathError> {
            self.0.push(path.to_owned());
            Ok(Handle::default())
        }
    }

    const ACCENT: ThemeToken = ThemeToken::new_static("test.accent");
    const FILL: ThemeToken = ThemeToken::new_static("test.fill");

    fn read(text: &str) -> Result<ThemeSheet, ThemeFileError> {
        read_sheet(text.as_bytes(), &mut Placeholders::default())
    }

    /// The skin `sheet` gives `name`, which must be a skin of its own.
    fn skin_of<'a>(sheet: &'a ThemeSheet, name: &str) -> &'a Skin {
        match &sheet.skins[&ThemeToken::new(name.to_owned())] {
            Follows::Value(skin) => skin,
            Follows::From(from) => panic!("{name} follows {from} rather than having a skin"),
        }
    }

    #[test]
    fn a_skin_can_follow_another() {
        let sheet = read_sheet(
            br#"(skins: { "touchy.slider.thumb": (from: "touchy.skin.raised") })"#,
            &mut Placeholders::default(),
        )
        .unwrap();
        assert_eq!(
            sheet.skins[&ThemeToken::new("touchy.slider.thumb")],
            Follows::From(ThemeToken::new("touchy.skin.raised"))
        );
    }

    #[test]
    fn a_skin_that_follows_has_no_layers_of_its_own() {
        let error = read_sheet(
            br#"(skins: { "touchy.slider.thumb": (from: "touchy.skin.raised", image: "a.png") })"#,
            &mut Placeholders::default(),
        );
        assert!(error.is_err(), "a following skin took a picture too");
    }

    #[test]
    fn an_empty_file_is_an_empty_sheet() {
        let sheet = read("()").unwrap();
        assert!(sheet.colors.is_empty() && sheet.sizes.is_empty() && sheet.skins.is_empty());
    }

    #[test]
    fn colors_are_hex_or_derived() {
        let sheet = read(
            r##"(colors: {
                "test.accent": "#ff8000",
                "test.accent:hover": "ff800080",
                "test.fill": (from: "test.accent", shade: Darker(0.1)),
                "test.fill:pressed": (from: "test.accent"),
                "test.fill:disabled": "transparent",
            })"##,
        )
        .unwrap();
        assert_eq!(
            sheet.colors[&ACCENT],
            ColorSource::Value(Color::srgb_u8(255, 128, 0))
        );
        assert_eq!(
            sheet.colors[&ACCENT.with_state(WidgetState::Hover)],
            ColorSource::Value(Color::srgba_u8(255, 128, 0, 128))
        );
        assert_eq!(
            sheet.colors[&FILL],
            ColorSource::Derived {
                from: ACCENT,
                shade: Shade::Darker(0.1)
            }
        );
        assert_eq!(
            sheet.colors[&FILL.with_state(WidgetState::Pressed)],
            ColorSource::Derived {
                from: ACCENT,
                shade: Shade::Same
            }
        );
        assert_eq!(
            sheet.colors[&FILL.with_state(WidgetState::Disabled)],
            ColorSource::Value(Color::NONE)
        );
    }

    #[test]
    fn sizes_and_state_shades_are_read() {
        let sheet = read(
            r#"(
                sizes: {
                    "test.radius": 6,
                    "test.gap": (from: "test.radius"),
                    "test.wide": (from: "test.radius", scale: 2.0),
                },
                state_shades: { "hover": Lighter(0.2), "disabled": Alpha(0.5) },
            )"#,
        )
        .unwrap();
        assert_eq!(
            sheet.sizes[&ThemeToken::new("test.radius")],
            SizeSource::Value(6.0)
        );
        assert_eq!(
            sheet.sizes[&ThemeToken::new("test.gap")],
            SizeSource::From {
                from: ThemeToken::new("test.radius"),
                scale: 1.0
            }
        );
        assert_eq!(
            sheet.sizes[&ThemeToken::new("test.wide")],
            SizeSource::From {
                from: ThemeToken::new("test.radius"),
                scale: 2.0
            }
        );
        assert_eq!(sheet.state_shades[&WidgetState::Hover], Shade::Lighter(0.2));
        assert_eq!(
            sheet.state_shades[&WidgetState::Disabled],
            Shade::Alpha(0.5)
        );
    }

    #[test]
    fn skins_default_like_skin_new_and_pass_paths_as_written() {
        let mut files = Placeholders::default();
        let sheet = read_sheet(
            br#"(skins: {
                "test.plain": (image: "plain.png"),
                "test.framed": (image: "../art/frame.png", slice: 5.0, tinted: false, inside_border: true),
            })"#,
            &mut files,
        )
        .unwrap();
        // Ordered by token name.
        assert_eq!(files.0, ["../art/frame.png", "plain.png"]);
        assert_eq!(*skin_of(&sheet, "test.plain"), Skin::new(Handle::default()));
        assert_eq!(
            *skin_of(&sheet, "test.framed"),
            Skin::new(Handle::default())
                .sliced(5.0)
                .untinted()
                .inside_border()
        );
    }

    /// Each mistake is reported with the entry it's in, so a theme author can
    /// find it without reading the loader.
    #[test]
    fn errors_name_the_entry_at_fault() {
        let message = |text: &str| read(text).err().unwrap().to_string();

        assert_eq!(
            message(r#"(colors: { "test.accent": "orange" })"#),
            "`test.accent`: `orange` isn't a hex colour like \"#3873cc\", or `transparent`"
        );
        assert_eq!(
            message(r##"(colors: { "test.accent:hovered": "#fff" })"##),
            "`test.accent:hovered` isn't a valid token: `hovered` isn't a state; \
             expected one of `hover`, `focused`, `pressed`, `disabled`"
        );
        assert_eq!(
            message(r#"(colors: { "test.fill": (from: ":hover") })"#),
            "`:hover` isn't a valid token: the token has no name"
        );
        assert!(message(r#"(state_shades: { "hovered": Same })"#).starts_with("in `state_shades`"));
    }

    #[test]
    fn a_color_that_is_neither_form_says_what_was_expected() {
        let message = read(r#"(colors: { "test.accent": 5 })"#)
            .err()
            .unwrap()
            .to_string();
        assert!(message.contains("a hex colour like"), "{message}");
    }

    #[test]
    fn typos_in_field_names_are_errors_not_silence() {
        assert!(matches!(
            read(r##"(colours: { "test.accent": "#fff" })"##),
            Err(ThemeFileError::Syntax(_))
        ));
        assert!(matches!(
            read(r#"(skins: { "test.bg": (image: "a.png", slices: 4.0) })"#),
            Err(ThemeFileError::Syntax(_))
        ));
    }

    #[test]
    fn bad_image_paths_are_reported() {
        struct Resolving;
        impl LoadFile for Resolving {
            fn load<A: Asset>(&mut self, path: &str) -> Result<Handle<A>, ParseAssetPathError> {
                bevy::asset::AssetPath::parse("themes/ember.theme.ron").resolve_embed_str(path)?;
                Ok(Handle::default())
            }
        }
        let error = read_sheet(br#"(fonts: { "test.font": "a.ttf#" })"#, &mut Resolving)
            .err()
            .unwrap();
        assert!(matches!(error, ThemeFileError::FilePath { .. }));
        assert!(
            error
                .to_string()
                .starts_with("`test.font`: `a.ttf#` isn't a valid file path")
        );
    }

    #[test]
    fn a_slice_is_one_border_or_one_per_axis() {
        let sheet = read(
            r#"(skins: {
                "test.even": (image: "a.png", slice: 5.0),
                "test.wide": (image: "a.png", slice: (8.0, 3.0)),
            })"#,
        )
        .unwrap();
        let skin = |name| Skin::new(Handle::default()).sliced(name);
        assert_eq!(*skin_of(&sheet, "test.even"), skin(BorderRect::all(5.0)));
        let wide = skin(BorderRect::axes(8.0, 3.0));
        assert_eq!(*skin_of(&sheet, "test.wide"), wide);
    }

    /// A sliced skin tiles its edges and middle unless told to stretch them;
    /// a tiled skin repeats whole.
    #[test]
    fn skins_tile_unless_told_to_stretch() {
        let sheet = read(
            r#"(skins: {
                "test.tiled": (image: "a.png", slice: 4.0),
                "test.smooth": (image: "a.png", slice: 4.0, sides: Stretch, center: Stretch, corner_scale: 2.0),
                "test.cloth": (image: "a.png", tile: X),
            })"#,
        )
        .unwrap();
        let image = || Skin::new(Handle::default());
        assert_eq!(*skin_of(&sheet, "test.tiled"), image().sliced(4.0));
        let smooth = image().sliced(4.0).stretched().max_corner_scale(2.0);
        assert_eq!(*skin_of(&sheet, "test.smooth"), smooth);
        let cloth = image().mode(NodeImageMode::Tiled {
            tile_x: true,
            tile_y: false,
            stretch_value: 1.0,
        });
        assert_eq!(*skin_of(&sheet, "test.cloth"), cloth);
    }

    #[test]
    fn skins_can_be_sprite_sheets_and_flipped() {
        let sheet = read(
            r#"(skins: {
                "test.thumb": (image: "a.png", frames: (columns: 5)),
                "test.spinner": (image: "a.png", frames: (columns: 4, rows: 2, fps: 12.0), flip: X),
            })"#,
        )
        .unwrap();
        let image = || Skin::new(Handle::default());
        assert_eq!(*skin_of(&sheet, "test.thumb"), image().frames(5, 1));
        let spinner = image().frames(4, 2).animated(12.0).flip_x();
        assert_eq!(*skin_of(&sheet, "test.spinner"), spinner);
        let empty = read(r#"(skins: { "test.bg": (image: "a.png", frames: (columns: 0)) })"#);
        assert!(
            empty
                .err()
                .unwrap()
                .to_string()
                .contains("at least one column")
        );
    }

    #[test]
    fn skin_options_that_cant_apply_are_errors() {
        let message = |text: &str| read(text).err().unwrap().to_string();
        assert_eq!(
            message(r#"(skins: { "test.bg": (image: "a.png", slice: 4.0, tile: Both) })"#),
            "`test.bg`: a skin is sliced or tiled, not both"
        );
        assert_eq!(
            message(r#"(skins: { "test.bg": (image: "a.png", sides: Stretch) })"#),
            "`test.bg`: `sides`, `center` and `corner_scale` need a `slice`"
        );
    }

    /// Every layer reads, each colour in any of its forms, and a skin without
    /// a picture keeps its token's colour.
    #[test]
    fn skins_are_stacks_of_layers() {
        let sheet = read(
            r##"(skins: {
                "test.glow": (shadows: [(color: (from: "test.accent"), blur: 12.0)]),
                "test.lit": (
                    fill: "transparent",
                    gradients: [
                        Linear(stops: [(shade: Lighter(0.2)), ("#000000", 0.8)]),
                        Radial(center: (0.5, 0.0), shape: Circle(20.0), stops: ["#ffffff40", "transparent"]),
                    ],
                    border: ["#ffffff", Linear(angle: 90.0, stops: [(from: "test.accent", shade: Darker(0.1))])],
                    shadows: [(color: "#00000080", offset: (0.0, 2.0), blur: 4.0, spread: 1.0)],
                    outline: (color: (shade: Same), width: 2.0, offset: 1.0),
                ),
                "test.framed": (image: "frame.png", fill: (shade: Darker(0.2))),
            })"##,
        )
        .unwrap();
        let accent = |shade| SkinColor::Derived {
            from: ACCENT,
            shade,
        };
        let glow = Skin::default().shadow(SkinShadow::new(accent(Shade::Same)).blur(12.0));
        assert_eq!(*skin_of(&sheet, "test.glow"), glow);

        let black = Color::srgb(0.0, 0.0, 0.0);
        let lit = Skin::default()
            .fill(Color::NONE)
            .gradient(SkinGradient::linear(
                180_f32.to_radians(),
                [
                    SkinStop::from(SkinColor::Own(Shade::Lighter(0.2))),
                    SkinStop::new(black, 0.8),
                ],
            ))
            .gradient(SkinGradient::radial(
                UiPosition::anchor(Vec2::new(0.0, -0.5)),
                RadialGradientShape::Circle(px(20.0)),
                [Color::srgba_u8(255, 255, 255, 64), Color::NONE],
            ))
            .border(SkinGradient::flat(Color::srgb(1.0, 1.0, 1.0)))
            .border(SkinGradient::linear(
                90_f32.to_radians(),
                [accent(Shade::Darker(0.1))],
            ))
            .shadow(
                SkinShadow::new(Color::srgba_u8(0, 0, 0, 128))
                    .offset(Vec2::new(0.0, 2.0))
                    .blur(4.0)
                    .spread(1.0),
            )
            .outline(SkinOutline::new(SkinColor::OWN, 2.0).offset(1.0));
        assert_eq!(*skin_of(&sheet, "test.lit"), lit);

        let framed = Skin::new(Handle::default()).fill(SkinColor::Own(Shade::Darker(0.2)));
        assert_eq!(*skin_of(&sheet, "test.framed"), framed);
    }

    /// A transition is seconds for every layer, or for each its own.
    #[test]
    fn skins_take_transitions() {
        let sheet = read(
            r#"(skins: {
                "test.all": (transition: 0.15),
                "test.each": (transition: (shadows: 0.5, outline: 0.2)),
            })"#,
        )
        .unwrap();
        let all = skin_of(&sheet, "test.all");
        assert_eq!(all.transition, SkinTransition::all(0.15));
        let each = skin_of(&sheet, "test.each");
        let expected = SkinTransition {
            shadows: 0.5,
            outline: 0.2,
            ..default()
        };
        assert_eq!(each.transition, expected);

        let message = |text: &str| read(text).err().unwrap().to_string();
        assert_eq!(
            message(r#"(skins: { "test.bg": (transition: -1.0) })"#),
            "`test.bg`: a transition takes a number of seconds, 0 or more"
        );
        assert!(
            message(r#"(skins: { "test.bg": (transition: (shadow: 0.5)) })"#)
                .contains("a time for each layer"),
            "a misspelt layer is reported, not ignored"
        );
    }

    #[test]
    fn layer_mistakes_are_errors() {
        let message = |text: &str| read(text).err().unwrap().to_string();
        assert_eq!(
            message(r#"(skins: { "test.bg": (slice: 4.0) })"#),
            "`test.bg`: `slice`, `tile`, `frames`, `flip`, `tinted` and `inside_border` need an `image`"
        );
        assert_eq!(
            message(r#"(skins: { "test.bg": (gradients: [Linear(stops: [])]) })"#),
            "`test.bg`: a gradient needs at least one stop"
        );
        assert_eq!(
            message(r#"(skins: { "test.bg": (fill: "orange") })"#),
            "`test.bg`: `orange` isn't a hex colour like \"#3873cc\", or `transparent`"
        );
        assert!(
            message(
                r##"(skins: { "test.bg": (gradients: [Linear(angel: 90.0, stops: ["#fff"])]) })"##
            )
            .contains("a colour, or a gradient"),
            "a typo in a gradient is reported, not ignored"
        );
        assert!(matches!(
            read(r#"(skins: { "test.bg": (shadows: [(blur: 4.0)]) })"#),
            Err(ThemeFileError::Syntax(_))
        ));
    }

    /// Only sizes scale; a scale on a colour or a font is a mistake, not ignored.
    #[test]
    fn only_sizes_can_be_scaled() {
        let message = |text: &str| read(text).err().unwrap().to_string();
        assert_eq!(
            message(r#"(colors: { "test.fill": (from: "test.accent", scale: 2.0) })"#),
            "`test.fill`: only sizes can be scaled"
        );
        assert_eq!(
            message(r#"(fonts: { "test.label": (from: "test.body", scale: 2.0) })"#),
            "`test.label`: only sizes can be scaled"
        );
    }

    /// Only colours have shades; a shade anywhere else is a mistake, not ignored.
    #[test]
    fn only_colours_can_be_shaded() {
        let error = read(r#"(sizes: { "test.gap": (from: "test.radius", shade: Lighter(0.2)) })"#)
            .err()
            .unwrap();
        assert_eq!(error.to_string(), "`test.gap`: only colours can be shaded");
    }

    #[test]
    fn fonts_are_files_default_or_followed() {
        let mut files = Placeholders::default();
        let sheet = read_sheet(
            br#"(fonts: {
                "test.body": "/fonts/body.ttf",
                "test.plain": "default",
                "test.label": (from: "test.body"),
            })"#,
            &mut files,
        )
        .unwrap();
        // Only real files are loaded.
        assert_eq!(files.0, ["/fonts/body.ttf"]);
        assert_eq!(
            sheet.fonts[&ThemeToken::new("test.plain")],
            Follows::Value(Handle::default())
        );
        assert_eq!(
            sheet.fonts[&ThemeToken::new("test.label")],
            Follows::From(ThemeToken::new("test.body"))
        );
    }

    /// The theme files the crate ships, the example's and the reference, stay
    /// readable as the format changes, and every file they name exists.
    #[test]
    fn shipped_theme_files_read_and_the_files_they_name_exist() {
        let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
        let mut sheets = 0;
        for entry in std::fs::read_dir(assets.join("themes")).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            if !name.ends_with(".theme.ron") {
                continue;
            }
            let mut files = Placeholders::default();
            let bytes = std::fs::read(&path).unwrap();
            if let Err(error) = read_sheet(&bytes, &mut files) {
                panic!("{}: {error}", path.display());
            }
            // Resolved the way the asset loader resolves them.
            let sheet_path = bevy::asset::AssetPath::parse(&format!("themes/{name}")).into_owned();
            for written in files.0 {
                let resolved = sheet_path.resolve_embed_str(&written).unwrap();
                let file = assets.join(resolved.path());
                assert!(file.exists(), "{name} names {written}, which doesn't exist");
            }
            sheets += 1;
        }
        assert!(sheets > 0, "no theme files in {}", assets.display());
    }
}
