//! Meters: how full something is, between a minimum and a maximum.
//!
//! Set [`MeterValue`] to move it. A value outside the range draws the meter
//! empty or full; show the number beside it when that matters.
//!
//! In a [keyed list](crate::keyed), a [`Bind`](crate::keyed::Bind) can write
//! each row's [`MeterValue`] from its item, so a row's meter moves only when its
//! data changes.

use bevy::prelude::*;
use bevy::ui::UiSystems;

use crate::EnsurePlugin;
use crate::sanitize::at_least;
use crate::theme::palette::{ACCENT, CONTROL, CONTROL_CORNER_RADIUS};
use crate::theme::skins;
use crate::theme::{Shade, Theme, ThemeCornerRadius, ThemePlugin, ThemeSkin, ThemeToken};

/// Prefix of the meter's theme tokens.
pub const METER_STYLE: &str = "touchy.meter";

/// How thick a bar is, across its length, in pixels.
const DEFAULT_THICKNESS: f32 = 8.0;

/// How a meter is drawn. Chosen at spawn, as each style is built differently.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum MeterStyle {
    /// A track, filled from the left.
    #[default]
    Bar,
}

/// Theme tokens for a meter.
#[derive(Component, Clone, PartialEq, Debug)]
#[component(immutable)]
pub struct MeterTokens {
    /// The track's background.
    pub track: ThemeToken,
    /// The filled part of the track.
    pub fill: ThemeToken,
    /// The track's corner radius.
    pub corner_radius: ThemeToken,
}

impl MeterTokens {
    /// Names the tokens under `prefix`.
    pub fn with_prefix(prefix: &str) -> Self {
        let token = |suffix: &str| ThemeToken::new(format!("{prefix}.{suffix}"));
        Self {
            track: token("track"),
            fill: token("fill"),
            corner_radius: token("corner_radius"),
        }
    }

    /// Register the default values for the meter tokens, unless the app sets them.
    pub fn register_theme_defaults(&self, theme: &mut Theme) {
        theme
            .default_derived(self.track.clone(), CONTROL, Shade::Same)
            .default_derived(self.fill.clone(), ACCENT, Shade::Same)
            .default_size_scaled(self.corner_radius.clone(), CONTROL_CORNER_RADIUS, 0.75);
        // Each part draws its role's skin, once a theme gives the role one.
        theme.default_skin_from(self.track.clone(), skins::SUNKEN);
    }
}

/// The built-in token set.
impl Default for MeterTokens {
    fn default() -> Self {
        Self::with_prefix(METER_STYLE)
    }
}

/// The meter's range. Change it and the meter redraws.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct Meter {
    /// The minimum value of the meter's range.
    pub min: f32,
    /// The maximum value of the meter's range.
    pub max: f32,
}

impl Meter {
    /// How full `value` makes the meter, from 0 to 1. An empty or invalid
    /// range, or a value that isn't a number, reads as empty.
    pub fn fraction(&self, value: f32) -> f32 {
        let span = self.max - self.min;
        if !span.is_finite() || span <= 0.0 || !value.is_finite() {
            return 0.0;
        }
        ((value - self.min) / span).clamp(0.0, 1.0)
    }
}

/// The value the meter shows. Set it to move the meter.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct MeterValue(pub f32);

/// The filled part of a bar.
#[derive(Component, Clone, Copy, Debug)]
struct MeterParts {
    /// The filled part of a bar.
    fill: Entity,
}

/// Construction-time configuration for a meter.
///
/// Consumed by [`SpawnUi::meter`](crate::spawn_ui::SpawnUi::meter) and not retained as a component.
///
/// # Examples
/// ```
/// use touchy::prelude::*;
/// let health = MeterConfig::new(75.0).range(0.0, 100.0).length(120.0);
/// # assert_eq!(health.value, 75.0);
/// # assert_eq!(health.meter.max, 100.0);
/// ```
#[derive(Clone, Debug)]
pub struct MeterConfig {
    /// The value the meter shows at spawn time.
    pub value: f32,
    /// The meter's range.
    pub meter: Meter,
    /// How the meter is drawn.
    pub style: MeterStyle,
    /// The bar's length in pixels; `None` fills the space its parent gives it.
    pub length: Option<f32>,
    /// The meter's thickness in pixels.
    pub thickness: f32,
    /// The meter's theme tokens.
    pub tokens: MeterTokens,
}

impl MeterConfig {
    /// Create a new configuration for a meter. Range is from 0 to 1.
    pub fn new(value: f32) -> Self {
        Self {
            value,
            meter: Meter { min: 0.0, max: 1.0 },
            style: MeterStyle::default(),
            length: None,
            thickness: DEFAULT_THICKNESS,
            tokens: MeterTokens::with_prefix(METER_STYLE),
        }
    }
    /// Set the meter's range. The meter draws empty or full if the value exceeds this.
    pub fn range(mut self, min: f32, max: f32) -> Self {
        self.meter.min = min;
        self.meter.max = max;
        self
    }
    /// Set the meter's style. Default is [`MeterStyle::Bar`].
    pub fn style(mut self, style: MeterStyle) -> Self {
        self.style = style;
        self
    }
    /// Give the meter a fixed length in pixels, instead of filling the parent.
    pub fn length(mut self, pixels: f32) -> Self {
        self.length = Some(pixels);
        self
    }
    /// Set the meter's thickness in pixels.
    pub fn thickness(mut self, pixels: f32) -> Self {
        self.thickness = pixels;
        self
    }
    /// Set the meter's theme tokens.
    pub fn tokens(mut self, tokens: MeterTokens) -> Self {
        self.tokens = tokens;
        self
    }
    /// Ensure the meter's configuration is valid.
    fn sanitized(mut self) -> Self {
        self.meter.max = at_least(self.meter.max, self.meter.min, "MeterConfig.meter.max");
        self.length = at_least(self.length, 0.0, "MeterConfig.length");
        self.thickness = at_least(self.thickness, 0.0, "MeterConfig.thickness");
        self
    }
}

/// Spawns it; documented on [`SpawnUi::meter`](crate::spawn_ui::SpawnUi::meter).
pub(crate) fn spawn_meter<'a>(
    parent: &'a mut ChildSpawnerCommands,
    config: MeterConfig,
) -> EntityCommands<'a> {
    let MeterConfig {
        value,
        meter,
        style,
        length,
        thickness,
        tokens,
    } = config.sanitized();
    let mut root = parent.spawn((
        meter,
        MeterValue(value),
        Node {
            width: length.map_or(percent(100), px),
            flex_grow: if length.is_some() { 0.0 } else { 1.0 },
            height: px(thickness),
            overflow: Overflow::clip(),
            ..default()
        },
    ));
    // Each style builds its own parts; colours come with the meter's tokens, in `style_meter`.
    let parts = match style {
        MeterStyle::Bar => {
            let mut fill = Entity::PLACEHOLDER;
            root.with_children(|track| {
                fill = track
                    .spawn((
                        Pickable::IGNORE,
                        Node {
                            width: percent(100.0 * meter.fraction(value)),
                            height: percent(100),
                            ..default()
                        },
                    ))
                    .id();
            });
            MeterParts { fill }
        }
    };
    root.insert((parts, tokens));
    root
}

/// A meter whose value or range changed.
type MeterChanged = Or<(Changed<Meter>, Changed<MeterValue>)>;

/// Fills each bar to its value whenever the value or the range changes.
fn fill_bars(
    meters: Query<(&Meter, &MeterValue, &MeterParts), MeterChanged>,
    mut nodes: Query<&mut Node>,
) {
    for (meter, value, parts) in &meters {
        let width = percent(100.0 * meter.fraction(value.0));
        if let Ok(mut fill) = nodes.get_mut(parts.fill)
            && fill.width != width
        {
            fill.width = width;
        }
    }
}

/// Styles a meter when its tokens are set: at spawn and whenever the app sets new ones.
fn style_meter(
    insert: On<Insert<MeterTokens>>,
    meters: Query<(&MeterParts, &MeterTokens)>,
    mut commands: Commands,
) {
    let Ok((parts, tokens)) = meters.get(insert.entity) else {
        return;
    };
    let rounded = ThemeCornerRadius(tokens.corner_radius.clone());
    commands
        .entity(insert.entity)
        .insert((rounded.clone(), ThemeSkin(tokens.track.clone())));
    commands
        .entity(parts.fill)
        .insert((rounded, ThemeSkin(tokens.fill.clone())));
}

/// Meter drawing and default token values.
pub struct MeterPlugin;

impl Plugin for MeterPlugin {
    fn build(&self, app: &mut App) {
        app.ensure_plugin(ThemePlugin)
            .add_observer(style_meter)
            .add_systems(PostUpdate, fill_bars.before(UiSystems::Prepare));
        MeterTokens::with_prefix(METER_STYLE)
            .register_theme_defaults(&mut app.world_mut().resource_mut::<Theme>());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit() -> Meter {
        Meter { min: 0.0, max: 1.0 }
    }

    #[test]
    fn a_value_fills_its_share_of_the_range_and_no_more() {
        let meter = Meter {
            min: -10.0,
            max: 30.0,
        };
        assert_eq!(meter.fraction(10.0), 0.5);
        assert_eq!(meter.fraction(-10.0), 0.0);
        assert_eq!(meter.fraction(50.0), 1.0, "past the end draws full");
        assert_eq!(meter.fraction(-20.0), 0.0, "before the start draws empty");
    }

    #[test]
    fn a_broken_range_or_value_reads_as_empty() {
        let backwards = Meter { min: 1.0, max: 0.0 };
        assert_eq!(backwards.fraction(0.5), 0.0);
        let empty = Meter { min: 0.5, max: 0.5 };
        assert_eq!(empty.fraction(0.5), 0.0);
        assert_eq!(unit().fraction(f32::NAN), 0.0);
        let endless = Meter {
            max: f32::INFINITY,
            ..unit()
        };
        assert_eq!(endless.fraction(3.0), 0.0);
    }

    fn fill_width(app: &mut App) -> Val {
        let world = app.world_mut();
        let parts = *world.query::<&MeterParts>().single(world).unwrap();
        world.get::<Node>(parts.fill).unwrap().width
    }

    #[test]
    fn the_bar_follows_its_value_and_its_range() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, MeterPlugin));
        let mut meter = None;
        app.world_mut()
            .commands()
            .spawn(Node::default())
            .with_children(|parent| {
                meter = Some(spawn_meter(parent, MeterConfig::new(0.25)).id());
            });
        app.update();
        let meter = meter.unwrap();
        assert_eq!(fill_width(&mut app), percent(25));

        app.world_mut().get_mut::<MeterValue>(meter).unwrap().0 = 0.75;
        app.update();
        assert_eq!(fill_width(&mut app), percent(75));

        app.world_mut().get_mut::<Meter>(meter).unwrap().max = 3.0;
        app.update();
        assert_eq!(fill_width(&mut app), percent(25), "0.75 of 3");
    }

    #[test]
    fn new_tokens_restyle_the_meter_at_once() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, MeterPlugin));
        let mut meter = None;
        app.world_mut()
            .commands()
            .spawn(Node::default())
            .with_children(|parent| meter = Some(spawn_meter(parent, MeterConfig::new(0.5)).id()));
        app.update();
        let meter = meter.unwrap();
        let custom = MeterTokens::with_prefix("test.meter");
        custom.register_theme_defaults(&mut app.world_mut().resource_mut::<Theme>());
        app.world_mut().entity_mut(meter).insert(custom.clone());
        app.world_mut().flush();
        let fill = app.world().get::<MeterParts>(meter).unwrap().fill;
        assert_eq!(app.world().get::<ThemeSkin>(meter).unwrap().0, custom.track);
        assert_eq!(app.world().get::<ThemeSkin>(fill).unwrap().0, custom.fill);
    }
}
