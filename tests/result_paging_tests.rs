//! `PageUp`/`PageDown` in the Results panel.
//!
//! They were answered only in the Log panel. In a result list of five
//! hundred rows -- what a search across ten sources returns -- scrolling
//! back meant pressing `k` once per row, because the one key that pages
//! did nothing at all where the list is.

use doris::sources::models::TorrentItem;
use doris::ui::app::App;

fn app_with(rows: usize) -> App {
    let mut app = App::new("http://127.0.0.1:1".into(), None);
    app.results = (0..rows)
        .map(|i| TorrentItem {
            title: format!("Torrent {i}"),
            source: "rutor".into(),
            ..Default::default()
        })
        .collect();
    app.zones.filter_input.clear();
    app.update_filter();
    app
}

fn title(app: &App) -> String {
    app.results[app.selected].title.clone()
}

#[test]
fn a_page_down_moves_a_page_and_stays_inside_the_list() {
    let mut app = app_with(100);

    assert!(app.navigate_page(10), "the key is answered");
    assert_eq!(title(&app), "Torrent 10");

    app.navigate_page(10);
    assert_eq!(title(&app), "Torrent 20");
}

#[test]
fn a_page_up_moves_back() {
    let mut app = app_with(100);
    app.navigate_page(30);

    app.navigate_page(-10);
    assert_eq!(title(&app), "Torrent 20");
}

#[test]
fn paging_stops_at_the_ends_instead_of_wrapping() {
    let mut app = app_with(100);

    app.navigate_page(-10);
    assert_eq!(title(&app), "Torrent 0", "not the last row, not a wrap");

    app.navigate_page(1000);
    assert_eq!(
        title(&app),
        "Torrent 99",
        "a page past the end lands on the last row"
    );

    app.navigate_page(1000);
    assert_eq!(title(&app), "Torrent 99", "and stays there");
}

#[test]
fn a_filter_narrows_what_a_page_lands_on() {
    let mut app = app_with(100);
    // Keep every tenth row: the page counts what the user sees, not what
    // was loaded underneath.
    app.zones.filter_input = "Torrent".into();
    for (i, item) in app.results.iter_mut().enumerate() {
        item.title = if i % 10 == 0 {
            format!("Torrent {i}")
        } else {
            format!("hidden {i}")
        };
    }
    app.update_filter();
    assert_eq!(app.filtered_indices.len(), 10, "setup: ten visible rows");

    app.navigate_page(3);
    assert_eq!(
        title(&app),
        "Torrent 30",
        "a page of three lands on the fourth visible row"
    );
}

#[test]
fn an_empty_list_answers_nothing() {
    let mut app = app_with(0);
    assert!(!app.navigate_page(10));
    assert_eq!(app.selected, 0);
}

/// The search box and the modals own the keyboard; a page key that
/// scrolled a list behind an open dialog would be a keypress the user
/// did not mean to send there.
#[test]
fn paging_is_refused_while_typing_or_a_modal_is_open() {
    let mut app = app_with(50);
    app.enter_input_mode();
    assert!(!app.navigate_page(10), "the search box owns the keyboard");
    app.exit_input_mode();

    app.open_login_modal();
    assert!(!app.navigate_page(10), "a modal owns the keyboard");
}
