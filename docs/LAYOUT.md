# Layout

Touchy lays things out with Bevy's own flexbox `Node`. The goal is not to hide it... Like all other
decisions in this crate, the idea is to remove boilerplate code and provide simple public API for
the most used features. Treat `.node()` as the lower level escape hatch.

## Rows, columns and adjusting any node

`row` lays its children out left to right and `column` top to bottom, a theme-set gap apart.
Both take a closure that spawns their children:

```rust
# use bevy::prelude::*;
# use touchy::prelude::*;
fn spawn(page: &mut ChildSpawnerCommands) {
    page.column(|form| {
        form.heading("New game");
        form.row(|name| {
            name.text("Name");
            name.text_input(TextInputConfig::default());
        });
        form.row(|buttons| {
            buttons.spacer();
            buttons.button(ButtonConfig::new("Start"));
        });
    });
}
```

Every spawning method returns the entity's `EntityCommands`, and `.node(|node| ...)` changes
whatever else you want about its `Node`, keeping the rest:

```rust
# use bevy::prelude::*;
# use touchy::prelude::*;
fn spawn(page: &mut ChildSpawnerCommands) {
    page.row(|toolbar| {
        toolbar.button(ButtonConfig::new("Undo"));
        toolbar.button(ButtonConfig::new("Redo"));
    })
    .node(|node| {
        node.justify_content = JustifyContent::Center;
        node.padding = UiRect::all(px(8.0));
    });
}
```

`Row`, `Column` and the other layout pieces are components, so they also go on nodes you spawn
yourself: `commands.spawn((Row, MyMarker))`.

## The pieces

- **`text`, `heading`, `small`**: body text, a heading, and smaller, muted small print, each in
  the theme's font and sizes.
- **`bullets` and `numbered`**: lists whose items wrap under themselves, beside their markers.
- **`divider`**: a thin rule across its parent, horizontal in a column and vertical in a row.
- **`spacer`**: empty room taking whatever its parent has spare, pushing what follows to the far
  end. The `Settings` button at a toolbar's right edge sits after a spacer.
- **`Card`**: the look of a card on any node: the theme's surface, an edge, rounded corners and
  room inside. Insert it: `page.column(build).insert(Card)`.
- **`Unstretched`**: keeps a control from being stretched across its parent, while following the
  parent's other alignment. A column stretches its children to its width; a button marked
  `Unstretched` keeps the width its label needs.

## Containers

**Scroll areas** scroll what they hold, with a draggable thumb, the wheel and touch. They fill
the space they're given unless they have a `max_height`, and `stick_to_end` keeps a log or chat
pinned to its newest line:

```rust
# use bevy::prelude::*;
# use touchy::prelude::*;
fn spawn(page: &mut ChildSpawnerCommands) {
    let config = ScrollAreaConfig::default().max_height(200.0).stick_to_end(true);
    page.scroll_area(config, |log| {
        log.text("The gates open.");
    });
}
```

**Sections** are collapsible, with a title to click:

```rust
# use bevy::prelude::*;
# use touchy::prelude::*;
fn spawn(page: &mut ChildSpawnerCommands) {
    page.section(SectionConfig::new("Advanced").open(false), |advanced| {
        advanced.checkbox(CheckboxConfig::new().label("Show frame times"));
    });
}
```

**Panels** are cards that can dock to an edge or corner of their parent, float, be carried by a
grip and dropped on another edge, and be resized by hand:

```rust
# use bevy::prelude::*;
# use touchy::prelude::*;
fn spawn(screen: &mut ChildSpawnerCommands) {
    let config = PanelConfig::new()
        .placement(PanelPlacement::Right)
        .width(280.0)
        .movable(GrabberStyle::Grip)
        .resizable(true);
    screen.panel(config, |inspector| {
        inspector.heading("Inspector");
    });
}
```

A docked panel takes its place from its parent, so the parent needs room of its own: a
panel on the screen layer docks to the window, and a panel inside a content-sized column has
only as much room as the column.

**Toolbars** run along an edge with a body filling the rest. The body is where pages go, so
swapping a page is despawning the body's children and spawning new ones:

```rust
# use bevy::prelude::*;
# use touchy::prelude::*;
fn spawn(screen: &mut ChildSpawnerCommands) {
    screen.toolbar(
        ToolbarConfig::new(ToolbarSide::Top),
        |bar| {
            bar.button(ButtonConfig::new("File"));
            bar.spacer();
            bar.button(ButtonConfig::new("?"));
        },
        |body| {
            body.text("The page goes here.");
        },
    );
}
```

The toolbar's `Toolbar` component knows its body: `toolbar.body()`.

**Popovers** float beside whatever opens them: dropdowns and combo boxes are built on them, and
`popover` spawns one of your own.

## Layers

Touchy makes three root nodes, one for each layer of the screen, held in the `UiLayers` resource:

| Layer      | For                                                                   |
| ---------- | --------------------------------------------------------------------- |
| `screen`   | The game's main layout: menus, HUDs, pages.                           |
| `overlay`  | Modal screens. A dimmer covers the screen layer while one is open.    |
| `feedback` | Tooltips, toasts and drag previews, over everything.                  |

Spawn under a layer's root, or spawn anything with a `UiLayer` component and it's moved there:

```rust
# use bevy::prelude::*;
# use touchy::prelude::*;
fn ask(mut commands: Commands) {
    commands
        .spawn((UiLayer::Overlay, Column, Card))
        .with_children(|dialog| {
            dialog.heading("Quit without saving?");
            dialog.row(|buttons| {
                buttons.spacer();
                buttons.button(ButtonConfig::new("Cancel"));
                buttons.button(ButtonConfig::new("Quit").tokens(ButtonTokens::danger()));
            });
        });
}
```

## Escape

Escape is one `CancelRequest` a frame, offered to whatever is on top first, in the order of
`CancelPhase`: an open radial menu, then open popovers and dropdowns, then menus and dialogs,
then anything in progress, then the game. Touchy's own widgets take their turn already. A
dialog of your own takes it in `CancelPhase::Menu`, so it closes before the game opens its
pause menu:

```rust
# use bevy::prelude::*;
# use touchy::prelude::*;
use touchy::cancel::{CancelPhase, CancelRequest};

#[derive(Component)]
struct QuitDialog;

fn plugin(app: &mut App) {
    app.add_systems(PreUpdate, close_on_escape.in_set(CancelPhase::Menu));
}

fn close_on_escape(
    mut cancel: ResMut<CancelRequest>,
    dialog: Query<Entity, With<QuitDialog>>,
    mut commands: Commands,
) {
    if dialog.is_empty() || cancel.peek().is_none() {
        return;
    }
    cancel.consume();
    for dialog in &dialog {
        commands.entity(dialog).despawn();
    }
}
```
