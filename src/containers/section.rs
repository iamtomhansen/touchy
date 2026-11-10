//! Collapsible sections: a header you click to open/close a body.

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui::{Pressed, UiSystems};
use bevy::ui_widgets::{Activate, Button, ValueChange};

use crate::EnsurePlugin;
use crate::cursor::{HoverCursor, HoverCursorPlugin};
use crate::motion::{ResponseCurve, Tween};
use crate::theme::palette::{
    CONTROL, CONTROL_CORNER_RADIUS, CONTROL_TEXT_SIZE, FONT, TEXT, TEXT_MUTED,
};
use crate::theme::skins;
use crate::theme::{
    FocusVisible, Shade, Theme, ThemeCornerRadius, ThemeFont, ThemeFontSize, ThemePlugin,
    ThemeSkin, ThemeTextColor, ThemeToken, WidgetFlags, WidgetState,
};
use crate::widgets::chevron::{
    Chevron, ChevronPlugin, ChevronSystems, ChevronTurn, chevron_colour, spawn_chevron,
};

/// Prefix of the section container's theme tokens.
pub const SECTION_STYLE: &str = "touchy.section";

/// Seconds to open or close fully; a partial move takes its share.
const DEFAULT_DURATION: f32 = 0.18;
/// Fast start, gentle landing.
const DEFAULT_CURVE: ResponseCurve = ResponseCurve::Power(0.5);
/// The padding around the header in pixels.
const HEADER_PADDING: Vec2 = Vec2::new(4.0, 2.0);
/// The gap between the header's parts in pixels.
const HEADER_GAP: f32 = 6.0;
/// Space between the header and the body.
const BODY_GAP: f32 = 4.0;

/// Theme tokens for a section container. When collapsed, the header is the title plus the chevron.
#[derive(Component, Clone, PartialEq, Debug)]
#[component(immutable)]
pub struct SectionTokens {
    /// The title's color.
    pub title: ThemeToken,
    /// The title's font size.
    pub title_size: ThemeToken,
    /// The title's font face.
    pub font: ThemeToken,
    /// The chevron's color.
    pub chevron: ThemeToken,
    /// The header's background color.
    pub header_bg: ThemeToken,
    /// The header box's corner radius.
    pub corner_radius: ThemeToken,
}

impl SectionTokens {
    /// Names the tokens under `prefix`.
    pub fn with_prefix(prefix: &str) -> Self {
        let token = |suffix: &str| ThemeToken::new(format!("{prefix}.{suffix}"));
        Self {
            title: token("title"),
            title_size: token("title_size"),
            font: token("font"),
            chevron: token("chevron"),
            header_bg: token("header_bg"),
            corner_radius: token("corner_radius"),
        }
    }
    /// Derives these tokens from the palette, unless the app sets them.
    pub fn register_theme_defaults(&self, theme: &mut Theme) {
        let active = |state| self.header_bg.clone().with_state(state);
        theme
            .default_derived(self.title.clone(), TEXT, Shade::Same)
            .default_derived(self.chevron.clone(), TEXT_MUTED, Shade::Same)
            .default_color(self.header_bg.clone(), Color::NONE)
            .default_derived(active(WidgetState::Hover), CONTROL, Shade::Same)
            .default_derived(active(WidgetState::Pressed), CONTROL, Shade::Lighter(0.1))
            .default_size_from(self.title_size.clone(), CONTROL_TEXT_SIZE)
            .default_font_from(self.font.clone(), FONT)
            .default_size_from(self.corner_radius.clone(), CONTROL_CORNER_RADIUS);
        // Each part draws its role's skin, once a theme gives the role one.
        theme.default_skin_from(self.header_bg.clone(), skins::RAISED);
    }
}

/// The built-in token set.
impl Default for SectionTokens {
    fn default() -> Self {
        Self::with_prefix(SECTION_STYLE)
    }
}

/// Whether the section is open. Set it to open or close the section; clicking
/// the header flips it.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SectionOpen(pub bool);

/// The section's title. Set it to retitle the section.
#[derive(Component, Clone, Debug, Default, PartialEq, Eq)]
pub struct SectionTitle(pub String);

/// How far open the section is drawn, from 0 to 1, and its move under way.
#[derive(Component, Clone, Copy, Debug)]
struct Openness {
    shown: f32,
    moving: Option<Tween<f32>>,
    /// The state last announced, so writing the same state again isn't a change.
    settled_on: bool,
    motion: Option<ResponseCurve>,
}

/// The parts the section moves and styles.
#[derive(Component, Clone, Copy, Debug)]
struct SectionParts {
    header: Entity,
    title: Entity,
    chevron: Option<Entity>,
    /// Clips the body to how far open the section is.
    inner: Entity,
    body: Entity,
}

/// A section's header, and the section it opens.
#[derive(Component, Clone, Copy, Debug)]
struct HeaderOf(Entity);

/// Construction-time configuration for a collapsible section container.
///
/// Consumed by [`SpawnUi::section`](crate::spawn_ui::SpawnUi::section) and not retained as a
/// component.
///
/// # Examples
/// ```
/// use touchy::prelude::*;
///
/// let config = SectionConfig::new("My Section")
///     .open(false)
///     .motion(Some(ResponseCurve::Power(0.5)));
///
/// # assert_eq!(config.title, "My Section");
/// # assert!(!config.open);
/// ```
#[derive(Clone, Debug)]
pub struct SectionConfig {
    /// The text to show in the header.
    pub title: String,
    /// Whether or not the section is open when spawned.
    pub open: bool,
    /// The curve the section opens and closes along; `None` snaps.
    pub motion: Option<ResponseCurve>,
    /// Whether the header shows a chevron that turns as the section opens.
    pub chevron: bool,
    /// The section's theme tokens.
    pub tokens: SectionTokens,
}

impl SectionConfig {
    /// A section titled `title`, open until told otherwise.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            open: true,
            motion: Some(DEFAULT_CURVE),
            chevron: true,
            tokens: SectionTokens::with_prefix(SECTION_STYLE),
        }
    }
    /// Set whether the section is open when spawned.
    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }
    /// The curve to open and close along, or `None` to snap.
    pub fn motion(mut self, curve: Option<ResponseCurve>) -> Self {
        self.motion = curve;
        self
    }
    /// Set whether the header shows a chevron.
    pub fn chevron(mut self, chevron: bool) -> Self {
        self.chevron = chevron;
        self
    }
    /// Set the theme tokens for the collapsible section.
    pub fn tokens(mut self, tokens: SectionTokens) -> Self {
        self.tokens = tokens;
        self
    }
}

/// Spawns it; documented on [`SpawnUi::section`](crate::spawn_ui::SpawnUi::section).
pub(crate) fn spawn_section<'a>(
    parent: &'a mut ChildSpawnerCommands,
    config: SectionConfig,
    build: impl FnOnce(&mut ChildSpawnerCommands),
) -> EntityCommands<'a> {
    let SectionConfig {
        title,
        open,
        motion,
        chevron,
        tokens,
    } = config;
    let shown = if open { 1.0 } else { 0.0 };
    let mut section = parent.spawn((
        SectionOpen(open),
        SectionTitle(title.clone()),
        Openness {
            shown,
            moving: None,
            settled_on: open,
            motion,
        },
        Node {
            flex_direction: FlexDirection::Column,
            ..default()
        },
    ));
    let root = section.id();
    // The parts' colours and fonts come with the section's tokens, in `style_sections`.
    let (mut header, mut title_text, mut chevron_mark) =
        (Entity::PLACEHOLDER, Entity::PLACEHOLDER, None);
    let (mut inner, mut body) = (Entity::PLACEHOLDER, Entity::PLACEHOLDER);
    section.with_children(|section| {
        let mut header_node = section.spawn((
            HeaderOf(root),
            Button,
            Hovered::default(),
            TabIndex(0),
            HoverCursor::POINTER,
            Node {
                align_items: AlignItems::Center,
                column_gap: px(HEADER_GAP),
                padding: UiRect::axes(px(HEADER_PADDING.x), px(HEADER_PADDING.y)),
                ..default()
            },
        ));
        header = header_node.id();
        header_node.with_children(|header| {
            if chevron {
                chevron_mark = Some(spawn_chevron(header, UiRect::ZERO, ChevronTurn::FOLD, open));
            }
            title_text = header.spawn((Pickable::IGNORE, Text::new(title))).id();
        });
        let mut inner_node = section.spawn(Node {
            flex_direction: FlexDirection::Column,
            overflow: Overflow::clip(),
            display: if open { Display::Flex } else { Display::None },
            ..default()
        });
        inner = inner_node.id();
        inner_node.with_children(|inner| {
            // It keeps its own height inside the inner node, so it can be measured mid-move.
            body = inner
                .spawn(Node {
                    flex_direction: FlexDirection::Column,
                    flex_shrink: 0.0,
                    padding: UiRect::top(px(BODY_GAP)),
                    ..default()
                })
                .with_children(build)
                .id();
        });
    });
    section.insert((
        SectionParts {
            header,
            title: title_text,
            chevron: chevron_mark,
            inner,
            body,
        },
        tokens,
    ));
    section
}

/// Puts each changed title in its header.
fn show_titles(
    sections: Query<(&SectionTitle, &SectionParts), Changed<SectionTitle>>,
    mut texts: Query<&mut Text>,
) {
    for (title, parts) in &sections {
        if let Ok(mut text) = texts.get_mut(parts.title)
            && text.0 != title.0
        {
            text.0.clone_from(&title.0);
        }
    }
}

/// Opens or closes the section whose header was activated.
fn toggle_from_header(
    activate: On<Activate>,
    headers: Query<&HeaderOf>,
    mut sections: Query<&mut SectionOpen>,
) {
    if let Ok(&HeaderOf(section)) = headers.get(activate.entity)
        && let Ok(mut open) = sections.get_mut(section)
    {
        open.0 = !open.0;
    }
}

/// Starts a move toward each changed state, and announces it. A newly
/// spawned section already shows its state, so it neither moves nor announces.
fn start_moves(
    mut sections: Query<(Entity, &SectionOpen, &mut Openness), Changed<SectionOpen>>,
    mut commands: Commands,
) {
    for (section, open, mut openness) in &mut sections {
        // A new section is spawned settled on its state.
        if open.0 == openness.settled_on {
            continue;
        }
        openness.settled_on = open.0;
        let target = if open.0 { 1.0 } else { 0.0 };
        let distance = (target - openness.shown).abs();
        openness.moving = match openness.motion {
            Some(curve) if distance > 0.0 => Some(Tween::new(
                openness.shown,
                target,
                DEFAULT_DURATION * distance,
                curve,
            )),
            _ => {
                openness.shown = target;
                None
            }
        };
        commands.trigger(ValueChange {
            source: section,
            value: open.0,
            is_final: true,
        });
    }
}

/// Moves each section along. Real time, so a paused game's menus still move.
fn advance_moves(time: Res<Time<Real>>, mut sections: Query<&mut Openness>) {
    for mut openness in &mut sections {
        let Some(mut tween) = openness.moving else {
            continue;
        };
        let (shown, arrived) = tween.advance(time.delta_secs());
        openness.shown = shown;
        openness.moving = (!arrived).then_some(tween);
    }
}

/// Sizes each section's inner node to how open it is, and turns its chevron.
fn draw_openness(
    sections: Query<(&Openness, &SectionParts), Changed<Openness>>,
    measured: Query<&ComputedNode>,
    mut nodes: Query<&mut Node>,
    mut chevrons: Query<&mut Chevron>,
) {
    for (openness, parts) in &sections {
        let body_height = measured
            .get(parts.body)
            .map_or(0.0, |body| body.size.y * body.inverse_scale_factor);
        if let Ok(mut inner) = nodes.get_mut(parts.inner) {
            let (display, height) = match (openness.moving, openness.shown >= 1.0) {
                (None, true) => (Display::Flex, Val::Auto),
                (None, false) => (Display::None, Val::Auto),
                (Some(_), _) => (Display::Flex, px(body_height * openness.shown)),
            };
            if inner.display != display {
                inner.display = display;
            }
            if inner.height != height {
                inner.height = height;
            }
        }
        // The chevron turns with the body, partway back included.
        if let Some(chevron) = parts.chevron
            && let Ok(mut chevron) = chevrons.get_mut(chevron)
        {
            chevron.follow(openness.shown);
        }
    }
}

/// What a header's look is picked from, and its current background.
type HeaderLook = (
    Entity,
    &'static HeaderOf,
    &'static Hovered,
    Has<FocusVisible>,
    Has<Pressed>,
    &'static ThemeSkin,
);

/// Shows the header's background while it's hovered, focused or pressed.
fn restyle_headers(
    headers: Query<HeaderLook>,
    sections: Query<&SectionTokens>,
    mut commands: Commands,
) {
    for (entity, &HeaderOf(section), hovered, focused, pressed, old) in &headers {
        let Ok(tokens) = sections.get(section) else {
            continue;
        };
        let flags = WidgetFlags {
            hovered: hovered.0,
            focused,
            pressed,
            disabled: false,
        };
        let state = flags.state();
        let wanted = ThemeSkin(tokens.header_bg.clone().with_state(state));
        if *old != wanted {
            commands.entity(entity).insert(wanted);
        }
    }
}

/// Restyle a collapsible section container when its tokens change.
fn style_section(
    insert: On<Insert<SectionTokens>>,
    sections: Query<(&SectionParts, &SectionTokens)>,
    mut commands: Commands,
) {
    // Tokens on an entity that isn't a section: nothing to style.
    let Ok((parts, tokens)) = sections.get(insert.entity) else {
        return;
    };
    // Its resting colour; `restyle_headers` lights it while hovered, focused or pressed.
    commands.entity(parts.header).insert((
        ThemeSkin(tokens.header_bg.clone()),
        ThemeCornerRadius(tokens.corner_radius.clone()),
    ));
    commands.entity(parts.title).insert((
        ThemeTextColor(tokens.title.clone()),
        ThemeFontSize(tokens.title_size.clone()),
        ThemeFont(tokens.font.clone()),
    ));
    if let Some(chevron) = parts.chevron {
        commands
            .entity(chevron)
            .insert(chevron_colour(&tokens.chevron));
    }
}

/// Section folding and default token values.
pub struct SectionPlugin;

impl Plugin for SectionPlugin {
    fn build(&self, app: &mut App) {
        app.ensure_plugin(HoverCursorPlugin)
            .ensure_plugin(ThemePlugin)
            .ensure_plugin(ChevronPlugin)
            .add_observer(toggle_from_header)
            .add_observer(style_section)
            .add_systems(Update, (start_moves, advance_moves).chain())
            .add_systems(
                PostUpdate,
                (draw_openness, show_titles, restyle_headers)
                    .before(UiSystems::Prepare)
                    .before(ChevronSystems),
            );
        SectionTokens::with_prefix(SECTION_STYLE)
            .register_theme_defaults(&mut app.world_mut().resource_mut::<Theme>());
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy::time::TimeUpdateStrategy;

    use super::*;
    use crate::theme::ThemeBorder;

    /// Frames are this long in these tests: a full move takes a few of them.
    const FRAME_MS: u64 = 40;
    /// The body's laid-out height, set by hand since tests run no layout.
    const BODY_HEIGHT: f32 = 100.0;

    #[derive(Resource, Default)]
    struct Heard(Vec<bool>);

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, SectionPlugin))
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
                FRAME_MS,
            )))
            .init_resource::<Heard>()
            .add_observer(|change: On<ValueChange<bool>>, mut heard: ResMut<Heard>| {
                heard.0.push(change.value)
            });
        app
    }

    fn spawn(app: &mut App, config: SectionConfig) -> Entity {
        let mut section = None;
        app.world_mut()
            .commands()
            .spawn(Node::default())
            .with_children(|parent| {
                section = Some(
                    spawn_section(parent, config, |body| {
                        body.spawn(Text::new("content"));
                    })
                    .id(),
                );
            });
        app.update();
        let section = section.unwrap();
        let body = app.world().get::<SectionParts>(section).unwrap().body;
        app.world_mut()
            .get_mut::<ComputedNode>(body)
            .unwrap()
            .size
            .y = BODY_HEIGHT;
        section
    }

    fn inner(app: &App, section: Entity) -> Node {
        let inner = app.world().get::<SectionParts>(section).unwrap().inner;
        app.world().get::<Node>(inner).unwrap().clone()
    }

    fn header(app: &mut App, section: Entity) -> Entity {
        let world = app.world_mut();
        world
            .query::<(Entity, &HeaderOf)>()
            .iter(world)
            .find(|(_, of)| of.0 == section)
            .unwrap()
            .0
    }

    fn heard(app: &mut App) -> Vec<bool> {
        std::mem::take(&mut app.world_mut().resource_mut::<Heard>().0)
    }

    #[test]
    fn a_section_spawns_in_its_state_without_moving_or_announcing() {
        let mut app = app();
        let open = spawn(&mut app, SectionConfig::new("open"));
        let closed = spawn(&mut app, SectionConfig::new("closed").open(false));
        app.update();
        assert_eq!(
            (inner(&app, open).display, inner(&app, open).height),
            (Display::Flex, Val::Auto)
        );
        assert_eq!(inner(&app, closed).display, Display::None);
        assert!(heard(&mut app).is_empty());
    }

    #[test]
    fn the_header_flips_it_and_each_change_is_announced_once() {
        let mut app = app();
        let section = spawn(&mut app, SectionConfig::new("s"));
        let header = header(&mut app, section);
        app.world_mut().trigger(Activate { entity: header });
        app.update();
        assert_eq!(
            app.world().get::<SectionOpen>(section),
            Some(&SectionOpen(false))
        );
        // Writing the same state again isn't a change.
        app.world_mut().get_mut::<SectionOpen>(section).unwrap().0 = false;
        app.update();
        app.world_mut().trigger(Activate { entity: header });
        app.update();
        assert_eq!(heard(&mut app), [false, true]);
    }

    #[test]
    fn opening_grows_the_inner_node_to_the_body_then_fits_it() {
        let mut app = app();
        let section = spawn(&mut app, SectionConfig::new("s").open(false));
        app.world_mut().get_mut::<SectionOpen>(section).unwrap().0 = true;
        let mut heights = Vec::new();
        for _ in 0..10 {
            app.update();
            heights.push(inner(&app, section).height);
        }
        let growing: Vec<f32> = heights
            .iter()
            .filter_map(|height| match height {
                Val::Px(px) => Some(*px),
                _ => None,
            })
            .collect();
        assert!(growing.len() >= 2, "it takes a few frames: {heights:?}");
        assert!(
            growing.windows(2).all(|pair| pair[0] < pair[1]),
            "and only grows: {growing:?}"
        );
        assert!(growing.iter().all(|&px| (0.0..BODY_HEIGHT).contains(&px)));
        assert_eq!(
            heights.last(),
            Some(&Val::Auto),
            "at rest it fits whatever the body becomes"
        );
        assert_eq!(inner(&app, section).display, Display::Flex);
    }

    #[test]
    fn retitling_a_section_changes_its_header_and_not_its_state() {
        let mut app = app();
        let section = spawn(&mut app, SectionConfig::new("decided tick 40").open(false));
        app.world_mut().get_mut::<SectionOpen>(section).unwrap().0 = true;
        app.update();
        app.world_mut().get_mut::<SectionTitle>(section).unwrap().0 = "decided tick 41".into();
        app.update();
        let title = app.world().get::<SectionParts>(section).unwrap().title;
        assert_eq!(app.world().get::<Text>(title).unwrap().0, "decided tick 41");
        assert!(
            app.world()
                .get::<Openness>(section)
                .unwrap()
                .moving
                .is_some(),
            "still opening"
        );
    }

    #[test]
    fn without_motion_it_snaps() {
        let mut app = app();
        let section = spawn(&mut app, SectionConfig::new("s").motion(None));
        app.world_mut().get_mut::<SectionOpen>(section).unwrap().0 = false;
        app.update();
        app.update();
        assert_eq!(inner(&app, section).display, Display::None);
    }

    #[test]
    fn a_section_may_go_without_its_chevron() {
        let mut app = app();
        let plain = spawn(&mut app, SectionConfig::new("plain").chevron(false));
        let parts = *app.world().get::<SectionParts>(plain).unwrap();
        assert!(parts.chevron.is_none());
        assert_eq!(
            app.world().get::<Children>(parts.header).unwrap().len(),
            1,
            "just the title"
        );
        app.world_mut().get_mut::<SectionOpen>(plain).unwrap().0 = false;
        for _ in 0..10 {
            app.update();
        }
        assert_eq!(inner(&app, plain).display, Display::None, "it still closes");
    }

    #[test]
    fn new_tokens_restyle_the_section_at_once() {
        let mut app = app();
        let section = spawn(&mut app, SectionConfig::new("s"));
        let parts = *app.world().get::<SectionParts>(section).unwrap();
        let title = |app: &App| {
            app.world()
                .get::<ThemeTextColor>(parts.title)
                .unwrap()
                .0
                .clone()
        };
        assert_eq!(title(&app), SectionTokens::with_prefix(SECTION_STYLE).title);
        let custom = SectionTokens::with_prefix("test.section");
        custom.register_theme_defaults(&mut app.world_mut().resource_mut::<Theme>());
        app.world_mut().entity_mut(section).insert(custom.clone());
        app.world_mut().flush();
        assert_eq!(title(&app), custom.title);
        assert_eq!(
            app.world().get::<ThemeBorder>(parts.chevron.unwrap()),
            Some(&ThemeBorder::all(custom.chevron))
        );
    }

    #[test]
    fn a_hovered_header_lights_up() {
        let mut app = app();
        let section = spawn(&mut app, SectionConfig::new("s"));
        let header = header(&mut app, section);
        app.world_mut().entity_mut(header).insert(Hovered(true));
        app.update();
        app.world_mut().flush();
        assert_eq!(
            app.world().get::<ThemeSkin>(header),
            Some(&ThemeSkin(
                SectionTokens::with_prefix(SECTION_STYLE)
                    .header_bg
                    .with_state(WidgetState::Hover)
            ))
        );
    }

    #[test]
    fn the_chevron_turns_with_the_body() {
        let mut app = app();
        let section = spawn(&mut app, SectionConfig::new("s").open(false));
        let chevron = app
            .world()
            .get::<SectionParts>(section)
            .unwrap()
            .chevron
            .unwrap();
        let turn = |app: &App| app.world().get::<UiTransform>(chevron).unwrap().rotation;
        let closed = turn(&app);
        app.world_mut().get_mut::<SectionOpen>(section).unwrap().0 = true;
        app.update();
        app.update();
        let midway = turn(&app);
        for _ in 0..10 {
            app.update();
        }
        let open = turn(&app);
        assert_ne!(closed, open);
        assert!(
            midway != closed && midway != open,
            "partway, as the body is"
        );
    }

    #[test]
    fn turning_back_midway_takes_only_the_way_back() {
        let mut app = app();
        let section = spawn(&mut app, SectionConfig::new("s").open(false));
        app.world_mut().get_mut::<SectionOpen>(section).unwrap().0 = true;
        app.update();
        app.update();
        let shown = app.world().get::<Openness>(section).unwrap().shown;
        assert!(shown > 0.0 && shown < 1.0);
        app.world_mut().get_mut::<SectionOpen>(section).unwrap().0 = false;
        app.update();
        let tween = app
            .world()
            .get::<Openness>(section)
            .unwrap()
            .moving
            .unwrap();
        assert_eq!(tween.target(), 0.0);
        // The way back is shorter than a full close, so it's done sooner.
        let frames = (DEFAULT_DURATION * shown / (FRAME_MS as f32 / 1000.0)).ceil() as usize;
        for _ in 0..=frames {
            app.update();
        }
        assert_eq!(inner(&app, section).display, Display::None);
    }
}
