//! Compiles touchy's premade themes into the crate. Every file in each
//! `premade_themes/<name>/` folder goes in when the `theme-<name>` feature is
//! on, so a picture or font added to a theme ships with it, with no ledger to keep.

use std::fmt::Write;
use std::path::{Path, PathBuf};
use std::{env, fs};

/// The folder of premade themes, one folder each, at the crate root.
const FOLDER: &str = "premade_themes";
/// The file written for `src/premade.rs` to include.
const OUTPUT: &str = "premade_themes.rs";

fn main() {
    println!("cargo:rerun-if-changed={FOLDER}");
    let root = Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap()).join(FOLDER);
    let mut files = String::from("&[\n");
    for theme in sorted_entries(&root)
        .into_iter()
        .filter(|path| path.is_dir())
    {
        let name = theme.file_name().unwrap().to_string_lossy().into_owned();
        let feature = format!(
            "CARGO_FEATURE_THEME_{}",
            name.to_uppercase().replace('-', "_")
        );
        if env::var_os(feature).is_none() {
            continue;
        }
        for file in files_in(&theme) {
            let inside = file
                .strip_prefix(&root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            writeln!(
                files,
                "    ({inside:?}, include_bytes!({:?})),",
                file.display().to_string()
            )
            .unwrap();
        }
    }
    files.push(']');
    let out = Path::new(&env::var("OUT_DIR").unwrap()).join(OUTPUT);
    fs::write(out, files).unwrap();
}

/// Every file in `folder` and the folders inside it, skipping hidden ones, in order.
fn files_in(folder: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for path in sorted_entries(folder) {
        if path.is_dir() {
            files.extend(files_in(&path));
        } else {
            files.push(path);
        }
    }
    files
}

/// What `folder` holds, hidden entries left out, sorted so builds are repeatable.
fn sorted_entries(folder: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries
        .map(|entry| entry.unwrap().path())
        .filter(|path| !path.file_name().unwrap().to_string_lossy().starts_with('.'))
        .collect();
    paths.sort();
    paths
}
