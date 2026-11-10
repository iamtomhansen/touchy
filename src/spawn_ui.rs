//! touchy's facade: everything it builds, as one method each on the spawner a
//! closure is given, named for what it spawns.
//!
//! ```
//! # use bevy::prelude::*;
//! # use touchy::prelude::*;
//! fn settings(page: &mut ChildSpawnerCommands) {
//!     page.panel(PanelConfig::new(), |body| {
//!         body.heading("Units");
//!         body.capsule_flip(CapsuleFlipConfig::new().options("Imperial", "Metric"));
//!         body.divider();
//!         body.checkbox(CheckboxConfig::new().label("Subtitles"));
//!     });
//! }
//! ```
//!
//! Each method returns its entity's commands, to add components or observers to, and
//! [`node`](NodePatch::node) changes just the layout fields it names:
//!
//! ```
//! # use bevy::prelude::*;
//! # use touchy::prelude::*;
//! fn page(page: &mut ChildSpawnerCommands) {
//!     page.row(|row| {
//!         row.column(|card| {
//!             card.text("A default slider.");
//!             card.slider(SliderConfig::default());
//!         })
//!         .insert(Card);
//!     })
//!     .node(|node| node.width = percent(100.0));
//! }
//! ```
//! The handful of primitives are bundles too, which is useful for `children![]`.

use bevy::prelude::*;

use crate::primitives;

/// Spawns touchy's primitives, widgets and containers as children: `body.text("Hi")`,
/// `body.button(config)`, `body.panel(config, |inside| ..)`.
pub trait SpawnUi {
    /// Body text, in the palette's text colour, font and size: [`text`](primitives::text).
    fn text(&mut self, words: impl Into<String>) -> EntityCommands<'_>;

    /// A heading: [`heading`](primitives::heading).
    fn heading(&mut self, words: impl Into<String>) -> EntityCommands<'_>;

    /// Small print: [`small`](primitives::small).
    fn small(&mut self, words: impl Into<String>) -> EntityCommands<'_>;

    /// A thin line across its parent: a [`Divider`](primitives::Divider).
    fn divider(&mut self) -> EntityCommands<'_>;

    /// Room that pushes what follows to the far end: a [`Spacer`](primitives::Spacer).
    fn spacer(&mut self) -> EntityCommands<'_>;

    /// A bulleted [`List`](primitives::List) of `items`.
    fn bullets(&mut self, items: impl IntoIterator<Item = impl Into<String>>)
    -> EntityCommands<'_>;

    /// A [`List`](primitives::List) of `items`, numbered from 1.
    fn numbered(
        &mut self,
        items: impl IntoIterator<Item = impl Into<String>>,
    ) -> EntityCommands<'_>;

    /// What `build` spawns, left to right in a [`Row`](primitives::Row).
    fn row(&mut self, build: impl FnOnce(&mut ChildSpawnerCommands)) -> EntityCommands<'_>;

    /// What `build` spawns, top to bottom in a [`Column`](primitives::Column).
    fn column(&mut self, build: impl FnOnce(&mut ChildSpawnerCommands)) -> EntityCommands<'_>;

    /// Spawns a themed button and returns it for further inserts. It holds
    /// [`ButtonTokens`](crate::widgets::button::ButtonTokens) and
    /// [`ButtonParts`](crate::widgets::button::ButtonParts). React to presses by observing
    /// [`Activate`](bevy::ui_widgets::Activate) on it.
    ///
    /// # Examples
    /// ```
    /// use bevy::prelude::*;
    /// use bevy::ui_widgets::Activate;
    /// use touchy::spawn_ui::SpawnUi;
    /// use touchy::widgets::button::{ButtonConfig, ButtonTokens};
    ///
    /// fn spawn_menu(parent: &mut ChildSpawnerCommands) {
    ///     parent.button(ButtonConfig::new("Play")).observe(|_: On<Activate>| {
    ///         info!("Play pressed");
    ///     });
    ///     let quit = ButtonConfig::new("Quit").tokens(ButtonTokens::danger());
    ///     parent.button(quit);
    /// }
    /// ```
    fn button(&mut self, config: crate::widgets::button::ButtonConfig) -> EntityCommands<'_>;

    /// Spawns a canvas. Returns the canvas, which holds [`Canvas`](crate::canvas::Canvas) and
    /// [`CanvasPointer`](crate::canvas::CanvasPointer), and for an image,
    /// [`CanvasImage`](crate::canvas::CanvasImage) and [`CanvasFit`](crate::canvas::CanvasFit).
    ///
    /// # Examples
    /// ```
    /// use bevy::prelude::*;
    /// use touchy::canvas::{Canvas, CanvasConfig, CanvasSketch};
    /// use touchy::spawn_ui::SpawnUi;
    ///
    /// /// Marks the canvas a radar is drawn on.
    /// #[derive(Component)]
    /// struct Radar;
    ///
    /// fn spawn_radar(parent: &mut ChildSpawnerCommands) -> Entity {
    ///     parent.canvas(CanvasConfig::painted().size(160.0, 160.0))
    ///         .insert(Radar)
    ///         .id()
    /// }
    ///
    /// // Each frame, draw the radar's sweep; the sketch starts empty every frame.
    /// fn sweep(mut sketch: CanvasSketch, radars: Query<Entity, With<Radar>>) {
    ///     for radar in &radars {
    ///         let middle = sketch.picture_size(radar) / 2.0;
    ///         if let Some(mut lines) = sketch.on(radar) {
    ///             lines.circle_2d(middle, middle.x, Color::srgb(0.2, 0.9, 0.4));
    ///         }
    ///     }
    /// }
    ///
    /// # let mut app = App::new();
    /// # // What a canvas needs from Bevy, which `DefaultPlugins` brings in a game.
    /// # app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()))
    /// #     .init_asset::<Image>()
    /// #     .init_resource::<bevy::picking::hover::HoverMap>()
    /// #     .init_resource::<bevy::picking::pointer::PointerMap>()
    /// #     .add_plugins(touchy::canvas::CanvasPlugin)
    /// #     .add_systems(Update, sweep);
    /// # let mut radar = None;
    /// # app.world_mut()
    /// #     .commands()
    /// #     .spawn(Node::default())
    /// #     .with_children(|parent| radar = Some(spawn_radar(parent)));
    /// # app.update();
    /// # let canvas = app.world().get::<Canvas>(radar.unwrap()).unwrap();
    /// # assert!(canvas.stage().is_some(), "a painting has a stage to draw on");
    /// ```
    fn canvas(&mut self, config: crate::canvas::CanvasConfig) -> EntityCommands<'_>;

    /// Spawns a capsule flip widget. Returns the capsule, which holds
    /// [`CapsuleFlipTokens`](crate::widgets::capsule_flip::CapsuleFlipTokens) and
    /// [`CapsuleFlipParts`](crate::widgets::capsule_flip::CapsuleFlipParts). Observe
    /// `ValueChange<bool>` on it to follow its choice.
    ///
    /// # Examples
    /// ```
    /// use bevy::prelude::*;
    /// use touchy::prelude::*;
    ///
    /// fn input_choice(parent: &mut ChildSpawnerCommands) {
    ///     parent.capsule_flip(CapsuleFlipConfig::new().options("Mouse", "Keyboard"));
    /// }
    /// ```
    fn capsule_flip(
        &mut self,
        config: crate::widgets::capsule_flip::CapsuleFlipConfig,
    ) -> EntityCommands<'_>;

    /// Spawns a checkbox. Returns the checkbox, which holds
    /// [`CheckboxTokens`](crate::widgets::checkbox::CheckboxTokens) and
    /// [`CheckboxParts`](crate::widgets::checkbox::CheckboxParts). Observe `ValueChange<bool>` on
    /// it, or read whether it has [`Checked`](bevy::ui::Checked).
    ///
    /// # Examples
    /// ```
    /// use bevy::prelude::*;
    /// use bevy::ui_widgets::ValueChange;
    /// use touchy::prelude::*;
    ///
    /// fn spawn_loop_toggle(parent: &mut ChildSpawnerCommands) {
    ///     parent.checkbox(CheckboxConfig::new().label("Loop")).observe(
    ///         |change: On<ValueChange<bool>>| info!("Looping: {}", change.value),
    ///     );
    /// }
    /// ```
    fn checkbox(&mut self, config: crate::widgets::checkbox::CheckboxConfig) -> EntityCommands<'_>;

    /// Spawns a combo box. Returns the combo, which holds its [`Chosen`](crate::choice::Chosen),
    /// [`ComboText`](crate::widgets::combo::ComboText),
    /// [`ComboTokens`](crate::widgets::combo::ComboTokens) and
    /// [`ComboParts`](crate::widgets::combo::ComboParts), and announces its changes.
    ///
    /// # Examples
    /// ```
    /// use bevy::prelude::*;
    /// use bevy::ui_widgets::ValueChange;
    /// use touchy::prelude::*;
    ///
    /// const CITIES: [&str; 3] = ["Montreal", "Ottawa", "Toronto"];
    ///
    /// fn spawn_city_picker(parent: &mut ChildSpawnerCommands) {
    ///     let config = ComboConfig::new(CITIES).chosen(1).placeholder("City");
    ///     parent.combo(config).observe(|change: On<ValueChange<usize>>| {
    ///         info!("Off to {}", CITIES[change.value]);
    ///     });
    /// }
    /// ```
    fn combo(&mut self, config: crate::widgets::combo::ComboConfig) -> EntityCommands<'_>;

    /// Spawns a D-pad widget. Returns the pad, which holds
    /// [`DpadValue`](crate::widgets::dpad::DpadValue),
    /// [`DpadSettings`](crate::widgets::dpad::DpadSettings),
    /// [`DpadTokens`](crate::widgets::dpad::DpadTokens) and
    /// [`DpadParts`](crate::widgets::dpad::DpadParts). Observe `ValueChange<IVec2>` on it to follow
    /// the direction pressed. A floating pad fills its parent, and appears wherever in it is
    /// pressed.
    ///
    /// # Examples
    /// A pad that moves a menu's selection, again and again while it's held:
    /// ```
    /// use bevy::prelude::*;
    /// use bevy::ui_widgets::ValueChange;
    /// use touchy::prelude::*;
    ///
    /// /// The menu row selected.
    /// #[derive(Resource, Default)]
    /// struct Selected(i32);
    ///
    /// fn spawn_menu_pad(parent: &mut ChildSpawnerCommands) {
    ///     let config = DpadConfig::new(DpadWays::Vertical).repeat(0.25);
    ///     parent.dpad(config).observe(
    ///         |change: On<ValueChange<IVec2>>, mut selected: ResMut<Selected>| {
    ///             // +y is up, and up the menu is a row back.
    ///             selected.0 -= change.value.y;
    ///         },
    ///     );
    /// }
    /// ```
    fn dpad(&mut self, config: crate::widgets::dpad::DpadConfig) -> EntityCommands<'_>;

    /// Spawns a dropdown selector. Returns the dropdown, which holds its
    /// [`Chosen`](crate::choice::Chosen),
    /// [`DropdownTokens`](crate::widgets::dropdown::DropdownTokens) and
    /// [`DropdownParts`](crate::widgets::dropdown::DropdownParts), and announces a new choice as
    /// `ValueChange<usize>`.
    ///
    /// # Examples
    /// ```
    /// use bevy::prelude::*;
    /// use bevy::ui_widgets::ValueChange;
    /// use touchy::prelude::*;
    ///
    /// const SIZES: [&str; 3] = ["Small", "Medium", "Large"];
    ///
    /// fn spawn_size_picker(parent: &mut ChildSpawnerCommands) {
    ///     let config = DropdownConfig::new(SIZES).placeholder("Pick a size");
    ///     parent.dropdown(config).observe(|change: On<ValueChange<usize>>| {
    ///         info!("You have selected {}", SIZES[change.value]);
    ///     });
    /// }
    /// ```
    fn dropdown(&mut self, config: crate::widgets::dropdown::DropdownConfig) -> EntityCommands<'_>;

    /// Spawns a gearshift. It fills its parent's width.
    /// Observe `ValueChange<usize>` on it to hear gear changes.
    fn gearshift(
        &mut self,
        config: crate::widgets::gearshift::GearshiftConfig,
    ) -> EntityCommands<'_>;

    /// Spawns a grabber: a [`Grabbable`](crate::widgets::grabber::Grabbable) in its style's look.
    /// Observe [`Grabbed`](crate::widgets::grabber::Grabbed) on it.
    ///
    /// # Examples
    /// A sidebar with a handle on its edge to resize it by:
    /// ```
    /// use bevy::prelude::*;
    /// use touchy::spawn_ui::SpawnUi;
    /// use touchy::widgets::grabber::{GrabAxis, GrabPhase, Grabbed, GrabberConfig};
    ///
    /// /// The sidebar's width when a drag started.
    /// #[derive(Component, Clone, Copy)]
    /// struct Resizing(f32);
    ///
    /// fn spawn_sidebar(row: &mut ChildSpawnerCommands) {
    ///     let sidebar = row.spawn(Node { width: px(200.0), ..default() }).id();
    ///     row.grabber(GrabberConfig::handle(GrabAxis::Horizontal)).observe(
    ///         move |grabbed: On<Grabbed>,
    ///               mut nodes: Query<&mut Node>,
    ///               resizing: Query<&Resizing>,
    ///               mut commands: Commands| {
    ///             let Ok(mut node) = nodes.get_mut(sidebar) else {
    ///                 return;
    ///             };
    ///             if grabbed.phase == GrabPhase::Start {
    ///                 if let Val::Px(width) = node.width {
    ///                     commands.entity(grabbed.grabber).insert(Resizing(width));
    ///                 }
    ///             } else if let Ok(&Resizing(start)) = resizing.get(grabbed.grabber) {
    ///                 // A cancelled drag's offset is zero: back where it started.
    ///                 node.width = px((start + grabbed.offset.x).max(80.0));
    ///             }
    ///         },
    ///     );
    /// }
    /// ```
    fn grabber(&mut self, config: crate::widgets::grabber::GrabberConfig) -> EntityCommands<'_>;

    /// Spawns a grid selector. Returns the grid, which holds its [`Chosen`](crate::choice::Chosen),
    /// [`GridShape`](crate::widgets::grid::GridShape),
    /// [`GridTokens`](crate::widgets::grid::GridTokens) and
    /// [`GridParts`](crate::widgets::grid::GridParts), and announces a new choice as
    /// `ValueChange<usize>`.
    ///
    /// # Examples
    /// ```
    /// use bevy::prelude::*;
    /// use bevy::ui_widgets::ValueChange;
    /// use touchy::prelude::*;
    ///
    /// const SEASONS: [&str; 4] = ["Spring", "Summer", "Autumn", "Winter"];
    ///
    /// fn spawn_season_picker(parent: &mut ChildSpawnerCommands) {
    ///     let config = GridConfig::new(SEASONS, 2).chosen(0);
    ///     parent.grid(config).observe(|change: On<ValueChange<usize>>| {
    ///         info!("On to {}", SEASONS[change.value]);
    ///     });
    /// }
    /// ```
    fn grid(&mut self, config: crate::widgets::grid::GridConfig) -> EntityCommands<'_>;

    /// Spawns a joystick. Returns the joystick, which holds
    /// [`JoystickValue`](crate::widgets::joystick::JoystickValue),
    /// [`JoystickSettings`](crate::widgets::joystick::JoystickSettings),
    /// [`JoystickTokens`](crate::widgets::joystick::JoystickTokens) and
    /// [`JoystickParts`](crate::widgets::joystick::JoystickParts). Read the value each frame, or
    /// observe `ValueChange<Vec2>` on it (`is_final` on release). A floating joystick fills its
    /// parent, and appears wherever in it is pressed.
    ///
    /// # Examples
    /// ```
    /// use bevy::prelude::*;
    /// use touchy::prelude::*;
    ///
    /// /// The ship the stick steers.
    /// #[derive(Component)]
    /// struct Ship;
    ///
    /// fn spawn_stick(parent: &mut ChildSpawnerCommands) {
    ///     parent.joystick(JoystickConfig::new());
    /// }
    ///
    /// // Each frame, the stick's value is the ship's heading and speed.
    /// fn steer(
    ///     time: Res<Time>,
    ///     sticks: Query<&JoystickValue>,
    ///     mut ships: Query<&mut Transform, With<Ship>>,
    /// ) {
    ///     let Ok(stick) = sticks.single() else {
    ///         return;
    ///     };
    ///     for mut ship in &mut ships {
    ///         ship.translation += stick.0.extend(0.0) * 200.0 * time.delta_secs();
    ///     }
    /// }
    /// ```
    fn joystick(&mut self, config: crate::widgets::joystick::JoystickConfig) -> EntityCommands<'_>;

    /// Spawns a menu. Returns the menu, which holds its
    /// [`MenuTokens`](crate::widgets::menu::MenuTokens) and
    /// [`DropdownParts`](crate::widgets::dropdown::DropdownParts); observe
    /// [`MenuPick`](crate::widgets::menu::MenuPick) on it.
    ///
    /// # Examples
    /// A menu bar of two menus:
    /// ```
    /// use bevy::prelude::*;
    /// use touchy::prelude::*;
    ///
    /// fn spawn_menu_bar(parent: &mut ChildSpawnerCommands) {
    ///     parent.spawn(Node::default()).with_children(|bar| {
    ///         bar.menu(MenuConfig::new("File", ["New", "Open", "Quit"])).observe(
    ///             |pick: On<MenuPick>, mut exit: MessageWriter<AppExit>| {
    ///                 if pick.index == 2 {
    ///                     exit.write(AppExit::Success);
    ///                 }
    ///             },
    ///         );
    ///         bar.menu(MenuConfig::new("Edit", ["Undo", "Redo"]));
    ///     });
    /// }
    /// ```
    fn menu(&mut self, config: crate::widgets::menu::MenuConfig) -> EntityCommands<'_>;

    /// Spawns a meter to show magnitude of a value.
    ///
    /// Behaviour is set at spawn time with [`MeterConfig`](crate::readout::meter::MeterConfig).
    /// Different types of meter can exist but for now only
    /// [`MeterStyle::Bar`](crate::readout::meter::MeterStyle::Bar) is implemented. Data can be
    /// pushed into the meter by changing its [`MeterValue`](crate::readout::meter::MeterValue).
    ///
    /// # Examples
    /// ```
    /// use bevy::prelude::*;
    /// use touchy::prelude::*;
    ///
    /// fn spawn_health_bar(parent: &mut ChildSpawnerCommands, health: f32) -> Entity {
    ///     parent.meter(MeterConfig::new(health).range(0.0, 100.0)).id()
    /// }
    ///
    /// // Later, when the player takes a hit, set the value and the bar follows.
    /// fn take_hit(mut meters: Query<&mut MeterValue>) {
    ///     for mut value in &mut meters {
    ///         value.0 -= 10.0;
    ///     }
    /// }
    ///
    /// # let mut app = App::new();
    /// # app.add_plugins((MinimalPlugins, touchy::readout::meter::MeterPlugin))
    /// #     .add_systems(Update, take_hit);
    /// # let mut bar = None;
    /// # app.world_mut()
    /// #     .commands()
    /// #     .spawn(Node::default())
    /// #     .with_children(|parent| bar = Some(spawn_health_bar(parent, 75.0)));
    /// # app.update();
    /// # assert_eq!(app.world().get::<MeterValue>(bar.unwrap()), Some(&MeterValue(65.0)));
    /// ```
    fn meter(&mut self, config: crate::readout::meter::MeterConfig) -> EntityCommands<'_>;

    /// Spawns a number input. Returns the input, which holds its
    /// [`NumberValue`](crate::widgets::number_input::NumberValue),
    /// [`NumberInputTokens`](crate::widgets::number_input::NumberInputTokens) and
    /// [`NumberInputParts`](crate::widgets::number_input::NumberInputParts). Observe
    /// `ValueChange<f64>` on it, or read its value.
    ///
    /// # Examples
    /// A stepper with no field, whose value the game shows as hearts:
    /// ```
    /// use bevy::prelude::*;
    /// use bevy::ui_widgets::ValueChange;
    /// use touchy::prelude::*;
    ///
    /// /// The lives a new game starts with.
    /// #[derive(Resource)]
    /// struct Lives(u32);
    ///
    /// fn spawn_lives_picker(parent: &mut ChildSpawnerCommands) {
    ///     let config = NumberInputConfig::new(3.0)
    ///         .range(1.0, 9.0)
    ///         .field(NumberField::None);
    ///     parent.number_input(config).observe(
    ///         |change: On<ValueChange<f64>>, mut lives: ResMut<Lives>| {
    ///             lives.0 = change.value as u32;
    ///         },
    ///     );
    /// }
    /// ```
    fn number_input(
        &mut self,
        config: crate::widgets::number_input::NumberInputConfig,
    ) -> EntityCommands<'_>;

    /// Spawns a panel, filled by `build`. By default, sits in the parent's layout like any other
    /// node; a [`PanelPlacement`](crate::containers::panel::PanelPlacement) docks it to an edge or
    /// corner, or floats it.
    ///
    /// Returns commands for the panel, which carries
    /// [`PanelPlacement`](crate::containers::panel::PanelPlacement),
    /// [`PanelSize`](crate::containers::panel::PanelSize),
    /// [`PanelSettings`](crate::containers::panel::PanelSettings) and
    /// [`PanelTokens`](crate::containers::panel::PanelTokens).
    ///
    /// # Examples
    /// ```
    /// use bevy::prelude::*;
    /// use touchy::prelude::*;
    ///
    /// fn info_panel(parent: &mut ChildSpawnerCommands, info: &str) -> Entity {
    ///     parent.panel(PanelConfig::new(), |panel| {
    ///         panel.spawn(Text::new(info));
    ///     })
    ///     .id()
    /// }
    ///
    /// # let mut app = App::new();
    /// # app.add_plugins((MinimalPlugins, touchy::containers::panel::PanelPlugin));
    /// # let mut panel = None;
    /// # app.world_mut()
    /// #     .commands()
    /// #     .spawn(Node::default())
    /// #     .with_children(|parent| panel = Some(info_panel(parent, "Hola, Mundo!")));
    /// # app.update();
    /// # assert_eq!(
    /// #     app.world().get::<PanelPlacement>(panel.unwrap()),
    /// #     Some(&PanelPlacement::Inline)
    /// # );
    /// ```
    fn panel(
        &mut self,
        config: crate::containers::panel::PanelConfig,
        build: impl FnOnce(&mut ChildSpawnerCommands),
    ) -> EntityCommands<'_>;

    /// Spawns a plot. Returns the plot, which holds [`PlotData`](crate::readout::plot::PlotData),
    /// [`PlotAxes`](crate::readout::plot::PlotAxes),
    /// [`PlotTicks`](crate::readout::plot::PlotTicks),
    /// [`PointerGuide`](crate::readout::plot::PointerGuide) and
    /// [`PlotPointer`](crate::readout::plot::PlotPointer).
    ///
    /// # Examples
    /// ```
    /// use bevy::prelude::*;
    /// use touchy::prelude::*;
    ///
    /// /// Marks the plot of each team's shots, to find it again.
    /// #[derive(Component)]
    /// struct ShotsPlot;
    ///
    /// fn spawn_shots(parent: &mut ChildSpawnerCommands) -> Entity {
    ///     let config = PlotConfig::new()
    ///         .bars("shots", &[12.0, 9.0, 15.0])
    ///         .x_categories(["Home", "Away", "Neutral"])
    ///         .size(300.0, 200.0);
    ///     parent.plot(config).insert(ShotsPlot).id()
    /// }
    ///
    /// // Later, when the numbers change, set them and the bars follow.
    /// fn count_shots(mut plots: Query<&mut PlotData, With<ShotsPlot>>) {
    ///     for mut data in &mut plots {
    ///         data.set_values("shots", &[13.0, 9.0, 15.0]);
    ///     }
    /// }
    ///
    /// # let mut app = App::new();
    /// # // What a canvas needs from Bevy, which `DefaultPlugins` brings in a game.
    /// # app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()))
    /// #     .init_asset::<Image>()
    /// #     .init_resource::<bevy::picking::hover::HoverMap>()
    /// #     .init_resource::<bevy::picking::pointer::PointerMap>()
    /// #     .add_plugins(touchy::readout::plot::PlotPlugin)
    /// #     .add_systems(Update, count_shots);
    /// # let mut plot = None;
    /// # app.world_mut()
    /// #     .commands()
    /// #     .spawn(Node::default())
    /// #     .with_children(|parent| plot = Some(spawn_shots(parent)));
    /// # app.update();
    /// # let data = app.world().get::<PlotData>(plot.unwrap()).unwrap();
    /// # assert_eq!(data.get("shots").unwrap().points[0], Vec2::new(0.0, 13.0));
    /// ```
    fn plot(&mut self, config: crate::readout::plot::PlotConfig) -> EntityCommands<'_>;

    /// Spawns a popover floating beside the entity it's spawned in, filled by `build`.
    ///
    /// # Examples
    /// ```
    /// use bevy::prelude::*;
    /// use touchy::prelude::*;
    ///
    /// // A "?" that opens a note beside it. The anchor holds both, so pressing
    /// // the "?" again closes the note rather than counting as a press elsewhere.
    /// fn spawn_help(parent: &mut ChildSpawnerCommands, note: &str) -> Entity {
    ///     let mut anchor = parent.spawn(Node::default());
    ///     anchor.with_children(|anchor| {
    ///         // Spawned closed, as configs are by default.
    ///         let popover = anchor.popover(PopoverConfig::new(), |popover| {
    ///             popover.spawn(Text::new(note));
    ///         })
    ///         .id();
    ///         anchor.button(ButtonConfig::new("?")).insert(TogglesPopover(popover));
    ///     });
    ///     anchor.id()
    /// }
    ///
    /// # let mut app = App::new();
    /// # app.add_plugins((MinimalPlugins, bevy::input::InputPlugin, touchy::popover::PopoverPlugin));
    /// # app.world_mut()
    /// #     .commands()
    /// #     .spawn(Node::default())
    /// #     .with_children(|parent| {
    /// #         spawn_help(parent, "Shots on goal, last 10 games");
    /// #     });
    /// # app.update();
    /// # let world = app.world_mut();
    /// # assert_eq!(world.query::<&PopoverOpen>().single(world).unwrap(), &PopoverOpen(false));
    /// ```
    fn popover(
        &mut self,
        config: crate::popover::PopoverConfig,
        build: impl FnOnce(&mut ChildSpawnerCommands),
    ) -> EntityCommands<'_>;

    /// Spawns a radial with no button, closed, for the game to open with
    /// [`OpenRadial`](crate::widgets::radial::OpenRadial). Returns it; it holds the radial's
    /// [`Chosen`](crate::choice::Chosen), [`RadialTokens`](crate::widgets::radial::RadialTokens)
    /// and [`RadialParts`](crate::widgets::radial::RadialParts), and announces its changes. It
    /// takes no room where it's spawned.
    ///
    /// # Examples
    /// A quick-select ring opened at the cursor while a key is held:
    /// ```
    /// use bevy::prelude::*;
    /// use touchy::prelude::*;
    ///
    /// /// The weapon wheel.
    /// #[derive(Component)]
    /// struct Wheel;
    ///
    /// fn spawn_wheel(parent: &mut ChildSpawnerCommands) {
    ///     parent.radial(RadialConfig::new(["Sword", "Bow", "Staff"])).insert(Wheel);
    /// }
    ///
    /// fn hold_to_open(
    ///     keys: Res<ButtonInput<KeyCode>>,
    ///     wheels: Query<Entity, With<Wheel>>,
    ///     mut commands: Commands,
    /// ) {
    ///     let Ok(radial) = wheels.single() else {
    ///         return;
    ///     };
    ///     if keys.just_pressed(KeyCode::KeyQ) {
    ///         commands.trigger(OpenRadial { radial, at: RadialAt::Cursor, held: true });
    ///     } else if keys.just_released(KeyCode::KeyQ) {
    ///         commands.trigger(ReleaseRadial { radial });
    ///     }
    /// }
    /// ```
    fn radial(&mut self, config: crate::widgets::radial::RadialConfig) -> EntityCommands<'_>;

    /// Spawns a button showing the choice, which opens its ring around where it's pressed. Returns
    /// the button, which holds the radial's [`Chosen`](crate::choice::Chosen),
    /// [`RadialTokens`](crate::widgets::radial::RadialTokens) and
    /// [`RadialParts`](crate::widgets::radial::RadialParts), and announces its changes.
    ///
    /// # Examples
    /// ```
    /// use bevy::prelude::*;
    /// use bevy::ui_widgets::ValueChange;
    /// use touchy::prelude::*;
    ///
    /// const TOOLS: [&str; 4] = ["Brush", "Eraser", "Fill", "Picker"];
    ///
    /// fn spawn_tool_picker(parent: &mut ChildSpawnerCommands) {
    ///     let config = RadialConfig::new(TOOLS).chosen(0);
    ///     parent.radial_button(config).observe(|change: On<ValueChange<usize>>| {
    ///         info!("Now using the {}", TOOLS[change.value]);
    ///     });
    /// }
    /// ```
    fn radial_button(&mut self, config: crate::widgets::radial::RadialConfig)
    -> EntityCommands<'_>;

    /// Spawns a scroll area.
    ///
    /// Overlays a scrollbar on the edge of its `view` node, or two if `config.axes` is
    /// [`ScrollAxes::Both`](crate::containers::scroll_area::ScrollAxes::Both). The scrollbar track
    /// is hidden until the view's contents exceed the maximum size.
    ///
    /// # Examples
    /// ```
    /// use bevy::prelude::*;
    /// use touchy::prelude::*;
    ///
    /// fn spawn_log(parent: &mut ChildSpawnerCommands, lines: &[&str]) -> Entity {
    ///     parent.scroll_area(ScrollAreaConfig::default().stick_to_end(true), |view| {
    ///         for &line in lines {
    ///             view.spawn(Text::new(line));
    ///         }
    ///     })
    ///     .id()
    /// }
    ///
    /// # let mut app = App::new();
    /// # app.add_plugins((MinimalPlugins, touchy::containers::scroll_area::ScrollAreaPlugin));
    /// # let mut log = None;
    /// # app.world_mut().commands().spawn(Node::default()).with_children(|parent| {
    /// #     log = Some(spawn_log(parent, &["It was the best of times,", "it was the worst of times."]));
    /// # });
    /// # app.update();
    /// # let view = app.world().get::<ScrollArea>(log.unwrap()).unwrap().view();
    /// # assert_eq!(app.world().get::<Children>(view).unwrap().len(), 2);
    /// ```
    fn scroll_area(
        &mut self,
        config: crate::containers::scroll_area::ScrollAreaConfig,
        build: impl FnOnce(&mut ChildSpawnerCommands),
    ) -> EntityCommands<'_>;

    /// Spawns a scrubber. Returns the scrubber, which holds
    /// [`ScrubValue`](crate::widgets::scrubber::ScrubValue),
    /// [`ScrubSettings`](crate::widgets::scrubber::ScrubSettings),
    /// [`ScrubberTokens`](crate::widgets::scrubber::ScrubberTokens) and
    /// [`ScrubberParts`](crate::widgets::scrubber::ScrubberParts). Observe `ValueChange<f64>` on it
    /// to follow the value; it's final when a drag ends or a step button is pressed.
    ///
    /// # Examples
    /// ```
    /// use bevy::prelude::*;
    /// use bevy::ui_widgets::ValueChange;
    /// use touchy::prelude::*;
    ///
    /// fn volume_control(parent: &mut ChildSpawnerCommands) {
    ///     let config = ScrubberConfig::new(50.0)
    ///         .range(0.0, 100.0)
    ///         .precision(0)
    ///         .label("Volume");
    ///     parent.scrubber(config).observe(|change: On<ValueChange<f64>>| {
    ///         if change.is_final {
    ///             info!("Volume set to {}", change.value);
    ///         }
    ///     });
    /// }
    /// ```
    fn scrubber(&mut self, config: crate::widgets::scrubber::ScrubberConfig) -> EntityCommands<'_>;

    /// Spawns a collapsible section container.
    ///
    /// The header shows title and a chevron indicating whether the section is
    /// open or closed. Toggling it will expand or collapse the body, and announce
    /// the new state. The body is a column that can hold any content.
    ///
    /// # Examples
    /// ```
    /// use bevy::prelude::*;
    /// use touchy::prelude::*;
    ///
    /// fn spawn_options(parent: &mut ChildSpawnerCommands) -> Entity {
    ///     parent.section(SectionConfig::new("Options"), |body| {
    ///         body.spawn(Text::new("Sound"));
    ///     })
    ///     .id()
    /// }
    ///
    /// # let mut app = App::new();
    /// # app.add_plugins((MinimalPlugins, touchy::containers::section::SectionPlugin));
    /// # let mut section = None;
    /// # app.world_mut()
    /// #     .commands()
    /// #     .spawn(Node::default())
    /// #     .with_children(|parent| section = Some(spawn_options(parent)));
    /// # app.update();
    /// # assert_eq!(app.world().get::<SectionOpen>(section.unwrap()), Some(&SectionOpen(true)));
    /// ```
    fn section(
        &mut self,
        config: crate::containers::section::SectionConfig,
        build: impl FnOnce(&mut ChildSpawnerCommands),
    ) -> EntityCommands<'_>;

    /// Spawns a segmented selector. Returns the control, which holds its
    /// [`Chosen`](crate::choice::Chosen),
    /// [`SegmentedTokens`](crate::widgets::segmented::SegmentedTokens) and
    /// [`SegmentedParts`](crate::widgets::segmented::SegmentedParts), and announces a new selection
    /// as `ValueChange<usize>`.
    ///
    /// # Examples
    /// ```
    /// use bevy::prelude::*;
    /// use bevy::ui_widgets::ValueChange;
    /// use touchy::prelude::*;
    ///
    /// const QUALITY: [&str; 3] = ["Low", "Medium", "High"];
    ///
    /// fn spawn_quality_picker(parent: &mut ChildSpawnerCommands) {
    ///     let config = SegmentedConfig::new(QUALITY).chosen(1);
    ///     parent.segmented(config).observe(|change: On<ValueChange<usize>>| {
    ///         info!("Graphics quality: {}", QUALITY[change.value]);
    ///     });
    /// }
    /// ```
    fn segmented(
        &mut self,
        config: crate::widgets::segmented::SegmentedConfig,
    ) -> EntityCommands<'_>;

    /// Spawns a slider, allowing a user to drag it to change its value. It fills its parent's
    /// width; size the parent to size it. Returns the slider, which holds
    /// [`SliderTokens`](crate::widgets::slider::SliderTokens) and
    /// [`SliderParts`](crate::widgets::slider::SliderParts). Observe `ValueChange<f32>` on it, or
    /// read its [`SliderValue`](bevy::ui_widgets::SliderValue).
    ///
    /// # Examples
    /// ```
    /// use bevy::prelude::*;
    /// use touchy::prelude::*;
    ///
    /// fn spawn_brightness_slider(parent: &mut ChildSpawnerCommands) {
    ///     parent.slider(SliderConfig::new(40.0).range(20.0..=80.0));
    /// }
    /// ```
    fn slider(&mut self, config: crate::widgets::slider::SliderConfig) -> EntityCommands<'_>;

    /// Spawns a text field. Returns the field, which holds
    /// [`TextInputTokens`](crate::widgets::text_input::TextInputTokens),
    /// [`TextInputParts`](crate::widgets::text_input::TextInputParts) and
    /// [`TextInputSettings`](crate::widgets::text_input::TextInputSettings). Observe
    /// `ValueChange<String>` on it, or read its
    /// [`TextInputValue`](crate::widgets::text_input::TextInputValue).
    ///
    /// # Examples
    /// ```
    /// use bevy::prelude::*;
    /// use bevy::ui_widgets::ValueChange;
    /// use touchy::prelude::*;
    ///
    /// fn spawn_name_field(parent: &mut ChildSpawnerCommands) {
    ///     let config = TextInputConfig::new("").placeholder("Player name").max_chars(16);
    ///     parent.text_input(config)
    ///         .observe(|change: On<ValueChange<String>>| info!("Name: {}", change.value));
    /// }
    /// ```
    fn text_input(
        &mut self,
        config: crate::widgets::text_input::TextInputConfig,
    ) -> EntityCommands<'_>;

    /// Spawns a toolbar that reserves space along one edge of its parent.
    ///
    /// The `bar` and `body` closures populate separate child
    /// containers. The bar occupies its natural thickness,
    /// while the body fills the remaining space.
    ///
    /// Returns commands for the root entity, which carries
    /// [`Toolbar`](crate::containers::toolbar::Toolbar) and
    /// [`ToolbarTokens`](crate::containers::toolbar::ToolbarTokens).
    ///
    /// # Examples
    /// ```
    /// use bevy::prelude::*;
    /// use touchy::prelude::*;
    ///
    /// fn setup_window(parent: &mut ChildSpawnerCommands) -> Entity {
    ///     parent.toolbar(ToolbarConfig::new(ToolbarSide::Top),
    ///         |bar| {
    ///             bar.spawn(Name::new("menu"));
    ///         },
    ///         |body| {
    ///             body.spawn(Name::new("content"));
    ///         },
    ///     ).id()
    /// }
    ///
    /// # let mut app = App::new();
    /// # app.add_plugins((MinimalPlugins, touchy::containers::toolbar::ToolbarPlugin));
    /// # let mut root = None;
    /// # app.world_mut().commands()
    /// #     .spawn(Node::default())
    /// #     .with_children(|parent| {
    /// #         root = Some(setup_window(parent));
    /// #     });
    /// # app.update();
    /// # let toolbar = app.world().get::<Toolbar>(root.unwrap()).unwrap();
    /// # let menu = app.world().get::<Children>(toolbar.bar()).unwrap()[0];
    /// # assert_eq!(app.world().get::<Name>(menu).unwrap().as_str(), "menu");
    /// ```
    fn toolbar(
        &mut self,
        config: crate::containers::toolbar::ToolbarConfig,
        bar: impl FnOnce(&mut ChildSpawnerCommands),
        body: impl FnOnce(&mut ChildSpawnerCommands),
    ) -> EntityCommands<'_>;

    /// Spawns a transport bar, showing and controlling the
    /// config's timeline.
    ///
    /// # Examples
    /// ```
    /// use bevy::prelude::*;
    /// use touchy::prelude::*;
    ///
    /// fn spawn_replay(parent: &mut ChildSpawnerCommands) -> Entity {
    ///     // The host's timeline, which it plays by observing the bar's requests.
    ///     let timeline = parent
    ///         .commands()
    ///         .spawn((Timeline::default(), TimelineLabels::default()))
    ///         .observe(|request: On<TransportRequest>, mut timelines: Query<&mut Timeline>| {
    ///             if let Ok(mut timeline) = timelines.get_mut(request.timeline)
    ///                 && request.command == TransportCommand::TogglePlay
    ///             {
    ///                 timeline.playing = !timeline.playing;
    ///             }
    ///         })
    ///         .id();
    ///     parent.transport(TransportConfig::new(timeline)).id()
    /// }
    ///
    /// # let mut app = App::new();
    /// # app.add_plugins((MinimalPlugins, touchy::widgets::transport::TransportWidgetPlugin));
    /// # let mut bar = None;
    /// # app.world_mut()
    /// #     .commands()
    /// #     .spawn(Node::default())
    /// #     .with_children(|parent| bar = Some(spawn_replay(parent)));
    /// # app.update();
    /// # let world = app.world_mut();
    /// # assert_eq!(world.query::<&TransportButton>().iter(world).count(), 5);
    /// ```
    fn transport(
        &mut self,
        config: crate::widgets::transport::TransportConfig,
    ) -> EntityCommands<'_>;
}

impl SpawnUi for ChildSpawnerCommands<'_> {
    fn text(&mut self, words: impl Into<String>) -> EntityCommands<'_> {
        self.spawn(primitives::text(words))
    }
    fn heading(&mut self, words: impl Into<String>) -> EntityCommands<'_> {
        self.spawn(primitives::heading(words))
    }
    fn small(&mut self, words: impl Into<String>) -> EntityCommands<'_> {
        self.spawn(primitives::small(words))
    }
    fn divider(&mut self) -> EntityCommands<'_> {
        self.spawn(primitives::Divider)
    }
    fn spacer(&mut self) -> EntityCommands<'_> {
        self.spawn(primitives::Spacer)
    }
    fn bullets(
        &mut self,
        items: impl IntoIterator<Item = impl Into<String>>,
    ) -> EntityCommands<'_> {
        primitives::spawn_list(self, items, false)
    }
    fn numbered(
        &mut self,
        items: impl IntoIterator<Item = impl Into<String>>,
    ) -> EntityCommands<'_> {
        primitives::spawn_list(self, items, true)
    }
    fn row(&mut self, build: impl FnOnce(&mut ChildSpawnerCommands)) -> EntityCommands<'_> {
        let mut row = self.spawn(primitives::Row);
        row.with_children(build);
        row
    }
    fn column(&mut self, build: impl FnOnce(&mut ChildSpawnerCommands)) -> EntityCommands<'_> {
        let mut column = self.spawn(primitives::Column);
        column.with_children(build);
        column
    }
    fn button(&mut self, config: crate::widgets::button::ButtonConfig) -> EntityCommands<'_> {
        crate::widgets::button::spawn_button(self, config)
    }
    fn canvas(&mut self, config: crate::canvas::CanvasConfig) -> EntityCommands<'_> {
        crate::canvas::spawn_canvas(self, config)
    }
    fn capsule_flip(
        &mut self,
        config: crate::widgets::capsule_flip::CapsuleFlipConfig,
    ) -> EntityCommands<'_> {
        crate::widgets::capsule_flip::spawn_capsule_flip(self, config)
    }
    fn checkbox(&mut self, config: crate::widgets::checkbox::CheckboxConfig) -> EntityCommands<'_> {
        crate::widgets::checkbox::spawn_checkbox(self, config)
    }
    fn combo(&mut self, config: crate::widgets::combo::ComboConfig) -> EntityCommands<'_> {
        crate::widgets::combo::spawn_combo(self, config)
    }
    fn dpad(&mut self, config: crate::widgets::dpad::DpadConfig) -> EntityCommands<'_> {
        crate::widgets::dpad::spawn_dpad(self, config)
    }
    fn dropdown(&mut self, config: crate::widgets::dropdown::DropdownConfig) -> EntityCommands<'_> {
        crate::widgets::dropdown::spawn_dropdown(self, config)
    }
    fn gearshift(
        &mut self,
        config: crate::widgets::gearshift::GearshiftConfig,
    ) -> EntityCommands<'_> {
        crate::widgets::gearshift::spawn_gearshift(self, config)
    }
    fn grabber(&mut self, config: crate::widgets::grabber::GrabberConfig) -> EntityCommands<'_> {
        crate::widgets::grabber::spawn_grabber(self, config)
    }
    fn grid(&mut self, config: crate::widgets::grid::GridConfig) -> EntityCommands<'_> {
        crate::widgets::grid::spawn_grid(self, config)
    }
    fn joystick(&mut self, config: crate::widgets::joystick::JoystickConfig) -> EntityCommands<'_> {
        crate::widgets::joystick::spawn_joystick(self, config)
    }
    fn menu(&mut self, config: crate::widgets::menu::MenuConfig) -> EntityCommands<'_> {
        crate::widgets::menu::spawn_menu(self, config)
    }
    fn meter(&mut self, config: crate::readout::meter::MeterConfig) -> EntityCommands<'_> {
        crate::readout::meter::spawn_meter(self, config)
    }
    fn number_input(
        &mut self,
        config: crate::widgets::number_input::NumberInputConfig,
    ) -> EntityCommands<'_> {
        crate::widgets::number_input::spawn_number_input(self, config)
    }
    fn panel(
        &mut self,
        config: crate::containers::panel::PanelConfig,
        build: impl FnOnce(&mut ChildSpawnerCommands),
    ) -> EntityCommands<'_> {
        crate::containers::panel::spawn_panel(self, config, build)
    }
    fn plot(&mut self, config: crate::readout::plot::PlotConfig) -> EntityCommands<'_> {
        crate::readout::plot::spawn_plot(self, config)
    }
    fn popover(
        &mut self,
        config: crate::popover::PopoverConfig,
        build: impl FnOnce(&mut ChildSpawnerCommands),
    ) -> EntityCommands<'_> {
        crate::popover::spawn_popover(self, config, build)
    }
    fn radial(&mut self, config: crate::widgets::radial::RadialConfig) -> EntityCommands<'_> {
        crate::widgets::radial::spawn_radial(self, config)
    }
    fn radial_button(
        &mut self,
        config: crate::widgets::radial::RadialConfig,
    ) -> EntityCommands<'_> {
        crate::widgets::radial::spawn_radial_button(self, config)
    }
    fn scroll_area(
        &mut self,
        config: crate::containers::scroll_area::ScrollAreaConfig,
        build: impl FnOnce(&mut ChildSpawnerCommands),
    ) -> EntityCommands<'_> {
        crate::containers::scroll_area::spawn_scroll_area(self, config, build)
    }
    fn scrubber(&mut self, config: crate::widgets::scrubber::ScrubberConfig) -> EntityCommands<'_> {
        crate::widgets::scrubber::spawn_scrubber(self, config)
    }
    fn section(
        &mut self,
        config: crate::containers::section::SectionConfig,
        build: impl FnOnce(&mut ChildSpawnerCommands),
    ) -> EntityCommands<'_> {
        crate::containers::section::spawn_section(self, config, build)
    }
    fn segmented(
        &mut self,
        config: crate::widgets::segmented::SegmentedConfig,
    ) -> EntityCommands<'_> {
        crate::widgets::segmented::spawn_segmented(self, config)
    }
    fn slider(&mut self, config: crate::widgets::slider::SliderConfig) -> EntityCommands<'_> {
        crate::widgets::slider::spawn_slider(self, config)
    }
    fn text_input(
        &mut self,
        config: crate::widgets::text_input::TextInputConfig,
    ) -> EntityCommands<'_> {
        crate::widgets::text_input::spawn_text_input(self, config)
    }
    fn toolbar(
        &mut self,
        config: crate::containers::toolbar::ToolbarConfig,
        bar: impl FnOnce(&mut ChildSpawnerCommands),
        body: impl FnOnce(&mut ChildSpawnerCommands),
    ) -> EntityCommands<'_> {
        crate::containers::toolbar::spawn_toolbar(self, config, bar, body)
    }
    fn transport(
        &mut self,
        config: crate::widgets::transport::TransportConfig,
    ) -> EntityCommands<'_> {
        crate::widgets::transport::spawn_transport(self, config)
    }
}

/// Changes some of an entity's layout fields and keeps the rest:
/// `.node(|node| node.width = percent(100.0))`.
pub trait NodePatch {
    /// Changes the fields `patch` sets on the entity's [`Node`], if it has one. A widget
    /// keeps some fields up to date itself, such as a panel's position, and those win.
    fn node(&mut self, patch: impl FnOnce(&mut Node) + Send + Sync + 'static) -> &mut Self;
}

impl NodePatch for EntityCommands<'_> {
    fn node(&mut self, patch: impl FnOnce(&mut Node) + Send + Sync + 'static) -> &mut Self {
        self.entry::<Node>()
            .and_modify(move |mut node| patch(&mut node));
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A patch changes only the fields it names.
    #[test]
    fn a_patch_keeps_the_fields_it_doesnt_name() {
        let mut app = App::new();
        let mut row = None;
        app.world_mut()
            .commands()
            .spawn(Node::default())
            .with_children(|page| {
                let mut spawned = page.row(|_| {});
                spawned.node(|node| node.width = percent(100.0));
                row = Some(spawned.id());
            });
        app.update();
        let node = app.world().get::<Node>(row.unwrap()).unwrap();
        assert_eq!(node.width, percent(100.0));
        assert_eq!(node.flex_direction, FlexDirection::Row, "still a row");
    }
}
