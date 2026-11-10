//! Interactive controls. Each module has a config builder, a token set for
//! theming and a plugin; [`SpawnUi`](crate::spawn_ui::SpawnUi) spawns them.

pub mod button;
pub mod capsule_flip;
pub mod checkbox;
pub(crate) mod chevron;
pub mod combo;
pub mod dpad;
pub mod dropdown;
pub(crate) mod floating;
pub(crate) mod frame;
pub mod gearshift;
pub mod grabber;
pub mod grid;
pub mod joystick;
pub mod menu;
pub mod number_input;
pub mod radial;
pub mod scrubber;
pub mod segmented;
pub mod slider;
pub mod text_input;
pub mod transport;
pub(crate) mod wedge;

use bevy::app::PluginGroupBuilder;
use bevy::prelude::*;

/// Every touchy widget made of widgets alone. Each one is inert until it's
/// spawned, so adding the whole group costs nothing for
/// widgets a game never uses.
///
/// Widgets built from containers, such as the dropdown and the combo box, whose
/// lists are scroll containers, aren't in it: they follow the containers in
/// [`TouchyPlugins`](crate::TouchyPlugins), since a dependency has to come first.
pub struct WidgetPlugins;

impl PluginGroup for WidgetPlugins {
    fn build(self) -> PluginGroupBuilder {
        // Dependencies come before their dependents (slider before gearshift),
        // otherwise alphabetical. Bevy panics if a dependent adds one first.
        PluginGroupBuilder::start::<Self>()
            .add(slider::SliderWidgetPlugin)
            .add(button::ButtonWidgetPlugin)
            .add(capsule_flip::CapsuleFlipWidgetPlugin)
            .add(checkbox::CheckboxWidgetPlugin)
            .add(dpad::DpadWidgetPlugin)
            .add(gearshift::GearshiftWidgetPlugin)
            .add(grabber::GrabberWidgetPlugin)
            .add(grid::GridWidgetPlugin)
            .add(joystick::JoystickWidgetPlugin)
            .add(radial::RadialWidgetPlugin)
            .add(scrubber::ScrubberWidgetPlugin)
            .add(segmented::SegmentedWidgetPlugin)
            .add(text_input::TextInputWidgetPlugin)
            .add(number_input::NumberInputWidgetPlugin)
            // After the button and slider it's built from.
            .add(transport::TransportWidgetPlugin)
    }
}
