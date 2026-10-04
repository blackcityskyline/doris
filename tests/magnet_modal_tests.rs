//! The magnet field: a dialog with one line of typing in it.
//!
//! It is the only way to start a torrent that is in no list -- a link from a
//! message board, a `.torrent` somebody sent -- so what it accepts and what
//! it refuses is the whole of what it does.

use doris::config::Config;
use doris::ui::view::{App, Modal};
use ratatui::backend::TestBackend;
use ratatui::Terminal;

fn app() -> App {
    App::new("http://127.0.0.1:8090".into(), None)
}

fn key(code: crossterm::event::KeyCode) -> crossterm::event::KeyEvent {
    crossterm::event::KeyEvent::new(code, crossterm::event::KeyModifiers::NONE)
}

fn rows(app: &mut App, w: u16, h: u16) -> Vec<String> {
    let config = Config {
        show_boxes: true,
        ..Config::default()
    };
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal.draw(|f| app.render(f, &config)).unwrap();
    let buf = terminal.backend().buffer().clone();
    (0..buf.area.height)
        .map(|y| {
            (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
        })
        .collect()
}

/// What the field says, typed one character at a time.
#[test]
fn the_field_holds_what_was_typed() {
    let mut app = app();
    app.open_magnet_modal();
    for c in "magnet:?xt=urn:btih:abc".chars() {
        assert!(app
            .magnet_key(key(crossterm::event::KeyCode::Char(c)))
            .is_none());
    }
    let Modal::Magnet(state) = &app.modal else {
        panic!("the field closed while being typed into");
    };
    assert_eq!(state.input, "magnet:?xt=urn:btih:abc");
}

/// Enter hands the link over; it does not close the dialog by itself, because
/// the answer belongs to the daemon and the dialog is where the answer is
/// shown.
#[test]
fn enter_hands_the_link_to_the_orchestrator() {
    let mut app = app();
    app.open_magnet_modal();
    for c in "magnet:?xt=urn:btih:abc".chars() {
        app.magnet_key(key(crossterm::event::KeyCode::Char(c)));
    }

    let link = app.magnet_key(key(crossterm::event::KeyCode::Enter));

    assert_eq!(link.as_deref(), Some("magnet:?xt=urn:btih:abc"));
    assert!(
        matches!(app.modal, Modal::Magnet(_)),
        "the field stays open until the daemon has answered"
    );
}

/// Something that is neither a magnet nor a `.torrent` is refused in the
/// dialog, by name. `torrent-add` would answer "unrecognized info", which
/// says less than "that is not a magnet link".
#[test]
fn something_that_is_not_a_magnet_is_refused_in_the_dialog() {
    let mut app = app();
    app.open_magnet_modal();
    for c in "the matrix".chars() {
        app.magnet_key(key(crossterm::event::KeyCode::Char(c)));
    }

    assert!(app
        .magnet_key(key(crossterm::event::KeyCode::Enter))
        .is_none());
    let Modal::Magnet(state) = &app.modal else {
        panic!("a refusal closed the dialog");
    };
    let error = state.error.as_deref().expect("a refusal must say why");
    assert!(error.contains("the matrix"), "{error}");
}

/// A `.torrent` path is addable too, because that is what a file manager
/// drops into a terminal.
#[test]
fn a_torrent_path_is_something_the_daemon_can_be_given() {
    let mut app = app();
    app.open_magnet_modal();
    for c in "/home/u/Movie.2024.torrent".chars() {
        app.magnet_key(key(crossterm::event::KeyCode::Char(c)));
    }

    let link = app.magnet_key(key(crossterm::event::KeyCode::Enter));

    assert_eq!(link.as_deref(), Some("/home/u/Movie.2024.torrent"));
}

/// Esc closes and throws the text away: a half-typed magnet is not something
/// to come back to.
#[test]
fn esc_closes_the_field() {
    let mut app = app();
    app.open_magnet_modal();
    app.magnet_key(key(crossterm::event::KeyCode::Esc));

    assert_eq!(app.modal, Modal::None, "Esc must close the dialog");
}

/// The field is a text field, not a line of append-only output: a typo in the
/// middle of a 60-character magnet is fixed by moving the caret, not by
/// starting again.
#[test]
fn the_field_edits_where_the_caret_is() {
    let mut app = app();
    app.open_magnet_modal();
    for c in "magnet:?xt=urn:btih:abc".chars() {
        app.magnet_key(key(crossterm::event::KeyCode::Char(c)));
    }
    app.magnet_key(key(crossterm::event::KeyCode::Left));
    app.magnet_key(key(crossterm::event::KeyCode::Left));
    app.magnet_key(key(crossterm::event::KeyCode::Char('X')));

    let Modal::Magnet(state) = &app.modal else {
        panic!("the field closed while being edited");
    };
    assert_eq!(
        state.input, "magnet:?xt=urn:btih:aXbc",
        "typed at the caret"
    );

    app.magnet_key(key(crossterm::event::KeyCode::Backspace));
    let Modal::Magnet(state) = &app.modal else {
        panic!("the field closed on backspace");
    };
    assert_eq!(
        state.input, "magnet:?xt=urn:btih:abc",
        "and Backspace took it again"
    );
}

/// The dialog is drawn over the app, with a title and the text in it.
#[test]
fn the_dialog_is_drawn_with_its_title_and_its_text() {
    let mut app = app();
    app.open_magnet_modal();
    for c in "magnet:?xt=urn:btih:0123456789abcdef".chars() {
        app.magnet_key(key(crossterm::event::KeyCode::Char(c)));
    }

    let drawn = rows(&mut app, 100, 30).join("\n");

    assert!(drawn.contains("Add a magnet"), "no title:\n{drawn}");
    assert!(
        drawn.contains("magnet:?xt=urn:btih:0123456789abcdef"),
        "the text is not on screen:\n{drawn}"
    );
    assert!(
        drawn.contains("Enter") && drawn.contains("Esc"),
        "and neither is the hint about what the keys do:\n{drawn}"
    );
}

/// An empty field says what goes in it, instead of showing nothing at all:
/// a dialog with a blank line in it reads as a field that is broken.
#[test]
fn an_empty_field_says_what_belongs_in_it() {
    let mut app = app();
    app.open_magnet_modal();

    let drawn = rows(&mut app, 100, 30).join("\n");

    assert!(
        drawn.contains("magnet:?xt=urn:btih:"),
        "no placeholder in the empty field:\n{drawn}"
    );
}
