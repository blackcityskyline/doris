use doris::search::models::TorrentItem;
use doris::ui::app::{App as UiApp, HeaderHint, TorrentClickAction};
use doris::ui::zones::ZoneId;
use ratatui::layout::Rect;

fn make_app(browser_info: &str, torrserver_url: &str) -> UiApp {
    UiApp::new(
        torrserver_url.to_string(),
        browser_info.to_string(),
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

fn make_results(n: usize) -> Vec<TorrentItem> {
    (0..n)
        .map(|i| TorrentItem {
            title: format!("Torrent {}", i),
            size: format!("{} GB", i + 1),
            seeds: format!("{}", i * 10),
            date: format!("01-Jan-2{}", i),
            download_url: format!("/forum/dl.php?t={}", 1000 + i),
            page_url: format!("viewtopic.php?t={}", 1000 + i),
            query: "test".into(),
        })
        .collect()
}

// --- hint_at_column ---------------------------------------------------

#[test]
fn test_hint_at_column_finds_each_hint() {
    let app = make_app("chrome [hidden] (/usr/bin/chrome)", "http://127.0.0.1:8090");
    // Header text: "[<browser_info>] <torrserver_url> | s: search | S: settings | L: log | F: filter"
    let prefix_len = format!("[{}] {} | ", app.browser_info, app.torrserver_url).chars().count() as u16;

    // "s: search" starts right after the prefix (+1 for the border offset
    // hint_at_column itself accounts for).
    let search_col = 1 + prefix_len;
    assert_eq!(app.hint_at_column(search_col), Some(HeaderHint::Search));

    let settings_col = search_col + "s: search".chars().count() as u16 + 3;
    assert_eq!(app.hint_at_column(settings_col), Some(HeaderHint::Settings));

    let log_col = settings_col + "S: settings".chars().count() as u16 + 3;
    assert_eq!(app.hint_at_column(log_col), Some(HeaderHint::Log));

    let filter_col = log_col + "L: log".chars().count() as u16 + 3;
    assert_eq!(app.hint_at_column(filter_col), Some(HeaderHint::Filter));
}

#[test]
fn test_hint_at_column_returns_none_outside_any_hint() {
    let app = make_app("chrome", "http://127.0.0.1:8090");
    assert_eq!(app.hint_at_column(0), None);
    assert_eq!(app.hint_at_column(3), None); // inside "[chrome]", not a hint
}

#[test]
fn test_hint_at_column_returns_none_in_input_mode() {
    let mut app = make_app("chrome", "http://127.0.0.1:8090");
    app.input_mode = true;
    // Even a column that would normally hit "s: search" should return
    // None while typing -- the header shows "INPUT (s/i)" instead.
    for col in 0..80 {
        assert_eq!(app.hint_at_column(col), None);
    }
}

// --- zone_at ------------------------------------------------------------

#[test]
fn test_zone_at_finds_the_containing_zone() {
    let mut app = make_app("chrome", "http://127.0.0.1:8090");
    app.zones.update_areas(Rect::new(0, 0, 80, 24));

    let results_area = app.zones.get_area(ZoneId::Results);
    assert_eq!(app.zone_at(results_area.y, results_area.x), Some(ZoneId::Results));

    let torrent_area = app.zones.get_area(ZoneId::Torrent);
    assert_eq!(app.zone_at(torrent_area.y, torrent_area.x), Some(ZoneId::Torrent));
}

#[test]
fn test_zone_at_returns_none_outside_all_zones() {
    let mut app = make_app("chrome", "http://127.0.0.1:8090");
    app.zones.update_areas(Rect::new(0, 0, 80, 24));
    // Far outside the terminal entirely.
    assert_eq!(app.zone_at(1000, 1000), None);
}

#[test]
fn test_zone_at_only_hits_fullscreened_zone() {
    let mut app = make_app("chrome", "http://127.0.0.1:8090");
    app.zones.set_fullscreen(Some(ZoneId::Log));
    app.zones.update_areas(Rect::new(0, 0, 80, 24));

    // Every point in the full terminal area should resolve to Log...
    assert_eq!(app.zone_at(10, 10), Some(ZoneId::Log));
    // ...since every other zone's area is zeroed out while fullscreened.
    let torrent_area_before = app.zones.get_area(ZoneId::Torrent);
    assert_eq!(torrent_area_before, Rect::default());
}

// --- click_at -------------------------------------------------------------

#[test]
fn test_click_at_focuses_the_clicked_zone() {
    let mut app = make_app("chrome", "http://127.0.0.1:8090");
    app.zones.update_areas(Rect::new(0, 0, 80, 24));
    assert_eq!(app.zones.focused, ZoneId::Results);

    let log_area = app.zones.get_area(ZoneId::Log);
    app.click_at(log_area.y, log_area.x);
    assert_eq!(app.zones.focused, ZoneId::Log);
}

#[test]
fn test_click_at_results_header_row_does_not_select_a_row() {
    let mut app = make_app("chrome", "http://127.0.0.1:8090");
    app.results = make_results(5);
    app.filtered_indices = (0..5).collect();
    app.zones.update_areas(Rect::new(0, 0, 80, 24));

    let results_area = app.zones.get_area(ZoneId::Results);
    let before = app.selected;
    app.click_at(results_area.y, results_area.x); // header row, not a data row
    assert_eq!(app.selected, before);
}

#[test]
fn test_click_at_results_data_row_selects_that_item() {
    let mut app = make_app("chrome", "http://127.0.0.1:8090");
    app.results = make_results(5);
    app.filtered_indices = (0..5).collect();
    app.zones.update_areas(Rect::new(0, 0, 80, 24));

    let results_area = app.zones.get_area(ZoneId::Results);
    // Row 0 is the header, row 1 is the first data row (index 0).
    app.click_at(results_area.y + 1, results_area.x);
    assert_eq!(app.selected, 0);

    app.click_at(results_area.y + 2, results_area.x);
    assert_eq!(app.selected, 1);
}

#[test]
fn test_click_at_respects_filtered_indices_not_raw_results_order() {
    let mut app = make_app("chrome", "http://127.0.0.1:8090");
    app.results = make_results(5);
    // Simulate a filter that only kept results 3 and 4.
    app.filtered_indices = vec![3, 4];
    app.zones.update_areas(Rect::new(0, 0, 80, 24));

    let results_area = app.zones.get_area(ZoneId::Results);
    app.click_at(results_area.y + 1, results_area.x); // first visible (filtered) row
    assert_eq!(app.selected, 3);
}

#[test]
fn test_click_at_returns_none_outside_the_torrent_hint_line() {
    let mut app = make_app("chrome", "http://127.0.0.1:8090");
    app.zones.update_areas(Rect::new(0, 0, 80, 24));
    let torrent_area = app.zones.get_area(ZoneId::Torrent);
    // Click the Torrent zone's border/top row, not its pause/remove hint line.
    let action = app.click_at(torrent_area.y, torrent_area.x);
    assert_eq!(action, None);
}

#[test]
fn test_click_at_torrent_pause_hint_returns_toggle_pause() {
    let mut app = make_app("chrome", "http://127.0.0.1:8090");
    app.zones.update_areas(Rect::new(0, 0, 80, 24));
    let torrent_area = app.zones.get_area(ZoneId::Torrent);
    // Hint line is 1 (border) + 4 (content lines above it) rows down.
    let hint_row = torrent_area.y + 1 + 4;
    let action = app.click_at(hint_row, torrent_area.x + 1); // inside "p: pause/resume"
    assert_eq!(action, Some(TorrentClickAction::TogglePause));
}

#[test]
fn test_click_at_torrent_remove_hint_returns_remove() {
    let mut app = make_app("chrome", "http://127.0.0.1:8090");
    app.zones.update_areas(Rect::new(0, 0, 80, 24));
    let torrent_area = app.zones.get_area(ZoneId::Torrent);
    let hint_row = torrent_area.y + 1 + 4;
    // "p: pause/resume" is 15 chars + a 2-char gap before "d: remove".
    let remove_col = torrent_area.x + 1 + 15 + 2;
    let action = app.click_at(hint_row, remove_col);
    assert_eq!(action, Some(TorrentClickAction::Remove));
}
