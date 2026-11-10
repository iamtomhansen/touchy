//! A capsule flip: a pill-shaped switch whose thumb glides from one end to the
//! other, accelerating into place, with an optional label for each option.
//!
//! Behaviour is Bevy's headless [`Checkbox`], so it's checked (on) or not, emits
//! `ValueChange<bool>`, and toggles with a click or Enter/Space. The thumb's
//! motion is a [`Glide`], and runs whenever [`Checked`] changes, including
//! when the app changes it.

use bevy::ecs::entity::EntityHashSet;
use bevy::ecs::query::QueryData;
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui::{Checked, InteractionDisabled, UiSystems};
use bevy::ui_widgets::{Checkbox, checkbox_self_update};

use crate::EnsurePlugin;
use crate::cursor::{HoverCursor, HoverCursorPlugin};
use crate::motion::Glide;
use crate::sanitize::at_least;
use crate::theme::palette::{ACCENT, CONTROL, EDGE, FONT, HIGHLIGHT, SMALL_SIZE, TEXT, TEXT_MUTED};
use crate::theme::skins;
use crate::theme::{
    FocusVisible, Shade, SkinFrame, Theme, ThemeBorder, ThemeFont, ThemeFontSize, ThemePlugin,
    ThemeSkin, ThemeTextColor, ThemeToken, WidgetFlags, WidgetState,
};

/// Prefix of the capsule widget's theme tokens.
pub const CAPSULE_FLIP_STYLE: &str = "touchy.capsule";

/// The default size of the capsule's track in pixels.
const DEFAULT_TRACK_SIZE: Vec2 = Vec2::new(40.0, 22.0);
/// The width of the track's border in pixels.
const TRACK_BORDER: f32 = 1.0;
/// Gap between the thumb and the track's inner edge.
const THUMB_INSET: f32 = 2.0;
/// Space between the track's edge and the thumb's, so the least a track can be.
const THUMB_MARGIN: f32 = TRACK_BORDER + THUMB_INSET;
/// The smallest thumb a capsule draws, in pixels.
const MIN_THUMB: f32 = 2.0;
/// Constant acceleration of the flip, in whole flips per second squared.
/// A full flip takes about 0.2 s.
const FLIP_ACCELERATION: f32 = 50.0;
/// The gap between the capsule and its labels, in pixels.
const LABEL_GAP: f32 = 8.0;

/// Theme tokens for a capsule widget. Insert new ones to restyle a capsule.
#[derive(Component, Clone, PartialEq, Debug)]
#[component(immutable)]
pub struct CapsuleFlipTokens {
    /// The track when it is off.
    pub track_off: ThemeToken,
    /// The track when it is on.
    pub track_on: ThemeToken,
    /// The border around the track.
    pub border: ThemeToken,
    /// The thumb that glides across the track.
    pub thumb: ThemeToken,
    /// The label of the option currently chosen.
    pub label_active: ThemeToken,
    /// The label of the option not chosen.
    pub label_inactive: ThemeToken,
    /// The font of the labels.
    pub font: ThemeToken,
    /// The font size of the labels.
    pub font_size: ThemeToken,
}

impl CapsuleFlipTokens {
    /// Names the tokens under `prefix`.
    pub fn with_prefix(prefix: &str) -> Self {
        let token = |suffix: &str| ThemeToken::new(format!("{prefix}.{suffix}"));
        Self {
            track_off: token("track.off"),
            track_on: token("track.on"),
            border: token("border"),
            thumb: token("thumb"),
            label_active: token("label.active"),
            label_inactive: token("label.inactive"),
            font: token("font"),
            font_size: token("font_size"),
        }
    }

    /// Derives these tokens from the palette, unless the app sets them.
    pub fn register_theme_defaults(&self, theme: &mut Theme) {
        let lit_border = |state| self.border.clone().with_state(state);
        theme
            .default_derived(self.track_off.clone(), CONTROL, Shade::Same)
            .default_derived(self.track_on.clone(), ACCENT, Shade::Same)
            .default_derived(self.border.clone(), EDGE, Shade::Same)
            .default_derived(lit_border(WidgetState::Hover), HIGHLIGHT, Shade::Same)
            .default_derived(lit_border(WidgetState::Focused), HIGHLIGHT, Shade::Same)
            .default_derived(self.thumb.clone(), TEXT, Shade::Same)
            .default_derived(self.label_active.clone(), TEXT, Shade::Same)
            .default_derived(self.label_inactive.clone(), TEXT_MUTED, Shade::Same)
            .default_font_from(self.font.clone(), FONT)
            .default_size_from(self.font_size.clone(), SMALL_SIZE);
        // Each part draws its role's skin, once a theme gives the role one. Not the pill-shaped
        // track, whose roundness a role's square picture would spoil.
        theme.default_skin_from(self.thumb.clone(), skins::KNOB);
    }

    /// The track's background: on or off picks the token, disabling its state.
    fn track(&self, state: CapsuleState) -> ThemeSkin {
        let token = if state.checked {
            &self.track_on
        } else {
            &self.track_off
        };
        ThemeSkin(token.clone().with_state(disabled_state(state.disabled)))
    }

    /// The track's border, which alone shows hovering and the focus.
    fn border(&self, state: CapsuleState) -> ThemeBorder {
        let flags = WidgetFlags {
            hovered: state.hovered,
            focused: state.focused,
            pressed: false,
            disabled: state.disabled,
        };
        let state = flags.state();
        ThemeBorder::all(self.border.clone().with_state(state))
    }

    /// An option's label: chosen or not picks the token, disabling its state.
    fn label(&self, on: bool, state: CapsuleState) -> ThemeTextColor {
        let token = if on == state.checked {
            &self.label_active
        } else {
            &self.label_inactive
        };
        ThemeTextColor(token.clone().with_state(disabled_state(state.disabled)))
    }
}

/// The built-in token set.
impl Default for CapsuleFlipTokens {
    fn default() -> Self {
        Self::with_prefix(CAPSULE_FLIP_STYLE)
    }
}

/// Only disabling changes the track and labels; hovering and the focus change just the border.
fn disabled_state(disabled: bool) -> Option<WidgetState> {
    disabled.then_some(WidgetState::Disabled)
}

/// Construction-time configuration for a capsule flip widget.
///
/// Consumed by [`SpawnUi::capsule_flip`](crate::spawn_ui::SpawnUi::capsule_flip) and not retained
/// as a component.
///
/// # Examples
/// ```
/// use touchy::prelude::*;
///
/// let config = CapsuleFlipConfig::new()
///     .options("Mouse", "Keyboard");
/// ```
#[derive(Clone, Debug)]
pub struct CapsuleFlipConfig {
    /// Labels for the off and on options, drawn either side of the capsule.
    pub options: Option<(String, String)>,
    /// The track's size, in pixels.
    pub size: Vec2,
    /// The thumb's diameter, in pixels, or `None` for "fill track".
    pub thumb: Option<f32>,
    /// Whether the capsule starts checked (on).
    pub checked: bool,
    /// Whether the capsule is disabled at spawn.
    pub disabled: bool,
    /// Whether the capsule should self-update its [`Checked`] state on a click.
    pub self_update: bool,
    /// Theme tokens for the capsule's track, thumb, border and labels.
    pub tokens: CapsuleFlipTokens,
}

impl CapsuleFlipConfig {
    /// Creates a new config with default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the labels for the off and on options, drawn either side of the capsule.
    pub fn options(mut self, off: impl Into<String>, on: impl Into<String>) -> Self {
        self.options = Some((off.into(), on.into()));
        self
    }

    /// Sets the track's size.
    pub fn size(mut self, width: f32, height: f32) -> Self {
        self.size = Vec2::new(width, height);
        self
    }

    /// Sets the thumb's diameter, in pixels.
    pub fn thumb(mut self, diameter: f32) -> Self {
        self.thumb = Some(diameter);
        self
    }

    /// Sets whether the capsule starts checked (on).
    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }

    /// Sets whether the capsule is disabled at spawn.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Sets whether the capsule should self-update its [`Checked`] state on a click.
    pub fn self_update(mut self, self_update: bool) -> Self {
        self.self_update = self_update;
        self
    }

    /// Sets the theme tokens for capsule flip's track, thumb, border and labels. The default is
    /// [`CAPSULE_FLIP_STYLE`].
    pub fn tokens(mut self, tokens: CapsuleFlipTokens) -> Self {
        self.tokens = tokens;
        self
    }

    /// This config with every value made usable, warning about each repair.
    fn sanitized(mut self) -> Self {
        match self.thumb {
            // A thumb of its own needs only a track to slide along.
            Some(diameter) => {
                let least = Vec2::splat(2.0 * TRACK_BORDER);
                self.size = at_least(self.size, least, "CapsuleConfig size");
                self.thumb = Some(at_least(diameter, MIN_THUMB, "CapsuleConfig thumb"));
            }
            // Room for a thumb inside the track, however small.
            None => {
                let least = Vec2::splat(2.0 * THUMB_MARGIN + MIN_THUMB);
                let what = "CapsuleConfig size, which its thumb fits inside (`thumb` sets one \
                            that overhangs a thinner track),";
                self.size = at_least(self.size, least, what);
            }
        }
        // Room for the thumb to travel.
        self.size.x = at_least(self.size.x, self.size.y, "CapsuleConfig width");
        self
    }

    /// The thumb's diameter: as given, or as big as fits inside the track.
    fn thumb_diameter(&self) -> f32 {
        self.thumb.unwrap_or(self.size.y - 2.0 * THUMB_MARGIN)
    }
}

impl Default for CapsuleFlipConfig {
    fn default() -> Self {
        Self {
            options: None,
            size: DEFAULT_TRACK_SIZE,
            thumb: None,
            checked: false,
            disabled: false,
            self_update: true,
            tokens: CapsuleFlipTokens::with_prefix(CAPSULE_FLIP_STYLE),
        }
    }
}

/// The parts of a capsule, on the capsule.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct CapsuleFlipParts {
    /// The pill the thumb glides along.
    pub track: Entity,
    /// The thumb.
    pub thumb: Entity,
    /// The labels of the off and on options, if it has them.
    pub options: Option<(Entity, Entity)>,
}

/// Where a capsule's thumb is, and its glide to the other end while it flips.
#[derive(Component, Clone, Copy, Debug)]
struct Flip {
    /// 0 at the off end, 1 at the on end.
    position: f32,
    /// How far in from the edge the thumb sits at either end, in pixels (negative for overhangs).
    inset: f32,
    /// How far the thumb's left edge moves between the ends, in pixels.
    travel: f32,
    /// The thumb's glide to the other end, if it is flipping.
    glide: Option<Glide<f32>>,
}

impl Flip {
    /// The thumb's left edge.
    fn left(&self) -> Val {
        px(self.inset + self.position * self.travel)
    }
}

/// Spawns it; documented on [`SpawnUi::capsule_flip`](crate::spawn_ui::SpawnUi::capsule_flip).
pub(crate) fn spawn_capsule_flip<'a>(
    parent: &'a mut ChildSpawnerCommands,
    config: CapsuleFlipConfig,
) -> EntityCommands<'a> {
    let config = config.sanitized();
    let thumb_size = config.thumb_diameter();
    // The thumb is centred across the track.
    let inset = (config.size.y - 2.0 * TRACK_BORDER - thumb_size) / 2.0;
    let flip = Flip {
        position: if config.checked { 1.0 } else { 0.0 },
        inset,
        travel: config.size.x - config.size.y,
        glide: None,
    };
    let mut capsule = parent.spawn((
        Checkbox,
        // Tab in layout order; Bevy's checkbox flips it on Enter and Space once focused.
        TabIndex(0),
        Hovered::default(),
        HoverCursor::POINTER,
        flip,
        Node {
            align_items: AlignItems::Center,
            column_gap: px(LABEL_GAP),
            ..default()
        },
    ));
    if config.checked {
        capsule.insert(Checked);
    }
    if config.disabled {
        capsule.insert(InteractionDisabled);
    }
    if config.self_update {
        capsule.observe(checkbox_self_update);
    }

    let (off, on) = config.options.unzip();
    let label = |text| (Pickable::IGNORE, Text::new(text));
    let mut parts = CapsuleFlipParts {
        track: Entity::PLACEHOLDER,
        thumb: Entity::PLACEHOLDER,
        options: None,
    };
    capsule.with_children(|row| {
        let off = off.map(|text| row.spawn(label(text)).id());
        parts.track = row
            .spawn((
                Pickable::IGNORE,
                Node {
                    width: px(config.size.x),
                    height: px(config.size.y),
                    // Track doesn't get squeezed by neighbours.
                    flex_shrink: 0.0,
                    // Room for an overhanging thumb, so it doesn't cover its neighbours.
                    margin: UiRect::all(px((-inset).max(0.0))),
                    border: UiRect::all(px(TRACK_BORDER)),
                    border_radius: BorderRadius::MAX,
                    ..default()
                },
            ))
            .with_children(|track| {
                let thumb = Node {
                    position_type: PositionType::Absolute,
                    top: px(inset),
                    left: flip.left(),
                    width: px(thumb_size),
                    height: px(thumb_size),
                    border_radius: BorderRadius::MAX,
                    ..default()
                };
                // A sprite-sheet skin on the thumb plays through as it glides across.
                let frame = SkinFrame(flip.position);
                parts.thumb = track.spawn((Pickable::IGNORE, thumb, frame)).id();
            })
            .id();
        let on = on.map(|text| row.spawn(label(text)).id());
        parts.options = off.zip(on);
    });
    // Its look comes with its tokens, in `style_capsule_flip`, once its state is in.
    capsule.insert((parts, config.tokens));
    capsule
}

/// What a capsule is doing, which picks its look.
#[derive(Clone, Copy, Debug)]
struct CapsuleState {
    hovered: bool,
    focused: bool,
    checked: bool,
    disabled: bool,
}

/// A capsule's state, as its components hold it.
#[derive(QueryData)]
struct CapsuleStatus {
    hovered: &'static Hovered,
    focused: Has<FocusVisible>,
    checked: Has<Checked>,
    disabled: Has<InteractionDisabled>,
}

impl CapsuleStatusItem<'_, '_> {
    fn state(&self) -> CapsuleState {
        CapsuleState {
            hovered: self.hovered.0,
            focused: self.focused,
            checked: self.checked,
            disabled: self.disabled,
        }
    }
}

/// Starts the thumb gliding whenever the capsule is checked or unchecked,
/// whether by a click or by the app.
fn start_flips(
    checked: Query<Entity, (With<CapsuleFlipParts>, Added<Checked>)>,
    mut unchecked: RemovedComponents<Checked>,
    mut capsules: Query<(&mut Flip, Has<Checked>)>,
) {
    for entity in checked.iter().chain(unchecked.read()) {
        let Ok((mut flip, is_checked)) = capsules.get_mut(entity) else {
            continue;
        };
        let target = if is_checked { 1.0 } else { 0.0 };
        if flip.position != target {
            flip.glide = Some(Glide::new(flip.position, target, FLIP_ACCELERATION));
        }
    }
}

/// Moves flipping thumbs. Real time, so flips finish while the game is paused.
fn animate_flips(time: Res<Time<Real>>, mut capsules: Query<&mut Flip>) {
    // Only flips that move are touched, so resting thumbs aren't redrawn.
    for mut flip in capsules.iter_mut().filter(|flip| flip.glide.is_some()) {
        let Some(glide) = &mut flip.glide else {
            continue;
        };
        let (next, arrived) = glide.advance(time.delta_secs());
        flip.position = next;
        if arrived {
            flip.glide = None;
        }
    }
}

/// Draws each thumb where its flip has it, and the sprite-sheet frame for how far it's got:
/// the first at the off end, the last at the on end.
fn position_thumbs(
    capsules: Query<(&Flip, &CapsuleFlipParts), Changed<Flip>>,
    mut thumbs: Query<(&mut Node, &mut SkinFrame)>,
) {
    for (flip, parts) in &capsules {
        let Ok((mut node, mut frame)) = thumbs.get_mut(parts.thumb) else {
            continue;
        };
        let left = flip.left();
        if node.left != left {
            node.left = left;
        }
        frame.set_if_neq(SkinFrame(flip.position));
    }
}

/// State changes that can alter a capsule's look. Removals are read separately,
/// because change detection doesn't see them, and new tokens in `style_capsule_flip`.
type LookChanged = (
    With<CapsuleFlipParts>,
    Or<(
        Changed<Hovered>,
        Added<FocusVisible>,
        Added<Checked>,
        Added<InteractionDisabled>,
    )>,
);

/// Recolours the track, border and labels as state changes.
fn restyle_capsule_flips(
    changed: Query<Entity, LookChanged>,
    // States that ended, which change detection doesn't see.
    (mut unchecked, mut enabled, mut unfocused): (
        RemovedComponents<Checked>,
        RemovedComponents<InteractionDisabled>,
        RemovedComponents<FocusVisible>,
    ),
    capsules: Query<(&CapsuleFlipTokens, &CapsuleFlipParts, CapsuleStatus)>,
    tracks: Query<(&ThemeSkin, &ThemeBorder)>,
    labels: Query<&ThemeTextColor>,
    mut commands: Commands,
) {
    let mut dirty: EntityHashSet = changed.iter().collect();
    dirty.extend(
        unchecked
            .read()
            .chain(enabled.read())
            .chain(unfocused.read()),
    );
    for entity in dirty {
        let Ok((tokens, parts, status)) = capsules.get(entity) else {
            continue;
        };
        let state = status.state();
        if let Ok((old_track, old_border)) = tracks.get(parts.track) {
            let (track, border) = (tokens.track(state), tokens.border(state));
            if *old_track != track {
                commands.entity(parts.track).insert(track);
            }
            if *old_border != border {
                commands.entity(parts.track).insert(border);
            }
        }
        let Some((off, on)) = parts.options else {
            continue;
        };
        for (label, is_on) in [(off, false), (on, true)] {
            let color = tokens.label(is_on, state);
            if labels.get(label).is_ok_and(|old| *old != color) {
                commands.entity(label).insert(color);
            }
        }
    }
}

/// Styles a capsule when its tokens are set: at spawn, and whenever the app sets new ones.
fn style_capsule_flip(
    insert: On<Insert<CapsuleFlipTokens>>,
    capsules: Query<(&CapsuleFlipTokens, &CapsuleFlipParts, CapsuleStatus)>,
    mut commands: Commands,
) {
    let Ok((tokens, parts, status)) = capsules.get(insert.entity) else {
        return;
    };
    let state = status.state();
    commands
        .entity(parts.track)
        .insert((tokens.track(state), tokens.border(state)));
    commands
        .entity(parts.thumb)
        .insert(ThemeSkin(tokens.thumb.clone()));
    if let Some((off, on)) = parts.options {
        let font = (
            ThemeFont(tokens.font.clone()),
            ThemeFontSize(tokens.font_size.clone()),
        );
        for (label, is_on) in [(off, false), (on, true)] {
            commands
                .entity(label)
                .insert((font.clone(), tokens.label(is_on, state)));
        }
    }
}

/// Capsule flip motion, styling and default token values.
pub struct CapsuleFlipWidgetPlugin;

impl Plugin for CapsuleFlipWidgetPlugin {
    fn build(&self, app: &mut App) {
        app.ensure_plugin(HoverCursorPlugin)
            .ensure_plugin(ThemePlugin)
            .add_observer(style_capsule_flip)
            .add_systems(Update, (start_flips, animate_flips).chain())
            .add_systems(
                PostUpdate,
                (position_thumbs, restyle_capsule_flips).before(UiSystems::Prepare),
            );
        CapsuleFlipTokens::with_prefix(CAPSULE_FLIP_STYLE)
            .register_theme_defaults(&mut app.world_mut().resource_mut::<Theme>());
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy::time::TimeUpdateStrategy;
    use bevy::ui_widgets::{CheckboxPlugin, ToggleChecked};

    use super::*;

    fn app_with_capsule(config: CapsuleFlipConfig) -> (App, Entity) {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, CheckboxPlugin, CapsuleFlipWidgetPlugin))
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
                20,
            )));
        let mut capsule = None;
        app.world_mut()
            .commands()
            .spawn(Node::default())
            .with_children(|root| {
                capsule = Some(spawn_capsule_flip(root, config).id());
            });
        app.update();
        (app, capsule.unwrap())
    }

    fn parts(app: &App, capsule: Entity) -> CapsuleFlipParts {
        *app.world().get::<CapsuleFlipParts>(capsule).unwrap()
    }

    /// The thumb's left edge, in pixels.
    fn thumb_left(app: &App, capsule: Entity) -> f32 {
        let thumb = parts(app, capsule).thumb;
        let Val::Px(left) = app.world().get::<Node>(thumb).unwrap().left else {
            panic!("the thumb is placed in pixels")
        };
        left
    }

    /// The far end of the default track, where the thumb rests when on.
    fn on_end() -> f32 {
        let thumb = DEFAULT_TRACK_SIZE.y - 2.0 * THUMB_MARGIN;
        // Inside the border, an inset short of the end.
        DEFAULT_TRACK_SIZE.x - 2.0 * TRACK_BORDER - THUMB_INSET - thumb
    }

    fn label_color(app: &App, capsule: Entity, on: bool) -> ThemeToken {
        let (off_label, on_label) = parts(app, capsule).options.unwrap();
        let label = if on { on_label } else { off_label };
        app.world().get::<ThemeTextColor>(label).unwrap().0.clone()
    }

    #[test]
    fn a_click_glides_the_thumb_across_and_swaps_which_option_is_lit() {
        let (mut app, capsule) = app_with_capsule(CapsuleFlipConfig::new().options("Home", "Away"));
        let tokens = CapsuleFlipTokens::with_prefix(CAPSULE_FLIP_STYLE);
        assert_eq!(thumb_left(&app, capsule), THUMB_INSET);
        assert_eq!(label_color(&app, capsule, false), tokens.label_active);

        app.world_mut().trigger(ToggleChecked { entity: capsule });
        app.update();
        app.update();
        let midway = thumb_left(&app, capsule);
        assert!(
            midway > THUMB_INSET && midway < on_end(),
            "thumb is on its way, at {midway}"
        );
        assert_eq!(
            label_color(&app, capsule, true),
            tokens.label_active,
            "the on option lights up at once"
        );

        for _ in 0..20 {
            app.update();
        }
        assert_eq!(
            thumb_left(&app, capsule),
            on_end(),
            "thumb lands exactly at the on end"
        );
    }

    /// The thumb's sprite-sheet frame follows its glide, so an animated skin plays through.
    #[test]
    fn the_thumb_frame_follows_the_glide() {
        let (mut app, capsule) = app_with_capsule(CapsuleFlipConfig::new());
        let thumb = parts(&app, capsule).thumb;
        let frame = |app: &App| app.world().get::<SkinFrame>(thumb).unwrap().0;
        assert_eq!(frame(&app), 0.0);
        app.world_mut().entity_mut(capsule).insert(Checked);
        app.update();
        app.update();
        let midway = frame(&app);
        assert!(midway > 0.0 && midway < 1.0, "the frame jumped to {midway}");
        for _ in 0..60 {
            app.update();
        }
        assert_eq!(frame(&app), 1.0);
    }

    #[test]
    fn the_app_flipping_it_animates_too_and_spawning_checked_does_not() {
        let (mut app, capsule) = app_with_capsule(CapsuleFlipConfig::new().checked(true));
        let flipping = |app: &App| app.world().get::<Flip>(capsule).unwrap().glide.is_some();
        assert_eq!(
            thumb_left(&app, capsule),
            on_end(),
            "starts at the on end with no animation"
        );
        assert!(!flipping(&app));

        app.world_mut().entity_mut(capsule).remove::<Checked>();
        app.update();
        assert!(flipping(&app), "the app unchecking it starts a flip");
        for _ in 0..20 {
            app.update();
        }
        assert_eq!(thumb_left(&app, capsule), THUMB_INSET);
    }

    #[test]
    fn a_bigger_track_has_a_bigger_thumb_that_travels_further() {
        let (app, capsule) =
            app_with_capsule(CapsuleFlipConfig::new().size(80.0, 30.0).checked(true));
        let thumb = parts(&app, capsule).thumb;
        let node = app.world().get::<Node>(thumb).unwrap();
        assert_eq!(node.width, px(30.0 - 2.0 * THUMB_MARGIN));
        let thumb_size = 30.0 - 2.0 * THUMB_MARGIN;
        let on_end = 80.0 - 2.0 * TRACK_BORDER - THUMB_INSET - thumb_size;
        assert_eq!(thumb_left(&app, capsule), on_end);

        let tiny = CapsuleFlipConfig::new().size(1.0, 2.0).sanitized();
        assert_eq!(tiny.size, Vec2::splat(2.0 * THUMB_MARGIN + MIN_THUMB));
    }

    /// A thumb bigger than its thin track overhangs it evenly, travels the
    /// same way, and the track makes room for it.
    #[test]
    fn a_big_thumb_overhangs_a_thin_track() {
        let config = CapsuleFlipConfig::new()
            .size(36.0, 8.0)
            .thumb(18.0)
            .checked(true);
        let (app, capsule) = app_with_capsule(config);
        let parts = parts(&app, capsule);
        let thumb = app.world().get::<Node>(parts.thumb).unwrap();
        // The track is 6 pixels inside its border, so the thumb hangs 6 over each side.
        assert_eq!(thumb.top, px(-6.0));
        assert_eq!(thumb.width, px(18.0));
        assert_eq!(thumb_left(&app, capsule), -6.0 + (36.0 - 8.0));
        let track = app.world().get::<Node>(parts.track).unwrap();
        assert_eq!(track.margin, UiRect::all(px(6.0)));

        // A thin track is fine with a thumb of its own.
        let thin = CapsuleFlipConfig::new()
            .size(20.0, 5.0)
            .thumb(12.0)
            .sanitized();
        assert_eq!(thin.size, Vec2::new(20.0, 5.0));
    }

    #[test]
    fn new_tokens_and_disabling_restyle_it() {
        let config = CapsuleFlipConfig::new().options("A", "B");
        let (mut app, capsule) = app_with_capsule(config);
        let tokens = CapsuleFlipTokens::with_prefix("game.mode");
        app.world_mut().entity_mut(capsule).insert(tokens.clone());
        app.world_mut()
            .entity_mut(capsule)
            .insert(InteractionDisabled);
        app.update();

        let parts = parts(&app, capsule);
        let world = app.world();
        let disabled = tokens.track_off.clone().with_state(WidgetState::Disabled);
        let track = world.get::<ThemeSkin>(parts.track).unwrap();
        assert_eq!(track.0, disabled);
        let thumb = world.get::<ThemeSkin>(parts.thumb).unwrap();
        assert_eq!(thumb.0, tokens.thumb);
        let (off, _) = parts.options.unwrap();
        assert_eq!(world.get::<ThemeFont>(off).unwrap().0, tokens.font);
        assert_eq!(
            world.get::<bevy::picking::cursor::EntityCursor>(capsule),
            Some(&bevy::picking::cursor::EntityCursor::System(
                bevy::window::SystemCursorIcon::NotAllowed
            ))
        );
    }

    /// Labels short of room never squeeze the track, which the thumb's
    /// travel was measured for.
    #[test]
    fn the_track_never_shrinks() {
        let (app, capsule) = app_with_capsule(CapsuleFlipConfig::new().options("", "A long label"));
        let track = app.world().get::<Node>(parts(&app, capsule).track).unwrap();
        assert_eq!(track.flex_shrink, 0.0);
    }
}
