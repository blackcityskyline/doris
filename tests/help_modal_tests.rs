//! The help page (UI_REFACTOR_PLAN §3): btop's `helpMenu`
//! (`btop_menu.cpp:1743`) rebuilt as a modal.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use doris::config::Config;
use doris::ui::app::App as UiApp;
use doris::ui::modals::help::HELP_TEXT;
use ratatui::backend::TestBackend;
use ratatui::Terminal;

fn make_app() -> UiApp {
    UiApp::new("http://127.0.0.1:8090".into(), None)
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

/// Render `app` once at `w`x`h` -- the same pass that publishes the page
/// count `help_key` clamps against -- and hand back the drawn rows.
fn render(app: &mut UiApp, w: u16, h: u16) -> Vec<String> {
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

/// btop draws `[key, description]` pairs from one table
/// (`help_text`, `btop_menu.cpp:174`). Ours has to keep naming the keys
/// AGENTS.md documents, or the page silently falls behind the bindings.
#[test]
fn test_help_text_names_the_documented_keybinds() {
    let keys: Vec<&str> = HELP_TEXT.iter().map(|(k, _)| *k).collect();
    for expected in [
        "s, i, S",
        "Enter",
        "Shift+Enter, D",
        "b",
        "L",
        "T",
        "R",
        "F",
        "f",
        "m",
        "1, 2, 3, 4",
        "Shift+P",
        "Tab, Shift+Tab",
        "j, k, Up, Down",
        "g, G",
        "d",
        "v",
        "p",
        "Esc",
        "q, ctrl + c",
        "? , /, F1",
        "PageUp, PageDown",
        "ctrl + u, ctrl + w",
    ] {
        assert!(
            keys.iter().any(|k| k == &expected),
            "HELP_TEXT is missing '{}'; it must match AGENTS.md",
            expected
        );
    }
}

/// The help page has to say what the app does, not what it used to do.
///
/// Four things were wrong at once, and every one of them is a key a user
/// can press and get something the page does not describe:
///
/// - it said `Esc` "closes a modal", and in the main view `Esc` *opens*
///   the menu;
/// - it said a click hits "tabs", which stopped being a thing when the
///   Trackers panel became a list of checkboxes;
/// - `ctrl + u` and `ctrl + w` have always worked and were never written
///   down, so nobody could know to use them;
/// - it did not say that `d` on a torrent asks first.
#[test]
fn the_page_describes_what_the_keys_do_now() {
    let page: Vec<String> = HELP_TEXT
        .iter()
        .flat_map(|(k, d)| [k.to_string(), d.to_string()])
        .collect();
    let text = page.join("\n");

    assert!(
        !text.contains("tabs"),
        "there are no tabs: the Trackers panel is a list of checkboxes"
    );
    assert!(
        text.contains("opens the menu"),
        "Esc opens the main menu outside a modal, and the page must say so"
    );
    assert!(
        text.contains("ctrl + u") && text.contains("ctrl + w"),
        "the input editing keys are undocumented"
    );
    assert!(
        text.contains("Confirms the removal"),
        "`d` on a torrent asks before it removes; the page must say so"
    );
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
        other => panic!(
            "expected the help modal, got {:?}",
            std::mem::discriminant(other)
        ),
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
        rows.iter()
            .any(|r| r.contains("Key:") && r.contains("Description:")),
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

    let header = rows
        .iter()
        .find(|r| r.contains("Key:"))
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
        let found = page.iter().any(|r| {
            r.chars()
                .skip(key_col)
                .take(20)
                .collect::<String>()
                .contains(expected)
        });
        assert!(found, "'{}' is not in the key column of page 1", expected);
    }
}

/// The page pairs a key with what it actually does; the keys alone are
/// not enough to catch a swap. `f` filters and `F` goes fullscreen
/// (`app.rs` routes them that way, and the Results frame legend prints
/// `f Filter`), but the table had them the other way round -- the page
/// sent the user to the wrong binding while the key list test passed.
#[test]
fn test_help_text_pairs_the_lower_f_with_filter_and_the_upper_with_fullscreen() {
    let desc = |key: &str| {
        HELP_TEXT
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, d)| *d)
            .unwrap_or_else(|| panic!("no help row for `{key}`"))
    };

    assert_eq!(
        desc("f"),
        "Filter mode; words, -not, src:, size:, seeds:.",
        "`f` is the filter, as routed in `handle_key`, and the box's \
         syntax is src/filter.rs"
    );
    assert_eq!(
        desc("F"),
        "Toggles fullscreen for the focused zone.",
        "`F` is fullscreen"
    );

    // The frame legend is the other place these keys are named, so the
    // two tables have to agree about `f` or one of them is lying.
    let legend = doris::ui::zones::zone_buttons(doris::ui::zones::ZoneId::Results);
    assert!(
        legend.iter().any(|b| b.key == 'f' && b.label == "filter"),
        "the Results frame prints `f filter`: {:?}",
        legend
            .iter()
            .map(|b| (b.key, b.label.clone()))
            .collect::<Vec<_>>()
    );
}

/// The page grew a second table: how the filter is written and how a
/// row gets its category -- the two things `f` and `g` do that the
/// single key table could not explain. `←`/`→` pick the table.
#[test]
fn test_help_switches_sections_with_the_arrow_keys() {
    let mut app = make_app();
    app.open_help_modal();

    let title = |app: &UiApp, rows: &[String]| match &app.modal {
        doris::ui::app::Modal::Help(s) => rows
            .iter()
            .find(|r| r.contains("help:"))
            .cloned()
            .unwrap_or_else(|| panic!("the box is titled, section {}", s.section)),
        other => panic!("help modal is gone: {:?}", std::mem::discriminant(other)),
    };

    let mut rows = render(&mut app, 110, 44);
    assert!(
        title(&app, &rows).contains("keys"),
        "opens on the key table: {:?}",
        title(&app, &rows)
    );

    app.help_key(key(KeyCode::Right));
    rows = render(&mut app, 110, 44);
    let shown = title(&app, &rows);
    assert!(
        shown.contains("filter"),
        "Right moves to the filter table, got {shown:?}"
    );
    assert!(
        rows.iter().any(|r| r.contains("src:id")),
        "and its rows are drawn:\n{}",
        rows.join("\n")
    );

    app.help_key(key(KeyCode::Left));
    rows = render(&mut app, 110, 44);
    assert!(title(&app, &rows).contains("keys"), "Left comes back");
    assert!(
        !rows.iter().any(|r| r.contains("src:id")),
        "and the filter table is gone"
    );
}

/// The section keys have to work on a terminal where the table fits on
/// one page: `help_key` returns early when there is nothing to page, and
/// that guard sits *above* the page keys.
#[test]
fn test_help_switches_sections_even_when_one_page_fits() {
    let mut app = make_app();
    app.open_help_modal();
    render(&mut app, 150, 60);

    app.help_key(key(KeyCode::Right));
    match &app.modal {
        doris::ui::app::Modal::Help(s) => assert_eq!(s.section, 1, "Right switched section"),
        other => panic!("help modal is gone: {:?}", std::mem::discriminant(other)),
    }
    app.help_key(key(KeyCode::Left));
    match &app.modal {
        doris::ui::app::Modal::Help(s) => assert_eq!(s.section, 0, "and back"),
        other => panic!("help modal is gone: {:?}", std::mem::discriminant(other)),
    }
}

/// Every token `src/filter.rs` parses has a row that says so, or the
/// page describes a language the parser does not speak.
#[test]
fn test_filter_help_names_every_token_the_parser_accepts() {
    let text = doris::ui::modals::help::FILTER_HELP
        .iter()
        .map(|(k, d)| format!("{k} {d}"))
        .collect::<Vec<_>>()
        .join("\n");

    for token in [
        "word",
        "-word",
        "src:id",
        "tracker:",
        "group:name",
        "cat:",
        "title:word",
        "size:>1gb",
        "seeds:>50",
    ] {
        assert!(
            text.contains(token),
            "FILTER_HELP never mentions `{token}`:\n{text}"
        );
    }

    // And the grouping half has to name the two ways a row gets its
    // category, because that is why the category can disagree with the
    // rows on screen (see the `all` rule).
    for fact in ["all", "rutracker", "nnmclub"] {
        assert!(
            text.contains(fact),
            "FILTER_HELP never names `{fact}`:\n{text}"
        );
    }
}

#[test]
fn test_filter_help_descriptions_fit_an_80_column_terminal() {
    for (key, desc) in doris::ui::modals::help::FILTER_HELP {
        assert!(
            desc.chars().count() <= 50,
            "'{}' description is {} chars and would be cut off: {}",
            key,
            desc.chars().count(),
            desc
        );
    }
}

/// The page still names every binding AGENTS.md documents, now across
/// both tables: a key that only the second table mentions is a key the
/// first table's test can no longer see.
#[test]
fn test_the_section_switch_is_itself_documented() {
    let both = doris::ui::modals::help::HELP_TEXT
        .iter()
        .chain(doris::ui::modals::help::FILTER_HELP)
        .map(|(k, _)| *k)
        .collect::<Vec<_>>();
    assert!(
        both.iter().any(|k| k.contains('←') && k.contains('→')),
        "`←`/`→` switch sections and must say so: {both:?}"
    );
}
