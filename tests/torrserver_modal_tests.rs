//! The TorrServer panel: what it says about a list, and the file pane `v`
//! opens. The keys are pinned next to `handle_key`; this is the drawing.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use doris::config::Config;
use doris::torrserver::api::{TorrentFile, TorrentInfo};
use doris::ui::layout::ZoneId;
use doris::ui::view::App as UiApp;
use ratatui::backend::TestBackend;
use ratatui::Terminal;

mod common;

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

fn app_with_one_torrent() -> UiApp {
    let mut app = common::make_app();
    app.torrserver_torrents = vec![TorrentInfo {
        name: "Big Buck Bunny".into(),
        hash: "dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c".into(),
        total_size: 1_000,
        loaded_size: 500,
        status_string: "Torrent working".into(),
        files: vec![
            TorrentFile {
                id: 0,
                path: "sample.mkv".into(),
                length: 10,
            },
            TorrentFile {
                id: 1,
                path: "bunny.mkv".into(),
                length: 1_000,
            },
        ],
        ..Default::default()
    }];
    // The panel drawn as an overlay: the same content as its tile, but the
    // box is on top of the tiling, which is where the keys are checked.
    app.zones.toggle_overlay(ZoneId::TorrServer);
    app
}

#[test]
fn the_list_shows_the_torrent_and_its_progress() {
    let mut app = app_with_one_torrent();
    let text = render(&mut app, 120, 30);
    assert!(text.contains("TorrServer"), "the box names itself:\n{text}");
    assert!(text.contains("Big Buck Bunny"), "the row is there:\n{text}");
    assert!(
        text.contains("50.0%"),
        "one decimal, like the panel:\n{text}"
    );
    assert!(
        text.contains("┘save└"),
        "the keys are bracketed on the bottom border:\n{text}"
    );
    assert!(
        !text.contains("pause p  delete"),
        "the keys are not a row inside the panel:\n{text}"
    );
}

#[test]
fn v_opens_the_file_list_that_s_saves_from() {
    let mut app = app_with_one_torrent();
    let key = KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE);
    app.torrserver_key(key, true);

    let text = render(&mut app, 120, 30);
    assert!(text.contains("sample.mkv"), "the files are listed:\n{text}");
    assert!(text.contains("bunny.mkv"), "{text}");
    assert!(
        text.contains("Files of"),
        "the pane names the torrent it belongs to:\n{text}"
    );
}

#[test]
fn an_empty_list_says_what_to_check() {
    let mut app = common::make_app();
    app.zones.toggle_overlay(ZoneId::TorrServer);
    let text = render(&mut app, 120, 30);
    assert!(
        text.contains("is TorrServer running?"),
        "an empty list is not a claim that there is nothing:\n{text}"
    );
}

/// `T` takes the frame over with the same list -- a takeover, not the
/// tiling with the panel hidden, and not the search bar above it.
#[test]
fn t_takes_the_frame_over_with_the_list() {
    let mut app = app_with_one_torrent();
    app.zones.close_overlay();
    app.detail_view = Some(ZoneId::TorrServer);

    let text = render(&mut app, 120, 30);

    assert!(text.contains("Big Buck Bunny"), "the row is there:\n{text}");
    assert!(
        text.contains("┘save└"),
        "the keys come with the frame:\n{text}"
    );
    assert!(
        !text.contains("Search"),
        "a takeover is not the tiling plus a search bar:\n{text}"
    );
}
