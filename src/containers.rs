//! Containers: Themed nodes using Bevy's UI system (Taffy).

pub mod panel;
pub mod scroll_area;
pub mod section;
pub mod toolbar;

use bevy::app::PluginGroupBuilder;
use bevy::prelude::*;

/// Every touchy container. Each one is inert until it's spawned.
pub struct ContainerPlugins;

impl PluginGroup for ContainerPlugins {
    fn build(self) -> PluginGroupBuilder {
        PluginGroupBuilder::start::<Self>()
            .add(panel::PanelPlugin)
            .add(scroll_area::ScrollAreaPlugin)
            .add(section::SectionPlugin)
            .add(toolbar::ToolbarPlugin)
    }
}
