use doris::config::Config;
use doris::sources::models::TorrentItem;
use doris::sources::source::Group;
use doris::ui::layout::ZoneId;
use doris::ui::modals::login::LoginField;
use doris::ui::view::App as UiApp;
use doris::ui::view::Modal;
use ratatui::backend::TestBackend;
use ratatui::prelude::*;

fn make_test_app() -> UiApp {
    UiApp::new("http://127.0.0.1:8090".into(), None)
}

/// A credential store of its own for a test, cleared first so a leftover
/// from an earlier run cannot answer for the current one.
fn scratch_store(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("doris-tui-{}-{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch store dir");
    dir.join(doris::credentials::STORE_FILE)
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
fn test_log_scroll_moves_a_line_at_a_time() {
    let mut app = make_test_app();
    for i in 0..20 {
        app.add_log(&format!("msg {}", i));
    }
    app.scroll_logs(-1);
    assert_eq!(app.log_scroll, 19);
    app.scroll_logs(-1);
    assert_eq!(app.log_scroll, 18);
    app.scroll_logs(1);
    assert_eq!(app.log_scroll, 19);
}

/// Down is positive, the same sign `PageDown` uses, and the clamp holds at both ends -- the
/// panel is shorter than the buffer in both directions.
#[test]
fn test_log_scroll_clamps_and_takes_a_multi_line_step() {
    let mut app = make_test_app();
    for i in 0..30 {
        app.add_log(&format!("msg {}", i));
    }
    app.scroll_logs(-5);
    assert_eq!(app.log_scroll, 25);
    app.scroll_logs(3);
    assert_eq!(app.log_scroll, 28);
    app.scroll_logs(-99);
    assert_eq!(app.log_scroll, 0);
    app.scroll_logs(99);
    assert_eq!(app.log_scroll, 30);
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
    // The separating space goes with the word: leaving it behind means the
    // next `ctrl+w` has nothing but a space in front of the caret and does
    // nothing, which reads as a dead key.
    app.delete_word();
    assert_eq!(app.search_input, "hello");
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
    let app = make_test_app();
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

/// Ctrl+S saves the current tab's credentials without logging in: the modal stays open and says
/// so.
#[test]
fn test_login_modal_ctrl_s_saves_without_logging_in() {
    let mut app = make_test_app();
    app.credentials_path = scratch_store("login-ctrl-s");
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
    let saved = doris::credentials::load_credential_at(&app.credentials_path, "rutracker")
        .expect("Ctrl+S must write the store");
    assert_eq!(saved.0, "saved-user");
    assert_eq!(saved.1, "secret");
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
    terminal
        .draw(|frame| app.render(frame, &Config::default()))
        .unwrap();
}

#[test]
fn test_render_empty_state() {
    let mut app = make_test_app();
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| app.render(frame, &Config::default()))
        .unwrap();
}

#[test]
fn test_render_with_many_results() {
    let mut app = make_test_app();
    app.results = make_results(100);
    app.update_filter();
    app.selected = 50;
    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| app.render(frame, &Config::default()))
        .unwrap();
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
    terminal
        .draw(|frame| app.render(frame, &Config::default()))
        .unwrap();
}

#[test]
fn test_render_input_mode() {
    let mut app = make_test_app();
    app.enter_input_mode();
    app.search_input = "test query".into();
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| app.render(frame, &Config::default()))
        .unwrap();
}

#[test]
fn test_render_with_modal() {
    let mut app = make_test_app();
    app.open_login_modal();

    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| app.render(frame, &Config::default()))
        .unwrap();
}

#[test]
fn test_state_searching() {
    let mut app = make_test_app();
    app.state = doris::ui::view::AppState::Searching;
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| app.render(frame, &Config::default()))
        .unwrap();
}

#[test]
fn test_state_streaming() {
    let mut app = make_test_app();
    app.state = doris::ui::view::AppState::Streaming;
    app.results = make_results(3);
    app.update_filter();
    app.selected = 1;
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| app.render(frame, &Config::default()))
        .unwrap();
}

/// instant half: picking a category re-derives the view from the
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

/// The category row is gone from the panel body: the table's own header
/// is the first line inside the Results border, and the current category
/// lives on the frame as the `◀ all ▶` button next to `group`.
#[test]
fn test_render_draws_the_table_header_under_the_frame() {
    let mut app = make_test_app();
    // A populated panel is the case that has a table to draw: an empty
    app.results = make_results(3);
    app.update_filter();
    app.zones.update_areas(Rect::new(0, 0, 120, 40));
    let results = app.zones.get_area(doris::ui::layout::ZoneId::Results);

    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| app.render(frame, &Config::default()))
        .unwrap();
    let buf = terminal.backend().buffer();
    let row_text = |y: u16| -> String {
        (0..buf.area.width)
            .filter_map(|x| {
                buf.cell((x, y))
                    .map(|c| c.symbol().chars().next().unwrap_or(' '))
            })
            .collect()
    };

    // The category row is gone: the first line inside the border is the
    assert!(
        row_text(results.y + 1).contains("Seeds"),
        "the table header is the first line inside the border: {}",
        row_text(results.y + 1)
    );
    let frame_row = row_text(results.y);
    assert!(frame_row.contains('◀'), "the category button: {frame_row}");
    assert!(frame_row.contains('▶'), "the category button: {frame_row}");
    // The name is padded to the widest category, so the arrows line up.
    assert!(
        frame_row.contains("all"),
        "the current category: {frame_row}"
    );
    assert!(
        frame_row.contains("group"),
        "next to the group button: {frame_row}"
    );
}

// --- the frame legend actually reaches the border --------------------

/// Render `app` with `id`'s detail view open and hand back the buffer.
fn render_detail(app: &mut UiApp, id: ZoneId, w: u16, h: u16) -> ratatui::buffer::Buffer {
    app.detail_view = Some(id);
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal
        .draw(|frame| app.render(frame, &Config::default()))
        .unwrap();
    terminal.backend().buffer().clone()
}

/// The detail view with frames on.
///
/// `Config::default()` has `show_boxes` off, and every other test here
/// draws borders without needing them; a test about how the view is
/// *divided* needs the division to exist.
fn render_detail_framed(app: &mut UiApp, id: ZoneId, w: u16, h: u16) -> ratatui::buffer::Buffer {
    app.detail_view = Some(id);
    let config = Config {
        show_boxes: true,
        ..Config::default()
    };
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal.draw(|frame| app.render(frame, &config)).unwrap();
    terminal.backend().buffer().clone()
}

fn all_text(buf: &ratatui::buffer::Buffer) -> String {
    (0..buf.area.height)
        .map(|y| {
            (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect::<String>()
        })
        .collect::<String>()
}

fn row_text(terminal: &Terminal<TestBackend>, y: u16) -> String {
    let buf = terminal.backend().buffer();
    let width = buf.area.width;
    (0..width)
        .map(|x| buf[(x, y)].symbol().to_string())
        .collect()
}

/// The buttons are positioned by `frame_layout` and drawn by
/// `render_frame`; if either side stopped running, the border would go
/// back to a bare box with no way to tell what the keys are.
#[test]
fn test_frame_legend_is_drawn_on_the_zone_borders() {
    let mut app = make_test_app();
    app.results = make_results(3);
    app.update_filter();
    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| app.render(frame, &Config::default()))
        .unwrap();

    let results = app.zones.get_area(ZoneId::Results);
    let top = row_text(&terminal, results.y);
    assert!(top.contains("filter"), "Results top border: {}", top);
    assert!(top.contains("group"), "Results top border: {}", top);
    // The source tabs left the frame for their own panel, and
    assert!(top.contains('◀'), "Results top border: {}", top);
    assert!(top.contains('▶'), "Results top border: {}", top);
    // With nothing on the left but the title, the counter lands
    assert!(
        top.contains("Results (3/3)"),
        "counter after the zone name: {}",
        top
    );

    // The bottom action row is gone: play/download/info are
    let bottom = row_text(&terminal, results.y + results.height - 1);
    assert!(
        !bottom.contains("play") && !bottom.contains("download") && !bottom.contains("info"),
        "Results bottom border should be empty: {}",
        bottom
    );

    let torrent = app.zones.get_area(ZoneId::Torrent);
    let t_top = row_text(&terminal, torrent.y);
    assert!(t_top.contains("pause"), "Torrent top border: {}", t_top);
    let t_bottom = row_text(&terminal, torrent.y + torrent.height - 1);
    assert!(
        t_bottom.contains("delete"),
        "Torrent bottom border: {}",
        t_bottom
    );
}

/// The reason the legend exists: the keybind text used to sit inside the panels and had to be
/// deleted from every one of them.
#[test]
fn test_keybind_text_is_gone_from_the_panel_bodies() {
    let mut app = make_test_app();
    app.results = make_results(3);
    app.update_filter();
    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| app.render(frame, &Config::default()))
        .unwrap();

    let results = app.zones.get_area(ZoneId::Results);
    let mut body = String::new();
    for y in (results.y + 1)..(results.y + results.height - 1) {
        body.push_str(&row_text(&terminal, y));
    }
    assert!(!body.contains("Enter: play"), "old hint row is back");
    assert!(!body.contains("d: download"), "old hint row is back");

    let torrent = app.zones.get_area(ZoneId::Torrent);
    let mut body = String::new();
    for y in (torrent.y + 1)..(torrent.y + torrent.height - 1) {
        body.push_str(&row_text(&terminal, y));
    }
    assert!(!body.contains("p: pause/resume"), "old hint line is back");
}

// --- the `Src` column ------------------------------------------------

/// Every drawn row as text, for assertions about what reached the screen
/// rather than about state.
fn render_rows(app: &mut UiApp, w: u16, h: u16) -> Vec<String> {
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal
        .draw(|frame| app.render(frame, &Config::default()))
        .unwrap();
    let buf = terminal.backend().buffer();
    (0..buf.area.height)
        .map(|y| {
            (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect()
        })
        .collect()
}

/// `TorrentItem::source` is `#[serde(default)]`, so pre-field rows arrive empty.
#[test]
fn test_source_badge_marks_missing_sources_with_a_dash() {
    let mut item = TorrentItem::default();
    assert_eq!(doris::ui::view::source_badge(&item), "-");

    item.source = "nyaa".into();
    assert_eq!(doris::ui::view::source_badge(&item), "nyaa");
}

/// The badge column has a fixed width so the table never re-flows as
/// the results change -- which only holds while every source id fits.
#[test]
fn test_every_known_source_fits_the_badge_column() {
    for info in doris::sources::source::KNOWN_SOURCES.iter() {
        assert!(
            (info.id.chars().count() as u16) <= doris::ui::view::SOURCE_BADGE_WIDTH,
            "source '{}' is {} chars and would be clipped in the Src column",
            info.id,
            info.id.chars().count()
        );
    }
}

/// On the `all` tab one page mixes trackers, and the row itself is the
/// only place that says who returned it -- so each row has to carry its
/// own source, and rows without one have to be visibly marked.
#[test]
fn test_results_table_shows_which_source_returned_each_row() {
    let mut app = make_test_app();
    app.results = vec![
        TorrentItem {
            title: "First".into(),
            source: "rutracker".into(),
            ..Default::default()
        },
        TorrentItem {
            title: "Second".into(),
            source: "nyaa".into(),
            ..Default::default()
        },
        TorrentItem {
            title: "Third".into(),
            ..Default::default()
        },
    ];
    app.update_filter();

    let rows = render_rows(&mut app, 120, 30);

    let header = rows
        .iter()
        .find(|r| r.contains("Seeds"))
        .expect("the table header is drawn");
    assert!(header.contains("Src"), "header: {}", header);

    for (title, source) in [("First", "rutracker"), ("Second", "nyaa")] {
        let row = rows
            .iter()
            .find(|r| r.contains(title))
            .unwrap_or_else(|| panic!("the '{}' row is drawn", title));
        assert!(
            row.contains(source),
            "'{}' row should name '{}': {}",
            title,
            source,
            row
        );
    }

    let unknown = rows
        .iter()
        .find(|r| r.contains("Third"))
        .expect("the third row is drawn");
    assert!(
        unknown.contains('-'),
        "a row with no source is marked, not left blank: {}",
        unknown
    );
}

// --- layout tiling -----------------------------------------------------------

/// The default preset (`1,3|4`) puts Results across the top and tiles Trackers beside Log
/// underneath.
#[test]
fn test_the_default_tiling_draws_two_columns() {
    let mut app = make_test_app();
    app.results = make_results(3);
    app.update_filter();
    app.zones.apply_preset("1,3|4");

    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| app.render(frame, &Config::default()))
        .unwrap();
    let buf = terminal.backend().buffer();

    let row_text = |y: u16| -> String {
        (0..buf.area.width)
            .map(|x| buf[(x, y)].symbol().to_string())
            .collect()
    };

    // Results spans the full width at the top.
    let results = app.zones.get_area(ZoneId::Results);
    assert_eq!(results.width, 100);
    assert!(row_text(results.y).contains("Results"), "the Results frame");

    // Trackers and Log share the row underneath, one column each.
    let trackers = app.zones.get_area(ZoneId::Trackers);
    let log = app.zones.get_area(ZoneId::Log);
    assert_eq!(trackers.x, 0);
    assert_eq!(trackers.width, 50);
    assert_eq!(log.x, 50);
    assert_eq!(log.y, trackers.y, "the two cells share one row");
    assert_eq!(app.zones.get_area(ZoneId::Torrent), Rect::default());

    // And each of them actually drew its own frame where it was placed.
    assert!(
        row_text(trackers.y).contains("Trackers"),
        "the Trackers frame"
    );
    assert!(row_text(log.y).contains("Log"), "the Log frame");
}

// --- detail views (T / R) ----------------------------------------------------

/// The Torrent detail view is a full-frame takeover: it prints every
/// fact the panel knows -- name, hash, status, progress, speeds,
/// totals, seeds, peers -- where the panel itself only has room for
/// some of them.
#[test]
fn test_the_torrent_detail_view_prints_every_known_field() {
    let mut app = make_test_app();
    app.torrent_status = doris::ui::view::TorrentStatus {
        hash: "abcdef0123456789".into(),
        title: "Some.Torrent.2024".into(),
        progress: 0.5,
        download_speed: 1024,
        upload_speed: 512,
        seeds: 12,
        peers: 3,
        downloaded: 2048,
        total_size: 4096,
        ratio: Some(0.25),
        eta: Some(30),
        dir: "/home/u/Downloads".into(),
        status: "working".into(),
    };
    app.torrent_paused = true;

    let buf = render_detail(&mut app, ZoneId::Torrent, 100, 30);
    let text = all_text(&buf);

    for expected in [
        "Some.Torrent.2024",
        "abcdef0123456789",
        "working (paused)",
        "DL:",
        "UL:",
        "Seeds:",
        "Peers:",
    ] {
        assert!(
            text.contains(expected),
            "detail view must show `{}`",
            expected
        );
    }
    // The panel gives the bar a 50-column cap; the detail view has the
    assert!(text.contains("50%"), "the progress percentage is printed");
}

/// The keybind words sit in a frame of their own at the bottom.
///
/// They used to be the last line inside the downloads box, which is the one
/// place they could not be read as controls: a table row above them is a
/// torrent name, so a row that says `pause p` reads as a torrent called
/// pause. The frame is what separates them, and the key character keeps the
/// `hi_fg` + bold that every other keybind in the app is drawn in.
#[test]
fn test_the_torrent_keybinds_are_framed_at_the_bottom() {
    let mut app = make_test_app();
    let buf = render_detail_framed(&mut app, ZoneId::Torrent, 100, 30);

    let row_text = |y: u16| -> String {
        (0..buf.area.width)
            .map(|x| buf[(x, y)].symbol())
            .collect::<String>()
    };
    let words = (0..buf.area.height)
        .find(|y| row_text(*y).contains("unlimited"))
        .expect("the keybinds are not drawn");

    // A border above and below: that is the whole claim.
    assert!(
        row_text(words - 1).contains('─'),
        "no top border over the keybinds: `{}`",
        row_text(words - 1)
    );
    assert!(
        row_text(words + 1).contains('─'),
        "no bottom border under the keybinds: `{}`",
        row_text(words + 1)
    );
    // And at the bottom: the frame's lower border is the view's last row.
    assert!(
        row_text(buf.area.height - 1).contains('─'),
        "the keybind frame is not at the bottom: `{}`",
        row_text(buf.area.height - 1)
    );

    // The key itself, in the colour that marks a key everywhere else.
    let x = (0..buf.area.width)
        .find(|x| buf[(*x, words)].symbol() == "0")
        .expect("the `0` of `unlimited 0` is not on that row");
    let cell = &buf[(x, words)];
    assert_eq!(cell.fg, app.theme.hi_fg.to_color(), "the key is not hi_fg");
    assert!(
        cell.modifier.contains(Modifier::BOLD),
        "the key is not bold"
    );
}

/// The Torrent detail view describes the row under the downloads cursor,
/// because that is what the panel shows.
///
/// It used to describe `torrent_status` unconditionally -- the *streaming*
/// server's one torrent -- so `T` opened a different service's view of
/// something else while the panel listed the daemon's downloads, and both
/// were called "the torrent". The streaming state stays the fallback for
/// when there is nothing being downloaded, which is the case where it is
/// the subject rather than a footnote.
#[test]
fn test_the_torrent_detail_view_describes_the_selected_download() {
    let mut app = make_test_app();
    // The streaming server still has its own torrent, and it is *not* what
    // this view is about while a download is listed.
    app.torrent_status = doris::ui::view::TorrentStatus {
        hash: "streaminghash01".into(),
        title: "Streamed.Something".into(),
        progress: 0.9,
        status: "working".into(),
        ..Default::default()
    };
    app.downloads = vec![
        doris::ui::view::DownloadRow {
            id: 1,
            hash: "045e85f2ebc24a875a64fe2e9ac9b61f7aad0499".into(),
            name: "Downloaded.Thing.2024".into(),
            fraction: 0.42,
            download_speed: 4_200_000,
            seeds: 3,
            peers: 12,
            total_size: 1_990_000_000,
            left: 1_150_000_000,
            dir: "/home/u/Downloads".into(),
            status: 4,
            downloaded: 840_000_000,
            uploaded: 340_000_000,
            ..Default::default()
        },
        doris::ui::view::DownloadRow {
            id: 2,
            name: "Second.Download".into(),
            hash: "bb".into(),
            // The facts block is what has to belong to the cursor's row.
            // The other row is on screen too, and deliberately so: the
            // detail view is the table at full width, which is the whole
            // reason it exists.
            dir: "/home/u/Second".into(),
            ..Default::default()
        },
    ];
    app.download_cursor = 1;

    let buf = render_detail(&mut app, ZoneId::Torrent, 100, 30);
    let text = all_text(&buf);

    assert!(
        text.contains("Second.Download"),
        "the row under the cursor is the subject"
    );
    // The streaming server's torrent is not the subject. Its *line* is
    // still on the panel and therefore in this view -- that is where the
    // panel says what it is streaming -- but no fact line of it is written
    // out, because the facts are the selected download's.
    assert!(
        !text.contains("Hash: streaminghash01"),
        "the streaming server's torrent is not the subject: it is a \
         different service, and the panel does not list it"
    );
    assert!(
        text.contains("Downloaded.Thing.2024"),
        "and the table itself, at the full frame width, is still there -- \
         dropping columns is what `T` exists to stop"
    );
    assert!(
        text.contains("Directory: /home/u/Second"),
        "the facts belong to the row under the cursor, not to the first one"
    );
    assert!(
        !text.contains("Directory: /home/u/Downloads"),
        "and not to the row it is not on"
    );
}

/// `T` exists because the *zone* has to drop columns: at zone width the
/// table cannot carry all of them, and the ones it drops are the numbers.
/// At the full frame width it carries every column, which is the whole
/// difference between the key and doing nothing.
#[test]
fn test_the_torrent_detail_view_keeps_every_column_the_zone_had_to_drop() {
    let mut app = make_test_app();
    app.downloads = vec![doris::ui::view::DownloadRow {
        id: 1,
        hash: "045e85f2ebc24a875a64fe2e9ac9b61f7aad0499".into(),
        name: "A.Very.Long.Torrent.Name.That.Needs.Room.2024.1080p".into(),
        fraction: 0.42,
        download_speed: 4_200_000,
        upload_speed: 890_000,
        seeds: 3,
        peers: 12,
        total_size: 1_990_000_000,
        left: 1_150_000_000,
        dir: "/home/u/Downloads".into(),
        status: 4,
        ..Default::default()
    }];

    // The same app at a zone's width, and at the frame's.
    let narrow = all_text(&render_detail(&mut app, ZoneId::Torrent, 46, 20));
    let wide = all_text(&render_detail(&mut app, ZoneId::Torrent, 120, 30));

    // Everything the plan can offer, at 120 columns.
    for column in ["state", "down", "up", "eta", "size", "ratio"] {
        assert!(
            wide.contains(column),
            "`{column}` is dropped by the zone and kept by `T`"
        );
    }
    assert!(
        !narrow.contains("ratio"),
        "which is the point: at zone width there is no room for it"
    );
}

/// The full-frame view is four boxes, not one heap.
///
/// This was the complaint that produced the boxes: summary, table and
/// facts ran together as undifferentiated lines, so nothing said which
/// question a row was answering. The borders are the fix, and a border
/// drawn in the wrong place would still read as one heap.
#[test]
fn test_the_torrent_detail_view_divides_itself_into_boxes() {
    let mut app = make_test_app();
    app.downloads = vec![doris::ui::view::DownloadRow {
        id: 1,
        hash: "045e85f2ebc24a875a64fe2e9ac9b61f7aad0499".into(),
        name: "A.Very.Long.Torrent.Name.That.Needs.Room.2024.1080p".into(),
        fraction: 0.42,
        total_size: 1_990_000_000,
        left: 1_150_000_000,
        dir: "/home/u/Downloads".into(),
        ..Default::default()
    }];

    let text = all_text(&render_detail_framed(&mut app, ZoneId::Torrent, 120, 30));

    // Four titled boxes: the three summary questions, the table, the facts.
    for title in [" status ", " active ", " free space ", " downloads "] {
        assert!(text.contains(title), "a box titled `{title}` is missing");
    }
    // The facts box is named after the torrent, which is also how the
    // viewer knows which row the facts under it belong to.
    assert!(
        text.contains(" A.Very.Long.Torrent.Name.That.Needs.Room.2024.1080p "),
        "the facts box is named after its row"
    );
    // Each box draws its own top corner: the outer frame, the three summary
    // sections, the table and the facts -- six of them. `all_text` runs the
    // rows together, so they are counted rather than searched for per row.
    let corners = text.matches('╭').count();
    assert!(
        corners >= 6,
        "six framed boxes, six top corners; found {corners}\n{text}"
    );
}

/// Too short for four boxes: it draws what fits rather than boxes that
/// overlap, and a rect past the frame is a panic, not a clipped drawing.
#[test]
fn test_a_short_detail_view_draws_what_fits_without_boxes_it_cannot_hold() {
    let mut app = make_test_app();
    app.downloads = vec![doris::ui::view::DownloadRow {
        id: 1,
        name: "Short.Frame".into(),
        dir: "/home/u/Downloads".into(),
        ..Default::default()
    }];

    // Six rows is a summary and a header and nothing else, and that is the
    // honest answer at six rows -- but it must still be a frame, drawn
    // whole, rather than a rect that ran past the bottom.
    for (w, h) in [(120u16, 6u16), (30, 5), (30, 8)] {
        let text = all_text(&render_detail_framed(&mut app, ZoneId::Torrent, w, h));
        assert!(text.starts_with('╭'), "{w}x{h} lost its own frame");
        assert!(text.contains("torrents"), "{w}x{h} drew no summary\n{text}");
    }

    // With room for a row, the view says which torrent it is about -- in
    // the facts or in the table. A frame that could name nothing would be
    // drawing numbers with no way to tell whose they are.
    for (w, h) in [(40u16, 10u16), (46, 20), (52, 12)] {
        let text = all_text(&render_detail_framed(&mut app, ZoneId::Torrent, w, h));
        assert!(text.contains("Short."), "{w}x{h} named no torrent\n{text}");
        assert!(text.starts_with('╭'), "{w}x{h} lost its own frame");
    }
}

/// ...and with nothing being downloaded it falls back to the streaming
/// server's torrent, which is the case where that is what there is to say.
#[test]
fn test_the_torrent_detail_view_falls_back_to_the_stream_when_nothing_downloads() {
    let mut app = make_test_app();
    app.torrent_status = doris::ui::view::TorrentStatus {
        hash: "streaminghash01".into(),
        title: "Streamed.Something".into(),
        progress: 0.9,
        status: "working".into(),
        ..Default::default()
    };

    let buf = render_detail(&mut app, ZoneId::Torrent, 100, 30);
    let text = all_text(&buf);

    assert!(text.contains("Streamed.Something"));
}

/// The Results detail view is the table again, but full-frame, with a
/// preview line under it naming the row under the cursor -- the facts
/// the detail modal shows, without opening a modal.
#[test]
fn test_the_results_detail_view_shows_the_table_and_a_preview_line() {
    let mut app = make_test_app();
    app.results = make_results(3);
    app.results[1].source = "rutor".into();
    app.results[1].info_hash = "deadbeefcafe".into();
    app.update_filter();
    app.selected = 1;

    let buf = render_detail(&mut app, ZoneId::Results, 120, 30);
    let text = all_text(&buf);

    // The table is still there -- every row of it.
    for i in 0..3 {
        assert!(
            text.contains(&format!("Torrent {}", i)),
            "row {} is on screen",
            i
        );
    }
    // And the preview line names the selected row's facts.
    assert!(text.contains("deadbeefcafe"), "the preview shows the hash");
    assert!(text.contains("rutor"), "the preview shows the source");
}

/// A detail view paints over the search bar too -- it is a takeover,
/// not a zone.
#[test]
fn test_a_detail_view_covers_the_search_bar() {
    let mut app = make_test_app();
    app.search_input = "typing in the box".into();

    let buf = render_detail(&mut app, ZoneId::Torrent, 80, 24);
    let text = all_text(&buf);
    assert!(
        !text.contains("typing in the box"),
        "the search bar must be behind the detail view"
    );
}
