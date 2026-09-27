use doris::config::Config;
use doris::sources::models::TorrentItem;
use doris::sources::source::Group;
use doris::ui::app::{App as UiApp, UiAction};
use doris::ui::zones::ZoneId;
use ratatui::layout::Rect;

fn make_app() -> UiApp {
    UiApp::new(
        "http://127.0.0.1:8090".to_string(),
        true,
        None,
        "/tmp".into(),
        "braille".into(),
        true,
        true,
        true,
        false,
    )
}

fn make_results(n: usize) -> Vec<TorrentItem> {
    (0..n)
        .map(|i| TorrentItem {
            title: format!("Torrent {}", i),
            size: format!("{} GB", i + 1),
            seeds: format!("{}", i * 10),
            date: format!("01-Jan-2{}", i),
            download_url: format!("/forum/dl.php?t={}", 1000 + i),
            page_url: format!("viewtopic.php?t={}", 1000 + i),
            query: "test".into(),
            ..Default::default()
        })
        .collect()
}

// --- search_box_at ---------------------------------------------------------

/// The input box owns the top three rows of the frame and nothing else:
/// `render_search_bar` draws into exactly that, and `update_areas`
/// starts the zones below it. Rows the box does not own belong to the
/// zones, so a click there must not start a search.
#[test]
fn test_search_box_covers_the_top_three_rows_only() {
    let app = make_app();
    for row in 0..3 {
        assert!(app.search_box_at(row), "row {} is the input box", row);
    }
    assert!(!app.search_box_at(3));
    assert!(!app.search_box_at(40));
}

/// Something can paint over the box: fullscreen stretches a zone across
/// the whole frame and the detail log takes it too. While that is on,
/// those rows belong to the cover, not to the input.
#[test]
fn test_search_box_is_not_a_target_when_something_covers_it() {
    let mut app = make_app();

    app.zones.fullscreen = Some(ZoneId::Results);
    assert!(!app.search_box_at(0));

    app.zones.fullscreen = None;
    app.detail_log_mode = true;
    assert!(!app.search_box_at(0));
}

// --- zone_at ------------------------------------------------------------

#[test]
fn test_zone_at_finds_the_containing_zone() {
    let mut app = make_app();
    app.zones.update_areas(Rect::new(0, 0, 80, 24));

    let results_area = app.zones.get_area(ZoneId::Results);
    assert_eq!(app.zone_at(results_area.y, results_area.x), Some(ZoneId::Results));

    let torrent_area = app.zones.get_area(ZoneId::Torrent);
    assert_eq!(app.zone_at(torrent_area.y, torrent_area.x), Some(ZoneId::Torrent));
}

#[test]
fn test_zone_at_returns_none_outside_all_zones() {
    let mut app = make_app();
    app.zones.update_areas(Rect::new(0, 0, 80, 24));
    // Far outside the terminal entirely.
    assert_eq!(app.zone_at(1000, 1000), None);
}

#[test]
fn test_zone_at_only_hits_fullscreened_zone() {
    let mut app = make_app();
    app.zones.set_fullscreen(Some(ZoneId::Log));
    app.zones.update_areas(Rect::new(0, 0, 80, 24));

    // Every point in the full terminal area should resolve to Log...
    assert_eq!(app.zone_at(10, 10), Some(ZoneId::Log));
    // ...since every other zone's area is zeroed out while fullscreened.
    let torrent_area_before = app.zones.get_area(ZoneId::Torrent);
    assert_eq!(torrent_area_before, Rect::default());
}

// --- click_at -------------------------------------------------------------

#[test]
fn test_click_at_focuses_the_clicked_zone() {
    let mut app = make_app();
    app.zones.update_areas(Rect::new(0, 0, 80, 24));
    assert_eq!(app.zones.focused, ZoneId::Results);

    let log_area = app.zones.get_area(ZoneId::Log);
    app.click_at(log_area.y, log_area.x);
    assert_eq!(app.zones.focused, ZoneId::Log);
}

#[test]
fn test_click_at_results_header_row_does_not_select_a_row() {
    let mut app = make_app();
    app.results = make_results(5);
    app.filtered_indices = (0..5).collect();
    app.zones.update_areas(Rect::new(0, 0, 80, 24));

    let results_area = app.zones.get_area(ZoneId::Results);
    let before = app.selected;
    // +1 for the border, +2 for the source and category tab rows above
    // the table's own header row -- see render_results_zone.
    app.click_at(results_area.y + 3, results_area.x); // table header row, not a data row
    assert_eq!(app.selected, before);
}

#[test]
fn test_click_at_results_data_row_selects_that_item() {
    let mut app = make_app();
    app.results = make_results(5);
    app.filtered_indices = (0..5).collect();
    app.zones.update_areas(Rect::new(0, 0, 80, 24));

    let results_area = app.zones.get_area(ZoneId::Results);
    // y+1 = source tabs, y+2 = category tabs, y+3 = table header,
    // y+4 = first data row (index 0).
    app.click_at(results_area.y + 4, results_area.x);
    assert_eq!(app.selected, 0);

    app.click_at(results_area.y + 5, results_area.x);
    assert_eq!(app.selected, 1);
}

#[test]
fn test_click_at_respects_filtered_indices_not_raw_results_order() {
    let mut app = make_app();
    app.results = make_results(5);
    // Simulate a filter that only kept results 3 and 4.
    app.filtered_indices = vec![3, 4];
    app.zones.update_areas(Rect::new(0, 0, 80, 24));

    let results_area = app.zones.get_area(ZoneId::Results);
    app.click_at(results_area.y + 4, results_area.x); // first visible (filtered) row
    assert_eq!(app.selected, 3);
}

// --- source tab bar (Results panel, btop proc-tab style) -----------------

#[test]
fn test_source_tab_at_finds_each_tab_on_the_tab_row() {
    let mut app = make_app();
    app.zones.update_areas(Rect::new(0, 0, 80, 24));
    let results_area = app.zones.get_area(ZoneId::Results);
    let tab_row = results_area.y + 1;

    // Default active source is "rutracker", shown as "[rutracker]".
    assert_eq!(app.source_tab_at(tab_row, results_area.x + 1), Some("rutracker"));
}

#[test]
fn test_source_tab_at_returns_none_off_the_tab_row() {
    let mut app = make_app();
    app.zones.update_areas(Rect::new(0, 0, 80, 24));
    let results_area = app.zones.get_area(ZoneId::Results);
    // Table header row, one below the tab row.
    assert_eq!(app.source_tab_at(results_area.y + 2, results_area.x + 1), None);
}

#[test]
fn test_click_at_tab_row_switches_active_source() {
    let mut app = make_app();
    app.zones.update_areas(Rect::new(0, 0, 80, 24));
    let results_area = app.zones.get_area(ZoneId::Results);
    let tab_row = results_area.y + 1;

    assert_eq!(app.active_source, "rutracker");
    // "[rutracker]" is 11 chars ("rutracker" + brackets) starting right
    // after the left border; "rutor" starts right after that plus a
    // 2-space gap.
    let rutor_col = results_area.x + 1 + "[rutracker]".chars().count() as u16 + 2;
    let action = app.click_at(tab_row, rutor_col);
    assert_eq!(app.active_source, "rutor");
    assert_eq!(action, None); // switching source isn't a UiAction
}

#[test]
fn test_cycle_source_wraps_through_all_tabs() {
    let mut app = make_app();
    // Derived from the same function the header draws from, rather than
    // spelled out: B8 adds a tab per source, and a hardcoded list here
    // would either break on every addition or -- worse -- stop proving
    // that the cycle covers everything the header draws.
    let tabs = doris::ui::app::source_tabs(&Config::default());
    assert!(tabs.contains(&"all"), "the merge tab is part of the cycle");
    assert_eq!(app.active_source, tabs[0]);

    for expected in tabs.iter().skip(1) {
        app.cycle_source();
        assert_eq!(&app.active_source, expected, "walks the whole list");
    }
    app.cycle_source();
    assert_eq!(&app.active_source, tabs[0], "the cycle must wrap");
}

#[test]
fn test_click_at_returns_none_on_the_zone_title() {
    let mut app = make_app();
    app.zones.update_areas(Rect::new(0, 0, 80, 24));
    let torrent_area = app.zones.get_area(ZoneId::Torrent);
    // The top-left corner carries the zone title, not a frame button.
    let action = app.click_at(torrent_area.y, torrent_area.x);
    assert_eq!(action, None);
}

/// The frame legend is drawn from `frame_layout`, and `click_at` tests
/// those very rects -- so a click on the drawn `pause` word has to come
/// back as the pause action, not fall through to "clicked the panel".
#[test]
fn test_click_at_torrent_pause_button_returns_toggle_pause() {
    let mut app = make_app();
    app.zones.update_areas(Rect::new(0, 0, 80, 24));
    let torrent_area = app.zones.get_area(ZoneId::Torrent);
    let layout = app.frame_layout(ZoneId::Torrent, torrent_area);
    let rect = layout.buttons.iter()
        .find(|(b, _)| b.key == 'p')
        .map(|(_, r)| *r)
        .expect("the Torrent frame has a pause button");

    assert_eq!(app.click_at(rect.y, rect.x), Some(UiAction::TogglePause));
    // And the last column of the word too, not just its first.
    assert_eq!(
        app.click_at(rect.y, rect.x + rect.width - 1),
        Some(UiAction::TogglePause)
    );
}

#[test]
fn test_click_at_torrent_delete_button_returns_remove() {
    let mut app = make_app();
    app.zones.update_areas(Rect::new(0, 0, 80, 24));
    let torrent_area = app.zones.get_area(ZoneId::Torrent);
    let layout = app.frame_layout(ZoneId::Torrent, torrent_area);
    let rect = layout.buttons.iter()
        .find(|(b, _)| b.key == 'd')
        .map(|(_, r)| *r)
        .expect("the Torrent frame has a delete button");

    assert_eq!(app.click_at(rect.y, rect.x), Some(UiAction::Remove));
}

/// Between the two buttons there is a gap: landing in it must do
/// nothing, or the legend would be a single imprecise hot zone.
#[test]
fn test_click_between_two_frame_buttons_does_nothing() {
    let mut app = make_app();
    app.zones.update_areas(Rect::new(0, 0, 80, 24));
    let results_area = app.zones.get_area(ZoneId::Results);
    let layout = app.frame_layout(ZoneId::Results, results_area);
    // `group` and `source` are the two right-aligned buttons; the gap
    // between them is at least one column wide.
    let group = layout.buttons.iter().find(|(b, _)| b.key == 'g')
        .map(|(_, r)| *r).expect("group button");
    let source = layout.buttons.iter().find(|(b, _)| b.key == ']')
        .map(|(_, r)| *r).expect("source button");
    assert!(source.x > group.x + group.width);

    let gap_col = group.x + group.width;
    assert!(app.click_at(group.y, gap_col).is_none());
}

/// The buttons `ui::App` can act on itself happen right here rather than
/// being handed to the orchestrator.
#[test]
fn test_clicking_the_filter_button_enters_filter_mode() {
    let mut app = make_app();
    app.zones.update_areas(Rect::new(0, 0, 80, 24));
    assert!(!app.zones.filter_mode);

    let results_area = app.zones.get_area(ZoneId::Results);
    let layout = app.frame_layout(ZoneId::Results, results_area);
    let rect = layout.buttons.iter()
        .find(|(b, _)| b.key == 'F')
        .map(|(_, r)| *r)
        .expect("the Results frame has a filter button");

    assert_eq!(app.click_at(rect.y, rect.x), None);
    assert!(app.zones.filter_mode, "the click entered filter mode");
}

// --- the tab bar is derived from config, not written down -------------------

/// Decided with the user after wave 1's live run: a source the user
/// switched off must not leave behind a tab whose search can only
/// answer "Selected source is disabled".
#[test]
fn test_a_switched_off_source_has_no_tab() {
    let mut config = Config::default();
    config.enabled_sources = vec!["rutracker".to_string(), "tpb".to_string()];

    assert_eq!(
        doris::ui::app::source_tabs(&config),
        vec!["rutracker", "tpb", "all"],
        "registry order, and only what is switched on"
    );
}

#[test]
fn test_unimplemented_sources_never_get_a_tab() {
    // A source that is not implemented offers nothing: no tab, no
    // rows, no way to be enabled into existence. With every source
    // implemented today this is the invariant the next planned source
    // has to satisfy -- the bar is derived from the registry, so a
    // planned id cannot appear there by accident.
    let tabs = doris::ui::app::source_tabs(&Config::default());
    for info in doris::sources::source::KNOWN_SOURCES.iter().filter(|s| !s.implemented) {
        assert!(!tabs.contains(&info.id), "{} is planned and has a tab", info.id);
    }
    assert!(tabs.contains(&"nnmclub"), "wave 3's first source does");
    assert!(tabs.contains(&"1337x"), "and wave 3's second one does too");
    assert!(tabs.contains(&"torentino"), "and wave 3's third one does too");
}

#[test]
fn test_the_all_tab_survives_switching_every_source_off() {
    let mut config = Config::default();
    config.enabled_sources = Vec::new();

    let tabs = doris::ui::app::source_tabs(&config);
    assert_eq!(tabs, vec!["all"], "the bar always has somewhere to be");
}

/// Losing the tab you were on must move the selection somewhere that
/// exists, rather than leaving `active_source` pointing at nothing (a
/// silent search that returns no rows and no explanation).
#[test]
fn test_losing_the_active_tab_moves_the_selection_to_a_live_one() {
    let mut app = make_app();
    app.active_source = "subsplease".to_string();

    let mut config = Config::default();
    config.enabled_sources = vec!["rutracker".to_string(), "eztv".to_string()];
    app.set_result_tabs(&config);

    assert_eq!(app.active_source, "rutracker", "first live tab wins");
    assert!(app.source_tabs.contains(&app.active_source.as_str()));
}

/// Opening and closing Settings (which re-derives the bar every time)
/// must not disturb where the user already was.
#[test]
fn test_keeping_the_active_tab_leaves_the_selection_alone() {
    let mut app = make_app();
    app.active_source = "eztv".to_string();

    app.set_result_tabs(&Config::default());

    assert_eq!(app.active_source, "eztv");
    assert_eq!(
        app.source_tabs,
        vec![
            "rutracker", "rutor", "yts", "tpb", "subsplease", "nyaa", "eztv", "nnmclub",
            "1337x", "torentino", "all",
        ],
        "and the bar itself still lists everything the default config enables"
    );
}

/// The rendered bar and the click hit-test read the same field, so a
/// tab that is drawn can be clicked: derivations must stay in one
/// place (`set_result_tabs`), never in the render path alone.
#[test]
fn test_clicking_a_tab_that_was_derived_selects_it() {
    let mut app = make_app();
    let mut config = Config::default();
    config.enabled_sources = vec!["yts".to_string(), "all".to_string()];
    app.set_result_tabs(&config);
    app.active_source = "yts".to_string();
    app.zones.update_areas(Rect::new(0, 0, 80, 24));

    let results = app.zones.get_area(ZoneId::Results);
    // yts starts one column in; its label is 3 wide, then two spaces.
    let tpb_col = results.x + 1 + 3 + 2;
    assert_eq!(app.source_tab_at(results.y + 1, tpb_col), None);
    assert_eq!(app.source_tab_at(results.y + 1, results.x + 1), Some("yts"));
}

// --- category tab row (B6's second Results row) ---------------------------

/// Availability rather than a fixed four: with only single-group
/// sources enabled the row shrinks to what they can answer, which is
/// the same rule `source_tabs` applies to disabled sources.
#[test]
fn test_category_row_offers_only_groups_an_enabled_source_serves() {
    let mut yts_only = Config::default();
    yts_only.enabled_sources = vec!["yts".to_string()];
    assert_eq!(
        doris::ui::modals::settings::group_tabs(&yts_only),
        vec![None, Some(Group::Movies)],
        "yts declares Movies and nothing else"
    );

    let mut eztv_only = Config::default();
    eztv_only.enabled_sources = vec!["eztv".to_string()];
    assert_eq!(
        doris::ui::modals::settings::group_tabs(&eztv_only),
        vec![None, Some(Group::TV)]
    );

    let mut all_off = Config::default();
    all_off.enabled_sources = Vec::new();
    assert_eq!(
        doris::ui::modals::settings::group_tabs(&all_off),
        vec![None],
        "with everything off the row still has somewhere to be"
    );

    // The default config enables every implemented source, and between
    // them they serve all four groups -- in `GROUP_ORDER`, "all" first.
    assert_eq!(
        doris::ui::modals::settings::group_tabs(&Config::default()),
        vec![
            None,
            Some(Group::Movies),
            Some(Group::TV),
            Some(Group::Games),
            Some(Group::Anime),
        ]
    );
}

#[test]
fn test_cycle_group_walks_the_row_and_wraps() {
    let mut app = make_app();
    // Derived from the function the row is drawn from rather than
    // spelled out: a group added to the registry must not leave this
    // test walking a list the UI no longer shows.
    let tabs = doris::ui::modals::settings::group_tabs(&Config::default());
    assert_eq!(app.active_group, tabs[0], "starts on all");
    assert_eq!(app.active_group, None);

    for expected in tabs.iter().skip(1) {
        app.cycle_group(true);
        assert_eq!(&app.active_group, expected, "walks the whole row");
        assert!(app.group_changed, "every step is owed a re-search");
    }
    app.cycle_group(true);
    assert_eq!(app.active_group, tabs[0], "the cycle must wrap");

    // Backwards from "all" lands on the last tab, never one before it.
    app.cycle_group(false);
    assert_eq!(app.active_group, tabs[tabs.len() - 1]);
}

/// The row that is drawn is the row that is clickable: the hit-test
/// walks the same `group_tabs` field the render does, one row lower.
#[test]
fn test_group_tab_at_finds_each_tab_it_drew() {
    let mut app = make_app();
    app.zones.update_areas(Rect::new(0, 0, 80, 24));
    let results = app.zones.get_area(ZoneId::Results);
    let row = results.y + 2; // one row below the source tabs
    let first = results.x + 1;

    // "[all]" is bracketed while selected: 5 cells, then two spaces.
    assert_eq!(app.group_tab_at(row, first), Some(None), "the all tab");
    assert_eq!(app.group_tab_at(row, first + 4), Some(None), "its bracket");
    assert_eq!(
        app.group_tab_at(row, first + 5 + 2),
        Some(Some(Group::Movies))
    );
    // The source row above and the table's header row below are not
    // this row, however close their columns look.
    assert_eq!(app.group_tab_at(results.y + 1, first), None);
    assert_eq!(app.group_tab_at(results.y + 3, first), None);

    // A click selects the tab, flags the owed search, and focuses the
    // zone -- like every other Results click.
    assert_eq!(app.click_at(row, first + 5 + 2), None);
    assert_eq!(app.active_group, Some(Group::Movies));
    assert!(app.group_changed);
    assert_eq!(app.zones.focused, ZoneId::Results);

    // Now "[all]" has lost its brackets and every label to its right
    // moved left; the hit-test follows on its own because both sides
    // derive from the same field.
    assert_eq!(
        app.group_tab_at(row, first + 4),
        None,
        "the bracket cell is a gap now"
    );
    assert_eq!(app.group_tab_at(row, first + 5), Some(Some(Group::Movies)));
}

/// Switching the source that was the last one serving a category must
/// not leave the row pointing at a tab that is no longer drawn.
#[test]
fn test_losing_the_category_moves_the_selection_to_all() {
    let mut app = make_app();
    app.active_group = Some(Group::Games);

    let mut config = Config::default();
    config.enabled_sources = vec!["yts".to_string()];
    app.set_result_tabs(&config);

    assert_eq!(app.active_group, None, "falls back to all");
    assert!(app.group_tabs.contains(&Some(Group::Movies)));
    assert!(!app.group_tabs.contains(&Some(Group::Games)));
}

/// Opening and closing Settings re-derives both rows every time; a
/// category that still has sources behind it stays where it was.
#[test]
fn test_keeping_the_category_leaves_the_selection_alone() {
    let mut app = make_app();
    app.active_group = Some(Group::Movies);

    app.set_result_tabs(&Config::default());

    assert_eq!(app.active_group, Some(Group::Movies));
    assert!(!app.group_changed, "re-deriving is not a switch");
}

// --- the frame legend's geometry ------------------------------------------

/// `frame_layout` is what both the renderer and `click_at` go through,
/// so a button that escaped its own zone would be drawn on top of
/// somebody else's border and click through to it. Checked at several
/// terminal sizes, including ones small enough that btop would have
/// dropped the right-hand cluster entirely.
#[test]
fn test_every_frame_button_stays_on_its_own_border() {
    for (w, h) in [(80u16, 24u16), (40, 12), (20, 8), (10, 4), (6, 3)] {
        let mut app = make_app();
        app.zones.update_areas(Rect::new(0, 0, w, h));
        app.zones.set_fullscreen(Some(ZoneId::Results));
        let area = app.zones.get_area(ZoneId::Results);
        let layout = app.frame_layout(ZoneId::Results, area);

        for (button, rect) in &layout.buttons {
            assert!(rect.x > area.x, "{:?} at {} touches the left border", button, w);
            assert!(
                rect.x + rect.width < area.x + area.width,
                "{:?} at {} overruns the right border",
                button,
                w
            );
            assert!(
                rect.y == area.y || rect.y + 1 == area.y + area.height,
                "{:?} at {}x{} is not on a border row",
                button,
                w,
                h
            );
        }
        if layout.info.width > 0 {
            assert_eq!(layout.info.y, area.y, "info stays on the top border");
            assert!(layout.info.x + layout.info.width < area.x + area.width);
        }
    }
}

/// Two buttons must never share a column, or a click would be a guess
/// about which of them the user meant.
#[test]
fn test_frame_buttons_do_not_overlap() {
    let mut app = make_app();
    app.zones.update_areas(Rect::new(0, 0, 80, 24));
    for &id in ZoneId::all() {
        let area = app.zones.get_area(id);
        let layout = app.frame_layout(id, area);
        for (i, (a, ra)) in layout.buttons.iter().enumerate() {
            for (b, rb) in layout.buttons.iter().skip(i + 1) {
                assert!(
                    ra.y != rb.y || ra.x + ra.width <= rb.x || rb.x + rb.width <= ra.x,
                    "{:?} and {:?} overlap on {:?}",
                    a,
                    b,
                    id
                );
            }
        }
        if layout.info.width > 0 {
            for (_, rect) in &layout.buttons {
                if rect.y != layout.info.y {
                    continue;
                }
                assert!(
                    rect.x + rect.width <= layout.info.x
                        || layout.info.x + layout.info.width <= rect.x,
                    "info text overlaps a button on {:?}",
                    id
                );
            }
        }
    }
}
