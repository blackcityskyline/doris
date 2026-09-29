//! The Trackers panel shows what each source answered: the map was
//! filled in on every dispatch and read by nobody, so a source that
//! hung or 503'd was invisible unless you happened to catch its line in
//! the Log.

use doris::config::Config;
use doris::sources::orchestrator::SourceStatus;
use doris::ui::app::App as UiApp;
use ratatui::backend::TestBackend;
use ratatui::Terminal;

fn make_app() -> UiApp {
    UiApp::new("http://127.0.0.1:8090".into(), None)
}

fn render(app: &mut UiApp, w: u16, h: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal
        .draw(|frame| app.render(frame, &Config::default()))
        .unwrap();
    let buf = terminal.backend().buffer();
    (0..buf.area.height)
        .map(|y| {
            (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn test_answered_source_shows_how_many_rows_it_brought() {
    let mut app = make_app();
    app.source_status
        .insert("rutor".into(), SourceStatus::Ok(42));
    let text = render(&mut app, 120, 40);
    let row = text
        .lines()
        .find(|l| l.contains("rutor"))
        .expect("the rutor row is on screen");
    assert!(
        row.contains('✓') && row.contains("42"),
        "a source that answered must show its count: {row}"
    );
}

#[test]
fn test_failed_source_shows_its_error() {
    let mut app = make_app();
    app.source_status
        .insert("yts".into(), SourceStatus::Error("HTTP 503".into()));
    let text = render(&mut app, 120, 40);
    let row = text
        .lines()
        .find(|l| l.contains("yts"))
        .expect("the yts row is on screen");
    assert!(
        row.contains('✗') && row.contains("HTTP 503"),
        "a source that failed must say so in the panel: {row}"
    );
}

#[test]
fn test_a_source_still_working_reads_as_in_flight() {
    let mut app = make_app();
    app.source_status
        .insert("tpb".into(), SourceStatus::Pending);
    let text = render(&mut app, 120, 40);
    let row = text
        .lines()
        .find(|l| l.contains("tpb"))
        .expect("the tpb row is on screen");
    assert!(
        row.contains("…"),
        "a source nobody has heard from must not look idle: {row}"
    );
}

#[test]
fn test_timeout_is_named_not_just_marked() {
    let mut app = make_app();
    app.source_status
        .insert("nyaa".into(), SourceStatus::Timeout);
    let text = render(&mut app, 120, 40);
    let row = text
        .lines()
        .find(|l| l.contains("nyaa"))
        .expect("the nyaa row is on screen");
    assert!(
        row.contains("timeout"),
        "a deadline is not the same as an error: {row}"
    );
}

#[test]
fn test_a_long_error_cannot_stretch_the_panel() {
    let mut app = make_app();
    let long = "x".repeat(400);
    app.source_status
        .insert("subsplease".into(), SourceStatus::Error(long));
    let text = render(&mut app, 120, 40);
    let row = text
        .lines()
        .find(|l| l.contains("subsplease"))
        .expect("the subsplease row is on screen");
    assert!(
        row.chars().count() <= 120,
        "the status must be clamped to the panel: {}",
        row.chars().count()
    );
}

#[test]
fn test_no_status_means_no_noise() {
    let mut app = make_app();
    let text = render(&mut app, 120, 40);
    assert!(
        !text.contains('✓') && !text.contains('✗'),
        "before a search there is nothing to report: {text}"
    );
}
