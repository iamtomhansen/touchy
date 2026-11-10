# Theming

Everything touchy draws takes its look from the theme: every colour, size, font and skin is a
*token*, looked up by name in the `Theme` resource. A theme changes tokens, from code or from
`.theme.ron` files, and every widget follows at once, including ones already on screen.

A theme has two halves, which you can pick separately:

- A **palette** says what colours things are drawn in.
- A **skin** says *how* they're drawn: the pictures, gradients and shadows on each part, and
  the font and sizes.

Touchy's skins are grey pictures that the palette tints, so any skin goes with any palette.

## Tokens

A token is a dotted name, like `touchy.button.bg` or `touchy.palette.accent`. Most of touchy's
tokens don't have values of their own: they *follow* other tokens. Every widget's colours
follow a dozen palette colours, its sizes follow a few palette sizes, and its skins follow five
skin roles. So a theme that sets those restyles everything, and a theme that sets one widget's
token changes only that widget.

| Group               | Tokens                                                                  |
| ------------------- | ----------------------------------------------------------------------- |
| Palette colours     | `touchy.palette.` `accent`, `highlight`, `surface`, `control`, `edge`, `text`, `text.muted`, `danger`, `warning`, `info`, `success`, `scrim` |
| Palette sizes       | `touchy.palette.` `text.size`, `heading.size`, `small.size`, `control.text.size`, `corner_radius`, `control.corner_radius` |
| Palette font        | `touchy.palette.font`                                                   |
| Skin roles          | `touchy.skin.` `surface`, `raised`, `sunken`, `knob`, `mark`             |

The skin roles are the shapes widgets are made of. *Surface* is what other things sit on:
panels, cards, menus, toasts. *Raised* is what's pressed: buttons, steppers, dropdowns.
*Sunken* is what's filled in or slid along: fields, tracks, troughs. *Knob* is every thumb,
and *mark* a checkbox's tick.

A token in a state is written `token:state`, as `touchy.button.bg:hover`. The states are
`disabled`, `pressed`, `focused` and `hover`. A state with no value of its own takes its resting
token's, shaded by the theme's `state_shades`, so a theme rarely needs to set them.

[`assets/themes/reference.theme.ron`](../assets/themes/reference.theme.ron) lists every token
at its default, and documents the whole theme file format at its top. It's generated from the
code by touchy's tests, so it's always complete.

## Theme files

A theme file sets tokens in up to five sections, all optional:

```ron
(
    colors: {
        "touchy.palette.accent": "#D9622B",
        "touchy.palette.highlight": (from: "touchy.palette.accent", shade: Lighter(0.25)),
    },
    sizes: {
        "touchy.palette.corner_radius": 0.0,
    },
    fonts: {
        "touchy.palette.font": "Inter.ttf",
    },
    skins: {
        "touchy.skin.raised": (image: "plate.png", slice: 8.0),
    },
    state_shades: {
        "hover": Lighter(0.08),
    },
)
```

Paths are beside the theme file. The Ember palette in `assets/themes` is eight colours and
restyles all of touchy.

To use theme files, add the `ThemeSheetPlugin` and put sheets in the `ActiveTheme` resource's
two slots:

```rust,no_run
use bevy::prelude::*;
use touchy::prelude::*;
use touchy::theme::ThemeSheetPlugin;

fn main() {
    App::new()
        .add_plugins((DefaultPlugins, TouchyPlugins, ThemeSheetPlugin))
        .add_systems(Startup, choose_theme)
        .run();
}

fn choose_theme(server: Res<AssetServer>, mut active: ResMut<ActiveTheme>) {
    active
        .set_skin(server.load("themes/stone.theme.ron"))
        .set_palette(server.load("themes/ember.theme.ron"));
}
```

The palette is laid over the skin, token by token. A skin can suggest colours of its own, and
a palette chosen after it still wins. `clear_skin` and `clear_palette` take one away again.
A sheet waits for every picture and font it names, so the theme changes all at once. With
Bevy's `file_watcher` feature, saving a theme file restyles the running game.

## The premade themes

Touchy ships two themes, each a skin and a palette, compiled in by the `theme-lords` and
`theme-foundation` cargo features, which are on by default:

- **Lords**: carved wood, iron studs and brass plates for a fantasy game, in IM Fell English.
- **Foundation**: chamfered pixel-art panels with scanlines for science fiction, in Silkscreen,
  with an animated sprite-sheet orb on its switches.

```rust
# use bevy::prelude::*;
# use touchy::prelude::*;
use touchy::premade;

fn foundation(server: Res<AssetServer>, mut active: ResMut<ActiveTheme>) {
    active
        .set_skin(server.load(premade::foundation::SKIN))
        .set_palette(server.load(premade::foundation::PALETTE));
}
```

Mix them as you like: the Lords skin with the Foundation palette, or either skin with a palette
of your own. Their files are in the crate's `premade_themes` folder, and make good starting
points for your own.

## Skins

A skin draws a token in place of its flat colour. It's a stack of layers, all optional:
shadows, a fill, gradients, a border, a picture and an outline. Colours in a skin can follow
the token's own colour, so one skin serves every state and every palette:

```ron
"touchy.button.bg": (
    gradients: [Linear(stops: [(shade: Lighter(0.12)), (shade: Darker(0.12))])],
    shadows: [(color: "#00000080", offset: (0.0, 2.0), blur: 4.0)],
    transition: 0.15,
),
```

### Pictures

A picture is tinted by its token's colour: white shows the colour at full strength, and greys
shade it. So draw skin art in greys, with its light and shadow baked in, and every palette
colours it. A danger button and a primary button can share one picture.

Most pictures are **nine-sliced**. `slice: 8.0` keeps the 8-pixel corners at their size and
tiles the edges and middle, so a small picture covers a button of any size and a texture
repeats rather than smearing. To stretch instead, so a gradient or a light sweeps once across
the whole thing, say so:

```ron
"touchy.skin.raised": (
    image: "brass.png",
    slice: 24.0,
    sides: Stretch,
    center: Stretch,
    corner_scale: 0.25,
),
```

Stretching a small picture blurs it, so the Lords brass plate is drawn at four times the size it
appears. `corner_scale: 0.25` then draws its 24-pixel corners, rivets and all, at 6 pixels on
screen.

Other picture options: `tile: Both` repeats the whole picture, `flip: X` mirrors it,
`tinted: false` keeps the picture's own colours, for art that's meant to be exactly that colour,
and `inside_border: true` keeps it inside the node's border.

### Roles and exceptions

Give the five roles skins and every widget follows. Then skin individual tokens only for the
exceptions, and a token's own skin beats its role's:

```ron
skins: {
    "touchy.skin.raised": (image: "raised.png", slice: 8.0),
    "touchy.skin.raised:pressed": (image: "raised_pressed.png", slice: 8.0),
    // The danger button gets its own plate, still tinted red by the palette.
    "touchy.button.danger.bg": (image: "warning_plate.png", slice: 8.0),
    // The slider's thumb follows a different role.
    "touchy.slider.thumb": (from: "touchy.skin.raised"),
},
```

A role in a state reaches every widget in that state: the `raised:pressed` skin above shows on
every pressed button, stepper and dropdown.

### Sprite sheets

A picture can be a sheet of frames. `frames: (columns: 8)` cuts it into eight, and the widget
picks the frame: a slider's thumb by its value, a capsule switch's thumb by how far it has
glided. So a thumb can change as it moves, as Foundation's orb turns from cyan to violet when
its switch flips. Add `fps: 12.0`, and the frames loop on their own instead, as a spinner.

```ron
"touchy.capsule.thumb": (image: "flip_orb.png", frames: (columns: 8), tinted: false),
```

Your own nodes can do the same: give the node a `SkinFrame` from 0.0 (the first frame) to 1.0
(the last) and change it as you like.

## Theming from code

The `Theme` resource is the theme itself. Setting a token there overrides any sheet:

```rust
# use bevy::prelude::*;
# use touchy::prelude::*;
use touchy::theme::{Shade, palette};

fn high_contrast(mut theme: ResMut<Theme>) {
    theme
        .set_color(palette::TEXT, Color::WHITE)
        .set_derived(palette::HIGHLIGHT, palette::ACCENT, Shade::Lighter(0.3))
        .set_size(palette::TEXT_SIZE, 18.0);
}
```

Each widget's config takes a token set, so one widget can use different tokens from the rest:
`ButtonConfig::new("Delete").tokens(ButtonTokens::danger())` uses the danger button's tokens.

## Your own tokens

A game's own UI can be themed the same way. Name tokens under your game's prefix, give them
defaults in a plugin (following the palette, so they follow every theme), and bind your nodes
to them:

```rust
# use bevy::prelude::*;
# use touchy::prelude::*;
use touchy::theme::{Shade, ThemeSkin, ThemeTextColor, palette};

const MANA: ThemeToken = ThemeToken::new_static("game.mana");
const MANA_TEXT: ThemeToken = ThemeToken::new_static("game.mana.text");

fn plugin(app: &mut App) {
    let mut theme = app.world_mut().resource_mut::<Theme>();
    theme
        .default_derived(MANA, palette::INFO, Shade::Same)
        .default_derived(MANA_TEXT, MANA, Shade::Contrast);
}

fn spawn(hud: &mut ChildSpawnerCommands) {
    hud.spawn((Node::default(), ThemeSkin(MANA)))
        .with_children(|orb| {
            orb.spawn((Text::new("12"), ThemeTextColor(MANA_TEXT)));
        });
}
```

A default is only used until a sheet or the app sets the token, so a theme file can now set
`"game.mana"` too. Register defaults before touchy's, if they share a token: the first default
registered wins.

Canvases have a token set of their own, so a canvas can have its own background:

```rust
# use bevy::prelude::*;
# use touchy::prelude::*;
const WORLD: &str = "game.world";

fn plugin(app: &mut App) {
    let mut theme = app.world_mut().resource_mut::<Theme>();
    let world = CanvasTokens::with_prefix(WORLD);
    theme.default_color(world.bg.clone(), Color::srgb(0.10, 0.15, 0.12));
    world.register_theme_defaults(&mut theme);
}

fn spawn(page: &mut ChildSpawnerCommands) {
    let canvas = CanvasConfig::painted()
        .size(360.0, 360.0)
        .tokens(CanvasTokens::with_prefix(WORLD));
    page.canvas(canvas);
}
```

## Making a theme of your own

1. **Start with a palette.** Copy `assets/themes/ember.theme.ron` and change its colours. That
   alone restyles everything, and it's a good test of whether the colours work together.
2. **Draw the five roles** in greys, in one folder, on one grid: a surface, a raised plate and
   its pressed state, a sunken well, a knob and a mark. Keep the corners' detail inside the
   slice, and test at a button's size, a field's and a card's.
3. **Write the skin** with the roles, then the font and sizes, then exceptions only where a
   widget looks wrong.
4. **Try it over other palettes.** If your skin only works with its own colours, check for art
   that's coloured rather than grey, or colours the skin sets that belong in the palette.

The showcase is the place to try a theme: `cargo run --example showcase`, with its skin and
palette pickers in the top right.
