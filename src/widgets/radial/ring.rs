//! Drawing a radial: showing its ring above everything as it opens, keeping it
//! on screen, and lighting its wedges, middle and mark as it's aimed.

use std::f32::consts::FRAC_PI_4;

use bevy::input_focus::{FocusCause, InputFocus};
use bevy::prelude::*;
use bevy::ui::{BackgroundGradient, Pressed};
use bevy::ui_widgets::Button;

use super::{
    HIGHLIGHT_SCALE, MARK_GAP, MARK_SIZE, OPEN_CURVE, OPEN_FROM_SCALE, OPEN_SECONDS, Phase, Radial,
    RadialCenter, RadialParts, RadialTokens, on_ring, words,
};
use crate::choice::{ChoiceSet, ChoiceState, Chosen};
use crate::keyed::SyncChildren;
use crate::layers::PICKER_LIFT;
use crate::motion::Tween;
use crate::popover::{Stacked, layer_under};
use crate::theme::{Theme, ThemeTextColor};
use crate::widgets::wedge::{bearing_of, wedges};

/// Where a ring of `radius` sits to be centred on `wanted` (or the middle of a
/// `viewport` that size), kept wholly on screen where it fits. Before the
/// viewport is laid out, it's taken at its word.
fn placed(wanted: Option<Vec2>, viewport: Vec2, radius: f32) -> Vec2 {
    let wanted = wanted.unwrap_or(viewport / 2.0);
    let axis = |wanted: f32, size: f32| {
        if size <= 0.0 {
            wanted
        } else if size >= 2.0 * radius {
            wanted.clamp(radius, size - radius)
        } else {
            size / 2.0
        }
    };
    Vec2::new(axis(wanted.x, viewport.x), axis(wanted.y, viewport.y))
}

/// A radial, and whether it's a button and pressed, as it opens and closes.
type Opening = (
    Entity,
    &'static mut Radial,
    &'static RadialParts,
    Has<Button>,
    Has<Pressed>,
);

/// Shows and hides each ring as its radial opens and closes: floats it above
/// its layer, starts it growing, and lends a buttonless radial the keyboard's
/// focus while it's open. Lets go of a radial button pressed to hold it open,
/// since the ring took the release.
pub(super) fn show_opening(
    mut radials: Query<Opening>,
    mut rings: Query<&mut UiTransform>,
    parents: Query<&ChildOf>,
    stacked: Query<Stacked>,
    mut focus: Option<ResMut<InputFocus>>,
    mut commands: Commands,
) {
    for (root, mut radial, parts, is_button, pressed) in &mut radials {
        if pressed && !matches!(radial.phase, Phase::Held { .. }) {
            commands.entity(root).remove::<Pressed>();
        }
        let open = radial.is_open();
        if open == radial.shown {
            continue;
        }
        radial.shown = open;
        let visibility = if open {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        commands.entity(parts.scrim).insert(visibility);
        if open {
            let (layer, _) = layer_under(root, &parents, &stacked);
            commands
                .entity(parts.scrim)
                .insert(GlobalZIndex(layer + PICKER_LIFT));
            radial.opening = Some(Tween::new(OPEN_FROM_SCALE, 1.0, OPEN_SECONDS, OPEN_CURVE));
            if let Ok(mut ring) = rings.get_mut(parts.ring) {
                ring.scale = Vec2::splat(OPEN_FROM_SCALE);
            }
        }
        let Some(focus) = focus.as_deref_mut() else {
            continue;
        };
        if is_button {
            continue;
        }
        if open {
            radial.focus_before = focus.get();
            focus.set(root, FocusCause::Navigated);
        } else if focus.get() == Some(root) {
            match radial.focus_before.take() {
                Some(before) => focus.set(before, FocusCause::Navigated),
                None => focus.clear(),
            }
        }
    }
}

/// Keeps each open ring where it was opened, or as near as stays on screen.
pub(super) fn place_rings(
    mut radials: Query<(&mut Radial, &RadialParts)>,
    viewports: Query<&ComputedNode>,
    mut nodes: Query<&mut Node>,
) {
    for (mut radial, parts) in &mut radials {
        if !radial.is_open() {
            continue;
        }
        let viewport = viewports
            .get(parts.scrim)
            .map_or(Vec2::ZERO, |node| node.size() * node.inverse_scale_factor);
        let middle = placed(radial.wanted, viewport, radial.radius);
        if radial.middle != middle {
            radial.middle = middle;
        }
        if let Ok(mut ring) = nodes.get_mut(parts.ring) {
            let (left, top) = (px(middle.x - radial.radius), px(middle.y - radial.radius));
            if ring.left != left || ring.top != top {
                ring.left = left;
                ring.top = top;
            }
        }
    }
}

/// Grows each opening ring to its size.
pub(super) fn grow_rings(
    time: Res<Time<Real>>,
    mut radials: Query<(&mut Radial, &RadialParts)>,
    mut rings: Query<&mut UiTransform>,
) {
    // Only opening ones are touched, so the rest aren't marked changed.
    for (mut radial, parts) in radials
        .iter_mut()
        .filter(|(radial, _)| radial.opening.is_some())
    {
        let Some(mut tween) = radial.opening else {
            continue;
        };
        let (scale, arrived) = tween.advance(time.delta_secs());
        radial.opening = (!arrived).then_some(tween);
        if let Ok(mut ring) = rings.get_mut(parts.ring) {
            ring.scale = Vec2::splat(scale);
        }
    }
}

/// What an open ring's look is drawn from.
type RingLook = (
    &'static Radial,
    &'static RadialParts,
    &'static ChoiceSet,
    &'static Chosen,
    &'static RadialTokens,
);

/// Lights each open ring's wedges: the chosen option's, and the highlighted
/// one's over it, in the theme's colours as they are now.
pub(super) fn light_wedges(
    radials: Query<RingLook>,
    theme: Res<Theme>,
    mut gradients: Query<&mut BackgroundGradient>,
    mut commands: Commands,
) {
    for (radial, parts, choices, chosen, tokens) in &radials {
        if !radial.is_open() {
            continue;
        }
        let count = choices.len();
        let lit = [
            (chosen.0, &tokens.wedge_chosen),
            (choices.highlighted(), &tokens.wedge_highlighted),
        ]
        .into_iter()
        .filter_map(|(index, token)| {
            index.map(|index| (bearing_of(index, count), theme.color(token)))
        });
        let wanted = wedges(lit, count);
        match gradients.get_mut(parts.wedges) {
            Ok(mut had) => {
                had.set_if_neq(wanted);
            }
            Err(_) => {
                commands.entity(parts.wedges).insert(wanted);
            }
        }
    }
}

/// Restyles each open ring: grows the highlighted option's disc, names it in
/// the middle, and points the mark its way.
pub(super) fn restyle_rings(
    radials: Query<RingLook>,
    states: Query<&ChoiceState>,
    mut transforms: Query<&mut UiTransform>,
    mut texts: Query<(&mut Text, &ThemeTextColor)>,
    mut nodes: Query<(&mut Node, &mut Visibility)>,
    mut commands: Commands,
) {
    for (radial, parts, choices, chosen, tokens) in &radials {
        if !radial.is_open() {
            continue;
        }
        for item in &parts.items {
            let highlighted = states.get(item.item).is_ok_and(|state| state.highlighted);
            let scale = Vec2::splat(if highlighted { HIGHLIGHT_SCALE } else { 1.0 });
            if let Ok(mut transform) = transforms.get_mut(item.item)
                && transform.scale != scale
            {
                transform.scale = scale;
            }
        }

        let named = choices.highlighted().or(match radial.center_means {
            RadialCenter::Keep => chosen.0,
            RadialCenter::Clear => None,
        });
        let (line, colour) = match named.and_then(|index| choices.get(index)) {
            Some(choice) => (choice.label.as_str(), &tokens.text),
            None => (radial.placeholder.as_str(), &tokens.placeholder),
        };
        if let Ok((mut text, had)) = texts.get_mut(parts.hub_text) {
            if text.0 != line {
                text.0 = line.to_owned();
            }
            if had.0 != *colour {
                commands
                    .entity(parts.hub_text)
                    .insert(ThemeTextColor(colour.clone()));
            }
        }

        if let Ok((mut node, mut visibility)) = nodes.get_mut(parts.mark) {
            let shown = if radial.aim.is_some() {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
            visibility.set_if_neq(shown);
            if let Some(aim) = radial.aim {
                let at = on_ring(aim, radial.dead_zone + MARK_GAP, radial.radius);
                let (left, top) = (px(at.x - MARK_SIZE / 2.0), px(at.y - MARK_SIZE / 2.0));
                if node.left != left || node.top != top {
                    node.left = left;
                    node.top = top;
                }
                // The square's corner points up and right; turn it the rest
                // of the way.
                let rotation = Rot2::radians(aim - FRAC_PI_4);
                if let Ok(mut transform) = transforms.get_mut(parts.mark)
                    && transform.rotation != rotation
                {
                    transform.rotation = rotation;
                }
            }
        }
    }
}

/// What a radial button's face shows changed: the choice, or its tokens.
type FaceChanged = Or<(Changed<Chosen>, Changed<RadialTokens>)>;

/// Shows the choice on a radial button's face: built afresh only when the
/// choice changes, as a keyed child, or when new tokens restyle it.
pub(super) fn show_chosen(
    radials: Query<
        (
            &Radial,
            &RadialParts,
            &ChoiceSet,
            &Chosen,
            Ref<RadialTokens>,
        ),
        FaceChanged,
    >,
    mut commands: Commands,
) {
    for (radial, parts, choices, chosen, tokens) in &radials {
        let Some(face) = parts.face else {
            continue;
        };
        if tokens.is_changed() && !tokens.is_added() {
            // The same choice in a new look: built again from scratch.
            commands.entity(face).despawn_related::<Children>();
        }
        let chosen = chosen.0.filter(|&index| index < choices.len());
        let (choices, placeholder, tokens) = (
            choices.choices().to_vec(),
            radial.placeholder.clone(),
            RadialTokens::clone(&tokens),
        );
        commands.entity(face).sync_children(
            [chosen],
            |chosen| *chosen,
            move |face, chosen| match chosen {
                Some(index) => {
                    choices[*index].build(face, |label| words(label, &tokens.text, &tokens));
                }
                None => {
                    face.spawn((
                        words(&placeholder, &tokens.placeholder, &tokens),
                        Pickable::IGNORE,
                    ));
                }
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use bevy::picking::pointer::PointerLocation;
    use bevy::ui::Gradient;

    use super::super::tests::{EAST, MIDDLE, app, compass, location, parts, spawn};
    use super::super::{OpenRadial, RadialAt};
    use super::*;

    #[test]
    fn a_ring_stays_on_screen_where_it_fits() {
        let viewport = Vec2::new(800.0, 600.0);
        assert_eq!(
            placed(Some(Vec2::new(10.0, 590.0)), viewport, 100.0),
            Vec2::new(100.0, 500.0)
        );
        assert_eq!(placed(None, viewport, 100.0), Vec2::new(400.0, 300.0));
        assert_eq!(
            placed(Some(Vec2::new(10.0, 10.0)), Vec2::new(150.0, 0.0), 100.0),
            Vec2::new(75.0, 10.0),
            "too narrow to fit is centred; not laid out yet is taken at its word"
        );
    }

    #[test]
    fn the_highlighted_and_chosen_options_wedges_light_up() {
        let mut app = app();
        let radial = spawn(&mut app, compass().chosen(2), false);
        app.world_mut().trigger(OpenRadial {
            radial,
            at: RadialAt::Cursor,
            held: false,
        });
        app.update();
        let world = app.world_mut();
        let mut pointers = world.query::<&mut PointerLocation>();
        for mut pointer in pointers.iter_mut(world) {
            *pointer = PointerLocation::new(location(MIDDLE + EAST));
        }
        app.update();

        let wedges = parts(&app, radial).wedges;
        let gradient = app.world().get::<BackgroundGradient>(wedges).unwrap();
        let tokens = RadialTokens::with_prefix(super::super::RADIAL_STYLE);
        let theme = app.world().resource::<Theme>();
        let lit: Vec<(f32, Color)> = gradient
            .0
            .iter()
            .map(|gradient| {
                let Gradient::Conic(conic) = gradient else {
                    panic!("conic wedges");
                };
                (conic.start, conic.stops[0].color)
            })
            .collect();
        let quarter = std::f32::consts::TAU / 4.0;
        // South (chosen) starts an eighth before half way round; east
        // (highlighted) an eighth before a quarter, drawn over it.
        assert_eq!(lit.len(), 2);
        assert!((lit[0].0 - (2.0 * quarter - quarter / 2.0)).abs() < 1e-5);
        assert!((lit[1].0 - (quarter - quarter / 2.0)).abs() < 1e-5);
        assert_eq!(lit[1].1, theme.color(&tokens.wedge_highlighted));
    }

    #[test]
    fn new_tokens_rebuild_a_buttons_face_in_its_look() {
        let mut app = app();
        let radial = spawn(&mut app, compass().chosen(1), true);
        let face = parts(&app, radial).face.unwrap();
        let tokens = RadialTokens::with_prefix("game.wheel");
        app.world_mut().entity_mut(radial).insert(tokens.clone());
        app.update();
        let world = app.world();
        let label = world.get::<Children>(face).unwrap()[0];
        let label = world
            .get::<Children>(label)
            .map_or(label, |children| children[0]);
        assert_eq!(world.get::<Text>(label).unwrap().0, "East");
        assert_eq!(world.get::<ThemeTextColor>(label).unwrap().0, tokens.text);
    }
}
