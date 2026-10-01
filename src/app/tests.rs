//! Key routing: what each keypress does, and what it must not reach. These live beside
//! `handle_key` because that function is private, so a test outside this module could not call
//! it.

use super::input::MOUSE_SCROLL_STEP;
use super::*;
use crate::ui::view::source_rows;
use clap::Parser;

use std::path::PathBuf;

/// An `App` with the Trackers panel focused, no bridge listener, and
/// no sources checked, so a key that submits a search starts no
/// network task and the routing decision is all there is to observe.
async fn app_focused_on_sources(config_path: Option<PathBuf>) -> App {
    let config = Config {
        bridge_port: 0,
        enabled_sources: Vec::new(),
        vim_keys: true,
        ..Config::default()
    };
    let mut argv = vec!["doris"];
    if let Some(path) = &config_path {
        argv.push("--config");
        argv.push(path.to_str().expect("utf-8 test path"));
    }
    let mut app = App::new(Args::parse_from(argv), config)
        .await
        .expect("an App for a key-routing test");
    app.ui.show_menu = false;
    app.ui.zones.focused = ZoneId::Trackers;
    app
}

fn press(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

/// `i`, `s` and `S` all focus the search box -- three keys for one action, so muscle memory
/// from vi (`i`), from this app (`s`) and from the old Settings binding (`S`) all land in the
/// same place.
#[tokio::test]
async fn all_three_search_keys_enter_input_mode() {
    for code in [KeyCode::Char('i'), KeyCode::Char('s'), KeyCode::Char('S')] {
        let mut app = app_focused_on_sources(None).await;
        app.ui.exit_input_mode();

        app.handle_key(press(code)).await.expect("a search key");

        assert!(app.ui.input_mode, "{code:?} must open the search box");
    }
}

/// `S` is a search key now: it must not open the Settings modal
/// behind the box it just focused.
#[tokio::test]
async fn search_capital_s_does_not_open_settings() {
    let mut app = app_focused_on_sources(None).await;

    app.handle_key(press(KeyCode::Char('S'))).await.expect("S");

    assert!(
        !matches!(app.ui.modal, Modal::Settings(_)),
        "Settings moved to the menu; `S` belongs to search"
    );
}

#[tokio::test]
async fn enter_submits_the_query_with_the_sources_panel_focused() {
    let mut app = app_focused_on_sources(None).await;
    app.ui.enter_input_mode();
    app.ui.type_char('m');
    let before = app.config.enabled_sources.clone();

    app.handle_key(press(KeyCode::Enter)).await.expect("Enter");

    assert_eq!(
        app.config.enabled_sources, before,
        "Enter was stolen by the Trackers panel while the query was typed"
    );
    assert!(!app.ui.input_mode, "the typed query was submitted");
    assert!(app.search_generation > 0, "and it started a search");
}

/// The zone digits follow the zone table, not a stale copy of it:
/// `3` is Trackers and `4` is Log, the renumbered way -- and a digit
/// is *focus first, hide second*: the zone you are standing in is
/// the one you mean to take away, every other digit is a zone you
/// want to look at.
#[tokio::test]
async fn zone_digit_keys_focus_first_and_hide_second() {
    let mut app = app_focused_on_sources(None).await;
    assert!(app.ui.zones.is_visible(ZoneId::Trackers));
    assert!(app.ui.zones.is_visible(ZoneId::Log));
    assert_eq!(app.ui.zones.focused, ZoneId::Trackers, "the fixture");

    // Visible but not focused: `4` looks at Log, it does not close it.
    app.handle_key(press(KeyCode::Char('4'))).await.expect("4");
    assert!(
        app.ui.zones.is_visible(ZoneId::Log),
        "a digit on a zone the user is not in must not hide it"
    );
    assert_eq!(
        app.ui.zones.focused,
        ZoneId::Log,
        "and it must put the focus there"
    );

    // Focused already: the second press is the one that hides it,
    // and focus has to leave the zone nobody can see any more.
    app.handle_key(press(KeyCode::Char('4')))
        .await
        .expect("4 again");
    assert!(
        !app.ui.zones.is_visible(ZoneId::Log),
        "the second press hides"
    );
    assert_ne!(
        app.ui.zones.focused,
        ZoneId::Log,
        "focus on a hidden zone is focus nowhere"
    );
    assert!(
        app.ui.zones.is_visible(app.ui.zones.focused),
        "and it landed on a zone that is still there"
    );

    // A hidden zone comes back under focus.
    app.handle_key(press(KeyCode::Char('4')))
        .await
        .expect("4 back");
    assert!(app.ui.zones.is_visible(ZoneId::Log));
    assert_eq!(app.ui.zones.focused, ZoneId::Log);
}

/// A fresh app applies `presets[preset_index]`, and the default first preset is `1,3|4`:
/// Torrent starts hidden, Trackers and Log share the row under Results.
#[tokio::test]
async fn startup_applies_the_first_config_preset() {
    let app = app_focused_on_sources(None).await;
    assert!(app.ui.zones.is_visible(ZoneId::Results));
    assert!(
        !app.ui.zones.is_visible(ZoneId::Torrent),
        "the default preset has no Torrent"
    );
    assert!(app.ui.zones.is_visible(ZoneId::Trackers));
    assert!(app.ui.zones.is_visible(ZoneId::Log));
}

/// `Shift+P` cycles the *configured* presets, not a private copy of
/// them: the index moves and the tiling that spec asks for lands on
/// the zones.
#[tokio::test]
async fn shift_p_cycles_the_configured_presets() {
    let mut app = app_focused_on_sources(None).await;
    assert_eq!(app.config.preset_index, 0, "starts on the first preset");

    app.handle_key(press(KeyCode::Char('P'))).await.expect("P");

    assert_eq!(app.config.preset_index, 1, "`P` moves the index");
    assert!(
        app.ui.zones.is_visible(ZoneId::Torrent),
        "preset 2 is `1,2,3,4`, every zone back on"
    );
}

/// `disable_presets` freezes both the tiling and the index: `P`
/// must not move a layout the user turned off.
#[tokio::test]
async fn shift_p_is_a_no_op_when_presets_are_disabled() {
    let mut app = app_focused_on_sources(None).await;
    app.config.disable_presets = true;
    let before = app.config.preset_index;

    app.handle_key(press(KeyCode::Char('P'))).await.expect("P");

    assert_eq!(app.config.preset_index, before, "`P` must not move");
    assert!(
        !app.ui.zones.is_visible(ZoneId::Torrent),
        "the tiling must stay the default one"
    );
}

/// A query is typed while the panel is focused: every one of its
/// letters -- including the `j`/`k` that are nav keys everywhere
/// else, and the arrow keys after them -- belongs to the box.
#[tokio::test]
async fn every_letter_of_a_query_reaches_the_box() {
    let mut app = app_focused_on_sources(None).await;
    app.ui.enter_input_mode();

    for c in "john wick".chars() {
        app.handle_key(press(KeyCode::Char(c)))
            .await
            .expect("a letter");
    }
    app.handle_key(press(KeyCode::Down)).await.expect("Down");
    app.handle_key(press(KeyCode::Up)).await.expect("Up");

    assert_eq!(
        app.ui.search_input, "john wick",
        "the query was eaten by the panel's (or Results') navigation keys"
    );
    assert_eq!(app.ui.sources_cursor, 0, "the panel cursor must not move");
}

/// The other half of the same guard: outside input mode the panel's
/// keys must still do exactly what they did.
#[tokio::test]
async fn enter_still_switches_the_focused_source_row() {
    let mut app = app_focused_on_sources(None).await;
    app.ui.sources_cursor = 1; // the first real source, after `all`
    let id = source_rows()[1].id();

    app.handle_key(press(KeyCode::Enter)).await.expect("Enter");

    assert_eq!(app.config.enabled_sources, vec![id.to_string()]);
}

/// A checkbox the app forgets on quit is a checkbox that never
/// happened: "Save config on exit" defaults to off, so the toggle
/// has to reach the disk itself -- through the same `--config` path
/// the config was loaded from.
#[tokio::test]
async fn switching_a_source_row_persists_the_config() {
    let dir = std::env::temp_dir().join(format!("doris-cfg-{}", std::process::id()));
    let path = dir.join("config.toml");
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::remove_file(&path);

    let mut app = app_focused_on_sources(Some(path.clone())).await;
    app.ui.sources_cursor = 1;
    let id = source_rows()[1].id();

    app.handle_key(press(KeyCode::Enter)).await.expect("Enter");

    let text =
        std::fs::read_to_string(&path).expect("the checkbox change must reach the config file");
    assert!(
        text.contains(id),
        "`{id}` must be in the saved config:\n{text}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// The path every "you should know this" line takes: the Log zone, the full log `L` opens, and
/// the file.
#[tokio::test]
async fn report_reaches_both_logs() {
    let mut app = app_focused_on_sources(None).await;

    app.report("torrserver", "a line worth seeing");

    assert!(
        app.ui
            .logs
            .iter()
            .any(|l| l.contains("a line worth seeing")),
        "short log: {:?}",
        app.ui.logs
    );
    assert!(
        app.ui
            .detail_logs
            .iter()
            .any(|l| l.contains("a line worth seeing")),
        "full log: {:?}",
        app.ui.detail_logs
    );
}

/// A local server answering 200 on whatever port it is given, so the
/// "TorrServer is up" branch can be tested without the real one --
/// and without ever reaching the `systemctl` fallback.
async fn fake_torrserver() -> u16 {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let port = listener.local_addr().expect("addr").port();
    tokio::spawn(async move {
        while let Ok((mut sock, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buf = [0u8; 1024];
                let _ = sock.read(&mut buf).await;
                let _ = sock
                    .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 0\r\n\r\n")
                    .await;
            });
        }
    });
    port
}

/// Put the Settings cursor on the row whose action is `action`, so a
/// test can press Enter on it without walking the modal's layout.
fn focus_setting(app: &mut App, action: SettingsAction) {
    let Modal::Settings(state) = &mut app.ui.modal else {
        panic!("settings modal is not open");
    };
    let found = state.categories.iter().enumerate().find_map(|(c, cat)| {
        cat.items
            .iter()
            .position(|i| i.action == action)
            .map(|r| (c, r))
    });
    match found {
        Some((c, r)) => {
            state.selected_category = c;
            state.selected = r;
        }
        None => panic!("no settings row for {action:?}"),
    }
}

/// Switching TorrServer on has to say whether it is actually there:
/// the toggle used to invert a bool in silence, and with the unit
/// stopped nothing happened until a stream was started -- with no
/// reason attached there either.
#[tokio::test]
async fn enabling_torrserver_reports_where_it_answers() {
    let port = fake_torrserver().await;
    let dir = std::env::temp_dir().join(format!("doris-ts-{}", std::process::id()));
    let path = dir.join("config.toml");
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::remove_file(&path);

    let config = Config {
        bridge_port: 0,
        enabled_sources: Vec::new(),
        vim_keys: true,
        enable_torrserver: false, // off, so Enter turns it on
        torrserver_url: format!("http://127.0.0.1:{port}"),
        ..Config::default()
    };
    let mut app = App::new(
        Args::parse_from(["doris", "--config", path.to_str().expect("utf-8")]),
        config,
    )
    .await
    .expect("app");
    app.ui.show_menu = false;
    app.ui.open_settings(&app.config, true);
    focus_setting(&mut app, SettingsAction::ToggleEnableTorrserver);

    app.handle_key(press(KeyCode::Enter)).await.expect("Enter");

    assert!(app.config.enable_torrserver, "the switch flipped on");
    let url = format!("http://127.0.0.1:{port}");
    let logs = app.ui.logs.iter().cloned().collect::<Vec<_>>().join("\n");
    assert!(
        logs.contains(&format!("reachable at {url}")),
        "the Log zone must name the answer:\n{logs}"
    );
    let full = app.ui.detail_logs.join("\n");
    assert!(
        full.contains(&format!("reachable at {url}")),
        "the full log must have it too:\n{full}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// `g`/`G` are Results keys: they cycle the category when the panel
/// has focus, and they are letters of the query when the box is
/// being typed into -- the same hand-over every other key gets.
#[tokio::test]
async fn g_cycles_the_category_and_types_when_the_box_is_open() {
    let mut app = app_focused_on_sources(None).await;
    app.ui.zones.focused = ZoneId::Results;
    // The category row is derived from what is checked, and this app
    // starts with nothing checked.
    app.config.enabled_sources = vec!["rutracker".into(), "tpb".into(), "yts".into()];
    app.ui.set_group_tabs(&app.config);
    let all = app.ui.active_group;

    app.handle_key(press(KeyCode::Char('g'))).await.expect("g");
    assert_ne!(app.ui.active_group, all, "the category moved forward");
    assert!(app.ui.group_changed, "Enter owes the server-side search");

    app.handle_key(press(KeyCode::Char('G'))).await.expect("G");
    assert_eq!(app.ui.active_group, all, "and back again");

    app.ui.enter_input_mode();
    let parked = app.ui.active_group;
    app.handle_key(press(KeyCode::Char('g'))).await.expect("g");
    assert_eq!(app.ui.active_group, parked, "typing must not move it");
    assert_eq!(app.ui.search_input, "g", "the letter belongs to the query");
}

/// A category is a request, not only a view.
#[tokio::test]
async fn switching_the_category_reasks_the_sources() {
    let mut app = app_focused_on_sources(None).await;
    app.ui.zones.focused = ZoneId::Results;
    app.ui.group_tabs = vec![None, Some(source::Group::Movies)];
    app.ui.search_query = Some("world war z".into());
    app.ui.results = vec![crate::sources::models::TorrentItem {
        title: "untagged row".into(),
        ..Default::default()
    }];
    app.ui.update_filter();
    assert_eq!(app.ui.active_group, None, "the fixture starts on all");

    app.handle_key(press(KeyCode::Char('g'))).await.expect("g");

    assert_eq!(
        app.ui.active_group,
        Some(source::Group::Movies),
        "the category moved"
    );
    assert!(
        !app.ui.group_changed,
        "the re-ask fired on the spot, so Enter owes nothing"
    );
    let logs = app.ui.logs.iter().cloned().collect::<Vec<_>>().join("\n");
    assert!(
        logs.contains("Searching 'world war z' [Movies]"),
        "the new search names the new category:\n{logs}"
    );
    assert!(
        app.ui.results.is_empty(),
        "the row for the old category does not answer for the new one"
    );
}

/// Before anything has been searched there is no query to re-ask
/// with: the category is only a filter, and Enter keeps the debt it
/// always had (nothing to restart, nothing to play).
#[tokio::test]
async fn switching_the_category_before_any_search_does_not_dispatch() {
    let mut app = app_focused_on_sources(None).await;
    app.ui.zones.focused = ZoneId::Results;
    app.ui.group_tabs = vec![None, Some(source::Group::Movies)];

    app.handle_key(press(KeyCode::Char('g'))).await.expect("g");

    assert_eq!(app.ui.active_group, Some(source::Group::Movies));
    assert!(
        app.ui.group_changed,
        "nothing was asked, so Enter still owes the search"
    );
    let logs = app.ui.logs.iter().cloned().collect::<Vec<_>>().join("\n");
    assert!(
        !logs.contains("Searching '"),
        "and no search was announced:\n{logs}"
    );
}

/// The category travels with the search: it is named in the line the
/// user reads, and a category no checked source can serve says so --
/// the honest "Anime 0/0" answer, rather than a table that looks
/// like nothing was found.
#[tokio::test]
async fn a_search_names_its_category_and_says_when_nothing_can_answer_it() {
    let mut app = app_focused_on_sources(None).await;
    app.config.enabled_sources = vec!["yts".to_string()]; // Movies only
    app.ui.active_group = Some(source::Group::TV);

    app.start_search("matrix".to_string()).await;

    let logs = app.ui.logs.iter().cloned().collect::<Vec<_>>().join("\n");
    assert!(
        logs.contains("Searching 'matrix' [TV]"),
        "the category is named in the log:\n{logs}"
    );
    assert!(
        logs.contains("No checked source serves 'TV'"),
        "and the reason for the empty table names it:\n{logs}"
    );
    assert!(
        app.ui.state == AppState::Idle,
        "nothing was dispatched, so nothing is searching"
    );
}

/// `L`, `T` and `R` each open their zone's detail view -- the same full-frame takeover the
/// detailed log already had -- and the same key closes it again.
#[tokio::test]
async fn shift_enter_opens_the_detail_modal() {
    let mut app = app_focused_on_sources(None).await;
    app.ui.zones.focused = ZoneId::Results;
    app.ui.results = vec![crate::sources::models::TorrentItem {
        title: "Dune 2024".into(),
        source: "rutor".into(),
        // Loopback, refused instantly: the modal opens before the
        // file list arrives, and the fetch behind it must not leave
        // the test machine.
        page_url: "http://127.0.0.1:1/".into(),
        ..Default::default()
    }];
    app.ui.update_filter();

    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT))
        .await
        .expect("Shift+Enter");

    assert!(
        matches!(app.ui.modal, crate::ui::view::Modal::TorrentDetail(_)),
        "Shift+Enter opens the detail modal, got {:?}",
        app.ui.modal
    );

    // Plain Enter in the same place still plays.
    app.ui.modal = crate::ui::view::Modal::None;
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .expect("Enter");
    assert!(
        !matches!(app.ui.modal, crate::ui::view::Modal::TorrentDetail(_)),
        "plain Enter must not open the details"
    );
}

/// The theme row's number has to survive the cycle that changes the theme.
#[tokio::test]
async fn cycling_the_theme_moves_the_row_number_with_it() {
    let mut app = app_focused_on_sources(None).await;
    app.ui.open_settings(&app.config, false);
    let themes = crate::ui::theme::Theme::load_themes();
    let was = app.ui.theme.name.clone();

    app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE))
        .await
        .expect("Right");

    let (pos, name) = match &app.ui.modal {
        crate::ui::view::Modal::Settings(state) => (state.theme_pos, app.ui.theme.name.clone()),
        other => panic!("the settings modal stays open, got {other:?}"),
    };
    assert_ne!(name, was, "Right cycled the theme");
    let want = themes.iter().position(|t| t.name == name).map(|i| i + 1);
    assert_eq!(
        pos.map(|(n, _)| n),
        want,
        "{name} is number {want:?} of {} themes",
        themes.len()
    );
    assert_eq!(
        pos.map(|(_, t)| t),
        Some(themes.len()),
        "and the total is all of them"
    );
}

/// Every mode that grabs the keyboard -- the menu, the search box, a modal, a detail view --
/// answers before the plain-view Ctrl+C arm is ever reached, so quitting has to be the *first*
/// thing `handle_key` asks, not the last.
#[tokio::test]
async fn ctrl_c_quits_from_every_mode() {
    for mode in ["main", "search", "menu", "detail"] {
        let mut app = app_focused_on_sources(None).await;
        match mode {
            "search" => app.ui.input_mode = true,
            "menu" => app.ui.show_menu = true,
            "detail" => app.ui.detail_view = Some(ZoneId::Torrent),
            _ => {}
        }

        app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL))
            .await
            .expect("Ctrl+C is not an error");

        assert!(
            !app.ui.running,
            "Ctrl+C quits with the {mode} mode owning the keyboard"
        );
    }
}

#[tokio::test]
async fn l_t_and_r_open_and_close_their_detail_views() {
    for (code, view) in [
        (KeyCode::Char('L'), ZoneId::Log),
        (KeyCode::Char('T'), ZoneId::Torrent),
        (KeyCode::Char('R'), ZoneId::Results),
    ] {
        let mut app = app_focused_on_sources(None).await;

        app.handle_key(press(code)).await.expect("open");
        assert_eq!(
            app.ui.detail_view,
            Some(view),
            "{code:?} opens {:?}'s detail view",
            view
        );

        app.handle_key(press(code)).await.expect("close again");
        assert_eq!(
            app.ui.detail_view, None,
            "{code:?} closes it again -- the key is a toggle"
        );

        app.handle_key(press(code)).await.expect("reopen");
        app.handle_key(press(KeyCode::Esc)).await.expect("Esc");
        assert_eq!(
            app.ui.detail_view, None,
            "Esc closes {:?}'s detail view",
            view
        );
    }
}

/// While a detail view is open it owns the keys: the zone digits
/// must not toggle visibility underneath the takeover, and the
/// search box must not be reachable.
#[tokio::test]
async fn a_detail_view_owns_the_keyboard() {
    let mut app = app_focused_on_sources(None).await;
    app.handle_key(press(KeyCode::Char('T'))).await.expect("T");

    app.handle_key(press(KeyCode::Char('1'))).await.expect("1");

    assert_eq!(
        app.ui.detail_view,
        Some(ZoneId::Torrent),
        "the digit must not reach the zone toggles"
    );
    assert!(
        !app.ui.search_box_at(0),
        "the search box is covered by the detail view"
    );
}

/// The menu's Help item opens the very same modal `?` does: one
/// help page, two ways to reach it -- a menu item that toggled its
/// own private flag drew nothing.
#[tokio::test]
async fn menu_help_opens_the_same_modal_as_question_mark() {
    let mut app = app_focused_on_sources(None).await;
    app.ui.show_menu = true;
    app.ui.menu.select(); // sanity: the menu has an item to pick
    app.ui.menu.selected = 1; // Help

    app.handle_key(press(KeyCode::Enter)).await.expect("Enter");

    assert!(
        matches!(app.ui.modal, Modal::Help(_)),
        "the Help item must open the help modal"
    );
    assert!(!app.ui.show_menu, "and leave the menu behind");
}

/// name the key the Trackers panel is on, which moved to `3` when the
/// placeholder zone went away -- a stale key points at a zone that
/// does not exist.
#[tokio::test]
async fn nothing_checked_points_at_the_panel_that_switches_them() {
    let mut app = app_focused_on_sources(None).await;
    assert!(app.config.enabled_sources.is_empty(), "starts unchecked");

    app.start_search("matrix".to_string()).await;

    let logs = app.ui.logs.iter().cloned().collect::<Vec<_>>().join("\n");
    assert!(
        logs.contains("Trackers panel (3)"),
        "the message must name the panel's current key:\n{logs}"
    );
}

/// `PageUp`/`PageDown` page the Results table.
#[tokio::test]
async fn page_keys_page_the_results_and_still_page_the_log() {
    let mut app = app_focused_on_sources(None).await;
    app.ui.zones.focused = ZoneId::Results;
    app.terminal_size = (80, 24);
    app.ui.results = (0..100)
        .map(|i| crate::sources::models::TorrentItem {
            title: format!("Torrent {i}"),
            source: "rutor".into(),
            ..Default::default()
        })
        .collect();
    app.ui.zones.filter_input.clear();
    app.ui.update_filter();

    let start = app.ui.selected;
    app.handle_key(press(KeyCode::PageDown))
        .await
        .expect("PageDown");
    assert!(
        app.ui.selected > start,
        "PageDown must move the cursor, {start} -> {}",
        app.ui.selected
    );

    let after_down = app.ui.selected;
    app.handle_key(press(KeyCode::PageUp))
        .await
        .expect("PageUp");
    assert_eq!(
        app.ui.selected, start,
        "PageUp must come back to where it started"
    );
    assert!(
        after_down > start,
        "sanity: the page down before it actually moved"
    );

    // And the Log panel keeps its own paging, which is where these
    // keys worked before.
    for i in 0..100 {
        app.ui.add_log(&format!("line {i}"));
    }
    app.ui.zones.focused = ZoneId::Log;
    app.ui.scroll_logs(crate::ui::view::LOG_PAGE_STEP as isize);
    let scrolled = app.ui.log_scroll;
    assert!(scrolled > 0, "the log has more than one page of log");
    app.handle_key(press(KeyCode::PageUp))
        .await
        .expect("PageUp");
    assert!(app.ui.log_scroll < scrolled, "PageUp must scroll the log");
}

/// Options rows that hand the modal over do not get it handed back.
#[tokio::test]
async fn a_row_that_opens_another_window_keeps_it() {
    let mut app = app_focused_on_sources(None).await;
    app.ui.open_settings(&app.config, true);

    // Categories are chosen by digit or Tab -- `Left`/`Right` act on
    // the row, not the tab -- so the row is found by index and reached
    // with the digit.
    let (category, row) = {
        let state = match &app.ui.modal {
            crate::ui::view::Modal::Settings(s) => s,
            other => panic!("expected the Options modal, got {other:?}"),
        };
        state
            .categories
            .iter()
            .enumerate()
            .flat_map(|(c, cat)| cat.items.iter().enumerate().map(move |(r, it)| (c, r, it)))
            .find(|(_, _, it)| {
                it.action == crate::ui::modals::settings::SettingsAction::EditCredentials
            })
            .map(|(c, r, _)| (c, r))
            .expect("the Options list has an Edit credentials row")
    };
    app.ui
        .settings_key(press(KeyCode::Char(char::from(b'1' + category as u8))));
    for _ in 0..row {
        app.ui.settings_key(press(KeyCode::Down));
    }

    app.handle_key(press(KeyCode::Enter)).await.expect("Enter");

    assert!(
        matches!(app.ui.modal, crate::ui::view::Modal::Login(_)),
        "the login window must survive; got {:?}",
        app.ui.modal
    );
}

/// Every bool row flips its own field.
#[tokio::test]
async fn every_bool_row_flips_its_own_field() {
    // Find the row for each toggle the same way the modal does, then
    // press it on a real `App` and read the field back.
    for (action, _) in BOOL_TOGGLES {
        let before = field_named(action, &Config::default());
        let mut config = Config::default();
        assert!(
            apply_bool_toggle(&mut config, *action),
            "{action:?} must be a bool row"
        );
        assert_eq!(
            field_named(action, &config),
            !before,
            "{action:?} flipped something else"
        );
        // And exactly one field moved.
        let moved = count_true_fields(&Config::default(), &config);
        assert_eq!(moved, 1, "{action:?} moved {moved} fields");
    }
}

/// The field an action names, read back off a `Config`.
fn field_named(action: &SettingsAction, config: &Config) -> bool {
    match action {
        SettingsAction::ToggleCloseBrowserOnExit => config.close_browser_on_exit,
        SettingsAction::ToggleSaveCookies => config.save_cookies,
        SettingsAction::ToggleSaveCredentials => config.save_credentials,
        SettingsAction::ToggleEnableTorrserver => config.enable_torrserver,
        SettingsAction::ToggleThemeBackground => config.theme_background,
        SettingsAction::ToggleTruecolor => config.truecolor,
        SettingsAction::ToggleFalseTty => config.false_tty,
        SettingsAction::ToggleVimKeys => config.vim_keys,
        SettingsAction::ToggleMouse => config.disable_mouse,
        SettingsAction::ToggleDisablePresets => config.disable_presets,
        SettingsAction::ToggleShowBoxes => config.show_boxes,
        SettingsAction::ToggleRoundedCorners => config.rounded_corners,
        SettingsAction::ToggleTerminalSync => config.terminal_sync,
        SettingsAction::ToggleDownloadEnabled => config.download_enabled,
        SettingsAction::ToggleCloseTorrentCoreOnExit => config.close_torrent_core_on_exit,
        SettingsAction::ToggleSaveOnExit => config.save_config_on_exit,
        other => panic!("{other:?} is not a bool row"),
    }
}

/// How many bool fields differ between two configs -- a row that flips
/// one field must not flip two.
fn count_true_fields(a: &Config, b: &Config) -> usize {
    BOOL_TOGGLES
        .iter()
        .filter(|(action, _)| field_named(action, a) != field_named(action, b))
        .count()
}

/// A modal owns the keyboard: a keypress handled inside Options never reaches the main view, so
/// `2` does not move the zone focus behind the window and `d` does not start a download while
/// the user is looking at a settings list.
#[tokio::test]
async fn a_key_the_modal_handles_does_not_reach_the_main_view() {
    let mut app = app_focused_on_sources(None).await;
    app.ui.open_settings(&app.config, true);
    app.ui.zones.focused = ZoneId::Results;

    // A zone digit: in the main view this moves the focus.
    app.handle_key(press(KeyCode::Char('2'))).await.expect("2");

    assert!(
        matches!(app.ui.modal, crate::ui::view::Modal::Settings(_)),
        "Options must still be up"
    );
    assert_eq!(
        app.ui.zones.focused,
        ZoneId::Results,
        "the zone behind the modal must not move"
    );
}

/// The armed removal is cancelled by *any* other key, and that is a claim about `handle_key`
/// rather than about the state machine -- the state machine only knows that something cancelled
/// it.
#[tokio::test]
async fn any_key_other_than_d_cancels_an_armed_removal() {
    for code in [
        KeyCode::Char('j'),
        KeyCode::Char('q'),
        KeyCode::Down,
        KeyCode::Enter,
        KeyCode::Esc,
    ] {
        let mut app = app_focused_on_sources(None).await;
        app.ui.zones.focused = ZoneId::Torrent;
        app.ui.active_torrent_hash = Some("deadbeef".into());

        app.ui.confirm_remove();
        assert!(app.ui.remove_prompt().is_some(), "setup: armed");

        app.handle_key(press(code)).await.expect("a key");

        assert!(
            app.ui.remove_prompt().is_none(),
            "{code:?} must answer the armed question with no"
        );
        assert!(
            app.ui.active_torrent_hash.is_some(),
            "{code:?} must not have removed the torrent either"
        );
    }
}

/// And the second `d` is the one that goes through -- the whole point of arming it.
#[tokio::test]
async fn d_arms_then_removes_and_a_cancelled_d_asks_again() {
    let mut app = app_focused_on_sources(None).await;
    app.ui.zones.focused = ZoneId::Torrent;
    app.ui.active_torrent_hash = Some("deadbeef".into());

    app.handle_key(press(KeyCode::Char('d'))).await.expect("d");
    assert!(
        app.ui.active_torrent_hash.is_some(),
        "the first d must not remove"
    );
    assert!(app.ui.remove_prompt().is_some(), "it must ask");

    // A cancel in between.
    app.handle_key(press(KeyCode::Char('j'))).await.expect("j");
    assert!(app.ui.remove_prompt().is_none());

    app.handle_key(press(KeyCode::Char('d'))).await.expect("d");
    assert!(
        app.ui.remove_prompt().is_some(),
        "after a cancel the next d asks again, it does not remove"
    );
    assert!(app.ui.active_torrent_hash.is_some());
}

/// The full Log view scrolls by line and by page, in the direction the key names.
#[tokio::test]
async fn the_full_log_scrolls_the_way_the_key_names() {
    let mut app = app_focused_on_sources(None).await;
    for i in 0..100 {
        app.ui.add_detail(&format!("line {i}"));
    }
    app.ui.detail_view = Some(ZoneId::Log);
    app.ui.detail_log_scroll = 50;

    app.handle_key(press(KeyCode::Down)).await.expect("Down");
    assert_eq!(app.ui.detail_log_scroll, 51, "Down moves toward the newest");

    app.handle_key(press(KeyCode::Up)).await.expect("Up");
    assert_eq!(app.ui.detail_log_scroll, 50, "Up moves back");

    app.handle_key(press(KeyCode::PageDown))
        .await
        .expect("PageDown");
    assert_eq!(
        app.ui.detail_log_scroll,
        50 + crate::ui::view::LOG_PAGE_STEP,
        "a page is LOG_PAGE_STEP, not the bare literal 20 the old arms used"
    );

    app.handle_key(press(KeyCode::PageUp))
        .await
        .expect("PageUp");
    assert_eq!(app.ui.detail_log_scroll, 50);

    // And the same with vim letters, which only exist when the setting
    // is on -- the helper gates them there rather than in the match, and
    // that gate is the other half of what this test is pinning.
    app.handle_key(press(KeyCode::Char('j'))).await.expect("j");
    assert_eq!(app.ui.detail_log_scroll, 51, "j is Down when vim is on");
    app.handle_key(press(KeyCode::Char('k'))).await.expect("k");
    assert_eq!(app.ui.detail_log_scroll, 50);

    app.config.vim_keys = false;
    app.handle_key(press(KeyCode::Char('j'))).await.expect("j");
    assert_eq!(
        app.ui.detail_log_scroll, 50,
        "and is nothing at all when vim keys are off"
    );
}

/// The wheel over the full Log view moves it by its own step, not by the
/// one-line step the arrow keys use.
#[tokio::test]
async fn the_wheel_scrolls_the_full_log_by_its_own_step() {
    let mut app = app_focused_on_sources(None).await;
    for i in 0..100 {
        app.ui.add_detail(&format!("line {i}"));
    }
    app.ui.detail_view = Some(ZoneId::Log);
    app.ui.detail_log_scroll = 50;

    // crossterm reports a wheel notch as a ScrollUp/ScrollDown event with
    // no coordinate movement, which is what with
    // only the kind replaced describes.
    let wheel = |down: bool| MouseEvent {
        kind: if down {
            MouseEventKind::ScrollDown
        } else {
            MouseEventKind::ScrollUp
        },
        column: 0,
        row: 0,
        modifiers: crossterm::event::KeyModifiers::NONE,
    };

    app.handle_mouse(wheel(false)).await;
    assert_eq!(
        app.ui.detail_log_scroll,
        50 - MOUSE_SCROLL_STEP as usize,
        "one notch up moves toward the oldest"
    );

    app.handle_mouse(wheel(true)).await;
    assert_eq!(
        app.ui.detail_log_scroll, 50,
        "and the notch after it comes back"
    );

    app.handle_mouse(wheel(true)).await;
    assert_eq!(
        app.ui.detail_log_scroll,
        50 + MOUSE_SCROLL_STEP as usize,
        "down moves the other way"
    );
}

/// Options' "cycle this row" keys move the value they name.
#[tokio::test]
async fn a_cycle_row_in_options_moves_the_value_it_names() {
    /// Which row of the open category carries `label`.
    fn row_of(app: &App, label: &str) -> usize {
        match &app.ui.modal {
            Modal::Settings(state) => {
                let cat = &state.categories[state.selected_category];
                cat.items
                    .iter()
                    .position(|i| i.label == label)
                    .unwrap_or_else(|| panic!("the {label} row exists"))
            }
            other => panic!("expected Settings, got {other:?}"),
        }
    }

    let mut app = app_focused_on_sources(None).await;
    app.ui.open_settings(&app.config, false);

    // The cursor opens on row 0, so walking down `n` times lands on row
    // `n` -- unless it is already further down, which a re-entered
    // modal would be. Starting each walk from the top keeps the count
    // honest.
    async fn select_row(app: &mut App, at: usize) {
        while row_at(app) > 0 {
            app.handle_key(press(KeyCode::Char('k'))).await.expect("k");
        }
        for _ in 0..at {
            app.handle_key(press(KeyCode::Char('j'))).await.expect("j");
        }
    }
    fn row_at(app: &App) -> usize {
        match &app.ui.modal {
            Modal::Settings(state) => state.selected,
            other => panic!("expected Settings, got {other:?}"),
        }
    }

    let symbol_row = row_of(&app, "Graph symbol");
    select_row(&mut app, symbol_row).await;
    let before = app.config.graph_symbol.clone();

    app.handle_key(press(KeyCode::Right)).await.expect("Right");
    assert_ne!(
        app.config.graph_symbol, before,
        "one Right on the Graph symbol row must step it"
    );

    app.handle_key(press(KeyCode::Left)).await.expect("Left");
    assert_eq!(
        app.config.graph_symbol, before,
        "and Left must step back, not forward again"
    );

    // A value not in the list at all lands on the first entry rather
    // than on itself -- the alternative is a row that cannot be fixed
    // from the UI at all.
    app.config.graph_symbol = "nonsense".into();
    app.handle_key(press(KeyCode::Right)).await.expect("Right");
    assert_eq!(
        app.config.graph_symbol, "braille",
        "an unknown value snaps to the first entry"
    );
}
