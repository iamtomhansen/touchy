//! A gearshift: a slider with detents. While dragging, the thumb is pulled
//! toward the nearest gear, lagging the cursor near a detent and hurrying
//! across the gap between two. On release it accelerates into the nearest gear.
//!
//! Built on touchy's slider. The slider's value stays "where the pointer is",
//! in gear units (gear `i` sits at value `i`); the bent position is drawn
//! through [`SliderDisplay`]. The chosen gear lives in [`GearIndex`], and
//! changes are announced as `ValueChange<usize>`.

use bevy::picking::cursor::CursorIconPlugin;
use bevy::picking::events::PointerRelease;
use bevy::prelude::*;
use bevy::ui_widgets::{SliderDragState, SliderValue, TrackClick, ValueChange};

use crate::EnsurePlugin;
use crate::motion::{Glide, ResponseCurve};
use crate::sanitize::{at_least, at_most};
use crate::theme::palette::TEXT_MUTED;
use crate::theme::{Shade, Theme, ThemePlugin, ThemeSkin, ThemeToken};
use crate::widgets::slider::{
    SliderConfig, SliderDisplay, SliderMark, SliderTokens, SliderWidgetPlugin, spawn_slider,
};

/// Prefix of the gearshift widget's theme tokens.
pub const GEARSHIFT_STYLE: &str = "touchy.gearshift";
/// Color of the tick drawn at each gear.
pub const GEAR_DETENT: ThemeToken = ThemeToken::new_static("touchy.gearshift.detent");

/// How strongly the thumb is held near a gear: 0 tracks the cursor exactly,
/// 1 holds it on the gear until the cursor is well on its way to the next.
const DEFAULT_PULL: f32 = 0.6;
/// Shape of the pull. Higher holds the thumb longer, then crosses the gap faster.
const PULL_EXPONENT: f32 = 3.0;
/// Track beyond the first and last gear, as a fraction of the gap between gears.
const DEFAULT_PADDING: f32 = 0.35;
/// Padding can't exceed half a gap, or the ends would have their own midpoint.
const MAX_PADDING: f32 = 0.5;
/// Constant acceleration of the release animation, in gears per second squared.
/// Half a gear takes about 0.13 s.
const SETTLE_ACCELERATION: f32 = 60.0;
const DEFAULT_THICKNESS: f32 = 20.0;
const DEFAULT_THUMB_LENGTH: f32 = 14.0;

/// Configuration for a gearshift widget. Use [`GearshiftConfig::new`] to create one.
#[derive(Clone, Debug)]
pub struct GearshiftConfig {
    /// Number of gears (options), at least 2.
    pub gears: usize,
    /// The gear to start in. If out of range, the last gear is used.
    pub selected: usize,
    /// The strength of the pull towards each gear. 0 means no pull, 1 means full pull.
    pub pull: f32,
    /// Extra track beyond the first and last gear, as a fraction of the gap between gears.
    pub padding: f32,
    /// Theme tokens for the widget.
    pub tokens: SliderTokens,
    /// The track's thickness in pixels, across it.
    pub thickness: f32,
    /// The thumb's length in pixels, along the track.
    pub thumb_length: f32,
}

impl GearshiftConfig {
    /// Creates a new gearshift config with the given number of gears (options).
    pub fn new(gears: usize) -> Self {
        Self {
            gears,
            selected: 0,
            pull: DEFAULT_PULL,
            padding: DEFAULT_PADDING,
            tokens: SliderTokens::with_prefix(GEARSHIFT_STYLE),
            thickness: DEFAULT_THICKNESS,
            thumb_length: DEFAULT_THUMB_LENGTH,
        }
    }

    /// Sets the number of gears (options).
    pub fn gears(mut self, gears: usize) -> Self {
        self.gears = gears;
        self
    }

    /// The gear to start in. If out of range, the last gear is used.
    pub fn selected(mut self, gear: usize) -> Self {
        self.selected = gear;
        self
    }

    /// The strength of the pull towards each gear. 0 means no pull, 1 means full pull.
    pub fn pull(mut self, pull: f32) -> Self {
        self.pull = pull;
        self
    }

    /// Extra track beyond the first and last gear, as a fraction of the gap between gears.
    pub fn padding(mut self, padding: f32) -> Self {
        self.padding = padding.clamp(0.0, MAX_PADDING);
        self
    }

    /// Theme tokens for the widget.
    pub fn tokens(mut self, tokens: SliderTokens) -> Self {
        self.tokens = tokens;
        self
    }

    /// The thickness of the track in pixels, across it.
    pub fn thickness(mut self, pixels: f32) -> Self {
        self.thickness = pixels;
        self
    }

    /// Validates the config.
    fn sanitized(mut self) -> Self {
        self.gears = at_least(self.gears, 2, "GearshiftConfig gears");
        self.selected = at_most(self.selected, self.gears - 1, "GearshiftConfig selected");
        self
    }
}

/// The gearshift's shape: how many gears, and how hard each one pulls.
#[derive(Component, Clone, Copy, Debug)]
pub struct Gearshift {
    gears: usize,
    pull: f32,
}

/// The selected gear. Set it to change gear from code; the thumb animates there.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct GearIndex(pub usize);

/// The release animation, accelerating into a gear.
#[derive(Component, Clone, Copy, Debug)]
struct Settling(Glide<f32>);

/// Spawns it; documented on [`SpawnUi::gearshift`](crate::spawn_ui::SpawnUi::gearshift).
pub(crate) fn spawn_gearshift<'a>(
    parent: &'a mut ChildSpawnerCommands,
    config: GearshiftConfig,
) -> EntityCommands<'a> {
    let config = config.sanitized();
    let last = (config.gears - 1) as f32;
    let range = -config.padding..=last + config.padding;
    let selected = config.selected as f32;
    let slider = SliderConfig::new(selected)
        .range(range)
        .track_click(TrackClick::Snap)
        .step(1.0) // Arrow keys move one gear.
        .thickness(config.thickness)
        .thumb_length(config.thumb_length)
        .self_update(false); // The gearshift decides where the thumb goes.

    let mut gearshift = spawn_slider(parent, slider);
    gearshift
        .insert((
            Gearshift {
                gears: config.gears,
                pull: config.pull,
            },
            GearIndex(config.selected),
            SliderDisplay(selected),
        ))
        .observe(follow_pointer)
        .observe(settle_on_release)
        .with_children(|track| {
            for gear in 0..config.gears {
                let mut detent = track.spawn((SliderMark::at(gear as f32), ThemeSkin(GEAR_DETENT)));
                detent.insert(ZIndex(-1)); // Behind the thumb.
            }
        });
    gearshift
}

/// The gear nearest to `value`.
fn nearest_gear(value: f32, gears: usize) -> usize {
    value.round().clamp(0.0, (gears - 1) as f32) as usize
}

/// Where to draw the thumb when the pointer is at `raw`, in gear units.
///
/// Within each gear's half-gaps, the offset from the gear (`u` in `-1..=1`) is
/// bent by a [`ResponseCurve::Blend`]. Its guarantees are what make this feel
/// right: gears and midpoints stay fixed, so the thumb never jumps; both sides
/// of a gear feel the same; and the thumb never moves against the cursor. Near
/// a gear the slope is `1 - pull` (the thumb lags); near a midpoint it exceeds
/// 1 (it hurries across).
fn detent_pull(raw: f32, gears: usize, pull: f32) -> f32 {
    let gear = nearest_gear(raw, gears) as f32;
    let curve = ResponseCurve::Blend {
        power: PULL_EXPONENT,
        amount: pull,
    };
    gear + 0.5 * curve.apply((raw - gear) / 0.5)
}

/// While the pointer drags the thumb, draw it pulled toward the nearest gear.
/// Anything else settles into a gear with the accelerating move: a click on
/// the track (which isn't a drag), an arrow key or the end of a drag (both final).
fn follow_pointer(
    change: On<ValueChange<f32>>,
    gearshifts: Query<(&Gearshift, &SliderDisplay, &GearIndex, &SliderDragState)>,
    mut commands: Commands,
) {
    let Ok((&shift, display, &index, drag)) = gearshifts.get(change.source) else {
        return;
    };
    if change.is_final || !drag.dragging {
        let target = nearest_gear(change.value, shift.gears);
        settle(&mut commands, change.source, display.0, target, index);
    } else {
        commands
            .entity(change.source)
            .remove::<Settling>()
            .insert(SliderDisplay(detent_pull(
                change.value,
                shift.gears,
                shift.pull,
            )));
    }
}

/// A click without a drag never sends a final change, so settle on release too.
fn settle_on_release(
    release: On<PointerRelease>,
    gearshifts: Query<(&Gearshift, &SliderDisplay, &GearIndex, Has<Settling>)>,
    mut commands: Commands,
) {
    let Ok((&shift, display, &index, settling)) = gearshifts.get(release.entity) else {
        return;
    };
    if !settling {
        settle(
            &mut commands,
            release.entity,
            display.0,
            nearest_gear(display.0, shift.gears),
            index,
        );
    }
}

/// Selects `target` and starts the thumb's accelerating move into it.
fn settle(commands: &mut Commands, entity: Entity, display: f32, target: usize, index: GearIndex) {
    let to = target as f32;
    commands.entity(entity).insert((
        SliderValue(to), // The next drag starts from the gear.
        Settling(Glide::new(display, to, SETTLE_ACCELERATION)),
    ));
    if index.0 != target {
        commands.entity(entity).insert(GearIndex(target));
        commands.trigger(ValueChange {
            source: entity,
            value: target,
            is_final: true,
        });
    }
}

/// The app set `GearIndex` itself: move the thumb there.
fn follow_gear_index(
    gearshifts: Query<(Entity, &GearIndex, &SliderValue, &SliderDisplay), Changed<GearIndex>>,
    mut commands: Commands,
) {
    for (entity, &GearIndex(gear), value, display) in &gearshifts {
        let to = gear as f32;
        if value.0 != to {
            commands.entity(entity).insert((
                SliderValue(to),
                Settling(Glide::new(display.0, to, SETTLE_ACCELERATION)),
            ));
        }
    }
}

/// Moves settling thumbs, accelerating from rest so each lands with a clunk.
/// Real time, so the animation runs even while the game is paused.
fn animate_settling(
    time: Res<Time<Real>>,
    mut settling: Query<(Entity, &mut Settling)>,
    mut commands: Commands,
) {
    for (entity, mut settling) in &mut settling {
        let (position, arrived) = settling.0.advance(time.delta_secs());
        let mut gearshift = commands.entity(entity);
        gearshift.insert(SliderDisplay(position));
        if arrived {
            gearshift.remove::<Settling>();
        }
    }
}

/// Gearshift behaviour and default token values. Needs `SliderWidgetPlugin`.
pub struct GearshiftWidgetPlugin;

impl Plugin for GearshiftWidgetPlugin {
    fn build(&self, app: &mut App) {
        app.ensure_plugin(CursorIconPlugin)
            .ensure_plugin(SliderWidgetPlugin)
            .ensure_plugin(ThemePlugin)
            .add_systems(Update, (follow_gear_index, animate_settling).chain());
        let mut theme = app.world_mut().resource_mut::<Theme>();
        theme.default_derived(GEAR_DETENT, TEXT_MUTED, Shade::Same);
        SliderTokens::with_prefix(GEARSHIFT_STYLE).register_theme_defaults(&mut theme);
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy::time::TimeUpdateStrategy;

    use super::*;
    use crate::theme::ThemePlugin;
    use crate::widgets::slider::SliderWidgetPlugin;

    const GEARS: usize = 4;

    #[test]
    fn pull_never_jumps_never_reverses_and_holds_gears() {
        let pull = |raw| detent_pull(raw, GEARS, DEFAULT_PULL);
        for gear in 0..GEARS {
            assert_eq!(pull(gear as f32), gear as f32, "gears stay put");
        }
        for gap in 0..GEARS - 1 {
            let midpoint = gap as f32 + 0.5;
            assert!(
                (pull(midpoint) - midpoint).abs() < 1e-5,
                "midpoints stay put, so no jump"
            );
        }
        // Sweep the whole track: the thumb only ever moves with the cursor.
        let mut previous = pull(-DEFAULT_PADDING);
        for step in 1..=1000 {
            let raw = -DEFAULT_PADDING
                + step as f32 * (GEARS as f32 - 1.0 + 2.0 * DEFAULT_PADDING) / 1000.0;
            let shown = pull(raw);
            assert!(
                shown >= previous - 1e-5,
                "thumb moved against the cursor at {raw}"
            );
            previous = shown;
        }
        // Just past a gear, the thumb lags the cursor toward the gear.
        assert!(pull(1.2) > 1.0 && pull(1.2) < 1.2);
        assert_eq!(
            detent_pull(1.2, GEARS, 0.0),
            1.2,
            "zero pull is a plain slider"
        );
    }

    fn app_with_gearshift() -> (App, Entity) {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            ThemePlugin,
            SliderWidgetPlugin,
            GearshiftWidgetPlugin,
        ))
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
            20,
        )))
        .init_resource::<Announced>()
        .add_observer(
            |change: On<ValueChange<usize>>, mut announced: ResMut<Announced>| {
                announced.0.push(change.value);
            },
        );
        let mut entity = None;
        app.world_mut()
            .commands()
            .spawn(Node::default())
            .with_children(|root| {
                entity = Some(spawn_gearshift(root, GearshiftConfig::new(GEARS)).id());
            });
        app.update();
        (app, entity.unwrap())
    }

    #[derive(Resource, Default)]
    struct Announced(Vec<usize>);

    fn display(app: &App, entity: Entity) -> f32 {
        app.world().get::<SliderDisplay>(entity).unwrap().0
    }

    #[test]
    fn dragging_bends_the_thumb_and_release_settles_into_the_nearest_gear() {
        let (mut app, shift) = app_with_gearshift();
        // A pointer drag in progress, so the thumb bends rather than settles.
        app.world_mut()
            .get_mut::<SliderDragState>(shift)
            .unwrap()
            .dragging = true;
        app.world_mut().trigger(ValueChange {
            source: shift,
            value: 1.3_f32,
            is_final: false,
        });
        app.update();
        let dragged = display(&app, shift);
        assert!(
            dragged > 1.0 && dragged < 1.3,
            "thumb lags toward gear 1, got {dragged}"
        );

        app.world_mut().trigger(ValueChange {
            source: shift,
            value: 1.3_f32,
            is_final: true,
        });
        app.update();
        assert_eq!(app.world().get::<GearIndex>(shift), Some(&GearIndex(1)));
        assert_eq!(app.world().resource::<Announced>().0, [1]);
        for _ in 0..10 {
            app.update();
        }
        assert_eq!(display(&app, shift), 1.0, "thumb ends exactly on the gear");
    }

    #[test]
    fn setting_the_gear_from_code_animates_the_thumb_there() {
        let (mut app, shift) = app_with_gearshift();
        app.world_mut().entity_mut(shift).insert(GearIndex(3));
        app.update();
        app.update();
        let moving = display(&app, shift);
        assert!(
            moving > 0.0 && moving < 3.0,
            "thumb is on its way, got {moving}"
        );
        for _ in 0..30 {
            app.update();
        }
        assert_eq!(display(&app, shift), 3.0);
        assert!(
            app.world().resource::<Announced>().0.is_empty(),
            "code changes aren't re-announced"
        );
    }
}
