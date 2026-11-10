//! Cancel request detection and handling system. Allows one in-progress action at a time
//! to be cancelled by Escape or Right-click

use bevy::input::InputSystems;
use bevy::prelude::*;

/// The input source that initiated a cancel action.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CancelSource {
    /// The Escape key was pressed.
    Escape,
    /// The right mouse button was pressed.
    RightClick,
}

/// This frame's cancel input, consumed by the first system to handle it.
/// Tracking 'source' allows selective handling of different cancel inputs.
/// When source is `None`, either no request was made or it has already been consumed.
#[derive(Resource, Default, Debug)]
pub struct CancelRequest {
    /// The input source that initiated the cancel action, or `None` if no cancel request.
    pub source: Option<CancelSource>,
}

impl CancelRequest {
    /// Look at but don't consume the cancel request. Returns the source of the cancel input.
    pub fn peek(&self) -> Option<CancelSource> {
        self.source
    }

    /// Consume the cancel request. Returns the source of the cancel input.
    pub fn consume(&mut self) -> Option<CancelSource> {
        self.source.take()
    }
}

/// Ordered phases through which a `CancelRequest` passes. Earlier phases get priority.
#[derive(SystemSet, Clone, PartialEq, Eq, Hash, Debug)]
pub enum CancelPhase {
    /// Open pickers that hold the pointer, like a radial menu, which float
    /// above everything on their layer.
    Picker,
    /// Open popovers, which float above the overlays and menus they belong to.
    Popover,
    /// Open overlays and menus.
    Menu,
    /// In-progress actions that can be cancelled.
    Mode,
    /// Fallback handler when no higher-priority phase consumed the request.
    Fallback,
}

/// An app without a keyboard or a mouse (a headless one) simply never cancels with it.
fn detect_cancel(
    keyboard: Option<Res<ButtonInput<KeyCode>>>,
    mouse: Option<Res<ButtonInput<MouseButton>>>,
    mut cancel: ResMut<CancelRequest>,
) {
    cancel.source = if keyboard.is_some_and(|keys| keys.just_pressed(KeyCode::Escape)) {
        Some(CancelSource::Escape)
    } else if mouse.is_some_and(|buttons| buttons.just_pressed(MouseButton::Right)) {
        Some(CancelSource::RightClick)
    } else {
        None
    };
}

/// Plugin managing cancel request detection and the ordered `CancelPhase` system sets.
pub struct CancelPlugin;

impl Plugin for CancelPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CancelRequest>()
            .configure_sets(
                PreUpdate,
                (
                    CancelPhase::Picker,
                    CancelPhase::Popover,
                    CancelPhase::Menu,
                    CancelPhase::Mode,
                    CancelPhase::Fallback,
                )
                    .chain()
                    .after(InputSystems),
            )
            .add_systems(
                PreUpdate,
                detect_cancel
                    .after(InputSystems)
                    .before(CancelPhase::Picker),
            );
    }
}
