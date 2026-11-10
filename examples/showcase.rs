//! Every example in one window, a page each, under a toolbar.
//!
//! ```text
//! cargo run --example showcase
//! ```
//!
//! This showcase was built using the same UI elements and API that it showcases.
//! Delicious dog food.
//!
//! To add a page, give its example a `TITLE`, a `pub fn page`, and a `pub fn plugin`, for any
//! systems or theme tokens it needs, then list it in [`PAGES`].
//! A page that needs systems of its own gives them in a `pub fn plugin`, added in `main`.

// Each page brings its own copy of `common`, so it also runs on its own.
#![allow(clippy::duplicate_mod)]

#[path = "common/mod.rs"]
mod common;

#[path = "containers.rs"]
#[allow(dead_code)]
mod containers;
#[path = "controllers.rs"]
#[allow(dead_code)]
mod controllers;
#[path = "selection.rs"]
#[allow(dead_code)]
mod selection;
#[path = "traffic.rs"]
#[allow(dead_code)]
mod traffic;

use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use touchy::cancel::{CancelPhase, CancelRequest, CancelSource};
use touchy::prelude::*;
use touchy::premade;
use touchy::theme::ThemeSheetPlugin;

/// A page's systems and tokens, added once for the whole showcase.
type Plugin = fn(&mut App);

/// The pages, in the order the picker lists them.
const PAGES: [(&str, common::Page, Plugin); 4] = [
    (containers::TITLE, containers::page, containers::plugin),
    (controllers::TITLE, controllers::page, controllers::plugin),
    (selection::TITLE, selection::page, selection::plugin),
    (traffic::TITLE, traffic::page, traffic::plugin),
];

/// The skins the skin picker offers: how things are drawn. `None` is touchy's own flat look.
const SKINS: [(&str, Option<&str>); 4] = [
    ("Flat", None),
    ("Lords", Some(premade::lords::SKIN)),
    ("Foundation", Some(premade::foundation::SKIN)),
    ("Layered", Some("themes/layered.theme.ron")),
];
/// The palettes the palette picker offers: what colours things are drawn in, over any skin.
const PALETTES: [(&str, Option<&str>); 6] = [
    ("Default", None),
    ("Lords", Some(premade::lords::PALETTE)),
    ("Foundation", Some(premade::foundation::PALETTE)),
    ("Ember", Some("themes/ember.theme.ron")),
    ("Moss", Some("themes/moss.theme.ron")),
    ("Terminal", Some("themes/terminal.theme.ron")),
];

const PICKER_WIDTH: f32 = 180.0;
const THEME_PICKER_WIDTH: f32 = 160.0;
const WELCOME_WIDTH: f32 = 440.0;
const WELCOME_TITLE: &str = "touchy showcase";
const WELCOME: [&str; 4] = [
    "touchy is a UI library for Bevy, wrapping its UI widgets in an ergonomic API.",
    "This showcase was built using the same UI elements and API that it showcases. Delicious dog food.",
    "Each page is an example you can also run on its own: cargo run --example controllers",
    "The pickers at the top right restyle every page at once: a skin says how things are drawn, a palette what colours, and any skin goes with any palette.",
];

fn main() {
    let mut app = common::app("touchy showcase");
    app.add_plugins(ThemeSheetPlugin);
    // The pages' own systems and theme tokens handled by their own plugins.
    for (_, _, plugin) in PAGES {
        plugin(&mut app);
    }
    app.init_resource::<ShownPage>()
        .add_systems(Startup, (spawn_bar, open_welcome))
        .add_systems(Update, show_page)
        .add_systems(PreUpdate, close_welcome_on_escape.in_set(CancelPhase::Menu))
        .run();
}

/// Which of the [`PAGES`] is showing.
#[derive(Resource, Default, PartialEq)]
struct ShownPage(usize);

impl ShownPage {
    /// The page `by` places on, wrapping around the ends.
    fn stepped(&self, by: isize) -> Self {
        let count = PAGES.len() as isize;
        Self((self.0 as isize + by).rem_euclid(count) as usize)
    }
}

#[derive(Component)]
struct ShowcaseBar;

#[derive(Component)]
struct PagePicker;

/// The welcome on the overlay layer.
#[derive(Component)]
struct Welcome;

/// Spawns the toolbar container. A page picker with navigation arrows, and a theme picker.
fn spawn_bar(layers: Res<UiLayers>, mut commands: Commands) {
    commands.spawn(Camera2d);
    commands.entity(layers.screen).with_children(|screen| {
        let config = ToolbarConfig::new(ToolbarSide::Top);
        // Empty body to start.
        screen.toolbar(config, bar, |_| {}).insert(ShowcaseBar);
    });
}

fn bar(bar: &mut ChildSpawnerCommands) {
    let previous = ButtonConfig::new("<").tooltip(Tooltip::new("Previous page"));
    bar.button(previous).observe(step_page(-1));
    let titles = PAGES.map(|(title, ..)| title);
    let picker = DropdownConfig::new(titles).chosen(0).width(PICKER_WIDTH);
    bar.dropdown(picker).insert(PagePicker).observe(pick_page);
    let next = ButtonConfig::new(">").tooltip(Tooltip::new("Next page"));
    bar.button(next).observe(step_page(1));

    bar.spacer();

    bar.small("Skin");
    let skins = DropdownConfig::new(SKINS.map(|(name, _)| name))
        .chosen(0)
        .width(THEME_PICKER_WIDTH);
    bar.dropdown(skins).observe(pick_skin);
    bar.small("Palette");
    let palettes = DropdownConfig::new(PALETTES.map(|(name, _)| name))
        .chosen(0)
        .width(THEME_PICKER_WIDTH);
    bar.dropdown(palettes).observe(pick_palette);
    let help = ButtonConfig::new("?").tooltip(Tooltip::new("About the showcase"));
    bar.button(help).observe(reopen_welcome);
}

/// An arrow's press: the page `by` places on.
fn step_page(by: isize) -> impl Fn(On<Activate>, ResMut<ShownPage>) {
    move |_, mut shown| {
        let stepped = shown.stepped(by);
        shown.set_if_neq(stepped);
    }
}

fn pick_page(change: On<ValueChange<usize>>, mut shown: ResMut<ShownPage>) {
    shown.set_if_neq(ShownPage(change.value));
}

/// Puts the skin picked under the palette, or takes it away for touchy's flat look.
fn pick_skin(
    change: On<ValueChange<usize>>,
    server: Res<AssetServer>,
    mut active: ResMut<ActiveTheme>,
) {
    match SKINS[change.value].1 {
        Some(path) => active.set_skin(server.load(path)),
        None => active.clear_skin(),
    };
}

/// Lays the palette picked over the skin, or takes it away for touchy's own colours.
fn pick_palette(
    change: On<ValueChange<usize>>,
    server: Res<AssetServer>,
    mut active: ResMut<ActiveTheme>,
) {
    match PALETTES[change.value].1 {
        Some(path) => active.set_palette(server.load(path)),
        None => active.clear_palette(),
    };
}

/// Shows the page picked in the toolbar's body, and keeps the picker in step with it.
fn show_page(
    shown: Res<ShownPage>,
    bar: Single<&Toolbar, With<ShowcaseBar>>,
    pickers: Query<Entity, With<PagePicker>>,
    focus: Option<ResMut<InputFocus>>,
    mut commands: Commands,
) {
    if !shown.is_changed() {
        return;
    }
    let (_, page, _) = PAGES[shown.0];
    // Despawn what was there.
    commands
        .entity(bar.body())
        .despawn_children()
        .with_children(|area| {
            common::frame(area, page);
        });
    // Updated the picker to match the page.
    for picker in &pickers {
        commands.entity(picker).insert(Chosen(Some(shown.0)));
    }
    // The keyboard's focus may have been on the page that went.
    if let Some(mut focus) = focus {
        focus.clear();
    }
}

/// The welcome overlay on loading or resetting the showcase.
fn open_welcome(welcome: Query<(), With<Welcome>>, mut commands: Commands) {
    // There can be only one!
    if !welcome.is_empty() {
        return;
    }
    commands
        .spawn((Welcome, UiLayer::Overlay, Column, Card))
        .node(|node| node.width = px(WELCOME_WIDTH))
        .with_children(|card| {
            card.heading(WELCOME_TITLE);
            card.bullets(WELCOME);
            card.row(|row| {
                row.spacer();
                row.button(ButtonConfig::new("Cool."))
                    .observe(close_welcome);
            });
        });
}

fn reopen_welcome(_: On<Activate>, welcome: Query<(), With<Welcome>>, commands: Commands) {
    open_welcome(welcome, commands);
}

fn close_welcome(_: On<Activate>, welcome: Query<Entity, With<Welcome>>, mut commands: Commands) {
    for welcome in &welcome {
        commands.entity(welcome).despawn();
    }
}

/// Escape closes the welcome, as it does a menu.
fn close_welcome_on_escape(
    mut cancel: ResMut<CancelRequest>,
    welcome: Query<Entity, With<Welcome>>,
    mut commands: Commands,
) {
    if welcome.is_empty() || cancel.peek() != Some(CancelSource::Escape) {
        return;
    }
    cancel.consume();
    for welcome in &welcome {
        commands.entity(welcome).despawn();
    }
}
