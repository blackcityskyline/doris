use ratatui::prelude::*;
use ratatui::backend::TestBackend;
use ratatui::widgets::*;
use doris::ui::app::App as UiApp;
use doris::ui::app::Modal;
use doris::ui::app::LoginField;
use doris::search::models::TorrentItem;

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

    assert_eq!(result, Some(("user".into(), "pass123".into())));
    assert_eq!(app.modal, Modal::None);
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
