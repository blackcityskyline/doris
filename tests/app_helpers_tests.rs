use doris::app::{cycle_index, resolve_cookie_file, source_needs_browser};
use doris::config::Config;
use std::path::PathBuf;

// --- source_needs_browser (fixes B0.1: streaming ignored item.source) -------

#[test]
fn test_rutor_rows_do_not_need_the_browser() {
    assert!(!source_needs_browser("rutor"));
}

#[test]
fn test_rutracker_rows_need_the_browser() {
    assert!(source_needs_browser("rutracker"));
}

#[test]
fn test_legacy_and_unknown_sources_fall_back_to_the_browser() {
    // Results fetched before the `source` field existed deserialize to "",
    // and any future browser-backed source should default to the same
    // client the old hardcoded path always used.
    assert!(source_needs_browser(""));
    assert!(source_needs_browser("1337x"));
}

// --- cycle_index (fixes: Left/Right in Options both cycling forward) -----

#[test]
fn test_cycle_index_forward_wraps() {
    assert_eq!(cycle_index(0, 3, 1), 1);
    assert_eq!(cycle_index(1, 3, 1), 2);
    assert_eq!(cycle_index(2, 3, 1), 0); // wraps
}

#[test]
fn test_cycle_index_backward_wraps() {
    assert_eq!(cycle_index(2, 3, -1), 1);
    assert_eq!(cycle_index(1, 3, -1), 0);
    assert_eq!(cycle_index(0, 3, -1), 2); // wraps the other way
}

#[test]
fn test_cycle_index_forward_and_backward_are_inverses() {
    for len in 2..8 {
        for pos in 0..len {
            let forward = cycle_index(pos, len, 1);
            assert_eq!(cycle_index(forward, len, -1), pos, "len={} pos={}", len, pos);
        }
    }
}

#[test]
fn test_cycle_index_empty_list_never_panics() {
    assert_eq!(cycle_index(0, 0, 1), 0);
    assert_eq!(cycle_index(0, 0, -1), 0);
}

#[test]
fn test_cycle_index_single_item_stays_put() {
    assert_eq!(cycle_index(0, 1, 1), 0);
    assert_eq!(cycle_index(0, 1, -1), 0);
}

// --- resolve_cookie_file (fixes: config.toml's cookie_file being dead) ---

#[test]
fn test_cookie_file_disabled_when_save_cookies_off() {
    let mut config = Config::default();
    config.save_cookies = false;
    assert_eq!(resolve_cookie_file(&config, Some(std::path::Path::new("/tmp/x.txt"))), None);
}

#[test]
fn test_cookie_file_falls_back_to_config_toml_setting() {
    // This is the actual regression: previously only the CLI flag was
    // ever read, so with no --cookie-file given, login always ran with
    // no cookie file at all regardless of what config.toml said.
    let mut config = Config::default();
    config.save_cookies = true;
    config.cookie_file = "my-cookies.txt".to_string();
    assert_eq!(resolve_cookie_file(&config, None), Some(PathBuf::from("my-cookies.txt")));
}

#[test]
fn test_cookie_file_cli_flag_takes_priority_over_config() {
    let mut config = Config::default();
    config.save_cookies = true;
    config.cookie_file = "config-cookies.txt".to_string();
    let cli_path = PathBuf::from("/explicit/cli-cookies.txt");
    assert_eq!(resolve_cookie_file(&config, Some(&cli_path)), Some(cli_path));
}

#[test]
fn test_cookie_file_default_config_value_is_usable() {
    let config = Config::default();
    assert!(config.save_cookies);
    let resolved = resolve_cookie_file(&config, None);
    assert_eq!(resolved, Some(PathBuf::from("cookies.txt")));
}
