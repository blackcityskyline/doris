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

/// Verbatim output of the noctalia template engine for the doris theme template -- kept here as
/// the format contract between the two tools.
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
menu_fg.r = 222
menu_fg.g = 228
menu_fg.b = 225
menu_selected_bg.r = 132
menu_selected_bg.g = 214
menu_selected_bg.b = 195
menu_selected_fg.r = 0
menu_selected_fg.g = 56
menu_selected_fg.b = 47
primary.r = 228
primary.g = 144
primary.b = 160
error.r = 224
error.g = 108
error.b = 117
on_hover.r = 250
on_hover.g = 200
on_hover.b = 190
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
    let noctalia = themes
        .iter()
        .find(|t| t.name == "noctalia")
        .expect("rendered noctalia theme must be loaded");

    assert_eq!(noctalia.main_bg.r, 14);
    assert_eq!(noctalia.main_bg.g, 21);
    assert_eq!(noctalia.main_bg.b, 19);
    assert_eq!(noctalia.main_fg.r, 222);
    // The four optional accents are what a rendered theme is actually
    assert_eq!(
        noctalia.primary_color(),
        ratatui::style::Color::Rgb(228, 144, 160)
    );
    assert_eq!(
        noctalia.error_color(),
        ratatui::style::Color::Rgb(224, 108, 117)
    );
    assert_eq!(
        noctalia.on_hover_color(),
        ratatui::style::Color::Rgb(250, 200, 190)
    );
}

/// A theme file that still spells out the fields `Theme` no longer has must still load.
#[test]
fn test_a_theme_from_an_older_build_still_loads() {
    let mut file = String::from("name = \"stale\"\n");
    for f in [
        "main_bg",
        "main_fg",
        "title",
        "hi_fg",
        "selected_bg",
        "selected_fg",
        "inactive_fg",
        "div_line",
        "graph_text",
        "menu_fg",
        "menu_selected_bg",
        "menu_selected_fg",
    ] {
        file.push_str(&format!("{f}.r = 10\n{f}.g = 20\n{f}.b = 30\n"));
    }
    // The fields this build dropped, still spelled out.
    for f in [
        "meter_bg",
        "search_box",
        "log_box",
        "player_box",
        "menu_bg",
        "gradient_start",
        "gradient_mid",
        "gradient_end",
    ] {
        file.push_str(&format!("{f}.r = 99\n{f}.g = 99\n{f}.b = 99\n"));
    }

    let dir = temp_theme_dir("stale");
    write_theme(&dir, "stale.toml", &file);

    let themes = Theme::load_themes_from(Some(&dir));
    let stale = themes
        .iter()
        .find(|t| t.name == "stale")
        .expect("a theme written by an older build must still load");

    assert_eq!(stale.main_bg.b, 30);
    assert_eq!(stale.main_fg.r, 10);
}

/// The fields that were dropped were never read by anything, so a theme that stops naming them
/// draws identically.
#[test]
fn test_zone_border_colours_do_not_come_from_the_dropped_fields() {
    let theme = Theme::dark();
    // The focused zone and the unfocused ones differ, which is what makes
    let focused = doris::ui::layout::zone_border_color(
        doris::ui::layout::ZoneId::Results,
        doris::ui::layout::ZoneId::Results,
        &theme,
    );
    let unfocused = doris::ui::layout::zone_border_color(
        doris::ui::layout::ZoneId::Results,
        doris::ui::layout::ZoneId::Log,
        &theme,
    );
    assert_ne!(focused, unfocused, "the focus must be visible");
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
    assert!(
        layered.iter().any(|t| t.name == "default"),
        "bundled themes must survive next to user themes"
    );
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
    let default_theme = layered
        .iter()
        .find(|t| t.name == "default")
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

/// End-to-end check against the file noctalia actually generated on this machine.
#[test]
fn test_generated_noctalia_theme_on_disk_is_valid() {
    let Some(home) = dirs::home_dir() else {
        return;
    };
    let path = home
        .join(".config")
        .join("doris")
        .join("themes")
        .join("noctalia.toml");
    if !path.exists() {
        return;
    }

    let theme = Theme::from_config(&path).expect("noctalia-generated theme must parse as a Theme");
    assert_eq!(theme.name, "noctalia");
}
