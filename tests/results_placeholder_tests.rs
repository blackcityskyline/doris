//! The Results panel's empty state: an empty table is four different
//! situations wearing the same face -- nothing asked for yet, a search
//! in flight, a query that came back empty, and a filter that hid every
//! row -- and before this the log was the only place that told them
//! apart.

use doris::config::Config;
use doris::sources::models::TorrentItem;
use doris::sources::orchestrator::SourceStatus;
use doris::ui::view::App as UiApp;
use doris::ui::view::AppState;
use ratatui::backend::TestBackend;
use ratatui::Terminal;

mod common;

fn make_app() -> UiApp {
    common::make_app()
}

fn item(title: &str) -> TorrentItem {
    TorrentItem {
        title: title.to_string(),
        ..Default::default()
    }
}

/// Render `app` once at `w`x`h` and hand back the drawn rows.
fn render(app: &mut UiApp, w: u16, h: u16) -> Vec<String> {
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal
        .draw(|frame| app.render(frame, &Config::default()))
        .unwrap();
    let buf = terminal.backend().buffer();
    (0..buf.area.height)
        .map(|y| {
            (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect()
        })
        .collect()
}

fn shown(app: &mut UiApp) -> String {
    render(app, 120, 40).join("\n")
}

#[test]
fn test_never_searched_says_how_to_start() {
    let mut app = make_app();
    let text = shown(&mut app);
    assert!(
        text.contains("Nothing searched yet"),
        "a fresh panel must say what to do, not sit blank:\n{text}"
    );
}

#[test]
fn test_search_in_flight_says_searching() {
    let mut app = make_app();
    app.state = AppState::Searching;
    let text = shown(&mut app);
    assert!(
        text.contains("Searching..."),
        "a running search must not look like an empty result:\n{text}"
    );
}

#[test]
fn test_empty_answer_names_the_query() {
    let mut app = make_app();
    app.search_query = Some("zzz".to_string());
    app.state = AppState::Idle;
    let text = shown(&mut app);
    assert!(
        text.contains("No results for 'zzz'"),
        "an empty answer must name the query it answered:\n{text}"
    );
}

#[test]
fn test_empty_browse_is_worded_as_browse() {
    let mut app = make_app();
    app.search_query = Some(String::new());
    app.browsing = true;
    let text = shown(&mut app);
    assert!(
        text.contains("No fresh rows"),
        "an empty browse must not read as a failed search:\n{text}"
    );
}

#[test]
fn test_filter_that_hides_everything_says_so() {
    let mut app = make_app();
    app.search_query = Some("torrent".to_string());
    app.results = vec![item("Torrent 1"), item("Torrent 2")];
    app.zones.filter_input = "nothing-matches-this".to_string();
    app.update_filter();
    let text = shown(&mut app);
    assert!(
        text.contains("matches none of the 2 rows"),
        "a filter that hides every row must name itself:\n{text}"
    );
}

#[test]
fn test_all_sources_down_is_not_worded_as_no_matches() {
    let mut app = make_app();
    app.search_query = Some("batman".to_string());
    app.state = AppState::Idle;
    app.source_status
        .insert("rutor".into(), SourceStatus::Error("HTTP 503".into()));
    app.source_status
        .insert("yts".into(), SourceStatus::Timeout);

    let text = shown(&mut app);
    assert!(
        text.contains("Every source failed"),
        "a network outage must not read as 'nobody has it':\n{text}"
    );
    assert!(
        !text.contains("No results for 'batman'"),
        "the outage wording replaces the empty-answer wording:\n{text}"
    );
}

#[test]
fn test_one_source_answering_keeps_the_empty_answer_wording() {
    let mut app = make_app();
    app.search_query = Some("batman".to_string());
    app.state = AppState::Idle;
    app.source_status
        .insert("rutor".into(), SourceStatus::Ok(0));
    app.source_status
        .insert("yts".into(), SourceStatus::Error("HTTP 503".into()));

    let text = shown(&mut app);
    assert!(
        text.contains("No results for 'batman'"),
        "one source that answered makes this a query result, not an \
         outage:\n{text}"
    );
}

#[test]
fn test_rows_present_render_no_placeholder() {
    let mut app = make_app();
    app.search_query = Some("torrent".to_string());
    app.results = vec![item("Torrent 1")];
    app.update_filter();
    let text = shown(&mut app);
    assert!(
        text.contains("Torrent 1"),
        "the rows themselves must be drawn:\n{text}"
    );
    assert!(
        !text.contains("Nothing searched yet"),
        "a populated panel must not carry the empty state:\n{text}"
    );
}

/// An empty table is still a list the TUI can leave.
#[test]
fn test_an_empty_result_set_still_wants_the_next_page() {
    let mut app = make_app();
    app.search_query = Some("frieren crack".to_string());
    app.state = AppState::Idle;
    app.all_loaded = false;
    app.filtered_indices = Vec::new();

    assert!(app.results.is_empty(), "starts empty");
    assert!(
        app.needs_more(),
        "an empty page must still promise the next one"
    );
}

#[test]
fn test_a_search_that_really_finished_does_not() {
    let mut app = make_app();
    app.search_query = Some("frieren crack".to_string());
    app.state = AppState::Idle;
    app.all_loaded = true;

    assert!(!app.needs_more(), "no more pages to ask for");
}

#[test]
fn test_a_search_still_running_does_not_ask_for_more() {
    let mut app = make_app();
    app.search_query = Some("frieren crack".to_string());
    app.state = AppState::Searching;

    assert!(!app.needs_more(), "one search at a time");
}
