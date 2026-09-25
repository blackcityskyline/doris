use doris::app::{
    EnterAction, apply_search_results, cycle_index, enter_action, resolve_cookie_file,
    source_id_for, source_needs_browser, source_outcome_line,
};
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
    let applied = apply_search_results(&mut ui, 1, 2, vec![item("stale")], false);

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

    let applied = apply_search_results(&mut ui, 3, 3, vec![item("a"), item("b")], false);

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

    let applied = apply_search_results(&mut ui, 7, 7, vec![item("page2")], true);

    assert!(applied);
    assert_eq!(ui.results.len(), 2, "offset > 0 extends instead of replacing");
    assert_eq!(ui.results[1].title, "page2");
    assert_eq!(ui.search_offset, 2);
}

// --- apply_search_results: has_more replaces the `count < 50` guess (B2) -----

#[test]
fn test_last_page_marks_all_loaded() {
    let mut ui = make_ui();
    ui.state = AppState::Searching;

    apply_search_results(&mut ui, 1, 1, vec![item("a")], false);

    assert!(ui.all_loaded, "a source reporting no more pages must stop the pager");
}

#[test]
fn test_a_source_with_more_pages_keeps_loading_available() {
    let mut ui = make_ui();
    ui.state = AppState::Searching;
    // Exactly 50 results -- the size that made the old `count < 50` test
    // look right for rutracker. `has_more` is now what decides.
    let full_page: Vec<TorrentItem> = (0..50).map(|i| item(&format!("row {}", i))).collect();

    apply_search_results(&mut ui, 1, 1, full_page, true);

    assert!(!ui.all_loaded, "a full page must leave Load more available");
}

#[test]
fn test_all_loaded_resets_on_a_fresh_search() {
    let mut ui = make_ui();
    ui.state = AppState::Searching;
    ui.all_loaded = true; // left over from the previous query's last page
    ui.search_offset = 0;

    apply_search_results(&mut ui, 2, 2, vec![item("fresh")], true);

    assert!(!ui.all_loaded, "a new search must not inherit the old query's state");
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

// --- source_id_for (B2: rows route through the registry) -------------------

#[test]
fn test_rows_route_to_their_own_registered_source() {
    assert_eq!(source_id_for(&item_with_source("rutor")), "rutor");
    assert_eq!(source_id_for(&item_with_source("rutracker")), "rutracker");
}

#[test]
fn test_legacy_rows_fall_back_to_rutracker() {
    // Rows serialized before the `source` field existed deserialize to "",
    // and they carry rutracker-shaped URLs -- the same conservative
    // fallback `source_needs_browser` makes.
    assert_eq!(source_id_for(&item_with_source("")), "rutracker");
    assert_eq!(source_id_for(&item_with_source("1337x")), "rutracker");
}

fn item_with_source(source: &str) -> TorrentItem {
    TorrentItem {
        source: source.to_string(),
        ..Default::default()
    }
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

// --- source_outcome_line (fixes B0.3: per-source errors were dropped) -------

#[test]
fn test_outcome_line_reports_a_successful_source() {
    assert_eq!(source_outcome_line("rutor", &Ok(42)), "rutor: 42 results");
}

#[test]
fn test_outcome_line_reports_a_failing_source() {
    assert_eq!(
        source_outcome_line("rutracker", &Err("HTTP 503".to_string())),
        "rutracker: HTTP 503"
    );
}

#[test]
fn test_outcome_line_distinguishes_sources_on_the_same_error() {
    // The whole point of B0.3: when one source fails and the other
    // succeeds, the failure still has to name which source it came from.
    let healthy: Result<usize, String> = Ok(7);
    let broken: Result<usize, String> = Err("timeout".to_string());
    assert_eq!(source_outcome_line("rutor", &healthy), "rutor: 7 results");
    assert_eq!(source_outcome_line("rutracker", &broken), "rutracker: timeout");
}

// --- enter_action (fixes B0.4: Enter on an empty query could stream) --------

#[test]
fn test_enter_on_empty_query_in_input_mode_does_nothing() {
    // The regression: `submit_search()` returned None (having already left
    // input mode) and the same key fell through to submit_selection() and
    // spawned a stream. It must be DoNothing even with results selected
    // and a pending source switch.
    assert_eq!(
        enter_action(true, false, true, true),
        EnterAction::DoNothing
    );
}

#[test]
fn test_enter_with_a_query_submits_the_search() {
    assert_eq!(enter_action(true, true, false, true), EnterAction::SubmitQuery);
    // A pending source switch must not steal Enter from the typed query.
    assert_eq!(enter_action(true, true, true, false), EnterAction::SubmitQuery);
}

#[test]
fn test_enter_plays_outside_input_mode() {
    assert_eq!(enter_action(false, true, false, true), EnterAction::Play);
    assert_eq!(enter_action(false, false, false, true), EnterAction::Play);
}

#[test]
fn test_enter_after_source_switch_restarts_the_search_instead_of_playing() {
    assert_eq!(enter_action(false, true, true, true), EnterAction::RestartSearch);
}

#[test]
fn test_enter_without_a_selection_does_nothing() {
    assert_eq!(enter_action(false, true, false, false), EnterAction::DoNothing);
}
