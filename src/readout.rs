//! Readouts: read-only views of the app's values, such as meters and plots,
//! and later tables. They show what the app gives them, and the user
//! never edits them directly. Each module has a config builder, a token set
//! for theming and a plugin; [`SpawnUi`](crate::spawn_ui::SpawnUi) spawns them.

pub mod meter;
pub mod plot;

use bevy::app::PluginGroupBuilder;
use bevy::prelude::*;

/// Every touchy readout. Each one is inert until it's spawned.
pub struct ReadoutPlugins;

impl PluginGroup for ReadoutPlugins {
    fn build(self) -> PluginGroupBuilder {
        PluginGroupBuilder::start::<Self>()
            .add(meter::MeterPlugin)
            .add(plot::PlotPlugin)
    }
}
