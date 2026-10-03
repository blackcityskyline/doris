//! The search box as a text field: a caret where the typing happens,
//! and the text you are typing drawn in the box rather than in its
//! border.

use doris::config::Config;
use doris::ui::view::App as UiApp;
use ratatui::backend::{Backend, TestBackend};
use ratatui::Terminal;

mod common;

fn make_app() -> UiApp {
    common::make_app()
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

/// The box used to be append-only, so a typo in the middle of a query could
/// not be fixed: `ctrl+w` and typing again is not editing. These are the
/// keys that make it a text field.
#[test]
fn the_caret_moves_through_the_query_and_typing_inserts_where_it_stands() {
    let mut app = make_app();
    app.enter_input_mode();
    for c in "batman".chars() {
        app.type_char(c);
    }

    app.move_cursor(-3); // bat|man
    app.type_char('X');
    assert_eq!(
        app.search_input, "batXman",
        "typed at the caret, not at the end"
    );

    app.backspace(); // bat|man again
    assert_eq!(
        app.search_input, "batman",
        "backspace took what was before it"
    );

    app.move_cursor(2); // batma|n
    app.delete_under_cursor();
    assert_eq!(app.search_input, "batma", "and Del took what was under it");
}

/// The ends are two keys, not arithmetic: `Home` and `End` name a place, and
/// a caret that wraps from the start to the end is a caret that surprises.
#[test]
fn home_and_end_go_to_the_ends_and_arrows_stop_there() {
    let mut app = make_app();
    app.enter_input_mode();
    app.set_input("dune part two");

    app.cursor_home();
    assert_eq!(app.cursor_column(), 0);
    app.move_cursor(-1);
    assert_eq!(app.cursor_column(), 0, "and stays at the start");

    app.cursor_end();
    assert_eq!(app.cursor_column(), 13);
    app.move_cursor(5);
    assert_eq!(app.cursor_column(), 13, "and stays at the end");
}

/// Deleting a word takes the one *before* the caret, which is what every text
/// field does and what the append-only version could not do at all: it only
/// ever meant "the last word".
#[test]
fn a_word_is_deleted_before_the_caret_and_not_after_it() {
    let mut app = make_app();
    app.enter_input_mode();
    app.set_input("dune part two");

    app.cursor_end();
    app.delete_word();
    assert_eq!(app.search_input, "dune part", "the last word went");

    app.delete_word();
    assert_eq!(app.search_input, "dune", "and then the one before it");

    app.delete_word();
    assert_eq!(app.search_input, "", "and then the only one left");

    // The half of the word the caret is inside of stays: `ctrl+w` is
    // backwards, and a key that deletes forwards is a key that deletes text
    // you were not looking at.
    app.set_input("dune part two");
    app.move_cursor(-1); // dune part tw|o
    app.delete_word();
    assert_eq!(app.search_input, "dune parto", "only what was before it");

    app.cursor_home();
    app.delete_word();
    assert_eq!(
        app.search_input, "dune parto",
        "a space before the caret: nothing to take"
    );
}

/// A query that arrives whole -- typed on the command line, or sent by the
/// browser add-on -- must not leave the caret at the start of it.
#[test]
fn a_query_set_from_outside_puts_the_caret_at_the_end() {
    let mut app = make_app();
    app.enter_input_mode();
    app.set_input("oblivion");
    assert_eq!(app.cursor_column(), 8);
    app.type_char('x');
    assert_eq!(
        app.search_input, "oblivionx",
        "so typing appends, as it should"
    );
}

/// Characters, not bytes: a Cyrillic query is a dozen positions, not thirty.
#[test]
fn the_caret_counts_characters_rather_than_bytes() {
    let mut app = make_app();
    app.enter_input_mode();
    app.set_input("Довод");

    app.move_cursor(-1); // Дово|д
    app.type_char('ъ');
    assert_eq!(
        app.search_input, "Довоъд",
        "before the last character, and not inside one of them"
    );
    assert_eq!(app.cursor_column(), 5, "five characters, not fifteen bytes");

    app.cursor_end();
    app.type_char('ъ');
    assert_eq!(app.search_input, "Довоъдъ");

    app.backspace();
    assert_eq!(
        app.search_input, "Довоъд",
        "and the whole character before the caret went, not half of it"
    );
}

/// The drawn caret has to be where the caret is, or the keys are honest and
/// the screen is not.
#[test]
fn the_drawn_caret_is_where_the_caret_is() {
    let mut app = make_app();
    app.enter_input_mode();
    for c in "batman".chars() {
        app.type_char(c);
    }
    let (_, after_typing) = render(&mut app, 120, 40);

    app.move_cursor(-3);
    let (_, moved) = render(&mut app, 120, 40);

    assert_eq!(after_typing.0, 7, "after six letters");
    assert_eq!(moved.0, 4, "and three to the left");
    assert_eq!(
        input_row(&render(&mut app, 120, 40).0),
        "batman",
        "text is unchanged"
    );
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

/// The box's idle title is a name, not a whisper: `Search`, capital S,
/// the way every other zone title is written (`Results`, `Torrent`).
#[test]
fn test_the_idle_box_title_is_capitalized_search() {
    let mut app = make_app();
    let (rows, _) = render(&mut app, 120, 40);
    assert!(
        rows[0].contains("Search"),
        "the box is titled 'Search': {}",
        rows[0]
    );
    assert!(
        !rows[0].contains("search"),
        "and not in lowercase: {}",
        rows[0]
    );
}
