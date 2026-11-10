//! A theme sheet: a set of colours, sizes, fonts, skins and state shades,
//! applied and replaced as one. UI elements are themed by referring to a
//! token, which the [`Theme`](super::Theme) looks up in its code values, then
//! in the active sheet, then in the plugins' defaults.

use bevy::asset::{UntypedAssetId, VisitAssetDependencies};
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::text::Font;

use super::resolve::{ColorSource, Follows, SizeSource};
use super::{Shade, Skin, ThemeToken, WidgetState};

/// A set of theme values that is applied, and replaced, as one. The active
/// sheet sits between plugin defaults and values the app sets in code, so
/// switching themes means setting a different sheet with
/// [`Theme::set_sheet`](super::Theme::set_sheet): everything the old sheet
/// set goes, and the app's own values stay on top.
///
/// Sheets are also assets, written as `.theme.ron` files and made active with
/// [`ActiveTheme`](super::ActiveTheme). Build one in code when the
/// theme belongs to the game itself:
///
/// ```
/// # use bevy::prelude::*;
/// # use touchy::theme::{Shade, ThemeSheet, palette};
/// let ember = ThemeSheet::default()
///     .color(palette::ACCENT, Color::srgb(0.93, 0.45, 0.24))
///     .derived(palette::HIGHLIGHT, palette::ACCENT, Shade::Lighter(0.2));
/// ```
#[derive(Clone, Debug, Default, TypePath)]
pub struct ThemeSheet {
    pub(super) colors: HashMap<ThemeToken, ColorSource>,
    pub(super) sizes: HashMap<ThemeToken, SizeSource>,
    pub(super) fonts: HashMap<ThemeToken, Follows<Handle<Font>>>,
    pub(super) skins: HashMap<ThemeToken, Follows<Skin>>,
    pub(super) state_shades: HashMap<WidgetState, Shade>,
}

impl ThemeSheet {
    /// Sets `token` to `color`.
    pub fn color(mut self, token: ThemeToken, color: Color) -> Self {
        self.colors.insert(token, ColorSource::Value(color));
        self
    }

    /// Makes `token` follow `from`, shaded.
    pub fn derived(mut self, token: ThemeToken, from: ThemeToken, shade: Shade) -> Self {
        self.colors
            .insert(token, ColorSource::Derived { from, shade });
        self
    }

    /// Sets `token` to `size`.
    pub fn size(mut self, token: ThemeToken, size: f32) -> Self {
        self.sizes.insert(token, SizeSource::Value(size));
        self
    }

    /// Makes `token` follow the size of `from`.
    pub fn size_from(self, token: ThemeToken, from: ThemeToken) -> Self {
        self.size_scaled(token, from, 1.0)
    }

    /// Makes `token` follow the size of `from`, times `scale`.
    pub fn size_scaled(mut self, token: ThemeToken, from: ThemeToken, scale: f32) -> Self {
        self.sizes.insert(token, SizeSource::From { from, scale });
        self
    }

    /// Sets `token` to `font`.
    pub fn font(mut self, token: ThemeToken, font: Handle<Font>) -> Self {
        self.fonts.insert(token, Follows::Value(font));
        self
    }

    /// Makes `token` follow the font of `from`.
    pub fn font_from(mut self, token: ThemeToken, from: ThemeToken) -> Self {
        self.fonts.insert(token, Follows::From(from));
        self
    }

    /// Draws `skin` wherever `token` would show a flat colour.
    pub fn skin(mut self, token: ThemeToken, skin: Skin) -> Self {
        self.skins.insert(token, Follows::Value(skin));
        self
    }

    /// Makes `token` draw the skin of `from`, tinted by `token`'s own colour.
    pub fn skin_from(mut self, token: ThemeToken, from: ThemeToken) -> Self {
        self.skins.insert(token, Follows::From(from));
        self
    }

    /// Sets how `state` shades tokens that have no value of their own in it.
    pub fn state_shade(mut self, state: WidgetState, shade: Shade) -> Self {
        self.state_shades.insert(state, shade);
        self
    }

    /// Lays `top` over this sheet, token by token: where both set a value,
    /// `top`'s wins, and everything else either sets is kept. How a palette
    /// goes on top of a skin.
    pub fn lay_over(&mut self, top: &ThemeSheet) -> &mut Self {
        self.colors.extend(top.colors.clone());
        self.sizes.extend(top.sizes.clone());
        self.fonts.extend(top.fonts.clone());
        self.skins.extend(top.skins.clone());
        self.state_shades.extend(top.state_shades.clone());
        self
    }
}

impl Asset for ThemeSheet {}

/// A sheet depends on its skins' pictures and its fonts, so it counts as loaded
/// with its dependencies only once every file it uses has loaded too.
impl VisitAssetDependencies for ThemeSheet {
    fn visit_dependencies(&self, visit: &mut impl FnMut(UntypedAssetId)) {
        let skins = self.skins.values().filter_map(|skin| match skin {
            Follows::Value(skin) => Some(skin),
            Follows::From(_) => None,
        });
        for picture in skins.filter_map(|skin| skin.picture.as_ref()) {
            visit(picture.image.id().untyped());
        }
        for font in self.fonts.values() {
            if let Follows::Value(font) = font {
                visit(font.id().untyped());
            }
        }
    }
}
