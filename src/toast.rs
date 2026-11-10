//! Toasts: short notifications that stack in a corner and expire on their own.
//!
//! Show one from anywhere with `commands.trigger(ShowToast::error("Save failed"))`,
//! or `world.trigger(...)` in tests. No need to find a container first.

use std::time::Duration;

use bevy::prelude::*;

use crate::EnsurePlugin;
use crate::layers::{FeedbackLayerRoot, LayersPlugin};
use crate::theme::palette::{
    CONTROL_CORNER_RADIUS, CONTROL_TEXT_SIZE, DANGER, FONT, INFO, SUCCESS, WARNING,
};
use crate::theme::skins;
use crate::theme::{
    Shade, Theme, ThemeBorder, ThemeCornerRadius, ThemeFont, ThemeFontSize, ThemePlugin, ThemeSkin,
    ThemeTextColor, ThemeToken,
};

/// Prefix of the toasts' theme tokens.
pub const TOAST_STYLE: &str = "touchy.toast";
/// Every toast's font size.
pub const TOAST_FONT_SIZE: ThemeToken = ThemeToken::new_static("touchy.toast.font_size");
/// Every toast's font.
pub const TOAST_FONT: ThemeToken = ThemeToken::new_static("touchy.toast.font");
/// The radius of every toast's corners.
pub const TOAST_CORNER_RADIUS: ThemeToken = ThemeToken::new_static("touchy.toast.corner_radius");

/// How long a toast stays up unless it asks otherwise.
const DEFAULT_DURATION: Duration = Duration::from_secs(4);
/// Oldest toasts are removed beyond this, so a burst of errors can't fill the screen.
const MAX_VISIBLE: usize = 5;
/// Distance of the toast stack from the bottom-right corner.
const STACK_INSET: f32 = 20.0;
const STACK_GAP: f32 = 8.0;
const MIN_WIDTH: f32 = 240.0;
const MAX_WIDTH: f32 = 420.0;
const PADDING: Vec2 = Vec2::new(12.0, 8.0);
const BORDER: f32 = 1.5;

/// How much lighter than the palette's danger fill an error's border is,
/// to read as a signal like the other kinds' borders.
const ERROR_LIFT: f32 = 0.17;
/// Each kind's background: dark and only slightly tinted, which no shade of
/// the border gives, as darkening keeps a colour's strength.
const ERROR_BG: Color = Color::srgb(0.25, 0.08, 0.08);
const WARNING_BG: Color = Color::srgb(0.25, 0.22, 0.08);
const INFO_BG: Color = Color::srgb(0.08, 0.15, 0.25);
const SUCCESS_BG: Color = Color::srgb(0.08, 0.25, 0.12);
/// How much lighter than its border a toast's text is.
const TEXT_LIFT: f32 = 0.22;

/// A toast is a short notification that appears in the corner of the screen
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ToastKind {
    /// Something has gone wrong and the user should know.
    Error,
    /// Something is not right, but the user can continue.
    Warning,
    /// Something the user should know, but not a problem.
    Info,
    /// Something has gone right and the user should know.
    Success,
}

impl ToastKind {
    const ALL: [Self; 4] = [Self::Error, Self::Warning, Self::Info, Self::Success];

    /// The kind's name in its tokens: `touchy.toast.error.bg`, and so on.
    fn name(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Info => "info",
            Self::Success => "success",
        }
    }

    /// Gives this kind's border and background their default colours.
    fn default_colors(self, tokens: &ToastTokens, theme: &mut Theme) {
        let (border, bg) = (tokens.border.clone(), tokens.bg.clone());
        match self {
            Self::Error => theme
                .default_derived(border, DANGER, Shade::Lighter(ERROR_LIFT))
                .default_color(bg, ERROR_BG),
            Self::Warning => theme
                .default_derived(border, WARNING, Shade::Same)
                .default_color(bg, WARNING_BG),
            Self::Info => theme
                .default_derived(border, INFO, Shade::Same)
                .default_color(bg, INFO_BG),
            Self::Success => theme
                .default_derived(border, SUCCESS, Shade::Same)
                .default_color(bg, SUCCESS_BG),
        };
    }
}

/// Theme tokens for a kind of toast.
#[derive(Component, Clone, PartialEq, Debug)]
#[component(immutable)]
pub struct ToastTokens {
    /// The toast's background.
    pub bg: ThemeToken,
    /// The line around the toast.
    pub border: ThemeToken,
    /// The toast's text.
    pub text: ThemeToken,
}

impl ToastTokens {
    /// Names the tokens under `prefix`.
    pub fn with_prefix(prefix: &str) -> Self {
        let token = |suffix: &str| ThemeToken::new(format!("{prefix}.{suffix}"));
        Self {
            bg: token("bg"),
            border: token("border"),
            text: token("text"),
        }
    }

    /// The tokens for `kind`: `touchy.toast.error.bg`, and so on.
    pub fn of(kind: ToastKind) -> Self {
        Self::with_prefix(&format!("{TOAST_STYLE}.{}", kind.name()))
    }

    /// Derives the text from the border, unless the app sets it.
    pub fn register_theme_defaults(&self, theme: &mut Theme) {
        theme.default_derived(
            self.text.clone(),
            self.border.clone(),
            Shade::Lighter(TEXT_LIFT),
        );
        // Each part draws its role's skin, once a theme gives the role one.
        theme.default_skin_from(self.bg.clone(), skins::SURFACE);
    }
}

/// Trigger this to show a toast.
#[derive(Event, Clone, Debug, PartialEq)]
pub struct ShowToast {
    /// The kind of toast: error, warning, info or success.
    pub kind: ToastKind,
    /// The words to show.
    pub text: String,
    /// How long the toast stays up. Defaults to four seconds.
    pub duration: Duration,
}

impl ShowToast {
    /// Creates a toast of `kind` with `text`, and the default duration.
    pub fn new(kind: ToastKind, text: impl Into<String>) -> Self {
        Self {
            kind,
            text: text.into(),
            duration: DEFAULT_DURATION,
        }
    }
    /// Creates a toast of `kind` with `text`, and the default duration.
    pub fn error(text: impl Into<String>) -> Self {
        Self::new(ToastKind::Error, text)
    }
    /// Creates a toast of `kind` with `text`, and the default duration.
    pub fn warning(text: impl Into<String>) -> Self {
        Self::new(ToastKind::Warning, text)
    }
    /// Creates a toast of `kind` with `text`, and the default duration.
    pub fn info(text: impl Into<String>) -> Self {
        Self::new(ToastKind::Info, text)
    }
    /// Creates a toast of `kind` with `text`, and the default duration.
    pub fn success(text: impl Into<String>) -> Self {
        Self::new(ToastKind::Success, text)
    }
    /// Sets how long the toast stays up. Defaults to four seconds.
    pub fn duration(mut self, duration: Duration) -> Self {
        self.duration = duration;
        self
    }
}

/// A toast on screen, and the time it has left.
#[derive(Component, Debug, Clone)]
pub struct Toast {
    kind: ToastKind,
    remaining: Timer,
    /// The node showing its words.
    text: Entity,
}

impl Toast {
    /// What kind of toast it is.
    pub fn kind(&self) -> ToastKind {
        self.kind
    }
}

/// The stack toasts appear in, inside the Feedback layer.
#[derive(Component, Debug, Clone, Copy)]
struct ToastStack;

/// Plugin that ensures the Feedback layer exists and shows toasts in it.
pub struct ToastPlugin;

impl Plugin for ToastPlugin {
    fn build(&self, app: &mut App) {
        app.ensure_plugin(ThemePlugin)
            .ensure_plugin(LayersPlugin)
            .add_observer(spawn_toast_stack)
            .add_observer(show_toast)
            .add_observer(style_toast)
            .add_systems(Update, expire_toasts);

        let mut theme = app.world_mut().resource_mut::<Theme>();
        for kind in ToastKind::ALL {
            let tokens = ToastTokens::of(kind);
            kind.default_colors(&tokens, &mut theme);
            tokens.register_theme_defaults(&mut theme);
        }
        theme
            .default_size_from(TOAST_FONT_SIZE, CONTROL_TEXT_SIZE)
            .default_font_from(TOAST_FONT, FONT)
            .default_size_from(TOAST_CORNER_RADIUS, CONTROL_CORNER_RADIUS);
    }
}

fn spawn_toast_stack(layer: On<Add<FeedbackLayerRoot>>, mut commands: Commands) {
    commands.entity(layer.entity).with_child((
        ToastStack,
        Pickable::IGNORE,
        Node {
            position_type: PositionType::Absolute,
            bottom: px(STACK_INSET),
            right: px(STACK_INSET),
            flex_direction: FlexDirection::ColumnReverse,
            row_gap: px(STACK_GAP),
            align_items: AlignItems::End,
            ..default()
        },
    ));
}

/// Adds the toast to the stack, removing the oldest if the stack is full.
fn show_toast(
    show: On<ShowToast>,
    stacks: Query<(Entity, Option<&Children>), With<ToastStack>>,
    mut commands: Commands,
) {
    let Ok((stack, toasts)) = stacks.single() else {
        warn!(
            "toast {:?} dropped: no toast stack (is there a FeedbackLayerRoot?)",
            show.text
        );
        return;
    };
    if let Some(toasts) = toasts {
        // Children are in arrival order, so the oldest come first.
        let excess = (toasts.len() + 1).saturating_sub(MAX_VISIBLE);
        for oldest in toasts.iter().take(excess) {
            commands.entity(oldest).despawn();
        }
    }

    let toast = commands
        .spawn((
            ChildOf(stack),
            // Toasts inform; they never take clicks meant for what's underneath.
            Pickable::IGNORE,
            Node {
                min_width: px(MIN_WIDTH),
                max_width: px(MAX_WIDTH),
                padding: UiRect::axes(px(PADDING.x), px(PADDING.y)),
                border: UiRect::all(px(BORDER)),
                align_items: AlignItems::Center,
                ..default()
            },
            ThemeCornerRadius(TOAST_CORNER_RADIUS),
        ))
        .id();
    let text = commands
        .spawn((
            ChildOf(toast),
            Pickable::IGNORE,
            Text::new(show.text.clone()),
            ThemeFontSize(TOAST_FONT_SIZE),
            ThemeFont(TOAST_FONT),
        ))
        .id();
    // Its colours come with its tokens, in `style_toast`.
    commands.entity(toast).insert((
        Toast {
            kind: show.kind,
            remaining: Timer::new(show.duration, TimerMode::Once),
            text,
        },
        ToastTokens::of(show.kind),
    ));
}

/// Colours a toast when its tokens are set.
fn style_toast(
    insert: On<Insert<ToastTokens>>,
    toasts: Query<(&Toast, &ToastTokens)>,
    mut commands: Commands,
) {
    let Ok((toast, tokens)) = toasts.get(insert.entity) else {
        return;
    };
    commands.entity(insert.entity).insert((
        ThemeSkin(tokens.bg.clone()),
        ThemeBorder::all(tokens.border.clone()),
    ));
    commands
        .entity(toast.text)
        .insert(ThemeTextColor(tokens.text.clone()));
}

/// Counts down in real time, so toasts still expire while the game is paused.
fn expire_toasts(
    time: Res<Time<Real>>,
    mut toasts: Query<(Entity, &mut Toast)>,
    mut commands: Commands,
) {
    for (entity, mut toast) in &mut toasts {
        if toast.remaining.tick(time.delta()).is_finished() {
            commands.entity(entity).despawn();
        }
    }
}

#[cfg(test)]
mod tests {
    use bevy::time::TimeUpdateStrategy;

    use super::*;
    use crate::theme::ThemePlugin;

    const FRAME: Duration = Duration::from_millis(500);

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, ThemePlugin, ToastPlugin))
            .insert_resource(TimeUpdateStrategy::ManualDuration(FRAME));
        app.update();
        app
    }

    fn toast_count(app: &mut App) -> usize {
        let world = app.world_mut();
        world.query::<&Toast>().iter(world).count()
    }

    #[test]
    fn toasts_appear_styled_by_kind_and_expire() {
        let mut app = app();
        app.world_mut()
            .trigger(ShowToast::error("Save failed").duration(Duration::from_secs(1)));
        app.update();

        let world = app.world_mut();
        let background = world
            .query_filtered::<&ThemeSkin, With<Toast>>()
            .single(world)
            .unwrap();
        assert_eq!(background.0, ToastTokens::of(ToastKind::Error).bg);

        for _ in 0..3 {
            app.update();
        }
        assert_eq!(
            toast_count(&mut app),
            0,
            "toast should expire after its duration"
        );
    }

    #[test]
    fn a_toasts_colours_follow_its_kind_and_the_palette() {
        let mut app = app();
        app.world_mut().trigger(ShowToast::success("Saved"));
        app.update();
        let world = app.world_mut();
        let (toast, border) = world
            .query::<(&Toast, &ThemeBorder)>()
            .single(world)
            .unwrap();
        assert_eq!(toast.kind(), ToastKind::Success);
        assert_eq!(
            *border,
            ThemeBorder::all(ToastTokens::of(ToastKind::Success).border)
        );
        let text = toast.text;
        assert_eq!(
            world.get::<ThemeTextColor>(text),
            Some(&ThemeTextColor(ToastTokens::of(ToastKind::Success).text))
        );

        // An error's border and text derive from the palette's danger colour.
        let error = ToastTokens::of(ToastKind::Error);
        let colors = |app: &App| {
            let theme = app.world().resource::<Theme>();
            (theme.color(&error.border), theme.color(&error.text))
        };
        let before = colors(&app);
        app.world_mut()
            .resource_mut::<Theme>()
            .set_color(DANGER, Color::srgb(0.1, 0.1, 0.6));
        let after = colors(&app);
        assert!(after.0 != before.0 && after.1 != before.1);
    }

    #[test]
    fn a_burst_of_toasts_keeps_only_the_newest() {
        let mut app = app();
        // Spread over frames, and all in one frame, as an error firing every
        // frame or a loop of failures would produce.
        for i in 0..MAX_VISIBLE + 3 {
            app.world_mut()
                .trigger(ShowToast::info(format!("toast {i}")));
            app.update();
        }
        assert_eq!(toast_count(&mut app), MAX_VISIBLE);
        for i in 0..MAX_VISIBLE + 3 {
            app.world_mut()
                .commands()
                .trigger(ShowToast::info(format!("burst {i}")));
        }
        app.update();
        assert_eq!(toast_count(&mut app), MAX_VISIBLE);
    }
}
