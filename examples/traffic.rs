//! This tiny traffic simulation shows plots and keyed lists following a game's data.
//!
//! - **Controls and a canvas** set up and display a tiny traffic simulation.
//! - **A plot**: Touchy has simple plotting tools. Here, used to track traffic volume.
//! - **A keyed list** allows the UI to redraw only the rows that change, so it does not need to
//!   recalculate the whole list each frame. Here, the cars join the list when they are waiting at
//!   the light, and leave when they drive off. The rest of the list is left alone, so it can be
//!   very long without slowing the simulation.
//!
//! Each car drives itself: it speeds up towards a speed of its own, and brakes for the car ahead,
//! a red light, or a crossing in use. Careless drivers don't give way, so with both lights green
//! some of them crash: the wreck blocks the crossing for a few seconds, then is cleared away.
//!
//! Three systems run: one drives the cars and the light, one draws them, and one reports the
//! ID numbers to the plot and the list. The widgets' observers set the simulation's settings.
//!
//! ```text
//! cargo run --example traffic
//! ```

#[path = "common/mod.rs"]
mod common;

use std::collections::VecDeque;
use std::f32::consts::TAU;

use bevy::prelude::*;
use touchy::prelude::*;

/// The page's name, in its window and in the showcase's page list.
pub const TITLE: &str = "Traffic";

/// The control panel's width in pixels.
const CONTROL_PANEL_WIDTH: f32 = 300.0;
/// The report card's width in pixels.
const REPORT_WIDTH: f32 = 380.0;
/// The canvas for the traffic crossing (square). Roads cross in its middle.
const SIM_WORLD_SIZE: f32 = 360.0;
/// Prefix of the simulation canvas's theme tokens: `traffic.world.bg`.
const SIM_WORLD_STYLE: &str = "traffic.world";
/// The simulation canvas's background unless a theme sets it.
const SIM_WORLD_COLOUR: Color = Color::srgb(0.12, 0.15, 0.12);
/// Drawn width of the roads, in pixels.
const ROAD_WIDTH: f32 = 36.0;
/// Where the crossing itself starts (corner 1 of the square).
const CROSSING_BOX_START: f32 = (SIM_WORLD_SIZE - ROAD_WIDTH) / 2.0;
/// Where the crossing itself ends (corner 2 of the square).
const CROSSING_BOX_END: f32 = (SIM_WORLD_SIZE + ROAD_WIDTH) / 2.0;
/// Where a car's front stops for a red light: just short of the other road.
const STOP_LINE: f32 = CROSSING_BOX_START - 4.0;
/// A car size (length x width).
const CAR_SIZE: Vec2 = Vec2::new(20.0, 12.0);

/// Target speed of each car, in pixels/second.
const CRUISE_SPEED: f32 = 90.0;
/// How much each car's speed varies from the target, in pixels/second.
const CRUISE_VARIETY: f32 = 0.2;
/// How hard the car accelerates, in pixels/second squared.
const ACCELERATION: f32 = 60.0;
/// How hard the car brakes, in pixels/second squared.
const COMFORTABLE_BRAKING: f32 = 90.0;
/// How hard the car brakes in an emergency, in pixels/second squared.
const HARDEST_BRAKING: f32 = 400.0;
/// The gap a car tries to leave ahead of it when stopped.
const STANDSTILL_GAP: f32 = 8.0;
/// The time a car keeps behind the car ahead when moving.
const TIME_GAP: f32 = 0.7;
/// The simulation's step, in seconds.
const STEP: f32 = 1.0 / 120.0;
/// The maximum number of steps a slow frame runs: a slow frame runs several.
const MOST_STEPS: u32 = 12;
/// Below this speed a car counts as waiting, in pixels per second.
const WAITING_SPEED: f32 = 15.0;
/// Braking harder than this shows the car's brake lights, in pixels per second squared.
const BRAKE_LIGHT_THRESHOLD: f32 = 20.0;
/// How far behind a car its brake lights are drawn, in pixels, clear of its outline.
const BRAKE_LIGHT_GAP: f32 = 3.0;

/// How long a crash blocks the crossing, in seconds.
const CRASH_TIME: f32 = 3.0;
/// How long a wreck then takes to fade away, in seconds.
const CRASH_FADE: f32 = 1.0;
/// Which seed a car's carelessness is drawn from, so it isn't tied to its cruising speed.
const CARELESS_SEED: u32 = 0x5bd1_e995;

/// How long both lights stay red between greens, so the crossing clears.
const CLEARANCE: f32 = 1.0;

/// How often the plot takes a sample, in seconds, and how many it keeps: a minute's worth.
const SAMPLE_EVERY: f32 = 0.5;
/// How many samples the plot keeps: a minute's worth.
const SAMPLES: usize = 120;
/// The longest wait the list's meters show, in seconds; longer waits fill them.
const LONGEST_WAIT: f32 = 30.0;
/// The default arrivals, cars a minute, and green times, seconds; and their limits.
const ARRIVALS: f64 = 20.0;
/// Limits for the arrival rate.
const ARRIVAL_RANGE: (f64, f64) = (0.0, 60.0);
/// The default green time, in seconds, and its limits.
const GREEN: f64 = 6.0;
/// Limits for the green light time.
const GREEN_RANGE: (f64, f64) = (2.0, 20.0);
/// The default share of careless drivers, in percent.
const CARELESS: f64 = 10.0;
/// Limits for the share of careless drivers.
const CARELESS_RANGE: (f64, f64) = (0.0, 100.0);
/// The ways the lights can run: the capsule's off side, then its on side.
const SIGNALS: (&str, &str) = ("Take turns", "Both green");

/// Colour of the roads.
const ROAD_COLOUR: Color = Color::srgb(0.30, 0.30, 0.40);
/// Colour of the red lights.
const STOP: Color = Color::srgb(0.90, 0.25, 0.20);
/// Colour of the green lights.
const GO: Color = Color::srgb(0.30, 0.85, 0.40);
/// Colour of the burst around a wreck.
const CRASH: Color = Color::srgb(1.00, 0.60, 0.15);
/// How tall the waiting list grows before it scrolls, in pixels.
const LIST_HEIGHT: f32 = 220.0;
/// The plot's height: more cars than fit before the stop line.
const MOST_WAITING: f32 = 8.0;

/// The two roads, one way each: west to east, and south to north.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Road {
    EastWest,
    NorthSouth,
}

impl Road {
    const BOTH: [Road; 2] = [Road::EastWest, Road::NorthSouth];
    /// Index representation of the road (0 => East-west, 1 => North-south).
    fn index(self) -> usize {
        self as usize
    }

    /// The road crossing this one.
    fn other(self) -> Road {
        match self {
            Road::EastWest => Road::NorthSouth,
            Road::NorthSouth => Road::EastWest,
        }
    }

    /// String representation of the road, for labels and plot series names.
    fn name(self) -> &'static str {
        match self {
            Road::EastWest => "East-west",
            Road::NorthSouth => "North-south",
        }
    }

    /// This road's colour, for its cars and its line on the plot. Set by them tokens.
    fn token(self) -> ThemeToken {
        match self {
            Road::EastWest => ThemeToken::new_static("traffic.road.east_west"),
            Road::NorthSouth => ThemeToken::new_static("traffic.road.north_south"),
        }
    }

    /// The colour [`token`](Self::token) has unless a theme sets it.
    fn default_colour(self) -> Color {
        match self {
            Road::EastWest => Color::srgb(0.30, 0.80, 0.90),
            Road::NorthSouth => Color::srgb(0.90, 0.80, 0.30),
        }
    }

    /// Where a car `along` this road is on the canvas, and which way it's facing.
    fn place(self, along: f32) -> (Vec2, Vec2) {
        let middle = SIM_WORLD_SIZE / 2.0;
        match self {
            Road::EastWest => (Vec2::new(along, middle), Vec2::X),
            Road::NorthSouth => (Vec2::new(middle, along), Vec2::Y),
        }
    }
}

/// `--example` entry point.
fn main() {
    common::run_with(TITLE, page, plugin);
}

/// The simulation's systems, and its drawn colours.
pub fn plugin(app: &mut App) {
    app.add_systems(
        Update,
        (drive_cars, draw_crossing, gather_report_data).chain(),
    );
    let mut theme = app.world_mut().resource_mut::<Theme>();
    for road in Road::BOTH {
        theme.default_color(road.token(), road.default_colour());
    }
    // The first default wins, so the world's goes in before the canvas's own fills the rest.
    let world = CanvasTokens::with_prefix(SIM_WORLD_STYLE);
    theme.default_color(world.bg.clone(), SIM_WORLD_COLOUR);
    world.register_theme_defaults(&mut theme);
}

/// Spawns the page.
pub fn page(page: &mut ChildSpawnerCommands) {
    page.row(|row| {
        // First column: Control panel.
        row.column(build_control_panel)
            .insert(Card)
            .node(|node| node.width = px(CONTROL_PANEL_WIDTH));

        // Second column: Traffic view.
        row.column(|card| {
            card.heading("Traffic simulation");
            let canvas = CanvasConfig::painted()
                .size(SIM_WORLD_SIZE, SIM_WORLD_SIZE)
                .line_width(2.0)
                .tokens(CanvasTokens::with_prefix(SIM_WORLD_STYLE));
            card.canvas(canvas).insert(Traffic::default());
            card.small("").insert(Throughput);
        })
        .insert(Card);

        // Third column: Reports.
        row.column(build_reports)
            .insert(Card)
            .node(|node| node.width = px(REPORT_WIDTH));
    });
}

/// The user-configurable traffic control settings.
fn build_control_panel(card: &mut ChildSpawnerCommands) {
    card.heading("Controls");
    card.small("Arrivals, cars a minute. `scrubber` widgets.");
    for road in Road::BOTH {
        let config = scrubber_helper(road.name(), ARRIVALS, ARRIVAL_RANGE);
        card.scrubber(config).observe(set(move |traffic, value| {
            traffic.arrivals[road.index()] = value;
        }));
    }
    card.divider();
    card.small("Signals. `capsule_flip` widget.");
    card.capsule_flip(CapsuleFlipConfig::new().options(SIGNALS.0, SIGNALS.1))
        .observe(choose_signals);
    card.small("Green light, seconds, when they take turns.");
    for road in Road::BOTH {
        let config = scrubber_helper(road.name(), GREEN, GREEN_RANGE);
        card.scrubber(config).observe(set(move |traffic, value| {
            traffic.green[road.index()] = value;
        }));
    }
    card.small("Careless drivers, percent of arrivals. They don't give way.");
    let config = scrubber_helper("Careless", CARELESS, CARELESS_RANGE);
    card.scrubber(config).observe(set(|traffic, value| {
        traffic.careless = value / 100.0;
    }));
    card.divider();
    card.bullets([
        "Give one road more green than its arrivals need, and watch the other queue up.",
        "Watch a queue start: each car waits for the gap ahead to open, so a wave runs back.",
        "With both green, cars take turns at the crossing by themselves.",
        "Careless drivers don't avoid collisions, so if both lights are green, some will crash.",
    ]);
}

/// Abstracted logic for ScrubberConfigs to save on LOC.
fn scrubber_helper(label: &str, value: f64, range: (f64, f64)) -> ScrubberConfig {
    ScrubberConfig::new(value)
        .range(range.0, range.1)
        .step(1.0)
        .precision(0)
        .label(label)
}

/// The plot of waiting cars, and the list of who's waiting.
/// The list is keyed by each car's ID number, so updates only redraw the rows that change.
fn build_reports(card: &mut ChildSpawnerCommands) {
    card.heading("Plots and keyed lists");
    card.small("Cars waiting on each road, over the last minute. `plot` readout.");
    let mut plot = PlotConfig::new()
        .y_range(0.0..=MOST_WAITING)
        .size(REPORT_WIDTH - 24.0, 180.0);
    for road in Road::BOTH {
        let series = PlotSeries::new(road.name(), Vec::<Vec2>::new()).token(road.token());
        plot = plot.with_series(series);
    }
    card.plot(plot).insert(WaitingPlot);

    card.divider();
    card.small("Each car waiting, longest first. A keyed list: rows come and go with their cars.");
    let list = ScrollAreaConfig::default().max_height(LIST_HEIGHT);
    card.scroll_area(list, |view| {
        view.column(|_| {}).insert(WaitingList);
    });
}

// ================================
// THE SIMULATION'S STATE

/// The simulation. Attached to the canvas it's drawn on.
#[derive(Component)]
struct Traffic {
    /// Cars arriving on each road, a minute.
    arrivals: [f32; 2],
    /// How long each road's light stays green, in seconds.
    green: [f32; 2],
    /// Whether the lights take turns, or both stay green.
    both_green: bool,
    /// The share of arriving drivers who are careless, from 0 to 1.
    careless: f32,
    /// All current cars.
    cars: Vec<Car>,
    /// Current traffic light state.
    light: Light,
    /// Arrivals due on each road but not yet in, as a car rarely arrives on a frame's edge.
    due: [f32; 2],
    /// Cars that have crossed, on each road.
    crossed: [u32; 2],
    /// Crashes so far.
    crashes: u32,
    /// The next ID number to assign to a new car.
    next_number: u32,
    /// The plot's samples: when, and how many waited on each road.
    history: VecDeque<(f32, [usize; 2])>,
    /// The simulation's clock, in seconds.
    clock: f32,
    /// Time since the last sample, in seconds.
    since_sample: f32,
    /// Time not yet simulated, less than a step.
    unsimulated: f32,
}

impl Default for Traffic {
    fn default() -> Self {
        Self {
            arrivals: [ARRIVALS as f32; 2],
            green: [GREEN as f32; 2],
            both_green: false,
            careless: CARELESS as f32 / 100.0,
            cars: Vec::new(),
            light: Light::Green(Road::EastWest, 0.0),
            due: [0.0; 2],
            crossed: [0; 2],
            crashes: 0,
            next_number: 1,
            history: VecDeque::new(),
            clock: 0.0,
            since_sample: 0.0,
            unsimulated: 0.0,
        }
    }
}

impl Traffic {
    /// Whether `road` has a green light now.
    fn is_green(&self, road: Road) -> bool {
        self.both_green || self.light.is_green(road)
    }
}

/// The traffic light: Can only be green on one road at a time, with a gap between changes.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Light {
    Green(Road, f32),
    Clearing { next: Road, elapsed: f32 },
}

impl Light {
    fn is_green(self, road: Road) -> bool {
        matches!(self, Light::Green(green, _) if green == road)
    }

    /// The light `dt` later.
    fn advance(self, dt: f32, green: [f32; 2]) -> Self {
        match self {
            Light::Green(road, elapsed) if elapsed + dt >= green[road.index()] => Light::Clearing {
                next: road.other(),
                elapsed: 0.0,
            },
            Light::Green(road, elapsed) => Light::Green(road, elapsed + dt),
            Light::Clearing { next, elapsed } if elapsed + dt >= CLEARANCE => {
                Light::Green(next, 0.0)
            }
            Light::Clearing { next, elapsed } => Light::Clearing {
                next,
                elapsed: elapsed + dt,
            },
        }
    }
}

/// One car: its ID number, its road, how far along it is (its front, in pixels), how it's moving,
/// and how long it's been waiting.
#[derive(Clone, Copy, Debug)]
struct Car {
    id: u32,
    road: Road,
    along: f32,
    speed: f32,
    acceleration: f32,
    cruise: f32,
    waited: f32,
    careless: bool,
    /// If the car has crashed, the time it crashed.
    wrecked: Option<f32>,
}

impl Car {
    /// Whether it's waiting now: stopped, or nearly.
    fn is_waiting(&self) -> bool {
        self.speed < WAITING_SPEED
    }

    /// Whether any of it is in the crossing.
    fn in_crossing(&self) -> bool {
        self.along > CROSSING_BOX_START && self.along - CAR_SIZE.x < CROSSING_BOX_END
    }

    /// Whether any of it is where the other road's cars drive, allowing collisions.
    fn in_the_way(&self) -> bool {
        let middle = SIM_WORLD_SIZE / 2.0;
        let half = CAR_SIZE.y / 2.0;
        self.along > middle - half && self.along - CAR_SIZE.x < middle + half
    }

    /// Whether its wreck has faded away, so it can be cleared.
    fn cleared(&self) -> bool {
        self.wrecked
            .is_some_and(|age| age >= CRASH_TIME + CRASH_FADE)
    }

    /// How hard it speeds up (or, below zero, brakes) for a free road, and for something
    /// `gap` pixels ahead going at `ahead_speed`: semi-intelligent driver model.
    fn acceleration(&self, ahead: Option<(f32, f32)>) -> f32 {
        let free = 1.0 - (self.speed / self.cruise).powi(4);
        let Some((gap, ahead_speed)) = ahead else {
            return ACCELERATION * free;
        };
        let closing = self.speed - ahead_speed;
        let wanted = STANDSTILL_GAP
            + self.speed * TIME_GAP
            + self.speed * closing / (2.0 * (ACCELERATION * COMFORTABLE_BRAKING).sqrt());
        let crowded = (wanted.max(0.0) / gap.max(0.1)).powi(2);
        ACCELERATION * (free - crowded)
    }
}

/// A number from 0 to 1 for car `id`, a different one for each `seed`, so cars differ from each
/// other without a random number source.
fn spread(id: u32, seed: u32) -> f32 {
    ((id ^ seed).wrapping_mul(2_654_435_761) >> 16) as f32 / u16::MAX as f32
}

/// Uses car ID number to give each car a slightly different cruise speed.
fn cruise_for(id: u32) -> f32 {
    CRUISE_SPEED * (1.0 + CRUISE_VARIETY * (spread(id, 0) * 2.0 - 1.0))
}

/// The line under the crossing counting the cars through.
#[derive(Component)]
struct Throughput;

/// The plot showing the count of cars waiting on each road.
#[derive(Component)]
struct WaitingPlot;

/// The list of cars waiting on the light, keyed by each car's ID.
#[derive(Component)]
struct WaitingList;

/// A row of the waiting list: the data its keyed row shows. Its wait is in tenths of a second,
/// so the row changes ten times a second rather than every frame.
#[derive(Clone, PartialEq)]
struct Waiting {
    id: u32,
    road: Road,
    tenths: u32,
}

// ================================
// THE SIMULATION'S SYSTEMS

/// Runs the simulation in fixed steps, however long the frame was.
fn drive_cars(time: Res<Time>, mut traffic: Single<&mut Traffic>) {
    // The plain struct, so its fields can be borrowed apart.
    let traffic = &mut **traffic;
    traffic.unsimulated += time.delta_secs();
    let mut steps = 0;
    while traffic.unsimulated >= STEP && steps < MOST_STEPS {
        traffic.unsimulated -= STEP;
        step(traffic, STEP);
        steps += 1;
    }
    // Too far behind: let go of the rest rather than fall further.
    if steps == MOST_STEPS {
        traffic.unsimulated = 0.0;
    }
}

/// One step: turns the light, brings cars in, and drives each car for `dt`.
fn step(traffic: &mut Traffic, dt: f32) {
    traffic.clock += dt;
    traffic.light = traffic.light.advance(dt, traffic.green);

    // Arrivals, each at its road's start, when there's room there.
    for road in Road::BOTH {
        let due = &mut traffic.due[road.index()];
        *due = (*due + traffic.arrivals[road.index()] / 60.0 * dt).min(1.0);
        let last = traffic
            .cars
            .iter()
            .filter(|car| car.road == road)
            .min_by(|a, b| a.along.total_cmp(&b.along));
        let room = last.is_none_or(|car| car.along > CAR_SIZE.x + STANDSTILL_GAP * 2.0);
        if *due >= 1.0 && room {
            *due -= 1.0;
            let id = traffic.next_number;
            traffic.next_number += 1;
            let cruise = cruise_for(id);
            // In at the speed of the car ahead, if it's slower, so it isn't on its bumper.
            let speed = last.map_or(cruise, |car| car.speed.min(cruise));
            traffic.cars.push(Car {
                id,
                road,
                along: 0.0,
                speed,
                acceleration: 0.0,
                cruise,
                waited: 0.0,
                careless: spread(id, CARELESS_SEED) < traffic.careless,
                wrecked: None,
            });
        }
    }

    // Front cars first, so each car knows where the one ahead has got to, and which cars are
    // in the crossing before the next decides whether to go in.
    traffic.cars.sort_by(|a, b| b.along.total_cmp(&a.along));
    let mut ahead: [Option<(f32, f32)>; 2] = [None; 2];
    let mut in_crossing = [false; 2];
    for car in &traffic.cars {
        in_crossing[car.road.index()] |= car.in_crossing();
    }
    // Everyone stops for a wreck (careless or not).
    let wreck_in_crossing = traffic
        .cars
        .iter()
        .any(|car| car.wrecked.is_some() && car.in_crossing());
    for index in 0..traffic.cars.len() {
        let car = traffic.cars[index];
        let road = car.road;
        // A wreck stays where it is until it's cleared, in the way of the cars behind it.
        if let Some(age) = car.wrecked {
            traffic.cars[index].wrecked = Some(age + dt);
            ahead[road.index()] = Some((car.along - CAR_SIZE.x, 0.0));
            continue;
        }
        // What's ahead: the car in front, and a stop at the line when the light is red or the
        // other road is in the crossing, unless it's too late to stop.
        let mut acceleration =
            car.acceleration(ahead[road.index()].map(|(back, speed)| (back - car.along, speed)));
        let to_line = STOP_LINE - car.along;
        let can_stop = car.speed * car.speed / (2.0 * HARDEST_BRAKING) < to_line;
        let gives_way = !car.careless && in_crossing[road.other().index()];
        let blocked = !traffic.is_green(road) || wreck_in_crossing || gives_way;
        if to_line >= 0.0 && can_stop && blocked {
            let for_line = car.acceleration(Some((to_line + STANDSTILL_GAP, 0.0)));
            acceleration = acceleration.min(for_line);
        }
        let acceleration = acceleration.max(-HARDEST_BRAKING);

        let mut moved = car;
        moved.acceleration = acceleration;
        moved.speed = (car.speed + acceleration * dt).max(0.0);
        moved.along = car.along + moved.speed * dt;
        // Never into the car ahead, whatever the model says.
        if let Some((back, speed)) = ahead[road.index()]
            && moved.along > back
        {
            moved.along = back;
            moved.speed = speed;
        }
        if moved.is_waiting() {
            moved.waited += dt;
        }
        if car.along < CROSSING_BOX_END && moved.along >= CROSSING_BOX_END {
            traffic.crossed[road.index()] += 1;
        }
        // Into the crossing: the other road now waits for it.
        in_crossing[road.index()] |= moved.in_crossing();
        ahead[road.index()] = Some((moved.along - CAR_SIZE.x, moved.speed));
        traffic.cars[index] = moved;
    }
    if collide(&mut traffic.cars) {
        traffic.crashes += 1;
    }
    traffic
        .cars
        .retain(|car| car.along < SIM_WORLD_SIZE + CAR_SIZE.x && !car.cleared());

    // The plot's samples.
    traffic.since_sample += dt;
    if traffic.since_sample >= SAMPLE_EVERY {
        traffic.since_sample = 0.0;
        let waiting = Road::BOTH.map(|road| waiting_on(&traffic.cars, road).count());
        traffic.history.push_back((traffic.clock, waiting));
        if traffic.history.len() > SAMPLES {
            traffic.history.pop_front();
        }
    }
}

/// Wrecks intersecting cars, and returns whether any new wrecks happened.
fn collide(cars: &mut [Car]) -> bool {
    let in_the_way = |road| cars.iter().any(|car| car.road == road && car.in_the_way());
    if !Road::BOTH.into_iter().all(in_the_way) {
        return false;
    }
    let mut crashed = false;
    for car in cars
        .iter_mut()
        .filter(|car| car.in_the_way() && car.wrecked.is_none())
    {
        car.wrecked = Some(0.0);
        car.speed = 0.0;
        car.acceleration = 0.0;
        crashed = true;
    }
    crashed
}

/// The cars waiting on `road`.
fn waiting_on(cars: &[Car], road: Road) -> impl Iterator<Item = &Car> {
    cars.iter()
        .filter(move |car| car.road == road && car.waited > 0.0)
}

/// Draws the roads, the lights, and the cars, with brake lights on those braking.
fn draw_crossing(traffic: Single<(Entity, &Traffic)>, theme: Res<Theme>, mut sketch: CanvasSketch) {
    let (canvas, traffic) = *traffic;
    let Some(mut lines) = sketch.on(canvas) else {
        return;
    };
    let middle = SIM_WORLD_SIZE / 2.0;
    let half = ROAD_WIDTH / 2.0;
    for edge in [middle - half, middle + half] {
        lines.line_2d(
            Vec2::new(0.0, edge),
            Vec2::new(SIM_WORLD_SIZE, edge),
            ROAD_COLOUR,
        );
        lines.line_2d(
            Vec2::new(edge, 0.0),
            Vec2::new(edge, SIM_WORLD_SIZE),
            ROAD_COLOUR,
        );
    }
    // Each road's light, at its stop line, beside the road.
    for road in Road::BOTH {
        let colour = if traffic.is_green(road) { GO } else { STOP };
        let (at, facing) = road.place(STOP_LINE);
        let beside = at + facing.perp() * -(half + 10.0);
        for radius in [2.0, 4.0, 6.0] {
            lines.circle_2d(beside, radius, colour);
        }
    }
    for car in &traffic.cars {
        let (front, facing) = car.road.place(car.along);
        let centre = front - facing * CAR_SIZE.x / 2.0;
        let size = if facing == Vec2::X {
            CAR_SIZE
        } else {
            CAR_SIZE.yx()
        };
        // A wreck shows for a while, then fades.
        let fade = car.wrecked.map_or(1.0, |age| {
            1.0 - ((age - CRASH_TIME) / CRASH_FADE).clamp(0.0, 1.0)
        });
        let colour = theme.color(&car.road.token()).with_alpha(fade);
        lines.rect_2d(centre, size, colour);
        // Careful drivers' cars are drawn with a second outline.
        if !car.careless {
            lines.rect_2d(centre, size - Vec2::splat(4.0), colour);
        }
        if car.wrecked.is_some() {
            for ray in 0..8 {
                let out = Vec2::from_angle(ray as f32 * TAU / 8.0);
                lines.line_2d(
                    centre + out * 10.0,
                    centre + out * 16.0,
                    CRASH.with_alpha(fade),
                );
            }
        } else if car.acceleration < -BRAKE_LIGHT_THRESHOLD || car.is_waiting() {
            // Draw brake lights.
            let back = front - facing * (CAR_SIZE.x + BRAKE_LIGHT_GAP);
            let across = facing.perp() * (CAR_SIZE.y / 2.0 - 1.0);
            lines.line_2d(back - across, back + across, STOP);
        }
    }
}

/// Reports the traffic data: the plot's lines, the count under the crossing, and the waiting list,
/// synced by each car's ID number.
fn gather_report_data(
    traffic: Single<&Traffic>,
    mut plot: Single<&mut PlotData, With<WaitingPlot>>,
    mut throughput: Single<&mut Text, With<Throughput>>,
    list: Single<Entity, With<WaitingList>>,
    mut shown: Local<Vec<Waiting>>,
    mut commands: Commands,
) {
    for road in Road::BOTH {
        let points: Vec<Vec2> = traffic
            .history
            .iter()
            .map(|&(at, waiting)| Vec2::new(at, waiting[road.index()] as f32))
            .collect();
        if plot
            .get(road.name())
            .is_some_and(|series| series.points != points)
        {
            plot.set(road.name(), points);
        }
    }

    let counted = format!(
        "Crossed: {} east-west, {} north-south. Crashes: {}.",
        traffic.crossed[0], traffic.crossed[1], traffic.crashes
    );
    if throughput.0 != counted {
        throughput.0 = counted;
    }

    // Collect the list of cars waiting on the light, keyed by their ID numbers.
    let mut waiting: Vec<Waiting> = Road::BOTH
        .iter()
        .flat_map(|&road| waiting_on(&traffic.cars, road))
        .map(|car| Waiting {
            id: car.id,
            road: car.road,
            tenths: (car.waited * 10.0) as u32,
        })
        .collect();
    waiting.sort_by(|a, b| b.tenths.cmp(&a.tenths).then(a.id.cmp(&b.id)));
    // Only when something shown has changed: the sync itself would leave unchanged rows alone,
    // but there's no need to queue it at all.
    if *shown == waiting {
        return;
    }
    shown.clone_from(&waiting);
    commands
        .entity(*list)
        .sync_children(waiting, |car| car.id, waiting_row);
}

/// A new row of the waiting list: its shape only. Its binds write what it shows, once now and
/// again each time its car's data changes.
fn waiting_row(list: &mut ChildSpawnerCommands, _: &Waiting) {
    list.row(|row| {
        row.text("")
            .insert(Bind::new(|car: &Waiting, text: &mut Text| {
                text.0 = format!("Car {} - {}", car.id, car.road.name());
            }));
        row.spacer();
        row.meter(MeterConfig::new(0.0).range(0.0, LONGEST_WAIT).length(100.0))
            .insert(Bind::new(|car: &Waiting, meter: &mut MeterValue| {
                meter.0 = car.tenths as f32 / 10.0;
            }));
        row.small("")
            .insert(Bind::new(|car: &Waiting, text: &mut Text| {
                text.0 = format!("{:.1} s", car.tenths as f32 / 10.0);
            }));
    })
    .node(|node| node.align_items = AlignItems::Center);
}

// ================================
// OBSERVERS ON THE WIDGETS

/// A scrubber's announcement sets one of the simulation's settings.
fn set(
    apply: impl Fn(&mut Traffic, f32) + Send + Sync + 'static,
) -> impl Fn(On<ValueChange<f64>>, Single<&mut Traffic>) {
    move |change, mut traffic| apply(&mut traffic, change.value as f32)
}

/// The signal picker: the lights take turns, or both stay green.
fn choose_signals(change: On<ValueChange<bool>>, mut traffic: Single<&mut Traffic>) {
    traffic.both_green = change.value;
}
