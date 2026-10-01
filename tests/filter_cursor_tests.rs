//! Where the Results cursor stands while a filter narrows and widens
//! the table.
//!
//! `update_filter` used to jump the cursor to the first surviving row
//! and remember nothing, so clearing the filter left it parked in the
//! middle of the list -- the row the user had been reading was one
//! keystroke away and one keystroke was not enough to get back.

use doris::ui::view::App as UiApp;

fn make_app() -> UiApp {
    let mut app = UiApp::new("http://127.0.0.1:8090".into(), None);
    // Ten rows alternating two groups, so a filter can narrow twice:
    // "seed0" keeps 0/2/4/6/8, adding "4" keeps only 4.
    app.results = (0..10)
        .map(|i| doris::sources::models::TorrentItem {
            title: format!("item{i} seed{}", i % 2),
            ..Default::default()
        })
        .collect();
    app.update_filter();
    app
}

fn set_filter(app: &mut UiApp, text: &str) {
    app.zones.filter_mode = true;
    app.zones.filter_input = text.to_string();
    app.update_filter();
}

#[test]
fn clearing_the_filter_returns_to_the_row_it_left() {
    let mut app = make_app();

    for _ in 0..7 {
        app.navigate_down();
    }
    assert_eq!(app.selected, 7, "the cursor was on row 7");

    set_filter(&mut app, "seed0");
    assert_eq!(
        app.selected, 0,
        "row 7 is out; the cursor moves to the first match"
    );

    set_filter(&mut app, "");
    assert_eq!(
        app.selected, 7,
        "clearing the filter must put the cursor back on row 7"
    );
}

#[test]
fn narrowing_twice_keeps_the_original_row_as_the_anchor() {
    let mut app = make_app();

    for _ in 0..7 {
        app.navigate_down();
    }
    set_filter(&mut app, "seed0");
    assert_eq!(app.selected, 0);

    set_filter(&mut app, "seed0 4");
    assert_eq!(app.selected, 4, "only item4 matches both tokens");

    set_filter(&mut app, "");
    assert_eq!(
        app.selected, 7,
        "the second jump must not overwrite the row the cursor came from"
    );
}

#[test]
fn navigating_by_hand_releases_the_anchor() {
    let mut app = make_app();

    for _ in 0..7 {
        app.navigate_down();
    }
    set_filter(&mut app, "seed0");
    app.navigate_down();
    assert_eq!(app.selected, 2, "the user moved on while filtering");

    set_filter(&mut app, "");
    assert_eq!(
        app.selected, 2,
        "a cursor the user walked away from is not a cursor to restore"
    );
}

#[test]
fn a_selection_the_filter_keeps_is_left_alone() {
    let mut app = make_app();

    for _ in 0..4 {
        app.navigate_down();
    }
    set_filter(&mut app, "seed0");
    assert_eq!(
        app.selected, 4,
        "item4 still matches, so there is nothing to remember or move"
    );

    set_filter(&mut app, "");
    assert_eq!(app.selected, 4, "and nothing to restore either");
}
