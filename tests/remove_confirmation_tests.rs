//! Removing a torrent takes it off TorrServer's disk and there is no
//! undo. One `d` used to do that, on the same key that downloads a row
//! in the zone next door.
//!
//! The state machine itself is here because the removal needs a network
//! call to run for real, and what is worth pinning is the decision: the
//! first press arms, only the second one removes, and anything else is a
//! "no". The question the panel shows in between is checked by rendering
//! it, since a prompt nobody sees is not a confirmation.

use doris::config::Config;
use doris::ui::view::App;
use ratatui::backend::TestBackend;
use ratatui::Terminal;

fn app() -> App {
    let mut app = App::new("http://127.0.0.1:1".into(), None);
    app.torrent_status.hash = "abc123".into();
    app.torrent_status.title = "some torrent".into();
    app.active_torrent_hash = Some("abc123".into());
    app
}

#[test]
fn one_press_asks_and_does_not_remove() {
    let mut app = app();
    assert!(!app.confirm_remove(), "the first press must not remove");
    assert!(app.remove_prompt().is_some(), "it must ask");
}

#[test]
fn a_second_press_removes() {
    let mut app = app();
    app.confirm_remove();
    assert!(
        app.confirm_remove(),
        "the second press is the one that removes"
    );
    assert!(
        app.remove_prompt().is_none(),
        "the question is answered once"
    );
}

#[test]
fn any_other_key_cancels() {
    let mut app = app();
    app.confirm_remove();
    app.disarm_remove();
    assert!(app.remove_prompt().is_none());
    assert!(
        !app.confirm_remove(),
        "after a cancel the next press asks again, it does not remove"
    );
}

#[test]
fn nothing_is_armed_to_begin_with() {
    let app = app();
    assert!(app.remove_prompt().is_none());
}

/// A prompt that is not drawn is not a confirmation: the user has to be
/// able to see that the app is waiting for them.
#[test]
fn the_panel_shows_the_question_while_armed() {
    let mut app = app();
    // The first preset hides Torrent, and the question is appended after
    // the panel's four fact lines -- so the others are folded away to
    // give it room, which is also how it looks when a user has the zone
    // full-height.
    app.zones
        .focus_or_toggle(doris::ui::layout::ZoneId::Torrent);
    app.zones.focus_or_toggle(doris::ui::layout::ZoneId::Log);
    app.zones
        .focus_or_toggle(doris::ui::layout::ZoneId::Trackers);
    app.zones.focused = doris::ui::layout::ZoneId::Torrent;
    assert!(app.zones.is_visible(doris::ui::layout::ZoneId::Torrent));

    let mut terminal = Terminal::new(TestBackend::new(80, 40)).unwrap();

    app.confirm_remove();
    terminal
        .draw(|f| app.render(f, &Config::default()))
        .unwrap();
    let armed = buffer_text(&terminal);
    assert!(
        armed.contains("d again to confirm"),
        "the armed panel must ask: {armed}"
    );

    app.disarm_remove();
    terminal
        .draw(|f| app.render(f, &Config::default()))
        .unwrap();
    let plain = buffer_text(&terminal);
    assert!(
        !plain.contains("d again to confirm"),
        "a cancelled question must not stay on screen"
    );
    assert!(
        plain.contains("abc123"),
        "the panel keeps its facts while the question is up"
    );
}

fn buffer_text(terminal: &Terminal<TestBackend>) -> String {
    let buf = terminal.backend().buffer().clone();
    (0..buf.area.height)
        .map(|y| {
            (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// "Show boxes" was an Options row that persisted a value nothing read:
/// `themed_block` always asked for `Borders::ALL`. It now asks for
/// `Borders::NONE` when the option is off, and a panel without its frame
/// is a visibly different picture -- which is the only honest way to
/// check an Options row that claims to change what is drawn.
#[test]
fn the_show_boxes_option_takes_the_borders_off() {
    let mut app = App::new("http://127.0.0.1:1".into(), None);
    let mut terminal = Terminal::new(TestBackend::new(80, 30)).unwrap();

    let with_boxes = Config {
        show_boxes: true,
        ..Default::default()
    };
    terminal.draw(|f| app.render(f, &with_boxes)).unwrap();
    let boxed = buffer_text(&terminal);
    assert!(
        boxed.contains('╭') || boxed.contains('┌'),
        "with the option on the panels are framed:\n{boxed}"
    );

    let without = Config {
        show_boxes: false,
        ..Default::default()
    };
    terminal.draw(|f| app.render(f, &without)).unwrap();
    let bare = buffer_text(&terminal);
    assert!(
        !bare.contains('╭') && !bare.contains('┌') && !bare.contains('╰'),
        "with it off every frame goes, the search bar included:\n{bare}"
    );
    // The content is still there -- this removes the frames, not the
    // panels.
    assert!(
        bare.contains("Results") && bare.contains("Trackers"),
        "the panels are still drawn:\n{bare}"
    );
}
