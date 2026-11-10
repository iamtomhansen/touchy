//! What a token is: the name (or key) to a themed value. Tokens are looked up
//! in the [`Theme`](super::Theme) resource, which holds the values set in code,
//! the active [`ThemeSheet`](super::ThemeSheet), and the plugins' defaults.

use std::borrow::Cow;
use std::fmt;
use std::str::FromStr;

use bevy::prelude::*;

/// An interaction state a widget can be in. A token in a state, such as
/// `BG.with_state(WidgetState::Hover)`, needs no value of its own: the theme
/// shades the stateless token by its shade for that state.
///
/// A widget is in one state at a time, picked by [`WidgetFlags::state`].
/// Lasting looks, such as a checked box or a chosen cell, are tokens of their
/// own (`box.checked`), so they take states too: `box.checked:hover`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Reflect)]
pub enum WidgetState {
    /// Cursor is over the widget.
    Hover,
    /// Widget has the keyboard or gamepad focus, and shows it: the focus a
    /// click gives is shown only by fields that are typed into.
    Focused,
    /// Widget is being clicked or tapped.
    Pressed,
    /// Widget is disabled and can't be interacted with.
    Disabled,
}

impl WidgetState {
    /// Every state, for parsing and error messages.
    pub(super) const ALL: [Self; 4] = [Self::Hover, Self::Focused, Self::Pressed, Self::Disabled];

    /// How the state is written after a token's name: `hover`.
    fn name(self) -> &'static str {
        match self {
            Self::Hover => "hover",
            Self::Focused => "focused",
            Self::Pressed => "pressed",
            Self::Disabled => "disabled",
        }
    }
}

/// What's true of a widget, from which its [`WidgetState`] is picked. Leave
/// out what a widget can't be: `WidgetFlags { hovered, ..default() }`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WidgetFlags {
    /// The cursor is over it.
    pub hovered: bool,
    /// It shows the focus: see [`FocusVisible`](super::FocusVisible).
    pub focused: bool,
    /// It's being clicked, tapped or held.
    pub pressed: bool,
    /// It can't be used.
    pub disabled: bool,
}

impl WidgetFlags {
    /// The one state the widget shows, or `None` at rest. Disabled outranks
    /// pressed, which outranks focused, which outranks hovered, so the focus
    /// stays in sight while the cursor passes over.
    pub fn state(self) -> Option<WidgetState> {
        if self.disabled {
            Some(WidgetState::Disabled)
        } else if self.pressed {
            Some(WidgetState::Pressed)
        } else if self.focused {
            Some(WidgetState::Focused)
        } else if self.hovered {
            Some(WidgetState::Hover)
        } else {
            None
        }
    }
}

impl fmt::Display for WidgetState {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Parses `hover`, `focused`, `pressed` or `disabled`.
impl FromStr for WidgetState {
    type Err = ParseTokenError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|state| state.name() == text)
            .ok_or_else(|| ParseTokenError::UnknownState(text.to_owned()))
    }
}

/// Why text isn't a valid [`ThemeToken`] or [`WidgetState`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParseTokenError {
    /// Nothing before the state, as in `""` or `":hover"`.
    EmptyName,
    /// The text after the colon isn't a state.
    UnknownState(String),
}

impl fmt::Display for ParseTokenError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::EmptyName => f.write_str("the token has no name"),
            Self::UnknownState(state) => {
                write!(f, "`{state}` isn't a state; expected one of ")?;
                for (i, known) in WidgetState::ALL.into_iter().enumerate() {
                    let separator = if i == 0 { "" } else { ", " };
                    write!(f, "{separator}`{known}`")?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for ParseTokenError {}

/// The name of a themed value. Define tokens as constants next to the widget
/// that uses them, namespaced by crate: `ThemeToken::new_static("touchy.button.bg")`.
#[derive(Clone, PartialEq, Eq, Hash, Debug, Reflect)]
pub struct ThemeToken {
    name: Cow<'static, str>,
    state: Option<WidgetState>,
}

impl ThemeToken {
    /// Creates a token with a static name and no state. Use this for constants.
    pub const fn new_static(name: &'static str) -> Self {
        Self {
            name: Cow::Borrowed(name),
            state: None,
        }
    }

    /// Creates a token with an owned name and no state. Use this for dynamic names.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: Cow::Owned(name.into()),
            state: None,
        }
    }

    /// This token as it looks in `state`, replacing any state it already had.
    /// Pass `None`, as [`WidgetFlags::state`] gives at rest, for the resting token.
    pub fn with_state(self, state: impl Into<Option<WidgetState>>) -> Self {
        Self {
            state: state.into(),
            ..self
        }
    }

    /// The state this token is in, if any, and the same token without it.
    pub(super) fn split_state(&self) -> Option<(WidgetState, Self)> {
        let state = self.state?;
        let stateless = Self {
            name: self.name.clone(),
            state: None,
        };
        Some((state, stateless))
    }
}

/// `touchy.button.bg`, or `touchy.button.bg:hover` in a state.
impl fmt::Display for ThemeToken {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.state {
            Some(state) => write!(f, "{}:{state}", self.name),
            None => f.write_str(&self.name),
        }
    }
}

/// Parses what [`Display`](fmt::Display) writes: a name, then optionally a
/// colon and a state, as in `touchy.button.bg:hover`. Theme files name tokens this way.
impl FromStr for ThemeToken {
    type Err = ParseTokenError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let (name, state) = match text.split_once(':') {
            Some((name, state)) => (name, Some(state.parse()?)),
            None => (text, None),
        };
        if name.is_empty() {
            return Err(ParseTokenError::EmptyName);
        }
        Ok(Self::new(name).with_state(state))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BG: ThemeToken = ThemeToken::new_static("test.bg");

    /// A state makes a distinct token, so it can have its own value.
    #[test]
    fn states_make_distinct_tokens() {
        assert_ne!(BG.with_state(WidgetState::Hover), BG);
        assert_ne!(
            BG.with_state(WidgetState::Hover),
            BG.with_state(WidgetState::Pressed)
        );
        assert_eq!(
            BG.with_state(WidgetState::Hover)
                .with_state(WidgetState::Pressed),
            BG.with_state(WidgetState::Pressed)
        );
    }

    #[test]
    fn no_state_returns_the_resting_token() {
        assert_eq!(BG.with_state(WidgetState::Hover).with_state(None), BG);
    }

    #[test]
    fn disabled_outranks_pressed_outranks_focused_outranks_hovered() {
        use WidgetState::*;
        let all = WidgetFlags {
            hovered: true,
            focused: true,
            pressed: true,
            disabled: true,
        };
        assert_eq!(all.state(), Some(Disabled));
        let enabled = WidgetFlags {
            disabled: false,
            ..all
        };
        assert_eq!(enabled.state(), Some(Pressed));
        let released = WidgetFlags {
            pressed: false,
            ..enabled
        };
        assert_eq!(released.state(), Some(Focused));
        let hovered = WidgetFlags {
            hovered: true,
            ..default()
        };
        assert_eq!(hovered.state(), Some(Hover));
        assert_eq!(WidgetFlags::default().state(), None);
    }

    #[test]
    fn splitting_a_state_returns_the_stateless_token() {
        assert_eq!(BG.split_state(), None);
        assert_eq!(
            BG.with_state(WidgetState::Disabled).split_state(),
            Some((WidgetState::Disabled, BG))
        );
    }

    #[test]
    fn display_shows_the_state_after_a_colon() {
        assert_eq!(BG.to_string(), "test.bg");
        assert_eq!(
            BG.with_state(WidgetState::Hover).to_string(),
            "test.bg:hover"
        );
    }

    #[test]
    fn parsing_reverses_display() {
        let mut tokens = vec![BG];
        tokens.extend(WidgetState::ALL.map(|state| BG.with_state(state)));
        for token in tokens {
            assert_eq!(token.to_string().parse(), Ok(token));
        }
    }

    #[test]
    fn parsing_rejects_unknown_states_and_empty_names() {
        assert_eq!(
            "test.bg:hovered".parse::<ThemeToken>(),
            Err(ParseTokenError::UnknownState("hovered".into()))
        );
        assert_eq!(
            "test.bg:hover:pressed".parse::<ThemeToken>(),
            Err(ParseTokenError::UnknownState("hover:pressed".into()))
        );
        assert_eq!("".parse::<ThemeToken>(), Err(ParseTokenError::EmptyName));
        assert_eq!(
            ":hover".parse::<ThemeToken>(),
            Err(ParseTokenError::EmptyName)
        );
    }

    #[test]
    fn unknown_states_list_the_real_ones() {
        let message = ParseTokenError::UnknownState("hovered".into()).to_string();
        assert_eq!(
            message,
            "`hovered` isn't a state; expected one of `hover`, `focused`, `pressed`, `disabled`"
        );
    }
}
