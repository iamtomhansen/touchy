//! Floating controls typical for mobile devices: the control waits, hidden,
//! in an area of the screen, and appears wherever that area is pressed.
//!
//! A floating control's own entity is the area: it fills its parent and takes
//! every press in it, so the control's input works as it does anywhere. Its
//! body, the part that's drawn, is a child placed where the press lands.

use bevy::prelude::*;
use bevy::ui::{ComputedUiRenderTargetInfo, UiGlobalTransform};

/// A control that floats in its area, hidden until pressed.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct Floating;

/// Where a node is laid out, as a pointer reads it.
pub(crate) type Placement = (
    &'static ComputedNode,
    &'static ComputedUiRenderTargetInfo,
    &'static UiGlobalTransform,
);

/// A floating control's area: laid over its parent.
pub(crate) fn area() -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: px(0),
        top: px(0),
        width: percent(100),
        height: percent(100),
        ..default()
    }
}

/// A floating control's body, from the `node` it would have in the layout:
/// laid over the area, hidden, and letting presses through to the area.
pub(crate) fn body(node: Node) -> impl Bundle {
    (
        Pickable::IGNORE,
        Node {
            position_type: PositionType::Absolute,
            ..node
        },
        Visibility::Hidden,
    )
}

/// The app's UI scale, which turns window pixels into UI pixels.
pub(crate) fn scale_of(ui_scale: Option<Res<UiScale>>) -> f32 {
    ui_scale.map_or(1.0, |scale| scale.0).max(0.1)
}

/// Where a pointer at `position`, in window pixels, is in `area`: in UI
/// pixels from the area's top-left.
pub(crate) fn point_in(
    position: Vec2,
    (node, target, transform): (
        &ComputedNode,
        &ComputedUiRenderTargetInfo,
        &UiGlobalTransform,
    ),
    ui_scale: f32,
) -> Option<Vec2> {
    // The target's scale factor includes the UI scale; pointers know only the window's.
    let physical = position * target.scale_factor() / ui_scale;
    let from_centre = transform.try_inverse()?.transform_point2(physical);
    Some((from_centre + node.size() / 2.0) * node.inverse_scale_factor())
}

/// Shows a body with its centre at `centre`, in UI pixels from its area's top-left.
pub(crate) fn show_at(
    centre: Vec2,
    (mut node, computed, mut visibility): (Mut<Node>, &ComputedNode, Mut<Visibility>),
) {
    let corner = centre - computed.size() * computed.inverse_scale_factor() / 2.0;
    let (left, top) = (px(corner.x), px(corner.y));
    if (node.left, node.top) != (left, top) {
        node.left = left;
        node.top = top;
    }
    visibility.set_if_neq(Visibility::Inherited);
}

/// Hides a body until its area is pressed again.
pub(crate) fn hide(mut visibility: Mut<Visibility>) {
    visibility.set_if_neq(Visibility::Hidden);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_point_is_read_from_the_areas_top_left_in_ui_pixels() {
        // A 200 by 100 physical-pixel area centred at (300, 150), at half a
        // UI pixel per physical pixel: its corner is at (200, 100).
        let node = ComputedNode {
            size: Vec2::new(200.0, 100.0),
            inverse_scale_factor: 0.5,
            ..default()
        };
        let target = ComputedUiRenderTargetInfo::default();
        let transform = UiGlobalTransform::from_translation(Vec2::new(300.0, 150.0));
        // The default target's scale factor is 1, so pointer pixels are physical.
        let at = point_in(Vec2::new(250.0, 125.0), (&node, &target, &transform), 1.0);
        // 50 by 25 physical pixels in from the corner: 25 by 12.5 UI pixels.
        assert_eq!(at, Some(Vec2::new(25.0, 12.5)));
    }
}
