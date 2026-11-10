//! Transitions: a node changing state eases into its new skin, layer by
//! layer, over the times the skin gives, instead of snapping.
//!
//! A widget changes state by inserting a new [`ThemeSkin`] (bindings are
//! immutable), so that's where a transition starts: this module's observer
//! notes what the node showed, applies the new skin, then puts the old look
//! back on each layer that has a time and starts a clock for it. One system
//! moves only the nodes that are mid-change, and lets go when they arrive, so
//! a node at rest is never touched. A new theme snaps: it's applied whole.

use bevy::color::{Mix, Oklaba};
use bevy::ecs::query::QueryItem;
use bevy::prelude::*;
use bevy::ui::{ColorStop, Gradient, ShadowStyle};

use super::binding::ThemeBinding;
use super::{SkinTransition, Theme, ThemeSkin};
use crate::motion::{ResponseCurve, Tween};

/// How a transition eases: quickly at first, then settling, so a state
/// change answers at once.
const EASING: ResponseCurve = ResponseCurve::Power(0.5);

/// The components a skin draws with, as [`ThemeSkin`] writes them.
type Drawn<'w, 's> = QueryItem<'w, 's, <ThemeSkin as ThemeBinding>::Target>;

/// One layer on its way from what the node showed to what its new skin
/// shows, with the clock that times it.
#[derive(Clone, Debug)]
struct Easing {
    layer: Layer,
    clock: Tween<f32>,
}

/// A layer's old and new looks.
#[derive(Clone, Debug, PartialEq)]
enum Layer {
    Fill(Color, Color),
    Tint(Color, Color),
    Gradients(Vec<Gradient>, Vec<Gradient>),
    Border(Vec<Gradient>, Vec<Gradient>),
    Shadows(Vec<ShadowStyle>, Vec<ShadowStyle>),
    Outline(Outline, Outline),
}

/// On a node whose skin is changing: the layers still on their way. Gone
/// once they arrive.
#[derive(Component, Clone, Debug)]
#[component(storage = "SparseSet")]
pub(super) struct Transitioning(Vec<Easing>);

/// Applies a node's new skin, easing into it the layers the skin gives a
/// time. Replaces any transition already under way, starting from wherever
/// it had got to.
pub(super) fn transition_on_insert(
    insert: On<Insert<ThemeSkin>>,
    theme: Res<Theme>,
    mut skins: Query<(&ThemeSkin, <ThemeSkin as ThemeBinding>::Target)>,
    mut commands: Commands,
) {
    let entity = insert.entity;
    let Ok((binding, drawn)) = skins.get_mut(entity) else {
        return;
    };
    let binding = binding.clone();
    let before = Shown::of(&drawn);
    binding.apply(&theme, drawn);
    let Ok((_, mut drawn)) = skins.get_mut(entity) else {
        return;
    };
    let times = theme
        .skin(&binding.0)
        .map(|painted| painted.skin.transition)
        .unwrap_or_default();
    let easings = before.easings_to(&Shown::of(&drawn), times);
    if easings.is_empty() {
        commands.entity(entity).try_remove::<Transitioning>();
        return;
    }
    // Start from the old look, so nothing flashes before the first step.
    for easing in &easings {
        easing.layer.show(0.0, &mut drawn);
    }
    commands.entity(entity).try_insert(Transitioning(easings));
}

/// Moves each changing node's layers along, in real time so a paused game's
/// menus still ease, and lets go of each node once it arrives. A new theme
/// has just been applied whole, so what was on its way is dropped.
pub(super) fn ease_transitions(
    theme: Res<Theme>,
    time: Res<Time<Real>>,
    mut nodes: Query<(
        Entity,
        &mut Transitioning,
        <ThemeSkin as ThemeBinding>::Target,
    )>,
    mut commands: Commands,
) {
    if theme.is_changed() {
        for (entity, ..) in &nodes {
            commands.entity(entity).try_remove::<Transitioning>();
        }
        return;
    }
    let delta = time.delta_secs();
    for (entity, mut transitioning, mut drawn) in &mut nodes {
        transitioning.0.retain_mut(|easing| {
            let (progress, arrived) = easing.clock.advance(delta);
            easing.layer.show(progress, &mut drawn);
            !arrived
        });
        if transitioning.0.is_empty() {
            commands.entity(entity).try_remove::<Transitioning>();
        }
    }
}

/// What a skinned node shows, layer by layer.
struct Shown {
    fill: Color,
    image: AssetId<Image>,
    tint: Color,
    gradients: Vec<Gradient>,
    border: Vec<Gradient>,
    shadows: Vec<ShadowStyle>,
    outline: Outline,
}

impl Shown {
    fn of((fill, image, gradients, border, shadows, outline): &Drawn) -> Self {
        Self {
            fill: fill.0,
            image: image.image.id(),
            tint: image.color,
            gradients: gradients.0.clone(),
            border: border.0.clone(),
            shadows: shadows.0.clone(),
            outline: **outline,
        }
    }

    /// The layers that change on the way to `to` and have a time to do it in.
    fn easings_to(self, to: &Shown, times: SkinTransition) -> Vec<Easing> {
        // A new picture can't blend into the old one, so its tint just changes.
        let same_picture = self.image == to.image;
        let layers = [
            (times.fill, Layer::Fill(self.fill, to.fill)),
            (
                if same_picture { times.tint } else { 0.0 },
                Layer::Tint(self.tint, to.tint),
            ),
            (
                times.gradients,
                Layer::Gradients(self.gradients, to.gradients.clone()),
            ),
            (times.border, Layer::Border(self.border, to.border.clone())),
            (
                times.shadows,
                Layer::Shadows(self.shadows, to.shadows.clone()),
            ),
            (times.outline, Layer::Outline(self.outline, to.outline)),
        ];
        layers
            .into_iter()
            .filter(|(seconds, layer)| *seconds > 0.0 && layer.can_ease())
            .map(|(seconds, layer)| Easing {
                layer,
                clock: Tween::new(0.0, 1.0, seconds, EASING),
            })
            .collect()
    }
}

impl Layer {
    /// Whether the old and new looks differ, in a way that can be eased.
    fn can_ease(&self) -> bool {
        match self {
            Self::Fill(from, to) | Self::Tint(from, to) => from != to,
            Self::Gradients(from, to) | Self::Border(from, to) => {
                from != to && mix_list(from, to, 0.0, vanished_gradient, mix_gradient).is_some()
            }
            Self::Shadows(from, to) => from != to,
            Self::Outline(from, to) => from != to,
        }
    }

    /// Shows the layer `progress` of the way, from 0 to 1, from its old look to its new.
    fn show(&self, progress: f32, (fill, image, gradients, border, shadows, outline): &mut Drawn) {
        let arrived = progress >= 1.0;
        match self {
            Self::Fill(from, to) => fill.0 = mix_color(*from, *to, progress),
            Self::Tint(from, to) => image.color = mix_color(*from, *to, progress),
            Self::Gradients(from, to) => {
                gradients.0 =
                    eased_list(from, to, progress, arrived, vanished_gradient, mix_gradient);
            }
            Self::Border(from, to) => {
                border.0 = eased_list(from, to, progress, arrived, vanished_gradient, mix_gradient);
            }
            Self::Shadows(from, to) => {
                let mix = |from: &_, to: &_, at| Some(mix_shadow(from, to, at));
                shadows.0 = eased_list(from, to, progress, arrived, vanished_shadow, mix);
            }
            Self::Outline(from, to) => **outline = mix_outline(from, to, progress),
        }
    }
}

/// A list `progress` of the way between two, or exactly the new one on arrival.
fn eased_list<T: Clone>(
    from: &[T],
    to: &[T],
    progress: f32,
    arrived: bool,
    vanish: impl Fn(&T) -> T,
    mix: impl Fn(&T, &T, f32) -> Option<T>,
) -> Vec<T> {
    if arrived {
        return to.to_vec();
    }
    mix_list(from, to, progress, vanish, mix).unwrap_or_else(|| to.to_vec())
}

/// Two lists mixed item by item. Where one is longer, its extra items fade
/// from or to nothing, so a glow that appears fades in. `None` if any pair
/// can't be mixed.
fn mix_list<T: Clone>(
    from: &[T],
    to: &[T],
    progress: f32,
    vanish: impl Fn(&T) -> T,
    mix: impl Fn(&T, &T, f32) -> Option<T>,
) -> Option<Vec<T>> {
    (0..from.len().max(to.len()))
        .map(|index| {
            let (old, new) = match (from.get(index), to.get(index)) {
                (Some(old), Some(new)) => (old.clone(), new.clone()),
                (Some(old), None) => (old.clone(), vanish(old)),
                (None, Some(new)) => (vanish(new), new.clone()),
                (None, None) => unreachable!("within the longer list"),
            };
            mix(&old, &new, progress)
        })
        .collect()
}

/// Two colours mixed in Oklab, which keeps the halfway colour as bright as
/// its ends. A colour fading in or out keeps the other's hue, rather than
/// passing through black.
fn mix_color(from: Color, to: Color, progress: f32) -> Color {
    let (from, to) = match (from.alpha() == 0.0, to.alpha() == 0.0) {
        (true, false) => (to.with_alpha(0.0), to),
        (false, true) => (from, from.with_alpha(0.0)),
        _ => (from, to),
    };
    Oklaba::from(from).mix(&Oklaba::from(to), progress).into()
}

/// A length `progress` of the way between two. Only pixels mix; other units
/// take the new length.
fn mix_val(from: Val, to: Val, progress: f32) -> Val {
    match (from, to) {
        (Val::Px(from), Val::Px(to)) => Val::Px(from + (to - from) * progress),
        _ => to,
    }
}

fn mix_shadow(from: &ShadowStyle, to: &ShadowStyle, progress: f32) -> ShadowStyle {
    ShadowStyle {
        color: mix_color(from.color, to.color, progress),
        x_offset: mix_val(from.x_offset, to.x_offset, progress),
        y_offset: mix_val(from.y_offset, to.y_offset, progress),
        spread_radius: mix_val(from.spread_radius, to.spread_radius, progress),
        blur_radius: mix_val(from.blur_radius, to.blur_radius, progress),
    }
}

/// A shadow faded to nothing, the same size, to fade in from or out to.
fn vanished_shadow(shadow: &ShadowStyle) -> ShadowStyle {
    ShadowStyle {
        color: shadow.color.with_alpha(0.0),
        ..*shadow
    }
}

fn mix_outline(from: &Outline, to: &Outline, progress: f32) -> Outline {
    Outline {
        width: mix_val(from.width, to.width, progress),
        offset: mix_val(from.offset, to.offset, progress),
        color: mix_color(from.color, to.color, progress),
    }
}

/// Two gradients of the same kind and number of stops, their colours and
/// angle mixed; the rest is the new one's. `None` for gradients of
/// different shapes.
fn mix_gradient(from: &Gradient, to: &Gradient, progress: f32) -> Option<Gradient> {
    let stops = |from: &[ColorStop], to: &[ColorStop]| -> Option<Vec<ColorStop>> {
        let mixed = |(old, new): (&ColorStop, &ColorStop)| ColorStop {
            color: mix_color(old.color, new.color, progress),
            ..*new
        };
        (from.len() == to.len()).then(|| from.iter().zip(to).map(mixed).collect())
    };
    match (from, to) {
        (Gradient::Linear(old), Gradient::Linear(new)) => {
            let mut mixed = new.clone();
            mixed.angle = old.angle + (new.angle - old.angle) * progress;
            mixed.stops = stops(&old.stops, &new.stops)?;
            Some(mixed.into())
        }
        (Gradient::Radial(old), Gradient::Radial(new)) => {
            let mut mixed = new.clone();
            mixed.stops = stops(&old.stops, &new.stops)?;
            Some(mixed.into())
        }
        _ => None,
    }
}

/// A gradient faded to nothing, the same shape, to fade in from or out to.
fn vanished_gradient(gradient: &Gradient) -> Gradient {
    let mut gradient = gradient.clone();
    let stops: &mut Vec<ColorStop> = match &mut gradient {
        Gradient::Linear(linear) => &mut linear.stops,
        Gradient::Radial(radial) => &mut radial.stops,
        // Conic gradients take angular stops; skins draw none.
        Gradient::Conic(_) => return gradient,
    };
    for stop in stops {
        stop.color = stop.color.with_alpha(0.0);
    }
    gradient
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{Skin, SkinColor, SkinShadow, ThemePlugin, ThemeToken, WidgetState};
    use bevy::time::TimeUpdateStrategy;
    use std::time::Duration;

    const BG: ThemeToken = ThemeToken::new_static("test.bg");
    const RED: Color = Color::srgb(1.0, 0.0, 0.0);
    const FRAME: Duration = Duration::from_millis(100);

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, ThemePlugin))
            .insert_resource(TimeUpdateStrategy::ManualDuration(FRAME));
        app.world_mut().resource_mut::<Theme>().set_color(BG, RED);
        app.update();
        app
    }

    fn glow(app: &App, entity: Entity) -> Option<ShadowStyle> {
        app.world()
            .get::<BoxShadow>(entity)
            .unwrap()
            .0
            .first()
            .copied()
    }

    /// A glow on the focused state's skin fades in over its time, while the
    /// fill, given none, changes at once.
    #[test]
    fn a_layer_with_a_time_eases_in_and_the_rest_snap() {
        let mut app = app();
        let focused = BG.with_state(WidgetState::Focused);
        let skin = Skin::default()
            .shadow(SkinShadow::new(SkinColor::OWN).blur(8.0))
            .transition(SkinTransition {
                shadows: 0.4,
                ..default()
            });
        app.world_mut()
            .resource_mut::<Theme>()
            .set_skin(focused.clone(), skin);
        let entity = app.world_mut().spawn(ThemeSkin(BG)).id();
        app.update();

        app.world_mut()
            .entity_mut(entity)
            .insert(ThemeSkin(focused.clone()));
        let lit = app.world().resource::<Theme>().color(&focused);
        let fill = |app: &App| app.world().get::<BackgroundColor>(entity).unwrap().0;
        assert_eq!(fill(&app), lit, "no time for the fill: at once");
        let start = glow(&app, entity).unwrap();
        assert_eq!(start.color.alpha(), 0.0, "the glow starts unseen");
        assert_eq!(start.blur_radius, px(8.0), "at its full size");

        app.update();
        app.update();
        let halfway = glow(&app, entity).unwrap().color.alpha();
        assert!(0.0 < halfway && halfway < 1.0, "fading in: {halfway}");
        assert!(app.world().entity(entity).contains::<Transitioning>());

        for _ in 0..4 {
            app.update();
        }
        assert_eq!(glow(&app, entity).unwrap().color, lit);
        assert!(
            !app.world().entity(entity).contains::<Transitioning>(),
            "let go on arrival"
        );
    }

    /// Leaving for a state whose skin has no time snaps back at once, and
    /// drops the transition under way.
    #[test]
    fn a_skin_without_a_time_snaps_and_stops_any_transition() {
        let mut app = app();
        let hover = BG.with_state(WidgetState::Hover);
        let skin = Skin::default()
            .shadow(SkinShadow::new(Color::WHITE).blur(8.0))
            .transition(1.0);
        app.world_mut()
            .resource_mut::<Theme>()
            .set_skin(hover.clone(), skin);
        let entity = app.world_mut().spawn(ThemeSkin(BG)).id();
        app.update();
        app.world_mut().entity_mut(entity).insert(ThemeSkin(hover));
        app.update();
        assert!(app.world().entity(entity).contains::<Transitioning>());

        app.world_mut().entity_mut(entity).insert(ThemeSkin(BG));
        assert_eq!(glow(&app, entity), None);
        assert!(!app.world().entity(entity).contains::<Transitioning>());
    }

    /// A new theme is applied whole, so a transition under way is dropped
    /// and the node shows the new theme's look.
    #[test]
    fn a_new_theme_snaps() {
        let mut app = app();
        let hover = BG.with_state(WidgetState::Hover);
        app.world_mut()
            .resource_mut::<Theme>()
            .set_skin(BG, Skin::default().transition(1.0));
        let entity = app.world_mut().spawn(ThemeSkin(BG)).id();
        app.update();
        app.world_mut()
            .entity_mut(entity)
            .insert(ThemeSkin(hover.clone()));
        app.update();
        assert!(app.world().entity(entity).contains::<Transitioning>());

        let blue = Color::srgb(0.0, 0.0, 1.0);
        app.world_mut()
            .resource_mut::<Theme>()
            .set_color(hover.clone(), blue);
        app.update();
        assert!(!app.world().entity(entity).contains::<Transitioning>());
        assert_eq!(app.world().get::<BackgroundColor>(entity).unwrap().0, blue);
    }

    #[test]
    fn colours_fade_in_with_their_own_hue() {
        let halfway = mix_color(Color::NONE, RED, 0.5);
        let srgb = Srgba::from(halfway);
        assert!((srgb.alpha - 0.5).abs() < 1e-4);
        assert!(srgb.red > 0.9 && srgb.green < 0.1, "still red: {srgb:?}");
    }

    #[test]
    fn gradients_of_different_shapes_cant_ease() {
        use bevy::ui::{LinearGradient, RadialGradient};
        let linear: Gradient = LinearGradient::to_bottom(vec![ColorStop::auto(RED)]).into();
        let radial: Gradient = RadialGradient::default().into();
        assert!(mix_gradient(&linear, &radial, 0.5).is_none());
        assert!(!Layer::Gradients(vec![linear.clone()], vec![radial]).can_ease());
        assert!(
            Layer::Gradients(vec![], vec![linear]).can_ease(),
            "fades in"
        );
    }
}
