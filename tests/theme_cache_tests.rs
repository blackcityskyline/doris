//! The theme list, and the cache in front of it. `load_themes` parsed forty files on every
//! call, and the Options modal calls it twice per keypress -- so a key that changed nothing
//! spent about eight milliseconds re-parsing files that cannot have changed.

use doris::ui::theme::Theme;
use std::path::PathBuf;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("doris-themecache-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

const MINIMAL: &str = r#"name = "probe"
main_bg.r = 1
main_bg.g = 2
main_bg.b = 3
main_fg.r = 200
main_fg.g = 200
main_fg.b = 200
title.r = 210
title.g = 210
title.b = 210
hi_fg.r = 220
hi_fg.g = 220
hi_fg.b = 220
selected_bg.r = 4
selected_bg.g = 5
selected_bg.b = 6
selected_fg.r = 230
selected_fg.g = 230
selected_fg.b = 230
inactive_fg.r = 7
inactive_fg.g = 7
inactive_fg.b = 7
div_line.r = 8
div_line.g = 8
div_line.b = 8
graph_text.r = 9
graph_text.g = 9
graph_text.b = 9
menu_fg.r = 10
menu_fg.g = 10
menu_fg.b = 10
menu_selected_bg.r = 11
menu_selected_bg.g = 11
menu_selected_bg.b = 11
menu_selected_fg.r = 12
menu_selected_fg.g = 12
menu_selected_fg.b = 12
"#;

#[test]
fn the_uncached_entry_point_sees_a_new_directory_each_time() {
    // This is what a test depends on, and what a cache on the default
    let first = scratch("a");
    std::fs::write(first.join("probe.toml"), MINIMAL).unwrap();

    let a = Theme::load_themes_from(Some(&first));
    assert!(
        a.iter().any(|t| t.name == "probe"),
        "the theme in the directory must load: {} bundled",
        a.len()
    );

    let second = scratch("b");
    std::fs::write(second.join("other.toml"), MINIMAL.replace("probe", "other")).unwrap();
    let b = Theme::load_themes_from(Some(&second));
    assert!(
        !b.iter().any(|t| t.name == "probe"),
        "the first directory's theme must not leak into the second"
    );
    assert!(b.iter().any(|t| t.name == "other"));
}

#[test]
fn a_broken_theme_file_does_not_take_the_rest_with_it() {
    let dir = scratch("broken");
    std::fs::write(dir.join("probe.toml"), MINIMAL).unwrap();
    std::fs::write(dir.join("junk.toml"), "this is not toml = = =").unwrap();

    let themes = Theme::load_themes_from(Some(&dir));

    assert!(themes.len() >= 41, "40 bundled plus the good one");
    assert!(themes.iter().any(|t| t.name == "probe"));
}

#[test]
fn the_cached_list_agrees_with_itself() {
    // Not a timing assertion -- timings are not tests. This is the
    let a = Theme::load_themes();
    let b = Theme::load_themes();
    assert_eq!(a.len(), b.len());
    let names_a: Vec<&str> = a.iter().map(|t| t.name.as_str()).collect();
    let names_b: Vec<&str> = b.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names_a, names_b, "the same list, in the same order");
    assert!(!names_a.is_empty(), "there are themes to cycle");
}
