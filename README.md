# touchy

Themeable, touch-friendly UI widgets for data-driven [Bevy](https://bevyengine.org) games.

Touchy wraps Bevy's headless UI widgets in an API that spawns a whole control in one call, and
dresses every one of them from a theme: colours, fonts, sizes, and nine-sliced, tinted, animated
skins. The same page can look like a debug panel, a fantasy codex or a pixel-art console without
changing a line of game code.

```rust,no_run
use bevy::prelude::*;
use touchy::prelude::*;

fn main() {
    App::new()
        .add_plugins((DefaultPlugins, TouchyPlugins))
        .add_systems(Startup, spawn_menu)
        .run();
}

fn spawn_menu(layers: Res<UiLayers>, mut commands: Commands) {
    commands.spawn(Camera2d);
    commands.entity(layers.screen).with_children(|screen| {
        screen.column(|menu| {
            menu.heading("Settings");
            menu.slider(SliderConfig::new(0.8).range(0.0..=1.0))
                .observe(|change: On<ValueChange<f32>>| info!("Volume: {}", change.value));
            menu.checkbox(CheckboxConfig::new().label("Subtitles"));
            menu.button(ButtonConfig::new("Play"))
                .observe(|_: On<Activate>| info!("Let's go"));
        })
        .insert(Card);
    });
}
```

## What's in the box

- **Controls**: buttons, checkboxes, capsule switches, sliders, gearshifts, number fields,
  scrubbers, text fields, combo boxes, dropdowns, menus, segmented controls, grids, radial
  menus, joysticks, d-pads and media transport bars.
- **Containers**: rows, columns, cards, dockable and movable panels, scroll areas, collapsible
  sections, toolbars and popovers.
- **Readouts**: meters, live plots, and keyed lists that only redraw the rows that changed.
- **Feedback and structure**: tooltips, toasts, and screen, overlay and feedback layers with
  Escape handling that closes the topmost thing first.
- **Canvases** to draw on with gizmos, and pointers into the game world.
- **Theming**: every colour, size, font and skin is a theme token. Themes load from `.theme.ron`
  files that reload while the game runs, in two independent layers: a *skin* for how things are
  drawn and a *palette* for their colours.
- **Two premade themes**, compiled in: *Lords*, carved wood and brass for fantasy games, and
  *Foundation*, pixel-art panels for science fiction.

## Learn more

- [Quick start](docs/QUICK_START.md): an app, a page, and reading what the player did.
- [Widgets](docs/WIDGETS.md): what every control announces and where its value lives.
- [Layout](docs/LAYOUT.md): rows, columns, cards, panels, layers, and adjusting any node.
- [Theming](docs/THEMING.md): tokens, palettes, skins, theme files and the premade themes.

The examples are the best tour. Each is a page you can run on its own, and the showcase puts
them all in one window with pickers for every skin and palette:

```text
cargo run --example showcase
cargo run --example controllers
```

## Cargo features

| Feature            | Default | What it adds                                             |
| ------------------ | ------- | -------------------------------------------------------- |
| `premade-themes`   | yes     | Both premade themes.                                     |
| `theme-lords`      | yes     | The Lords theme, with the IM Fell English font.          |
| `theme-foundation` | yes     | The Foundation theme, with the Silkscreen font.          |
| `dev`              | no      | Bevy's dynamic linking, for faster builds while working. |

A game that uses neither premade theme can turn off default features and carries none of
their pictures or fonts.

## Bevy versions

| touchy | Bevy |
| ------ | ---- |
| 0.1    | 0.20 |

## Licence

Touchy is dual-licensed under either the MIT licence or the Apache Licence 2.0, at your option.
The premade themes' fonts are under the SIL Open Font License; each sits beside its licence in
`premade_themes`.
