use doris::app::{
    EnterAction, apply_source_done, cycle_index, enter_action, finish_search, resolve_cookie_file,
    source_id_for, source_needs_browser, source_outcome_line,
};
use doris::config::Config;
use doris::search::models::TorrentItem;
use doris::ui::app::App as UiApp;
use doris::ui::app::AppState;
use std::collections::HashMap;
use std::path::PathBuf;

// --- apply_source_done (B0.2 stale-drop; B3 per-source arrival) -------------

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
    ui.state = AppState::Searching;

    // Generation 1 answers after generation 2's search started.
    let applied = apply_source_done(&mut ui, 1, 2, "rutor", vec![item("stale")], None);

    assert!(!applied, "stale results must not be applied");
    assert_eq!(ui.results.len(), 1, "the fresh results must survive");
    assert_eq!(ui.results[0].title, "fresh");
    assert!(ui.state == AppState::Searching, "a stale event must not flip state");
}

#[test]
fn test_rows_append_and_the_outcome_line_is_logged() {
    let mut ui = make_ui();
    ui.results = vec![item("already here")];
    ui.state = AppState::Searching;

    let applied = apply_source_done(&mut ui, 3, 3, "rutor", vec![item("a"), item("b")], None);

    assert!(applied);
    assert_eq!(ui.results.len(), 3, "each source appends into the same list");
    assert_eq!(ui.results[2].title, "b");
    assert!(
        ui.logs.iter().any(|l| l.contains("rutor: 2 results")),
        "the per-source outcome line must reach the log: {:?}",
        ui.logs
    );
    assert!(
        ui.state == AppState::Searching,
        "one source answering must not end the search others are still in"
    );
}

#[test]
fn test_a_failing_source_logs_its_line_without_adding_rows() {
    // B0.3: a source that fails still reports, naming itself -- even
    // though it brings no rows.
    let mut ui = make_ui();
    ui.results = vec![item("kept")];
    ui.state = AppState::Searching;

    let applied = apply_source_done(
        &mut ui,
        3,
        3,
        "rutracker",
        vec![],
        Some("timed out after 25s"),
    );

    assert!(applied, "a failed source still counts as an answer");
    assert_eq!(ui.results.len(), 1, "a failure adds no rows");
    assert!(ui.results[0].title == "kept");
    assert!(
        ui.logs.iter().any(|l| l.contains("rutracker: timed out after 25s")),
        "the failure line must reach the log: {:?}",
        ui.logs
    );
}

// --- finish_search (B3: the generation is over when every source answered) --

#[test]
fn test_completion_marks_all_loaded_when_no_source_has_more() {
    let mut ui = make_ui();
    ui.state = AppState::Searching;
    let has_more: HashMap<String, bool> = HashMap::new();

    assert!(finish_search(&mut ui, 1, 1, &has_more));
    assert!(ui.all_loaded, "no source with another page means stop paging");
    assert!(ui.state == AppState::Idle);
}

#[test]
fn test_completion_keeps_paging_open_while_any_source_has_more() {
    let mut ui = make_ui();
    ui.state = AppState::Searching;
    let mut has_more = HashMap::new();
    has_more.insert("rutor".to_string(), false);
    has_more.insert("rutracker".to_string(), true);

    assert!(finish_search(&mut ui, 1, 1, &has_more));
    assert!(!ui.all_loaded, "one source with another page keeps Load more alive");
    assert!(ui.state == AppState::Idle);
}

#[test]
fn test_stale_completion_does_not_flip_a_newer_search_idle() {
    let mut ui = make_ui();
    ui.state = AppState::Searching;

    assert!(!finish_search(&mut ui, 1, 2, &HashMap::new()));
    assert!(
        ui.state == AppState::Searching,
        "a superseded generation must not end the newer search"
    );
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

// --- B4: dedup + default order applied when a generation finishes ------------

fn row(hash: &str, seeds: u32) -> TorrentItem {
    TorrentItem {
        title: format!("row-{}", hash),
        info_hash: hash.to_string(),
        source: "rutor".to_string(),
        seeds_n: seeds,
        ..Default::default()
    }
}

#[test]
fn test_finish_search_dedupes_and_orders_the_merged_list() {
    let mut ui = make_ui();
    ui.state = AppState::Searching;
    // The same torrent arrived from two sources with different health,
    // plus a healthier unrelated row that arrived first.
    ui.results = vec![row("other", 5), row("dup", 3), row("dup", 12)];
    ui.selected = 0;

    finish_search(&mut ui, 1, 1, &HashMap::new());

    assert_eq!(ui.results.len(), 2, "one row per info hash");
    assert_eq!(ui.results[0].seeds_n, 12, "healthiest copy first");
    assert_eq!(ui.results[1].title, "row-other");
    assert!(
        ui.logs.iter().any(|l| l.contains("Removed 1 duplicate results")),
        "the dedup must be visible in the log: {:?}",
        ui.logs
    );
}

#[test]
fn test_finish_search_follows_the_selected_row_to_its_new_position() {
    let mut ui = make_ui();
    ui.state = AppState::Searching;
    ui.results = vec![row("cold", 1), row("warm", 9), row("mild", 5)];
    ui.selected = 0; // the cold row, which the default order moves last

    finish_search(&mut ui, 1, 1, &HashMap::new());

    assert_eq!(
        ui.results[ui.selected].title, "row-cold",
        "reordering must not silently change what the user had highlighted"
    );
    assert_eq!(ui.results[0].title, "row-warm", "the list itself is reordered");
}

#[test]
fn test_finish_search_clamps_the_selection_when_dedup_removed_that_row() {
    let mut ui = make_ui();
    ui.state = AppState::Searching;
    // Same torrent, two sources named it differently -- the selected
    // row is the copy that loses the dedup and disappears entirely.
    let loser = TorrentItem {
        title: "the one I highlighted".to_string(),
        info_hash: "deadbeef".to_string(),
        source: "rutor".to_string(),
        seeds_n: 3,
        ..Default::default()
    };
    ui.results = vec![loser, row("deadbeef", 12), row("other", 1)];
    ui.selected = 0;

    finish_search(&mut ui, 1, 1, &HashMap::new());

    assert_eq!(ui.results.len(), 2, "the two hash-equal rows collapsed");
    assert!(
        ui.selected < ui.results.len(),
        "selection must land on a real row, not past the end"
    );
    assert_ne!(
        ui.results[ui.selected].title,
        "the one I highlighted",
        "the removed row cannot stay selected"
    );
}
