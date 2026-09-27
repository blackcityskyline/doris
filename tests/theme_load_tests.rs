//! Regression tests for `Theme::load_themes_from` -- the user themes
//! directory layer on top of `BUNDLED_THEMES`.
//!
//! This is the mechanism that lets noctalia generate a theme for doris
//! without the theme ever being committed to this repo: noctalia renders
//! `~/.config/noctalia/user-templates/doris/noctalia-theme.toml` into
//! `~/.config/doris/themes/noctalia.toml` (`noctalia msg templates-apply`)
//! and doris picks it up on the next run.

use doris::ui::theme::Theme;
use std::path::{Path, PathBuf};

/// Verbatim output of the noctalia template engine for the doris theme
/// template -- kept here as the format contract between the two tools.
/// Field names and nesting (`field.r` / `field.g` / `field.b`) must match
/// `Theme`/`ColorDef` exactly, otherwise the generated theme is silently
/// dropped by `load_themes` (it parses to `None`).
const NOCTALIA_RENDERED: &str = r#"name = "noctalia"

main_bg.r = 14
main_bg.g = 21
main_bg.b = 19
main_fg.r = 222
main_fg.g = 228
main_fg.b = 225
title.r = 132
title.g = 214
title.b = 195
hi_fg.r = 171
hi_fg.g = 202
hi_fg.b = 228
selected_bg.r = 63
selected_bg.g = 73
selected_bg.b = 70
selected_fg.r = 190
selected_fg.g = 201
selected_fg.b = 197
inactive_fg.r = 190
inactive_fg.g = 201
inactive_fg.b = 197
div_line.r = 137
div_line.g = 147
div_line.b = 143
graph_text.r = 177
graph_text.g = 204
graph_text.b = 196
meter_bg.r = 63
meter_bg.g = 73
meter_bg.b = 70
search_box.r = 132
search_box.g = 214
search_box.b = 195
log_box.r = 171
log_box.g = 202
log_box.b = 228
player_box.r = 177
player_box.g = 204
player_box.b = 196
menu_bg.r = 14
menu_bg.g = 21
menu_bg.b = 19
menu_fg.r = 222
menu_fg.g = 228
menu_fg.b = 225
menu_selected_bg.r = 132
menu_selected_bg.g = 214
menu_selected_bg.b = 195
menu_selected_fg.r = 0
menu_selected_fg.g = 56
menu_selected_fg.b = 47
gradient_start.r = 132
gradient_start.g = 214
gradient_start.b = 195
gradient_mid.r = 177
gradient_mid.g = 204
gradient_mid.b = 196
gradient_end.r = 171
gradient_end.g = 202
gradient_end.b = 228
"#;

/// A fresh per-test directory under the system temp dir, emptied first so
/// a leftover run can never make a test pass on stale data.
fn temp_theme_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("doris_theme_{}_{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp theme dir");
    dir
}

fn write_theme(dir: &Path, file: &str, contents: &str) {
    std::fs::write(dir.join(file), contents).expect("write theme file");
}

/// The noctalia-rendered file must parse as a `Theme` with every field
/// filled in -- a rename on either side leaves the theme silently missing
/// from the list ("Color theme 1/1" forever).
#[test]
fn test_rendered_noctalia_theme_parses() {
    let dir = temp_theme_dir("rendered");
    write_theme(&dir, "noctalia.toml", NOCTALIA_RENDERED);

    let themes = Theme::load_themes_from(Some(&dir));
    let noctalia = themes.iter().find(|t| t.name == "noctalia")
        .expect("rendered noctalia theme must be loaded");

    assert_eq!(noctalia.main_bg.r, 14);
    assert_eq!(noctalia.main_bg.g, 21);
    assert_eq!(noctalia.main_bg.b, 19);
    assert_eq!(noctalia.main_fg.r, 222);
    assert_eq!(noctalia.gradient_end.b, 228);
}

/// User themes are appended to the bundled list rather than replacing it,
/// and the result stays sorted by name so the theme cycler is stable.
#[test]
fn test_user_theme_dir_layers_on_bundled() {
    let dir = temp_theme_dir("layered");
    write_theme(&dir, "noctalia.toml", NOCTALIA_RENDERED);

    let bundled_only = Theme::load_themes_from(None);
    let layered = Theme::load_themes_from(Some(&dir));

    assert_eq!(layered.len(), bundled_only.len() + 1);
    assert!(layered.iter().any(|t| t.name == "default"),
        "bundled themes must survive next to user themes");
    assert!(layered.iter().any(|t| t.name == "noctalia"));

    let names: Vec<&str> = layered.iter().map(|t| t.name.as_str()).collect();
    let mut sorted = names.clone();
    sorted.sort_unstable();
    assert_eq!(names, sorted);
}

/// A user file whose `name` matches a bundled theme replaces that theme
/// instead of duplicating it -- how users override a shipped palette.
#[test]
fn test_user_theme_overrides_bundled_same_name() {
    let dir = temp_theme_dir("override");
    let override_toml = NOCTALIA_RENDERED.replace("name = \"noctalia\"", "name = \"default\"");
    write_theme(&dir, "default.toml", &override_toml);

    let bundled_only = Theme::load_themes_from(None);
    let layered = Theme::load_themes_from(Some(&dir));

    assert_eq!(layered.len(), bundled_only.len());
    let default_theme = layered.iter().find(|t| t.name == "default")
        .expect("default theme must still be present");
    // Bundled default has main_bg 10/22/40, the override has 14/21/19.
    assert_eq!(default_theme.main_bg.r, 14);
    assert_eq!(default_theme.main_bg.g, 21);
    assert_eq!(default_theme.main_bg.b, 19);
}

/// Unparsable files in the user dir are skipped, never fatal -- one broken
/// theme must not take the whole theme list down with it.
#[test]
fn test_broken_user_theme_is_skipped() {
    let dir = temp_theme_dir("broken");
    write_theme(&dir, "noctalia.toml", NOCTALIA_RENDERED);
    write_theme(&dir, "garbage.toml", "this is not toml at all {{{");

    let bundled_only = Theme::load_themes_from(None);
    let layered = Theme::load_themes_from(Some(&dir));

    assert_eq!(layered.len(), bundled_only.len() + 1);
    assert!(layered.iter().any(|t| t.name == "noctalia"));
}

/// End-to-end check against the file noctalia actually generated on this
/// machine. Skipped where it does not exist (CI / fresh installs) so the
/// suite stays green anywhere; when present it is the real proof that the
/// two tools still agree.
#[test]
fn test_generated_noctalia_theme_on_disk_is_valid() {
    let Some(home) = dirs::home_dir() else {
        return;
    };
    let path = home.join(".config").join("doris").join("themes").join("noctalia.toml");
    if !path.exists() {
        return;
    }

    let theme = Theme::from_config(&path)
        .expect("noctalia-generated theme must parse as a Theme");
    assert_eq!(theme.name, "noctalia");
}
