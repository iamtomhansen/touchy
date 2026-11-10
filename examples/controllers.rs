//! touchy's widgets as game controllers, steering a little "crawler" that paints a trail.
//!
//! This example shows how touchy's widgets can be configured and used.
//! - **D-pad**: D-pads can have variable frequency. Here, it nudges the crawler.
//! - **Gain stick**: Joysticks can be single-axis and don't need to reset themselves. Here, it
//!   scales the power of the D-pad.
//! - **Analog stick**: Mobile-style pop-up joystick.
//! - **Radial selector**: Pop-up radial selector menus.
//! - **Scrubbers**: For fine tuning numbers. Here, sets the trail size.
//! - **Transport**: Controls playback. Here, controls the paint queue.
//! - **Overlay**: The overlay dimmer from [`UiLayers`] blocks input.
//! - **Custom tokens**: The canvas and the analog stick have their own tokens, given defaults
//!   here, so a theme can restyle this page's widgets without touching anyone else's.
//!
//! Each frame, three systems run the game: one moves the crawler and queues where it went, one
//! paints from the queue at a steady rate, and one draws the crawler. Everything else is an
//! observer on the widget it listens to.
//!
//! ```text
//! cargo run --example controllers
//! ```

#[path = "common/mod.rs"]
mod common;

use std::collections::VecDeque;

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use touchy::prelude::*;

/// The page's name, in its window and in the showcase's page list.
pub const TITLE: &str = "Controllers";

/// Prefixes of this page's own theme tokens: `controllers.canvas.bg`, `controllers.stick.thumb`.
const CANVAS_STYLE: &str = "controllers.canvas";
const STICK_STYLE: &str = "controllers.stick";
/// Their defaults: a white canvas, and an analog stick you can see the paint through.
const CANVAS_COLOUR: Color = Color::WHITE;
const STICK_BG: Color = Color::srgba(0.10, 0.12, 0.16, 0.25);
const STICK_BORDER: Color = Color::srgba(0.30, 0.34, 0.42, 0.5);
const STICK_THUMB: Color = Color::srgba(0.26, 0.28, 0.33, 0.6);

/// The colours the crawler paints with, translucent so crossing trails mix.
const COLORS: [(&str, Color); 5] = [
    ("Cyan", Color::srgba(0.0, 1.0, 1.0, 0.2)),
    ("Magenta", Color::srgba(1.0, 0.0, 1.0, 0.2)),
    ("Yellow", Color::srgba(1.0, 1.0, 0.0, 0.2)),
    ("White", Color::srgba(1.0, 1.0, 1.0, 0.2)),
    ("Black", Color::srgba(0.0, 0.0, 0.0, 0.2)),
];
/// Wide enough for the transport bar: its buttons, and its readout, which keeps a steady width.
const CONTROL_PANEL_WIDTH: f32 = 400.0;
/// The canvas the crawler paints on, in pixels.
const CANVAS_SIZE: Vec2 = Vec2::new(640.0, 480.0);

/// Size of the crawler.
const CRAWLER_RADIUS: f32 = 7.0;
/// Opaque colour for the crawler itself.
const CRAWLER_COLOUR: Color = Color::srgb(0.10, 0.10, 0.12);
/// How much each D-pad nudge adds to the crawler's speed, at full gain, in pixels a second.
const NUDGE: f32 = 70.0;
/// How quickly the analog stick speeds the crawler up, at full tilt, in pixels a second squared.
const STICK_ACCELERATION: f32 = 600.0;
/// How quickly the crawler slows by itself: the fraction of its speed it keeps each second.
const DRAG: f32 = 0.35;
/// How many marks are painted from the queue each second while it plays. Faster than this, the
/// crawler queues marks quicker than they're painted, and the trail catches up after it.
const PAINT_RATE: f32 = 90.0;
/// A mark's size in pixels at a width and height of 1.
const MARK_BASE_SIZE: f32 = 16.0;
/// The most marks the canvas keeps; the oldest go first.
const MAX_MARKS: usize = 9999;
/// How far above the last each mark is drawn, so newer paint lies over older.
const MARK_LIFT: f32 = 0.0001;
/// How often a held D-pad direction repeats, in seconds, to start with, and its limits.
const REPEAT: f64 = 0.15;
/// The range of the repeat scrubber, in seconds.
const REPEAT_RANGE: (f64, f64) = (0.05, 1.0);
/// The range of the width and height scrubbers, as multiples of [`MARK_SIZE`].
const SIZE_RANGE: (f64, f64) = (0.1, 2.0);
/// The size of the swatches in the colour radial, in pixels.
const SWATCH: f32 = 20.0;

const CONFIRM_WIDTH: f32 = 320.0;

/// `--example` entry point.
fn main() {
    common::run_with(TITLE, page, plugin);
}

/// The game's three systems, and the defaults of this page's tokens. The rest are observers on
/// the widgets.
pub fn plugin(app: &mut App) {
    app.add_systems(
        Update,
        (move_crawler, paint_from_queue, draw_crawler).chain(),
    );
    let mut theme = app.world_mut().resource_mut::<Theme>();
    // The first default wins, so this page's go in before the widgets' own fill the rest.
    let canvas = CanvasTokens::with_prefix(CANVAS_STYLE);
    theme.default_color(canvas.bg.clone(), CANVAS_COLOUR);
    canvas.register_theme_defaults(&mut theme);
    let stick = JoystickTokens::with_prefix(STICK_STYLE);
    theme
        .default_color(stick.bg.clone(), STICK_BG)
        .default_color(stick.border.clone(), STICK_BORDER)
        .default_color(stick.thumb.clone(), STICK_THUMB);
    stick.register_theme_defaults(&mut theme);
}

/// Spawns the page.
pub fn page(page: &mut ChildSpawnerCommands) {
    page.row(|row| {
        row.column(build_controls)
            .insert(Card)
            .node(|node| node.width = px(CONTROL_PANEL_WIDTH));
        row.column(build_canvas).insert(Card);
    });
}

/// The canvas the crawler paints on, styled by this page's own tokens.
fn build_canvas(card: &mut ChildSpawnerCommands) {
    card.heading("Canvas");
    card.small(
        "This canvas is a touchy `canvas`: a node that can contain images and be decorated.",
    );
    card.small("Press to pop up an analog stick. `joystick` widget.");
    let canvas = CanvasConfig::painted()
        .size(CANVAS_SIZE.x, CANVAS_SIZE.y)
        .tokens(CanvasTokens::with_prefix(CANVAS_STYLE));
    card.canvas(canvas)
        .insert((PaintCanvas, Crawler::default()))
        .with_children(|field| {
            let stick = JoystickConfig::new()
                .floating(true)
                .tokens(JoystickTokens::with_prefix(STICK_STYLE));
            field.joystick(stick).insert(AnalogStick);
        });
}

/// The control panel card.
fn build_controls(card: &mut ChildSpawnerCommands) {
    // The `transport` widget has an associated Timeline. Here, we hide it in a hidden Node.
    let recording = card
        .spawn((
            Node {
                display: Display::None,
                ..default()
            },
            PaintQueue::default(),
        ))
        .insert((
            Timeline {
                playing: true,
                ..default()
            },
            TimelineLabels::default(),
        ))
        .observe(play_recording)
        .id();

    card.heading("Controls");
    card.small("This control panel is made using touchy's widgets.");

    card.divider();
    card.small("Move. `dpad` widget.");
    card.row(|row| {
        let pad = DpadConfig::new(DpadWays::Four).repeat(REPEAT as f32);
        row.dpad(pad).insert(MovePad).observe(nudge);
        row.column(|repeat| {
            repeat.small("D-pad frequency (s)");
            let config = NumberInputConfig::new(REPEAT)
                .range(REPEAT_RANGE.0, REPEAT_RANGE.1)
                .step(0.05)
                .precision(2);
            repeat.number_input(config).observe(set_repeat);
        });
    })
    .node(|node| {
        node.align_items = AlignItems::Center;
    });

    card.divider();
    card.small("D-pad strength (left inverts!). `joystick` widget.");
    let gain = JoystickConfig::new()
        .axes(JoystickAxes::Horizontal)
        .returns(false)
        .start(Vec2::new(0.5, 0.0));
    card.joystick(gain).insert(GainStick);

    card.spacer();
    card.divider();
    card.small("Colour picker. `radial` widget.");
    let colours = COLORS.map(|(name, colour)| swatch_for_selector(name, colour));
    card.radial_button(RadialConfig::new(colours).chosen(0))
        .observe(set_colour);

    card.divider();
    card.small("Trail size. `scrubber` widgets.");
    card.scrubber(size_scrubber("Width"))
        .observe(set_size(Dimension::Width));
    card.scrubber(size_scrubber("Height"))
        .observe(set_size(Dimension::Height));

    card.divider();
    card.small("Paint queue. `transport` widget. Try pausing it!");
    // Bevy's own font has only plain ASCII, so the symbols are made of it.
    let buttons = [
        TransportButton::new(TransportCommand::TogglePlay, ">")
            .on_label("||")
            .tooltip("Paint from the queue, or pause"),
        TransportButton::new(TransportCommand::Step(1), ">|").tooltip("Paint the next mark"),
        TransportButton::new(TransportCommand::JumpToLive, ">>|")
            .tooltip("Paint everything queued"),
    ];
    card.transport(
        TransportConfig::new(recording)
            .buttons(buttons)
            .track(false),
    );

    card.spacer();
    let clear = ButtonConfig::new("Clear").tokens(ButtonTokens::danger());
    card.button(clear).observe(ask_to_clear);
}

/// A colour's choice: a swatch of it, named.
fn swatch_for_selector(name: &'static str, colour: Color) -> Choice {
    Choice::new(name, move |choice| {
        let node = Node {
            width: px(SWATCH),
            height: px(SWATCH),
            border_radius: BorderRadius::MAX,
            ..default()
        };
        // Shown solid: the paint itself is translucent.
        choice.spawn((
            node,
            BackgroundColor(colour.with_alpha(1.0)),
            Pickable::IGNORE,
        ));
    })
}

fn size_scrubber(label: &str) -> ScrubberConfig {
    ScrubberConfig::new(1.0)
        .range(SIZE_RANGE.0, SIZE_RANGE.1)
        .step(0.1)
        .precision(1)
        .label(label)
}

// ================================
// "GAME" STATE

/// The `canvas` the crawler paints on: a literal painted canvas.
#[derive(Component)]
struct PaintCanvas;

/// The stick that appears wherever the canvas is pressed.
#[derive(Component)]
struct AnalogStick;

/// The 2-way stick that scales the D-pad's nudges.
#[derive(Component)]
struct GainStick;

/// The D-pad that nudges the crawler.
#[derive(Component)]
struct MovePad;

/// The crawler: where it is, how it's moving, and the paint it leaves.
#[derive(Component)]
struct Crawler {
    /// Where it is on the canvas, in pixels from the bottom-left corner.
    at: Vec2,
    /// How fast it's going, in pixels a second.
    velocity: Vec2,
    /// Where it last left a mark.
    last_mark: Vec2,
    /// The paint's current colour.
    colour: Color,
    /// The paint's width and height, as multiples of [`MARK_SIZE`].
    size: Vec2,
}

impl Default for Crawler {
    fn default() -> Self {
        let middle = CANVAS_SIZE / 2.0;
        Self {
            at: middle,
            velocity: Vec2::ZERO,
            last_mark: middle,
            colour: COLORS[0].1,
            size: Vec2::ONE,
        }
    }
}

/// One mark of paint.
#[derive(Clone, Copy, Debug)]
struct Mark {
    at: Vec2,
    colour: Color,
    size: Vec2,
}

/// Marks waiting to be painted: filled as the crawler moves, drained as it's painted.
#[derive(Component, Default)]
struct PaintQueue {
    /// Queued marks of paint, oldest first.
    waiting: VecDeque<Mark>,
    /// How many marks are on the canvas.
    painted: usize,
    /// Marks due to be painted but not yet, as the steady rate rarely makes a whole one a frame.
    due: f32,
}

/// A mark painted on the canvas.
#[derive(Component)]
struct Paint;

/// The question before clearing, on the overlay.
#[derive(Component)]
struct ConfirmClear;

#[derive(Clone, Copy)]
enum Dimension {
    Width,
    Height,
}

// ================================
// GAME SYSTEMS

/// Moves the crawler and updated its acceleration with inputs/drag, and reflects it
/// back at the canvas's edges. Drops marks of paint when distance is covered.
fn move_crawler(
    time: Res<Time>,
    canvas: Single<(&Canvas, &mut Crawler), With<PaintCanvas>>,
    stick: Single<&JoystickValue, With<AnalogStick>>,
    mut recording: Single<&mut PaintQueue>,
) {
    let (canvas, mut crawler) = canvas.into_inner();
    // The picture's own size, in the pixels the crawler moves in.
    let bounds = canvas.picture_size();
    // Not laid out yet: nowhere to move.
    if bounds.min_element() <= 0.0 {
        return;
    }
    let dt = time.delta_secs();
    // Read each frame, as a held stick sends nothing while it's still.
    crawler.velocity += stick.0 * STICK_ACCELERATION * dt;
    crawler.velocity *= DRAG.powf(dt);
    let mut at = crawler.at + crawler.velocity * dt;
    for axis in 0..2 {
        if !(0.0..=bounds[axis]).contains(&at[axis]) {
            at[axis] = at[axis].clamp(0.0, bounds[axis]);
            crawler.velocity[axis] = -crawler.velocity[axis];
        }
    }
    crawler.at = at;
    // Drop a mark if the crawler has gone far enough not to overlap the last one.
    if at.distance(crawler.last_mark) >= crawler.size.min_element() * MARK_BASE_SIZE {
        crawler.last_mark = at;
        let mark = Mark {
            at,
            colour: crawler.colour,
            size: crawler.size * MARK_BASE_SIZE,
        };
        recording.waiting.push_back(mark);
    }
}

/// Paint is drawn at a constant rate, even if the queue builds up. Can be paused and resumed.
fn paint_from_queue(
    time: Res<Time>,
    canvas: Single<&Canvas, With<PaintCanvas>>,
    queue: Single<(&mut PaintQueue, &Timeline)>,
    marks: Marks,
    mut commands: Commands,
) {
    let (mut queue, timeline) = queue.into_inner();
    let Some(stage) = canvas.stage() else {
        return;
    };
    // Paused, or nothing waiting: no marks fall due, so none pile up to rush out later.
    if !timeline.playing || queue.waiting.is_empty() {
        queue.due = 0.0;
        return;
    }
    queue.due += PAINT_RATE * time.delta_secs();
    let count = queue.due.floor();
    queue.due -= count;
    paint_marks(&mut queue, count as usize, stage, &marks, &mut commands);
}

/// Draws the crawler over the canvas, and keeps the transport bar's labels up to date.
fn draw_crawler(
    canvas: Single<(Entity, &Crawler), With<PaintCanvas>>,
    mut labels: Single<(&PaintQueue, &mut TimelineLabels)>,
    mut sketch: CanvasSketch,
) {
    let (canvas, crawler) = *canvas;
    let (recording, labels) = &mut *labels;
    if let Some(mut lines) = sketch.on(canvas) {
        lines.circle_2d(crawler.at, CRAWLER_RADIUS, CRAWLER_COLOUR);
    }
    let clock = format!("{} painted", recording.painted);
    let status = format!("{} waiting", recording.waiting.len());
    if labels.clock != clock || labels.status != status {
        **labels = TimelineLabels { clock, status };
    }
}

/// The marks on the canvas, oldest first.
#[derive(SystemParam)]
struct Marks<'w, 's> {
    stages: Query<'w, 's, &'static Children>,
    paint: Query<'w, 's, (), With<Paint>>,
}

/// Paints the next `count` waiting marks on `stage`, letting the oldest go past the limit.
fn paint_marks(
    recording: &mut PaintQueue,
    count: usize,
    stage: Entity,
    marks: &Marks,
    commands: &mut Commands,
) {
    let fresh: Vec<Mark> = recording
        .waiting
        .drain(..count.min(recording.waiting.len()))
        .collect();
    let mut painter = commands.entity(stage);
    for mark in fresh {
        // A mark's height above the last keeps newer paint on top, until the limit wraps it.
        let lift = (recording.painted % MAX_MARKS) as f32 * MARK_LIFT;
        painter.with_child((
            Paint,
            Sprite::from_color(mark.colour, mark.size),
            Transform::from_translation(mark.at.extend(lift)),
        ));
        recording.painted += 1;
    }
    let painted = marks
        .stages
        .get(stage)
        .map(|children| children.to_vec())
        .unwrap_or_default();
    let painted: Vec<Entity> = painted
        .into_iter()
        .filter(|&child| marks.paint.contains(child))
        .collect();
    for &old in painted.iter().take(painted.len().saturating_sub(MAX_MARKS)) {
        commands.entity(old).despawn();
    }
}

// ================================
// OBSERVERS ON THE WIDGETS

/// The D-pad nudges the crawler, scaled by the gain stick.
fn nudge(
    change: On<ValueChange<IVec2>>,
    gain: Single<&JoystickValue, With<GainStick>>,
    mut crawler: Single<&mut Crawler>,
) {
    crawler.velocity += change.value.as_vec2() * NUDGE * gain.0.x;
}

/// The number beside the D-pad sets how often a held direction repeats.
fn set_repeat(change: On<ValueChange<f64>>, mut pad: Single<&mut DpadSettings, With<MovePad>>) {
    pad.repeat = Some(change.value as f32);
}

/// The colour radial sets the crawler's paint colour.
fn set_colour(change: On<ValueChange<usize>>, mut crawler: Single<&mut Crawler>) {
    crawler.colour = COLORS[change.value].1;
}

/// A scrubber sets the paint's width or height.
fn set_size(dimension: Dimension) -> impl Fn(On<ValueChange<f64>>, Single<&mut Crawler>) {
    move |change, mut crawler| {
        let value = change.value as f32;
        match dimension {
            Dimension::Width => crawler.size.x = value,
            Dimension::Height => crawler.size.y = value,
        }
    }
}

/// The transport bar's requests: play or pause painting, paint the next mark, or paint
/// everything queued at once.
fn play_recording(
    request: On<TransportRequest>,
    mut recordings: Query<(&mut PaintQueue, &mut Timeline)>,
    canvas: Single<&Canvas, With<PaintCanvas>>,
    marks: Marks,
    mut commands: Commands,
) {
    let Ok((mut recording, mut timeline)) = recordings.get_mut(request.timeline) else {
        return;
    };
    match request.command {
        TransportCommand::TogglePlay => timeline.playing = !timeline.playing,
        TransportCommand::Step(steps) if steps > 0 => {
            if let Some(stage) = canvas.stage() {
                paint_marks(&mut recording, steps as usize, stage, &marks, &mut commands);
            }
        }
        TransportCommand::JumpToLive => {
            if let Some(stage) = canvas.stage() {
                let waiting = recording.waiting.len();
                paint_marks(&mut recording, waiting, stage, &marks, &mut commands);
            }
        }
        _ => {}
    }
}

/// Clearing asks first, demonstrating the overlay dimmer.
fn ask_to_clear(_: On<Activate>, asking: Query<(), With<ConfirmClear>>, mut commands: Commands) {
    if !asking.is_empty() {
        return;
    }
    commands
        .spawn((ConfirmClear, UiLayer::Overlay, Column, Card))
        .node(|node| node.width = px(CONFIRM_WIDTH))
        .with_children(|card| {
            card.heading("Clear the canvas?");
            card.text("Everything painted, and everything waiting, goes.");
            card.row(|row| {
                row.spacer();
                row.button(ButtonConfig::new("Keep it"))
                    .observe(close_confirm);
                let clear = ButtonConfig::new("Clear").tokens(ButtonTokens::danger());
                row.button(clear).observe(clear_all_marks);
            });
        });
}

/// Erase your masterpiece?
fn close_confirm(
    _: On<Activate>,
    asking: Query<Entity, With<ConfirmClear>>,
    mut commands: Commands,
) {
    for asking in &asking {
        commands.entity(asking).despawn();
    }
}

/// Clear the canvas and the queue.
fn clear_all_marks(
    _: On<Activate>,
    asking: Query<Entity, With<ConfirmClear>>,
    paint: Query<Entity, With<Paint>>,
    mut recording: Single<&mut PaintQueue>,
    mut commands: Commands,
) {
    for entity in asking.iter().chain(&paint) {
        commands.entity(entity).despawn();
    }
    recording.waiting.clear();
    recording.painted = 0;
}
