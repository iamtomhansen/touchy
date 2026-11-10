//! Theme files as assets: loading `.theme.ron` files and choosing which ones
//! the [`Theme`] uses.

use bevy::asset::io::Reader;
use bevy::asset::{
    AssetLoader, AssetPath, LoadContext, ParseAssetPathError, RecursiveDependencyLoadState,
};
use bevy::prelude::*;

use super::file::{self, LoadFile, ThemeFileError};
use super::{Theme, ThemePlugin, ThemeSheet};
use crate::EnsurePlugin;

/// The suffix to look for that denotes a theme file.
const EXTENSIONS: &[&str] = &["theme.ron"];

/// Loads `.theme.ron` files as [`ThemeSheet`] assets and applies the sheets in
/// [`ActiveTheme`] to the [`Theme`].
pub struct ThemeSheetPlugin;

impl Plugin for ThemeSheetPlugin {
    fn build(&self, app: &mut App) {
        app.ensure_plugin(ThemePlugin)
            .init_asset::<ThemeSheet>()
            .register_asset_loader(ThemeSheetLoader)
            .init_resource::<ActiveTheme>()
            .add_systems(Update, apply_active_sheets);
        crate::premade::embed(app);
    }
}

/// The sheets the [`Theme`] uses: a skin, and a palette over it. Each switches
/// on the fly once loaded, and either can be left empty.
///
/// ```
/// # use bevy::prelude::*;
/// # use touchy::theme::ActiveTheme;
/// fn ember_on_stone(asset_server: Res<AssetServer>, mut active: ResMut<ActiveTheme>) {
///     active.set_skin(asset_server.load("themes/stone.theme.ron"));
///     active.set_palette(asset_server.load("themes/ember.theme.ron"));
/// }
/// ```
///
/// A skin says how things are drawn: pictures, gradients, fonts and sizes. A
/// palette says what colours they're drawn in. Any sheet can hold any value,
/// though, so the slots only decide the order: the palette is laid over the
/// skin with [`ThemeSheet::lay_over`], so a skin can suggest colours of its own
/// and a palette chosen after it still wins. A game with one theme file can put
/// it in either slot.
///
/// Bevy's `file_watcher` allows live editing of a theme file. A sheet built in
/// code works too. Add it to `Assets<ThemeSheet>` and set the handle.
///
/// The sheets wait for the files they name, their skins' pictures and their
/// fonts, so the theme changes all at once rather than in pieces. A file that
/// fails to load stops the wait instead of holding the theme back for good.
#[derive(Resource, Default, Debug)]
pub struct ActiveTheme {
    skin: Option<Handle<ThemeSheet>>,
    palette: Option<Handle<ThemeSheet>>,
    /// A sheet was chosen, or a new version of one arrived, and the theme
    /// hasn't taken it yet.
    pending: bool,
}

impl ActiveTheme {
    /// Switches the skin to `sheet` once it has loaded.
    pub fn set_skin(&mut self, sheet: Handle<ThemeSheet>) -> &mut Self {
        self.skin = Some(sheet);
        self.pending = true;
        self
    }

    /// Switches the palette to `sheet` once it has loaded.
    pub fn set_palette(&mut self, sheet: Handle<ThemeSheet>) -> &mut Self {
        self.palette = Some(sheet);
        self.pending = true;
        self
    }

    /// Removes the skin, uncovering the plugin defaults under the palette.
    pub fn clear_skin(&mut self) -> &mut Self {
        self.skin = None;
        self.pending = true;
        self
    }

    /// Removes the palette, uncovering the skin's colours, or the defaults.
    pub fn clear_palette(&mut self) -> &mut Self {
        self.palette = None;
        self.pending = true;
        self
    }

    /// Removes both sheets, uncovering the plugin defaults.
    pub fn clear(&mut self) -> &mut Self {
        self.clear_skin().clear_palette()
    }

    /// The skin's handle, if any.
    pub fn skin(&self) -> Option<&Handle<ThemeSheet>> {
        self.skin.as_ref()
    }

    /// The palette's handle, if any.
    pub fn palette(&self) -> Option<&Handle<ThemeSheet>> {
        self.palette.as_ref()
    }

    /// The chosen sheets, bottom first.
    fn sheets(&self) -> impl Iterator<Item = &Handle<ThemeSheet>> {
        self.skin.iter().chain(&self.palette)
    }
}

/// Whether the asset server has finished with `sheet` and every file it
/// names, loaded or failed. A sheet built in code was never loaded, so it's ready.
fn finished_loading(server: &AssetServer, sheet: &Handle<ThemeSheet>) -> bool {
    match server.get_recursive_dependency_load_state(sheet) {
        None => true,
        Some(state) => matches!(
            state,
            RecursiveDependencyLoadState::Loaded | RecursiveDependencyLoadState::Failed(_)
        ),
    }
}

/// Updates the Theme resource with the active sheets, laid one over the other,
/// when one is chosen or a new version arrives, once every one of them and
/// their files have finished loading.
fn apply_active_sheets(
    mut active: ResMut<ActiveTheme>,
    mut events: MessageReader<AssetEvent<ThemeSheet>>,
    sheets: Res<Assets<ThemeSheet>>,
    server: Res<AssetServer>,
    mut theme: ResMut<Theme>,
) {
    // Drain the queue of messages, noting any for the active sheets.
    let mut arrived = false;
    for event in events.read() {
        arrived |= active
            .sheets()
            .any(|handle| event.is_added(handle) || event.is_modified(handle));
    }
    if arrived && !active.pending {
        active.pending = true;
    }
    if !active.pending {
        return;
    }
    let mut combined = ThemeSheet::default();
    for handle in active.sheets() {
        // Not ready yet: keep the current sheets, and look again next frame.
        let Some(sheet) = sheets.get(handle) else {
            return;
        };
        if !finished_loading(&server, handle) {
            return;
        }
        combined.lay_over(sheet);
    }
    theme.set_sheet(combined);
    active.pending = false;
}

/// Reads `.theme.ron` files. Skin images load as dependencies of the sheet,
/// with paths relative to the theme file.
#[derive(TypePath)]
struct ThemeSheetLoader;

impl AssetLoader for ThemeSheetLoader {
    type Asset = ThemeSheet;
    type Settings = ();
    type Error = ThemeFileError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &(),
        load_context: &mut LoadContext<'_>,
    ) -> Result<ThemeSheet, ThemeFileError> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        let sheet = load_context.path().clone();
        file::read_sheet(
            &bytes,
            &mut NextToSheet {
                context: load_context,
                sheet,
            },
        )
    }

    fn extensions(&self) -> &[&str] {
        EXTENSIONS
    }
}

/// Loads the files a theme file names as part of it. A path is read from the
/// theme file's own folder, or from the asset folder's root if it starts with
/// `/`. In `assets/themes/ember.theme.ron`:
///
/// ```ron
/// (
///     skins: { "touchy.palette.control": (image: "panel.png", slice: 4.0) },
///     fonts: { "touchy.palette.font": "/fonts/Mono.ttf" },
/// )
/// ```
///
/// loads `assets/themes/panel.png` and `assets/fonts/Mono.ttf`.
struct NextToSheet<'a, 'b> {
    context: &'a mut LoadContext<'b>,
    sheet: AssetPath<'static>,
}

impl LoadFile for NextToSheet<'_, '_> {
    fn load<A: Asset>(&mut self, path: &str) -> Result<Handle<A>, ParseAssetPathError> {
        Ok(self.context.load(self.sheet.resolve_embed_str(path)?))
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::time::{Duration, Instant};

    use bevy::asset::io::memory::{Dir, MemoryAssetReader};
    use bevy::asset::io::{AssetSourceBuilder, AssetSourceId};
    use bevy::image::{CompressedImageFormats, ImageLoader, ImagePlugin};
    use bevy::text::{Font, FontLoader, FontSource};

    use super::super::resolve::Follows;
    use super::*;
    use crate::theme::{Shade, ThemeFont, WidgetState, palette};

    /// How long a test waits for a file to load before failing.
    const LOAD_TIMEOUT: Duration = Duration::from_secs(10);

    const EMBER: &str = "themes/ember.theme.ron";
    const MOSS: &str = "themes/moss.theme.ron";

    const EMBER_ACCENT: Color = Color::srgb_u8(0xee, 0x73, 0x40);
    const MOSS_ACCENT: Color = Color::srgb_u8(0x4f, 0x8a, 0x3c);

    /// A warm accent, a derived highlight, a hover shade, and a skin whose
    /// picture would sit next to the file (the tests never provide it).
    const EMBER_TEXT: &str = r##"(
        colors: {
            "touchy.palette.accent": "#ee7340",
            "touchy.palette.highlight": (from: "touchy.palette.accent", shade: Lighter(0.2)),
        },
        skins: { "touchy.palette.control": (image: "panel.png", slice: 4.0) },
        state_shades: { "hover": Lighter(0.12) },
    )"##;
    const MOSS_TEXT: &str = r##"(colors: { "touchy.palette.accent": "#4f8a3c" })"##;
    const BROKEN_TEXT: &str = r#"(colors: { "touchy.palette.accent": "orange" })"#;

    /// An app whose asset folder is `dir`, held in memory so the files live
    /// in the tests that use them.
    fn app() -> (App, Dir) {
        let dir = Dir::default();
        let root = dir.clone();
        let mut app = App::new();
        app.register_asset_source(
            AssetSourceId::Default,
            AssetSourceBuilder::new(move || Box::new(MemoryAssetReader { root: root.clone() })),
        )
        .add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            ImagePlugin::default(),
            ThemeSheetPlugin,
        ))
        // The renderer registers the PNG loader, and Bevy's text plugin the
        // font loader, in a real app.
        .register_asset_loader(ImageLoader::new(CompressedImageFormats::NONE))
        .init_asset::<Font>()
        .init_asset_loader::<FontLoader>();
        app.update();
        (app, dir)
    }

    fn write(dir: &Dir, path: &str, text: &str) {
        dir.insert_asset_text(Path::new(path), text);
    }

    fn accent(app: &App) -> Color {
        app.world().resource::<Theme>().color(&palette::ACCENT)
    }

    fn set_active(app: &mut App, path: &'static str) -> Handle<ThemeSheet> {
        let handle = app.world().resource::<AssetServer>().load(path);
        app.world_mut()
            .resource_mut::<ActiveTheme>()
            .set_palette(handle.clone());
        handle
    }

    /// Runs frames until `done`, failing the test if it takes too long.
    fn update_until(app: &mut App, mut done: impl FnMut(&mut App) -> bool) {
        let start = Instant::now();
        while !done(app) {
            assert!(
                start.elapsed() < LOAD_TIMEOUT,
                "timed out waiting for assets"
            );
            app.update();
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// An app showing the ember sheet.
    fn ember_app() -> (App, Dir, Handle<ThemeSheet>) {
        let (mut app, dir) = app();
        write(&dir, EMBER, EMBER_TEXT);
        let handle = set_active(&mut app, EMBER);
        update_until(&mut app, |app| accent(app) == EMBER_ACCENT);
        (app, dir, handle)
    }

    #[test]
    fn a_loaded_sheet_restyles_the_theme() {
        let (app, _dir, _) = ember_app();
        let theme = app.world().resource::<Theme>();
        assert_eq!(
            theme.color(&palette::HIGHLIGHT),
            Shade::Lighter(0.2).apply(EMBER_ACCENT)
        );
        assert_eq!(
            theme.color(&palette::ACCENT.with_state(WidgetState::Hover)),
            Shade::Lighter(0.12).apply(EMBER_ACCENT)
        );
    }

    /// The picture is missing: the sheet waits for it, then applies once it
    /// fails, so one bad image never holds back a theme for good. The failure
    /// shows in the sheet's dependency state, proving the image was loaded as
    /// part of the sheet.
    #[test]
    fn a_missing_skin_image_holds_the_sheet_back_only_until_it_fails() {
        let (app, _dir, handle) = ember_app();
        let world = app.world();
        let server = world.resource::<AssetServer>();
        let state = server.recursive_dependency_load_state(&handle);
        assert!(state.is_failed(), "applied before its files had finished");
        let theme = world.resource::<Theme>();
        let painted = theme.skin(&palette::CONTROL).expect("ember skins controls");
        let picture = painted.skin.picture.as_ref().unwrap();
        let path = server.get_path(&picture.image).unwrap();
        assert_eq!(path.path(), Path::new("themes/panel.png"));
    }

    #[test]
    fn switching_keeps_the_old_sheet_until_the_new_one_loads() {
        let (mut app, dir, _) = ember_app();
        write(&dir, MOSS, MOSS_TEXT);
        set_active(&mut app, MOSS);
        let mut saw_defaults = false;
        update_until(&mut app, |app| {
            let current = accent(app);
            saw_defaults |= current != EMBER_ACCENT && current != MOSS_ACCENT;
            current == MOSS_ACCENT
        });
        assert!(!saw_defaults, "the theme fell back to defaults mid-switch");
    }

    #[test]
    fn clearing_the_sheet_restores_the_defaults() {
        let defaults = accent(&app().0);
        let (mut app, _dir, _) = ember_app();
        app.world_mut().resource_mut::<ActiveTheme>().clear();
        app.update();
        assert_eq!(accent(&app), defaults);
    }

    /// The path `file_watcher` takes when a theme file is saved.
    #[test]
    fn a_reloaded_file_is_applied_again() {
        let (mut app, dir, _) = ember_app();
        write(&dir, EMBER, MOSS_TEXT);
        app.world().resource::<AssetServer>().reload(EMBER);
        update_until(&mut app, |app| accent(app) == MOSS_ACCENT);
    }

    /// Saving a mistake while the game runs keeps the last good theme on screen.
    #[test]
    fn a_broken_reload_leaves_the_current_sheet_alone() {
        let (mut app, dir, handle) = ember_app();
        write(&dir, EMBER, BROKEN_TEXT);
        app.world().resource::<AssetServer>().reload(EMBER);
        update_until(&mut app, |app| {
            app.world()
                .resource::<AssetServer>()
                .load_state(&handle)
                .is_failed()
        });
        app.update();
        assert_eq!(accent(&app), EMBER_ACCENT);
    }

    #[test]
    fn sheets_built_in_code_work_as_assets() {
        let (mut app, _dir) = app();
        let handle = app
            .world_mut()
            .resource_mut::<Assets<ThemeSheet>>()
            .add(ThemeSheet::default().color(palette::ACCENT, MOSS_ACCENT));
        app.world_mut()
            .resource_mut::<ActiveTheme>()
            .set_palette(handle);
        update_until(&mut app, |app| accent(app) == MOSS_ACCENT);
    }

    /// The palette lies over the skin: where both set a colour the palette's
    /// wins, and what only the skin sets stays.
    #[test]
    fn the_palette_lies_over_the_skin() {
        let (mut app, _dir) = app();
        let mut sheets = app.world_mut().resource_mut::<Assets<ThemeSheet>>();
        let skin = sheets.add(
            ThemeSheet::default()
                .color(palette::ACCENT, EMBER_ACCENT)
                .color(palette::SURFACE, MOSS_ACCENT),
        );
        let colours = sheets.add(ThemeSheet::default().color(palette::ACCENT, MOSS_ACCENT));
        app.world_mut()
            .resource_mut::<ActiveTheme>()
            .set_skin(skin)
            .set_palette(colours);
        update_until(&mut app, |app| accent(app) == MOSS_ACCENT);
        let surface = app.world().resource::<Theme>().color(&palette::SURFACE);
        assert_eq!(surface, MOSS_ACCENT, "the skin's own colour was lost");

        // Without the palette, the skin's colours show again.
        app.world_mut()
            .resource_mut::<ActiveTheme>()
            .clear_palette();
        app.update();
        assert_eq!(accent(&app), EMBER_ACCENT);
    }

    /// Every shipped theme loads from inside the crate, pictures and font
    /// included, and restyles the theme.
    #[test]
    #[cfg(any(feature = "theme-lords", feature = "theme-foundation"))]
    fn shipped_themes_load_from_inside_the_crate() {
        let mut shipped: Vec<(&str, &str)> = Vec::new();
        #[cfg(feature = "theme-lords")]
        shipped.push((crate::premade::lords::SKIN, crate::premade::lords::PALETTE));
        #[cfg(feature = "theme-foundation")]
        shipped.push((
            crate::premade::foundation::SKIN,
            crate::premade::foundation::PALETTE,
        ));
        for (skin, colours) in shipped {
            let (mut app, _dir) = app();
            let defaults = accent(&app);
            let server = app.world().resource::<AssetServer>().clone();
            let (skin, colours) = (server.load(skin), server.load(colours));
            app.world_mut()
                .resource_mut::<ActiveTheme>()
                .set_skin(skin.clone())
                .set_palette(colours);
            update_until(&mut app, |app| accent(app) != defaults);
            let state = server.recursive_dependency_load_state(&skin);
            assert!(
                state.is_loaded(),
                "{skin:?} or one of its files failed: {state:?}"
            );
            let theme = app.world().resource::<Theme>();
            assert!(
                theme.is_skinned(&crate::theme::skins::RAISED),
                "{skin:?} skins no role"
            );
        }
    }

    #[test]
    fn an_unused_resource_leaves_a_directly_set_sheet_alone() {
        let (mut app, _dir) = app();
        app.world_mut()
            .resource_mut::<Theme>()
            .set_sheet(ThemeSheet::default().color(palette::ACCENT, MOSS_ACCENT));
        app.update();
        app.update();
        assert_eq!(accent(&app), MOSS_ACCENT);
    }

    /// Every link from a theme file's `fonts` entry to text on screen, each
    /// checked in turn so a failure names the link that broke.
    #[test]
    fn a_theme_files_font_reaches_text() {
        const TERMINAL: &str = "themes/terminal.theme.ron";
        const FONT_FILE: &str = "fonts/Mono.ttf";
        let (mut app, dir) = app();
        write(
            &dir,
            TERMINAL,
            r#"(fonts: { "touchy.palette.font": "/fonts/Mono.ttf" })"#,
        );
        // Bevy's font loader keeps the bytes without checking them.
        dir.insert_asset(Path::new(FONT_FILE), b"not really a font".to_vec());
        let text = app
            .world_mut()
            .spawn((Text::new("score"), ThemeFont(palette::FONT)))
            .id();
        let sheet = set_active(&mut app, TERMINAL);
        update_until(&mut app, |app| {
            app.world()
                .resource::<AssetServer>()
                .is_loaded_with_dependencies(&sheet)
        });
        app.update();
        app.update();

        let world = app.world();
        let font = match world
            .resource::<Assets<ThemeSheet>>()
            .get(&sheet)
            .unwrap()
            .fonts
            .get(&palette::FONT)
        {
            Some(Follows::Value(font)) => font.clone(),
            other => panic!("the loader didn't read the `fonts` section: {other:?}"),
        };
        assert_eq!(
            world
                .resource::<AssetServer>()
                .get_path(&font)
                .unwrap()
                .path(),
            Path::new(FONT_FILE),
            "the font path wasn't resolved from the asset folder"
        );
        assert!(
            world.resource::<Assets<Font>>().contains(&font),
            "the font file didn't load"
        );

        let theme = world.resource::<Theme>();
        if theme.font(&palette::FONT) != font {
            let mut told = theme.clone();
            told.set_font_loaded(font.id(), true);
            assert_eq!(
                told.font(&palette::FONT),
                font,
                "`set_sheet` didn't take the sheet's fonts"
            );
            panic!("the theme never heard the font loaded (`track_loaded_fonts`)");
        }
        assert_eq!(
            world.get::<TextFont>(text).unwrap().font,
            FontSource::Handle(font),
            "`ThemeFont` didn't apply the theme's font"
        );
    }
}
