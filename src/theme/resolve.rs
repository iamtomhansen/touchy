//! How a token gets its value: the [`Theme`] resource and its lookups.
//!
//! A colour token holds a colour, or instructions for deriving it from another
//! token, such as `(from: "touchy.palette.accent", shade: Lighter(0.08))` for
//! "the accent, 8% lighter". Size and font tokens hold a value, or another
//! token of their kind to follow. Rules are followed every time a value is
//! looked up, so changing a base value changes everything built on it.
//!
//! These links must be declared, by a plugin's defaults or by a theme;
//! nothing is inferred from how tokens are named (e.g. "button.background" and
//! "button.background.dark" are not related unless explicitly linked).
//!
//! Every value sits in one of three layers, and a lookup takes the first that
//! has it: 1. Set in code, 2. The active [`ThemeSheet`], 3. Defaults registered
//! by the plugins.
//!
//! A token in a [`WidgetState`] that has no value in any layer falls back
//! to the theme's shade for that state, applied to the same token without its
//! state. So every widget gets `hover`, `pressed` and `disabled` colours if not
//! otherwise declared. The app tunes them all at once with
//! [`Theme::set_state_shade`].
//!
//! A colour token can also have a [`Skin`]: layers such as a picture, gradients
//! or a glow, drawn in place of its colour. Skins follow the same state fallback.

use std::hash::Hash;

use bevy::platform::collections::{HashMap, HashSet};
use bevy::prelude::*;
use bevy::text::Font;

use super::{Skin, SkinColor, ThemeSheet, ThemeToken, WidgetState};

/// Shown for any token the theme has no value for, so gaps are obvious on screen.
const MISSING_COLOR: Color = Color::srgb(1.0, 0.0, 1.0);
/// Used for any size token the theme has no value for.
const MISSING_SIZE: f32 = 12.0;

/// The longest chain of derived colours a lookup follows. Real chains are two
/// or three links long; anything this long is a cycle.
const MAX_CHAIN: usize = 16;

/// How each state shades a token that has no value of its own in that state.
const DEFAULT_HOVER_SHADE: Shade = Shade::Lighter(0.08);
/// The same as hovering: in a game, the item a gamepad or the keyboard is on
/// is the one the cursor would light up. A theme can tell them apart.
const DEFAULT_FOCUSED_SHADE: Shade = Shade::Lighter(0.08);
const DEFAULT_PRESSED_SHADE: Shade = Shade::Darker(0.08);
const DEFAULT_DISABLED_SHADE: Shade = Shade::Alpha(0.35);

/// Text colours chosen by [`Shade::Contrast`].
const CONTRAST_DARK: Color = Color::BLACK;
const CONTRAST_LIGHT: Color = Color::WHITE;
/// The relative luminance above which dark text contrasts better than light.
/// Black and white text have equal WCAG contrast ratios against a background
/// of this luminance: (L + 0.05) / 0.05 = 1.05 / (L + 0.05).
const CONTRAST_CROSSOVER: f32 = 0.179;

/// How a derived colour is made from the colour it derives from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shade {
    /// The same colour.
    Same,
    /// Lighter by this amount of perceived lightness, from 0 (black) to 1
    /// (white), as Oklch measures it: the same step looks the same on a dark
    /// colour as on a light one.
    Lighter(f32),
    /// Darker by this amount of perceived lightness, as for `Lighter`. A dark
    /// colour made darker is darker still, not black.
    Darker(f32),
    /// The same colour at this opacity.
    Alpha(f32),
    /// Black or white, whichever reads better on top of the colour. For text
    /// on a themed background.
    Contrast,
}

impl Shade {
    pub(crate) fn apply(self, base: Color) -> Color {
        // Bevy steps linear luminance, which a dark colour has little of: the
        // smallest darkening sends a dark grey to black. Oklch's lightness is
        // perceptual, so the step looks the same at any lightness.
        let perceived = |shift: f32| {
            let mut oklch = Oklcha::from(base);
            oklch.lightness = (oklch.lightness + shift).clamp(0.0, 1.0);
            Color::Srgba(oklch.into())
        };
        match self {
            Self::Same => base,
            Self::Lighter(amount) => perceived(amount),
            Self::Darker(amount) => perceived(-amount),
            Self::Alpha(alpha) => base.with_alpha(alpha),
            Self::Contrast => {
                if LinearRgba::from(base).luminance() > CONTRAST_CROSSOVER {
                    CONTRAST_DARK
                } else {
                    CONTRAST_LIGHT
                }
            }
        }
    }
}

/// Where a colour token's value comes from.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum ColorSource {
    /// A colour value in any of Bevy's accepted formats.
    Value(Color),
    /// A colour derived from another token, which can be "shaded".
    Derived {
        /// The token to derive from, which can itself be derived.
        from: ThemeToken,
        /// How to shade the colour derived from `from`.
        shade: Shade,
    },
}

/// Where a size token's value comes from.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum SizeSource {
    /// A size of its own, in pixels.
    Value(f32),
    /// Another token's size, times `scale`.
    From {
        /// The token to follow, which can itself follow another.
        from: ThemeToken,
        /// What to multiply the followed size by: 1 for the same size.
        scale: f32,
    },
}

/// Where any other token's value comes from: a value of its own, or the
/// value of the token it follows. Forms a graph of derivations.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Follows<V> {
    /// A value of its own.
    Value(V),
    /// The value of the token it follows.
    From(ThemeToken),
}

/// The shade a state applies when nothing sets one.
fn default_state_shade(state: WidgetState) -> Shade {
    match state {
        WidgetState::Hover => DEFAULT_HOVER_SHADE,
        WidgetState::Focused => DEFAULT_FOCUSED_SHADE,
        WidgetState::Pressed => DEFAULT_PRESSED_SHADE,
        WidgetState::Disabled => DEFAULT_DISABLED_SHADE,
    }
}

/// One kind of value in three layers. A lookup takes the first layer that has
/// the key: values set in code, then the active sheet, then plugin defaults.
#[derive(Clone, Debug)]
struct Layers<K, V> {
    overrides: HashMap<K, V>,
    sheet: HashMap<K, V>,
    defaults: HashMap<K, V>,
}

impl<K, V> Default for Layers<K, V> {
    fn default() -> Self {
        Self {
            overrides: HashMap::default(),
            sheet: HashMap::default(),
            defaults: HashMap::default(),
        }
    }
}

impl<K: Eq + Hash, V> Layers<K, V> {
    fn get(&self, key: &K) -> Option<&V> {
        self.overrides
            .get(key)
            .or_else(|| self.sheet.get(key))
            .or_else(|| self.defaults.get(key))
    }

    /// Fills the default for `key`, unless a plugin already did: the first default wins.
    fn default(&mut self, key: K, value: V) {
        self.defaults.entry(key).or_insert(value);
    }
}

/// Why a lookup failed.
enum Unresolved {
    Missing(ThemeToken),
    Cycle(ThemeToken),
}

impl Unresolved {
    /// What to warn about, for a lookup of this kind of value.
    fn message(&self, kind: &str) -> String {
        match self {
            Self::Missing(token) => {
                format!("theme has no {kind} for {token} (and possibly others)")
            }
            Self::Cycle(token) => format!("theme {kind}s follow each other in a loop at {token}"),
        }
    }
}

/// The size shown when a lookup fails, after warning why.
fn missing_size(unresolved: Unresolved) -> f32 {
    warn_once!("{}", unresolved.message("size"));
    MISSING_SIZE
}

/// Follows `token` through `layers` until a value of its own.
fn follow<'a, V>(
    layers: &'a Layers<ThemeToken, Follows<V>>,
    token: &ThemeToken,
) -> Result<&'a V, Unresolved> {
    let mut current = token;
    for _ in 0..=MAX_CHAIN {
        match layers.get(current) {
            Some(Follows::Value(value)) => return Ok(value),
            Some(Follows::From(next)) => current = next,
            None => return Err(Unresolved::Missing(current.clone())),
        }
    }
    Err(Unresolved::Cycle(token.clone()))
}

/// The values behind every token. Widget plugins register defaults for their
/// own tokens, a [`ThemeSheet`] can restyle them as a set, and the app
/// overrides whichever it likes through `ResMut<Theme>`. Replacing the whole
/// resource would discard the defaults, so modify it instead.
#[derive(Resource, Clone, Debug, Default)]
pub struct Theme {
    colors: Layers<ThemeToken, ColorSource>,
    sizes: Layers<ThemeToken, SizeSource>,
    fonts: Layers<ThemeToken, Follows<Handle<Font>>>,
    skins: Layers<ThemeToken, Follows<Skin>>,
    state_shades: Layers<WidgetState, Shade>,
    /// Fonts that have finished loading.
    loaded_fonts: HashSet<AssetId<Font>>,
}

impl Theme {
    /// The colour behind `token`, following any derivation rules. A missing
    /// token, or a rule that leads back to itself, shows as magenta and logs a
    /// warning.
    pub fn color(&self, token: &ThemeToken) -> Color {
        self.resolve(token, 0).unwrap_or_else(|unresolved| {
            warn_once!("{}", unresolved.message("color"));
            MISSING_COLOR
        })
    }

    /// Sets `token` to `color`, over any sheet or default.
    pub fn set_color(&mut self, token: ThemeToken, color: Color) -> &mut Self {
        self.colors
            .overrides
            .insert(token, ColorSource::Value(color));
        self
    }

    /// Makes `token` follow `from`, shaded, over any sheet or default.
    pub fn set_derived(&mut self, token: ThemeToken, from: ThemeToken, shade: Shade) -> &mut Self {
        self.colors
            .overrides
            .insert(token, ColorSource::Derived { from, shade });
        self
    }

    /// Removes the app's colour for `token`, so the sheet's or the default applies again.
    pub fn clear_color(&mut self, token: &ThemeToken) -> &mut Self {
        self.colors.overrides.remove(token);
        self
    }

    /// Gives `token` a default colour. The sheet's and the app's values always
    /// take precedence, whether set before or after. For widget plugins; the
    /// first default wins.
    pub fn default_color(&mut self, token: ThemeToken, color: Color) -> &mut Self {
        self.colors.default(token, ColorSource::Value(color));
        self
    }

    /// Makes `token` follow `from`, shaded, unless a sheet or the app says
    /// otherwise. For widget plugins; the first default wins.
    pub fn default_derived(
        &mut self,
        token: ThemeToken,
        from: ThemeToken,
        shade: Shade,
    ) -> &mut Self {
        self.colors
            .default(token, ColorSource::Derived { from, shade });
        self
    }

    /// The size behind `token`, following any tokens it follows and scaling
    /// by each link's scale. A missing token, or one that leads back to
    /// itself, logs a warning.
    pub fn size(&self, token: &ThemeToken) -> f32 {
        let mut current = token;
        let mut scale = 1.0;
        for _ in 0..=MAX_CHAIN {
            match self.sizes.get(current) {
                Some(SizeSource::Value(size)) => return size * scale,
                Some(SizeSource::From { from, scale: link }) => {
                    scale *= link;
                    current = from;
                }
                None => return missing_size(Unresolved::Missing(current.clone())),
            }
        }
        missing_size(Unresolved::Cycle(token.clone()))
    }

    /// Sets `token` to `size`, over any sheet or default.
    pub fn set_size(&mut self, token: ThemeToken, size: f32) -> &mut Self {
        self.sizes.overrides.insert(token, SizeSource::Value(size));
        self
    }

    /// Makes `token` follow the size of `from`, over any sheet or default.
    pub fn set_size_from(&mut self, token: ThemeToken, from: ThemeToken) -> &mut Self {
        self.set_size_scaled(token, from, 1.0)
    }

    /// Makes `token` follow the size of `from` times `scale`, over any sheet
    /// or default.
    pub fn set_size_scaled(
        &mut self,
        token: ThemeToken,
        from: ThemeToken,
        scale: f32,
    ) -> &mut Self {
        self.sizes
            .overrides
            .insert(token, SizeSource::From { from, scale });
        self
    }

    /// Removes the app's size for `token`, so the sheet's or the default applies again.
    pub fn clear_size(&mut self, token: &ThemeToken) -> &mut Self {
        self.sizes.overrides.remove(token);
        self
    }

    /// Gives `token` a default size. For widget plugins; the first default wins.
    pub fn default_size(&mut self, token: ThemeToken, size: f32) -> &mut Self {
        self.sizes.default(token, SizeSource::Value(size));
        self
    }

    /// Makes `token` follow the size of `from`, unless a sheet or the app says
    /// otherwise. For widget plugins; the first default wins.
    pub fn default_size_from(&mut self, token: ThemeToken, from: ThemeToken) -> &mut Self {
        self.default_size_scaled(token, from, 1.0)
    }

    /// Makes `token` follow the size of `from` times `scale`, unless a sheet or
    /// the app says otherwise. For widget plugins; the first default wins.
    pub fn default_size_scaled(
        &mut self,
        token: ThemeToken,
        from: ThemeToken,
        scale: f32,
    ) -> &mut Self {
        self.sizes.default(token, SizeSource::From { from, scale });
        self
    }

    /// The font behind `token`, following any tokens it follows. Bevy's own
    /// font stands in until the file has loaded. A missing token, or one that
    /// leads back to itself, logs a warning.
    pub fn font(&self, token: &ThemeToken) -> Handle<Font> {
        match follow(&self.fonts, token) {
            Ok(font) if self.is_available(font) => font.clone(),
            Ok(_) => Handle::default(),
            Err(unresolved) => {
                warn_once!("{}", unresolved.message("font"));
                Handle::default()
            }
        }
    }

    /// Sets `token` to `font`, over any sheet or default.
    pub fn set_font(&mut self, token: ThemeToken, font: Handle<Font>) -> &mut Self {
        self.fonts.overrides.insert(token, Follows::Value(font));
        self
    }

    /// Makes `token` follow the font of `from`, over any sheet or default.
    pub fn set_font_from(&mut self, token: ThemeToken, from: ThemeToken) -> &mut Self {
        self.fonts.overrides.insert(token, Follows::From(from));
        self
    }

    /// Removes the app's font for `token`, so the sheet's or the default applies again.
    pub fn clear_font(&mut self, token: &ThemeToken) -> &mut Self {
        self.fonts.overrides.remove(token);
        self
    }

    /// Gives `token` a default font. For widget plugins; the first default wins.
    pub fn default_font(&mut self, token: ThemeToken, font: Handle<Font>) -> &mut Self {
        self.fonts.default(token, Follows::Value(font));
        self
    }

    /// Makes `token` follow the font of `from`, unless a sheet or the app says
    /// otherwise. For widget plugins; the first default wins.
    pub fn default_font_from(&mut self, token: ThemeToken, from: ThemeToken) -> &mut Self {
        self.fonts.default(token, Follows::From(from));
        self
    }

    /// Records that `font` has loaded, or been unloaded, so tokens using it
    /// show it, or Bevy's font again. Kept up to date by the theme plugin.
    pub(super) fn set_font_loaded(&mut self, font: AssetId<Font>, loaded: bool) {
        if loaded {
            self.loaded_fonts.insert(font);
        } else {
            self.loaded_fonts.remove(&font);
        }
    }

    /// Whether `font` is recorded as loaded.
    pub(super) fn is_font_loaded(&self, font: AssetId<Font>) -> bool {
        self.loaded_fonts.contains(&font)
    }

    /// Every font file a token is set to, at any layer.
    pub(super) fn named_fonts(&self) -> impl Iterator<Item = AssetId<Font>> + '_ {
        let layers = &self.fonts;
        [&layers.overrides, &layers.sheet, &layers.defaults]
            .into_iter()
            .flat_map(|layer| layer.values())
            .filter_map(|follows| match follows {
                Follows::Value(font) => Some(font.id()),
                Follows::From(_) => None,
            })
    }

    fn is_available(&self, font: &Handle<Font>) -> bool {
        // Bevy's own font is built in, so it never waits on a file.
        *font == Handle::default() || self.loaded_fonts.contains(&font.id())
    }

    /// Whether the theme draws a picture for `token` rather than its colour,
    /// for widgets that draw differently when skinned.
    pub fn is_skinned(&self, token: &ThemeToken) -> bool {
        self.skin(token)
            .is_some_and(|painted| painted.skin.picture.is_some())
    }

    /// The skin to draw for `token`, if any, ready to resolve its colours.
    pub(super) fn skin(&self, token: &ThemeToken) -> Option<Painted<'_>> {
        let (skin, exact) = self.find_skin(token, 0)?;
        // A skin made for exactly this token, in its state, is drawn as
        // authored: its own colour is the token's, with no state shade on top.
        // One borrowed from the resting token still has to show the state.
        let borrowed = match token.split_state() {
            Some((state, _)) if !exact => Some(self.state_shade(state)),
            _ => None,
        };
        Some(Painted {
            skin,
            theme: self,
            own: self.color(token),
            borrowed,
        })
    }

    /// The skin `token` draws, following any skin it follows, and whether it
    /// was made for `token`'s state. A token in a state with no skin of its own
    /// takes its resting token's. A skin follows in the same state, so a role's
    /// pressed skin reaches every widget that follows the role.
    fn find_skin(&self, token: &ThemeToken, depth: usize) -> Option<(&Skin, bool)> {
        if depth > MAX_CHAIN {
            warn_once!("{}", Unresolved::Cycle(token.clone()).message("skin"));
            return None;
        }
        let state = token.split_state().map(|(state, _)| state);
        let follow = |from: &ThemeToken| self.find_skin(&from.clone().with_state(state), depth + 1);
        match self.skins.get(token) {
            Some(Follows::Value(skin)) => Some((skin, true)),
            Some(Follows::From(from)) => follow(from),
            None => {
                let (_, resting) = token.split_state()?;
                match self.skins.get(&resting)? {
                    Follows::Value(skin) => Some((skin, false)),
                    Follows::From(from) => follow(from),
                }
            }
        }
    }

    /// Draws `skin` wherever `token` would show a flat colour. A token in a
    /// state with no skin of its own uses its resting token's skin.
    pub fn set_skin(&mut self, token: ThemeToken, skin: Skin) -> &mut Self {
        self.skins.overrides.insert(token, Follows::Value(skin));
        self
    }

    /// Makes `token` draw the skin of `from`, such as a role like
    /// [`skins::RAISED`](super::skins::RAISED), tinted by `token`'s own colour.
    pub fn set_skin_from(&mut self, token: ThemeToken, from: ThemeToken) -> &mut Self {
        self.skins.overrides.insert(token, Follows::From(from));
        self
    }

    /// Makes `token` draw the skin of `from`, unless a sheet or the app says
    /// otherwise: how a widget's tokens take their role's skin. A role with no
    /// skin draws nothing but the colour, as before.
    pub fn default_skin_from(&mut self, token: ThemeToken, from: ThemeToken) -> &mut Self {
        self.skins.default(token, Follows::From(from));
        self
    }

    /// Removes the app's skin for `token`, so the sheet's applies again, or
    /// the flat colour if the sheet has none.
    pub fn clear_skin(&mut self, token: &ThemeToken) -> &mut Self {
        self.skins.overrides.remove(token);
        self
    }

    /// Sets how `state` shades tokens that have no value of their own in it,
    /// such as making every hover colour lighter.
    pub fn set_state_shade(&mut self, state: WidgetState, shade: Shade) -> &mut Self {
        self.state_shades.overrides.insert(state, shade);
        self
    }

    fn state_shade(&self, state: WidgetState) -> Shade {
        self.state_shades
            .get(&state)
            .copied()
            .unwrap_or_else(|| default_state_shade(state))
    }

    /// Makes `sheet` the active sheet, replacing the last one entirely: a
    /// value only the old sheet set goes back to its default. Values set in
    /// code stay on top.
    ///
    /// Logs a warning for every entry nothing reads: a token no plugin gave a
    /// default of that kind, which is usually a typo or a value in the wrong
    /// section. A game's own tokens count once it registers defaults for them.
    pub fn set_sheet(&mut self, sheet: ThemeSheet) -> &mut Self {
        for entry in self.unread_entries(&sheet) {
            warn!("{entry}");
        }
        self.colors.sheet = sheet.colors;
        self.sizes.sheet = sheet.sizes;
        self.fonts.sheet = sheet.fonts;
        self.skins.sheet = sheet.skins;
        self.state_shades.sheet = sheet.state_shades;
        self
    }

    /// Removes the active sheet, so plugin defaults apply again under the app's values.
    pub fn clear_sheet(&mut self) -> &mut Self {
        self.set_sheet(ThemeSheet::default())
    }

    /// Every default as one sheet: what the theme looks like with no sheet
    /// and no app values. The reference theme file is written from this.
    #[cfg(test)]
    pub(super) fn defaults_sheet(&self) -> ThemeSheet {
        let state_shades = WidgetState::ALL
            .into_iter()
            .map(|state| {
                let shade = self.state_shades.defaults.get(&state).copied();
                (state, shade.unwrap_or_else(|| default_state_shade(state)))
            })
            .collect();
        ThemeSheet {
            colors: self.colors.defaults.clone(),
            sizes: self.sizes.defaults.clone(),
            fonts: self.fonts.defaults.clone(),
            skins: self.skins.defaults.clone(),
            state_shades,
        }
    }

    /// Describes each entry in `sheet` that names a token nothing reads.
    pub(super) fn unread_entries(&self, sheet: &ThemeSheet) -> Vec<String> {
        // A token in a state is read whenever its resting token is.
        let resting = |token: &ThemeToken| match token.split_state() {
            Some((_, resting)) => resting,
            None => token.clone(),
        };
        let is_color = |token: &ThemeToken| self.colors.defaults.contains_key(&resting(token));
        let is_size = |token: &ThemeToken| self.sizes.defaults.contains_key(token);
        let is_font = |token: &ThemeToken| self.fonts.defaults.contains_key(token);
        let what_it_is = |token: &ThemeToken| {
            if is_color(token) {
                "; it's a color"
            } else if is_size(token) {
                "; it's a size"
            } else if is_font(token) {
                "; it's a font"
            } else {
                ""
            }
        };

        let mut unread = Vec::new();
        let mut check = |action: &str, token: &ThemeToken, kind: &str, known: bool| {
            if !known {
                let hint = what_it_is(token);
                unread.push(format!(
                    "theme sheet {action} `{token}`, but no {kind} token has that name{hint}"
                ));
            }
        };
        for (token, source) in &sheet.colors {
            check("sets color", token, "color", is_color(token));
            if let ColorSource::Derived { from, .. } = source {
                check("derives a color from", from, "color", is_color(from));
            }
        }
        for (token, source) in &sheet.sizes {
            check("sets size", token, "size", is_size(token));
            if let SizeSource::From { from, .. } = source {
                check("makes a size follow", from, "size", is_size(from));
            }
        }
        for (token, source) in &sheet.fonts {
            check("sets font", token, "font", is_font(token));
            if let Follows::From(from) = source {
                check("makes a font follow", from, "font", is_font(from));
            }
        }
        // A skin is drawn in place of a colour, so it needs a colour token.
        for (token, skin) in &sheet.skins {
            check("skins", token, "color", is_color(token));
            match skin {
                Follows::Value(skin) => {
                    for from in skin.colors().filter_map(SkinColor::token) {
                        check("colours a skin from", from, "color", is_color(from));
                    }
                }
                Follows::From(from) => check("makes a skin follow", from, "color", is_color(from)),
            }
        }
        unread.sort();
        unread
    }

    /// Follows `token`'s chain of derivations down to a colour.
    fn resolve(&self, token: &ThemeToken, depth: usize) -> Result<Color, Unresolved> {
        if depth > MAX_CHAIN {
            return Err(Unresolved::Cycle(token.clone()));
        }
        match self.colors.get(token) {
            Some(ColorSource::Value(color)) => Ok(*color),
            Some(ColorSource::Derived { from, shade }) => {
                Ok(shade.apply(self.resolve(from, depth + 1)?))
            }
            None => {
                let (state, stateless) = token
                    .split_state()
                    .ok_or_else(|| Unresolved::Missing(token.clone()))?;
                let base = self.resolve(&stateless, depth + 1)?;
                Ok(self.state_shade(state).apply(base))
            }
        }
    }
}

/// A skin as a node shows it: drawn for a token, which its colours resolve against.
pub(super) struct Painted<'a> {
    /// The skin.
    pub(super) skin: &'a Skin,
    theme: &'a Theme,
    /// The colour of the token it's drawn for, state included.
    own: Color,
    /// The state shade, when the skin was borrowed from the resting token.
    borrowed: Option<Shade>,
}

impl Painted<'_> {
    /// What `color` is, drawn for this token. The token's own colour carries
    /// its state; any other colour in a borrowed skin is shaded by the state.
    pub(super) fn color(&self, color: &SkinColor) -> Color {
        let unshaded = match color {
            SkinColor::Own(shade) => return shade.apply(self.own),
            SkinColor::Derived { from, shade } => shade.apply(self.theme.color(from)),
            SkinColor::Value(color) => *color,
        };
        self.borrowed
            .map_or(unshaded, |state| state.apply(unshaded))
    }

    /// What the picture is multiplied by, if there is one.
    pub(super) fn tint(&self) -> Option<Color> {
        let picture = self.skin.picture.as_ref()?;
        Some(self.color(&picture.tint))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: ThemeToken = ThemeToken::new_static("test.base");
    const BG: ThemeToken = ThemeToken::new_static("test.bg");
    const BG_HOVER: ThemeToken = ThemeToken::new_static("test.bg.hover");
    const RED: Color = Color::srgb(1.0, 0.0, 0.0);
    const BLUE: Color = Color::srgb(0.0, 0.0, 1.0);
    const YELLOW: Color = Color::srgb(1.0, 0.9, 0.2);
    const NAVY: Color = Color::srgb(0.05, 0.08, 0.3);
    const SHADE: Shade = Shade::Lighter(0.1);

    /// A widget plugin's defaults must not overwrite values the app already set.
    #[test]
    fn defaults_never_override_app_values() {
        let mut theme = Theme::default();
        theme.set_color(BG, RED).default_color(BG, BLUE);
        assert_eq!(theme.color(&BG), RED);
    }

    /// The point of deriving: change the base, and everything built on it follows.
    #[test]
    fn derived_colors_follow_their_base() {
        let mut theme = Theme::default();
        theme
            .default_color(BG, RED)
            .default_derived(BG_HOVER, BG, SHADE);
        assert_eq!(theme.color(&BG_HOVER), SHADE.apply(RED));

        theme.set_color(BG, BLUE);
        assert_eq!(theme.color(&BG_HOVER), SHADE.apply(BLUE));
    }

    /// Shades step perceived lightness, so a dark colour pressed is darker
    /// still, not black, and lighter and darker by the same step undo each other.
    #[test]
    fn a_dark_colour_shaded_darker_is_darker_not_black() {
        let control = Color::srgb_u8(0x42, 0x47, 0x54);
        let luminance = |color: Color| LinearRgba::from(color).luminance();
        let pressed = Shade::Darker(0.08).apply(control);
        assert!(luminance(pressed) > 0.01 && luminance(pressed) < luminance(control));
        let back = Shade::Lighter(0.08).apply(pressed);
        let apart = LinearRgba::from(back).to_vec3() - LinearRgba::from(control).to_vec3();
        assert!(apart.length() < 0.002);
    }

    #[test]
    fn chains_apply_every_shade_in_order() {
        let mut theme = Theme::default();
        theme
            .default_color(BASE, RED)
            .default_derived(BG, BASE, Shade::Darker(0.2))
            .default_derived(BG_HOVER, BG, Shade::Alpha(0.5));
        assert_eq!(
            theme.color(&BG_HOVER),
            Shade::Darker(0.2).apply(RED).with_alpha(0.5)
        );
    }

    /// An override replaces a rule, and clearing it brings the rule back.
    #[test]
    fn overrides_win_and_clearing_restores_the_default() {
        let mut theme = Theme::default();
        theme
            .default_color(BG, RED)
            .default_derived(BG_HOVER, BG, SHADE);

        theme.set_color(BG_HOVER, BLUE);
        assert_eq!(theme.color(&BG_HOVER), BLUE);

        theme.clear_color(&BG_HOVER);
        assert_eq!(theme.color(&BG_HOVER), SHADE.apply(RED));
    }

    /// The app can re-point a derived token at a different base.
    #[test]
    fn apps_can_override_with_their_own_rule() {
        let mut theme = Theme::default();
        theme
            .default_color(BASE, RED)
            .default_color(BG, BLUE)
            .default_derived(BG_HOVER, BG, SHADE)
            .set_derived(BG_HOVER, BASE, Shade::Same);
        assert_eq!(theme.color(&BG_HOVER), RED);
    }

    #[test]
    fn a_missing_base_shows_as_missing() {
        let mut theme = Theme::default();
        theme.default_derived(BG_HOVER, BG, SHADE);
        assert_eq!(theme.color(&BG_HOVER), MISSING_COLOR);
    }

    /// A loop of rules must not hang or overflow the stack.
    #[test]
    fn cycles_show_as_missing() {
        let mut theme = Theme::default();
        theme
            .default_derived(BG, BG_HOVER, SHADE)
            .default_derived(BG_HOVER, BG, SHADE);
        assert_eq!(theme.color(&BG), MISSING_COLOR);
    }

    /// A state needs no rule: the theme's shade for it applies to the base.
    #[test]
    fn states_fall_back_to_the_theme_shade() {
        let mut theme = Theme::default();
        theme.default_color(BG, RED);
        let expectations = [
            (WidgetState::Hover, DEFAULT_HOVER_SHADE),
            (WidgetState::Focused, DEFAULT_FOCUSED_SHADE),
            (WidgetState::Pressed, DEFAULT_PRESSED_SHADE),
            (WidgetState::Disabled, DEFAULT_DISABLED_SHADE),
        ];
        for (state, shade) in expectations {
            assert_eq!(theme.color(&BG.with_state(state)), shade.apply(RED));
        }
    }

    /// Exceptions (a ghost button's hover, say) beat the generic shade,
    /// whether a plugin registers them or the app sets them.
    #[test]
    fn explicit_state_values_beat_the_theme_shade() {
        let hover = BG.with_state(WidgetState::Hover);
        let mut theme = Theme::default();
        theme
            .default_color(BG, RED)
            .default_color(BASE, BLUE)
            .default_derived(hover.clone(), BASE, Shade::Same);
        assert_eq!(theme.color(&hover), BLUE);

        theme.set_color(hover.clone(), YELLOW);
        assert_eq!(theme.color(&hover), YELLOW);
    }

    #[test]
    fn changing_a_state_shade_restyles_every_token_in_that_state() {
        let mut theme = Theme::default();
        theme
            .default_color(BG, RED)
            .default_color(BASE, BLUE)
            .set_state_shade(WidgetState::Hover, Shade::Darker(0.3));
        for token in [BG, BASE] {
            let base = theme.color(&token);
            let hover = theme.color(&token.with_state(WidgetState::Hover));
            assert_eq!(hover, Shade::Darker(0.3).apply(base));
        }
    }

    /// A state of a missing token is missing too, not a shaded magenta.
    #[test]
    fn a_state_of_a_missing_token_is_missing() {
        let theme = Theme::default();
        assert_eq!(
            theme.color(&BG.with_state(WidgetState::Hover)),
            MISSING_COLOR
        );
    }

    fn skin() -> Skin {
        Skin::new(Handle::default())
    }

    const ROLE: ThemeToken = ThemeToken::new_static("test.role");

    /// A theme whose BG follows ROLE, as a widget's part follows a skin role.
    fn following() -> Theme {
        let mut theme = Theme::default();
        theme
            .default_color(ROLE, BLUE)
            .default_color(BG, RED)
            .default_skin_from(BG, ROLE);
        theme
    }

    /// A role with no skin leaves its followers flat, as before roles.
    #[test]
    fn following_a_role_without_a_skin_draws_the_colour() {
        let theme = following();
        assert!(theme.skin(&BG).is_none());
        assert!(theme.skin(&BG.with_state(WidgetState::Hover)).is_none());
    }

    /// The role's picture, tinted by the follower's own colour, not the role's.
    #[test]
    fn a_role_skin_is_tinted_by_its_follower() {
        let mut theme = following();
        theme.set_skin(ROLE, skin());
        assert_eq!(theme.skin(&BG).unwrap().tint(), Some(RED));
        let hover = BG.with_state(WidgetState::Hover);
        let shaded = DEFAULT_HOVER_SHADE.apply(RED);
        assert_eq!(theme.skin(&hover).unwrap().tint(), Some(shaded));
    }

    /// A role's own pressed skin reaches every pressed follower.
    #[test]
    fn a_role_state_skin_reaches_its_followers_in_that_state() {
        let mut theme = following();
        let pressed_art = Skin::new(Handle::default()).sliced(3.0);
        theme
            .set_skin(ROLE, skin())
            .set_skin(ROLE.with_state(WidgetState::Pressed), pressed_art.clone());
        let pressed = BG.with_state(WidgetState::Pressed);
        assert_eq!(*theme.skin(&pressed).unwrap().skin, pressed_art);
        assert_eq!(*theme.skin(&BG).unwrap().skin, skin());
    }

    /// A follower's own skin beats its role's.
    #[test]
    fn a_skin_of_its_own_beats_the_role() {
        let mut theme = following();
        let own = Skin::new(Handle::default()).sliced(2.0);
        theme.set_skin(ROLE, skin()).set_skin(BG, own.clone());
        assert_eq!(*theme.skin(&BG).unwrap().skin, own);
    }

    #[test]
    fn skins_following_in_a_loop_draw_nothing() {
        let mut theme = Theme::default();
        theme
            .default_color(BG, RED)
            .set_skin_from(BG, ROLE)
            .set_skin_from(ROLE, BG);
        assert!(theme.skin(&BG).is_none());
    }

    /// A tinted skin takes its token's colour, state shades included.
    #[test]
    fn states_borrow_the_resting_skin_and_tint_it() {
        let mut theme = Theme::default();
        theme.default_color(BG, RED).set_skin(BG, skin());
        let hover = BG.with_state(WidgetState::Hover);
        let painted = theme.skin(&hover).unwrap();
        assert_eq!(painted.skin, &skin());
        assert_eq!(painted.tint(), Some(DEFAULT_HOVER_SHADE.apply(RED)));
    }

    /// Untinted art keeps its colours at rest, but pressing still darkens it.
    #[test]
    fn untinted_skins_are_shaded_only_by_state() {
        let mut theme = Theme::default();
        theme.set_skin(BG, skin().untinted());
        assert_eq!(theme.skin(&BG).unwrap().tint(), Some(Color::WHITE));
        let pressed = BG.with_state(WidgetState::Pressed);
        assert_eq!(
            theme.skin(&pressed).unwrap().tint(),
            Some(DEFAULT_PRESSED_SHADE.apply(Color::WHITE))
        );
    }

    /// A pressed frame drawn for the purpose is shown as is, not darkened again.
    #[test]
    fn a_state_with_its_own_skin_uses_it_as_authored() {
        let pressed = BG.with_state(WidgetState::Pressed);
        let frame = Skin::new(Handle::default()).sliced(4.0).untinted();
        let mut theme = Theme::default();
        theme
            .set_skin(BG, skin())
            .set_skin(pressed.clone(), frame.clone());
        let painted = theme.skin(&pressed).unwrap();
        assert_eq!((painted.skin, painted.tint()), (&frame, Some(Color::WHITE)));
    }

    /// Every colour in a borrowed skin shows the state: the token's own
    /// carries it, and the rest are shaded by it.
    #[test]
    fn a_borrowed_skin_shades_every_colour_by_the_state() {
        const ACCENT: ThemeToken = ThemeToken::new_static("test.accent");
        let mut theme = Theme::default();
        theme.default_color(BG, RED).default_color(ACCENT, BLUE);
        theme.set_skin(BG, Skin::default());
        let accent = SkinColor::Derived {
            from: ACCENT,
            shade: Shade::Lighter(0.2),
        };
        let lit = Shade::Lighter(0.2).apply(BLUE);

        let resting = theme.skin(&BG).unwrap();
        assert_eq!(resting.color(&SkinColor::OWN), RED);
        assert_eq!(resting.color(&accent), lit);
        assert_eq!(resting.color(&Color::BLACK.into()), Color::BLACK);

        let hover = BG.with_state(WidgetState::Hover);
        let hovered = theme.skin(&hover).unwrap();
        assert_eq!(
            hovered.color(&SkinColor::OWN),
            DEFAULT_HOVER_SHADE.apply(RED)
        );
        assert_eq!(hovered.color(&accent), DEFAULT_HOVER_SHADE.apply(lit));
        assert_eq!(
            hovered.color(&Color::BLACK.into()),
            DEFAULT_HOVER_SHADE.apply(Color::BLACK)
        );
    }

    #[test]
    fn only_pictures_count_as_skinned() {
        let mut theme = Theme::default();
        theme.set_skin(BG, Skin::default().fill(Color::BLACK));
        assert!(!theme.is_skinned(&BG));
        theme.set_skin(BG, skin());
        assert!(theme.is_skinned(&BG));
    }

    #[test]
    fn tokens_without_skins_have_none() {
        let mut theme = Theme::default();
        theme.default_color(BG, RED);
        assert!(theme.skin(&BG).is_none());

        theme.set_skin(BG, skin()).clear_skin(&BG);
        assert!(theme.skin(&BG.with_state(WidgetState::Hover)).is_none());
    }

    #[test]
    fn contrast_picks_readable_text() {
        assert_eq!(Shade::Contrast.apply(YELLOW), CONTRAST_DARK);
        assert_eq!(Shade::Contrast.apply(NAVY), CONTRAST_LIGHT);
    }

    const ACCENT: ThemeToken = ThemeToken::new_static("test.accent");

    /// Code beats the sheet, which beats plugin defaults.
    #[test]
    fn layers_resolve_app_then_sheet_then_defaults() {
        let mut theme = Theme::default();
        theme.default_color(BG, RED);
        theme.set_sheet(ThemeSheet::default().color(BG, BLUE));
        assert_eq!(theme.color(&BG), BLUE);

        theme.set_color(BG, YELLOW);
        assert_eq!(theme.color(&BG), YELLOW);

        theme.clear_color(&BG);
        assert_eq!(
            theme.color(&BG),
            BLUE,
            "clearing the app's value uncovers the sheet"
        );
    }

    /// Switching themes must not leave the old theme's values behind.
    #[test]
    fn a_new_sheet_replaces_the_old_one_entirely() {
        let mut theme = Theme::default();
        theme.default_color(BG, RED).default_color(ACCENT, RED);
        theme.set_sheet(ThemeSheet::default().color(BG, BLUE).color(ACCENT, BLUE));
        theme.set_sheet(ThemeSheet::default().color(ACCENT, YELLOW));
        assert_eq!(theme.color(&BG), RED, "only the old sheet set it");
        assert_eq!(theme.color(&ACCENT), YELLOW);

        theme.set_color(ACCENT, NAVY).clear_sheet();
        assert_eq!(
            theme.color(&ACCENT),
            NAVY,
            "the app's values survive a switch"
        );
        assert_eq!(theme.color(&BG), RED);
    }

    /// A sheet that only sets a base colour still restyles its derived and state tokens.
    #[test]
    fn sheet_values_flow_through_derivations_and_states() {
        let mut theme = Theme::default();
        theme
            .default_color(ACCENT, RED)
            .default_derived(BG, ACCENT, SHADE);
        theme.set_sheet(
            ThemeSheet::default()
                .color(ACCENT, BLUE)
                .state_shade(WidgetState::Hover, Shade::Darker(0.3)),
        );
        let hover = BG.with_state(WidgetState::Hover);
        assert_eq!(
            theme.color(&hover),
            Shade::Darker(0.3).apply(SHADE.apply(BLUE))
        );
    }

    #[test]
    fn sheets_carry_sizes_and_skins() {
        let mut theme = Theme::default();
        theme.default_size(BG, 4.0);
        theme.set_sheet(ThemeSheet::default().size(BG, 8.0).skin(BG, skin()));
        assert_eq!(theme.size(&BG), 8.0);
        assert!(theme.skin(&BG).is_some());

        theme.set_skin(BG, skin().untinted()).clear_sheet();
        assert_eq!(theme.size(&BG), 4.0);
        assert!(
            theme.skin(&BG).unwrap().tint() == Some(Color::WHITE),
            "the app's skin survives"
        );
    }

    const FAMILY: ThemeToken = ThemeToken::new_static("test.family.size");
    const MEMBER: ThemeToken = ThemeToken::new_static("test.member.size");

    #[test]
    fn sizes_follow_their_base_until_given_their_own() {
        let mut theme = Theme::default();
        theme
            .default_size(FAMILY, 13.0)
            .default_size_from(MEMBER, FAMILY);
        assert_eq!(theme.size(&MEMBER), 13.0);

        theme.set_size(FAMILY, 16.0);
        assert_eq!(theme.size(&MEMBER), 16.0);

        theme.set_size(MEMBER, 20.0);
        assert_eq!((theme.size(&FAMILY), theme.size(&MEMBER)), (16.0, 20.0));

        theme.clear_size(&MEMBER);
        assert_eq!(theme.size(&MEMBER), 16.0);
    }

    /// Scales multiply along a chain, so a gap at twice the radius follows
    /// the radius, and anything at half the gap follows both.
    #[test]
    fn scaled_sizes_multiply_along_the_chain() {
        let mut theme = Theme::default();
        theme
            .default_size(FAMILY, 4.0)
            .default_size_scaled(MEMBER, FAMILY, 2.0)
            .default_size_scaled(BG, MEMBER, 0.5);
        assert_eq!(theme.size(&MEMBER), 8.0);
        assert_eq!(theme.size(&BG), 4.0);
        theme.set_size(FAMILY, 6.0);
        assert_eq!((theme.size(&MEMBER), theme.size(&BG)), (12.0, 6.0));
    }

    #[test]
    fn sheets_can_make_sizes_follow() {
        let mut theme = Theme::default();
        theme.default_size(FAMILY, 13.0).default_size(MEMBER, 9.0);
        theme.set_sheet(ThemeSheet::default().size_from(MEMBER, FAMILY));
        assert_eq!(theme.size(&MEMBER), 13.0);
    }

    #[test]
    fn sizes_in_a_loop_or_missing_fall_back() {
        let mut theme = Theme::default();
        theme
            .default_size_from(FAMILY, MEMBER)
            .default_size_from(MEMBER, FAMILY);
        assert_eq!(theme.size(&MEMBER), MISSING_SIZE);
        assert_eq!(theme.size(&BG), MISSING_SIZE);
    }

    /// Typos and values in the wrong section are reported, so a theme
    /// author isn't left wondering why an entry does nothing.
    #[test]
    fn entries_nothing_reads_are_reported() {
        let mut theme = Theme::default();
        theme.default_color(BG, RED).default_size(FAMILY, 13.0);
        let sheet = ThemeSheet::default()
            .color(BG, BLUE)
            .color(BG.with_state(WidgetState::Hover), BLUE)
            .derived(BASE, BG, SHADE)
            .derived(BG.with_state(WidgetState::Pressed), MEMBER, SHADE)
            .color(FAMILY, BLUE)
            .size(FAMILY, 16.0)
            .size_from(MEMBER, FAMILY)
            .size_from(FAMILY, BASE)
            .skin(
                BG,
                skin().tint(SkinColor::Derived {
                    from: BASE,
                    shade: SHADE,
                }),
            )
            .skin(MEMBER, skin());
        assert_eq!(
            theme.unread_entries(&sheet),
            [
                "theme sheet colours a skin from `test.base`, but no color token has that name",
                "theme sheet derives a color from `test.member.size`, but no color token has that name",
                "theme sheet makes a size follow `test.base`, but no size token has that name",
                "theme sheet sets color `test.base`, but no color token has that name",
                "theme sheet sets color `test.family.size`, but no color token has that name; it's a size",
                "theme sheet sets size `test.member.size`, but no size token has that name",
                "theme sheet skins `test.member.size`, but no color token has that name",
            ]
        );
    }

    const BODY: ThemeToken = ThemeToken::new_static("test.font");
    const LABEL: ThemeToken = ThemeToken::new_static("test.label.font");

    /// A font handle that isn't Bevy's own, standing in for a loaded file.
    fn file_font() -> Handle<Font> {
        bevy::asset::uuid_handle!("6e5b2f1c-1c1d-4a8e-9f3b-6a2d7c9e0b11")
    }

    #[test]
    fn fonts_follow_and_wait_for_their_file() {
        let mut theme = Theme::default();
        theme
            .default_font(BODY, Handle::default())
            .default_font_from(LABEL, BODY);
        assert_eq!(theme.font(&LABEL), Handle::default());

        // Set, but not loaded yet: Bevy's font stands in.
        theme.set_font(BODY, file_font());
        assert_eq!(theme.font(&LABEL), Handle::default());

        theme.set_font_loaded(file_font().id(), true);
        assert_eq!(theme.font(&LABEL), file_font());

        theme.set_font_loaded(file_font().id(), false);
        assert_eq!(theme.font(&LABEL), Handle::default());
    }

    #[test]
    fn sheets_carry_fonts_and_report_unread_ones() {
        let mut theme = Theme::default();
        theme
            .default_font(BODY, Handle::default())
            .default_size(FAMILY, 1.0);
        theme.set_font_loaded(file_font().id(), true);
        let sheet = ThemeSheet::default()
            .font(BODY, file_font())
            .font_from(LABEL, BODY)
            .font(FAMILY, file_font());
        assert_eq!(
            theme.unread_entries(&sheet),
            [
                "theme sheet sets font `test.family.size`, but no font token has that name; it's a size",
                "theme sheet sets font `test.label.font`, but no font token has that name",
            ]
        );
        theme.set_sheet(sheet);
        assert_eq!(theme.font(&BODY), file_font());
        theme.clear_sheet();
        assert_eq!(theme.font(&BODY), Handle::default());
    }
}
