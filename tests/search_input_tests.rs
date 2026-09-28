//! The search box as a text field: a caret where the typing happens,
//! and the text you are typing drawn in the box rather than in its
//! border.

use doris::config::Config;
use doris::ui::app::App as UiApp;
use ratatui::backend::{Backend, TestBackend};
use ratatui::Terminal;

fn make_app() -> UiApp {
    UiApp::new("http://127.0.0.1:8090".into(), None)
}

/// Render `app` once at `w`x`h` and hand back the drawn rows plus the
/// caret position the frame asked for (`(0, 0)` means "no caret").
fn render(app: &mut UiApp, w: u16, h: u16) -> (Vec<String>, (u16, u16)) {
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal
        .draw(|frame| app.render(frame, &Config::default()))
        .unwrap();
    let pos = terminal.backend_mut().get_cursor_position().unwrap();
    let buf = terminal.backend().buffer();
    let rows = (0..buf.area.height)
        .map(|y| {
            (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect()
        })
        .collect();
    (rows, (pos.x, pos.y))
}

/// The first row inside the search box's border: where the typed text
/// is drawn (`SEARCH_BAR_HEIGHT` starts with a border, so row 1).
fn input_row(rows: &[String]) -> String {
    rows[1].trim().trim_matches('│').trim().to_string()
}

#[test]
fn test_caret_sits_after_the_typed_text() {
    let mut app = make_app();
    app.enter_input_mode();
    for c in "batman".chars() {
        app.type_char(c);
    }
    let (rows, pos) = render(&mut app, 120, 40);
    assert_eq!(input_row(&rows), "batman");
    // Border column, then one cell per character.
    assert_eq!(pos, (1 + 6, 1), "caret must sit after the text");
}

#[test]
fn test_no_caret_while_reading() {
    let mut app = make_app();
    let (_, pos) = render(&mut app, 120, 40);
    assert_eq!(pos, (0, 0), "an idle box must not show a caret");
}

#[test]
fn test_filter_text_is_drawn_in_the_box() {
    let mut app = make_app();
    app.search_input = "batman".to_string();
    app.enter_input_mode();
    app.exit_input_mode();
    app.zones.filter_mode = true;
    app.zones.filter_input = "abr".to_string();
    app.update_filter();

    let (rows, _) = render(&mut app, 120, 40);
    let inner = input_row(&rows);
    assert!(
        inner.starts_with("abr"),
        "the text being typed belongs in the box, not only in its \
         border title:\n{inner}"
    );
    assert!(
        !inner.contains("batman"),
        "while filtering, the box must not still show the stale query:\n{inner}"
    );
}

#[test]
fn test_caret_follows_the_filter_being_typed() {
    let mut app = make_app();
    app.zones.filter_mode = true;
    app.zones.filter_input = "abr".to_string();
    let (_, pos) = render(&mut app, 120, 40);
    assert_eq!(pos, (1 + 3, 1), "the caret belongs to the filter text");
}

#[test]
fn test_filter_mode_labels_the_mode_and_leaves_the_text_to_the_body() {
    let mut app = make_app();
    app.search_input = "batman".to_string();
    app.zones.filter_mode = true;
    app.zones.filter_input = "gotham".to_string();
    app.update_filter();

    let (rows, _) = render(&mut app, 120, 40);
    let border = &rows[0];
    assert!(
        border.contains("filter"),
        "the border names the mode: {border}"
    );
    assert!(
        !border.contains("gotham"),
        "the text is in the box already -- saying it twice is noise: {border}"
    );
    assert_eq!(input_row(&rows), "gotham");
}

#[test]
fn test_an_applied_filter_is_still_announced_on_the_border() {
    let mut app = make_app();
    app.search_input = "batman".to_string();
    app.zones.filter_mode = false;
    app.zones.filter_input = "gotham".to_string();
    app.update_filter();

    let (rows, _) = render(&mut app, 120, 40);
    assert!(
        rows[0].contains("filter: gotham"),
        "a filter in effect, not being edited, says what it is: {}",
        rows[0]
    );
    assert_eq!(input_row(&rows), "batman");
}

#[test]
fn test_idle_box_still_shows_the_query() {
    let mut app = make_app();
    app.search_input = "batman".to_string();
    app.zones.filter_mode = false;
    app.zones.filter_input = "gotham".to_string();
    app.update_filter();

    let (rows, pos) = render(&mut app, 120, 40);
    assert_eq!(input_row(&rows), "batman");
    assert_eq!(pos, (0, 0), "an applied filter is not an editing session");
}
