//! The Torrent panel while a stream is being set up: pressing Enter
//! starts browser, magnet fetch, TorrServer add and upload, and until
//! TorrServer answered with a hash the panel used to print
//! `Hash: Status:` with both values empty -- the most consequential
//! key in the app gave no visible state.

use doris::config::Config;
use doris::ui::view::App as UiApp;
use doris::ui::view::AppState;
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
fn test_a_stream_in_progress_says_it_is_starting() {
    let mut app = make_app();
    app.state = AppState::Streaming;
    let text = render(&mut app, 120, 40);
    assert!(
        text.contains("Starting stream"),
        "the launch window must not look idle:\n{text}"
    );
}

#[test]
fn test_an_idle_panel_does_not_claim_a_stream() {
    let mut app = make_app();
    app.state = AppState::Idle;
    let text = render(&mut app, 120, 40);
    assert!(
        !text.contains("Starting stream"),
        "nothing is being launched:\n{text}"
    );
    assert!(text.contains("Status"), "the panel still labels itself");
}

#[test]
fn test_a_live_torrent_reports_its_own_hash() {
    let mut app = make_app();
    app.state = AppState::Streaming;
    app.torrent_status.hash = "abcdef0123456789".to_string();
    app.torrent_status.status = "Downloading".to_string();
    let text = render(&mut app, 120, 40);
    assert!(
        text.contains("abcdef0123456789"),
        "once TorrServer answered, the hash is the real state:\n{text}"
    );
    assert!(
        !text.contains("Starting stream"),
        "and the launch wording steps aside:\n{text}"
    );
}
