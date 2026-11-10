# Widgets

Every touchy control works the same way. It's spawned by one method taking a config, it
*announces* what the player does as a Bevy event on the widget, and it *keeps* its current value
in a component on the widget. This page explains that pattern, then lists what each widget
announces and keeps.

## Events and state

A widget's value is in two places at once, for two different jobs.

**The event** says that something changed, once, at the moment it changed. Most controls
announce Bevy's `ValueChange<T>`, whose `value` is the new value and whose `is_final` says whether
the player has finished (the drag ended, the field lost focus) or is still going. Observe it on
the widget:

```rust
# use bevy::prelude::*;
# use touchy::prelude::*;
fn spawn(page: &mut ChildSpawnerCommands) {
    page.slider(SliderConfig::new(50.0).range(0.0..=100.0))
        .observe(|change: On<ValueChange<f32>>| {
            if change.is_final {
                info!("Settled on {}", change.value);
            }
        });
}
```

An observer is an ordinary system with the event as its first argument, so it can take
resources and queries too. Write it as a named function once it's more than a line:

```rust
# use bevy::prelude::*;
# use touchy::prelude::*;
#[derive(Resource)]
struct Difficulty(usize);

fn spawn(page: &mut ChildSpawnerCommands) {
    page.segmented(SegmentedConfig::new(["Easy", "Normal", "Hard"]).chosen(1))
        .observe(choose_difficulty);
}

fn choose_difficulty(change: On<ValueChange<usize>>, mut difficulty: ResMut<Difficulty>) {
    difficulty.0 = change.value;
}
```

**The state** says what the value is now, for as long as the widget exists. A slider keeps a
`SliderValue`, a choice widget a `Chosen`, a checkbox has `Checked` while it's checked. Read it
from any system with a query, marking the widget with a component of your own to find it:

```rust
# use bevy::prelude::*;
# use touchy::prelude::*;
#[derive(Component)]
struct Throttle;

fn spawn(page: &mut ChildSpawnerCommands) {
    page.slider(SliderConfig::new(0.0).range(0.0..=1.0)).insert(Throttle);
}

fn drive(throttle: Single<&SliderValue, With<Throttle>>, time: Res<Time>) {
    let _speed = throttle.0 * 30.0 * time.delta_secs();
}
```

Which to use is a question of when the game needs to know:

- Use the **event** for things that happen *because* the player acted: saving a setting, opening
  a page, playing a sound. It runs once per change and costs nothing in between.
- Use the **state** for things the game does *continuously* with the value: steering with a
  joystick every frame, scaling a sprite by a slider. Reading a component every frame is cheap,
  and you never miss a value or handle one twice.

`Single` makes a system skip quietly when no such widget exists, so systems for a page that
isn't showing simply don't run.

State also works the other way. Write it, and the widget follows without announcing anything:
setting `SliderValue` moves the thumb, inserting `Checked` ticks the box, setting `Chosen`
changes a dropdown's choice. That's how a game loads saved settings into a page, or keeps two
controls in step.

## Choices

Dropdowns, combo boxes, menus, segmented controls, grids and radial menus all hold *choices*.
A choice can be plain text, as above, or anything a closure spawns: an icon and a name, a colour
swatch, a picture. Its label still names it, for typing into a combo box:

```rust
# use bevy::prelude::*;
# use touchy::prelude::*;
use touchy::choice::Choice;

fn spawn(page: &mut ChildSpawnerCommands) {
    let colours = [("Red", Color::srgb(0.9, 0.2, 0.2)), ("Blue", Color::srgb(0.2, 0.4, 0.9))];
    let options = colours.map(|(name, colour)| {
        Choice::new(name, move |row| {
            row.row(|swatch| {
                swatch.spawn((Node { width: px(12.0), height: px(12.0), ..default() },
                    BackgroundColor(colour)));
                swatch.text(name);
            });
        })
    });
    page.dropdown(DropdownConfig::new(options).chosen(0));
}
```

`Choice::text(label).disabled(true)` shows a choice that can't be picked.

## Controls

| Spawn with           | Announces                                  | Keeps                       |
| -------------------- | ------------------------------------------ | --------------------------- |
| `button`             | `Activate`                                 | `Pressed` while held        |
| `checkbox`           | `ValueChange<bool>`                        | `Checked`                   |
| `capsule_flip`       | `ValueChange<bool>`                        | `Checked`                   |
| `slider`             | `ValueChange<f32>`                         | `SliderValue`               |
| `gearshift`          | `ValueChange<usize>`, the gear             | `GearIndex`                 |
| `scrubber`           | `ValueChange<f64>`                         | `ScrubValue`                |
| `number_input`       | `ValueChange<f64>`                         | `NumberValue`               |
| `text_input`         | `ValueChange<String>`, on commit           | `TextInputValue`            |
| `combo`              | `ValueChange<usize>`, or `<String>` if free | `Chosen`, `ComboText`       |
| `dropdown`           | `ValueChange<usize>`                       | `Chosen`                    |
| `segmented`          | `ValueChange<usize>`                       | `Chosen`                    |
| `grid`               | `ValueChange<usize>`                       | `Chosen`                    |
| `radial`, `radial_button` | `ValueChange<usize>`, or `<Option<usize>>` if the centre clears | `Chosen` |
| `menu`               | `MenuPick`, with the item's `index`        | nothing: a menu keeps no choice |
| `joystick`           | `ValueChange<Vec2>`, +y up, in the unit disc | `JoystickValue`           |
| `dpad`               | `ValueChange<IVec2>`, +y up                | `DpadValue`                 |
| `transport`          | `TransportRequest` to its `Timeline`       | the `Timeline` it drives    |

Some notes on particular widgets:

- **Buttons** come in variants by their tokens: `ButtonConfig::new("Delete")
  .tokens(ButtonTokens::danger())`, likewise `secondary()` and `ghost()`. Any button takes
  `.tooltip(Tooltip::new("..."))`.
- **Text fields** announce when the text is *committed*: Enter, or clicking away, as `CommitOn`
  says. `masked` fields, for passwords, keep the real text out of the editor entirely.
- **Combo boxes** are *choosing* by default, where the text must name an option. A *free* combo
  (`free_text`) accepts any text, suggesting the options, and announces the text instead.
- **Radial menus** open around where they're pressed. `radial_button` makes a button that shows
  the current choice; `radial` makes a ring alone, for the game to open with `OpenRadial`.
- **Joysticks and d-pads** can be `floating`: hidden until the player presses somewhere in their
  area, then centred under the finger, as on a phone. A d-pad held down repeats its
  announcement at the rate its config's `repeat` sets.
- **Transport bars** don't keep time themselves. They send requests (`TogglePlay`, `Step`,
  `Seek`, and so on) to a `Timeline` component the game owns and advances.

## Containers that announce

| Spawn with   | Announces                                  | Keeps          |
| ------------ | ------------------------------------------ | -------------- |
| `section`    | `ValueChange<bool>`, opened or closed      | `SectionOpen`  |
| `popover`    | `ValueChange<bool>`, opened or closed      | `PopoverOpen`  |
| `grabber`    | `Grabbed`, with its phase and offset       | nothing        |

Write `SectionOpen` or `PopoverOpen` to open or close one from the game.

## Readouts

Readouts only show what the game writes into them.

- A **meter** fills to its `MeterValue`.
- A **plot** draws the series in its `PlotData`; `set` replaces a series' points, by name.
- A **keyed list** keeps one row per item and only rebuilds the rows whose data changed, so a
  long list is cheap to keep up to date. Give each row a key, and `Bind`s that write the row's
  data into its parts:

```rust
# use bevy::prelude::*;
# use touchy::prelude::*;
#[derive(Clone, PartialEq)]
struct Player {
    id: u32,
    name: String,
    score: u32,
}

fn spawn_row(list: &mut ChildSpawnerCommands, _: &Player) {
    list.row(|row| {
        row.text("").insert(Bind::new(|player: &Player, text: &mut Text| {
            text.0 = player.name.clone();
        }));
        row.spacer();
        row.text("").insert(Bind::new(|player: &Player, text: &mut Text| {
            text.0 = player.score.to_string();
        }));
    });
}

fn show_scores(players: Vec<Player>, list: Entity, mut commands: Commands) {
    commands.entity(list).sync_children(players, |player| player.id, spawn_row);
}
```

The traffic example shows a plot and a keyed list kept up to date from a running simulation.

## Feedback

- **Tooltips**: insert a `Tooltip` on any node, or pass one to a config's `tooltip` builder.
- **Toasts**: `commands.trigger(ShowToast::info("..."))`, likewise `success`, `error` and
  `warning`, from anywhere.
