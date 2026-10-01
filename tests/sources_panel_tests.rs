//! The Trackers panel scrolls its roster to follow the cursor: rows past
//! the end of a short zone used to be simply not drawn while the cursor
//! kept walking every one of them, so it could sit on a row nobody could
//! see.

use doris::config::Config;
use doris::ui::view::source_rows;
use doris::ui::view::{App as UiApp, SourceRow};
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
fn test_the_last_row_is_reachable_on_a_short_terminal() {
    let mut app = make_app();
    let last = source_rows().len() - 1;
    app.sources_cursor = last;
    let last_id = source_rows()[last].id();

    let text = render(&mut app, 120, 40);
    assert!(
        text.contains(last_id),
        "the cursor walked to `{last_id}`, so `{last_id}` must be on screen:\n{text}"
    );
}

#[test]
fn test_the_roster_still_starts_at_the_top_when_the_cursor_is_there() {
    let mut app = make_app();
    app.sources_cursor = 0;

    let text = render(&mut app, 120, 40);
    assert!(
        text.contains(source_rows()[0].id()),
        "cursor at the top, roster at the top:\n{text}"
    );
}

#[test]
fn test_every_row_can_be_reached_by_walking_the_cursor() {
    let mut app = make_app();
    let rows = source_rows();
    let mut missing = Vec::new();
    for (i, row) in rows.iter().enumerate() {
        app.sources_cursor = i;
        let text = render(&mut app, 120, 40);
        if !text.contains(row.id()) {
            missing.push(row.id());
        }
    }
    assert!(
        missing.is_empty(),
        "rows the cursor can stand on but the panel never draws: {missing:?}"
    );
}

/// Every source the registry knows about is in the panel *and* switches
/// both ways from the keyboard: a source that can be seen but not
/// toggled is a source the user can never turn on or off.
#[test]
fn test_every_implemented_source_toggles_through_its_checkbox() {
    let mut app = make_app();
    let mut config = Config::default();
    config.enabled_sources.clear();

    let mut checked = 0usize;
    for (i, row) in source_rows().iter().enumerate() {
        if !row.is_implemented() {
            continue;
        }
        app.sources_cursor = i;
        app.toggle_source(&mut config);
        assert!(row.is_checked(&config), "{} must switch on", row.id());
        app.toggle_source(&mut config);
        assert!(!row.is_checked(&config), "{} must switch off", row.id());
        if row.id() != "all" {
            checked += 1;
        }
    }
    assert_eq!(
        checked,
        doris::sources::source::KNOWN_SOURCES
            .iter()
            .filter(|info| info.implemented)
            .count(),
        "the panel must offer every implemented source"
    );
}

/// The `all` row is the master switch: it must reach every implemented
/// source at once, both directions, not just the ones that happened to
/// be checked.
#[test]
fn test_the_all_row_switches_the_whole_roster() {
    let mut app = make_app();
    let mut config = Config::default();
    config.enabled_sources.clear();
    let last = source_rows().iter().position(|r| r.id() == "all").unwrap();
    app.sources_cursor = last;

    app.toggle_source(&mut config);
    assert!(SourceRow::All.is_checked(&config));
    app.toggle_source(&mut config);
    assert!(!SourceRow::All.is_checked(&config));
    assert!(config.enabled_sources.is_empty());
}
