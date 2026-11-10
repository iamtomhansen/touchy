//! Skin roles: a few shapes every touchy widget's skins follow, as its colours
//! follow the [palette](super::palette). Skin these to restyle all of touchy at
//! once, and skin an individual widget's tokens only for exceptions.
//!
//! A role has no skin until a theme gives it one, so widgets draw their flat
//! colours. Once it has one, every widget token following it draws it, tinted
//! by that token's own colour: one grey picture serves the primary button,
//! the danger button, and every state of each.
//!
//! ```
//! # use bevy::prelude::*;
//! # use touchy::theme::{Skin, Theme, skins};
//! fn bevel_every_control(asset_server: Res<AssetServer>, mut theme: ResMut<Theme>) {
//!     let bevel = asset_server.load("ui/raised.png");
//!     theme.set_skin(skins::RAISED, Skin::new(bevel).sliced(8.0));
//! }
//! ```
//!
//! A role in a state follows too: a skin for `touchy.skin.raised:pressed`
//! reaches every pressed control that follows the role.
//!
//! In a theme file, a widget token can follow a different role, or none:
//! `"touchy.slider.thumb": (from: "touchy.skin.raised")`.
//!
//! Each role is also a colour token, the colour a node drawn with the role
//! directly, as `ThemeSkin(skins::RAISED)`, takes. Widgets tint it with their own.

use super::palette::{ACCENT, CONTROL, SURFACE as SURFACE_COLOUR, TEXT};
use super::{Shade, Theme, ThemeToken};

/// Panels, cards, menus, popovers, toasts and tooltips: what other things sit on.
pub const SURFACE: ThemeToken = ThemeToken::new_static("touchy.skin.surface");
/// What's pressed: buttons, steppers, dropdowns, segmented highlights, d-pad arms.
pub const RAISED: ThemeToken = ThemeToken::new_static("touchy.skin.raised");
/// What's filled in or slid along: text fields, tracks, troughs, plots.
pub const SUNKEN: ThemeToken = ThemeToken::new_static("touchy.skin.sunken");
/// What's grabbed: every thumb and knob.
pub const KNOB: ThemeToken = ThemeToken::new_static("touchy.skin.knob");
/// What shows a choice: a checkbox's mark.
pub const MARK: ThemeToken = ThemeToken::new_static("touchy.skin.mark");

/// How much darker than the surface a sunken role is, unless a theme sets it.
const SUNKEN_DEPTH: f32 = 0.04;

/// Gives every role its colour. Called by `ThemePlugin`.
pub(super) fn default_roles(theme: &mut Theme) {
    theme
        .default_derived(SURFACE, SURFACE_COLOUR, Shade::Same)
        .default_derived(RAISED, CONTROL, Shade::Same)
        .default_derived(SUNKEN, SURFACE_COLOUR, Shade::Darker(SUNKEN_DEPTH))
        .default_derived(KNOB, ACCENT, Shade::Same)
        .default_derived(MARK, TEXT, Shade::Same);
}
