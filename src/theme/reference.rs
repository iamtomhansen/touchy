//! The reference theme file: every token touchy defines, at its default,
//! written from the code by these tests so it can never fall out of date.
//!
//! When a widget adds or changes a default, the first test fails until the
//! file is regenerated, so the change shows up in review as a diff of the file.

use bevy::prelude::*;

use super::file::tests::Placeholders;
use super::file::{DEFAULT_FONT, TRANSPARENT, read_sheet};
use super::resolve::{ColorSource, Follows, SizeSource};
use super::{Shade, Theme, ThemeSheet};
use crate::TouchyPlugins;

const REFERENCE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/themes/reference.theme.ron"
);
/// Set this to rewrite the file instead of checking it.
const UPDATE_VAR: &str = "TOUCHY_UPDATE_REFERENCE";
const INDENT: &str = "    ";
/// Tokens are grouped under a comment by this many leading name segments,
/// so `touchy.button.primary.bg` goes under `touchy.button`.
const GROUP_SEGMENTS: usize = 2;

const HEADER: &str = "\
// Every touchy theme token at its default. Generated from the code by
// touchy's tests: don't edit it here.
//
// To start a theme, copy this file, change what you want and delete the rest:
// anything a theme leaves out keeps its default. Most tokens derive from the
// `touchy.palette` group, colours and font, so changing those restyles every
// widget; most skins follow the `touchy.skin` roles the same way. Every section
// is optional.
//
// TOKENS
//   \"name\" is a token; \"name:state\" is the token in a state. A widget is in
//   one state at a time, the first of these that's true of it:
//     disabled   it can't be used
//     pressed    it's being clicked, tapped or held
//     focused    the keyboard or a gamepad is on it (text fields: any focus)
//     hover      the cursor is over it
//   A state with no entry of its own is its resting token, shaded by the
//   state's entry in `state_shades`.
//
// colors
//   \"#3873cc\", \"#3873cc80\" with alpha, or \"transparent\"
//   (from: \"token\") follows another colour, with an optional shade:
//     shade: Same          as it is
//     shade: Lighter(0.1)  a tenth of the way to white
//     shade: Darker(0.1)   a tenth of the way to black
//     shade: Alpha(0.5)    the same colour, half see-through
//     shade: Contrast      black or white, whichever reads better on it
// sizes
//   6.0 pixels, or (from: \"token\") with an optional scale: 2.0
// fonts
//   \"Inter.ttf\" beside this file, \"/fonts/Inter.ttf\" from the asset folder,
//   \"default\" for Bevy's own, or (from: \"token\")
// state_shades
//   A state and the shade its tokens take: \"hover\": Lighter(0.08)
// skins
//   (from: \"token\") draws another token's skin, tinted by this one's colour,
//   as every widget's parts follow a `touchy.skin` role. A role has no skin
//   until a theme gives it one, so every widget draws its flat colour.
//   Otherwise, a colour token drawn as a stack of layers, all optional, back to front:
//     shadows: [(color: ..., offset: (0.0, 2.0), blur: 4.0, spread: 1.0)]
//       in pixels, each over the one before; with no offset, a glow
//     fill: a colour under the rest: the token's own, unless there's an image
//     gradients: [...] over the fill, each over the one before
//     border: [...] gradients on the node's border
//     image: \"panel.png\", the picture, beside this file or from \"/...\":
//       slice: 5.0, or (8.0, 3.0) for left and right, top and bottom: its
//         corners keep their size, its edges and middle tile; sides: Stretch,
//         center: Stretch and corner_scale: 2.0 change how
//       tile: X, Y or Both repeats the whole picture
//       frames: (columns: 5, rows: 1, fps: 12.0) cuts a sprite sheet: the
//         widget picks the frame, as a slider by its value, unless fps loops it
//       flip: X, Y or Both mirrors it
//       tinted: false keeps its own colours instead of the token's
//       inside_border: true draws it inside the border
//     outline: (color: ..., width: 2.0, offset: 1.0) a line outside the
//       border, which takes no space
//   and how the layers change into this skin as a widget changes state:
//     transition: 0.15 eases every layer over 0.15 seconds; (shadows: 0.5,
//       outline: 0.2) times each layer on its own, among fill, gradients,
//       border, shadows, outline and tint (the picture's colour), and the
//       rest change at once
//   A gradient is Linear(angle: 180.0, stops: [...]), in degrees clockwise
//   from pointing up (180, down, if left out), or Radial(center: (0.5, 0.5),
//   shape: FarthestCorner, stops: [...]), from fractions of the node's width
//   and height; a shape is FarthestCorner, ClosestCorner, FarthestSide,
//   ClosestSide, Circle(20.0) or Ellipse(30.0, 20.0). A stop is a colour, or
//   (colour, 0.25) to place it a quarter of the way along. A colour on its own
//   is a flat gradient: border: [\"#ffffff\"] is a border colour.
//   A colour in a skin is written as in `colors`, or (shade: Lighter(0.1)) for
//   the skin's own token's colour, so it follows the palette and every state.
//   A state can have a skin of its own, which replaces the resting token's
//   whole; without one, it borrows the resting token's, and every colour in
//   it takes the state's shade. So a transition on the resting skin covers
//   every state that borrows it, and one on a state's own skin covers
//   changing into that state.
//
// For example:
";

/// A small theme, shown in the header after \"For example\", and read by a
/// test, so the example can't fall out of date either.
const EXAMPLE: &str = r##"(
    colors: {
        "touchy.palette.accent": "#5b6fd9",
        "touchy.button.border.light:hover": (from: "touchy.palette.accent", shade: Lighter(0.2)),
    },
    sizes: {
        "touchy.slider.corner_radius": (from: "touchy.button.corner_radius", scale: 2.0),
    },
    skins: {
        "touchy.button.bg": (
            gradients: [Linear(stops: [(shade: Lighter(0.12)), (shade: Darker(0.12))])],
            shadows: [(color: "#00000080", offset: (0.0, 2.0), blur: 4.0)],
        ),
        "touchy.joystick.thumb": (image: "spinner.png", frames: (columns: 8, fps: 12.0)),
        "touchy.slider.thumb:focused": (
            outline: (color: (from: "touchy.palette.accent"), width: 2.0, offset: 1.0),
            shadows: [(color: (from: "touchy.palette.accent", shade: Alpha(0.5)), blur: 8.0)],
            transition: (shadows: 0.5),
        ),
    },
    state_shades: {
        "focused": Lighter(0.15),
    },
)"##;

/// The last header lines: how to regenerate the file.
const FOOTER: &str = "\
//
// Regenerate with: TOUCHY_UPDATE_REFERENCE=1 cargo test -p touchy reference
";

/// The file's opening comment: the syntax, the example, and how to regenerate it.
fn header() -> String {
    let example: String = EXAMPLE
        .lines()
        .map(|line| format!("//{INDENT}{line}\n"))
        .collect();
    format!("{HEADER}{example}{FOOTER}")
}

/// A theme holding every default touchy's plugins register.
fn theme() -> Theme {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, TouchyPlugins));
    app.world().resource::<Theme>().clone()
}

fn defaults() -> ThemeSheet {
    theme().defaults_sheet()
}

/// Writes `sheet` in the `.theme.ron` format, sorted and grouped by token name.
fn write_sheet(sheet: &ThemeSheet) -> String {
    let colors = sheet.colors.iter().map(|(token, source)| {
        let value = match source {
            ColorSource::Value(color) if Srgba::from(*color) == Srgba::NONE => {
                format!("\"{TRANSPARENT}\"")
            }
            ColorSource::Value(color) => format!("\"{}\"", Srgba::from(*color).to_hex()),
            ColorSource::Derived {
                from,
                shade: Shade::Same,
            } => format!("(from: \"{from}\")"),
            ColorSource::Derived { from, shade } => {
                format!("(from: \"{from}\", shade: {})", shade_text(*shade))
            }
        };
        (token.to_string(), value)
    });
    let sizes = sheet.sizes.iter().map(|(token, size)| {
        let value = match size {
            SizeSource::Value(size) => format!("{size:?}"),
            SizeSource::From { from, scale } if *scale == 1.0 => format!("(from: \"{from}\")"),
            SizeSource::From { from, scale } => format!("(from: \"{from}\", scale: {scale:?})"),
        };
        (token.to_string(), value)
    });
    let fonts = sheet.fonts.iter().map(|(token, font)| {
        let value = match font {
            Follows::Value(font) => {
                // A font from a file has no path to write here; touchy's
                // defaults only ever use Bevy's own.
                assert!(
                    *font == Handle::default(),
                    "{token} defaults to a font file; teach `write_sheet` to write font paths"
                );
                format!("\"{DEFAULT_FONT}\"")
            }
            Follows::From(from) => format!("(from: \"{from}\")"),
        };
        (token.to_string(), value)
    });
    let skins = sheet.skins.iter().map(|(token, skin)| {
        let value = match skin {
            // A skin's pictures have no path to write here; touchy's defaults
            // only ever follow a role.
            Follows::Value(_) => {
                panic!("{token} defaults to a skin of its own; teach `write_sheet` to write skins")
            }
            Follows::From(from) => format!("(from: \"{from}\")"),
        };
        (token.to_string(), value)
    });
    let state_shades = sheet
        .state_shades
        .iter()
        .map(|(state, shade)| (state.to_string(), shade_text(*shade)));

    let mut out = format!("{}(\n", header());
    write_section(&mut out, "colors", colors, true);
    write_section(&mut out, "sizes", sizes, true);
    write_section(&mut out, "fonts", fonts, true);
    write_section(&mut out, "skins", skins, true);
    write_section(&mut out, "state_shades", state_shades, false);
    out.push_str(")\n");
    out
}

fn write_section(
    out: &mut String,
    name: &str,
    entries: impl Iterator<Item = (String, String)>,
    grouped: bool,
) {
    let mut entries: Vec<_> = entries.collect();
    entries.sort();
    out.push_str(&format!("{INDENT}{name}: {{\n"));
    let mut current_group = None;
    for (key, value) in &entries {
        if grouped {
            let group = group(key);
            if current_group != Some(group) {
                if current_group.is_some() {
                    out.push('\n');
                }
                out.push_str(&format!("{INDENT}{INDENT}// {group}\n"));
                current_group = Some(group);
            }
        }
        out.push_str(&format!("{INDENT}{INDENT}\"{key}\": {value},\n"));
    }
    out.push_str(&format!("{INDENT}}},\n"));
}

/// The leading name segments a token is grouped by, ignoring any state.
fn group(key: &str) -> &str {
    let name = key.split(':').next().unwrap_or(key);
    match name.match_indices('.').nth(GROUP_SEGMENTS - 1) {
        Some((end, _)) => &name[..end],
        None => name,
    }
}

fn shade_text(shade: Shade) -> String {
    match shade {
        Shade::Same => "Same".into(),
        Shade::Lighter(amount) => format!("Lighter({amount:?})"),
        Shade::Darker(amount) => format!("Darker({amount:?})"),
        Shade::Alpha(alpha) => format!("Alpha({alpha:?})"),
        Shade::Contrast => "Contrast".into(),
    }
}

#[test]
fn the_reference_file_lists_every_default() {
    let expected = write_sheet(&defaults());

    if std::env::var_os(UPDATE_VAR).is_some() {
        std::fs::write(REFERENCE_PATH, &expected).unwrap();
        return;
    }
    let current = std::fs::read_to_string(REFERENCE_PATH).unwrap_or_default();
    assert!(
        current == expected,
        "{REFERENCE_PATH} is out of date with touchy's defaults; regenerate it with \
         `{UPDATE_VAR}=1 cargo test -p touchy reference` and review the diff"
    );
}

/// The file reads back as the same sheet, so copying it changes nothing
/// until an author edits it. Colours are compared as written, since hex
/// rounds them to the nearest 1/255.
#[test]
fn the_reference_file_reads_back_as_the_defaults() {
    let written = write_sheet(&defaults());
    let read = read_sheet(written.as_bytes(), &mut Placeholders::default()).unwrap();
    assert_eq!(write_sheet(&read), written);
}

/// The header's example is a real theme: it reads, and every token it names
/// is one of touchy's.
#[test]
fn the_header_example_reads_and_names_real_tokens() {
    let sheet = read_sheet(EXAMPLE.as_bytes(), &mut Placeholders::default()).unwrap();
    assert_eq!(theme().unread_entries(&sheet), Vec::<String>::new());
    assert_eq!(sheet.skins.len(), 3);
}

#[test]
fn tokens_group_by_their_first_two_segments() {
    assert_eq!(group("touchy.button.primary.bg:hover"), "touchy.button");
    assert_eq!(group("touchy.palette.text.muted"), "touchy.palette");
    assert_eq!(group("short"), "short");
}

/// Every theme file in `folder`, or in a folder inside it.
fn theme_files(folder: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(folder).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files.extend(theme_files(&path));
        } else if path.to_string_lossy().ends_with(".theme.ron") {
            files.push(path);
        }
    }
    files
}

/// The shipped theme files only name touchy tokens that exist, so renaming a
/// token can't silently break them. Tokens of the example's own aren't
/// touchy's to check.
#[test]
fn shipped_theme_files_name_only_real_touchy_tokens() {
    const OWN_PREFIX: &str = "touchy.";
    let theme = theme();
    let crate_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let shipped = theme_files(&crate_root.join("premade_themes"));
    for path in theme_files(&crate_root.join("assets/themes"))
        .into_iter()
        .chain(shipped)
    {
        let bytes = std::fs::read(&path).unwrap();
        let sheet = read_sheet(&bytes, &mut Placeholders::default()).unwrap();
        let unread: Vec<_> = theme
            .unread_entries(&sheet)
            .into_iter()
            .filter(|entry| entry.contains(&format!("`{OWN_PREFIX}")))
            .collect();
        assert!(unread.is_empty(), "{}: {unread:#?}", path.display());
    }
}
