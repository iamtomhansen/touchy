//! Game-agnostic UI building blocks for Bevy: themed widgets, containers, readouts, tooltips, toasts,
//! UI layers, cancel routing and pointing into the game world.
//!
//! Add [`TouchyPlugins`], then build UI with the [`SpawnUi`](spawn_ui::SpawnUi) methods, such as
//! `body.button(config)`, and style it by writing colours and sizes into the
//! [`Theme`](theme::Theme) resource, or by loading `.theme.ron` files with
//! [`ThemeSheetPlugin`](theme::ThemeSheetPlugin): a skin for how things are drawn and a palette
//! for their colours, your own or one of the [`premade`] themes touchy ships with.
//!
//! ```no_run
//! use bevy::prelude::*;
//! use touchy::prelude::*;
//!
//! App::new()
//!     .add_plugins((DefaultPlugins, TouchyPlugins))
//!     .run();
//! ```
//!
//! Every plugin adds the touchy and Bevy plugins it depends on, so a game can
//! also take a single plugin, or the group minus the parts it doesn't want:
//!
//! ```no_run
//! # use bevy::prelude::*;
//! # use touchy::prelude::*;
//! use touchy::widgets::joystick::JoystickWidgetPlugin;
//!
//! App::new().add_plugins((
//!     DefaultPlugins,
//!     TouchyPlugins.build().disable::<JoystickWidgetPlugin>(),
//! ));
//! ```
//!
//! One plugin is left for the game to add, because it acts on its own rather
//! than waiting for something to be spawned: Bevy's `TabNavigationPlugin` makes
//! Tab move keyboard focus through the whole app. Touchy's widgets answer Enter,
//! Space and the arrow keys once focused; clicking focuses them already
//! (`DefaultPlugins` includes that), and with this plugin Tab does too. Content
//! on the screen layer is tabbable without more setup.
#![warn(rustdoc::broken_intra_doc_links)]
#![warn(clippy::mod_module_files)]
#![warn(missing_docs)]

use bevy::app::PluginGroupBuilder;
use bevy::prelude::*;

mod adorner;
pub mod cancel;
pub mod canvas;
pub mod choice;
pub mod containers;
pub mod cursor;
pub mod keyed;
pub mod layers;
pub mod motion;
pub mod popover;
pub mod premade;
pub mod primitives;
pub mod readout;
mod sanitize;
pub mod spawn_ui;
pub mod theme;
pub mod toast;
pub mod tooltip;
pub mod widgets;
pub mod world_view;

/// The items most games name when building UI with touchy.
pub mod prelude {
    // Touchy is a fancy wrapper around Bevy's UI widgets.
    pub use bevy::ui::Checked;
    pub use bevy::ui_widgets::{Activate, SliderValue, TrackClick, ValueChange};

    // Touchy code.
    pub use crate::TouchyPlugins;
    pub use crate::canvas::{Canvas, CanvasConfig, CanvasPointer, CanvasSketch, CanvasTokens};
    pub use crate::choice::{Choice, ChoiceState, ChoiceStates, Chosen};
    pub use crate::containers::panel::{
        DragFeedback, PanelConfig, PanelDockTargets, PanelLimits, PanelPlacement, PanelSettings,
        PanelSize, PanelTokens,
    };
    pub use crate::containers::scroll_area::{
        ScrollArea, ScrollAreaConfig, ScrollAreaSettings, ScrollAreaTokens, ScrollAxes, ScrollTo,
    };
    pub use crate::containers::section::{SectionConfig, SectionOpen, SectionTitle, SectionTokens};
    pub use crate::containers::toolbar::{Toolbar, ToolbarConfig, ToolbarSide, ToolbarTokens};
    pub use crate::keyed::{Bind, SyncChildren};
    pub use crate::layers::{LayersPlugin, UiLayer, UiLayers};
    pub use crate::motion::ResponseCurve;
    pub use crate::popover::{
        PopoverAlign, PopoverConfig, PopoverOpen, PopoverSide, PopoverTokens, TogglesPopover,
    };
    pub use crate::primitives::{
        Card, Column, Divider, List, ListMarker, Row, Spacer, Unstretched, heading, small, text,
    };
    pub use crate::readout::meter::{Meter, MeterConfig, MeterStyle, MeterTokens, MeterValue};
    pub use crate::readout::plot::{
        AxisRange, PlotAxes, PlotConfig, PlotData, PlotHit, PlotPointer, PlotSeries, PlotTicks,
        PlotTokens, PointerGuide, SeriesPlotType,
    };
    pub use crate::spawn_ui::{NodePatch, SpawnUi};
    pub use crate::theme::{ActiveTheme, Theme, ThemeAppExt, ThemeToken};
    pub use crate::toast::{ShowToast, Toast, ToastKind, ToastTokens};
    pub use crate::tooltip::{Tooltip, TooltipTokens};
    pub use crate::widgets::WidgetPlugins;
    pub use crate::widgets::button::{ButtonConfig, ButtonTokens};
    pub use crate::widgets::capsule_flip::{CapsuleFlipConfig, CapsuleFlipTokens};
    pub use crate::widgets::checkbox::{CheckboxConfig, CheckboxTokens};
    pub use crate::widgets::combo::{ComboConfig, ComboText, ComboTokens};
    pub use crate::widgets::dpad::{
        DpadConfig, DpadSettings, DpadShape, DpadTokens, DpadValue, DpadWays,
    };
    pub use crate::widgets::dropdown::{DropdownConfig, DropdownTokens};
    pub use crate::widgets::gearshift::{GearIndex, GearshiftConfig};
    pub use crate::widgets::grabber::{
        GrabAxis, GrabCorner, GrabPhase, Grabbable, Grabbed, GrabberConfig, GrabberStyle,
        GrabberTokens,
    };
    pub use crate::widgets::grid::{GridConfig, GridShape, GridTokens};
    pub use crate::widgets::joystick::{
        JoystickAxes, JoystickConfig, JoystickSettings, JoystickTokens, JoystickValue,
    };
    pub use crate::widgets::menu::{MenuConfig, MenuPick, MenuTokens};
    pub use crate::widgets::number_input::{
        NumberField, NumberInputConfig, NumberInputTokens, NumberValue, StepButtons,
    };
    pub use crate::widgets::radial::{
        OpenRadial, RadialAt, RadialCenter, RadialConfig, RadialTokens, ReleaseRadial,
    };
    pub use crate::widgets::scrubber::{ScrubValue, ScrubberConfig, ScrubberTokens};
    pub use crate::widgets::segmented::{
        Highlight, SegmentWidths, SegmentedConfig, SegmentedTokens,
    };
    pub use crate::widgets::slider::{SliderConfig, SliderTokens};
    pub use crate::widgets::text_input::{
        CommitOn, TextInputConfig, TextInputTokens, TextInputValue,
    };
    pub use crate::widgets::transport::{
        Marker, Timeline, TimelineLabels, TransportButton, TransportCommand, TransportConfig,
        TransportKeys, TransportOf, TransportRequest, TransportSystems, TransportTokens,
        Transports,
    };
    pub use crate::world_view::{
        CursorPlane, WorldPoint, WorldPointer, WorldPress, WorldViewPlugin,
    };
}

/// Every touchy plugin. The UI layers' roots spawn at startup and become a Resource.
/// The ThemePlugin loads defaults and initializes the Theme resource. Everything else
/// stays inert until the game spawns or triggers something: world pointing, cancel
/// routing, tooltips, toasts, popovers, hover cursors, canvases, choices, widgets,
/// readouts, and containers.
///
/// Configure it like any Bevy plugin group: `.build().disable::<P>()` to drop a
/// plugin, `.build().set(P { .. })` to replace one with a configured instance.
///
/// Shared dependencies (`ThemePlugin`, Bevy's `CursorIconPlugin`) are not listed:
/// the plugins that need them ensure them, so a game may add them itself beforehand.
pub struct TouchyPlugins;

impl PluginGroup for TouchyPlugins {
    fn build(self) -> PluginGroupBuilder {
        PluginGroupBuilder::start::<Self>()
            // UI infrastructure: main window, layers, cancel routing
            .add(theme::ThemePlugin)
            .add(world_view::WorldViewPlugin)
            .add(layers::LayersPlugin)
            .add(cancel::CancelPlugin)
            // UI-level feedback: tooltips, toasts, popovers, cursors
            .add(tooltip::TooltipPlugin)
            .add(toast::ToastPlugin)
            .add(popover::PopoverPlugin)
            .add(cursor::HoverCursorPlugin)
            .add(canvas::CanvasPlugin)
            // Before every widget that offers choices.
            .add(choice::ChoicePlugin)
            // Widgets before the containers built from them (a panel's grabbers).
            .add_group(widgets::WidgetPlugins)
            .add_group(readout::ReadoutPlugins)
            .add_group(containers::ContainerPlugins)
            // Widgets built from containers after them (a dropdown's list scrolls).
            .add(widgets::dropdown::DropdownWidgetPlugin)
            // Menus after the dropdown they're built on.
            .add(widgets::menu::MenuWidgetPlugin)
            // The combo box after the dropdown, whose look its list follows.
            .add(widgets::combo::ComboWidgetPlugin)
    }
}

/// Adds `plugin` unless the app already has it.
///
/// Each touchy plugin calls this for its dependencies, so any plugin works on its
/// own. Bevy panics when a plugin is added twice, which has two consequences:
/// a game that adds a dependency itself must add it first, and a plugin group
/// must list a dependency before anything that depends on it.
pub(crate) fn ensure_plugin<P: Plugin>(app: &mut App, plugin: P) {
    if !app.is_plugin_added::<P>() {
        app.add_plugins(plugin);
    }
}

/// [`ensure_plugin`] as a method, so plugin builds can chain it.
pub(crate) trait EnsurePlugin {
    fn ensure_plugin<P: Plugin>(&mut self, plugin: P) -> &mut Self;
}

impl EnsurePlugin for App {
    fn ensure_plugin<P: Plugin>(&mut self, plugin: P) -> &mut Self {
        ensure_plugin(self, plugin);
        self
    }
}

#[cfg(test)]
mod tests {
    use bevy::picking::cursor::CursorIconPlugin;

    use super::*;
    use crate::theme::ThemePlugin;
    use crate::widgets::button::ButtonWidgetPlugin;
    use crate::widgets::joystick::JoystickWidgetPlugin;
    use crate::widgets::transport::TransportWidgetPlugin;

    /// A game that already added a shared dependency can still add the whole group.
    #[test]
    fn group_tolerates_dependencies_added_first() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, CursorIconPlugin, ThemePlugin, TouchyPlugins));
        assert!(app.is_plugin_added::<JoystickWidgetPlugin>());
    }

    /// A single plugin brings what it needs, so it works without the group.
    #[test]
    fn plugins_work_alone() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, TransportWidgetPlugin));
        assert!(app.is_plugin_added::<ButtonWidgetPlugin>());
        assert!(app.is_plugin_added::<ThemePlugin>());
        assert!(app.is_plugin_added::<CursorIconPlugin>());
    }

    /// The theme can be added again, before or after the group, and a theme
    /// set up in between is kept.
    #[test]
    fn the_theme_can_be_added_again_and_keeps_what_was_set() {
        use crate::theme::{Theme, palette};

        let mut app = App::new();
        app.add_plugins((MinimalPlugins, ThemePlugin));
        app.world_mut()
            .resource_mut::<Theme>()
            .set_color(palette::ACCENT, Color::WHITE);
        app.add_plugins((TouchyPlugins, ThemePlugin));
        let accent = app.world().resource::<Theme>().color(&palette::ACCENT);
        assert_eq!(accent, Color::WHITE);
    }

    /// Disabling a widget inside the nested group removes only that widget.
    #[test]
    fn widgets_can_be_disabled_from_the_top_level_group() {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            TouchyPlugins.build().disable::<JoystickWidgetPlugin>(),
        ));
        assert!(!app.is_plugin_added::<JoystickWidgetPlugin>());
        assert!(app.is_plugin_added::<ButtonWidgetPlugin>());
    }

    /// Tab only moves focus inside a `TabGroup`, and only to entities with a
    /// `TabIndex`. The screen layer supplies the group; widgets supply the index.
    #[test]
    fn tab_reaches_a_button_on_the_screen_layer() {
        use bevy::input::ButtonState;
        use bevy::input::keyboard::{Key, KeyboardInput, NativeKey};
        use bevy::input_focus::tab_navigation::TabNavigationPlugin;
        use bevy::input_focus::{InputDispatchPlugin, InputFocus, InputFocusPlugin};
        use bevy::window::PrimaryWindow;

        use crate::layers::{LayersPlugin, ScreenLayerRoot};
        use crate::widgets::button::{ButtonConfig, spawn_button};

        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::input::InputPlugin,
            InputFocusPlugin,
            InputDispatchPlugin,
            TabNavigationPlugin,
            LayersPlugin,
            ButtonWidgetPlugin,
        ));
        let window = app
            .world_mut()
            .spawn((Window::default(), PrimaryWindow))
            .id();
        app.update();

        let world = app.world_mut();
        let screen = world
            .query_filtered::<Entity, With<ScreenLayerRoot>>()
            .single(world)
            .unwrap();
        let mut button = None;
        world.commands().entity(screen).with_children(|screen| {
            // Tab skips hidden entities, and nothing computes visibility without
            // the rendering plugins, so mark the button visible by hand.
            let config = ButtonConfig::new("OK");
            button = Some(
                spawn_button(screen, config)
                    .insert(InheritedVisibility::VISIBLE)
                    .id(),
            );
        });
        app.update();

        app.world_mut().write_message(KeyboardInput {
            key_code: KeyCode::Tab,
            logical_key: Key::Unidentified(NativeKey::Unidentified),
            state: ButtonState::Pressed,
            text: None,
            repeat: false,
            window,
        });
        app.update();
        assert_eq!(app.world().resource::<InputFocus>().get(), button);
    }

    /// Clicks land on a button's label, and the focus request bubbles up from
    /// there. The button's `TabIndex` is what lets it take the focus.
    #[test]
    fn clicking_a_buttons_label_focuses_the_button() {
        use bevy::input_focus::pointer_focus::PointerFocusPlugin;
        use bevy::input_focus::{AcquireFocus, InputFocus, InputFocusPlugin};
        use bevy::window::PrimaryWindow;

        use crate::widgets::button::{ButtonConfig, spawn_button};

        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            InputFocusPlugin,
            PointerFocusPlugin,
            ButtonWidgetPlugin,
        ));
        let window = app
            .world_mut()
            .spawn((Window::default(), PrimaryWindow))
            .id();
        let mut button = None;
        app.world_mut()
            .commands()
            .spawn(Node::default())
            .with_children(|root| button = Some(spawn_button(root, ButtonConfig::new("OK")).id()));
        app.update();

        let button = button.unwrap();
        let label = app.world().get::<Children>(button).unwrap()[0];
        app.world_mut().trigger(AcquireFocus {
            focused_entity: label,
            window,
        });
        assert_eq!(app.world().resource::<InputFocus>().get(), Some(button));
    }
}

/// The README and the guides in `docs/`, compiled as doctests so their code keeps up with touchy.
#[cfg(doctest)]
mod docs {
    #[doc = include_str!("../README.md")]
    struct Readme;
    #[doc = include_str!("../docs/QUICK_START.md")]
    struct QuickStart;
    #[doc = include_str!("../docs/WIDGETS.md")]
    struct Widgets;
    #[doc = include_str!("../docs/LAYOUT.md")]
    struct Layout;
    #[doc = include_str!("../docs/THEMING.md")]
    struct Theming;
}
