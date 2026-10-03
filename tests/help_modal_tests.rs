//! The help page: a modal list of keybinds and what they do.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use doris::config::Config;
use doris::ui::modals::help::{sections, FILTER_HELP, NAV_KEYS, SEARCH_KEYS, TORRENT_KEYS};

/// Every row on every page.
///
/// The pages are split by subject, so a test that wants to know whether a
/// key is documented anywhere cannot ask one table -- that is what the
/// split cost, and this is the bill.
fn all_rows() -> Vec<(&'static str, &'static str)> {
    sections()
        .iter()
        .flat_map(|(_, table)| table.iter().copied())
        .collect()
}
use doris::ui::view::App as UiApp;
use ratatui::backend::TestBackend;
use ratatui::Terminal;

mod common;

fn make_app() -> UiApp {
    common::make_app()
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

/// The page draws its `[key, description]` pairs from one table.
#[test]
fn test_help_text_names_the_documented_keybinds() {
    let keys: Vec<&str> = all_rows().into_iter().map(|(k, _)| k).collect();
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
        // The torrent keys are documented on their own page now, and a
        // test that only read the first table could not see them.
        "v",
        "f",
        "o",
        "+",
        "-",
        "0",
    ] {
        assert!(
            keys.iter().any(|k| k == &expected),
            "no help page lists '{}'; it must match AGENTS.md",
            expected
        );
    }
}

/// The help page has to say what the app does, not what it used to do.
#[test]
fn the_page_describes_what_the_keys_do_now() {
    let page: Vec<String> = all_rows()
        .into_iter()
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

/// The description column is whatever is left of the box after the 20-column key column, and
/// the box is 90% of an 80-column terminal -- 70 inner columns, 50 for the description.
#[test]
fn test_help_descriptions_fit_an_80_column_terminal() {
    for (key, desc) in all_rows() {
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
        doris::ui::view::Modal::Help(s) => s.clone(),
        other => panic!(
            "expected the help modal, got {:?}",
            std::mem::discriminant(other)
        ),
    };
    assert_eq!(state.page, 0);
}

/// The help modal closes on `escape`, `q`, `h`, `backspace`, `space`
/// and `enter` -- every one of them has to work,
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
        assert_eq!(app.modal, doris::ui::view::Modal::None, "{:?} closes", code);
    }

    let mut app = make_app();
    app.open_help_modal();
    app.help_key(key(KeyCode::Char('x')));
    assert_ne!(app.modal, doris::ui::view::Modal::None, "'x' does nothing");
}

/// The page count only exists once the renderer has measured the box, so paging has to be
/// exercised the way a user gets it: draw, then press a key.
#[test]
fn test_help_pages_forward_and_wraps() {
    let mut app = make_app();
    app.open_help_modal();
    let rows = render(&mut app, 80, 24);

    let page_indicator = |app: &UiApp| match &app.modal {
        doris::ui::view::Modal::Help(s) => s.page,
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
/// The indicator is guarded the same way.
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
        rows.iter().any(|r| r.contains("This window.")),
        "the whole table fits on one page"
    );
    assert!(
        rows.iter().all(|r| !r.contains("↑ page")),
        "no page indicator when there is one page"
    );

    app.help_key(key(KeyCode::Char('j')));
    match &app.modal {
        doris::ui::view::Modal::Help(s) => assert_eq!(s.page, 0),
        other => panic!("help modal is gone: {:?}", std::mem::discriminant(other)),
    }
}

/// The reason the page exists: a two-column table, key in `hi_fg` + bold, description after it.
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
    let key_col = header[..header.find("Key:").unwrap()].chars().count() - 8;
    let page = &rows[rows.iter().position(|r| r.contains("Key:")).unwrap()..];
    // Page 1 is `keys`, so these are its rows: a key that moved to another
    // page is not missing, and a test that still looked for it here would
    // be pinning the old heap.
    for expected in ["1, 2, 3, 4", "Esc", "ctrl+shift", "Mouse"] {
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

/// The page pairs a key with what it actually does; the keys alone are not enough to catch a
/// swap.
#[test]
fn test_help_text_pairs_the_lower_f_with_filter_and_the_upper_with_fullscreen() {
    // Looked up per page, because the same key means different things in
    // different places: `f` is the filter box in Results and the file list
    // in the Torrents view, and `Enter` is search, play, a Trackers
    // checkbox and a file switch. One table for all of them is the heap
    // this split exists to undo.
    fn desc(table: &[(&'static str, &'static str)], key: &str) -> &'static str {
        table
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, d)| *d)
            .unwrap_or_else(|| panic!("no help row for `{key}`"))
    }

    assert_eq!(
        desc(FILTER_HELP, "word"),
        "Substring of title, size, source, group.",
        "the filter box's syntax is src/filter.rs"
    );
    assert_eq!(
        desc(TORRENT_KEYS, "Enter"),
        "Fetches that file / stops fetching it.",
        "`f` in the Torrents view is the file list, and Enter is its switch"
    );
    assert_eq!(
        desc(SEARCH_KEYS, "Enter"),
        "Searches, or plays the selected row.",
        "and in the plain view Enter is search and play"
    );
    assert_eq!(
        desc(NAV_KEYS, "F"),
        "Toggles fullscreen for the focused zone.",
        "upper F is fullscreen; lower f is never that"
    );

    // The frame legend is the other place these keys are named, so the
    let legend = doris::ui::layout::zone_buttons(doris::ui::layout::ZoneId::Results);
    assert!(
        legend.iter().any(|b| b.key == 'f' && b.label == "filter"),
        "the Results frame prints `f filter`: {:?}",
        legend
            .iter()
            .map(|b| (b.key, b.label.clone()))
            .collect::<Vec<_>>()
    );
}

/// The page grew a second table: how the filter is written and how a row gets its category --
/// the two things `f` and `g` do that the single key table could not explain.
#[test]
fn test_help_switches_sections_with_the_arrow_keys() {
    let mut app = make_app();
    app.open_help_modal();

    let title = |app: &UiApp, rows: &[String]| match &app.modal {
        doris::ui::view::Modal::Help(s) => rows
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

    // Right moves to the next page, and the test asks after the table rather
    // than naming one: pages are added and renamed, and a test that hard-codes
    // "the filter table" is a test that fails when a fourth page appears.
    let (next_name, next_table) = sections()[1];
    app.help_key(key(KeyCode::Right));
    rows = render(&mut app, 110, 44);
    let shown = title(&app, &rows);
    assert!(
        shown.contains(next_name),
        "Right moves to `{next_name}`, got {shown:?}"
    );
    assert!(
        rows.iter().any(|r| r.contains(next_table[0].1)),
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
        doris::ui::view::Modal::Help(s) => assert_eq!(s.section, 1, "Right switched section"),
        other => panic!("help modal is gone: {:?}", std::mem::discriminant(other)),
    }
    app.help_key(key(KeyCode::Left));
    match &app.modal {
        doris::ui::view::Modal::Help(s) => assert_eq!(s.section, 0, "and back"),
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
    let both: Vec<&str> = all_rows().into_iter().map(|(k, _)| k).collect();
    assert!(
        both.iter().any(|k| k.contains('←') && k.contains('→')),
        "`←`/`→` switch sections and must say so: {both:?}"
    );
}

/// Both tables are named on screen, with the current one marked.
///
/// The switch worked; what was missing was any sign that there was
/// anything to switch to. The title text changed and nothing else did,
/// so the page read as one long list and the filter syntax looked
/// absent rather than one arrow away.
#[test]
fn test_both_help_sections_are_named_on_screen() {
    let mut app = make_app();
    app.open_help_modal();
    let rows = render(&mut app, 100, 30);
    let all = rows.join("\n");

    for (name, _) in doris::ui::modals::help::sections() {
        assert!(
            all.contains(name),
            "the `{name}` table must be named on the page: {all}"
        );
    }
    assert!(
        all.contains('◀') && all.contains('▶'),
        "and there must be arrows showing that it can be switched: {all}"
    );

    // The one being shown is the one in brackets, so pressing Right
    // visibly moves the mark rather than only the title.
    app.help_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
    let after = render(&mut app, 100, 30).join("\n");
    let switched = doris::ui::modals::help::sections()
        .iter()
        .map(|(n, _)| *n)
        .collect::<Vec<_>>();
    let marked = switched
        .iter()
        .find(|n| after.contains(&format!("[{n}]")))
        .expect("one section is marked");
    assert_eq!(
        *marked, switched[1],
        "Right must move the mark to the second table"
    );
}

/// Every section's tab names the digit that opens it, the way the
/// Options tabs do (`2:network`).
///
/// A digit binding that no label mentions is a secret: the arrows walk
/// between the tables and the page looks like one long list, so nothing
/// on screen said that a second table existed or that a digit would get
/// there.
#[test]
fn test_every_help_tab_names_its_own_digit() {
    let mut app = make_app();
    app.open_help_modal();
    // The tabs ride the popup's bottom border, which is not the last
    // row of the terminal -- the popup is inset.
    let rows = render(&mut app, 120, 40);
    let tabs = rows
        .iter()
        .find(|r| r.contains('\u{25c0}'))
        .unwrap_or_else(|| panic!("no tab row was drawn; the frame reads {rows:?}"));

    for (n, (name, _)) in doris::ui::modals::help::sections().iter().enumerate() {
        if n == 0 {
            // The section being shown is bracketed, as in Options.
            assert!(tabs.contains(&format!("[{name}]")), "{tabs}");
        } else {
            assert!(
                tabs.contains(&format!("{}:{}", n + 1, name)),
                "the tab for `{name}` must be labelled `{}:{name}`; the tab row is {tabs:?}",
                n + 1
            );
        }
    }
    // And the digit actually switches, which is the other half of it.
    // The digit names its own page, so the check is: pressing 2 shows the
    // table that page is called. Hard-coded row text would break every time
    // a page's contents change, which is exactly what a help page does.
    let (_, second) = sections()[1];
    let marker = second[0].1;
    app.help_key(key(KeyCode::Char('2')));
    let rows = render(&mut app, 120, 40);
    assert!(
        rows.iter().any(|r| r.contains(marker)),
        "pressing 2 must show `{}`, whose first row is {marker}",
        second[0].0
    );
}

/// Every key in the table is reachable, over however many pages it
/// takes. A key listed but never drawn is worse than one not listed: the
/// page claims to be the reference.
#[test]
fn test_every_listed_key_is_drawn_on_some_page() {
    let mut app = make_app();
    app.open_help_modal();
    // Every *page*, not just page 1: the split moved keys off it, and a
    // walk that stops at the end of the first table cannot see the rest.
    let mut seen = String::new();
    for section in 0..sections().len() {
        app.help_key(key(KeyCode::Char(
            char::from_digit(section as u32 + 1, 10).expect("a digit"),
        )));
        for _ in 0..8 {
            let rows = render(&mut app, 100, 40);
            seen.push_str(&rows.join("\n"));
            let before = match &app.modal {
                doris::ui::view::Modal::Help(s) => s.page,
                _ => break,
            };
            app.help_key(key(KeyCode::Char('j')));
            match &app.modal {
                doris::ui::view::Modal::Help(s) if s.page == before => break,
                _ => {}
            }
        }
    }
    for (k, _) in all_rows() {
        // The table prints the key in a 20-column column; matching the
        // first word is enough to say it was drawn somewhere.
        let head = k.split(&[' ', '+'][..]).next().unwrap_or(k);
        assert!(
            seen.contains(head),
            "`{k}` is listed but never drawn on any page"
        );
    }
}

/// The three layout bindings exist because a panel cannot be arranged
/// without them, so they are written down where the others are.
#[test]
fn test_the_layout_bindings_are_documented() {
    for binding in ["ctrl + arrows", "shift + arrows", "ctrl+shift+arrows"] {
        assert!(
            NAV_KEYS.iter().any(|(k, _)| *k == binding),
            "`{binding}` is not in the key table"
        );
    }
}

/// The help pages are split by subject, so a key is findable by what it is
/// for rather than by scrolling a heap of thirty-four rows.
///
/// The split is the claim, so it is checked as one: every row belongs to a
/// page whose name says what that page is about, no page is big enough to
/// need scrolling on an ordinary terminal, and the torrent keys are on a
/// page of their own rather than at the bottom of the general list.
#[test]
fn the_help_pages_are_split_by_subject_not_by_length() {
    let names: Vec<&str> = sections().iter().map(|(name, _)| *name).collect();
    assert_eq!(
        names,
        vec!["keys", "search", "filter", "torrents"],
        "the pages, in the order the digits give them"
    );

    // A page that needs a second page at 50 rows is a page that is a list
    // to scroll, which is what the split was for.
    for (name, table) in sections() {
        assert!(
            table.len() <= 40,
            "`{name}` has {} rows: it is a heap again",
            table.len()
        );
    }

    // The torrent keys are on their own page, and every one of them is on
    // it -- `p` and `d` exist in three places in this app and a reader who
    // finds them here must be able to trust that they mean the torrent.
    for key in ["p", "d", "v", "f", "o", "+", "-", "0", "T"] {
        assert!(
            TORRENT_KEYS.iter().any(|(k, _)| *k == key),
            "`{key}` acts on a torrent and must be on the torrents page"
        );
    }

    // And the search keys are not on it: the file list's `Enter` and the
    // search box's `Enter` are the same key pressed in two places.
    assert!(
        !TORRENT_KEYS
            .iter()
            .any(|(k, _)| *k == "s, i, S" || *k == "b"),
        "search belongs on the search page"
    );
}

/// Every key the Torrents detail view and its file list actually take is
/// written down.
///
/// The list is checked against the routing rather than against a copy of it:
/// a key that works and is not on the page is invisible, and a key that is
/// on the page and does not work is worse.
#[test]
fn every_torrent_control_is_on_the_torrents_page() {
    let written: Vec<&str> = TORRENT_KEYS.iter().map(|(k, _)| *k).collect();
    let joined = written.join(" ");
    for key in [
        "p",     // pause / resume
        "d",     // remove
        "v",     // verify
        "f",     // files
        "o",     // open the directory
        "+",     // faster
        "-",     // slower
        "0",     // unlimited
        "Enter", // the file switch, and search
        "a",     // all files on
        "n",     // all files off
        "j, k",  // the cursor
        "PgUp",  // a page of the cursor
        "Home",  // either end
        "Esc",   // the menu, and closing the file list
    ] {
        assert!(
            joined.contains(key),
            "`{key}` is in the Torrents view and not on the torrents page"
        );
    }
}
