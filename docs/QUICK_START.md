# Quick start

This doc goes through a small settings screen: adding touchy, spawning a page of controls, reading
what the player does with them, and changing the page from the game.

## 1. Add the plugins

```toml
[dependencies]
bevy = "0.20"
touchy = "0.1"
```

```rust,no_run
use bevy::input_focus::tab_navigation::TabNavigationPlugin;
use bevy::prelude::*;
use touchy::prelude::*;

fn main() {
    App::new()
        .add_plugins((DefaultPlugins, TouchyPlugins, TabNavigationPlugin))
        .run();
}
```

`TouchyPlugins` is a plugin group: every widget, container and readout, the theme, and the UI
layers. Each plugin adds the ones it depends on, so you can also take a single plugin, or the
group with parts disabled:

```rust,no_run
# use bevy::prelude::*;
# use touchy::prelude::*;
use touchy::widgets::joystick::JoystickWidgetPlugin;

App::new().add_plugins((
    DefaultPlugins,
    TouchyPlugins.build().disable::<JoystickWidgetPlugin>(),
));
```

Bevy's `TabNavigationPlugin` is optional, and left for you to add because it acts on the whole
app: it makes Tab move the keyboard focus between controls. Clicking focuses a control without
it, and a focused control answers Enter, Space and the arrow keys.

## 2. Spawn a page

UI goes on a *layer*. Touchy makes three root nodes at startup, held in the `UiLayers` resource:
`screen` for the game's main layout, `overlay` for modal screens (with a dimmer under them),
and `feedback` for tooltips and toasts. Spawn under one of them, and you need a camera:

```rust
use bevy::prelude::*;
use touchy::prelude::*;

/// The volume the player chose, for the game to use.
#[derive(Resource)]
struct Volume(f32);

fn spawn_settings(layers: Res<UiLayers>, mut commands: Commands) {
    commands.spawn(Camera2d);
    commands.entity(layers.screen).with_children(|screen| {
        screen
            .column(|page| {
                page.heading("Settings");
                page.small("Changes apply at once.");
                page.divider();

                page.text("Volume");
                page.slider(SliderConfig::new(0.8).range(0.0..=1.0).step(0.05))
                    .observe(set_volume);

                page.checkbox(CheckboxConfig::new().label("Subtitles").checked(true));

                page.row(|buttons| {
                    buttons.spacer();
                    buttons
                        .button(ButtonConfig::new("Reset").tokens(ButtonTokens::danger()))
                        .observe(reset);
                    buttons.button(ButtonConfig::new("Done"));
                });
            })
            .insert(Card)
            .node(|node| node.width = px(320.0));
    });
}

fn set_volume(change: On<ValueChange<f32>>, mut volume: ResMut<Volume>) {
    volume.0 = change.value;
}

fn reset(_: On<Activate>, mut volume: ResMut<Volume>) {
    volume.0 = 0.8;
}
```

Every widget is one method on whatever you're spawning children into, taking a *config*: a plain
struct with public fields and builder methods. `SliderConfig::new(0.8).range(0.0..=1.0)` and
`SliderConfig { value: 0.8, ..default() }` say the same thing. Where a widget holds other
things, like a column, a card or a panel, the method also takes a closure that spawns them.

Each method returns Bevy's `EntityCommands` for the widget, so you can `.insert` your own
components, `.observe` its events, or `.node(|node| ...)` to adjust its layout. See
[Layout](LAYOUT.md) for rows, columns and the rest.

## 3. Read what the player did

Widgets *announce* changes as Bevy events, and *keep* their current value in a component. Use
whichever fits:

- **To act when something happens**, observe the event on the widget, as `set_volume` does
  above. A slider announces `ValueChange<f32>`, a checkbox `ValueChange<bool>`, a button
  `Activate`. The observer runs once per change, and nothing runs while nothing changes.
- **To read the value whenever you need it**, query its component. A slider keeps
  `SliderValue`, a checkbox has `Checked` while it's checked. Tag the widget with a marker of
  your own to find it:

```rust
# use bevy::prelude::*;
# use touchy::prelude::*;
#[derive(Component)]
struct Subtitles;

fn spawn(page: &mut ChildSpawnerCommands) {
    page.checkbox(CheckboxConfig::new().label("Subtitles")).insert(Subtitles);
}

fn draw_subtitles(subtitles: Single<Has<Checked>, With<Subtitles>>) {
    if *subtitles {
        // ...
    }
}
```

`Single` also makes the system skip quietly while the page isn't spawned. [Widgets](WIDGETS.md)
lists what each control announces and keeps.

## 4. Change the page from the game

The values work the other way too: write the component, and the widget follows. Setting
`SliderValue` moves the slider's thumb, inserting `Checked` ticks a checkbox, and setting a
meter's `MeterValue` fills it:

```rust
# use bevy::prelude::*;
# use touchy::prelude::*;
#[derive(Component)]
struct Health;

fn spawn(page: &mut ChildSpawnerCommands) {
    page.meter(MeterConfig::new(100.0).range(0.0, 100.0)).insert(Health);
}

fn show_health(mut meter: Single<&mut MeterValue, With<Health>>) {
    meter.0 = 42.0;
}
```

The game can also send messages that aren't attached to a widget. A toast pops up on the
feedback layer, then goes:

```rust
# use bevy::prelude::*;
# use touchy::prelude::*;
fn saved(mut commands: Commands) {
    commands.trigger(ShowToast::success("Game saved"));
}
```

## 5. Give it a look

Without a theme, touchy draws in its default dark palette. To use a premade theme, add the
`ThemeSheetPlugin`, which loads theme files, and choose a skin and a palette:

```rust,no_run
# use bevy::prelude::*;
# use touchy::prelude::*;
use touchy::premade;
use touchy::theme::ThemeSheetPlugin;

fn main() {
    App::new()
        .add_plugins((DefaultPlugins, TouchyPlugins, ThemeSheetPlugin))
        .add_systems(Startup, crown_the_lords)
        .run();
}

fn crown_the_lords(server: Res<AssetServer>, mut active: ResMut<ActiveTheme>) {
    active
        .set_skin(server.load(premade::lords::SKIN))
        .set_palette(server.load(premade::lords::PALETTE));
}
```

[Theming](THEMING.md) covers writing your own.

## Where next?

The examples are complete pages. `cargo run --example showcase` runs them all with a skin and a
palette picker, and each also runs alone, as `cargo run --example traffic` does.
