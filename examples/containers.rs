//! Demonstration of the different container types available in Touchy. These are all built from
//! Bevy's `Node` and Touchy widgets.
//!
//! ```text
//! cargo run --example containers
//! ```

#[path = "common/mod.rs"]
mod common;

use bevy::prelude::*;
use touchy::prelude::*;

/// The page's name, in its window and in the showcase's page list.
pub const TITLE: &str = "Containers";

// ARE THESE NECESSARY?
/// The tallest the scroll area grows before it scrolls.
const SCROLL_HEIGHT: f32 = 220.0;
/// Where the panel starts, from the page's top-left corner, and its size.
const PANEL_AT: Vec2 = Vec2::new(24.0, 440.0);
const PANEL_SIZE: Vec2 = Vec2::new(380.0, 240.0);

/// `--example` entry point.
fn main() {
    common::run_with(TITLE, page, plugin);
}

/// Spawns the page.
pub fn page(page: &mut ChildSpawnerCommands) {
    page.spawn(rain_area());
    page.row(|row| {
        // First column: A bare column.
        row.column(|body| {
            body.heading("Containers");
            body.bullets([
                "Bevy's UI uses Taffy, which is a lightweight flexbox layout engine.",
                "touchy adds a few container types and styles which are built around Bevy's `Node`.",
                "This text is in a bare `column` which is why it has no background or border.",
                "This entire page is in a `row` which is why the columns are side by side.",
            ]);
            body.divider();
            body.text("This is a touchy widget, which wraps Bevy's headless UiWidgets.");
            body.checkbox(CheckboxConfig::new()
                .label("Feel the rain on your skin!")
                .checked(false)
            ).insert(RainToggle);
        });

        // Second column: A card with a nested container.
        row.column(|card| {
            card.heading("Cards.");
            card.text("This text is in a `column`, with a `Card` component inserted.");
            card.divider();
            card.row(|card_row| {
                card_row.text("This text is in a `row` inside that card.");
                card_row.text("So is this. Nesting builds web-style layouts.");
            });
        })
        .insert(Card);

        // Third column: A scroll area in a card.
        row.column(|card| {
            card.heading("Scroll areas.");
            let scroll_config = ScrollAreaConfig::default()
                .stick_to_end(true)
                .max_height(SCROLL_HEIGHT);
            card.scroll_area(scroll_config, |scroll| {
                scroll.text("This text is in a `scroll_area`, which is a container that scrolls if its content exceed its height.");
                scroll.text("Scroll areas can be configured. This one sticks to the end, so it will follow new text when added.");
                scroll.column(|_| {})
                    .insert(LoremIpsum)
                    // IS THIS NECESSARY?
                    .node(|node| node.row_gap = px(0.0));
            });
            card.small("Use the slider below to add or remove text from the scroll area. The scroll area will automatically scroll to show the last text added.");
            let slider_config = SliderConfig::new(0.0)
                .range(0.0..=LOREM_IPSUM.len() as f32)
                .step(1.0);
            card.slider(slider_config).observe(show_lorem_ipsum);
        })
        .insert(Card);
    });

    // A floating panel over the page.
    let panel_config = PanelConfig::new()
        .placement(PanelPlacement::Floating(PANEL_AT))
        .size(PANEL_SIZE.x, PANEL_SIZE.y)
        .movable(GrabberStyle::default())
        .dock_targets(PanelDockTargets::ALL)
        .resizable(true);
    page.panel(panel_config, | panel| {
            panel.heading("Panels.");
            panel.text("This is a `panel`, which is a container than be moved and resized.");
            panel.text("Panels can be configured to be movable, resizable, and dockable.");
            panel.section(SectionConfig::new("Docking (click me!)").open(false), |section| {
                section.text("This text is in a `section`, which is a container that can be opened and closed.");
                section.text("This panel can be dragged by its grip and docked to the edges of the page.");
            });
            panel.spacer();
            let button_config = ButtonConfig::new("Panelception");
            panel.button(button_config).observe(toggle_nested_panel);
    }).insert(OuterPanel);
}

/// A marker for the panel that can spawn a nested panel. Also used by the example's minigame.
#[derive(Component)]
struct OuterPanel;

/// A marker for the nested panel to allow querying and despawning.
#[derive(Component)]
struct NestedPanel;

/// Add or remove the panel-within-a-panel (panelception).
fn toggle_nested_panel(
    activate: On<Activate>,
    outer: Single<Entity, With<OuterPanel>>,
    nested: Query<Entity, With<NestedPanel>>,
    mut commands: Commands,
) {
    // Despawn the outer panel and uncheck the button if it already exists.
    if let Ok(nested) = nested.single() {
        commands.entity(nested).despawn();
        commands.entity(activate.entity).remove::<Checked>();
        return;
    }

    // Docked in the outer panel, and movable, but confined within it.
    let nested_config = PanelConfig::new()
        .placement(PanelPlacement::Bottom)
        .dock_targets(PanelDockTargets::ALL)
        .movable(GrabberStyle::default())
        .resizable(true);
    commands.entity(*outer).with_children(|outer| {
        outer
            .panel(nested_config, |nested| {
                nested.heading("Nested panel.");
                nested.text("Just like other containers, panels can be nested.");
            })
            .insert(NestedPanel);
    });

    // Latch the button to the new panel's existence.
    commands.entity(activate.entity).insert(Checked);
}

// ================================
// Helper logic for the scroll area.

/// Marker for the element that will have text added/removed from it.
#[derive(Component)]
struct LoremIpsum;

/// A Study in Scarlet, Chapter 1, by Arthur Conan Doyle. Public domain.
const LOREM_IPSUM: &[&str] = &[
    "In the year 1878 I took my degree of Doctor of Medicine of the University of London, and",
    "proceeded to Netley to go through the course prescribed for surgeons in the army. Having",
    "completed my studies there, I was duly attached to the Fifth Northumberland Fusiliers as",
    "Assistant Surgeon. The regiment was stationed in India at the time, and before I could join",
    "it, the second Afghan war had broken out. On landing at Bombay, I learned that my corps had",
    "advanced through the passes, and was already deep in the enemy's country. I followed,",
    "however, with many other officers who were in the same situation as myself, and succeeded in",
    "reaching Candahar in safety, where I found my regiment, and at once entered upon my new",
    "duties. The campaign brought honours and promotion to many, but for me it had nothing but",
    "misfortune and disaster; I was removed from my brigade and attached to the Berkshires, with",
    "whom I served at the fatal battle of Maiwand. There I was struck on the shoulder by a Jezail",
    "bullet, which shattered the bone and grazed the subclavian artery. I should have fallen into",
    "the hands of the murderous Ghazis had it not been for the devotion and courage shown by",
    "Murray, my orderly, who threw me across a pack-horse, and succeeded in bringing me safely to",
    "the British lines.",
    "",
    "Worn with pain, and weak from the prolonged hardships which I had undergone, I was removed",
    "with a great train of wounded sufferers, to the base hospital at Peshawur. Here I rallied,
    and had already improved so far as to be able to walk about the wards, and even to bask a",
    "little upon the verandah, when I was struck down by enteric fever, that curse of our Indian",
    "possessions. For months my life was despaired of, and when at last I came to myself and",
    "became convalescent, I was so weak and emaciated that a medical board determined that not a",
    "day should be lost in sending me back to England. I was despatched, accordingly, in the",
    "troopship Orontes, and landed a month later on Portsmouth jetty, with my health",
    "irretrievably ruined, but with permission from a paternal government to spend the next nine",
    "months in attempting to improve it.",
    "",
    "I had neither kith nor kin in England, and was therefore as free as air—or as free as an",
    "income of eleven shillings and six-pence a day will permit a man to be. Under such",
    "circumstances I naturally gravitated to London, that great cesspool into which all the",
    "loungers and idlers of the Empire are irresistibly drained. There I stayed for some time at",
    "a private hotel in the Strand, leading a comfortless, meaningless existence, and spending",
    "such money as I had, considerably more freely than I ought. So alarming did the state of my",
    "finances become, that I soon realized that I must either leave the metropolis and rusticate",
    "somewhere in the country, or that I must make a complete alteration in my style of living.",
    "Choosing the latter alternative, I began by making up my mind to leave the hotel, and to",
    "take up my quarters in some less pretentious and less expensive domicile.",
    "",
    "On the very day that I had come to this conclusion, I was standing at the Criterion Bar,",
    "when some one tapped me on the shoulder, and turning round I recognized young Stamford, who",
    "had been a dresser under me at Barts. The sight of a friendly face in the great wilderness",
    "of London is a pleasant thing indeed to a lonely man. In old days Stamford had never been a",
    "particular crony of mine, but now I hailed him with enthusiasm, and he, in his turn,",
    "appeared to be delighted to see me. In the exuberance of my joy, I asked him to lunch with",
    "me at the Holborn, and we started off together in a hansom.",
    "",
    "'Whatever have you been doing with yourself, Watson?' he asked in undisguised wonder, as we",
    "rattled through the crowded London streets. 'You are as thin as a lath and as brown as a",
    "nut.'",
    "",
    "I gave him a short sketch of my adventures, and had hardly concluded it by the time that we",
    "reached our destination.",
    "",
    "'Poor devil!' he said, commiseratingly, after he had listened to my misfortunes. 'What are",
    "you up to now?'",
    "",
    "'Looking for lodgings,' I answered. 'Trying to solve the problem as to whether it is",
    "possible to get comfortable rooms at a reasonable price.'",
    "",
    "'That's a strange thing,' remarked my companion; 'you are the second man to-day that has",
    "used that expression to me.'",
    "",
    "'And who was the first?' I asked.",
    "",
    "'A fellow who is working at the chemical laboratory up at the hospital. He was bemoaning",
    "himself this morning because he could not get some one to go halves with him in some nice",
    "rooms which he had found, and which were too much for his purse.'",
];

fn show_lorem_ipsum(
    change: On<ValueChange<f32>>,
    text_lines: Single<(Entity, Option<&Children>), With<LoremIpsum>>,
    mut commands: Commands,
) {
    // The slider lands on whole steps, but mid-drag it might be between.
    let wanted = (change.value.round().max(0.0) as usize).min(LOREM_IPSUM.len());
    let (column, shown) = *text_lines;
    let shown = shown.map_or(&[][..], |children| &children[..]);
    // Too many: remove the excess.
    for &line in shown.iter().skip(wanted) {
        commands.entity(line).despawn();
    }
    // Too few: Add the missing lines.
    commands.entity(column).with_children(|column| {
        for &words in LOREM_IPSUM.iter().take(wanted).skip(shown.len()) {
            column.text(words);
        }
    });
}

// ===============================
// Minigame for the otherwise boring containers page: Rain.
// Rain drops sprinkle from the top of the page and collide with the drag-and-drop panel.

use touchy::theme::ThemeSkin;
use touchy::theme::palette::{ACCENT, HIGHLIGHT, INFO, SUCCESS, WARNING};

/// Drops/s.
const RAIN_RATE: f32 = 30.0;
/// Max on screen..
const MAX_DROPS: usize = 300;
/// A drop's size, in pixels: a little taller than wide.
const DROP_SIZE: Vec2 = Vec2::new(5.0, 8.0);
/// Initial downward velocity.
const DROP_SPEED: f32 = 120.0;
/// Gravity in px/s^2.
const GRAVITY: f32 = 400.0;
/// Speed kept on bouncing.
const BOUNCE: f32 = 0.35;
/// Sideways runoff speed.
const SPLASH: f32 = 90.0;
/// The palette colours a drop is painted with, chosen at random.
const DROP_COLOURS: [ThemeToken; 5] = [ACCENT, HIGHLIGHT, INFO, SUCCESS, WARNING];

/// The "minigame" for the containers page: rain that collides with the floating panel.
pub fn plugin(app: &mut App) {
    app.add_systems(Update, update_rain);
}

/// Marker for area the "rain" falls in.
#[derive(Component)]
struct RainArea;

/// An area as the page's first child. Z-index is behind everything else, but Pickable::IGNORE just
/// for good measure.
fn rain_area() -> impl Bundle {
    let fills = Node {
        position_type: PositionType::Absolute,
        left: px(0),
        top: px(0),
        width: percent(100),
        height: percent(100),
        ..default()
    };
    (RainArea, fills, Pickable::IGNORE)
}

/// Queryable component for the checkbox that toggles the rain on and off.
#[derive(Component)]
struct RainToggle;

/// A single raindrop: Drawn as a UI node.
#[derive(Component)]
struct RainDrop {
    /// (x,y) coords.
    at: Vec2,
    /// (x,y) velocity (px/s).
    velocity: Vec2,
}

/// Starts new drops, moves every drop, bounces drops off the panel, and despawns drops that have
/// fallen off the page. Drops are moved by `UiTransform` so they don't need to be laid out again
/// each frame.
#[expect(
    clippy::too_many_arguments,
    reason = "a system's arguments are what it reads; grouping them would only hide that"
)]
fn update_rain(
    time: Res<Time>,
    raining: Single<Has<Checked>, With<RainToggle>>,
    area: Single<(Entity, &ComputedNode, &UiGlobalTransform), With<RainArea>>,
    panel: Option<Single<(&ComputedNode, &UiGlobalTransform), With<OuterPanel>>>,
    mut drops: Query<(Entity, &mut RainDrop, &mut UiTransform)>,
    mut random: Local<Random>,
    mut drops_to_spawn: Local<f32>,
    mut commands: Commands,
) {
    let (area, area_node, area_place) = *area;
    let area_rect = logical_rect(area_node, area_place);
    if area_rect.width() <= 0.0 {
        return;
    }
    // The panel in the area's own space, if it's there.
    let panel_rect = panel.map(|panel| {
        let (node, transform) = *panel;
        let rect = logical_rect(node, transform);
        Rect::from_corners(rect.min - area_rect.min, rect.max - area_rect.min)
    });

    // Put the update schedule in real-time terms, then process rain drops.
    let dt = time.delta_secs();
    for (drop, mut rain, mut transform) in &mut drops {
        rain.velocity.y += GRAVITY * dt;
        let at = rain.at + rain.velocity * dt;
        let bottom = Vec2::new(at.x + DROP_SIZE.x / 2.0, at.y + DROP_SIZE.y);
        if let Some(panel) = panel_rect
            && rain.velocity.y > 0.0
            && panel.contains(bottom)
        {
            // Splash! Bounce off the panel and give a small sideways velocity if it didn't have one.
            rain.at = Vec2::new(at.x, panel.min.y - DROP_SIZE.y);
            rain.velocity.y *= -BOUNCE;
            if rain.velocity.x == 0.0 {
                let side = if random.next() < 0.5 { -1.0 } else { 1.0 };
                rain.velocity.x = side * SPLASH * (0.5 + random.next() / 2.0);
            }
        } else {
            rain.at = at;
        }
        if rain.at.y > area_rect.height() {
            commands.entity(drop).despawn();
            continue;
        }
        transform.translation = Val2::new(px(rain.at.x), px(rain.at.y));
    }

    // Early return if the rain toggle is off.
    if !*raining {
        return;
    }
    *drops_to_spawn += RAIN_RATE * dt;
    let starting = drops_to_spawn.floor();
    *drops_to_spawn -= starting;
    let room = MAX_DROPS.saturating_sub(drops.iter().len());
    let mut area = commands.entity(area);
    for _ in 0..(starting as usize).min(room) {
        let at = Vec2::new(random.next() * area_rect.width(), -DROP_SIZE.y);
        let pick = (random.next() * DROP_COLOURS.len() as f32) as usize;
        let colour = DROP_COLOURS[pick.min(DROP_COLOURS.len() - 1)].clone();
        let node = Node {
            position_type: PositionType::Absolute,
            width: px(DROP_SIZE.x),
            height: px(DROP_SIZE.y),
            border_radius: BorderRadius::MAX,
            ..default()
        };
        area.with_child((
            RainDrop {
                at,
                velocity: Vec2::new(0.0, DROP_SPEED),
            },
            node,
            ThemeSkin(colour),
            UiTransform::from_translation(Val2::new(px(at.x), px(at.y))),
            Pickable::IGNORE,
        ));
    }
}

/// Where a node is on screen, in logical pixels.
fn logical_rect(node: &ComputedNode, transform: &UiGlobalTransform) -> Rect {
    let scale = node.inverse_scale_factor;
    Rect::from_center_size(transform.translation * scale, node.size * scale)
}

/// Pseudorandom source: no need to use a random number generator crate.
struct Random(u32);

impl Default for Random {
    fn default() -> Self {
        Self(0x9E37_79B9)
    }
}

impl Random {
    /// The next xorshift: never stuck at zero from a non-zero start.
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0 as f32 / u32::MAX as f32
    }
}
