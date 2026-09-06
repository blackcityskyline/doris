use ratatui::prelude::*;
use ratatui::backend::TestBackend;
use ratatui::widgets::*;
use t_hunter::ui::app::App as UiApp;
use t_hunter::search::models::TorrentItem;

fn make_test_app() -> UiApp {
    UiApp::new(
        "http://127.0.0.1:8090".into(),
        "helium [gui] (/usr/bin/helium)".into(),
    )
}

fn make_results(n: usize) -> Vec<TorrentItem> {
    (0..n)
        .map(|i| TorrentItem {
            title: format!("Torrent {}", i),
            size: format!("{} GB", i + 1),
            seeds: format!("{}", i * 10),
            date: format!("01-Янв-2{}", i),
            download_url: format!("/forum/dl.php?t={}", 1000 + i),
            page_url: format!("viewtopic.php?t={}", 1000 + i),
            query: "test".into(),
        })
        .collect()
}

#[test]
fn test_app_initial_state() {
    let app = make_test_app();
    assert!(app.search_input.is_empty());
    assert!(app.results.is_empty());
    assert_eq!(app.selected, 0);
    assert_eq!(app.logs.len(), 0);
    assert!(app.running);
    assert!(!app.input_mode);
}

#[test]
fn test_add_log() {
    let mut app = make_test_app();
    app.add_log("first message");
    app.add_log("second message");
    assert_eq!(app.logs.len(), 2);
    assert!(app.logs[0].contains("first message"));
    assert!(app.logs[1].contains("second message"));
}

#[test]
fn test_add_log_timestamp_format() {
    let mut app = make_test_app();
    app.add_log("test");
    assert!(app.logs[0].starts_with('['));
    assert!(app.logs[0].contains("] test"));
}

#[test]
fn test_add_log_buffer_limit() {
    let mut app = make_test_app();
    for i in 0..600 {
        app.add_log(&format!("msg {}", i));
    }
    assert_eq!(app.logs.len(), 500);
    assert!(app.logs[0].contains("msg 100"));
    assert!(app.logs[499].contains("msg 599"));
}

#[test]
fn test_log_scroll_initial() {
    let mut app = make_test_app();
    for i in 0..20 {
        app.add_log(&format!("msg {}", i));
    }
    assert_eq!(app.log_scroll, 20);
}

#[test]
fn test_log_scroll_up() {
    let mut app = make_test_app();
    for i in 0..20 {
        app.add_log(&format!("msg {}", i));
    }
    app.scroll_logs_up();
    assert_eq!(app.log_scroll, 19);
    app.scroll_logs_up();
    assert_eq!(app.log_scroll, 18);
}

#[test]
fn test_log_scroll_down() {
    let mut app = make_test_app();
    for i in 0..20 {
        app.add_log(&format!("msg {}", i));
    }
    app.log_scroll = 10;
    app.scroll_logs_down();
    assert_eq!(app.log_scroll, 11);
    app.scroll_logs_down();
    assert_eq!(app.log_scroll, 12);
}

#[test]
fn test_log_scroll_cannot_go_below_zero() {
    let mut app = make_test_app();
    app.add_log("msg");
    app.log_scroll = 0;
    app.scroll_logs_up();
    assert_eq!(app.log_scroll, 0);
}

#[test]
fn test_log_scroll_cannot_go_past_end() {
    let mut app = make_test_app();
    for i in 0..5 {
        app.add_log(&format!("msg {}", i));
    }
    app.log_scroll = 5;
    app.scroll_logs_down();
    assert_eq!(app.log_scroll, 5);
}

#[test]
fn test_render_does_not_panic() {
    let mut app = make_test_app();
    app.results = make_results(10);
    app.add_log("test log");

    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| app.render(frame)).unwrap();
}

#[test]
fn test_render_empty_state() {
    let app = make_test_app();
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| app.render(frame)).unwrap();
}

#[test]
fn test_render_with_many_results() {
    let mut app = make_test_app();
    app.results = make_results(100);
    app.selected = 50;
    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| app.render(frame)).unwrap();
}

#[test]
fn test_render_log_scroll() {
    let mut app = make_test_app();
    for i in 0..50 {
        app.add_log(&format!("message {}", i));
    }
    app.log_scroll = 30;
    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| app.render(frame)).unwrap();
}

#[test]
fn test_render_input_mode() {
    let mut app = make_test_app();
    app.enter_input_mode();
    app.search_input = "test query".into();
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| app.render(frame)).unwrap();
}

#[test]
fn test_state_searching() {
    let mut app = make_test_app();
    app.state = t_hunter::ui::app::AppState::Searching;
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| app.render(frame)).unwrap();
}

#[test]
fn test_state_streaming() {
    let mut app = make_test_app();
    app.state = t_hunter::ui::app::AppState::Streaming;
    app.results = make_results(3);
    app.selected = 1;
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| app.render(frame)).unwrap();
}
