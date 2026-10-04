use doris::app::{
    cycle_index, enter_action, fill_missing_magnet, row_needs_source, safe_filename, source_id_for,
    source_needs_browser, torrent_handoff, EnterAction,
};
use doris::config::Config;
use doris::results::{apply_source_done, finish_search, resolve_cookie_file, source_outcome_line};
use doris::sources::models::TorrentItem;
use doris::sources::source::{
    build_source, AuthContext, Group, LogFn, SearchPage, SearchRequest, Source, SourceEnv,
};
use doris::ui::view::App as UiApp;
use doris::ui::view::AppState;
use std::collections::HashMap;
use std::path::PathBuf;

mod common;

// --- apply_source_done (B0.2 stale-drop; B3 per-source arrival) -------------

fn make_ui() -> UiApp {
    common::make_app()
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
    assert!(
        ui.state == AppState::Searching,
        "a stale event must not flip state"
    );
}

#[test]
fn test_rows_append_and_the_outcome_line_is_logged() {
    let mut ui = make_ui();
    ui.results = vec![item("already here")];
    ui.state = AppState::Searching;

    let applied = apply_source_done(&mut ui, 3, 3, "rutor", vec![item("a"), item("b")], None);

    assert!(applied);
    assert_eq!(
        ui.results.len(),
        3,
        "each source appends into the same list"
    );
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
        ui.logs
            .iter()
            .any(|l| l.contains("rutracker: timed out after 25s")),
        "the failure line must reach the log: {:?}",
        ui.logs
    );
}

// --- finish_search the generation is over when every source answered --

#[test]
fn test_completion_marks_all_loaded_when_no_source_has_more() {
    let mut ui = make_ui();
    ui.state = AppState::Searching;
    let has_more: HashMap<String, bool> = HashMap::new();

    assert!(finish_search(&mut ui, 1, 1, &has_more));
    assert!(
        ui.all_loaded,
        "no source with another page means stop paging"
    );
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
    assert!(
        !ui.all_loaded,
        "one source with another page keeps Load more alive"
    );
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

// --- source_needs_browser ( streaming ignored item.source) -------

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
    assert!(source_needs_browser(""));
    assert!(source_needs_browser("never-heard-of-it"));
    assert!(
        !source_needs_browser("1337x"),
        "it needs no browser session"
    );
}

// --- source_id_for rows route through the registry -------------------

#[test]
fn test_rows_route_to_their_own_registered_source() {
    assert_eq!(source_id_for(&item_with_source("rutor")), "rutor");
    assert_eq!(source_id_for(&item_with_source("rutracker")), "rutracker");
}

#[test]
fn test_legacy_rows_fall_back_to_rutracker() {
    // Rows serialized before the `source` field existed deserialize to "",
    assert_eq!(source_id_for(&item_with_source("")), "rutracker");
    assert_eq!(
        source_id_for(&item_with_source("never-heard-of-it")),
        "rutracker"
    );
    // An id the registry *knows* keeps itself even while it is still
    assert_eq!(source_id_for(&item_with_source("1337x")), "1337x");
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
            assert_eq!(
                cycle_index(forward, len, -1),
                pos,
                "len={} pos={}",
                len,
                pos
            );
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
    let config = Config {
        save_cookies: false,
        ..Default::default()
    };
    assert_eq!(
        resolve_cookie_file(&config, Some(std::path::Path::new("/tmp/x.txt"))),
        None
    );
}

#[test]
fn test_cookie_file_falls_back_to_config_toml_setting() {
    // This is the actual regression: previously only the CLI flag was
    let config = Config {
        save_cookies: true,
        cookie_file: "my-cookies.txt".to_string(),
        ..Default::default()
    };
    assert_eq!(
        resolve_cookie_file(&config, None),
        Some(PathBuf::from("my-cookies.txt"))
    );
}

#[test]
fn test_cookie_file_cli_flag_takes_priority_over_config() {
    let config = Config {
        save_cookies: true,
        cookie_file: "config-cookies.txt".to_string(),
        ..Default::default()
    };
    let cli_path = PathBuf::from("/explicit/cli-cookies.txt");
    assert_eq!(
        resolve_cookie_file(&config, Some(&cli_path)),
        Some(cli_path)
    );
}

/// The default has to be usable as-is, and it has to be a real location
/// rather than a name that only resolves from the directory doris was
/// started in: a bare `cookies.txt` is how a session ended up inside
/// `target/release/`.
#[test]
fn test_cookie_file_default_config_value_is_usable() {
    let config = Config::default();
    assert!(config.save_cookies);
    let resolved = resolve_cookie_file(&config, None).expect("a default must resolve");
    assert!(resolved.is_absolute(), "got {:?}", resolved);
    assert_eq!(resolved.file_name().unwrap(), "cookies.txt");
}

// --- source_outcome_line ( per-source errors were dropped) -------

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
    let healthy: Result<usize, String> = Ok(7);
    let broken: Result<usize, String> = Err("timeout".to_string());
    assert_eq!(source_outcome_line("rutor", &healthy), "rutor: 7 results");
    assert_eq!(
        source_outcome_line("rutracker", &broken),
        "rutracker: timeout"
    );
}

// --- enter_action ( Enter on an empty query could stream) --------

#[test]
fn test_enter_on_empty_query_in_input_mode_does_nothing() {
    // The regression: `submit_search()` returned None (having already left
    assert_eq!(
        enter_action(true, false, true, false, true),
        EnterAction::DoNothing
    );
    assert_eq!(
        enter_action(true, false, false, true, true),
        EnterAction::DoNothing
    );
}

#[test]
fn test_enter_with_a_query_submits_the_search() {
    assert_eq!(
        enter_action(true, true, false, false, true),
        EnterAction::SubmitQuery
    );
    // A pending switch of either tab row must not steal Enter from the
    assert_eq!(
        enter_action(true, true, true, false, false),
        EnterAction::SubmitQuery
    );
    assert_eq!(
        enter_action(true, true, false, true, false),
        EnterAction::SubmitQuery
    );
}

#[test]
fn test_enter_plays_outside_input_mode() {
    assert_eq!(
        enter_action(false, true, false, false, true),
        EnterAction::Play
    );
    assert_eq!(
        enter_action(false, false, false, false, true),
        EnterAction::Play
    );
}

#[test]
fn test_enter_after_source_switch_restarts_the_search_instead_of_playing() {
    assert_eq!(
        enter_action(false, true, true, false, true),
        EnterAction::RestartSearch
    );
}

/// B6: switching the category row owes the same re-search a source tab
/// switch does -- the view was re-derived from the rows on screen, but
/// only the sources that filter server-side have been asked yet.
#[test]
fn test_enter_after_category_switch_restarts_the_search_instead_of_playing() {
    assert_eq!(
        enter_action(false, true, false, true, true),
        EnterAction::RestartSearch
    );
    // With nothing selected yet, the owed search still wins over
    assert_eq!(
        enter_action(false, false, false, true, false),
        EnterAction::RestartSearch
    );
}

#[test]
fn test_enter_without_a_selection_does_nothing() {
    assert_eq!(
        enter_action(false, true, false, false, false),
        EnterAction::DoNothing
    );
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
    ui.results = vec![row("other", 5), row("dup", 3), row("dup", 12)];
    ui.selected = 0;

    finish_search(&mut ui, 1, 1, &HashMap::new());

    assert_eq!(ui.results.len(), 2, "one row per info hash");
    assert_eq!(ui.results[0].seeds_n, 12, "healthiest copy first");
    assert_eq!(ui.results[1].title, "row-other");
    assert!(
        ui.logs
            .iter()
            .any(|l| l.contains("Removed 1 duplicate results")),
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
    assert_eq!(
        ui.results[0].title, "row-warm",
        "the list itself is reordered"
    );
}

#[test]
fn test_finish_search_clamps_the_selection_when_dedup_removed_that_row() {
    let mut ui = make_ui();
    ui.state = AppState::Searching;
    // Same torrent, two sources named it differently -- the selected
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
        ui.results[ui.selected].title, "the one I highlighted",
        "the removed row cannot stay selected"
    );
}

// --- download key: what the daemon is handed ----------------------

/// The handoff file lives under doris's own state, named after the row.
///
/// Not in the downloads directory: a `.torrent` lying next to the user's
/// films is what the `d` key used to leave behind, and the whole point is
/// that it now downloads instead.
///
/// A data URI was the first answer and it does not work: Transmission 4
/// answers `unrecognized info` to `data:application/x-bittorrent;base64,...`
/// (measured against 4.1.3's RPC), so the bytes go to a file and the daemon
/// reads that path.
#[test]
fn test_the_handoff_file_is_named_after_the_row_and_kept_out_of_downloads() {
    let path = torrent_handoff(
        std::path::Path::new("/state/doris"),
        "YTS: The Matrix / 2003",
    );

    assert_eq!(
        path,
        std::path::PathBuf::from("/state/doris/add/YTS_ The Matrix _ 2003.torrent"),
        "the file is under doris's state, named like every other download"
    );
    assert!(
        !path.to_string_lossy().contains("Downloads"),
        "a .torrent in the downloads directory is the thing this stops doing"
    );
}

/// The sanitizer both download paths now share: only the characters a
/// filesystem objects to become `_`, and the result is trimmed.
#[test]
fn test_safe_filename_escapes_path_characters_and_trims() {
    assert_eq!(
        safe_filename("YTS: The Matrix / Reloaded? (2003)"),
        "YTS_ The Matrix _ Reloaded_ _2003_"
    );
    assert_eq!(safe_filename("  spaced  "), "spaced");
    assert_eq!(safe_filename("сериал 1 сезон"), "сериал 1 сезон");
    assert_eq!(
        safe_filename("a\\b<c>d\"e|f"),
        "a_b_c_d_e_f",
        "every separator and shell-special character is neutralised"
    );
}

// --- fill_missing_magnet 1337x rows carry no link ----------------

/// A source whose rows arrive with neither a magnet nor a `.torrent` link, so the row's own
/// page is the only place one lives (1337x).
struct LazySource {
    asked: std::sync::atomic::AtomicUsize,
}

impl LazySource {
    fn new() -> Self {
        Self {
            asked: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    fn asked(&self) -> usize {
        self.asked.load(std::sync::atomic::Ordering::SeqCst)
    }
}

#[async_trait::async_trait]
impl Source for LazySource {
    fn id(&self) -> &'static str {
        "lazy"
    }

    fn label(&self) -> &'static str {
        "Lazy"
    }

    fn groups(&self) -> &'static [Group] {
        &[]
    }

    fn home_url(&self) -> &'static str {
        "https://lazy.invalid"
    }

    fn requires_browser(&self) -> bool {
        false
    }

    fn supports_browse(&self) -> bool {
        false
    }

    async fn ensure_logged_in(&self, _auth: &AuthContext, _log: &LogFn) -> anyhow::Result<bool> {
        Ok(true)
    }

    async fn search(&self, _req: &SearchRequest) -> anyhow::Result<SearchPage> {
        Ok(SearchPage {
            items: Vec::new(),
            has_more: false,
            next_offset: None,
        })
    }

    async fn download_torrent(&self, _url: &str) -> anyhow::Result<Vec<u8>> {
        anyhow::bail!("this mock never serves a file")
    }

    async fn resolve_magnet(&self, page_url: &str) -> anyhow::Result<Option<String>> {
        self.asked.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(
            page_url, "https://lazy.invalid/torrent/1/x/",
            "the row's own page is what gets fetched"
        );
        Ok(Some("magnet:?xt=urn:btih:abc".to_string()))
    }
}

#[tokio::test]
async fn test_a_row_with_no_link_gets_the_magnet_off_its_own_page() {
    let source = LazySource::new();
    let mut item = TorrentItem {
        title: "1337x row".to_string(),
        page_url: "https://lazy.invalid/torrent/1/x/".to_string(),
        ..Default::default()
    };

    fill_missing_magnet(&mut item, &source)
        .await
        .expect("resolve");

    assert_eq!(
        item.magnet.as_deref(),
        Some("magnet:?xt=urn:btih:abc"),
        "the row is playable after this"
    );
    assert_eq!(source.asked(), 1, "exactly one request, at play time");
}

#[tokio::test]
async fn test_a_row_that_already_has_a_way_to_play_is_never_asked() {
    let with_file = LazySource::new();
    let mut file_row = TorrentItem {
        page_url: "https://lazy.invalid/torrent/1/x/".to_string(),
        download_url: "https://lazy.invalid/download/1".to_string(),
        ..Default::default()
    };
    fill_missing_magnet(&mut file_row, &with_file)
        .await
        .expect("resolve");
    assert_eq!(with_file.asked(), 0, "a fetchable row needs no lookup");
    assert_eq!(file_row.magnet, None, "and is not quietly rewritten");

    let with_link = LazySource::new();
    let mut link_row = TorrentItem {
        page_url: "https://lazy.invalid/torrent/1/x/".to_string(),
        magnet: Some("magnet:?xt=urn:btih:present".to_string()),
        ..Default::default()
    };
    fill_missing_magnet(&mut link_row, &with_link)
        .await
        .expect("resolve");
    assert_eq!(with_link.asked(), 0, "a row that has one keeps it");
    assert_eq!(
        link_row.magnet.as_deref(),
        Some("magnet:?xt=urn:btih:present")
    );
}

#[tokio::test]
async fn test_the_default_lookup_answers_without_touching_the_network() {
    // Six sources' rows always carry a magnet or a file; their answer
    let source =
        build_source("rutor", SourceEnv { browser: None }).expect("rutor is in the registry");
    let found = source
        .resolve_magnet("https://nonexistent.invalid/torrent/1/")
        .await
        .expect("the default never fails");
    assert!(found.is_none(), "no link to find, and nothing was fetched");
}

/// Whether streaming a row needs a tracker behind it.
///
/// The bug this pins: a rutracker row carries a *relative* `dl.php?t=...`
/// link and no magnet, so it has a way in while looking link-less. Counting
/// only "no magnet" said no tracker was needed, the magnet add never ran, and
/// the fallback that would have fetched the `.torrent` had no source to fetch
/// it with -- the dead end the log called "the magnet link was refused and no
/// tracker is known for this row", said about a magnet the row never had.
#[test]
fn a_row_with_a_link_needs_a_tracker_even_when_it_has_no_magnet() {
    let rutracker = TorrentItem {
        title: "Дюна".into(),
        source: "rutracker".into(),
        magnet: None,
        download_url: "dl.php?t=6124572".into(),
        page_url: "viewtopic.php?t=6124572".into(),
        ..Default::default()
    };
    assert!(
        row_needs_source(&rutracker),
        "a row with a link and no magnet needs one: the magnet add can fail \
         and the .torrent fallback is what is left"
    );

    // A magnet and nothing to fall back to: the link is the whole of what
    // goes to the server, and building a source would launch a browser to do
    // nothing.
    let magnet_only = TorrentItem {
        magnet: Some("magnet:?xt=urn:btih:abc".into()),
        download_url: String::new(),
        ..rutracker.clone()
    };
    assert!(
        !row_needs_source(&magnet_only),
        "a row named by --magnet needs no tracker"
    );

    // Both, and neither.
    assert!(row_needs_source(&TorrentItem {
        magnet: Some("magnet:?xt=urn:btih:abc".into()),
        download_url: "dl.php?t=1".into(),
        ..Default::default()
    }));
    assert!(row_needs_source(&TorrentItem {
        title: "nothing".into(),
        ..Default::default()
    }));
}
