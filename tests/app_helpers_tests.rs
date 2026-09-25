use doris::app::{apply_search_results, cycle_index, resolve_cookie_file, source_needs_browser};
use doris::config::Config;
use doris::search::models::TorrentItem;
use doris::ui::app::App as UiApp;
use doris::ui::app::AppState;
use std::path::PathBuf;

// --- apply_search_results (fixes B0.2: stale search overwrites fresh) -------

fn make_ui() -> UiApp {
    UiApp::new(
        "http://127.0.0.1:8090".into(),
        "helium [visible] (/usr/bin/helium)".into(),
        true,
        None,
        "/tmp".into(),
        "braille".into(),
        true,
        true,
        true,
        false,
    )
}

fn item(title: &str) -> TorrentItem {
    TorrentItem {
        title: title.to_string(),
        ..Default::default()
    }
}

#[test]
fn test_stale_generation_results_are_dropped() {
    let mut ui = make_ui();
    ui.results = vec![item("fresh")];
    ui.search_offset = ui.results.len();
    ui.state = AppState::Searching;

    // Generation 1 answers after generation 2's search started.
    let applied = apply_search_results(&mut ui, 1, 2, vec![item("stale")]);

    assert!(!applied, "stale results must not be applied");
    assert_eq!(ui.results.len(), 1, "the fresh results must survive");
    assert_eq!(ui.results[0].title, "fresh");
    assert!(ui.state == AppState::Searching, "a stale event must not flip state");
}

#[test]
fn test_current_generation_results_replace_the_list() {
    let mut ui = make_ui();
    ui.results = vec![item("old")];
    ui.state = AppState::Searching;

    let applied = apply_search_results(&mut ui, 3, 3, vec![item("a"), item("b")]);

    assert!(applied);
    assert_eq!(ui.results.len(), 2);
    assert_eq!(ui.results[0].title, "a");
    assert_eq!(ui.selected, 0);
    assert_eq!(ui.search_offset, 2);
    assert!(ui.state == AppState::Idle);
}

#[test]
fn test_current_generation_extends_when_paging() {
    let mut ui = make_ui();
    ui.results = vec![item("page1")];
    ui.search_offset = ui.results.len();
    ui.state = AppState::Searching;

    let applied = apply_search_results(&mut ui, 7, 7, vec![item("page2")]);

    assert!(applied);
    assert_eq!(ui.results.len(), 2, "offset > 0 extends instead of replacing");
    assert_eq!(ui.results[1].title, "page2");
    assert_eq!(ui.search_offset, 2);
}

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
