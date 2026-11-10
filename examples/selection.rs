//! A fake "character creation" UI, showing how different selection widgets work.
//!
//! ```text
//! cargo run --example selection
//! ```

#[path = "common/mod.rs"]
mod common;

use bevy::image::TRANSPARENT_IMAGE_HANDLE;
use bevy::prelude::*;
use touchy::prelude::*;

/// The page's name, in its window and in the showcase's page list.
pub const TITLE: &str = "Selection widgets";

/// Width of the control panel.
const CONTROL_PANEL_WIDTH: f32 = 340.0;

/// `--example` entry point.
fn main() {
    common::run_with(TITLE, page, plugin);
}

/// Spawns the page.
pub fn page(page: &mut ChildSpawnerCommands) {
    page.row(|row| {
        row.column(controls)
            .insert(Card)
            .node(|node| node.width = px(CONTROL_PANEL_WIDTH));
        row.column(view).insert(Card);
    });
}

/// The choices that pick parts of the character.
fn controls(card: &mut ChildSpawnerCommands) {
    card.heading("Create a character.");

    card.small("Species. `segmented` widget.");
    let species = SPECIES.map(|(name, ..)| name);
    card.segmented(
        SegmentedConfig::new(species)
            .chosen(0)
            .motion(ResponseCurve::Linear),
    )
    .observe(choose(|character, species| character.species = species));

    card.divider();
    card.small("Strength. `gearshift` widget.");
    card.gearshift(GearshiftConfig::new(STRENGTHS.len()).selected(1).pull(1.0))
        .observe(choose(|character, strength| character.strength = strength));
    card.row(|labels| {
        for (index, (name, _)) in STRENGTHS.iter().enumerate() {
            if index > 0 {
                labels.spacer();
            }
            labels.small(*name);
        }
    });

    card.divider();
    card.small("Alignment. `grid` widget.");
    card.grid(
        GridConfig::new(ALIGNMENTS, MOUTHS.len())
            .chosen(4)
            .motion(ResponseCurve::Power(1.5)),
    )
    .observe(choose(|character, cell| {
        character.eyes = cell / MOUTHS.len();
        character.mouth = cell % MOUTHS.len();
    }));

    card.divider();
    card.small(
        "Class. `dropdown` widget, showing that each `Choice` can be customized beyond text.",
    );
    let classes = CLASSES
        .iter()
        .enumerate()
        .map(|(index, &(name, _))| class_choice(index, name));
    card.dropdown(DropdownConfig::new(classes).chosen(0))
        .observe(choose(|character, class| character.class = class));
}

/// The character, with height, weight, and name widgets.
fn view(card: &mut ChildSpawnerCommands) {
    card.heading("Your character");
    card.row(|row| {
        let height = SliderConfig::new(1.0)
            .range(BUILD_RANGE)
            .vertical(true)
            .length(STAGE_SIZE.y);
        row.slider(height)
            .observe(build(|character, height| character.height = height));

        // Reserve space to avoid spilling onto the sliders.
        row.spawn((
            Character::default(),
            Node {
                width: px(STAGE_SIZE.x),
                height: px(STAGE_SIZE.y),
                ..default()
            },
        ))
        .with_children(|stage| {
            // The figure scales; the name under it doesn't.
            stage
                .spawn((Hero, view_node(), Pickable::IGNORE))
                .with_children(|view| {
                    // Back to front: reorder to suit the art.
                    for layer in [
                        Layer::Body,
                        Layer::Arms,
                        Layer::Armour,
                        Layer::Eyes,
                        Layer::Mouth,
                    ] {
                        view.spawn((
                            layer,
                            hero_node(),
                            ImageNode::new(TRANSPARENT_IMAGE_HANDLE),
                            Pickable::IGNORE,
                        ));
                    }
                });
            let name = ComboConfig::new(NAMES)
                .free_text(true)
                .placeholder("Name your hero")
                .columns(NAME_COLUMNS)
                .justify(Justify::Center);
            stage.spawn(bottom_ui_node()).with_children(|ui| {
                ui.combo(name);
            });
        });
    });
    let weight = SliderConfig::new(1.0)
        .range(BUILD_RANGE)
        .length(STAGE_SIZE.x);
    card.slider(weight)
        .observe(build(|character, weight| character.weight = weight))
        .node(|node| node.align_self = AlignSelf::End);
}

/// A node that fills its parent, for the character and its layers.
fn view_node() -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: px(0),
        top: px(0),
        width: percent(100),
        height: percent(100),
        ..default()
    }
}

/// A node for just the hero's artwork.
fn hero_node() -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: px((STAGE_SIZE.x - ART_SIZE.x) / 2.0),
        bottom: px(0),
        width: px(ART_SIZE.x),
        height: px(ART_SIZE.y),
        ..default()
    }
}

/// A centered node for UI elements at the bottom of the character.
fn bottom_ui_node() -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: px(0),
        bottom: px(0),
        width: percent(100),
        justify_content: JustifyContent::Center,
        ..default()
    }
}

/// A class's choice: its outfit, small, and its name. The picture is filled in by
/// [`character_creation`], which has the page's art, as a choice is built where there's none.
fn class_choice(index: usize, name: &'static str) -> Choice {
    Choice::new(name, move |choice| {
        choice
            .row(|row| {
                let image = Node {
                    width: px(CLASS_IMAGE_SIZE.x),
                    height: px(CLASS_IMAGE_SIZE.y),
                    ..default()
                };
                row.spawn((
                    ClassArmour(index),
                    image,
                    ImageNode::new(TRANSPARENT_IMAGE_HANDLE),
                    Pickable::IGNORE,
                ));
                row.spawn((text(name), Pickable::IGNORE));
            })
            .insert(Pickable::IGNORE)
            .node(|node| node.align_items = AlignItems::Center);
    })
}

// ================================
// CHARACTER CREATION PAGE LOGIC

/// Relative to $CRATE/assets/
const ASSET_PREFIX: &str = "examples/character/";

/// The species, each with its skin's theme token and that token's default colour.
const SPECIES: [(&str, ThemeToken, Color); 3] = [
    (
        "Elf",
        ThemeToken::new_static("character.skin.elf"),
        Color::srgb(0.45, 0.80, 0.40),
    ),
    (
        "Orc",
        ThemeToken::new_static("character.skin.orc"),
        Color::srgb(0.85, 0.35, 0.30),
    ),
    (
        "Human",
        ThemeToken::new_static("character.skin.human"),
        Color::srgb(0.95, 0.80, 0.45),
    ),
];
/// The strengths, each with its arms.
const STRENGTHS: [(&str, &str); 3] = [
    ("Weak", "arms_weak.png"),
    ("Average", "arms_average.png"),
    ("Strong", "arms_strong.png"),
];
/// The classes, each with its outfit.
const CLASSES: [(&str, &str); 3] = [
    ("Scout", "armour_scout.png"),
    ("Tank", "armour_tank.png"),
    ("Support", "armour_support.png"),
];
/// The alignment chart's rows, good to evil, each with its eyes.
const EYES: [(&str, &str); 3] = [
    ("Good", "eyes_good.png"),
    ("Neutral", "eyes_neutral.png"),
    ("Evil", "eyes_evil.png"),
];
/// The chart's columns, lawful to chaotic, each with its mouth.
const MOUTHS: [(&str, &str); 3] = [
    ("Lawful", "mouth_lawful.png"),
    ("Neutral", "mouth_neutral.png"),
    ("Chaotic", "mouth_chaotic.png"),
];
/// The chart's cells, row by row: lawful good to chaotic evil.
const ALIGNMENTS: [&str; 9] = [
    "Lawful Good",
    "Neutral Good",
    "Chaotic Good",
    "Lawful Neutral",
    "True Neutral",
    "Chaotic Neutral",
    "Lawful Evil",
    "Neutral Evil",
    "Chaotic Evil",
];
/// Names the combo box suggests; any other can be typed.
const NAMES: [&str; 4] = ["Legolith", "Azraz", "Orgorn", "Thom"];

/// The size every part is drawn at, in pixels.
const ART_SIZE: Vec2 = Vec2::new(300.0, 400.0);
/// How far height and weight stretch the character, either way.
const MAX_BUILD: f32 = 1.2;
/// How far height and weight stretch the character, either way.
const BUILD_RANGE: std::ops::RangeInclusive<f32> = 0.8..=MAX_BUILD;
/// The stage the character stands on: room for it at its tallest and widest.
const STAGE_SIZE: Vec2 = Vec2::new(ART_SIZE.x * MAX_BUILD, ART_SIZE.y * MAX_BUILD);
/// The class images in the dropdown: their outfit, shrunk.
const CLASS_IMAGE_SIZE: Vec2 = Vec2::new(24.0, 32.0);
/// The name field's width, in characters.
const NAME_COLUMNS: f32 = 16.0;

/// The page's one system, its pictures, and its tokens' defaults.
pub fn plugin(app: &mut App) {
    app.add_systems(Update, character_creation);
    let assets = app.world().resource::<AssetServer>();
    let load = |file: &str| assets.load(format!("{ASSET_PREFIX}{file}"));
    let art = CharacterArt {
        body: load("body.png"),
        arms: STRENGTHS.map(|(_, file)| load(file)),
        armour: CLASSES.map(|(_, file)| load(file)),
        eyes: EYES.map(|(_, file)| load(file)),
        mouths: MOUTHS.map(|(_, file)| load(file)),
    };
    app.insert_resource(art);
    let mut theme = app.world_mut().resource_mut::<Theme>();
    for (_, token, colour) in SPECIES {
        theme.default_color(token, colour);
    }
}

// ================================
// CHARACTER STATE

/// The character being created. Each different aspect that can be customized.
#[derive(Component)]
struct Character {
    height: f32,
    weight: f32,
    strength: usize,
    species: usize,
    eyes: usize,
    mouth: usize,
    class: usize,
}

impl Default for Character {
    fn default() -> Self {
        Self {
            height: 1.0,
            weight: 1.0,
            strength: 1,
            species: 0,
            // True neutral, the chart's middle.
            eyes: 1,
            mouth: 1,
            class: 0,
        }
    }
}

/// The character's parts, as one node that height and weight stretch.
#[derive(Component)]
struct Hero;

/// The different layers that are overlaid to create the character.
#[derive(Component, Clone, Copy)]
enum Layer {
    Body,
    Arms,
    Armour,
    Eyes,
    Mouth,
}

/// The artwork used for each class' armour.
#[derive(Component)]
struct ClassArmour(usize);

/// The character's pictures, loaded once.
#[derive(Resource)]
struct CharacterArt {
    body: Handle<Image>,
    arms: [Handle<Image>; 3],
    armour: [Handle<Image>; 3],
    eyes: [Handle<Image>; 3],
    mouths: [Handle<Image>; 3],
}

// ================================
// THE PAGE'S SYSTEM, AND ITS OBSERVERS

/// Query filter: class armour images that are not part of the character's layers, i.e. they are
/// in the dropdown's choices.
type NewClassOption = (Added<ClassArmour>, Without<Layer>);

/// Dresses the character whenever it, or the theme, changes: picks each layer's picture, tints
/// the skin, and stretches the figure. Also gives each new class logo its outfit.
fn character_creation(
    character: Single<Ref<Character>>,
    art: Res<CharacterArt>,
    theme: Res<Theme>,
    mut figure: Single<&mut UiTransform, With<Hero>>,
    mut layers: Query<(&Layer, &mut ImageNode)>,
    mut images: Query<(&ClassArmour, &mut ImageNode), NewClassOption>,
) {
    for (&ClassArmour(class), mut image) in &mut images {
        image.image = art.armour[class].clone();
    }
    if !character.is_changed() && !theme.is_changed() {
        return;
    }
    let skin = theme.color(&SPECIES[character.species].1);
    for (layer, mut image) in &mut layers {
        let (picture, tint) = match layer {
            Layer::Body => (&art.body, skin),
            Layer::Arms => (&art.arms[character.strength], skin),
            Layer::Armour => (&art.armour[character.class], Color::WHITE),
            Layer::Eyes => (&art.eyes[character.eyes], Color::WHITE),
            Layer::Mouth => (&art.mouths[character.mouth], Color::WHITE),
        };
        if image.image != *picture {
            image.image = picture.clone();
        }
        if image.color != tint {
            image.color = tint;
        }
    }
    // Stretched about its middle, then moved down by half what it grew, so its feet stay put.
    let scale = Vec2::new(character.weight, character.height);
    figure.scale = scale;
    figure.translation = Val2::new(px(0.0), px((1.0 - scale.y) * ART_SIZE.y / 2.0));
}

/// A choice's announcement sets one thing about the character.
fn choose(
    apply: fn(&mut Character, usize),
) -> impl Fn(On<ValueChange<usize>>, Single<&mut Character>) {
    move |change, mut character| apply(&mut character, change.value)
}

/// A slider's announcement sets one of the character's proportions.
fn build(apply: fn(&mut Character, f32)) -> impl Fn(On<ValueChange<f32>>, Single<&mut Character>) {
    move |change, mut character| apply(&mut character, change.value)
}
