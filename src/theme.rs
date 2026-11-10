//! Theming for touchy widgets.
//!
//! A [`ThemeToken`] names a themed value, such as `"touchy.button.bg"`. Widgets
//! attach binding components that say which token each visual property uses
//! ([`ThemeSkin`], [`ThemeBorder`], [`ThemeTextColor`], [`ThemeFont`],
//! [`ThemeFontSize`], [`ThemeCornerRadius`]). The app's [`Theme`] resource supplies the value
//! behind every token.
//!
//! Widgets express state by swapping tokens: a hovered button re-inserts
//! `ThemeSkin(BG.with_state(WidgetState::Hover))`. Nothing here knows
//! which widgets exist, so adding a widget never means editing this module.
//!
//! The pieces live in submodules, re-exported here:
//! - `token`: what a token is, including its interaction state ([`WidgetState`]).
//! - `resolve`: how a token gets its value, including colours derived from
//!   other tokens ([`Shade`]).
//! - `sheet`: a theme as one set of values ([`ThemeSheet`]), layered between
//!   plugin defaults and the app's own values.
//! - `file`: the `.theme.ron` format a sheet is written in.
//! - `asset`: loading theme files and choosing the active one
//!   ([`ThemeSheetPlugin`], [`ActiveTheme`]).
//! - `skin`: pictures drawn in place of flat colours ([`Skin`]).
//! - `binding`: how values reach components.
//!
//! The public [`palette`] module holds the semantic colours (accent, surface,
//! text, ...) that every widget's defaults derive from, and [`skins`] the skin
//! roles (surface, raised, sunken, ...) that every widget's skins follow.

use bevy::prelude::*;

mod asset;
mod binding;
mod file;
mod focus;
pub mod palette;
#[cfg(test)]
mod reference;
mod resolve;
mod sheet;
mod skin;
pub mod skins;
mod token;
mod transition;

pub use asset::{ActiveTheme, ThemeSheetPlugin};
pub use binding::{
    ThemeAppExt, ThemeBinding, ThemeBorder, ThemeCornerRadius, ThemeFont, ThemeFontSize, ThemeSkin,
    ThemeTextColor,
};
pub use focus::FocusVisible;
pub use resolve::{Shade, Theme};
pub use sheet::ThemeSheet;
pub use skin::{
    Skin, SkinColor, SkinFrame, SkinFrames, SkinGradient, SkinOutline, SkinPicture, SkinShadow,
    SkinStop, SkinTransition,
};
pub use token::{ParseTokenError, ThemeToken, WidgetFlags, WidgetState};

/// Adds the [`Theme`] resource, with the palette's defaults, and touchy's
/// built-in bindings.
///
/// Every touchy plugin brings it, and so may the game, as often and in any
/// order: only the first does the work, so a theme set up before it is kept.
pub struct ThemePlugin;

/// Notes that [`ThemePlugin`] has done its work, so another copy does nothing.
#[derive(Resource)]
struct ThemePluginBuilt;

impl Plugin for ThemePlugin {
    fn build(&self, app: &mut App) {
        if app.world().contains_resource::<ThemePluginBuilt>() {
            return;
        }
        app.insert_resource(ThemePluginBuilt)
            .init_resource::<Theme>();
        let mut theme = app.world_mut().resource_mut::<Theme>();
        palette::default_palette(&mut theme);
        skins::default_roles(&mut theme);
        // A skin eases into a new state, so it has an insert observer of its own.
        app.add_observer(transition::transition_on_insert)
            .register_theme_binding::<ThemeBorder>()
            .register_theme_binding::<ThemeTextColor>()
            .register_theme_binding::<ThemeFontSize>()
            .register_theme_binding::<ThemeFont>()
            .register_theme_binding::<ThemeCornerRadius>()
            .add_observer(binding::apply_border_on_background_insert)
            // Fonts exist only with Bevy's text and asset plugins.
            .add_systems(
                Update,
                (
                    binding::track_loaded_fonts.run_if(resource_exists::<Assets<bevy::text::Font>>),
                    // Late, so focus moved by the game's own systems shows this frame.
                    focus::mark_visible_focus.after(binding::track_loaded_fonts),
                ),
            )
            .add_systems(
                PostUpdate,
                (
                    binding::apply_on_theme_change::<ThemeSkin>.run_if(resource_changed::<Theme>),
                    transition::ease_transitions,
                    binding::show_skin_frames,
                )
                    .before(bevy::ui::UiSystems::Prepare),
            );
    }
    /// Safe to add more than once, so plugin groups can list it and games can
    /// still add it themselves.
    fn is_unique(&self) -> bool {
        false
    }
}
