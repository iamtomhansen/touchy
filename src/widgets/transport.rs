//! Transport controls (play, pause, seek, step, loop) for anything with a timeline.
//!
//! The host app owns playback. A timeline is an entity the host spawns with a
//! [`Timeline`] and its [`TimelineLabels`]; a transport bar is pointed at it by
//! [`TransportOf`], and only displays it and asks for changes with
//! [`TransportRequest`]s. The host applies those to whatever it is playing,
//! then writes the result back into the timeline.
//!
//! Any number of bars may show one timeline, and a bar can be pointed at
//! another timeline at any time by inserting a new [`TransportOf`].

use bevy::picking::cursor::CursorIconPlugin;
use bevy::prelude::*;

mod interact;
mod view;

use crate::EnsurePlugin;
use crate::theme::{ThemePlugin, ThemeToken};
use crate::widgets::{button::ButtonWidgetPlugin, slider::SliderWidgetPlugin};

pub use interact::TransportKeys;
pub(crate) use view::spawn_transport;
pub use view::{TRANSPORT_STYLE, TimelineTrack, TransportButton, TransportConfig, TransportTokens};

/// A timeline's playback state, on the host's timeline entity. The host writes
/// it; transport bars only read it. Times are in arbitrary units (e.g. seconds,
/// ticks, samples).
#[derive(Component, Debug, Clone, PartialEq)]
pub struct Timeline {
    /// Length of the timeline.
    pub duration: f32,
    /// Current playhead position.
    pub position: f32,
    /// Furthest position reached so far, for timelines still being recorded.
    pub live_edge: f32,
    /// Whether playback is currently active.
    pub playing: bool,
    /// Playback rate; 1.0 is normal speed.
    pub speed: f32,
    /// Whether playback wraps to the start on reaching the end.
    pub looping: bool,
    /// A list of points of interest on the timeline.
    pub markers: Vec<Marker>,
}

impl Default for Timeline {
    fn default() -> Self {
        Self {
            duration: 0.0,
            position: 0.0,
            live_edge: 0.0,
            playing: false,
            speed: 1.0,
            looping: false,
            markers: Vec::new(),
        }
    }
}

/// Text shown beside a timeline, on the host's timeline entity. The host writes
/// it; transport bars only display it.
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct TimelineLabels {
    /// Main readout, e.g. "TICK 1/30", "2ND 12:34", "00:01.23/00:03.45"
    pub clock: String,
    /// Secondary status line.
    pub status: String,
}

/// The timeline a transport bar shows and controls. Insert a new one to point
/// the bar at another timeline.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
#[relationship(relationship_target = Transports)]
pub struct TransportOf(pub Entity);

/// The transport bars showing a timeline, kept by Bevy from their [`TransportOf`].
#[derive(Component, Debug, Default, PartialEq, Eq)]
#[relationship_target(relationship = TransportOf)]
pub struct Transports(Vec<Entity>);

/// A point of interest on the timeline, drawn as a tick on the track.
#[derive(Debug, Clone, PartialEq)]
pub struct Marker {
    /// Time of the marker, in the same units as [`Timeline`].
    pub time: f32,
    /// How the marker looks, as a theme token the host defines; `None` takes
    /// the bar's [`TransportTokens::marker`].
    pub style: Option<ThemeToken>,
    /// Shown on hover.
    pub label: String,
}

impl Marker {
    /// Creates a new marker in the bar's marker style.
    pub fn new(time: f32, label: impl Into<String>) -> Self {
        Self {
            time,
            style: None,
            label: label.into(),
        }
    }
    /// Sets the marker's style.
    pub fn style(mut self, style: ThemeToken) -> Self {
        self.style = Some(style);
        self
    }
}

/// What transport widgets and shortcuts ask the host to do.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TransportCommand {
    /// Play/pause the timeline.
    TogglePlay,
    /// Play (but don't pause if already playing).
    Play,
    /// Pause (but don't play if already paused).
    Pause,
    /// Step forward or backward by a number of ticks.
    Step(i32),
    /// Go to a specific position on the timeline.
    Seek(f32),
    /// Go to the start of the timeline.
    Restart,
    /// Toggle looping on/off.
    ToggleLoop,
    /// Jump to the start of the timeline.
    JumpToStart,
    /// Jump to the live edge of the timeline.
    JumpToLive,
    /// Set the playback speed. 1.0 is normal speed.
    SetSpeed(f32),
}

/// A transport bar or shortcut asking the host to apply `command` to `timeline`.
/// Triggered on the timeline, so the host can observe it there or globally.
#[derive(EntityEvent, Clone, Copy, Debug, PartialEq)]
pub struct TransportRequest {
    /// The timeline to apply it to.
    #[event_target]
    pub timeline: Entity,
    /// What to do.
    pub command: TransportCommand,
}

/// Frame order for transport, in `Update`. Requests arrive as they happen, before it.
#[derive(SystemSet, Clone, PartialEq, Eq, Hash, Debug)]
pub enum TransportSystems {
    /// The host advances playback and updates each [`Timeline`] and its [`TimelineLabels`].
    Apply,
    /// Bars redraw from their timelines.
    View,
}

/// Registers transport bars and their systems. Playback itself is the host's.
pub struct TransportWidgetPlugin;

impl Plugin for TransportWidgetPlugin {
    fn build(&self, app: &mut App) {
        app.ensure_plugin(ThemePlugin)
            .ensure_plugin(ButtonWidgetPlugin)
            .ensure_plugin(CursorIconPlugin)
            .ensure_plugin(SliderWidgetPlugin)
            .configure_sets(
                Update,
                (TransportSystems::Apply, TransportSystems::View).chain(),
            );
        interact::build(app);
        view::build(app);
    }
}
