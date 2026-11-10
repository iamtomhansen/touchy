//! Premade themes that ship with touchy, compiled into the game so there are no
//! files to copy. Each has a skin and a palette, which go in the two slots of
//! [`ActiveTheme`](crate::theme::ActiveTheme) and mix with any other:
//!
//! ```
//! # use bevy::prelude::*;
//! # use touchy::prelude::*;
//! # use touchy::premade;
//! fn crown_the_lords(asset_server: Res<AssetServer>, mut active: ResMut<ActiveTheme>) {
//!     active
//!         .set_skin(asset_server.load(premade::lords::SKIN))
//!         .set_palette(asset_server.load(premade::lords::PALETTE));
//! }
//! ```
//!
//! Each is a cargo feature, on by default through `premade-themes`: a game that
//! uses neither turns off default features and carries none of their pictures
//! or fonts. Touchy's default theme is no sheet at all, so it needs no feature.
//!
//! - `theme-lords`: carved wood and iron for a fantasy game, set in IM Fell English.
//! - `theme-foundation`: pixel-art panels for science fiction, set in Silkscreen.
//!
//! Their files are in the `premade_themes` folder at the crate's root, one folder
//! a theme, with their fonts' licences. Every file in a theme's folder is compiled
//! in, so a theme file can name any picture or font beside it.

use std::path::Path;

use bevy::asset::io::embedded::EmbeddedAssetRegistry;
use bevy::prelude::*;

/// Lords: carved wood and iron for a fantasy game.
#[cfg(feature = "theme-lords")]
pub mod lords {
    /// How Lords draws things: its pictures, its font and its sizes.
    pub const SKIN: &str = "embedded://touchy/premade/lords/skin.theme.ron";
    /// Lords' colours: dark oak, parchment and old gold.
    pub const PALETTE: &str = "embedded://touchy/premade/lords/palette.theme.ron";
}

/// Foundation: pixel-art panels for a science fiction game.
#[cfg(feature = "theme-foundation")]
pub mod foundation {
    /// How Foundation draws things: its pictures, its font and its sizes.
    pub const SKIN: &str = "embedded://touchy/premade/foundation/skin.theme.ron";
    /// Foundation's colours: a dark console in teal, with amber for attention.
    pub const PALETTE: &str = "embedded://touchy/premade/foundation/palette.theme.ron";
}

/// Where the premade themes' folder is, and where its files are found once embedded.
const FOLDER: &str = "premade_themes";
const EMBEDDED_UNDER: &str = "touchy/premade";

/// Every file of the enabled themes: its path in the folder, and its bytes. Written by
/// `build.rs`, which reads the folder, so there's no list here to keep in step with it.
const FILES: &[(&str, &[u8])] = include!(concat!(env!("OUT_DIR"), "/premade_themes.rs"));

/// Compiles every enabled theme's files into the game, under `embedded://touchy/premade/`.
/// Called by [`ThemeSheetPlugin`](crate::theme::ThemeSheetPlugin), which loads them.
pub(crate) fn embed(app: &mut App) {
    let registry = app.world().resource::<EmbeddedAssetRegistry>();
    let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join(FOLDER);
    for &(inside, bytes) in FILES {
        // The file's own path lets Bevy's `embedded_watcher` reload it as it's edited.
        registry.insert_asset(
            folder.join(inside),
            &Path::new(EMBEDDED_UNDER).join(inside),
            bytes,
        );
    }
}
