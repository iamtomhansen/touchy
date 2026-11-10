//! The palette: a few semantic colours, and a font, that every touchy
//! widget's defaults derive from. Set these to restyle all of touchy at once, and override an
//! individual widget's tokens only for exceptions.
//!
//! ```
//! # use bevy::prelude::*;
//! # use touchy::theme::{Theme, palette};
//! # let mut theme = Theme::default();
//! theme.set_color(palette::ACCENT, Color::srgb(0.93, 0.45, 0.24));
//! ```

use bevy::prelude::*;

use super::{Shade, Theme, ThemeToken};

/// Selection and fills: a checked box, a slider's fill, the primary button.
pub const ACCENT: ThemeToken = ThemeToken::new_static("touchy.palette.accent");
/// Attention edges: a hovered border, a dragged thumb, a timeline marker.
pub const HIGHLIGHT: ThemeToken = ThemeToken::new_static("touchy.palette.highlight");
/// Sunken areas: tracks, wells, fields and the inside of a checkbox.
pub const SURFACE: ThemeToken = ThemeToken::new_static("touchy.palette.surface");
/// Raised parts you press or grab: secondary buttons, knobs.
pub const CONTROL: ThemeToken = ThemeToken::new_static("touchy.palette.control");
/// Outlines around surfaces and controls.
pub const EDGE: ThemeToken = ThemeToken::new_static("touchy.palette.edge");
/// Text and primary readouts.
pub const TEXT: ThemeToken = ThemeToken::new_static("touchy.palette.text");
/// Labels and secondary readouts.
pub const TEXT_MUTED: ThemeToken = ThemeToken::new_static("touchy.palette.text.muted");
/// Destructive actions: a fill dark enough for light text, as on a delete button.
pub const DANGER: ThemeToken = ThemeToken::new_static("touchy.palette.danger");
/// Cautions: a warning toast, a low reading.
pub const WARNING: ThemeToken = ThemeToken::new_static("touchy.palette.warning");
/// News that's neither good nor bad: an info toast.
pub const INFO: ThemeToken = ThemeToken::new_static("touchy.palette.info");
/// Good outcomes: a success toast, a full bar.
pub const SUCCESS: ThemeToken = ThemeToken::new_static("touchy.palette.success");
/// The see-through veil over the game behind a modal overlay.
pub const SCRIM: ThemeToken = ThemeToken::new_static("touchy.palette.scrim");
/// The typeface every widget's text follows. Bevy's own font by default.
pub const FONT: ThemeToken = ThemeToken::new_static("touchy.palette.font");

/// The size of body text, in pixels; headings and small print follow it.
pub const TEXT_SIZE: ThemeToken = ThemeToken::new_static("touchy.palette.text.size");
/// The size of headings: body text, scaled by [`HEADING_SCALE`] unless set.
pub const HEADING_SIZE: ThemeToken = ThemeToken::new_static("touchy.palette.heading.size");
/// The size of small print: body text, scaled by [`SMALL_SCALE`] unless set.
pub const SMALL_SIZE: ThemeToken = ThemeToken::new_static("touchy.palette.small.size");

/// A list's markers: its bullets and numbers. The palette's muted text unless set; uses a skin
/// instead if available.
pub const BULLET: ThemeToken = ThemeToken::new_static("touchy.palette.bullet");

/// How much larger than body text a heading is, unless a theme sets its size.
pub const HEADING_SCALE: f32 = 1.5;
/// How much smaller than body text small print is, unless a theme sets its size.
pub const SMALL_SCALE: f32 = 0.85;

/// The size of the text on control widgets. Can be set to specific size for pixel-fonts.
pub const CONTROL_TEXT_SIZE: ThemeToken =
    ThemeToken::new_static("touchy.palette.control.text.size");
/// The corner radius of a [`Card`](crate::primitives::Card), in pixels.
pub const CORNER_RADIUS: ThemeToken = ThemeToken::new_static("touchy.palette.corner_radius");
/// The corner radius of controls, in pixels; small parts, as a checkbox, round less. 0 = square.
pub const CONTROL_CORNER_RADIUS: ThemeToken =
    ThemeToken::new_static("touchy.palette.control.corner_radius");

/// Dark enough that [`Shade::Contrast`](super::Shade::Contrast) puts white text on it.
const DEFAULT_ACCENT: Color = Color::srgb(0.22, 0.45, 0.80);
const DEFAULT_HIGHLIGHT: Color = Color::srgb(0.95, 0.78, 0.25);
const DEFAULT_SURFACE: Color = Color::srgb(0.10, 0.12, 0.16);
const DEFAULT_CONTROL: Color = Color::srgb(0.26, 0.28, 0.33);
const DEFAULT_EDGE: Color = Color::srgb(0.30, 0.34, 0.42);
const DEFAULT_TEXT: Color = Color::srgb(0.94, 0.95, 0.97);
const DEFAULT_TEXT_MUTED: Color = Color::srgb(0.58, 0.62, 0.70);
const DEFAULT_DANGER: Color = Color::srgb(0.55, 0.14, 0.14);
const DEFAULT_WARNING: Color = Color::srgb(0.85, 0.75, 0.25);
const DEFAULT_INFO: Color = Color::srgb(0.25, 0.55, 0.85);
const DEFAULT_SUCCESS: Color = Color::srgb(0.25, 0.85, 0.45);
const DEFAULT_SCRIM: Color = Color::srgba(0.02, 0.04, 0.08, 0.7);
const DEFAULT_TEXT_SIZE: f32 = 14.0;
const DEFAULT_CONTROL_TEXT_SIZE: f32 = 13.0;
const DEFAULT_CORNER_RADIUS: f32 = 6.0;
const DEFAULT_CONTROL_CORNER_RADIUS: f32 = 4.0;

/// Gives every palette token its default. Called by `ThemePlugin`.
pub(super) fn default_palette(theme: &mut Theme) {
    theme
        .default_color(ACCENT, DEFAULT_ACCENT)
        .default_color(HIGHLIGHT, DEFAULT_HIGHLIGHT)
        .default_color(SURFACE, DEFAULT_SURFACE)
        .default_color(CONTROL, DEFAULT_CONTROL)
        .default_color(EDGE, DEFAULT_EDGE)
        .default_color(TEXT, DEFAULT_TEXT)
        .default_color(TEXT_MUTED, DEFAULT_TEXT_MUTED)
        .default_color(DANGER, DEFAULT_DANGER)
        .default_color(WARNING, DEFAULT_WARNING)
        .default_color(INFO, DEFAULT_INFO)
        .default_color(SUCCESS, DEFAULT_SUCCESS)
        .default_color(SCRIM, DEFAULT_SCRIM)
        .default_font(FONT, Handle::default())
        .default_size(TEXT_SIZE, DEFAULT_TEXT_SIZE)
        .default_size_scaled(HEADING_SIZE, TEXT_SIZE, HEADING_SCALE)
        .default_size_scaled(SMALL_SIZE, TEXT_SIZE, SMALL_SCALE)
        .default_size(CONTROL_TEXT_SIZE, DEFAULT_CONTROL_TEXT_SIZE)
        .default_size(CORNER_RADIUS, DEFAULT_CORNER_RADIUS)
        .default_size(CONTROL_CORNER_RADIUS, DEFAULT_CONTROL_CORNER_RADIUS)
        .default_derived(BULLET, TEXT_MUTED, Shade::Same);
}
