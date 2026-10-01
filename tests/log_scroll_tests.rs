//! Log scrolling follows the reading position instead of overriding it:
//! a new line used to drag the panel back to the bottom no matter where
//! the user had scrolled to.

use doris::ui::view::App as UiApp;

fn make_app() -> UiApp {
    UiApp::new("http://127.0.0.1:8090".into(), None)
}

fn fill(app: &mut UiApp, n: usize) {
    for i in 0..n {
        app.add_log(&format!("msg {i}"));
    }
}

#[test]
fn test_new_line_keeps_a_reader_where_they_were() {
    let mut app = make_app();
    fill(&mut app, 30);
    app.scroll_logs(-1);
    app.scroll_logs(-1);
    assert_eq!(app.log_scroll, 28, "the reader moved off the bottom");

    app.add_log("later");

    assert_eq!(
        app.log_scroll, 28,
        "a new line must not yank a reader back to the bottom"
    );
}

#[test]
fn test_new_line_still_follows_the_bottom() {
    let mut app = make_app();
    fill(&mut app, 30);
    assert_eq!(app.log_scroll, 30, "the panel sits at the newest line");

    app.add_log("later");

    assert_eq!(
        app.log_scroll, 31,
        "someone at the bottom stays at the bottom"
    );
}

#[test]
fn test_reader_position_survives_the_ring_buffer_turning_over() {
    let mut app = make_app();
    // The ring holds 500 lines: once it turns, every stored index moves
    fill(&mut app, 500);
    app.log_scroll = 400;

    app.add_log("the line that evicts the oldest");

    assert_eq!(app.log_scroll, 399, "the reading position follows the rows");
    assert_eq!(app.logs.len(), 500, "the ring keeps its cap");
}

#[test]
fn test_page_scrolls_are_not_overwritten_either() {
    let mut app = make_app();
    fill(&mut app, 100);
    app.scroll_logs(-(doris::ui::view::LOG_PAGE_STEP as isize));
    let parked = app.log_scroll;

    app.add_log("later");

    assert_eq!(app.log_scroll, parked, "page-up stays where it was put");
}

#[test]
fn test_detail_log_follows_the_reader_too() {
    let mut app = make_app();
    for i in 0..20 {
        app.add_detail(&format!("stream {i}"));
    }
    assert_eq!(app.detail_log_scroll, 20, "at the bottom by default");

    app.detail_log_scroll = 7; // parked in the middle of the stream log
    app.add_detail("later");
    assert_eq!(
        app.detail_log_scroll, 7,
        "a live stream must not drag a reader to the newest line"
    );
}
