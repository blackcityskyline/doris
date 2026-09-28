//! The help page (UI_REFACTOR_PLAN §3): btop's `helpMenu`
//! (`btop_menu.cpp:1743`) rebuilt as a modal.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use doris::ui::app::App as UiApp;
use doris::ui::modals::help::HELP_TEXT;
use ratatui::backend::TestBackend;
use doris::config::Config;
use ratatui::Terminal;

fn make_app() -> UiApp {
    UiApp::new(
        "http://127.0.0.1:8090".into(),
        true,
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

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

/// Render `app` once at `w`x`h` -- the same pass that publishes the page
/// count `help_key` clamps against -- and hand back the drawn rows.
fn render(app: &mut UiApp, w: u16, h: u16) -> Vec<String> {
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal.draw(|frame| app.render(frame, &Config::default())).unwrap();
    let buf = terminal.backend().buffer();
    (0..buf.area.height)
        .map(|y| {
            (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect()
        })
        .collect()
}

/// btop draws `[key, description]` pairs from one table
/// (`help_text`, `btop_menu.cpp:174`). Ours has to keep naming the keys
/// AGENTS.md documents, or the page silently falls behind the bindings.
#[test]
fn test_help_text_names_the_documented_keybinds() {
    let keys: Vec<&str> = HELP_TEXT.iter().map(|(k, _)| *k).collect();
    for expected in [
        "s, i", "Enter", "Shift+Enter", "b", "S", "L", "F", "f", "m", "1, 2, 3, 4, 5",
        "Shift+P", "Tab, Shift+Tab", "j, k, Up, Down", "g, G", "d", "v", "p", "Esc",
        "q, ctrl + c", "? , /, F1",
    ] {
        assert!(
            keys.iter().any(|k| k == &expected),
            "HELP_TEXT is missing '{}'; it must match AGENTS.md",
            expected
        );
    }
}

/// The description column is whatever is left of the box after the
/// 20-column key column, and the box is 90% of an 80-column terminal --
/// 70 inner columns, 50 for the description. btop truncates silently if
/// it doesn't fit; ours should simply not have anything that long.
#[test]
fn test_help_descriptions_fit_an_80_column_terminal() {
    for (key, desc) in HELP_TEXT {
        assert!(
            desc.chars().count() <= 50,
            "'{}' description is {} chars and would be cut off: {}",
            key,
            desc.chars().count(),
            desc
        );
    }
}

#[test]
fn test_help_opens_at_the_top() {
    let mut app = make_app();
    app.open_help_modal();
    let state = match &app.modal {
        doris::ui::app::Modal::Help(s) => s.clone(),
        other => panic!("expected the help modal, got {:?}", std::mem::discriminant(other)),
    };
    assert_eq!(state.page, 0);
}

/// btop's `helpMenu` closes on `escape`, `q`, `h`, `backspace`, `space`
/// and `enter` (`btop_menu.cpp:1770`) -- every one of them has to work,
/// and anything else has to leave the page open.
#[test]
fn test_help_closes_on_the_close_keys_only() {
    for code in [
        KeyCode::Esc,
        KeyCode::Char('q'),
        KeyCode::Char('h'),
        KeyCode::Char(' '),
        KeyCode::Enter,
        KeyCode::Backspace,
    ] {
        let mut app = make_app();
        app.open_help_modal();
        app.help_key(key(code));
        assert_eq!(app.modal, doris::ui::app::Modal::None, "{:?} closes", code);
    }

    let mut app = make_app();
    app.open_help_modal();
    app.help_key(key(KeyCode::Char('x')));
    assert_ne!(app.modal, doris::ui::app::Modal::None, "'x' does nothing");
}

/// The page count only exists once the renderer has measured the box, so
/// paging has to be exercised the way a user gets it: draw, then press a
/// key. 80x24 leaves room for 18 rows of the 21-entry table -- two pages.
#[test]
fn test_help_pages_forward_and_wraps() {
    let mut app = make_app();
    app.open_help_modal();
    let rows = render(&mut app, 80, 24);

    let page_indicator = |app: &UiApp| match &app.modal {
        doris::ui::app::Modal::Help(s) => s.page,
        _ => panic!("help modal is gone"),
    };
    assert!(
        rows.iter().any(|r| r.contains("↑ page 1/2 ↓")),
        "the page indicator is on the bottom border"
    );
    assert_eq!(page_indicator(&app), 0);

    app.help_key(key(KeyCode::Char('j')));
    assert_eq!(page_indicator(&app), 1);
    app.help_key(key(KeyCode::Char('j')));
    assert_eq!(page_indicator(&app), 0, "the last page wraps to the first");

    app.help_key(key(KeyCode::Char('k')));
    assert_eq!(page_indicator(&app), 1, "up wraps the other way");
    app.help_key(key(KeyCode::PageDown));
    assert_eq!(page_indicator(&app), 0);
    app.help_key(key(KeyCode::PageUp));
    assert_eq!(page_indicator(&app), 1);
}

/// Nothing to page through means nothing for the arrow keys to do --
/// btop guards the same way (`else if (pages > 1 and ...)`).
#[test]
fn test_help_does_not_page_when_it_all_fits() {
    let mut app = make_app();
    app.open_help_modal();
    let rows = render(&mut app, 120, 50);

    assert!(
        rows.iter().any(|r| r.contains("Key:") && r.contains("Description:")),
        "the header row is drawn"
    );
    assert!(
        rows.iter().any(|r| r.contains("Shows this window.")),
        "the whole table fits on one page"
    );
    assert!(
        rows.iter().all(|r| !r.contains("↑ page")),
        "no page indicator when there is one page"
    );

    app.help_key(key(KeyCode::Char('j')));
    match &app.modal {
        doris::ui::app::Modal::Help(s) => assert_eq!(s.page, 0),
        other => panic!("help modal is gone: {:?}", std::mem::discriminant(other)),
    }
}

/// The reason the page exists: a two-column table, key in `hi_fg` +
/// bold, description after it. If the header or the keys stopped being
/// drawn, this is the test that notices.
#[test]
fn test_help_draws_the_header_and_the_keys() {
    let mut app = make_app();
    app.open_help_modal();
    let rows = render(&mut app, 100, 40);

    let header = rows.iter().find(|r| r.contains("Key:"))
        .expect("header row");
    header.find("Description:").expect("Description column");

    // Every visible key sits in the same 20-column column as `Key:`:
    // `cjust(..., 20)` on both sides is what lines the table up. The
    // column is counted in display cells, not bytes -- the rows in front
    // of the popup carry different prefixes (`╭ ¹ │` vs `│[ru│`), so a
    // byte offset taken from the header would land mid-glyph elsewhere.
    // The key is read out of that column rather than searched for in the
    // whole row, or "Enter" would match inside "Enters search input
    // mode." on the row above it.
    let key_col = header[..header.find("Key:").unwrap()].chars().count() - 8;
    let page = &rows[rows.iter().position(|r| r.contains("Key:")).unwrap()..];
    for expected in ["s, i", "Enter", "Esc", "? , /, F1"] {
        let found = page
            .iter()
            .any(|r| r.chars().skip(key_col).take(20).collect::<String>().contains(expected));
        assert!(found, "'{}' is not in the key column of page 1", expected);
    }
}
