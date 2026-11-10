//! What every example shares: its app, and the frame a page is shown in, so a page looks
//! the same run on its own as in the showcase.

// Each example uses only some of this.
#![allow(dead_code)]

use bevy::input_focus::tab_navigation::TabNavigationPlugin;
use bevy::prelude::*;
use touchy::prelude::*;

const WINDOW_SIZE: UVec2 = UVec2::new(1280, 800);
/// The room around a page's contents, in pixels.
const PAGE_PADDING: f32 = 24.0;

/// A page: a function that spawns it into what it's given.
pub type Page = fn(&mut ChildSpawnerCommands);

/// An app with a window titled `title`, touchy, and Tab moving between controls.
pub fn app(title: &str) -> App {
    let window = Window {
        title: title.into(),
        resolution: WINDOW_SIZE.into(),
        ..default()
    };
    let mut app = App::new();
    app.add_plugins((
        DefaultPlugins.set(WindowPlugin {
            primary_window: Some(window),
            ..default()
        }),
        TabNavigationPlugin,
        TouchyPlugins,
    ));
    app
}

/// Runs `page` on its own, filling a window titled `title`.
pub fn run(title: &str, page: Page) {
    run_with(title, page, |_| {});
}

/// Runs `page` on its own, with the systems `plugin` adds.
pub fn run_with(title: &str, page: Page, plugin: fn(&mut App)) {
    let mut app = app(title);
    plugin(&mut app);
    app.add_systems(
        Startup,
        move |layers: Res<UiLayers>, mut commands: Commands| {
            commands.spawn(Camera2d);
            commands.entity(layers.screen).with_children(|screen| {
                frame(screen, page);
            });
        },
    )
    .run();
}

/// Spawns `page` padded, in a scroll area filling `area`. The page is at least as tall as the
/// area, so what it centres or stretches has the whole area, and longer pages scroll.
pub fn frame(area: &mut ChildSpawnerCommands, page: Page) -> Entity {
    area.scroll_area(default(), |view| {
        view.column(page).node(|node| {
            node.min_height = percent(100.0);
            node.flex_shrink = 0.0;
            node.padding = UiRect::all(px(PAGE_PADDING));
        });
    })
    .id()
}
