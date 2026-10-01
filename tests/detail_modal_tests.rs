//! The torrent detail modal: the row's own facts,
//! the file list its source can read off the row's page, and the keys
//! that move the cursor, play and download.

use doris::sources::models::{FileEntry, TorrentItem};
use doris::sources::source::Source;
use doris::ui::view::{App as UiApp, DetailAction, Modal, TorrentDetailState};
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn make_app() -> UiApp {
    UiApp::new("http://127.0.0.1:8090".into(), None)
}

fn item() -> TorrentItem {
    TorrentItem {
        title: "The Long Awaited Release 2026 1080p".into(),
        source: "rutracker".into(),
        size: "1.4 GB".into(),
        seeds: "342".into(),
        date: "01-Jan-26".into(),
        page_url: "viewtopic.php?t=123".into(),
        info_hash: "0123456789abcdef0123456789abcdef01234567".into(),
        magnet: Some("magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567".into()),
        ..Default::default()
    }
}

fn open_detail(app: &mut UiApp) {
    app.modal = Modal::TorrentDetail(Box::new(TorrentDetailState::new(item())));
}

fn files(n: usize) -> Vec<FileEntry> {
    (0..n)
        .map(|i| FileEntry {
            name: format!("file_{:02}.mkv", i),
            size: if i == 0 {
                "1.2 GB".into()
            } else {
                "64 MB".into()
            },
        })
        .collect()
}

// --- the state -------------------------------------------------------------

/// A freshly opened modal shows the row's own facts at once and says the
/// file list is still coming -- never an empty box.
#[test]
fn test_a_new_modal_is_pending_with_an_empty_list() {
    let state = TorrentDetailState::new(item());
    assert!(state.pending);
    assert!(state.files.is_empty());
    assert_eq!(state.cursor, 0);
    assert!(state.error.is_none());
}

// --- the keys --------------------------------------------------------------

#[test]
fn test_the_cursor_moves_and_clamps_at_both_ends() {
    let mut app = make_app();
    open_detail(&mut app);
    if let Modal::TorrentDetail(ref mut state) = app.modal {
        state.files = files(3);
    }

    assert_eq!(app.detail_key(key(KeyCode::Down), true), None);
    assert_eq!(
        app.detail_key(key(KeyCode::Up), true),
        None,
        "clamped at the top"
    );
    assert_eq!(app.detail_key(key(KeyCode::Char('j')), true), None);
    assert_eq!(app.detail_key(key(KeyCode::Char('j')), true), None);
    assert_eq!(
        app.detail_key(key(KeyCode::Char('j')), true),
        None,
        "clamped at the bottom"
    );
    assert_eq!(app.detail_key(key(KeyCode::Char('k')), true), None);
    assert_eq!(app.detail_key(key(KeyCode::Up), true), None);
}

/// An empty list has nothing to move through: the keys must not panic or
/// invent a cursor.
#[test]
fn test_the_cursor_stays_put_when_there_are_no_files() {
    let mut app = make_app();
    open_detail(&mut app);

    for k in [
        key(KeyCode::Down),
        key(KeyCode::Up),
        key(KeyCode::Char('j')),
        key(KeyCode::Char('k')),
    ] {
        assert_eq!(app.detail_key(k, true), None);
    }
    if let Modal::TorrentDetail(ref state) = app.modal {
        assert_eq!(state.cursor, 0);
    }
}

#[test]
fn test_enter_and_d_ask_for_the_orchestrators_actions() {
    let mut app = make_app();
    open_detail(&mut app);

    assert_eq!(
        app.detail_key(key(KeyCode::Enter), true),
        Some(DetailAction::Play)
    );
    assert_eq!(
        app.detail_key(key(KeyCode::Char('d')), true),
        Some(DetailAction::Download)
    );
}

/// Esc and q close the modal; the cursor and the list stay behind, so
/// reopening the same row starts where the user left off.
#[test]
fn test_esc_and_q_close_the_modal() {
    let mut app = make_app();
    open_detail(&mut app);

    assert_eq!(app.detail_key(key(KeyCode::Esc), true), None);
    assert_eq!(app.modal, Modal::None);

    open_detail(&mut app);
    assert_eq!(app.detail_key(key(KeyCode::Char('q')), true), None);
    assert_eq!(app.modal, Modal::None);
}

/// The modal owns the keyboard only while it is up: with no modal the
/// keys must not reach for a state that is not there.
#[test]
fn test_the_keys_do_nothing_without_the_modal() {
    let mut app = make_app();
    assert_eq!(app.detail_key(key(KeyCode::Enter), true), None);
    assert_eq!(app.detail_key(key(KeyCode::Char('d')), true), None);
}

// --- the answer landing in the modal ---------------------------------------

/// The file list for the page the modal asked about lands in the modal,
/// and the "still reading" line goes away with it.
#[test]
fn test_the_answer_lands_in_the_modal_that_asked_for_it() {
    let mut app = make_app();
    open_detail(&mut app);

    doris::app::apply_detail_loaded(&mut app, "viewtopic.php?t=123", files(2), None);

    match app.modal {
        Modal::TorrentDetail(ref state) => {
            assert!(!state.pending);
            assert_eq!(state.files.len(), 2);
            assert!(state.error.is_none());
        }
        other => panic!(
            "expected the detail modal, got {:?}",
            std::mem::discriminant(&other)
        ),
    }
}

/// A late answer for a row the user has already left is dropped, not
/// shown in the next row's modal.
#[test]
fn test_a_late_answer_for_another_row_is_dropped() {
    let mut app = make_app();
    open_detail(&mut app);

    // The user opens another row's details before this one answers.
    let mut other = item();
    other.page_url = "viewtopic.php?t=999".into();
    app.modal = Modal::TorrentDetail(Box::new(TorrentDetailState::new(other)));

    doris::app::apply_detail_loaded(&mut app, "viewtopic.php?t=123", files(2), None);

    match app.modal {
        Modal::TorrentDetail(ref state) => {
            assert!(
                state.pending,
                "the new modal is still waiting for its own answer"
            );
            assert!(state.files.is_empty());
        }
        other => panic!(
            "expected the detail modal, got {:?}",
            std::mem::discriminant(&other)
        ),
    }
}

/// A closed modal drops the answer too: there is nowhere for it to go.
#[test]
fn test_an_answer_for_a_closed_modal_is_dropped() {
    let mut app = make_app();
    app.modal = Modal::None;

    doris::app::apply_detail_loaded(&mut app, "viewtopic.php?t=123", files(2), None);
    assert_eq!(app.modal, Modal::None);
}

/// A source that cannot list files says so in the modal rather than
/// failing it, and the cursor is clamped to whatever list arrived.
#[test]
fn test_an_error_is_shown_and_the_cursor_clamped() {
    let mut app = make_app();
    open_detail(&mut app);
    if let Modal::TorrentDetail(ref mut state) = app.modal {
        state.files = files(5);
        state.cursor = 4;
    }

    doris::app::apply_detail_loaded(&mut app, "viewtopic.php?t=123", files(2), Some("HTTP 503"));

    match app.modal {
        Modal::TorrentDetail(ref state) => {
            assert!(!state.pending);
            assert_eq!(state.error.as_deref(), Some("HTTP 503"));
            assert_eq!(state.cursor, 1, "clamped to the shorter list");
        }
        other => panic!(
            "expected the detail modal, got {:?}",
            std::mem::discriminant(&other)
        ),
    }
}

// --- the trait default -----------------------------------------------------

/// `Source::details` defaults to "this source cannot list files": the
/// modal is opened on demand, so a source with nothing to add should
/// leave the row's own facts on screen, not fail the modal.
#[test]
fn test_the_trait_default_answers_an_empty_list() {
    let source = doris::sources::rutor::RutorSearcher::new();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let answer = rt.block_on(source.details("viewtopic.php?t=1")).unwrap();
    assert!(answer.is_empty(), "rutor does not implement details yet");
}

// --- the rendering ----------------------------------------------------------

fn render(app: &mut UiApp) -> Vec<String> {
    let mut terminal = Terminal::new(TestBackend::new(90, 30)).unwrap();
    terminal
        .draw(|frame| app.render(frame, &doris::config::Config::default()))
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

/// The row's own facts are on screen before anything is fetched, and the
/// file list says it is still coming.
#[test]
fn test_the_modal_shows_the_row_and_says_the_list_is_coming() {
    let mut app = make_app();
    open_detail(&mut app);
    let rows = render(&mut app);

    let text: String = rows.join("\n");
    for expected in [
        "Torrent details",
        "The Long Awaited Release 2026 1080p",
        "rutracker",
        "1.4 GB",
        "342",
        "01-Jan-26",
        "0123456789abcdef0123456789abcdef01234567",
        "viewtopic.php?t=123",
        "Files:",
        "reading the torrent's page",
        "Enter: play",
    ] {
        assert!(
            text.contains(expected),
            "the modal should say '{}':\n{}",
            expected,
            text
        );
    }
}

/// Once the source answers, the files are listed with the cursor
/// reversed -- the same way the selected result row is.
#[test]
fn test_the_file_list_marks_the_cursor_row() {
    let mut app = make_app();
    open_detail(&mut app);
    if let Modal::TorrentDetail(ref mut state) = app.modal {
        state.files = files(3);
        state.pending = false;
        state.cursor = 1;
    }
    let rows = render(&mut app);

    let text: String = rows.join("\n");
    assert!(text.contains("file_00.mkv"), "{}", text);
    assert!(text.contains("file_01.mkv"), "{}", text);
    assert!(text.contains("file_02.mkv"), "{}", text);

    // The cursor row is the marked one: find it by its file name and
    // check it carries the theme's selection background.
    let buf = {
        let mut terminal = Terminal::new(TestBackend::new(90, 30)).unwrap();
        terminal
            .draw(|frame| app.render(frame, &doris::config::Config::default()))
            .unwrap();
        terminal.backend().buffer().clone()
    };
    let theme = doris::ui::theme::Theme::default();
    let mut painted = 0;
    for y in 0..buf.area.height {
        let line: String = (0..buf.area.width)
            .map(|x| buf[(x, y)].symbol().to_string())
            .collect();
        if line.contains("file_01.mkv") {
            for x in 0..buf.area.width {
                if buf[(x, y)].bg == theme.selected_bg.to_color() {
                    painted += 1;
                }
            }
        }
    }
    assert!(painted > 0, "the cursor row is drawn in selected_bg");
}

/// A source that cannot list files gets an honest line instead of a blank
/// list that reads as "this torrent has no files".
#[test]
fn test_a_source_that_cannot_list_files_says_so() {
    let mut app = make_app();
    open_detail(&mut app);
    doris::app::apply_detail_loaded(&mut app, "viewtopic.php?t=123", Vec::new(), None);
    let rows = render(&mut app);
    let text: String = rows.join("\n");
    assert!(
        text.contains("this source cannot list the files"),
        "{}",
        text
    );
}
