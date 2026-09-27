use ratatui::prelude::*;
use ratatui::backend::TestBackend;
use ratatui::widgets::*;
use doris::ui::app::App as UiApp;
use doris::ui::app::Modal;
use doris::ui::app::LoginField;
use doris::search::models::TorrentItem;
use doris::search::source::Group;

fn make_test_app() -> UiApp {
    UiApp::new(
        "http://127.0.0.1:8090".into(),
        "helium [visible] (/usr/bin/helium)".into(),
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
            date: format!("01-Янв-2{}", i),
            download_url: format!("/forum/dl.php?t={}", 1000 + i),
            page_url: format!("viewtopic.php?t={}", 1000 + i),
            query: "test".into(),
            ..Default::default()
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
    assert_eq!(app.modal, Modal::None);
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
fn test_mouse_scroll_logs() {
    let mut app = make_test_app();
    for i in 0..30 {
        app.add_log(&format!("msg {}", i));
    }
    app.mouse_scroll_logs(5);
    assert_eq!(app.log_scroll, 25);
    app.mouse_scroll_logs(-3);
    assert_eq!(app.log_scroll, 28);
}

#[test]
fn test_type_char() {
    let mut app = make_test_app();
    app.enter_input_mode();
    app.type_char('h');
    app.type_char('i');
    assert_eq!(app.search_input, "hi");
}

#[test]
fn test_type_char_not_in_input_mode() {
    let mut app = make_test_app();
    app.type_char('h');
    assert!(app.search_input.is_empty());
}

#[test]
fn test_backspace() {
    let mut app = make_test_app();
    app.enter_input_mode();
    app.type_char('a');
    app.type_char('b');
    app.backspace();
    assert_eq!(app.search_input, "a");
}

#[test]
fn test_delete_word() {
    let mut app = make_test_app();
    app.enter_input_mode();
    for c in "hello world".chars() {
        app.type_char(c);
    }
    app.delete_word();
    assert_eq!(app.search_input, "hello ");
}

#[test]
fn test_navigate_down() {
    let mut app = make_test_app();
    app.results = make_results(5);
    app.update_filter();
    assert!(app.navigate_down());
    assert_eq!(app.selected, 1);
}

#[test]
fn test_navigate_down_clamps() {
    let mut app = make_test_app();
    app.results = make_results(3);
    app.update_filter();
    app.selected = 2;
    app.all_loaded = true;
    assert!(!app.navigate_down());
    assert_eq!(app.selected, 2);
}

#[test]
fn test_navigate_up() {
    let mut app = make_test_app();
    app.results = make_results(5);
    app.update_filter();
    app.selected = 3;
    assert!(app.navigate_up());
    assert_eq!(app.selected, 2);
}

#[test]
fn test_navigate_up_clamps() {
    let mut app = make_test_app();
    app.results = make_results(5);
    assert!(app.navigate_up());
    assert_eq!(app.selected, 0);
}

#[test]
fn test_navigate_blocked_in_input_mode() {
    let mut app = make_test_app();
    app.results = make_results(5);
    app.enter_input_mode();
    assert!(!app.navigate_down());
    assert!(!app.navigate_up());
}

#[test]
fn test_navigate_blocked_in_modal() {
    let mut app = make_test_app();
    app.results = make_results(5);
    app.open_login_modal();
    assert!(!app.navigate_down());
    assert!(!app.navigate_up());
    assert_eq!(app.submit_selection(), None);
}

#[test]
fn test_submit_search() {
    let mut app = make_test_app();
    app.enter_input_mode();
    for c in "ubuntu".chars() {
        app.type_char(c);
    }
    assert_eq!(app.submit_search(), Some("ubuntu".into()));
    assert!(!app.input_mode);
}

#[test]
fn test_submit_search_empty() {
    let mut app = make_test_app();
    app.enter_input_mode();
    assert_eq!(app.submit_search(), None);
}

#[test]
fn test_submit_selection() {
    let mut app = make_test_app();
    app.results = make_results(5);
    app.update_filter();
    app.selected = 2;
    assert_eq!(app.submit_selection(), Some(2));
}

#[test]
fn test_submit_selection_empty() {
    let mut app = make_test_app();
    assert_eq!(app.submit_selection(), None);
}

#[test]
fn test_quit() {
    let mut app = make_test_app();
    app.quit();
    assert!(!app.running);
}

#[test]
fn test_login_modal_open_close() {
    let mut app = make_test_app();
    app.open_login_modal();
    assert!(matches!(app.modal, Modal::Login(_)));
    app.close_login_modal();
    assert_eq!(app.modal, Modal::None);
}

#[test]
fn test_login_modal_typing() {
    let mut app = make_test_app();
    app.open_login_modal();

    for c in "black".chars() {
        let key = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char(c),
            crossterm::event::KeyModifiers::NONE,
        );
        app.login_modal_key(key);
    }

    if let Modal::Login(ref state) = app.modal {
        assert_eq!(state.username, "black");
        assert_eq!(state.focus, LoginField::Username);
    } else {
        panic!("Expected login modal");
    }
}

#[test]
fn test_login_modal_tab_switches_field() {
    let mut app = make_test_app();
    app.open_login_modal();

    if let Modal::Login(ref state) = app.modal {
        assert_eq!(state.focus, LoginField::Username);
    }

    let key = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Tab,
        crossterm::event::KeyModifiers::NONE,
    );
    app.login_modal_key(key);

    if let Modal::Login(ref state) = app.modal {
        assert_eq!(state.focus, LoginField::Password);
    }

    let key = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Tab,
        crossterm::event::KeyModifiers::NONE,
    );
    app.login_modal_key(key);

    if let Modal::Login(ref state) = app.modal {
        assert_eq!(state.focus, LoginField::Username);
    }
}

#[test]
fn test_login_modal_enter_submits() {
    let mut app = make_test_app();
    app.open_login_modal();

    for c in "user".chars() {
        let key = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char(c),
            crossterm::event::KeyModifiers::NONE,
        );
        app.login_modal_key(key);
    }

    let key = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Tab,
        crossterm::event::KeyModifiers::NONE,
    );
    app.login_modal_key(key);

    for c in "pass123".chars() {
        let key = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char(c),
            crossterm::event::KeyModifiers::NONE,
        );
        app.login_modal_key(key);
    }

    let key = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Enter,
        crossterm::event::KeyModifiers::NONE,
    );
    let result = app.login_modal_key(key);

    assert_eq!(result, Some(("rutracker", "user".into(), "pass123".into())));
    assert_eq!(app.modal, Modal::None);
}

/// The resource tab is part of what Enter submits: the credentials
/// belong to the selected resource, so the tuple carries its id.
#[test]
fn test_login_modal_enter_submits_the_selected_resource() {
    let mut app = make_test_app();
    app.open_login_modal();

    if let Modal::Login(ref state) = app.modal {
        assert_eq!(state.resource, "rutracker", "the first tab is selected");
    }

    // With one resource the tab wraps onto itself; the point is that
    // the id travels with the credentials.
    let right = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Right,
        crossterm::event::KeyModifiers::NONE,
    );
    app.login_modal_key(right);
    let left = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Left,
        crossterm::event::KeyModifiers::NONE,
    );
    app.login_modal_key(left);

    if let Modal::Login(ref state) = app.modal {
        assert_eq!(state.resource, "rutracker", "one tab wraps onto itself");
    }
}

/// Ctrl+S saves the current tab's credentials without logging in: the
/// modal stays open and says so. The store write is real, so the test
/// backs up whatever was saved and puts it back -- an offline test must
/// not clobber the user's login.
#[test]
fn test_login_modal_ctrl_s_saves_without_logging_in() {
    let mut app = make_test_app();
    app.open_login_modal();

    for c in "saved-user".chars() {
        app.login_modal_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char(c),
            crossterm::event::KeyModifiers::NONE,
        ));
    }
    app.login_modal_key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Tab,
        crossterm::event::KeyModifiers::NONE,
    ));
    for c in "secret".chars() {
        app.login_modal_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char(c),
            crossterm::event::KeyModifiers::NONE,
        ));
    }

    let before = doris::credentials::load_credential("rutracker");

    let ctrl_s = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('s'),
        crossterm::event::KeyModifiers::CONTROL,
    );
    let result = app.login_modal_key(ctrl_s);

    assert_eq!(result, None, "saving does not close the modal");
    assert!(matches!(app.modal, Modal::Login(_)), "the modal stays open");
    if let Modal::Login(ref state) = app.modal {
        assert_eq!(state.message.as_deref(), Some("Saved"));
    }

    // And the store really holds them, under the tab's resource.
    let saved = doris::credentials::load_credential("rutracker")
        .expect("Ctrl+S must write the store");
    assert_eq!(saved.0, "saved-user");
    assert_eq!(saved.1, "secret");

    // Put back exactly what was there before.
    match before {
        Some((user, pass)) => {
            let _ = doris::credentials::save_credential("rutracker", &user, &pass);
        }
        None => {
            let _ = doris::credentials::delete_credential("rutracker");
        }
    }
}

#[test]
fn test_login_modal_enter_empty_fails() {
    let mut app = make_test_app();
    app.open_login_modal();

    let key = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Enter,
        crossterm::event::KeyModifiers::NONE,
    );
    let result = app.login_modal_key(key);

    assert_eq!(result, None);
    assert!(matches!(app.modal, Modal::Login(_)));
}

#[test]
fn test_login_modal_esc_closes() {
    let mut app = make_test_app();
    app.open_login_modal();

    let key = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Esc,
        crossterm::event::KeyModifiers::NONE,
    );
    app.login_modal_key(key);

    assert_eq!(app.modal, Modal::None);
}

#[test]
fn test_login_modal_backspace() {
    let mut app = make_test_app();
    app.open_login_modal();

    let key = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('x'),
        crossterm::event::KeyModifiers::NONE,
    );
    app.login_modal_key(key);

    let key = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Backspace,
        crossterm::event::KeyModifiers::NONE,
    );
    app.login_modal_key(key);

    if let Modal::Login(ref state) = app.modal {
        assert!(state.username.is_empty());
    }
}

#[test]
fn test_login_modal_password_shows_asterisks() {
    let mut app = make_test_app();
    app.open_login_modal();

    let key = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Tab,
        crossterm::event::KeyModifiers::NONE,
    );
    app.login_modal_key(key);

    for c in "secret".chars() {
        let key = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char(c),
            crossterm::event::KeyModifiers::NONE,
        );
        app.login_modal_key(key);
    }

    if let Modal::Login(ref state) = app.modal {
        assert_eq!(state.password, "secret");
    } else {
        panic!("Expected login modal");
    }
}

#[test]
fn test_full_flow() {
    let mut app = make_test_app();
    app.enter_input_mode();
    for c in "world war".chars() {
        app.type_char(c);
    }
    assert_eq!(app.submit_search(), Some("world war".into()));
    app.results = (0..50)
        .map(|i| TorrentItem {
            title: format!("Result {}", i),
            size: "1 GB".into(),
            seeds: format!("{}", i),
            date: "".into(),
            download_url: format!("/dl.php?t={}", i),
            page_url: "".into(),
            query: "world war".into(),
            ..Default::default()
        })
        .collect();
    app.update_filter();
    for _ in 0..10 {
        app.navigate_down();
    }
    assert_eq!(app.submit_selection(), Some(10));
}

#[test]
fn test_render_does_not_panic() {
    let mut app = make_test_app();
    app.results = make_results(10);
    app.update_filter();
    app.add_log("test log");

    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| app.render(frame)).unwrap();
}

#[test]
fn test_render_empty_state() {
    let mut app = make_test_app();
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| app.render(frame)).unwrap();
}

#[test]
fn test_render_with_many_results() {
    let mut app = make_test_app();
    app.results = make_results(100);
    app.update_filter();
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
fn test_render_with_modal() {
    let mut app = make_test_app();
    app.open_login_modal();

    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| app.render(frame)).unwrap();
}

#[test]
fn test_state_searching() {
    let mut app = make_test_app();
    app.state = doris::ui::app::AppState::Searching;
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| app.render(frame)).unwrap();
}

#[test]
fn test_state_streaming() {
    let mut app = make_test_app();
    app.state = doris::ui::app::AppState::Streaming;
    app.results = make_results(3);
    app.update_filter();
    app.selected = 1;
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| app.render(frame)).unwrap();
}

/// B6's instant half: picking a category re-derives the view from the
/// rows already on screen, and rows no source could attribute
/// (`group = None`) belong to the "all" view alone.
#[test]
fn test_switching_the_category_rederives_the_view_instantly() {
    let mut app = make_test_app();
    let mut results = make_results(4);
    results[0].group = Some(Group::Movies);
    results[1].group = Some(Group::TV);
    results[2].group = None; // a source that could not attribute it
    results[3].group = Some(Group::Movies);
    app.results = results;
    app.update_filter();
    assert_eq!(
        app.filtered_indices,
        vec![0, 1, 2, 3],
        "the all view keeps every row"
    );

    app.set_group(Some(Group::Movies));
    assert_eq!(
        app.filtered_indices,
        vec![0, 3],
        "only Movies: TV is out, and the unattributed row claims nothing"
    );
    assert!(app.group_changed, "Enter still owes the server-side search");

    app.set_group(None);
    assert_eq!(
        app.filtered_indices,
        vec![0, 1, 2, 3],
        "back to all restores every row -- nothing was discarded"
    );
}

/// The row exists on screen and not only in state: the table's own
/// header moved down a line to make room for it.
#[test]
fn test_render_draws_the_category_row_under_the_source_row() {
    let mut app = make_test_app();
    app.zones.update_areas(Rect::new(0, 0, 120, 40));
    let results = app.zones.get_area(doris::ui::zones::ZoneId::Results);

    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| app.render(frame)).unwrap();
    let buf = terminal.backend().buffer();
    let row_text = |y: u16| -> String {
        (0..buf.area.width)
            .filter_map(|x| buf.cell((x, y)).map(|c| c.symbol().chars().next().unwrap_or(' ')))
            .collect()
    };

    let source_row = row_text(results.y + 1);
    assert!(source_row.contains("rutracker"), "source row: {source_row}");

    // "all" is the selected category on a fresh app, so it is the
    // bracketed one here -- the row below the source row, and only it.
    let category_row = row_text(results.y + 2);
    assert!(category_row.contains("[all]"), "category row: {category_row}");
    for group in ["Movies", "TV", "Games", "Anime"] {
        assert!(category_row.contains(group), "missing {group}: {category_row}");
    }
    assert!(
        !category_row.contains("rutracker"),
        "a separate line, not a repeat of the row above: {category_row}"
    );
    assert!(
        row_text(results.y + 3).contains("Seeds"),
        "the table header sits one row lower now"
    );
}
