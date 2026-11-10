//! Plots: Simple line and bar graphs, with axes, ticks, and a pointer.
//!
//! Lines, the grid and the pointer's marks are drawn on a [canvas](crate::canvas),
//! which is redrawn each frame. Bars and tick labels are UI nodes kept by the
//! [keyed](crate::keyed) system, so they're touched only when what they show
//! changes: updating one bar of a grouped bar graph moves that bar alone. The
//! plot's pointer [`PlotPointer`] reports the highlighted series and its point
//! in the plot's units.

use bevy::prelude::*;
use bevy::text::{Justify, TextLayout};
use bevy::ui::UiSystems;

use crate::EnsurePlugin;
use crate::canvas::{CanvasConfig, CanvasPlugin, CanvasPointer, CanvasSketch, spawn_canvas};
use crate::keyed::{Bind, SyncChildren};
use crate::sanitize::at_least;
use crate::theme::palette::{ACCENT, EDGE, HIGHLIGHT, SMALL_SIZE, TEXT_MUTED};
use crate::theme::{
    Shade, Theme, ThemeCornerRadius, ThemeFontSize, ThemePlugin, ThemeSkin, ThemeTextColor,
    ThemeToken,
};

/// Prefix of the plot readout's theme tokens.
pub const PLOT_STYLE: &str = "touchy.plot";

/// Room beside the plotted area for the y labels, unless the config says
/// otherwise, and below it for the x labels, in pixels.
const DEFAULT_Y_LABEL_ROOM: f32 = 40.0;
const X_LABEL_ROOM: f32 = 18.0;
/// Room above and right of it, so the end labels, centred on their ticks,
/// aren't cut off.
const TOP_ROOM: f32 = 7.0;
const RIGHT_ROOM: f32 = 14.0;
/// Space between a y label and the plotted area.
const LABEL_GAP: f32 = 4.0;
/// The least room between ticks, in pixels, along each axis.
const MIN_X_TICK_SPACING: f32 = 64.0;
const MIN_Y_TICK_SPACING: f32 = 28.0;
/// How thick the plotted lines are, in pixels.
const DEFAULT_LINE_WIDTH: f32 = 2.0;
/// How big the dot is where the pointer meets a series, in pixels.
const POINTER_DOT_RADIUS: f32 = 3.0;
/// How faint the grid is, beside the axes.
const GRID_ALPHA: f32 = 0.35;
/// How much of the room between neighbouring bars a bar takes.
const DEFAULT_BAR_FILL: f32 = 0.7;
const DEFAULT_BAR_CORNER_RADIUS: f32 = 2.0;
/// How many series colours a plot's tokens hold; further series take them
/// again from the first.
pub const SERIES_COLOURS: usize = 6;
/// The series colours after the first two, which are the palette's accent and
/// highlight: distinct from them and from each other, on light or dark.
const MORE_SERIES_COLOURS: [Color; SERIES_COLOURS - 2] = [
    Color::srgb(0.35, 0.78, 0.72),
    Color::srgb(0.86, 0.45, 0.62),
    Color::srgb(0.55, 0.78, 0.38),
    Color::srgb(0.68, 0.56, 0.92),
];

/// Theme tokens for a plot readout.
#[derive(Component, Clone, PartialEq, Debug)]
#[component(immutable)]
pub struct PlotTokens {
    /// Each series' colour, in turn, unless the series names its own.
    pub series: [ThemeToken; SERIES_COLOURS],
    /// The axes and the grid, and the pointer's marks.
    pub axis: ThemeToken,
    /// The grid lines, on the background of the plot.
    pub grid: ThemeToken,
    /// The pointer's dot and guide line, and the nearest series' point.
    pub pointer: ThemeToken,
    /// The tick labels, and the pointer's readout.
    pub label: ThemeToken,
    /// The font size of the labels, in pixels.
    pub font_size: ThemeToken,
    /// How much of the room between neighbouring bars a bar takes, from 0 to 1.
    pub bar_fill: ThemeToken,
    /// How round bar elements' corners are, in pixels.
    pub bar_corner_radius: ThemeToken,
}

impl PlotTokens {
    /// Names the tokens under `prefix`.
    pub fn with_prefix(prefix: &str) -> Self {
        let token = |suffix: &str| ThemeToken::new(format!("{prefix}.{suffix}"));
        Self {
            series: std::array::from_fn(|index| token(&format!("series.{}", index + 1))),
            axis: token("axis"),
            grid: token("grid"),
            pointer: token("pointer"),
            label: token("label"),
            font_size: token("font_size"),
            bar_fill: token("bar_fill"),
            bar_corner_radius: token("bar_corner_radius"),
        }
    }

    /// Register the default values for these tokens, unless the app sets them.
    pub fn register_theme_defaults(&self, theme: &mut Theme) {
        let [first, second, more @ ..] = &self.series;
        theme
            .default_derived(first.clone(), ACCENT, Shade::Same)
            .default_derived(second.clone(), HIGHLIGHT, Shade::Same);
        for (token, colour) in more.iter().zip(MORE_SERIES_COLOURS) {
            theme.default_color(token.clone(), colour);
        }
        theme
            .default_derived(self.axis.clone(), EDGE, Shade::Same)
            .default_derived(self.grid.clone(), EDGE, Shade::Alpha(GRID_ALPHA))
            .default_derived(self.pointer.clone(), TEXT_MUTED, Shade::Same)
            .default_derived(self.label.clone(), TEXT_MUTED, Shade::Same)
            .default_size_from(self.font_size.clone(), SMALL_SIZE)
            .default_size(self.bar_fill.clone(), DEFAULT_BAR_FILL)
            .default_size(self.bar_corner_radius.clone(), DEFAULT_BAR_CORNER_RADIUS);
    }
}

/// The built-in token set.
impl Default for PlotTokens {
    fn default() -> Self {
        Self::with_prefix(PLOT_STYLE)
    }
}

/// What type of plot to draw for a series.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SeriesPlotType {
    /// A line through its points, in order.
    #[default]
    Line,
    /// A bar rising from the x axis's zero to each point.
    Bars,
    /// A bar reaching from the y axis's zero to each point.
    HorizontalBars,
}

impl SeriesPlotType {
    /// The axis a series' values run along: up for lines and vertical bars,
    /// across for horizontal bars. The other axis is where its points, or its
    /// categories, are placed. As an index into a `Vec2`: 0 for x, 1 for y.
    fn value_axis(self) -> usize {
        match self {
            Self::HorizontalBars => 0,
            Self::Line | Self::Bars => 1,
        }
    }

    /// The axis its points, or categories, are placed along: the other one.
    fn place_axis(self) -> usize {
        1 - self.value_axis()
    }

    /// The point for the `index`th of a list of values: `value` along the
    /// value axis, and the index along the other, one unit per value. Down the
    /// y axis for horizontal bars, so a list reads top to bottom as it does
    /// left to right.
    fn category_point(self, index: usize, value: f32) -> Vec2 {
        let mut point = Vec2::ZERO;
        point[self.value_axis()] = value;
        point[self.place_axis()] = match self {
            Self::HorizontalBars => -(index as f32),
            Self::Line | Self::Bars => index as f32,
        };
        point
    }
}

/// One series on a plot, its points in the app's units, and how to draw it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlotSeries {
    /// The name of the series.
    pub label: String,
    /// The data to plot.
    pub points: Vec<Vec2>,
    /// The type of plot to draw for this series.
    pub plot_type: SeriesPlotType,
    /// The theme tokens for a plotted series.
    pub token: Option<ThemeToken>,
}

impl PlotSeries {
    /// Create a new plotted series with name `label` from data `points`.
    pub fn new(label: impl Into<String>, points: impl Into<Vec<Vec2>>) -> Self {
        Self {
            label: label.into(),
            points: points.into(),
            plot_type: SeriesPlotType::Line,
            token: None,
        }
    }
    /// Bars, one per value, for categories named with [`PlotConfig::x_categories`]
    /// (or `y_categories`, for horizontal bars).
    pub fn bars(label: impl Into<String>, style: SeriesPlotType, values: &[f32]) -> Self {
        Self::new(label, Vec::new()).plot_type(style).values(values)
    }
    /// Change the type of plot to draw for this series.
    pub fn plot_type(mut self, style: SeriesPlotType) -> Self {
        self.plot_type = style;
        self
    }
    /// Replaces its points with one per value, a unit apart along the axis
    /// its [`SeriesPlotType`] places them on: for bars named by category, or
    /// any series of evenly spaced values.
    pub fn values(mut self, values: &[f32]) -> Self {
        self.set_values(values);
        self
    }
    /// Colours it by `token` rather than by its place among the series.
    pub fn token(mut self, token: ThemeToken) -> Self {
        self.token = Some(token);
        self
    }
    /// Replaces its points with one per value, as [`PlotSeries::values`] does.
    fn set_values(&mut self, values: &[f32]) {
        let plot_type = self.plot_type;
        self.points = (values.iter().enumerate())
            .map(|(index, &value)| plot_type.category_point(index, value))
            .collect();
    }

    /// How wide its bars are, in the plot's units: a share of the least room
    /// between neighbouring points, or of one unit if there's only one.
    fn bar_band(&self, fill: f32) -> f32 {
        let axis = self.plot_type.place_axis();
        let mut places: Vec<f32> = self.points.iter().map(|point| point[axis]).collect();
        places.sort_by(f32::total_cmp);
        let room = places
            .windows(2)
            .map(|pair| pair[1] - pair[0])
            .filter(|room| *room > 0.0)
            .fold(f32::INFINITY, f32::min);
        fill.clamp(0.0, 1.0) * if room.is_finite() { room } else { 1.0 }
    }
}

/// Everything a plot shows. Change it to change what's plotted.
#[derive(Component, Clone, Debug, Default, PartialEq)]
pub struct PlotData(pub Vec<PlotSeries>);

impl PlotData {
    /// The series labelled `label`, if there is one.
    pub fn get(&self, label: &str) -> Option<&PlotSeries> {
        self.0.iter().find(|series| series.label == label)
    }

    /// Replaces the points of the series labelled `label`, or adds a line
    /// through them if there's none.
    pub fn set(&mut self, label: &str, points: impl Into<Vec<Vec2>>) {
        let points = points.into();
        match self.0.iter_mut().find(|series| series.label == label) {
            Some(series) => series.points = points,
            None => self.0.push(PlotSeries::new(label, points)),
        }
    }

    /// Replaces the bars of the series labelled `label`, one per value, as its
    /// style places them. Does nothing if there's no such series.
    pub fn set_values(&mut self, label: &str, values: &[f32]) {
        if let Some(series) = self.0.iter_mut().find(|series| series.label == label) {
            series.set_values(values);
        }
    }
}

/// What one axis of a plot shows. A range of numbers is a fixed axis, so
/// `.y_range(0.0..=1.0)` holds the y axis from 0 to 1.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum AxisRange {
    /// Whatever the points span, and zero beneath any bars, rounded out to
    /// whole ticks.
    #[default]
    Fit,
    /// From `min` to `max`, regardless of data.
    Fixed {
        /// The minimum value shown on this axis.
        min: f32,
        /// The maximum value shown on this axis.
        max: f32,
    },
}

impl From<std::ops::RangeInclusive<f32>> for AxisRange {
    fn from(range: std::ops::RangeInclusive<f32>) -> Self {
        Self::Fixed {
            min: *range.start(),
            max: *range.end(),
        }
    }
}

/// What a plot's axes show. Change it to rescale the plot.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct PlotAxes {
    /// Range of values to show on the x-axis.
    pub x: AxisRange,
    /// Range of values to show on the y-axis.
    pub y: AxisRange,
}

/// Ticks an axis names itself, rather than placing on round numbers: each a
/// place on the axis and its label. Change it to rename them.
#[derive(Component, Clone, Debug, Default, PartialEq)]
pub struct PlotTicks {
    /// The x axis's ticks, or `None` to place them on round numbers.
    pub x: Option<Vec<(f32, String)>>,
    /// The y axis's ticks, or `None` to place them on round numbers.
    pub y: Option<Vec<(f32, String)>>,
}

/// The line marking the pointer's place over a plot.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PointerGuide {
    /// Across the nearest series' bars, through the end of the bar the pointer's
    /// over, or upright at the pointer over lines.
    #[default]
    Auto,
    /// Upright, through the nearest point, or the pointer.
    Vertical,
    /// Level, through the nearest point, or the pointer.
    Horizontal,
    /// Both.
    Cross,
    /// None: only the dot.
    Hidden,
}

/// Where the mouse is over a plot, and the series nearest it there.
#[derive(Component, Clone, Debug, Default, PartialEq)]
pub struct PlotPointer {
    /// Where the mouse is, in the plot's units, or `None` while it's elsewhere.
    pub at: Option<Vec2>,
    /// Of the lines its x crosses and the bars it's over, the one passing
    /// closest to it.
    pub nearest: Option<PlotHit>,
}

/// A series the pointer is near, and the point on it: on a line, where the
/// pointer's x crosses it; on bars, the point the bar it's over reaches.
#[derive(Clone, Debug, PartialEq)]
pub struct PlotHit {
    /// Its place in [`PlotData`].
    pub series: usize,
    /// The label to use for a highlighted point.
    pub label: String,
    /// The coordinates of the point, in the plot's units.
    pub point: Vec2,
}

/// Construction-time configuration for a plot.
///
/// Consumed by [`SpawnUi::plot`](crate::spawn_ui::SpawnUi::plot) and not retained as a component.
///
/// # Examples
/// ```
/// use touchy::prelude::*;
///
/// let config = PlotConfig::new()
///     .x_range(0.0..=10.0)
///     .y_range(-1.0..=1.0);
/// ```
#[derive(Clone, Debug)]
pub struct PlotConfig {
    /// The series to plot, and their points in the app's units.
    pub data: PlotData,
    /// What the x and y axes show: [`AxisRange::Fit`], or a range such as `0.0..=10.0`.
    pub axes: PlotAxes,
    /// Specific ticks along the axes.
    pub ticks: PlotTicks,
    /// How to show the line marking the pointer's place over the plot.
    pub guide: PointerGuide,
    /// Size, in pixels, labels included. Without one, plot fills available space.
    pub size: Option<Vec2>,
    /// How thick the plotted lines are, in pixels.
    pub line_width: f32,
    /// Room beside the plotted area for the y axis's labels, in pixels.
    pub y_label_room: f32,
    /// Theme tokens for the plot.
    pub tokens: PlotTokens,
}

impl Default for PlotConfig {
    fn default() -> Self {
        Self {
            data: PlotData::default(),
            axes: PlotAxes::default(),
            ticks: PlotTicks::default(),
            guide: PointerGuide::default(),
            size: None,
            line_width: DEFAULT_LINE_WIDTH,
            y_label_room: DEFAULT_Y_LABEL_ROOM,
            tokens: PlotTokens::with_prefix(PLOT_STYLE),
        }
    }
}

impl PlotConfig {
    /// An empty plot, both axes fitting whatever it's given.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a line called `label` through `points`.
    pub fn series(self, label: impl Into<String>, points: impl Into<Vec<Vec2>>) -> Self {
        self.with_series(PlotSeries::new(label, points))
    }

    /// Adds vertical bars called `label`, one per value, left to right.
    pub fn bars(self, label: impl Into<String>, values: &[f32]) -> Self {
        self.with_series(PlotSeries::bars(label, SeriesPlotType::Bars, values))
    }

    /// Adds horizontal bars called `label`, one per value, top to bottom.
    pub fn horizontal_bars(self, label: impl Into<String>, values: &[f32]) -> Self {
        self.with_series(PlotSeries::bars(
            label,
            SeriesPlotType::HorizontalBars,
            values,
        ))
    }

    /// Adds a series made beforehand, such as one with a colour of its own.
    pub fn with_series(mut self, series: PlotSeries) -> Self {
        self.data.0.push(series);
        self
    }

    /// What the x axis shows: [`AxisRange::Fit`], or a range such as `0.0..=10.0`.
    pub fn x_range(mut self, range: impl Into<AxisRange>) -> Self {
        self.axes.x = range.into();
        self
    }

    /// What the y axis shows: [`AxisRange::Fit`], or a range such as `-1.0..=1.0`.
    pub fn y_range(mut self, range: impl Into<AxisRange>) -> Self {
        self.axes.y = range.into();
        self
    }

    /// Names the x axis's ticks: one per category, left to right, under
    /// vertical bars made from values.
    pub fn x_categories<S: Into<String>>(mut self, names: impl IntoIterator<Item = S>) -> Self {
        self.ticks.x = Some(categories(SeriesPlotType::Bars, names));
        self
    }

    /// Names the y axis's ticks: one per category, top to bottom, beside
    /// horizontal bars made from values.
    pub fn y_categories<S: Into<String>>(mut self, names: impl IntoIterator<Item = S>) -> Self {
        self.ticks.y = Some(categories(SeriesPlotType::HorizontalBars, names));
        self
    }

    /// The line marking the pointer's place.
    pub fn guide(mut self, guide: PointerGuide) -> Self {
        self.guide = guide;
        self
    }

    /// Fixes its size, in pixels, labels included.
    pub fn size(mut self, width: f32, height: f32) -> Self {
        self.size = Some(Vec2::new(width, height));
        self
    }

    /// How thick the plotted lines are, in pixels.
    pub fn line_width(mut self, pixels: f32) -> Self {
        self.line_width = pixels;
        self
    }

    /// Room beside the plotted area for the y axis's labels, in pixels: more
    /// for long category names.
    pub fn y_label_room(mut self, pixels: f32) -> Self {
        self.y_label_room = pixels;
        self
    }

    /// Theme tokens for the plot.
    pub fn tokens(mut self, tokens: PlotTokens) -> Self {
        self.tokens = tokens;
        self
    }
    /// Ensures all values are valid.
    fn sanitized(mut self) -> Self {
        self.line_width = at_least(self.line_width, 0.0, "PlotConfig line width");
        self.y_label_room = at_least(self.y_label_room, 0.0, "PlotConfig y-label room");
        self.size = at_least(self.size, Vec2::ZERO, "PlotConfig size");
        self
    }
}

/// Ticks named `names`, where bars of `plot_type` made from values stand.
fn categories<S: Into<String>>(
    plot_type: SeriesPlotType,
    names: impl IntoIterator<Item = S>,
) -> Vec<(f32, String)> {
    let axis = plot_type.place_axis();
    names
        .into_iter()
        .enumerate()
        .map(|(index, name)| (plot_type.category_point(index, 0.0)[axis], name.into()))
        .collect()
}

/// A plot's parts, and what its keyed lists were last synced to.
#[derive(Component, Clone, Debug)]
struct PlotParts {
    canvas: Entity,
    bars: Entity,
    x_labels: Entity,
    y_labels: Entity,
    y_label_room: f32,
    shown_labels: [Vec<Tick>; 2],
    shown_bars: Vec<BarItem>,
}

/// A tick's label, and where along the plot's edge it sits, in pixels from
/// the plot's own top left corner.
#[derive(Clone, Debug, PartialEq)]
struct Tick {
    text: String,
    at: f32,
}

/// A bar: which series and point it shows, its colour, and where it is, as
/// (left, top, width, height) in pixels from the plotted area's top left.
#[derive(Clone, Debug, PartialEq)]
struct BarItem {
    key: (usize, usize),
    token: ThemeToken,
    rect: Vec4,
}

/// Spawns it; documented on [`SpawnUi::plot`](crate::spawn_ui::SpawnUi::plot).
pub(crate) fn spawn_plot<'a>(
    parent: &'a mut ChildSpawnerCommands,
    config: PlotConfig,
) -> EntityCommands<'a> {
    let PlotConfig {
        data,
        axes,
        ticks,
        guide,
        size,
        line_width,
        y_label_room,
        tokens,
    } = config.sanitized();
    let (width, height, grow) = match size {
        Some(size) => (px(size.x), px(size.y), 0.0),
        None => (Val::Auto, Val::Auto, 1.0),
    };
    let room = UiRect {
        left: px(y_label_room),
        right: px(RIGHT_ROOM),
        top: px(TOP_ROOM),
        bottom: px(X_LABEL_ROOM),
    };
    let mut plot = parent.spawn((
        Node {
            width,
            height,
            flex_grow: grow,
            min_width: px(0.0),
            min_height: px(0.0),
            padding: room,
            ..default()
        },
        data,
        axes,
        ticks,
        guide,
        PlotPointer::default(),
    ));
    let (mut canvas, mut bars, mut x_labels, mut y_labels) = (
        Entity::PLACEHOLDER,
        Entity::PLACEHOLDER,
        Entity::PLACEHOLDER,
        Entity::PLACEHOLDER,
    );
    plot.with_children(|plot| {
        let layer = |inset: UiRect| {
            (
                Node {
                    position_type: PositionType::Absolute,
                    left: inset.left,
                    right: inset.right,
                    top: inset.top,
                    bottom: inset.bottom,
                    ..default()
                },
                Pickable::IGNORE,
            )
        };
        // Bars under the canvas, which is clear, so lines, the grid and the
        // pointer's marks show over them.
        bars = plot.spawn(layer(room)).id();
        canvas = spawn_canvas(plot, CanvasConfig::painted().line_width(line_width)).id();
        x_labels = plot.spawn(layer(UiRect::DEFAULT)).id();
        y_labels = plot.spawn(layer(UiRect::DEFAULT)).id();
    });
    plot.insert((
        PlotParts {
            canvas,
            bars,
            x_labels,
            y_labels,
            y_label_room,
            shown_labels: [Vec::new(), Vec::new()],
            shown_bars: Vec::new(),
        },
        tokens,
    ));
    plot
}

/// The roundest step of at least `raw`: one, two or five times a power of ten.
fn nice_step(raw: f32) -> f32 {
    if !raw.is_finite() || raw <= 0.0 {
        return 1.0;
    }
    let power = 10f32.powf(raw.log10().floor());
    let mantissa = raw / power;
    let nice = if mantissa <= 1.0 {
        1.0
    } else if mantissa <= 2.0 {
        2.0
    } else if mantissa <= 5.0 {
        5.0
    } else {
        10.0
    };
    nice * power
}

/// An axis's span, and the step between its ticks: fixed, or fitted to
/// `data`, with at most `count` steps. A fitted axis is rounded out to whole
/// steps, unless its ticks are `named`, which fall where they're put.
fn axis_span(
    range: AxisRange,
    data: Option<(f32, f32)>,
    count: f32,
    named: bool,
) -> (f32, f32, f32) {
    let count = count.max(1.0);
    let (min, max) = match (range, data) {
        (AxisRange::Fixed { min, max }, _) => (min, max),
        (AxisRange::Fit, Some((low, high))) if high > low => (low, high),
        // A single value, or none: a unit either side of it.
        (AxisRange::Fit, Some((value, _))) => (value - 1.0, value + 1.0),
        (AxisRange::Fit, None) => (0.0, 1.0),
    };
    let step = nice_step((max - min).abs() / count);
    match range {
        AxisRange::Fit if !named => (
            (min / step).floor() * step,
            (max / step).ceil() * step,
            step,
        ),
        _ => (min, max, step),
    }
}

/// The ticks from `min` to `max`, a `step` apart, on whole steps.
fn ticks(min: f32, max: f32, step: f32) -> Vec<f32> {
    let (low, high) = (min.min(max), min.max(max));
    let first = (low / step).ceil() as i64;
    let last = (high / step + 1e-4).floor() as i64;
    (first..=last).map(|n| n as f32 * step).collect()
}

/// A tick's label: as many decimals as its step needs, and no "-0".
fn label(value: f32, step: f32) -> String {
    let decimals = (-step.log10().floor()).max(0.0) as usize;
    let text = format!("{value:.decimals$}");
    if text
        .trim_start_matches('-')
        .chars()
        .all(|c| c == '0' || c == '.')
    {
        text.trim_start_matches('-').to_string()
    } else {
        text
    }
}

/// What a plot's points span along each axis, bars' widths and zeros included.
fn spans(data: &PlotData, fill: f32) -> [Option<(f32, f32)>; 2] {
    let mut spans = [None; 2];
    let mut take = |axis: usize, value: f32| {
        if value.is_finite() {
            spans[axis] = Some(match spans[axis] {
                None => (value, value),
                Some((low, high)) => (f32::min(low, value), f32::max(high, value)),
            });
        }
    };
    for series in &data.0 {
        let half = series.bar_band(fill) / 2.0;
        for point in &series.points {
            match series.plot_type {
                SeriesPlotType::Line => {
                    take(0, point.x);
                    take(1, point.y);
                }
                SeriesPlotType::Bars => {
                    take(0, point.x - half);
                    take(0, point.x + half);
                    take(1, point.y);
                    take(1, 0.0);
                }
                SeriesPlotType::HorizontalBars => {
                    take(1, point.y - half);
                    take(1, point.y + half);
                    take(0, point.x);
                    take(0, 0.0);
                }
            }
        }
    }
    spans
}

/// Where the line through `points` crosses `x`, if it does: the first segment
/// spanning it, interpolated.
fn line_at(points: &[Vec2], x: f32) -> Option<f32> {
    points.windows(2).find_map(|pair| {
        let (a, b) = (pair[0], pair[1]);
        let (low, high) = (a.x.min(b.x), a.x.max(b.x));
        if x < low || x > high {
            return None;
        }
        let across = b.x - a.x;
        Some(if across == 0.0 {
            a.y
        } else {
            a.y + (b.y - a.y) * (x - a.x) / across
        })
    })
}

/// The point on `series` nearest `at`, if any: where a line crosses `at`'s x,
/// or where the bar `at` is over reaches.
fn point_near(series: &PlotSeries, at: Vec2, fill: f32) -> Option<Vec2> {
    let half = series.bar_band(fill) / 2.0;
    match series.plot_type {
        SeriesPlotType::Line => Some(Vec2::new(at.x, line_at(&series.points, at.x)?)),
        SeriesPlotType::Bars => series
            .points
            .iter()
            .copied()
            .find(|point| (point.x - at.x).abs() <= half),
        SeriesPlotType::HorizontalBars => series
            .points
            .iter()
            .copied()
            .find(|point| (point.y - at.y).abs() <= half),
    }
}

/// Of the series with a point near `at`, the one passing closest to it on
/// screen, with that point.
fn nearest_series(
    data: &PlotData,
    at: Vec2,
    fill: f32,
    to_canvas: impl Fn(Vec2) -> Vec2,
) -> Option<PlotHit> {
    let pointer = to_canvas(at);
    data.0
        .iter()
        .enumerate()
        .filter_map(|(index, series)| {
            let point = point_near(series, at, fill)?;
            let off = to_canvas(point) - pointer;
            // Along each series' values: up a line or a vertical bar, across a
            // horizontal one.
            let distance = off[series.plot_type.value_axis()].abs();
            Some((index, series, point, distance))
        })
        .min_by(|a, b| a.3.total_cmp(&b.3))
        .map(|(index, series, point, _)| PlotHit {
            series: index,
            label: series.label.clone(),
            point,
        })
}

type PlotDrawing = (
    &'static PlotData,
    &'static PlotAxes,
    &'static PlotTicks,
    &'static PointerGuide,
    &'static PlotTokens,
    &'static mut PlotPointer,
    &'static mut PlotParts,
);

/// Draws each plot: lines, the grid and the pointer's marks on its canvas,
/// bars and labels as keyed nodes. Reports the pointer in the plot's units.
fn draw_plots(
    mut plots: Query<PlotDrawing>,
    pointers: Query<&CanvasPointer>,
    mut sketch: CanvasSketch,
    theme: Res<Theme>,
    mut commands: Commands,
) {
    for (data, axes, named, guide, tokens, mut pointer, mut parts) in &mut plots {
        let area = sketch.picture_size(parts.canvas);
        if area.min_element() <= 0.0 {
            continue;
        }
        let fill = theme.size(&tokens.bar_fill);
        let [x_data, y_data] = spans(data, fill);
        let (x_min, x_max, x_step) = axis_span(
            axes.x,
            x_data,
            area.x / MIN_X_TICK_SPACING,
            named.x.is_some(),
        );
        let (y_min, y_max, y_step) = axis_span(
            axes.y,
            y_data,
            area.y / MIN_Y_TICK_SPACING,
            named.y.is_some(),
        );
        let (low, high) = (Vec2::new(x_min, y_min), Vec2::new(x_max, y_max));
        let extent = (high - low).max(Vec2::splat(f32::EPSILON));
        let to_canvas = |point: Vec2| (point - low) / extent * area;
        let to_data = |at: Vec2| low + at / area * extent;
        let token_of = |index: usize, series: &PlotSeries| {
            series
                .token
                .clone()
                .unwrap_or_else(|| tokens.series[index % SERIES_COLOURS].clone())
        };
        // Each axis's ticks: where they fall, in the plot's units, and their labels.
        let axis_ticks =
            |named: &Option<Vec<(f32, String)>>, min: f32, max: f32, step: f32| match named {
                Some(named) => named
                    .iter()
                    .filter(|(at, _)| (min.min(max)..=min.max(max)).contains(at))
                    .cloned()
                    .collect::<Vec<_>>(),
                None => ticks(min, max, step)
                    .into_iter()
                    .map(|at| (at, label(at, step)))
                    .collect(),
            };
        let x_ticks = axis_ticks(&named.x, x_min, x_max, x_step);
        let y_ticks = axis_ticks(&named.y, y_min, y_max, y_step);

        let hovered = pointers
            .get(parts.canvas)
            .ok()
            .and_then(|pointer| pointer.0)
            .map(to_data);
        let nearest = hovered.and_then(|at| nearest_series(data, at, fill, to_canvas));
        pointer.set_if_neq(PlotPointer {
            at: hovered,
            nearest: nearest.clone(),
        });

        if let Some(mut lines) = sketch.on(parts.canvas) {
            let (grid, axis) = (theme.color(&tokens.grid), theme.color(&tokens.axis));
            for &(x, _) in &x_ticks {
                let at = to_canvas(Vec2::new(x, y_min)).x;
                lines.line_2d(Vec2::new(at, 0.0), Vec2::new(at, area.y), grid);
            }
            for &(y, _) in &y_ticks {
                let at = to_canvas(Vec2::new(x_min, y)).y;
                lines.line_2d(Vec2::new(0.0, at), Vec2::new(area.x, at), grid);
            }
            lines.line_2d(Vec2::ZERO, Vec2::new(area.x, 0.0), axis);
            lines.line_2d(Vec2::ZERO, Vec2::new(0.0, area.y), axis);
            for (index, series) in data.0.iter().enumerate() {
                if series.plot_type == SeriesPlotType::Line {
                    lines.linestrip_2d(
                        series.points.iter().map(|&point| to_canvas(point)),
                        theme.color(&token_of(index, series)),
                    );
                }
            }
            if let Some(at) = hovered {
                let marks = theme.color(&tokens.pointer);
                let through = to_canvas(nearest.as_ref().map_or(at, |hit| hit.point));
                let across_bars = nearest.as_ref().map(|hit| data.0[hit.series].plot_type);
                let (upright, level) = match (guide, across_bars) {
                    (PointerGuide::Auto, Some(SeriesPlotType::Bars)) => (false, true),
                    (PointerGuide::Auto, Some(SeriesPlotType::HorizontalBars)) => (true, false),
                    (PointerGuide::Auto, _) | (PointerGuide::Vertical, _) => (true, false),
                    (PointerGuide::Horizontal, _) => (false, true),
                    (PointerGuide::Cross, _) => (true, true),
                    (PointerGuide::Hidden, _) => (false, false),
                };
                if upright {
                    let x = through.x;
                    lines.line_2d(Vec2::new(x, 0.0), Vec2::new(x, area.y), marks);
                }
                if level {
                    let y = through.y;
                    lines.line_2d(Vec2::new(0.0, y), Vec2::new(area.x, y), marks);
                }
                if let Some(hit) = &nearest {
                    let colour = theme.color(&token_of(hit.series, &data.0[hit.series]));
                    lines.circle_2d(to_canvas(hit.point), POINTER_DOT_RADIUS, colour);
                }
            }
        }

        // Bars, in pixels from the plotted area's top left, y down.
        let down = |point: Vec2| {
            let at = to_canvas(point);
            Vec2::new(at.x, area.y - at.y)
        };
        let mut bars = Vec::new();
        for (index, series) in data.0.iter().enumerate() {
            let half = series.bar_band(fill) / 2.0;
            let corners = |point: Vec2| match series.plot_type {
                SeriesPlotType::Bars => {
                    let base = 0.0f32.clamp(y_min.min(y_max), y_min.max(y_max));
                    (
                        Vec2::new(point.x - half, base),
                        Vec2::new(point.x + half, point.y),
                    )
                }
                _ => {
                    let base = 0.0f32.clamp(x_min.min(x_max), x_min.max(x_max));
                    (
                        Vec2::new(base, point.y - half),
                        Vec2::new(point.x, point.y + half),
                    )
                }
            };
            if series.plot_type == SeriesPlotType::Line {
                continue;
            }
            for (place, &point) in series.points.iter().enumerate() {
                let (a, b) = corners(point);
                let (a, b) = (down(a), down(b));
                let (top_left, size) = (a.min(b), (a - b).abs());
                bars.push(BarItem {
                    key: (index, place),
                    token: token_of(index, series),
                    rect: Vec4::new(top_left.x, top_left.y, size.x, size.y),
                });
            }
        }
        if parts.shown_bars != bars {
            let tokens = tokens.clone();
            commands.entity(parts.bars).sync_children(
                bars.clone(),
                |bar| bar.key,
                move |list, _| spawn_bar(list, &tokens),
            );
            parts.shown_bars = bars;
        }

        // Labels sit in the plot's own pixels, from its top left corner.
        let room = parts.y_label_room;
        let x_labels: Vec<Tick> = x_ticks
            .into_iter()
            .map(|(x, text)| Tick {
                text,
                at: room + to_canvas(Vec2::new(x, y_min)).x,
            })
            .collect();
        let y_labels: Vec<Tick> = y_ticks
            .into_iter()
            .map(|(y, text)| Tick {
                text,
                at: TOP_ROOM + area.y - to_canvas(Vec2::new(x_min, y)).y,
            })
            .collect();
        for (axis, labels) in [x_labels, y_labels].into_iter().enumerate() {
            if parts.shown_labels[axis] == labels {
                continue;
            }
            let (tokens, along_x) = (tokens.clone(), axis == 0);
            let layer = if along_x {
                parts.x_labels
            } else {
                parts.y_labels
            };
            commands.entity(layer).sync_children(
                labels.clone(),
                |tick| tick.text.clone(),
                move |list, _| spawn_label(list, &tokens, along_x, room),
            );
            parts.shown_labels[axis] = labels;
        }
    }
}

/// A bar: a themed node, placed and coloured by its item.
fn spawn_bar(list: &mut ChildSpawnerCommands, tokens: &PlotTokens) {
    list.spawn((
        Node {
            position_type: PositionType::Absolute,
            ..default()
        },
        ThemeCornerRadius(tokens.bar_corner_radius.clone()),
        Pickable::IGNORE,
        Bind::new(|bar: &BarItem, node: &mut Node| {
            let place = (
                px(bar.rect.x),
                px(bar.rect.y),
                px(bar.rect.z),
                px(bar.rect.w),
            );
            if (node.left, node.top, node.width, node.height) != place {
                (node.left, node.top, node.width, node.height) = place;
            }
        })
        .and(Bind::insert(|bar: &BarItem| ThemeSkin(bar.token.clone()))),
    ));
}

/// A tick label: centred under its x-axis tick, or right-aligned
/// beside its y-axis tick, in `room` beside the plotted area. Its text and
/// place are bound to its tick.
fn spawn_label(list: &mut ChildSpawnerCommands, tokens: &PlotTokens, along_x: bool, room: f32) {
    let node = if along_x {
        Node {
            position_type: PositionType::Absolute,
            bottom: px(0.0),
            ..default()
        }
    } else {
        Node {
            position_type: PositionType::Absolute,
            left: px(0.0),
            width: px((room - LABEL_GAP).max(0.0)),
            ..default()
        }
    };
    // Centred on the tick across the axis's direction, by half its own size.
    let centring = if along_x {
        Val2::percent(-50.0, 0.0)
    } else {
        Val2::percent(0.0, -50.0)
    };
    list.spawn((
        node,
        Text::default(),
        TextLayout::justify(if along_x {
            Justify::Center
        } else {
            Justify::Right
        }),
        ThemeTextColor(tokens.label.clone()),
        ThemeFontSize(tokens.font_size.clone()),
        UiTransform {
            translation: centring,
            ..default()
        },
        Pickable::IGNORE,
        Bind::new(move |tick: &Tick, node: &mut Node| {
            if along_x {
                node.left = px(tick.at);
            } else {
                node.top = px(tick.at);
            }
        })
        .and(Bind::insert(|tick: &Tick| Text::new(tick.text.clone()))),
    ));
}

/// Restyles a plot's bars and labels when it's given new tokens. Its canvas
/// reads the tokens as it draws each frame, but the bars and labels took theirs
/// when they were spawned, so they're cleared to be spawned again, and the
/// next draw brings them back in the new style.
fn style_plot(
    insert: On<Insert<PlotTokens>>,
    mut plots: Query<&mut PlotParts>,
    mut commands: Commands,
) {
    let Ok(mut parts) = plots.get_mut(insert.entity) else {
        return;
    };
    for layer in [parts.bars, parts.x_labels, parts.y_labels] {
        commands.entity(layer).despawn_related::<Children>();
    }
    parts.shown_bars.clear();
    parts.shown_labels = [Vec::new(), Vec::new()];
}

/// Plots, and default token values.
pub struct PlotPlugin;

impl Plugin for PlotPlugin {
    fn build(&self, app: &mut App) {
        app.ensure_plugin(ThemePlugin)
            .ensure_plugin(CanvasPlugin)
            .add_observer(style_plot)
            .add_systems(PostUpdate, draw_plots.before(UiSystems::Prepare));
        PlotTokens::with_prefix(PLOT_STYLE)
            .register_theme_defaults(&mut app.world_mut().resource_mut::<Theme>());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_are_round() {
        assert_eq!(nice_step(0.7), 1.0);
        assert_eq!(nice_step(1.3), 2.0);
        assert_eq!(nice_step(3.0), 5.0);
        assert_eq!(nice_step(7.0), 10.0);
        assert_eq!(nice_step(140.0), 200.0);
        assert!((nice_step(0.03) - 0.05).abs() < 1e-6);
    }

    #[test]
    fn a_fitted_axis_rounds_out_to_whole_ticks() {
        let (min, max, step) = axis_span(AxisRange::Fit, Some((0.3, 8.6)), 5.0, false);
        assert_eq!((min, max, step), (0.0, 10.0, 2.0));
        assert_eq!(ticks(min, max, step), [0.0, 2.0, 4.0, 6.0, 8.0, 10.0]);

        let fixed = AxisRange::Fixed { min: 0.0, max: 1.0 };
        let (min, max, step) = axis_span(fixed, Some((-5.0, 5.0)), 4.0, false);
        assert_eq!((min, max), (0.0, 1.0), "a fixed axis ignores the points");
        assert!((step - 0.5).abs() < 1e-6);

        let (min, max, _) = axis_span(AxisRange::Fit, Some((3.0, 3.0)), 2.0, false);
        assert_eq!(
            (min, max),
            (2.0, 4.0),
            "a single value gets a unit either side"
        );

        let (min, max, _) = axis_span(AxisRange::Fit, Some((-0.35, 2.35)), 5.0, true);
        assert_eq!((min, max), (-0.35, 2.35), "named ticks fit exactly");
    }

    #[test]
    fn labels_have_the_decimals_their_step_needs() {
        assert_eq!(label(2.0, 1.0), "2");
        assert_eq!(label(0.25, 0.05), "0.25");
        assert_eq!(label(-0.0000001, 0.5), "0.0");
        assert_eq!(label(-1.5, 0.5), "-1.5");
    }

    #[test]
    fn bars_take_their_share_of_the_room_and_reach_down_to_zero() {
        let bars = PlotSeries::bars("scores", SeriesPlotType::Bars, &[0.5, 0.9, 0.2]);
        assert_eq!(
            bars.points,
            [
                Vec2::new(0.0, 0.5),
                Vec2::new(1.0, 0.9),
                Vec2::new(2.0, 0.2)
            ]
        );
        assert!((bars.bar_band(0.7) - 0.7).abs() < 1e-6);
        let [x, y] = spans(&PlotData(vec![bars]), 0.7);
        assert_eq!(x, Some((-0.35, 2.35)), "half a bar beyond either end");
        assert_eq!(y, Some((0.0, 0.9)), "down to zero");

        let across = PlotSeries::bars("across", SeriesPlotType::HorizontalBars, &[3.0, 1.0]);
        assert_eq!(
            across.points,
            [Vec2::new(3.0, 0.0), Vec2::new(1.0, -1.0)],
            "listed top to bottom"
        );
        assert_eq!(
            categories(SeriesPlotType::HorizontalBars, ["first", "second"]),
            [(0.0, "first".into()), (-1.0, "second".into())],
            "named where the bars stand"
        );
    }

    #[test]
    fn the_nearest_series_is_the_closest_line_or_the_bar_underneath() {
        let data = PlotData(vec![
            PlotSeries::new("low", [Vec2::ZERO, Vec2::new(1.0, 0.2)]),
            PlotSeries::new("high", [Vec2::new(0.0, 1.0), Vec2::new(1.0, 0.8)]),
            PlotSeries::new("short", [Vec2::new(2.0, 0.5), Vec2::new(3.0, 0.5)]),
        ]);
        // A plot ten times as tall as its units, so distances are on screen.
        let to_canvas = |point: Vec2| point * Vec2::new(100.0, 1000.0);
        let hit = nearest_series(&data, Vec2::new(0.5, 0.7), 0.7, to_canvas).unwrap();
        assert_eq!((hit.series, hit.label.as_str()), (1, "high"));
        assert_eq!(hit.point, Vec2::new(0.5, 0.9));
        let hit = nearest_series(&data, Vec2::new(0.5, 0.3), 0.7, to_canvas).unwrap();
        assert_eq!(hit.label, "low", "the short series isn't under the pointer");
        assert!(nearest_series(&data, Vec2::new(5.0, 0.5), 0.7, to_canvas).is_none());

        let bars = PlotData(vec![PlotSeries::bars(
            "scores",
            SeriesPlotType::Bars,
            &[0.5, 0.9],
        )]);
        let hit = nearest_series(&bars, Vec2::new(1.2, 0.1), 0.7, to_canvas).unwrap();
        assert_eq!(
            hit.point,
            Vec2::new(1.0, 0.9),
            "the end of the bar it's over"
        );
        assert!(
            nearest_series(&bars, Vec2::new(0.5, 0.1), 0.7, to_canvas).is_none(),
            "between bars there's none"
        );
    }

    #[test]
    fn a_range_of_numbers_is_a_fixed_axis_and_data_is_set_by_label() {
        let config = PlotConfig::new().y_range(-3.0..=3.0);
        assert_eq!(
            config.axes.y,
            AxisRange::Fixed {
                min: -3.0,
                max: 3.0
            }
        );
        assert_eq!(config.axes.x, AxisRange::Fit);

        let mut data = PlotData::default();
        data.set("wave", [Vec2::ZERO]);
        data.set("wave", [Vec2::ONE]);
        data.set("other", [Vec2::X]);
        assert_eq!(
            data.0.len(),
            2,
            "set replaces a series by label, or adds one"
        );
        assert_eq!(data.get("wave").unwrap().points, [Vec2::ONE]);

        let mut bars = PlotData(vec![PlotSeries::bars(
            "scores",
            SeriesPlotType::Bars,
            &[1.0],
        )]);
        bars.set_values("scores", &[0.2, 0.4]);
        assert_eq!(bars.0[0].points, [Vec2::new(0.0, 0.2), Vec2::new(1.0, 0.4)]);
    }

    #[test]
    fn the_pointer_meets_the_line_between_its_points() {
        let points = [
            Vec2::new(0.0, 0.0),
            Vec2::new(2.0, 4.0),
            Vec2::new(4.0, 0.0),
        ];
        assert_eq!(line_at(&points, 1.0), Some(2.0));
        assert_eq!(line_at(&points, 3.0), Some(2.0));
        assert_eq!(line_at(&points, 5.0), None, "past the last point");
    }

    #[test]
    fn new_tokens_respawn_the_bars_and_labels_in_their_style() {
        let mut app = App::new();
        // What a canvas needs from Bevy, which `DefaultPlugins` brings in a game.
        app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()))
            .init_asset::<Image>()
            .init_resource::<bevy::picking::hover::HoverMap>()
            .init_resource::<bevy::picking::pointer::PointerMap>()
            .add_plugins(PlotPlugin);
        let mut plot = None;
        app.world_mut()
            .commands()
            .spawn(Node::default())
            .with_children(|parent| {
                plot = Some(spawn_plot(parent, PlotConfig::new().bars("b", &[1.0])).id());
            });
        app.update();
        let plot = plot.unwrap();
        // As a draw leaves them: a bar shown, and noted as shown.
        let layer = app.world().get::<PlotParts>(plot).unwrap().bars;
        let bar = app.world_mut().spawn(ChildOf(layer)).id();
        app.world_mut()
            .get_mut::<PlotParts>(plot)
            .unwrap()
            .shown_bars
            .push(BarItem {
                key: (0, 0),
                token: ThemeToken::new_static("test.bar"),
                rect: Vec4::ONE,
            });

        app.world_mut()
            .entity_mut(plot)
            .insert(PlotTokens::with_prefix("test.plot"));
        app.world_mut().flush();
        assert!(app.world().get_entity(bar).is_err(), "the old bar is gone");
        assert!(
            app.world()
                .get::<PlotParts>(plot)
                .unwrap()
                .shown_bars
                .is_empty(),
            "so the next draw spawns it again"
        );
    }

    #[test]
    fn values_run_along_their_axis_one_unit_apart() {
        assert_eq!(
            SeriesPlotType::Bars.category_point(2, 0.5),
            Vec2::new(2.0, 0.5)
        );
        assert_eq!(
            SeriesPlotType::HorizontalBars.category_point(2, 0.5),
            Vec2::new(0.5, -2.0)
        );
        assert_eq!(SeriesPlotType::HorizontalBars.value_axis(), 0);
        assert_eq!(SeriesPlotType::Line.place_axis(), 0);
    }
}
